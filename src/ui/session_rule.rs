// SPDX-License-Identifier: Apache-2.0

//! The rule above the composer that carries the session's name and colour,
//! drawn the way Claude Code 2.1.293 draws its prompt bar: `──── name ─`, and
//! with a colour the whole rule in that colour and the name as a badge.

use super::theme;
use crate::agent::model::SessionColor;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const RULE: &str = "─";
const ELLIPSIS: char = '…';
/// Stock's badge text is its inverse text colour, black on these backgrounds.
const BADGE_TEXT: Color = Color::Rgb(0, 0, 0);
/// One rule cell before the name and one after it, as stock leaves.
const MIN_EDGE_CELLS: usize = 2;
const BADGE_PADDING_CELLS: usize = 2;

/// Claude Code's `<colour>_FOR_SUBAGENTS_ONLY` theme tokens, read from the
/// 2.1.293 binary; a truecolour probe of `/color blue` measured (106,155,204).
const fn session_color_rgb(color: SessionColor) -> Color {
    match color {
        SessionColor::Red => Color::Rgb(220, 38, 38),
        SessionColor::Blue => Color::Rgb(106, 155, 204),
        SessionColor::Green => Color::Rgb(22, 163, 74),
        SessionColor::Yellow => Color::Rgb(202, 138, 4),
        SessionColor::Purple => Color::Rgb(130, 125, 189),
        SessionColor::Orange => Color::Rgb(217, 119, 87),
        SessionColor::Pink => Color::Rgb(196, 102, 134),
        SessionColor::Cyan => Color::Rgb(8, 145, 178),
    }
}

/// The rule row for a session with a name or a colour; `None` keeps the
/// composer exactly as it is for a session with neither.
pub(crate) fn session_rule_line(
    title: Option<&str>,
    color: Option<SessionColor>,
    width: u16,
) -> Option<Line<'static>> {
    if title.is_none() && color.is_none() {
        return None;
    }
    let width = usize::from(width);
    let rule_style = Style::default().fg(color.map_or(theme::DIM, session_color_rgb));
    let label_style = color.map_or_else(Style::default, |color| {
        Style::default().fg(BADGE_TEXT).bg(session_color_rgb(color))
    });

    let name_budget = width.saturating_sub(MIN_EDGE_CELLS + BADGE_PADDING_CELLS);
    let name = title.map(|title| truncate_to_width(title, name_budget)).unwrap_or_default();
    if name.is_empty() {
        return Some(Line::from(Span::styled(RULE.repeat(width), rule_style)));
    }
    let label = format!(" {name} ");
    let lead = width.saturating_sub(label.width() + 1);
    Some(Line::from(vec![
        Span::styled(RULE.repeat(lead), rule_style),
        Span::styled(label, label_style),
        Span::styled(RULE, rule_style),
    ]))
}

/// The name cut to `max_cells` display cells, ending in `…` when cut.
fn truncate_to_width(name: &str, max_cells: usize) -> String {
    let name = name.trim();
    if name.width() <= max_cells {
        return name.to_owned();
    }
    if max_cells == 0 {
        return String::new();
    }
    let mut kept = String::new();
    let mut used = 0;
    for ch in name.chars() {
        let cells = ch.width().unwrap_or(0);
        if used + cells + 1 > max_cells {
            break;
        }
        kept.push(ch);
        used += cells;
    }
    kept.push(ELLIPSIS);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|span| span.content.as_ref()).collect()
    }

    fn span_containing<'a>(line: &'a Line<'a>, needle: &str) -> &'a Span<'a> {
        line.spans.iter().find(|span| span.content.contains(needle)).expect("span")
    }

    #[test]
    fn no_rule_without_a_name_or_a_colour() {
        assert!(session_rule_line(None, None, 80).is_none());
    }

    #[test]
    fn name_sits_right_aligned_in_a_full_width_rule() {
        let line = session_rule_line(Some("probe-e2e"), None, 30).expect("rule");
        assert_eq!(text(&line), format!("{} probe-e2e ─", "─".repeat(18)));
        assert_eq!(text(&line).width(), 30);
        let name = span_containing(&line, "probe-e2e");
        assert_eq!(name.style.bg, None, "an uncoloured name is plain text");
    }

    #[test]
    fn colour_paints_the_rule_and_makes_the_name_a_black_badge() {
        let line =
            session_rule_line(Some("probe-e2e"), Some(SessionColor::Blue), 30).expect("rule");
        let blue = Color::Rgb(106, 155, 204);
        let badge = span_containing(&line, "probe-e2e");
        assert_eq!(badge.content, " probe-e2e ");
        assert_eq!((badge.style.fg, badge.style.bg), (Some(Color::Rgb(0, 0, 0)), Some(blue)));
        for rule in line.spans.iter().filter(|span| span.content.contains('─')) {
            assert_eq!(rule.style.fg, Some(blue));
        }
    }

    #[test]
    fn colour_alone_draws_a_plain_coloured_rule() {
        let line = session_rule_line(None, Some(SessionColor::Green), 12).expect("rule");
        assert_eq!(text(&line), "─".repeat(12));
        assert_eq!(line.spans[0].style.fg, Some(Color::Rgb(22, 163, 74)));
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_never_wraps() {
        for width in [4_u16, 5, 8, 12, 20] {
            let line =
                session_rule_line(Some("a-very-long-session-name"), None, width).expect("rule");
            assert_eq!(text(&line).width(), usize::from(width), "width {width}");
        }
        let line = session_rule_line(Some("a-very-long-session-name"), None, 12).expect("rule");
        assert_eq!(text(&line), "─ a-very-… ─");
        let wide = session_rule_line(Some("名前のセッション"), None, 12).expect("rule");
        assert_eq!(text(&wide).width(), 12);
        assert!(text(&wide).contains('…'));
    }
}
