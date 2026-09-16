const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

test("fresh installs use self-hosted servers and enable the menu bar icon", async (t) => {
  installBrowserGlobals(t, {
    window: { location: { search: "" }, electronAPI: { getPlatform: () => "darwin" } },
  });
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-transcription-defaults-",
  });
  const { useSettingsStore } = await vite.ssrLoadModule("/stores/settingsStore.ts");
  const state = useSettingsStore.getState();
  assert.equal(state.transcriptionMode, "self-hosted");
  assert.equal(state.cleanupMode, "self-hosted");
  assert.equal(state.useLocalWhisper, false);
  assert.equal(state.remoteTranscriptionUrl, "");
  assert.equal(state.remoteTranscriptionModel, "");
  assert.equal(state.cleanupRemoteUrl, "");
  assert.equal(state.cleanupModel, "");
  assert.equal(state.showMenuBarIcon, true);
});
