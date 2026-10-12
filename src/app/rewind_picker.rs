// SPDX-License-Identifier: Apache-2.0

use super::connect::begin_rewind;
use super::events::push_system_message_with_severity;
use super::{App, AppStatus, FullscreenView, SurfaceMode, SystemSeverity, view};
use crate::agent::types::RewindRestoreMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone)]
pub(crate) struct RewindPickerState {
    pub session_id: String,
    pub selected: usize,
    pub restore_mode: Option<RewindRestoreMode>,
}

pub(crate) fn open(app: &mut App) -> bool {
    if app.status != AppStatus::Ready {
        return false;
    }
    let Some(session_id) = app.session_runtime.session_id.clone() else {
        return false;
    };
    let Some(conn) = app.session_runtime.conn.clone() else {
        return false;
    };
    let session_id_text = session_id.to_string();
    let needs_refresh = app.sdk_inventory.rewind_targets_session_id.as_ref() != Some(&session_id);
    let requested_for_session = app.sdk_inventory.rewind_targets_in_flight
        && app.sdk_inventory.rewind_targets_request_session_id.as_ref() == Some(&session_id);
    app.rewind_picker = Some(Box::new(RewindPickerState {
        session_id: session_id_text.clone(),
        selected: 0,
        restore_mode: None,
    }));
    view::set_fullscreen_view(app, FullscreenView::Rewind);
    if needs_refresh && !requested_for_session {
        app.sdk_inventory.rewind_targets.clear();
        app.sdk_inventory.rewind_targets_session_id = None;
        app.sdk_inventory.rewind_targets_error = None;
        app.sdk_inventory.rewind_targets_request_session_id = Some(session_id);
        app.sdk_inventory.rewind_targets_in_flight = true;
        if let Err(error) = conn.get_rewind_targets(session_id_text) {
            app.sdk_inventory.rewind_targets_in_flight = false;
            app.sdk_inventory.rewind_targets_request_session_id = None;
            app.sdk_inventory.rewind_targets_error = Some(error.to_string());
        }
    }
    true
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) {
    if app.rewind_picker.is_none() {
        view::set_chat_surface(app);
        return;
    }
    if key.code == KeyCode::Esc {
        if let Some(picker) = app.rewind_picker.as_mut()
            && picker.restore_mode.take().is_some()
        {
            return;
        }
        close(app);
        return;
    }
    if key.modifiers != KeyModifiers::NONE {
        return;
    }
    let Some(picker) = app.rewind_picker.as_mut() else { return };
    if picker.restore_mode.is_some() {
        match key.code {
            KeyCode::Char('b') => picker.restore_mode = Some(RewindRestoreMode::Both),
            KeyCode::Char('c') => picker.restore_mode = Some(RewindRestoreMode::Conversation),
            KeyCode::Char('f') => picker.restore_mode = Some(RewindRestoreMode::Code),
            KeyCode::Enter => activate(app),
            _ => {}
        }
        return;
    }
    let count = app.sdk_inventory.rewind_targets.len();
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => picker.selected = picker.selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => {
            picker.selected = (picker.selected + 1).min(count.saturating_sub(1));
        }
        KeyCode::Home => picker.selected = 0,
        KeyCode::End => picker.selected = count.saturating_sub(1),
        KeyCode::Enter if count > 0 && !app.sdk_inventory.rewind_targets_in_flight => {
            picker.restore_mode = Some(RewindRestoreMode::Conversation);
        }
        _ => {}
    }
}

fn activate(app: &mut App) {
    let Some(picker) = app.rewind_picker.as_ref() else { return };
    let Some(target) = app.sdk_inventory.rewind_targets.get(picker.selected) else { return };
    let Some(mode) = picker.restore_mode else { return };
    if app.session_runtime.session_id.as_ref().is_none_or(|id| id.as_str() != picker.session_id) {
        close(app);
        return;
    }
    let Some(conn) = app.session_runtime.conn.clone() else { return };
    let result = begin_rewind(app, &conn, picker.session_id.clone(), target.uuid.clone(), mode);
    close(app);
    match result {
        Ok(()) => {
            app.status = AppStatus::CommandPending;
            app.turn.pending_command_label = Some(format!("Rewinding {}...", mode.as_stored()));
        }
        Err(error) => push_system_message_with_severity(
            app,
            Some(SystemSeverity::Error),
            &format!("Failed to rewind: {error}"),
        ),
    }
}

fn close(app: &mut App) {
    app.rewind_picker = None;
    if app.surface_mode == SurfaceMode::Fullscreen(FullscreenView::Rewind) {
        view::set_chat_surface(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{model, wire::BridgeCommand};

    #[test]
    fn picker_cancel_preserves_draft_and_never_rewinds() {
        let mut app = App::test_default();
        let (conn, mut commands) = crate::agent::client::AgentConnection::test_channel();
        app.session_runtime.conn = Some(std::rc::Rc::new(conn));
        app.session_runtime.session_id = Some("s1".into());
        app.input.set_text("keep draft");
        assert!(open(&mut app));
        assert!(matches!(
            commands.try_recv().expect("targets").command,
            BridgeCommand::GetRewindTargets { .. }
        ));
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.surface_mode, SurfaceMode::Chat);
        assert_eq!(app.input.text(), "keep draft");
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn picker_replaces_in_flight_request_for_previous_session() {
        let mut app = App::test_default();
        let (conn, mut commands) = crate::agent::client::AgentConnection::test_channel();
        app.session_runtime.conn = Some(std::rc::Rc::new(conn));
        app.session_runtime.session_id = Some("current".into());
        app.sdk_inventory.rewind_targets_in_flight = true;
        app.sdk_inventory.rewind_targets_request_session_id = Some("old".into());
        assert!(open(&mut app));
        assert!(
            matches!(commands.try_recv().expect("current request").command, BridgeCommand::GetRewindTargets { session_id } if session_id == "current")
        );
        assert_eq!(
            app.sdk_inventory
                .rewind_targets_request_session_id
                .as_ref()
                .map(model::SessionId::as_str),
            Some("current")
        );
    }

    #[test]
    fn picker_confirms_exact_selected_target_and_mode() {
        let mut app = App::test_default();
        let (conn, mut commands) = crate::agent::client::AgentConnection::test_channel();
        app.session_runtime.conn = Some(std::rc::Rc::new(conn));
        app.session_runtime.session_id = Some("s1".into());
        app.sdk_inventory.rewind_targets_session_id = Some("s1".into());
        app.sdk_inventory.rewind_targets = ["u1", "u2"]
            .into_iter()
            .enumerate()
            .map(|(index, uuid)| model::RewindTarget {
                uuid: uuid.to_owned(),
                first_text: format!("prompt {index}"),
                input_text: String::new(),
                index: index as u64,
                previous_assistant_uuid: None,
                resume_anchor_uuid: None,
            })
            .collect();
        assert!(open(&mut app));
        handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        handle_key(&mut app, KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            matches!(commands.try_recv().expect("rewind").command, BridgeCommand::Rewind { session_id, target_user_message_id, restore_mode, .. } if session_id == "s1" && target_user_message_id == "u2" && restore_mode == RewindRestoreMode::Both)
        );
        assert_eq!(app.surface_mode, SurfaceMode::Chat);
    }
}
