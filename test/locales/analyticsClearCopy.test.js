const test = require("node:test");
const assert = require("node:assert/strict");

const en = require("../../src/locales/en/translation.json");

test("Clear History describes local data removal", () => {
  const confirmationCopy = en.controlPanel.history.clearAllDescriptionDevice;

  assert.match(confirmationCopy, /transcriptions/);
  assert.match(confirmationCopy, /audio files/);
  assert.match(confirmationCopy, /Insights counters/);
  assert.match(confirmationCopy, /device/);
  assert.doesNotMatch(confirmationCopy, /account|synced/);
});
