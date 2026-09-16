const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

function selfHostedConfig() {
  return {
    useLocalWhisper: false,
    localTranscriptionProvider: "whisper",
    whisperModel: "",
    parakeetModel: "",
    isOpenWhisprCloud: false,
    getApiKey: () => "must-not-leak",
    cloudTranscriptionProvider: "custom",
    cloudTranscriptionBaseUrl: "",
    cloudTranscriptionModel: "",
    language: "en",
    transcriptionMode: "self-hosted",
  };
}

test("self-hosted file transcription bypasses stale Custom endpoint validation", async (t) => {
  const { window } = installBrowserGlobals(t);
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-file-self-hosted-endpoint-test-",
    mockModules: {
      "/lib/auth": "export const withSessionRefresh = (fn) => fn();",
    },
  });
  const { transcribeFile } = await vite.ssrLoadModule("/services/fileTranscription.ts");

  let receivedOptions = null;
  window.electronAPI.transcribeAudioFileByok = async (options) => {
    receivedOptions = options;
    return { success: true, text: "self-hosted" };
  };

  const result = await transcribeFile(
    "/tmp/audio.webm",
    {
      ...selfHostedConfig(),
      transcriptionMode: "self-hosted",
      remoteTranscriptionUrl: "http://192.168.1.20:9000/v1",
      remoteTranscriptionModel: "whisper-large-v3",
    },
    false
  );

  assert.equal(result.success, true);
  assert.equal(receivedOptions.transcriptionMode, "self-hosted");
  assert.equal(receivedOptions.remoteTranscriptionUrl, "http://192.168.1.20:9000/v1");
  assert.equal(receivedOptions.remoteTranscriptionModel, "whisper-large-v3");
});
