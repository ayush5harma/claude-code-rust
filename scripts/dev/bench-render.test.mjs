import assert from "node:assert/strict";
import test from "node:test";
import { assertSameScenarios, compare, median, parseBench } from "./bench-render.mjs";

const run = (entries) => new Map(Object.entries(entries).map(([key, value]) => [key, { median: value, p95: value * 2 }]));
const KEY = "120x40 streaming";

test("parseBench keeps only BENCH lines and reads median and p95", () => {
  const parsed = parseBench(
    ["running 1 test", "BENCH 120x40 streaming median_us=152 p95_us=401 frames=2000", "BENCH 200x60 scroll median_us=7 p95_us=9 frames=3", "ok"].join("\r\n"),
  );
  assert.deepEqual([...parsed], [
    ["120x40 streaming", { median: 152, p95: 401 }],
    ["200x60 scroll", { median: 7, p95: 9 }],
  ]);
});

test("median of an even count takes the lower middle value", () => {
  assert.equal(median([4, 1, 3, 2]), 2);
  assert.equal(median([5]), 5);
});

test("a quiet machine gates at the threshold and flags only a change beyond it", () => {
  const groups = (head) => ({
    baseA: [run({ [KEY]: 100 }), run({ [KEY]: 100 })],
    baseB: [run({ [KEY]: 100 }), run({ [KEY]: 100 })],
    head: [run({ [KEY]: head }), run({ [KEY]: head })],
  });
  const [steady] = compare(groups(109), 10);
  assert.equal(steady.gate, 10);
  assert.equal(steady.regression, false);
  const [slow] = compare(groups(111), 10);
  assert.equal(slow.regression, true);
  assert.equal(Math.round(slow.change), 11);
});

test("two identical builds that drift apart widen the gate to twice the spread", () => {
  // Base medians 100 and 120 (spread 20/100 = 20%), the case where a fixed 10% gate fails a
  // build that did not change.
  const groups = {
    baseA: [run({ [KEY]: 100 })],
    baseB: [run({ [KEY]: 120 })],
    head: [run({ [KEY]: 118 })],
  };
  const [row] = compare(groups, 10);
  assert.equal(row.spread, 20);
  assert.equal(row.gate, 40);
  assert.equal(row.regression, false);
});

test("a scenario that only one side reports is an error, not a zero change", () => {
  const groups = {
    baseA: [run({ [KEY]: 100 })],
    baseB: [run({ [KEY]: 100 })],
    head: [run({ [KEY]: 100, "120x40 scroll": 50 })],
  };
  assert.throws(() => compare(groups, 10), /do not report the same scenarios[\s\S]*baseA run 1 lacks: 120x40 scroll/);
  assert.throws(() => assertSameScenarios({ head: [run({ a: 1 }), run({ b: 1 })] }), /head run 1 lacks: b/);
});
