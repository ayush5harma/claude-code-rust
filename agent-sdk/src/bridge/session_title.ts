import { getSessionInfo } from "@anthropic-ai/claude-agent-sdk";
import type { BridgeCommand } from "../types.js";
import { emitSessionUpdate, refreshSessionsList } from "./events.js";
import { bridgeLogger, LOG_TARGETS } from "./logger.js";
import type { SessionState } from "./session_lifecycle.js";

const RENAME_COMMAND = /^\/rename(?:\s|$)/;

/**
 * Send the title Claude Code persisted for the session, read through the
 * public session API, so the app never keeps a title of its own making. This
 * runs after every connect and session replacement and after a /rename turn;
 * whether a title survives /clear is whatever the API reports for the new
 * session id (Claude Code 2.1.296 carries it over).
 */
export async function emitSessionTitle(session: SessionState): Promise<void> {
  const sessionId = session.sessionId;
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
  // A replacement while reading sends its own title.
  if (title === undefined || session.closing || session.sessionId !== sessionId) {
    return;
  }
  emitSessionUpdate(sessionId, { type: "session_title_update", title });
}

/** Remember a prompt sent as /rename: its turn's result changes the title. */
export function notePromptTitleChange(
  session: SessionState,
  command: Extract<BridgeCommand, { command: "prompt" }>,
): void {
  const text = command.chunks
    .map((chunk) => (chunk.kind === "text" && typeof chunk.value === "string" ? chunk.value : ""))
    .join("");
  if (RENAME_COMMAND.test(text.trimStart())) {
    session.renamePromptUuids ??= new Set();
    session.renamePromptUuids.add(command.message_uuid);
  }
}

/** Refresh the title once a turn that ran one of those prompts has its result. */
export function refreshTitleAfterTurn(
  session: SessionState,
  userMessageUuids: readonly string[],
): void {
  const pending = session.renamePromptUuids;
  // Every uuid is consumed, since a batched turn can run several prompts.
  const ran = pending ? userMessageUuids.filter((uuid) => pending.delete(uuid)) : [];
  if (ran.length === 0) {
    return;
  }
  void emitSessionTitle(session);
  // The resume picker lists sessions by their title.
  refreshSessionsList();
}
