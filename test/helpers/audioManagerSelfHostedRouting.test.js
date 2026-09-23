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

test("leftover hosted-provider settings never divert self-hosted audio", async (t) => {
  const { setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-selfhosted-leftover-test-",
    settingsKey: "__leftoverSettings",
  });
  const fetched = captureFetch(t, okJson({ text: "self-hosted text" }));
  const manager = createManager({ getTranscriptionModel: () => "self-hosted-model" });

  const audioBlob = new Blob([new Uint8Array([1, 2, 3])], { type: "audio/webm" });
  for (const provider of ["mistral", "xai", "corti", "groq", "openai"]) {
    setSettings({
      cloudTranscriptionProvider: provider,
      transcriptionMode: "self-hosted",
      remoteTranscriptionUrl: "https://stt.internal.example.com",
    });
    const result = await manager.processWithSelfHostedServer(audioBlob);
    assert.equal(result.success, true);
  }

  assert.equal(
    fetched.every((e) => e === "https://stt.internal.example.com/audio/transcriptions"),
    true,
    `unexpected endpoints: ${fetched.join(", ")}`
  );
});

test("a missing self-hosted URL fails closed instead of reaching a hosted provider", async (t) => {
  const { setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-selfhosted-missing-url-test-",
    settingsKey: "__missingUrlSettings",
  });
  const fetched = captureFetch(t, okJson({ text: "should not happen" }));
  const manager = createManager();
  const audioBlob = new Blob([new Uint8Array([1, 2, 3])], { type: "audio/webm" });

  for (const provider of ["openai", "custom"]) {
    setSettings({
      cloudTranscriptionProvider: provider,
      cloudTranscriptionBaseUrl: "https://custom.example.com/v1",
      transcriptionMode: "self-hosted",
      remoteTranscriptionUrl: "",
    });
    await assert.rejects(manager.processWithSelfHostedServer(audioBlob), {
      code: "CUSTOM_ENDPOINT_INVALID",
    });
  }
  assert.deepEqual(fetched, []);
});

test("self-hosted Azure endpoints keep their deployment URL", async (t) => {
  const { setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-selfhosted-azure-test-",
    settingsKey: "__selfHostedAzureSettings",
  });
  const fetched = captureFetch(t, okJson({ text: "azure text" }));
  const manager = createManager({
    getTranscriptionModel: () => "my-deployment",
  });
  const audioBlob = new Blob([new Uint8Array([1, 2, 3])], { type: "audio/webm" });

  const useRemote = (remoteTranscriptionUrl, remoteTranscriptionModel) =>
    setSettings({
      transcriptionMode: "self-hosted",
      remoteTranscriptionUrl,
      remoteTranscriptionModel,
    });

  await t.test("a bare Azure origin gains the deployment path and api-version", async () => {
    useRemote("https://myorg.openai.azure.com", "my-deployment");
    await manager.processWithSelfHostedServer(audioBlob);
    assert.deepEqual(fetched, [
      "https://myorg.openai.azure.com/openai/deployments/my-deployment/audio/transcriptions?api-version=2025-03-01-preview",
    ]);
  });

  await t.test("a pinned deployment URL is preserved verbatim", async () => {
    fetched.length = 0;
    const pinned =
      "https://myorg.openai.azure.com/openai/deployments/pinned/audio/transcriptions?api-version=2024-06-01";
    useRemote(pinned, "my-deployment");
    await manager.processWithSelfHostedServer(audioBlob);
    assert.deepEqual(fetched, [pinned]);
  });

  await t.test("a non-Azure self-hosted host is untouched", async () => {
    fetched.length = 0;
    useRemote("https://stt.internal.example.com", "tiny");
    await manager.processWithSelfHostedServer(audioBlob);
    assert.deepEqual(fetched, ["https://stt.internal.example.com/audio/transcriptions"]);
  });
});
