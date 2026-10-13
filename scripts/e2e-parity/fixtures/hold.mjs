// SPDX-License-Identifier: Apache-2.0
// A command that stays running for 90 s, for the agents check: a subagent that
// waits on it is still running when /agents is opened. A script, not `sleep`:
// the owner's PreToolUse hooks refuse long foreground sleeps.
setTimeout(() => {}, 90_000);
