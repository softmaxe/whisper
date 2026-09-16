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
        getAPIKey: async () => "test-key",
        getWhisperPrompt: () => null,
        getKeyterms: () => [],
        shouldStreamTranscription: () => false,
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

test("self-hosted mode is never hijacked by a leftover proxied provider", async (t) => {
  const { window, setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-selfhosted-hijack-test-",
    settingsKey: "__hijackSettings",
  });

  const proxyCalls = { mistral: 0, xai: 0, corti: 0 };
  for (const [provider, channel] of [
    ["mistral", "proxyMistralTranscription"],
    ["xai", "proxyXaiTranscription"],
    ["corti", "proxyCortiTranscription"],
  ]) {
    window.electronAPI[channel] = async () => {
      proxyCalls[provider] += 1;
      throw new Error("must not be called in self-hosted mode");
    };
  }
  const fetched = captureFetch(t, okJson({ text: "self-hosted text" }));
  const manager = createManager({
    getTranscriptionModel: () => "self-hosted-model",
    getAPIKey: async () => null,
  });

  const audioBlob = new Blob([new Uint8Array([1, 2, 3])], { type: "audio/webm" });
  for (const provider of ["mistral", "xai", "corti"]) {
    setSettings({
      allowLocalFallback: false,
      cloudTranscriptionProvider: provider,
      transcriptionMode: "self-hosted",
      remoteTranscriptionUrl: "https://stt.internal.example.com",
      useLocalWhisper: false,
    });
    const result = await manager.processWithOpenAIAPI(audioBlob);
    assert.equal(result.success, true);
  }

  assert.deepEqual(proxyCalls, { mistral: 0, xai: 0, corti: 0 });
  assert.equal(
    fetched.every((e) => e.startsWith("https://stt.internal.example.com")),
    true,
    `unexpected endpoints: ${fetched.join(", ")}`
  );
});

test("self-hosted Azure endpoints keep their deployment URL", async (t) => {
  const { setSettings, createManager } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-selfhosted-azure-test-",
    settingsKey: "__selfHostedAzureSettings",
  });
  const fetched = captureFetch(t, okJson({ text: "azure text" }));
  const manager = createManager({
    getTranscriptionModel: () => "my-deployment",
    getAPIKey: async () => "azure-key",
  });
  const audioBlob = new Blob([new Uint8Array([1, 2, 3])], { type: "audio/webm" });

  const useRemote = (remoteTranscriptionUrl, remoteTranscriptionModel) =>
    setSettings({
      allowLocalFallback: false,
      transcriptionMode: "self-hosted",
      remoteTranscriptionUrl,
      remoteTranscriptionModel,
      useLocalWhisper: false,
    });

  await t.test("a bare Azure origin gains the deployment path and api-version", async () => {
    useRemote("https://myorg.openai.azure.com", "my-deployment");
    await manager.processWithOpenAIAPI(audioBlob);
    assert.deepEqual(fetched, [
      "https://myorg.openai.azure.com/openai/deployments/my-deployment/audio/transcriptions?api-version=2025-03-01-preview",
    ]);
  });

  await t.test("a pinned deployment URL is preserved verbatim", async () => {
    fetched.length = 0;
    const pinned =
      "https://myorg.openai.azure.com/openai/deployments/pinned/audio/transcriptions?api-version=2024-06-01";
    useRemote(pinned, "my-deployment");
    await manager.processWithOpenAIAPI(audioBlob);
    assert.deepEqual(fetched, [pinned]);
  });

  await t.test("a non-Azure self-hosted host is untouched", async () => {
    fetched.length = 0;
    useRemote("https://stt.internal.example.com", "tiny");
    await manager.processWithOpenAIAPI(audioBlob);
    assert.deepEqual(fetched, ["https://stt.internal.example.com/audio/transcriptions"]);
  });
});
