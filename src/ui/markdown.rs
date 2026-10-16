// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

mod lists;

use super::highlight;
use super::theme;
use super::wrap::{hard_wrap_line, line_display_width};
use crate::failure;
use pulldown_cmark::{Event, Options, Parser, Tag};
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
                .fg(theme::role_assistant())
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            2 => Style::new().add_modifier(Modifier::BOLD),
            _ => Style::new().fg(theme::tertiary()).add_modifier(Modifier::BOLD),
        }
    }

    fn heading_marker(&self, _level: u8) -> &'static str {
        ""
    }
}

pub(super) fn render_markdown_safe(text: &str, bg: Option<Color>) -> Vec<Line<'static>> {
    render_markdown_at_width(text, u16::MAX, bg)
}

/// List rows are laid out while parser ownership is available, before becoming plain rows.
pub(super) fn render_markdown_at_width(
    text: &str,
    width: u16,
    bg: Option<Color>,
) -> Vec<Line<'static>> {
    render_markdown_safe_with(text, bg, |text, bg| lists::render(text, width, bg))
}

fn parser_options() -> Options {
    // Match the dialect used by tui-markdown for non-list blocks.
    Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_MATH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_GFM
}

fn list_ranges(source: &str) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
    Parser::new_ext(source, parser_options())
        .into_offset_iter()
        .filter_map(|(event, range)| matches!(event, Event::Start(Tag::List(_))).then_some(range))
}

/// A boundary inside one parsed list cannot be frozen or rendered independently.
pub(super) fn list_crosses_boundary(source: &str, boundary: usize) -> bool {
    list_ranges(source).any(|range| range.start < boundary && range.end > boundary)
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
    shorten_link_destinations(lines)
}

fn shorten_link_destinations(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    for line in &mut lines {
        for span_idx in 1..line.spans.len() {
            let is_destination = line.spans[span_idx - 1].content.as_ref() == " (";
            if !is_destination {
                continue;
            }
            let span = &line.spans[span_idx];
            line.spans[span_idx] = link_destination_span(span.content.as_ref(), span.style);
        }
    }
    lines
}

/// A web link's destination after its label: dimmed, not underlined, and shortened to
/// `MAX_LINK_DESTINATION_WIDTH`. Other destinations keep their text and style.
fn link_destination_span(destination: &str, style: Style) -> Span<'static> {
    const MAX_LINK_DESTINATION_WIDTH: usize = 48;
    if destination.starts_with("http://") || destination.starts_with("https://") {
        Span::styled(
            shorten_link_destination(destination, MAX_LINK_DESTINATION_WIDTH),
            style.fg(theme::tertiary()).remove_modifier(Modifier::UNDERLINED),
        )
    } else {
        Span::styled(destination.to_owned(), style)
    }
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
        spans.push(Span::styled(CODE_BLOCK_RAIL, fill.fg(theme::tertiary())));
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
            Style::default().fg(theme::tertiary()).add_modifier(Modifier::ITALIC),
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
    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|span| span.content.as_ref()).collect()
    }

    fn line_is_blank(line: &Line<'_>) -> bool {
        line.spans.iter().all(|span| span.content.trim().is_empty())
    }
    use super::*;
    use std::panic::catch_unwind;

    fn rendered_text(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(line_text).collect()
    }

    fn document_rows(source: &str, width: u16) -> Vec<String> {
        let lines =
            crate::ui::document_table::render_markdown_with_tables(source, width, None, false);
        crate::ui::wrap::wrap_lines_to_physical_rows(&lines, width)
            .iter()
            .map(|line| line_text(line).trim_end().to_owned())
            .collect()
    }

    #[test]
    fn list_continuation_paragraphs_and_nested_items_retain_their_parent() {
        let rows = document_rows(
            "3. First paragraph.\n\n   Second paragraph.\n\n   - Child item.\n\n     Child continuation.\n\n   Parent again.\n4. Next item.",
            60,
        );
        for (text, column) in [
            ("Second paragraph.", 5),
            ("- Child item.", 5),
            ("Child continuation.", 7),
            ("Parent again.", 5),
        ] {
            let row = rows.iter().find(|row| row.trim() == text).expect("list content survives");
            assert_eq!(row.find(text), Some(column), "wrong parent alignment: {rows:?}");
        }
        let first = rows.iter().position(|row| row.contains("First paragraph.")).unwrap();
        let second = rows.iter().position(|row| row.contains("Second paragraph.")).unwrap();
        assert!(
            rows[first + 1..second].iter().any(String::is_empty),
            "separate item paragraphs must stay separate: {rows:?}"
        );
        assert!(rows.iter().any(|row| row.trim() == "4. Next item."));
    }

    #[test]
    fn ordered_and_task_item_wrapping_use_the_actual_body_column() {
        for (source, prefix, column) in [
            ("9) alpha beta gamma delta epsilon\n1) zeta eta theta iota kappa", "9.", 5),
            ("10. alpha beta gamma delta epsilon", "10.", 6),
            ("- [x] alpha beta gamma delta epsilon", "- [x]", 8),
        ] {
            let rows = document_rows(source, 22);
            let first =
                rows.iter().position(|row| row.trim_start().starts_with(prefix)).expect("marker");
            let continuation = &rows[first + 1];
            assert!(!continuation.trim().is_empty());
            assert_eq!(continuation.len() - continuation.trim_start().len(), column, "{rows:?}");
            assert!(rows.iter().all(|row| crate::ui::wrap::display_width(row) <= 22));
            if source.starts_with("9)") {
                assert!(
                    rows.iter().any(|row| row.trim_start().starts_with("10. zeta")),
                    "number sequentially from the declared start: {rows:?}"
                );
            }
        }
    }

    #[test]
    fn code_and_escaped_markers_do_not_acquire_list_indentation() {
        for source in [
            "~~~text\n- literal marker\n~~~",
            "    - indented code",
            "\\- escaped marker",
            "3a ordinary text",
        ] {
            let rows = document_rows(source, 60);
            let content = rows
                .iter()
                .find(|row| {
                    row.contains("marker") || row.contains("ordinary") || row.contains("indented")
                })
                .expect("literal content");
            // Code is drawn as a code block, whose own indent and rail are not list padding.
            let content = content.strip_prefix("  \u{258e} ").unwrap_or(content);
            assert!(
                !content.starts_with("  "),
                "non-list source acquired list padding: {source:?}: {rows:?}"
            );
        }
    }

    #[test]
    fn nested_list_wrapping_preserves_inline_styles_links_and_unicode() {
        let lines = crate::ui::document_table::render_markdown_with_tables(
            "1. Parent\n   - **Bold** and *italic* [docs](https://example.com) 日本語😀 words words words",
            28,
            Some(Color::Blue),
            false,
        );
        let text = lines.iter().map(line_text).collect::<Vec<_>>();
        let content = text.join(" ");
        for required in ["Bold", "italic", "https://example.com", "日本語😀"] {
            assert!(content.contains(required), "lost inline content: {text:?}");
        }
        for (word, modifier) in
            [("Bold", ratatui::style::Modifier::BOLD), ("italic", ratatui::style::Modifier::ITALIC)]
        {
            assert!(
                lines.iter().flat_map(|line| &line.spans).any(|span| span.content.contains(word)
                    && span.style.add_modifier.contains(modifier)),
                "lost {word} formatting"
            );
        }
        assert!(lines.iter().all(|line| crate::ui::wrap::line_display_width(line) <= 28));
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style.bg == Some(Color::Blue))
        );
        let child = text.iter().position(|row| row.contains("- Bold")).expect("nested item");
        for row in &text[child + 1..] {
            assert_eq!(
                row.find(|ch: char| !ch.is_whitespace()),
                Some(7),
                "child continuation lost its parent: {text:?}"
            );
        }
    }

    #[test]
    fn table_and_code_inside_a_list_keep_the_parent_column() {
        let rows = document_rows(
            "1. Parent\n\n   | Name | Value |\n   | --- | --- |\n   | Alpha | Beta |\n\n   ~~~text\n   - literal code\n       indented code\n   ~~~\n\n   Tail paragraph.",
            60,
        );
        for word in ["Name", "Alpha", "- literal code", "Tail paragraph."] {
            let row = rows.iter().find(|row| row.contains(word)).expect("nested content survives");
            assert!(row.starts_with("     "), "content escaped its parent: {rows:?}");
        }
        let code = rows.iter().find(|row| row.contains("- literal code")).unwrap();
        let code_column = code.chars().position(|ch| ch == '-');
        assert_eq!(code_column, Some(9), "code marker must stay literal, after the code rail");
        let indented = rows.iter().find(|row| row.contains("indented code")).unwrap();
        let indented_column =
            indented.find("indented code").map(|byte| indented[..byte].chars().count());
        assert_eq!(
            indented_column,
            Some(13),
            "code indentation must survive inside its parent list"
        );
    }

    #[test]
    fn quoted_lists_keep_one_quote_prefix_on_wrapped_and_continued_rows() {
        let rows = document_rows(
            "> Introduction\n> continued\n>\n> 1. Parent item with enough words to wrap onto another row\n>\n>    Continuation paragraph\n>\n>    - Child item\n>\n> End quote",
            30,
        );
        for row in rows.iter().filter(|row| !row.trim().is_empty()) {
            assert!(row.starts_with("> "), "quote ownership lost: {rows:?}");
            assert_eq!(row.matches('>').count(), 1, "quote prefixes duplicated: {rows:?}");
        }
        assert!(rows.iter().any(|row| row.contains("Child item")));
        assert!(rows.iter().any(|row| row.contains("End quote")));
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
    fn unordered_lists_preserve_surrounding_paragraph_separators() {
        let lines = render_markdown_safe("Intro\n\n- One\n- Two\n\nOutro", None);

        assert_eq!(rendered_text(&lines), vec!["Intro", "", "  - One", "  - Two", "", "Outro"]);
    }

    #[test]
    fn ordered_lists_preserve_surrounding_paragraph_separators() {
        let lines = render_markdown_safe("Intro\n\n1. One\n2. Two\n\nOutro", None);

        assert_eq!(rendered_text(&lines), vec!["Intro", "", "  1. One", "  2. Two", "", "Outro"]);
    }

    #[test]
    fn nested_lists_keep_relative_indentation() {
        let lines = render_markdown_safe("- Parent\n  - Child\n    - Grandchild", None);

        assert_eq!(rendered_text(&lines), vec!["  - Parent", "    - Child", "      - Grandchild"]);
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
        assert_eq!(styles[2].fg, Some(theme::tertiary()));
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
        assert_eq!(destination.style.fg, Some(theme::tertiary()));
        assert!(!destination.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn literal_parenthesized_urls_are_not_shortened_as_link_destinations() {
        let url =
            "https://example.com/a/very/long/path/that/is/plain/prose/and/must/remain/visible";
        let lines = render_markdown_safe(&format!("See ({url}) for details."), None);

        assert!(rendered_text(&lines).join("\n").contains(url));
    }

    #[test]
    fn list_links_are_shortened_like_paragraph_links() {
        let lines = render_markdown_safe(
            "- see [documentation](https://example.com/a/very/long/path/that/should/not/take/the/whole/terminal)",
            None,
        );
        let destination = lines
            .iter()
            .flat_map(|line| &line.spans)
            .find(|span| span.content.starts_with("https://"))
            .expect("rendered link destination");
        assert!(destination.content.ends_with('\u{2026}'));
        assert_eq!(destination.style.fg, Some(theme::tertiary()));
    }
}
