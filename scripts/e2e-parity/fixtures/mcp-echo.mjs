#!/usr/bin/env node
// Minimal stdio MCP server for the parity suite: one tool, parity_echo,
// that answers "PARITY_MCP_ECHO:<text>". Newline-delimited JSON-RPC 2.0.
import { createInterface } from "node:readline";

const send = (msg) => process.stdout.write(`${JSON.stringify(msg)}\n`);

const TOOL = {
  name: "parity_echo",
  description: "Echo the given text back, prefixed with PARITY_MCP_ECHO.",
  inputSchema: {
    type: "object",
    properties: { text: { type: "string" } },
    required: ["text"],
  },
};

function handle(req) {
  switch (req.method) {
    case "initialize":
      return {
        protocolVersion: req.params?.protocolVersion ?? "2025-06-18",
        capabilities: { tools: {} },
        serverInfo: { name: "parity", version: "1.0.0" },
      };
    case "tools/list":
      return { tools: [TOOL] };
    case "tools/call": {
      const text = String(req.params?.arguments?.text ?? "");
      return { content: [{ type: "text", text: `PARITY_MCP_ECHO:${text}` }] };
    }
    case "ping":
      return {};
    default:
      return null;
  }
}

createInterface({ input: process.stdin }).on("line", (line) => {
  let req;
  try {
    req = JSON.parse(line);
  } catch {
    return;
  }
  if (req.id === undefined) return; // notification
  const result = handle(req);
  if (result === null) {
    send({ jsonrpc: "2.0", id: req.id, error: { code: -32601, message: "method not found" } });
  } else {
    send({ jsonrpc: "2.0", id: req.id, result });
  }
});
