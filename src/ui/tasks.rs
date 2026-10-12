// SPDX-License-Identifier: Apache-2.0

use crate::app::App;
use ratatui::{
    Frame,
    layout::Margin,
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub(crate) fn render(frame: &mut Frame, app: &App) {
    let area = frame.area().inner(Margin { vertical: 1, horizontal: 1 });
    let inner = area.inner(Margin { vertical: 1, horizontal: 2 });
    frame.render_widget(
        Block::default().title("Tasks · Ctrl+T or Esc to close").borders(Borders::ALL),
        area,
    );
    if app.sdk_inventory.tasks.is_empty() {
        frame.render_widget(Paragraph::new("No tasks in the current session."), inner);
        return;
    }
    let text = app
        .sdk_inventory
        .tasks
        .iter()
        .map(|task| {
            let state = match task.status {
                crate::agent::model::TaskStatus::Pending => "Pending",
                crate::agent::model::TaskStatus::InProgress => "In progress",
                crate::agent::model::TaskStatus::Completed => "Completed",
            };
            format!(
                "#{}  {}  [{}]",
                task.task_id,
                task.subject.lines().next().unwrap_or_default(),
                state
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
}
