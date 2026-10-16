import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const loggerModuleUrl = new URL("./logger.js", import.meta.url).href;

/** Run the logger in a child, because it reads its mode from the environment once. */
function stderrLines(mode: string | undefined): Array<Record<string, unknown>> {
  const script = `
    const { bridgeLogger, LOG_TARGETS } = await import(${JSON.stringify(loggerModuleUrl)});
    const fields = { error_message: "spawn failed api_key=sk-ant-verysecret", detail: "y".repeat(2000), count: 3 };
    for (const level of ["debug", "info", "warn", "error"]) {
      bridgeLogger[level]({ target: LOG_TARGETS.BRIDGE_SDK, eventName: level + "_event", message: level, fields });
    }
  `;
  const env = { ...process.env };
  delete env.CLAUDE_RS_BRIDGE_DIAGNOSTICS;
  if (mode !== undefined) env.CLAUDE_RS_BRIDGE_DIAGNOSTICS = mode;
  const result = spawnSync(process.execPath, ["--input-type=module", "-e", script], {
    cwd: fileURLToPath(new URL(".", import.meta.url)),
    env,
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  return result.stderr
    .split("\n")
    .filter((line) => line.length > 0)
    .map((line) => JSON.parse(line) as Record<string, unknown>);
}

test("default diagnostics write only warnings and errors, with bounded and redacted fields", () => {
  const records = stderrLines("0");

  assert.deepEqual(
    records.map((record) => record.level),
    ["warn", "error"],
  );
  const fields = records[0].fields as Record<string, unknown>;
  assert.equal(fields.error_message, "spawn failed api_key=[redacted]");
  assert.ok(String(fields.detail).length <= 243);
  assert.equal(fields.count, 3);
});

test("detailed diagnostics keep every level and the full fields", () => {
  const records = stderrLines("1");

  assert.deepEqual(
    records.map((record) => record.level),
    ["debug", "info", "warn", "error"],
  );
  assert.equal((records[0].fields as Record<string, unknown>).detail, "y".repeat(2000));
});

test("an unset mode writes nothing", () => {
  assert.deepEqual(stderrLines(undefined), []);
});
