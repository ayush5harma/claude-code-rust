// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

//! Body of a successful text Read: one source line per row behind a dim line-number gutter.

use crate::agent::model;
use crate::app::ToolCallInfo;
use crate::ui::diff::lang_from_title;
use crate::ui::highlight;
use crate::ui::theme;
use crate::ui::wrap::wrap_lines_to_physical_rows;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::standard::{READ_SYNTAX_THEME, cap_read_items, read_hidden_marker};

const GUTTER_SEPARATOR: &str = "  ";
/// A terminal cell grid has no tab stops; without this a tab would take no width at all.
const TAB_REPLACEMENT: &str = "    ";

enum ReadEntry {
    Source { number: u32, text: Line<'static> },
    Marker(Line<'static>),
}

/// Render `file` as gutter + text rows no wider than `width`. The head/tail cap applies to
/// source lines and runs before wrapping, so a long line costs rows, not source lines.
/// Wrapped rows hang under the text column instead of under the line numbers.
pub(super) fn render_file_text(
    tc: &ToolCallInfo,
    file: &model::FileText,
    width: u16,
) -> Vec<Line<'static>> {
    // File bytes are untrusted: strip control sequences and never colour from them.
    let clean = highlight::strip_ansi(&file.text).replace('\t', TAB_REPLACEMENT);
    let title_lang = lang_from_title(&tc.title);
    let highlighted = highlight::highlight_code_for_path_with_theme(
        &clean,
        &file.path,
        (!title_lang.is_empty()).then_some(title_lang.as_str()),
        READ_SYNTAX_THEME,
    );

    let source_count = clean.lines().count();
    let last_number = file
        .start_line
        .saturating_add(u32::try_from(source_count.saturating_sub(1)).unwrap_or(u32::MAX));
    let gutter_width = last_number.to_string().len();
    let prefix_width = gutter_width + GUTTER_SEPARATOR.len();
    let text_width = width.saturating_sub(u16::try_from(prefix_width).unwrap_or(u16::MAX)).max(1);

    let entries = highlighted
        .into_iter()
        .take(source_count)
        .zip(file.start_line..)
        .map(|(text, number)| ReadEntry::Source { number, text })
        .collect::<Vec<_>>();
    let entries = cap_read_items(entries, |omitted| ReadEntry::Marker(read_hidden_marker(omitted)));

    let gutter_style = Style::default().fg(theme::tertiary());
    let continuation = " ".repeat(prefix_width);
    let mut rows = Vec::new();
    for entry in entries {
        match entry {
            ReadEntry::Marker(marker) => {
                rows.extend(wrap_lines_to_physical_rows(&[marker], width));
            }
            ReadEntry::Source { number, text } => {
                let mut wrapped = wrap_lines_to_physical_rows(&[text], text_width).into_iter();
                let first = wrapped.next().unwrap_or_default();
                let mut spans = vec![
                    Span::styled(format!("{number:>gutter_width$}"), gutter_style),
                    Span::styled(GUTTER_SEPARATOR, gutter_style),
                ];
                spans.extend(first.spans);
                rows.push(Line::from(spans));
                rows.extend(wrapped.map(|row| {
                    let mut spans = vec![Span::raw(continuation.clone())];
                    spans.extend(row.spans);
                    Line::from(spans)
                }));
            }
        }
    }
    rows
}
