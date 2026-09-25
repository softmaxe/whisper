const test = require("node:test");
const assert = require("node:assert/strict");

const load = () => import("../../src/helpers/transcriptionRoute.ts");

const resolve = async (settings, extra = {}) => {
  const { resolveTranscriptionRoute } = await load();
  return resolveTranscriptionRoute({ settings, ...extra });
};

test("transcription routes to the configured self-hosted server", async () => {
  const route = await resolve({
    remoteTranscriptionUrl: "http://192.168.1.5:11434/v1",
    remoteTranscriptionModel: "whisper-1",
  });
  assert.equal(route.transport, "http-batch");
  assert.equal(route.endpoint, "http://192.168.1.5:11434/v1/audio/transcriptions");
  assert.equal(route.model, "whisper-1");
});

test("a missing server URL fails closed", async () => {
  const route = await resolve({ remoteTranscriptionUrl: "  " });
  assert.equal(route.transport, "error");
  assert.equal(route.code, "CUSTOM_ENDPOINT_INVALID");
  assert.match(route.message, /not configured/);
});

test("invalid or public-http URLs fail closed", async () => {
  for (const remoteTranscriptionUrl of [
    "not a url",
    "ftp://localhost:8080",
    "http://example.com/v1",
  ]) {
    const route = await resolve({ remoteTranscriptionUrl });
    assert.equal(route.transport, "error", remoteTranscriptionUrl);
    assert.match(route.message, /invalid or unsupported/, remoteTranscriptionUrl);
  }
});

test("a bare server origin gains the transcription path", async () => {
  const plain = await resolve({
    remoteTranscriptionUrl: "https://stt.internal.example.com",
    remoteTranscriptionModel: "tiny",
  });
  assert.equal(plain.endpoint, "https://stt.internal.example.com/audio/transcriptions");
});

test("an unset model is omitted and the effective language wins", async () => {
  const route = await resolve(
    { remoteTranscriptionUrl: "http://localhost:8000/v1", preferredLanguage: "de-DE" },
    { request: { effectiveLanguage: "ja" } }
  );
  assert.equal(route.model, null);
  assert.equal(route.language, "ja");

  const fromPreference = await resolve({
    remoteTranscriptionUrl: "http://localhost:8000/v1",
    preferredLanguage: "de-DE",
  });
  assert.equal(fromPreference.language, "de");
});
