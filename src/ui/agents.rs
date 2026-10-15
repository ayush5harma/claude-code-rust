// SPDX-License-Identifier: Apache-2.0

//! The `/agents` screen: the active session's subagents, the selected agent's
//! details, and one saved transcript.

use super::agent_rows::{
    AgentColumns, AgentRow, elapsed_text, render_agent_row, session_agent_rows, token_text,
    tool_activity,
};
use super::config::common::{
    badge_span, help_style, hint_text_with_escape, position_counter, render_message,
    selection_line, title_style, wrapped_height,
};
use super::theme;
use crate::agent::types::SubagentMessageRole;
use crate::app::App;
use crate::app::agents::{NoticeKind, SubagentStatus, TranscriptView};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

const TITLE: &str = "Agents";
/// Below this width the details sit under the list instead of beside it.
const SIDE_BY_SIDE_MIN_WIDTH: u16 = 110;
const DETAIL_LABEL_WIDTH: usize = 12;
const PROMPT_PREVIEW_LINES: usize = 6;

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
    let notice = notice_line(app);
    let hint = hint(app, inner.width);
    let notice_height = notice.as_ref().map_or(0, |line| wrapped_height(line.clone(), inner.width));
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
        let scroll = render_agents(frame, chunks[0], chunks[2], app);
        app.agents.detail_scroll = scroll;
    }
    if let Some(notice) = notice {
        frame.render_widget(Paragraph::new(notice).wrap(Wrap { trim: false }), chunks[3]);
    }
    frame.render_widget(
        Paragraph::new(hint).style(help_style()).wrap(Wrap { trim: false }),
        chunks[4],
    );
}

fn notice_line(app: &App) -> Option<Line<'static>> {
    if let Some(notice) = &app.agents.notice {
        let color = match notice.kind {
            NoticeKind::Error => theme::danger(),
            NoticeKind::Info => theme::tertiary(),
        };
        return Some(Line::styled(notice.text.clone(), Style::default().fg(color)));
    }
    (app.agents.listing && app.agents.transcript.is_none()).then(|| {
        Line::styled("Checking saved transcripts...", Style::default().fg(theme::tertiary()))
    })
}

fn hint(app: &App, width: u16) -> String {
    if app.agents.transcript.is_some() {
        return hint_text_with_escape(
            vec!["Up/Down scroll", "PgUp/PgDn page"],
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
        SubagentStatus::Stopped => theme::Tone::Neutral,
    };
    badge_span(status.label(), tone)
}

fn labeled(label: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label:<DETAIL_LABEL_WIDTH$}"),
            Style::default().fg(theme::tertiary()),
        ),
        Span::styled(value.into(), Style::default().fg(theme::label())),
    ])
}

fn continuation(value: impl Into<String>) -> Line<'static> {
    labeled("", value)
}

/// The selected agent: identity and model choice, usage, transcript, activity and prompt.
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
    if !row.model.is_empty() {
        identity.push(row.model.to_owned());
    }
    if let Some(alias) = row.requested_model {
        identity.push(format!("asked for {alias}"));
    } else if row.definition_model.is_some() {
        identity.push("agent default".to_owned());
    }
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
    lines.push(labeled("Transcript", transcript_state(app, &row.task.task_id)));
    if !row.now.is_empty() {
        lines.push(labeled("Now", row.now.clone().into_owned()));
    }
    if let Some(parent) = row.parent_id {
        // "Now" already shows the latest child tool.
        for (index, tool) in app.recent_child_tools(parent).skip(1).enumerate() {
            let activity = tool_activity(tool).into_owned();
            lines.push(if index == 0 {
                labeled("Recent", activity)
            } else {
                continuation(activity)
            });
        }
    }
    if let Some(prompt) =
        row.raw_input.and_then(|input| input.get("prompt")).and_then(serde_json::Value::as_str)
    {
        let available = usize::from(width).saturating_sub(DETAIL_LABEL_WIDTH).max(1);
        let wrapped = crate::ui::wrap::wrap_plain(prompt, available);
        for (index, line) in wrapped.iter().take(PROMPT_PREVIEW_LINES).enumerate() {
            lines.push(if index == 0 {
                labeled("Prompt", line.clone())
            } else {
                continuation(line.clone())
            });
        }
        if wrapped.len() > PROMPT_PREVIEW_LINES {
            lines.push(continuation("..."));
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
    let subject = app
        .sdk_inventory
        .tasks
        .iter()
        .find(|task| {
            app.agents.transcript.as_ref().is_some_and(|view| view.agent_id == task.task_id)
        })
        .map(|task| task.subject.clone());
    let Some(view) = app.agents.transcript.as_mut() else { return };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(TITLE, title_style().fg(theme::accent())),
            Span::styled(" › ", Style::default().fg(theme::tertiary())),
            Span::styled(subject.unwrap_or_else(|| view.agent_id.clone()), title_style()),
        ])),
        title_area,
    );
    if let Some(error) = &view.error {
        render_message(frame, body, "Could not read the transcript", error);
        return;
    }
    let Some(messages) = &view.messages else {
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
    let text = Text::from(transcript_lines(view));
    let total =
        Paragraph::new(text.clone()).wrap(Wrap { trim: false }).line_count(body.width.max(1));
    let max_scroll =
        u16::try_from(total.saturating_sub(usize::from(body.height))).unwrap_or(u16::MAX);
    view.scroll = view.scroll.min(max_scroll);
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }).scroll((view.scroll, 0)),
        body,
    );
}

fn transcript_lines(view: &TranscriptView) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for message in view.messages.iter().flatten() {
        match message.role {
            SubagentMessageRole::Tool => {
                lines.push(Line::styled(
                    format!("• {}", message.text),
                    Style::default().fg(theme::tertiary()),
                ));
                continue;
            }
            SubagentMessageRole::User => lines.push(Line::styled("Prompt", help_style())),
            SubagentMessageRole::Assistant => lines.push(Line::styled("Reply", title_style())),
        }
        lines.extend(message.text.lines().map(|line| Line::from(line.to_owned())));
        lines.push(Line::default());
    }
    lines
}

#[cfg(test)]
mod tests;
