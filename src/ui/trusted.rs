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

    // Rows are handed out in priority order: the choices, the question, then
    // the directory (its tail is what tells projects apart), then the message,
    // and the explanatory text last. Paragraph wrapping cannot show the tail of
    // a path that is too long for the rows left, so the path is cut here.
    let action_rows = actions_height.min(inner.height);
    let question_rows = question_height.min(inner.height - action_rows);
    let mut free = inner.height - action_rows - question_rows;
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

    // Double-width characters can leave a column unused at the end of a row, so
    // the first budget may need more rows than it has columns for; shrink it
    // until the tail, ellipsis included, fits in `max_rows`.
    let mut budget = max_rows * width - 1;
    loop {
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
        let rows = chunk_columns(tail.into_iter(), width);
        if rows.len() <= max_rows || budget == 0 {
            return rows;
        }
        budget -= 1;
    }
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

    /// The screen rows as text. The cell after a double-width character is
    /// skipped, so a row reads as the characters that were drawn.
    fn draw_rows(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| render(frame, app)).expect("draw");
        terminal
            .backend()
            .buffer()
            .content
            .chunks(usize::from(width))
            .map(|row| {
                let mut text = String::new();
                let mut skip = 0usize;
                for cell in row {
                    if skip > 0 {
                        skip -= 1;
                        continue;
                    }
                    text.push_str(cell.symbol());
                    skip = unicode_width::UnicodeWidthStr::width(cell.symbol()).saturating_sub(1);
                }
                text
            })
            .collect()
    }

    fn draw_text(app: &mut App) -> String {
        draw_rows(app, 70, 14).join("\n")
    }

    /// A row without the frame and its padding (the margin is two columns, one of
    /// them the frame's own).
    fn content(row: &str) -> String {
        let inside = row.trim_start_matches('\u{2502}').trim_end_matches('\u{2502}');
        inside.strip_prefix(' ').unwrap_or(inside).trim_end().to_owned()
    }

    /// The rows the screen gives the directory: the ones under the question
    /// up to the next element of the prompt.
    fn directory_on_screen(rows: &[String]) -> Vec<String> {
        let question = rows.iter().position(|row| row.contains(QUESTION)).expect("question row");
        rows[question + 1..]
            .iter()
            .map(|row| content(row))
            .take_while(|row| {
                !(row.is_empty()
                    || row.starts_with("Claude Rust will wait")
                    || row.starts_with("Choose Yes")
                    || row.starts_with("Invalid")
                    || row.starts_with("> Yes"))
            })
            .collect()
    }

    const LONG_PATH: &str = "/Users/someone/Github/a-rather-long-organisation-name/client-projects/deeply-nested-service-checkout";
    const CJK_PATH: &str =
        "/Users/田中/Github/プロジェクト一覧/顧客向け-サービス/チェックアウト画面v2";

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
        assert_eq!(directory_on_screen(&rows), ["/work/project"], "{text}");
    }

    #[test]
    fn trusted_view_wraps_a_long_directory_without_losing_characters() {
        // Width 40 puts no row break on a space, which a row's padding would hide.
        for key in [LONG_PATH, "C:/demo/Client Projects/some service/checkout"] {
            let mut app = App::test_default();
            app.trust.project_key = key.to_owned();

            let rows = draw_rows(&mut app, 40, 20);

            assert_eq!(directory_on_screen(&rows).concat(), key, "{rows:#?}");
            assert!(rows.iter().any(|row| row.contains("> Yes")), "{rows:#?}");
        }
    }

    #[test]
    fn trusted_view_keeps_the_end_of_a_path_behind_a_leading_ellipsis() {
        let mut app = App::test_default();
        app.trust.project_key = LONG_PATH.to_owned();

        let rows = draw_rows(&mut app, 36, 8);

        let directory = directory_on_screen(&rows);
        assert!(directory.len() > 1, "{rows:#?}");
        assert!(directory[0].starts_with('\u{2026}'), "{rows:#?}");
        let shown = directory.concat();
        assert!(LONG_PATH.ends_with(shown.trim_start_matches('\u{2026}')), "{rows:#?}");
        assert!(shown.ends_with("service-checkout"), "{rows:#?}");
        assert!(rows.join("\n").contains("> Yes"), "{rows:#?}");
    }

    #[test]
    fn trusted_view_keeps_the_end_of_a_wide_character_path_at_odd_widths() {
        // A double-width character that does not fit leaves a column unused at
        // the end of a row, so a budget counted in columns can need a third row.
        for inner_width in [31u16, 33, 35, 37] {
            let mut app = App::test_default();
            app.trust.project_key = CJK_PATH.to_owned();

            let rows = draw_rows(&mut app, inner_width + 4, 7);

            let directory = directory_on_screen(&rows);
            assert!(directory.len() <= 2, "{inner_width}: {rows:#?}");
            let shown = directory.concat();
            assert!(shown.ends_with("画面v2"), "{inner_width}: {rows:#?}");
            assert!(CJK_PATH.ends_with(shown.trim_start_matches('\u{2026}')), "{rows:#?}");
            if shown != CJK_PATH {
                assert!(directory[0].starts_with('\u{2026}'), "{inner_width}: {rows:#?}");
            }
            assert!(rows.join("\n").contains("> Yes"), "{inner_width}: {rows:#?}");
        }
    }

    #[test]
    fn trusted_view_keeps_the_choices_on_very_small_screens() {
        for (width, height) in [(30u16, 12u16), (20, 14), (12, 24), (20, 5), (20, 4)] {
            let mut app = App::test_default();
            app.trust.project_key = LONG_PATH.to_owned();

            let text = draw_rows(&mut app, width, height).join("\n");

            assert!(text.contains("> Yes"), "{width}x{height}: {text}");
            assert!(text.contains("  No"), "{width}x{height}: {text}");
        }
    }

    #[test]
    fn trusted_view_does_not_mark_a_path_that_fits() {
        let mut app = App::test_default();
        app.trust.project_key = "/work/project".to_owned();

        let rows = draw_rows(&mut app, 70, 14);

        assert!(!directory_on_screen(&rows)[0].contains('\u{2026}'));
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
