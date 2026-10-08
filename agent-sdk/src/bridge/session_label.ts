import { open, readdir, realpath } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import type { SessionColor } from "../types.js";
import { emitSessionUpdate } from "./events.js";
import type { SessionState } from "./session_lifecycle.js";
import { bridgeLogger, LOG_TARGETS } from "./logger.js";

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

// Stock names a project directory after the cwd with every non-alphanumeric
// character replaced by "-", and past 200 characters keeps that prefix and
// appends a hash of the full path.
const PROJECT_DIR_NAME_LIMIT = 200;
// Stock re-appends a session's metadata (agent-color among it) at the end of
// its transcript, so the newest entry sits in the tail; never read the whole
// file of a long session looking for a colour it does not have.
const TRANSCRIPT_TAIL_LIMIT_BYTES = 4 * 1024 * 1024;
const TRANSCRIPT_CHUNK_BYTES = 64 * 1024;

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
  }
}

/**
 * Called right after a connect or session-replaced event reached the app.
 * The returned promise settles once the stored colour, if any, was sent.
 */
export function emitSessionLabelAfterConnect(
  session: SessionState,
): Promise<void> {
  if (session.sessionTitle !== undefined) {
    emitSessionUpdate(session.sessionId, {
      type: "session_title_update",
      title: session.sessionTitle,
    });
  }
  return emitStoredSessionColor(session);
}

async function emitStoredSessionColor(session: SessionState): Promise<void> {
  const sessionId = session.sessionId;
  try {
    const stored = await readStoredSessionColor(
      claudeConfigDir(),
      session.cwd,
      sessionId,
    );
    if (stored && session.sessionId === sessionId && !session.closing) {
      emitSessionUpdate(sessionId, { type: "session_color_update", color: stored });
    }
  } catch (error) {
    bridgeLogger.debug({
      target: LOG_TARGETS.APP_SESSION,
      eventName: "session_color_read_failed",
      message: "stored session colour could not be read",
      outcome: "failure",
      sessionId,
      fields: { error_message: String(error) },
    });
  }
}

function claudeConfigDir(): string {
  return process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), ".claude");
}

function projectDirName(cwd: string): string {
  return cwd.replace(/[^a-zA-Z0-9]/g, "-");
}

async function transcriptCandidates(
  projectsDir: string,
  cwd: string,
  sessionId: string,
): Promise<string[]> {
  const cwds = new Set([cwd]);
  try {
    cwds.add(await realpath(cwd));
  } catch {
    // A cwd that no longer exists still names its project directory.
  }
  const candidates: string[] = [];
  for (const dir of cwds) {
    const name = projectDirName(dir);
    if (name.length <= PROJECT_DIR_NAME_LIMIT) {
      candidates.push(path.join(projectsDir, name, `${sessionId}.jsonl`));
      continue;
    }
    const prefix = `${name.slice(0, PROJECT_DIR_NAME_LIMIT)}-`;
    let entries: string[] = [];
    try {
      entries = await readdir(projectsDir);
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (entry.startsWith(prefix)) {
        candidates.push(path.join(projectsDir, entry, `${sessionId}.jsonl`));
      }
    }
  }
  return candidates;
}

/**
 * The session's colour as its transcript last recorded it (Claude Code writes
 * `{"type":"agent-color","agentColor":...}` on /color and on exit), or
 * undefined when none is recorded or the last one is "default".
 */
export async function readStoredSessionColor(
  configDir: string,
  cwd: string,
  sessionId: string,
): Promise<SessionColor | undefined> {
  const projectsDir = path.join(configDir, "projects");
  for (const file of await transcriptCandidates(projectsDir, cwd, sessionId)) {
    const recorded = await lastRecordedColor(file);
    if (recorded !== "missing") {
      return asSessionColor(recorded);
    }
  }
  return undefined;
}

async function lastRecordedColor(file: string): Promise<unknown> {
  let handle: Awaited<ReturnType<typeof open>>;
  try {
    handle = await open(file, "r");
  } catch {
    return "missing";
  }
  try {
    const { size } = await handle.stat();
    const floor = Math.max(0, size - TRANSCRIPT_TAIL_LIMIT_BYTES);
    let end = size;
    let carry = Buffer.alloc(0);
    while (end > floor) {
      const start = Math.max(floor, end - TRANSCRIPT_CHUNK_BYTES);
      const chunk = Buffer.alloc(end - start);
      await handle.read(chunk, 0, chunk.length, start);
      let data = Buffer.concat([chunk, carry]);
      end = start;
      if (start > floor) {
        // Bytes before the first newline belong to a line that starts in the
        // chunk read next; keep them (as bytes, so no character is split).
        const firstNewline = data.indexOf(0x0a);
        if (firstNewline < 0) {
          carry = data;
          continue;
        }
        carry = data.subarray(0, firstNewline);
        data = data.subarray(firstNewline + 1);
      }
      for (const line of data.toString("utf8").split("\n").reverse()) {
        const color = agentColorOfLine(line);
        if (color !== undefined) {
          return color;
        }
      }
    }
    return undefined;
  } finally {
    await handle.close();
  }
}

function agentColorOfLine(line: string): unknown {
  if (!line.includes('"agent-color"')) {
    return undefined;
  }
  try {
    const entry = JSON.parse(line) as Record<string, unknown>;
    return entry.type === "agent-color" ? entry.agentColor : undefined;
  } catch {
    return undefined;
  }
}
