// SPDX-License-Identifier: Apache-2.0
use super::type_converters::map_session_update;
use crate::Cli;
use crate::agent::model;
use crate::agent::types;
use crate::app::{FullscreenView, SurfaceMode, TerminalLifecycleState};
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn map_session_update_preserves_config_option_update() {
    let mapped = map_session_update(types::SessionUpdate::ConfigOptionUpdate {
        option_id: "model".to_owned(),
        value: serde_json::Value::String("sonnet".to_owned()),
    });

    let Some(model::SessionUpdate::ConfigOptionUpdate(cfg)) = mapped else {
        panic!("expected ConfigOptionUpdate mapping");
    };
    assert_eq!(cfg.option_id, "model");
    assert_eq!(cfg.value, serde_json::Value::String("sonnet".to_owned()));
}

#[test]
fn create_app_defers_file_index_and_routes_untrusted_cwd_to_trust_surface() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cli = Cli {
        prompt: None,
        resume: None,
        continue_session: false,
        session_options: crate::SessionOptions::default(),
        command: None,
        no_update_check: true,
        dir: Some(dir.path().to_path_buf()),
        bridge_script: None,
        enable_logs: false,
        diagnostics_preset: None,
        log_file: None,
        log_filter: None,
        log_append: false,
    };

    let app = super::create_app(&cli);

    assert!(app.file_index.root.is_none());
    assert!(app.file_index.scan.is_none());
    assert!(app.file_index.watch.is_none());
    assert_eq!(app.surface_mode, SurfaceMode::Fullscreen(FullscreenView::Trusted));
    assert_eq!(app.terminal_lifecycle, TerminalLifecycleState::Bootstrapping);
}

fn no_dot_components(path: &std::path::Path) -> bool {
    !path
        .components()
        .any(|part| matches!(part, std::path::Component::CurDir | std::path::Component::ParentDir))
}

/// The fixture root as the platform spells its own working directory: physical on Unix
/// (macOS temp dirs sit behind a symlink), as given on Windows.
fn platform_root(path: &std::path::Path) -> std::path::PathBuf {
    if cfg!(windows) { path.to_path_buf() } else { path.canonicalize().expect("canonical tempdir") }
}

#[test]
fn a_relative_dir_resolves_to_the_absolute_directory_it_names() {
    let base = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(base.path().join("project").join("sub")).expect("project");
    // Every expected child is spelled out by name under the fixture root.
    let root = platform_root(base.path());
    let cases = [
        (".", root.clone()),
        ("project", root.join("project")),
        ("./project/./sub", root.join("project").join("sub")),
        ("project/sub/..", root.join("project")),
    ];
    for (dir, expected) in cases {
        let resolved = super::startup_cwd(Some(std::path::Path::new(dir)), base.path().into());
        assert_eq!(resolved, expected, "--dir {dir}");
        assert!(resolved.is_absolute() && no_dot_components(&resolved), "{}", resolved.display());
    }
}

#[test]
fn an_absolute_dir_is_used_as_given() {
    let base = tempfile::tempdir().expect("tempdir");
    let project = base.path().join("project");
    std::fs::create_dir_all(&project).expect("project");
    let elsewhere = tempfile::tempdir().expect("process dir");

    let resolved = super::startup_cwd(Some(&project), elsewhere.path().into());

    assert_eq!(resolved, project);
}

#[test]
fn create_app_keeps_the_resolved_dir_for_session_state_and_display() {
    // Tests run from the package root, which has `src`; `src/..` names that root.
    let expected = std::env::current_dir().expect("process dir");
    let cli = Cli {
        prompt: None,
        resume: None,
        continue_session: false,
        session_options: crate::SessionOptions::default(),
        command: None,
        no_update_check: true,
        dir: Some(std::path::Path::new("src").join("..")),
        bridge_script: None,
        enable_logs: false,
        diagnostics_preset: None,
        log_file: None,
        log_filter: None,
        log_append: false,
    };

    let app = super::create_app(&cli);

    assert_eq!(std::path::Path::new(&app.cwd_raw), expected);
    let shown = std::path::Path::new(app.cwd.trim_start_matches('~'));
    let root_name = expected.file_name().expect("package root name");
    assert!(shown.ends_with(root_name) && no_dot_components(shown), "Loc: {}", app.cwd);
}

#[test]
fn app_client_event_queue_has_the_configured_capacity() {
    let mut app = crate::app::App::test_default();
    assert_eq!(app.event_tx.max_capacity(), super::CLIENT_EVENT_QUEUE_CAPACITY);

    for _ in 0..super::CLIENT_EVENT_QUEUE_CAPACITY {
        app.event_tx
            .try_send(crate::agent::events::ClientEvent::LogoutCompleted)
            .expect("event should fit within configured capacity");
    }
    assert!(matches!(
        app.event_tx.try_send(crate::agent::events::ClientEvent::LogoutCompleted),
        Err(tokio::sync::mpsc::error::TrySendError::Full(_))
    ));
    assert!(app.event_rx.try_recv().is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_connection_signals_and_awaits_the_owned_bridge_task() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut app = crate::app::App::test_default();
            let completed = Rc::new(Cell::new(false));
            let completed_for_task = Rc::clone(&completed);
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let join_handle = tokio::task::spawn_local(async move {
                let _ = shutdown_rx.await;
                completed_for_task.set(true);
            });
            app.bridge_task = Some(super::BridgeTask { shutdown_tx, join_handle });

            super::shutdown_connection(&mut app).await;

            assert!(completed.get());
            assert!(app.bridge_task.is_none());
            assert!(app.session_runtime.conn.is_none());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn forced_connection_shutdown_aborts_the_owned_bridge_task() {
    tokio::task::LocalSet::new()
        .run_until(async {
            struct DropMarker(Rc<Cell<bool>>);

            impl Drop for DropMarker {
                fn drop(&mut self) {
                    self.0.set(true);
                }
            }

            let mut app = crate::app::App::test_default();
            let dropped = Rc::new(Cell::new(false));
            let dropped_for_task = Rc::clone(&dropped);
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let join_handle = tokio::task::spawn_local(async move {
                let _marker = DropMarker(dropped_for_task);
                let _ = shutdown_rx.await;
                std::future::pending::<()>().await;
            });
            tokio::task::yield_now().await;
            app.bridge_task = Some(super::BridgeTask { shutdown_tx, join_handle });

            let shutdown = super::begin_shutdown_connection(&mut app).expect("shutdown handle");
            shutdown.force().await;

            assert!(dropped.get());
            assert!(app.bridge_task.is_none());
            assert!(app.session_runtime.conn.is_none());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn history_export_chunks_count_the_updates_the_app_cannot_map() {
    let (connection, _commands) = crate::agent::client::AgentConnection::test_channel();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(4);
    let mut connected_once = true;
    // An image of a type the app cannot show has no mapped form; the text next to it does.
    let envelope: crate::agent::wire::EventEnvelope = serde_json::from_value(serde_json::json!({
        "request_id": "export-1",
        "event": "session_history_export",
        "session_id": "s1",
        "updates": [
            { "type": "user_message_chunk",
              "content": { "type": "text", "text": "hello" } },
            { "type": "user_message_chunk",
              "content": { "type": "image", "mime_type": "image/x-unsupported", "data": "AAAA" } },
            { "type": "agent_message_chunk",
              "content": { "type": "image", "mime_type": "image/png", "data": "" } }
        ],
        "last": true,
        "omitted_updates": 0
    }))
    .expect("history export envelope");

    super::event_dispatch::handle_bridge_event(
        &event_tx,
        &connection,
        &mut connected_once,
        false,
        envelope,
    )
    .await;

    let crate::agent::events::ClientEvent::SessionHistoryChunk { chunk, .. } =
        event_rx.recv().await.expect("mapped event")
    else {
        panic!("expected a history chunk");
    };
    assert_eq!(chunk.updates.len(), 1);
    assert_eq!(chunk.unmapped_updates, 2);
    assert_eq!(chunk.request_id.as_deref(), Some("export-1"));
}
