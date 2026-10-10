import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import type { SDKMessage, SessionStoreEntry } from "@anthropic-ai/claude-agent-sdk";
import { replaceProtocolEventWriter } from "./events.js";
import { handleSdkMessage } from "./message_handlers.js";
import type { SessionState } from "./session_lifecycle.js";
import { emitSessionLabelAfterConnect, storedSessionLabel } from "./session_label.js";

function captureEvents(run: () => void): Array<Record<string, unknown>> {
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter((line) => {
    writes.push(line);
  });
  try {
    run();
  } finally {
    restore();
  }
  return writes.map((line) => JSON.parse(line) as Record<string, unknown>);
}

function captureUpdates(run: () => void): Array<Record<string, unknown>> {
  return captureEvents(run)
    .filter((event) => event.event === "session_update")
    .map((event) => ({ session_id: event.session_id, ...(event.update as object) }));
}

function minimalSession(connected: boolean): SessionState {
  return {
    sessionId: "session-1",
    cwd: "/nowhere",
    connected,
    closing: false,
    toolCalls: new Map(),
    hiddenToolUseIds: new Set(),
  } as unknown as SessionState;
}

const titleChanged = (title: string) =>
  ({
    type: "system",
    subtype: "session_title_changed",
    title,
    session_id: "session-1",
  }) as unknown as SDKMessage;

test("a title announced before connect reaches the app after it, and again after a replacement", () => {
  const session = minimalSession(false);

  assert.deepEqual(captureUpdates(() => handleSdkMessage(session, titleChanged("probe-e2e"))), []);

  session.connected = true;
  assert.deepEqual(captureUpdates(() => emitSessionLabelAfterConnect(session)), [
    { session_id: "session-1", type: "session_title_update", title: "probe-e2e" },
  ]);

  // /clear replaces the session id and the child keeps the title.
  session.sessionTitle = "renamed";
  session.sessionId = "session-2";
  assert.deepEqual(captureUpdates(() => emitSessionLabelAfterConnect(session)), [
    { session_id: "session-2", type: "session_title_update", title: "renamed" },
  ]);
});

test("a live rename refreshes the session list the resume picker shows", async () => {
  const configDir = mkdtempSync(path.join(os.tmpdir(), "label-config-"));
  const previous = process.env.CLAUDE_CONFIG_DIR;
  process.env.CLAUDE_CONFIG_DIR = configDir;
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter((line) => {
    writes.push(line);
  });
  const listed = () => writes.some((line) => line.includes('"sessions_listed"'));
  try {
    handleSdkMessage(minimalSession(true), titleChanged("renamed-live"));
    const deadline = Date.now() + 5_000;
    while (!listed() && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    assert.ok(listed(), "a rename must re-list sessions");
  } finally {
    restore();
    if (previous === undefined) {
      delete process.env.CLAUDE_CONFIG_DIR;
    } else {
      process.env.CLAUDE_CONFIG_DIR = previous;
    }
    rmSync(configDir, { recursive: true, force: true });
  }
});

test("the stored label is the transcript's last custom title", () => {
  const entry = (record: Record<string, unknown>) => record as unknown as SessionStoreEntry;
  assert.deepEqual(
    storedSessionLabel([
      entry({ type: "custom-title", customTitle: "first" }),
      entry({ type: "user", message: { role: "user", content: "hi" } }),
      entry({ type: "custom-title", customTitle: "renamed-live" }),
    ]),
    { title: "renamed-live" },
  );
  assert.deepEqual(storedSessionLabel([]), {});
});

test("stored session titles discard terminal controls before trimming for the SDK name argument", () => {
  const entry = (customTitle: string) =>
    ({ type: "custom-title", customTitle }) as SessionStoreEntry;
  assert.deepEqual(
    storedSessionLabel([entry(" \u0000Re\u001bna\u007f\u009fmed\u001f\u0085 ")]),
    { title: "Renamed" },
  );
  assert.deepEqual(storedSessionLabel([entry(" \u0000\u001f\u007f\u009f ")]), {});
});
