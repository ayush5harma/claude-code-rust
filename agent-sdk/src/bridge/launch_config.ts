import type { McpServerConfig, Settings } from "@anthropic-ai/claude-agent-sdk";
import type { SessionLaunchSettings } from "../types.js";

/**
 * Session configuration from the command line, parsed from the JSON text the app read.
 * The app never parses it; a parse error stops session creation with the flag's name.
 */
export type LaunchConfig = {
  settings?: Settings;
  mcpServers?: Record<string, McpServerConfig>;
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

/**
 * Merge the `mcpServers` objects of every --mcp-config in command-line order, a later
 * server of the same name replacing an earlier one. Server definitions are validated by
 * the SDK. A Map keeps a server named `__proto__` an ordinary entry.
 */
function mergeMcpConfigs(texts: string[]): Record<string, McpServerConfig> | undefined {
  if (texts.length === 0) {
    return undefined;
  }
  const servers = new Map<string, McpServerConfig>();
  texts.forEach((text, index) => {
    const flag = texts.length > 1 ? `--mcp-config #${index + 1}` : "--mcp-config";
    const config = parseJsonObject(text, flag).mcpServers;
    if (!config || typeof config !== "object" || Array.isArray(config)) {
      throw new Error(`${flag} must contain an "mcpServers" object`);
    }
    for (const [name, server] of Object.entries(config)) {
      if (!server || typeof server !== "object" || Array.isArray(server)) {
        throw new Error(`${flag}: MCP server "${name}" must be an object`);
      }
      servers.set(name, server as McpServerConfig);
    }
  });
  return Object.fromEntries(servers);
}

export function parseLaunchConfig(launchSettings: SessionLaunchSettings): LaunchConfig {
  const mcpServers = mergeMcpConfigs(launchSettings.mcp_configs ?? []);
  return {
    ...(launchSettings.settings !== undefined
      ? { settings: parseJsonObject(launchSettings.settings, "--settings") as Settings }
      : {}),
    ...(mcpServers ? { mcpServers } : {}),
  };
}
