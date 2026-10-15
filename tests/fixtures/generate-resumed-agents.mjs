// Rewrites `history_updates` in resumed-agents.json from its `records` through the
// bridge's resume replay. Run `npm run build --prefix agent-sdk` first.
// agent-sdk/src/bridge/history.test.ts fails when the two drift apart.
import { readFileSync, writeFileSync } from "node:fs";
import { mapSessionMessagesToUpdates } from "../../agent-sdk/dist/bridge/history.js";

const file = new URL("resumed-agents.json", import.meta.url);
const fixture = JSON.parse(readFileSync(file, "utf8"));
fixture.history_updates = mapSessionMessagesToUpdates(fixture.records);
writeFileSync(file, `${JSON.stringify(fixture, null, 2)}\n`);
