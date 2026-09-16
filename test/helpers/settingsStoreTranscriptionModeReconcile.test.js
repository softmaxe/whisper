const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

test("legacy profiles retain self-hosted endpoints without re-enabling removed providers", async (t) => {
  const { storage } = installBrowserGlobals(t);
  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-transcription-mode-reconcile-test-",
  });

  // Migrations run once per module evaluation, so every case re-evaluates the store.
  const writes = [];
  const setItem = storage.setItem.bind(storage);
  storage.setItem = (key, value) => {
    writes.push(key);
    setItem(key, value);
  };
  const load = async (seed) => {
    storage.clear();
    for (const [key, value] of Object.entries(seed)) storage.setItem(key, value);
    writes.length = 0;
    vite.moduleGraph.invalidateAll();
    const { useSettingsStore } = await vite.ssrLoadModule("/stores/settingsStore.ts");
    return useSettingsStore.getState();
  };

  for (const mode of ["local", "providers", "openwhispr", "enterprise", "self-hosted"]) {
    await t.test(mode, async () => {
      const state = await load({
        transcriptionMode: mode,
        useLocalWhisper: "true",
        uploadTranscriptionMode: mode,
        uploadUseLocalWhisper: "true",
        uploadCloudTranscriptionMode: "openwhispr",
        cleanupMode: mode,
        cleanupCloudMode: "openwhispr",
        isSignedIn: "true",
        useDictationAgent: "true",
        useDictationTranslation: "true",
        remoteTranscriptionUrl: "http://localhost:8000/v1",
        remoteTranscriptionModel: "test-asr",
        cleanupRemoteUrl: "http://localhost:11434/v1",
        cleanupModel: "test-cleanup",
      });
      assert.equal(state.transcriptionMode, "self-hosted");
      assert.equal(state.cleanupMode, "self-hosted");
      assert.equal(state.useLocalWhisper, false);
      assert.equal(state.uploadTranscriptionMode, "self-hosted");
      assert.equal(state.uploadUseLocalWhisper, false);
      assert.equal(state.uploadCloudTranscriptionMode, "byok");
      assert.equal(state.cleanupCloudMode, "byok");
      assert.equal(state.isSignedIn, false);
      assert.equal(state.useDictationAgent, false);
      assert.equal(state.useDictationTranslation, false);
      assert.equal(state.remoteTranscriptionUrl, "http://localhost:8000/v1");
      assert.equal(state.remoteTranscriptionModel, "test-asr");
      assert.equal(state.cleanupRemoteUrl, "http://localhost:11434/v1");
      assert.equal(state.cleanupModel, "test-cleanup");
    });
  }
});
