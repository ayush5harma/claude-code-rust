// SPDX-License-Identifier: Apache-2.0

use crate::agent::model::{ContentBlock, ToolCallContent};
use crate::app::{App, FullscreenView, MessageBlock, MessageRole};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone)]
pub(crate) struct SearchRow {
    pub role: &'static str,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SearchMatch {
    pub row: usize,
    pub start: usize,
    pub end: usize,
}

pub(crate) struct SearchState {
    pub query: String,
    pub rows: Vec<SearchRow>,
    pub matches: Vec<SearchMatch>,
    pub selected: usize,
    session_id: Option<String>,
    revision: u64,
}

pub(crate) fn open(app: &mut App, query: &str) {
    app.search = Some(Box::new(SearchState {
        query: query.to_owned(),
        rows: Vec::new(),
        matches: Vec::new(),
        selected: 0,
        session_id: app.session_runtime.session_id.as_ref().map(|id| id.as_str().to_owned()),
        revision: u64::MAX,
    }));
    refresh(app);
    crate::app::view::set_fullscreen_view(app, FullscreenView::Search);
}

pub(crate) fn refresh(app: &mut App) {
    let Some(search) = app.search.as_mut() else { return };
    let current_session =
        app.session_runtime.session_id.as_ref().map(crate::agent::model::SessionId::as_str);
    if search.session_id.as_deref() != current_session {
        app.search = None;
        crate::app::view::set_chat_surface(app);
        return;
    }
    if search.revision == app.transcript.revision {
        return;
    }
    search.rows.clear();
    for message in &app.transcript.messages {
        let role = match message.role {
            MessageRole::User => "User",
            MessageRole::Assistant => "Claude",
            _ => continue,
        };
        for block in &message.blocks {
            match block {
                MessageBlock::Text(text) => push_lines(&mut search.rows, role, &text.text),
                MessageBlock::ToolCall(tool) | MessageBlock::ToolResult { tool, .. }
                    if !tool.hidden =>
                {
                    push_lines(&mut search.rows, "Tool", &tool.display().title);
                    for content in &tool.content {
                        if let ToolCallContent::Content(content) = content
                            && let ContentBlock::Text(text) = &content.content
                        {
                            push_lines(&mut search.rows, "Tool", &text.text);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    search.revision = app.transcript.revision;
    let selected = search.selected;
    recompute(search);
    search.selected = selected.min(search.matches.len().saturating_sub(1));
}

fn push_lines(rows: &mut Vec<SearchRow>, role: &'static str, text: &str) {
    for line in text.lines().filter(|line| !line.is_empty()) {
        rows.push(SearchRow { role, text: line.to_owned() });
    }
}

fn recompute(search: &mut SearchState) {
    search.matches.clear();
    search.selected = 0;
    if search.query.is_empty() {
        return;
    }
    let needle: Vec<char> = search.query.chars().flat_map(char::to_uppercase).collect();
    for (row, source) in search.rows.iter().enumerate() {
        let mut folded = Vec::new();
        let mut original_spans = Vec::new();
        for (start, ch) in source.text.char_indices() {
            let end = start + ch.len_utf8();
            for folded_ch in ch.to_uppercase() {
                folded.push(folded_ch);
                original_spans.push((start, end));
            }
        }
        for (offset, window) in folded.windows(needle.len()).enumerate() {
            if window == needle {
                let candidate = SearchMatch {
                    row,
                    start: original_spans[offset].0,
                    end: original_spans[offset + needle.len() - 1].1,
                };
                if search.matches.last() != Some(&candidate) {
                    search.matches.push(candidate);
                }
            }
        }
    }
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) {
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) => {
            app.search = None;
            crate::app::view::set_chat_surface(app);
        }
        (KeyCode::Enter | KeyCode::Down, KeyModifiers::NONE) => navigate(app, true),
        (KeyCode::Enter, KeyModifiers::SHIFT) | (KeyCode::Up, KeyModifiers::NONE) => {
            navigate(app, false);
        }
        (KeyCode::Backspace, KeyModifiers::NONE) => edit_query(app, |query| {
            query.pop();
        }),
        (KeyCode::Char(ch), modifiers)
            if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
        {
            edit_query(app, |query| query.push(ch));
        }
        _ => {}
    }
}

pub(crate) fn handle_paste(app: &mut App, text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    edit_query(app, |query| query.push_str(text));
    true
}

fn edit_query(app: &mut App, edit: impl FnOnce(&mut String)) {
    if let Some(search) = app.search.as_mut() {
        edit(&mut search.query);
        recompute(search);
    }
}

fn navigate(app: &mut App, next: bool) {
    let Some(search) = app.search.as_mut() else { return };
    let len = search.matches.len();
    if len > 0 {
        search.selected =
            if next { (search.selected + 1) % len } else { (search.selected + len - 1) % len };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model::{Content, TextContent, ToolCallStatus};
    use crate::app::{ChatMessage, TextBlock};

    #[test]
    fn unicode_queries_and_empty_or_missing_results_preserve_text() {
        let mut app = App::test_default();
        app.push_message_tracked(ChatMessage::new(
            MessageRole::User,
            vec![MessageBlock::Text(TextBlock::from_complete("Résumé κόσμος"))],
            None,
        ));
        open(&mut app, "ΚΌΣ");
        assert_eq!(app.search.as_ref().expect("search").matches.len(), 1);
        assert_eq!(app.search.as_ref().expect("search").rows[0].text, "Résumé κόσμος");
        edit_query(&mut app, String::clear);
        assert!(app.search.as_ref().expect("search").matches.is_empty());
        edit_query(&mut app, |query| query.push_str("absent"));
        assert!(app.search.as_ref().expect("search").matches.is_empty());
    }

    #[test]
    fn case_expansion_matches_original_character_spans_in_both_directions() {
        let mut app = App::test_default();
        app.push_message_tracked(ChatMessage::new(
            MessageRole::User,
            vec![MessageBlock::Text(TextBlock::from_complete("a Straße, SS, ﬃ, and FFI z"))],
            None,
        ));
        for (query, expected) in [("STRASSE", "Straße"), ("ß", "SS"), ("FFI", "ﬃ"), ("ﬃ", "FFI")]
        {
            open(&mut app, query);
            let search = app.search.as_ref().expect("search");
            assert!(
                search
                    .matches
                    .iter()
                    .any(|hit| { &search.rows[hit.row].text[hit.start..hit.end] == expected }),
                "missing {expected:?} for {query:?}"
            );
        }

        open(&mut app, "S");
        let search = app.search.as_ref().expect("search");
        let sharp_s_hits = search
            .matches
            .iter()
            .filter(|hit| &search.rows[hit.row].text[hit.start..hit.end] == "ß");
        assert_eq!(sharp_s_hits.count(), 1);
    }

    #[test]
    fn new_content_refreshes_and_session_change_closes_search() {
        let mut app = App::test_default();
        open(&mut app, "needle");
        app.push_message_tracked(ChatMessage::new(
            MessageRole::Assistant,
            vec![MessageBlock::Text(TextBlock::from_complete("a needle"))],
            None,
        ));
        refresh(&mut app);
        assert_eq!(app.search.as_ref().expect("search").matches.len(), 1);
        app.session_runtime.session_id = Some(crate::agent::model::SessionId::new("next"));
        refresh(&mut app);
        assert!(app.search.is_none());
    }

    #[test]
    fn searches_visible_tool_text_without_raw_input_or_hidden_tool_content() {
        let mut app = App::test_default();
        let mut visible =
            crate::app::test_support::tool_call_info("tool", ToolCallStatus::Completed);
        visible.title = "needle title".to_owned();
        visible.raw_input = Some(serde_json::json!({"token": "concealed"}));
        visible.content.push(ToolCallContent::Content(Content::new(ContentBlock::Text(
            TextContent::new("needle result"),
        ))));
        let mut hidden =
            crate::app::test_support::tool_call_info("hidden", ToolCallStatus::Completed);
        hidden.hidden = true;
        hidden.title = "concealed".to_owned();
        hidden.content.push(ToolCallContent::Content(Content::new(ContentBlock::Text(
            TextContent::new("concealed"),
        ))));
        app.push_message_tracked(ChatMessage::new(
            MessageRole::Assistant,
            vec![
                MessageBlock::ToolCall(Box::new(visible)),
                MessageBlock::ToolCall(Box::new(hidden)),
            ],
            None,
        ));
        open(&mut app, "needle");
        assert_eq!(app.search.as_ref().expect("search").matches.len(), 2);
        edit_query(&mut app, |query| *query = "concealed".to_owned());
        assert!(app.search.as_ref().expect("search").matches.is_empty());
    }

    #[test]
    fn evicted_history_is_not_searched() {
        let mut app = App::test_default();
        app.push_message_tracked(ChatMessage::new(
            MessageRole::User,
            vec![MessageBlock::Text(TextBlock::from_complete("needle from old history"))],
            None,
        ));
        app.history_retention.max_bytes = 1;
        app.enforce_history_retention_tracked();
        open(&mut app, "needle");
        assert!(app.search.as_ref().expect("search").matches.is_empty());
    }
}
