// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Simon Peter Rothgang

use super::connect::{begin_resume_session, begin_resume_session_at};
use super::events::push_system_message_with_severity;
use super::view;
use super::{App, AppStatus, SessionPickerMutation, SystemSeverity};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(crate) const MAX_PICKER_SESSIONS: usize = 10;
pub(crate) const LARGE_SESSION_BYTES: u64 = 1024 * 1024;

pub(crate) fn picker_session_count(app: &App) -> usize {
    app.recent_sessions.len().min(MAX_PICKER_SESSIONS)
}

pub(crate) fn startup_picker_is_loading(app: &App) -> bool {
    app.startup.startup_picker_is_loading(app.session_runtime.conn.is_some())
}

pub(crate) fn picker_turn_count(app: &App) -> usize {
    app.sdk_inventory.rewind_targets.len()
}

pub fn handle_key(app: &mut App, key: KeyEvent) {
    if is_ctrl(key, 'q') {
        app.request_shutdown();
        return;
    }

    if startup_picker_is_loading(app) {
        return;
    }

    if app.session_picker.mutation.is_some() {
        handle_mutation_key(app, key);
        return;
    }

    if app.session_picker.turn_session_id.is_some() {
        handle_turn_key(app, key);
        return;
    }

    let session_count = picker_session_count(app);
    if session_count == 0 {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            app.startup.resolve_session_picker();
            app.startup.complete_launch();
            view::set_chat_surface(app);
        }
        return;
    }

    match (key.code, key.modifiers) {
        (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
            app.session_picker.selected = app.session_picker.selected.saturating_sub(1);
        }
        (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
            app.session_picker.selected =
                (app.session_picker.selected + 1).min(session_count.saturating_sub(1));
        }
        (KeyCode::Home, _) => app.session_picker.selected = 0,
        (KeyCode::End, _) => app.session_picker.selected = session_count.saturating_sub(1),
        (KeyCode::Enter, KeyModifiers::NONE) => activate_selection(app),
        (KeyCode::Char('c'), KeyModifiers::NONE) => activate_compact_first(app),
        (KeyCode::Right, KeyModifiers::NONE) => open_turn_selection(app),
        (KeyCode::Char('t'), KeyModifiers::NONE) => open_mutation(app, false),
        (KeyCode::Char('d'), KeyModifiers::NONE) => open_mutation(app, true),
        (KeyCode::Esc, KeyModifiers::NONE) => {
            app.startup.resolve_session_picker();
            app.startup.complete_launch();
            view::set_chat_surface(app);
        }
        _ => {}
    }
}

fn open_mutation(app: &mut App, delete: bool) {
    let Some(session) = app.recent_sessions.get(app.session_picker.selected) else { return };
    if delete
        && app
            .session_runtime
            .session_id
            .as_ref()
            .is_some_and(|active| active.as_str() == session.session_id)
    {
        app.session_picker.error =
            Some("Switch sessions before deleting the active session.".to_owned());
        return;
    }
    app.session_picker.error = None;
    app.session_picker.mutation = Some(Box::new(if delete {
        SessionPickerMutation::Delete {
            session_id: session.session_id.clone(),
            revision: app.session_picker.list_revision,
            request_id: None,
        }
    } else {
        SessionPickerMutation::Tag {
            session_id: session.session_id.clone(),
            revision: app.session_picker.list_revision,
            draft: session.tag.clone().unwrap_or_default(),
            request_id: None,
        }
    }));
}

fn handle_mutation_key(app: &mut App, key: KeyEvent) {
    if matches!(key.code, KeyCode::Esc)
        && !app.session_picker.mutation.as_deref().is_some_and(|mutation| match mutation {
            SessionPickerMutation::Tag { request_id, .. }
            | SessionPickerMutation::Delete { request_id, .. } => request_id.is_some(),
        })
    {
        app.session_picker.mutation = None;
        app.session_picker.error = None;
        return;
    }
    let Some(mutation) = app.session_picker.mutation.as_deref_mut() else { return };
    match mutation {
        SessionPickerMutation::Tag { draft, request_id: None, .. } => {
            match (key.code, key.modifiers) {
                (KeyCode::Char(ch), modifiers)
                    if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
                {
                    draft.push(ch);
                }
                (KeyCode::Backspace, _) => {
                    draft.pop();
                }
                (KeyCode::Char('u'), KeyModifiers::CONTROL) => draft.clear(),
                (KeyCode::Enter, _) => {
                    submit_mutation(app);
                }
                _ => {}
            }
        }
        SessionPickerMutation::Delete { request_id: None, .. }
            if matches!(key.code, KeyCode::Char('y')) =>
        {
            submit_mutation(app);
        }
        _ => {}
    }
}

fn submit_mutation(app: &mut App) {
    let Some(mutation) = app.session_picker.mutation.as_deref() else { return };
    let (session_id, revision) = match mutation {
        SessionPickerMutation::Tag { session_id, revision, .. }
        | SessionPickerMutation::Delete { session_id, revision, .. } => {
            (session_id.clone(), *revision)
        }
    };
    if revision != app.session_picker.list_revision
        || !app.recent_sessions.iter().any(|session| session.session_id == session_id)
    {
        app.session_picker.error =
            Some("Session list changed; reopen the action on the current selection.".to_owned());
        app.session_picker.mutation = None;
        return;
    }
    if matches!(mutation, SessionPickerMutation::Delete { .. })
        && app
            .session_runtime
            .session_id
            .as_ref()
            .is_some_and(|active| active.as_str() == session_id)
    {
        app.session_picker.error =
            Some("Switch sessions before deleting the active session.".to_owned());
        app.session_picker.mutation = None;
        return;
    }
    let Some(conn) = app.session_runtime.conn.as_ref() else {
        app.session_picker.error = Some("Session service is unavailable.".to_owned());
        return;
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    let result = match mutation {
        SessionPickerMutation::Tag { draft, .. } => conn.tag_session(
            session_id,
            (!draft.trim().is_empty()).then(|| draft.trim().to_owned()),
            request_id.clone(),
        ),
        SessionPickerMutation::Delete { .. } => conn.delete_session(session_id, request_id.clone()),
    };
    match result {
        Ok(()) => {
            match app.session_picker.mutation.as_deref_mut() {
                Some(
                    SessionPickerMutation::Tag { request_id: pending, .. }
                    | SessionPickerMutation::Delete { request_id: pending, .. },
                ) => {
                    *pending = Some(request_id);
                }
                None => {}
            }
            app.session_picker.error = None;
        }
        Err(error) => {
            app.session_picker.error = Some(format!("Failed to send session action: {error}"));
        }
    }
}

pub(crate) fn apply_mutation_result(
    app: &mut App,
    session_id: &str,
    request_id: Option<&str>,
    operation: &str,
    error: Option<String>,
    refresh_error: Option<String>,
    sessions: Option<Vec<crate::agent::types::SessionListEntry>>,
) {
    let Some(mutation) = app.session_picker.mutation.as_deref() else { return };
    let matches_request = match mutation {
        SessionPickerMutation::Tag { session_id: id, request_id: pending, .. } => {
            operation == "tag"
                && id == session_id
                && pending.as_deref() == request_id
                && pending.is_some()
        }
        SessionPickerMutation::Delete { session_id: id, request_id: pending, .. } => {
            operation == "delete"
                && id == session_id
                && pending.as_deref() == request_id
                && pending.is_some()
        }
    };
    if !matches_request {
        return;
    }
    if let Some(error) = error {
        app.session_picker.error = Some(error);
        if let Some(mutation) = app.session_picker.mutation.as_deref_mut() {
            match mutation {
                SessionPickerMutation::Tag { request_id, .. }
                | SessionPickerMutation::Delete { request_id, .. } => *request_id = None,
            }
        }
        return;
    }
    if let Some(sessions) = sessions {
        app.session_picker.mutation = None;
        app.session_picker.error = None;
        crate::app::events::session::handle_sessions_listed_event(app, sessions);
    } else {
        app.session_picker.mutation = None;
        app.session_picker.error = Some(format!(
            "Session action completed, but the session list could not refresh: {}",
            refresh_error.unwrap_or_else(|| "no refreshed list received".to_owned())
        ));
    }
}

fn handle_turn_key(app: &mut App, key: KeyEvent) {
    if matches!((key.code, key.modifiers), (KeyCode::Esc | KeyCode::Left, KeyModifiers::NONE)) {
        close_turn_selection(app);
        return;
    }
    if app.sdk_inventory.rewind_targets_in_flight {
        return;
    }
    let turn_count = picker_turn_count(app);
    if turn_count == 0 {
        return;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
            app.session_picker.turn_selected = app.session_picker.turn_selected.saturating_sub(1);
        }
        (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
            app.session_picker.turn_selected =
                (app.session_picker.turn_selected + 1).min(turn_count.saturating_sub(1));
        }
        (KeyCode::Home, _) => app.session_picker.turn_selected = 0,
        (KeyCode::End, _) => app.session_picker.turn_selected = turn_count.saturating_sub(1),
        (KeyCode::Enter, KeyModifiers::NONE) => activate_turn_selection(app),
        _ => {}
    }
}

fn open_turn_selection(app: &mut App) {
    let Some(session_id) = app
        .recent_sessions
        .iter()
        .take(MAX_PICKER_SESSIONS)
        .nth(app.session_picker.selected)
        .map(|session| session.session_id.clone())
    else {
        return;
    };
    let Some(conn) = app.session_runtime.conn.clone() else {
        return;
    };

    app.session_picker.turn_session_id = Some(session_id.clone());
    app.session_picker.turn_selected = 0;
    app.session_picker.turn_scroll_offset = 0;
    app.sdk_inventory.rewind_targets.clear();
    app.sdk_inventory.rewind_targets_session_id = None;
    app.sdk_inventory.rewind_targets_error = None;
    app.sdk_inventory.rewind_targets_request_session_id =
        Some(crate::agent::model::SessionId::new(session_id.clone()));
    app.sdk_inventory.rewind_targets_in_flight = true;
    if let Err(error) = conn.get_rewind_targets(session_id) {
        close_turn_selection(app);
        push_system_message_with_severity(
            app,
            Some(SystemSeverity::Error),
            &format!("Failed to load session turns: {error}"),
        );
    }
}

fn close_turn_selection(app: &mut App) {
    app.session_picker.turn_session_id = None;
    app.session_picker.turn_selected = 0;
    app.session_picker.turn_scroll_offset = 0;
    app.sdk_inventory.clear_rewind_targets();
}

fn activate_turn_selection(app: &mut App) {
    let Some(session_id) = app.session_picker.turn_session_id.clone() else {
        return;
    };
    let Some(target) =
        app.sdk_inventory.rewind_targets.get(app.session_picker.turn_selected).cloned()
    else {
        return;
    };
    let Some(conn) = app.session_runtime.conn.clone() else {
        return;
    };

    app.startup.resolve_session_picker();
    app.status = AppStatus::CommandPending;
    app.turn.pending_command_label = Some("Forking before selected message...".to_owned());
    app.turn.pending_command_ack = None;
    if let Err(error) = begin_resume_session_at(app, &conn, session_id, target.uuid) {
        app.turn.pending_command_label = None;
        app.turn.pending_command_ack = None;
        app.status = AppStatus::Ready;
        app.clear_pending_session_resume();
        push_system_message_with_severity(
            app,
            Some(SystemSeverity::Error),
            &format!("Failed to fork before selected message: {error}"),
        );
    }
    view::set_chat_surface(app);
}

fn activate_selection(app: &mut App) {
    resume_selected(app, false);
}

fn activate_compact_first(app: &mut App) {
    let large = app
        .recent_sessions
        .get(app.session_picker.selected)
        .is_some_and(|session| session.file_size_bytes >= LARGE_SESSION_BYTES);
    if large {
        resume_selected(app, true);
    }
}

fn resume_selected(app: &mut App, compact_first: bool) {
    let Some(session) =
        app.recent_sessions.iter().take(MAX_PICKER_SESSIONS).nth(app.session_picker.selected)
    else {
        return;
    };
    let session_id = session.session_id.clone();
    let Some(conn) = app.session_runtime.conn.clone() else {
        app.startup.resolve_session_picker();
        view::set_chat_surface(app);
        return;
    };

    app.startup.resolve_session_picker();
    if compact_first {
        app.startup.queue_compact_first(session_id.clone());
    }
    app.status = AppStatus::CommandPending;
    app.turn.pending_command_label = Some(format!("Resuming session {session_id}..."));
    app.turn.pending_command_ack = None;
    if let Err(e) = begin_resume_session(app, &conn, session_id) {
        app.turn.pending_command_label = None;
        app.turn.pending_command_ack = None;
        app.status = AppStatus::Ready;
        app.clear_pending_session_resume();
        push_system_message_with_severity(
            app,
            Some(SystemSeverity::Error),
            &format!("Failed to resume session: {e}"),
        );
    }

    view::set_chat_surface(app);
}

fn is_ctrl(key: KeyEvent, ch: char) -> bool {
    matches!(key.code, KeyCode::Char(c) if c == ch) && key.modifiers == KeyModifiers::CONTROL
}

#[cfg(test)]
mod tests {
    use super::{apply_mutation_result, handle_key};
    use crate::agent::client::AgentConnection;
    use crate::agent::wire::BridgeCommand;
    use crate::app::{
        App, AppStatus, FullscreenView, RecentSessionInfo, SessionPickerMutation, SurfaceMode,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::rc::Rc;

    fn picker_app() -> App {
        let mut app = App::test_default();
        app.surface_mode = SurfaceMode::Fullscreen(FullscreenView::SessionPicker);
        app.startup =
            crate::app::state::StartupState::new(None, crate::StartupLaunch::SessionPicker);
        app.startup.request_connection();
        assert!(app.startup.mark_connection_started());
        app.startup.mark_recent_sessions_loaded();
        app.startup.resolve_session_picker();
        app.recent_sessions = vec![
            RecentSessionInfo {
                session_id: "session-1".to_owned(),
                summary: "one".to_owned(),
                last_modified_ms: 1,
                file_size_bytes: 1,
                cwd: Some("/test".to_owned()),
                git_branch: Some("main".to_owned()),
                custom_title: Some("First".to_owned()),
                first_prompt: Some("prompt one".to_owned()),
                tag: None,
            },
            RecentSessionInfo {
                session_id: "session-2".to_owned(),
                summary: "two".to_owned(),
                last_modified_ms: 2,
                file_size_bytes: 2,
                cwd: Some("/test".to_owned()),
                git_branch: Some("main".to_owned()),
                custom_title: Some("Second".to_owned()),
                first_prompt: Some("prompt two".to_owned()),
                tag: None,
            },
        ];
        app
    }

    fn key(app: &mut App, code: KeyCode) {
        handle_key(app, KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn delete_requires_confirmation_and_never_targets_active_session() {
        let mut app = picker_app();
        let (connection, mut rx) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(connection));
        app.session_runtime.session_id = Some(crate::agent::model::SessionId::new("session-1"));
        key(&mut app, KeyCode::Char('d'));
        assert!(app.session_picker.mutation.is_none());
        assert!(rx.try_recv().is_err());

        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Esc);
        assert!(app.session_picker.mutation.is_none());
        assert!(rx.try_recv().is_err());

        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Char('y'));
        let sent = rx.try_recv().expect("confirmed deletion");
        assert!(
            matches!(sent.command, BridgeCommand::DeleteSession { session_id } if session_id == "session-2")
        );
        assert!(sent.request_id.is_some());
    }

    #[test]
    fn tag_clear_is_correlated_and_failed_write_keeps_the_draft() {
        let mut app = picker_app();
        app.recent_sessions[0].tag = Some("old".to_owned());
        let (connection, mut rx) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(connection));
        key(&mut app, KeyCode::Char('t'));
        handle_key(&mut app, KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        key(&mut app, KeyCode::Enter);
        let sent = rx.try_recv().expect("tag clear");
        assert!(
            matches!(sent.command, BridgeCommand::TagSession { ref session_id, tag: None } if session_id == "session-1")
        );
        let request_id = sent.request_id.expect("request id");
        apply_mutation_result(
            &mut app,
            "session-2",
            Some(&request_id),
            "tag",
            None,
            None,
            Some(Vec::new()),
        );
        assert!(app.session_picker.mutation.is_some());
        apply_mutation_result(
            &mut app,
            "session-1",
            Some("stale"),
            "tag",
            None,
            None,
            Some(Vec::new()),
        );
        assert!(app.session_picker.mutation.is_some());
        apply_mutation_result(
            &mut app,
            "session-1",
            Some(&request_id),
            "tag",
            Some("SDK refused".to_owned()),
            None,
            None,
        );
        assert!(
            matches!(app.session_picker.mutation.as_deref(), Some(SessionPickerMutation::Tag { draft, request_id: None, .. }) if draft.is_empty())
        );
        assert_eq!(app.session_picker.error.as_deref(), Some("SDK refused"));
        assert_eq!(app.recent_sessions[0].tag.as_deref(), Some("old"));

        key(&mut app, KeyCode::Enter);
        let retry = rx.try_recv().expect("retry after SDK failure");
        let retry_id = retry.request_id.expect("retry request id");
        apply_mutation_result(
            &mut app,
            "session-1",
            Some(&retry_id),
            "tag",
            None,
            Some("list unavailable".to_owned()),
            None,
        );
        assert!(app.session_picker.mutation.is_none());
        assert!(app.session_picker.error.as_deref().is_some_and(|error| {
            error.contains("action completed") && error.contains("list unavailable")
        }));
        assert_eq!(app.recent_sessions[0].tag.as_deref(), Some("old"));
    }

    #[test]
    fn list_revision_prevents_stale_confirmation_from_mutating_another_id() {
        let mut app = picker_app();
        let (connection, mut rx) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(connection));
        key(&mut app, KeyCode::Char('d'));
        app.session_picker.list_revision += 1;
        app.session_picker.selected = 1;
        key(&mut app, KeyCode::Char('y'));
        assert!(rx.try_recv().is_err());
        assert!(app.session_picker.mutation.is_none());
    }

    #[test]
    fn loading_state_ignores_navigation_keys() {
        let mut app = picker_app();
        app.startup =
            crate::app::state::StartupState::new(None, crate::StartupLaunch::SessionPicker);
        app.startup.request_connection();
        assert!(app.startup.mark_connection_started());
        app.session_runtime.conn = None;

        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        assert_eq!(app.session_picker.selected, 0);
        assert_eq!(app.surface_mode, SurfaceMode::Fullscreen(FullscreenView::SessionPicker));
    }

    #[test]
    fn up_and_down_move_selection() {
        let mut app = picker_app();

        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.session_picker.selected, 1);

        handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.session_picker.selected, 0);
    }

    #[test]
    fn enter_triggers_resume() {
        let mut app = picker_app();
        let (connection, mut rx) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(connection));

        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.surface_mode, SurfaceMode::Chat);
        assert!(matches!(app.status, AppStatus::CommandPending));
        assert_eq!(app.pending_session_resume_id(), Some("session-1"));
        let envelope = rx.try_recv().expect("resume command");
        assert!(matches!(
            envelope.command,
            BridgeCommand::ResumeSession {
                session_id,
                ..
            } if session_id == "session-1"
        ));
    }

    #[test]
    fn compact_first_key_resumes_only_large_selected_session() {
        let mut app = picker_app();
        let (connection, mut rx) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(connection));
        handle_key(&mut app, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(rx.try_recv().is_err());
        app.recent_sessions[0].file_size_bytes = super::LARGE_SESSION_BYTES;
        handle_key(&mut app, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(matches!(rx.try_recv().expect("resume command").command,
            BridgeCommand::ResumeSession { session_id, .. } if session_id == "session-1"));
        assert!(app.startup.pending_compact_first("session-1"));
    }

    #[test]
    fn space_is_ignored_to_avoid_accidental_destructive_navigation() {
        let mut app = picker_app();
        let (connection, mut rx) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(connection));

        handle_key(&mut app, KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

        assert!(app.session_picker.turn_session_id.is_none());
        assert!(!app.sdk_inventory.rewind_targets_in_flight);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn right_opens_turn_picker_and_left_returns_to_sessions() {
        let mut app = picker_app();
        let (connection, _rx) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(connection));

        handle_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.session_picker.turn_session_id.as_deref(), Some("session-1"));

        handle_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(app.session_picker.turn_session_id.is_none());
        assert!(!app.sdk_inventory.rewind_targets_in_flight);
        assert_eq!(app.surface_mode, SurfaceMode::Fullscreen(FullscreenView::SessionPicker));
    }

    #[test]
    fn enter_on_turn_sends_resume_at_without_changing_plain_resume_command() {
        let mut app = picker_app();
        let (connection, mut rx) = AgentConnection::test_channel();
        app.session_runtime.conn = Some(Rc::new(connection));
        app.session_picker.turn_session_id = Some("session-1".to_owned());
        app.sdk_inventory.rewind_targets = vec![crate::agent::model::RewindTarget {
            uuid: "user-2".to_owned(),
            first_text: "second prompt".to_owned(),
            input_text: "second prompt".to_owned(),
            index: 3,
            previous_assistant_uuid: Some("assistant-1".to_owned()),
            resume_anchor_uuid: Some("assistant-1".to_owned()),
        }];

        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.surface_mode, SurfaceMode::Chat);
        assert_eq!(app.pending_session_resume_id(), Some("session-1"));
        let operation_id =
            app.pending_resume_at_operation_id().expect("pending resume-at operation");
        let envelope = rx.try_recv().expect("resume-at command");
        assert_eq!(envelope.request_id.as_deref(), Some(operation_id));
        assert!(matches!(
            envelope.command,
            BridgeCommand::ResumeSessionAt {
                session_id,
                target_user_message_id,
                ..
            } if session_id == "session-1" && target_user_message_id == "user-2"
        ));
    }

    #[test]
    fn esc_switches_back_to_chat() {
        let mut app = picker_app();

        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(app.surface_mode, SurfaceMode::Chat);
        assert!(app.startup.session_picker_resolved());
    }

    #[test]
    fn failed_resume_restores_ready_state_and_surfaces_error() {
        let mut app = picker_app();
        let (connection, rx) = AgentConnection::test_channel();
        drop(rx);
        app.session_runtime.conn = Some(Rc::new(connection));

        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.surface_mode, SurfaceMode::Chat);
        assert!(matches!(app.status, AppStatus::Ready));
        assert!(app.pending_session_resume.is_none());
        assert!(app.turn.pending_command_label.is_none());
        let last = app.transcript.messages.last().expect("error message");
        let text = match last.blocks.first().expect("text block") {
            crate::app::MessageBlock::Text(block) => block.text.as_str(),
            _ => panic!("expected text block"),
        };
        assert!(text.contains("Failed to resume session:"));
    }
}
