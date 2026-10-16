// SPDX-License-Identifier: Apache-2.0

//! Markdown rendering of a session's complete history for `/export`.
//!
//! The input is the bridge's resume translation of the saved transcript, so this
//! module owns only presentation: message text is kept verbatim, tool calls get a
//! heading with a compact input summary, and results and diffs go into fenced
//! blocks that are truncated with a note. Terminal control sequences never reach
//! the file.

use crate::agent::model::{
    ContentBlock, SessionUpdate, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdateFields,
};
use crate::ui::highlight::strip_ansi;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

/// Longest tool result kept in the file, in characters and in lines.
const MAX_RESULT_CHARS: usize = 4_000;
const MAX_RESULT_LINES: usize = 60;
const MAX_DIFF_LINES: usize = 200;
const MAX_SUMMARY_CHARS: usize = 100;
/// Tool input fields that best name what a call does, in priority order.
const SUMMARY_KEYS: [&str; 8] =
    ["command", "file_path", "path", "pattern", "url", "query", "description", "prompt"];

pub(super) struct Header<'a> {
    pub title: Option<&'a str>,
    pub session_id: &'a str,
    pub cwd: &'a str,
    pub model: Option<&'a str>,
    pub exported_at: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

pub(super) fn render(
    header: &Header<'_>,
    updates: &[SessionUpdate],
    omitted_updates: u64,
) -> String {
    let tools = final_tool_states(updates);
    let mut out = String::new();
    write_header(&mut out, header);
    let mut role = None;
    for update in updates {
        match update {
            SessionUpdate::UserMessageChunk(chunk) => {
                write_text(&mut out, &mut role, Role::User, &chunk.content);
            }
            SessionUpdate::AgentMessageChunk(chunk) => {
                write_text(&mut out, &mut role, Role::Assistant, &chunk.content);
            }
            SessionUpdate::ToolCall(call) => {
                enter_role(&mut out, &mut role, Role::Assistant);
                if let Some(call) = tools.get(call.tool_call_id.as_str()) {
                    write_tool(&mut out, call);
                }
            }
            _ => {}
        }
    }
    if omitted_updates > 0 {
        let _ = writeln!(
            out,
            "---\n\n_Export truncated: {omitted_updates} later history entries were left out because the session is very large._"
        );
    }
    out
}

/// Each tool call as the session ended: its call merged with every later update.
fn final_tool_states(updates: &[SessionUpdate]) -> HashMap<&str, ToolCall> {
    let mut tools: HashMap<&str, ToolCall> = HashMap::new();
    for update in updates {
        match update {
            SessionUpdate::ToolCall(call) => {
                tools.insert(call.tool_call_id.as_str(), call.clone());
            }
            SessionUpdate::ToolCallUpdate(update) => {
                if let Some(call) = tools.get_mut(update.tool_call_id.as_str()) {
                    apply_fields(call, &update.fields);
                }
            }
            _ => {}
        }
    }
    tools
}

fn apply_fields(call: &mut ToolCall, fields: &ToolCallUpdateFields) {
    if let Some(title) = &fields.title {
        call.title.clone_from(title);
    }
    if let Some(status) = fields.status {
        call.status = status;
    }
    if let Some(content) = &fields.content {
        call.content.clone_from(content);
    }
    if let Some(raw_input) = &fields.raw_input {
        call.raw_input = Some(raw_input.clone());
    }
    if let Some(raw_output) = &fields.raw_output {
        call.raw_output = Some(raw_output.clone());
    }
}

fn write_header(out: &mut String, header: &Header<'_>) {
    let title = header.title.map(clean_line).filter(|title| !title.is_empty());
    let _ = writeln!(out, "# {}\n", title.as_deref().unwrap_or("Conversation"));
    let _ = writeln!(out, "- Session: {}", clean_line(header.session_id));
    let _ = writeln!(out, "- Directory: {}", clean_line(header.cwd));
    if let Some(model) = header.model {
        let _ = writeln!(out, "- Model: {}", clean_line(model));
    }
    let _ = writeln!(out, "- Exported: {}\n", clean_line(header.exported_at));
}

fn enter_role(out: &mut String, current: &mut Option<Role>, role: Role) {
    if *current == Some(role) {
        return;
    }
    let heading = match role {
        Role::User => "User",
        Role::Assistant => "Assistant",
    };
    let _ = writeln!(out, "## {heading}\n");
    *current = Some(role);
}

fn write_text(out: &mut String, current: &mut Option<Role>, role: Role, block: &ContentBlock) {
    let text = match block {
        ContentBlock::Text(text) => clean(&text.text),
        ContentBlock::Image(_) => "[image]".to_owned(),
    };
    let text = text.trim_end_matches('\n');
    if text.trim().is_empty() {
        return;
    }
    enter_role(out, current, role);
    out.push_str(text);
    out.push_str("\n\n");
}

fn write_tool(out: &mut String, call: &ToolCall) {
    let name = tool_name(call);
    match input_summary(call.raw_input.as_ref()) {
        Some(summary) => {
            let _ = writeln!(out, "### Tool: {name} - {summary}\n");
        }
        None => {
            let _ = writeln!(out, "### Tool: {name}\n");
        }
    }

    let mut wrote_diff = false;
    for content in &call.content {
        if let ToolCallContent::Diff(diff) = content {
            write_diff(out, &diff.path, diff.old_text.as_deref(), &diff.new_text);
            wrote_diff = true;
        }
    }
    if !wrote_diff && let Some((path, old, new)) = edit_from_input(name, call.raw_input.as_ref()) {
        write_diff(out, Path::new(&path), old.as_deref(), &new);
    }

    let result = result_text(call);
    if !result.trim().is_empty() {
        let label = if call.status == ToolCallStatus::Failed { "Error" } else { "Result" };
        let _ = writeln!(out, "{label}:\n");
        write_fenced(out, "", &result, MAX_RESULT_LINES, MAX_RESULT_CHARS);
    }
}

fn tool_name(call: &ToolCall) -> &str {
    call.meta
        .as_ref()
        .and_then(|meta| meta.get("claudeCode"))
        .and_then(|code| code.get("toolName"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&call.title)
}

fn input_summary(input: Option<&serde_json::Value>) -> Option<String> {
    let input = input?.as_object()?;
    let value = SUMMARY_KEYS
        .iter()
        .filter_map(|key| input.get(*key)?.as_str())
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .find(|value| !value.is_empty())?;
    let value = clean_line(&value);
    if value.chars().count() <= MAX_SUMMARY_CHARS {
        return Some(value);
    }
    let kept: String = value.chars().take(MAX_SUMMARY_CHARS).collect();
    Some(format!("{kept}..."))
}

/// An Edit or Write call whose result no longer carries its diff.
fn edit_from_input(
    name: &str,
    input: Option<&serde_json::Value>,
) -> Option<(String, Option<String>, String)> {
    let input = input?.as_object()?;
    let path = input.get("file_path")?.as_str()?.to_owned();
    match name {
        "Edit" => Some((
            path,
            Some(input.get("old_string")?.as_str()?.to_owned()),
            input.get("new_string")?.as_str()?.to_owned(),
        )),
        "Write" => Some((path, None, input.get("content")?.as_str()?.to_owned())),
        _ => None,
    }
}

fn result_text(call: &ToolCall) -> String {
    let texts: Vec<&str> = call
        .content
        .iter()
        .filter_map(|content| match content {
            ToolCallContent::Content(content) => match &content.content {
                ContentBlock::Text(text) => Some(text.text.as_str()),
                ContentBlock::Image(_) => None,
            },
            ToolCallContent::McpResource(resource) => resource.text.as_deref(),
            _ => None,
        })
        .collect();
    if !texts.is_empty() {
        return clean(&texts.join("\n"));
    }
    match &call.raw_output {
        Some(serde_json::Value::String(text)) => clean(text),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => clean(&serde_json::to_string_pretty(other).unwrap_or_default()),
    }
}

fn write_diff(out: &mut String, path: &Path, old: Option<&str>, new: &str) {
    let path = clean_line(&path.display().to_string());
    let old_label = if old.is_some() { path.as_str() } else { "/dev/null" };
    let text_diff = similar::TextDiff::from_lines(old.unwrap_or(""), new);
    let mut unified = text_diff.unified_diff();
    unified.context_radius(3).header(old_label, &path);
    write_fenced(out, "diff", &clean(&unified.to_string()), MAX_DIFF_LINES, usize::MAX);
}

/// Write `body` as a fenced block no longer than the limits, with a note when cut.
fn write_fenced(out: &mut String, language: &str, body: &str, max_lines: usize, max_chars: usize) {
    let (shown, cut) = truncate(body.trim_end_matches('\n'), max_lines, max_chars);
    let ticks = longest_backtick_run(shown).max(2) + 1;
    let fence = "`".repeat(ticks);
    let _ = writeln!(out, "{fence}{language}\n{shown}\n{fence}\n");
    if cut {
        let total = body.chars().count();
        let kept = shown.chars().count();
        let _ = writeln!(
            out,
            "_Output truncated: the first {kept} of {total} characters are shown._\n"
        );
    }
}

/// The longest prefix within both limits, and whether anything was cut.
fn truncate(text: &str, max_lines: usize, max_chars: usize) -> (&str, bool) {
    let mut lines = 1;
    for (chars, (index, ch)) in text.char_indices().enumerate() {
        if chars >= max_chars {
            return (&text[..index], true);
        }
        if ch == '\n' {
            lines += 1;
            if lines > max_lines {
                return (&text[..index], true);
            }
        }
    }
    (text, false)
}

fn longest_backtick_run(text: &str) -> usize {
    text.split(|ch| ch != '`').map(str::len).max().unwrap_or(0)
}

/// Remove terminal control sequences and stray control characters, keeping text as written.
fn clean(text: &str) -> String {
    strip_ansi(text)
        .replace("\r\n", "\n")
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect()
}

/// A single-line form for headings and header fields.
fn clean_line(text: &str) -> String {
    clean(text).split_whitespace().collect::<Vec<_>>().join(" ")
}
