// SPDX-License-Identifier: Apache-2.0

use crate::app::App;
use ratatui::{
    Frame,
    layout::Margin,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

pub(crate) fn render(frame: &mut Frame, app: &App) {
    let Some(picker) = app.rewind_picker.as_ref() else { return };
    let area = frame.area().inner(Margin { vertical: 1, horizontal: 1 });
    let inner = area.inner(Margin { vertical: 1, horizontal: 2 });
    let title = if picker.restore_mode.is_some() {
        "Rewind: restore mode"
    } else {
        "Rewind: select message"
    };
    let block = Block::default().title(title).borders(Borders::ALL);
    frame.render_widget(block, area);
    if let Some(mode) = picker.restore_mode {
        let text = format!(
            "Selected message {} of {}\n\n[b] Code and conversation\n[c] Conversation only\n[f] Code only\n\nCurrent: {}\nEnter to rewind · Esc back",
            picker.selected + 1,
            app.sdk_inventory.rewind_targets.len(),
            mode.as_stored()
        );
        frame.render_widget(Paragraph::new(text), inner);
        return;
    }
    if app.sdk_inventory.rewind_targets_in_flight {
        frame.render_widget(Paragraph::new("Loading messages..."), inner);
        return;
    }
    if let Some(error) = &app.sdk_inventory.rewind_targets_error {
        frame.render_widget(Paragraph::new(error.as_str()), inner);
        return;
    }
    let items: Vec<_> = app
        .sdk_inventory
        .rewind_targets
        .iter()
        .map(|target| {
            let preview = target
                .first_text
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .filter(|ch| !ch.is_control())
                .take(90)
                .collect::<String>();
            ListItem::new(format!("{}  {}", target.index, preview))
        })
        .collect();
    if items.is_empty() {
        frame
            .render_widget(Paragraph::new("No messages available to rewind. Esc to close."), inner);
        return;
    }
    let mut state = ListState::default().with_selected(Some(picker.selected));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> "),
        inner,
        &mut state,
    );
}
