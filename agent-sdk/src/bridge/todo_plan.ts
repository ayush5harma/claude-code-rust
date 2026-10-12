import type { TodoPlanItem } from "../types.js";

const PLAN_LIMIT = 32;
const CONTENT_LIMIT = 200;

export function confirmedTodoPlan(rawInput: unknown): TodoPlanItem[] | undefined {
  if (!rawInput || typeof rawInput !== "object" || Array.isArray(rawInput)) return undefined;
  const todos = (rawInput as Record<string, unknown>).todos;
  if (!Array.isArray(todos) || todos.length > PLAN_LIMIT) return undefined;
  const items: TodoPlanItem[] = [];
  for (const raw of todos) {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) return undefined;
    const todo = raw as Record<string, unknown>;
    const content = typeof todo.content === "string" ? todo.content.trim() : "";
    if (!content || content.length > CONTENT_LIMIT ||
        (todo.status !== "pending" && todo.status !== "in_progress" && todo.status !== "completed")) {
      return undefined;
    }
    const activeForm = typeof todo.activeForm === "string" ? todo.activeForm.trim() : "";
    items.push({ content, status: todo.status, ...(activeForm ? { active_form: activeForm.slice(0, CONTENT_LIMIT) } : {}) });
  }
  return items;
}
