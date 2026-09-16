const test = require("node:test");
const assert = require("node:assert/strict");
const { loadAudioManager } = require("./harness/audioManager");

// transcriptions.timestamp is what the history list sorts and groups on. A
// successful dictation stores when speech started, so every other row a
// recording can produce has to be dated the same way -- otherwise a failed
// take sorts above the dictation that was still being transcribed when it
// happened, and the column means two different things depending on the row.
async function loadManagerClass(t) {
  return loadAudioManager(t, {
    cachePrefix: "openwhispr-occurrence-time-test-",
    settingsKey: "__occurrenceTimeSettings",
    settings: { dataRetentionEnabled: true, audioRetentionDays: 0 },
  });
}

test("self-hosted dictation records spoken words and duration before saving History", async (t) => {
  const { createManager, setSettings, window } = await loadManagerClass(t);
  setSettings({
    dataRetentionEnabled: true,
    audioRetentionDays: 0,
    transcriptionMode: "self-hosted",
  });
  const occurredAt = new Date(2026, 3, 1, 12).toISOString();
  const events = [];
  const saved = [];
  window.electronAPI.recordAnalyticsEvent = async (event) => {
    events.push(event);
    return { success: true };
  };
  window.electronAPI.saveTranscription = async (text, rawText, options) => {
    saved.push({ text, rawText, options });
    return { id: 1 };
  };
  const manager = createManager({
    lastAudioBlob: null,
    lastAudioMetadata: { durationMs: 12000, provider: "self-hosted", model: "test-asr" },
    translationRequested: false,
  });

  assert.equal(
    await manager.saveTranscription("Please open ~/projects/.", "please open my projects", {
      clientTranscriptionId: "dictation-1",
      analyticsOccurredAt: occurredAt,
    }),
    true
  );
  assert.deepEqual(events, [
    {
      eventId: "dictation-1",
      wordCount: 4,
      occurredAt,
      localDate: "2026-04-01",
      spokenDurationMs: 12000,
      mode: "self_hosted",
      provider: "self-hosted",
      model: "test-asr",
    },
  ]);
  assert.equal(saved[0].options.clientTranscriptionId, events[0].eventId);
  assert.equal(saved[0].options.analyticsOccurredAt, occurredAt);
  assert.equal(saved[0].text, "Please open ~/projects/.");
});

test("an Insights write failure does not prevent saving the dictation", async (t) => {
  const { createManager, window } = await loadManagerClass(t);
  const saved = [];
  window.electronAPI.recordAnalyticsEvent = async () => {
    throw new Error("Insights storage unavailable");
  };
  window.electronAPI.saveTranscription = async (text) => {
    saved.push(text);
    return { id: 1 };
  };
  const manager = createManager({
    lastAudioBlob: null,
    lastAudioMetadata: null,
    translationRequested: false,
  });

  assert.equal(await manager.saveTranscription("Retained dictation"), true);
  assert.deepEqual(saved, ["Retained dictation"]);
});

test("disabled local history also disables Insights event recording", async (t) => {
  const { createManager, setSettings, window } = await loadManagerClass(t);
  setSettings({ dataRetentionEnabled: false, audioRetentionDays: 0 });
  const writes = [];
  window.electronAPI.recordAnalyticsEvent = async () => writes.push("analytics");
  window.electronAPI.saveTranscription = async () => writes.push("history");
  const manager = createManager({
    lastAudioBlob: new Blob(["audio"]),
    lastAudioMetadata: { durationMs: 12000 },
    translationRequested: false,
  });

  assert.equal(await manager.saveTranscription("Private dictation"), true);
  assert.deepEqual(writes, []);
  assert.equal(manager.lastAudioBlob, null);
  assert.equal(manager.lastAudioMetadata, null);
});

test("a failed take is dated when it was recorded, not when the failure was written", async (t) => {
  const { AudioManager, window } = await loadManagerClass(t);
  const saved = [];
  window.electronAPI.saveTranscription = async (text, rawText, options) => {
    saved.push(options);
    return { id: 1 };
  };

  const manager = Object.assign(Object.create(AudioManager.prototype), {
    lastAudioBlob: null,
    lastAudioMetadata: null,
    translationRequested: false,
  });
  await manager.saveFailedTranscription("boom", null, {
    analyticsOccurredAt: "2026-04-01T09:00:00.000Z",
  });

  assert.equal(saved.length, 1);
  assert.equal(saved[0].analyticsOccurredAt, "2026-04-01T09:00:00.000Z");
});

test("a discarded take carries its recording time past the state reset", async (t) => {
  const { AudioManager } = await loadManagerClass(t);
  const manager = Object.assign(Object.create(AudioManager.prototype), {
    recordingStartTime: Date.parse("2026-04-01T09:00:00.000Z"),
    audioChunks: [],
    _batchSegments: [],
    recordingMimeType: "audio/webm",
  });

  // The snapshot exists because the save is async and runs after the manager
  // has already been reset for the next recording, so recordingStartTime is
  // gone by then -- the occurrence time has to be captured with the audio.
  const snapshot = manager.takeDiscardedBatchSnapshot();

  assert.equal(snapshot.analyticsOccurredAt, "2026-04-01T09:00:00.000Z");
});
