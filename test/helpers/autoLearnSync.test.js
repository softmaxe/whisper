const test = require("node:test");
const assert = require("node:assert/strict");

const { createAutomaticPasteFixture } = require("../lib/automaticPasteFixture");

test("auto-learn sync coerces the renderer value to a boolean", (t) => {
  const f = createAutomaticPasteFixture(t, { autoLearn: false });
  f.emit("auto-learn-changed", 1);
  assert.equal(f.owner._autoLearnEnabled, true);
  f.emit("auto-learn-changed", undefined);
  assert.equal(f.owner._autoLearnEnabled, false);
});

test("disabling auto-learn drops the pending correction", (t) => {
  const f = createAutomaticPasteFixture(t);
  f.owner._autoLearnLatestData = { originalText: "a", newFieldValue: "b" };
  f.owner._autoLearnDebounceTimer = setTimeout(() => assert.fail("timer ran"), 1000);
  f.emit("auto-learn-changed", false);
  assert.equal(f.owner._autoLearnLatestData, null);
  assert.equal(f.owner._autoLearnDebounceTimer, null);
  f.owner._autoLearnLatestData = { originalText: "c", newFieldValue: "d" };
  // Both renderer windows re-sync on mount; a same-value sync changes nothing (#1080).
  f.emit("auto-learn-changed", false);
  assert.deepEqual(f.owner._autoLearnLatestData, { originalText: "c", newFieldValue: "d" });
  t.mock.timers.tick(1000);
});

test("a same-value enable keeps the pending correction", (t) => {
  const f = createAutomaticPasteFixture(t);
  const pending = { originalText: "a", newFieldValue: "b" };
  f.owner._autoLearnLatestData = pending;
  f.emit("auto-learn-changed", true);
  assert.equal(f.owner._autoLearnEnabled, true);
  assert.equal(f.owner._autoLearnLatestData, pending);
});
