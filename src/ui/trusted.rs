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
const INTRO: &str = "Trusting this directory starts a session here and loads its project settings.";
const DEFAULT_MESSAGE: &str = "Yes continues; No closes claude-rs.";
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
    let intro = vec![Line::default(), Line::from(INTRO)];
    let message = app.trust.last_error.clone().unwrap_or_else(|| DEFAULT_MESSAGE.to_owned());
    let message_style = if app.trust.last_error.is_some() {
        Style::default().fg(theme::STATUS_ERROR)
    } else {
        Style::default().fg(theme::DIM)
    };
    let message_lines = vec![Line::from(Span::styled(message, message_style))];

    let question_height = wrapped_line_count(Text::from(question.clone()), inner.width);
    let actions_height = wrapped_line_count(Text::from(action_lines(app)), inner.width);
    let intro_height = wrapped_line_count(Text::from(intro.clone()), inner.width);
    let message_height = wrapped_line_count(Text::from(message_lines.clone()), inner.width);

    // Rows are handed out in priority order: question and actions, then the
    // directory (the tail of a long path is what tells projects apart), then
    // the message, and the intro sentence last. The path is cut into rows here
    // rather than by Paragraph wrapping so that a path too long for the space
    // left can show its tail behind an ellipsis.
    let question_rows = question_height.min(inner.height);
    let action_rows = actions_height.min(inner.height - question_rows);
    let mut free = inner.height - question_rows - action_rows;
    let directory = directory_rows(&shown_directory(app), inner.width, usize::from(free));
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
    frame.render_widget(Paragraph::new(intro).wrap(Wrap { trim: false }), chunks[2]);
    frame.render_widget(Paragraph::new(message_lines).wrap(Wrap { trim: false }), chunks[3]);
    frame.render_widget(Paragraph::new(action_lines(app)).wrap(Wrap { trim: false }), chunks[4]);
}

/// The directory the answer applies to: the key the trust store reads and
/// writes (set by `trust::refresh`), or, while that is empty because the
/// preferences file could not be read, the same normalisation applied to the
/// session cwd. Never the raw `-C` value, which may be relative.
fn shown_directory(app: &App) -> String {
    if app.trust.project_key.is_empty() {
        crate::app::trust::store::normalize_project_key(std::path::Path::new(&app.cwd_raw))
    } else {
        app.trust.project_key.clone()
    }
}

/// Cuts `path` into rows of at most `width` columns. When it needs more than
/// `max_rows`, keeps the tail behind a leading ellipsis in at most `max_rows`.
fn directory_rows(path: &str, width: u16, max_rows: usize) -> Vec<String> {
    let width = usize::from(width);
    if width == 0 || max_rows == 0 {
        return Vec::new();
    }
    let rows = chunk_columns(path.chars(), width);
    if rows.len() <= max_rows {
        return rows;
    }

    // Keep the last `max_rows * width - 1` columns of the path.
    let budget = max_rows * width - 1;
    let mut tail: Vec<char> = Vec::new();
    let mut used = 0usize;
    for ch in path.chars().rev() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > budget {
            break;
        }
        tail.push(ch);
        used += w;
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
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > width && !current.is_empty() {
            rows.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(ch);
        used += w;
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

    const LONG_PATH_SEGMENTS: [&str; 6] = [
        "Users",
        "someone",
        "Github",
        "a-rather-long-organisation-name",
        "client-projects",
        "deeply-nested-service-checkout",
    ];

    fn frame_stripped(rows: &[String]) -> String {
        rows.iter().map(|row| row.trim_matches(|c: char| c == '\u{2502}' || c == ' ')).collect()
    }

    #[test]
    fn trusted_view_wraps_a_long_directory_without_losing_segments() {
        let mut app = App::test_default();
        app.cwd_raw = format!("/{}", LONG_PATH_SEGMENTS.join("/"));

        let rows = draw_rows(&mut app, 60, 16);

        // A path has no spaces, so a wrap point is a plain character break.
        let body = frame_stripped(&rows);
        assert!(body.contains(&app.cwd_raw), "{body}");
        assert_eq!(rows.iter().filter(|row| row.contains("Trust this")).count(), 1);
    }

    #[test]
    fn trusted_view_shows_the_normalized_absolute_key_for_a_relative_cwd() {
        let mut app = App::test_default();
        app.cwd_raw = ".".to_owned();
        app.trust.project_key = "/work/the-real/project".to_owned();

        let text = draw_text(&mut app);

        assert!(text.contains("/work/the-real/project"), "{text}");
        assert!(
            !text.lines().any(|row| row.trim_matches(|c: char| c == '\u{2502}' || c == ' ') == ".")
        );
    }

    #[test]
    fn trusted_view_absolutizes_a_relative_cwd_when_the_key_is_empty() {
        let mut app = App::test_default();
        app.cwd_raw = "does-not-exist-claude-rs/sub".to_owned();
        let expected = std::env::current_dir().expect("cwd").join("does-not-exist-claude-rs/sub");

        let rows = draw_rows(&mut app, 200, 12);

        let body = frame_stripped(&rows);
        assert!(body.contains(&expected.to_string_lossy().replace('\\', "/")), "{body}");
    }

    #[test]
    fn trusted_view_keeps_the_path_tail_and_actions_on_short_screens() {
        for (width, height) in [(30u16, 14u16), (20, 16), (12, 24)] {
            let mut app = App::test_default();
            app.cwd_raw = format!("/{}", LONG_PATH_SEGMENTS.join("/"));

            let rows = draw_rows(&mut app, width, height);
            let body = frame_stripped(&rows);

            assert!(body.contains("service-checkout"), "{width}x{height}: {body}");
            let text = rows.join("\n");
            assert!(text.contains("> Yes"), "{width}x{height}: {text}");
            assert!(text.contains("  No"), "{width}x{height}: {text}");
        }
    }

    #[test]
    fn trusted_view_marks_a_path_cut_to_its_tail() {
        let mut app = App::test_default();
        app.trust.project_key = format!("/{}", "segment/".repeat(60));

        let rows = draw_rows(&mut app, 30, 14);
        let body = frame_stripped(&rows);

        assert!(body.contains('\u{2026}'), "{body}");
        assert!(body.contains("segment"), "{body}");
        assert!(rows.join("\n").contains("> Yes"), "{body}");
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
