// SPDX-License-Identifier: Apache-2.0

use crate::agent::{model, wire::WorkingDiffFile};
use crate::app::{App, FullscreenView, MessageBlock, view};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) struct DiffEntry {
    pub path: String,
    pub status: Option<String>,
    pub working_diff: Option<String>,
    pub session_diff: Option<model::Diff>,
}

pub(crate) struct DiffViewState {
    pub entries: Vec<DiffEntry>,
    pub selected: usize,
    pub scroll: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub rendered_session_diff: Option<(usize, u16, Vec<ratatui::text::Line<'static>>)>,
    request_id: String,
}

fn session_entries(app: &App) -> BTreeMap<String, DiffEntry> {
    let mut entries = BTreeMap::new();
    for message in &app.transcript.messages {
        for block in &message.blocks {
            let MessageBlock::ToolCall(tool) = block else { continue };
            for content in &tool.content {
                let model::ToolCallContent::Diff(diff) = content else { continue };
                let path = diff.path.strip_prefix(Path::new(&app.cwd_raw)).unwrap_or(&diff.path);
                let label = path.to_string_lossy().into_owned();
                entries.insert(
                    label.clone(),
                    DiffEntry {
                        path: label,
                        status: None,
                        working_diff: None,
                        session_diff: Some(diff.clone()),
                    },
                );
            }
        }
    }
    entries
}

pub(crate) fn open(app: &mut App) {
    let Some(session_id) = app.session_runtime.session_id.as_ref().map(|id| id.as_str().to_owned())
    else {
        crate::app::events::push_submission_feedback(
            app,
            crate::app::SystemSeverity::Info,
            "Connect to a session before opening /diff.",
        );
        return;
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    let entries = session_entries(app).into_values().collect();
    app.diff_view = Some(Box::new(DiffViewState {
        entries,
        selected: 0,
        scroll: 0,
        loading: true,
        error: None,
        rendered_session_diff: None,
        request_id: request_id.clone(),
    }));
    view::set_fullscreen_view(app, FullscreenView::Diff);
    request(app, session_id, request_id);
}

fn request(app: &mut App, session_id: String, request_id: String) {
    let Some(conn) = app.session_runtime.conn.as_ref() else { return };
    if let Err(error) = conn.get_working_diff(session_id, request_id)
        && let Some(state) = &mut app.diff_view
    {
        state.loading = false;
        state.error = Some(format!("Could not request Git changes: {error}"));
    }
}

fn refresh(app: &mut App) {
    let Some(session_id) = app.session_runtime.session_id.as_ref().map(|id| id.as_str().to_owned())
    else {
        return;
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    let entries = session_entries(app).into_values().collect();
    app.diff_view = Some(Box::new(DiffViewState {
        entries,
        selected: 0,
        scroll: 0,
        loading: true,
        error: None,
        rendered_session_diff: None,
        request_id: request_id.clone(),
    }));
    request(app, session_id, request_id);
    app.request_active_surface_repaint();
}

pub(crate) fn apply(
    app: &mut App,
    session_id: &str,
    request_id: Option<&str>,
    files: Vec<WorkingDiffFile>,
    error: Option<String>,
) {
    if app.session_runtime.session_id.as_ref().map(model::SessionId::as_str) != Some(session_id) {
        return;
    }
    let Some(state) = &mut app.diff_view else { return };
    if request_id != Some(state.request_id.as_str()) {
        return;
    }
    let selected_path = state.entries.get(state.selected).map(|entry| entry.path.clone());
    let mut entries: BTreeMap<String, DiffEntry> = std::mem::take(&mut state.entries)
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    for file in files {
        let entry = entries.entry(file.path.clone()).or_insert_with(|| DiffEntry {
            path: file.path,
            status: None,
            working_diff: None,
            session_diff: None,
        });
        entry.status = Some(file.status);
        entry.working_diff = Some(file.diff);
    }
    state.entries = entries.into_values().collect();
    state.selected = selected_path
        .and_then(|path| state.entries.iter().position(|entry| entry.path == path))
        .unwrap_or(0);
    state.loading = false;
    state.error = error;
    state.rendered_session_diff = None;
    app.request_active_surface_repaint();
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) {
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) => view::set_chat_surface(app),
        (KeyCode::Char('r' | 'R'), KeyModifiers::NONE | KeyModifiers::SHIFT) => refresh(app),
        (KeyCode::Up, _) => {
            if let Some(state) = &mut app.diff_view {
                state.selected = state.selected.saturating_sub(1);
                state.scroll = 0;
                state.rendered_session_diff = None;
            }
        }
        (KeyCode::Down, _) => {
            if let Some(state) = &mut app.diff_view {
                state.selected = (state.selected + 1).min(state.entries.len().saturating_sub(1));
                state.scroll = 0;
                state.rendered_session_diff = None;
            }
        }
        (KeyCode::PageUp, _) => {
            if let Some(state) = &mut app.diff_view {
                state.scroll = state.scroll.saturating_sub(20);
            }
        }
        (KeyCode::PageDown, _) => {
            if let Some(state) = &mut app.diff_view {
                state.scroll = state.scroll.saturating_add(20);
            }
        }
        _ => {}
    }
    app.request_active_surface_repaint();
}
