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
