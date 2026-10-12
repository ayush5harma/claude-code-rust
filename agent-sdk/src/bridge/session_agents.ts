import { getSubagentMessages, listSubagents } from "@anthropic-ai/claude-agent-sdk";
import type { BridgeCommand } from "../types.js";
import { writeEvent } from "./events.js";
import { mapSessionMessagesToUpdates } from "./history.js";
import { sessionById } from "./session_lifecycle.js";

const MAX_AGENTS = 100;
const MAX_MESSAGES = 80;
const MAX_MESSAGE_CHARS = 2_000;
const SDK_REQUEST_TIMEOUT_MS = 10_000;

type AgentCommand = Extract<BridgeCommand, {
  command: "list_session_agents" | "get_subagent_messages" | "stop_subagent_task";
}>;

function isSubagentTask(task: { metadata?: unknown }): boolean {
  const metadata = task.metadata;
  return metadata !== null && typeof metadata === "object" && !Array.isArray(metadata) &&
    (("subagent_type" in metadata && typeof metadata.subagent_type === "string") ||
      ("task_type" in metadata && metadata.task_type === "local_agent"));
}

async function boundedSdkCall<T>(request: Promise<T>): Promise<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      request,
      new Promise<T>((_, reject) => {
        timeout = setTimeout(() => reject(new Error("Agent request timed out.")), SDK_REQUEST_TIMEOUT_MS);
      }),
    ]);
  } finally {
    if (timeout !== undefined) clearTimeout(timeout);
  }
}

export async function handleSessionAgentCommand(command: AgentCommand, requestId?: string): Promise<void> {
  const session = sessionById(command.session_id);
  if (!session || session.closing) {
    writeEvent({ event: "session_agents_result", session_id: command.session_id,
      action: command.command, agent_id: command.command === "list_session_agents" ? undefined : command.agent_id,
      agent_ids: [], messages: [], error: "No active session is available." }, requestId);
    return;
  }
  try {
    if (command.command === "list_session_agents") {
      const recorded = await boundedSdkCall(listSubagents(session.sessionId, { dir: session.cwd }));
      const taskIds = [...session.tasksById.values()]
        .filter(isSubagentTask).map((task) => task.task_id);
      const agentIds = [...new Set([...recorded, ...taskIds])].slice(0, MAX_AGENTS);
      writeEvent({ event: "session_agents_result", session_id: session.sessionId,
        action: command.command, agent_ids: agentIds, messages: [] }, requestId);
      return;
    }
    if (command.command === "get_subagent_messages") {
      const recorded = await boundedSdkCall(listSubagents(session.sessionId, { dir: session.cwd }));
      const knownTask = session.tasksById.get(command.agent_id);
      if (!recorded.includes(command.agent_id) && (!knownTask || !isSubagentTask(knownTask))) {
        throw new Error("Selected agent is no longer in this session.");
      }
      const history = await boundedSdkCall(getSubagentMessages(session.sessionId, command.agent_id,
        { dir: session.cwd, limit: MAX_MESSAGES }));
      const messages = mapSessionMessagesToUpdates(history).flatMap((update) => {
        if (update.type !== "user_message_chunk" && update.type !== "agent_message_chunk") return [];
        if (update.content.type !== "text") return [];
        return [{ role: update.type === "user_message_chunk" ? "user" as const : "assistant" as const,
          text: update.content.text.slice(0, MAX_MESSAGE_CHARS) }];
      }).slice(-MAX_MESSAGES);
      writeEvent({ event: "session_agents_result", session_id: session.sessionId,
        action: command.command, agent_id: command.agent_id, agent_ids: [], messages }, requestId);
      return;
    }
    const task = session.tasksById.get(command.agent_id);
    if (!task || !isSubagentTask(task) || task.status !== "in_progress") {
      throw new Error("Selected agent has no running task to stop.");
    }
    await boundedSdkCall(session.query.stopTask(task.task_id));
    writeEvent({ event: "session_agents_result", session_id: session.sessionId,
      action: command.command, agent_id: command.agent_id, agent_ids: [], messages: [] }, requestId);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    writeEvent({ event: "session_agents_result", session_id: command.session_id,
      action: command.command, agent_id: command.command === "list_session_agents" ? undefined : command.agent_id,
      agent_ids: [], messages: [], error: message }, requestId);
  }
}
