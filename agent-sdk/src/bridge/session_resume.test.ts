import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import readline from "node:readline";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

type Envelope = Record<string, unknown>;

test("resume commands restore a transcript name", async () => {
  for (const command of ["resume_session", "create_session"] as const) {
    const directory = realpathSync(mkdtempSync(join(tmpdir(), "bridge-label-resume-")));
    const cwd = join(directory, "project");
    const profile = join(directory, "config");
    const sessionId = "11111111-1111-4111-8111-111111111111";
    const projectDir = join(profile, "projects", cwd.replace(/[^a-zA-Z0-9]/g, "-"));
    mkdirSync(cwd);
    mkdirSync(projectDir, { recursive: true });
    const records = [
      { type: "custom-title", customTitle: "Saved name" },
      { type: "user", uuid: "user-1", parentUuid: null, message: { role: "user", content: "hello" } },
    ].map((record) => ({ ...record, sessionId, cwd, timestamp: "2026-10-09T00:00:00.000Z" }));
    writeFileSync(join(projectDir, `${sessionId}.jsonl`), `${records.map((record) => JSON.stringify(record)).join("\n")}\n`);

    const journal = join(directory, "query.jsonl");
    const fixturePath = join(directory, "sdk-fixture.mjs");
    const sdkUrl = import.meta.resolve("@anthropic-ai/claude-agent-sdk");
    writeFileSync(join(directory, "package.json"), JSON.stringify({ version: "0.3.296" }));
    writeFileSync(fixturePath, `
      export * from ${JSON.stringify(sdkUrl)};
      import { appendFileSync } from "node:fs";
      export async function listSessions() {
        return [{ sessionId: ${JSON.stringify(sessionId)}, cwd: ${JSON.stringify(cwd)}, lastModified: 1 }];
      }
      export function query({ options }) {
        appendFileSync(process.env.QUERY_JOURNAL, JSON.stringify({ resume: options.resume, extraArgs: options.extraArgs }) + "\\n");
        return {
          [Symbol.asyncIterator]() { return this; },
          next() { return new Promise(() => {}); },
          close() {},
          async initializationResult() { return { current_permission_mode: "default", models: [], commands: [], agents: [], account: { apiKeySource: "fixture" }, fast_mode_state: "off" }; },
          async getSettings() { return { applied: { model: "opus", effort: "high", ultracodeAvailable: false, ultracodeRequested: false, ultracode: false } }; },
          async supportedCommands() { return []; },
        };
      }
    `);
    const loaderPath = join(directory, "loader.mjs");
    writeFileSync(loaderPath, `
      import { registerHooks } from "node:module";
      registerHooks({ resolve(specifier, context, nextResolve) {
        return specifier === "@anthropic-ai/claude-agent-sdk"
          ? { url: ${JSON.stringify(pathToFileURL(fixturePath).href)}, shortCircuit: true }
          : nextResolve(specifier, context);
      } });
    `);
    const bridgePath = join(dirname(fileURLToPath(import.meta.url)), "../bridge.js");
    const child = spawn(process.execPath, ["--import", pathToFileURL(loaderPath).href, bridgePath], {
      env: { ...process.env, CLAUDE_CONFIG_DIR: profile, CLAUDE_CODE_PROJECT_DIR_NAME: undefined, QUERY_JOURNAL: journal, CLAUDE_CODE_EXECUTABLE: "" },
      stdio: "pipe",
    });
    const output = readline.createInterface({ input: child.stdout });
    const events: Envelope[] = [];
    const errors: string[] = [];
    child.stderr.on("data", (chunk: Buffer) => errors.push(chunk.toString()));
    const timeout = setTimeout(() => child.kill(), 10_000);
    try {
      child.stdin.write(`${JSON.stringify(command === "resume_session"
        ? { command, session_id: sessionId, request_id: "label-resume", launch_settings: {} }
        : { command, cwd, resume: sessionId, request_id: "label-resume", launch_settings: {} })}\n`);
      for await (const line of output) {
        events.push(JSON.parse(line) as Envelope);
        if (events.some((event) => event.event === "sessions_listed")) break;
      }
      assert.ok(events.some((event) => event.event === "connected"), `${command}: ${JSON.stringify(events)} ${errors.join("")}`);
      const query = JSON.parse(readFileSync(journal, "utf8").trim()) as Envelope;
      assert.equal(query.resume, sessionId);
      assert.deepEqual(query.extraArgs, { name: "Saved name" }, `${command}: ${JSON.stringify(events)}`);
    } finally {
      clearTimeout(timeout);
      output.close();
      if (child.exitCode === null && child.signalCode === null) {
        child.kill();
        await new Promise<void>((resolve) => child.once("exit", () => resolve()));
      }
      rmSync(directory, { recursive: true, force: true });
    }
  }
});
