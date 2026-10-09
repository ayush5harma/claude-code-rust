// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

use crate::app::WelcomeBlock;
use crate::ui::host_tips::HOST_TIPS;
use crate::ui::theme;
use crate::ui::wrap::{
    StyledChunk, display_width, join_column_lines, wrap_styled_chunks,
    wrap_styled_chunks_with_hanging_prefix,
};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

// Claude Code's mascot exactly as the 2.1.293 welcome banner draws it (read
// from its screen with colour escapes): block glyphs in the body colour, and
// the cells holding the eyes on a black background, so the empty quadrant of
// each eye glyph shows black on any terminal theme. Each segment is
// (text, on the eye background).
const LOGO_ART: &[&[(&str, bool)]] = &[
    &[
        (" \u{2590}", false),
        ("\u{259b}\u{2588}\u{2588}\u{2588}\u{259b}\u{2588}", true),
        (" ", false),
    ],
    &[
        ("\u{259d}\u{259c}", false),
        ("\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}", true),
        ("\u{2588}\u{2580}", false),
    ],
    &[(" \u{259d}\u{259d}   \u{259d}\u{259d} ", false)],
];
const LOGO_LEFT_PADDING: &str = "  ";
const LOGO_TEXT_GAP: usize = 2;
const MIN_INLINE_FIELD_VALUE_WIDTH: usize = 8;
const WELCOME_FIELD_LABELS: &[&str] = &["Version", "Subscription", "Cwd", "Session ID", "Tips"];

pub(crate) fn overview_lines(
    block: &WelcomeBlock,
    loading_status: Option<&str>,
    width: u16,
) -> Vec<Line<'static>> {
    let width = usize::from(width);
    if width == 0 {
        return vec![Line::default()];
    }

    let loading = loading_status.unwrap_or("Loading");
    let subscription_missing = welcome_value_missing(&block.subscription);
    let session_missing = welcome_value_missing(&block.session_id);
    let subscription_value =
        if subscription_missing { loading.to_owned() } else { block.subscription.clone() };
    let session_value = if session_missing { loading.to_owned() } else { block.session_id.clone() };
    let subscription_style = if subscription_missing {
        Style::default().fg(theme::DIM)
    } else {
        Style::default().fg(theme::RUST_ORANGE).add_modifier(Modifier::BOLD)
    };

    let logo_width = logo_column_width();
    let overview_offset = logo_width.saturating_add(LOGO_TEXT_GAP);
    let overview_width = width.saturating_sub(overview_offset);
    let side_by_side = overview_width >= minimum_overview_width();
    let text_width = if side_by_side { overview_width } else { width };
    let text_rows = overview_text_rows(
        block,
        &subscription_value,
        subscription_style,
        &session_value,
        text_width,
    );
    let mut lines = if side_by_side {
        join_column_lines(logo_rows(), text_rows, logo_width, LOGO_TEXT_GAP)
    } else {
        text_rows
    };
    lines.push(Line::default());
    lines
}

fn overview_text_rows(
    block: &WelcomeBlock,
    subscription_value: &str,
    subscription_style: Style,
    session_value: &str,
    width: usize,
) -> Vec<Line<'static>> {
    let dim = Style::default().fg(theme::DIM);
    let mut rows = Vec::new();
    rows.extend(welcome_field_lines("Version", &block.version, dim, width));
    rows.extend(welcome_field_lines("Subscription", subscription_value, subscription_style, width));
    rows.extend(welcome_field_lines("Cwd", &block.cwd, dim, width));
    rows.extend(welcome_field_lines("Session ID", session_value, dim, width));
    rows.push(Line::default());
    rows.extend(welcome_field_lines("Tips", selected_tip(block), dim, width));
    rows
}

fn welcome_field_lines(
    label: &str,
    value: &str,
    value_style: Style,
    width: usize,
) -> Vec<Line<'static>> {
    let prefix =
        vec![StyledChunk { text: format!("{label}: "), style: Style::default().fg(theme::DIM) }];
    let body = vec![StyledChunk { text: value.to_owned(), style: value_style }];
    let prefix_width = prefix.iter().map(|chunk| display_width(&chunk.text)).sum::<usize>();
    if width.saturating_sub(prefix_width) < MIN_INLINE_FIELD_VALUE_WIDTH {
        let mut rows = wrap_styled_chunks(&prefix, width);
        rows.extend(wrap_styled_chunks(&body, width));
        return rows;
    }

    wrap_styled_chunks_with_hanging_prefix(&prefix, &body, width, Style::default())
}

fn logo_row_width(row: &[(&str, bool)]) -> usize {
    row.iter().map(|(text, _)| display_width(text)).sum()
}

fn logo_column_width() -> usize {
    display_width(LOGO_LEFT_PADDING)
        .saturating_add(LOGO_ART.iter().map(|row| logo_row_width(row)).max().unwrap_or(0))
}

fn logo_rows() -> Vec<Line<'static>> {
    let body = Style::default().fg(theme::CLAWD_BODY);
    LOGO_ART
        .iter()
        .map(|row| {
            let mut spans = vec![Span::raw(LOGO_LEFT_PADDING)];
            spans.extend(row.iter().map(|&(text, eye)| {
                Span::styled(text, if eye { body.bg(theme::CLAWD_EYES) } else { body })
            }));
            Line::from(spans)
        })
        .collect()
}

fn minimum_overview_width() -> usize {
    WELCOME_FIELD_LABELS
        .iter()
        .map(|label| display_width(&format!("{label}: ")))
        .max()
        .unwrap_or(0)
        .saturating_add(MIN_INLINE_FIELD_VALUE_WIDTH)
}

fn welcome_value_missing(value: &str) -> bool {
    value.trim().is_empty() || value == "-"
}

pub(crate) fn selected_tip(block: &WelcomeBlock) -> &'static str {
    let Some(first_tip) = HOST_TIPS.first().copied() else {
        return "Enter sends, Shift+Enter inserts a newline, and Ctrl+C clears or quits";
    };
    let len_u64 = u64::try_from(HOST_TIPS.len()).unwrap_or(1);
    let idx_u64 = block.tip_seed % len_u64;
    let idx = usize::try_from(idx_u64).unwrap_or(0);
    HOST_TIPS.get(idx).copied().unwrap_or(first_tip)
}

#[cfg(test)]
mod tests {
    use super::{HOST_TIPS, Line, overview_lines};
    use crate::app::{ChatMessage, MessageBlock};
    use crate::ui::theme;
    use crate::ui::wrap::{display_width, line_display_width};

    const LOGO_MIDDLE_ROW: &str =
        "\u{259d}\u{259c}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2580}";

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|span| span.content.as_ref()).collect()
    }

    // The logo's glyphs are three bytes each, so a column is a display width,
    // not a byte offset.
    fn column_of(text: &str, needle: &str) -> Option<usize> {
        text.find(needle).map(|byte| display_width(&text[..byte]))
    }

    #[test]
    fn overview_lines_render_expected_fields() {
        let mut message = ChatMessage::welcome(env!("CARGO_PKG_VERSION"), "-", "/cwd", "-");
        let MessageBlock::Welcome(block) = &mut message.blocks[0] else {
            panic!("expected welcome block");
        };
        block.tip_seed = 16;
        let lines: Vec<String> = overview_lines(block, None, 120)
            .into_iter()
            .map(|line| line.spans.into_iter().map(|s| s.content).collect())
            .collect();
        assert!(lines.iter().any(|line| line.contains(LOGO_MIDDLE_ROW)));
        assert!(!lines.iter().any(|line| line.contains("_~^~^~_")));
        assert!(!lines.iter().any(|line| line.contains("Welcome back to Claude, in Rust!")));
        assert!(lines.iter().any(|line| line.contains("Version:")));
        assert!(lines.iter().any(|line| line.contains("Subscription: Loading")));
        assert!(lines.iter().any(|line| line.contains("Cwd: /cwd")));
        assert!(lines.iter().any(|line| line.contains("Session ID: Loading")));
        assert!(lines.iter().any(|line| line.contains("Tips: ")));
        assert!(
            HOST_TIPS.iter().any(|tip| lines.iter().any(|line| line.contains(tip))),
            "expected one welcome tip to be rendered"
        );
    }

    #[test]
    fn wide_overview_wraps_tip_below_its_value() {
        let mut message = ChatMessage::welcome("1.2.3", "Pro", "/workspace/demo", "session-123");
        let MessageBlock::Welcome(block) = &mut message.blocks[0] else {
            panic!("expected welcome block");
        };
        block.tip_seed = 7;

        let lines = overview_lines(block, None, 103);
        let text = lines.iter().map(line_text).collect::<Vec<_>>();
        let tip_row = text.iter().position(|line| line.contains("Tips: Start")).expect("tip row");

        assert_eq!(column_of(&text[tip_row], "Tips:"), Some(13));
        assert_eq!(text[tip_row + 1].find(|ch: char| !ch.is_whitespace()), Some(19));
        assert!(lines.iter().all(|line| line_display_width(line) <= 103));
    }

    #[test]
    fn wide_overview_wraps_long_metadata_below_its_value() {
        let mut message = ChatMessage::welcome(
            "1.2.3",
            "Pro",
            "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda",
            "session-123",
        );
        let MessageBlock::Welcome(block) = &mut message.blocks[0] else {
            panic!("expected welcome block");
        };

        let text = overview_lines(block, None, 63).iter().map(line_text).collect::<Vec<_>>();
        let cwd_row = text.iter().position(|line| line.contains("Cwd: alpha")).expect("cwd row");

        assert_eq!(column_of(&text[cwd_row], "Cwd:"), Some(13));
        assert_eq!(text[cwd_row + 1].find(|ch: char| !ch.is_whitespace()), Some(18));
    }

    #[test]
    fn narrow_overview_hides_logo_and_keeps_hanging_indent() {
        let mut message = ChatMessage::welcome("1.2.3", "Pro", "/workspace/demo", "session-123");
        let MessageBlock::Welcome(block) = &mut message.blocks[0] else {
            panic!("expected welcome block");
        };
        block.tip_seed = 7;

        let lines = overview_lines(block, None, 32);
        let text = lines.iter().map(line_text).collect::<Vec<_>>();
        let tip_row =
            text.iter().position(|line| line.starts_with("Tips: Start")).expect("tip row");

        assert!(!text.iter().any(|line| line.contains(LOGO_MIDDLE_ROW)));
        assert_eq!(text[tip_row + 1].find(|ch: char| !ch.is_whitespace()), Some(6));
        assert!(lines.iter().all(|line| line_display_width(line) <= 32));
    }

    #[test]
    fn logo_draws_claude_code_glyphs_with_black_eye_cells() {
        let message = ChatMessage::welcome("1.2.3", "Pro", "/workspace/demo", "session-123");
        let MessageBlock::Welcome(block) = &message.blocks[0] else {
            panic!("expected welcome block");
        };
        let lines = overview_lines(block, None, 120);
        let logo = &lines[..3];

        let rows = logo.iter().map(line_text).collect::<Vec<_>>();
        assert!(rows[0].starts_with("   \u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259b}\u{2588}"));
        assert!(rows[1].starts_with(&format!("  {LOGO_MIDDLE_ROW}")));
        assert!(rows[2].starts_with("   \u{259d}\u{259d}   \u{259d}\u{259d}"));

        let eye_span = logo[0]
            .spans
            .iter()
            .find(|span| span.content.starts_with('\u{259b}'))
            .expect("eye cells");
        assert_eq!(eye_span.style.fg, Some(theme::CLAWD_BODY));
        assert_eq!(eye_span.style.bg, Some(theme::CLAWD_EYES));
        let foot_span =
            logo[2].spans.iter().find(|span| span.content.contains('\u{259d}')).expect("feet");
        assert_eq!(foot_span.style.fg, Some(theme::CLAWD_BODY));
        assert_eq!(foot_span.style.bg, None);
    }
}
