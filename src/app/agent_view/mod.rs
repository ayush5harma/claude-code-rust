// SPDX-License-Identifier: Apache-2.0

//! Claude Code's agent view from claude-rs: the composer footer's
//! `← N agents` status, read from Claude Code's job files, and the hand-over
//! of the terminal to the stock `claude agents` view.

mod jobs;
pub(crate) mod status;
#[cfg(test)]
mod tests;

pub use status::AgentStatus;

use crate::agent::events::ClientEvent;
use crate::app::terminal_runtime::{TerminalChild, claim_terminal, run_with_terminal};
use crate::app::{App, FocusOwner, ReleaseReason, SystemSeverity};
use jobs::{JobsScanner, Scan, Stamp, stamp_of};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// A poll is a few `stat` calls (see `jobs`), so the interval matches the
/// stock footer's freshness without a cost worth backing off from.
const POLL_INTERVAL: Duration = Duration::from_secs(10);
/// `claude agents --json` starts the 236 MB stock binary (about 0.17 s of
/// CPU, measured on 2.1.293). It is used only when the job files are in a
/// layout this reader does not know, and then at most this often.
const CLI_FALLBACK_INTERVAL: Duration = Duration::from_secs(60);
/// `claude agents --json` answered in about 0.2 s when measured; a call that
/// takes this long is stuck and is killed rather than awaited.
const CLI_TIMEOUT: Duration = Duration::from_secs(5);
/// Stock keeps this in its global config (`.claude.json`, beside the trust
/// records), not in settings.json; absent means on.
const LEFT_ARROW_SETTING: &str = "leftArrowOpensAgents";

#[derive(Debug)]
pub(crate) struct AgentViewState {
    /// Stock's `leftArrowOpensAgents`, as the poller last read it. The UI
    /// thread never reads the config file itself: it can be megabytes.
    pub(crate) enabled: bool,
    /// The latest background-session counts. `None` hides the footer hint:
    /// no reading yet, the reading failed, or the agent view is turned off.
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

fn left_arrow_opens_agents(preferences: &Path) -> bool {
    std::fs::read(preferences)
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
        .and_then(|document| document.get(LEFT_ARROW_SETTING).and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}

/// `leftArrowOpensAgents`, re-parsed only when `.claude.json` changes: the
/// file holds every project's records and can be megabytes. Missing or
/// unreadable keeps stock's default, on.
struct PreferencesCache {
    path: Option<PathBuf>,
    stamp: Option<Stamp>,
    enabled: bool,
}

impl PreferencesCache {
    fn new(path: Option<PathBuf>) -> Self {
        Self { path, stamp: None, enabled: true }
    }

    fn enabled(&mut self) -> bool {
        let Some(path) = self.path.as_deref() else {
            return true;
        };
        let stamp = stamp_of(path);
        if stamp.is_none() {
            self.stamp = None;
            self.enabled = true;
        } else if stamp != self.stamp {
            self.stamp = stamp;
            self.enabled = left_arrow_opens_agents(path);
        }
        self.enabled
    }
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

/// What a file poll found, before any CLI fallback.
enum FilePoll {
    Disabled,
    Scanned(Scan),
}

/// The file side of the poller, moved to a blocking worker for each poll.
struct FileReader {
    preferences: PreferencesCache,
    jobs: Option<JobsScanner>,
}

impl FileReader {
    fn poll(&mut self) -> FilePoll {
        if !self.preferences.enabled() {
            return FilePoll::Disabled;
        }
        let Some(jobs) = self.jobs.as_mut() else {
            return FilePoll::Scanned(Scan::Unrecognised("no Claude config directory".to_owned()));
        };
        if !cfg!(unix) {
            return FilePoll::Scanned(Scan::Unrecognised(
                "process liveness is read only on Unix".to_owned(),
            ));
        }
        FilePoll::Scanned(jobs.scan(chrono::Utc::now().timestamp_millis(), &pid_alive))
    }
}

/// Whether `pid` names a running process (stock also compares its start
/// time to rule out pid reuse; this does not).
fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: signal 0 performs only the existence and permission check;
        // no signal is delivered and no memory is passed.
        #[allow(unsafe_code)]
        let result = unsafe { libc::kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// `claude agents --json`, for a job layout the file reader does not know:
/// at most once per `CLI_FALLBACK_INTERVAL`, reusing the last answer between.
struct CliFallback {
    program: Option<PathBuf>,
    cwd: String,
    last_run: Option<Instant>,
    last: Option<AgentStatus>,
}

impl CliFallback {
    async fn status(&mut self) -> Option<AgentStatus> {
        if self.last_run.is_some_and(|ran| ran.elapsed() < CLI_FALLBACK_INTERVAL) {
            return self.last;
        }
        self.last_run = Some(Instant::now());
        self.last = match self.program.clone() {
            // The child and the parse run on a runtime worker.
            Some(program) => tokio::spawn(run_agents_json(program, self.cwd.clone(), CLI_TIMEOUT))
                .await
                .ok()
                .flatten(),
            None => None,
        };
        self.last
    }
}

/// Starts the poller once, after workspace trust is settled (the TUI loop
/// calls this when it starts the connection). It reads the job files at
/// once, every `POLL_INTERVAL`, and right after the agent view exits. One
/// poll runs at a time, on a blocking worker, never on the UI thread.
pub(crate) fn ensure_status_poller(app: &mut App) {
    if app.agent_view.poller_started {
        return;
    }
    app.agent_view.poller_started = true;
    let paths = crate::claude_paths::ClaudePaths::resolve(app.settings_home_override.as_deref());
    let mut reader = FileReader {
        preferences: PreferencesCache::new(paths.as_ref().map(|paths| paths.preferences.clone())),
        jobs: paths.map(|paths| JobsScanner::new(paths.config_dir)),
    };
    let mut fallback = CliFallback {
        program: resolve_executable(),
        cwd: app.cwd_raw.clone(),
        last_run: None,
        last: None,
    };
    let event_tx = app.event_tx.clone();
    let refresh = Rc::clone(&app.agent_view.refresh);
    tokio::task::spawn_local(async move {
        let mut last_sent = AgentPoll { enabled: true, status: None };
        let mut trigger = "start";
        loop {
            let Ok((returned, file_poll)) = tokio::task::spawn_blocking(move || {
                let file_poll = reader.poll();
                (reader, file_poll)
            })
            .await
            else {
                return;
            };
            reader = returned;
            let (poll, source) = match file_poll {
                FilePoll::Disabled => (AgentPoll { enabled: false, status: None }, "disabled"),
                FilePoll::Scanned(Scan::Counted(status)) => {
                    (AgentPoll { enabled: true, status: Some(status) }, "files")
                }
                FilePoll::Scanned(Scan::Unrecognised(reason)) => {
                    tracing::debug!(
                        target: crate::logging::targets::APP_LIFECYCLE,
                        event_name = "agent_jobs_unrecognised",
                        message = "job files not recognised; using claude agents --json",
                        outcome = "fallback",
                        reason,
                    );
                    (AgentPoll { enabled: true, status: fallback.status().await }, "cli")
                }
            };
            tracing::debug!(
                target: crate::logging::targets::APP_LIFECYCLE,
                event_name = "agent_status_polled",
                message = "agent status polled",
                outcome = if poll.status.is_some() { "success" } else { "hidden" },
                trigger,
                source,
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
            trigger = tokio::select! {
                () = tokio::time::sleep(POLL_INTERVAL) => "interval",
                () = refresh.notified() => "refresh",
            };
        }
    });
}

async fn run_agents_json(program: PathBuf, cwd: String, timeout: Duration) -> Option<AgentStatus> {
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
