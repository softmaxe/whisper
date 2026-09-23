const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

const transcription = {
  remoteTranscriptionUrl: "http://127.0.0.1:8000/v1",
  remoteTranscriptionModel: "fixture-asr",
  language: "en",
};

async function waitForQueueSettled(store) {
  const deadline = Date.now() + 5000;
  while (store.getState().isProcessing) {
    if (Date.now() > deadline) throw new Error("Queue did not finish");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  return store.getState().queue;
}

for (const enabled of [true, false]) {
  test(`batch uses original self-hosted file transport with History retention ${enabled}`, async (t) => {
    const { window } = installBrowserGlobals(t);
    const vite = await createRendererServer(t, {
      cachePrefix: "whisper-upload-history-integration-test-",
      mockModules: {
        "/stores/settingsStore": `export const getSettings = () => ({ dataRetentionEnabled: ${enabled} });`,
      },
    });
    const requests = [];
    const saves = [];
    Object.assign(window.electronAPI, {
      transcribeAudioFile: async (options) => {
        requests.push(options);
        return { success: true, text: `Transcript ${requests.length}` };
      },
      saveTranscription: async (...args) => {
        saves.push(args);
        return { success: true, id: saves.length };
      },
    });
    const store = await vite.ssrLoadModule("/stores/batchQueueStore.ts");
    store.addFiles([
      { name: "first.wav", path: "/tmp/first.wav", sizeBytes: 1024 },
      { name: "second.m4a", path: "/tmp/second.m4a", sizeBytes: 2048 },
    ]);
    store.processBatchQueue({ transcription });
    const queue = await waitForQueueSettled(store.useBatchQueueStore);
    assert.deepEqual(
      queue.map((item) => item.status),
      ["done", "done"]
    );
    assert.deepEqual(
      queue.map((item) => item.text),
      ["Transcript 1", "Transcript 2"]
    );
    for (const request of requests) {
      assert.equal(request.remoteTranscriptionUrl, transcription.remoteTranscriptionUrl);
      assert.equal(request.remoteTranscriptionModel, "fixture-asr");
      assert.equal(request.language, "en");
    }
    if (enabled) {
      assert.deepEqual(saves, [
        ["Transcript 1", null, { routeKind: "upload" }],
        ["Transcript 2", null, { routeKind: "upload" }],
      ]);
      assert.deepEqual(
        queue.map((item) => item.transcriptionId),
        [1, 2]
      );
    } else {
      assert.deepEqual(saves, []);
      assert.ok(queue.every((item) => item.transcriptionId === undefined));
    }
  });
}
