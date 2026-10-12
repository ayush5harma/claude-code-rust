import type { Settings } from "@anthropic-ai/claude-agent-sdk";
import type { SessionLaunchSettings } from "../types.js";

/**
 * Session configuration from the command line, parsed from the JSON text the app read.
 * The app never parses it; a parse error stops session creation with the flag's name.
 */
export type LaunchConfig = {
  settings?: Settings;
};

function parseJsonObject(text: string, flag: string): Record<string, unknown> {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`${flag} is not valid JSON: ${message}`);
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${flag} must be a JSON object`);
  }
  return value as Record<string, unknown>;
}

export function parseLaunchConfig(launchSettings: SessionLaunchSettings): LaunchConfig {
  return {
    ...(launchSettings.settings !== undefined
      ? { settings: parseJsonObject(launchSettings.settings, "--settings") as Settings }
      : {}),
  };
}
