const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

async function loadUploadHistory(t, enabled = true) {
  const { window } = installBrowserGlobals(t);
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-upload-history-save-test-",
    mockModules: {
      "/stores/settingsStore": `export const getSettings = () => ({ dataRetentionEnabled: ${enabled} });`,
    },
  });
  const service = await vite.ssrLoadModule("/services/uploadNotes.ts");
  return { ...service, window };
}

test("uploads save raw text through History with the upload route", async (t) => {
  const { saveUploadTranscription, window } = await loadUploadHistory(t);
  const calls = [];
  window.electronAPI.saveTranscription = async (...args) => {
    calls.push(args);
    return { success: true, id: 42 };
  };
  assert.deepEqual(await saveUploadTranscription("Original transcript"), { success: true, id: 42 });
  assert.deepEqual(calls, [["Original transcript", null, { routeKind: "upload" }]]);
});

test("retention disabled leaves successful upload text unsaved", async (t) => {
  const { saveUploadTranscription, window } = await loadUploadHistory(t, false);
  window.electronAPI.saveTranscription = () => assert.fail("History write must not run");
  assert.deepEqual(await saveUploadTranscription("Private transcript"), {
    success: true,
    id: null,
  });
});

test("History save failures remain observable to the upload caller", async (t) => {
  const { saveUploadTranscription, window } = await loadUploadHistory(t);
  window.electronAPI.saveTranscription = async () => ({ success: false, id: 0 });
  assert.equal((await saveUploadTranscription("Transcript")).success, false);
  window.electronAPI.saveTranscription = async () => {
    throw new Error("Database unavailable");
  };
  await assert.rejects(saveUploadTranscription("Transcript"), /Database unavailable/);
});
