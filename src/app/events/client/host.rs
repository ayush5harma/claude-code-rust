// SPDX-License-Identifier: Apache-2.0

use super::super::{App, notices, session};
use crate::agent::events::ClientEvent;
use crate::app::{PendingCommandAck, SystemSeverity};

/// The SDK's own `commands_changed` message refreshes the command inventory;
/// this only acknowledges /reload and says what was re-read.
fn apply_skills_and_styles_reloaded(app: &mut App, skills: usize, output_styles: usize) {
    if matches!(app.turn.pending_command_ack, Some(PendingCommandAck::SkillsAndStylesReload)) {
        session::clear_pending_command(app);
    }
    notices::emit_system_notice(
        app,
        SystemSeverity::Info,
        &format!("Reloaded skills ({skills}) and output styles ({output_styles})"),
    );
}

pub(super) fn handle(app: &mut App, event: ClientEvent) {
    match event {
        ClientEvent::UpdateCheckCompleted { result } => {
            crate::app::update_check::apply_check_result(app, &result);
        }
        ClientEvent::TerminalReleasedToChild { reason, ready_tx, cancel_tx } => {
            app.terminal_lifecycle = crate::app::TerminalLifecycleState::ReleasedToChild(reason);
            app.surface_dirty.clear_for_child_release();
            if app.shutdown_requested() {
                let _ = cancel_tx.send(());
            } else {
                app.terminal_child_cancel = Some(cancel_tx);
            }
            let _ = ready_tx.send(());
        }
        ClientEvent::TerminalReturnedFromChild { reason: _ } => {
            app.terminal_child_cancel = None;
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
        ClientEvent::SkillsAndStylesReloaded { session_id: _, skill_names, output_style_names } => {
            apply_skills_and_styles_reloaded(app, skill_names.len(), output_style_names.len());
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
