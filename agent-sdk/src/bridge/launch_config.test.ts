import test from "node:test";
import assert from "node:assert/strict";
import { parseLaunchConfig } from "./launch_config.js";

test("--settings text parses into the settings object", () => {
  assert.deepEqual(parseLaunchConfig({}), {});
  assert.deepEqual(
    parseLaunchConfig({ settings: ' {"spinnerTipsEnabled": false, "env": {"A": "1"}}\n' }),
    { settings: { spinnerTipsEnabled: false, env: { A: "1" } } },
  );
});

test("malformed --settings names the flag instead of failing inside the SDK", () => {
  assert.throws(() => parseLaunchConfig({ settings: "{\"model\": " }), /^Error: --settings is not valid JSON: /);
  for (const text of ["[]", "null", "\"opus\""]) {
    assert.throws(() => parseLaunchConfig({ settings: text }), { message: "--settings must be a JSON object" });
  }
});

test("--mcp-config servers merge in command-line order with later definitions winning", () => {
  const config = parseLaunchConfig({
    mcp_configs: [
      JSON.stringify({ mcpServers: { docs: { command: "docs-v1" }, search: { type: "http", url: "https://search.example" } } }),
      '{"mcpServers": {"docs": {"command": "docs-v2", "args": ["--stdio"]}, "__proto__": {"command": "odd-name"}}}',
    ],
  });
  assert.deepEqual(Object.keys(config.mcpServers ?? {}), ["docs", "search", "__proto__"]);
  assert.deepEqual(config.mcpServers?.docs, { command: "docs-v2", args: ["--stdio"] });
  assert.deepEqual(config.mcpServers?.search, { type: "http", url: "https://search.example" });
  assert.equal(Object.getPrototypeOf(config.mcpServers), Object.prototype);
  assert.deepEqual(parseLaunchConfig({ strict_mcp_config: true }), {});
});

test("malformed --mcp-config names the config that failed", () => {
  assert.throws(() => parseLaunchConfig({ mcp_configs: ["{}"] }), {
    message: '--mcp-config must contain an "mcpServers" object',
  });
  assert.throws(
    () => parseLaunchConfig({ mcp_configs: ['{"mcpServers":{}}', "{oops"] }),
    /^Error: --mcp-config #2 is not valid JSON: /,
  );
  assert.throws(() => parseLaunchConfig({ mcp_configs: ['{"mcpServers":{"docs":"docs-mcp"}}'] }), {
    message: '--mcp-config: MCP server "docs" must be an object',
  });
});
