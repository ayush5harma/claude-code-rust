// SPDX-License-Identifier: Apache-2.0

use crate::app::App;
use crate::ui::{agent_rows, theme};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

pub(super) fn render(frame: &mut Frame, area: Rect, app: &App) {
    let panes = Layout::default()
        .direction(if area.width >= 72 { Direction::Horizontal } else { Direction::Vertical })
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);
    let agent_rows: Vec<_> = app
        .session_agents
        .agent_ids
        .iter()
        .filter_map(|id| app.sdk_inventory.tasks.iter().find(|task| task.task_id == *id))
        .map(|task| agent_rows::agent_row(app, task))
        .collect();
    let columns = agent_rows::AgentColumns::for_rows(&agent_rows);
    let mut rows =
        vec![Line::styled("Session agents", Style::default().fg(app.session_runtime.accent))];
    if app.session_agents.agent_ids.is_empty() {
        rows.push(Line::from(if app.session_agents.loading {
            "Loading agents..."
        } else {
            "No agents in this session."
        }));
    }
    let visible = usize::from(panes[0].height.saturating_sub(2)).max(1);
    let offset = app.session_agents.selected.saturating_sub(visible.saturating_sub(1));
    for (index, id) in app.session_agents.agent_ids.iter().enumerate().skip(offset).take(visible) {
        let mut line = if let Some(row) = agent_rows.iter().find(|row| row.task.task_id == *id) {
            agent_rows::render_agent_row(row, panes[0].width.saturating_sub(2), &columns, true)
        } else {
            Line::from(format!("{id}  Recorded"))
        };
        line.spans.insert(
            0,
            Span::styled(
                if index == app.session_agents.selected { "> " } else { "  " },
                Style::default().fg(app.session_runtime.accent),
            ),
        );
        rows.push(line);
    }
    frame.render_widget(Paragraph::new(rows), panes[0]);
    let mut details = inspector_lines(app, panes[1].width);
    if let Some(error) = &app.session_agents.error {
        details.push(Line::styled(error.clone(), Style::default().fg(Color::Red)));
    } else if app.session_agents.loading {
        details.push(Line::from("Loading..."));
    } else if app.session_agents.messages.is_empty() {
        details.push(Line::from("No saved messages for this agent."));
    }
    for message in &app.session_agents.messages {
        details.push(Line::styled(
            if message.role == "user" { "User" } else { "Assistant" },
            Style::default().fg(theme::DIM),
        ));
        details.extend(message.text.lines().map(|line| Line::from(line.to_owned())));
        details.push(Line::default());
    }
    frame.render_widget(
        Paragraph::new(details)
            .wrap(Wrap { trim: false })
            .scroll((app.session_agents.message_scroll, 0)),
        panes[1],
    );
}

fn inspector_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let task = app
        .session_agents
        .selected_id()
        .and_then(|id| app.sdk_inventory.tasks.iter().find(|task| task.task_id == id));
    if let Some(task) = task {
        let row = agent_rows::agent_row(app, task);
        let (glyph, color) = agent_rows::status_glyph(&row);
        let state = match agent_rows::task_status(task) {
            "running" => "Running",
            "done" => "Completed",
            "failed" => "Failed",
            "stopped" => "Stopped",
            _ => "Pending",
        };
        lines.push(Line::from(vec![
            Span::styled(task.subject.clone(), Style::default().fg(app.session_runtime.accent)),
            Span::raw("  "),
            Span::styled(
                format!("{glyph} {state}  {}", agent_rows::elapsed_text(row.elapsed_ms)),
                Style::default().fg(color),
            ),
        ]));
        let mut identity = vec![row.name.to_owned()];
        if !row.model.is_empty() {
            identity.push(row.model.to_owned());
        }
        if let Some(alias) = row.requested_model {
            identity.push(format!("asked for {alias}"));
        } else if row.definition_model.is_some() {
            identity.push("agent default".to_owned());
        }
        if row
            .raw_input
            .and_then(|input| input.get("isolation"))
            .and_then(serde_json::Value::as_str)
            == Some("worktree")
        {
            identity.push("isolated worktree".to_owned());
        }
        lines.push(Line::from(identity.join(" · ")));
        let mut metrics = Vec::new();
        if let Some(tools) = row.tools {
            metrics.push(format!("{tools} tools"));
        }
        if let Some(tokens) = row.tokens {
            metrics.push(format!("{} tokens", agent_rows::token_text(tokens)));
        }
        if !metrics.is_empty() {
            lines.push(Line::from(metrics.join(" · ")));
        }
        if !row.now.is_empty() {
            lines.push(Line::from(format!("Now     {}", row.now)));
        }
        if let Some(parent) = row.parent_id {
            for (index, tool) in app.recent_child_tools(parent).enumerate() {
                lines.push(Line::from(format!(
                    "{}{}",
                    if index == 0 { "Recent  " } else { "        " },
                    agent_rows::tool_activity(tool)
                )));
            }
        }
        if let Some(prompt) =
            row.raw_input.and_then(|input| input.get("prompt")).and_then(serde_json::Value::as_str)
        {
            let available = usize::from(width.saturating_sub(8)).max(1);
            let wrapped = crate::ui::wrap::wrap_plain(prompt, available);
            for (index, line) in wrapped.iter().take(6).enumerate() {
                lines.push(Line::from(format!(
                    "{}{}",
                    if index == 0 { "Prompt  " } else { "        " },
                    line
                )));
            }
            if wrapped.len() > 6 {
                lines.push(Line::from("        ..."));
            }
        }
        lines.push(Line::default());
    }
    lines.push(Line::styled("Messages", Style::default().fg(app.session_runtime.accent)));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model::{TaskItem, TaskStatus, ToolCallStatus};
    use crate::app::{ChatMessage, MessageBlock, MessageRole, ToolCallScope};
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn inspector_associates_model_choice_recent_prompt_and_metrics_with_selected_agent() {
        let mut app = App::test_default();
        let mut parent =
            crate::app::test_support::tool_call_info("parent", ToolCallStatus::InProgress);
        parent.sdk_tool_name = "Agent".to_owned();
        parent.raw_input = Some(
            serde_json::json!({"name": "builder", "model": "opus", "isolation": "worktree", "prompt": "Build the selected inspector."}),
        );
        let mut child =
            crate::app::test_support::tool_call_info("child", ToolCallStatus::Completed);
        child.raw_input = Some(serde_json::json!({"file_path": "/src/agents.rs"}));
        app.transcript.messages.push(ChatMessage::new(
            MessageRole::Assistant,
            vec![MessageBlock::ToolCall(Box::new(parent)), MessageBlock::ToolCall(Box::new(child))],
            None,
        ));
        app.index_tool_call("parent".to_owned(), 0, 0);
        app.index_tool_call("child".to_owned(), 0, 1);
        app.register_tool_call_scope(
            "child".to_owned(),
            ToolCallScope::SubagentChild { parent_tool_use_id: "parent".to_owned() },
        );
        let mut task = TaskItem::new("selected", "Build inspector", TaskStatus::InProgress);
        task.source_tool_call_id = Some("parent".to_owned());
        task.metadata = Some(
            serde_json::json!({"task_type": "local_agent", "model_display": "Opus 5.5", "usage": {"tool_uses": 12, "total_tokens": 34200, "duration_ms": 62000}}),
        );
        app.sdk_inventory.tasks.push(task);
        app.session_agents.agent_ids = vec!["selected".to_owned()];
        let mut terminal = Terminal::new(TestBackend::new(140, 30)).expect("terminal");
        terminal.draw(|frame| render(frame, frame.area(), &app)).expect("draw inspector");
        let buffer = terminal.backend().buffer();
        let right: Vec<String> =
            (0..15).map(|y| (53..140).map(|x| buffer[(x, y)].symbol()).collect()).collect();
        assert!(right[0].contains("Build inspector"));
        assert!(right[0].contains("Running"));
        assert!(right[1].contains("builder · Opus 5.5 · asked for opus · isolated worktree"));
        assert!(right[2].contains("12 tools · 34.2k tokens"));
        assert!(right[3].contains("Now     Read agents.rs"));
        assert!(right[4].contains("Recent  Read agents.rs"));
        assert!(right[5].contains("Prompt  Build the selected inspector."));
        let left: String = (0..53).map(|x| buffer[(x, 1)].symbol()).collect();
        assert!(left.contains("> ● builder"));
        assert!(left.contains("Opus 5.5"));
        assert!(left.contains("1m02s"));
    }
}
