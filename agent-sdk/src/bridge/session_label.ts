import type { SessionStoreEntry } from "@anthropic-ai/claude-agent-sdk";
import { emitSessionUpdate, refreshSessionsList } from "./events.js";
import type { SessionState } from "./session_lifecycle.js";

/** The name a session's transcript last recorded. */
export type StoredSessionLabel = {
  title?: string;
};

/**
 * The last `custom-title` record among a transcript's raw entries, as the
 * resume path loads them through the SDK. Claude Code writes it on /rename
 * and re-appends it on exit.
 */
export function storedSessionLabel(
  entries: readonly SessionStoreEntry[],
): StoredSessionLabel {
  let title: string | undefined;
  for (const entry of entries) {
    if (entry.type === "custom-title" && typeof entry.customTitle === "string") {
      title = Array.from(entry.customTitle)
        .filter((character) => {
          const codePoint = character.codePointAt(0) ?? 0;
          return codePoint > 0x1f && (codePoint < 0x7f || codePoint > 0x9f);
        })
        .join("")
        .trim() || undefined;
    }
  }
  return title !== undefined ? { title } : {};
}

export function handleSessionTitleChanged(
  session: SessionState,
  msg: Record<string, unknown>,
): void {
  if (typeof msg.title !== "string") {
    return;
  }
  session.sessionTitle = msg.title;
  if (session.connected) {
    emitSessionUpdate(session.sessionId, {
      type: "session_title_update",
      title: msg.title,
    });
    // The resume picker lists sessions by their title.
    refreshSessionsList();
  }
}

/** Called right after a connect or session-replaced event reached the app. */
export function emitSessionLabelAfterConnect(session: SessionState): void {
  if (session.sessionTitle !== undefined) {
    emitSessionUpdate(session.sessionId, {
      type: "session_title_update",
      title: session.sessionTitle,
    });
  }
}
