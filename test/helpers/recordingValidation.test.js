const test = require("node:test");
const assert = require("node:assert/strict");

const load = () => import("../../src/helpers/recordingValidation.js");

test("allows a recording with audio frames and a real-sized blob", async () => {
  const { evaluateFinishedRecording } = await load();
  assert.deepEqual(evaluateFinishedRecording({ blobSize: 50000, receivedAudioData: true }), {
    usable: true,
    reason: null,
  });
});

test("rejects when no audio chunk was ever delivered (issue #871 cold start)", async () => {
  const { evaluateFinishedRecording } = await load();
  assert.deepEqual(evaluateFinishedRecording({ blobSize: 0, receivedAudioData: false }), {
    usable: false,
    reason: "no-audio-data",
  });
});

test("rejects a header-only ~110 byte blob even when a chunk arrived", async () => {
  const { evaluateFinishedRecording } = await load();
  assert.deepEqual(evaluateFinishedRecording({ blobSize: 110, receivedAudioData: true }), {
    usable: false,
    reason: "empty-container",
  });
});

test("treats missing / undefined args as unusable (defensive, does not throw)", async () => {
  const { evaluateFinishedRecording } = await load();
  assert.deepEqual(evaluateFinishedRecording(), { usable: false, reason: "no-audio-data" });
  assert.deepEqual(evaluateFinishedRecording({}), { usable: false, reason: "no-audio-data" });
  assert.deepEqual(evaluateFinishedRecording({ receivedAudioData: true }), {
    usable: false,
    reason: "empty-container",
  });
});

test("withSalvageWarning attaches a warning to a successful salvaged transcription", async () => {
  const { withSalvageWarning } = await load();
  assert.deepEqual(withSalvageWarning({ success: true, text: "hi" }, true), {
    success: true,
    text: "hi",
    warning: "salvaged-recording",
  });
});

test("withSalvageWarning leaves non-salvaged results untouched", async () => {
  const { withSalvageWarning } = await load();
  const result = { success: true, text: "hi" };
  assert.equal(withSalvageWarning(result, false), result);
});

test("withSalvageWarning keeps an existing warning", async () => {
  const { withSalvageWarning } = await load();
  const result = { success: true, text: "hi", warning: "truncated" };
  assert.equal(withSalvageWarning(result, true), result);
});

test("withSalvageWarning ignores failed or missing results", async () => {
  const { withSalvageWarning } = await load();
  const failed = { success: false, message: "no audio" };
  assert.equal(withSalvageWarning(failed, true), failed);
  assert.equal(withSalvageWarning(null, true), null);
});

test("flags an empty WebM container (issue #864 double-trigger, ~110 bytes)", async () => {
  const { isEmptyRecording } = await load();
  assert.equal(isEmptyRecording(110), true);
});

test("flags a zero-byte blob", async () => {
  const { isEmptyRecording } = await load();
  assert.equal(isEmptyRecording(0), true);
});

test("flags 255 bytes (just under the threshold)", async () => {
  const { isEmptyRecording } = await load();
  assert.equal(isEmptyRecording(255), true);
});

test("allows exactly 256 bytes (boundary)", async () => {
  const { isEmptyRecording } = await load();
  assert.equal(isEmptyRecording(256), false);
});

test("allows a short real utterance with real audio bytes (no silent data loss)", async () => {
  const { isEmptyRecording } = await load();
  assert.equal(isEmptyRecording(3000), false);
});

test("allows a normal full-length recording", async () => {
  const { isEmptyRecording } = await load();
  assert.equal(isEmptyRecording(1674472), false);
});

test("treats missing / null / NaN size as empty (defensive)", async () => {
  const { isEmptyRecording } = await load();
  assert.equal(isEmptyRecording(undefined), true);
  assert.equal(isEmptyRecording(null), true);
  assert.equal(isEmptyRecording(NaN), true);
});
