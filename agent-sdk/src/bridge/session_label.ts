import type { SessionStoreEntry } from "@anthropic-ai/claude-agent-sdk";
import type { SessionColor } from "../types.js";
import { emitSessionUpdate, refreshSessionsList } from "./events.js";
import type { SessionState } from "./session_lifecycle.js";

// The colours Claude Code 2.1.293's /color accepts besides "default"
// (measured: its invalid-colour reply lists exactly these).
const SESSION_COLORS: readonly SessionColor[] = [
  "red",
  "blue",
  "green",
  "yellow",
  "purple",
  "orange",
  "pink",
  "cyan",
];

const COLOR_SET_PREFIX = "Session color set to: ";
const COLOR_RESET_REPLY = "Session color reset to default";

/** The name and colour a session's transcript last recorded. */
export type StoredSessionLabel = {
  title?: string;
  color?: SessionColor;
};

function asSessionColor(value: unknown): SessionColor | undefined {
  return SESSION_COLORS.find((color) => color === value);
}

/**
 * The colour a /color reply establishes: a colour, null for a reset, or
 * undefined when the reply changed nothing (an invalid colour).
 */
export function colorFromColorReply(
  text: string,
): SessionColor | null | undefined {
  const reply = text.trim();
  if (reply === COLOR_RESET_REPLY) {
    return null;
  }
  if (reply.startsWith(COLOR_SET_PREFIX)) {
    return asSessionColor(reply.slice(COLOR_SET_PREFIX.length).trim());
  }
  return undefined;
}

export function emitSessionColorFromReply(
  session: SessionState,
  text: string,
): void {
  const color = colorFromColorReply(text);
  if (color !== undefined) {
    emitSessionUpdate(session.sessionId, { type: "session_color_update", color });
  }
}

/**
 * The last `custom-title` and `agent-color` records among a transcript's raw
 * entries, as the resume path loads them through the SDK. Claude Code writes
 * them on /rename and /color and re-appends them on exit; a last colour of
 * "default" means none.
 */
export function storedSessionLabel(
  entries: readonly SessionStoreEntry[],
): StoredSessionLabel {
  let title: string | undefined;
  let color: unknown;
  for (const entry of entries) {
    if (entry.type === "custom-title" && typeof entry.customTitle === "string") {
      title = Array.from(entry.customTitle)
        .filter((character) => {
          const codePoint = character.codePointAt(0) ?? 0;
          return codePoint > 0x1f && (codePoint < 0x7f || codePoint > 0x9f);
        })
        .join("")
        .trim() || undefined;
    } else if (entry.type === "agent-color") {
      color = entry.agentColor;
    }
  }
  const stored = asSessionColor(color);
  return {
    ...(title !== undefined ? { title } : {}),
    ...(stored !== undefined ? { color: stored } : {}),
  };
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
  // A resumed colour belongs to the session as it was resumed; Claude Code
  // drops the colour on /clear (measured on 2.1.295), so it is sent once.
  const color = session.resumedColor;
  session.resumedColor = undefined;
  if (color !== undefined) {
    emitSessionUpdate(session.sessionId, { type: "session_color_update", color });
  }
}
