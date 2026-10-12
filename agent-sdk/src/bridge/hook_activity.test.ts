import assert from "node:assert/strict";
import test from "node:test";
import type { SDKMessage } from "@anthropic-ai/claude-agent-sdk";
import { replaceProtocolEventWriter } from "./events.js";
import {
  buildHookActivityUpdate,
  flushPendingHookActivity,
  HOOK_OUTPUT_EXCERPT_LIMIT,
} from "./hook_activity.js";
import { handleSdkMessage } from "./message_handlers.js";
import type { SessionState } from "./session_lifecycle.js";

// Shapes follow SDKHookStartedMessage, SDKHookProgressMessage and
// SDKHookResponseMessage in sdk.d.ts (SDK 0.3.288).
const started = {
  type: "system",
  subtype: "hook_started",
  hook_id: "hook-1",
  hook_name: "SessionStart:startup",
  hook_event: "SessionStart",
  uuid: "u1",
  session_id: "session-1",
};
const progress = {
  type: "system",
  subtype: "hook_progress",
  hook_id: "hook-1",
  hook_name: "SessionStart:startup",
  hook_event: "SessionStart",
  stdout: "warming cache\n",
  stderr: "",
  output: "warming cache\n",
  uuid: "u2",
  session_id: "session-1",
};
const finished = {
  type: "system",
  subtype: "hook_response",
  hook_id: "hook-1",
  hook_name: "SessionStart:startup",
  hook_event: "SessionStart",
  output: "done\n",
  stdout: "done\n",
  stderr: "",
  exit_code: 0,
  outcome: "success",
  uuid: "u3",
  session_id: "session-1",
};

test("the three SDK hook messages normalise into one hook_activity update", () => {
  assert.deepEqual(buildHookActivityUpdate(started, 1_000), {
    type: "hook_activity",
    hook_id: "hook-1",
    hook_name: "SessionStart:startup",
    hook_event: "SessionStart",
    phase: "started",
    observed_at_ms: 1_000,
  });
  assert.deepEqual(buildHookActivityUpdate(progress, 1_500), {
    type: "hook_activity",
    hook_id: "hook-1",
    hook_name: "SessionStart:startup",
    hook_event: "SessionStart",
    phase: "progress",
    output: "warming cache",
    observed_at_ms: 1_500,
  });
  assert.deepEqual(buildHookActivityUpdate(finished, 2_700.9), {
    type: "hook_activity",
    hook_id: "hook-1",
    hook_name: "SessionStart:startup",
    hook_event: "SessionStart",
    phase: "finished",
    outcome: "success",
    exit_code: 0,
    output: "done",
    observed_at_ms: 2_700,
  });
});

test("a failed hook reports its stderr, and a bounded excerpt of it", () => {
  const long = "x".repeat(HOOK_OUTPUT_EXCERPT_LIMIT * 2);
  const update = buildHookActivityUpdate(
    { ...finished, hook_event: "PreToolUse", outcome: "error", exit_code: 2, stdout: "ignored", output: "combined", stderr: long },
    5,
  );
  assert.equal(update?.outcome, "error");
  assert.equal(update?.exit_code, 2);
  assert.equal(update?.output, `${"x".repeat(HOOK_OUTPUT_EXCERPT_LIMIT)}…`);

  const stdoutOnly = buildHookActivityUpdate({ ...finished, outcome: "error", exit_code: 1, output: "", stderr: "", stdout: "only stdout" }, 5);
  assert.equal(stdoutOnly?.output, "only stdout");
});

test("a message without the identity the SDK declares, or an undeclared outcome, adds nothing", () => {
  assert.equal(buildHookActivityUpdate({ ...started, hook_id: "" }, 1), undefined);
  assert.equal(buildHookActivityUpdate({ ...started, hook_event: undefined }, 1), undefined);
  assert.equal(buildHookActivityUpdate({ type: "system", subtype: "init" }, 1), undefined);
  const odd = buildHookActivityUpdate({ ...finished, outcome: "mystery", exit_code: 1.5, output: "" , stdout: "", stderr: ""}, 1);
  assert.equal(odd?.outcome, undefined);
  assert.equal(odd?.exit_code, undefined);
  assert.equal(odd?.output, undefined);
});

function makeSession(connected: boolean): SessionState {
  return { sessionId: "session-1", cwd: "/work", connected, closing: false } as unknown as SessionState;
}

function hookUpdates(run: () => void): Array<Record<string, unknown>> {
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
    .map((event) => event.update as Record<string, unknown>)
    .filter((update) => update.type === "hook_activity");
}

test("hook messages before the connect event are held and sent in order after it", () => {
  const session = makeSession(false);
  const beforeConnect = hookUpdates(() => {
    for (const message of [started, finished]) handleSdkMessage(session, message as unknown as SDKMessage);
  });
  assert.deepEqual(beforeConnect, []);

  session.connected = true;
  const afterFlush = hookUpdates(() => flushPendingHookActivity(session));
  assert.deepEqual(afterFlush.map((update) => update.phase), ["started", "finished"]);
  assert.deepEqual(hookUpdates(() => flushPendingHookActivity(session)), []);
});

test("hook messages after the connect event are sent at once, and a hook update is a complete message", () => {
  const session = makeSession(true);
  const updates = hookUpdates(() => {
    for (const message of [started, progress, finished]) handleSdkMessage(session, message as unknown as SDKMessage);
  });
  assert.deepEqual(updates.map((update) => update.phase), ["started", "progress", "finished"]);
});
