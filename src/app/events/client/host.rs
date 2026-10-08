// SPDX-License-Identifier: Apache-2.0

use super::super::App;
use crate::agent::events::ClientEvent;

pub(super) fn handle(app: &mut App, event: ClientEvent) {
    match event {
        ClientEvent::UpdateCheckCompleted { result } => {
            crate::app::update_check::apply_check_result(app, &result);
        }
        ClientEvent::AgentStatusUpdated { enabled, status } => {
            app.agent_view.enabled = enabled;
            app.agent_view.status = status;
        }
        ClientEvent::AgentViewFailed { message } => {
            crate::app::events::push_system_message_with_severity(
                app,
                Some(crate::app::SystemSeverity::Error),
                &message,
            );
        }
        ClientEvent::TerminalReleasedToChild { reason, ready_tx, cancel_tx } => {
            app.terminal_lifecycle = crate::app::TerminalLifecycleState::ReleasedToChild(reason);
            app.surface_dirty.clear_for_child_release();
            crate::app::terminal_runtime::child_took_terminal(app, cancel_tx);
            let _ = ready_tx.send(());
        }
        ClientEvent::TerminalReturnedFromChild { reason: _ } => {
            crate::app::terminal_runtime::child_returned_terminal(app);
            app.terminal_lifecycle =
                crate::app::TerminalLifecycleState::Running(crate::app::SurfaceMode::Chat);
            app.surface_dirty.terminal_mode = true;
            app.chat_render.clear_measurements();
            app.chat_render.invalidate_live_anchor();
            app.request_chat_visible_rebuild();
        }
        ClientEvent::RuntimeReloadCompleted { session_id: _ } => {
            crate::app::plugins::apply_runtime_reload_success(app);
        }
        ClientEvent::RuntimeReloadHeld { session_id: _, cache_impact } => {
            crate::app::plugins::apply_runtime_reload_held(app, &cache_impact);
            if app.mcp.in_flight {
                app.mcp.in_flight = false;
            }
        }
        ClientEvent::RuntimeReloadFailed { session_id: _, message } => {
            crate::app::plugins::apply_runtime_reload_failure(app, &message);
            if app.mcp.in_flight {
                app.mcp.in_flight = false;
                app.mcp.last_error =
                    Some(format!("Failed to reload MCP server snapshot: {message}"));
            }
        }
        ClientEvent::UsageRefreshStarted { epoch } => {
            if app.session_runtime.session_scope_epoch == epoch {
                crate::app::usage::apply_refresh_started(app);
            }
        }
        ClientEvent::StructuredUsageReceived { session_id: _, snapshot, error } => {
            crate::app::usage::apply_structured_sdk_result(app, snapshot, error);
        }
        ClientEvent::UsageSnapshotReceived { epoch, snapshot } => {
            if app.session_runtime.session_scope_epoch == epoch {
                crate::app::usage::apply_refresh_success(app, snapshot);
            }
        }
        ClientEvent::UsageRefreshFailed { epoch, message, source } => {
            if app.session_runtime.session_scope_epoch == epoch {
                crate::app::usage::apply_refresh_failure(app, message, source);
            }
        }
        ClientEvent::PluginsInventoryUpdated { cwd_raw, snapshot, claude_path } => {
            if app.cwd_raw == cwd_raw {
                crate::app::plugins::apply_inventory_refresh_success(app, snapshot, claude_path);
            }
        }
        ClientEvent::PluginsInventoryRefreshFailed { cwd_raw, message } => {
            if app.cwd_raw == cwd_raw {
                crate::app::plugins::apply_inventory_refresh_failure(app, message);
            }
        }
        ClientEvent::PluginsCliActionSucceeded { cwd_raw, result } => {
            if app.cwd_raw == cwd_raw {
                crate::app::plugins::apply_cli_action_success(app, result);
            }
        }
        ClientEvent::PluginsCliActionFailed { cwd_raw, message } => {
            if app.cwd_raw == cwd_raw {
                crate::app::plugins::apply_cli_action_failure(app, message);
            }
        }
        _ => unreachable!("client event family routed a non-host event to the host handler"),
    }
}
