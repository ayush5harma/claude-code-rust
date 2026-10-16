// SPDX-License-Identifier: Apache-2.0

use crate::agent::client::{AgentConnection, CommandReceiver};
use crate::agent::events::ClientEvent;
use crate::agent::model::{self, SessionId, TaskItem, TaskStatus};
use crate::agent::types::{SubagentMessage, SubagentMessageRole};
use crate::app::App;
use crate::app::agents::{TranscriptView, apply_transcript_list, handle_key, open};
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

fn app_with_subagents() -> (App, CommandReceiver) {
    let mut app = App::test_default();
    let (conn, commands) = AgentConnection::test_channel();
    app.session_runtime.conn = Some(Rc::new(conn));
    app.session_runtime.session_id = Some(SessionId::new("session-1"));
    app.sdk_inventory.tasks = vec![
        subagent(
            "agent-1",
            "Run the slow check",
            TaskStatus::InProgress,
            serde_json::json!({ "subagent_type": "general-purpose" }),
        ),
        TaskItem::new("shell-1", "Background shell", TaskStatus::InProgress),
        subagent(
            "agent-2",
            "Review the parser",
            TaskStatus::Completed,
            serde_json::json!({ "subagent_type": "reviewer", "terminal_status": "completed", "summary": "Parser looks fine" }),
        ),
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
fn rows_show_each_subagents_status_type_and_transcript_under_their_own_title() {
    let (mut app, _commands) = app_with_subagents();
    apply_transcript_list(&mut app, vec!["agent-2".to_owned()], None);
    handle_key(&mut app, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    let lines = screen(&mut app, 80, 30);

    let (title, title_line) = row_with(&lines, "Subagents");
    assert!(title_line.contains("1/3"), "{title_line}");
    assert!(title_line.contains("1 running"), "a stopping subagent still runs: {title_line}");
    assert!(
        !lines.iter().any(|line| line.contains("Background shell")),
        "only subagents are listed"
    );

    let (running, running_line) = row_with(&lines, "Run the slow check");
    assert!(running > title);
    assert!(running_line.contains('›'), "the first row is selected: {running_line}");
    assert!(running_line.contains("stopping"), "{running_line}");
    assert!(lines[running + 1].contains("general-purpose"));
    assert!(lines[running + 1].contains("no transcript"));

    let (done, done_line) = row_with(&lines, "Review the parser");
    assert!(done_line.contains("completed") && !done_line.contains('›'), "{done_line}");
    assert!(lines[done + 1].contains("reviewer") && lines[done + 1].contains("· transcript"));
    assert!(
        !lines.iter().any(|line| line.contains("Parser looks fine")),
        "an unselected subagent's result stays out of the list"
    );

    let (_, stopped_line) = row_with(&lines, "Abandoned search");
    assert!(stopped_line.contains("stopped"), "{stopped_line}");

    let (hint_row, _) = row_with(&lines, "Up/Down select");
    assert!(hint_row > running, "the hint row follows the list");
    let hint = lines[hint_row..].join(" ");
    for key in ["Enter transcript", "s stop", "r refresh", "Esc", "close"] {
        assert!(hint.contains(key), "{hint}");
    }
}

/// Reports one tool call the way the bridge does; `parent` makes it a subagent's own call.
fn tool_call(
    app: &mut App,
    id: &str,
    name: &str,
    title: &str,
    input: serde_json::Value,
    parent: Option<&str>,
) {
    let mut meta = serde_json::json!({ "claudeCode": { "toolName": name } });
    if let Some(parent) = parent {
        meta["claudeCode"]["parentToolUseId"] = serde_json::json!(parent);
    }
    crate::app::handle_client_event(
        app,
        ClientEvent::SessionUpdate {
            session_id: "session-1".to_owned(),
            update: model::SessionUpdate::ToolCall(
                model::ToolCall::new(id, title)
                    .status(model::ToolCallStatus::InProgress)
                    .raw_input(input)
                    .meta(meta),
            ),
        },
    );
}

#[test]
fn a_long_title_is_shortened_so_the_status_badge_stays_in_the_list_column() {
    let (mut app, _commands) = app_with_subagents();
    app.sdk_inventory.tasks[0].subject =
        "List every file in the working directory and summarise each one".to_owned();
    let lines = screen(&mut app, 120, 30);
    let (_, row) = row_with(&lines, "› List every file");
    let list_column: String = row.chars().take(2 + 52).collect();
    assert!(list_column.contains("... ") && list_column.contains(" running "), "{row}");
}

#[test]
fn details_show_the_selected_subagents_model_activity_and_prompt_beside_or_below_the_list() {
    let (mut app, _commands) = app_with_subagents();
    tool_call(
        &mut app,
        "launch-1",
        "Agent",
        "Run the slow check",
        serde_json::json!({ "description": "Run the slow check", "prompt": "List the files here, then summarise", "subagent_type": "general-purpose", "model": "haiku" }),
        None,
    );
    for (id, title) in
        [("child-1", "Glob **/*"), ("child-2", "Read notes.md"), ("child-3", "Bash wc -l notes.md")]
    {
        tool_call(&mut app, id, "Read", title, serde_json::json!({}), Some("launch-1"));
    }
    app.sdk_inventory.tasks[0].source_tool_call_id = Some("launch-1".to_owned());
    app.sdk_inventory
        .available_agents
        .push(model::AvailableAgent::new("reviewer", "Reviews code").model("sonnet"));
    apply_transcript_list(&mut app, vec!["agent-1".to_owned()], None);

    let wide = screen(&mut app, 120, 30);
    let (row, list_line) = row_with(&wide, "› Run the slow check");
    let detail_column =
        list_line.find("Run the slow check  ").and(list_line.rfind("Run the slow check"));
    assert!(
        detail_column.is_some_and(|column| column > 50),
        "details sit beside the list: {list_line}"
    );
    let detail = |label: &str| {
        wide.iter()
            .find(|line| line.contains(label))
            .unwrap_or_else(|| panic!("no {label:?} row:\n{}", wide.join("\n")))
            .clone()
    };
    assert!(detail("Model: ").contains("haiku (requested)"));
    assert!(detail("Transcript: ").contains("saved, Enter to read"));
    assert!(detail("Now: ").contains("Bash wc -l notes.md"), "the newest child call is current");
    let (recent, _) = row_with(&wide, "Recent");
    assert!(wide[recent + 1].contains("Read notes.md") && wide[recent + 2].contains("Glob **/*"));
    assert!(
        !wide[recent + 1..].iter().any(|line| line.contains("Bash wc -l")),
        "Now is not repeated"
    );
    let (prompt, _) = row_with(&wide, "Prompt");
    assert!(wide[prompt + 1].contains("List the files here, then summarise"));
    assert!(row < prompt);

    handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let narrow = screen(&mut app, 80, 30);
    let (selected, _) = row_with(&narrow, "› Review the parser");
    let (_, model) = row_with(&narrow, "Model: ");
    assert!(model.contains("sonnet (agent default)"), "{model}");
    let (result, result_line) = row_with(&narrow, "Result: ");
    assert!(result_line.contains("Parser looks fine"), "{result_line}");
    assert!(result > selected, "details sit below the list when narrow");
    assert!(
        !narrow.iter().any(|line| line.contains("Now: ")),
        "a finished subagent has no current activity"
    );
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
    let (_, title) = row_with(&lines, "Subagents");
    assert!(title.contains("Review the parser"), "{title}");
    assert!(!lines.iter().any(|line| line.contains("Run the slow check")), "the list is replaced");
    let (prompt, _) = row_with(&lines, "Prompt");
    assert!(lines[prompt + 1].contains("Inspect the parser"));
    row_with(&lines, "• ls parser");
    let (_, hint) = row_with(&lines, "Esc/Left back");
    assert!(hint.contains("Up/Down scroll"), "{hint}");

    for _ in 0..200 {
        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    let bottom = screen(&mut app, 60, 20);
    row_with(&bottom, "finding 30");
    assert!(!bottom.iter().any(|line| line.contains("Inspect the parser")));
    // Scrolling past the end stops at the last page, so one Up moves back.
    handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
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

#[test]
fn transcript_follows_width_changes_and_replaced_messages_through_its_wrap_cache() {
    let (mut app, _commands) = app_with_subagents();
    let long = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
    let reply = |text: &str| SubagentMessage {
        role: SubagentMessageRole::Assistant,
        text: text.to_owned(),
    };
    app.agents.transcript = Some(TranscriptView {
        agent_id: "agent-2".to_owned(),
        messages: Some(vec![reply(long)]),
        ..TranscriptView::default()
    });

    let wide = screen(&mut app, 100, 12);
    row_with(&wide, long);
    let narrow = screen(&mut app, 30, 12);
    assert!(!narrow.iter().any(|line| line.contains(long)), "the cached rows were rewrapped");
    row_with(&narrow, "alpha beta");
    row_with(&narrow, "lambda mu");

    if let Some(view) = app.agents.transcript.as_mut() {
        view.messages = Some(vec![reply("fresh answer")]);
        view.wrapped = None;
    }
    row_with(&screen(&mut app, 30, 12), "fresh answer");
}
