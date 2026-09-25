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

test("History save failures remain observable to the upload caller", async (t) => {
  const { saveUploadTranscription, window } = await loadUploadHistory(t);
  window.electronAPI.saveTranscription = async () => ({ success: false, id: 0 });
  assert.equal((await saveUploadTranscription("Transcript")).success, false);
  window.electronAPI.saveTranscription = async () => {
    throw new Error("Database unavailable");
  };
  await assert.rejects(saveUploadTranscription("Transcript"), /Database unavailable/);
});
