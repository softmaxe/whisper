const test = require("node:test");
const assert = require("node:assert/strict");
const { loadAudioManager: loadAudioManagerHarness } = require("./harness/audioManager");
const { deferred } = require("./harness/deferred");

async function loadAudioManager(t, { cachePrefix, settingsKey }) {
  const { window, vite, setSettings, createManager } = await loadAudioManagerHarness(t, {
    cachePrefix,
    settingsKey,
  });
  return {
    window,
    vite,
    setSettings,
    createManager: (overrides = {}) =>
      createManager({
        getEffectiveSttLanguage: () => "auto",
        getTranscriptionModel: () => "whisper-1",
        getWhisperPrompt: () => null,
        isDictionaryEcho: () => false,
        processTranscription: async (text) => text,
        isReasoningAvailable: async () => false,
        ...overrides,
      }),
  };
}

function captureFetch(t, respond, probe = async () => ({ status: 405 })) {
  const originalFetch = globalThis.fetch;
  const endpoints = [];
  globalThis.fetch = async (endpoint, init) => {
    if (init.method === "HEAD") return probe(String(endpoint), init);
    endpoints.push(String(endpoint));
    return respond(String(endpoint), init);
  };
  t.after(() => {
    globalThis.fetch = originalFetch;
  });
  return endpoints;
}

const okJson = (body) => async () => ({
  ok: true,
  status: 200,
  headers: { get: () => "application/json" },
  text: async () => JSON.stringify(body),
});

test("self-hosted audio goes to the configured endpoint", async (t) => {
  const { setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "whisper-selfhosted-endpoint-test-",
    settingsKey: "__endpointSettings",
  });
  const probes = [];
  const fetched = captureFetch(t, okJson({ text: "self-hosted text" }), async (endpoint, init) => {
    probes.push(endpoint);
    assert.equal(init.body, undefined, "the connection probe must not upload audio");
    assert.equal(init.cache, "no-store", "a cached response cannot establish reachability");
    return { status: 405 };
  });
  const manager = createManager({ getTranscriptionModel: () => "self-hosted-model" });

  const audioBlob = new Blob([new Uint8Array([1, 2, 3])], { type: "audio/webm" });
  setSettings({
    remoteTranscriptionUrl: "https://stt.internal.example.com",
  });
  const result = await manager.processWithSelfHostedServer(audioBlob);

  assert.equal(result.success, true);
  assert.deepEqual(probes, ["https://stt.internal.example.com/audio/transcriptions"]);
  assert.deepEqual(fetched, ["https://stt.internal.example.com/audio/transcriptions"]);
});

test("a missing self-hosted URL fails closed instead of reaching a hosted provider", async (t) => {
  const { setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-selfhosted-missing-url-test-",
    settingsKey: "__missingUrlSettings",
  });
  const fetched = captureFetch(t, okJson({ text: "should not happen" }));
  const manager = createManager();
  const audioBlob = new Blob([new Uint8Array([1, 2, 3])], { type: "audio/webm" });

  setSettings({ remoteTranscriptionUrl: "" });
  await assert.rejects(manager.processWithSelfHostedServer(audioBlob), {
    code: "CUSTOM_ENDPOINT_INVALID",
  });
  assert.deepEqual(fetched, []);
});

test("a refused connection reports server unavailability without uploading audio", async (t) => {
  const { setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "whisper-selfhosted-refused-test-",
    settingsKey: "__refusedSettings",
  });
  setSettings({ remoteTranscriptionUrl: "http://localhost:8000/v1" });
  const fetched = captureFetch(t, okJson({ text: "should not happen" }), async () => {
    throw new TypeError("Failed to fetch");
  });
  const manager = createManager();
  await assert.rejects(manager.processWithSelfHostedServer(new Blob(["audio"])), {
    code: "TRANSCRIPTION_CONNECTION_FAILED",
  });
  assert.deepEqual(fetched, []);
  assert.equal(manager._activeTranscriptionAbortController, null);
});

test("a reachable server can take more than three seconds to transcribe", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "whisper-selfhosted-slow-transcription-test-",
    settingsKey: "__slowTranscriptionSettings",
  });
  setSettings({ remoteTranscriptionUrl: "http://localhost:8000/v1" });
  const response = deferred();
  const requested = deferred();
  let signal;
  captureFetch(t, (_endpoint, init) => {
    signal = init.signal;
    requested.resolve();
    return response.promise;
  });
  const request = createManager().processWithSelfHostedServer(new Blob(["audio"]));
  await requested.promise;
  t.mock.timers.tick(5_000);
  assert.equal(signal.aborted, false, "the three-second deadline applies only to reachability");
  response.resolve(await okJson({ text: "A completed dictation." })());
  assert.equal((await request).text, "A completed dictation.");
});

test("the transcription deadline also aborts a stalled response body", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "whisper-selfhosted-body-timeout-test-",
    settingsKey: "__bodyTimeoutSettings",
  });
  setSettings({ remoteTranscriptionUrl: "http://localhost:8000/v1" });
  const reading = deferred();
  let signal;
  captureFetch(t, async (_endpoint, init) => {
    signal = init.signal;
    return {
      ok: true,
      text: () => {
        reading.resolve();
        return new Promise((_, reject) => {
          signal.addEventListener(
            "abort",
            () => reject(new DOMException("The operation was aborted", "AbortError")),
            { once: true }
          );
        });
      },
    };
  });
  const manager = createManager();
  const request = manager.processWithSelfHostedServer(new Blob(["audio"]));
  const rejected = assert.rejects(request, { code: "TRANSCRIPTION_REQUEST_TIMEOUT" });
  await reading.promise;
  t.mock.timers.tick(30_000);
  assert.equal(signal.aborted, true, "receiving headers must not disable the deadline");
  await rejected;
  assert.equal(manager._activeTranscriptionAbortController, null);
});

test("a completed transcription clears its deadline before text cleanup", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "whisper-selfhosted-timeout-cleanup-test-",
    settingsKey: "__timeoutCleanupSettings",
  });
  setSettings({ remoteTranscriptionUrl: "http://localhost:8000/v1" });
  let signal;
  captureFetch(t, async (_endpoint, init) => {
    signal = init.signal;
    return okJson({ text: "A completed dictation." })();
  });
  const cleanup = deferred();
  const cleaning = deferred();
  const manager = createManager({
    processTranscription: () => {
      cleaning.resolve();
      return cleanup.promise;
    },
  });
  const request = manager.processWithSelfHostedServer(new Blob(["audio"]));
  await cleaning.promise;
  t.mock.timers.tick(30_000);
  assert.equal(signal.aborted, false, "text cleanup owns its own timeout");
  cleanup.resolve("Cleaned dictation.");
  assert.equal((await request).text, "Cleaned dictation.");
  assert.equal(manager._activeTranscriptionAbortController, null);
});
