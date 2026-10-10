import { getSessionInfo } from "@anthropic-ai/claude-agent-sdk";
import { emitSessionUpdate, refreshSessionsList } from "./events.js";
import { bridgeLogger, LOG_TARGETS } from "./logger.js";
import type { SessionState } from "./session_lifecycle.js";

/**
 * Send the title Claude Code persisted for the session, read through the
 * public session API, so the app never keeps a title of its own making.
 *
 * Measured on Claude Code 2.1.296: a launch-time `-n` name and a /rename reach
 * the transcript only with a turn (`getSessionInfo` reports nothing right
 * after init), so the title is read after every connect and replacement, and
 * again after every top-level turn, where only a changed title is sent. A
 * title survives /clear when the API reports it for the new session id.
 * `customTitle` also falls back to Claude Code's generated title, so an
 * unnamed session shows that title after its first turn.
 */
export async function emitSessionTitle(
  session: SessionState,
  after: "connect" | "turn",
): Promise<void> {
  if (after === "connect") {
    // The app forgets the title when the session id changes.
    session.sentTitle = undefined;
  }
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
    title === session.sentTitle ||
    session.closing ||
    session.sessionId !== sessionId ||
    session.titleReads !== read
  ) {
    return;
  }
  session.sentTitle = title;
  emitSessionUpdate(sessionId, { type: "session_title_update", title });
  if (after === "turn") {
    // The resume picker lists sessions by their title.
    refreshSessionsList();
  }
}
