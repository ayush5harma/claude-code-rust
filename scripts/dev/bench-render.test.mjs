import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { assertQuiet, assertSameScenarios, BenchError, compare, median, parseBench } from "./bench-render.mjs";

const run = (entries) => new Map(Object.entries(entries).map(([key, value]) => [key, { median: value, p95: value * 2 }]));
const KEY = "120x40 streaming";

test("parseBench keeps only BENCH lines and reads median and p95", () => {
  const parsed = parseBench(
    ["running 1 test", "BENCH 120x40 streaming median_us=152 p95_us=401 frames=2000", "BENCH 200x60 scroll median_us=7 p95_us=9 frames=3", "ok"].join("\r\n"),
  );
  assert.deepEqual([...parsed], [
    ["120x40 streaming", { median: 152, p95: 401 }],
    ["200x60 scroll", { median: 7, p95: 9 }],
  ]);
});

test("median of an even count takes the lower middle value", () => {
  assert.equal(median([4, 1, 3, 2]), 2);
  assert.equal(median([5]), 5);
});

test("a quiet machine gates at the threshold and flags only a change beyond it", () => {
  const groups = (head) => ({
    baseA: [run({ [KEY]: 100 }), run({ [KEY]: 100 })],
    baseB: [run({ [KEY]: 100 }), run({ [KEY]: 100 })],
    head: [run({ [KEY]: head }), run({ [KEY]: head })],
  });
  const [steady] = compare(groups(109), 10);
  assert.equal(steady.gate, 10);
  assert.equal(steady.regression, false);
  const [slow] = compare(groups(111), 10);
  assert.equal(slow.regression, true);
  assert.equal(Math.round(slow.change), 11);
});

test("two identical builds that drift apart widen the gate to twice the spread", () => {
  // Base medians 100 and 120 (spread 20/100 = 20%), the case where a fixed 10% gate fails a
  // build that did not change.
  const groups = {
    baseA: [run({ [KEY]: 100 })],
    baseB: [run({ [KEY]: 120 })],
    head: [run({ [KEY]: 118 })],
  };
  const [row] = compare(groups, 10);
  assert.equal(row.spread, 20);
  assert.equal(row.gate, 40);
  assert.equal(row.regression, false);
});

test("a scenario that only one side reports is an error, not a zero change", () => {
  const groups = {
    baseA: [run({ [KEY]: 100 })],
    baseB: [run({ [KEY]: 100 })],
    head: [run({ [KEY]: 100, "120x40 scroll": 50 })],
  };
  assert.throws(() => compare(groups, 10), /do not report the same scenarios[\s\S]*baseA run 1 lacks: 120x40 scroll/);
  assert.throws(() => assertSameScenarios({ head: [run({ a: 1 }), run({ b: 1 })] }), /head run 1 lacks: b/);
});

test("a scenario that only the base reports is an error too", () => {
  const groups = {
    baseA: [run({ [KEY]: 100, "120x40 scroll": 50 })],
    baseB: [run({ [KEY]: 100, "120x40 scroll": 50 })],
    head: [run({ [KEY]: 100 })],
  };
  assert.throws(() => compare(groups, 10), /head run 1 lacks: 120x40 scroll/);
});

test("benchmark lines with missing or non-numeric fields are errors, not NaN rows", () => {
  for (const line of [
    "BENCH 120x40 streaming p95_us=401 frames=2000",
    "BENCH 120x40 streaming median_us=152 frames=2000",
    "BENCH 120x40 streaming median_us=abc p95_us=401 frames=2000",
    "BENCH 120x40 streaming median_us=NaN p95_us=401 frames=2000",
    "BENCH 120x40 streaming median_us=152 p95_us=Infinity frames=2000",
    "BENCH 120x40 streaming median_us=0 p95_us=401 frames=2000",
    "BENCH 120x40 streaming median_us=152 p95_us=0 frames=2000",
    "BENCH 120x40 streaming median_us=152 p95_us=401 frames=0",
    "BENCH 120x40 streaming median_us=152 p95_us=401",
    "BENCH 120x40",
  ]) {
    assert.throws(() => parseBench(line), BenchError, line);
    assert.throws(() => parseBench(`ok\n${line}\n`), /malformed benchmark line/, line);
  }
});

test("duplicate scenario output is rejected rather than silently replaced", () => {
  const line = "BENCH 120x40 streaming median_us=152 p95_us=401 frames=2000";
  assert.throws(() => parseBench(`${line}\n${line}`), /duplicate benchmark scenario: 120x40 streaming/);
});

test("a zero base median cannot anchor a percentage", () => {
  const groups = {
    baseA: [run({ [KEY]: 0 })],
    baseB: [run({ [KEY]: 0 })],
    head: [run({ [KEY]: 5 })],
  };
  assert.throws(() => compare(groups, 10), /120x40 streaming: base median is 0 us/);
});

test("a noisy base is too noisy to judge instead of passing a large regression", () => {
  // Base medians 100 and 150: spread 50/100 = 50%, so the uncapped gate would be 100% and a
  // head of 190 (+90%) would pass.
  const groups = {
    baseA: [run({ [KEY]: 100 })],
    baseB: [run({ [KEY]: 150 })],
    head: [run({ [KEY]: 190 })],
  };
  const rows = compare(groups, 10);
  assert.equal(rows[0].gate, 100);
  assert.equal(rows[0].regression, false);
  assert.equal(rows[0].tooNoisy, true);
  assert.throws(() => assertQuiet(rows, 10), /too noisy to judge[\s\S]*120x40 streaming \(spread 50\.0%\)/);
});

test("the ceiling is three thresholds: a gate of exactly 30% is still judged", () => {
  // Base medians 100 and 115: spread 15%, gate 30%.
  const groups = {
    baseA: [run({ [KEY]: 100 })],
    baseB: [run({ [KEY]: 115 })],
    head: [run({ [KEY]: 140 })],
  };
  const rows = compare(groups, 10);
  assert.equal(rows[0].gate, 30);
  assert.equal(rows[0].tooNoisy, false);
  assert.equal(rows[0].regression, true);
  assert.doesNotThrow(() => assertQuiet(rows, 10));
});

async function assertSignalCleanup(signal, exitCode, phase = "build") {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "bench-render-signal-test-"));
  const bin = path.join(temp, "bin");
  const marker = path.join(temp, "cargo.json");
  fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, "cargo"), `#!/usr/bin/env node
const fs = require("node:fs");
fs.writeFileSync(process.env.BENCH_RENDER_TEST_MARKER, JSON.stringify({ cwd: process.cwd(), pid: process.pid }));
setInterval(() => {}, 1000);
`, { mode: 0o755 });
  let realGit;
  if (phase === "worktree add") {
    realGit = process.env.PATH.split(path.delimiter).map((dir) => path.join(dir, "git")).find((candidate) => fs.existsSync(candidate));
    assert.ok(realGit, "Git should be available on PATH");
    fs.writeFileSync(path.join(bin, "git"), `#!/usr/bin/env node
const fs = require("node:fs");
const { spawnSync } = require("node:child_process");
const args = process.argv.slice(2);
const result = spawnSync(process.env.BENCH_RENDER_TEST_REAL_GIT, args, { stdio: "inherit" });
if (result.error) throw result.error;
if (args.includes("worktree") && args.includes("add") && result.status === 0) {
  fs.writeFileSync(process.env.BENCH_RENDER_TEST_MARKER, JSON.stringify({ worktree: args.at(-2) }));
  setInterval(() => {}, 1000);
} else {
  process.exit(result.status);
}
`, { mode: 0o755 });
  }

  const script = fileURLToPath(new URL("./bench-render.mjs", import.meta.url));
  const repo = path.join(temp, "repo");
  const copiedScript = path.join(repo, "scripts", "dev", "bench-render.mjs");
  fs.mkdirSync(path.dirname(copiedScript), { recursive: true });
  fs.copyFileSync(script, copiedScript);
  const benchSource = path.join(repo, "src", "app", "terminal_runtime", "chat_session", "render_bench.rs");
  fs.mkdirSync(path.dirname(benchSource), { recursive: true });
  fs.writeFileSync(benchSource, "// benchmark fixture\n");
  execFileSync("git", ["init", "--quiet", repo]);
  execFileSync("git", ["-C", repo, "add", "."]);
  execFileSync("git", ["-C", repo, "-c", "user.name=Benchmark Test", "-c", "user.email=benchmark@example.invalid", "commit", "--quiet", "-m", "test fixture"]);
  const child = spawn(process.execPath, [copiedScript, "HEAD", "HEAD"], {
    env: { ...process.env, PATH: `${bin}${path.delimiter}${process.env.PATH}`, BENCH_RENDER_TEST_MARKER: marker, ...(realGit ? { BENCH_RENDER_TEST_REAL_GIT: realGit } : {}) },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const closed = new Promise((resolve) => child.on("close", (code, signal) => resolve({ code, signal })));
  let worktree;
  try {
    const deadline = Date.now() + 5000;
    while (!fs.existsSync(marker) && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    assert.ok(fs.existsSync(marker), `the fake ${phase} process should start: exit=${child.exitCode}, stderr=${stderr}`);
    const marked = JSON.parse(fs.readFileSync(marker, "utf8"));
    worktree = marked.worktree ?? marked.cwd;
    assert.ok(fs.existsSync(worktree), `the benchmark worktree should exist during ${phase}`);
    child.kill(signal);
    assert.deepEqual(await closed, { code: exitCode, signal: null });
    assert.equal(fs.existsSync(worktree), false, "the worktree directory should be removed");
    assert.equal(fs.existsSync(path.dirname(worktree)), false, "the temporary parent directory should be removed");
    const listed = execFileSync("git", ["-C", repo, "worktree", "list", "--porcelain"], { encoding: "utf8" });
    assert.ok(!listed.includes(worktree), "Git should not retain a stale worktree entry");
  } finally {
    if (child.exitCode === null) child.kill("SIGKILL");
    await closed;
    fs.rmSync(temp, { recursive: true, force: true });
  }
}

for (const [signal, exitCode] of [["SIGINT", 130], ["SIGTERM", 143]]) {
  test(`${signal} stops the running build and removes its temporary worktree`, { skip: process.platform === "win32", timeout: 10000 }, () => assertSignalCleanup(signal, exitCode));
}

test("SIGTERM during worktree add removes its Git registration", { skip: process.platform === "win32", timeout: 10000 }, () => assertSignalCleanup("SIGTERM", 143, "worktree add"));
