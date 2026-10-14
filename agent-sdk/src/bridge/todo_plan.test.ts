import assert from "node:assert/strict";
import test from "node:test";
import { replaceProtocolEventWriter } from "./events.js";
import type { SessionState } from "./session_lifecycle.js";
import { emitToolResultUpdate } from "./tool_calls.js";
import { createToolCall } from "./tooling.js";

function session(): SessionState {
  return {
    sessionId: "session-1", connected: true, closing: false,
    toolCalls: new Map(), tasksById: new Map(), taskOrder: [], taskToolUseIds: new Map(),
    taskIdsByToolUseId: new Map(), childModelsByToolUseId: new Map(), informationalDedupKeys: new Set(),
  } as unknown as SessionState;
}

function updates(run: () => void): Array<Record<string, unknown>> {
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter((line) => writes.push(line));
  try { run(); } finally { restore(); }
  return writes.map((line) => JSON.parse(line) as Record<string, unknown>)
    .map((event) => event.update as Record<string, unknown>)
    .filter((update) => update?.type === "todo_plan_update");
}

test("a confirmed main-agent TodoWrite result supplies a bounded plan snapshot", () => {
  const current = session();
  current.toolCalls.set("todo-1", createToolCall("todo-1", "TodoWrite", {
    todos: [
      { content: "Check parser", status: "completed" },
      { content: "Run tests", activeForm: "Running tests", status: "in_progress" },
    ],
  }));
  const emitted = updates(() => emitToolResultUpdate(current, "todo-1", false, "ok"));
  assert.deepEqual(emitted, [{
    type: "todo_plan_update",
    items: [
      { content: "Check parser", status: "completed" },
      { content: "Run tests", active_form: "Running tests", status: "in_progress" },
    ],
  }]);
});

test("failed or child TodoWrite results cannot replace the main plan", () => {
  const current = session();
  current.toolCalls.set("failed", createToolCall("failed", "TodoWrite", {
    todos: [{ content: "Should stay hidden", status: "pending" }],
  }));
  current.toolCalls.set("child", createToolCall("child", "TodoWrite", {
    todos: [{ content: "Child plan", status: "pending" }],
  }, "agent-parent"));
  const emitted = updates(() => {
    emitToolResultUpdate(current, "failed", true, "failed");
    emitToolResultUpdate(current, "child", false, "ok");
  });
  assert.deepEqual(emitted, []);
});
