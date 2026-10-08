import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const eventsModule = join(dirname(fileURLToPath(import.meta.url)), "events.js");

// The event that killed sessions at start was the slash-command list: 86-131 KB
// with a few hundred skills installed, against a 64 KB pipe buffer. The reader
// waits before draining so the writer meets a full pipe mid-event.
test("an event larger than the pipe buffer reaches a slow reader whole", async () => {
  const description = "x".repeat(200 * 1024);
  const script = `
    import { writeEvent } from ${JSON.stringify(eventsModule)};
    writeEvent({ event: "slash_error", session_id: "s", message: ${JSON.stringify(description)} });
  `;
  const child = spawn(process.execPath, ["--input-type=module", "-e", script], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.pause();
  let stderr = "";
  child.stderr.on("data", (chunk: Buffer) => {
    stderr += chunk.toString();
  });
  await new Promise((resolve) => setTimeout(resolve, 500));
  const chunks: Buffer[] = [];
  child.stdout.on("data", (chunk: Buffer) => chunks.push(chunk));
  child.stdout.resume();
  const code = await new Promise<number | null>((resolve) => child.on("close", resolve));

  assert.equal(code, 0, stderr);
  const lines = Buffer.concat(chunks).toString().split("\n").filter((line) => line.length > 0);
  assert.equal(lines.length, 1);
  const envelope = JSON.parse(lines[0] ?? "") as { message?: string };
  assert.equal(envelope.message, description);
});
