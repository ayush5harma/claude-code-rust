import { getSessionInfo, renameSession } from "@anthropic-ai/claude-agent-sdk";
import { emitSessionUpdate } from "./events.js";
import { bridgeLogger, LOG_TARGETS } from "./logger.js";
import type { SessionState } from "./session_lifecycle.js";

/**
 * Send the session's title as the public session API reports it, so the app
 * never keeps a title of its own making. The app stores it idempotently, so
 * every read that returns a title sends it.
 *
 * Measured on Claude Code 2.1.288 (bundled with the pinned SDK): a launch-time
 * `-n` name and a /rename reach `getSessionInfo` only after a turn, so the
 * title is read again after every top-level turn and every rename the app
 * requests. After /clear the new session id already reports the old title.
 * `customTitle` falls back to the generated title after the first turn.
 */
export async function emitSessionTitle(session: SessionState): Promise<void> {
  const sessionId = session.sessionId;
  const read = (session.titleReads ?? 0) + 1;
  session.titleReads = read;
  let title: string | undefined;
  try {
    title = (await getSessionInfo(sessionId, { dir: session.cwd }))?.customTitle;
  } catch (error) {
    bridgeLogger.warn({
      target: LOG_TARGETS.APP_SESSION,
      eventName: "session_title_read_failed",
      message: "failed to read the session title",
      outcome: "failure",
      sessionId,
      fields: { error_message: error instanceof Error ? error.message : String(error) },
    });
    return;
  }
  // A later read, a replacement or a close while reading supersedes this one.
  if (
    title === undefined ||
    session.closing ||
    session.sessionId !== sessionId ||
    session.titleReads !== read
  ) {
    return;
  }
  emitSessionUpdate(sessionId, { type: "session_title_update", title });
}

/** Restore a saved name only when resume lost it; never overwrite a newer name. */
export async function restoreSessionTitleAfterResume(
  session: SessionState,
  savedTitle: string | undefined,
): Promise<void> {
  const title = savedTitle?.trim();
  if (!title || session.closing) return;
  try {
    const current = (await getSessionInfo(session.sessionId, { dir: session.cwd }))?.customTitle?.trim();
    if (current) return;
    // The 2026-10-12 parity run found that an SDK resume can drop a known
    // custom title from the session registry used by other clients.
    await renameSession(session.sessionId, title, { dir: session.cwd });
    await emitSessionTitle(session);
  } catch (error) {
    bridgeLogger.warn({
      target: LOG_TARGETS.APP_SESSION,
      eventName: "session_title_restore_failed",
      message: "failed to restore the saved session title after resume",
      outcome: "failure",
      sessionId: session.sessionId,
      fields: { error_message: error instanceof Error ? error.message : String(error) },
    });
  }
}
