import assert from "node:assert/strict";
import test from "node:test";
import type { SDKMessage } from "@anthropic-ai/claude-agent-sdk";
import { replaceProtocolEventWriter } from "./events.js";
import { handleSdkMessage } from "./message_handlers.js";
import type { SessionState } from "./session_lifecycle.js";

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

/** The last `task_state_update` row the bridge wrote for `taskId` while `run` ran. */
function lastTaskRow(taskId: string, run: () => void): Record<string, unknown> | undefined {
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
    .map((line) => (JSON.parse(line) as { update?: { type?: string; tasks?: Array<Record<string, unknown>> } }).update)
    .filter((update) => update?.type === "task_state_update")
    .flatMap((update) => update?.tasks ?? [])
    .filter((task) => task.task_id === taskId)
    .at(-1);
}

// A foreground Agent call can finish through its tool result before the task
// notification arrives. The finished tool call then rejects the notification,
// which must still settle the separate task row the footer and /agents count.
test("a subagent task row settles when its notification follows the finished tool call", () => {
  for (const outcome of ["completed", "failed", "stopped"]) {
    const session = makeSession();
    const emit = (message: Record<string, unknown>) =>
      handleSdkMessage(session, {
        session_id: "session-1",
        parent_tool_use_id: null,
        uuid: crypto.randomUUID(),
        ...message,
      } as unknown as SDKMessage);
    const system = (subtype: string, fields: Record<string, unknown>) =>
      emit({ type: "system", subtype, task_id: "agent-1", ...fields });

    const row = lastTaskRow("agent-1", () => {
      emit({ type: "assistant", message: { id: "response-1", role: "assistant", content: [{ type: "tool_use", id: "tool-1", name: "Agent", input: { description: "Sleep", prompt: "Sleep" } }] } });
      system("task_started", { tool_use_id: "tool-1", task_type: "local_agent", subagent_type: "general-purpose", description: "Sleep" });
      if (outcome === "stopped") system("task_updated", { patch: { status: "killed", end_time: 1 } });
      else emit({ type: "user", message: { role: "user", content: [{ type: "tool_result", tool_use_id: "tool-1", content: "ok" }] } });
      system("task_notification", { tool_use_id: "tool-1", status: outcome, summary: "Slept", output_file: "" });
      // A queued progress frame must not revive the settled row.
      system("task_progress", { tool_use_id: "tool-1", description: "Stale progress", usage: { total_tokens: 1, tool_uses: 1, duration_ms: 1 } });
    });

    assert.ok(row, `${outcome}: the task row was emitted`);
    assert.equal(row.status, "completed", `${outcome}: the row is settled`);
    const metadata = row.metadata as Record<string, unknown>;
    assert.equal(metadata.terminal_status, outcome, `${outcome}: the notification's outcome is kept`);
    assert.equal(metadata.summary, "Slept");
    assert.notEqual(row.description, "Stale progress", `${outcome}: stale progress does not rewrite the row`);
  }
});

// Measured on Claude Code 2.1.288: a backgrounded subagent is listed by
// background_tasks_changed, and the snapshot that drops it arrives right after
// its task_notification (or after its killed patch when stopped).
test("a backgrounded subagent keeps its lifecycle row after it leaves the background snapshot", () => {
  const session = makeSession();
  const emit = (message: Record<string, unknown>) =>
    handleSdkMessage(session, { session_id: "session-1", uuid: crypto.randomUUID(), ...message } as unknown as SDKMessage);
  const background = (tasks: Array<Record<string, unknown>>) => emit({ type: "system", subtype: "background_tasks_changed", tasks });
  const listed = [
    { task_id: "agent-1", task_type: "local_agent", description: "Background sleep" },
    { task_id: "shell-1", task_type: "local_bash", description: "Watch logs" },
  ];
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter((line) => {
    writes.push(line);
  });
  try {
    background(listed);
    emit({ type: "system", subtype: "task_started", task_id: "agent-1", task_type: "local_agent", subagent_type: "general-purpose", description: "Background sleep" });
    background(listed);
    emit({ type: "system", subtype: "task_notification", task_id: "agent-1", status: "completed", summary: "ok", output_file: "" });
    background([]);
  } finally {
    restore();
  }

  type Update = { source?: string; removed_task_ids?: string[]; tasks?: Array<Record<string, unknown>> };
  const updates = writes
    .map((line) => (JSON.parse(line) as { update?: Update & { type?: string } }).update)
    .filter((update): update is Update & { type: string } => update?.type === "task_state_update");
  const final = updates.at(-1);
  assert.equal(final?.source, "background_tasks");
  assert.deepEqual(final?.removed_task_ids, ["shell-1"], "a task with no reported outcome still leaves with the snapshot");
  const row = updates.flatMap((update) => update.tasks ?? []).filter((task) => task.task_id === "agent-1").at(-1);
  assert.equal(row?.status, "completed");
  assert.deepEqual(row?.metadata, {
    task_type: "local_agent",
    subagent_type: "general-purpose",
    sdk_lifecycle_task: true,
    is_backgrounded: true,
    output_file: "",
    summary: "ok",
    terminal_status: "completed",
  }, "the settled row keeps its lifecycle metadata and drops only the background marker");
  assert.equal(session.tasksById.get("agent-1")?.status, "completed");
  assert.equal(session.tasksById.has("shell-1"), false);
});

test("a subagent that leaves the background snapshot before its outcome stops counting as running, then takes its real outcome", () => {
  const session = makeSession();
  const emit = (message: Record<string, unknown>) =>
    handleSdkMessage(session, { session_id: "session-1", uuid: crypto.randomUUID(), ...message } as unknown as SDKMessage);
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter((line) => { writes.push(line); });
  try {
    emit({ type: "system", subtype: "task_started", task_id: "agent-1", task_type: "local_agent", description: "Background sleep" });
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [
      { task_id: "agent-1", task_type: "local_agent", description: "Background sleep" },
      { task_id: "snapshot-only", task_type: "local_agent", description: "No lifecycle start" },
    ] });
    assert.equal(session.tasksById.get("agent-1")?.status, "in_progress", "listed rows keep running");
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
    const unlisted = session.tasksById.get("agent-1");
    assert.equal(unlisted?.status, "completed", "a running row the snapshot no longer lists is not running");
    assert.deepEqual(unlisted?.metadata, { task_type: "local_agent", sdk_lifecycle_task: true, is_backgrounded: true, terminal_status: "interrupted" }, "no outcome is invented");
    assert.equal(session.tasksById.has("snapshot-only"), false, "a background-only row leaves with the snapshot");
    emit({ type: "system", subtype: "task_notification", task_id: "agent-1", status: "completed", summary: "ok" });
  } finally {
    restore();
  }

  const updates = writes.map(line => (JSON.parse(line) as { update?: { type?: string; removed_task_ids?: string[]; tasks?: Array<Record<string, unknown>> } }).update)
    .filter(update => update?.type === "task_state_update");
  assert.ok(!updates.some(update => update?.removed_task_ids?.includes("agent-1")));
  const final = updates.flatMap(update => update?.tasks ?? []).filter(task => task.task_id === "agent-1").at(-1);
  assert.equal(final?.status, "completed");
  assert.deepEqual(final?.metadata, { task_type: "local_agent", sdk_lifecycle_task: true, is_backgrounded: true, summary: "ok", terminal_status: "completed" });
});

test("a relisted interrupted subagent runs again and its real outcome still wins", () => {
  const session = makeSession();
  const emit = (message: Record<string, unknown>) =>
    handleSdkMessage(session, { session_id: "session-1", uuid: crypto.randomUUID(), ...message } as unknown as SDKMessage);
  const restore = replaceProtocolEventWriter(() => {});
  try {
    emit({ type: "system", subtype: "task_started", task_id: "agent-1", task_type: "local_agent", description: "Background sleep" });
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [{ task_id: "agent-1", task_type: "local_agent" }] });
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
    assert.equal(session.tasksById.get("agent-1")?.status, "completed", "dropped: settled as interrupted");
    emit({ type: "system", subtype: "task_progress", task_id: "agent-1", summary: "still going" });
    assert.equal(session.tasksById.get("agent-1")?.status, "in_progress", "progress revives an interrupted row");
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
    assert.equal(session.tasksById.get("agent-1")?.status, "completed", "dropped again");
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [{ task_id: "agent-1", task_type: "local_agent" }] });
    const relisted = session.tasksById.get("agent-1");
    assert.equal(relisted?.status, "in_progress", "a relisted row is live");
    assert.equal((relisted?.metadata as Record<string, unknown>).terminal_status, undefined, "the inferred status is cleared");
    emit({ type: "system", subtype: "task_notification", task_id: "agent-1", status: "failed", summary: "boom" });
    const final = session.tasksById.get("agent-1");
    assert.equal(final?.status, "completed", "the wire has no failed status");
    assert.equal((final?.metadata as Record<string, unknown>).terminal_status, "failed");
  } finally {
    restore();
  }
});

test("a subagent with a recorded outcome is not revived by a relisting or later progress", () => {
  const session = makeSession();
  const emit = (message: Record<string, unknown>) =>
    handleSdkMessage(session, { session_id: "session-1", uuid: crypto.randomUUID(), ...message } as unknown as SDKMessage);
  const restore = replaceProtocolEventWriter(() => {});
  try {
    emit({ type: "system", subtype: "task_started", task_id: "agent-1", task_type: "local_agent", description: "Background sleep" });
    emit({ type: "system", subtype: "task_notification", task_id: "agent-1", status: "completed", summary: "ok" });
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [{ task_id: "agent-1", task_type: "local_agent" }] });
    emit({ type: "system", subtype: "task_progress", task_id: "agent-1", summary: "late" });
    const row = session.tasksById.get("agent-1");
    assert.equal(row?.status, "completed");
    assert.equal((row?.metadata as Record<string, unknown>).terminal_status, "completed");
  } finally {
    restore();
  }
});

test("a foreground subagent the snapshot never listed keeps running when a snapshot arrives", () => {
  const session = makeSession();
  const emit = (message: Record<string, unknown>) =>
    handleSdkMessage(session, { session_id: "session-1", uuid: crypto.randomUUID(), ...message } as unknown as SDKMessage);
  const restore = replaceProtocolEventWriter(() => {});
  try {
    emit({ type: "system", subtype: "task_started", task_id: "agent-fg", task_type: "local_agent", description: "Foreground" });
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
  } finally {
    restore();
  }
  assert.equal(session.tasksById.get("agent-fg")?.status, "in_progress");
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
