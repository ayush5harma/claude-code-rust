import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import readline from "node:readline";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

type Envelope = Record<string, unknown>;

const SESSION_ID = "11111111-1111-4111-8111-111111111111";
const CLEARED_SESSION_ID = "22222222-2222-4222-8222-222222222222";

/**
 * A stand-in for the SDK. Its session API reports titles from its own state,
 * which its query changes the way Claude Code 2.1.288 does, as measured: a
 * launch-time name and a /rename show only once their turn's result is in, and
 * /clear starts a new session id that reports the old title. Every prompt is a
 * local command answered with one assistant message and the result. Prompts
 * prefixed `/test-` steer the session API: fail the next read, hold the next
 * read until a later one starts, or reset the conversation.
 */
function writeSdkFixture(fixturePath: string, cwd: string): void {
  const sdkUrl = import.meta.resolve("@anthropic-ai/claude-agent-sdk");
  writeFileSync(fixturePath, `
    export * from ${JSON.stringify(sdkUrl)};
    import { appendFileSync } from "node:fs";
    const cwd = ${JSON.stringify(cwd)};
    const titles = new Map([[${JSON.stringify(SESSION_ID)}, "Saved name"]]);
    let failNextRead = false;
    let holdNextRead = false;
    let releaseHeldRead;
    export async function getSessionInfo(sessionId) {
      const release = releaseHeldRead;
      releaseHeldRead = undefined;
      release?.();
      if (failNextRead) {
        failNextRead = false;
        throw new Error("session info unavailable");
      }
      if (holdNextRead) {
        holdNextRead = false;
        return new Promise((resolve) => {
          releaseHeldRead = () => resolve({ sessionId, summary: "Stale read", lastModified: 1, customTitle: "Stale read" });
        });
      }
      const title = titles.get(sessionId);
      return title === undefined ? undefined : { sessionId, summary: title, lastModified: 1, customTitle: title, cwd };
    }
    export async function renameSession(sessionId, title) {
      titles.set(sessionId, title);
    }
    export async function importSessionToStore() {}
    export async function getSessionMessages() {
      return [];
    }
    export async function listSessions() {
      return [{ sessionId: ${JSON.stringify(SESSION_ID)}, cwd, lastModified: 1 }];
    }
    export function query({ prompt, options }) {
      appendFileSync(process.env.QUERY_JOURNAL, JSON.stringify({ resume: options.resume }) + "\\n");
      const input = prompt[Symbol.asyncIterator]();
      const pending = [];
      let sessionId = options.resume;
      let replies = 0;
      return {
        [Symbol.asyncIterator]() { return this; },
        async next() {
          if (pending.length === 0) {
            const next = await input.next();
            if (next.done) return { done: true, value: undefined };
            const content = next.value.message.content;
            const text = typeof content === "string" ? content : content.map((block) => block.text ?? "").join("");
            let reply = "";
            replies += 1;
            if (text.startsWith("/rename ")) {
              titles.set(sessionId, text.slice("/rename ".length));
              reply = "Session renamed to: " + titles.get(sessionId);
            } else if (text === "first turn") {
              titles.set(sessionId, "Launch name");
              reply = "ok";
            } else if (text === "/clear") {
              const title = titles.get(sessionId);
              sessionId = ${JSON.stringify(CLEARED_SESSION_ID)};
              titles.set(sessionId, title);
              pending.push({ type: "system", subtype: "init", session_id: sessionId, uuid: "init-" + replies });
            } else if (text === "/test-fail-read") {
              failNextRead = true;
            } else if (text === "/test-hold-read") {
              holdNextRead = true;
            } else if (text === "/test-reset-conversation") {
              pending.push({ type: "conversation_reset", session_id: sessionId, uuid: "reset-" + replies, new_conversation_id: "conversation-" + replies });
            } else {
              reply = "Unknown command: " + text;
            }
            if (reply) {
              pending.push({ type: "assistant", uuid: "reply-" + replies, session_id: sessionId, parent_tool_use_id: null,
                message: { id: "local-" + replies, role: "assistant", content: [{ type: "text", text: reply }] } });
            }
            pending.push({ type: "result", subtype: "success", uuid: "result-" + replies, session_id: sessionId, parent_tool_use_id: null,
              result: reply, user_message_uuid: next.value.uuid, user_message_uuids: [next.value.uuid] });
          }
          return { done: false, value: pending.shift() };
        },
        close() {},
        async generateSessionTitle() {
          titles.set(sessionId, "Generated name");
          return "Generated name";
        },
        async initializationResult() { return { current_permission_mode: "default", models: [], commands: [], agents: [], account: { apiKeySource: "fixture" }, fast_mode_state: "off" }; },
        async getSettings() { return { applied: { model: "opus", effort: "high", ultracodeAvailable: false, ultracodeRequested: false, ultracode: false } }; },
        async supportedCommands() { return []; },
      };
    }
  `);
}

const isTitleUpdate = (event: Envelope) =>
  event.event === "session_update" && (event.update as Envelope)?.type === "session_title_update";
const titleOf = (event: Envelope) => (event.update as Envelope).title;

test("the bridge sends the session API's title through resume, turns, renames, resets and /clear", async () => {
  for (const command of ["resume_session", "create_session"] as const) {
    const directory = realpathSync(mkdtempSync(join(tmpdir(), "bridge-title-")));
    const cwd = join(directory, "project");
    mkdirSync(cwd);
    const journal = join(directory, "query.jsonl");
    const fixturePath = join(directory, "sdk-fixture.mjs");
    // The bridge checks the SDK version beside the module it resolves, which
    // is the stand-in here, so it gets the installed SDK's version.
    const sdkPackage = join(dirname(fileURLToPath(import.meta.resolve("@anthropic-ai/claude-agent-sdk"))), "package.json");
    writeFileSync(join(directory, "package.json"), JSON.stringify({ version: JSON.parse(readFileSync(sdkPackage, "utf8")).version }));
    writeSdkFixture(fixturePath, cwd);
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
      env: {
        ...process.env,
        CLAUDE_CONFIG_DIR: join(directory, "config"),
        CLAUDE_CODE_PROJECT_DIR_NAME: undefined,
        CLAUDE_RS_BRIDGE_DIAGNOSTICS: "1",
        QUERY_JOURNAL: journal,
        CLAUDE_CODE_EXECUTABLE: "",
      },
      stdio: "pipe",
    });
    const output = readline.createInterface({ input: child.stdout });
    const diagnostics = readline.createInterface({ input: child.stderr });
    const events: Envelope[] = [];
    const stderrLines: string[] = [];
    let onStderr = () => {};
    diagnostics.on("line", (line) => {
      stderrLines.push(line);
      onStderr();
    });
    const timeout = setTimeout(() => child.kill(), 10_000);
    const send = (message: Envelope) => child.stdin.write(`${JSON.stringify(message)}\n`);
    const prompt = (uuid: string, text: string) =>
      send({ command: "prompt", session_id: SESSION_ID, message_uuid: uuid, chunks: [{ kind: "text", value: text }] });
    const lines = output[Symbol.asyncIterator]();
    const readUntil = async (done: (event: Envelope) => boolean) => {
      for (;;) {
        const next = await lines.next();
        assert.ok(!next.done, `${command}: bridge closed: ${JSON.stringify(events)} ${stderrLines.join("\n")}`);
        const event = JSON.parse(next.value) as Envelope;
        events.push(event);
        if (done(event)) return;
      }
    };
    const titleUpdate = (sessionId: string, title: string) => (event: Envelope) =>
      isTitleUpdate(event) && event.session_id === sessionId && titleOf(event) === title;
    try {
      send(command === "resume_session"
        ? { command, session_id: SESSION_ID, request_id: "title-resume", launch_settings: {} }
        : { command, cwd, resume: SESSION_ID, request_id: "title-resume", launch_settings: {} });
      await readUntil(titleUpdate(SESSION_ID, "Saved name"));
      assert.ok(events.some((event) => event.event === "connected"), `${command}: ${JSON.stringify(events)}`);
      assert.equal((JSON.parse(readFileSync(journal, "utf8").trim()) as Envelope).resume, SESSION_ID);

      // A failed read is logged, and nothing is sent for it.
      const beforeFailure = events.length;
      const failureLogged = new Promise<void>((resolve) => {
        onStderr = () => {
          if (stderrLines.some((line) => line.includes('"event_name":"session_title_read_failed"'))) resolve();
        };
      });
      prompt("prompt-fail", "/test-fail-read");
      await failureLogged;
      await readUntil((event) => event.event === "turn_complete");
      assert.deepEqual(events.slice(beforeFailure).filter(isTitleUpdate), [], command);

      // A launch-time name shows once the first turn's result is in.
      prompt("prompt-first", "first turn");
      await readUntil(titleUpdate(SESSION_ID, "Launch name"));

      // A read that a later one overtakes is dropped: the held read resolves
      // with a stale title only once the /rename turn's read has started.
      prompt("prompt-hold", "/test-hold-read");
      await readUntil((event) => event.event === "turn_complete");
      prompt("prompt-rename", "/rename Fresh name");
      await readUntil(titleUpdate(SESSION_ID, "Fresh name"));

      // The Status tab's rename and title generation.
      send({ command: "rename_session", session_id: SESSION_ID, title: "Status name", request_id: "status-rename" });
      await readUntil(titleUpdate(SESSION_ID, "Status name"));
      send({ command: "generate_session_title", session_id: SESSION_ID, description: "hello", request_id: "status-generate" });
      await readUntil(titleUpdate(SESSION_ID, "Generated name"));

      // The app drops the title with the conversation, so it is sent again.
      prompt("prompt-reset", "/test-reset-conversation");
      await readUntil((event) => event.event === "session_update" && (event.update as Envelope).type === "conversation_reset");
      await readUntil(titleUpdate(SESSION_ID, "Generated name"));

      // /clear replaces the session id; the new id's title is sent.
      prompt("prompt-clear", "/clear");
      await readUntil(titleUpdate(CLEARED_SESSION_ID, "Generated name"));
      assert.ok(events.some((event) => event.event === "session_replaced" && event.session_id === CLEARED_SESSION_ID));

      const shown = events
        .filter(isTitleUpdate)
        .map((event) => [event.session_id, titleOf(event)])
        .filter((entry, index, all) => index === 0 || JSON.stringify(entry) !== JSON.stringify(all[index - 1]));
      assert.deepEqual(
        shown,
        [
          [SESSION_ID, "Saved name"],
          [SESSION_ID, "Launch name"],
          [SESSION_ID, "Fresh name"],
          [SESSION_ID, "Status name"],
          [SESSION_ID, "Generated name"],
          [CLEARED_SESSION_ID, "Generated name"],
        ],
        command,
      );
    } finally {
      clearTimeout(timeout);
      output.close();
      diagnostics.close();
      if (child.exitCode === null && child.signalCode === null) {
        child.kill();
        await new Promise<void>((resolve) => child.once("exit", () => resolve()));
      }
      rmSync(directory, { recursive: true, force: true });
    }
  }
});
