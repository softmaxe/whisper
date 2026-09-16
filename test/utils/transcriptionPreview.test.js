const test = require("node:test");
const assert = require("node:assert/strict");

const load = () => import("../../src/utils/transcriptionPreview.ts");

test("batch-only self-hosted transcription does not advertise live preview", async () => {
  const { supportsLiveTranscriptionPreview } = await load();

  assert.equal(supportsLiveTranscriptionPreview("self-hosted", true), false);
});
