// SPDX-License-Identifier: Apache-2.0

//! Claude Code's agent view from claude-rs: the composer footer's
//! `← N agents` status, polled from `claude agents --json`, and the hand-over
//! of the terminal to the stock `claude agents` view.

pub(crate) mod status;
#[cfg(test)]
mod tests;

pub use status::AgentStatus;

use crate::agent::events::ClientEvent;
use crate::app::App;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::rc::Rc;
use std::time::Duration;
use tokio::sync::Notify;

const POLL_INTERVAL: Duration = Duration::from_secs(10);
/// `claude agents --json` answered in about 0.2 s when measured; a call that
/// takes this long is stuck and is killed rather than awaited.
const POLL_TIMEOUT: Duration = Duration::from_secs(5);
/// Stock keeps this in its global config (`.claude.json`, beside the trust
/// records), not in settings.json; absent means on.
const LEFT_ARROW_SETTING: &str = "leftArrowOpensAgents";

#[derive(Debug, Default)]
pub(crate) struct AgentViewState {
    /// The latest background-session counts. `None` hides the footer hint:
    /// no listing yet, the listing failed, or the agent view is turned off.
    pub(crate) status: Option<AgentStatus>,
    /// Wakes the poller before its interval ends.
    refresh: Rc<Notify>,
}

/// The stock executable the session runs (`CLAUDE_CODE_EXECUTABLE`, possibly
/// a launcher wrapper that passes non-session argv to the stock binary
/// unchanged), else `claude` on PATH as /login finds it.
fn resolve_executable() -> Option<PathBuf> {
    executable_from(std::env::var_os("CLAUDE_CODE_EXECUTABLE"), || which::which("claude").ok())
}

fn executable_from(
    session_executable: Option<OsString>,
    on_path: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    match session_executable.filter(|value| !value.is_empty()) {
        // The bridge refuses a missing CLAUDE_CODE_EXECUTABLE too, so a
        // different `claude` from PATH would not be the session's own.
        Some(path) => Some(PathBuf::from(path)).filter(|path| path.is_file()),
        None => on_path(),
    }
}

fn preferences_path(app: &App) -> Option<PathBuf> {
    crate::claude_paths::ClaudePaths::resolve(app.settings_home_override.as_deref())
        .map(|paths| paths.preferences)
}

/// Read at each use, so a change made in Claude Code's /config applies
/// without restarting claude-rs. An unreadable file keeps stock's default.
fn left_arrow_opens_agents(preferences: Option<&Path>) -> bool {
    preferences
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
        .and_then(|document| document.get(LEFT_ARROW_SETTING).and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}

/// Polls `claude agents --json` at start, every `POLL_INTERVAL`, and right
/// after the agent view exits. One poll runs at a time; the child and the
/// parse run on a runtime worker, never on the UI thread.
pub fn start_status_poller(app: &App) {
    let Some(program) = resolve_executable() else {
        tracing::debug!(
            target: crate::logging::targets::APP_LIFECYCLE,
            event_name = "agent_status_poller_skipped",
            message = "no claude executable for the agent status",
            outcome = "skipped",
        );
        return;
    };
    let preferences = preferences_path(app);
    let cwd = app.cwd_raw.clone();
    let event_tx = app.event_tx.clone();
    let refresh = Rc::clone(&app.agent_view.refresh);
    tokio::task::spawn_local(async move {
        let mut last_sent = None;
        loop {
            let poll = tokio::spawn(poll_status(
                program.clone(),
                preferences.clone(),
                cwd.clone(),
                POLL_TIMEOUT,
            ));
            let status = poll.await.ok().flatten();
            if status != last_sent {
                if event_tx.send(ClientEvent::AgentStatusUpdated { status }).await.is_err() {
                    return;
                }
                last_sent = status;
            }
            tokio::select! {
                () = tokio::time::sleep(POLL_INTERVAL) => {}
                () = refresh.notified() => {}
            }
        }
    });
}

async fn poll_status(
    program: PathBuf,
    preferences: Option<PathBuf>,
    cwd: String,
    timeout: Duration,
) -> Option<AgentStatus> {
    if !left_arrow_opens_agents(preferences.as_deref()) {
        return None;
    }
    let result = match tokio::process::Command::new(&program)
        .args(["agents", "--json"])
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        // Dropping the timed-out future drops the child, which kills it.
        Ok(child) => match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(output)) if output.status.success() => status::parse_agents_json(&output.stdout),
            Ok(Ok(output)) => Err(format!("exit status {}", output.status)),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err(format!("no answer within {timeout:?}")),
        },
        Err(error) => Err(error.to_string()),
    };
    result
        .map_err(|error| {
            tracing::debug!(
                target: crate::logging::targets::APP_LIFECYCLE,
                event_name = "agent_status_poll_failed",
                message = "claude agents --json failed",
                outcome = "failure",
                error_message = %error,
            );
        })
        .ok()
}
