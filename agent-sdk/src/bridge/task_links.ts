import { applyLinkedChildModel } from "./tasks.js";
import type { SessionState } from "./session_lifecycle.js";

export function linkTaskToolUse(
  session: SessionState,
  taskId: string,
  toolUseId: string,
): void {
  if (!taskId || !toolUseId) {
    return;
  }

  const previousToolUseId = session.taskToolUseIds.get(taskId);
  if (previousToolUseId && previousToolUseId !== toolUseId) {
    const reverseTaskId = session.taskIdsByToolUseId.get(previousToolUseId);
    if (reverseTaskId === taskId) {
      session.taskIdsByToolUseId.delete(previousToolUseId);
      session.childModelsByToolUseId.delete(previousToolUseId);
    }
  }

  const previousTaskId = session.taskIdsByToolUseId.get(toolUseId);
  if (previousTaskId && previousTaskId !== taskId) {
    const forwardToolUseId = session.taskToolUseIds.get(previousTaskId);
    if (forwardToolUseId === toolUseId) {
      session.taskToolUseIds.delete(previousTaskId);
    }
  }

  session.taskToolUseIds.set(taskId, toolUseId);
  session.taskIdsByToolUseId.set(toolUseId, taskId);
  applyLinkedChildModel(session, taskId, toolUseId);
}

export function unlinkTaskToolUse(session: SessionState, taskId: string): void {
  if (!taskId) {
    return;
  }
  const toolUseId = session.taskToolUseIds.get(taskId);
  session.taskToolUseIds.delete(taskId);
  if (toolUseId && session.taskIdsByToolUseId.get(toolUseId) === taskId) {
    session.taskIdsByToolUseId.delete(toolUseId);
    session.childModelsByToolUseId.delete(toolUseId);
  }
}

/**
 * The Agent tool use a subagent's frame belongs to. SDK 0.3.302 child frames
 * can carry a null `parent_tool_use_id`, so the producing agent's task link
 * resolves it instead.
 */
export function childParentToolUseId(
  session: SessionState,
  message: Record<string, unknown>,
): string | undefined {
  const field = (key: string): string | undefined => {
    const value = message[key];
    return typeof value === "string" ? value.trim() || undefined : undefined;
  };
  const producer = field("agent_id");
  return field("parent_tool_use_id") ??
    (producer ? session.taskToolUseIds.get(producer) ?? session.tasksById.get(producer)?.source_tool_call_id : undefined);
}

export function activeTaskIdForToolUse(
  session: SessionState,
  toolUseId: string,
): string | undefined {
  return toolUseId ? session.taskIdsByToolUseId.get(toolUseId) : undefined;
}
