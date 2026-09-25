const test = require("node:test");
const assert = require("node:assert/strict");
const { loadAudioManager: loadAudioManagerHarness } = require("./harness/audioManager");

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

function captureFetch(t, respond) {
  const originalFetch = globalThis.fetch;
  const endpoints = [];
  globalThis.fetch = async (endpoint, init) => {
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
  const fetched = captureFetch(t, okJson({ text: "self-hosted text" }));
  const manager = createManager({ getTranscriptionModel: () => "self-hosted-model" });

  const audioBlob = new Blob([new Uint8Array([1, 2, 3])], { type: "audio/webm" });
  setSettings({
    remoteTranscriptionUrl: "https://stt.internal.example.com",
  });
  const result = await manager.processWithSelfHostedServer(audioBlob);

  assert.equal(result.success, true);
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
