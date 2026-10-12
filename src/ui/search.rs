// SPDX-License-Identifier: Apache-2.0

use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub(crate) fn render(frame: &mut Frame, app: &mut App) {
    crate::app::search::refresh(app);
    let Some(search) = app.search.as_ref() else { return };
    let area = frame.area().inner(Margin { vertical: 1, horizontal: 1 });
    let parts = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(area);
    frame.render_widget(Paragraph::new("Transcript search\nSearches retained session content only; evicted history and archives are excluded."), parts[0]);
    frame.render_widget(
        Paragraph::new(search.query.as_str())
            .block(Block::default().borders(Borders::ALL).title("Query")),
        parts[1],
    );
    let body = if search.query.is_empty() {
        Line::from("Type a query to find retained user, assistant, and visible tool text.")
    } else if search.matches.is_empty() {
        Line::from("No matches in retained transcript.")
    } else {
        let matched = search.matches[search.selected];
        let row = &search.rows[matched.row];
        let before = &row.text[..matched.start];
        let prefix_start = before.char_indices().rev().nth(80).map_or(0, |(index, _)| index);
        let prefix = &before[prefix_start..];
        let hit = &row.text[matched.start..matched.end];
        let after = &row.text[matched.end..];
        let suffix_end = after.char_indices().nth(80).map_or(after.len(), |(index, _)| index);
        let suffix = &after[..suffix_end];
        Line::from(vec![
            Span::raw(format!("{}: ", row.role)),
            Span::raw(if prefix_start > 0 { "…" } else { "" }),
            Span::raw(prefix),
            Span::styled(
                hit,
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(suffix),
            Span::raw(if suffix_end < after.len() { "…" } else { "" }),
        ])
    };
    frame.render_widget(
        Paragraph::new(body)
            .block(Block::default().borders(Borders::ALL).title(format!(
                "Match {} of {}",
                if search.matches.is_empty() { 0 } else { search.selected + 1 },
                search.matches.len()
            )))
            .wrap(Wrap { trim: false }),
        parts[2],
    );
    frame.render_widget(
        Paragraph::new("Enter/Down next · Shift+Enter/Up previous · Esc close"),
        parts[3],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ChatMessage, MessageBlock, MessageRole, TextBlock};
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn selected_result_shows_role_and_highlights_only_the_match() {
        let mut app = App::test_default();
        app.push_message_tracked(ChatMessage::new(
            MessageRole::Assistant,
            vec![MessageBlock::Text(TextBlock::from_complete("before delta after"))],
            None,
        ));
        crate::app::search::open(&mut app, "delta");
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).expect("terminal");
        terminal.draw(|frame| render(frame, &mut app)).expect("search render");
        let buffer = terminal.backend().buffer();
        let row = buffer
            .content
            .chunks(60)
            .find(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    .contains("Claude: before delta after")
            })
            .expect("selected result row");
        let text: String = row.iter().map(ratatui::buffer::Cell::symbol).collect();
        let start = text[..text.find("delta").expect("match")].chars().count();
        assert!(row[start..start + 5].iter().all(|cell| cell.bg == Color::Yellow));
        assert_ne!(row[start - 1].bg, Color::Yellow);
    }

    #[test]
    fn expanded_case_match_highlights_the_original_unicode_text() {
        let mut app = App::test_default();
        app.push_message_tracked(ChatMessage::new(
            MessageRole::User,
            vec![MessageBlock::Text(TextBlock::from_complete("before Straße after"))],
            None,
        ));
        crate::app::search::open(&mut app, "STRASSE");
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).expect("terminal");
        terminal.draw(|frame| render(frame, &mut app)).expect("search render");
        let buffer = terminal.backend().buffer();
        let row = buffer
            .content
            .chunks(60)
            .find(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    .contains("User: before Straße after")
            })
            .expect("selected result row");
        let text: String = row.iter().map(ratatui::buffer::Cell::symbol).collect();
        let start = text[..text.find("Straße").expect("match")].chars().count();
        assert!(row[start..start + 6].iter().all(|cell| cell.bg == Color::Yellow));
        assert_ne!(row[start - 1].bg, Color::Yellow);
        assert_ne!(row[start + 6].bg, Color::Yellow);
    }
}
