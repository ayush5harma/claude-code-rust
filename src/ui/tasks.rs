// SPDX-License-Identifier: Apache-2.0

use crate::agent::model::TaskStatus;
use crate::app::App;
use ratatui::{
    Frame,
    layout::Margin,
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

pub(crate) fn render(frame: &mut Frame, app: &App) {
    let area = frame.area().inner(Margin { vertical: 1, horizontal: 1 });
    let inner = area.inner(Margin { vertical: 1, horizontal: 2 });
    frame.render_widget(
        Block::default()
            .title("Tasks · Up/Down select · Enter inspect · Ctrl+T or Esc close")
            .borders(Borders::ALL),
        area,
    );
    let mut rows = Vec::new();
    let mut selected_row = None;
    let plan: Vec<_> = match app.footer.plan_source {
        crate::app::PlanSource::Todo => {
            app.footer.todo_plan.iter().map(|item| (item.status, item.content.as_str())).collect()
        }
        _ => app
            .sdk_inventory
            .tasks
            .iter()
            .filter(|task| crate::app::tasks::is_user_plan_task(task))
            .map(|task| (task.status, task.subject.as_str()))
            .collect(),
    };
    if !plan.is_empty() {
        let done = plan.iter().filter(|(status, _)| *status == TaskStatus::Completed).count();
        rows.push(Line::from(format!("Plan  {done}/{}", plan.len())));
        for (status, title) in plan {
            let glyph = match status {
                TaskStatus::Completed => "✓",
                TaskStatus::InProgress => "●",
                _ => "○",
            };
            rows.push(Line::from(format!("{glyph}  {title}")));
        }
        rows.push(Line::default());
    }
    let agents = super::agent_rows::session_agent_rows(app);
    if !agents.is_empty() {
        rows.push(Line::from("Agents"));
        let selected = app
            .session_agents
            .overlay_selected_id
            .as_deref()
            .filter(|id| agents.iter().any(|task| task.task.task_id == *id))
            .unwrap_or(&agents[0].task.task_id);
        let columns = super::agent_rows::AgentColumns::for_rows(&agents);
        for row in &agents {
            if row.task.task_id == selected {
                selected_row = Some(rows.len());
            }
            let mut line = super::agent_rows::render_agent_row(
                row,
                inner.width.saturating_sub(2),
                &columns,
                false,
            );
            line.spans.insert(
                0,
                ratatui::text::Span::raw(if row.task.task_id == selected { "> " } else { "  " }),
            );
            rows.push(line);
        }
        rows.push(Line::default());
    }
    let other: Vec<_> = app
        .sdk_inventory
        .tasks
        .iter()
        .filter(|task| {
            !crate::app::tasks::is_user_plan_task(task)
                && !crate::app::session_agents::is_session_agent_task(task)
        })
        .collect();
    if !other.is_empty() {
        rows.push(Line::from("Other tasks"));
        for task in other {
            rows.push(Line::from(format!("{}  {}", task.task_id, task.subject)));
        }
    }
    if rows.is_empty() {
        rows.push(Line::from("No tasks in the current session."));
    }
    let scroll =
        selected_row.unwrap_or(0).saturating_sub(usize::from(inner.height.saturating_sub(1)));
    frame.render_widget(
        Paragraph::new(rows).scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0)),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model::TaskItem;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn overlay_groups_plan_agents_and_shell_without_counting_agents_as_plan_steps() {
        let mut app = App::test_default();
        app.sdk_inventory.tasks.push(TaskItem::new("plan", "Write tests", TaskStatus::Completed));
        let mut agent = TaskItem::new("agent", "Inspect integration", TaskStatus::InProgress);
        agent.metadata = Some(
            serde_json::json!({"task_type": "local_agent", "subagent_type": "reviewer", "model_display": "Sonnet 5.5"}),
        );
        app.sdk_inventory.tasks.push(agent);
        let mut shell = TaskItem::new("shell", "cargo test", TaskStatus::InProgress);
        shell.metadata = Some(serde_json::json!({"task_type": "local_bash"}));
        app.sdk_inventory.tasks.push(shell);
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).expect("terminal");
        terminal.draw(|frame| render(frame, &app)).expect("draw overlay");
        let buffer = terminal.backend().buffer();
        let row = |y| (3..117).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert_eq!(row(2).trim(), "Plan  1/1");
        assert_eq!(row(3).trim(), "✓  Write tests");
        assert_eq!(row(5).trim(), "Agents");
        assert!(row(6).contains("> ● reviewer"));
        assert!(row(6).contains("Sonnet 5.5"));
        assert!(row(6).contains("Inspect integration"));
        assert_eq!(row(8).trim(), "Other tasks");
        assert_eq!(row(9).trim(), "shell  cargo test");
    }
}
