import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const eventsModule = join(dirname(fileURLToPath(import.meta.url)), "events.js");

// The event that killed sessions at start was the slash-command list: 86-131 KB
// with a few hundred skills installed, against a 64 KB pipe buffer. The reader
// waits before draining so the writer meets a full pipe mid-event.
test("an event larger than the pipe buffer reaches a slow reader whole", { timeout: 10_000 }, async (t) => {
  const description = "x".repeat(2 * 1024 * 1024);
  const script = `
    import { writeEvent } from ${JSON.stringify(eventsModule)};
    writeEvent({ event: "slash_error", session_id: "s", message: "x".repeat(2 * 1024 * 1024) });
  `;
  const child = spawn(process.execPath, ["--input-type=module", "-e", script], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  t.after(() => {
    if (child.exitCode === null) child.kill();
  });
  const closed = new Promise<number | null>((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  // Node auto-resumed paused stdout on early exit and lost a late close listener
  // in a 1 KB reproduction (2026-10-09); install both listeners before waiting.
  const chunks: Buffer[] = [];
  child.stdout.on("data", (chunk: Buffer) => chunks.push(chunk));
  child.stdout.pause();
  let stderr = "";
  child.stderr.on("data", (chunk: Buffer) => {
    stderr += chunk.toString();
  });
  const [code, exitCodeBeforeResume] = await Promise.all([
    closed,
    (async () => {
      await new Promise((resolve) => setTimeout(resolve, 500));
      const exitCode = child.exitCode;
      child.stdout.resume();
      return exitCode;
    })(),
  ]);

  assert.equal(exitCodeBeforeResume, null, "the writer must remain blocked while stdout is paused");
  assert.equal(code, 0, stderr);
  const lines = Buffer.concat(chunks).toString().split("\n").filter((line) => line.length > 0);
  assert.equal(lines.length, 1);
  const envelope = JSON.parse(lines[0] ?? "") as { message?: string };
  assert.equal(envelope.message, description);
});
