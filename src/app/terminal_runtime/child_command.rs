// SPDX-License-Identifier: Apache-2.0

//! Hand the terminal to an interactive child process and take it back.

use super::TerminalReleaseGuard;
use crate::agent::events::ClientEvent;
use crate::app::{App, ReleaseReason};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use tokio::sync::{mpsc, oneshot};

/// Who owns the terminal besides the TUI. One hand-over at a time: a second
/// request while one is claimed or running is refused, so two children never
/// race for the terminal and a cancel sender is never overwritten.
#[derive(Debug, Default)]
pub(crate) enum TerminalChildState {
    #[default]
    Idle,
    /// Claimed synchronously by the key or command that starts a child; the
    /// child task has not taken the terminal yet.
    Claimed,
    /// The child owns the terminal. Sending stops it.
    Running(oneshot::Sender<()>),
}

impl TerminalChildState {
    pub(crate) fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

/// Proof that this caller claimed the terminal; `run_with_terminal` takes it.
#[must_use]
pub(crate) struct TerminalClaim(());

/// Claims the terminal for one child, or returns `None` while another
/// hand-over is in flight (for example a repeated key).
pub(crate) fn claim_terminal(app: &mut App) -> Option<TerminalClaim> {
    if app.terminal_child.is_active() {
        return None;
    }
    app.terminal_child = TerminalChildState::Claimed;
    Some(TerminalClaim(()))
}

/// The child task's release request reached the UI.
pub(crate) fn child_took_terminal(app: &mut App, cancel_tx: oneshot::Sender<()>) {
    if app.shutdown_requested() {
        let _ = cancel_tx.send(());
        app.terminal_child = TerminalChildState::Claimed;
    } else {
        app.terminal_child = TerminalChildState::Running(cancel_tx);
    }
}

/// The terminal is back with the TUI; the next hand-over may start.
pub(crate) fn child_returned_terminal(app: &mut App) {
    app.terminal_child = TerminalChildState::Idle;
}

/// Asks a running child to stop (app shutdown). The claim stays until the
/// child task reports the terminal returned.
pub(crate) fn stop_terminal_child(app: &mut App) {
    if !matches!(app.terminal_child, TerminalChildState::Running(_)) {
        return;
    }
    if let TerminalChildState::Running(cancel_tx) =
        std::mem::replace(&mut app.terminal_child, TerminalChildState::Claimed)
    {
        let _ = cancel_tx.send(());
    }
}

/// One interactive child that owns the terminal until it exits.
pub(crate) struct TerminalChild<'a> {
    pub(crate) reason: ReleaseReason,
    /// Short name for logs, e.g. "login" or "agents".
    pub(crate) command: &'static str,
    /// How error messages name the child, e.g. "claude auth login".
    pub(crate) label: &'a str,
    pub(crate) program: &'a Path,
    pub(crate) args: &'a [&'a str],
    pub(crate) cwd: Option<&'a str>,
    /// The error returned when app shutdown stops the child.
    pub(crate) interrupted_message: &'a str,
}

/// Runs `child` with inherited stdio while the UI has released the terminal.
///
/// The UI acknowledges the release (dropping its input reader) before the
/// terminal modes change, and every path after that sends
/// `TerminalReturnedFromChild`, which also ends the claim.
pub(crate) async fn run_with_terminal(
    tx: &mpsc::Sender<ClientEvent>,
    claim: TerminalClaim,
    child: TerminalChild<'_>,
) -> Result<ExitStatus, String> {
    let TerminalClaim(()) = claim;
    let TerminalChild { reason, command, label, program, args, cwd, interrupted_message } = child;
    // Enqueuing an event alone does not transfer ownership of inherited stdin.
    let (ready_tx, ready_rx) = oneshot::channel();
    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    tx.send(ClientEvent::TerminalReleasedToChild { reason, ready_tx, cancel_tx })
        .await
        .map_err(|_| "UI stopped before terminal handoff".to_owned())?;
    if ready_rx.await.is_err() {
        send_returned(tx, reason).await;
        return Err("UI did not acknowledge terminal handoff".to_owned());
    }
    let terminal_release = match TerminalReleaseGuard::release(reason, command) {
        Ok(terminal_release) => terminal_release,
        Err(err) => {
            send_returned(tx, reason).await;
            return Err(format!("Failed to release terminal for {label}: {err}"));
        }
    };

    let mut process = tokio::process::Command::new(program);
    process
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    if let Some(cwd) = cwd {
        process.current_dir(cwd);
    }
    let result = match process.spawn() {
        Ok(mut child) => tokio::select! {
            status = child.wait() => status.map_err(|err| format!("Failed to run {label}: {err}")),
            // Only an explicit stop counts; a dropped sender is not a request
            // to stop, so that branch is disabled and the child keeps running.
            Ok(()) = &mut cancel_rx => {
                // This PID belongs to this task. Reap it before returning
                // terminal ownership, including during fatal bridge shutdown.
                let result = child.kill().await;
                Err(result.map_or_else(
                    |err| format!("Failed to stop {label}: {err}"),
                    |()| interrupted_message.to_owned(),
                ))
            }
        },
        Err(err) => Err(format!("Failed to run {label}: {err}")),
    };

    let restore_result = terminal_release.restore();
    send_returned(tx, reason).await;
    restore_result.map_err(|err| format!("Failed to restore terminal after {label}: {err}"))?;

    result
}

async fn send_returned(tx: &mpsc::Sender<ClientEvent>, reason: ReleaseReason) {
    let _ = tx.send(ClientEvent::TerminalReturnedFromChild { reason }).await;
}
