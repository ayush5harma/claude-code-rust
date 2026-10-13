// SPDX-License-Identifier: Apache-2.0

//! The `/agents` screen: the active session's subagents and one saved transcript.

use super::config::common::{
    badge_span, help_style, hint_text_with_escape, position_counter, render_message,
    selection_line, selection_style, title_style, wrapped_height,
};
use super::theme;
use crate::agent::types::SubagentMessageRole;
use crate::app::App;
use crate::app::agents::{
    NoticeKind, SubagentRow, SubagentStatus, TranscriptView, running_subagent_count, subagent_rows,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

const TITLE: &str = "Subagents";

pub(crate) fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 30 || area.height < 8 {
        frame.render_widget(
            Paragraph::new("Window too small. Resize to view subagents.\nEsc close")
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
        render_list(frame, chunks[0], chunks[2], app);
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
            NoticeKind::Error => theme::STATUS_ERROR,
            NoticeKind::Info => theme::DIM,
        };
        return Some(Line::styled(notice.text.clone(), Style::default().fg(color)));
    }
    (app.agents.listing && app.agents.transcript.is_none())
        .then(|| Line::styled("Checking saved transcripts...", Style::default().fg(theme::DIM)))
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
        vec!["Up/Down select", "Enter transcript", "s stop", "r re-check transcripts"],
        width,
        "Esc close",
    )
}

fn render_list(frame: &mut Frame, title_area: Rect, body: Rect, app: &App) {
    let rows = subagent_rows(app);
    let selected = app.agents.selected_index(&rows);
    let mut title = vec![
        Span::styled(TITLE, title_style().fg(theme::RUST_ORANGE)),
        Span::raw("  "),
        position_counter(selected, rows.len()),
    ];
    let running = running_subagent_count(app);
    if running > 0 {
        title.push(Span::styled(format!("  {running} running"), Style::default().fg(theme::DIM)));
    }
    frame.render_widget(Paragraph::new(Line::from(title)), title_area);
    if app.session_runtime.session_id.is_none() {
        render_message(
            frame,
            body,
            "No active session",
            "Start or resume a session to see its subagents.",
        );
        return;
    }
    if rows.is_empty() {
        render_message(
            frame,
            body,
            "No subagents yet",
            "Subagents this session starts appear here with their live status.",
        );
        return;
    }
    let items = rows.iter().enumerate().map(|(index, row)| {
        let is_selected = index == selected;
        ListItem::new(row_lines(app, row, is_selected)).style(selection_style(is_selected))
    });
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(List::new(items).highlight_symbol(""), body, &mut state);
}

fn status_badge(status: SubagentStatus) -> Span<'static> {
    let (fg, bg) = match status {
        SubagentStatus::Pending => (Color::Black, Color::Cyan),
        SubagentStatus::Running => (Color::Black, theme::RUST_ORANGE),
        SubagentStatus::Stopping => (Color::Black, theme::STATUS_WARNING),
        SubagentStatus::Completed => (Color::Black, Color::White),
        SubagentStatus::Failed => (Color::White, theme::STATUS_ERROR),
        SubagentStatus::Stopped => (Color::White, Color::DarkGray),
    };
    badge_span(status.label(), fg, bg)
}

fn row_lines(app: &App, row: &SubagentRow<'_>, selected: bool) -> Vec<Line<'static>> {
    let dim = Style::default().fg(theme::DIM);
    let mut details = Vec::new();
    if let Some(kind) = row.subagent_type() {
        details.push(kind.to_owned());
    }
    match app.agents.has_transcript(&row.task.task_id) {
        Some(true) => details.push("transcript".to_owned()),
        Some(false) => details.push("no transcript".to_owned()),
        None => {}
    }
    let mut lines = vec![selection_line(
        Line::from(vec![
            Span::styled(row.task.subject.clone(), title_style()),
            Span::raw("  "),
            status_badge(row.status),
        ]),
        selected,
    )];
    if !details.is_empty() {
        lines.push(Line::styled(format!("  {}", details.join(" · ")), dim));
    }
    if let Some(summary) = row.summary() {
        lines.push(Line::styled(format!("  {summary}"), Style::default().fg(Color::White)));
    }
    lines.push(Line::default());
    lines
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
            Span::styled(TITLE, title_style().fg(theme::RUST_ORANGE)),
            Span::styled(" › ", Style::default().fg(theme::DIM)),
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
                    Style::default().fg(theme::DIM),
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
