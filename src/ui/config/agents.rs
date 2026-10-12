// SPDX-License-Identifier: Apache-2.0

use crate::agent::model::TaskStatus;
use crate::app::App;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render(frame: &mut Frame, area: Rect, app: &App) {
    let panes = Layout::default()
        .direction(if area.width >= 72 { Direction::Horizontal } else { Direction::Vertical })
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);
    let mut rows = vec![Line::styled("Session agents", Style::default().fg(Color::Cyan))];
    if app.session_runtime.session_id.is_none() {
        rows.push(Line::from("Connect to view agents."));
    } else if app.session_agents.agent_ids.is_empty() {
        rows.push(Line::from(if app.session_agents.loading {
            "Loading agents..."
        } else {
            "No agents in this session."
        }));
    }
    let visible_rows = usize::from(panes[0].height.saturating_sub(2)).max(1);
    let offset = app.session_agents.selected.saturating_sub(visible_rows.saturating_sub(1));
    for (index, id) in
        app.session_agents.agent_ids.iter().enumerate().skip(offset).take(visible_rows)
    {
        let task = app.sdk_inventory.tasks.iter().find(|task| task.task_id == *id);
        let status = match task {
            Some(task)
                if task
                    .metadata
                    .as_ref()
                    .and_then(serde_json::Value::as_object)
                    .and_then(|metadata| metadata.get("terminal_status"))
                    .and_then(serde_json::Value::as_str)
                    == Some("stopped") =>
            {
                "Stopped"
            }
            Some(task)
                if task
                    .metadata
                    .as_ref()
                    .and_then(serde_json::Value::as_object)
                    .and_then(|metadata| metadata.get("terminal_status"))
                    .and_then(serde_json::Value::as_str)
                    == Some("failed") =>
            {
                "Failed"
            }
            Some(task) => match task.status {
                TaskStatus::InProgress => "Running",
                TaskStatus::Completed => "Completed",
                TaskStatus::Pending => "Pending",
            },
            None => "Recorded",
        };
        let label = task.map_or(id.as_str(), |task| task.subject.as_str());
        let prefix = if index == app.session_agents.selected { "> " } else { "  " };
        rows.push(Line::from(vec![
            Span::styled(
                format!("{prefix}{label}"),
                Style::default().fg(if index == app.session_agents.selected {
                    Color::Cyan
                } else {
                    Color::White
                }),
            ),
            Span::raw(format!("  {status}")),
        ]));
    }
    frame.render_widget(Paragraph::new(rows).wrap(Wrap { trim: false }), panes[0]);

    let mut details = vec![Line::styled("Messages", Style::default().fg(Color::Cyan))];
    if let Some(agent_id) = app.session_agents.selected_id() {
        details.push(Line::styled(agent_id.to_owned(), Style::default().fg(Color::Gray)));
    }
    if let Some(error) = &app.session_agents.error {
        details.push(Line::styled(error.clone(), Style::default().fg(Color::Red)));
    } else if app.session_agents.loading {
        details.push(Line::from("Loading..."));
    } else if app.session_agents.messages.is_empty() {
        details.push(Line::from("No saved messages for this agent."));
    }
    for message in &app.session_agents.messages {
        let who = if message.role == "user" { "User" } else { "Assistant" };
        details.push(Line::styled(who, Style::default().fg(Color::Yellow)));
        for line in message.text.lines() {
            details.push(Line::from(line.to_owned()));
        }
        details.push(Line::default());
    }
    frame.render_widget(
        Paragraph::new(details)
            .wrap(Wrap { trim: false })
            .scroll((app.session_agents.message_scroll, 0)),
        panes[1],
    );
}
