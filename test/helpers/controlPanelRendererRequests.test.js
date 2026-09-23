const test = require("node:test");
const assert = require("node:assert/strict");

let loadCount = 0;
// Each test gets fresh module state.
const loadFresh = (path) => import(`${path}?case=${++loadCount}`);

function installElectronApi(t, electronAPI) {
  const originalWindow = globalThis.window;
  t.after(() => {
    globalThis.window = originalWindow;
  });
  globalThis.window = { electronAPI };
}

test("a settings request that arrives before the panel mounts opens Settings on mount", async (t) => {
  let deliver;
  installElectronApi(t, {
    onShowSettings: (callback) => {
      deliver = callback;
      return () => {};
    },
  });
  const { listenForSettingsRequests, onSettingsRequested } = await loadFresh(
    "../../src/utils/settingsRequests.ts"
  );

  listenForSettingsRequests();
  deliver();

  let opened = 0;
  onSettingsRequested(() => opened++);
  assert.equal(opened, 1);

  onSettingsRequested(() => opened++);
  assert.equal(opened, 1, "a delivered request is not replayed");
});

test("a mounted panel receives settings requests directly", async (t) => {
  let deliver;
  installElectronApi(t, {
    onShowSettings: (callback) => {
      deliver = callback;
      return () => {};
    },
  });
  const { listenForSettingsRequests, onSettingsRequested } = await loadFresh(
    "../../src/utils/settingsRequests.ts"
  );

  listenForSettingsRequests();
  let opened = 0;
  const unsubscribe = onSettingsRequested(() => opened++);
  deliver();
  deliver();
  assert.equal(opened, 2);

  unsubscribe();
  deliver();
  assert.equal(opened, 2);
});

test("the panel reports retention only when the set of holds changes state", async (t) => {
  const reports = [];
  installElectronApi(t, { setControlPanelRetained: (retained) => reports.push(retained) });
  const { setControlPanelHold } = await loadFresh("../../src/utils/controlPanelRetention.ts");

  setControlPanelHold("settings", true);
  setControlPanelHold("upload-batch", true);
  setControlPanelHold("settings", false);
  assert.deepEqual(reports, [true]);

  setControlPanelHold("upload-batch", false);
  setControlPanelHold("upload-batch", false);
  assert.deepEqual(reports, [true, false]);
});
