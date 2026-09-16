const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

test("fresh-install transcription settings preserve backend defaults", async (t) => {
  installBrowserGlobals(t, {
    window: { location: { search: "" }, electronAPI: { getPlatform: () => "darwin" } },
  });
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-transcription-defaults-",
  });
  const { useSettingsStore } = await vite.ssrLoadModule("/stores/settingsStore.ts");
  const state = useSettingsStore.getState();
  for (const prefix of ["", "meeting", "upload"]) {
    const key = (name) => (prefix ? prefix + name[0].toUpperCase() + name.slice(1) : name);
    assert.equal(state[key("useLocalWhisper")], false);
    assert.equal(state[key("localTranscriptionProvider")], "whisper");
    assert.equal(state[key("parakeetModel")], "");
  }
});
