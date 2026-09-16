const test = require("node:test");
const assert = require("node:assert/strict");

const load = () => import("../../src/helpers/transcriptionRoute.ts");

const resolve = async (settings, extra = {}) => {
  const { resolveTranscriptionRoute } = await load();
  return resolveTranscriptionRoute({ settings, ...extra });
};

test("self-hosted routes to the configured server and wins over stale flags", async () => {
  const route = await resolve({
    transcriptionMode: "self-hosted",
    remoteTranscriptionUrl: "http://192.168.1.5:11434/v1",
    remoteTranscriptionModel: "whisper-1",
    useLocalWhisper: true,
    cloudTranscriptionProvider: "mistral",
  });
  assert.equal(route.transport, "http-batch");
  assert.equal(route.provider, "self-hosted");
  assert.equal(route.endpoint, "http://192.168.1.5:11434/v1/audio/transcriptions");
  assert.deepEqual(route.auth, { scheme: "none", keyRef: null });
  assert.equal(route.sizeCapBytes, null);
});

test("self-hosted mode without a URL fails closed unless the provider is custom", async () => {
  for (const provider of ["openai", "groq", "mistral", "xai", "corti", "gemini", "tinfoil"]) {
    const route = await resolve({
      transcriptionMode: "self-hosted",
      remoteTranscriptionUrl: "",
      cloudTranscriptionProvider: provider,
    });
    assert.equal(route.transport, "error", provider);
    assert.match(route.message, /not configured/, provider);
  }

  // byok+custom persists transcriptionMode="self-hosted" (deriveTranscriptionMode),
  // so a custom user with a cleared remote URL must still reach their endpoint.
  const customRoute = await resolve({
    transcriptionMode: "self-hosted",
    remoteTranscriptionUrl: "",
    cloudTranscriptionProvider: "custom",
    cloudTranscriptionBaseUrl: "https://stt.parasail.example.com/v1",
  });
  assert.equal(customRoute.transport, "http-batch");
  assert.equal(customRoute.provider, "custom");
  assert.equal(customRoute.endpoint, "https://stt.parasail.example.com/v1/audio/transcriptions");
});

test("self-hosted fails closed on invalid or public-http URLs", async () => {
  for (const remoteTranscriptionUrl of [
    "not a url",
    "ftp://localhost:8080",
    "http://example.com/v1",
  ]) {
    const route = await resolve({
      transcriptionMode: "self-hosted",
      remoteTranscriptionUrl,
      cloudTranscriptionProvider: "openai",
    });
    assert.equal(route.transport, "error", remoteTranscriptionUrl);
    assert.match(route.message, /invalid or unsupported/, remoteTranscriptionUrl);
  }
});

test("Azure self-hosted endpoints build deployment URLs, like Custom ones", async () => {
  const route = await resolve({
    transcriptionMode: "self-hosted",
    remoteTranscriptionUrl: "https://myres.openai.azure.com",
    remoteTranscriptionModel: "my-deployment",
  });
  assert.equal(route.provider, "self-hosted");
  assert.equal(
    route.endpoint,
    "https://myres.openai.azure.com/openai/deployments/my-deployment/audio/transcriptions?api-version=2025-03-01-preview"
  );

  const pinned = await resolve({
    transcriptionMode: "self-hosted",
    remoteTranscriptionUrl:
      "https://myres.openai.azure.com/openai/deployments/d1/audio/transcriptions?api-version=2024-06-01",
    remoteTranscriptionModel: "ignored",
  });
  assert.match(pinned.endpoint, /deployments\/d1\/audio\/transcriptions\?api-version=2024-06-01/);

  // Non-Azure self-hosted servers keep the plain OpenAI-compatible path.
  const plain = await resolve({
    transcriptionMode: "self-hosted",
    remoteTranscriptionUrl: "https://stt.internal.example.com",
    remoteTranscriptionModel: "tiny",
  });
  assert.equal(plain.endpoint, "https://stt.internal.example.com/audio/transcriptions");
});

test("self-hosted keeps its configured model and honors the effective language", async () => {
  const route = await resolve(
    {
      transcriptionMode: "self-hosted",
      remoteTranscriptionUrl: "http://localhost:8000/v1",
      remoteTranscriptionModel: "whisper-large-v3",
      preferredLanguage: "de-DE",
    },
    { request: { model: "ignored-override", effectiveLanguage: "ja" } }
  );
  assert.equal(route.model, "whisper-large-v3");
  assert.equal(route.language, "ja");
});
