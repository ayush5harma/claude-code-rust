import { execFile } from "node:child_process";
import { lstat, readFile } from "node:fs/promises";
import { isAbsolute, relative, resolve, sep } from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const MAX_FILES = 80;
const MAX_OUTPUT = 256 * 1024;
const MAX_SNAPSHOT = 1024 * 1024;
const TIMEOUT_MS = 3_000;

export type WorkingDiffFile = { path: string; status: string; diff: string };
export type WorkingDiffSnapshot = { files: WorkingDiffFile[]; error?: string };

async function git(cwd: string, args: string[], signal: AbortSignal): Promise<string> {
  const { stdout } = await execFileAsync("git", ["-C", cwd, ...args], {
    timeout: TIMEOUT_MS,
    maxBuffer: MAX_OUTPUT,
    windowsHide: true,
    encoding: "utf8",
    signal,
  });
  return stdout;
}

function inside(root: string, path: string): boolean {
  const rel = relative(root, path);
  return rel !== ".." && !rel.startsWith(`..${sep}`) && !isAbsolute(rel);
}

async function untrackedDiff(root: string, name: string): Promise<string> {
  const file = resolve(root, name);
  if (!inside(root, file)) return "File is outside the repository.";
  const stat = await lstat(file);
  if (!stat.isFile()) return "Untracked non-regular file; content preview unavailable.";
  if (stat.size > MAX_OUTPUT) return `Untracked file exceeds ${MAX_OUTPUT} bytes; preview unavailable.`;
  const bytes = await readFile(file);
  if (bytes.includes(0)) return "Binary untracked file; content preview unavailable.";
  const lines = bytes.toString("utf8").split("\n");
  if (lines.at(-1) === "") lines.pop();
  return [`--- /dev/null`, `+++ b/${name}`, `@@ -0,0 +1,${lines.length} @@`, ...lines.map((line) => `+${line}`)].join("\n");
}

export async function workingDiff(cwd: string): Promise<WorkingDiffSnapshot> {
  const controller = new AbortController();
  const deadline = setTimeout(() => controller.abort(), 8_000);
  try {
    const root = (await git(cwd, ["rev-parse", "--show-toplevel"], controller.signal)).trim();
    const raw = await git(root, ["status", "--porcelain=v1", "-z", "--untracked-files=all"], controller.signal);
    let hasHead = true;
    try {
      await git(root, ["rev-parse", "--verify", "--quiet", "HEAD"], controller.signal);
    } catch (error) {
      if (typeof error === "object" && error !== null && "code" in error && error.code === 1) {
        hasHead = false;
      } else {
        throw error;
      }
    }
    const records = raw.split("\0");
    const paths = new Map<string, string>();
    for (let i = 0; i < records.length && paths.size < MAX_FILES; i++) {
      const record = records[i];
      if (!record) continue;
      const status = record.slice(0, 2);
      const name = record.slice(3);
      if (status.includes("R") || status.includes("C")) i++;
      if (name && !paths.has(name)) paths.set(name, status);
    }
    const files: WorkingDiffFile[] = [];
    let bytes = 0;
    let truncated = paths.size === MAX_FILES;
    for (const [name, status] of paths) {
      const diff = status === "??"
        ? await untrackedDiff(root, name)
        : hasHead
          ? await git(root, ["diff", "HEAD", "--no-ext-diff", "--no-textconv", "--no-color", "--", name], controller.signal)
          : [
              await git(root, ["diff", "--cached", "--no-ext-diff", "--no-textconv", "--no-color", "--", name], controller.signal),
              await git(root, ["diff", "--no-ext-diff", "--no-textconv", "--no-color", "--", name], controller.signal),
            ].filter(Boolean).join("\n");
      bytes += Buffer.byteLength(diff);
      if (bytes > MAX_SNAPSHOT) {
        truncated = true;
        break;
      }
      files.push({ path: name, status, diff: diff || "Binary or metadata-only change; text diff unavailable." });
    }
    return { files, ...(truncated ? { error: `Working diff limited to ${MAX_FILES} files and ${MAX_SNAPSHOT} bytes.` } : {}) };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return { files: [], error: message.includes("not a git repository") ? "This directory is not a Git repository." : `Could not read Git changes: ${message}` };
  } finally {
    clearTimeout(deadline);
  }
}
