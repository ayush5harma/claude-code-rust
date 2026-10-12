#!/usr/bin/env node
// Compare per-frame render cost between two git refs.
//
//   node scripts/dev/bench-render.mjs <base-ref> [<head-ref>]    head defaults to HEAD
//
// Builds each ref in a temporary worktree and runs the ignored `render_bench` test (see
// src/app/terminal_runtime/chat_session/render_bench.rs) in release mode RUNS times per
// ref. Base runs twice as many times, in two interleaved groups, so the script can print
// how far two identical builds drift apart on this machine (the base-vs-base spread).
// A scenario regresses when the head median is more than the gate above the base median,
// where gate = max(THRESHOLD, 2 * spread), all in percent. A noisy machine therefore
// widens the gate instead of failing, and says so in the table. The gate has a ceiling:
// when 2 * spread exceeds NOISE_CEILING_FACTOR * THRESHOLD the run is too noisy to judge
// and fails instead of passing a large regression.
//
// Exit codes: 0 no regression, 1 a median regressed, 2 setup or run failure (including a
// scenario only one ref reports, malformed benchmark output, or too much noise), 130 or
// 143 after SIGINT or SIGTERM. Both refs must contain the benchmark, because it is built
// from each ref's own sources.
//
// Environment:
//   BENCH_RENDER_THRESHOLD   minimum gate in percent (default 10)
//   BENCH_RENDER_RUNS        runs per group (default 5); base runs 2x this, head 1x
//   BENCH_RENDER_TARGET_DIR  parent of the cargo target dirs <dir>/base and <dir>/head
//                            (default <repo>/target/bench-render). They persist so that
//                            dependencies compile once; they are separate because the two
//                            worktrees share one cargo unit hash, so in a shared target
//                            dir the head build would find the base build fresh and run
//                            base's binary.
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const BENCH_SOURCE = "src/app/terminal_runtime/chat_session/render_bench.rs";
const SPREAD_GATE_FACTOR = 2;
// A gate wider than this many thresholds means the measurement cannot support a verdict.
const NOISE_CEILING_FACTOR = 3;
const EXIT_BY_SIGNAL = { SIGINT: 130, SIGTERM: 143 };

export class BenchError extends Error {}

class Interrupted extends Error {
  constructor(code) {
    super(`interrupted (exit ${code})`);
    this.code = code;
  }
}

export function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length / 2) - 1];
}

/**
 * `BENCH <size> <scenario> median_us=N p95_us=M frames=F` lines, keyed by "size scenario".
 * All measurements must be positive integers. A missing, zero, or invalid number would
 * otherwise produce a meaningless percentage or silently pass a comparison.
 */
export function parseBench(output) {
  const results = new Map();
  for (const line of output.split(/\r?\n/)) {
    if (!line.startsWith("BENCH ")) {
      continue;
    }
    const match = /^BENCH (\S+) (\S+) median_us=(\d+) p95_us=(\d+) frames=(\d+)$/.exec(line);
    const medianUs = Number(match?.[3]);
    const p95Us = Number(match?.[4]);
    const frames = Number(match?.[5]);
    if (!match || ![medianUs, p95Us, frames].every((value) => Number.isSafeInteger(value) && value > 0)) {
      throw new BenchError(`malformed benchmark line: ${line}`);
    }
    const key = `${match[1]} ${match[2]}`;
    if (results.has(key)) throw new BenchError(`duplicate benchmark scenario: ${key}`);
    results.set(key, { median: medianUs, p95: p95Us });
  }
  return results;
}

/** Every group of runs must report the same scenarios; otherwise there is nothing to compare. */
export function assertSameScenarios(groups) {
  const keysOf = (runs) => new Set(runs.flatMap((run) => [...run.keys()]));
  const all = new Set(Object.values(groups).flatMap((runs) => [...keysOf(runs)]));
  const problems = [];
  for (const [name, runs] of Object.entries(groups)) {
    runs.forEach((run, index) => {
      const missing = [...all].filter((key) => !run.has(key));
      if (missing.length > 0) {
        problems.push(`${name} run ${index + 1} lacks: ${missing.join(", ")}`);
      }
    });
  }
  if (problems.length > 0) {
    throw new BenchError(`the refs do not report the same scenarios:\n  ${problems.join("\n  ")}`);
  }
}

/**
 * `groups` holds arrays of parsed runs: baseA and baseB (the same build, run twice) and head.
 * Returns one row per scenario with the medians, the base-vs-base spread, and the verdict.
 * A base median that is not positive cannot anchor a percentage and is an error.
 */
export function compare(groups, thresholdPercent) {
  assertSameScenarios(groups);
  const keys = [...groups.head[0].keys()];
  const across = (runs, key, field) => median(runs.map((run) => run.get(key)[field]));
  return keys.map((key) => {
    const baseRuns = [...groups.baseA, ...groups.baseB];
    const base = across(baseRuns, key, "median");
    if (!(base > 0)) {
      throw new BenchError(`${key}: base median is ${base} us, so no percentage can be computed`);
    }
    const head = across(groups.head, key, "median");
    const spread =
      (Math.abs(across(groups.baseA, key, "median") - across(groups.baseB, key, "median")) /
        base) *
      100;
    const gate = Math.max(thresholdPercent, SPREAD_GATE_FACTOR * spread);
    const change = ((head - base) / base) * 100;
    return {
      key,
      base,
      head,
      baseP95: across(baseRuns, key, "p95"),
      headP95: across(groups.head, key, "p95"),
      change,
      spread,
      gate,
      tooNoisy: gate > NOISE_CEILING_FACTOR * thresholdPercent,
      regression: change > gate,
    };
  });
}

/** Rows whose spread pushes the gate past the ceiling cannot be judged either way. */
export function assertQuiet(rows, thresholdPercent) {
  const noisy = rows.filter((row) => row.tooNoisy);
  if (noisy.length > 0) {
    const detail = noisy.map((row) => `${row.key} (spread ${row.spread.toFixed(1)}%)`).join(", ");
    throw new BenchError(
      `too noisy to judge: the base-vs-base spread needs a gate above ${NOISE_CEILING_FACTOR * thresholdPercent}%: ${detail}; rerun on a quieter machine`,
    );
  }
}

export function formatTable(rows) {
  const header = ["size", "scenario", "base_us", "head_us", "change", "spread", "gate", "base_p95", "head_p95"];
  const body = rows.map((row) => {
    const [size, scenario] = row.key.split(" ");
    const verdict = row.tooNoisy ? "  TOO NOISY" : row.regression ? "  REGRESSION" : "";
    return [
      size,
      scenario,
      String(row.base),
      String(row.head),
      `${row.change >= 0 ? "+" : ""}${row.change.toFixed(1)}%`,
      `${row.spread.toFixed(1)}%`,
      `${row.gate.toFixed(1)}%`,
      String(row.baseP95),
      `${row.headP95}${verdict}`,
    ];
  });
  const widths = header.map((cell, column) => Math.max(cell.length, ...body.map((row) => row[column].length)));
  const line = (cells) => cells.map((cell, column) => cell.padEnd(widths[column])).join("  ").trimEnd();
  return [line(header), ...body.map(line)].join("\n");
}

function number(name, fallback, { integer }) {
  const raw = process.env[name] ?? fallback;
  const value = Number(raw);
  if (!Number.isFinite(value) || value <= 0 || (integer && !Number.isInteger(value))) {
    throw new BenchError(`${name} must be a positive ${integer ? "integer" : "number"}, got ${raw}`);
  }
  return value;
}

function git(repo, ...args) {
  const result = spawnSync("git", ["-C", repo, ...args], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  if (result.error) {
    throw new BenchError(`git could not run: ${result.error.message}`);
  }
  return result;
}

function resolveCommit(repo, ref) {
  const result = git(repo, "rev-parse", "--verify", "--quiet", `${ref}^{commit}`);
  if (result.status !== 0) {
    throw new BenchError(`unknown ref: ${ref}`);
  }
  const sha = result.stdout.trim();
  if (git(repo, "cat-file", "-e", `${sha}:${BENCH_SOURCE}`).status !== 0) {
    throw new BenchError(
      `${sha.slice(0, 12)} has no render benchmark (${BENCH_SOURCE}); use refs that descend from the commit that added it`,
    );
  }
  return sha;
}

// The one child process that is running, so a signal can stop it. Children get their own
// process group on POSIX so the whole cargo/rustc/test tree can be stopped at once.
let runningChild = null;

function stopRunningChild(signal = "SIGTERM") {
  const child = runningChild;
  if (!child?.pid) {
    return;
  }
  if (process.platform === "win32") {
    spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    return;
  }
  try {
    process.kill(-child.pid, signal);
  } catch {
    child.kill(signal);
  }
}

function runAsync(command, args, { cwd, env, captureStdout, captureStderr = false } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env,
      detached: process.platform !== "win32",
      stdio: ["ignore", captureStdout ? "pipe" : "inherit", captureStderr ? "pipe" : "inherit"],
    });
    runningChild = child;
    let stdout = "";
    let stderr = "";
    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr?.setEncoding("utf8");
    child.stderr?.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (error) => {
      runningChild = null;
      reject(new BenchError(`${command} could not run: ${error.message}`));
    });
    child.on("close", (status) => {
      runningChild = null;
      resolve({ status, stdout, stderr });
    });
  });
}

function gitAsync(repo, ...args) {
  return runAsync("git", ["-C", repo, ...args], { captureStdout: true, captureStderr: true });
}

async function main(argv) {
  if (argv.length < 1 || argv.length > 2) {
    throw new BenchError("usage: node scripts/dev/bench-render.mjs <base-ref> [<head-ref>]");
  }
  const threshold = number("BENCH_RENDER_THRESHOLD", "10", { integer: false });
  const runs = number("BENCH_RENDER_RUNS", "5", { integer: true });

  // The repository this script belongs to, wherever it is invoked from.
  const here = path.dirname(fileURLToPath(import.meta.url));
  const top = git(here, "rev-parse", "--show-toplevel");
  if (top.status !== 0) {
    throw new BenchError("not inside a git repository");
  }
  const repo = path.resolve(top.stdout.trim());
  const shas = { base: resolveCommit(repo, argv[0]), head: resolveCommit(repo, argv[1] ?? "HEAD") };
  const targetRoot = path.resolve(process.env.BENCH_RENDER_TARGET_DIR ?? path.join(repo, "target", "bench-render"));

  const work = fs.mkdtempSync(path.join(os.tmpdir(), "bench-render-"));
  const worktrees = [];
  const cleanup = async () => {
    // Only the worktrees this run added; other worktrees of the repository are not touched.
    for (const dir of worktrees.splice(0)) {
      const removed = await gitAsync(repo, "worktree", "remove", "--force", dir);
      if (removed.status !== 0) {
        console.error(
          `bench-render: could not remove worktree ${dir} (${removed.stderr.trim()}); delete it by hand, then run "git worktree prune" if git still lists it`,
        );
      }
    }
    fs.rmSync(work, { recursive: true, force: true });
  };

  // A signal stops the running child; the awaiting code then sees `interrupted` and unwinds
  // through the finally below, which cleans up. A second signal kills the child tree.
  let interrupted = 0;
  let cleaningUp = false;
  const onSignal = (signal) => {
    if (interrupted) {
      if (!cleaningUp) stopRunningChild("SIGKILL");
      return;
    }
    interrupted = EXIT_BY_SIGNAL[signal];
    if (!cleaningUp) stopRunningChild();
  };
  const signals = Object.keys(EXIT_BY_SIGNAL);
  const handlers = new Map(signals.map((signal) => [signal, () => onSignal(signal)]));
  for (const [signal, handler] of handlers) process.on(signal, handler);
  const checkpoint = () => {
    if (interrupted) {
      throw new Interrupted(interrupted);
    }
  };

  try {
    for (const name of ["base", "head"]) {
      checkpoint();
      const dir = path.join(work, name);
      // Git can register the worktree before the add command exits. Track the attempt so
      // interruption in that interval still removes its registration.
      worktrees.push(dir);
      const added = await gitAsync(repo, "worktree", "add", "--quiet", "--detach", dir, shas[name]);
      checkpoint();
      if (added.status !== 0) {
        throw new BenchError(`git worktree add failed for ${name}: ${added.stderr.trim()}`);
      }
    }

    // cargo reads the pinned toolchain from rust-toolchain.toml in each worktree.
    const cargoTest = async (name, args, captureStdout) => {
      checkpoint();
      const result = await runAsync("cargo", ["test", "--locked", "--release", "--lib", ...args], {
        cwd: path.join(work, name),
        env: { ...process.env, CARGO_TARGET_DIR: path.join(targetRoot, name) },
        captureStdout,
      });
      checkpoint();
      return result;
    };

    console.error(`bench-render: building base ${shas.base.slice(0, 12)} and head ${shas.head.slice(0, 12)}`);
    for (const name of ["base", "head"]) {
      if ((await cargoTest(name, ["--no-run"], false)).status !== 0) {
        throw new BenchError(`building ${name} failed`);
      }
    }

    const groups = { baseA: [], head: [], baseB: [] };
    const plan = { baseA: "base", head: "head", baseB: "base" };
    for (let index = 1; index <= runs; index++) {
      for (const [group, name] of Object.entries(plan)) {
        console.error(`bench-render: run ${index}/${runs} of ${group} (${name})`);
        const result = await cargoTest(name, ["render_bench", "--", "--ignored", "--nocapture"], true);
        const parsed = parseBench(result.stdout);
        if (result.status !== 0 || parsed.size === 0) {
          throw new BenchError(`${group} run ${index} failed or printed no BENCH lines`);
        }
        groups[group].push(parsed);
      }
    }

    const rows = compare(groups, threshold);
    console.log(formatTable(rows));
    const widest = Math.max(...rows.map((row) => row.spread));
    console.log(
      `\nbase-vs-base spread up to ${widest.toFixed(1)}% (${runs} runs per group); gate = max(${threshold}%, ${SPREAD_GATE_FACTOR} x spread), judged only up to ${NOISE_CEILING_FACTOR * threshold}%`,
    );
    assertQuiet(rows, threshold);
    if (rows.some((row) => row.regression)) {
      console.error("bench-render: a median regressed by more than its gate");
      return 1;
    }
    return 0;
  } finally {
    cleaningUp = true;
    try {
      await cleanup();
    } finally {
      for (const [signal, handler] of handlers) process.off(signal, handler);
    }
    checkpoint();
  }
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url))) {
  try {
    process.exitCode = await main(process.argv.slice(2));
  } catch (error) {
    if (error instanceof Interrupted) {
      console.error(`bench-render: ${error.message}`);
      process.exitCode = error.code;
    } else if (error instanceof BenchError) {
      console.error(`bench-render: ${error.message}`);
      process.exitCode = 2;
    } else {
      throw error;
    }
  }
}
