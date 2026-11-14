import assert from "node:assert/strict";
import { mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test, { after } from "node:test";
import { registerHooks } from "node:module";
import type { SDKMessage } from "@anthropic-ai/claude-agent-sdk";
// This suite owns ingress/publication. Title/list APIs are controlled here so
// result ingress cannot read a user's profile or outlive the captured writer.
const sdkUrl = import.meta.resolve("@anthropic-ai/claude-agent-sdk");
const controlledSdkUrl = `data:text/javascript,${encodeURIComponent(`
  export * from ${JSON.stringify(sdkUrl)};
  const pending = new Set();
  function read(value) { const operation = Promise.resolve().then(() => value);pending.add(operation);void operation.finally(() => pending.delete(operation));return operation; }
  export function getSessionInfo() { return read(undefined); }
  export function listSessions() { return read([]); }
  export async function settleReads() { while (pending.size) await Promise.all([...pending]); }
`)}`;
const loader = registerHooks({
  resolve(specifier, context, next) {
    return specifier === "@anthropic-ai/claude-agent-sdk" && !context.conditions.includes("require")
      ? { url: controlledSdkUrl, shortCircuit: true }
      : next(specifier, context);
  },
});
const controlledSdk = await import(controlledSdkUrl);
const settleSdkReads: () => Promise<void> = controlledSdk.settleReads;
const scratch = realpathSync(mkdtempSync(join(tmpdir(), "handler-ingress-")));
const { replaceProtocolEventWriter } = await import("./events.js");
const { bridgeLogger } = await import("./logger.js");
const { handleSdkMessage } = await import("./message_handlers.js");
const { createToolCall } = await import("./tooling.js");
import type { SessionState } from "./session_lifecycle.js";
const { handleSubagentCommand } = await import("./subagents.js");
const { sessions, updateSessionId } = await import("./session_lifecycle.js");
const { rememberColorPrompt } = await import("./session_color.js");

loader.deregister();
after(async () => {
  await settleSdkReads();
  rmSync(scratch, { recursive: true, force: true });
});

async function taskScenario(
  run: (
    session: SessionState,
    emit: (message: Record<string, unknown>) => void,
    writes: string[],
  ) => void | Promise<void>,
): Promise<void> {
  const session = makeSession();
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter((line) => {
    writes.push(line);
  });
  try {
    await run(
      session,
      (message) =>
        handleSdkMessage(session, {
          session_id: session.sessionId,
          uuid: crypto.randomUUID(),
          ...message,
        } as unknown as SDKMessage),
      writes,
    );
  } finally {
    await settleSdkReads();
    restore();
  }
}

type Wire = Record<string, unknown>;
const envelopes = (writes: string[], start = 0): Wire[] =>
  writes.slice(start).map((line) => JSON.parse(line));
const publicUpdates = (writes: string[], start = 0): Wire[] =>
  envelopes(writes, start).flatMap((event) => (event.update ? [event.update as Wire] : []));
const taskRows = (writes: string[], id: string, start = 0): Wire[] =>
  publicUpdates(writes, start).flatMap((update) =>
    update.type === "task_state_update"
      ? (update.tasks as Wire[]).filter((task) => task.task_id === id)
      : [],
  );
function publicTool(writes: string[], id: string): Wire | undefined {
  let tool: Wire | undefined;
  for (const update of publicUpdates(writes)) {
    if (update.type === "tool_call" && (update.tool_call as Wire).tool_call_id === id)
      tool = { ...(update.tool_call as Wire) };
    if (update.type === "tool_call_update" && (update.tool_call_update as Wire).tool_call_id === id)
      tool = { ...tool, ...((update.tool_call_update as Wire).fields as Wire) };
  }
  return tool;
}
const toolDeltas = (writes: string[], id: string, start = 0): Wire[] =>
  publicUpdates(writes, start).flatMap((update) =>
    update.type === "tool_call_update" && (update.tool_call_update as Wire).tool_call_id === id
      ? [(update.tool_call_update as Wire).fields as Wire]
      : [],
  );
type Ingress = (message: Wire) => void;
function invoke(
  emit: Ingress,
  id: string,
  name = "Agent",
  input: Wire = { description: "Work", prompt: "Perform the work" },
  parent: string | null = null,
): void {
  emit({
    type: "assistant",
    uuid: `source-${id}`,
    parent_tool_use_id: parent,
    message: {
      id: `response-${id}`,
      role: "assistant",
      content: [{ type: "tool_use", id, name, input }],
    },
  });
}

test("hook heartbeats cannot change parent tool timing, retry state or task correlation", () => taskScenario((session, emit, writes) => {
  const parent = createToolCall("parent", "Agent", { description: "Running worker" });
  parent.output_metadata = { timing: { duration_ms: 45_000, source: "progress" } };
  parent.task_metadata = { subagent_retry: { state: "waiting", attempt: 1, max_retries: 3, retry_delay_ms: 10_000 } };
  session.toolCalls.set(parent.tool_call_id, parent);
  session.taskToolUseIds.set("worker", parent.tool_call_id);
  const before = structuredClone(parent);
  for (const parent_tool_use_id of [undefined, "parent", "no-tool"]) {
    emit({ type: "tool_progress", tool_use_id: "hook-heartbeat", tool_name: "PreToolUse", parent_tool_use_id, task_id: "worker", elapsed_time_seconds: 1 });
  }
  assert.deepEqual(session.toolCalls.get("parent"), before);
  assert.equal(session.toolCalls.has("hook-heartbeat"), false);
  assert.equal(writes.length, 0);
  emit({ type: "tool_progress", tool_use_id: "child-progress", tool_name: "Read", parent_tool_use_id: "parent", elapsed_time_seconds: 46 });
  assert.equal(session.toolCalls.get("parent")?.output_metadata?.timing?.duration_ms, 46_000);
  assert.ok(writes.map(line => JSON.parse(line)).some(event => event.update?.tool_call_update?.tool_call_id === "parent"));
  const realTool = createToolCall("hook-real-tool", "Read", { file_path: "README.md" });
  session.toolCalls.set(realTool.tool_call_id, realTool);
  emit({ type: "tool_progress", tool_use_id: realTool.tool_call_id, tool_name: "Read", elapsed_time_seconds: 2 });
  assert.equal(session.toolCalls.get(realTool.tool_call_id)?.output_metadata?.timing?.duration_ms, 2_000);
}));

test("init replaces question capabilities so a previous advertisement cannot leak into a new init", async () => {
  const session = makeSession();
  session.query = { supportedCommands: async () => [], supportedAgents: async () => [] } as unknown as SessionState["query"];
  const restore = replaceProtocolEventWriter(() => {});
  try {
    for (const capabilities of [["ask_user_question_unanswered_v1", 17], undefined]) {
      handleSdkMessage(session, { type: "system", subtype: "init", session_id: session.sessionId, capabilities } as unknown as SDKMessage);
      assert.deepEqual([...session.sdkCapabilities ?? []], capabilities ? ["ask_user_question_unanswered_v1"] : []);
      await new Promise(resolve => setImmediate(resolve));
    }
  } finally { restore(); }
});

test("session write loss warns once without invalidating a delivered result or pending shutdown", () => {
  for (const [error_code, lost] of [["ENOSPC", ["transcript"]], ["EDQUOT", ["global_config"]], ["ENOSPC", ["transcript", "global_config"]]] as Array<[string, string[]]>) {
    taskScenario((session, emit, writes) => {
      emit({ type: "result", subtype: "success", is_error: false, result: "Valid result" });
      emit({ type: "system", subtype: "worker_shutting_down" });
      const shutdown = session.pendingWorkerShutdown;
      assert.ok(shutdown);
      const frame = { type: "system", subtype: "session_writes_lost", uuid: "loss-warning", error_code, lost, transcript_session_ids: ["before-clear", "after-clear"] };
      emit(frame);
      emit(frame);
      const events = writes.map(line => JSON.parse(line));
      const warnings = events.filter(event => event.update?.type === "system_notice_update");
      assert.equal(warnings.length, 1, JSON.stringify(events));
      const warning = warnings[0].update;
      assert.equal(warning.severity, "warning");
      assert.match(warning.message, error_code === "EDQUOT" ? /quota/ : /disk is full/);
      assert.equal(warning.message.includes("before-clear, after-clear"), lost.includes("transcript"));
      assert.equal(warning.message.includes("Global configuration"), lost.includes("global_config"));
      assert.equal(events.filter(event => event.event === "turn_complete").length, 1);
      assert.equal(events.some(event => event.event === "turn_error"), false);
      assert.equal(session.pendingWorkerShutdown, shutdown);
    });
  }
});

test("TaskCreate saved descriptions own both the live card and task state, including an empty saved description", () => {
  for (const description of ["Saved by the SDK", "", undefined]) taskScenario((session, emit) => {
    emit({ type: "assistant", message: { content: [{ type: "tool_use", id: "create-task", name: "TaskCreate", input: { subject: "Review", description: "Original input" } }] } });
    emit({ type: "user", message: { content: [{ type: "tool_result", tool_use_id: "create-task", content: "Task created" }] }, toolUseResult: { task: { id: "saved-task", subject: "Review", ...(description === undefined ? {} : { description }) } } });
    const expected = description ?? "Original input";
    assert.equal(session.tasksById.get("saved-task")?.description, expected);
    const card = session.toolCalls.get("create-task");
    assert.deepEqual(card?.raw_input, { subject: "Review", description: "Original input" }, "input provenance remains intact");
    const fields = card?.output_metadata?.presentation?.sections.flatMap(section => section.fields ?? []);
    assert.deepEqual(fields?.filter(field => field.label === "Description").map(field => field.value), expected ? [{ type: "text", value: expected }] : []);
  });
});

test("failed TaskCreate results do not publish saved task state or successful result presentation", () => taskScenario((session, emit) => {
  emit({ type: "assistant", message: { content: [{ type: "tool_use", id: "failed-create", name: "TaskCreate", input: { subject: "Review", description: "Original request" } }] } });
  emit({ type: "user", message: { content: [{ type: "tool_result", tool_use_id: "failed-create", content: "Create denied", is_error: true }] }, toolUseResult: { task: { id: "not-saved", subject: "Review", description: "Not saved" } } });
  assert.equal(session.tasksById.size, 0);
  assert.equal(session.toolCalls.get("failed-create")?.status, "failed");
  assert.equal(session.toolCalls.get("failed-create")?.output_metadata?.presentation, undefined);
}));

test("each API-rejected send reports its error even when only the first emits a rate limit update", () => taskScenario((session, emit, writes) => {
  const errorText = "You've hit your session limit · resets 1:30pm (Europe/Berlin)";
  for (const [index, prompt] of ["hello", "okay", "hello"].entries()) {
    const before = writes.length;
    const userUuid = `user-${index}`;
    emit({ type: "user", uuid: userUuid, message: { role: "user", content: prompt } });
    emit({ type: "assistant", error: "rate_limit", user_message_uuid: userUuid, message: { id: `reply-${index}`, content: [{ type: "text", text: errorText }] } });
    if (index === 0) {
      emit({ type: "rate_limit_event", rate_limit_info: { status: "rejected", rateLimitType: "five_hour", resetsAt: 1792409400 } });
    }
    emit({ type: "result", subtype: "success", is_error: true, terminal_reason: "api_error", result: errorText, user_message_uuid: userUuid, queued_turn_count: 0 });
    const events = writes.slice(before).map(line => JSON.parse(line));
    assert.deepEqual(events.filter(event => event.event === "turn_error" || event.event === "turn_complete"), [{
      event: "turn_error", session_id: session.sessionId, message: errorText,
      error_kind: "plan_limit", assistant_error: "rate_limit", sdk_result_subtype: "success",
      terminal_reason: "api_error", queued_turn_count: 0,
    }]);
    assert.equal(events.filter(event => event.update?.type === "rate_limit_update").length, index === 0 ? 1 : 0);
    assert.equal(events.some(event => event.update?.type === "agent_message_chunk"), false, "the error is carried once through turn_error");
  }
}));

test("either native failure signal settles the actual foreground tool and its own failed turn", async () => {
  for (const signal of [{ is_error: true }, { terminal_reason: "api_error" }, {}])
    await taskScenario((session, emit, writes) => {
      invoke(emit, "selected-read", "Read", { file_path: "notes.md" });
      const phase = writes.length,
        failed = Object.keys(signal).length > 0;
      emit({
        type: "result",
        uuid: "selected-result",
        user_message_uuid: "selected-turn",
        subtype: "success",
        result: failed ? "HTTP 429: request rejected" : "Read completed",
        api_error_status: failed ? 429 : undefined,
        ...signal,
      });
      const events = envelopes(writes, phase),
        terminals = events.filter(
          (event) => event.event === "turn_error" || event.event === "turn_complete",
        );
      assert.equal(terminals.length, 1);
      assert.equal(terminals[0].session_id, session.sessionId);
      assert.equal(terminals[0].event, failed ? "turn_error" : "turn_complete");
      if (failed) {
        assert.equal(terminals[0].message, "HTTP 429: request rejected");
        assert.equal(terminals[0].error_kind, "plan_limit");
        assert.equal(terminals[0].api_error_status, 429);
      }
      assert.ok(
        toolDeltas(writes, "selected-read", phase).some(
          (fields) => fields.status === (failed ? "failed" : "completed"),
        ),
      );
    });
});

test("recovery and a separately identified later failure do not inherit the earlier assistant error", () =>
  taskScenario((session, emit, writes) => {
    emit({
      type: "user",
      uuid: "earlier-user",
      message: { role: "user", content: "Earlier request" },
    });
    emit({
      type: "assistant",
      uuid: "earlier-error",
      error: "rate_limit",
      message: { id: "earlier-response", content: [{ type: "text", text: "temporary rejection" }] },
    });
    emit({
      type: "assistant",
      uuid: "recovered-source",
      user_message_uuid: "earlier-user",
      message: { id: "recovered-response", content: [{ type: "text", text: "Recovered answer" }] },
    });
    emit({
      type: "result",
      uuid: "recovered-result",
      user_message_uuid: "earlier-user",
      subtype: "success",
      is_error: false,
      terminal_reason: "completed",
      result: "Recovered answer",
    });
    assert.deepEqual(
      envelopes(writes)
        .filter((event) => event.event === "turn_complete" || event.event === "turn_error")
        .map((event) => [event.event, event.session_id]),
      [["turn_complete", session.sessionId]],
    );
    assert.deepEqual(
      publicUpdates(writes)
        .filter((update) => update.type === "agent_message_chunk")
        .map((update) => [update.source_message_uuid, update.content]),
      [["recovered-source", { type: "text", text: "Recovered answer" }]],
    );
    emit({
      type: "user",
      uuid: "later-user",
      message: { role: "user", content: "Independent later request" },
    });
    const phase = writes.length;
    emit({
      type: "result",
      uuid: "later-result",
      user_message_uuid: "later-user",
      subtype: "error_during_execution",
      errors: ["A separate failure"],
    });
    const errors = envelopes(writes, phase).filter((event) => event.event === "turn_error");
    assert.equal(errors.length, 1);
    assert.equal(errors[0].session_id, session.sessionId);
    assert.equal(errors[0].message, "A separate failure");
    assert.equal(errors[0].error_kind, "other");
    assert.equal(errors[0].assistant_error, undefined);
    assert.ok(!JSON.stringify(envelopes(writes, phase)).includes("temporary rejection"));
  }));

test("stale lifecycle and inventory frames cannot publish into a newer run or rewrite historical output", async () => {
  for (const [taskType, toolName] of [
    ["local_agent", "Agent"],
    ["local_bash", "Bash"],
    ["local_workflow", "Workflow"],
    ["local_mcp", "mcp__test__work"],
  ])
    await taskScenario((session, emit, writes) => {
      const input = (label: string) =>
        toolName === "Agent"
          ? { description: label, prompt: `Perform ${label}` }
          : toolName === "Bash"
            ? { command: `echo ${label}` }
            : {};
      invoke(emit, "launch-A", toolName, input("A"));
      const edge = (subtype: string, run_id: string, fields: Wire = {}) =>
        emit({ type: "system", subtype, task_id: "worker", run_id, ...fields });
      edge("task_started", "run-A", {
        tool_use_id: "launch-A",
        task_type: taskType,
        description: "A",
        parent_task_id: "parent",
      });
      emit({
        type: "user",
        message: {
          role: "user",
          content: [
            { type: "tool_result", tool_use_id: "launch-A", is_error: true, content: "A report" },
          ],
        },
      });
      edge("task_notification", "run-A", {
        status: "failed",
        summary: "A report",
        error: "A error",
        end_time: 123,
      });
      assert.ok(envelopes(writes).every(event => event.session_id === session.sessionId));
      const historical = structuredClone(publicTool(writes, "launch-A"));
      assert.equal(historical?.status, "failed");
      assert.ok(
        String(historical?.raw_output).includes("A report"),
        JSON.stringify({ toolName, historical, task: taskRows(writes, "worker").at(-1) }),
      );
      invoke(emit, "launch-B", toolName, input("B"));
      edge("task_started", "run-B", {
        tool_use_id: "launch-B",
        task_type: taskType,
        description: "B",
        is_backgrounded: true,
      });
      emit({
        type: "system",
        subtype: "task_started",
        task_id: "sibling",
        run_id: "sibling-run",
        task_type: "local_agent",
        description: "Sibling stays active",
      });
      const stale = [
        {
          type: "system",
          subtype: "task_started",
          task_id: "worker",
          run_id: "run-A",
          tool_use_id: "launch-A",
          description: "STALE-RUN start",
          parent_task_id: "wrong-parent",
        },
        {
          type: "system",
          subtype: "task_progress",
          task_id: "worker",
          run_id: "run-A",
          tool_use_id: "launch-A",
          description: "STALE-RUN progress",
          usage: { duration_ms: 999 },
        },
        {
          type: "system",
          subtype: "task_updated",
          task_id: "worker",
          run_id: "run-A",
          patch: { status: "failed", description: "STALE-RUN update", error: "STALE-RUN error" },
        },
        {
          type: "system",
          subtype: "task_notification",
          task_id: "worker",
          run_id: "run-A",
          tool_use_id: "launch-A",
          status: "stopped",
          summary: "STALE-RUN outcome",
        },
        {
          type: "system",
          subtype: "background_tasks_changed",
          tasks: [
            {
              task_id: "worker",
              run_id: "run-A",
              task_type: taskType,
              description: "STALE-RUN inventory",
            },
          ],
        },
      ];
      for (const frame of stale) {
        const phase = writes.length;
        emit(frame);
        assert.ok(
          !JSON.stringify(envelopes(writes, phase)).includes("STALE-RUN"),
          `${toolName}/${frame.subtype}: stale payload never crosses the wire`,
        );
        for (const row of taskRows(writes, "worker", phase)) {
          assert.equal(row.run_id, "run-B");
          assert.equal(row.source_tool_call_id, "launch-B");
          assert.equal(row.parent_task_id, "parent");
          assert.equal((row.metadata as Wire)?.error, undefined);
          assert.equal((row.metadata as Wire)?.summary, undefined);
        }
        assert.deepEqual(publicTool(writes, "launch-A"), historical);
        assert.equal(publicTool(writes, "launch-B")?.source_message_uuid, "source-launch-B");
        assert.ok(
          !["completed", "failed", "killed"].includes(
            String(publicTool(writes, "launch-B")?.status),
          ),
        );
        assert.equal(taskRows(writes, "sibling").at(-1)?.status, "in_progress");
        assert.ok(
          !toolDeltas(writes, "launch-B", phase).some(
            (fields) => ((fields.output_metadata as Wire)?.timing as Wire)?.duration_ms === 999,
          ),
        );
      }
      const phase = writes.length;
      edge("task_notification", "run-B", { status: "completed", summary: "B report" });
      edge("task_started", "run-B", { description: "Delayed same-run start" });
      for (const row of taskRows(writes, "worker", phase)) {
        assert.equal(row.run_id, "run-B");
        assert.equal(row.status, "completed");
        assert.equal((row.metadata as Wire)?.summary, "B report");
      }
      assert.equal(publicTool(writes, "launch-B")?.status, "completed");
      assert.equal((publicTool(writes, "launch-B")?.task_metadata as Wire)?.summary, "B report");
      assert.deepEqual(publicTool(writes, "launch-A"), historical);
    });
  for (const run_id of [undefined, "recorded-run"])
    await taskScenario((_session, emit, writes) => {
      invoke(emit, "late-launch");
      emit({
        type: "system",
        subtype: "task_started",
        task_id: "late-task",
        run_id,
        tool_use_id: "late-launch",
        task_type: "local_agent",
        description: "Launch",
      });
      emit({
        type: "user",
        message: {
          role: "user",
          content: [{ type: "tool_result", tool_use_id: "late-launch", content: "Actual report" }],
        },
      });
      emit({
        type: "system",
        subtype: "task_notification",
        task_id: "late-task",
        run_id,
        tool_use_id: "late-launch",
        status: "completed",
        summary: "Actual report",
      });
      const final = structuredClone(publicTool(writes, "late-launch")),
        phase = writes.length;
      assert.equal(final?.raw_output, "Actual report");
      emit({
        type: "user",
        tool_use_result: {
          isAsync: true,
          status: "async_launched",
          agentId: "late-task",
          description: "Late launch ack",
          outputFile: "worker.output",
        },
        message: {
          role: "user",
          content: [
            { type: "tool_result", tool_use_id: "late-launch", content: "Late launch ack" },
          ],
        },
      });
      assert.deepEqual(publicTool(writes, "late-launch"), final);
      for (const row of taskRows(writes, "late-task", phase)) {
        assert.equal(row.status, "completed");
        assert.equal((row.metadata as Wire)?.summary, "Actual report");
      }
    });
});

test("a new completed run publishes its own report without reopening the historical invocation", () =>
  taskScenario((session, emit, writes) => {
    invoke(emit, "launch");
    emit({
      type: "system",
      subtype: "task_started",
      task_id: "worker",
      tool_use_id: "launch",
      run_id: "run-A",
      task_type: "local_agent",
      description: "Worker",
    });
    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "worker",
      run_id: "run-A",
      status: "stopped",
      summary: "A stopped",
      error: "A error",
    });
    const historical = structuredClone(publicTool(writes, "launch")),
      phase = writes.length;
    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "worker",
      run_id: "run-B",
      status: "completed",
      summary: "B done",
    });
    const row = taskRows(writes, "worker", phase).at(-1);
    assert.equal(row?.run_id, "run-B");
    assert.equal(row?.status, "completed");
    assert.equal(row?.source_tool_call_id, "launch");
    assert.equal(row?.source_tool_call_run_id, "run-A");
    assert.equal((row?.metadata as Wire)?.summary, "B done");
    assert.equal((row?.metadata as Wire)?.error, undefined);
    assert.deepEqual(publicTool(writes, "launch"), historical);
    assert.deepEqual(toolDeltas(writes, "launch", phase), []);
    emit({
      type: "assistant",
      uuid: "child-source",
      agent_id: "worker",
      message: {
        id: "child-response",
        content: [{ type: "tool_use", id: "child-B", name: "Read", input: { file_path: "notes" } }],
      },
    });
    assert.equal(
      ((publicTool(writes, "child-B")?.meta as Wire)?.claudeCode as Wire)?.parentToolUseId,
      "launch",
    );
    assert.equal(publicTool(writes, "child-B")?.source_message_uuid, "child-source");
    const stale = writes.length;
    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "worker",
      tool_use_id: "launch",
      status: "failed",
      summary: "unversioned A",
    });
    assert.ok(!JSON.stringify(envelopes(writes, stale)).includes("unversioned A"));
    assert.deepEqual(publicTool(writes, "launch"), historical);
    assert.equal(taskRows(writes, "worker").at(-1)?.run_id, "run-B");
    assert.ok(envelopes(writes).every(event => event.session_id === session.sessionId));
  }));

test("actual parent result ingress settles foreground work while resumed background descendants keep their own outcomes", async () => {
  for (const failed of [false, true])
    await taskScenario((session, emit, writes) => {
      invoke(emit, "launch");
      emit({
        type: "system",
        subtype: "task_started",
        task_id: "worker",
        run_id: "A",
        tool_use_id: "launch",
        task_type: "local_agent",
        description: "Worker",
      });
      emit({
        type: "system",
        subtype: "task_notification",
        task_id: "worker",
        run_id: "A",
        status: "stopped",
        summary: "Historical A stopped",
      });
      const historical = structuredClone(publicTool(writes, "launch"));
      emit({
        type: "system",
        subtype: "task_started",
        task_id: "worker",
        run_id: "B",
        is_backgrounded: true,
        description: "B resumed",
      });
      emit({
        type: "assistant",
        uuid: "child-source",
        agent_id: "worker",
        message: {
          id: "child-response",
          content: [{ type: "tool_use", id: "child", name: "Read", input: { file_path: "notes" } }],
        },
      });
      invoke(emit, "foreground", "Read", { file_path: "root-notes" });
      const phase = writes.length;
      emit({
        type: "result",
        uuid: "root-result",
        subtype: failed ? "error_during_execution" : "success",
        ...(failed ? { errors: ["Root failed"] } : { result: "Root complete" }),
      });
      assert.ok(
        envelopes(writes, phase).some(
          (event) =>
            event.session_id === session.sessionId &&
            event.event === (failed ? "turn_error" : "turn_complete"),
        ),
      );
      assert.equal(publicTool(writes, "foreground")?.status, failed ? "failed" : "completed");
      assert.equal(publicTool(writes, "child")?.status, "in_progress");
      assert.deepEqual(toolDeltas(writes, "child", phase), []);
      assert.deepEqual(publicTool(writes, "launch"), historical);
      emit({
        type: "user",
        uuid: "child-result",
        agent_id: "worker",
        parent_tool_use_id: "launch",
        message: {
          role: "user",
          content: [
            { type: "tool_result", tool_use_id: "child", content: "Independent child output" },
          ],
        },
      });
      assert.equal(publicTool(writes, "child")?.status, "completed");
      assert.equal(publicTool(writes, "child")?.raw_output, "Independent child output");
      assert.deepEqual(publicTool(writes, "launch"), historical);
    });
});

test("first run observation retains invocation provenance and only matching legacy outcomes are admitted", () =>
  taskScenario((session, emit, writes) => {
    invoke(emit, "launch", "Agent", { description: "Original launch", prompt: "Original prompt" });
    emit({
      type: "system",
      subtype: "task_started",
      task_id: "worker",
      tool_use_id: "launch",
      task_type: "local_agent",
      description: "Original launch",
      prompt: "Original prompt",
    });
    emit({
      type: "system",
      subtype: "task_progress",
      task_id: "worker",
      run_id: "B",
      description: "Working",
    });
    const current = taskRows(writes, "worker").at(-1);
    assert.equal(current?.run_id, "B");
    assert.equal(current?.source_tool_call_id, "launch");
    assert.equal((current?.metadata as Wire)?.prompt, "Original prompt");
    for (const tool_use_id of [undefined, "old-launch"]) {
      const phase = writes.length;
      emit({
        type: "system",
        subtype: "task_notification",
        task_id: "worker",
        tool_use_id,
        status: "failed",
        summary: "unidentifiable stale report",
      });
      assert.ok(!JSON.stringify(envelopes(writes, phase)).includes("unidentifiable stale report"));
      assert.deepEqual(taskRows(writes, "worker").at(-1), current);
      assert.equal(publicTool(writes, "launch")?.status, "in_progress");
    }
    const phase = writes.length;
    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "worker",
      tool_use_id: "launch",
      status: "completed",
      summary: "Matched legacy outcome",
    });
    const completed = taskRows(writes, "worker", phase).at(-1);
    assert.equal(completed?.run_id, "B");
    assert.equal(completed?.status, "completed");
    assert.equal((completed?.metadata as Wire)?.summary, "Matched legacy outcome");
    assert.equal(completed?.source_tool_call_id, "launch");
    assert.equal((completed?.metadata as Wire)?.prompt, "Original prompt");
    assert.ok(envelopes(writes).every(event => event.session_id === session.sessionId));
  }));

test("inventory removals reject older runs but admit newer runs and the removed run's real final outcome", async () => {
  await taskScenario((session, emit, writes) => {
    const listed = (run_id: string) =>
      emit({
        type: "system",
        subtype: "background_tasks_changed",
        tasks: [
          { task_id: "worker", run_id, task_type: "local_agent", description: `Listed ${run_id}` },
        ],
      });
    listed("B");
    assert.equal(taskRows(writes, "worker").at(-1)?.run_id, "B");
    let phase = writes.length;
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
    assert.ok(
      publicUpdates(writes, phase).some((update) =>
        (update.removed_task_ids as string[] | undefined)?.includes("worker"),
      ),
    );
    phase = writes.length;
    listed("A");
    assert.equal(taskRows(writes, "worker", phase).length, 0, "older absent run cannot reappear");
    phase = writes.length;
    listed("C");
    const revived = taskRows(writes, "worker", phase).at(-1);
    assert.equal(revived?.run_id, "C");
    assert.equal(revived?.status, "in_progress");
    emit({
      type: "system",
      subtype: "task_started",
      task_id: "worker",
      run_id: "C",
      task_type: "local_agent",
      description: "C live",
    });
    phase = writes.length;
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
    const interrupted = taskRows(writes, "worker", phase).at(-1);
    assert.equal(interrupted?.run_id, "C");
    assert.equal(interrupted?.status, "completed");
    assert.equal((interrupted?.metadata as Wire)?.terminal_status, "interrupted");
    phase = writes.length;
    listed("C");
    assert.equal(taskRows(writes, "worker", phase).at(-1)?.status, "in_progress");
    for (const event of envelopes(writes)) assert.equal(event.session_id, session.sessionId);
  });
  await taskScenario((_session, emit, writes) => {
    emit({
      type: "system",
      subtype: "background_tasks_changed",
      tasks: [{ task_id: "removed", run_id: "B", task_type: "local_agent" }],
    });
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
    let phase = writes.length;
    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "removed",
      run_id: "A",
      status: "completed",
      summary: "STALE removed run",
    });
    assert.equal(taskRows(writes, "removed", phase).length, 0);
    assert.ok(!JSON.stringify(envelopes(writes, phase)).includes("STALE removed run"));
    phase = writes.length;
    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "removed",
      run_id: "B",
      status: "completed",
      summary: "Actual removed-run B report",
      task_type: "local_agent",
    });
    const restored = taskRows(writes, "removed", phase).at(-1);
    assert.equal(restored?.run_id, "B");
    assert.equal(restored?.status, "completed");
    assert.equal((restored?.metadata as Wire)?.summary, "Actual removed-run B report");
  });
});

test("published child tools retain producer and explicit parent identities without contaminating a later root reply", () =>
  taskScenario((session, emit, writes) => {
    invoke(emit, "launch-B");
    emit({
      type: "system",
      subtype: "task_started",
      task_id: "B",
      run_id: "B1",
      tool_use_id: "launch-B",
      task_type: "local_agent",
      description: "B",
    });
    const phase = writes.length;
    emit({
      type: "assistant",
      uuid: "child-source",
      agent_id: "B",
      parent_tool_use_id: null,
      error: "authentication_failed",
      context_usage: {},
      message: {
        id: "child",
        content: [
          { type: "text", text: "Private child reply" },
          {
            type: "tool_use",
            id: "launch-C",
            name: "Agent",
            input: { description: "C", prompt: "Nested work" },
          },
        ],
      },
    });
    emit({
      type: "user",
      uuid: "child-user",
      agent_id: "B",
      message: { role: "user", content: "Child user prompt" },
    });
    assert.equal(publicTool(writes, "launch-C")?.source_message_uuid, "child-source");
    assert.equal(
      ((publicTool(writes, "launch-C")?.meta as Wire)?.claudeCode as Wire)?.parentToolUseId,
      "launch-B",
    );
    assert.equal(
      taskRows(writes, "C").length,
      0,
      "a description alone cannot identify a new child task",
    );
    emit({
      type: "assistant",
      uuid: "explicit-child-source",
      agent_id: "B",
      parent_tool_use_id: "explicit-parent",
      message: {
        id: "explicit-child",
        content: [{ type: "tool_use", id: "nested", name: "Bash", input: { command: "pwd" } }],
      },
    });
    assert.equal(publicTool(writes, "nested")?.source_message_uuid, "explicit-child-source");
    assert.equal(
      ((publicTool(writes, "nested")?.meta as Wire)?.claudeCode as Wire)?.parentToolUseId,
      "explicit-parent",
    );
    emit({
      type: "assistant",
      uuid: "root-source",
      message: {
        id: "root-response",
        role: "assistant",
        content: [{ type: "text", text: "Independent root answer" }],
      },
    });
    emit({
      type: "result",
      uuid: "root-result",
      subtype: "success",
      result: "Independent root answer",
    });
    const events = envelopes(writes, phase);
    assert.equal(
      events.some((event) => event.event === "auth_required" || event.event === "turn_error"),
      false,
    );
    assert.ok(!JSON.stringify(events).includes("Private child reply"));
    assert.ok(!JSON.stringify(events).includes("Child user prompt"));
    assert.ok(
      events.some(
        (event) => event.session_id === session.sessionId && event.event === "turn_complete",
      ),
    );
    assert.deepEqual(
      publicUpdates(writes, phase)
        .filter((update) => update.type === "agent_message_chunk")
        .map((update) => [update.source_message_uuid, update.content]),
      [["root-source", { type: "text", text: "Independent root answer" }]],
    );
  }));

test("stop admission refuses stale intent and a gated native stop keeps its dispatched run after a real newer start", async () =>
  taskScenario(async (session, emit, writes) => {
    emit({
      type: "system",
      subtype: "task_started",
      task_id: "worker",
      run_id: "B",
      task_type: "local_agent",
      description: "B running",
    });
    const calls: string[] = [];
    let release!: () => void, admitted!: () => void;
    const held = new Promise<void>((resolve) => {
        release = resolve;
      }),
      entered = new Promise<void>((resolve) => {
        admitted = resolve;
      });
    session.query = {
      stopTask: async (id: string) => {
        calls.push(id);
        admitted();
        await held;
      },
    } as unknown as SessionState["query"];
    sessions.set(session.sessionId, session);
    let pending: Promise<void> | undefined;
    try {
      for (const expected_run_id of ["A", undefined]) {
        const request = expected_run_id ? "stale-A" : "missing-run",
          phase = writes.length;
        await handleSubagentCommand(
          {
            command: "stop_subagent",
            session_id: session.sessionId,
            task_id: "worker",
            expected_run_id,
          },
          request,
        );
        const replies = envelopes(writes, phase).filter(
          (event) => event.event === "subagent_stop_result",
        );
        assert.equal(replies.length, 1);
        assert.equal(replies[0].request_id, request);
        assert.equal(replies[0].session_id, session.sessionId);
        assert.equal(replies[0].task_id, "worker");
        assert.equal(replies[0].run_id, expected_run_id);
        assert.ok(replies[0].error);
        assert.deepEqual(calls, []);
      }
      const phase = writes.length;
      pending = handleSubagentCommand(
        {
          command: "stop_subagent",
          session_id: session.sessionId,
          task_id: "worker",
          expected_run_id: "B",
        },
        "stop-B",
      );
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          entered,
          pending.then(() => {
            throw new Error("stop finished before the gated SDK call");
          }),
          new Promise<never>((_, reject) => {
            timer = setTimeout(() => reject(new Error("native stop admission deadline")), 5_000);
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
      assert.deepEqual(calls, ["worker"]);
      emit({
        type: "system",
        subtype: "task_started",
        task_id: "worker",
        run_id: "C",
        task_type: "local_agent",
        description: "C running",
      });
      release();
      await pending;
      const replies = envelopes(writes, phase).filter(
        (event) => event.event === "subagent_stop_result",
      );
      assert.deepEqual(replies, [
        {
          event: "subagent_stop_result",
          request_id: "stop-B",
          session_id: session.sessionId,
          task_id: "worker",
          run_id: "B",
        },
      ]);
      const rows = taskRows(writes, "worker", phase);
      assert.ok(rows.length > 0);
      for (const row of rows) {
        assert.equal(row.run_id, "C");
        assert.equal(row.status, "in_progress");
      }
      assert.equal(rows.at(-1)?.description, "C running");
    } finally {
      release();
      try {
        await pending;
      } finally {
        sessions.delete(session.sessionId);
      }
    }
  }));

function makeSession(): SessionState {
  return {
    sessionId: "session-1",
    cwd: scratch,
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

test("stream completions are suppressed while valid nonstreamed replies survive irrelevant invalid context markers", () =>
  taskScenario((session, emit, writes) => {
    const stream = (event: Wire) =>
      emit({ type: "stream_event", uuid: "stream-source", parent_tool_use_id: null, event });
    const complete = (uuid: string, message: Wire, extra: Wire = {}) =>
      emit({
        type: "assistant",
        uuid,
        parent_tool_use_id: null,
        message: { role: "assistant", ...message },
        ...extra,
      });
    stream({ type: "message_start", message: { id: "streamed-response" } });
    stream({ type: "content_block_start", index: 0, content_block: { type: "text", text: "" } });
    stream({
      type: "content_block_delta",
      index: 0,
      delta: { type: "text_delta", text: "already streamed" },
    });
    complete("block-completion", {
      id: "streamed-response",
      stop_reason: null,
      content: [{ type: "text", text: "already streamed" }],
    });
    stream({ type: "content_block_stop", index: 0 });
    stream({ type: "message_stop" });
    complete("final-completion", {
      id: "streamed-response",
      stop_reason: "end_turn",
      content: [{ type: "text", text: "already streamed" }],
    });
    complete(
      "child",
      { id: "child-response", content: [{ type: "text", text: "private child text" }] },
      { parent_tool_use_id: "child-parent" },
    );
    complete("blank", { id: "blank-response", content: [{ type: "text", text: "   " }] });
    complete(
      "error",
      { id: "error-response", content: [{ type: "text", text: "API error text" }] },
      { error: "rate_limit" },
    );
    complete("ordinary-no-id", { content: [{ type: "text", text: "uncorrelated ordinary text" }] });
    for (const [index, marker] of ["invalid", [], null, 3].entries())
      complete(
        `invalid-context-${index}`,
        { content: [{ type: "text", text: "not context Markdown" }] },
        { context_usage: marker },
      );
    complete("empty-context", { content: [{ type: "text", text: "   " }] }, { context_usage: {} });
    complete(
      "valid-local-source",
      {
        id: "normal-local-response",
        stop_reason: "end_turn",
        content: [{ type: "text", text: "Valid local reply" }],
      },
      { context_usage: "irrelevant invalid metadata" },
    );
    const chunks = publicUpdates(writes).filter((update) => update.type === "agent_message_chunk");
    assert.deepEqual(
      chunks.map((update) => [update.source_message_uuid, update.content]),
      [
        ["stream-source", { type: "text", text: "already streamed" }],
        ["valid-local-source", { type: "text", text: "Valid local reply" }],
      ],
    );
    assert.ok(
      envelopes(writes)
        .filter((event) => (event.update as Wire)?.type === "agent_message_chunk")
        .every((event) => event.session_id === session.sessionId),
    );
  }));

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

// A foreground Agent call can finish through its tool result before the task
// notification arrives. The finished tool call then rejects the notification,
// which must still settle the separate task row the footer and /agents count.
test("task outcomes after concrete tool completion publish their own cause without rewriting the finished output", async () => {
  for (const outcome of ["completed", "failed", "stopped"])
    await taskScenario((session, emit, writes) => {
      invoke(emit, "finished-tool", "Agent", { description: "Work", prompt: "Perform work" });
      emit({
        type: "system",
        subtype: "task_started",
        task_id: "agent-1",
        tool_use_id: "finished-tool",
        task_type: "local_agent",
        description: "Work",
      });
      emit({
        type: "user",
        message: {
          role: "user",
          content: [
            {
              type: "tool_result",
              tool_use_id: "finished-tool",
              content: "Concrete foreground output",
            },
          ],
        },
      });
      assert.equal(publicTool(writes, "finished-tool")?.status, "completed");
      assert.equal(publicTool(writes, "finished-tool")?.raw_output, "Concrete foreground output");
      const phase = writes.length;
      emit({
        type: "system",
        subtype: "task_notification",
        task_id: "agent-1",
        tool_use_id: "finished-tool",
        status: outcome,
        summary: `Actual ${outcome} report`,
        output_file: "",
      });
      emit({
        type: "system",
        subtype: "task_progress",
        task_id: "agent-1",
        tool_use_id: "finished-tool",
        description: "Stale progress",
        usage: { duration_ms: 999 },
      });
      const rows = taskRows(writes, "agent-1", phase);
      assert.ok(rows.length > 0);
      for (const row of rows) {
        assert.equal(row.status, "completed");
        assert.equal((row.metadata as Wire)?.terminal_status, outcome);
        assert.equal((row.metadata as Wire)?.summary, `Actual ${outcome} report`);
        assert.notEqual(row.description, "Stale progress");
      }
      for (const fields of toolDeltas(writes, "finished-tool", phase)) {
        assert.ok(fields.status === undefined || fields.status === "completed");
        assert.ok(
          fields.raw_output === undefined || fields.raw_output === "Concrete foreground output",
        );
      }
      assert.equal(publicTool(writes, "finished-tool")?.status, "completed");
      assert.equal(publicTool(writes, "finished-tool")?.raw_output, "Concrete foreground output");
      assert.ok(envelopes(writes, phase).every((event) => event.session_id === session.sessionId));
    });
});

// Measured on Claude Code 2.1.288: a backgrounded subagent is listed by
// background_tasks_changed, and the snapshot that drops it arrives right after
// its task_notification (or after its killed patch when stopped).



test("background snapshots publish owned removals and inferred interruption without overriding real lifecycle outcomes", async () => {
  for (const route of ["already-terminal", "progress", "listing", "direct-outcome"])
    await taskScenario((session, emit, writes) => {
      const edge = (subtype: string, fields: Wire = {}) =>
        emit({ type: "system", subtype, task_id: "worker", ...fields });
      const listed = [
        { task_id: "worker", task_type: "local_agent", description: "Background worker" },
        { task_id: "inventory-only", task_type: "local_bash", description: "Inventory peer" },
      ];
      edge("task_started", { task_type: "local_agent", description: "Background worker" });
      emit({
        type: "system",
        subtype: "task_started",
        task_id: "foreground",
        task_type: "local_agent",
        description: "Never listed foreground",
      });
      emit({ type: "system", subtype: "background_tasks_changed", tasks: listed });
      const expected = route === "progress" || route === "listing" ? "failed" : "completed",
        summary = `${route} actual outcome`;
      if (route === "already-terminal") edge("task_notification", { status: expected, summary });
      let phase = writes.length;
      emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
      const removal = publicUpdates(writes, phase)
        .filter((update) => update.type === "task_state_update")
        .flatMap((update) => (update.removed_task_ids as string[]) ?? []);
      assert.ok(removal.includes("inventory-only"));
      assert.ok(!removal.includes("worker"));
      assert.ok(!removal.includes("foreground"));
      const disappeared =
        taskRows(writes, "worker", phase).at(-1) ?? taskRows(writes, "worker").at(-1);
      assert.equal(disappeared?.status, "completed");
      assert.equal(
        (disappeared?.metadata as Wire)?.terminal_status,
        route === "already-terminal" ? "completed" : "interrupted",
      );
      assert.equal(taskRows(writes, "foreground").at(-1)?.status, "in_progress");
      if (route === "progress" || route === "listing") {
        phase = writes.length;
        if (route === "progress")
          edge("task_progress", { description: "Live progress resumes worker" });
        else emit({ type: "system", subtype: "background_tasks_changed", tasks: [listed[0]] });
        const revived = taskRows(writes, "worker", phase).at(-1);
        assert.equal(revived?.status, "in_progress");
        assert.equal((revived?.metadata as Wire)?.terminal_status, undefined);
      }
      if (route !== "already-terminal") edge("task_notification", { status: expected, summary });
      phase = writes.length;
      emit({ type: "system", subtype: "background_tasks_changed", tasks: [listed[0]] });
      edge("task_progress", { description: "Late progress must not replace report" });
      const final = taskRows(writes, "worker").at(-1);
      assert.equal(final?.status, "completed");
      assert.equal((final?.metadata as Wire)?.terminal_status, expected);
      assert.equal((final?.metadata as Wire)?.summary, summary);
      for (const row of taskRows(writes, "worker", phase)) {
        assert.equal(row.status, "completed");
        assert.equal((row.metadata as Wire)?.summary, summary);
        assert.equal((row.metadata as Wire)?.terminal_status, expected);
      }
      assert.equal(taskRows(writes, "foreground").at(-1)?.status, "in_progress");
      assert.ok(envelopes(writes).every((event) => event.session_id === session.sessionId));
    });
});











/** Every `system_notice_update` the bridge wrote while `run` ran. */


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
type CapturedLog = { level: string; eventName: string; sessionId?: string; fields?: Wire; message?: string };
async function loggedScenario(run: (records: CapturedLog[]) => void | Promise<void>): Promise<void> {
  const records: CapturedLog[] = [], originals = { debug: bridgeLogger.debug, info: bridgeLogger.info, warn: bridgeLogger.warn, error: bridgeLogger.error };
  for (const level of ["debug", "info", "warn", "error"] as const) bridgeLogger[level] = event => { records.push({ ...event, level }); };
  try { await run(records);await settleSdkReads(); } finally { Object.assign(bridgeLogger, originals); }
}

test("hook failures stay attributed to their own hook, session and incident in the diagnostic log beside quiet controls", () =>
  taskScenario(async (session, emit) =>
    loggedScenario((records) => {
      for (const subtype of ["hook_started", "hook_progress"])
        emit({
          type: "system",
          subtype,
          hook_id: subtype,
          hook_name: "PreToolUse:quiet",
          hook_event: "PreToolUse",
        });
      handleSdkMessage(
        session,
        hookResponse("PreToolUse", { outcome: "success", exit_code: 0, output: "ok" }, "success"),
      );
      handleSdkMessage(session, hookResponse("Stop", { outcome: "cancelled" }, "cancelled"));
      const failure = hookResponse(
        "PreToolUse",
        {
          hook_name: "PreToolUse:shared",
          outcome: "error",
          exit_code: 1,
          stderr: "shared diagnostic cause",
        },
        "incident-one",
      );
      handleSdkMessage(session, failure);
      handleSdkMessage(
        session,
        hookResponse(
          "PreToolUse",
          {
            hook_name: "PreToolUse:shared",
            outcome: "error",
            exit_code: 1,
            stderr: "shared diagnostic cause",
          },
          "incident-two",
        ),
      );
      const other = makeSession();
      other.sessionId = "other-session";
      handleSdkMessage(
        other,
        hookResponse(
          "PreToolUse",
          {
            session_id: other.sessionId,
            hook_name: "PreToolUse:shared",
            outcome: "error",
            exit_code: 1,
            stderr: "shared diagnostic cause",
          },
          "incident-one",
        ),
      );
      for (const [owner, id] of [
        [session.sessionId, "incident-one"],
        [session.sessionId, "incident-two"],
        [other.sessionId, "incident-one"],
      ])
        assert.ok(
          records.some(
            (record) =>
              record.level === "warn" &&
              record.sessionId === owner &&
              record.fields?.hook_id === id &&
              record.fields?.exit_code === 1,
          ),
        );
    }),
  ));



/** The level and event name of every info, warning and error record logged while `run` ran. */


/** Log records for one exit-2 `hook_response` per event, in order. */


// The SDK reports every exit 2 as outcome "error". On these events the hook
// reference makes it the hook's decision (block the tool call, the prompt or
// the compaction, keep Claude working, or hand the output to Claude), so a gate
// must not read "failed" each turn. Measured on SDK 0.3.288, the SDK already
// shows the blocked tool call, prompt, task, compaction or model switch; Stop
// and SubagentStop keep Claude working, and the rest could not be measured, so
// none of them gets a notice that could duplicate what the user already sees.

test("independently named exit-two hook events retain identity and reporting severity in the diagnostic log", async () => {
  // Supplied SDK frames exercise host reporting; this table does not execute hooks.
  const rows: Array<{ event: string; level: string }> = [
    ...[
      "PreToolUse",
      "UserPromptSubmit",
      "UserPromptExpansion",
      "Stop",
      "SubagentStop",
      "TeammateIdle",
      "TaskCreated",
      "TaskCompleted",
      "ConfigChange",
      "PreCompact",
      "PreModelSwitch",
      "Elicitation",
      "ElicitationResult",
    ].map((event) => ({ event, level: "info" })),
    ...["PostToolUse", "PostToolUseFailure", "PostToolBatch"].map((event) => ({
      event,
      level: "info",
    })),
    ...["Notification", "PermissionRequest", "WorktreeCreate", "SomeFutureEvent"].map((event) => ({
      event,
      level: "warn",
    })),
  ];
  for (const row of rows)
    await taskScenario(async (session, _emit) =>
      loggedScenario((records) => {
        const id = `exit-two-${row.event}`,
          name = `${row.event}:selected-gate`,
          cause = `${row.event} independent decision diagnostic`;
        handleSdkMessage(
          session,
          hookResponse(
            row.event,
            { hook_name: name, outcome: "error", exit_code: 2, stderr: cause },
            id,
          ),
        );
        const diagnostic = records.filter(
          (record) =>
            record.fields?.hook_id === id &&
            (record.eventName === "sdk_hook_decision" || record.eventName === "sdk_hook_failed"),
        );
        assert.equal(diagnostic.length, 1);
        assert.equal(diagnostic[0].level, row.level);
        assert.equal(diagnostic[0].sessionId, session.sessionId);
        assert.equal(diagnostic[0].fields?.hook_name, name);
        assert.equal(diagnostic[0].fields?.hook_event, row.event);
        assert.equal(diagnostic[0].fields?.exit_code, 2);
        assert.equal(diagnostic[0].fields?.output_excerpt, cause);
      }),
    );
});

// Measured on SDK 0.3.288: after these hooks exit 2 only the hook_response
// carries the output (the tool result is the tool's own, or the turn ends with
// an empty result), so one info notice is the only place the user sees it.



// Measured on SDK 0.3.288 with a SessionStart command hook that exits 2: the
// session starts and the SDK sends nothing but this hook_response, so the
// failure notice is the only place the user sees the hook's error.


// Notification and PermissionRequest ignore exit 2, WorktreeCreate fails on any
// non-zero exit, and an event the hook reference does not list (one a newer SDK
// adds) has no documented decision: all of them stay failures.




test("targeted hook diagnostics select a useful fallback and bound Unicode output without credential disclosure", async () => {
  const oversized = `deploy failed api_key=sk-ant-verysecret ${"\ud83d\ude00".repeat(500)} TAIL-MUST-BE-CLIPPED`;
  const rows = [
    {
      stderr: oversized,
      output: "wrong-output-source",
      stdout: "wrong-stdout-source",
      expected: "deploy failed",
    },
    {
      stderr: " \n ",
      output: "output failed api_key=sk-ant-verysecret",
      stdout: "wrong-stdout-source",
      expected: "output failed",
    },
    {
      stderr: " ",
      output: " ",
      stdout: "stdout failed api_key=sk-ant-verysecret",
      expected: "stdout failed",
    },
  ];
  for (const [index, row] of rows.entries())
    await taskScenario(async (session) =>
      loggedScenario((records) => {
        const id = `privacy-${index}`;
        handleSdkMessage(
          session,
          hookResponse(
            "PostToolUse",
            {
              hook_name: `PostToolUse:diagnostic-${index}`,
              outcome: "error",
              exit_code: 1,
              stderr: row.stderr,
              output: row.output,
              stdout: row.stdout,
            },
            id,
          ),
        );
        const diagnostic = records.find(
          (record) => record.eventName === "sdk_hook_failed" && record.fields?.hook_id === id,
        );
        assert.ok(diagnostic);
        assert.equal(diagnostic.level, "warn");
        const excerpt = String(diagnostic.fields?.output_excerpt);
        assert.ok(excerpt.startsWith(row.expected));
        assert.ok(excerpt.includes("api_key=[redacted]"));
        assert.ok(
          Array.from(excerpt).length <= 240,
          "the hook diagnostic has a 240-code-point budget",
        );
        assert.equal(
          Buffer.from(excerpt, "utf8").toString("utf8"),
          excerpt,
          "truncation cannot leave half a surrogate pair",
        );
        const captured = JSON.stringify({ records });
        for (const secret of [
          "sk-ant-verysecret",
          "wrong-output-source",
          "wrong-stdout-source",
          "TAIL-MUST-BE-CLIPPED",
        ])
          assert.ok(!captured.includes(secret), secret);
        if (index === 0) assert.ok(excerpt.includes("\ud83d\ude00"));
      }),
    );
});

// Fork tests: subagent task rows, session accent colour and child-model telemetry.
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
  }, "the settled row keeps its lifecycle metadata");
  assert.equal(session.tasksById.get("agent-1")?.status, "completed");
  assert.equal(session.tasksById.has("shell-1"), false);
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
