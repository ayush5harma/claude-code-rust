// SPDX-License-Identifier: Apache-2.0

use super::markdown::{Header, render};
use super::{
    ExportProgress, apply_chunk, begin, default_file_name, resolve_destination, write_new_file,
};
use crate::agent::events::HistoryChunk;
use crate::agent::model::{
    ContentBlock, ContentChunk, Diff, SessionUpdate, TextContent, ToolCall, ToolCallContent,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use crate::app::App;
use pretty_assertions::assert_eq;

use serde_json::json;
use std::path::Path;

fn header() -> Header<'static> {
    Header {
        title: Some("Fix the bug"),
        session_id: "s-1",
        cwd: "/work/proj",
        model: Some("Opus"),
        exported_at: "2026-10-12 10:00:00 +00:00",
    }
}

const HEADER_MARKDOWN: &str = "# Fix the bug\n\n- Session: s-1\n- Directory: /work/proj\n- Model: Opus\n- Exported: 2026-10-12 10:00:00 +00:00\n\n";

fn text(text: &str) -> ContentChunk {
    ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
}

fn user(message: &str) -> SessionUpdate {
    SessionUpdate::UserMessageChunk(text(message))
}

fn assistant(message: &str) -> SessionUpdate {
    SessionUpdate::AgentMessageChunk(text(message))
}

fn tool(id: &str, name: &str, input: serde_json::Value) -> ToolCall {
    ToolCall::new(id, name)
        .raw_input(input)
        .meta(json!({ "claudeCode": { "toolName": name } }))
        .status(ToolCallStatus::InProgress)
}

fn result(id: &str, status: ToolCallStatus, output: &str) -> SessionUpdate {
    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
        id,
        ToolCallUpdateFields::new()
            .status(status)
            .content(vec![ToolCallContent::from(output)])
            .raw_output(json!(output)),
    ))
}

#[test]
fn a_conversation_becomes_markdown_with_tools_results_and_diffs() {
    let edit = tool("edit-1", "Edit", json!({ "file_path": "/work/proj/a.txt" })).content(vec![
        ToolCallContent::Diff(
            Diff::new("/work/proj/a.txt", "keep\nnew\n").old_text(Some("keep\nold\n")),
        ),
    ]);
    let updates = vec![
        user("Fix the bug"),
        assistant("I'll look."),
        SessionUpdate::ToolCall(tool("bash-1", "Bash", json!({ "command": "ls  -la\n" }))),
        result("bash-1", ToolCallStatus::Completed, "a\nb"),
        assistant("Done"),
        SessionUpdate::ToolCall(edit),
        result_without_text("edit-1"),
        user("Thanks"),
    ];

    let markdown = render(&header(), &updates, 0);

    let expected = [
        HEADER_MARKDOWN,
        "## User\n\nFix the bug\n\n",
        "## Assistant\n\nI'll look.\n\n",
        "### Tool: Bash - ls -la\n\nResult:\n\n```\na\nb\n```\n\n",
        "Done\n\n",
        "### Tool: Edit - /work/proj/a.txt\n\n",
        "```diff\n--- /work/proj/a.txt\n+++ /work/proj/a.txt\n@@ -1,2 +1,2 @@\n keep\n-old\n+new\n```\n\n",
        "## User\n\nThanks\n\n",
    ]
    .concat();
    assert_eq!(markdown, expected);
}

/// A completion that carries no output, as an Edit does once its diff is on the call.
fn result_without_text(id: &str) -> SessionUpdate {
    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
        id,
        ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
    ))
}

#[test]
fn a_failed_tool_is_labelled_as_an_error() {
    let updates = vec![
        SessionUpdate::ToolCall(tool("bash-1", "Bash", json!({ "command": "false" }))),
        result("bash-1", ToolCallStatus::Failed, "exit code 1"),
    ];

    let markdown = render(&header(), &updates, 0);

    let expected = [
        HEADER_MARKDOWN,
        "## Assistant\n\n",
        "### Tool: Bash - false\n\nError:\n\n```\nexit code 1\n```\n\n",
    ]
    .concat();
    assert_eq!(markdown, expected);
}

#[test]
fn terminal_control_sequences_never_reach_the_file() {
    let updates = vec![
        user("plain \u{1b}[1mbold\u{1b}[0m text"),
        SessionUpdate::ToolCall(tool("bash-1", "Bash", json!({ "command": "ls" }))),
        result(
            "bash-1",
            ToolCallStatus::Completed,
            "\u{1b}[31mred\u{1b}[0m \u{1b}]0;title\u{7}plain\r\nnext\u{8}",
        ),
    ];

    let markdown = render(&header(), &updates, 0);

    assert!(!markdown.contains('\u{1b}'), "escape byte in {markdown:?}");
    assert!(markdown.contains("plain bold text"));
    assert!(markdown.contains("```\nred plain\nnext\n```"));
}

#[test]
fn long_results_are_cut_at_sixty_lines_with_a_note() {
    // 70 one-character lines: 70 + 69 newlines = 139 characters; 60 lines keep 119.
    let output = vec!["x"; 70].join("\n");
    let updates = vec![
        SessionUpdate::ToolCall(tool("bash-1", "Bash", json!({ "command": "yes" }))),
        result("bash-1", ToolCallStatus::Completed, &output),
    ];

    let markdown = render(&header(), &updates, 0);

    let kept = vec!["x"; 60].join("\n");
    let expected = [
        HEADER_MARKDOWN,
        "## Assistant\n\n### Tool: Bash - yes\n\nResult:\n\n",
        &format!("```\n{kept}\n```\n\n"),
        "_Output truncated: the first 119 of 139 characters are shown._\n\n",
    ]
    .concat();
    assert_eq!(markdown, expected);
}

#[test]
fn a_single_huge_line_is_cut_at_four_thousand_characters() {
    let updates = vec![
        SessionUpdate::ToolCall(tool("bash-1", "Bash", json!({ "command": "cat min.js" }))),
        result("bash-1", ToolCallStatus::Completed, &"y".repeat(5000)),
    ];

    let markdown = render(&header(), &updates, 0);

    assert!(markdown.contains(&format!("```\n{}\n```", "y".repeat(4000))));
    assert!(markdown.contains("_Output truncated: the first 4000 of 5000 characters are shown._"));
}

#[test]
fn a_result_containing_a_fence_gets_a_longer_fence() {
    let updates = vec![
        SessionUpdate::ToolCall(tool("bash-1", "Bash", json!({ "command": "cat README" }))),
        result("bash-1", ToolCallStatus::Completed, "```rust\nfn main() {}\n```"),
    ];

    let markdown = render(&header(), &updates, 0);

    assert!(markdown.contains("````\n```rust\nfn main() {}\n```\n````\n"), "{markdown}");
}

#[test]
fn edit_and_write_calls_without_a_stored_diff_are_shown_from_their_input() {
    let updates = vec![
        SessionUpdate::ToolCall(tool(
            "w-1",
            "Write",
            json!({ "file_path": "/w/n.txt", "content": "hello\n" }),
        )),
        result("w-1", ToolCallStatus::Completed, ""),
        SessionUpdate::ToolCall(tool(
            "e-1",
            "Edit",
            json!({ "file_path": "/w/a.txt", "old_string": "one\n", "new_string": "two\n" }),
        )),
        result("e-1", ToolCallStatus::Completed, ""),
    ];

    let markdown = render(&header(), &updates, 0);

    let expected = [
        HEADER_MARKDOWN,
        "## Assistant\n\n",
        "### Tool: Write - /w/n.txt\n\n",
        "```diff\n--- /dev/null\n+++ /w/n.txt\n@@ -0,0 +1 @@\n+hello\n```\n\n",
        "### Tool: Edit - /w/a.txt\n\n",
        "```diff\n--- /w/a.txt\n+++ /w/a.txt\n@@ -1 +1 @@\n-one\n+two\n```\n\n",
    ]
    .concat();
    assert_eq!(markdown, expected);
}

#[test]
fn a_session_cut_by_the_bridge_says_how_much_is_missing() {
    let markdown = render(&header(), &[user("hi")], 12);

    assert!(markdown.ends_with(
        "---\n\n_Export truncated: 12 later history entries were left out because the session is very large._\n"
    ));
}

#[test]
fn file_names_carry_a_safe_title_or_the_short_session_id() {
    let stamp = "20261012-100000";
    let cases = [
        (Some("Fix the Auth bug!!"), "claude-rs-fix-the-auth-bug-20261012-100000.md"),
        (Some("  --weird//name:*?  "), "claude-rs-weird-name-20261012-100000.md"),
        (None, "claude-rs-abcdef12-20261012-100000.md"),
        (Some("\u{65e5}\u{672c}\u{8a9e}"), "claude-rs-abcdef12-20261012-100000.md"),
        (Some("a/../../etc"), "claude-rs-a-etc-20261012-100000.md"),
    ];
    for (title, expected) in cases {
        assert_eq!(default_file_name(title, "abcdef1234567890", stamp), expected, "{title:?}");
    }
    let long = default_file_name(Some(&format!("{} tail", "word ".repeat(20))), "abcdef12", stamp);
    // The slug is cut at 40 characters and never ends in a separator.
    assert_eq!(long, format!("claude-rs-{}-{stamp}.md", "word-".repeat(8).trim_end_matches('-')));
}

#[test]
fn destinations_resolve_against_the_session_directory_and_home() {
    let cwd = Path::new("/work/proj");
    let home = Path::new("/home/me");
    let default = "claude-rs-x.md";
    let cases = [
        ("", "/work/proj/claude-rs-x.md"),
        ("  ", "/work/proj/claude-rs-x.md"),
        ("notes.md", "/work/proj/notes.md"),
        ("sub/notes.md", "/work/proj/sub/notes.md"),
        ("/tmp/out.md", "/tmp/out.md"),
        ("~/out.md", "/home/me/out.md"),
        ("~", "/home/me"),
        ("my notes.md", "/work/proj/my notes.md"),
    ];
    for (requested, expected) in cases {
        assert_eq!(
            resolve_destination(requested, cwd, default, Some(home)),
            Path::new(expected),
            "{requested:?}"
        );
    }
    // Without a home directory `~` is just a name.
    assert_eq!(
        resolve_destination("~/out.md", cwd, default, None),
        Path::new("/work/proj/~/out.md")
    );
}

#[test]
fn an_existing_file_is_never_overwritten_and_no_directory_is_created() {
    let dir = tempfile::tempdir().expect("tempdir");
    let existing = dir.path().join("keep.md");
    std::fs::write(&existing, "original").expect("seed");

    let refused = write_new_file(&existing, "new content").expect_err("must refuse");

    assert!(refused.contains("already exists"), "{refused}");
    assert_eq!(std::fs::read_to_string(&existing).expect("read"), "original");

    let nested = dir.path().join("missing").join("out.md");
    let error = write_new_file(&nested, "x").expect_err("no directory is created");
    assert!(error.contains("directory does not exist"), "{error}");
    assert!(!dir.path().join("missing").exists());

    let fresh = dir.path().join("fresh.md");
    let written = write_new_file(&fresh, "hello").expect("write");
    assert_eq!(std::fs::read_to_string(written).expect("read"), "hello");
}

fn chunk(request_id: Option<&str>, updates: Vec<SessionUpdate>, last: bool) -> HistoryChunk {
    HistoryChunk {
        request_id: request_id.map(str::to_owned),
        updates,
        last,
        omitted_updates: 0,
        error: None,
    }
}

fn app_in(dir: &Path) -> App {
    let mut app = App::test_default();
    app.cwd_raw = dir.display().to_string();
    app.recent_sessions.push(crate::app::state::RecentSessionInfo {
        session_id: "s-1".to_owned(),
        summary: String::new(),
        last_modified_ms: 0,
        file_size_bytes: 0,
        cwd: None,
        git_branch: None,
        custom_title: Some("Fix the bug".to_owned()),
        first_prompt: None,
    });
    app
}

#[test]
fn begin_refuses_an_existing_target_before_any_history_is_requested() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("out.md"), "original").expect("seed");
    let mut app = app_in(dir.path());

    let refusal = begin(&mut app, "s-1", "out.md").expect_err("existing file");

    assert!(refusal.contains("already exists"), "{refusal}");
    assert!(app.history_export.is_none());
}

#[test]
fn chunks_are_collected_by_request_and_written_after_the_last() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut app = app_in(dir.path());
    let request_id = begin(&mut app, "s-1", "out.md").expect("begin");

    // A chunk from another request never joins this export.
    assert_eq!(
        apply_chunk(&mut app, chunk(Some("other"), vec![user("stale")], true)),
        ExportProgress::Ignored
    );
    assert_eq!(
        apply_chunk(&mut app, chunk(Some(&request_id), vec![user("first")], false)),
        ExportProgress::Collecting
    );
    assert!(!dir.path().join("out.md").exists(), "nothing is written before the last chunk");
    let progress = apply_chunk(&mut app, chunk(Some(&request_id), vec![assistant("second")], true));

    let path = dir.path().join("out.md");
    assert_eq!(progress, ExportProgress::Written(std::path::absolute(&path).expect("abs")));
    let file = std::fs::read_to_string(&path).expect("export file");
    assert!(file.starts_with("# Fix the bug\n\n- Session: s-1\n"), "{file}");
    assert!(file.contains("## User\n\nfirst\n\n## Assistant\n\nsecond\n\n"), "{file}");
    assert!(!file.contains("stale"));
    assert!(app.history_export.is_none());
}

#[test]
fn a_bridge_error_ends_the_export_without_a_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut app = app_in(dir.path());
    let request_id = begin(&mut app, "s-1", "out.md").expect("begin");

    let progress = apply_chunk(
        &mut app,
        HistoryChunk {
            error: Some("failed to load session history: boom".to_owned()),
            ..chunk(Some(&request_id), Vec::new(), true)
        },
    );

    assert_eq!(progress, ExportProgress::Failed("failed to load session history: boom".to_owned()));
    assert!(app.history_export.is_none());
    assert!(!dir.path().join("out.md").exists());
}

#[test]
fn the_default_destination_is_a_new_file_in_the_session_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut app = app_in(dir.path());
    let request_id = begin(&mut app, "s-1", "").expect("begin");

    let progress = apply_chunk(&mut app, chunk(Some(&request_id), vec![user("hi")], true));

    let ExportProgress::Written(path) = progress else { panic!("expected a written file") };
    let name = path.file_name().and_then(|name| name.to_str()).expect("file name");
    assert!(name.starts_with("claude-rs-fix-the-bug-"), "unexpected default name {name}");
    assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("md"));
    assert_eq!(path.parent(), Some(std::path::absolute(dir.path()).expect("abs").as_path()));
}
