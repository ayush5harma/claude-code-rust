// SPDX-License-Identifier: Apache-2.0

use crate::app::App;
use ratatui::{
    Frame,
    layout::Margin,
    widgets::{Block, Borders, Paragraph},
};

pub(crate) fn render(frame: &mut Frame, app: &App) {
    let Some(state) = app.verbose_view.as_ref() else { return };
    let area = frame.area().inner(Margin { vertical: 1, horizontal: 1 });
    let inner = area.inner(Margin { vertical: 1, horizontal: 2 });
    frame.render_widget(
        Block::default()
            .title("Detailed transcript snapshot · Ctrl+O or Esc to close")
            .borders(Borders::ALL),
        area,
    );
    let count = usize::from(inner.height);
    let end = (state.scroll + count).min(state.rows.len());
    let mut text = state.rows[state.scroll.min(end)..end].join("\n");
    if state.truncated && end == state.rows.len() {
        text.push_str("\n[View limited to 5000 retained rows]");
    }
    frame.render_widget(Paragraph::new(text), inner);
}
