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
// widens the gate instead of failing, and says so in the table.
//
// Exit codes: 0 no regression, 1 a median regressed, 2 setup or run failure (including a
// scenario that only one ref reports). Both refs must contain the benchmark, because it is
// built from each ref's own sources.
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
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const BENCH_SOURCE = "src/app/terminal_runtime/chat_session/render_bench.rs";
const SPREAD_GATE_FACTOR = 2;

class BenchError extends Error {}

export function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length / 2) - 1];
}

/** `BENCH <size> <scenario> median_us=N p95_us=M frames=F` lines, keyed by "size scenario". */
export function parseBench(output) {
  const results = new Map();
  for (const line of output.split(/\r?\n/)) {
    const match = /^BENCH (\S+) (\S+) (.*)$/.exec(line);
    if (!match) {
      continue;
    }
    const fields = Object.fromEntries(match[3].split(" ").map((pair) => pair.split("=")));
    results.set(`${match[1]} ${match[2]}`, {
      median: Number(fields.median_us),
      p95: Number(fields.p95_us),
    });
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
 */
export function compare(groups, thresholdPercent) {
  assertSameScenarios(groups);
  const keys = [...groups.head[0].keys()];
  const across = (runs, key, field) => median(runs.map((run) => run.get(key)[field]));
  return keys.map((key) => {
    const baseRuns = [...groups.baseA, ...groups.baseB];
    const base = across(baseRuns, key, "median");
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
      regression: change > gate,
    };
  });
}

export function formatTable(rows) {
  const header = ["size", "scenario", "base_us", "head_us", "change", "spread", "gate", "base_p95", "head_p95"];
  const body = rows.map((row) => {
    const [size, scenario] = row.key.split(" ");
    return [
      size,
      scenario,
      String(row.base),
      String(row.head),
      `${row.change >= 0 ? "+" : ""}${row.change.toFixed(1)}%`,
      `${row.spread.toFixed(1)}%`,
      `${row.gate.toFixed(1)}%`,
      String(row.baseP95),
      `${row.headP95}${row.regression ? "  REGRESSION" : ""}`,
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

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 1 << 28, ...options });
  if (result.error) {
    throw new BenchError(`${command} could not run: ${result.error.message}`);
  }
  return result;
}

function git(repo, ...args) {
  return run("git", ["-C", repo, ...args], { stdio: ["ignore", "pipe", "pipe"] });
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

function main(argv) {
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
  const cleanup = () => {
    // Only the worktrees this run added; other worktrees of the repository are not touched.
    for (const dir of worktrees.splice(0)) {
      git(repo, "worktree", "remove", "--force", dir);
    }
    fs.rmSync(work, { recursive: true, force: true });
  };
  for (const signal of ["SIGINT", "SIGTERM"]) {
    process.once(signal, () => {
      cleanup();
      process.exit(2);
    });
  }

  try {
    for (const name of ["base", "head"]) {
      const dir = path.join(work, name);
      const added = git(repo, "worktree", "add", "--quiet", "--detach", dir, shas[name]);
      if (added.status !== 0) {
        throw new BenchError(`git worktree add failed for ${name}: ${added.stderr.trim()}`);
      }
      worktrees.push(dir);
    }

    // cargo reads the pinned toolchain from rust-toolchain.toml in each worktree.
    const cargoTest = (name, args, stdio) =>
      run("cargo", ["test", "--locked", "--release", "--lib", ...args], {
        cwd: path.join(work, name),
        env: { ...process.env, CARGO_TARGET_DIR: path.join(targetRoot, name) },
        stdio,
      });

    console.error(`bench-render: building base ${shas.base.slice(0, 12)} and head ${shas.head.slice(0, 12)}`);
    for (const name of ["base", "head"]) {
      if (cargoTest(name, ["--no-run"], "inherit").status !== 0) {
        throw new BenchError(`building ${name} failed`);
      }
    }

    const groups = { baseA: [], head: [], baseB: [] };
    const plan = { baseA: "base", head: "head", baseB: "base" };
    for (let index = 1; index <= runs; index++) {
      for (const [group, name] of Object.entries(plan)) {
        console.error(`bench-render: run ${index}/${runs} of ${group} (${name})`);
        const result = cargoTest(name, ["render_bench", "--", "--ignored", "--nocapture"], ["ignore", "pipe", "inherit"]);
        const parsed = parseBench(result.stdout ?? "");
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
      `\nbase-vs-base spread up to ${widest.toFixed(1)}% (${runs} runs per group); gate = max(${threshold}%, ${SPREAD_GATE_FACTOR} x spread)`,
    );
    if (rows.some((row) => row.regression)) {
      console.error("bench-render: a median regressed by more than its gate");
      return 1;
    }
    return 0;
  } finally {
    cleanup();
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    if (!(error instanceof BenchError)) {
      throw error;
    }
    console.error(`bench-render: ${error.message}`);
    process.exitCode = 2;
  }
}
