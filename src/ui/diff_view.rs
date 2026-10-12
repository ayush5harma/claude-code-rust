// SPDX-License-Identifier: Apache-2.0

use crate::app::App;
use crate::ui::{diff, theme};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

pub(crate) fn render(frame: &mut Frame, app: &mut App) {
    let Some(state) = &mut app.diff_view else { return };
    let area = frame.area().inner(Margin { vertical: 1, horizontal: 1 });
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1), Constraint::Length(2)])
        .split(area);
    frame.render_widget(Paragraph::new("Diff  ·  current session and working tree"), rows[0]);
    frame.render_widget(
        Paragraph::new("↑/↓ files  ·  PgUp/PgDn diff  ·  r refresh  ·  Esc close"),
        rows[2],
    );
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(rows[1]);
    let items = state.entries.iter().map(|entry| {
        let source = if entry.status.is_some() && entry.session_diff.is_some() {
            "both"
        } else if entry.status.is_some() {
            "git"
        } else {
            "session"
        };
        ListItem::new(format!("{}  [{source}]", entry.path))
    });
    let mut selection =
        ListState::default().with_selected((!state.entries.is_empty()).then_some(state.selected));
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Files"))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(theme::RUST_ORANGE)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› "),
        columns[0],
        &mut selection,
    );
    let detail = columns[1].inner(Margin { vertical: 1, horizontal: 1 });
    let height = usize::from(detail.height);
    let selected = state.entries.get(state.selected);
    let lines: Vec<Line<'static>> = if let Some(entry) = selected {
        if let Some(raw) = &entry.working_diff {
            diff::render_raw_unified_diff_window(raw, state.scroll, height)
        } else if let Some(session) = &entry.session_diff {
            let refresh = state
                .rendered_session_diff
                .as_ref()
                .is_none_or(|(index, width, _)| *index != state.selected || *width != detail.width);
            if refresh {
                state.rendered_session_diff =
                    Some((state.selected, detail.width, diff::render_diff(session, detail.width)));
            }
            state.rendered_session_diff.as_ref().map_or_else(Vec::new, |(_, _, lines)| {
                lines.iter().skip(state.scroll).take(height).cloned().collect()
            })
        } else {
            Vec::new()
        }
    } else if state.loading {
        vec![Line::from("Loading Git changes...")]
    } else {
        vec![Line::from("No changes found in the retained session tool diffs or working tree.")]
    };
    let title = selected.map_or("Diff".to_owned(), |entry| {
        format!(
            "{}{}",
            entry.path,
            entry.status.as_ref().map_or(String::new(), |status| format!("  [{status}]"))
        )
    });
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        columns[1],
    );
    if let Some(error) = &state.error {
        frame.render_widget(
            Paragraph::new(error.as_str()).style(Style::default().fg(Color::Yellow)),
            rows[2],
        );
    }
}
