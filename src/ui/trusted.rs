// SPDX-License-Identifier: Apache-2.0
use crate::app::App;
use crate::app::TrustSelection;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::UnicodeWidthChar;

use super::theme;

const QUESTION: &str = "Trust this project directory?";
const ELLIPSIS: char = '\u{2026}';

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
    let body_intro = vec![
        Line::from("Claude Rust will wait here until you choose whether to trust this workspace."),
        Line::default(),
        Line::from("Review the project before continuing if you are unsure."),
    ];
    let message = app
        .trust
        .last_error
        .clone()
        .unwrap_or_else(|| "Choose Yes to continue or No to close Claude Rust.".to_owned());
    let message_style = if app.trust.last_error.is_some() {
        Style::default().fg(theme::STATUS_ERROR)
    } else {
        Style::default().fg(theme::DIM)
    };
    let message_lines = vec![Line::from(Span::styled(message, message_style))];

    let question_height = wrapped_line_count(Text::from(question.clone()), inner.width);
    let intro_height = wrapped_line_count(Text::from(body_intro.clone()), inner.width);
    let message_height = wrapped_line_count(Text::from(message_lines.clone()), inner.width);
    let actions_height = wrapped_line_count(Text::from(action_lines(app)), inner.width);

    // Rows are handed out in priority order: the question and the choices, then
    // the directory (its tail is what tells projects apart), then the message,
    // and the explanatory text last. Paragraph wrapping cannot show the tail of
    // a path that is too long for the rows left, so the path is cut here.
    let question_rows = question_height.min(inner.height);
    let action_rows = actions_height.min(inner.height - question_rows);
    let mut free = inner.height - question_rows - action_rows;
    let directory = directory_rows(&app.trust.project_key, inner.width, usize::from(free));
    let directory_rows_used = u16::try_from(directory.len()).unwrap_or(u16::MAX);
    free -= directory_rows_used;
    let message_rows = message_height.min(free);
    free -= message_rows;
    let intro_rows = if free >= intro_height { intro_height } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(question_rows),
            Constraint::Length(directory_rows_used),
            Constraint::Length(intro_rows),
            Constraint::Length(message_rows),
            Constraint::Length(action_rows),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(Paragraph::new(question).wrap(Wrap { trim: false }), chunks[0]);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(
            directory
                .into_iter()
                .map(|row| Line::from(Span::styled(row, bold)))
                .collect::<Vec<_>>(),
        ),
        chunks[1],
    );
    frame.render_widget(Paragraph::new(body_intro).wrap(Wrap { trim: false }), chunks[2]);
    frame.render_widget(Paragraph::new(message_lines).wrap(Wrap { trim: false }), chunks[3]);
    frame.render_widget(Paragraph::new(action_lines(app)).wrap(Wrap { trim: false }), chunks[4]);
}

/// Cuts `path` into rows of at most `width` columns. When it needs more than
/// `max_rows`, keeps the end of the path behind a leading ellipsis.
fn directory_rows(path: &str, width: u16, max_rows: usize) -> Vec<String> {
    let width = usize::from(width);
    if path.is_empty() || width == 0 || max_rows == 0 {
        return Vec::new();
    }
    let rows = chunk_columns(path.chars(), width);
    if rows.len() <= max_rows {
        return rows;
    }

    let budget = max_rows * width - 1;
    let mut tail: Vec<char> = Vec::new();
    let mut used = 0usize;
    for ch in path.chars().rev() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > budget {
            break;
        }
        tail.push(ch);
        used += ch_width;
    }
    tail.push(ELLIPSIS);
    tail.reverse();
    let mut rows = chunk_columns(tail.into_iter(), width);
    rows.truncate(max_rows);
    rows
}

fn chunk_columns(chars: impl Iterator<Item = char>, width: usize) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;
    for ch in chars {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > width && !current.is_empty() {
            rows.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(ch);
        used += ch_width;
    }
    if !current.is_empty() {
        rows.push(current);
    }
    rows
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

    /// Everything the screen shows inside the frame, with the frame, padding
    /// and spaces removed, so a path cut across rows reads as one string.
    fn compact(rows: &[String]) -> String {
        rows.iter()
            .flat_map(|row| row.chars())
            .filter(|c| !c.is_whitespace() && *c != '\u{2502}')
            .collect()
    }

    fn compact_path(path: &str) -> String {
        path.chars().filter(|c| !c.is_whitespace()).collect()
    }

    const LONG_PATH: &str = "/Users/someone/Github/a-rather-long-organisation-name/client-projects/deeply-nested-service-checkout";

    #[test]
    fn trusted_view_names_the_directory_right_under_the_question() {
        let mut app = App::test_default();
        app.trust.selection = TrustSelection::Yes;
        app.trust.project_key = "/work/project".to_owned();

        let rows = draw_rows(&mut app, 70, 14);
        let text = rows.join("\n");

        assert!(!text.contains("Unknown Project"));
        assert!(!text.contains("~/.claude.json"));
        assert!(!text.contains("hasTrustDialogAccepted"));
        let question = rows.iter().position(|row| row.contains(QUESTION)).expect("question row");
        assert!(rows[question + 1].contains("/work/project"), "{text}");
    }

    #[test]
    fn trusted_view_wraps_a_long_directory_without_losing_characters() {
        for key in [LONG_PATH, "C:/demo/Client Projects/some service/checkout"] {
            let mut app = App::test_default();
            app.trust.project_key = key.to_owned();

            let rows = draw_rows(&mut app, 40, 20);

            assert!(compact(&rows).contains(&compact_path(key)), "{key}: {rows:#?}");
            assert!(rows.iter().any(|row| row.contains("> Yes")), "{rows:#?}");
        }
    }

    #[test]
    fn trusted_view_keeps_the_end_of_a_path_and_the_choices_on_short_screens() {
        for (width, height) in [(30u16, 12u16), (20, 14), (12, 24)] {
            let mut app = App::test_default();
            app.trust.project_key = LONG_PATH.to_owned();

            let rows = draw_rows(&mut app, width, height);

            let body = compact(&rows);
            assert!(body.contains(&LONG_PATH[LONG_PATH.len() - 12..]), "{width}x{height}: {body}");
            assert!(
                body.contains('\u{2026}') || body.contains(&compact_path(LONG_PATH)),
                "{width}x{height}: {body}"
            );
            let text = rows.join("\n");
            assert!(text.contains("> Yes"), "{width}x{height}: {text}");
            assert!(text.contains("  No"), "{width}x{height}: {text}");
        }
    }

    #[test]
    fn trusted_view_does_not_mark_a_path_that_fits() {
        let mut app = App::test_default();
        app.trust.project_key = "/work/project".to_owned();

        let rows = draw_rows(&mut app, 70, 14);

        assert!(!rows.join("\n").contains('\u{2026}'));
    }

    #[test]
    fn trusted_view_keeps_the_error_and_the_choices_beside_the_directory() {
        let mut app = App::test_default();
        app.trust.project_key = "/work/project".to_owned();
        app.trust.last_error = Some("Invalid trust preferences: /x/.claude.json".to_owned());

        let text = draw_text(&mut app);

        assert!(text.contains("/work/project"), "{text}");
        assert!(text.contains("Invalid trust preferences: /x/.claude.json"), "{text}");
        assert!(text.contains("> Yes"), "{text}");
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
        let body_idx = text.find("Claude Rust will wait here").expect("body text");
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
