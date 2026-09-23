const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

async function loadTranscribeFile(t) {
  const { window } = installBrowserGlobals(t);
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-file-self-hosted-endpoint-test-",
  });
  const { transcribeFile } = await vite.ssrLoadModule("/services/fileTranscription.ts");
  return { window, transcribeFile };
}

test("file transcription sends the self-hosted server settings to the main process", async (t) => {
  const { window, transcribeFile } = await loadTranscribeFile(t);
  let receivedOptions = null;
  window.electronAPI.transcribeAudioFile = async (options) => {
    receivedOptions = options;
    return { success: true, text: "self-hosted" };
  };

  const result = await transcribeFile("/tmp/audio.webm", {
    remoteTranscriptionUrl: "http://192.168.1.20:9000/v1",
    remoteTranscriptionModel: "whisper-large-v3",
    language: "en",
  });

  assert.equal(result.success, true);
  assert.deepEqual(receivedOptions, {
    filePath: "/tmp/audio.webm",
    language: "en",
    remoteTranscriptionUrl: "http://192.168.1.20:9000/v1",
    remoteTranscriptionModel: "whisper-large-v3",
  });
});

test("a missing server URL fails before any IPC call", async (t) => {
  const { window, transcribeFile } = await loadTranscribeFile(t);
  window.electronAPI.transcribeAudioFile = async () => assert.fail("must not dispatch");

  const result = await transcribeFile("/tmp/audio.webm", {
    remoteTranscriptionUrl: "",
    remoteTranscriptionModel: "",
    language: "en",
  });

  assert.equal(result.success, false);
  assert.equal(result.code, "CUSTOM_ENDPOINT_INVALID");
});
