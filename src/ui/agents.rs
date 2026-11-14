// SPDX-License-Identifier: Apache-2.0

//! The `/agents` screen: the active session's subagents, the selected agent's
//! details, and one saved transcript.

use super::agent_rows::{
    AgentColumns, AgentRow, elapsed_text, linked_agent_tool, render_agent_row, session_agent_rows,
    token_text, tool_activity,
};
use super::config::common::{
    badge_span, help_style, hint_text_with_escape, position_counter, render_message,
    selection_line, title_style, wrapped_height,
};
use super::theme;
use crate::agent::types::{SubagentMessage, SubagentMessageRole};
use crate::app::App;
use crate::app::agents::{
    NoticeKind, SubagentStatus, WrappedTranscript, recent_child_tools, saved_response,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

const TITLE: &str = "Agents";
/// Below this width the details sit under the list instead of beside it.
const SIDE_BY_SIDE_MIN_WIDTH: u16 = 110;
const DETAIL_LABEL_WIDTH: usize = 14;
/// Rows the details show of a prompt or a response; the transcript has all of it.
const PREVIEW_LINES: usize = 6;

pub(crate) fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 30 || area.height < 8 {
        frame.render_widget(
            Paragraph::new("Window too small. Resize to view agents.\nEsc close")
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let compact = area.height < 16;
    let inner = area.inner(Margin { vertical: u16::from(!compact), horizontal: 2 });
    let notice = notice_lines(app);
    let hint = hint(app, inner.width);
    let notice_height =
        if notice.lines.is_empty() { 0 } else { wrapped_height(notice.clone(), inner.width) };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(u16::from(!compact)),
            Constraint::Min(3),
            Constraint::Length(notice_height),
            Constraint::Length(wrapped_height(hint.as_str(), inner.width).clamp(1, 3)),
        ])
        .split(inner);

    if app.agents.transcript.is_some() {
        render_transcript(frame, chunks[0], chunks[2], app);
    } else {
        app.agents.detail_scroll = render_agents(frame, chunks[0], chunks[2], app);
    }
    frame.render_widget(Paragraph::new(notice).wrap(Wrap { trim: false }), chunks[3]);
    frame.render_widget(
        Paragraph::new(hint).style(help_style()).wrap(Wrap { trim: false }),
        chunks[4],
    );
}

/// The transcript check's state under the list, then the last action's notice.
fn notice_lines(app: &App) -> Text<'static> {
    let mut lines = Vec::new();
    if app.agents.transcript.is_none() {
        if app.agents.listing {
            lines.push(Line::styled(
                "Checking saved transcripts...",
                Style::default().fg(theme::tertiary()),
            ));
        } else if let Some(error) = &app.agents.list_error {
            lines.push(Line::styled(
                format!("Could not check transcripts: {error}"),
                Style::default().fg(theme::danger()),
            ));
        }
    }
    if let Some(notice) = &app.agents.notice {
        let color = match notice.kind {
            NoticeKind::Error => theme::danger(),
            NoticeKind::Info => theme::tertiary(),
        };
        lines.push(Line::styled(notice.text.clone(), Style::default().fg(color)));
    }
    Text::from(lines)
}

fn hint(app: &App, width: u16) -> String {
    if app.agents.transcript.is_some() {
        return hint_text_with_escape(
            vec!["Up/Down scroll", "r refresh", "PgUp/PgDn"],
            width,
            "Esc/Left back",
        );
    }
    hint_text_with_escape(
        vec!["Up/Down select", "Enter transcript", "s stop", "r refresh", "PgUp/PgDn details"],
        width,
        "Esc close",
    )
}

/// Draws the list and the selected agent's details; returns the details' clamped scroll.
fn render_agents(frame: &mut Frame, title_area: Rect, body: Rect, app: &App) -> u16 {
    let rows = session_agent_rows(app);
    let selected = app.agents.selected_index(&rows);
    let counter = selected.map_or_else(
        || Span::styled(format!("-/{}", rows.len()), Style::default().fg(theme::tertiary())),
        |index| position_counter(index, rows.len()),
    );
    let mut title =
        vec![Span::styled(TITLE, title_style().fg(theme::accent())), Span::raw("  "), counter];
    let running = rows.iter().filter(|row| row.status.is_running()).count();
    if running > 0 {
        title.push(Span::styled(
            format!("  {running} running"),
            Style::default().fg(theme::tertiary()),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(title)), title_area);
    if app.session_runtime.session_id.is_none() {
        render_message(
            frame,
            body,
            "No active session",
            "Start or resume a session to see its subagents.",
        );
        return 0;
    }
    if rows.is_empty() {
        render_message(
            frame,
            body,
            "No subagents yet",
            "Subagents this session starts appear here with their live status.",
        );
        return 0;
    }
    let (list_area, detail_area) = split_body(body, rows.len());
    render_list(frame, list_area, &rows, selected);
    let Some(row) = selected.and_then(|index| rows.get(index)) else {
        render_message(frame, detail_area, "No agent selected", "Up/Down selects an agent.");
        return 0;
    };

    let details = Text::from(detail_lines(app, row, detail_area.width));
    let total =
        Paragraph::new(details.clone()).wrap(Wrap { trim: false }).line_count(detail_area.width);
    let max_scroll =
        u16::try_from(total.saturating_sub(usize::from(detail_area.height))).unwrap_or(u16::MAX);
    let scroll = app.agents.detail_scroll.min(max_scroll);
    frame.render_widget(
        Paragraph::new(details).wrap(Wrap { trim: false }).scroll((scroll, 0)),
        detail_area,
    );
    scroll
}

/// Beside each other when wide; otherwise the list takes at most a third of
/// the height and the details fill the rest.
fn split_body(body: Rect, row_count: usize) -> (Rect, Rect) {
    if body.width >= SIDE_BY_SIDE_MIN_WIDTH {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Length(2), Constraint::Min(0)])
            .split(body);
        return (panes[0], panes[2]);
    }
    let list_height = u16::try_from(row_count).unwrap_or(u16::MAX).min((body.height / 3).max(3));
    let panes = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(list_height), Constraint::Length(1), Constraint::Min(0)])
        .split(body);
    (panes[0], panes[2])
}

fn render_list(frame: &mut Frame, area: Rect, rows: &[AgentRow<'_>], selected: Option<usize>) {
    let columns = AgentColumns::for_rows(rows);
    let offset = selected.unwrap_or(0).saturating_sub(usize::from(area.height.saturating_sub(1)));
    let lines: Vec<Line<'static>> = rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(usize::from(area.height))
        .map(|(index, row)| {
            let line = render_agent_row(row, area.width.saturating_sub(2), &columns, false);
            selection_line(line, Some(index) == selected)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn status_badge(status: SubagentStatus) -> Span<'static> {
    let tone = match status {
        SubagentStatus::Pending => theme::Tone::Info,
        SubagentStatus::Running => theme::Tone::Accent,
        SubagentStatus::Stopping => theme::Tone::Warning,
        SubagentStatus::Completed => theme::Tone::Success,
        SubagentStatus::Failed => theme::Tone::Danger,
        SubagentStatus::Stopped | SubagentStatus::Interrupted => theme::Tone::Neutral,
    };
    badge_span(status.label(), tone)
}

fn labeled(label: &str, value: impl Into<String>) -> Line<'static> {
    labeled_in(label, value, theme::label())
}

fn labeled_in(
    label: &str,
    value: impl Into<String>,
    color: ratatui::style::Color,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label:<DETAIL_LABEL_WIDTH$}"),
            Style::default().fg(theme::tertiary()),
        ),
        Span::styled(value.into(), Style::default().fg(color)),
    ])
}

fn continuation(value: impl Into<String>) -> Line<'static> {
    labeled("", value)
}

/// The first `PREVIEW_LINES` rows of `text` under `label`, then `...` when it goes on.
fn push_preview(lines: &mut Vec<Line<'static>>, label: &str, text: &str, width: u16) {
    let available = usize::from(width).saturating_sub(DETAIL_LABEL_WIDTH).max(1);
    // A row holds at most `available` characters, so this head fills more rows
    // than are shown even where wrapping drops spaces: a long prompt or response
    // costs a frame no more than a short one.
    let head_end = text
        .char_indices()
        .nth((PREVIEW_LINES + 1) * available * 2)
        .map_or(text.len(), |(end, _)| end);
    let rows = crate::ui::wrap::wrap_plain(&text[..head_end], available);
    for (index, row) in rows.iter().take(PREVIEW_LINES).enumerate() {
        lines.push(if index == 0 {
            labeled(label, row.clone())
        } else {
            continuation(row.clone())
        });
    }
    if rows.len() > PREVIEW_LINES || head_end < text.len() {
        lines.push(continuation("..."));
    }
}

/// Model provenance for the identity line: what the SDK observed for this run
/// first, then the model the lifecycle resolved (with what was asked for), then
/// what the run inherited, requested or takes from its definition.
fn model_parts(app: &App, row: &AgentRow<'_>) -> Vec<String> {
    // A call that launched an earlier run of a resumed agent says nothing about this one.
    let call = linked_agent_tool(app, row.task)
        .filter(|_| row.task.source_tool_call_run_id == row.task.run_id);
    let choice =
        super::tool_call::agent_model_choice(call, row.subagent_type(), row.definition_model);
    if let Some((models, source)) = &choice
        && *source == "observed"
    {
        return vec![format!("{models} (observed)")];
    }
    if !row.model.is_empty() && !row.model_requested {
        let mut parts = vec![row.model.to_owned()];
        if let Some(alias) = row.requested_model {
            parts.push(format!("asked for {alias}"));
        } else if row.definition_model.is_some() {
            parts.push("agent default".to_owned());
        }
        return parts;
    }
    choice.map(|(model, source)| format!("{model} ({source})")).into_iter().collect()
}

fn known_parent<'a>(app: &'a App, row: &AgentRow<'_>) -> Option<&'a crate::agent::model::TaskItem> {
    let id = row.task.parent_task_id.as_deref()?;
    app.sdk_inventory.tasks.iter().find(|task| task.task_id == id)
}

/// The selected agent: identity and model choice, usage, transcript, activity,
/// prompt and the saved response.
fn detail_lines(app: &App, row: &AgentRow<'_>, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled(row.task.subject.clone(), title_style()),
        Span::raw("  "),
        status_badge(row.status),
        Span::styled(
            format!("  {}", elapsed_text(row.elapsed_ms)),
            Style::default().fg(theme::tertiary()),
        ),
    ])];
    let mut identity = vec![row.name.to_owned()];
    identity.extend(model_parts(app, row));
    if row.raw_input.and_then(|input| input.get("isolation")).and_then(serde_json::Value::as_str)
        == Some("worktree")
    {
        identity.push("isolated worktree".to_owned());
    }
    lines.push(Line::styled(identity.join(" · "), Style::default().fg(theme::secondary())));
    let mut metrics = Vec::new();
    if let Some(tools) = row.tools {
        metrics.push(format!("{tools} tools"));
    }
    if let Some(tokens) = row.tokens {
        metrics.push(format!("{} tokens", token_text(tokens)));
    }
    if !metrics.is_empty() {
        lines.push(Line::styled(metrics.join(" · "), Style::default().fg(theme::secondary())));
    }
    lines.push(Line::default());
    if let Some(kind) = row.subagent_type().filter(|kind| *kind != row.name) {
        lines.push(labeled("Type", kind));
    }
    if let Some(parent) = known_parent(app, row) {
        let kind =
            parent.metadata.as_ref().and_then(|metadata| metadata.get("subagent_type")?.as_str());
        let label = kind
            .map_or_else(|| parent.subject.clone(), |kind| format!("{} ({kind})", parent.subject));
        lines.push(labeled("Parent", label));
    }
    if let Some(run) = &row.task.run_id {
        lines.push(labeled("Run", run.clone()));
    }
    lines.push(labeled("Transcript", transcript_state(app, &row.task.task_id)));

    let children: Vec<String> = recent_child_tools(app, row.task)
        .into_iter()
        .map(|tool| tool_activity(tool).into_owned())
        .collect();
    let mut recent_from = 0;
    if row.status.is_running() {
        if !row.now.is_empty() {
            lines.push(labeled("Now", row.now.clone().into_owned()));
        }
        // "Recent" does not repeat the latest child tool when "Now" shows it.
        recent_from =
            usize::from(children.first().is_some_and(|latest| latest.as_str() == row.now.as_ref()));
    } else if let Some(outcome) = row.failure().or_else(|| row.summary()) {
        // An interrupted row has no outcome; its text is the last progress.
        let label =
            if row.status == SubagentStatus::Interrupted { "Last progress" } else { "Result" };
        push_preview(&mut lines, label, outcome, width);
    }
    for (index, activity) in children.iter().skip(recent_from).enumerate() {
        lines.push(if index == 0 {
            labeled("Recent", activity.clone())
        } else {
            continuation(activity.clone())
        });
    }

    let prompt =
        row.raw_input.and_then(|input| input.get("prompt")?.as_str()).or_else(|| row.prompt());
    if let Some(prompt) = prompt {
        push_preview(&mut lines, "Prompt", prompt, width);
    }
    if let Some(response) = saved_response(app, row) {
        match response {
            Ok(response) => push_preview(&mut lines, "Response", response, width),
            Err(error) => lines.push(labeled_in(
                "Response",
                format!("Could not read the transcript: {error}. Press r to try again."),
                theme::danger(),
            )),
        }
    }
    lines
}

fn transcript_state(app: &App, task_id: &str) -> &'static str {
    match app.agents.has_transcript(task_id) {
        Some(true) => "saved · Enter to read",
        Some(false) => "not saved yet · r to check again",
        None if app.agents.listing => "checking...",
        None => "unknown · r to check",
    }
}

fn render_transcript(frame: &mut Frame, title_area: Rect, body: Rect, app: &mut App) {
    let agents = &mut *app.agents;
    let (Some(read), Some(view)) = (agents.read.as_ref(), agents.transcript.as_mut()) else {
        return;
    };
    let subject = app
        .sdk_inventory
        .tasks
        .iter()
        .find(|task| task.task_id == read.agent_id)
        .map_or_else(|| read.agent_id.clone(), |task| task.subject.clone());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(TITLE, title_style().fg(theme::accent())),
            Span::styled(" › ", Style::default().fg(theme::tertiary())),
            Span::styled(
                if read.saved_snapshot { format!("{subject} (saved snapshot)") } else { subject },
                title_style(),
            ),
        ])),
        title_area,
    );
    if let Some(error) = &read.error {
        render_message(frame, body, "Could not read the transcript", error);
        return;
    }
    let Some(messages) = &read.messages else {
        if read.saved_snapshot {
            render_message(
                frame,
                body,
                "No saved snapshot",
                "The execution changed before the read completed. Press r to read the saved conversation.",
            );
            return;
        }
        render_message(
            frame,
            body,
            "Reading transcript",
            "Waiting for the saved subagent transcript.",
        );
        return;
    };
    if messages.is_empty() {
        render_message(
            frame,
            body,
            "Empty transcript",
            "The saved transcript has no messages yet.",
        );
        return;
    }
    let width = body.width.max(1);
    if view.wrapped.as_ref().is_none_or(|cached| cached.width != width) {
        // One logical line at a time: the helper caps its result at u16::MAX
        // rows, which a whole long transcript can exceed but one line cannot.
        let rows = transcript_lines(messages, width)
            .into_iter()
            .flat_map(|line| crate::ui::wrap::wrap_lines_to_physical_rows(&[line], width))
            .collect();
        view.wrapped = Some(WrappedTranscript { width, rows });
    }
    let Some(wrapped) = &view.wrapped else { return };
    // A re-read keeps the position; one that shrank the transcript ends at its last page.
    let max_scroll = wrapped.rows.len().saturating_sub(usize::from(body.height));
    view.scroll = view.scroll.min(max_scroll);
    let visible: Vec<Line<'static>> =
        wrapped.rows.iter().skip(view.scroll).take(usize::from(body.height)).cloned().collect();
    frame.render_widget(Paragraph::new(visible), body);
}

/// Prompts and SDK reminders as written and replies through the chat's
/// Markdown rendering, each under its label; tool calls as one dim line each.
fn transcript_lines(messages: &[SubagentMessage], width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for message in messages {
        let (label, body) = match message.role {
            SubagentMessageRole::Tool => {
                lines.push(Line::styled(
                    format!("• {}", message.text),
                    Style::default().fg(theme::tertiary()),
                ));
                continue;
            }
            SubagentMessageRole::User => (
                Line::styled("Prompt", help_style()),
                message.text.lines().map(|line| Line::from(line.to_owned())).collect(),
            ),
            SubagentMessageRole::SystemReminder => (
                Line::styled("System reminder", Style::default().fg(theme::tertiary())),
                message
                    .text
                    .lines()
                    .map(|line| {
                        Line::styled(line.to_owned(), Style::default().fg(theme::tertiary()))
                    })
                    .collect(),
            ),
            SubagentMessageRole::Assistant => (
                Line::styled("Reply", title_style()),
                super::markdown::render_markdown_at_width(&message.text, width, None, false),
            ),
        };
        lines.push(label);
        lines.extend(body);
        lines.push(Line::default());
    }
    lines
}

#[cfg(test)]
mod tests;
