// SPDX-License-Identifier: Apache-2.0
// A fixed-size history survives main-session compaction; SDK replay parsed all
// 400 generated records without a model call on 2026-10-09.
import { randomUUID } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [configDir, work, stockVersion = "2.1.293"] = process.argv.slice(2);
if (!configDir || !work) throw new Error("expected isolated config and work paths");
const cwd = join(work, "perf-resume-synthetic");
const projectDir = join(configDir, "projects", cwd.replace(/[^a-zA-Z0-9]/g, "-"));
mkdirSync(cwd, { recursive: true });
mkdirSync(projectDir, { recursive: true });

const sessionId = randomUUID();
const marker = `PARITY_SYNTHETIC_END_${sessionId.replaceAll("-", "")}`;
const common = {
  sessionId,
  cwd,
  isSidechain: false,
  userType: "external",
  entrypoint: "sdk-ts",
  version: stockVersion,
  gitBranch: "HEAD",
};
const records = [];
let parentUuid = null;
function append(type, message, index) {
  const uuid = randomUUID();
  records.push({
    ...common,
    uuid,
    parentUuid,
    type,
    message,
    timestamp: new Date(Date.UTC(2026, 9, 9, 0, 0, index)).toISOString(),
  });
  parentUuid = uuid;
}

for (let pair = 1; pair <= 200; pair++) {
  const label = String(pair).padStart(3, "0");
  append("user", {
    role: "user",
    content: [{ type: "text", text: `Synthetic parity prompt ${label}: summarize the fixed sample text.` }],
  }, 2 * pair - 2);
  append("assistant", {
    id: randomUUID(),
    role: "assistant",
    model: "claude-haiku-4-5-20251001",
    content: [{
      type: "text",
      text: pair === 200
        ? `Synthetic parity response ${label}. ${marker}`
        : `Synthetic parity response ${label}. ${"The quick brown fox crosses the measured terminal viewport. ".repeat(3)}`,
    }],
  }, 2 * pair - 1);
}

const transcript = `${records.map((record) => JSON.stringify(record)).join("\n")}\n`;
writeFileSync(join(work, "perf-resume-synthetic.source.jsonl"), transcript);
writeFileSync(join(projectDir, `${sessionId}.jsonl`), transcript);
process.stdout.write(`${sessionId}\t${cwd}\t${marker}\t${records.length}\n`);
