import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import type { SDKMessage, SessionStoreEntry } from "@anthropic-ai/claude-agent-sdk";
import { replaceProtocolEventWriter } from "./events.js";
import { handleSdkMessage } from "./message_handlers.js";
import type { SessionState } from "./session_lifecycle.js";
import {
  colorFromColorReply,
  emitSessionLabelAfterConnect,
  storedSessionLabel,
} from "./session_label.js";

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

// Reply texts measured from Claude Code 2.1.293.
test("a /color reply yields the colour it set, null on reset, nothing when rejected", () => {
  assert.equal(colorFromColorReply("Session color set to: blue"), "blue");
  assert.equal(colorFromColorReply("Session color reset to default"), null);
  assert.equal(
    colorFromColorReply(
      'Invalid color "nope". Available colors: red, blue, green, yellow, purple, orange, pink, cyan, default',
    ),
    undefined,
  );
  assert.equal(colorFromColorReply("Session color set to: magenta"), undefined);
});

test("only a /color reply changes the session colour", () => {
  const session = minimalSession(true);
  const reply = (command: string, text: string) =>
    ({
      type: "assistant",
      uuid: `${command}-reply`,
      session_id: "session-1",
      parent_tool_use_id: null,
      local_command_source: `<local-command-stdout>${text}</local-command-stdout>`,
      local_command_run: { command, args: "" },
      message: { model: "<synthetic>", role: "assistant", content: [{ type: "text", text }] },
    }) as unknown as SDKMessage;
  const updates = captureUpdates(() => {
    handleSdkMessage(session, reply("color", "Session color set to: orange"));
    handleSdkMessage(session, reply("color", 'Invalid color "x". Available colors: red'));
    handleSdkMessage(session, reply("rename", "Session color set to: red"));
    handleSdkMessage(session, reply("color", "Session color reset to default"));
  });
  assert.deepEqual(
    updates.filter((update) => update.type === "session_color_update"),
    [
      { session_id: "session-1", type: "session_color_update", color: "orange" },
      { session_id: "session-1", type: "session_color_update", color: null },
    ],
  );
});

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

test("a resumed session's colour reaches the app once, not again after /clear", () => {
  const session = minimalSession(true);
  session.resumedColor = "green";

  assert.deepEqual(captureUpdates(() => emitSessionLabelAfterConnect(session)), [
    { session_id: "session-1", type: "session_color_update", color: "green" },
  ]);
  // Claude Code 2.1.295 keeps the name but drops the colour on /clear.
  session.sessionId = "session-2";
  assert.deepEqual(captureUpdates(() => emitSessionLabelAfterConnect(session)), []);
});

test("the stored label is the transcript's last custom title and colour", () => {
  const entry = (record: Record<string, unknown>) => record as unknown as SessionStoreEntry;
  assert.deepEqual(
    storedSessionLabel([
      entry({ type: "custom-title", customTitle: "first" }),
      entry({ type: "agent-color", agentColor: "blue" }),
      entry({ type: "user", message: { role: "user", content: "hi" } }),
      entry({ type: "custom-title", customTitle: "renamed-live" }),
      entry({ type: "agent-color", agentColor: "pink" }),
    ]),
    { title: "renamed-live", color: "pink" },
  );
  assert.deepEqual(
    storedSessionLabel([
      entry({ type: "agent-color", agentColor: "blue" }),
      entry({ type: "agent-color", agentColor: "default" }),
    ]),
    {},
  );
  assert.deepEqual(storedSessionLabel([]), {});
});
