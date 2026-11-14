// SPDX-License-Identifier: Apache-2.0

use crate::agent::client::{AgentConnection, CommandReceiver};
use crate::agent::events::ClientEvent;
use crate::agent::model::{self, SessionId, TaskItem, TaskStatus, ToolCallStatus};
use crate::agent::types::{SubagentMessage, SubagentMessageRole};
use crate::agent::wire::BridgeCommand;
use crate::app::agents::{apply_transcript_list, handle_key, open};
use crate::app::{App, ChatMessage, MessageBlock, MessageRole};
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

/// A running, a completed and a stopped subagent with no Agent calls in the transcript.
fn plain_subagents() -> (App, CommandReceiver) {
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

fn press(app: &mut App, code: KeyCode) {
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE));
}

/// The id of the newest transcript read sent to the bridge.
fn last_read_request(commands: &mut CommandReceiver) -> String {
    std::iter::from_fn(|| commands.try_recv().ok())
        .filter(|envelope| matches!(envelope.command, BridgeCommand::GetSubagentMessages { .. }))
        .last()
        .and_then(|envelope| envelope.request_id)
        .expect("a transcript read was sent")
}

/// Opens the completed subagent's transcript as a user does; returns the read's request id.
fn open_transcript(app: &mut App, commands: &mut CommandReceiver) -> String {
    apply_transcript_list(app, vec!["agent-2".to_owned()], None);
    app.agents.selected_task_id = Some("agent-2".to_owned());
    press(app, KeyCode::Enter);
    last_read_request(commands)
}

fn answer(app: &mut App, request_id: &str, outcome: Result<Vec<SubagentMessage>, &str>) {
    answer_for(app, "agent-2", request_id, outcome);
}

fn answer_for(
    app: &mut App,
    agent_id: &str,
    request_id: &str,
    outcome: Result<Vec<SubagentMessage>, &str>,
) {
    let (messages, error) = match outcome {
        Ok(messages) => (messages, None),
        Err(error) => (Vec::new(), Some(error.to_owned())),
    };
    crate::app::handle_client_event(
        app,
        ClientEvent::SubagentMessagesReceived {
            session_id: "session-1".to_owned(),
            agent_id: agent_id.to_owned(),
            request_id: Some(request_id.to_owned()),
            messages,
            error,
        },
    );
}

fn message(role: SubagentMessageRole, text: &str) -> SubagentMessage {
    SubagentMessage { role, text: text.to_owned() }
}

fn reply(text: &str) -> SubagentMessage {
    message(SubagentMessageRole::Assistant, text)
}

fn drawn(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal.draw(|frame| super::render(frame, app)).expect("draw");
    terminal.backend().buffer().clone()
}

fn region(buffer: &ratatui::buffer::Buffer, area: ratatui::layout::Rect) -> Vec<String> {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn locate(buffer: &ratatui::buffer::Buffer, text: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let suffix: String =
                (x..buffer.area.width).map(|column| buffer[(column, y)].symbol()).collect();
            if suffix.starts_with(text) {
                return (x, y);
            }
        }
    }
    panic!("missing {text:?}: {:?}", region(buffer, buffer.area));
}

const BADGES: [&str; 7] = [
    " pending ",
    " running ",
    " stopping ",
    " completed ",
    " failed ",
    " stopped ",
    " interrupted ",
];

/// Where the selected agent's details start: the pane's left column and the row
/// of its header, found from its own anchors (the Transcript field and the
/// status badge above it), not from a layout percentage.
fn details_top(buffer: &ratatui::buffer::Buffer) -> (u16, u16) {
    let (x, transcript_y) = locate(buffer, "Transcript ");
    let width = buffer.area.width - x;
    let header = (0..transcript_y)
        .rev()
        .find(|&y| {
            let row = region(buffer, ratatui::layout::Rect::new(x, y, width, 1)).join("");
            // region() trims trailing blanks, which drops the badge's closing pad.
            BADGES.iter().any(|badge| row.contains(badge.trim_end()))
        })
        .expect("details header with a status badge");
    (x, header)
}

fn details(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    let (x, top) = details_top(buffer);
    let (_, hint_y) = locate(buffer, "Up/Down select");
    region(buffer, ratatui::layout::Rect::new(x, top, buffer.area.width - x, hint_y - top))
}

/// The details row of the field `label` (`Run`, `Parent`, ...).
fn field<'a>(pane: &'a [String], label: &str) -> &'a str {
    let prefix = format!("{label:<14}");
    pane.iter()
        .find(|row| row.starts_with(&prefix))
        .map_or_else(|| panic!("no {label} field:\n{}", pane.join("\n")), String::as_str)
}

fn reader_body(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    let (_, title_y) = locate(buffer, "Agents");
    let (_, hint_y) = locate(buffer, "Up/Down scroll");
    region(
        buffer,
        ratatui::layout::Rect::new(0, title_y + 1, buffer.area.width, hint_y - title_y - 1),
    )
}

fn assert_selected_reviewer(buffer: &ratatui::buffer::Buffer) {
    let rows = region(buffer, buffer.area);
    let (_, row) = row_with(&rows, "\u{203a} ");
    assert!(row.contains("reviewer"), "{row}");
}

fn event_key(app: &mut App, code: KeyCode) {
    crate::app::test_support::press(app, code);
}

fn expect_list(commands: &mut CommandReceiver) {
    let request = commands.try_recv().expect("transcript list request");
    assert!(
        matches!(request.command,BridgeCommand::ListSubagents {ref session_id} if session_id=="session-1")
    );
}

fn expect_read(commands: &mut CommandReceiver, agent: &str) -> String {
    let request = commands.try_recv().expect("transcript request");
    assert!(
        matches!(request.command,BridgeCommand::GetSubagentMessages {ref session_id,ref agent_id} if session_id=="session-1" && agent_id==agent)
    );
    assert!(commands.try_recv().is_err(), "no duplicate read or unrelated command");
    request.request_id.expect("correlated read")
}

fn listed(app: &mut App, ids: &[&str], error: Option<&str>) {
    crate::app::handle_client_event(
        app,
        ClientEvent::SubagentsListed {
            session_id: "session-1".into(),
            agent_ids: ids.iter().map(|id| (*id).into()).collect(),
            error: error.map(str::to_owned),
        },
    );
}

fn lifecycle(app: &mut App, tasks: Vec<TaskItem>, removed: &[&str]) {
    crate::app::test_support::deliver_update(
        app,
        model::SessionUpdate::TaskStateUpdate(
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle)
                .tasks(tasks)
                .removed_task_ids(removed.iter().map(|id| (*id).into()).collect()),
        ),
    );
}

fn audit_agents() -> (App, CommandReceiver) {
    let (app, mut commands) = plain_subagents();
    expect_list(&mut commands);
    assert!(commands.try_recv().is_err());
    (app, commands)
}

fn selected_reader(app: &mut App, commands: &mut CommandReceiver) -> String {
    listed(app, &["agent-2"], None);
    event_key(app, KeyCode::Down);
    let request = expect_read(commands, "agent-2");
    event_key(app, KeyCode::Enter);
    assert!(
        commands.try_recv().is_err(),
        "opening the pending preview does not duplicate its read"
    );
    request
}

fn long_notes(prefix: &str) -> Vec<SubagentMessage> {
    vec![message(
        SubagentMessageRole::User,
        &(1..=60).map(|n| format!("{prefix}-{n:02}")).collect::<Vec<_>>().join("\n"),
    )]
}

#[test]
fn ancestry_runs_and_fork_model_provenance_stay_with_the_selected_row() {
    let (mut app, mut commands) = audit_agents();
    listed(&mut app, &[], None);
    tool_call(
        &mut app,
        "fork-launch",
        "Agent",
        "Fork launch",
        serde_json::json!({"subagent_type":"fork","model":"ignored-model","effort":"max"}),
        None,
    );
    tool_call(
        &mut app,
        "parent-launch",
        "Agent",
        "Parent launch",
        serde_json::json!({"model":"parent-requested"}),
        None,
    );
    let mut child = subagent(
        "agent-1",
        "Run the slow check",
        TaskStatus::InProgress,
        serde_json::json!({"subagent_type":"fork"}),
    );
    child.run_id = Some("run-A".into());
    child.source_tool_call_run_id = Some("run-A".into());
    child.source_tool_call_id = Some("fork-launch".into());
    child.parent_task_id = Some("agent-2".into());
    let mut parent = subagent(
        "agent-2",
        "Review the parser",
        TaskStatus::Completed,
        serde_json::json!({"subagent_type":"reviewer","terminal_status":"completed"}),
    );
    parent.run_id = Some("parent-run".into());
    parent.source_tool_call_run_id = Some("parent-run".into());
    parent.source_tool_call_id = Some("parent-launch".into());
    lifecycle(&mut app, vec![child.clone(), parent], &[]);
    let pane = details(&drawn(&mut app, 150, 38));
    row_with(&pane, "parent model (inherited)");
    assert!(field(&pane, "Parent").contains("Review the parser"));
    assert!(field(&pane, "Run").contains("run-A"));
    assert!(!pane.iter().any(|row| row.contains("ignored-model") || row.contains("max")));
    crate::app::test_support::deliver_update(
        &mut app,
        model::SessionUpdate::ToolCallUpdate(model::ToolCallUpdate::new(
            "fork-launch",
            model::ToolCallUpdateFields::new().output_metadata(
                model::ToolOutputMetadata::new().agent(Some(
                    model::AgentOutputMetadata::new()
                        .resolved_model(Some("observed-sonnet".into()))
                        .models_used(Some(vec!["observed-opus".into(), "observed-sonnet".into()])),
                )),
            ),
        )),
    );
    let pane = details(&drawn(&mut app, 150, 38));
    row_with(&pane, "observed-opus -> observed-sonnet (observed)");
    event_key(&mut app, KeyCode::Down);
    let other = details(&drawn(&mut app, 150, 38));
    row_with(&other, "parent-requested");
    assert!(field(&other, "Run").contains("parent-run"));
    assert!(!other.join("\n").contains("observed-opus"));
    event_key(&mut app, KeyCode::Up);
    child.run_id = Some("run-B".into());
    lifecycle(&mut app, vec![child], &[]);
    let pane = details(&drawn(&mut app, 150, 38));
    row_with(&pane, "parent model (inherited)");
    assert!(!pane.join("\n").contains("observed-sonnet"));
    assert!(field(&pane, "Run").contains("run-B"));
    lifecycle(&mut app, vec![], &["agent-2"]);
    let pane = details(&drawn(&mut app, 150, 38));
    assert!(pane[0].contains("Run the slow check"));
    assert!(!pane.iter().any(|row| row.starts_with("Parent  ")));
    event_key(&mut app, KeyCode::Char('s'));
    let stop = commands.try_recv().expect("child stop remains usable");
    assert!(
        matches!(stop.command,BridgeCommand::StopSubagent {ref session_id,ref task_id,ref expected_run_id} if session_id=="session-1" && task_id=="agent-1" && expected_run_id.as_deref()==Some("run-B"))
    );
    assert!(stop.request_id.is_some());
    assert!(commands.try_recv().is_err());
}

#[test]
fn a_new_run_labels_the_open_saved_snapshot_without_replacing_its_rendered_content() {
    let (mut app, mut commands) = audit_agents();
    let mut task =
        app.sdk_inventory.tasks.iter().find(|task| task.task_id == "agent-2").unwrap().clone();
    task.run_id = Some("run-A".into());
    lifecycle(&mut app, vec![task.clone()], &[]);
    let request = selected_reader(&mut app, &mut commands);
    answer(&mut app, &request, Ok(long_notes("snapshot-A")));
    let _ = drawn(&mut app, 80, 24);
    event_key(&mut app, KeyCode::PageDown);
    let before = reader_body(&drawn(&mut app, 80, 24));
    assert!(before.iter().any(|row| row.contains("snapshot-A-20")));
    assert!(!before.iter().any(|row| row.contains("snapshot-A-01")));
    task.run_id = Some("run-B".into());
    lifecycle(&mut app, vec![task], &[]);
    let saved = drawn(&mut app, 80, 24);
    assert_eq!(reader_body(&saved), before);
    let header = region(&saved, ratatui::layout::Rect::new(0, locate(&saved, "Agents").1, 80, 1));
    assert!(header[0].contains("saved snapshot") && header[0].contains("Review the parser"));
    assert!(commands.try_recv().is_err());
    event_key(&mut app, KeyCode::Char('r'));
    let current = expect_read(&mut commands, "agent-2");
    assert_ne!(current, request);
    answer(&mut app, &current, Ok(long_notes("snapshot-B")));
    let replaced = drawn(&mut app, 80, 24);
    let body = reader_body(&replaced);
    assert!(body.iter().any(|row| row.contains("snapshot-B-20")));
    assert!(!body.join("\n").contains("snapshot-A"));
    assert!(
        !region(&replaced, replaced.area)[locate(&replaced, "Agents").1 as usize]
            .contains("saved snapshot")
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn list_rechecks_recover_their_error_without_clearing_an_unrelated_action_notice() {
    let (mut app, mut commands) = audit_agents();
    listed(&mut app, &[], Some("directory denied sentinel"));
    let failed = drawn(&mut app, 90, 30);
    assert_notice(&failed, "directory denied sentinel");
    event_key(&mut app, KeyCode::Char('r'));
    expect_list(&mut commands);
    listed(&mut app, &["agent-2"], None);
    assert!(
        !region(&drawn(&mut app, 90, 30), ratatui::layout::Rect::new(0, 0, 90, 30))
            .join("\n")
            .contains("directory denied sentinel")
    );
    // The interrupted row cannot be stopped and does not schedule a response read.
    event_key(&mut app, KeyCode::Down);
    let preview = expect_read(&mut commands, "agent-2");
    answer(&mut app, &preview, Ok(vec![]));
    event_key(&mut app, KeyCode::Down);
    event_key(&mut app, KeyCode::Char('s'));
    let stop_notice = app.agents.notice.as_ref().expect("non-running stop notice").text.clone();
    assert!(stop_notice.contains("running") && stop_notice.contains("stopped"));
    event_key(&mut app, KeyCode::Char('r'));
    expect_list(&mut commands);
    listed(&mut app, &[], Some("second directory cause"));
    assert_notice(&drawn(&mut app, 90, 30), "second directory cause");
    event_key(&mut app, KeyCode::Char('r'));
    expect_list(&mut commands);
    listed(&mut app, &[], None);
    let recovered = drawn(&mut app, 90, 30);
    assert_notice(&recovered, &stop_notice);
    assert!(!region(&recovered, recovered.area).join("\n").contains("second directory cause"));
    assert!(commands.try_recv().is_err());
    // Empty inventory and a populated session are distinct states on the same working connection.
    crate::app::test_support::deliver_update(
        &mut app,
        model::SessionUpdate::TaskStateUpdate(
            model::TaskStateUpdate::new(model::TaskUpdateSource::List).complete_snapshot(true),
        ),
    );
    event_key(&mut app, KeyCode::Char('r'));
    expect_list(&mut commands);
    listed(&mut app, &[], None);
    let empty = drawn(&mut app, 90, 30);
    assert!(region(&empty, empty.area).join("\n").contains("No subagents"));
    lifecycle(
        &mut app,
        vec![subagent(
            "new",
            "Recovered live agent",
            TaskStatus::InProgress,
            serde_json::json!({"subagent_type":"worker"}),
        )],
        &[],
    );
    event_key(&mut app, KeyCode::Down);
    assert!(details(&drawn(&mut app, 90, 30))[0].contains("Recovered live agent"));
}

fn assert_notice(buffer: &ratatui::buffer::Buffer, cause: &str) {
    let (_, row) = locate(buffer, cause);
    let (_, hint) = locate(buffer, "Up/Down select");
    assert!(
        row < hint && row > locate(buffer, "Transcript ").1,
        "notice belongs below content and above controls"
    );
}

#[test]
fn transcript_retry_distinguishes_pending_empty_failed_and_recovered_content() {
    let (mut app, mut commands) = audit_agents();
    let request = selected_reader(&mut app, &mut commands);
    assert!(reader_body(&drawn(&mut app, 80, 24)).join("\n").contains("Reading transcript"));
    answer(&mut app, &request, Err("first read denied sentinel"));
    assert!(
        reader_body(&drawn(&mut app, 80, 24)).join("\n").contains("first read denied sentinel")
    );
    event_key(&mut app, KeyCode::Char('r'));
    let retry = expect_read(&mut commands, "agent-2");
    assert_ne!(retry, request);
    assert!(reader_body(&drawn(&mut app, 80, 24)).join("\n").contains("Reading transcript"));
    answer(&mut app, &retry, Ok(vec![]));
    assert!(reader_body(&drawn(&mut app, 80, 24)).join("\n").contains("Empty transcript"));
    event_key(&mut app, KeyCode::Char('r'));
    let populated = expect_read(&mut commands, "agent-2");
    answer(&mut app, &populated, Ok(vec![reply("Independent parser findings")]));
    let body = reader_body(&drawn(&mut app, 80, 24));
    assert!(row_with(&body, "Independent parser findings").0 > row_with(&body, "Reply").0);
    assert!(!body.join("\n").contains("denied sentinel"));
    event_key(&mut app, KeyCode::Char('r'));
    let failed = expect_read(&mut commands, "agent-2");
    answer(&mut app, &failed, Err("later read denied sentinel"));
    assert!(
        reader_body(&drawn(&mut app, 80, 24)).join("\n").contains("later read denied sentinel")
    );
    event_key(&mut app, KeyCode::Char('r'));
    let final_read = expect_read(&mut commands, "agent-2");
    assert!(
        reader_body(&drawn(&mut app, 80, 24)).join("\n").contains("Independent parser findings")
    );
    answer(&mut app, &final_read, Ok(vec![reply("Newly recovered findings")]));
    let body = reader_body(&drawn(&mut app, 80, 24)).join("\n");
    assert!(body.contains("Newly recovered findings"));
    assert!(!body.contains("denied sentinel") && !body.contains("Independent parser findings"));
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
fn selected_details_keep_invocation_activity_prompt_and_model_provenance_together() {
    let (mut app, mut commands) = audit_agents();
    tool_call(
        &mut app,
        "launch-1",
        "Agent",
        "Primary launch",
        serde_json::json!({"prompt":"Primary prompt sentinel","model":"haiku"}),
        None,
    );
    tool_call(
        &mut app,
        "launch-rival",
        "Agent",
        "Rival launch",
        serde_json::json!({"prompt":"Rival prompt sentinel"}),
        None,
    );
    for (id, title, parent) in [
        ("older", "Primary oldest", "launch-1"),
        ("previous", "Primary previous", "launch-1"),
        ("now", "Primary newest", "launch-1"),
        ("rival-child", "Rival activity sentinel", "launch-rival"),
    ] {
        tool_call(&mut app, id, "Read", title, serde_json::json!({}), Some(parent));
    }
    let mut primary = app.sdk_inventory.tasks[0].clone();
    primary.source_tool_call_id = Some("launch-1".into());
    let mut rival = app.sdk_inventory.tasks[2].clone();
    rival.source_tool_call_id = Some("launch-rival".into());
    lifecycle(&mut app, vec![primary, rival], &[]);
    app.sdk_inventory
        .available_agents
        .push(model::AvailableAgent::new("reviewer", "Reviews code").model("sonnet"));
    listed(&mut app, &["agent-1"], None);
    for width in [140, 80] {
        let buffer = drawn(&mut app, width, 44);
        let pane = details(&buffer);
        row_with(&pane, "general-purpose \u{b7} haiku (requested)");
        assert!(field(&pane, "Transcript").contains("saved"));
        assert!(field(&pane, "Now").contains("Primary newest"));
        let recent = pane.iter().position(|row| row.starts_with("Recent ")).expect("Recent field");
        assert!(row_with(&pane, "Primary previous").0 >= recent);
        assert!(row_with(&pane, "Primary previous").0 < row_with(&pane, "Primary oldest").0);
        assert!(!pane[recent..].join("\n").contains("Primary newest"));
        assert!(field(&pane, "Prompt").contains("Primary prompt sentinel"));
        assert!(!pane.join("\n").contains("Rival"));
        let (list_x, list_y) = locate(&buffer, "\u{203a}");
        let (detail_x, top_y) = details_top(&buffer);
        if width == 140 {
            assert!(detail_x > list_x);
            assert_eq!(top_y, list_y, "beside the list, the details start at its top");
        } else {
            assert_eq!(detail_x, list_x);
            assert!(top_y > list_y, "below the list");
        }
    }
    event_key(&mut app, KeyCode::Down);
    for width in [80, 140] {
        let pane = details(&drawn(&mut app, width, 44));
        row_with(&pane, "reviewer \u{b7} sonnet \u{b7} agent default");
        assert!(field(&pane, "Result").contains("Parser looks fine"));
        assert!(field(&pane, "Prompt").contains("Rival prompt sentinel"));
        assert!(field(&pane, "Recent").contains("Rival activity sentinel"));
        assert!(!pane.join("\n").contains("Primary"));
        assert!(!pane.iter().any(|row| row.starts_with("Now ")));
    }
    assert!(commands.try_recv().is_err());
}

#[test]
fn transcript_and_detail_keys_page_clamp_and_return_to_the_selected_task() {
    let (mut app, mut commands) = audit_agents();
    let request = selected_reader(&mut app, &mut commands);
    let mut messages = vec![
        message(SubagentMessageRole::User, "Inspect the parser"),
        message(SubagentMessageRole::Tool, "ls parser"),
        reply("Opening finding"),
    ];
    messages.extend(long_notes("note"));
    messages.push(reply("FINAL finding"));
    answer(&mut app, &request, Ok(messages));
    let initial = drawn(&mut app, 80, 24);
    let body = reader_body(&initial);
    assert!(body[row_with(&body, "Prompt").0 + 1].contains("Inspect the parser"));
    row_with(&body, "\u{2022} ls parser");
    assert!(row_with(&body, "Opening finding").0 > row_with(&body, "Reply").0);
    assert!(!body.join("\n").contains("Run the slow check"));
    locate(&initial, "Esc/Left back");
    event_key(&mut app, KeyCode::Down);
    let after_down = reader_body(&drawn(&mut app, 80, 24));
    assert_eq!(
        after_down.iter().find(|line| !line.trim().is_empty()).unwrap().trim(),
        "Inspect the parser"
    );
    row_with(&after_down, "Inspect the parser");
    let mut reached = false;
    for _ in 0..15 {
        event_key(&mut app, KeyCode::PageDown);
        if reader_body(&drawn(&mut app, 80, 24)).join("\n").contains("FINAL finding") {
            reached = true;
            break;
        }
    }
    assert!(reached, "page keys reach independent final reply");
    for _ in 0..3 {
        event_key(&mut app, KeyCode::PageDown);
        drawn(&mut app, 80, 24);
    }
    let bottom = reader_body(&drawn(&mut app, 80, 24));
    event_key(&mut app, KeyCode::Down);
    assert_eq!(reader_body(&drawn(&mut app, 80, 24)), bottom);
    let first = bottom
        .iter()
        .find_map(|line| line.trim().strip_prefix("note-").and_then(|n| n.parse::<u32>().ok()))
        .expect("tail notes");
    event_key(&mut app, KeyCode::Up);
    let earlier = reader_body(&drawn(&mut app, 80, 24));
    row_with(&earlier, &format!("note-{:02}", first - 1));
    event_key(&mut app, KeyCode::PageUp);
    let previous = reader_body(&drawn(&mut app, 80, 24));
    row_with(&previous, &format!("note-{:02}", first - 11));
    event_key(&mut app, KeyCode::Left);
    let overview = drawn(&mut app, 80, 32);
    assert_selected_reviewer(&overview);
    assert!(details(&overview)[0].contains("Review the parser"));
    // A long real launch prompt makes the selected details independently pageable.
    tool_call(
        &mut app,
        "detail-launch",
        "Agent",
        "Parser launch",
        serde_json::json!({"prompt":(1..=30).map(|n|format!("detail-note-{n:02}")).collect::<Vec<_>>().join("\n")}),
        None,
    );
    let mut task = app.sdk_inventory.tasks.iter().find(|t| t.task_id == "agent-2").unwrap().clone();
    task.source_tool_call_id = Some("detail-launch".into());
    lifecycle(&mut app, vec![task], &[]);
    let initial = drawn(&mut app, 120, 14);
    let (x, y) = details_top(&initial);
    let (_, end) = locate(&initial, "Up/Down select");
    let area = ratatui::layout::Rect::new(x, y, 120 - x, end - y);
    event_key(&mut app, KeyCode::PageDown);
    let later = region(&drawn(&mut app, 120, 14), area);
    row_with(&later, "FINAL finding");
    event_key(&mut app, KeyCode::PageUp);
    assert!(region(&drawn(&mut app, 120, 14), area)[0].contains("Review the parser"));
    event_key(&mut app, KeyCode::PageDown);
    drawn(&mut app, 120, 14);
    event_key(&mut app, KeyCode::Up);
    assert!(details(&drawn(&mut app, 120, 24))[0].contains("Run the slow check"));
    assert!(commands.try_recv().is_err());
}

#[test]
fn warmed_transcript_wrapping_preserves_complete_source_and_replaces_stale_content() {
    let (mut app, mut commands) = audit_agents();
    let request = selected_reader(&mut app, &mut commands);
    let source = "alpha beta gamma delta distinct-middle-one epsilon zeta eta theta distinct-middle-two iota kappa lambda mu";
    answer(&mut app, &request, Ok(vec![reply(source)]));
    for width in [120, 30, 60, 120] {
        let body = reader_body(&drawn(&mut app, width, 32));
        let label = row_with(&body, "Reply").0;
        assert_eq!(
            body[label + 1..].join(" ").split_whitespace().collect::<Vec<_>>(),
            source.split_whitespace().collect::<Vec<_>>()
        );
    }
    event_key(&mut app, KeyCode::Char('r'));
    let fresh = expect_read(&mut commands, "agent-2");
    assert_ne!(fresh, request);
    let replacement =
        "Fresh independent answer with replacement-middle and complete final evidence";
    answer(&mut app, &fresh, Ok(vec![reply(replacement)]));
    for width in [30, 120] {
        let body = reader_body(&drawn(&mut app, width, 32));
        let label = row_with(&body, "Reply").0;
        assert_eq!(
            body[label + 1..].join(" ").split_whitespace().collect::<Vec<_>>(),
            replacement.split_whitespace().collect::<Vec<_>>()
        );
        assert!(!body.join("\n").contains("distinct-middle"));
    }
}

#[test]
fn r_rereads_the_transcript_at_the_same_position_and_ends_a_shrunk_one_on_its_last_page() {
    let (mut app, mut commands) = plain_subagents();
    let findings = |count: usize| (1..=count).map(|n| reply(&format!("finding {n}"))).collect();
    let request = open_transcript(&mut app, &mut commands);
    answer(&mut app, &request, Ok(findings(40)));
    for _ in 0..20 {
        press(&mut app, KeyCode::Down);
    }
    let before = screen(&mut app, 60, 20);
    let (title, _) = row_with(&before, "Review the parser");
    let (hint, _) = row_with(&before, "Esc/Left back");
    assert!(
        !before.iter().any(|line| line.trim() == "finding 1"),
        "the reader is scrolled: {}",
        before.join("\n")
    );

    press(&mut app, KeyCode::Char('r'));
    let request = last_read_request(&mut commands);
    answer(&mut app, &request, Ok(findings(60)));
    let after = screen(&mut app, 60, 20);
    assert_eq!(after[title..hint], before[title..hint], "the reading position is kept");

    press(&mut app, KeyCode::Char('r'));
    let request = last_read_request(&mut commands);
    answer(&mut app, &request, Ok(findings(3)));
    let shrunk = screen(&mut app, 60, 20);
    row_with(&shrunk, "finding 3");
    row_with(&shrunk, "finding 1");
}

#[test]
fn a_bounded_saved_preview_opens_the_complete_styled_report() {
    let (mut app, mut commands) = audit_agents();
    listed(&mut app, &["agent-2"], None);
    event_key(&mut app, KeyCode::Down);
    let request = expect_read(&mut commands, "agent-2");
    let entries: Vec<_> = (1..=30).map(|n| format!("page-{n:02}")).collect();
    let report = format!(
        "## Docs survey\n\nThe **docs** cover:\n\n{}",
        entries.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n")
    );
    answer(&mut app, &request, Ok(vec![reply(&report)]));
    let overview = drawn(&mut app, 80, 40);
    let pane = details(&overview);
    let start = row_with(&pane, "Response  ").0;
    let preview = pane[start..].join("\n");
    assert!(preview.contains("Docs survey"));
    assert!(!preview.contains("page-30"));
    assert!(
        preview.contains("...") || preview.contains('\u{2026}'),
        "omitted source disclosed: {preview}"
    );
    locate(&overview, "Enter transcript");
    event_key(&mut app, KeyCode::Enter);
    assert!(commands.try_recv().is_err());
    let initial = drawn(&mut app, 80, 24);
    let body = reader_body(&initial);
    assert!(row_with(&body, "Docs survey").0 > row_with(&body, "Reply").0);
    for text in ["Docs survey", "docs"] {
        let (x, y) = locate(&initial, text);
        assert!(
            initial[(x, y)].modifier.contains(ratatui::style::Modifier::BOLD),
            "{text} is styled Markdown"
        );
    }
    assert!(!body.join("\n").contains("##") && !body.join("\n").contains("**"));
    let mut seen = Vec::new();
    for _ in 0..8 {
        let body = reader_body(&drawn(&mut app, 80, 24));
        for line in body {
            if let Some(entry) = entries.iter().find(|entry| line.contains(entry.as_str()))
                && !seen.contains(entry)
            {
                seen.push(entry.clone());
            }
        }
        if seen.last() == entries.last() {
            break;
        }
        event_key(&mut app, KeyCode::PageDown);
    }
    assert_eq!(seen, entries, "all source items accessible in order");
}

#[test]
fn an_sdk_reminder_is_labelled_as_one_while_tags_a_user_or_agent_wrote_stay_as_written() {
    let (mut app, mut commands) = plain_subagents();
    let request = open_transcript(&mut app, &mut commands);
    let prompt = "Quote <system-reminder>tags</system-reminder> as written.";
    let reply_text = "Keeping <system-reminder>as written</system-reminder>";
    answer(
        &mut app,
        &request,
        Ok(vec![
            message(SubagentMessageRole::User, prompt),
            message(SubagentMessageRole::SystemReminder, "Report through SubagentHandback."),
            reply(reply_text),
        ]),
    );
    let lines = screen(&mut app, 80, 30);
    let (reminder, _) = row_with(&lines, "Report through SubagentHandback.");
    assert_eq!(lines[reminder - 1].trim(), "System reminder", "{}", lines.join("\n"));
    assert!(!lines[reminder].contains("system-reminder"), "{}", lines[reminder]);
    let (quoted, _) = row_with(&lines, prompt);
    assert_eq!(lines[quoted - 1].trim(), "Prompt");
    let (kept, _) = row_with(&lines, reply_text);
    assert_eq!(lines[kept - 1].trim(), "Reply");
}

#[test]
fn lifecycle_completion_renews_the_saved_preview_and_selects_the_final_assistant_response() {
    let (mut app, mut commands) = audit_agents();
    listed(&mut app, &["agent-1"], None);
    event_key(&mut app, KeyCode::Enter);
    let running = expect_read(&mut commands, "agent-1");
    answer_for(&mut app, "agent-1", &running, Ok(vec![reply("Halfway progress sentinel")]));
    row_with(&reader_body(&drawn(&mut app, 80, 24)), "Halfway progress sentinel");
    let mut task = app.sdk_inventory.tasks[0].clone();
    task.status = TaskStatus::Completed;
    task.metadata = Some(
        serde_json::json!({"task_type":"local_agent","subagent_type":"general-purpose","terminal_status":"completed"}),
    );
    lifecycle(&mut app, vec![task], &[]);
    assert!(commands.try_recv().is_err());
    event_key(&mut app, KeyCode::Esc);
    let completed = expect_read(&mut commands, "agent-1");
    assert_ne!(completed, running);
    assert!(!details(&drawn(&mut app, 80, 40)).join("\n").contains("Halfway"));
    answer_for(
        &mut app,
        "agent-1",
        &completed,
        Ok(vec![
            reply("Earlier answer sentinel"),
            message(SubagentMessageRole::Tool, "Intervening tool sentinel"),
            reply("Final verified findings"),
            message(SubagentMessageRole::User, "Trailing prompt sentinel"),
        ]),
    );
    let pane = details(&drawn(&mut app, 80, 40));
    let preview = pane[row_with(&pane, "Response  ").0..].join("\n");
    assert!(preview.contains("Final verified findings"));
    for stale in ["Halfway", "Earlier answer", "Intervening tool", "Trailing prompt"] {
        assert!(!preview.contains(stale));
    }
}

#[test]
fn an_interrupted_row_labels_its_text_as_last_progress_not_as_a_result() {
    let (mut app, _commands) = plain_subagents();
    app.sdk_inventory.tasks = vec![subagent(
        "agent-9",
        "Watch the logs",
        TaskStatus::Completed,
        serde_json::json!({ "terminal_status": "interrupted", "summary": "tailing the third file" }),
    )];
    handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let lines = screen(&mut app, 120, 30);
    let (_, progress) = row_with(&lines, "Last progress");
    assert!(progress.contains("tailing the third file"), "{progress}");
    assert!(!lines.iter().any(|line| line.contains("Result")), "no outcome is claimed");
}

#[test]
fn a_transcript_longer_than_the_wrap_helpers_row_cap_stays_readable_to_the_end() {
    let (mut app, mut commands) = plain_subagents();
    // Three rows per reply: more than u16::MAX rows in all.
    let messages: Vec<_> = (1..=25_000).map(|n| reply(&format!("finding {n}"))).collect();
    let request = open_transcript(&mut app, &mut commands);
    answer(&mut app, &request, Ok(messages));
    if let Some(view) = app.agents.transcript.as_mut() {
        view.scroll = usize::MAX;
    }
    row_with(&screen(&mut app, 60, 20), "finding 25000");
}

#[test]
fn undersized_agent_views_preserve_reader_selection_and_a_usable_return_to_chat() {
    for reader in [true, false] {
        let (mut app, mut commands) = audit_agents();
        app.input.set_text("preserved draft");
        if reader {
            let request = selected_reader(&mut app, &mut commands);
            answer(&mut app, &request, Ok(long_notes("resize-source")));
            drawn(&mut app, 80, 24);
            event_key(&mut app, KeyCode::PageDown);
        } else {
            let mut selected = app
                .sdk_inventory
                .tasks
                .iter()
                .find(|task| task.task_id == "agent-2")
                .unwrap()
                .clone();
            selected.status = TaskStatus::InProgress;
            selected.metadata =
                Some(serde_json::json!({"task_type":"local_agent","subagent_type":"reviewer"}));
            lifecycle(&mut app, vec![selected], &[]);
            listed(&mut app, &["agent-2"], None);
            event_key(&mut app, KeyCode::Down);
            assert!(commands.try_recv().is_err());
        }
        let before = drawn(&mut app, 80, 24);
        if reader {
            row_with(&reader_body(&before), "resize-source-20");
        } else {
            assert_selected_reviewer(&before);
        }
        for (width, height) in [(29, 24), (80, 7), (29, 7)] {
            drawn(&mut app, width, height);
            let restored = drawn(&mut app, 80, 24);
            assert_eq!(restored, before, "resize preserves populated view and reading position");
            assert!(commands.try_recv().is_err());
        }
        if !reader {
            event_key(&mut app, KeyCode::Enter);
            let request = expect_read(&mut commands, "agent-2");
            answer(&mut app, &request, Ok(vec![reply("Selected saved report")]));
            row_with(&reader_body(&drawn(&mut app, 80, 24)), "Selected saved report");
        }
        drawn(&mut app, 29, 7);
        event_key(&mut app, KeyCode::Esc);
        assert_selected_reviewer(&drawn(&mut app, 80, 24));
        drawn(&mut app, 29, 7);
        event_key(&mut app, KeyCode::Esc);
        assert_eq!(app.surface_mode, crate::app::SurfaceMode::Chat);
        assert_eq!(app.input.text(), "preserved draft");
        event_key(&mut app, KeyCode::Char('!'));
        assert_eq!(app.input.text(), "preserved draft!");
        assert!(commands.try_recv().is_err());
    }
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
        child.parent_tool_use_id = Some(parent.to_owned());
        blocks.push(MessageBlock::ToolCall(Box::new(child)));
    }
    let index = app.transcript.messages.len();
    app.transcript.messages.push(ChatMessage::new(MessageRole::Assistant, blocks, None));
    app.index_tool_call(parent.to_owned(), index, 0);
    for (offset, child_id) in child_ids.into_iter().enumerate() {
        app.index_tool_call(child_id, index, offset + 1);
    }
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

fn connected_app() -> (App, CommandReceiver) {
    let mut app = App::test_default();
    let (conn, commands) = AgentConnection::test_channel();
    app.session_runtime.conn = Some(Rc::new(conn));
    app.session_runtime.session_id = Some(SessionId::new("session-1"));
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
    let (reviewer_header, header_line) = row_with(&lines, " completed ");
    assert!(header_line.contains("Review the parser"), "{header_line}");
    let (_, transcript) = row_with(&lines, "Transcript");
    assert!(transcript.contains("saved · Enter to read"), "{transcript}");
    // A finished agent shows how it ended and the tools it ran, not a "Now".
    let (_, result) = row_with(&lines[reviewer_header..], "Result");
    assert!(result.contains("Parser looks fine"), "{result}");
    let (_, recent) = row_with(&lines[reviewer_header..], "Recent");
    assert!(recent.contains("Read parser.rs"), "{recent}");
    assert!(
        !lines[reviewer_header..].iter().any(|line| line.trim_start().starts_with("Now ")),
        "{}",
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
    let (mut app, mut commands) = app_with_subagents();
    apply_transcript_list(&mut app, vec!["agent-2".to_owned()], None);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let request = last_read_request(&mut commands);
    let mut messages = vec![
        message(SubagentMessageRole::User, "Inspect the parser"),
        message(SubagentMessageRole::Tool, "ls parser"),
    ];
    messages.extend((1..=30).map(|n| reply(&format!("finding {n}"))));
    answer(&mut app, &request, Ok(messages));

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

    let (mut app, mut commands) = app_with_subagents();
    apply_transcript_list(&mut app, vec!["agent-2".to_owned()], None);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let _ = last_read_request(&mut commands);
    let lines = screen(&mut app, 60, 16);
    row_with(&lines, "Reading transcript");
}
