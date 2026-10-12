#!/usr/bin/env node
// Reproduce the PR-gating CI checks of .github/workflows/pr.yml (rustfmt,
// clippy, test, lockfile, terminal-tests, cargo-deny-policy, duplicate-code)
// and _agent-sdk.yml locally and print one PASS/FAIL table at the end. CI runs
// several of these jobs in parallel; here they run one after another. A failing step does not stop the run, so the table is always
// complete.
//
// Not covered: the cargo-deny advisories job (only with --audit), the MSRV
// check, the system clipboard paste regression (it overwrites your clipboard),
// the npm package layout job and the installer syntax check. The last three
// run in CI only on the paths that change them or on specific runners.
//
// Rust steps use RUSTFLAGS=-Dwarnings like pr.yml, in their own target
// directory ($CARGO_TARGET_DIR or ./target, plus /check-all) so they do not
// invalidate your normal builds. The first run compiles from scratch.
//
// Every process is started with shell: false, so the script behaves the same
// on Windows, macOS and Linux.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { resolveRepoRoot } from "../shared/repo-root.mjs";

const USAGE = `Usage: node scripts/dev/check-all.mjs [--rust] [--bridge] [--quick] [--audit]
  --rust    only the Rust checks
  --bridge  only the agent-sdk (bridge) checks
  --quick   skip the serial terminal-tests re-run, the fake-bridge syntax check and the audits
  --audit   also run the npm audit and cargo-deny advisories (need network)
With neither --rust nor --bridge, both groups run.`;

const LOG_TAIL_LINES = 40;

function parseArgs(argv) {
  const options = { rust: false, bridge: false, quick: false, audit: false };
  for (const arg of argv) {
    switch (arg) {
      case "--rust":
        options.rust = true;
        break;
      case "--bridge":
        options.bridge = true;
        break;
      case "--quick":
        options.quick = true;
        break;
      case "--audit":
        options.audit = true;
        break;
      case "-h":
      case "--help":
        console.log(USAGE);
        process.exit(0);
        break;
      default:
        console.error(`check-all: unknown argument: ${arg}\n${USAGE}`);
        process.exit(2);
    }
  }
  if (!options.rust && !options.bridge) {
    options.rust = true;
    options.bridge = true;
  }
  return options;
}

const options = parseArgs(process.argv.slice(2));
const repoRoot = resolveRepoRoot(import.meta.url);
const logDir = fs.mkdtempSync(path.join(os.tmpdir(), "check-all-"));
const results = [];
let stepIndex = 0;

// CI runs with a clean environment. With the update check disabled, the
// app::update_check tests fail (measured: 2 lib tests), so do not inherit it.
const baseEnv = { ...process.env };
delete baseEnv.CLAUDE_RUST_NO_UPDATE_CHECK;

const rustTargetDir = path.join(path.resolve(repoRoot, process.env.CARGO_TARGET_DIR ?? "target"), "check-all");
const rustEnv = { RUSTFLAGS: "-Dwarnings", CARGO_TARGET_DIR: rustTargetDir };

// npm is npm.cmd on Windows, which Node refuses to spawn without a shell.
// Run npm's own entry script with this node instead.
function resolveNpmCommand() {
  if (process.platform !== "win32") {
    return { command: "npm", prefix: [] };
  }
  const candidates = [
    process.env.npm_execpath,
    path.join(path.dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js"),
  ];
  const cli = candidates.find((candidate) => candidate && candidate.endsWith("npm-cli.js") && fs.existsSync(candidate));
  if (!cli) {
    throw new Error("Could not locate npm-cli.js next to node.exe; run this script through npm or set npm_execpath.");
  }
  return { command: process.execPath, prefix: [cli] };
}

let npmCommand;
function npm(...args) {
  npmCommand ??= resolveNpmCommand();
  return { command: npmCommand.command, args: [...npmCommand.prefix, ...args] };
}

const cargo = (args, env = {}) => ({ command: "cargo", args, env });
const node = (args, env = {}) => ({ command: process.execPath, args, env });
const npmRun = (cwd, args, env = {}) => ({ ...npm(...args), cwd, env });

function runCommand({ command, args, cwd = ".", env = {} }, logFd) {
  fs.writeSync(logFd, `$ ${[command, ...args].join(" ")}   (in ${cwd})\n`);
  const result = spawnSync(command, args, {
    cwd: path.join(repoRoot, cwd),
    env: { ...baseEnv, ...env },
    shell: false,
    stdio: ["ignore", logFd, logFd],
  });
  if (result.error) {
    fs.writeSync(logFd, `failed to start ${command}: ${result.error.message}\n`);
    return false;
  }
  if (result.status !== 0) {
    fs.writeSync(logFd, `exit ${result.status ?? `signal ${result.signal}`}\n`);
    return false;
  }
  return true;
}

function tail(file, lines) {
  return fs.readFileSync(file, "utf8").trimEnd().split("\n").slice(-lines).join("\n");
}

// A step is one or more commands, built lazily so that an error while building
// them (npm not found, a missing workflow value) fails this step only. All
// commands run, and the step fails if any failed (duplicate-code needs the
// summary step even after a failed scan). Advisory steps report WARN instead
// of FAIL, like CI's continue-on-error.
function step(name, buildCommands, { advisory = false } = {}) {
  stepIndex += 1;
  const log = path.join(logDir, `${stepIndex}.log`);
  console.log(`==> ${name}`);
  const started = Date.now();
  const logFd = fs.openSync(log, "w");
  let ok = true;
  try {
    for (const command of buildCommands()) {
      ok = runCommand(command, logFd) && ok;
    }
  } catch (error) {
    fs.writeSync(logFd, `${error.message}\n`);
    ok = false;
  } finally {
    fs.closeSync(logFd);
  }
  const status = ok ? "PASS" : advisory ? "WARN" : "FAIL";
  if (!ok) {
    console.log(`--- last lines of ${name} (full log: ${log}) ---\n${tail(log, LOG_TAIL_LINES)}\n---`);
  }
  results.push({ status, seconds: Math.round((Date.now() - started) / 1000), name });
}

function skip(name) {
  results.push({ status: "SKIP", seconds: null, name });
}

function probe(command, args) {
  return spawnSync(command, args, { env: baseEnv, shell: false, stdio: "ignore" }).status === 0;
}

function warnOnToolchainDrift() {
  const toolchainFile = fs.readFileSync(path.join(repoRoot, "rust-toolchain.toml"), "utf8");
  const channel = /^channel\s*=\s*"(.*)"/m.exec(toolchainFile)?.[1] ?? "";
  const rustc = spawnSync("rustc", ["--version"], { cwd: repoRoot, env: baseEnv, shell: false, encoding: "utf8" });
  if (rustc.error || rustc.status !== 0) {
    console.error("WARNING: rustc not found; Rust checks will fail");
    return;
  }
  const actual = rustc.stdout.trim();
  // "1.89" pins the minor release (any 1.89.x is the same toolchain); "1.89.0"
  // pins a full version. Named channels (stable, nightly) are not compared.
  if (/^\d/.test(channel) && !actual.startsWith(`rustc ${channel} `) && !actual.startsWith(`rustc ${channel}.`)) {
    console.error(`WARNING: ${actual} differs from rust-toolchain.toml channel ${channel}; results may not match CI`);
  }
}

// npm ci when node_modules is missing, or older than the lockfile (npm ci
// rewrites it, so a newer lockfile means dependencies changed since).
function ensureNpmDeps(dir, label, extraArgs = []) {
  const lock = path.join(repoRoot, dir, "package-lock.json");
  const installed = path.join(repoRoot, dir, "node_modules", ".package-lock.json");
  const stale = !fs.existsSync(installed) || fs.statSync(lock).mtimeMs > fs.statSync(installed).mtimeMs;
  if (stale) {
    step(`${label}: npm ci`, () => [npmRun(dir, ["ci", ...extraArgs])]);
  }
}

if (options.rust) {
  warnOnToolchainDrift();
  step("rustfmt", () => [cargo(["fmt", "--all", "--", "--check"])]);
  step("clippy", () => [cargo(["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"], rustEnv)]);
  step("cargo test", () => [cargo(["test", "--all-features"], rustEnv)]);
  if (options.quick) {
    skip("terminal: fake-bridge.js syntax (--quick)");
    skip("terminal: serial re-run (--quick)");
  } else {
    step("terminal: fake-bridge.js syntax", () => [node(["--check", "tests/fixtures/fake-bridge.js"])]);
    step("terminal: serial re-run (--test-threads=1)", () => [
      cargo(["test", "--locked", "--all-features", "--test", "terminal_resize", "--", "--test-threads=1"], rustEnv),
    ]);
  }
  step("lockfile (cargo fetch --locked)", () => [cargo(["fetch", "--locked"])]);
  if (probe("cargo", ["deny", "--version"])) {
    step("cargo-deny policy", () => [cargo(["deny", "--all-features", "check", "bans", "licenses", "sources"])]);
    if (options.quick) {
      skip("cargo-deny advisories (--quick)");
    } else if (options.audit) {
      step("cargo-deny advisories", () => [cargo(["deny", "--all-features", "check", "advisories"])]);
    } else {
      skip("cargo-deny advisories (pass --audit)");
    }
  } else {
    skip("cargo-deny policy (cargo-deny not on PATH; cargo install cargo-deny, or nix develop)");
  }
}

// The thresholds live in pr.yml's duplicate-code job env; read them from there
// so there is one authority. Throws (failing the step) if they are not found.
function readJscpdEnv() {
  const workflow = fs.readFileSync(path.join(repoRoot, ".github", "workflows", "pr.yml"), "utf8");
  const job = /^ {2}duplicate-code:\n([\s\S]*?)(?=^ {2}\S|(?![\s\S]))/m.exec(workflow)?.[1] ?? "";
  const env = {};
  for (const key of ["JSCPD_WARNING_THRESHOLD", "JSCPD_FAILURE_THRESHOLD"]) {
    const value = new RegExp(`^\\s+${key}:\\s*"?([0-9.]+)"?\\s*$`, "m").exec(job)?.[1];
    if (value === undefined) {
      throw new Error(`Could not find ${key} in the duplicate-code job of .github/workflows/pr.yml`);
    }
    env[key] = value;
  }
  return env;
}

// The duplicate-code gate scans both Rust and TypeScript sources and needs
// only the root npm dependencies. CI runs the scan, then a summary step that
// applies the failure threshold.
ensureNpmDeps(".", "duplicate-code", ["--ignore-scripts"]);
step("duplicate-code (npm run quality:duplicates)", () => {
  const env = readJscpdEnv();
  return [npmRun(".", ["run", "quality:duplicates"], env), npmRun(".", ["run", "quality:duplicates:summary"], env)];
});

if (options.bridge) {
  ensureNpmDeps("agent-sdk", "agent-sdk");
  step("agent-sdk: build", () => [npmRun("agent-sdk", ["run", "build"])]);
  // The same command as _agent-sdk.yml, so the glob lives only in package.json.
  step("agent-sdk: test", () => [npmRun("agent-sdk", ["run", "test"])]);
  step("agent-sdk: lint", () => [npmRun("agent-sdk", ["run", "lint"])]);
  step("agent-sdk: knip", () => [npmRun("agent-sdk", ["run", "knip"])]);
  if (options.quick) {
    skip("agent-sdk: audit (--quick)");
  } else if (options.audit) {
    step("agent-sdk: audit:raw (advisory)", () => [npmRun("agent-sdk", ["run", "audit:raw"])], { advisory: true });
    step("agent-sdk: audit (direct deps)", () => [npmRun("agent-sdk", ["run", "audit"])]);
  } else {
    skip("agent-sdk: audit (pass --audit)");
  }
}

const statusWidth = 6;
console.log(`\n${"STATUS".padEnd(statusWidth)} ${"TIME".padStart(8)}  STEP`);
for (const { status, seconds, name } of results) {
  const time = seconds === null ? "-" : `${seconds}s`;
  console.log(`${status.padEnd(statusWidth)} ${time.padStart(8)}  ${name}`);
}

// The logs are the only place the first error of a failed step survives, and
// a WARN prints a log path too, so keep them unless every step passed.
const failed = results.some(({ status }) => status === "FAIL");
const needsLogs = results.some(({ status }) => status === "FAIL" || status === "WARN");
if (failed) {
  console.log(`\nOne or more checks FAILED. Full step logs: ${logDir}`);
  process.exitCode = 1;
} else if (needsLogs) {
  console.log(`\nNo failures, but some steps warned. Full step logs: ${logDir}`);
} else {
  fs.rmSync(logDir, { recursive: true, force: true });
  console.log("\nAll checks passed.");
}
