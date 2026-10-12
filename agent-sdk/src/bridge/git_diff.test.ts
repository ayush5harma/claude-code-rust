import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { workingDiff } from "./git_diff.js";

function git(cwd: string, ...args: string[]): void {
  execFileSync("git", ["-C", cwd, ...args], { stdio: "ignore" });
}

function repository(): string {
  const cwd = mkdtempSync(join(tmpdir(), "claude-rs-diff-"));
  git(cwd, "init", "--quiet");
  writeFileSync(join(cwd, "file with spaces.txt"), "base\n");
  writeFileSync(join(cwd, "staged.txt"), "base\n");
  git(cwd, "add", ".");
  git(cwd, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--quiet", "-m", "base");
  return cwd;
}

test("working diff includes staged and unstaged edits, spaces, and untracked text", async () => {
  const cwd = repository();
  try {
    writeFileSync(join(cwd, "file with spaces.txt"), "base\nworking edit\n");
    writeFileSync(join(cwd, "staged.txt"), "base\nstaged edit\n");
    git(cwd, "add", "staged.txt");
    writeFileSync(join(cwd, "staged.txt"), "base\nstaged edit\nworking edit\n");
    writeFileSync(join(cwd, "new file.txt"), "new content\n");
    const result = await workingDiff(cwd);
    assert.equal(result.error, undefined);
    const byPath = new Map(result.files.map((file) => [file.path, file]));
    assert.match(byPath.get("file with spaces.txt")?.diff ?? "", /\+working edit/);
    assert.match(byPath.get("staged.txt")?.diff ?? "", /\+staged edit[\s\S]*\+working edit/);
    assert.match(byPath.get("new file.txt")?.diff ?? "", /\+\+\+ b\/new file\.txt[\s\S]*\+new content/);
    assert.equal(byPath.get("new file.txt")?.status, "??");
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});

test("working diff reports empty and non-repository states", async () => {
  const cwd = repository();
  const other = mkdtempSync(join(tmpdir(), "claude-rs-not-repo-"));
  try {
    assert.deepEqual(await workingDiff(cwd), { files: [] });
    const missing = await workingDiff(other);
    assert.deepEqual(missing.files, []);
    assert.match(missing.error ?? "", /not a Git repository/);
  } finally {
    rmSync(cwd, { recursive: true, force: true });
    rmSync(other, { recursive: true, force: true });
  }
});

test("working diff includes staged, unstaged, and untracked files before the first commit", async () => {
  const cwd = mkdtempSync(join(tmpdir(), "claude-rs-unborn-diff-"));
  try {
    git(cwd, "init", "--quiet");
    writeFileSync(join(cwd, "initial file.txt"), "staged line\n");
    git(cwd, "add", "initial file.txt");
    writeFileSync(join(cwd, "initial file.txt"), "staged line\nworking line\n");
    writeFileSync(join(cwd, "untracked.txt"), "untracked line\n");
    const result = await workingDiff(cwd);
    assert.equal(result.error, undefined);
    const byPath = new Map(result.files.map((file) => [file.path, file]));
    assert.match(byPath.get("initial file.txt")?.diff ?? "", /\+staged line/);
    assert.match(byPath.get("initial file.txt")?.diff ?? "", /\+working line/);
    assert.match(byPath.get("untracked.txt")?.diff ?? "", /\+untracked line/);
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});

test("working diff marks binary untracked content without reading it as text", async () => {
  const cwd = repository();
  try {
    writeFileSync(join(cwd, "binary.dat"), Buffer.from([0, 1, 2]));
    const result = await workingDiff(cwd);
    assert.equal(result.files.find((file) => file.path === "binary.dat")?.diff, "Binary untracked file; content preview unavailable.");
    assert.deepEqual(readFileSync(join(cwd, "binary.dat")), Buffer.from([0, 1, 2]));
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});

test("working diff limits large untracked previews and total snapshot output", async () => {
  const cwd = repository();
  try {
    writeFileSync(join(cwd, "too-large.txt"), "x".repeat(256 * 1024 + 1));
    for (let index = 0; index < 5; index++) {
      writeFileSync(join(cwd, `large-${index}.txt`), "x".repeat(240 * 1024));
    }
    const result = await workingDiff(cwd);
    assert.match(result.error ?? "", /limited to/);
    assert.ok(result.files.length < 6);
    assert.ok(result.files.reduce((size, file) => size + Buffer.byteLength(file.diff), 0) <= 1024 * 1024);
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});
