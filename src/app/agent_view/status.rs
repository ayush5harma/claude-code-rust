// SPDX-License-Identifier: Apache-2.0

//! Counts of Claude Code's background sessions, read from `claude agents --json`.

use serde::Deserialize;

/// What the composer footer reports about the stock agent view's background
/// sessions. Interactive sessions (including this one) are not counted: the
/// stock footer's `← N agents` counts background sessions only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentStatus {
    pub background: usize,
    pub awaiting_input: usize,
    pub working: usize,
}

#[derive(Deserialize)]
struct ListedSession {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

/// Stock Claude Code 2.1.293 (`printAgentsJson`) gives every background entry
/// one `state` of working, blocked, done, failed or stopped; its agent view
/// files "blocked" under "Needs input" and its header counts it as
/// "awaiting input".
const STATE_AWAITING_INPUT: &str = "blocked";
const STATE_WORKING: &str = "working";
const KIND_BACKGROUND: &str = "background";

pub(crate) fn parse_agents_json(bytes: &[u8]) -> Result<AgentStatus, String> {
    let sessions: Vec<ListedSession> = serde_json::from_slice(bytes)
        .map_err(|error| format!("Failed to parse `claude agents --json`: {error}"))?;
    let mut status = AgentStatus::default();
    for session in
        sessions.iter().filter(|session| session.kind.as_deref() == Some(KIND_BACKGROUND))
    {
        status.background += 1;
        match session.state.as_deref() {
            Some(STATE_AWAITING_INPUT) => status.awaiting_input += 1,
            Some(STATE_WORKING) => status.working += 1,
            _ => {}
        }
    }
    Ok(status)
}
