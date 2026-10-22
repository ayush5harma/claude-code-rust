// SPDX-License-Identifier: Apache-2.0

//! The render corpus as an `App`: `tests/fixtures/render-corpus.json` replayed through the
//! app's own event path.
//!
//! The JSON is shared with the PTY scenario `render-corpus` in `fake-bridge.js`; it holds
//! bridge `session_update` payloads, so they are decoded with the wire types and mapped by
//! `map_session_update` exactly as `event_dispatch` does for a live bridge. `history` goes
//! in through `ClientEvent::Connected` (the resume path, which is also how user messages
//! reach the transcript), and `stream` is applied update by update through
//! `handle_client_event`, as a live reply would be. Nothing here builds a rendered string.

use super::type_converters::map_session_update;
use crate::agent::events::ClientEvent;
use crate::agent::{model, types};
use crate::app::{App, handle_client_event};

const FIXTURE: &str = include_str!("../../../tests/fixtures/render-corpus.json");
const SESSION_ID: &str = "render-corpus";

pub(crate) struct RenderCorpus {
    history: Vec<model::SessionUpdate>,
    stream: Vec<model::SessionUpdate>,
}

impl RenderCorpus {
    pub(crate) fn load() -> Self {
        let mut fixture: serde_json::Value =
            serde_json::from_str(FIXTURE).expect("render corpus is valid JSON");
        Self {
            history: decode_updates(&mut fixture, "history"),
            stream: decode_updates(&mut fixture, "stream"),
        }
    }

    /// An app that has resumed the corpus conversation: welcome banner, user messages,
    /// assistant Markdown, tool calls, notices.
    pub(crate) fn app_with_history(&self) -> App {
        let mut app = App::test_default();
        handle_client_event(
            &mut app,
            ClientEvent::Connected {
                session_id: model::SessionId::new(SESSION_ID),
                cwd: "/test".to_owned(),
                current_model: model::CurrentModel::new("fixture", "fixture", "fixture")
                    .authoritative(true),
                available_models: Vec::new(),
                mode: None,
                fast_mode_state: model::FastModeState::Off,
                fast_mode_disabled_reason: None,
                ultracode: None,
                history_updates: self.history.clone(),
            },
        );
        app
    }

    /// The whole conversation: the history plus the streamed reply, finished.
    pub(crate) fn finished_app(&self) -> App {
        let mut app = self.app_with_history();
        for update in &self.stream {
            Self::apply_stream_update(&mut app, update);
        }
        Self::complete_turn(&mut app);
        app
    }

    /// The live reply that follows the history, one bridge update per element.
    pub(crate) fn stream(&self) -> &[model::SessionUpdate] {
        &self.stream
    }

    pub(crate) fn apply_stream_update(app: &mut App, update: &model::SessionUpdate) {
        handle_client_event(
            app,
            ClientEvent::SessionUpdate {
                session_id: SESSION_ID.to_owned(),
                update: update.clone(),
            },
        );
    }

    pub(crate) fn complete_turn(app: &mut App) {
        handle_client_event(
            app,
            ClientEvent::TurnComplete {
                session_id: SESSION_ID.to_owned(),
                queued_turn_count: None,
                terminal_reason: None,
            },
        );
    }
}

fn decode_updates(fixture: &mut serde_json::Value, key: &str) -> Vec<model::SessionUpdate> {
    let raw = fixture.get_mut(key).expect("render corpus has the section").take();
    let wire: Vec<types::SessionUpdate> =
        serde_json::from_value(raw).expect("render corpus updates match the bridge wire types");
    wire.into_iter().filter_map(map_session_update).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model::ToolCallStatus::{Completed, Failed};
    use crate::app::{AppStatus, MessageBlock, MessageRole};
    use crate::ui::inline_chat_rows::serialize_live_rows_with_boundaries_excluding;

    fn rendered_text(app: &mut App, width: u16) -> Vec<String> {
        serialize_live_rows_with_boundaries_excluding(
            app,
            width,
            &std::collections::BTreeSet::new(),
            None,
        )
        .rows()
        .iter()
        .map(|row| row.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .collect()
    }

    #[test]
    fn history_carries_each_block_kind_the_chat_renders() {
        let app = RenderCorpus::load().app_with_history();
        let messages = &app.transcript.messages;
        assert_eq!(
            messages.iter().filter(|message| matches!(message.role, MessageRole::User)).count(),
            2
        );

        let mut tools = Vec::new();
        let mut diffs = Vec::new();
        let mut notices = 0;
        for block in messages.iter().flat_map(|message| &message.blocks) {
            match block {
                MessageBlock::ToolCall(tool) => {
                    tools.push((tool.sdk_tool_name.as_str(), tool.status));
                    for content in &tool.content {
                        if let model::ToolCallContent::Diff(diff) = content {
                            let old = diff.old_text.as_deref().unwrap_or("").lines().count();
                            diffs.push((old, diff.new_text.lines().count()));
                        }
                    }
                }
                MessageBlock::Notice(_) => notices += 1,
                _ => {}
            }
        }
        assert_eq!(
            tools,
            [
                ("Bash", Completed),
                ("Edit", Completed),
                ("Write", Completed),
                ("Write", Completed),
                ("Edit", Completed),
                ("Bash", Failed),
            ]
        );
        assert_eq!(diffs, [(1, 1), (0, 3), (0, 500), (250, 250)]);
        assert_eq!(notices, 2);

        let build = messages
            .iter()
            .flat_map(|message| &message.blocks)
            .find_map(|block| match block {
                MessageBlock::ToolCall(tool) if tool.id == "toolu_build" => Some(tool),
                _ => None,
            })
            .expect("build tool call");
        let output = build.terminal_output.as_deref().expect("build output");
        assert_eq!(output.lines().count(), 2000);
        assert!(output.contains("\u{1b}[1;32m   Compiling\u{1b}[0m"), "ANSI colour is kept");
    }

    #[test]
    fn stream_is_a_long_reply_in_about_two_hundred_chunks() {
        let corpus = RenderCorpus::load();
        let chunks = corpus
            .stream()
            .iter()
            .filter(|update| matches!(update, model::SessionUpdate::AgentMessageChunk(_)))
            .count();
        assert_eq!(chunks, 200);

        let mut app = corpus.app_with_history();
        let history_messages = app.transcript.messages.len();
        for update in corpus.stream() {
            RenderCorpus::apply_stream_update(&mut app, update);
        }
        assert_eq!(app.transcript.messages.len(), history_messages + 1);
        assert_eq!(app.status, AppStatus::Running, "the reply is still streaming");
        RenderCorpus::complete_turn(&mut app);
        assert_eq!(app.status, AppStatus::Ready);
    }

    #[test]
    fn rendered_transcript_shows_every_kind_of_content() {
        let mut app = RenderCorpus::load().finished_app();
        let rows = rendered_text(&mut app, 120);
        let has = |needle: &str| rows.iter().any(|row| row.contains(needle));

        // Markdown: heading, table cell, quote, nested list, link target, long paragraph.
        for needle in [
            "Transcript renderer overview",
            "Parse Markdown",
            "> The tail block is re-rendered on every chunk.",
            "- Added lines are green, removed lines are red, context is dim.",
            "https://example.com/docs/render",
            "is re-rendered on every frame while a reply streams",
        ] {
            assert!(has(needle), "missing Markdown: {needle}");
        }
        // One distinctive line from each fenced language.
        for needle in [
            "pub fn word_counts",
            "export function groupByTag",
            "class Span:",
            "set -euo pipefail",
            "\"widths\": [80, 120, 200]",
            "+    let limit = width.saturating_sub(1).max(1);",
        ] {
            assert!(has(needle), "missing fenced code: {needle}");
        }
        // Tools: output is capped, diffs carry their counts, the failure shows its error.
        for needle in [
            "1992 lines hidden",
            "(+500)",
            "(+250, -250)",
            "unresolved import `crate::ui::render_cache`",
            "Context is 62% full",
            "Good catch. Keep the generated file",
            "That is the full picture.",
        ] {
            assert!(has(needle), "missing: {needle}");
        }
    }
}
