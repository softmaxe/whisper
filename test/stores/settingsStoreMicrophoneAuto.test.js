const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

async function loadStore(t, initialStorage = {}) {
  const { storage } = installBrowserGlobals(t, { initialStorage });
  const vite = await createRendererServer(t, { cachePrefix: "whisper-microphone-settings-" });
  const { useSettingsStore } = await vite.ssrLoadModule("/stores/settingsStore.ts");
  return { storage, vite, useSettingsStore };
}

test("new installs default to Auto and persist it across reloads", async (t) => {
  const { storage, vite, useSettingsStore } = await loadStore(t);
  assert.equal(useSettingsStore.getState().microphoneSelectionMode, "auto");
  assert.equal(storage.getItem("microphoneSelectionMode"), "auto");

  useSettingsStore.getState().setMicrophoneSelectionMode("built-in");
  useSettingsStore.getState().setMicrophoneSelectionMode("auto");
  assert.equal(storage.getItem("microphoneSelectionMode"), "auto");
  assert.equal(storage.getItem("preferBuiltInMic"), "false");

  vite.moduleGraph.invalidateAll();
  const reloaded = await vite.ssrLoadModule("/stores/settingsStore.ts");
  assert.equal(reloaded.useSettingsStore.getState().microphoneSelectionMode, "auto");
  assert.equal(reloaded.useSettingsStore.getState().preferBuiltInMic, false);
});

test("System Default migrates to Auto despite a previously remembered device", async (t) => {
  const { storage, useSettingsStore } = await loadStore(t, {
    microphoneSelectionMode: "system",
    selectedMicDeviceId: "iphone",
    selectedMicDeviceLabel: "Rui's iPhone Microphone",
  });
  assert.equal(useSettingsStore.getState().microphoneSelectionMode, "auto");
  assert.equal(storage.getItem("microphoneSelectionMode"), "auto");
  assert.equal(useSettingsStore.getState().selectedMicDeviceId, "iphone");
});

for (const [name, stored, expected] of [
  [
    "explicit device",
    { microphoneSelectionMode: "specific", selectedMicDeviceId: "iphone" },
    "specific",
  ],
  ["legacy device", { selectedMicDeviceId: "iphone" }, "specific"],
  ["legacy built-in preference", { preferBuiltInMic: "true" }, "built-in"],
  ["explicit built-in preference", { microphoneSelectionMode: "built-in" }, "built-in"],
  ["default alias", { selectedMicDeviceId: "default" }, "auto"],
]) {
  test(`migration preserves ${name}`, async (t) => {
    const { storage, useSettingsStore } = await loadStore(t, stored);
    assert.equal(useSettingsStore.getState().microphoneSelectionMode, expected);
    assert.equal(storage.getItem("microphoneSelectionMode"), expected);
    assert.equal(useSettingsStore.getState().preferBuiltInMic, expected === "built-in");
    if (stored.selectedMicDeviceId) {
      assert.equal(useSettingsStore.getState().selectedMicDeviceId, stored.selectedMicDeviceId);
    }
  });
}

test("manual microphone choice survives a reload and invalid modes normalize to Auto", async (t) => {
  const { vite, storage, useSettingsStore } = await loadStore(t);
  useSettingsStore.getState().setSelectedMicDevice("iphone", "Rui's iPhone Microphone");
  useSettingsStore.getState().setMicrophoneSelectionMode("specific");
  vite.moduleGraph.invalidateAll();
  const { useSettingsStore: reloaded } = await vite.ssrLoadModule("/stores/settingsStore.ts");
  assert.equal(reloaded.getState().microphoneSelectionMode, "specific");
  assert.equal(reloaded.getState().selectedMicDeviceId, "iphone");
  assert.equal(reloaded.getState().selectedMicDeviceLabel, "Rui's iPhone Microphone");
  reloaded.getState().setMicrophoneSelectionMode("invalid");
  assert.equal(reloaded.getState().microphoneSelectionMode, "auto");
  assert.equal(storage.getItem("microphoneSelectionMode"), "auto");
});
