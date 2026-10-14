// SPDX-License-Identifier: Apache-2.0

//! Current-session subagent browsing. The SDK task inventory remains the status authority.

use crate::agent::events::ClientEvent;
use crate::agent::types::{SessionAgentsAction, SubagentMessage};
use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Default)]
pub(crate) struct SessionAgentsState {
    pub session_id: Option<String>,
    pub agent_ids: Vec<String>,
    pub selected: usize,
    pub overlay_selected_id: Option<String>,
    sdk_agent_ids: Vec<String>,
    pub messages: Vec<SubagentMessage>,
    pub message_scroll: u16,
    pub error: Option<String>,
    pub loading: bool,
    pending: Option<(String, SessionAgentsAction, Option<String>)>,
}

impl SessionAgentsState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.agent_ids.get(self.selected).map(String::as_str)
    }
}

pub(crate) fn sync_live(app: &mut App) {
    let previous = app.session_agents.selected_id().map(str::to_owned);
    let mut ids = app.session_agents.sdk_agent_ids.clone();
    for task in app.sdk_inventory.tasks.iter().filter(|task| is_session_agent_task(task)) {
        if !ids.contains(&task.task_id) {
            ids.push(task.task_id.clone());
        }
    }
    app.session_agents.agent_ids = ids;
    app.session_agents.selected = previous
        .as_ref()
        .and_then(|id| app.session_agents.agent_ids.iter().position(|candidate| candidate == id))
        .unwrap_or(0);
    if previous.as_deref() != app.session_agents.selected_id() {
        app.session_agents.messages.clear();
        app.session_agents.message_scroll = 0;
    }
}

fn select_agent(app: &mut App, selected: usize) {
    if app.session_agents.selected != selected {
        app.session_agents.messages.clear();
        app.session_agents.message_scroll = 0;
        app.session_agents.error = None;
    }
    app.session_agents.selected = selected;
}

pub(crate) fn handle_tasks_key(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc
        || app.keymap.resolve_event(crate::app::keymap::KeyContext::ChatInput, key).is_some_and(
            |binding| {
                binding.action
                    == crate::app::keymap::KeyAction::App(
                        crate::app::keymap::AppAction::ToggleTasks,
                    )
            },
        )
    {
        crate::app::view::set_chat_surface(app);
        return;
    }
    if !key.modifiers.is_empty() {
        return;
    }
    let ids: Vec<String> = crate::ui::agent_rows::session_agent_rows(app)
        .iter()
        .map(|row| row.task.task_id.clone())
        .collect();
    let current = app
        .session_agents
        .overlay_selected_id
        .as_ref()
        .and_then(|id| ids.iter().position(|candidate| candidate == id))
        .unwrap_or(0);
    match key.code {
        KeyCode::Up | KeyCode::Down => {
            let selected = if key.code == KeyCode::Up {
                current.saturating_sub(1)
            } else {
                (current + 1).min(ids.len().saturating_sub(1))
            };
            app.session_agents.overlay_selected_id = ids.get(selected).cloned();
        }
        KeyCode::Enter => {
            if let Some(id) = ids.get(current) {
                sync_live(app);
                let selected = app
                    .session_agents
                    .agent_ids
                    .iter()
                    .position(|candidate| candidate == id)
                    .unwrap_or(0);
                select_agent(app, selected);
                if let Err(error) = crate::app::config::open_tab(app, crate::app::ConfigTab::Agents)
                {
                    app.session_agents.error = Some(error);
                }
            }
        }
        _ => {}
    }
}

fn next_request(
    app: &mut App,
    action: SessionAgentsAction,
    agent_id: Option<String>,
) -> Option<(String, String)> {
    let sid = app.session_runtime.session_id.as_ref()?.to_string();
    let request_id = uuid::Uuid::new_v4().to_string();
    app.session_agents.session_id = Some(sid.clone());
    app.session_agents.pending = Some((request_id.clone(), action, agent_id));
    app.session_agents.loading = true;
    app.session_agents.error = None;
    Some((sid, request_id))
}

pub(crate) fn refresh(app: &mut App) {
    if app.session_runtime.conn.is_none() {
        sync_live(app);
        return;
    }
    sync_live(app);
    let Some((sid, request_id)) = next_request(app, SessionAgentsAction::ListSessionAgents, None)
    else {
        app.session_agents.clear();
        return;
    };
    let Some(conn) = app.session_runtime.conn.as_ref() else {
        app.session_agents.clear();
        return;
    };
    if let Err(error) = conn.list_session_agents(sid, request_id) {
        app.session_agents.loading = false;
        app.session_agents.error = Some(format!("Cannot load agents: {error}"));
    }
}

fn inspect_selected(app: &mut App) {
    if app.session_runtime.conn.is_none() {
        app.session_agents.clear();
        return;
    }
    let Some(agent_id) = app.session_agents.selected_id().map(str::to_owned) else {
        app.session_agents.messages.clear();
        return;
    };
    let Some((sid, request_id)) =
        next_request(app, SessionAgentsAction::GetSubagentMessages, Some(agent_id.clone()))
    else {
        return;
    };
    app.session_agents.messages.clear();
    app.session_agents.message_scroll = 0;
    let Some(conn) = app.session_runtime.conn.as_ref() else {
        app.session_agents.clear();
        return;
    };
    if let Err(error) = conn.get_subagent_messages(sid, agent_id, request_id) {
        app.session_agents.loading = false;
        app.session_agents.error = Some(format!("Cannot inspect agent: {error}"));
    }
}

pub(crate) fn is_running_task(app: &App, agent_id: &str) -> bool {
    app.sdk_inventory.tasks.iter().any(|task| {
        if task.task_id != agent_id || task.status != crate::agent::model::TaskStatus::InProgress {
            return false;
        }
        is_session_agent_task(task)
    })
}

pub(crate) fn is_session_agent_task(task: &crate::agent::model::TaskItem) -> bool {
    let Some(metadata) = task.metadata.as_ref().and_then(serde_json::Value::as_object) else {
        return false;
    };
    metadata.get("subagent_type").is_some_and(serde_json::Value::is_string)
        || metadata.get("task_type").and_then(serde_json::Value::as_str) == Some("local_agent")
}

pub(crate) fn stop_selected(app: &mut App) {
    if app.session_runtime.conn.is_none() {
        app.session_agents.clear();
        return;
    }
    let Some(agent_id) = app.session_agents.selected_id().map(str::to_owned) else {
        return;
    };
    if !is_running_task(app, &agent_id) {
        app.session_agents.error = Some("Selected agent has no running task to stop.".to_owned());
        return;
    }
    let Some((sid, request_id)) =
        next_request(app, SessionAgentsAction::StopSubagentTask, Some(agent_id.clone()))
    else {
        return;
    };
    let Some(conn) = app.session_runtime.conn.as_ref() else {
        app.session_agents.clear();
        return;
    };
    if let Err(error) = conn.stop_subagent_task(sid, agent_id, request_id) {
        app.session_agents.loading = false;
        app.session_agents.error = Some(format!("Cannot stop agent: {error}"));
    }
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
        return false;
    }
    match key.code {
        KeyCode::Up => {
            select_agent(app, app.session_agents.selected.saturating_sub(1));
            inspect_selected(app);
            true
        }
        KeyCode::Down => {
            let selected = (app.session_agents.selected + 1)
                .min(app.session_agents.agent_ids.len().saturating_sub(1));
            select_agent(app, selected);
            inspect_selected(app);
            true
        }
        KeyCode::PageDown => {
            app.session_agents.message_scroll = app.session_agents.message_scroll.saturating_add(8);
            true
        }
        KeyCode::PageUp => {
            app.session_agents.message_scroll = app.session_agents.message_scroll.saturating_sub(8);
            true
        }
        KeyCode::Char('r' | 'R') => {
            refresh(app);
            true
        }
        KeyCode::Char('s' | 'S') => {
            stop_selected(app);
            true
        }
        _ => false,
    }
}

pub(crate) fn apply_result(app: &mut App, event: ClientEvent) {
    let ClientEvent::SessionAgentsResultReceived {
        session_id,
        request_id,
        action,
        agent_id,
        agent_ids,
        messages,
        error,
    } = event
    else {
        return;
    };
    if app.session_runtime.session_id.as_ref().is_none_or(|sid| sid.to_string() != session_id)
        || app.session_agents.session_id.as_deref() != Some(session_id.as_str())
    {
        return;
    }
    let Some((expected_id, expected_action, expected_agent)) = app.session_agents.pending.as_ref()
    else {
        return;
    };
    if request_id.as_deref() != Some(expected_id.as_str())
        || action != *expected_action
        || agent_id.as_deref() != expected_agent.as_deref()
    {
        return;
    }
    app.session_agents.pending = None;
    app.session_agents.loading = false;
    if let Some(error) = error {
        app.session_agents.error = Some(error);
        return;
    }
    match action {
        SessionAgentsAction::ListSessionAgents => {
            app.session_agents.sdk_agent_ids = agent_ids;
            sync_live(app);
            inspect_selected(app);
        }
        SessionAgentsAction::GetSubagentMessages => {
            if agent_id.as_deref() == app.session_agents.selected_id() {
                app.session_agents.messages = messages;
            }
        }
        SessionAgentsAction::StopSubagentTask => {
            app.config.status_message = Some("Stop requested for the selected agent.".to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::AgentConnection;
    use crate::agent::model::{SessionId, TaskItem, TaskStatus};
    use crate::agent::wire::BridgeCommand;
    use std::rc::Rc;

    #[test]
    fn live_inventory_merges_sdk_ids_and_preserves_selection_by_identity() {
        let mut app = App::test_default();
        app.session_agents.sdk_agent_ids = vec!["recorded".to_owned()];
        let mut task = TaskItem::new("live", "Build", TaskStatus::InProgress);
        task.metadata = Some(serde_json::json!({"task_type": "local_agent"}));
        app.sdk_inventory.tasks.push(task);
        sync_live(&mut app);
        assert_eq!(app.session_agents.agent_ids, ["recorded", "live"]);
        app.session_agents.selected = 1;
        app.session_agents.sdk_agent_ids = vec!["live".to_owned(), "recorded".to_owned()];
        sync_live(&mut app);
        assert_eq!(app.session_agents.selected_id(), Some("live"));
        assert_eq!(app.session_agents.agent_ids.len(), 2);
        app.session_agents.sdk_agent_ids = vec!["recorded".to_owned()];
        app.sdk_inventory.tasks.clear();
        sync_live(&mut app);
        assert_eq!(app.session_agents.selected_id(), Some("recorded"));
    }

    #[test]
    fn opening_another_overlay_agent_clears_previous_messages_before_sdk_reply() {
        let mut app = App::test_default();
        let (conn, _commands) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(conn));
        app.session_runtime.session_id = Some(SessionId::new("session-1"));
        for id in ["first", "second"] {
            let mut task = TaskItem::new(id, id, TaskStatus::InProgress);
            task.metadata = Some(serde_json::json!({"task_type": "local_agent"}));
            app.sdk_inventory.tasks.push(task);
        }
        sync_live(&mut app);
        app.session_agents.messages = vec![SubagentMessage {
            role: "assistant".to_owned(),
            text: "First agent private context".to_owned(),
        }];
        app.session_agents.message_scroll = 8;
        app.session_agents.error = Some("first agent error".to_owned());
        app.session_agents.overlay_selected_id = Some("second".to_owned());
        handle_tasks_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.session_agents.selected_id(), Some("second"));
        assert!(app.session_agents.messages.is_empty());
        assert_eq!(app.session_agents.message_scroll, 0);
        assert!(app.session_agents.error.is_none());
        assert!(app.session_agents.loading);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_and_inspect_ignore_stale_requests_and_stop_only_the_matching_running_task() {
        let mut app = App::test_default();
        let (conn, mut commands) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(conn));
        app.session_runtime.session_id = Some(SessionId::new("session-1"));
        refresh(&mut app);
        let list = commands.recv_envelope().await.expect("list command");
        let request = list.request_id.expect("request id");
        apply_result(
            &mut app,
            ClientEvent::SessionAgentsResultReceived {
                session_id: "other-session".to_owned(),
                request_id: Some(request.clone()),
                action: SessionAgentsAction::ListSessionAgents,
                agent_id: None,
                agent_ids: vec!["wrong".to_owned()],
                messages: vec![],
                error: None,
            },
        );
        apply_result(
            &mut app,
            ClientEvent::SessionAgentsResultReceived {
                session_id: "session-1".to_owned(),
                request_id: Some("stale".to_owned()),
                action: SessionAgentsAction::ListSessionAgents,
                agent_id: None,
                agent_ids: vec!["wrong".to_owned()],
                messages: vec![],
                error: None,
            },
        );
        assert!(app.session_agents.agent_ids.is_empty());

        let mut task = TaskItem::new("agent-running", "Review", TaskStatus::InProgress);
        task.metadata = Some(serde_json::json!({ "subagent_type": "general-purpose" }));
        app.sdk_inventory.tasks.push(task);
        apply_result(
            &mut app,
            ClientEvent::SessionAgentsResultReceived {
                session_id: "session-1".to_owned(),
                request_id: Some(request.clone()),
                action: SessionAgentsAction::ListSessionAgents,
                agent_id: None,
                agent_ids: vec!["agent-running".to_owned()],
                messages: vec![],
                error: None,
            },
        );
        let inspect = commands.recv_envelope().await.expect("inspect command");
        assert!(
            matches!(inspect.command, BridgeCommand::GetSubagentMessages { ref agent_id, .. } if agent_id == "agent-running")
        );
        let inspect_request = inspect.request_id.expect("inspect request");
        apply_result(
            &mut app,
            ClientEvent::SessionAgentsResultReceived {
                session_id: "session-1".to_owned(),
                request_id: Some(inspect_request.clone()),
                action: SessionAgentsAction::GetSubagentMessages,
                agent_id: Some("wrong-agent".to_owned()),
                agent_ids: vec![],
                messages: vec![SubagentMessage {
                    role: "assistant".to_owned(),
                    text: "wrong".to_owned(),
                }],
                error: None,
            },
        );
        assert!(app.session_agents.messages.is_empty());
        apply_result(
            &mut app,
            ClientEvent::SessionAgentsResultReceived {
                session_id: "session-1".to_owned(),
                request_id: Some(inspect_request.clone()),
                action: SessionAgentsAction::GetSubagentMessages,
                agent_id: Some("agent-running".to_owned()),
                agent_ids: vec![],
                messages: vec![SubagentMessage {
                    role: "assistant".to_owned(),
                    text: "Checked".to_owned(),
                }],
                error: None,
            },
        );
        assert_eq!(app.session_agents.messages[0].text, "Checked");

        stop_selected(&mut app);
        let stop = commands.recv_envelope().await.expect("stop command");
        assert!(
            matches!(stop.command, BridgeCommand::StopSubagentTask { ref session_id, ref agent_id }
            if session_id == "session-1" && agent_id == "agent-running")
        );
        app.sdk_inventory.tasks[0].status = TaskStatus::Completed;
        stop_selected(&mut app);
        assert!(
            app.session_agents
                .error
                .as_deref()
                .is_some_and(|error| error.contains("no running task"))
        );
    }
}
