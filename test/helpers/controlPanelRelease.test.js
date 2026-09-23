const test = require("node:test");
const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");
const Module = require("node:module");

const RELEASE_DELAY_MS = 60_000;
const createdWindows = [];
const dockVisibility = [];

class FakeWebContents extends EventEmitter {
  constructor() {
    super();
    this.messages = [];
  }

  send(channel, payload) {
    this.messages.push({ channel, payload });
  }

  setWindowOpenHandler() {}

  isCrashed() {
    return false;
  }
}

class FakeBrowserWindow extends EventEmitter {
  constructor(options) {
    super();
    this.options = options;
    this.destroyed = false;
    this.visible = false;
    this.webContents = new FakeWebContents();
    createdWindows.push(this);
  }

  loadFile() {
    return Promise.resolve();
  }

  setTitle() {}

  isDestroyed() {
    return this.destroyed;
  }

  isVisible() {
    return this.visible;
  }

  isMinimized() {
    return false;
  }

  show() {
    this.visible = true;
    this.emit("show");
  }

  hide() {
    this.visible = false;
    this.emit("hide");
  }

  focus() {}

  close() {
    const event = { defaultPrevented: false, preventDefault: () => (event.defaultPrevented = true) };
    this.emit("close", event);
    if (!event.defaultPrevented) this.destroy();
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.visible = false;
    this.emit("closed");
  }
}

class FakeHotkeyManager {
  unregisterAll() {}

  isInListeningMode() {
    return false;
  }
}
FakeHotkeyManager.isGlobeLikeHotkey = () => false;

const originalLoad = Module._load;
Module._load = function loadWindowManagerWithStubs(request, parent, isMain) {
  if (request === "electron") {
    return {
      app: { on: () => undefined },
      screen: { getPrimaryDisplay: () => ({}), on: () => undefined },
      BrowserWindow: FakeBrowserWindow,
      shell: {},
      dialog: {},
    };
  }
  if (request === "./debugLogger") {
    return { info() {}, warn() {}, debug() {}, error() {} };
  }
  if (request === "./hotkeyManager") return FakeHotkeyManager;
  if (request === "./dragManager") return class FakeDragManager {};
  if (request === "./menuManager") return { setupControlPanelMenu() {} };
  if (request === "./devServerManager") {
    return {
      DEV_SERVER_PORT: 5173,
      getAppUrl: () => null,
      getAppFilePath: () => ({ path: __filename, query: { panel: "true" } }),
    };
  }
  if (request === "./dockManager") {
    return { setControlPanelVisible: (visible) => dockVisibility.push(visible) };
  }
  if (request === "./i18nMain") return { i18nMain: { t: (key) => key } };
  if (request === "./windowConfig") {
    return {
      MAIN_WINDOW_CONFIG: {},
      CONTROL_PANEL_CONFIG: {},
      NOTIFICATION_WINDOW_CONFIG: {},
      WINDOW_SIZES: {},
      WindowPositionUtil: {},
    };
  }
  return originalLoad.call(this, request, parent, isMain);
};
const WindowManager = require("../../src/helpers/windowManager");

Module._load = originalLoad;

async function createManagerWithOpenPanel(t) {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  createdWindows.length = 0;
  dockVisibility.length = 0;
  const manager = new WindowManager();
  const dictationWindow = new FakeBrowserWindow({});
  dictationWindow.visible = true;
  manager.mainWindow = dictationWindow;
  await manager.createControlPanelWindow();
  const panel = manager.controlPanelWindow;
  panel.webContents.emit("did-finish-load");
  // AppRouter commits the normal app, which releases the dictation gate.
  manager.setOnboardingActive(false);
  panel.show();
  dictationWindow.webContents.messages.length = 0;
  return { manager, panel, dictationWindow };
}

test("a hidden control panel is released after the grace period", async (t) => {
  const { manager, panel } = await createManagerWithOpenPanel(t);

  panel.close();
  assert.equal(panel.isDestroyed(), false, "closing only hides the panel");

  t.mock.timers.tick(RELEASE_DELAY_MS - 1);
  assert.equal(panel.isDestroyed(), false);

  t.mock.timers.tick(1);
  assert.equal(panel.isDestroyed(), true);
  assert.equal(manager.controlPanelWindow, null);
});

test("reopening within the grace period keeps the same renderer", async (t) => {
  const { manager, panel } = await createManagerWithOpenPanel(t);

  panel.close();
  t.mock.timers.tick(RELEASE_DELAY_MS / 2);
  await manager.createControlPanelWindow();
  t.mock.timers.tick(RELEASE_DELAY_MS);

  assert.equal(panel.isDestroyed(), false);
  assert.equal(manager.controlPanelWindow, panel);
});

test("retained work keeps the hidden panel until it finishes", async (t) => {
  const { manager, panel } = await createManagerWithOpenPanel(t);

  manager.setControlPanelRetained(true);
  panel.close();
  t.mock.timers.tick(RELEASE_DELAY_MS * 3);
  assert.equal(panel.isDestroyed(), false);

  manager.setControlPanelRetained(false);
  t.mock.timers.tick(RELEASE_DELAY_MS);
  assert.equal(panel.isDestroyed(), true);
});

test("releasing the panel leaves dictation available", async (t) => {
  const { manager, panel, dictationWindow } = await createManagerWithOpenPanel(t);

  panel.close();
  t.mock.timers.tick(RELEASE_DELAY_MS);

  assert.equal(panel.isDestroyed(), true);
  assert.equal(manager._onboardingActive, false);
  assert.equal(dictationWindow.isVisible(), true);
  assert.deepEqual(dictationWindow.webContents.messages, []);
});

test("a panel recreated after a release does not cancel dictation when it loads", async (t) => {
  const { manager, panel, dictationWindow } = await createManagerWithOpenPanel(t);

  panel.close();
  t.mock.timers.tick(RELEASE_DELAY_MS);
  await manager.createControlPanelWindow();
  const recreated = manager.controlPanelWindow;
  assert.notEqual(recreated, panel);

  recreated.webContents.emit("did-finish-load");
  assert.equal(manager._onboardingActive, false);
  assert.deepEqual(dictationWindow.webContents.messages, []);
});

test("quitting closes the panel without scheduling a release", async (t) => {
  const { manager, panel } = await createManagerWithOpenPanel(t);

  manager.isQuitting = true;
  panel.close();
  assert.equal(panel.isDestroyed(), true);
  assert.equal(manager._onboardingActive, true);
});
