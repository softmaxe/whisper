const test = require("node:test");
const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");
const Module = require("node:module");

function createDeferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

const createdWindows = [];
let devServerWaitPromise = Promise.resolve();

class FakeBrowserWindow extends EventEmitter {
  constructor(options) {
    super();
    this.options = options;
    this.destroyed = false;
    this.loadDeferred = createDeferred();
    this.messages = [];
    this.loadUrlCount = 0;
    this.showCount = 0;
    this.ignoreMouseEvents = [];
    this.webContents = {
      send: (channel, payload) => this.messages.push({ channel, payload }),
    };
    createdWindows.push(this);
  }

  setContentProtection() {}

  setIgnoreMouseEvents(ignore, options) {
    this.ignoreMouseEvents.push({ ignore, options });
  }

  loadFile() {
    return this.loadDeferred.promise;
  }

  loadURL() {
    this.loadUrlCount += 1;
    return Promise.resolve();
  }

  close() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.emit("closed");
  }

  isDestroyed() {
    return this.destroyed;
  }

  showInactive() {
    this.showCount += 1;
  }
}

class FakeHotkeyManager {
  unregisterAll() {}

  isInListeningMode() {
    return false;
  }
}
FakeHotkeyManager.isGlobeLikeHotkey = () => false;

class FakeDragManager {
  cleanup() {}
}

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
    return {
      info: () => undefined,
      warn: () => undefined,
      debug: () => undefined,
      error: () => undefined,
    };
  }
  if (request === "./hotkeyManager") return FakeHotkeyManager;
  if (request === "./dragManager") return FakeDragManager;
  if (request === "./menuManager") return {};
  if (request === "./devServerManager") {
    return {
      DEV_SERVER_PORT: 5173,
      DEV_SERVER_URL: "http://localhost:5173",
      getAppFilePath: () => ({ path: "/app/index.html", query: {} }),
      waitForDevServer: () => devServerWaitPromise,
    };
  }
  if (request === "./dockManager") return {};
  if (request === "./i18nMain") return { i18nMain: { t: (key) => key } };
  if (request === "./windowConfig") {
    const notificationSize = { width: 392, height: 92 };
    return {
      MAIN_WINDOW_CONFIG: {},
      CONTROL_PANEL_CONFIG: {},
      NOTIFICATION_WINDOW_CONFIG: { ...notificationSize, acceptFirstMouse: true },
      WINDOW_SIZES: {},
      WindowPositionUtil: {
        getNotificationPosition: () => ({
          ...notificationSize,
          x: 1000 - notificationSize.width,
          y: 16,
        }),
        setupAlwaysOnTop: () => undefined,
      },
    };
  }
  return originalLoad.call(this, request, parent, isMain);
};
const WindowManager = require("../../src/helpers/windowManager");

Module._load = originalLoad;

function createNormalWindowManager() {
  const manager = new WindowManager();
  manager.setOnboardingActive(false);
  return manager;
}

test.beforeEach(() => {
  createdWindows.length = 0;
});

test("a failed activation-mode change preserves the cached mode", async () => {
  const manager = createNormalWindowManager();
  manager.hotkeyManager.setActivationMode = async () => false;

  assert.equal(await manager.setActivationModeCache("push"), false);
  assert.equal(manager.getActivationMode(), "tap");
});

test("a Tap mode key combination starts Dictation, and pressing it again ends it", async () => {
  const manager = createNormalWindowManager();
  manager.mainWindow = new FakeBrowserWindow({});
  manager.showDictationPanel = () => undefined;

  await manager.createHotkeyCallback()("Control+Shift+R");
  const startMessages = manager.mainWindow.messages.map(({ channel }) => channel);
  assert.deepEqual(startMessages, ["prepare-dictation", "toggle-dictation"]);
  const { startupRequest } = manager.mainWindow.messages[1].payload;
  assert.ok(startupRequest.requestId);

  manager.mainWindow.messages.length = 0;
  manager.setDictationLifecycleState("recording", startupRequest.requestId);
  await manager.createHotkeyCallback()("Control+Shift+R");
  assert.deepEqual(manager.mainWindow.messages, [
    { channel: "toggle-dictation", payload: { startupRequest } },
  ]);
});

test("a second Dictation hotkey press before the renderer's first report ends the Dictation", async () => {
  const manager = createNormalWindowManager();
  manager.mainWindow = new FakeBrowserWindow({});
  manager.showDictationPanel = () => undefined;

  await manager.createHotkeyCallback()("Control+Shift+R");
  const { startupRequest } = manager.mainWindow.messages[1].payload;
  manager.mainWindow.messages.length = 0;
  await manager.createHotkeyCallback()("Control+Shift+R");

  // The stop carries the Dictation's id, so a renderer that never took up the
  // request still reports under the id main records.
  assert.deepEqual(manager.mainWindow.messages, [
    { channel: "toggle-dictation", payload: { startupRequest } },
  ]);
});

test("the Dictation hotkey is ignored while a Dictation is processing", async () => {
  const manager = createNormalWindowManager();
  manager.mainWindow = new FakeBrowserWindow({});
  manager.showDictationPanel = () => undefined;

  manager.setDictationLifecycleState("processing");
  await manager.createHotkeyCallback()("Control+Shift+R");
  assert.deepEqual(manager.mainWindow.messages, []);

  manager.setDictationLifecycleState("idle");
  await manager.createHotkeyCallback()("Control+Shift+R");
  assert.deepEqual(
    manager.mainWindow.messages.map(({ channel }) => channel),
    ["prepare-dictation", "toggle-dictation"]
  );
});

function pressedDictationId(manager) {
  const prepare = manager.mainWindow.messages.find(
    ({ channel }) => channel === "prepare-dictation"
  );
  return prepare.payload.startupRequest.requestId;
}

test("a late idle from the previous Dictation does not end the current one", async () => {
  const manager = createNormalWindowManager();
  manager.mainWindow = new FakeBrowserWindow({});
  manager.showDictationPanel = () => undefined;

  await manager.createHotkeyCallback()("Control+Shift+R");
  const previous = pressedDictationId(manager);
  manager.setDictationLifecycleState("preparing", previous);
  manager.setDictationLifecycleState("idle", previous);

  manager.mainWindow.messages.length = 0;
  await manager.createHotkeyCallback()("Control+Shift+R");
  const current = pressedDictationId(manager);
  assert.notEqual(current, previous);

  manager.setDictationLifecycleState("idle", previous);
  assert.equal(manager.isDictationActive(), true);
  manager.setDictationLifecycleState("recording", previous);
  assert.equal(manager.isDictating(), false);

  manager.setDictationLifecycleState("recording", current);
  assert.equal(manager.isDictating(), true);
  manager.setDictationLifecycleState("idle", current);
  assert.equal(manager.isDictationActive(), false);
});

test("a renderer-started Dictation is tracked under the renderer's id", async () => {
  const manager = createNormalWindowManager();
  manager.mainWindow = new FakeBrowserWindow({});
  manager.showDictationPanel = () => undefined;

  manager.setDictationLifecycleState("preparing", "pill-dictation");
  manager.setDictationLifecycleState("recording", "pill-dictation");
  assert.equal(manager.isDictating(), true);

  await manager.createHotkeyCallback()("Control+Shift+R");
  assert.deepEqual(manager.mainWindow.messages, [
    {
      channel: "toggle-dictation",
      payload: { startupRequest: { requestId: "pill-dictation", acceptedAt: null } },
    },
  ]);

  manager.setDictationLifecycleState("idle", "pill-dictation");
  assert.equal(manager.isDictationActive(), false);
});

test("the renderer's mount report clears a Dictation it never took up", async () => {
  const manager = createNormalWindowManager();
  manager.mainWindow = new FakeBrowserWindow({});
  manager.showDictationPanel = () => undefined;

  await manager.createHotkeyCallback()("Control+Shift+R");
  assert.equal(manager.isDictationActive(), true);

  manager.setDictationLifecycleState("idle");
  assert.equal(manager.isDictationActive(), false);
});

// A text-edit monitor whose captures report whichever app holds keyboard focus.
function createFocusMonitor(focusPid) {
  const monitor = {
    focusPid,
    lastTargetPid: null,
    captureTargetPid: async () => {
      monitor.lastTargetPid = monitor.focusPid;
      return monitor.focusPid;
    },
  };
  return monitor;
}

test("the Target app captured at the hotkey press stays with its Dictation until idle", async () => {
  const manager = createNormalWindowManager();
  manager.mainWindow = new FakeBrowserWindow({});
  manager.showDictationPanel = () => undefined;
  manager.textEditMonitor = createFocusMonitor(42);

  await manager.createHotkeyCallback()("Control+Shift+R");
  const dictationId = pressedDictationId(manager);
  await new Promise(setImmediate);

  // Focus moves before the renderer's own capture and later captures.
  manager.textEditMonitor.focusPid = 99;
  await manager.captureDictationTarget(dictationId);
  manager.setDictationLifecycleState("recording", dictationId);
  manager.setDictationLifecycleState("processing", dictationId);
  await manager.textEditMonitor.captureTargetPid();
  assert.equal(manager.textEditMonitor.lastTargetPid, 99);
  assert.equal(manager.getDictationTargetPid(dictationId), 42);

  manager.setDictationLifecycleState("idle", dictationId);
  assert.equal(manager.getDictationTargetPid(dictationId), null);
});

test("a renderer-started Dictation binds the Target app at its recording start", async () => {
  const manager = createNormalWindowManager();
  manager.mainWindow = new FakeBrowserWindow({});
  manager.showDictationPanel = () => undefined;
  manager.textEditMonitor = createFocusMonitor(42);

  manager.setDictationLifecycleState("preparing", "pill-dictation");
  assert.deepEqual(await manager.captureDictationTarget("pill-dictation"), {
    success: true,
    pid: 42,
  });
  await manager.captureDictationTarget("stale-dictation");
  assert.equal(manager.getDictationTargetPid("stale-dictation"), null);
  assert.equal(manager.getDictationTargetPid("pill-dictation"), 42);
});
