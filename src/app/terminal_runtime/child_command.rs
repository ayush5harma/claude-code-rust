// SPDX-License-Identifier: Apache-2.0

//! Hand the terminal to an interactive child process and take it back.

use super::TerminalReleaseGuard;
use crate::agent::events::ClientEvent;
use crate::app::ReleaseReason;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use tokio::sync::mpsc;

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
/// terminal modes change, and every path sends `TerminalReturnedFromChild`.
pub(crate) async fn run_with_terminal(
    tx: &mpsc::Sender<ClientEvent>,
    child: TerminalChild<'_>,
) -> Result<ExitStatus, String> {
    let TerminalChild { reason, command, label, program, args, cwd, interrupted_message } = child;
    // Enqueuing an event alone does not transfer ownership of inherited stdin.
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
    tx.send(ClientEvent::TerminalReleasedToChild { reason, ready_tx, cancel_tx })
        .await
        .map_err(|_| "UI stopped before terminal handoff".to_owned())?;
    ready_rx.await.map_err(|_| "UI did not acknowledge terminal handoff".to_owned())?;
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
            _ = &mut cancel_rx => {
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
