// SPDX-License-Identifier: Apache-2.0

//! Claude Code's agent view from claude-rs: the composer footer's
//! `← N agents` status, polled from `claude agents --json`, and the hand-over
//! of the terminal to the stock `claude agents` view.

pub(crate) mod status;
#[cfg(test)]
mod tests;

pub use status::AgentStatus;

use crate::agent::events::ClientEvent;
use crate::app::terminal_runtime::{TerminalChild, claim_terminal, run_with_terminal};
use crate::app::{App, FocusOwner, ReleaseReason, SystemSeverity};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::rc::Rc;
use std::time::Duration;
use tokio::sync::Notify;

/// The poll interval while background sessions exist.
const POLL_INTERVAL: Duration = Duration::from_secs(10);
/// One poll costs about 0.17 s of CPU (measured on 2.1.293) and several
/// claude-rs instances may run at once, so a listing with no background
/// sessions is checked less often. Returning from the view polls at once.
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(30);
/// `claude agents --json` answered in about 0.2 s when measured; a call that
/// takes this long is stuck and is killed rather than awaited.
const POLL_TIMEOUT: Duration = Duration::from_secs(5);
/// Stock keeps this in its global config (`.claude.json`, beside the trust
/// records), not in settings.json; absent means on.
const LEFT_ARROW_SETTING: &str = "leftArrowOpensAgents";

#[derive(Debug)]
pub(crate) struct AgentViewState {
    /// Stock's `leftArrowOpensAgents`, as the poller last read it. The UI
    /// thread never reads the config file itself: it can be megabytes.
    pub(crate) enabled: bool,
    /// The latest background-session counts. `None` hides the footer hint:
    /// no listing yet, the listing failed, or the agent view is turned off.
    pub(crate) status: Option<AgentStatus>,
    poller_started: bool,
    /// Wakes the poller before its interval ends.
    refresh: Rc<Notify>,
}

impl Default for AgentViewState {
    fn default() -> Self {
        Self { enabled: true, status: None, poller_started: false, refresh: Rc::default() }
    }
}

/// One poll's result, sent to the UI when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AgentPoll {
    enabled: bool,
    status: Option<AgentStatus>,
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

/// Read on the poller's worker at every poll, so a change made in Claude
/// Code's /config applies within one interval. An unreadable file keeps
/// stock's default.
fn left_arrow_opens_agents(preferences: Option<&Path>) -> bool {
    preferences
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
        .and_then(|document| document.get(LEFT_ARROW_SETTING).and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}

/// The one rule for whether the agent view action opens the view now, used
/// by the key and by the footer hint that advertises it: the setting allows
/// it, no other child owns the terminal, and the editable composer has focus
/// and is empty.
pub(crate) fn opens_from_composer(app: &App) -> bool {
    app.agent_view.enabled
        && !app.terminal_child.is_active()
        && app.composer_access().can_edit()
        && app.focus_owner() == FocusOwner::Input
        && !app.has_local_input()
}

/// Starts the poller once, after workspace trust is settled (the TUI loop
/// calls this when it starts the connection). It polls
/// `claude agents --json` at once, then every `POLL_INTERVAL` while
/// background sessions exist or `IDLE_POLL_INTERVAL` otherwise, and right
/// after the agent view exits. One poll runs at a time; the child and the
/// parse run on a runtime worker, never on the UI thread.
pub(crate) fn ensure_status_poller(app: &mut App) {
    if app.agent_view.poller_started {
        return;
    }
    app.agent_view.poller_started = true;
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
        let mut last_sent = AgentPoll { enabled: true, status: None };
        let mut trigger = "start";
        loop {
            let poll = tokio::spawn(poll_status(
                program.clone(),
                preferences.clone(),
                cwd.clone(),
                POLL_TIMEOUT,
            ));
            let poll = poll.await.unwrap_or(AgentPoll { enabled: true, status: None });
            tracing::debug!(
                target: crate::logging::targets::APP_LIFECYCLE,
                event_name = "agent_status_polled",
                message = "agent status polled",
                outcome = if poll.status.is_some() { "success" } else { "hidden" },
                trigger,
                enabled = poll.enabled,
                background = poll.status.map_or(0, |status| status.background),
            );
            if poll != last_sent {
                let event =
                    ClientEvent::AgentStatusUpdated { enabled: poll.enabled, status: poll.status };
                if event_tx.send(event).await.is_err() {
                    return;
                }
                last_sent = poll;
            }
            let interval = if poll.status.is_some_and(|status| status.background > 0) {
                POLL_INTERVAL
            } else {
                IDLE_POLL_INTERVAL
            };
            trigger = tokio::select! {
                () = tokio::time::sleep(interval) => "interval",
                () = refresh.notified() => "refresh",
            };
        }
    });
}

async fn poll_status(
    program: PathBuf,
    preferences: Option<PathBuf>,
    cwd: String,
    timeout: Duration,
) -> AgentPoll {
    if !left_arrow_opens_agents(preferences.as_deref()) {
        return AgentPoll { enabled: false, status: None };
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
    let status = result
        .map_err(|error| {
            tracing::debug!(
                target: crate::logging::targets::APP_LIFECYCLE,
                event_name = "agent_status_poll_failed",
                message = "claude agents --json failed",
                outcome = "failure",
                error_message = %error,
            );
        })
        .ok();
    AgentPoll { enabled: true, status }
}

/// Hands the terminal to the stock agent view until it exits (Esc, or
/// Ctrl+C twice). The claude-rs session keeps running meanwhile. Does
/// nothing while another hand-over is in flight, so a repeated key cannot
/// start a second child.
pub(crate) fn open(app: &mut App) {
    let Some(program) = resolve_executable() else {
        crate::app::events::push_system_message_with_severity(
            app,
            Some(SystemSeverity::Error),
            "claude CLI not found: set CLAUDE_CODE_EXECUTABLE or put claude on PATH to open the agent view.",
        );
        return;
    };
    let Some(claim) = claim_terminal(app) else {
        return;
    };
    let event_tx = app.event_tx.clone();
    let refresh = Rc::clone(&app.agent_view.refresh);
    let cwd = app.cwd_raw.clone();
    tokio::task::spawn_local(async move {
        let result = run_with_terminal(
            &event_tx,
            claim,
            TerminalChild {
                reason: ReleaseReason::AgentView,
                command: "agents",
                label: "claude agents",
                program: &program,
                args: &["agents"],
                cwd: Some(&cwd),
                interrupted_message: "Agent view closed by shutdown",
            },
        )
        .await;
        refresh.notify_one();
        let message = match result {
            Ok(status) if closed_normally(status) => return,
            Ok(status) => format!(
                "claude agents exited with code {}",
                status.code().map_or_else(|| "unknown".to_owned(), |code| code.to_string())
            ),
            Err(message) => message,
        };
        let _ = event_tx.send(ClientEvent::AgentViewFailed { message }).await;
    });
}

/// A Ctrl+C before the view takes raw mode interrupts it; that is the user
/// leaving, not a failure.
fn closed_normally(status: ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if status.signal() == Some(libc::SIGINT) {
            return true;
        }
    }
    status.success()
}
