const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

test("menu bar settings hydrate the startup value and send changes to the main process", async (t) => {
  const { storage, window } = installBrowserGlobals(t, {
    initialStorage: { showMenuBarIcon: "true" },
  });
  const changes = [];
  window.electronAPI.getMenuBarIconVisible = async () => false;
  window.electronAPI.setMenuBarIconVisible = async (visible) => {
    changes.push(visible);
    return visible;
  };
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-menu-bar-store-",
  });
  const { useSettingsStore, initializeSettings } = await vite.ssrLoadModule(
    "/stores/settingsStore.ts"
  );

  await initializeSettings();
  assert.equal(useSettingsStore.getState().showMenuBarIcon, false);
  assert.equal(storage.getItem("showMenuBarIcon"), "false");
  assert.deepEqual(changes, []);

  useSettingsStore.getState().setShowMenuBarIcon(true);
  assert.equal(useSettingsStore.getState().showMenuBarIcon, true);
  assert.equal(storage.getItem("showMenuBarIcon"), "true");
  assert.deepEqual(changes, [true]);

  useSettingsStore.getState().setShowMenuBarIcon(true);
  assert.deepEqual(changes, [true]);
  useSettingsStore.getState().setShowMenuBarIcon(false);
  assert.deepEqual(changes, [true, false]);

  vite.moduleGraph.invalidateAll();
  const reloaded = await vite.ssrLoadModule("/stores/settingsStore.ts");
  assert.equal(reloaded.useSettingsStore.getState().showMenuBarIcon, false);
});
