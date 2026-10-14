import assert from "node:assert/strict";
import test from "node:test";
import type { SDKMessage } from "@anthropic-ai/claude-agent-sdk";
import { replaceProtocolEventWriter } from "./events.js";
import { handleSdkMessage } from "./message_handlers.js";
import { rememberColorPrompt } from "./session_color.js";
import { type SessionState, updateSessionId } from "./session_lifecycle.js";

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
    childModelsByToolUseId: new Map(),
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

function colorUpdates(run: () => void): string[] {
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter((line) => writes.push(line));
  try {
    run();
  } finally {
    restore();
  }
  return writes.map((line) => JSON.parse(line) as Record<string, unknown>)
    .map((event) => event.update as Record<string, unknown> | undefined)
    .filter((update) => update?.type === "session_color_update")
    .map((update) => String(update?.color));
}

test("only a correlated successful local /color result changes the session accent", () => {
  const session = makeSession();
  const prompt = (uuid: string, text: string) => ({
    command: "prompt" as const, session_id: session.sessionId, message_uuid: uuid,
    chunks: [{ kind: "text", value: text }],
  });
  const result = (uuid: string | undefined, localCommand: string | undefined, subtype = "success") => ({
    type: "result", subtype, session_id: session.sessionId,
    user_message_uuid: uuid, local_command: localCommand, permission_denials: [], errors: [],
  });
  const colors = colorUpdates(() => {
    rememberColorPrompt(session, prompt("prose", "Please say /color red"));
    handleSdkMessage(session, result("prose", "/color") as unknown as SDKMessage);
    rememberColorPrompt(session, prompt("missing-id", "/color red"));
    handleSdkMessage(session, result(undefined, "/color") as unknown as SDKMessage);
    rememberColorPrompt(session, prompt("failed", "/color red"));
    handleSdkMessage(session, result("failed", "/color", "error_during_execution") as unknown as SDKMessage);
    rememberColorPrompt(session, prompt("other-command", "/color green"));
    handleSdkMessage(session, result("other-command", "/model") as unknown as SDKMessage);
    rememberColorPrompt(session, prompt("confirmed", "/color blue"));
    handleSdkMessage(session, result("confirmed", "/color") as unknown as SDKMessage);
  });
  assert.deepEqual(colors, ["blue"]);
});

test("late, mismatched and replaced-session color results cannot overwrite a newer accent", () => {
  const session = makeSession();
  const prompt = (uuid: string, color: string) => rememberColorPrompt(session, {
    command: "prompt", session_id: session.sessionId, message_uuid: uuid,
    chunks: [{ kind: "text", value: `/color ${color}` }],
  });
  const result = (uuid: string, command = "/color", sessionId = session.sessionId) => handleSdkMessage(session, {
    type: "result", subtype: "success", session_id: sessionId,
    user_message_uuid: uuid, local_command: command, permission_denials: [], errors: [],
  } as unknown as SDKMessage);
  const colors = colorUpdates(() => {
    prompt("red-1", "red");
    prompt("blue-2", "blue");
    result("blue-2");
    result("red-1");
    prompt("green-3", "green");
    result("green-3", "/color red");
    prompt("purple-4", "purple");
    session.closing = true;
    result("purple-4");
    session.closing = false;
    prompt("cyan-5", "cyan");
    result("cyan-5", "/color", "");
    result("cyan-5", "/color", "session-other");
    result("cyan-5");
    prompt("pink-old", "pink");
    updateSessionId(session, "session-2");
    result("pink-old", "/color", "session-1");
    prompt("orange-new", "orange");
    result("orange-new");
  });
  assert.deepEqual(colors, ["blue", "cyan", "orange"]);
});

// Measured against the pinned SDK (0.3.288) with a live foreground subagent
// stopped through query.stopTask: it sends task_updated {status: "killed"}
// first, then task_notification {status: "stopped"}, then the Agent tool
// result as an interrupted error. The task row must end as the stop the SDK
// confirmed, not as the "killed" patch that preceded it.
test("a stopped subagent keeps the SDK's stopped status after the killed patch", () => {
  const session = makeSession();
  const send = (subtype: string, fields: Record<string, unknown>) =>
    handleSdkMessage(session, {
      type: "system",
      subtype,
      task_id: "agent-1",
      uuid: crypto.randomUUID(),
      session_id: session.sessionId,
      ...fields,
    } as unknown as SDKMessage);
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter((line) => {
    writes.push(line);
  });
  try {
    send("task_started", { tool_use_id: "tool-1", task_type: "local_agent", description: "Run sleep" });
    send("task_progress", { tool_use_id: "tool-1", description: "Run sleep", usage: { total_tokens: 1, tool_uses: 0, duration_ms: 1 } });
    send("task_updated", { patch: { status: "killed", end_time: 1791875201893 } });
    send("task_notification", { tool_use_id: "tool-1", status: "stopped", summary: "Run sleep", output_file: "" });
  } finally {
    restore();
  }
  // What the Rust side renders from is the last task_state_update for the task.
  const rows = writes
    .map((line) => (JSON.parse(line) as { update?: { type?: string; tasks?: Array<Record<string, unknown>> } }).update)
    .filter((update) => update?.type === "task_state_update")
    .flatMap((update) => update?.tasks ?? [])
    .filter((task) => task.task_id === "agent-1");
  const last = rows.at(-1);
  assert.ok(last, "the task was emitted");
  assert.equal(last.status, "completed");
  assert.equal((last.metadata as Record<string, unknown>).terminal_status, "stopped");
});

function taskUpdates(run: () => void): Array<{ tasks: Array<{ task_id: string; metadata: Record<string, unknown> }> }> {
  const lines: string[] = [];
  const restore = replaceProtocolEventWriter(line => lines.push(line));
  try { run(); } finally { restore(); }
  return lines.map(line => JSON.parse(line).update).filter(update => update?.type === "task_state_update");
}

function childAssistant(parent: string, model: string): SDKMessage {
  return {
    type: "assistant", session_id: "session-1", uuid: "00000000-0000-0000-0000-000000000001", parent_tool_use_id: parent,
    message: {
      container: null, context_management: null, diagnostics: null, stop_details: null,
      id: "child-response", type: "message", role: "assistant", model,
      content: [{ type: "text", text: "Inspecting the source", citations: null }], stop_reason: "end_turn", stop_sequence: null,
      usage: { input_tokens: 10, output_tokens: 5, cache_creation_input_tokens: 0, cache_read_input_tokens: 0,
        cache_creation: null, inference_geo: null, iterations: null, output_tokens_details: null, server_tool_use: null, service_tier: null, speed: null },
    },
  };
}

function startTask(session: SessionState): void {
  handleSdkMessage(session, {
    type: "system", subtype: "task_started", session_id: "session-1", uuid: "00000000-0000-0000-0000-000000000002",
    task_id: "agent-1", tool_use_id: "tool-1", task_type: "local_agent", description: "Inspect the source",
  });
}

test("SDK progress and final notification carry telemetry through task_state_update", () => {
  const session = makeSession();
  const updates = taskUpdates(() => {
    startTask(session);
    handleSdkMessage(session, {
      type: "system", subtype: "task_progress", session_id: "session-1", uuid: "00000000-0000-0000-0000-000000000003",
      task_id: "agent-1", tool_use_id: "tool-1", description: "Inspect the source",
      summary: "Reading the renderer", last_tool_name: "Read",
      usage: { total_tokens: 1200, tool_uses: 3, duration_ms: 2000 },
    });
    handleSdkMessage(session, {
      type: "system", subtype: "task_notification", session_id: "session-1", uuid: "00000000-0000-0000-0000-000000000004",
      task_id: "agent-1", tool_use_id: "tool-1", status: "completed", output_file: "", summary: "Inspected",
      usage: { total_tokens: 1500, tool_uses: 4, duration_ms: 3000 },
    });
  });
  assert.deepEqual(updates[1]?.tasks[0]?.metadata.usage, { total_tokens: 1200, tool_uses: 3, duration_ms: 2000 });
  assert.equal(updates[1]?.tasks[0]?.metadata.last_tool_name, "Read");
  assert.equal(updates[1]?.tasks[0]?.metadata.summary, "Reading the renderer");
  assert.deepEqual(updates.at(-1)?.tasks[0]?.metadata.usage, { total_tokens: 1500, tool_uses: 4, duration_ms: 3000 });
});

test("child model changes update the linked task once without affecting the session model", () => {
  const session = makeSession();
  taskUpdates(() => startTask(session));
  const updates = taskUpdates(() => {
    handleSdkMessage(session, childAssistant("tool-1", "claude-sonnet-5-5"));
    handleSdkMessage(session, childAssistant("tool-1", "claude-sonnet-5-5"));
    handleSdkMessage(session, childAssistant("tool-1", "claude-opus-5-5"));
    handleSdkMessage(session, childAssistant("unrelated-tool", "claude-haiku-4-5"));
  });
  assert.equal(updates.length, 2);
  assert.equal(updates[0]?.tasks[0]?.metadata.model, "claude-sonnet-5-5");
  assert.equal(updates[0]?.tasks[0]?.metadata.model_display, "Sonnet 5.5");
  assert.equal(updates[1]?.tasks[0]?.metadata.model_display, "Opus 5.5");
  assert.equal(session.currentModel, undefined);
});

test("a child model observed before linkage appears on the first emitted task", () => {
  const session = makeSession();
  const updates = taskUpdates(() => {
    handleSdkMessage(session, childAssistant("tool-1", "claude-sonnet-5-5"));
    handleSdkMessage(session, childAssistant("tool-1", "claude-opus-5-5"));
    startTask(session);
    handleSdkMessage(session, childAssistant("tool-1", "claude-opus-5-5"));
  });
  assert.equal(updates.length, 1);
  assert.equal(updates[0]?.tasks[0]?.metadata.model, "claude-opus-5-5");
  assert.equal(updates[0]?.tasks[0]?.metadata.model_display, "Opus 5.5");
});

test("a remembered child model is applied when an existing task gains its tool link", () => {
  const session = makeSession();
  const updates = taskUpdates(() => {
    handleSdkMessage(session, {
      type: "system", subtype: "task_started", session_id: "session-1",
      uuid: "00000000-0000-0000-0000-000000000002", task_id: "agent-1", description: "Inspect the source",
    });
    handleSdkMessage(session, childAssistant("tool-1", "claude-sonnet-5-5"));
    handleSdkMessage(session, {
      type: "system", subtype: "task_progress", session_id: "session-1",
      uuid: "00000000-0000-0000-0000-000000000003", task_id: "agent-1", tool_use_id: "tool-1",
      description: "Inspect the source", usage: { total_tokens: 100, tool_uses: 1, duration_ms: 10 },
    });
  });
  assert.equal(updates.at(-1)?.tasks[0]?.metadata.model_display, "Sonnet 5.5");
  assert.deepEqual(updates.at(-1)?.tasks[0]?.metadata.usage, { total_tokens: 100, tool_uses: 1, duration_ms: 10 });
});

test("final notifications without usage preserve the task's last progress metrics and model", () => {
  const session = makeSession();
  const updates = taskUpdates(() => {
    startTask(session);
    handleSdkMessage(session, childAssistant("tool-1", "claude-sonnet-5-5"));
    handleSdkMessage(session, {
      type: "system", subtype: "task_progress", session_id: "session-1",
      uuid: "00000000-0000-0000-0000-000000000003", task_id: "agent-1", tool_use_id: "tool-1",
      description: "Inspect the source", last_tool_name: "Read",
      usage: { total_tokens: 100, tool_uses: 1, duration_ms: 10 },
    });
    handleSdkMessage(session, {
      type: "system", subtype: "task_notification", session_id: "session-1",
      uuid: "00000000-0000-0000-0000-000000000004", task_id: "agent-1", tool_use_id: "tool-1",
      status: "completed", output_file: "", summary: "Finished",
    });
  });
  const metadata = updates.at(-1)?.tasks[0]?.metadata;
  assert.deepEqual(metadata?.usage, { total_tokens: 100, tool_uses: 1, duration_ms: 10 });
  assert.equal(metadata?.model_display, "Sonnet 5.5");
  assert.equal(metadata?.last_tool_name, "Read");
});

for (const completionFirst of [false, true]) {
  test(`background membership preserves child telemetry with completion ${completionFirst ? "before" : "after"} removal`, () => {
    const session = makeSession();
    const updates = taskUpdates(() => {
      const snapshot = (ids: string[]) => handleSdkMessage(session, {
        type: "system", subtype: "background_tasks_changed", session_id: "session-1",
        uuid: "00000000-0000-0000-0000-000000000005",
        tasks: ids.map(task_id => ({ task_id, task_type: "local_agent", description: "Inspect the source" })),
      });
      snapshot(["agent-1"]);
      startTask(session);
      handleSdkMessage(session, childAssistant("tool-1", "claude-sonnet-5-5"));
      handleSdkMessage(session, {
        type: "system", subtype: "task_progress", session_id: "session-1",
        uuid: "00000000-0000-0000-0000-000000000003", task_id: "agent-1", tool_use_id: "tool-1",
        description: "Inspect the source", last_tool_name: "Read", summary: "Reading the renderer",
        usage: { total_tokens: 100, tool_uses: 1, duration_ms: 10 },
      });
      snapshot(["agent-1", "agent-2"]);
      if (!completionFirst) snapshot(["agent-2"]);
      handleSdkMessage(session, {
        type: "system", subtype: "task_notification", session_id: "session-1",
        uuid: "00000000-0000-0000-0000-000000000004", task_id: "agent-1", tool_use_id: "tool-1",
        status: "completed", output_file: "", summary: "Finished",
      });
      if (completionFirst) snapshot(["agent-2"]);
    });
    const snapshots = updates.filter(update => (update as { source?: string }).source === "background_tasks");
    const live = snapshots[1]?.tasks.find(task => task.task_id === "agent-1");
    assert.equal(live?.metadata.model_display, "Sonnet 5.5");
    assert.deepEqual(live?.metadata.usage, { total_tokens: 100, tool_uses: 1, duration_ms: 10 });
    assert.equal(live?.metadata.last_tool_name, "Read");
    assert.equal(live?.metadata.summary, "Reading the renderer");
    assert.equal(live?.metadata.sdk_background_task, undefined);
    assert.ok(updates.every(update => !(update as { removed_task_ids?: string[] }).removed_task_ids?.includes("agent-1")));
    const final = updates.flatMap(update => update.tasks).filter(task => task.task_id === "agent-1").at(-1);
    assert.equal(final?.metadata.model_display, "Sonnet 5.5");
    assert.deepEqual(final?.metadata.usage, { total_tokens: 100, tool_uses: 1, duration_ms: 10 });
    assert.equal(final?.metadata.terminal_status, "completed");
    assert.equal(session.tasksById.get("agent-1")?.status, "completed");
  });
}

for (const lifecycle of [
  { subtype: "task_started", description: "Background work" },
  { subtype: "task_progress", description: "Background work", usage: { total_tokens: 0, tool_uses: 0, duration_ms: 1 } },
  { subtype: "task_updated", patch: { status: "running" } },
  { subtype: "task_notification", status: "completed", output_file: "", summary: "Finished" },
] as const) {
  test(`${lifecycle.subtype} identifies lifecycle ownership without an earlier start or task type`, () => {
    const session = makeSession();
    const updates = taskUpdates(() => handleSdkMessage(session, {
      type: "system", ...lifecycle, task_id: "untyped-task", session_id: "session-1",
      uuid: "00000000-0000-0000-0000-000000000006",
    }));
    assert.equal(updates.at(-1)?.tasks[0]?.metadata.sdk_lifecycle_task, true);
  });
}
