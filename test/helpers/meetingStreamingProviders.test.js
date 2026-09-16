const test = require("node:test");
const assert = require("node:assert/strict");

test("a dictation-only streaming provider is not offered for note recording", async () => {
  const { getStreamingTranscriptionProviders, getMeetingStreamingTranscriptionProviders } =
    await import("../../src/models/ModelRegistry.ts");

  const streamingIds = getStreamingTranscriptionProviders().map((provider) => provider.id);
  const meetingIds = getMeetingStreamingTranscriptionProviders().map((provider) => provider.id);

  assert.ok(streamingIds.includes("gemini"), "gemini ships a streaming dictation model");
  assert.equal(meetingIds.includes("gemini"), false);
});

// The intersection is an allow-list, so a new streaming provider is silently
// excluded from note recording unless it is listed. Deepgram and AssemblyAI are
// the managed note-recording providers and must stay admitted.
test("deepgram and assemblyai are offered for note recording", async () => {
  const { getMeetingStreamingTranscriptionProviders } =
    await import("../../src/models/ModelRegistry.ts");

  const meetingIds = getMeetingStreamingTranscriptionProviders().map((provider) => provider.id);
  assert.ok(meetingIds.includes("deepgram"));
  assert.ok(meetingIds.includes("assemblyai"));
});
