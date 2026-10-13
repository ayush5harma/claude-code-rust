// SPDX-License-Identifier: Apache-2.0
use crate::app::App;
use crate::app::TrustSelection;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::theme;

const QUESTION: &str = "Trust this project directory?";
const INTRO: &str = "Trusting this directory lets the session read, edit and run files in it.";
const DEFAULT_MESSAGE: &str = "Yes continues; No closes claude-rs.";

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let outer = Block::default()
        .borders(Borders::ALL)
        .title("Trust project")
        .border_style(Style::default().fg(theme::DIM));
    frame.render_widget(outer, area);

    let inner = area.inner(Margin { vertical: 1, horizontal: 2 });
    let question = Line::from(Span::styled(
        QUESTION,
        Style::default().fg(theme::RUST_ORANGE).add_modifier(Modifier::BOLD),
    ));
    // The trust check and the saved decision both use `cwd_raw`, so this is
    // the directory the answer applies to. It wraps rather than truncates:
    // the tail of a long path is the part that tells projects apart.
    let directory = Line::from(Span::styled(
        app.cwd_raw.clone(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    let body_intro = vec![Line::default(), Line::from(INTRO)];
    let message = app.trust.last_error.clone().unwrap_or_else(|| DEFAULT_MESSAGE.to_owned());
    let question_height = wrapped_line_count(Text::from(question.clone()), inner.width);
    let directory_height = wrapped_line_count(Text::from(directory.clone()), inner.width);
    let body_height = wrapped_line_count(Text::from(body_intro.clone()), inner.width);
    let message_height = wrapped_line_count(
        Text::from(vec![Line::from(Span::styled(message.clone(), Style::default()))]),
        inner.width,
    );
    let actions_height = wrapped_line_count(Text::from(action_lines(app)), inner.width);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(question_height),
            Constraint::Length(directory_height),
            Constraint::Length(body_height),
            Constraint::Length(message_height),
            Constraint::Min(3),
        ])
        .split(inner);

    frame.render_widget(Paragraph::new(question).wrap(Wrap { trim: false }), chunks[0]);
    frame.render_widget(Paragraph::new(directory).wrap(Wrap { trim: false }), chunks[1]);
    frame.render_widget(Paragraph::new(body_intro).wrap(Wrap { trim: false }), chunks[2]);

    let message_style = if app.trust.last_error.is_some() {
        Style::default().fg(theme::STATUS_ERROR)
    } else {
        Style::default().fg(theme::DIM)
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(message, message_style))).wrap(Wrap { trim: false }),
        chunks[3],
    );

    let action_constraints = [Constraint::Length(actions_height), Constraint::Min(0)];
    let action_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(action_constraints)
        .split(chunks[4]);
    frame.render_widget(
        Paragraph::new(action_lines(app)).wrap(Wrap { trim: false }),
        action_chunks[0],
    );
}

fn wrapped_line_count(text: Text<'static>, width: u16) -> u16 {
    u16::try_from(Paragraph::new(text).wrap(Wrap { trim: false }).line_count(width))
        .unwrap_or(u16::MAX)
        .max(1)
}

fn action_lines(app: &App) -> Vec<Line<'static>> {
    vec![
        action_line("Yes", app.trust.selection == TrustSelection::Yes),
        action_line("No", app.trust.selection == TrustSelection::No),
    ]
}

fn action_line(label: &str, selected: bool) -> Line<'static> {
    let marker = if selected { ">" } else { " " };
    let style = if selected {
        Style::default().fg(ratatui::style::Color::White).bg(theme::RUST_ORANGE)
    } else {
        Style::default().fg(theme::DIM)
    }
    .add_modifier(Modifier::BOLD);

    Line::from(Span::styled(format!("{marker} {label}"), style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn draw_rows(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| render(frame, app)).expect("draw");
        terminal
            .backend()
            .buffer()
            .content
            .chunks(usize::from(width))
            .map(|row| row.iter().map(ratatui::buffer::Cell::symbol).collect::<String>())
            .collect()
    }

    fn draw_text(app: &mut App) -> String {
        draw_rows(app, 70, 14).join("\n")
    }

    #[test]
    fn trusted_view_shows_the_directory_without_storage_details() {
        let mut app = App::test_default();
        app.trust.selection = TrustSelection::Yes;
        app.cwd_raw = "/work/project".to_owned();

        let rows = draw_rows(&mut app, 70, 14);
        let text = rows.join("\n");

        assert!(text.contains("Trust project"));
        assert!(!text.contains("Unknown Project"));
        assert!(!text.contains("~/.claude.json"));
        assert!(!text.contains("hasTrustDialogAccepted"));
        let question = rows.iter().position(|row| row.contains("Trust this project directory?"));
        let directory = rows.iter().position(|row| row.contains("/work/project"));
        assert_eq!(directory, question.map(|row| row + 1), "{text}");
    }

    #[test]
    fn trusted_view_wraps_a_long_directory_without_losing_segments() {
        let mut app = App::test_default();
        let segments = [
            "Users",
            "someone",
            "Github",
            "a-rather-long-organisation-name",
            "client-projects",
            "deeply-nested-service-checkout",
        ];
        app.cwd_raw = format!("/{}", segments.join("/"));

        let rows = draw_rows(&mut app, 60, 16);

        // Strip the frame and margin, then rejoin the wrapped rows: a path has
        // no spaces, so a wrap point is a plain character break.
        let body: String = rows
            .iter()
            .map(|row| row.trim_matches(|c: char| c == '|' || c == '\u{2502}' || c == ' '))
            .collect();
        assert!(body.contains(&app.cwd_raw), "{body}");
        let text = rows.join("\n");
        assert!(rows.iter().filter(|row| row.contains("Trust this")).count() == 1, "{text}");
    }

    #[test]
    fn trusted_view_keeps_the_error_in_the_message_slot() {
        let mut app = App::test_default();
        app.trust.last_error = Some("Invalid trust preferences: /x/.claude.json".to_owned());

        let text = draw_text(&mut app);

        assert!(text.contains("Invalid trust preferences: /x/.claude.json"));
        assert!(!text.contains("Yes continues"));
    }

    #[test]
    fn trusted_view_highlights_no_when_selected() {
        let mut app = App::test_default();
        app.trust.selection = TrustSelection::No;

        let text = draw_text(&mut app);

        assert!(text.contains("  Yes"));
        assert!(text.contains("> No"));
    }

    #[test]
    fn trusted_view_renders_actions_below_body_text() {
        let mut app = App::test_default();
        app.trust.selection = TrustSelection::Yes;

        let text = draw_text(&mut app);
        let body_idx = text.find("Trusting this directory").expect("body text");
        let action_idx = text.find("> Yes").expect("yes action");

        assert!(action_idx > body_idx);
    }

    #[test]
    fn trusted_view_wraps_title_on_narrow_widths() {
        let mut app = App::test_default();
        let rows = draw_rows(&mut app, 18, 16);

        assert!(rows.iter().any(|row| row.contains("Trust this")));
        assert!(rows.iter().any(|row| row.contains("directory?")));
    }
}
