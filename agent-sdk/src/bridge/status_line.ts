import { spawn, type ChildProcess } from "node:child_process";
import { resolveSettings, type Settings } from "@anthropic-ai/claude-agent-sdk";
import { emitSessionUpdate } from "./events.js";
import { SessionObservations } from "./session_observations.js";
import type { SessionState } from "./session_lifecycle.js";

const observations = new SessionObservations();
const pending = new WeakMap<SessionState, { timer?: NodeJS.Timeout; abort?: AbortController }>();
const MAX_OUTPUT = 4096;
const TIMEOUT_MS = 2000;
type Config = { command: string; refreshInterval?: number };
type UserSettingsResolver = (options: { cwd: string; settingSources: ["user"] }) => Promise<{ effective: Pick<Settings, "statusLine" | "disableAllHooks"> }>;
export async function readConfiguredStatusLine(cwd: string, resolve: UserSettingsResolver = resolveSettings): Promise<Config | undefined> {
  const { effective } = await resolve({ cwd, settingSources: ["user"] });
  return effective.disableAllHooks || effective.statusLine?.type !== "command" ? undefined : effective.statusLine;
}
type StatusLineServices = {
  readConfig: (cwd: string) => Promise<Config | undefined>;
  run: typeof executeStatusLine;
};
const defaultServices: StatusLineServices = {
  readConfig: readConfiguredStatusLine,
  run: executeStatusLine,
};

export function stopStatusLine(session: SessionState): void {
  const current = pending.get(session);
  if (current?.timer) clearTimeout(current.timer);
  current?.abort?.abort();
  pending.delete(session);
  observations.begin(session)();
}

function stopChild(child: ChildProcess): void {
  if (!child.pid) return;
  try {
    if (process.platform !== "win32") process.kill(-child.pid, "SIGKILL");
    else spawn("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" }).unref();
  } catch { child.kill("SIGKILL"); }
}

export async function executeStatusLine(command: string, cwd: string, input: object, signal?: AbortSignal): Promise<string | null> {
  if (!command.trim() || command.length > 2048) return null;
  if (signal?.aborted) return null;
  return new Promise(resolve => {
    const child = spawn(command, { cwd, shell: true, detached: process.platform !== "win32", stdio: ["pipe", "pipe", "ignore"] });
    let output = "";
    let finished = false;
    const finish = (value: string | null) => {
      if (finished) return;
      finished = true;
      clearTimeout(timeout);
      if (value === null) stopChild(child);
      resolve(value);
    };
    const timeout = setTimeout(() => finish(null), TIMEOUT_MS);
    signal?.addEventListener("abort", () => finish(null), { once: true });
    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      output += chunk;
      if (Buffer.byteLength(output) > MAX_OUTPUT) finish(null);
    });
    child.on("error", () => finish(null));
    child.on("close", code => finish(code === 0 ? output : null));
    child.stdin?.on("error", () => finish(null));
    child.stdin?.end(`${JSON.stringify(input)}\n`);
  });
}

async function refresh(session: SessionState, current: () => boolean, abort: AbortController, services: StatusLineServices): Promise<void> {
  const sessionId = session.sessionId;
  try {
    const config = await services.readConfig(session.cwd);
    if (!current() || !session.connected) return;
    if (!config?.command) {
      emitSessionUpdate(sessionId, { type: "status_line_update", text: null });
      return;
    }
    let context: Awaited<ReturnType<SessionState["query"]["getContextUsage"]>> | undefined;
    try { context = await session.query.getContextUsage({ detail: "summary" }); } catch { /* Public context data is optional. */ }
    if (!current() || !session.connected) return;
    const input = {
      session_id: sessionId,
      cwd: session.cwd,
      workspace: { current_dir: session.cwd, project_dir: session.cwd },
      ...(session.currentModel ? { model: { id: session.currentModel.resolved_id, display_name: session.currentModel.display_name_short } } : {}),
      ...(context ? { context_window: { used_percentage: context.percentage, remaining_percentage: Math.max(0, 100 - context.percentage), context_window_size: context.maxTokens } } : {}),
      ...(session.statusRate ? { rate_limits: session.statusRate } : {}),
    };
    const result = await services.run(config.command, session.cwd, input, abort.signal);
    if (current() && session.connected) {
      emitSessionUpdate(sessionId, { type: "status_line_update", text: result });
      if (config.refreshInterval !== undefined && Number.isFinite(config.refreshInterval)) {
        const interval = Math.min(3600, Math.max(1, config.refreshInterval));
        const state = pending.get(session);
        if (state) state.timer = setTimeout(() => scheduleStatusLine(session, services), interval * 1000);
      }
    }
  } catch {
    if (current() && session.connected) emitSessionUpdate(sessionId, { type: "status_line_update", text: null });
  }
}

export function scheduleStatusLine(session: SessionState, services: StatusLineServices = defaultServices): void {
  if (!session.connected || session.closing) return;
  const current = observations.begin(session);
  const prior = pending.get(session);
  if (prior?.timer) clearTimeout(prior.timer);
  prior?.abort?.abort();
  const abort = new AbortController();
  const next: { timer?: NodeJS.Timeout; abort?: AbortController } = { abort };
  next.timer = setTimeout(() => void refresh(session, current, abort, services), 300);
  pending.set(session, next);
}
