// SPDX-License-Identifier: Apache-2.0

mod document;

pub(super) use document::DocumentCursor;

use super::wrap::line_display_width;
use crate::failure;
use crate::ui::theme;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// App styles for Markdown, shared by the document layout and `tui-markdown`. Headings get a
/// three-step hierarchy: H1 in the assistant colour, bold and underlined; H2 bold; H3 and
/// below bold in the tertiary colour.
#[derive(Clone, Copy)]
struct MarkdownStyle;

impl tui_markdown::StyleSheet for MarkdownStyle {
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

impl MarkdownStyle {
    fn code_label() -> Style {
        Style::new().fg(theme::tertiary()).add_modifier(Modifier::ITALIC)
    }
}

pub(super) fn render_markdown_safe(text: &str, bg: Option<Color>) -> Vec<Line<'static>> {
    render_markdown_at_width(text, u16::MAX, bg, false)
}

/// Lays out `text` from one parse. `preserve_newlines` renders soft line breaks as hard breaks.
pub(super) fn render_markdown_at_width(
    text: &str,
    width: u16,
    bg: Option<Color>,
    preserve_newlines: bool,
) -> Vec<Line<'static>> {
    render_markdown_with_control(
        text,
        width,
        bg,
        preserve_newlines,
        &super::layout_control::LayoutControl::uninterrupted(),
    )
}

pub(super) fn render_markdown_with_control(
    text: &str,
    width: u16,
    bg: Option<Color>,
    preserve_newlines: bool,
    control: &super::layout_control::LayoutControl<'_>,
) -> Vec<Line<'static>> {
    render_markdown_safe_with(text, bg, |text, bg| {
        document::render(text, width, bg, preserve_newlines, control)
    })
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

fn render_with_tui_markdown(text: &str, width: u16, bg: Option<Color>) -> Vec<Line<'static>> {
    render_markdown_safe_with(text, bg, |text, bg| {
        let rendered = tui_markdown::from_str_with_options(
            text,
            &tui_markdown::Options::new(MarkdownStyle).table_width(width),
        );
        let lines = rendered
            .lines
            .into_iter()
            .map(|line| {
                let owned_spans: Vec<Span<'static>> = line
                    .spans
                    .into_iter()
                    .map(|span| {
                        let style = if let Some(bg_color) = bg {
                            span.style.bg(bg_color)
                        } else {
                            span.style
                        };
                        Span::styled(span.content.into_owned(), style)
                    })
                    .collect();
                let line_style =
                    if let Some(bg_color) = bg { line.style.bg(bg_color) } else { line.style };
                Line::from(owned_spans).style(line_style)
            })
            .collect();
        shorten_link_destinations(lines)
    })
}

/// `tui-markdown` writes a link as its label, ` (`, the destination and `)`.
fn shorten_link_destinations(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    for line in &mut lines {
        for span_idx in 1..line.spans.len() {
            if line.spans[span_idx - 1].content.as_ref() != " (" {
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
    shortened.push('\u{2026}');
    shortened
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
    use super::*;
    use std::panic::catch_unwind;

    fn rendered_text(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(line_text).collect()
    }

    /// The indent, rail and gutter in front of every row of a shaded code block.
    const CODE_ROW: &str = "  \u{258e} ";

    fn code_text(row: &str) -> &str {
        row.strip_prefix(CODE_ROW).unwrap_or(row)
    }

    fn document_rows(source: &str, width: u16) -> Vec<String> {
        let lines = render_markdown_at_width(source, width, None, false);
        crate::ui::wrap::wrap_lines_to_physical_rows(&lines, width)
            .iter()
            .map(|line| line_text(line).trim_end().to_owned())
            .collect()
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
            // Code sits behind its own indent and rail; nothing else may be added before it.
            let content = code_text(content);
            assert!(
                !content.starts_with("  "),
                "non-list source acquired list padding: {source:?}: {rows:?}"
            );
        }
    }

    #[test]
    fn nested_list_wrapping_preserves_inline_styles_links_and_unicode() {
        let lines = render_markdown_at_width(
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
        let physical = crate::ui::wrap::wrap_lines_to_physical_rows(&lines, 28);
        assert!(physical.iter().all(|line| crate::ui::wrap::line_display_width(line) <= 28));
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style.bg == Some(Color::Blue))
        );
        let child = text.iter().position(|row| row.contains("- Bold")).expect("nested item");
        let parent_column = text[0].find("1.").expect("parent marker");
        assert!(text[child].find('-').unwrap() > parent_column, "nesting lost: {text:?}");
    }

    #[test]
    fn table_and_code_inside_a_list_preserve_cells_and_literal_code_lines() {
        let rows = document_rows(
            "1. Parent\n\n   | Name | Value |\n   | --- | --- |\n   | Alpha | Beta |\n\n   ~~~text\n   - literal code\n       indented code\n   ~~~\n\n   Tail paragraph.",
            60,
        );
        let header = rows.iter().find(|row| row.contains("Name")).expect("table header");
        let data = rows.iter().find(|row| row.contains("Alpha")).expect("table row");
        assert!(header.contains("Value") && data.contains("Beta"), "lost cells: {rows:?}");
        assert_eq!(header.find('│'), data.find('│'), "table columns shifted: {rows:?}");
        assert!(rows.iter().any(|row| row.trim() == "Tail paragraph."));
        let code = rows.iter().position(|row| row == "- literal code").expect("literal code line");
        assert_eq!(
            rows[code + 1],
            "    indented code",
            "literal code lines or indentation were lost: {rows:?}"
        );
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

        assert_eq!(rendered_text(&lines), vec!["Intro", "", "- One", "- Two", "", "Outro"]);
    }

    #[test]
    fn ordered_lists_preserve_surrounding_paragraph_separators() {
        let lines = render_markdown_safe("Intro\n\n1. One\n2. Two\n\nOutro", None);

        assert_eq!(rendered_text(&lines), vec!["Intro", "", "1. One", "2. Two", "", "Outro"]);
    }

    #[test]
    fn text_after_a_list_returns_to_the_normal_column_without_a_blank_line() {
        let lines = render_markdown_at_width("Intro\n1. One\n2. Two\nOutro", 40, None, true);
        let rows = rendered_text(&lines);
        let last_item = rows.iter().position(|row| row == "2. Two").expect("last list item");
        assert_eq!(rows[last_item + 1], "Outro", "trailing text acquired list indentation");
        assert_eq!(rows.first().map(String::as_str), Some("Intro"));
    }

    #[test]
    fn fenced_code_keeps_list_like_lines_unchanged() {
        let lines = render_markdown_safe("```md\n- Not a rendered list\n```", None);

        assert_eq!(
            rendered_text(&lines),
            vec!["  \u{258e} md", "  \u{258e} - Not a rendered list"]
        );
    }

    #[test]
    fn headings_render_bold_without_markers() {
        let lines = render_markdown_safe("# Title\n\nBody\n\n## Section", None);
        let text = rendered_text(&lines);

        for heading in ["Title", "Section"] {
            let line = lines.iter().find(|line| line_text(line) == heading).expect("heading row");
            assert!(
                line.spans.iter().all(|span| line
                    .style
                    .patch(span.style)
                    .add_modifier
                    .contains(Modifier::BOLD)),
                "{heading} lost heading style: {text:?}"
            );
        }
        assert!(!text.iter().any(|line| line.contains('#')), "{text:?}");
    }

    #[test]
    fn fenced_code_renders_hash_lines_verbatim_under_a_language_label() {
        let lines = render_markdown_safe(
            "Intro\n\n```bash\n#!/usr/bin/env bash\n# install deps\n  npm ci\n```\n\nAfter",
            None,
        );

        assert_eq!(
            rendered_text(&lines),
            vec![
                "Intro",
                "",
                "  \u{258e} bash",
                "  \u{258e} #!/usr/bin/env bash",
                "  \u{258e} # install deps",
                "  \u{258e}   npm ci",
                "",
                "After"
            ]
        );
        let label = lines[2].spans.iter().find(|span| span.content == "bash").expect("label span");
        // The label sits on the code block's fill.
        let fill = crate::ui::highlight::code_background();
        assert_eq!(
            label.style,
            fill.map_or(MarkdownStyle::code_label(), |bg| MarkdownStyle::code_label().bg(bg))
        );
    }

    #[test]
    fn unlabelled_and_indented_code_use_a_generic_label() {
        for source in ["```\nplain\n```", "    plain"] {
            let lines = render_markdown_safe(source, None);
            assert_eq!(
                rendered_text(&lines),
                vec!["  \u{258e} code", "  \u{258e} plain"],
                "{source:?}"
            );
        }
    }

    #[test]
    fn quoted_code_wraps_inside_the_quote_column() {
        let code = "let value = compute_something_long(alpha, beta);";
        let rows = document_rows(&format!("> ```rust\n> {code}\n> ```"), 24);

        assert_eq!(rows[0], format!("> {CODE_ROW}rust"));
        let body = &rows[1..];
        assert!(body.len() > 1, "code must wrap at the available width: {rows:?}");
        for row in body {
            assert!(row.starts_with("> "), "wrapped code escaped its quote: {rows:?}");
            assert!(crate::ui::wrap::display_width(row) <= 24);
        }
        let joined = body.iter().map(|row| code_text(&row[2..])).collect::<String>();
        assert_eq!(joined.replace(' ', ""), code.replace(' ', ""));
    }

    #[test]
    fn preserved_newlines_become_line_breaks_outside_code() {
        let lines = render_markdown_at_width(
            "first line\nsecond line\n\n- item one\n  continued\n\n```sh\necho a\necho b\n```",
            80,
            None,
            true,
        );
        let text = rendered_text(&lines);

        for row in ["first line", "second line", "- item one", "continued", "echo a", "echo b"] {
            let found = text.iter().any(|line| code_text(line).trim_end() == row);
            assert!(found, "missing {row:?}: {text:?}");
        }
        // Prose gets no hard-break spaces; code rows are padded to the row end on purpose.
        let prose = text.iter().filter(|line| !line.starts_with(CODE_ROW));
        assert!(prose.clone().all(|line| !line.ends_with(' ')), "trailing spaces: {text:?}");
    }

    #[test]
    fn list_markers_preserve_requested_background() {
        let lines = render_markdown_safe("- One", Some(Color::Blue));

        assert_eq!(rendered_text(&lines), vec!["- One"]);
        assert_eq!(lines[0].spans[0].style.bg, Some(Color::Blue));
        assert!(lines[0].spans.iter().all(|span| span.style.bg == Some(Color::Blue)));
    }

    fn line_is_blank(line: &Line<'_>) -> bool {
        line.spans.iter().all(|span| span.content.trim().is_empty())
    }

    #[test]
    fn headings_have_distinct_hierarchy_styles() {
        let lines = render_markdown_safe("# One\n\n## Two\n\n### Three", None);

        let styles: Vec<_> = lines
            .iter()
            .filter(|line| !line_is_blank(line))
            .map(|line| line.spans.iter().fold(line.style, |style, span| style.patch(span.style)))
            .collect();
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
        assert!(destination.content.ends_with('\u{2026}'));
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
