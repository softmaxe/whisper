const test = require("node:test");
const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");
const Module = require("node:module");

// macOS can drop the recording pill window's all-Spaces membership (observed
// after a clamshell sleep) while AppKit still reports it. showInactive() then
// orders the window onto a Space the user is not on: Electron's isVisible()
// is true, but the window never becomes visible on screen, so Electron emits no
// "show". Dictation keeps working without its pill until a new window replaces it.

const createdWindows = [];
let nextWindowReachesScreen = true;

class FakeWebContents extends EventEmitter {
  constructor() {
    super();
    this.messages = [];
  }

  send(channel, payload) {
    this.messages.push({ channel, payload });
  }

  isLoading() {
    return false;
  }
}

class FakeBrowserWindow extends EventEmitter {
  constructor(options) {
    super();
    this.options = options;
    this.destroyed = false;
    this.visible = false;
    this.onScreen = false;
    this.pendingHide = false;
    this.reachesScreen = nextWindowReachesScreen;
    this.webContents = new FakeWebContents();
    createdWindows.push(this);
  }

  loadFile() {
    return Promise.resolve();
  }

  setIgnoreMouseEvents() {}

  setTitle() {}

  getBounds() {
    return { x: 760, y: 993, width: 208, height: 120 };
  }

  isDestroyed() {
    return this.destroyed;
  }

  isVisible() {
    return this.visible;
  }

  isMinimized() {
    return false;
  }

  // On macOS Electron raises "show" and "hide" from occlusion changes, which
  // arrive after the window server has redrawn. A window shown again before its
  // hide lands never left the screen, so it raises neither.
  showInactive() {
    this.visible = true;
    if (!this.reachesScreen) return;
    if (this.pendingHide) {
      this.pendingHide = false;
      return;
    }
    if (!this.onScreen) {
      this.onScreen = true;
      this.emit("show");
    }
  }

  hide() {
    this.visible = false;
    if (this.onScreen) this.pendingHide = true;
  }

  deliverOcclusion() {
    if (!this.pendingHide) return;
    this.pendingHide = false;
    this.onScreen = false;
    this.emit("hide");
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.visible = false;
    this.emit("closed");
  }
}

class FakeHotkeyManager {
  initializeHotkey(mainWindow) {
    this.mainWindow = mainWindow;
    this.initializeCount = (this.initializeCount || 0) + 1;
  }

  unregisterAll() {}

  isInListeningMode() {
    return false;
  }
}
FakeHotkeyManager.isGlobeLikeHotkey = () => false;

class FakeDragManager {
  setTargetWindow(window) {
    this.targetWindow = window;
  }

  cleanup() {
    this.targetWindow = null;
  }
}

const display = {
  id: 1,
  bounds: { x: 0, y: 0, width: 1728, height: 1117 },
  workArea: { x: 0, y: 33, width: 1728, height: 1084 },
};

const originalLoad = Module._load;
Module._load = function loadWindowManagerWithStubs(request, parent, isMain) {
  if (request === "electron") {
    return {
      app: { on: () => undefined },
      screen: {
        getCursorScreenPoint: () => ({ x: 0, y: 0 }),
        getDisplayNearestPoint: () => display,
        getAllDisplays: () => [display],
        on: () => undefined,
        removeListener: () => undefined,
      },
      BrowserWindow: FakeBrowserWindow,
      ipcMain: { on() {}, removeListener() {} },
      shell: {},
      dialog: {},
    };
  }
  if (request === "./debugLogger") {
    return { info() {}, warn() {}, debug() {}, error() {} };
  }
  if (request === "./hotkeyManager") return FakeHotkeyManager;
  if (request === "./dragManager") return FakeDragManager;
  if (request === "./menuManager") return { setupMainMenu() {} };
  if (request === "./devServerManager") {
    return { DEV_SERVER_PORT: 5173, getAppFilePath: () => ({ path: __filename, query: {} }) };
  }
  if (request === "./dockManager") return {};
  if (request === "./i18nMain") return { i18nMain: { t: (key) => key } };
  if (request === "./windowConfig") {
    return {
      MAIN_WINDOW_CONFIG: {},
      CONTROL_PANEL_CONFIG: {},
      WINDOW_SIZES: {},
      WindowPositionUtil: {
        getMainWindowPosition: () => ({ x: 760, y: 993 }),
        setupAlwaysOnTop() {},
      },
    };
  }
  return originalLoad.call(this, request, parent, isMain);
};
const WindowManager = require("../../src/helpers/windowManager");

Module._load = originalLoad;

const ON_SCREEN_TIMEOUT_MS = 500;

async function flushPromises() {
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
}

async function createManager(t, reachesScreen) {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  createdWindows.length = 0;
  nextWindowReachesScreen = reachesScreen;
  const manager = new WindowManager();
  manager._floatingIconAutoHide = true;
  await manager.createMainWindow();
  manager.setOnboardingActive(false);
  const replacements = [];
  manager.onMainWindowReplaced = (window) => replacements.push(window);
  // Windows created after the stranded one are healthy.
  nextWindowReachesScreen = true;
  return { manager, window: manager.mainWindow, replacements };
}

test("a dictation window that never reaches the screen is replaced once dictation is idle", async (t) => {
  const { manager, window: stranded, replacements } = await createManager(t, false);

  manager.showDictationPanel({ reposition: false });
  manager.setDictationLifecycleState("recording");
  t.mock.timers.tick(ON_SCREEN_TIMEOUT_MS);
  await flushPromises();

  // The renderer still owns the recording, so it must not be torn down yet.
  assert.equal(manager.mainWindow, stranded);
  assert.equal(stranded.isDestroyed(), false);

  manager.setDictationLifecycleState("processing");
  manager.setDictationLifecycleState("idle");
  await flushPromises();

  assert.equal(stranded.isDestroyed(), true);
  assert.equal(createdWindows.length, 2);
  const replacement = createdWindows[1];
  assert.equal(manager.mainWindow, replacement);
  assert.deepEqual(replacements, [replacement]);
  assert.equal(manager.dragManager.targetWindow, replacement);
  assert.equal(manager.hotkeyManager.mainWindow, replacement);
  // The replacement reuses the registered hotkey instead of registering it again.
  assert.equal(manager.hotkeyManager.initializeCount, 1);

  manager.showDictationPanel({ reposition: false });
  t.mock.timers.tick(ON_SCREEN_TIMEOUT_MS);
  await flushPromises();
  assert.equal(manager.mainWindow, replacement);
  assert.equal(createdWindows.length, 2);
});

test("a stranded window found while idle is replaced right away", async (t) => {
  const { manager, window: stranded } = await createManager(t, false);

  manager.showDictationPanel({ reposition: false });
  t.mock.timers.tick(ON_SCREEN_TIMEOUT_MS);
  await flushPromises();

  assert.equal(stranded.isDestroyed(), true);
  assert.equal(manager.mainWindow, createdWindows[1]);
});

test("a dictation window that reaches the screen is kept", async (t) => {
  const { manager, window, replacements } = await createManager(t, true);

  manager.showDictationPanel({ reposition: false });
  t.mock.timers.tick(ON_SCREEN_TIMEOUT_MS);
  await flushPromises();

  assert.equal(manager.mainWindow, window);
  assert.equal(createdWindows.length, 1);
  assert.deepEqual(replacements, []);
});

test("a window shown again before its hide reached the screen is kept", async (t) => {
  const { manager, window } = await createManager(t, true);

  manager.showDictationPanel({ reposition: false });
  manager.hideDictationPanel();
  manager.showDictationPanel({ reposition: false });
  t.mock.timers.tick(ON_SCREEN_TIMEOUT_MS);
  await flushPromises();

  assert.equal(manager.mainWindow, window);
  assert.equal(createdWindows.length, 1);

  // A later show from off screen is still checked.
  manager.hideDictationPanel();
  window.deliverOcclusion();
  window.reachesScreen = false;
  manager.showDictationPanel({ reposition: false });
  t.mock.timers.tick(ON_SCREEN_TIMEOUT_MS);
  await flushPromises();

  assert.equal(window.isDestroyed(), true);
});

test("a window hidden before it could reach the screen is kept", async (t) => {
  const { manager, window } = await createManager(t, false);

  manager.showDictationPanel({ reposition: false });
  manager.hideDictationPanel();
  t.mock.timers.tick(ON_SCREEN_TIMEOUT_MS);
  await flushPromises();

  assert.equal(manager.mainWindow, window);
  assert.equal(createdWindows.length, 1);
});
