import assert from "node:assert/strict";
import test from "node:test";
import { redactStartupDetail } from "./startup_failures.js";

// Mirrors `redacts_credential_shapes` in src/cli/redaction.rs, the reference.
const CASES: Array<[string, string]> = [
  ["GET /v1?token=abc&x=1", "GET /v1?token=[redacted]&x=1"],
  ["CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-abcDEF123", "CLAUDE_CODE_OAUTH_TOKEN=[redacted]"],
  ["x-api-key: sk-ant-api03-secretvalue", "x-api-key: [redacted]"],
  ["access_token=abc123 next", "access_token=[redacted] next"],
  ["fetch https://user:pass@example.com/repo", "fetch https://[redacted]@example.com/repo"],
  ["Authorization: Basic dXNlcjpwYXNz", "Authorization: [redacted]"],
  ["Authorization: Bearer abc.def", "Authorization: [redacted]"],
  ["key sk-live_abc123 end", "key [redacted] end"],
  ["task-1 and risk-free are not keys", "task-1 and risk-free are not keys"],
];

for (const [input, expected] of CASES) {
  test(`redacts credential shape: ${input}`, () => {
    assert.equal(redactStartupDetail(input), expected);
  });
}
