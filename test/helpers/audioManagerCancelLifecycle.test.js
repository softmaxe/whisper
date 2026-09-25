const test = require("node:test");
const assert = require("node:assert/strict");
const { loadAudioManager } = require("./harness/audioManager");
const { deferred } = require("./harness/deferred");

async function loadManagerClass(t) {
  const { AudioManager, window } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-cancel-lifecycle-test-",
    settingsKey: "__cancelLifecycleSettings",
    settings: {
      remoteTranscriptionUrl: "http://localhost:8000/v1",
      remoteTranscriptionModel: "whisper-1",
    },
  });
  return { AudioManager, window };
}

function createManager(AudioManager, transcription) {
  const calls = { errors: 0, saved: 0, noAudio: 0, completed: 0, states: [] };
  const manager = Object.assign(Object.create(AudioManager.prototype), {
    isProcessing: true,
    _localSpeechGateState: null,
    _processingCancellationGeneration: 0,
    _activeTranscriptionAbortController: null,
    lastAudioBlob: {},
    processWithSelfHostedServer: () => transcription.promise,
    onStateChange: (state) => calls.states.push(state.isProcessing ? "processing" : "idle"),
    onNoAudio: () => calls.noAudio++,
    onError: () => calls.errors++,
    onTranscriptionComplete: () => calls.completed++,
    saveFailedTranscription: () => calls.saved++,
  });
  return { manager, calls };
}

function createBatchFinalizationManager(AudioManager, merge) {
  let processAudioCalls = 0;
  let processAudioMetadata = null;
  const mergedBlob = new Blob([new Uint8Array(512)], { type: "audio/webm" });
  const manager = Object.assign(Object.create(AudioManager.prototype), {
    isRecording: true,
    isProcessing: false,
    _processingCancellationGeneration: 0,
    _activeTranscriptionAbortController: null,
    _batchSegments: [],
    _receivedAudioData: true,
    _localSpeechGateState: null,
    recordingMimeType: "audio/webm",
    recordingStartTime: Date.now(),
    lastAudioBlob: null,
    micRecovery: { stop() {} },
    teardownSpeechGate() {},
    mergeRecordedSegments: () => merge.promise,
    getLargestRecordedSegment: () => null,
    processAudio: async (_audioBlob, metadata) => {
      processAudioCalls += 1;
      processAudioMetadata = metadata;
    },
    onStateChange() {},
  });
  return {
    manager,
    mergedBlob,
    getProcessAudioCalls: () => processAudioCalls,
    getProcessAudioMetadata: () => processAudioMetadata,
  };
}

test("batch finalization keeps the recording occurrence time", async (t) => {
  const { AudioManager } = await loadManagerClass(t);
  const merge = deferred();
  const recordingStartedAt = Date.parse("2026-09-02T14:00:00.000Z");
  const { manager, mergedBlob, getProcessAudioMetadata } = createBatchFinalizationManager(
    AudioManager,
    merge
  );
  manager.recordingStartTime = recordingStartedAt;

  const finalization = manager.finalizeBatchRecording(mergedBlob);
  merge.resolve(mergedBlob);
  await finalization;

  assert.equal(
    getProcessAudioMetadata().analyticsOccurredAt,
    new Date(recordingStartedAt).toISOString()
  );
});

test("cancelling while batch segments merge never starts transcription", async (t) => {
  const { AudioManager } = await loadManagerClass(t);
  const merge = deferred();
  const { manager, mergedBlob, getProcessAudioCalls } = createBatchFinalizationManager(
    AudioManager,
    merge
  );

  const finalization = manager.finalizeBatchRecording(mergedBlob);
  assert.equal(manager.isProcessing, true);
  assert.equal(manager.cancelProcessing(), true);
  merge.resolve(mergedBlob);
  await finalization;

  assert.equal(getProcessAudioCalls(), 0);
  assert.equal(manager.lastAudioBlob, null);
});

test("a cancelled older pipeline cannot publish or clear a newer pipeline", async (t) => {
  const { AudioManager } = await loadManagerClass(t);
  const firstTranscription = deferred();
  const secondTranscription = deferred();
  const completed = [];
  const firstBlob = new Blob(["first"], { type: "audio/webm" });
  const secondBlob = new Blob(["second"], { type: "audio/webm" });
  const { manager } = createManager(AudioManager, firstTranscription);
  manager.processWithSelfHostedServer = (audioBlob) =>
    audioBlob === firstBlob ? firstTranscription.promise : secondTranscription.promise;
  manager.onTranscriptionComplete = (result) => completed.push(result.text);

  const firstRun = manager.processAudio(firstBlob);
  assert.equal(manager.cancelProcessing(), true);

  manager.isProcessing = true;
  const secondRun = manager.processAudio(secondBlob);
  firstTranscription.resolve({ success: true, text: "old", source: "self-hosted", timings: {} });
  await firstRun;

  assert.equal(manager.isProcessing, true, "the old pipeline must not clear the new busy state");
  assert.deepEqual(completed, [], "the old pipeline must not publish after cancellation");

  secondTranscription.resolve({ success: true, text: "new", source: "self-hosted", timings: {} });
  await secondRun;

  assert.equal(manager.isProcessing, false);
  assert.deepEqual(completed, ["new"]);
});

test("batch processing carries its occurrence time into completion", async (t) => {
  const { AudioManager } = await loadManagerClass(t);
  const transcription = deferred();
  const { manager } = createManager(AudioManager, transcription);
  const analyticsOccurredAt = "2026-09-02T14:00:00.000Z";
  let completion;
  manager.onTranscriptionComplete = (result) => {
    completion = result;
  };

  const processing = manager.processAudio(new Blob(["audio"]), { analyticsOccurredAt });
  transcription.resolve({ success: true, text: "same event", source: "self-hosted", timings: {} });
  await processing;

  assert.equal(completion.analyticsOccurredAt, analyticsOccurredAt);
});

test("a user cancel during batch transcription is not an error and saves nothing", async (t) => {
  const { AudioManager } = await loadManagerClass(t);
  const transcription = deferred();
  const { manager, calls } = createManager(AudioManager, transcription);

  const run = manager.processAudio(new Blob(["audio"], { type: "audio/webm" }), {});
  assert.equal(manager.cancelProcessing(), true);
  const abort = new Error("The operation was aborted");
  abort.name = "AbortError";
  // The aborted request rejects after the cancel landed.
  const rejection = Promise.reject(abort);
  rejection.catch(() => {});
  transcription.resolve(rejection);
  await run;

  assert.equal(calls.errors, 0, "cancel must not surface a Transcription Error");
  assert.equal(calls.saved, 0, "cancel must not write a failed-transcription row");
  assert.equal(calls.completed, 0);
  assert.deepEqual(calls.states, ["idle"]);
});
