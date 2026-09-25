const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");
const { EventEmitter } = require("node:events");

class TestTray extends EventEmitter {
  static instances = [];

  constructor() {
    super();
    this.destroyed = false;
    TestTray.instances.push(this);
  }

  setIgnoreDoubleClickEvents() {}
  setToolTip() {}
  setContextMenu() {}
  destroy() {
    this.destroyed = true;
  }
}

const originalLoad = Module._load;
Module._load = function loadTrayWithStubs(request, parent, isMain) {
  if (request === "electron") {
    return {
      Tray: TestTray,
      Menu: { buildFromTemplate: (template) => template },
      nativeImage: {},
      app: {},
    };
  }
  if (request === "./debugLogger") return { info: () => undefined, debug: () => undefined };
  if (request === "./dockManager") return {};
  if (request === "./i18nMain") return { i18nMain: { t: (key) => key } };
  return originalLoad.call(this, request, parent, isMain);
};
const TrayManager = require("../../src/helpers/tray");
Module._load = originalLoad;

function createTrayManager(calls, { dictating = false } = {}) {
  const trayManager = new TrayManager();
  trayManager.windowManager = {
    isDictationPanelVisible: () => false,
    isDictating: () => dictating,
    sendStartDictation: () => calls.push("start-dictation"),
    sendStopDictation: () => calls.push("stop-dictation"),
  };
  return trayManager;
}

test("the tray offers dictation first", () => {
  const calls = [];
  const entries = createTrayManager(calls).buildContextMenuTemplate();
  const [listen, separator] = entries;
  assert.equal(listen.label, "app.commandMenu.startListening");
  assert.equal(separator.type, "separator");
  listen.click();
  assert.deepEqual(calls, ["start-dictation"]);
});

test("the tray's listen item stops the recording it reflects", () => {
  const calls = [];
  const [listen] = createTrayManager(calls, { dictating: true }).buildContextMenuTemplate();

  assert.equal(listen.label, "app.commandMenu.stopListening");
  listen.click();
  assert.deepEqual(calls, ["stop-dictation"]);
});

test("menu bar visibility creates one tray and hides it immediately", async () => {
  TestTray.instances = [];
  const manager = new TrayManager();
  let resolveIcon;
  let loads = 0;
  manager.loadTrayIcon = () => {
    loads += 1;
    return new Promise((resolve) => {
      resolveIcon = resolve;
    });
  };

  const first = manager.setVisible(true);
  const second = manager.setVisible(true);
  resolveIcon({ isEmpty: () => false });
  await Promise.all([first, second]);

  assert.equal(loads, 1);
  assert.equal(TestTray.instances.length, 1);
  const tray = manager.tray;
  await manager.setVisible(false);
  assert.equal(tray.destroyed, true);
  assert.equal(manager.tray, null);
  await manager.createTray();
  assert.equal(TestTray.instances.length, 1);
});

test("hiding while the icon loads does not create a late menu bar icon", async () => {
  TestTray.instances = [];
  const manager = new TrayManager();
  let resolveIcon;
  manager.loadTrayIcon = () =>
    new Promise((resolve) => {
      resolveIcon = resolve;
    });

  const creation = manager.setVisible(true);
  await manager.setVisible(false);
  resolveIcon({ isEmpty: () => false });
  await creation;
  assert.equal(TestTray.instances.length, 0);

  manager.loadTrayIcon = async () => ({ isEmpty: () => false });
  await manager.setVisible(true);
  assert.equal(TestTray.instances.length, 1);
});

test("rapid visibility changes preserve the newest tray after an old destroy event", async () => {
  TestTray.instances = [];
  const manager = new TrayManager();
  manager.loadTrayIcon = async () => ({ isEmpty: () => false });
  await manager.setVisible(true);
  const oldTray = manager.tray;
  await manager.setVisible(false);
  await manager.setVisible(true);
  const currentTray = manager.tray;

  oldTray.emit("destroyed");
  assert.equal(manager.tray, currentTray);
  assert.equal(currentTray.destroyed, false);
  assert.equal(TestTray.instances.length, 2);
});

test("showing during a cancelled creation's completion starts a new tray", async () => {
  TestTray.instances = [];
  const manager = new TrayManager();
  let resolveIcon;
  manager.loadTrayIcon = () =>
    new Promise((resolve) => {
      resolveIcon = resolve;
    });
  const first = manager.setVisible(true);
  await manager.setVisible(false);
  resolveIcon({ isEmpty: () => false });
  const shownAgain = Promise.resolve().then(() => {
    manager.loadTrayIcon = async () => ({ isEmpty: () => false });
    return manager.setVisible(true);
  });

  await Promise.all([first, shownAgain]);
  assert.equal(TestTray.instances.length, 1);
  assert.ok(manager.tray);
});
