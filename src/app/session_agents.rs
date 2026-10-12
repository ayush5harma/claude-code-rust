// SPDX-License-Identifier: Apache-2.0

//! Current-session subagent browsing. The SDK task inventory remains the status authority.

use crate::agent::events::ClientEvent;
use crate::agent::types::{SessionAgentsAction, SubagentMessage};
use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Default)]
pub(crate) struct SessionAgentsState {
    pub session_id: Option<String>,
    pub agent_ids: Vec<String>,
    pub selected: usize,
    pub messages: Vec<SubagentMessage>,
    pub message_scroll: u16,
    pub error: Option<String>,
    pub loading: bool,
    pending: Option<(String, SessionAgentsAction, Option<String>)>,
}

impl SessionAgentsState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.agent_ids.get(self.selected).map(String::as_str)
    }
}

fn next_request(
    app: &mut App,
    action: SessionAgentsAction,
    agent_id: Option<String>,
) -> Option<(String, String)> {
    let sid = app.session_runtime.session_id.as_ref()?.to_string();
    let request_id = uuid::Uuid::new_v4().to_string();
    app.session_agents.session_id = Some(sid.clone());
    app.session_agents.pending = Some((request_id.clone(), action, agent_id));
    app.session_agents.loading = true;
    app.session_agents.error = None;
    Some((sid, request_id))
}

pub(crate) fn refresh(app: &mut App) {
    if app.session_runtime.conn.is_none() {
        app.session_agents.clear();
        return;
    }
    let Some((sid, request_id)) = next_request(app, SessionAgentsAction::ListSessionAgents, None)
    else {
        app.session_agents.clear();
        return;
    };
    let Some(conn) = app.session_runtime.conn.as_ref() else {
        app.session_agents.clear();
        return;
    };
    if let Err(error) = conn.list_session_agents(sid, request_id) {
        app.session_agents.loading = false;
        app.session_agents.error = Some(format!("Cannot load agents: {error}"));
    }
}

fn inspect_selected(app: &mut App) {
    if app.session_runtime.conn.is_none() {
        app.session_agents.clear();
        return;
    }
    let Some(agent_id) = app.session_agents.selected_id().map(str::to_owned) else {
        app.session_agents.messages.clear();
        return;
    };
    let Some((sid, request_id)) =
        next_request(app, SessionAgentsAction::GetSubagentMessages, Some(agent_id.clone()))
    else {
        return;
    };
    app.session_agents.messages.clear();
    app.session_agents.message_scroll = 0;
    let Some(conn) = app.session_runtime.conn.as_ref() else {
        app.session_agents.clear();
        return;
    };
    if let Err(error) = conn.get_subagent_messages(sid, agent_id, request_id) {
        app.session_agents.loading = false;
        app.session_agents.error = Some(format!("Cannot inspect agent: {error}"));
    }
}

pub(crate) fn is_running_task(app: &App, agent_id: &str) -> bool {
    app.sdk_inventory.tasks.iter().any(|task| {
        if task.task_id != agent_id || task.status != crate::agent::model::TaskStatus::InProgress {
            return false;
        }
        let Some(metadata) = task.metadata.as_ref().and_then(serde_json::Value::as_object) else {
            return false;
        };
        metadata.get("subagent_type").is_some_and(serde_json::Value::is_string)
            || metadata.get("task_type").and_then(serde_json::Value::as_str) == Some("local_agent")
    })
}

pub(crate) fn stop_selected(app: &mut App) {
    if app.session_runtime.conn.is_none() {
        app.session_agents.clear();
        return;
    }
    let Some(agent_id) = app.session_agents.selected_id().map(str::to_owned) else {
        return;
    };
    if !is_running_task(app, &agent_id) {
        app.session_agents.error = Some("Selected agent has no running task to stop.".to_owned());
        return;
    }
    let Some((sid, request_id)) =
        next_request(app, SessionAgentsAction::StopSubagentTask, Some(agent_id.clone()))
    else {
        return;
    };
    let Some(conn) = app.session_runtime.conn.as_ref() else {
        app.session_agents.clear();
        return;
    };
    if let Err(error) = conn.stop_subagent_task(sid, agent_id, request_id) {
        app.session_agents.loading = false;
        app.session_agents.error = Some(format!("Cannot stop agent: {error}"));
    }
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
        return false;
    }
    match key.code {
        KeyCode::Up => {
            app.session_agents.selected = app.session_agents.selected.saturating_sub(1);
            inspect_selected(app);
            true
        }
        KeyCode::Down => {
            app.session_agents.selected = (app.session_agents.selected + 1)
                .min(app.session_agents.agent_ids.len().saturating_sub(1));
            inspect_selected(app);
            true
        }
        KeyCode::PageDown => {
            app.session_agents.message_scroll = app.session_agents.message_scroll.saturating_add(8);
            true
        }
        KeyCode::PageUp => {
            app.session_agents.message_scroll = app.session_agents.message_scroll.saturating_sub(8);
            true
        }
        KeyCode::Char('r' | 'R') => {
            refresh(app);
            true
        }
        KeyCode::Char('s' | 'S') => {
            stop_selected(app);
            true
        }
        _ => false,
    }
}

pub(crate) fn apply_result(app: &mut App, event: ClientEvent) {
    let ClientEvent::SessionAgentsResultReceived {
        session_id,
        request_id,
        action,
        agent_id,
        agent_ids,
        messages,
        error,
    } = event
    else {
        return;
    };
    if app.session_runtime.session_id.as_ref().is_none_or(|sid| sid.to_string() != session_id)
        || app.session_agents.session_id.as_deref() != Some(session_id.as_str())
    {
        return;
    }
    let Some((expected_id, expected_action, expected_agent)) = app.session_agents.pending.as_ref()
    else {
        return;
    };
    if request_id.as_deref() != Some(expected_id.as_str())
        || action != *expected_action
        || agent_id.as_deref() != expected_agent.as_deref()
    {
        return;
    }
    app.session_agents.pending = None;
    app.session_agents.loading = false;
    if let Some(error) = error {
        app.session_agents.error = Some(error);
        return;
    }
    match action {
        SessionAgentsAction::ListSessionAgents => {
            let previous = app.session_agents.selected_id().map(str::to_owned);
            app.session_agents.agent_ids = agent_ids;
            app.session_agents.selected = previous
                .and_then(|id| {
                    app.session_agents.agent_ids.iter().position(|candidate| *candidate == id)
                })
                .unwrap_or(0);
            inspect_selected(app);
        }
        SessionAgentsAction::GetSubagentMessages => app.session_agents.messages = messages,
        SessionAgentsAction::StopSubagentTask => {
            app.config.status_message = Some("Stop requested for the selected agent.".to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::AgentConnection;
    use crate::agent::model::{SessionId, TaskItem, TaskStatus};
    use crate::agent::wire::BridgeCommand;
    use std::rc::Rc;

    #[tokio::test(flavor = "current_thread")]
    async fn list_and_inspect_ignore_stale_requests_and_stop_only_the_matching_running_task() {
        let mut app = App::test_default();
        let (conn, mut commands) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(conn));
        app.session_runtime.session_id = Some(SessionId::new("session-1"));
        refresh(&mut app);
        let list = commands.recv_envelope().await.expect("list command");
        let request = list.request_id.expect("request id");
        apply_result(
            &mut app,
            ClientEvent::SessionAgentsResultReceived {
                session_id: "other-session".to_owned(),
                request_id: Some(request.clone()),
                action: SessionAgentsAction::ListSessionAgents,
                agent_id: None,
                agent_ids: vec!["wrong".to_owned()],
                messages: vec![],
                error: None,
            },
        );
        apply_result(
            &mut app,
            ClientEvent::SessionAgentsResultReceived {
                session_id: "session-1".to_owned(),
                request_id: Some("stale".to_owned()),
                action: SessionAgentsAction::ListSessionAgents,
                agent_id: None,
                agent_ids: vec!["wrong".to_owned()],
                messages: vec![],
                error: None,
            },
        );
        assert!(app.session_agents.agent_ids.is_empty());

        let mut task = TaskItem::new("agent-running", "Review", TaskStatus::InProgress);
        task.metadata = Some(serde_json::json!({ "subagent_type": "general-purpose" }));
        app.sdk_inventory.tasks.push(task);
        apply_result(
            &mut app,
            ClientEvent::SessionAgentsResultReceived {
                session_id: "session-1".to_owned(),
                request_id: Some(request.clone()),
                action: SessionAgentsAction::ListSessionAgents,
                agent_id: None,
                agent_ids: vec!["agent-running".to_owned()],
                messages: vec![],
                error: None,
            },
        );
        let inspect = commands.recv_envelope().await.expect("inspect command");
        assert!(
            matches!(inspect.command, BridgeCommand::GetSubagentMessages { ref agent_id, .. } if agent_id == "agent-running")
        );
        let inspect_request = inspect.request_id.expect("inspect request");
        apply_result(
            &mut app,
            ClientEvent::SessionAgentsResultReceived {
                session_id: "session-1".to_owned(),
                request_id: Some(inspect_request.clone()),
                action: SessionAgentsAction::GetSubagentMessages,
                agent_id: Some("wrong-agent".to_owned()),
                agent_ids: vec![],
                messages: vec![SubagentMessage {
                    role: "assistant".to_owned(),
                    text: "wrong".to_owned(),
                }],
                error: None,
            },
        );
        assert!(app.session_agents.messages.is_empty());
        apply_result(
            &mut app,
            ClientEvent::SessionAgentsResultReceived {
                session_id: "session-1".to_owned(),
                request_id: Some(inspect_request.clone()),
                action: SessionAgentsAction::GetSubagentMessages,
                agent_id: Some("agent-running".to_owned()),
                agent_ids: vec![],
                messages: vec![SubagentMessage {
                    role: "assistant".to_owned(),
                    text: "Checked".to_owned(),
                }],
                error: None,
            },
        );
        assert_eq!(app.session_agents.messages[0].text, "Checked");

        stop_selected(&mut app);
        let stop = commands.recv_envelope().await.expect("stop command");
        assert!(
            matches!(stop.command, BridgeCommand::StopSubagentTask { ref session_id, ref agent_id }
            if session_id == "session-1" && agent_id == "agent-running")
        );
        app.sdk_inventory.tasks[0].status = TaskStatus::Completed;
        stop_selected(&mut app);
        assert!(
            app.session_agents
                .error
                .as_deref()
                .is_some_and(|error| error.contains("no running task"))
        );
    }
}
