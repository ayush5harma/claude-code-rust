import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import type { SDKMessage } from "@anthropic-ai/claude-agent-sdk";
import { emitConnectEvent, replaceProtocolEventWriter } from "./events.js";
import { bridgeLogger } from "./logger.js";
import { handleSdkMessage } from "./message_handlers.js";
import type { SessionState } from "./session_lifecycle.js";

/** Poll until `done` holds, failing after `timeoutMs` instead of waiting on a fixed delay. */
async function waitFor(done: () => boolean, timeoutMs = 5_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!done()) {
    assert.ok(Date.now() < deadline, "timed out waiting for the event");
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

function makeSession(): SessionState {
  return {
    sessionId: "session-1",
    cwd: "C:/work",
    availableModels: [],
    mode: null,
    supportedModeIds: [],
    runtimeUnavailableModeIds: [],
    supportsBypassPermissionsMode: false,
    fastModeState: "off",
    connected: true,
    closing: false,
    connectEvent: "connected",
    toolCalls: new Map(),
    tasksById: new Map(),
    taskOrder: [],
    taskToolUseIds: new Map(),
    taskIdsByToolUseId: new Map(),
    pendingPermissions: new Map(),
    pendingQuestions: new Map(),
    pendingUserDialogs: new Map(),
    pendingElicitations: new Map(),
    informationalDedupKeys: new Set(),
    knownConnectedMcpServers: new Set(),
    mcpStatusRevalidatedAt: new Map(),
    mcpAuthMonitors: new Map(),
    hiddenToolUseIds: new Set(),
    authHintSent: false,
  } as unknown as SessionState;
}

/** The text of every `agent_message_chunk` the bridge wrote while `run` ran. */
function replyChunks(run: () => void): Array<Record<string, unknown>> {
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
    .map((event) => event.update as Record<string, unknown> | undefined)
    .filter((update): update is Record<string, unknown> => update?.type === "agent_message_chunk");
}

// Measured on Claude Code 2.1.288, the version the pinned SDK bundles (2.1.296
// behaves the same): a local command (/rename, /color, /usage, /context)
// replies with one complete top-level assistant message and no stream events,
// and no system/local_command_output message, so its text exists only in that
// message.
test("a complete top-level reply whose response never streamed is shown once", () => {
  const session = makeSession();
  const reply = (uuid: string, responseId: string, text: string) => ({
    type: "assistant",
    uuid,
    session_id: "session-1",
    parent_tool_use_id: null,
    message: {
      id: responseId,
      role: "assistant",
      stop_reason: "end_turn",
      content: [{ type: "text", text }],
    },
  });

  const chunks = replyChunks(() => {
    for (const message of [
      reply("rename-reply", "a4cc84fe-7eb1-48bf-8fcd-0a11a7d5e251", "Session renamed to: probe"),
      reply("color-reply", "5142ce8e-bb1c-4d99-a882-5752dd24280b", "Session color set to: blue"),
    ]) {
      handleSdkMessage(session, message as unknown as SDKMessage);
    }
  });

  assert.deepEqual(chunks, [
    {
      type: "agent_message_chunk",
      content: { type: "text", text: "Session renamed to: probe" },
      source_message_uuid: "rename-reply",
    },
    {
      type: "agent_message_chunk",
      content: { type: "text", text: "Session color set to: blue" },
      source_message_uuid: "color-reply",
    },
  ]);
});

test("a streamed, subagent, empty, error or uncorrelated reply is never shown again", () => {
  const session = makeSession();
  const emit = (message: Record<string, unknown>) =>
    handleSdkMessage(session, {
      session_id: "session-1",
      parent_tool_use_id: null,
      ...message,
    } as unknown as SDKMessage);
  const stream = (event: Record<string, unknown>) => emit({ type: "stream_event", uuid: "frame", event });
  const complete = (uuid: string, message: Record<string, unknown>, extra = {}) =>
    emit({ type: "assistant", uuid, message: { role: "assistant", ...message }, ...extra });

  const texts = replyChunks(() => {
    stream({ type: "message_start", message: { id: "msg_streamed" } });
    stream({ type: "content_block_start", index: 0, content_block: { type: "text", text: "" } });
    stream({ type: "content_block_delta", index: 0, delta: { type: "text_delta", text: "already streamed" } });
    // The per-block message arrives inside the response; a terminal one may
    // follow its stop. Neither is the reply's first appearance.
    complete("streamed-block", { id: "msg_streamed", stop_reason: null, content: [{ type: "text", text: "already streamed" }] });
    stream({ type: "content_block_stop", index: 0 });
    stream({ type: "message_stop" });
    complete("streamed-final", { id: "msg_streamed", stop_reason: "end_turn", content: [{ type: "text", text: "already streamed" }] });
    complete("subagent", { id: "msg_subagent", content: [{ type: "text", text: "subagent text" }] }, { parent_tool_use_id: "tool-1" });
    complete("empty", { id: "local-empty", content: [{ type: "text", text: "   " }] });
    // A synthetic API error is reported through the turn result.
    complete("synthetic-error", { id: "local-error", content: [{ type: "text", text: "API Error: rate limited" }] }, { error: "rate_limit" });
    // Without a response ID nothing proves the text was not streamed.
    complete("no-response-id", { content: [{ type: "text", text: "uncorrelated" }] });
  }).map((chunk) => (chunk.content as Record<string, unknown>).text);

  assert.deepEqual(texts, ["already streamed"]);
});

test("a non-streaming fallback after a partial stream is shown in full", () => {
  const session = makeSession();
  const emit = (message: Record<string, unknown>) =>
    handleSdkMessage(session, {
      session_id: "session-1",
      parent_tool_use_id: null,
      ...message,
    } as unknown as SDKMessage);
  const stream = (event: Record<string, unknown>) => emit({ type: "stream_event", uuid: "frame", event });

  const texts = replyChunks(() => {
    // The stream breaks off after a partial delta; the deltas already showed it.
    stream({ type: "message_start", message: { id: "msg_partial" } });
    stream({ type: "content_block_start", index: 0, content_block: { type: "text", text: "" } });
    stream({ type: "content_block_delta", index: 0, delta: { type: "text_delta", text: "partial" } });
    // The retried request answers without streaming, under a new response ID.
    emit({
      type: "assistant",
      uuid: "fallback",
      message: { id: "msg_fallback", role: "assistant", stop_reason: "end_turn", content: [{ type: "text", text: "the full reply" }] },
    });
  }).map((chunk) => (chunk.content as Record<string, unknown>).text);

  assert.deepEqual(texts, ["partial", "the full reply"]);
});

test("SDK context usage Markdown is shown even without a response ID", () => {
  const session = makeSession();
  const texts = replyChunks(() => {
    handleSdkMessage(session, {
      type: "assistant",
      uuid: "context-reply",
      session_id: "session-1",
      parent_tool_use_id: null,
      context_usage: { categories: [] },
      message: { role: "assistant", content: [{ type: "text", text: "## Context Usage" }] },
    } as unknown as SDKMessage);
  }).map((chunk) => (chunk.content as Record<string, unknown>).text);

  assert.deepEqual(texts, ["## Context Usage"]);
});

/** Every `system_notice_update` the bridge wrote while `run` ran. */
function noticesWritten(run: () => void): Array<{ severity: unknown; message: unknown }> {
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
    .map((line) => (JSON.parse(line) as Record<string, unknown>).update as Record<string, unknown> | undefined)
    .filter((update): update is Record<string, unknown> => update?.type === "system_notice_update")
    .map((update) => ({ severity: update.severity, message: update.message }));
}

function hookResponse(
  hookEvent: string,
  fields: Record<string, unknown>,
  hookId = "hook-1",
): SDKMessage {
  return {
    type: "system",
    subtype: "hook_response",
    hook_id: hookId,
    hook_name: `${hookEvent}:Bash`,
    hook_event: hookEvent,
    output: "",
    stdout: "",
    stderr: "",
    session_id: "session-1",
    uuid: `uuid-${hookId}`,
    ...fields,
  } as unknown as SDKMessage;
}

// Measured on SDK 0.3.288 with a PreToolUse command hook that exits 1: the
// tool still runs and nothing else reports the failure.
test("a failed hook with no other feedback is one warning, once", () => {
  const session = makeSession();
  const failed = hookResponse("PreToolUse", {
    outcome: "error",
    exit_code: 1,
    stderr: "lint gate: forbidden pattern found\n",
  });
  const notices = noticesWritten(() => {
    handleSdkMessage(session, failed);
    handleSdkMessage(session, failed);
  });

  assert.deepEqual(notices, [
    {
      severity: "warning",
      message: "Hook PreToolUse:Bash failed (exit 1): lint gate: forbidden pattern found",
    },
  ]);
});

// Exit 2 is a block decision on every event, and the SDK still reports it as
// outcome "error": a Stop or PostToolUse gate must not read "failed" each turn.
test("a hook that exits 2 is a block decision on any event, never a failure notice", () => {
  const session = makeSession();
  const events = ["PreToolUse", "UserPromptSubmit", "Stop", "PostToolUse"];
  const logged: Array<{ level: string; eventName: string }> = [];
  const originals = { info: bridgeLogger.info, warn: bridgeLogger.warn, error: bridgeLogger.error };
  for (const level of ["info", "warn", "error"] as const) {
    bridgeLogger[level] = (event) => {
      logged.push({ level, eventName: event.eventName });
    };
  }
  let notices: Array<{ severity: unknown; message: unknown }>;
  try {
    notices = noticesWritten(() => {
      events.forEach((event, index) => {
        handleSdkMessage(
          session,
          hookResponse(event, { outcome: "error", exit_code: 2, stderr: "gate says no" }, `block-${index}`),
        );
      });
    });
  } finally {
    Object.assign(bridgeLogger, originals);
  }

  assert.deepEqual(notices, []);
  // Info stays out of the default log, which keeps only warnings and errors.
  assert.deepEqual(
    logged,
    events.map(() => ({ level: "info", eventName: "sdk_hook_blocked" })),
  );
});

test("routine hook activity and a cancelled hook never become notices", () => {
  const session = makeSession();
  const notices = noticesWritten(() => {
    handleSdkMessage(session, {
      type: "system",
      subtype: "hook_started",
      hook_id: "hook-1",
      hook_name: "PreToolUse:Bash",
      hook_event: "PreToolUse",
      session_id: "session-1",
    } as unknown as SDKMessage);
    handleSdkMessage(session, hookResponse("PreToolUse", { outcome: "success", exit_code: 0, output: "ok" }, "ok"));
    handleSdkMessage(session, hookResponse("Stop", { outcome: "cancelled" }, "cancelled"));
  });

  assert.deepEqual(notices, []);
});

test("a hook failure notice carries a bounded, redacted excerpt", () => {
  const session = makeSession();
  const notices = noticesWritten(() => {
    handleSdkMessage(
      session,
      hookResponse("PostToolUse", {
        outcome: "error",
        exit_code: 1,
        stderr: `deploy failed api_key=sk-ant-verysecret ${"x".repeat(1_000)}`,
      }),
    );
  });

  const message = String(notices[0]?.message);
  assert.ok(!message.includes("verysecret"));
  assert.ok(message.includes("api_key=[redacted]"));
  assert.ok(message.length < 300, `notice was ${message.length} characters`);
});

// SessionStart hooks finish before the connect event, and the app ignores
// updates for a session it has not been told about.
test("a hook failure before the session connects is held, not written", () => {
  const session = makeSession();
  session.connected = false;
  const notices = noticesWritten(() => {
    handleSdkMessage(
      session,
      hookResponse("SessionStart", { outcome: "error", exit_code: 1, stderr: "missing env" }),
    );
  });

  assert.deepEqual(notices, []);
  assert.deepEqual(session.pendingHookNotices, ["Hook SessionStart:Bash failed (exit 1): missing env"]);
});

// emitConnectEvent starts an async session-title read that outlives the call. It
// is pointed at a scratch config directory that holds a titled session, so the
// test waits for that title update, the read's own completion, before the writer
// and the environment are restored: it never reads the real ~/.claude, and it
// never guesses a delay.
test("held hook failures are written after the connect event, then cleared", async () => {
  const configDir = realpathSync(mkdtempSync(join(tmpdir(), "claude-rs-config-")));
  const workDir = join(configDir, "work");
  mkdirSync(workDir);
  const sessionId = "2f6c1f0e-1111-4111-8111-aaaaaaaaaaaa";
  const projectDir = join(configDir, "projects", workDir.replace(/[^a-zA-Z0-9]/g, "-"));
  mkdirSync(projectDir, { recursive: true });
  writeFileSync(
    join(projectDir, `${sessionId}.jsonl`),
    [
      { type: "user", uuid: "11111111-1111-4111-8111-111111111111", sessionId, timestamp: "2026-10-01T00:00:00Z", cwd: workDir, message: { role: "user", content: "hi" } },
      { type: "custom-title", customTitle: "Fixture title", sessionId },
    ]
      .map((entry) => JSON.stringify(entry))
      .join("\n"),
  );
  const session = makeSession();
  session.sessionId = sessionId;
  session.cwd = workDir;
  session.connected = false;
  session.pendingHookNotices = ["Hook SessionStart:Bash failed (exit 1): missing env"];
  const previousConfigDir = process.env.CLAUDE_CONFIG_DIR;
  process.env.CLAUDE_CONFIG_DIR = configDir;
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter((line) => {
    writes.push(line);
  });
  try {
    emitConnectEvent(session);
    await waitFor(() => writes.some((line) => line.includes("session_title_update")));
  } finally {
    restore();
    if (previousConfigDir === undefined) {
      delete process.env.CLAUDE_CONFIG_DIR;
    } else {
      process.env.CLAUDE_CONFIG_DIR = previousConfigDir;
    }
    rmSync(configDir, { recursive: true, force: true });
  }

  const events = writes.map((line) => JSON.parse(line) as Record<string, unknown>);
  const connected = events.findIndex((event) => event.event === "connected");
  const notice = events.findIndex(
    (event) => (event.update as Record<string, unknown> | undefined)?.type === "system_notice_update",
  );
  assert.ok(connected >= 0 && notice > connected, JSON.stringify(events.map((event) => event.event)));
  assert.equal(session.pendingHookNotices, undefined);
});
