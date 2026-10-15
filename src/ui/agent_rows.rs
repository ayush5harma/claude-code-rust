// SPDX-License-Identifier: Apache-2.0

use crate::agent::model::{TaskItem, TaskStatus};
use crate::app::agents::{SubagentStatus, is_subagent_task, subagent_status};
use crate::app::{App, MessageBlock, ToolCallInfo};
use crate::ui::theme;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use std::borrow::Cow;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// One subagent as the footer shelf, the Ctrl+T list and the `/agents` view show it.
pub(crate) struct AgentRow<'a> {
    pub task: &'a TaskItem,
    pub status: SubagentStatus,
    pub name: &'a str,
    pub model: &'a str,
    pub model_requested: bool,
    pub requested_model: Option<&'a str>,
    pub definition_model: Option<&'a str>,
    pub raw_input: Option<&'a serde_json::Value>,
    pub parent_id: Option<&'a str>,
    pub now: Cow<'a, str>,
    pub tools: Option<u64>,
    pub tokens: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub accent: Color,
}

pub(crate) fn linked_agent_tool<'a>(app: &'a App, task: &TaskItem) -> Option<&'a ToolCallInfo> {
    let (mi, bi) = app.lookup_tool_call(task.source_tool_call_id.as_deref()?)?;
    match app.transcript.messages.get(mi)?.blocks.get(bi)? {
        MessageBlock::ToolCall(tool) => Some(tool),
        _ => None,
    }
}

fn text<'a>(value: Option<&'a serde_json::Value>, key: &str) -> Option<&'a str> {
    value?.get(key)?.as_str().filter(|s| !s.is_empty())
}

pub(crate) fn agent_row<'a>(app: &'a App, task: &'a TaskItem) -> AgentRow<'a> {
    let metadata = task.metadata.as_ref();
    let raw_input = linked_agent_tool(app, task).and_then(|tool| tool.raw_input.as_ref());
    let agent_type = text(raw_input, "subagent_type").or_else(|| text(metadata, "subagent_type"));
    let name = text(raw_input, "name").or(agent_type).unwrap_or("agent");
    let requested_model = text(raw_input, "model");
    let definition_model = agent_type
        .and_then(|kind| app.sdk_inventory.available_agents.iter().find(|agent| agent.name == kind))
        .and_then(|agent| agent.model.as_deref());
    let resolved_model = text(metadata, "model_display");
    let model = resolved_model.or(requested_model).or(definition_model).unwrap_or("");
    let parent_id = task.source_tool_call_id.as_deref();
    let status = subagent_status(app, task);
    let now = parent_id
        .and_then(|id| app.recent_child_tools(id).next())
        .map(tool_activity)
        .or_else(|| text(metadata, "summary").map(Cow::Borrowed))
        .or_else(|| text(metadata, "last_tool_name").map(Cow::Borrowed))
        .unwrap_or_else(|| match status {
            SubagentStatus::Failed => text(metadata, "error")
                .or_else(|| text(metadata, "terminal_reason"))
                .map_or(Cow::Borrowed("failed"), |reason| Cow::Owned(format!("failed: {reason}"))),
            SubagentStatus::Completed | SubagentStatus::Stopped => Cow::Borrowed(status.label()),
            SubagentStatus::Pending | SubagentStatus::Running | SubagentStatus::Stopping => {
                Cow::Borrowed("")
            }
        });
    let usage = metadata.and_then(|m| m.get("usage"));
    let duration = usage.and_then(|u| u.get("duration_ms")).and_then(serde_json::Value::as_u64);
    let observation = app.sdk_inventory.task_observations.get(&task.task_id);
    let elapsed_ms = duration
        .map(|duration| {
            let extra = if task.status == TaskStatus::InProgress {
                app.sdk_inventory.task_observations.get(&task.task_id).map_or(0, |o| {
                    u64::try_from(o.usage_reported_at.elapsed().as_millis()).unwrap_or(u64::MAX)
                })
            } else {
                0
            };
            duration.saturating_add(extra)
        })
        .or_else(|| {
            observation.map(|o| {
                u64::try_from(
                    o.finished_at
                        .unwrap_or_else(std::time::Instant::now)
                        .saturating_duration_since(o.started_at)
                        .as_millis(),
                )
                .unwrap_or(u64::MAX)
            })
        });
    AgentRow {
        task,
        status,
        name,
        model,
        model_requested: resolved_model.is_none() && requested_model.is_some(),
        requested_model,
        definition_model,
        raw_input,
        parent_id,
        now,
        tools: usage.and_then(|u| u.get("tool_uses")).and_then(serde_json::Value::as_u64),
        tokens: usage.and_then(|u| u.get("total_tokens")).and_then(serde_json::Value::as_u64),
        elapsed_ms,
        accent: app.session_runtime.accent,
    }
}

pub(crate) fn tool_activity(tool: &ToolCallInfo) -> Cow<'_, str> {
    let input = tool.raw_input.as_ref();
    let target = text(input, "file_path")
        .or_else(|| text(input, "path"))
        .map(|path| path.rsplit(['/', '\\']).next().unwrap_or(path))
        .or_else(|| text(input, "command"));
    if let Some(target) = target {
        Cow::Owned(format!("{} {target}", tool.sdk_tool_name))
    } else if tool.title.is_empty() {
        Cow::Borrowed(&tool.sdk_tool_name)
    } else {
        Cow::Borrowed(&tool.title)
    }
}

pub(crate) fn session_agent_rows(app: &App) -> Vec<AgentRow<'_>> {
    let mut tasks: Vec<_> =
        app.sdk_inventory.tasks.iter().filter(|t| is_subagent_task(t)).collect();
    tasks.sort_by(|a, b| {
        let running_a = a.status == TaskStatus::InProgress;
        let running_b = b.status == TaskStatus::InProgress;
        running_b.cmp(&running_a).then_with(|| {
            let a = app.sdk_inventory.task_observations.get(&a.task_id);
            let b = app.sdk_inventory.task_observations.get(&b.task_id);
            if running_a {
                a.map(|o| o.started_at).cmp(&b.map(|o| o.started_at))
            } else {
                b.and_then(|o| o.finished_at).cmp(&a.and_then(|o| o.finished_at))
            }
        })
    });
    tasks.into_iter().map(|t| agent_row(app, t)).collect()
}

pub(crate) struct AgentColumns {
    pub name: usize,
    pub model: usize,
}
impl AgentColumns {
    pub(crate) fn for_rows(rows: &[AgentRow<'_>]) -> Self {
        Self {
            name: rows.iter().map(|r| r.name.width()).max().unwrap_or(0).min(16),
            model: rows.iter().map(|r| r.model.width()).max().unwrap_or(0).min(12),
        }
    }
}

pub(crate) fn status_glyph(row: &AgentRow<'_>) -> (&'static str, Color) {
    match row.status {
        SubagentStatus::Running => ("●", row.accent),
        SubagentStatus::Stopping => ("●", theme::warning()),
        SubagentStatus::Completed => ("✓", theme::success()),
        SubagentStatus::Failed => ("✗", theme::danger()),
        SubagentStatus::Pending | SubagentStatus::Stopped => ("○", theme::tertiary()),
    }
}

pub(crate) fn elapsed_text(ms: Option<u64>) -> String {
    let Some(ms) = ms else {
        return String::new();
    };
    let seconds = ms / 1000;
    if seconds >= 3600 {
        format!("{}h{:02}m", seconds / 3600, seconds / 60 % 60)
    } else if seconds >= 60 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

pub(crate) fn token_text(tokens: u64) -> String {
    if tokens >= 1000 {
        let rounded = tokens / 100 + u64::from(tokens % 100 >= 50);
        let thousands = rounded / 10;
        let tenths = rounded % 10;
        if tenths == 0 { format!("{thousands}k") } else { format!("{thousands}.{tenths}k") }
    } else {
        tokens.to_string()
    }
}

pub(crate) fn clip(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let suffix = if width >= 3 { "..." } else { "" };
    let budget = width.saturating_sub(suffix.len());
    let mut result = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let cells = ch.width().unwrap_or(0);
        if used + cells > budget {
            break;
        }
        result.push(ch);
        used += cells;
    }
    result.push_str(suffix);
    result
}

pub(crate) fn render_agent_row(
    row: &AgentRow<'_>,
    width: u16,
    columns: &AgentColumns,
    compact: bool,
) -> Line<'static> {
    let width = usize::from(width);
    let (glyph, color) = status_glyph(row);
    let elapsed = elapsed_text(row.elapsed_ms);
    let prefix_width = 2 + columns.name + 2 + columns.model;
    let compact = compact || width < 60;
    let mut metrics = elapsed;
    if !compact {
        if let Some(tools) = row.tools {
            metrics = format!("{tools} tools  {metrics}");
        }
        if let Some(tokens) = row.tokens {
            metrics = row.tools.map_or_else(
                || format!("{}  {}", token_text(tokens), elapsed_text(row.elapsed_ms)),
                |tools| {
                    format!(
                        "{tools} tools  {}  {}",
                        token_text(tokens),
                        elapsed_text(row.elapsed_ms)
                    )
                },
            );
        }
        if prefix_width + metrics.width() + 18 > width {
            metrics = row.tools.map_or_else(
                || elapsed_text(row.elapsed_ms),
                |n| format!("{n} tools  {}", elapsed_text(row.elapsed_ms)),
            );
        }
        if prefix_width + metrics.width() + 18 > width {
            metrics = elapsed_text(row.elapsed_ms);
        }
    }
    let mut spans = vec![
        Span::styled(glyph, Style::default().fg(color)),
        Span::raw(" "),
        Span::styled(
            format!("{:<name_width$}", clip(row.name, columns.name), name_width = columns.name),
            Style::default().fg(theme::secondary()),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "{:<model_width$}",
                clip(row.model, columns.model),
                model_width = columns.model
            ),
            Style::default().fg(if row.model_requested {
                theme::tertiary()
            } else {
                theme::secondary()
            }),
        ),
    ];
    let available = width.saturating_sub(prefix_width + metrics.width() + 2);
    if !compact && available >= 12 {
        let task = row.task.description.as_deref().unwrap_or(&row.task.subject);
        let now_width = if available >= 30 { (available / 2).min(36) } else { 0 };
        let task_width = available.saturating_sub(now_width + usize::from(now_width > 0) * 2);
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("{:<task_width$}", clip(task, task_width)),
            Style::default().fg(theme::secondary()),
        ));
        if now_width > 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                clip(&row.now, now_width),
                Style::default().fg(theme::tertiary()),
            ));
        }
    }
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used + metrics.width()))));
    spans.push(Span::styled(metrics, Style::default().fg(theme::tertiary())));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{events::ClientEvent, model};
    fn tool(app: &mut App, id: &str, name: &str, input: serde_json::Value, parent: Option<&str>) {
        let mut meta = serde_json::json!({"claudeCode":{"toolName":name}});
        if let Some(parent) = parent {
            meta["claudeCode"]["parentToolUseId"] = serde_json::json!(parent);
        }
        crate::app::handle_client_event(
            app,
            ClientEvent::SessionUpdate {
                session_id: "session".into(),
                update: model::SessionUpdate::ToolCall(
                    model::ToolCall::new(id, name)
                        .status(model::ToolCallStatus::Completed)
                        .raw_input(input)
                        .meta(meta),
                ),
            },
        );
    }
    #[test]
    fn model_sources_and_child_activity_follow_facts_and_pruned_index() {
        let mut app = App::test_default();
        app.session_runtime.session_id = Some("session".into());
        tool(
            &mut app,
            "root",
            "Agent",
            serde_json::json!({"name":"mapper","subagent_type":"Explore","model":"sonnet"}),
            None,
        );
        let mut task = TaskItem::new("agent", "Map footer", TaskStatus::InProgress);
        task.source_tool_call_id = Some("root".into());
        task.metadata =
            Some(serde_json::json!({"task_type":"local_agent","summary":"Reading source"}));
        app.sdk_inventory
            .available_agents
            .push(model::AvailableAgent::new("Explore", "map").model("haiku"));
        let row = agent_row(&app, &task);
        assert_eq!(row.model, "sonnet");
        assert!(row.model_requested);
        assert_eq!(row.name, "mapper");
        task.metadata.as_mut().expect("metadata")["model_display"] =
            serde_json::json!("Sonnet 5.5");
        let row = agent_row(&app, &task);
        assert_eq!(row.model, "Sonnet 5.5");
        assert!(!row.model_requested);
        for index in 0..10 {
            tool(
                &mut app,
                &format!("child-{index}"),
                "Read",
                serde_json::json!({"file_path":format!("/fixture/file-{index}.rs")}),
                Some("root"),
            );
        }
        assert_eq!(app.recent_child_tools("root").count(), 8);
        assert_eq!(agent_row(&app, &task).now, "Read file-9.rs");
        let (mi, bi) = app.lookup_tool_call("child-8").expect("child");
        app.clear_tool_call_index();
        app.index_tool_call("child-8".into(), mi, bi);
        assert_eq!(agent_row(&app, &task).now, "Read file-8.rs");
        app.clear_tool_scope_tracking();
        assert_eq!(agent_row(&app, &task).now, "Reading source");
        task.metadata
            .as_mut()
            .expect("metadata")
            .as_object_mut()
            .expect("object")
            .remove("model_display");
        task.source_tool_call_id = None;
        assert_eq!(agent_row(&app, &task).model, "");
    }
}
