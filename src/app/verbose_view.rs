// SPDX-License-Identifier: Apache-2.0

use super::{App, FullscreenView, MessageBlock, MessageRole, SurfaceMode, view};
use crate::agent::model::{ContentBlock, ToolCallContent};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const MAX_ROWS: usize = 5000;

pub(crate) struct VerboseView {
    pub rows: Vec<String>,
    pub scroll: usize,
    pub truncated: bool,
}

pub(crate) fn open(app: &mut App) -> bool {
    let mut rows = Vec::new();
    let mut truncated = false;
    for message in &app.transcript.messages {
        let role = match message.role {
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
            _ => continue,
        };
        push_row(&mut rows, &mut truncated, &format!("{role}:"));
        for block in &message.blocks {
            match block {
                MessageBlock::Text(text) => push_text(&mut rows, &mut truncated, &text.text),
                MessageBlock::ToolCall(tool) | MessageBlock::ToolResult { tool, .. }
                    if !tool.hidden =>
                {
                    let tool = tool.display();
                    push_row(&mut rows, &mut truncated, &format!("  Tool: {}", tool.title));
                    // Measured 2026-10-13: Bash carries the same result in both
                    // fields, so terminal output replaces content text.
                    if let Some(output) = &tool.terminal_output {
                        push_text(&mut rows, &mut truncated, output);
                    } else {
                        for content in &tool.content {
                            if let ToolCallContent::Content(content) = content
                                && let ContentBlock::Text(text) = &content.content
                            {
                                push_text(&mut rows, &mut truncated, &text.text);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        push_row(&mut rows, &mut truncated, "");
    }
    app.verbose_view = Some(Box::new(VerboseView { rows, scroll: 0, truncated }));
    view::set_fullscreen_view(app, FullscreenView::Verbose);
    true
}

fn push_text(rows: &mut Vec<String>, truncated: &mut bool, text: &str) {
    for line in text.lines() {
        push_row(rows, truncated, line);
    }
}

fn push_row(rows: &mut Vec<String>, truncated: &mut bool, row: &str) {
    if rows.len() >= MAX_ROWS {
        *truncated = true;
        return;
    }
    rows.push(row.chars().map(|ch| if ch.is_control() && ch != '\t' { ' ' } else { ch }).collect());
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc
        || app.keymap.resolve_event(super::keymap::KeyContext::ChatInput, key).is_some_and(
            |binding| {
                binding.action
                    == super::keymap::KeyAction::App(super::keymap::AppAction::ToggleVerbose)
            },
        )
    {
        app.verbose_view = None;
        view::set_chat_surface(app);
        return;
    }
    let Some(state) = app.verbose_view.as_mut() else { return };
    if key.modifiers != KeyModifiers::NONE {
        return;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => state.scroll = state.scroll.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => {
            state.scroll = (state.scroll + 1).min(state.rows.len().saturating_sub(1));
        }
        KeyCode::PageUp => state.scroll = state.scroll.saturating_sub(20),
        KeyCode::PageDown => {
            state.scroll = (state.scroll + 20).min(state.rows.len().saturating_sub(1));
        }
        KeyCode::Home => state.scroll = 0,
        KeyCode::End => state.scroll = state.rows.len().saturating_sub(1),
        _ => {}
    }
}

pub(crate) fn reset_for_session_change(app: &mut App) {
    app.verbose_view = None;
    if app.surface_mode == SurfaceMode::Fullscreen(FullscreenView::Verbose) {
        view::set_chat_surface(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model;
    use crate::app::{ChatMessage, TextBlock};

    #[test]
    fn detailed_view_uses_visible_text_without_raw_or_hidden_fields() {
        let mut app = App::test_default();
        let mut visible =
            crate::app::test_support::tool_call_info("visible", model::ToolCallStatus::Completed);
        visible.raw_input = Some(serde_json::json!({"secret": "raw-private-value"}));
        visible.content.push("visible-result".into());
        let mut hidden =
            crate::app::test_support::tool_call_info("hidden", model::ToolCallStatus::Completed);
        hidden.hidden = true;
        hidden.content.push("hidden-private-value".into());
        app.transcript.messages.push(ChatMessage::new(
            MessageRole::Assistant,
            vec![
                MessageBlock::Text(TextBlock::from_complete("answer")),
                MessageBlock::ToolCall(Box::new(visible)),
                MessageBlock::ToolCall(Box::new(hidden)),
            ],
            None,
        ));
        assert!(open(&mut app));
        let text = app.verbose_view.as_ref().expect("view").rows.join("\n");
        assert!(text.contains("answer") && text.contains("visible-result"));
        assert!(!text.contains("raw-private-value") && !text.contains("hidden-private-value"));
    }
    #[test]
    fn terminal_output_replaces_the_same_text_in_content_and_content_stands_alone() {
        let mut app = App::test_default();
        let mut bash =
            crate::app::test_support::tool_call_info("bash", model::ToolCallStatus::Completed);
        bash.sdk_tool_name = "Bash".to_owned();
        bash.content.push("shared-output".into());
        bash.terminal_output = Some("shared-output".to_owned());
        let mut read =
            crate::app::test_support::tool_call_info("read", model::ToolCallStatus::Completed);
        read.content.push("file-body".into());
        app.transcript.messages.push(ChatMessage::new(
            MessageRole::Assistant,
            vec![MessageBlock::ToolCall(Box::new(bash)), MessageBlock::ToolCall(Box::new(read))],
            None,
        ));
        assert!(open(&mut app));
        let rows = &app.verbose_view.as_ref().expect("view").rows;
        assert_eq!(rows.iter().filter(|row| *row == "shared-output").count(), 1);
        assert_eq!(rows.iter().filter(|row| *row == "file-body").count(), 1);
    }
}
