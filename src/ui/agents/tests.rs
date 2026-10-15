// SPDX-License-Identifier: Apache-2.0

use crate::agent::client::{AgentConnection, CommandReceiver};
use crate::agent::model::{SessionId, TaskItem, TaskStatus, ToolCallStatus};
use crate::agent::types::{SubagentMessage, SubagentMessageRole};
use crate::app::agents::{TranscriptView, apply_transcript_list, handle_key, open};
use crate::app::{App, ChatMessage, MessageBlock, MessageRole, ToolCallScope};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::rc::Rc;

fn screen(app: &mut App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal.draw(|frame| super::render(frame, app)).expect("draw");
    let buffer = terminal.backend().buffer();
    buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .map(|row| row.iter().map(ratatui::buffer::Cell::symbol).collect::<String>())
        .collect()
}

fn row_with<'a>(lines: &'a [String], needle: &str) -> (usize, &'a str) {
    let Some(index) = lines.iter().position(|line| line.contains(needle)) else {
        panic!("no row contains {needle:?}:\n{}", lines.join("\n"));
    };
    (index, lines[index].as_str())
}

fn press(app: &mut App, code: KeyCode) {
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE));
}

fn subagent(
    id: &str,
    subject: &str,
    status: TaskStatus,
    mut metadata: serde_json::Value,
) -> TaskItem {
    let mut task = TaskItem::new(id, subject, status);
    metadata["task_type"] = serde_json::json!("local_agent");
    task.metadata = Some(metadata);
    task
}

/// Puts the Agent tool call `parent` (with `input`) and one child Read per file,
/// oldest first, in the transcript.
fn agent_call(app: &mut App, parent: &str, input: serde_json::Value, files: &[&str]) {
    let mut call = crate::app::test_support::tool_call_info(parent, ToolCallStatus::InProgress);
    call.sdk_tool_name = "Agent".to_owned();
    call.raw_input = Some(input);
    let mut blocks = vec![MessageBlock::ToolCall(Box::new(call))];
    let child_ids: Vec<String> =
        (0..files.len()).map(|index| format!("{parent}-child-{index}")).collect();
    for (child_id, file) in child_ids.iter().zip(files) {
        let mut child =
            crate::app::test_support::tool_call_info(child_id, ToolCallStatus::Completed);
        child.raw_input = Some(serde_json::json!({ "file_path": file }));
        blocks.push(MessageBlock::ToolCall(Box::new(child)));
    }
    let index = app.transcript.messages.len();
    app.transcript.messages.push(ChatMessage::new(MessageRole::Assistant, blocks, None));
    app.index_tool_call(parent.to_owned(), index, 0);
    for (offset, child_id) in child_ids.into_iter().enumerate() {
        app.index_tool_call(child_id.clone(), index, offset + 1);
        app.register_tool_call_scope(
            child_id,
            ToolCallScope::SubagentChild { parent_tool_use_id: parent.to_owned() },
        );
    }
}

fn connected_app() -> (App, CommandReceiver) {
    let mut app = App::test_default();
    let (conn, commands) = AgentConnection::test_channel();
    app.session_runtime.conn = Some(Rc::new(conn));
    app.session_runtime.session_id = Some(SessionId::new("session-1"));
    (app, commands)
}

/// A running builder with a long prompt and a child Read, a completed reviewer, a stopped search.
fn app_with_subagents() -> (App, CommandReceiver) {
    let (mut app, commands) = connected_app();
    let prompt = (1..=10)
        .map(|n| format!("Prompt line {n} explains the work in enough words to wrap."))
        .collect::<Vec<_>>()
        .join(" ");
    agent_call(
        &mut app,
        "parent-builder",
        serde_json::json!({ "name": "builder", "model": "opus", "isolation": "worktree", "prompt": prompt }),
        &["/src/model.rs", "/src/agents.rs"],
    );
    agent_call(
        &mut app,
        "parent-reviewer",
        serde_json::json!({ "name": "reviewer", "prompt": "Review the parser." }),
        &["/src/parser.rs"],
    );
    let mut builder = subagent(
        "agent-1",
        "Build the inspector",
        TaskStatus::InProgress,
        serde_json::json!({
            "model_display": "Opus 5.5",
            "usage": { "tool_uses": 12, "total_tokens": 34200, "duration_ms": 62000 },
        }),
    );
    builder.source_tool_call_id = Some("parent-builder".to_owned());
    let mut reviewer = subagent(
        "agent-2",
        "Review the parser",
        TaskStatus::Completed,
        serde_json::json!({ "model_display": "Sonnet 5.5", "terminal_status": "completed", "summary": "Parser looks fine" }),
    );
    reviewer.source_tool_call_id = Some("parent-reviewer".to_owned());
    app.sdk_inventory.tasks = vec![
        builder,
        TaskItem::new("shell-1", "Background shell", TaskStatus::InProgress),
        reviewer,
        subagent(
            "agent-3",
            "Abandoned search",
            TaskStatus::Completed,
            serde_json::json!({ "terminal_status": "killed" }),
        ),
    ];
    open(&mut app);
    (app, commands)
}

#[test]
fn the_selected_agent_shows_its_model_choice_usage_activity_prompt_and_transcript_below_the_list() {
    let (mut app, _commands) = app_with_subagents();
    apply_transcript_list(&mut app, vec!["agent-2".to_owned()], None);
    press(&mut app, KeyCode::Char('s'));
    let lines = screen(&mut app, 80, 24);

    let (title, title_line) = row_with(&lines, "Agents");
    assert!(title_line.contains("1/3"), "{title_line}");
    assert!(title_line.contains("1 running"), "a stopping agent still runs: {title_line}");
    assert!(!lines.iter().any(|line| line.contains("Background shell")), "only subagents");

    let (builder, builder_line) = row_with(&lines, "builder");
    assert!(builder > title);
    assert!(builder_line.contains('›') && builder_line.contains("Opus 5.5"), "{builder_line}");
    let (reviewer, reviewer_line) = row_with(&lines, "reviewer");
    assert!(reviewer > builder && !reviewer_line.contains('›'), "{reviewer_line}");
    assert!(reviewer_line.contains('✓') && reviewer_line.contains("Sonnet 5.5"), "{reviewer_line}");

    // The details follow the list and describe the selected builder only.
    let (header, header_line) = row_with(&lines, " stopping ");
    assert!(header > reviewer + 1, "details sit below the list:\n{}", lines.join("\n"));
    assert!(header_line.contains("Build the inspector"), "{header_line}");
    assert!(header_line.contains("1m02s"), "{header_line}");
    assert!(
        lines[header + 1].contains("builder · Opus 5.5 · asked for opus · isolated worktree"),
        "{}",
        lines[header + 1]
    );
    assert!(lines[header + 2].contains("12 tools · 34.2k tokens"), "{}", lines[header + 2]);
    let (_, transcript) = row_with(&lines, "Transcript");
    assert!(transcript.contains("not saved yet"), "{transcript}");
    // Now is the latest child tool; Recent lists the earlier ones without repeating it.
    let (_, now) = row_with(&lines, "Now");
    assert!(now.contains("Read agents.rs"), "{now}");
    let (recent, recent_line) = row_with(&lines, "Recent");
    assert!(recent_line.contains("Read model.rs"), "{recent_line}");
    assert_eq!(
        lines[header..].iter().filter(|line| line.contains("Read agents.rs")).count(),
        1,
        "{}",
        lines.join("\n")
    );
    assert!(
        !lines[header..].iter().any(|line| line.contains("parser.rs")),
        "another agent's tools"
    );
    let (prompt, prompt_line) = row_with(&lines, "Prompt");
    assert!(prompt > recent && prompt_line.contains("Prompt line 1"), "{prompt_line}");

    let (hint_row, _) = row_with(&lines, "Up/Down select");
    assert!(hint_row > prompt, "the hint row follows the details");
    let hint = lines[hint_row..].join(" ");
    for key in ["Enter transcript", "s stop", "r refresh", "Esc", "close"] {
        assert!(hint.contains(key), "{hint}");
    }

    press(&mut app, KeyCode::Down);
    let lines = screen(&mut app, 80, 24);
    let (_, header_line) = row_with(&lines, " completed ");
    assert!(header_line.contains("Review the parser"), "{header_line}");
    let (_, transcript) = row_with(&lines, "Transcript");
    assert!(transcript.contains("saved · Enter to read"), "{transcript}");
    let (reviewer_header, _) = row_with(&lines, " completed ");
    let (_, now) = row_with(&lines, "Now");
    assert!(now.contains("Read parser.rs"), "{now}");
    assert!(
        !lines[reviewer_header..].iter().any(|line| line.contains("Recent")),
        "a single child tool is only Now:\n{}",
        lines.join("\n")
    );
}

#[test]
fn wide_screens_put_the_details_beside_the_list() {
    let (mut app, _commands) = app_with_subagents();
    let lines = screen(&mut app, 140, 30);
    let (row, builder_line) = row_with(&lines, "› ● builder");
    let header_column = builder_line.find("Build the inspector").expect("details beside the row");
    assert!(header_column > 60, "{builder_line}");
    assert!(lines[row + 1].contains("reviewer"), "{}", lines[row + 1]);
    assert!(lines[row + 1].contains("asked for opus"), "{}", lines[row + 1]);
}

#[test]
fn a_short_screen_keeps_the_list_and_scrolls_the_details() {
    let (mut app, _commands) = app_with_subagents();
    let lines = screen(&mut app, 80, 14);
    row_with(&lines, "› ● builder");
    row_with(&lines, "builder · Opus 5.5");
    assert!(!lines.iter().any(|line| line.contains("Prompt line 1")), "{}", lines.join("\n"));

    press(&mut app, KeyCode::PageDown);
    let scrolled = screen(&mut app, 80, 14);
    row_with(&scrolled, "› ● builder");
    row_with(&scrolled, "Prompt line");
    assert!(!scrolled.iter().any(|line| line.contains("builder · Opus 5.5")));

    // The offset is clamped to the details, so one PageUp returns to the top.
    for _ in 0..20 {
        press(&mut app, KeyCode::PageDown);
    }
    screen(&mut app, 80, 14);
    press(&mut app, KeyCode::PageUp);
    row_with(&screen(&mut app, 80, 14), "builder · Opus 5.5");
}

#[test]
fn transcript_replaces_the_list_in_the_same_screen_and_scrolls() {
    let (mut app, _commands) = app_with_subagents();
    let message = |role, text: &str| SubagentMessage { role, text: text.to_owned() };
    let mut messages = vec![
        message(SubagentMessageRole::User, "Inspect the parser"),
        message(SubagentMessageRole::Tool, "ls parser"),
    ];
    messages
        .extend((1..=30).map(|n| message(SubagentMessageRole::Assistant, &format!("finding {n}"))));
    app.agents.transcript = Some(TranscriptView {
        agent_id: "agent-2".to_owned(),
        messages: Some(messages),
        ..TranscriptView::default()
    });

    let lines = screen(&mut app, 60, 20);
    let (_, title) = row_with(&lines, "Agents ›");
    assert!(title.contains("Review the parser"), "{title}");
    assert!(!lines.iter().any(|line| line.contains("Build the inspector")), "the list is replaced");
    let (prompt, _) = row_with(&lines, "Prompt");
    assert!(lines[prompt + 1].contains("Inspect the parser"));
    row_with(&lines, "• ls parser");
    let (_, hint) = row_with(&lines, "Esc/Left back");
    assert!(hint.contains("Up/Down scroll"), "{hint}");

    for _ in 0..200 {
        press(&mut app, KeyCode::Down);
    }
    let bottom = screen(&mut app, 60, 20);
    row_with(&bottom, "finding 30");
    assert!(!bottom.iter().any(|line| line.contains("Inspect the parser")));
    // Scrolling past the end stops at the last page, so one Up moves back.
    press(&mut app, KeyCode::Up);
    assert_ne!(screen(&mut app, 60, 20), bottom);
}

#[test]
fn a_session_without_subagents_and_an_unread_transcript_say_so() {
    let mut app = App::test_default();
    app.session_runtime.session_id = Some(SessionId::new("session-1"));
    open(&mut app);
    let lines = screen(&mut app, 60, 16);
    row_with(&lines, "No subagents yet");

    let (mut app, _commands) = app_with_subagents();
    app.agents.transcript =
        Some(TranscriptView { agent_id: "agent-2".to_owned(), ..TranscriptView::default() });
    let lines = screen(&mut app, 60, 16);
    row_with(&lines, "Reading transcript");
}
