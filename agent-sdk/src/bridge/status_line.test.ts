import assert from "node:assert/strict";
import test from "node:test";
import { replaceProtocolEventWriter } from "./events.js";
import { scheduleStatusLine, stopStatusLine, executeStatusLine, readConfiguredStatusLine } from "./status_line.js";
import { updateSessionId, type SessionState } from "./session_lifecycle.js";
import { resolveCurrentModel } from "./model_metadata.js";

const delay = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
function session(): SessionState {
  return {
    sessionId: "original", cwd: process.cwd(), connected: true, closing: false,
    query: { getContextUsage: async () => ({ percentage: 34, maxTokens: 200000 }) },
  } as unknown as SessionState;
}

test("status line receives documented public fields on stdin and bounds output", async () => {
  const value = await executeStatusLine(
    'node -e "process.stdin.on(\'data\', c => process.stdout.write(JSON.parse(c).session_id))"',
    process.cwd(), { session_id: "fixture" },
  );
  assert.equal(value, "fixture");
  const aborted = new AbortController();
  aborted.abort();
  assert.equal(await executeStatusLine("node -v", process.cwd(), {}, aborted.signal), null);
  const running = new AbortController();
  const started = Date.now();
  const result = executeStatusLine('node -e "setTimeout(() => {}, 10000)"', process.cwd(), {}, running.signal);
  setTimeout(() => running.abort(), 50);
  assert.equal(await result, null);
  assert.ok(Date.now() - started < 1000);
});

test("project and local shadow commands never enter the executable status line authority", async () => {
  const state = session();
  const commands: string[] = [];
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter(line => writes.push(line));
  try {
    scheduleStatusLine(state, {
      readConfig: cwd => readConfiguredStatusLine(cwd, async options => {
        assert.deepEqual(options.settingSources, ["user"]);
        const shadow = options.settingSources.join(",").includes("project") || options.settingSources.join(",").includes("local");
        return { effective: { statusLine: { type: "command", command: shadow ? "PROJECT_OR_LOCAL_SHADOW" : "USER_COMMAND" } } };
      }),
      run: async (command, _cwd, input) => {
        commands.push(command);
        assert.equal((input as { session_id: string }).session_id, "original");
        return "USER_RESULT";
      },
    });
    await delay(360);
    assert.deepEqual(commands, ["USER_COMMAND"]);
    assert.ok(writes.some(line => line.includes("USER_RESULT")));
    assert.ok(!writes.some(line => line.includes("PROJECT_OR_LOCAL_SHADOW")));
  } finally {
    stopStatusLine(state);
    restore();
  }
});

test("replaced session and changed command discard stale output", async () => {
  const writes: string[] = [];
  const restore = replaceProtocolEventWriter(line => writes.push(line));
  const state = session();
  let finishOld: ((text: string) => void) | undefined;
  try {
    scheduleStatusLine(state, {
      readConfig: async () => ({ command: "old" }),
      run: async () => new Promise(resolve => { finishOld = resolve; }),
    });
    await delay(360);
    assert.ok(finishOld);
    updateSessionId(state, "replacement");
    finishOld?.("STALE");
    await delay(0);
    assert.equal(writes.length, 0);

    state.sessionId = "current";
    let finishA: ((text: string) => void) | undefined;
    scheduleStatusLine(state, {
      readConfig: async () => ({ command: "A" }),
      run: async () => new Promise(resolve => { finishA = resolve; }),
    });
    await delay(360);
    assert.ok(finishA);
    scheduleStatusLine(state, { readConfig: async () => ({ command: "B" }), run: async () => "NEW" });
    finishA?.("OLD COMMAND");
    await delay(360);
    const updates = writes.map(line => JSON.parse(line).update).filter(update => update?.type === "status_line_update");
    assert.deepEqual(updates.map(update => update.text), ["NEW"]);
  } finally {
    stopStatusLine(state);
    restore();
  }
});

// The catalog entry that matches the running model is often "default", whose
// display name is the picker label "Default (recommended)". The status line
// command documents model.display_name as the model's name, so it must get the
// humanised id the footer shows, never the picker label.
test("status line model.display_name is the model name, not a catalog option label", async () => {
  const state = session();
  state.currentModel = resolveCurrentModel({
    requestedModelId: "default",
    resolvedRuntimeModelId: "claude-opus-5-5",
    availableModels: [
      { id: "default", resolved_model: "claude-opus-5-5", display_name: "Default (recommended)", supports_effort: true, supported_effort_levels: [] },
      { id: "sonnet", resolved_model: "claude-sonnet-5-5", display_name: "Sonnet", supports_effort: true, supported_effort_levels: [] },
    ],
  });
  assert.equal(state.currentModel.display_name_long, "Default (recommended)", "fixture must hit the default entry");
  const inputs: Array<{ model?: { id: string; display_name: string } }> = [];
  let observed: (() => void) | undefined;
  const ready = new Promise<void>(resolve => { observed = resolve; });
  try {
    scheduleStatusLine(state, {
      readConfig: async () => ({ command: "cmd" }),
      run: async (_command, _cwd, input) => {
        inputs.push(input as { model?: { id: string; display_name: string } });
        observed?.();
        return "ok";
      },
    });
    await ready;
  } finally {
    stopStatusLine(state);
  }
  assert.deepEqual(inputs.map(input => input.model), [{ id: "claude-opus-5-5", display_name: "Opus 5.5" }]);
});
