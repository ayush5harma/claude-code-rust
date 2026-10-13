// SPDX-License-Identifier: Apache-2.0

//! The `/agents` view of the active session's subagents.
//!
//! Rows and their live status come from the task lifecycle the bridge
//! normalizes into `sdk_inventory.tasks`. This module holds only view state and
//! the outcome of its own requests: which saved transcripts the SDK lists, the
//! transcript being read, and the stop requests it has sent.

use crate::agent::events::ClientEvent;
use crate::agent::model::{TaskItem, TaskStatus};
use crate::agent::types::SubagentMessage;
use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashSet;

const SUBAGENT_TASK_TYPE: &str = "local_agent";
const TRANSCRIPT_PAGE_ROWS: u16 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubagentStatus {
    Pending,
    Running,
    /// Still running by the lifecycle, after this view sent a stop request.
    Stopping,
    Completed,
    Failed,
    Stopped,
}

impl SubagentStatus {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NoticeKind {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

#[derive(Debug, Default)]
pub(crate) struct TranscriptView {
    pub agent_id: String,
    /// `None` until the SDK answers.
    pub messages: Option<Vec<SubagentMessage>>,
    pub error: Option<String>,
    pub scroll: u16,
}

#[derive(Debug, Default)]
pub(crate) struct AgentsView {
    pub selected_task_id: Option<String>,
    /// Ids `listSubagents()` returned; `None` until the first answer.
    pub transcript_ids: Option<HashSet<String>>,
    pub listing: bool,
    stop_requested: HashSet<String>,
    pub transcript: Option<TranscriptView>,
    pub notice: Option<Notice>,
}

pub(crate) struct SubagentRow<'a> {
    pub task: &'a TaskItem,
    pub status: SubagentStatus,
}

impl SubagentRow<'_> {
    pub(crate) fn subagent_type(&self) -> Option<&str> {
        metadata_str(self.task, "subagent_type")
    }

    pub(crate) fn summary(&self) -> Option<&str> {
        // The SDK often repeats the description as the summary.
        metadata_str(self.task, "summary")
            .filter(|summary| !summary.trim().is_empty() && *summary != self.task.subject)
    }
}

fn metadata_str<'a>(task: &'a TaskItem, key: &str) -> Option<&'a str> {
    task.metadata.as_ref()?.as_object()?.get(key)?.as_str()
}

pub(crate) fn is_subagent_task(task: &TaskItem) -> bool {
    metadata_str(task, "task_type") == Some(SUBAGENT_TASK_TYPE)
}

fn lifecycle_status(task: &TaskItem) -> SubagentStatus {
    match task.status {
        TaskStatus::Pending => SubagentStatus::Pending,
        TaskStatus::InProgress => SubagentStatus::Running,
        TaskStatus::Completed => match metadata_str(task, "terminal_status") {
            Some("failed") => SubagentStatus::Failed,
            Some("killed" | "stopped") => SubagentStatus::Stopped,
            _ => SubagentStatus::Completed,
        },
    }
}

/// The session's subagent rows in lifecycle order, one per task id.
pub(crate) fn subagent_rows(app: &App) -> Vec<SubagentRow<'_>> {
    app.sdk_inventory
        .tasks
        .iter()
        .filter(|task| is_subagent_task(task))
        .map(|task| {
            let status = match lifecycle_status(task) {
                SubagentStatus::Running if app.agents.stop_requested.contains(&task.task_id) => {
                    SubagentStatus::Stopping
                }
                status => status,
            };
            SubagentRow { task, status }
        })
        .collect()
}

/// Subagents the lifecycle reports as running, including those asked to stop.
pub(crate) fn running_subagent_count(app: &App) -> usize {
    app.sdk_inventory
        .tasks
        .iter()
        .filter(|task| is_subagent_task(task) && task.status == TaskStatus::InProgress)
        .count()
}

impl AgentsView {
    /// A saved transcript exists only for an id the SDK listed; nothing is inferred.
    pub(crate) fn has_transcript(&self, task_id: &str) -> Option<bool> {
        self.transcript_ids.as_ref().map(|ids| ids.contains(task_id))
    }

    /// The chosen row, else the first still running, else the first.
    pub(crate) fn selected_index(&self, rows: &[SubagentRow<'_>]) -> usize {
        self.selected_task_id
            .as_deref()
            .and_then(|id| rows.iter().position(|row| row.task.task_id == id))
            .or_else(|| {
                rows.iter().position(|row| {
                    matches!(row.status, SubagentStatus::Running | SubagentStatus::Stopping)
                })
            })
            .unwrap_or(0)
    }

    /// Forget everything tied to the previous session.
    pub(crate) fn clear_session(&mut self) {
        *self = Self::default();
    }
}

fn active_session(
    app: &App,
) -> Option<(String, std::rc::Rc<crate::agent::client::AgentConnection>)> {
    let session_id = app.session_runtime.session_id.as_ref()?.to_string();
    let conn = app.session_runtime.conn.clone()?;
    Some((session_id, conn))
}

fn notice(app: &mut App, kind: NoticeKind, text: impl Into<String>) {
    app.agents.notice = Some(Notice { kind, text: text.into() });
}

pub(crate) fn open(app: &mut App) {
    app.agents.transcript = None;
    app.agents.notice = None;
    app.agents.selected_task_id = None;
    crate::app::view::set_fullscreen_view(app, crate::app::FullscreenView::Agents);
    request_transcript_list(app);
}

fn close(app: &mut App) {
    app.agents.transcript = None;
    crate::app::view::set_chat_surface(app);
}

fn request_transcript_list(app: &mut App) {
    let Some((session_id, conn)) = active_session(app) else { return };
    match conn.list_subagents(session_id) {
        Ok(()) => app.agents.listing = true,
        Err(error) => {
            notice(app, NoticeKind::Error, format!("Could not check transcripts: {error}"));
        }
    }
}

/// Applies the bridge's answer to one of this view's requests.
pub(crate) fn apply_client_event(app: &mut App, event: ClientEvent) {
    match event {
        ClientEvent::SubagentsListed { agent_ids, error, .. } => {
            apply_transcript_list(app, agent_ids, error);
        }
        ClientEvent::SubagentMessagesReceived { agent_id, messages, error, .. } => {
            apply_transcript(app, &agent_id, messages, error);
        }
        ClientEvent::SubagentStopResult { task_id, error, .. } => {
            apply_stop_result(app, &task_id, error);
        }
        _ => {
            tracing::warn!(
                "client event family routed a non-subagent event to the subagent handler"
            );
        }
    }
}

pub(crate) fn apply_transcript_list(app: &mut App, agent_ids: Vec<String>, error: Option<String>) {
    app.agents.listing = false;
    match error {
        Some(error) => {
            notice(app, NoticeKind::Error, format!("Could not check transcripts: {error}"));
        }
        None => app.agents.transcript_ids = Some(agent_ids.into_iter().collect()),
    }
}

fn apply_transcript(
    app: &mut App,
    agent_id: &str,
    messages: Vec<SubagentMessage>,
    error: Option<String>,
) {
    let Some(transcript) = app.agents.transcript.as_mut().filter(|view| view.agent_id == agent_id)
    else {
        return;
    };
    transcript.messages = Some(messages);
    transcript.error = error;
}

fn apply_stop_result(app: &mut App, task_id: &str, error: Option<String>) {
    match error {
        Some(error) => {
            app.agents.stop_requested.remove(task_id);
            notice(app, NoticeKind::Error, format!("Could not stop the subagent: {error}"));
        }
        // The lifecycle can report the outcome before the acknowledgement.
        None if subagent_rows(app)
            .iter()
            .any(|row| row.task.task_id == task_id && row.status == SubagentStatus::Stopping) =>
        {
            notice(app, NoticeKind::Info, "Stop requested. The status changes when it stops.");
        }
        None => {}
    }
}

fn selected_row(app: &App) -> Option<(String, SubagentStatus)> {
    let rows = subagent_rows(app);
    let row = rows.get(app.agents.selected_index(&rows))?;
    Some((row.task.task_id.clone(), row.status))
}

fn move_selection(app: &mut App, down: bool) {
    let rows = subagent_rows(app);
    if rows.is_empty() {
        return;
    }
    let current = app.agents.selected_index(&rows);
    let next = if down { (current + 1).min(rows.len() - 1) } else { current.saturating_sub(1) };
    let id = rows[next].task.task_id.clone();
    app.agents.selected_task_id = Some(id);
}

fn open_transcript(app: &mut App) {
    let Some((task_id, _)) = selected_row(app) else { return };
    if app.agents.has_transcript(&task_id) != Some(true) {
        notice(
            app,
            NoticeKind::Info,
            "No transcript for this subagent yet. Press r to check again.",
        );
        return;
    }
    let Some((session_id, conn)) = active_session(app) else { return };
    if let Err(error) = conn.get_subagent_messages(session_id, task_id.clone()) {
        notice(app, NoticeKind::Error, format!("Could not read the transcript: {error}"));
        return;
    }
    app.agents.notice = None;
    app.agents.transcript = Some(TranscriptView { agent_id: task_id, ..TranscriptView::default() });
}

fn request_stop(app: &mut App) {
    let Some((task_id, status)) = selected_row(app) else { return };
    match status {
        SubagentStatus::Running => {}
        SubagentStatus::Stopping => return,
        _ => {
            notice(app, NoticeKind::Info, "Only a running subagent can be stopped.");
            return;
        }
    }
    let Some((session_id, conn)) = active_session(app) else { return };
    // Keep the row selected after it stops running.
    app.agents.selected_task_id = Some(task_id.clone());
    match conn.stop_subagent(session_id, task_id.clone()) {
        Ok(()) => {
            app.agents.stop_requested.insert(task_id);
            app.agents.notice = None;
        }
        Err(error) => {
            notice(app, NoticeKind::Error, format!("Could not stop the subagent: {error}"));
        }
    }
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) {
    if !matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) {
        return;
    }
    if let Some(transcript) = app.agents.transcript.as_mut() {
        match key.code {
            KeyCode::Esc | KeyCode::Left => app.agents.transcript = None,
            KeyCode::Up => transcript.scroll = transcript.scroll.saturating_sub(1),
            KeyCode::Down => transcript.scroll = transcript.scroll.saturating_add(1),
            KeyCode::PageUp => {
                transcript.scroll = transcript.scroll.saturating_sub(TRANSCRIPT_PAGE_ROWS);
            }
            KeyCode::PageDown => {
                transcript.scroll = transcript.scroll.saturating_add(TRANSCRIPT_PAGE_ROWS);
            }
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Esc => close(app),
        KeyCode::Up => move_selection(app, false),
        KeyCode::Down => move_selection(app, true),
        KeyCode::Enter => open_transcript(app),
        KeyCode::Char('s') => request_stop(app),
        KeyCode::Char('r') => request_transcript_list(app),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
