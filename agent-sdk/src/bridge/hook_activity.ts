import type { SessionUpdate } from "../types.js";
import { emitSessionUpdate } from "./events.js";
import type { SessionState } from "./session_lifecycle.js";

type HookActivityUpdate = Extract<SessionUpdate, { type: "hook_activity" }>;

/** Longest output excerpt sent for one hook message, in characters. */
export const HOOK_OUTPUT_EXCERPT_LIMIT = 240;

/**
 * Most hook updates held for a session that has not connected yet. SessionStart
 * hooks run before the SDK answers its initialize request, measured on SDK
 * 0.3.288: their `hook_started` and `hook_response` messages reach the bridge
 * about 40 ms before `initializationResult()` resolves, and the app drops
 * session updates for a session it has not been told about, so they wait for
 * the connect event.
 */
const PENDING_HOOK_ACTIVITY_LIMIT = 400;

const HOOK_PHASES = {
  hook_started: "started",
  hook_progress: "progress",
  hook_response: "finished",
} as const;

type HookSubtype = keyof typeof HOOK_PHASES;

function isHookSubtype(subtype: string): subtype is HookSubtype {
  return Object.hasOwn(HOOK_PHASES, subtype);
}

function stringField(msg: Record<string, unknown>, key: string): string | undefined {
  const value = msg[key];
  return typeof value === "string" ? value : undefined;
}

function excerpt(text: string): string | undefined {
  const trimmed = text.trim();
  if (trimmed.length === 0) return undefined;
  const characters = Array.from(trimmed);
  return characters.length <= HOOK_OUTPUT_EXCERPT_LIMIT
    ? trimmed
    : `${characters.slice(0, HOOK_OUTPUT_EXCERPT_LIMIT).join("").trimEnd()}…`;
}

/**
 * A failed hook explains itself on stderr, a successful one in its combined
 * output; the other streams are a fallback for hooks that only write to one.
 */
function outputExcerpt(msg: Record<string, unknown>, failed: boolean): string | undefined {
  const streams = failed ? ["stderr", "output", "stdout"] : ["output", "stdout", "stderr"];
  for (const key of streams) {
    const text = excerpt(stringField(msg, key) ?? "");
    if (text !== undefined) return text;
  }
  return undefined;
}

/**
 * Normalise the SDK's `hook_started`, `hook_progress` and `hook_response`
 * messages into one update. The SDK declares no timing, so `observed_at_ms`
 * is when the bridge saw the message and the app derives durations from it.
 * Returns undefined for a message without the identity fields the SDK
 * declares as required.
 */
export function buildHookActivityUpdate(
  msg: Record<string, unknown>,
  observedAtMs: number,
): HookActivityUpdate | undefined {
  const subtype = stringField(msg, "subtype") ?? "";
  if (!isHookSubtype(subtype)) return undefined;
  const hookId = stringField(msg, "hook_id");
  const hookName = stringField(msg, "hook_name");
  const hookEvent = stringField(msg, "hook_event");
  if (!hookId || hookName === undefined || !hookEvent) return undefined;

  const phase = HOOK_PHASES[subtype];
  const outcome = msg.outcome;
  const knownOutcome =
    outcome === "success" || outcome === "error" || outcome === "cancelled" ? outcome : undefined;
  const exitCode = msg.exit_code;
  const output =
    phase === "started" ? undefined : outputExcerpt(msg, knownOutcome !== undefined && knownOutcome !== "success");
  return {
    type: "hook_activity",
    hook_id: hookId,
    hook_name: hookName,
    hook_event: hookEvent,
    phase,
    ...(phase === "finished" && knownOutcome ? { outcome: knownOutcome } : {}),
    ...(phase === "finished" && typeof exitCode === "number" && Number.isInteger(exitCode)
      ? { exit_code: exitCode }
      : {}),
    ...(output !== undefined ? { output } : {}),
    observed_at_ms: Math.trunc(observedAtMs),
  };
}

/**
 * Forward one SDK hook message to the app as hook activity, held until the
 * connect event if need be. message_handlers.ts:handleHookSystemMessage owns
 * the message: it calls this and then logs it and decides on any notice.
 */
export function forwardHookActivity(
  session: SessionState,
  msg: Record<string, unknown>,
  observedAtMs: number = Date.now(),
): void {
  const update = buildHookActivityUpdate(msg, observedAtMs);
  if (!update) return;
  if (session.connected) {
    emitSessionUpdate(session.sessionId, update);
    return;
  }
  const pending = session.pendingHookActivity ?? [];
  if (pending.length < PENDING_HOOK_ACTIVITY_LIMIT) pending.push(update);
  session.pendingHookActivity = pending;
}

/** Send the hook updates held back until the connect event. */
export function flushPendingHookActivity(session: SessionState): void {
  const pending = session.pendingHookActivity;
  session.pendingHookActivity = undefined;
  for (const update of pending ?? []) {
    emitSessionUpdate(session.sessionId, update);
  }
}
