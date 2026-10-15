// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang
use super::{App, InvalidationLevel, MessageBlock, ToolCallInfo};
use crate::agent::model;
use std::collections::{BTreeSet, HashSet};

pub(super) fn apply_task_state_update(app: &mut App, update: model::TaskStateUpdate) {
    observe_task_update(app, &update);
    let selects_plan = update.source != model::TaskUpdateSource::BackgroundTasks
        && ((update.source == model::TaskUpdateSource::List && update.is_complete_snapshot)
            || update.tasks.iter().any(is_user_plan_task)
            || update.removed_task_ids.iter().any(|id| {
                app.sdk_inventory
                    .tasks
                    .iter()
                    .any(|task| task.task_id == *id && is_user_plan_task(task))
            }));
    if selects_plan {
        app.footer.plan_source = super::state::PlanSource::Tasks;
    }
    let previous_tasks = app.sdk_inventory.tasks.clone();
    let removed: HashSet<&str> =
        update.removed_task_ids.iter().map(std::string::String::as_str).collect();

    if update.is_complete_snapshot {
        if update.source == model::TaskUpdateSource::BackgroundTasks {
            app.sdk_inventory.tasks.retain(|task| {
                !is_sdk_background_task(task)
                    && !removed.contains(task.task_id.as_str())
                    && !update.tasks.iter().any(|incoming| incoming.task_id == task.task_id)
            });
            for task in update.tasks {
                if !removed.contains(task.task_id.as_str()) {
                    app.sdk_inventory.tasks.push(task);
                }
            }
            refresh_task_tool_displays_with_previous(app, &previous_tasks);
            return;
        }
        app.sdk_inventory.tasks = update
            .tasks
            .into_iter()
            .filter(|task| !removed.contains(task.task_id.as_str()))
            .collect();
        refresh_task_tool_displays_with_previous(app, &previous_tasks);
        return;
    }

    if !removed.is_empty() {
        app.sdk_inventory.tasks.retain(|task| !removed.contains(task.task_id.as_str()));
    }

    for task in update.tasks {
        if removed.contains(task.task_id.as_str()) {
            continue;
        }
        if let Some(existing) =
            app.sdk_inventory.tasks.iter_mut().find(|existing| existing.task_id == task.task_id)
        {
            *existing = task;
        } else {
            app.sdk_inventory.tasks.push(task);
        }
    }
    refresh_task_tool_displays_with_previous(app, &previous_tasks);
}

fn observe_task_update(app: &mut App, update: &model::TaskStateUpdate) {
    let now = std::time::Instant::now();
    for id in &update.removed_task_ids {
        app.sdk_inventory.task_observations.remove(id);
    }
    for task in &update.tasks {
        if update.removed_task_ids.contains(&task.task_id) {
            continue;
        }
        let duration_ms = task
            .metadata
            .as_ref()
            .and_then(|m| m.get("usage"))
            .and_then(|u| u.get("duration_ms"))
            .and_then(serde_json::Value::as_u64);
        let observation =
            app.sdk_inventory.task_observations.entry(task.task_id.clone()).or_insert(
                super::state::sdk_inventory::TaskObservation {
                    started_at: now,
                    finished_at: None,
                    usage_reported_at: now,
                    duration_ms,
                },
            );
        let usage_changed = app
            .sdk_inventory
            .tasks
            .iter()
            .find(|existing| existing.task_id == task.task_id)
            .is_some_and(|existing| {
                existing.metadata.as_ref().and_then(|m| m.get("usage"))
                    != task.metadata.as_ref().and_then(|m| m.get("usage"))
            });
        if task.status == model::TaskStatus::InProgress
            && app
                .sdk_inventory
                .tasks
                .iter()
                .find(|existing| existing.task_id == task.task_id)
                .is_some_and(|existing| existing.status == model::TaskStatus::Pending)
        {
            observation.started_at = now;
        }
        if observation.duration_ms != duration_ms || usage_changed {
            observation.duration_ms = duration_ms;
            observation.usage_reported_at = now;
        }
        if task.status == model::TaskStatus::Completed && observation.finished_at.is_none() {
            observation.finished_at = Some(now);
        } else if task.status == model::TaskStatus::InProgress {
            observation.finished_at = None;
        }
    }
    if update.is_complete_snapshot {
        app.sdk_inventory.task_observations.retain(|id, _| {
            update.tasks.iter().any(|t| t.task_id == *id)
                || (update.source == model::TaskUpdateSource::BackgroundTasks
                    && app
                        .sdk_inventory
                        .tasks
                        .iter()
                        .any(|t| t.task_id == *id && !is_sdk_background_task(t)))
        });
    }
}

pub(crate) fn is_sdk_background_task(task: &model::TaskItem) -> bool {
    task.metadata
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .and_then(|metadata| metadata.get("sdk_background_task"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn is_user_plan_task(task: &model::TaskItem) -> bool {
    !is_sdk_background_task(task)
        && !task.metadata.as_ref().is_some_and(|metadata| {
            metadata.get("task_type").is_some()
                || metadata.get("background_source").is_some()
                || metadata.get("sdk_lifecycle_task").and_then(serde_json::Value::as_bool)
                    == Some(true)
        })
}

pub(super) fn refresh_task_tool_displays(app: &mut App) -> bool {
    let previous_tasks = app.sdk_inventory.tasks.clone();
    refresh_task_tool_displays_with_previous(app, &previous_tasks)
}

fn refresh_task_tool_displays_with_previous(
    app: &mut App,
    previous_tasks: &[model::TaskItem],
) -> bool {
    let is_running = |task: &&model::TaskItem| {
        task.status == model::TaskStatus::InProgress && crate::app::agents::is_subagent_task(task)
    };
    let running = app.sdk_inventory.tasks.iter().any(|t| is_running(&t));
    if running && !previous_tasks.iter().any(|t| is_running(&t)) {
        app.sdk_inventory.agent_shelf_wave_started_at = app
            .sdk_inventory
            .tasks
            .iter()
            .filter(is_running)
            .filter_map(|t| {
                app.sdk_inventory.task_observations.get(&t.task_id).map(|o| o.started_at)
            })
            .min();
    } else if !running {
        app.sdk_inventory.agent_shelf_wave_started_at = None;
    }
    crate::app::agents::forget_unlisted_selection(app);
    let current_tasks = app.sdk_inventory.tasks.clone();
    let mut dirty_blocks = Vec::new();

    for (message_idx, message) in app.transcript.messages.iter_mut().enumerate() {
        for (block_idx, block) in message.blocks.iter_mut().enumerate() {
            let MessageBlock::ToolCall(tool_call) = block else {
                continue;
            };
            if refresh_task_tool_display(tool_call.as_mut(), &current_tasks, previous_tasks) {
                tool_call.invalidate_render_cache();
                dirty_blocks.push((message_idx, block_idx));
            }
        }
    }

    if dirty_blocks.is_empty() {
        return false;
    }

    let mut dirty_messages = BTreeSet::new();
    for (message_idx, block_idx) in dirty_blocks {
        app.sync_render_cache_slot(message_idx, block_idx);
        dirty_messages.insert(message_idx);
    }
    for message_idx in dirty_messages {
        app.recompute_message_retained_bytes(message_idx);
        app.invalidate_layout(InvalidationLevel::MessageChanged(message_idx));
    }
    true
}

fn refresh_task_tool_display(
    tool_call: &mut ToolCallInfo,
    current_tasks: &[model::TaskItem],
    previous_tasks: &[model::TaskItem],
) -> bool {
    match tool_call.sdk_tool_name.as_str() {
        "TaskCreate" => refresh_task_create_tool(tool_call, current_tasks, previous_tasks),
        "TaskUpdate" => refresh_task_update_tool(tool_call, current_tasks, previous_tasks),
        _ => false,
    }
}

fn refresh_task_create_tool(
    tool_call: &mut ToolCallInfo,
    current_tasks: &[model::TaskItem],
    previous_tasks: &[model::TaskItem],
) -> bool {
    let Some(task) = task_by_source_tool_call_id(current_tasks, previous_tasks, &tool_call.id)
    else {
        return false;
    };
    sync_string(&mut tool_call.title, format!("Create task #{}: {}", task.task_id, task.subject))
}

fn refresh_task_update_tool(
    tool_call: &mut ToolCallInfo,
    current_tasks: &[model::TaskItem],
    previous_tasks: &[model::TaskItem],
) -> bool {
    let Some(input) = tool_call.raw_input.as_ref().and_then(serde_json::Value::as_object) else {
        return false;
    };
    let Some(task_id) = json_string(input, "taskId") else {
        return false;
    };
    let task = task_by_id(current_tasks, previous_tasks, task_id);
    let subject = json_string(input, "subject").or_else(|| task.map(|task| task.subject.as_str()));
    let status = json_string(input, "status");
    let action = task_update_action(status);
    let mut changed =
        sync_string(&mut tool_call.title, task_update_title(action, task_id, subject));

    if !matches!(tool_call.status, model::ToolCallStatus::Failed | model::ToolCallStatus::Killed) {
        let desired_content = task_update_content(input, task, status);
        if tool_call.content != desired_content {
            tool_call.content = desired_content;
            changed = true;
        }
    }
    changed
}

fn task_by_source_tool_call_id<'a>(
    current_tasks: &'a [model::TaskItem],
    previous_tasks: &'a [model::TaskItem],
    tool_call_id: &str,
) -> Option<&'a model::TaskItem> {
    current_tasks
        .iter()
        .find(|task| task.source_tool_call_id.as_deref() == Some(tool_call_id))
        .or_else(|| {
            previous_tasks
                .iter()
                .find(|task| task.source_tool_call_id.as_deref() == Some(tool_call_id))
        })
}

fn task_by_id<'a>(
    current_tasks: &'a [model::TaskItem],
    previous_tasks: &'a [model::TaskItem],
    task_id: &str,
) -> Option<&'a model::TaskItem> {
    current_tasks
        .iter()
        .find(|task| task.task_id == task_id)
        .or_else(|| previous_tasks.iter().find(|task| task.task_id == task_id))
}

fn task_update_action(status: Option<&str>) -> &'static str {
    match status {
        Some("in_progress" | "running") => "Start",
        Some("completed") => "Complete",
        Some("deleted") => "Delete",
        Some("pending") => "Queue",
        _ => "Update",
    }
}

fn task_update_title(action: &str, task_id: &str, subject: Option<&str>) -> String {
    if let Some(subject) = subject.filter(|subject| !subject.trim().is_empty()) {
        format!("{action} task #{task_id}: {subject}")
    } else {
        format!("{action} task #{task_id}")
    }
}

fn task_update_content(
    input: &serde_json::Map<String, serde_json::Value>,
    task: Option<&model::TaskItem>,
    status: Option<&str>,
) -> Vec<model::ToolCallContent> {
    if !matches!(status, Some("in_progress" | "running")) {
        return Vec::new();
    }
    let activity = json_string(input, "activeForm")
        .or_else(|| task.and_then(|task| task.active_form.as_deref()))
        .filter(|activity| !activity.trim().is_empty());
    activity
        .map(|activity| vec![model::ToolCallContent::from(format!("Activity: {activity}"))])
        .unwrap_or_default()
}

fn json_string<'a>(
    input: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    input.get(key).and_then(serde_json::Value::as_str).filter(|value| !value.is_empty())
}

fn sync_string(current: &mut String, next: String) -> bool {
    if current == &next {
        return false;
    }
    *current = next;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{BlockCache, ChatMessage, MessageBlock, MessageRole, ToolCallInfo};
    use serde_json::json;

    fn task(id: &str, subject: &str, status: model::TaskStatus) -> model::TaskItem {
        model::TaskItem::new(id, subject, status)
    }

    fn background_task(id: &str, subject: &str) -> model::TaskItem {
        let mut task = model::TaskItem::new(id, subject, model::TaskStatus::InProgress);
        task.metadata = Some(json!({ "sdk_background_task": true }));
        task
    }

    fn task_tool_call(
        id: &str,
        sdk_tool_name: &str,
        title: &str,
        raw_input: serde_json::Value,
        status: model::ToolCallStatus,
    ) -> ToolCallInfo {
        ToolCallInfo {
            id: id.to_owned(),
            source_message_uuids: Vec::new(),
            title: title.to_owned(),
            sdk_tool_name: sdk_tool_name.to_owned(),
            raw_input: Some(raw_input),
            raw_input_bytes: 0,
            locations: Vec::new(),
            output_metadata: None,
            task_metadata: None,
            status,
            content: Vec::new(),
            hidden: false,
            terminal_id: None,
            terminal_command: None,
            terminal_output: None,
            terminal_output_len: 0,
            cache: BlockCache::default(),
            pending_permission: None,
            pending_question: None,
            history: crate::app::ToolCallHistory::Live,
        }
    }

    fn insert_tool_call(app: &mut App, tool_call: ToolCallInfo) {
        let id = tool_call.id.clone();
        let message_idx = app.transcript.messages.len();
        app.push_message_tracked(ChatMessage::new(
            MessageRole::Assistant,
            vec![MessageBlock::ToolCall(Box::new(tool_call))],
            None,
        ));
        app.index_tool_call(id, message_idx, 0);
    }

    fn only_tool_call(app: &App) -> &ToolCallInfo {
        let MessageBlock::ToolCall(tool_call) = &app.transcript.messages[0].blocks[0] else {
            panic!("expected task tool call");
        };
        tool_call
    }

    fn text_content(tool_call: &ToolCallInfo) -> Vec<String> {
        tool_call
            .content
            .iter()
            .filter_map(|content| {
                let model::ToolCallContent::Content(content) = content else {
                    return None;
                };
                let model::ContentBlock::Text(text) = &content.content else {
                    return None;
                };
                Some(text.text.clone())
            })
            .collect()
    }

    #[test]
    fn singular_updates_merge_by_task_id() {
        let mut app = App::test_default();
        app.sdk_inventory.tasks.push(task("task-1", "old", model::TaskStatus::Pending));

        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Update).tasks(vec![task(
                "task-1",
                "new",
                model::TaskStatus::InProgress,
            )]),
        );

        assert_eq!(app.sdk_inventory.tasks.len(), 1);
        assert_eq!(app.sdk_inventory.tasks[0].subject, "new");
        assert_eq!(app.sdk_inventory.tasks[0].status, model::TaskStatus::InProgress);
    }

    #[test]
    fn confirmed_task_updates_select_tasks_but_background_and_agent_updates_do_not() {
        let mut app = App::test_default();
        app.footer.plan_source = crate::app::state::PlanSource::Todo;
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::BackgroundTasks)
                .tasks(vec![background_task("background", "Index files")]),
        );
        assert!(app.footer.plan_source == crate::app::state::PlanSource::Todo);

        let mut agent = task("agent-1", "Review", model::TaskStatus::InProgress);
        agent.metadata =
            Some(serde_json::json!({"task_type":"local_agent","subagent_type":"general-purpose"}));
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle).tasks(vec![agent]),
        );
        assert!(app.footer.plan_source == crate::app::state::PlanSource::Todo);

        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Create).tasks(vec![task(
                "plan-1",
                "Ship",
                model::TaskStatus::Pending,
            )]),
        );
        assert!(app.footer.plan_source == crate::app::state::PlanSource::Tasks);

        app.footer.apply_todo_plan(vec![model::TodoPlanItem {
            content: "New todo".to_owned(),
            active_form: None,
            status: model::TaskStatus::InProgress,
        }]);
        assert!(app.footer.plan_source == crate::app::state::PlanSource::Todo);
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::List)
                .tasks(vec![task("plan-1", "Ship", model::TaskStatus::Completed)])
                .complete_snapshot(true),
        );
        assert!(app.footer.plan_source == crate::app::state::PlanSource::Tasks);
    }

    #[test]
    fn complete_empty_task_list_clears_a_selected_task_plan() {
        let mut app = App::test_default();
        app.footer.plan_source = crate::app::state::PlanSource::Todo;
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::List).complete_snapshot(true),
        );
        assert!(app.footer.plan_source == crate::app::state::PlanSource::Tasks);
        assert!(app.sdk_inventory.tasks.is_empty());
    }

    #[test]
    fn removed_task_ids_remove_state() {
        let mut app = App::test_default();
        app.sdk_inventory.tasks.push(task("task-1", "one", model::TaskStatus::Pending));
        app.sdk_inventory.tasks.push(task("task-2", "two", model::TaskStatus::Pending));

        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Update)
                .removed_task_ids(vec!["task-1".to_owned()]),
        );

        assert_eq!(app.sdk_inventory.tasks.len(), 1);
        assert_eq!(app.sdk_inventory.tasks[0].task_id, "task-2");
    }

    #[test]
    fn complete_snapshot_replaces_membership() {
        let mut app = App::test_default();
        app.sdk_inventory.tasks.push(task("task-1", "one", model::TaskStatus::Pending));
        app.sdk_inventory.tasks.push(task("task-2", "two", model::TaskStatus::Pending));

        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::List)
                .tasks(vec![task("task-2", "two updated", model::TaskStatus::Completed)])
                .complete_snapshot(true),
        );

        assert_eq!(app.sdk_inventory.tasks.len(), 1);
        assert_eq!(app.sdk_inventory.tasks[0].task_id, "task-2");
        assert_eq!(app.sdk_inventory.tasks[0].subject, "two updated");
    }

    #[test]
    fn background_task_snapshot_replaces_only_background_membership() {
        let mut app = App::test_default();
        app.sdk_inventory.tasks.push(task(
            "task-list",
            "from task list",
            model::TaskStatus::Pending,
        ));
        app.sdk_inventory.tasks.push(background_task("bg-old", "old background"));

        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::BackgroundTasks)
                .tasks(vec![background_task("bg-new", "new background")])
                .removed_task_ids(vec!["bg-old".to_owned()])
                .complete_snapshot(true),
        );

        assert_eq!(app.sdk_inventory.tasks.len(), 2);
        assert!(app.sdk_inventory.tasks.iter().any(|task| task.task_id == "task-list"));
        assert!(app.sdk_inventory.tasks.iter().any(|task| task.task_id == "bg-new"));
        assert!(!app.sdk_inventory.tasks.iter().any(|task| task.task_id == "bg-old"));
    }

    #[test]
    fn background_snapshot_replaces_a_foreground_subagent_by_task_id() {
        let mut app = App::test_default();
        let mut foreground = task("agent-1", "Research", model::TaskStatus::InProgress);
        foreground.metadata = Some(serde_json::json!({ "task_type": "local_agent" }));
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle).tasks(vec![foreground]),
        );

        let mut background = background_task("agent-1", "Research");
        background.metadata = Some(serde_json::json!({
            "task_type": "local_agent", "sdk_background_task": true
        }));
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::BackgroundTasks)
                .tasks(vec![background])
                .complete_snapshot(true),
        );
        assert_eq!(app.sdk_inventory.tasks.len(), 1);
        assert_eq!(app.sdk_inventory.tasks[0].task_id, "agent-1");

        let mut completed = task("agent-1", "Research", model::TaskStatus::Completed);
        completed.metadata = Some(serde_json::json!({
            "task_type": "local_agent", "terminal_status": "completed"
        }));
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle).tasks(vec![completed]),
        );
        assert_eq!(app.sdk_inventory.tasks.len(), 1);
        assert_eq!(app.sdk_inventory.tasks[0].status, model::TaskStatus::Completed);
    }

    #[test]
    fn ambient_inventory_task_does_not_enter_active_subagent_tracking() {
        let mut app = App::test_default();
        let mut ambient = background_task("ambient-watch", "watch artifact updates");
        ambient.metadata = Some(serde_json::json!({
            "sdk_background_task": true,
            "ambient": true
        }));

        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::BackgroundTasks)
                .tasks(vec![ambient])
                .complete_snapshot(true),
        );

        assert_eq!(app.sdk_inventory.tasks.len(), 1);
        assert!(app.turn.active_task_ids.is_empty());
    }

    #[test]
    fn create_tool_title_uses_numbered_task_state() {
        let mut app = App::test_default();
        insert_tool_call(
            &mut app,
            task_tool_call(
                "tool-create",
                "TaskCreate",
                "Create task: Scaffold Next.js 15 app via create-next-app",
                json!({
                    "subject": "Scaffold Next.js 15 app via create-next-app",
                    "description": "Run create-next-app"
                }),
                model::ToolCallStatus::Completed,
            ),
        );
        let mut created =
            task("1", "Scaffold Next.js 15 app via create-next-app", model::TaskStatus::Pending);
        created.source_tool_call_id = Some("tool-create".to_owned());

        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Create).tasks(vec![created]),
        );

        assert_eq!(
            only_tool_call(&app).title,
            "Create task #1: Scaffold Next.js 15 app via create-next-app"
        );
    }

    #[test]
    fn in_progress_update_uses_task_subject_and_activity() {
        let mut app = App::test_default();
        app.sdk_inventory.tasks.push(
            task("1", "Scaffold Next.js 15 app via create-next-app", model::TaskStatus::Pending)
                .active_form("Scaffolding Next.js app"),
        );
        insert_tool_call(
            &mut app,
            task_tool_call(
                "tool-update",
                "TaskUpdate",
                "Update task: 1",
                json!({ "taskId": "1", "status": "in_progress" }),
                model::ToolCallStatus::Completed,
            ),
        );

        refresh_task_tool_displays(&mut app);

        let tool_call = only_tool_call(&app);
        assert_eq!(tool_call.title, "Start task #1: Scaffold Next.js 15 app via create-next-app");
        assert_eq!(text_content(tool_call), vec!["Activity: Scaffolding Next.js app"]);
    }

    #[test]
    fn completed_update_clears_redundant_activity_content() {
        let mut app = App::test_default();
        app.sdk_inventory.tasks.push(task(
            "1",
            "Scaffold Next.js 15 app via create-next-app",
            model::TaskStatus::Completed,
        ));
        let mut update = task_tool_call(
            "tool-update",
            "TaskUpdate",
            "Update task: 1",
            json!({ "taskId": "1", "status": "completed" }),
            model::ToolCallStatus::Completed,
        );
        update.content = vec![model::ToolCallContent::from("Activity: Scaffolding Next.js app")];
        insert_tool_call(&mut app, update);

        refresh_task_tool_displays(&mut app);

        let tool_call = only_tool_call(&app);
        assert_eq!(
            tool_call.title,
            "Complete task #1: Scaffold Next.js 15 app via create-next-app"
        );
        assert!(tool_call.content.is_empty());
    }

    #[test]
    fn deleted_update_uses_previous_task_subject_after_removal() {
        let mut app = App::test_default();
        app.sdk_inventory.tasks.push(task(
            "1",
            "Remove temporary files",
            model::TaskStatus::Pending,
        ));
        insert_tool_call(
            &mut app,
            task_tool_call(
                "tool-delete",
                "TaskUpdate",
                "Update task: 1",
                json!({ "taskId": "1", "status": "deleted" }),
                model::ToolCallStatus::Completed,
            ),
        );

        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Update)
                .removed_task_ids(vec!["1".to_owned()]),
        );

        assert!(app.sdk_inventory.tasks.is_empty());
        assert_eq!(only_tool_call(&app).title, "Delete task #1: Remove temporary files");
    }
    #[test]
    fn background_snapshot_updates_lifecycle_rows_without_duplicate_membership() {
        let mut app = App::test_default();
        let mut live =
            model::TaskItem::new("agent", "Before snapshot", model::TaskStatus::InProgress);
        live.metadata = Some(
            serde_json::json!({"task_type":"local_agent","model_display":"Sonnet 5.5","usage":{"total_tokens":34000,"tool_uses":12,"duration_ms":62000}}),
        );
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle)
                .tasks(vec![live.clone()]),
        );
        live.subject = "After snapshot".into();
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::BackgroundTasks)
                .complete_snapshot(true)
                .tasks(vec![live]),
        );
        assert_eq!(app.sdk_inventory.tasks.len(), 1);
        assert_eq!(app.sdk_inventory.tasks[0].subject, "After snapshot");
        let row = crate::ui::agent_rows::agent_row(&app, &app.sdk_inventory.tasks[0]);
        assert_eq!(row.model, "Sonnet 5.5");
        assert_eq!(row.tools, Some(12));
        assert_eq!(row.tokens, Some(34000));
    }

    #[test]
    fn task_usage_clock_advances_running_and_freezes_finished_duration() {
        let mut app = App::test_default();
        let mut live = model::TaskItem::new("agent", "Clock", model::TaskStatus::Pending);
        live.metadata =
            Some(serde_json::json!({"task_type":"local_agent","usage":{"duration_ms":40000}}));
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle)
                .tasks(vec![live.clone()]),
        );
        assert!(app.sdk_inventory.task_observations["agent"].finished_at.is_none());
        live.status = model::TaskStatus::InProgress;
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle)
                .tasks(vec![live.clone()]),
        );
        app.sdk_inventory.task_observations.get_mut("agent").expect("clock").usage_reported_at =
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(3))
                .expect("test clock permits subtraction");
        let row = crate::ui::agent_rows::agent_row(&app, &app.sdk_inventory.tasks[0]);
        assert!(row.elapsed_ms.is_some_and(|ms| (43000..44000).contains(&ms)));
        live.status = model::TaskStatus::Completed;
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle).tasks(vec![live]),
        );
        assert!(app.sdk_inventory.task_observations["agent"].finished_at.is_some());
        assert_eq!(
            crate::ui::agent_rows::agent_row(&app, &app.sdk_inventory.tasks[0]).elapsed_ms,
            Some(40000)
        );
    }
    #[test]
    fn lifecycle_shell_and_mcp_inventory_never_become_plan_steps() {
        let mut app = App::test_default();
        app.footer.apply_todo_plan(vec![model::TodoPlanItem {
            content: "User plan".into(),
            active_form: None,
            status: model::TaskStatus::InProgress,
        }]);
        for kind in ["local_bash", "mcp_task"] {
            let mut background =
                model::TaskItem::new(kind, "Background work", model::TaskStatus::InProgress);
            background.metadata = Some(serde_json::json!({"task_type":kind}));
            apply_task_state_update(
                &mut app,
                model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle)
                    .tasks(vec![background]),
            );
        }
        assert!(app.footer.plan_source == crate::app::PlanSource::Todo);
        assert!(app.sdk_inventory.tasks.iter().all(|task| !is_user_plan_task(task)));
        let footer = crate::ui::footer_rows::serialize_footer_rows(&app, 100);
        let plan: String = footer.rows[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(plan.contains("0/1 User plan"), "{plan}");
    }
    #[test]
    fn notification_only_lifecycle_task_never_selects_plan_source() {
        let mut app = App::test_default();
        let mut task = model::TaskItem::new(
            "notification",
            "Background notification",
            model::TaskStatus::Completed,
        );
        task.metadata =
            Some(serde_json::json!({"sdk_lifecycle_task":true,"terminal_status":"completed"}));
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle).tasks(vec![task]),
        );
        assert!(app.footer.plan_source == crate::app::PlanSource::None);
        assert!(!is_user_plan_task(&app.sdk_inventory.tasks[0]));
    }
    #[test]
    fn unreported_usage_clock_uses_observed_start_and_freezes_on_completion() {
        let mut app = App::test_default();
        let mut task = model::TaskItem::new("clock", "Clock", model::TaskStatus::InProgress);
        task.metadata = Some(serde_json::json!({"task_type":"local_agent"}));
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle)
                .tasks(vec![task.clone()]),
        );
        app.sdk_inventory.task_observations.get_mut("clock").expect("clock").started_at =
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(3))
                .expect("test clock permits subtraction");
        assert!(
            crate::ui::agent_rows::agent_row(&app, &app.sdk_inventory.tasks[0])
                .elapsed_ms
                .is_some_and(|ms| (3000..4000).contains(&ms))
        );
        task.status = model::TaskStatus::Completed;
        apply_task_state_update(
            &mut app,
            model::TaskStateUpdate::new(model::TaskUpdateSource::Lifecycle).tasks(vec![task]),
        );
        let before = crate::ui::agent_rows::agent_row(&app, &app.sdk_inventory.tasks[0]).elapsed_ms;
        app.sdk_inventory.task_observations.get_mut("clock").expect("clock").usage_reported_at =
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(20))
                .expect("test clock permits subtraction");
        assert_eq!(
            crate::ui::agent_rows::agent_row(&app, &app.sdk_inventory.tasks[0]).elapsed_ms,
            before
        );
    }
}
