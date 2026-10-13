// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

use super::highlight;
use super::theme;
use super::wrap::{hard_wrap_line, line_display_width};
use crate::failure;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const CODE_BLOCK_INDENT: &str = "  ";
/// Starts every code row, so no code line can read as a list item or a fence to the
/// row wrapper, and marks where the block runs.
const CODE_BLOCK_RAIL: &str = "\u{258e}";
const CODE_BLOCK_GUTTER: &str = " ";
const CODE_BLOCK_PREFIX_WIDTH: usize = 4;
const CODE_TAB: &str = "    ";

/// claude-rs's Markdown styles. Headings are rendered by the Markdown parser, so a `#`
/// inside a code block or any other non-heading context keeps its text.
#[derive(Clone, Copy)]
struct ClaudeRsStyleSheet;

impl tui_markdown::StyleSheet for ClaudeRsStyleSheet {
    fn heading(&self, level: u8) -> Style {
        match level {
            1 => Style::new()
                .fg(theme::ROLE_ASSISTANT)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            2 => Style::new().add_modifier(Modifier::BOLD),
            _ => Style::new().fg(theme::DIM).add_modifier(Modifier::BOLD),
        }
    }

    fn heading_marker(&self, _level: u8) -> &'static str {
        ""
    }
}

pub(super) fn render_markdown_safe(text: &str, bg: Option<Color>) -> Vec<Line<'static>> {
    render_markdown_safe_with(text, bg, render_with_tui_markdown)
}

fn render_markdown_safe_with<F>(text: &str, bg: Option<Color>, renderer: F) -> Vec<Line<'static>>
where
    F: FnOnce(&str, Option<Color>) -> Vec<Line<'static>>,
{
    if let Ok(lines) = failure::catch_expected_panic(|| renderer(text, bg)) {
        lines
    } else {
        tracing::warn!(
            target: crate::logging::targets::APP_RENDER,
            event_name = "markdown_render_failed",
            message = "markdown renderer panicked; falling back to plain text",
            outcome = "fallback",
        );
        plain_text_fallback(text, bg)
    }
}

fn render_with_tui_markdown(text: &str, bg: Option<Color>) -> Vec<Line<'static>> {
    let rendered =
        tui_markdown::from_str_with_options(text, &tui_markdown::Options::new(ClaudeRsStyleSheet));
    let lines = rendered
        .lines
        .into_iter()
        .map(|line| {
            let owned_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| {
                    let style =
                        if let Some(bg_color) = bg { span.style.bg(bg_color) } else { span.style };
                    Span::styled(span.content.into_owned(), style)
                })
                .collect();
            let line_style =
                if let Some(bg_color) = bg { line.style.bg(bg_color) } else { line.style };
            Line::from(owned_spans).style(line_style)
        })
        .collect();
    normalize_list_indentation(shorten_link_destinations(lines))
}

fn normalize_list_indentation(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let list_lines = rendered_list_line_flags(&lines);
    let mut normalized = Vec::with_capacity(lines.len());

    for (idx, line) in lines.into_iter().enumerate() {
        if list_lines[idx] {
            normalized.push(indent_line(line));
        } else {
            normalized.push(line);
        }
    }

    normalized
}

fn rendered_list_line_flags(lines: &[Line<'_>]) -> Vec<bool> {
    let mut in_fenced_code = false;
    let mut flags = Vec::with_capacity(lines.len());

    for line in lines {
        let text = line_text(line);
        let starts_fence = text.trim_start().starts_with("```");
        let in_code_line = in_fenced_code || starts_fence;
        flags.push(!in_code_line && rendered_line_is_list_item(&text));
        if starts_fence {
            in_fenced_code = !in_fenced_code;
        }
    }

    flags
}

fn rendered_line_is_list_item(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("- ") || starts_with_ordered_list_marker(trimmed)
}

fn starts_with_ordered_list_marker(text: &str) -> bool {
    let digit_count = text.bytes().take_while(u8::is_ascii_digit).count();
    digit_count > 0 && text[digit_count..].starts_with(". ")
}

fn indent_line(mut line: Line<'static>) -> Line<'static> {
    let existing_indent = line.spans.first().map_or(0, |span| {
        span.content.as_ref().chars().take_while(|ch| ch.is_whitespace()).count()
    });
    if let Some(first) = line.spans.first_mut()
        && existing_indent > 0
    {
        let content = first.content.to_string();
        first.content = content.chars().skip(existing_indent).collect::<String>().into();
    }
    let indent = " ".repeat(2 + existing_indent / 2);
    line.spans.insert(0, Span::styled(indent, line.style));
    line
}

fn shorten_link_destinations(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    const MAX_LINK_DESTINATION_WIDTH: usize = 48;

    for line in &mut lines {
        for span_idx in 1..line.spans.len() {
            let is_destination = line.spans[span_idx - 1].content.as_ref() == " (";
            if !is_destination {
                continue;
            }
            let span = &mut line.spans[span_idx];
            let content = span.content.as_ref();
            if content.starts_with("http://") || content.starts_with("https://") {
                let shortened = shorten_link_destination(content, MAX_LINK_DESTINATION_WIDTH);
                span.content = shortened.into();
                span.style = span.style.fg(theme::DIM).remove_modifier(Modifier::UNDERLINED);
            }
        }
    }
    lines
}

fn shorten_link_destination(destination: &str, width: usize) -> String {
    if line_display_width(&Line::from(destination)) <= width {
        return destination.to_owned();
    }

    let mut shortened = String::new();
    let mut used = 0usize;
    for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(destination, true) {
        let grapheme_width = unicode_width::UnicodeWidthStr::width(grapheme);
        if used + grapheme_width + 1 > width {
            break;
        }
        shortened.push_str(grapheme);
        used += grapheme_width;
    }
    shortened.push('…');
    shortened
}

#[cfg(test)]
fn line_is_blank(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.trim().is_empty())
}

fn line_text(line: &Line<'_>) -> String {
    line.spans.iter().map(|span| span.content.as_ref()).collect()
}

/// Renders a code block as indented rows on the highlight theme's background, behind a dim
/// rail, with the fence's language as a label row. The code itself is shown exactly as
/// written (tabs expanded) and wrapped at the cell, never at a word.
pub(super) fn render_code_block(
    code: &str,
    language: Option<&str>,
    width: u16,
    bg: Option<Color>,
) -> Vec<Line<'static>> {
    let outer = bg.map_or_else(Style::default, |color| Style::default().bg(color));
    let fill = highlight::code_background()
        .map_or_else(Style::default, |color| Style::default().bg(color));
    let code_width = usize::from(width).saturating_sub(CODE_BLOCK_PREFIX_WIDTH).max(1);
    let row = |body: Line<'static>| {
        let padding = code_width.saturating_sub(line_display_width(&body));
        let mut spans = Vec::with_capacity(body.spans.len() + 4);
        spans.push(Span::styled(CODE_BLOCK_INDENT, outer));
        spans.push(Span::styled(CODE_BLOCK_RAIL, fill.fg(theme::DIM)));
        spans.push(Span::styled(CODE_BLOCK_GUTTER, fill));
        spans.extend(body.spans.into_iter().map(|span| {
            let style = fill.patch(span.style);
            Span::styled(span.content, style)
        }));
        if padding > 0 {
            spans.push(Span::styled(" ".repeat(padding), fill));
        }
        Line::from(spans)
    };

    let mut rows = Vec::new();
    if let Some(language) = language {
        let label = Span::styled(
            language.to_owned(),
            Style::default().fg(theme::DIM).add_modifier(Modifier::ITALIC),
        );
        rows.extend(hard_wrap_line(Line::from(label), code_width).into_iter().map(row));
    }
    let code = code.strip_suffix('\n').unwrap_or(code).replace('\t', CODE_TAB);
    for line in highlight::highlight_code(&code, language) {
        rows.extend(hard_wrap_line(line, code_width).into_iter().map(row));
    }
    rows
}

fn plain_text_fallback(text: &str, bg: Option<Color>) -> Vec<Line<'static>> {
    let style =
        if let Some(bg_color) = bg { Style::default().bg(bg_color) } else { Style::default() };

    text.split('\n').map(|line| Line::from(Span::styled(line.to_owned(), style))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::catch_unwind;

    fn rendered_text(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(line_text).collect()
    }

    #[test]
    fn render_markdown_safe_handles_common_and_edge_case_inputs_without_panicking() {
        let inputs = [
            "- [ ] one\n- [x] two",
            "- [ ] Move todos below input top line",
            "- [ ]\n- [x]\n- [ ]",
            "- [x] done\n  - [ ] child",
            "1. [ ] numbered checklist marker",
            "[]()[]()[]()",
            "```md\n- [ ] fenced checklist\n```",
            "> - [ ] blockquote checklist\n>\n> text",
            "# Heading\n- [ ] item\n\n| a | b |\n|---|---|\n| x | y |",
            "- [ ] [link](https://example.com) [",
            "- [ ] \u{200d}\u{200d}\u{200d}",
        ];

        for input in inputs {
            let result = catch_unwind(|| render_markdown_safe(input, None));
            assert!(result.is_ok(), "input triggered panic: {input}");
            assert!(!result.unwrap().is_empty(), "input rendered zero lines: {input}");
        }
    }

    #[test]
    fn render_markdown_safe_falls_back_to_plain_text_and_preserves_requested_bg() {
        let lines = render_markdown_safe_with("line1\nline2", Some(Color::Blue), |_text, _bg| {
            panic!("forced renderer panic for fallback path")
        });
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content.as_ref(), "line1");
        assert_eq!(lines[1].spans[0].content.as_ref(), "line2");
        assert_eq!(lines[0].spans[0].style.bg, Some(Color::Blue));
        assert_eq!(lines[1].spans[0].style.bg, Some(Color::Blue));
    }

    #[test]
    fn unordered_lists_keep_blank_rows_between_document_blocks() {
        let lines = render_markdown_safe("Intro\n\n- One\n- Two\n\nOutro", None);

        assert_eq!(rendered_text(&lines), vec!["Intro", "", "  - One", "  - Two", "", "Outro"]);
    }

    #[test]
    fn ordered_lists_keep_blank_rows_between_document_blocks() {
        let lines = render_markdown_safe("Intro\n\n1. One\n2. Two\n\nOutro", None);

        assert_eq!(rendered_text(&lines), vec!["Intro", "", "  1. One", "  2. Two", "", "Outro"]);
    }

    #[test]
    fn nested_lists_keep_relative_indentation() {
        let lines = render_markdown_safe("- Parent\n  - Child\n    - Grandchild", None);

        assert_eq!(rendered_text(&lines), vec!["  - Parent", "    - Child", "      - Grandchild"]);
    }

    #[test]
    fn wrapped_nested_list_continuations_keep_the_compact_indent() {
        let lines = render_markdown_safe(
            "- Parent\n  - nested list item that wraps to another physical row",
            None,
        );
        let rows = crate::ui::wrap::wrap_markdown_lines_to_physical_rows(&lines, 24);

        assert_eq!(
            rendered_text(&rows),
            vec![
                "  - Parent",
                "    - nested list item",
                "      that wraps to",
                "      another physical",
                "      row"
            ]
        );
    }

    #[test]
    fn task_lists_keep_markers_after_indentation() {
        let lines = render_markdown_safe("- [ ] Todo\n- [x] Done", None);

        assert_eq!(rendered_text(&lines), vec!["  - [ ] Todo", "  - [x] Done"]);
    }

    #[test]
    fn fenced_code_keeps_list_like_lines_unchanged() {
        let lines = render_markdown_safe("```md\n- Not a rendered list\n```", None);

        assert_eq!(rendered_text(&lines), vec!["```md", "- Not a rendered list", "```"]);
    }

    #[test]
    fn list_indent_preserves_requested_background() {
        let lines = render_markdown_safe("- One", Some(Color::Blue));

        assert_eq!(rendered_text(&lines), vec!["  - One"]);
        assert_eq!(lines[0].spans[0].style.bg, Some(Color::Blue));
        assert!(lines[0].spans.iter().all(|span| span.style.bg == Some(Color::Blue)));
    }

    #[test]
    fn headings_have_distinct_hierarchy_styles() {
        let lines = render_markdown_safe("# One\n\n## Two\n\n### Three", None);

        let styles: Vec<_> =
            lines.iter().filter(|line| !line_is_blank(line)).map(|line| line.style).collect();
        assert!(styles[0].add_modifier.contains(Modifier::BOLD));
        assert!(styles[0].add_modifier.contains(Modifier::UNDERLINED));
        assert!(styles[1].add_modifier.contains(Modifier::BOLD));
        assert!(!styles[1].add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(styles[2].fg, Some(theme::DIM));
    }

    #[test]
    fn long_link_destinations_are_dimmed_and_shortened_after_the_label() {
        let lines = render_markdown_safe(
            "[documentation](https://example.com/a/very/long/path/that/should/not/take/the/whole/terminal)",
            None,
        );

        let destination = lines[0]
            .spans
            .iter()
            .find(|span| span.content.starts_with("https://"))
            .expect("rendered link destination");
        assert!(destination.content.ends_with('…'));
        assert_eq!(destination.style.fg, Some(theme::DIM));
        assert!(!destination.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn literal_parenthesized_urls_are_not_shortened_as_link_destinations() {
        let url =
            "https://example.com/a/very/long/path/that/is/plain/prose/and/must/remain/visible";
        let lines = render_markdown_safe(&format!("See ({url}) for details."), None);

        assert!(rendered_text(&lines).join("\n").contains(url));
    }
}
