import type { BridgeCommand } from "../types.js";
import type { SessionState } from "./session_lifecycle.js";

const COLOR_NAMES = new Set(["red", "orange", "yellow", "green", "cyan", "blue", "purple", "pink", "default"]);
const MAX_PENDING_COLORS = 16;

type PromptCommand = Extract<BridgeCommand, { command: "prompt" }>;
type PendingColor = { color: string; order: number };
type ColorRequests = { pending: Map<string, PendingColor>; nextOrder: number; appliedOrder: number };
const colorRequests = new WeakMap<SessionState, ColorRequests>();

export function rememberColorPrompt(session: SessionState, command: PromptCommand): void {
  if (command.chunks.length !== 1) return;
  const chunk = command.chunks[0];
  if (chunk.kind !== "text" || typeof chunk.value !== "string") return;
  const match = /^\/color[ \t]+([a-z]+)$/i.exec(chunk.value.trim());
  const color = match?.[1]?.toLowerCase();
  if (!color || !COLOR_NAMES.has(color)) return;
  let requests = colorRequests.get(session);
  if (!requests) {
    requests = { pending: new Map(), nextOrder: 0, appliedOrder: 0 };
    colorRequests.set(session, requests);
  }
  requests.nextOrder += 1;
  requests.pending.set(command.message_uuid, { color, order: requests.nextOrder });
  if (requests.pending.size > MAX_PENDING_COLORS) {
    const oldest = requests.pending.keys().next().value;
    if (oldest !== undefined) requests.pending.delete(oldest);
  }
}

export function confirmedColorFromResult(session: SessionState, result: Record<string, unknown>): string | undefined {
  if (session.closing || !session.connected) return undefined;
  if (typeof result.session_id !== "string" || !result.session_id || result.session_id !== session.sessionId) return undefined;
  const requests = colorRequests.get(session);
  if (!requests) return undefined;
  const echoed = Array.isArray(result.user_message_uuids)
    ? result.user_message_uuids.filter((uuid): uuid is string => typeof uuid === "string")
    : [];
  const uuids = echoed.length > 0 ? echoed
    : typeof result.user_message_uuid === "string" ? [result.user_message_uuid] : [];
  const candidates = [...new Set(uuids)].flatMap((uuid) => {
    const pending = requests.pending.get(uuid);
    requests.pending.delete(uuid);
    return pending ? [pending] : [];
  });
  if (candidates.length !== 1 || result.subtype !== "success") return undefined;
  const candidate = candidates[0];
  const command = typeof result.local_command === "string" ? result.local_command.trim().toLowerCase() : "";
  const match = /^\/?color(?:[ \t]+([a-z]+))?$/.exec(command);
  if (!match || (match[1] && match[1] !== candidate.color) || candidate.order <= requests.appliedOrder) {
    return undefined;
  }
  requests.appliedOrder = candidate.order;
  return candidate.color;
}
