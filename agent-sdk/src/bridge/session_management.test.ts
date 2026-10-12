import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import readline from "node:readline";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { mutateListedSession } from "./command_session_data.js";

test("delete rechecks a target that becomes active while the SDK list is delayed", async () => {
  let releaseList!: (value: Array<{ sessionId: string; summary: string; lastModified: number; cwd: string }>) => void;
  const pendingList = new Promise<Array<{ sessionId: string; summary: string; lastModified: number; cwd: string }>>((resolve) => { releaseList = resolve; });
  let active = false;
  let deletes = 0;
  const pending = mutateListedSession(
    { command: "delete_session", session_id: "saved" },
    {
      listSessions: async () => pendingList,
      sessionById: () => active ? ({} as NonNullable<ReturnType<typeof import("./session_lifecycle.js").sessionById>>) : null,
      tagSession: async () => {},
      deleteSession: async () => { deletes += 1; },
    },
  );
  await Promise.resolve();
  active = true;
  releaseList([{ sessionId: "saved", summary: "Saved", lastModified: 1, cwd: "/project" }]);
  const result = await pending;
  assert.match(result.error ?? "", /Switch sessions before deleting/);
  assert.equal(deletes, 0);
});

test("session mutations use SDK IDs and directory, acknowledge failures, then return fresh metadata", async () => {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "bridge-session-management-")));
  const project = join(directory, "project");
  const journal = join(directory, "journal.jsonl");
  const sdkUrl = import.meta.resolve("@anthropic-ai/claude-agent-sdk");
  const sdkPackage = join(dirname(fileURLToPath(sdkUrl)), "package.json");
  writeFileSync(join(directory, "package.json"), JSON.stringify({ version: JSON.parse(readFileSync(sdkPackage, "utf8")).version }));
  const fixturePath = join(directory, "sdk-fixture.mjs");
  writeFileSync(fixturePath, `
    export * from ${JSON.stringify(sdkUrl)};
    import { appendFileSync } from "node:fs";
    const journal = ${JSON.stringify(journal)};
    const project = ${JSON.stringify(project)};
    let entries = [{ sessionId: "saved", summary: "Saved", lastModified: 2, cwd: project }];
    export async function listSessions() { return entries; }
    export async function tagSession(id, tag, options) {
      appendFileSync(journal, JSON.stringify({ op: "tag", id, tag, dir: options?.dir }) + "\\n");
      if (tag === "fail") throw new Error("tag refused");
      entries = entries.map((entry) => entry.sessionId === id ? { ...entry, tag: tag ?? undefined } : entry);
    }
    export async function deleteSession(id, options) {
      appendFileSync(journal, JSON.stringify({ op: "delete", id, dir: options?.dir }) + "\\n");
      entries = entries.filter((entry) => entry.sessionId !== id);
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
  const child = spawn(process.execPath, ["--import", pathToFileURL(loaderPath).href, bridgePath], { stdio: "pipe" });
  const lines = readline.createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  let stderr = "";
  child.stderr.on("data", (chunk: Buffer) => { stderr += chunk.toString(); });
  const timeout = setTimeout(() => child.kill(), 10_000);
  const send = (command: Record<string, unknown>) => child.stdin.write(`${JSON.stringify(command)}\n`);
  const result = async (requestId: string) => {
    for (;;) {
      const line = await lines.next();
      assert.equal(line.done, false, stderr);
      const event = JSON.parse(line.value) as Record<string, unknown>;
      if (event.event === "session_mutation_result" && event.request_id === requestId) return event;
    }
  };
  try {
    send({ command: "tag_session", session_id: "other", tag: "wrong", request_id: "unknown" });
    assert.match(String((await result("unknown")).error), /no longer in this project/);
    send({ command: "tag_session", session_id: "saved", tag: "work", request_id: "tag" });
    const tagged = await result("tag");
    assert.equal(tagged.error, undefined);
    assert.equal((tagged.sessions as Array<Record<string, unknown>>)[0]?.tag, "work");
    send({ command: "tag_session", session_id: "saved", tag: "fail", request_id: "fail" });
    assert.match(String((await result("fail")).error), /tag refused/);
    send({ command: "delete_session", session_id: "saved", request_id: "delete" });
    assert.deepEqual((await result("delete")).sessions, []);
    const writes = readFileSync(journal, "utf8").trim().split("\n").map((line) => JSON.parse(line) as Record<string, unknown>);
    assert.deepEqual(writes, [
      { op: "tag", id: "saved", tag: "work", dir: project },
      { op: "tag", id: "saved", tag: "fail", dir: project },
      { op: "delete", id: "saved", dir: project },
    ]);
  } finally {
    clearTimeout(timeout);
    child.stdin.end();
    child.kill();
    rmSync(directory, { recursive: true, force: true });
  }
});
