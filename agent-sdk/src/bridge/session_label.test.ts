import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import type { SDKMessage } from "@anthropic-ai/claude-agent-sdk";
import { replaceProtocolEventWriter } from "./events.js";
import { handleSdkMessage } from "./message_handlers.js";
import type { SessionState } from "./session_lifecycle.js";
import {
  colorFromColorReply,
  emitSessionLabelAfterConnect,
  readStoredSessionColor,
} from "./session_label.js";

function captureUpdates(run: () => void): Array<Record<string, unknown>> {
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter((line) => {
    writes.push(line);
  });
  try {
    run();
  } finally {
    restore();
  }
  return writes
    .map((line) => JSON.parse(line) as Record<string, unknown>)
    .filter((event) => event.event === "session_update")
    .map((event) => ({ session_id: event.session_id, ...(event.update as object) }));
}

function minimalSession(connected: boolean, cwd = "/nowhere"): SessionState {
  return {
    sessionId: "session-1",
    cwd,
    connected,
    closing: false,
    toolCalls: new Map(),
    hiddenToolUseIds: new Set(),
  } as unknown as SessionState;
}

function transcriptEntry(color: string): string {
  return JSON.stringify({ type: "agent-color", agentColor: color, sessionId: "s" });
}

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

test("a title announced before connect reaches the app after it, and again after a replacement", async () => {
  const session = minimalSession(false);
  const titleChanged = {
    type: "system",
    subtype: "session_title_changed",
    title: "probe-e2e",
    session_id: "session-1",
  } as unknown as SDKMessage;

  assert.deepEqual(captureUpdates(() => handleSdkMessage(session, titleChanged)), []);

  session.connected = true;
  let pending: Promise<void> = Promise.resolve();
  const afterConnect = captureUpdates(() => {
    pending = emitSessionLabelAfterConnect(session);
  });
  await pending;
  assert.deepEqual(afterConnect, [
    { session_id: "session-1", type: "session_title_update", title: "probe-e2e" },
  ]);

  const live = captureUpdates(() =>
    handleSdkMessage(session, { ...titleChanged, title: "renamed" } as unknown as SDKMessage),
  );
  assert.deepEqual(live, [
    { session_id: "session-1", type: "session_title_update", title: "renamed" },
  ]);

  // /clear replaces the session id and the child keeps the title.
  session.sessionId = "session-2";
  const afterReplace = captureUpdates(() => {
    pending = emitSessionLabelAfterConnect(session);
  });
  await pending;
  assert.deepEqual(afterReplace, [
    { session_id: "session-2", type: "session_title_update", title: "renamed" },
  ]);
});

test("the stored colour is the last agent-color entry of the session transcript", async () => {
  const configDir = mkdtempSync(path.join(os.tmpdir(), "label-config-"));
  try {
    const cwd = "/work/my.project";
    const projectDir = path.join(configDir, "projects", "-work-my-project");
    mkdirSync(projectDir, { recursive: true });
    const write = (id: string, lines: string[]) =>
      writeFileSync(path.join(projectDir, `${id}.jsonl`), `${lines.join("\n")}\n`);

    // The newest entry sits before 200 KB of later transcript, across chunks.
    const filler = JSON.stringify({ type: "user", text: "é".repeat(1000) });
    write("colored", [
      transcriptEntry("blue"),
      transcriptEntry("pink"),
      ...Array.from({ length: 100 }, () => filler),
    ]);
    write("reset", [transcriptEntry("blue"), transcriptEntry("default")]);
    write("plain", [filler]);

    assert.equal(await readStoredSessionColor(configDir, cwd, "colored"), "pink");
    assert.equal(await readStoredSessionColor(configDir, cwd, "reset"), undefined);
    assert.equal(await readStoredSessionColor(configDir, cwd, "plain"), undefined);
    assert.equal(await readStoredSessionColor(configDir, cwd, "missing"), undefined);
  } finally {
    rmSync(configDir, { recursive: true, force: true });
  }
});

test("a long cwd finds its transcript in the hashed project directory", async () => {
  const configDir = mkdtempSync(path.join(os.tmpdir(), "label-config-"));
  try {
    const cwd = `/${"deep/".repeat(60)}project`;
    const sanitized = cwd.replace(/[^a-zA-Z0-9]/g, "-");
    const projectDir = path.join(configDir, "projects", `${sanitized.slice(0, 200)}-1x2y3z`);
    mkdirSync(projectDir, { recursive: true });
    writeFileSync(path.join(projectDir, "long.jsonl"), `${transcriptEntry("cyan")}\n`);

    assert.equal(await readStoredSessionColor(configDir, cwd, "long"), "cyan");
  } finally {
    rmSync(configDir, { recursive: true, force: true });
  }
});

test("connecting a session with a stored colour sends it to the app", async () => {
  const configDir = mkdtempSync(path.join(os.tmpdir(), "label-config-"));
  const previous = process.env.CLAUDE_CONFIG_DIR;
  process.env.CLAUDE_CONFIG_DIR = configDir;
  try {
    const projectDir = path.join(configDir, "projects", "-resumed-cwd");
    mkdirSync(projectDir, { recursive: true });
    writeFileSync(path.join(projectDir, "session-1.jsonl"), `${transcriptEntry("green")}\n`);
    const session = minimalSession(true, "/resumed/cwd");

    const writes: string[] = [];
    const restore = replaceProtocolEventWriter((line) => {
      writes.push(line);
    });
    try {
      await emitSessionLabelAfterConnect(session);
    } finally {
      restore();
    }
    assert.deepEqual(
      writes.map((line) => (JSON.parse(line) as { update?: unknown }).update),
      [{ type: "session_color_update", color: "green" }],
    );
  } finally {
    if (previous === undefined) {
      delete process.env.CLAUDE_CONFIG_DIR;
    } else {
      process.env.CLAUDE_CONFIG_DIR = previous;
    }
    rmSync(configDir, { recursive: true, force: true });
  }
});
