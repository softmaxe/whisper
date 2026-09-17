const test = require("node:test");
const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");
const Module = require("node:module");

const { deferred } = require("./harness/deferred");
const { WINDOW_SIZES } = require("../../src/helpers/windowConfig");

const LAPTOP = {
  id: 1,
  bounds: { x: 0, y: 0, width: 1512, height: 982 },
  workArea: { x: 0, y: 33, width: 1512, height: 949 },
};
const EXTERNAL = {
  id: 2,
  bounds: { x: -451, y: -1440, width: 2560, height: 1440 },
  workArea: { x: -451, y: -1440, width: 2560, height: 1415 },
};
const DISPLAY_EVENTS = ["display-added", "display-removed", "display-metrics-changed"];
let displays;
let cursor;
const screen = new EventEmitter();
const ipcMain = new EventEmitter();

screen.getPrimaryDisplay = () => displays[0];
screen.getAllDisplays = () => [...displays];
screen.getCursorScreenPoint = () => cursor;
screen.getDisplayNearestPoint = (point) => {
  const distance = ({ bounds }) => {
    const dx = Math.max(bounds.x - point.x, 0, point.x - bounds.x - bounds.width);
    const dy = Math.max(bounds.y - point.y, 0, point.y - bounds.y - bounds.height);
    return dx * dx + dy * dy;
  };
  return displays.reduce((nearest, display) =>
    distance(display) < distance(nearest) ? display : nearest
  );
};
screen.getDisplayMatching = (bounds) =>
  screen.getDisplayNearestPoint({
    x: bounds.x + bounds.width / 2,
    y: bounds.y + bounds.height / 2,
  });

class FakeWindow extends EventEmitter {
  constructor(bounds) {
    super();
    this.bounds = { ...bounds };
    this.changes = [];
    this.destroyed = false;
    this.visible = true;
    this.webContents = {
      send: (channel, payload) => {
        if (channel === "main-window-will-resize" && payload.token !== undefined) {
          ipcMain.emit("main-window-resize-mask-ready", {}, payload.token);
        }
      },
    };
  }

  getBounds() {
    return { ...this.bounds };
  }

  setBounds(bounds) {
    this.bounds = { ...bounds };
    this.changes.push(this.getBounds());
    this.emit("move");
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

  showInactive() {
    this.visible = true;
  }

  close() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.emit("closed");
  }
}

class FakeDragManager {
  async startWindowDrag() {
    this.active = true;
    return { success: true };
  }

  async stopWindowDrag() {
    this.active = false;
    return { success: true };
  }

  isDragActive() {
    return Boolean(this.active);
  }

  cleanup() {
    this.active = false;
  }
}

const originalLoad = Module._load;
let WindowManager;
try {
  Module._load = function loadWithPlatformStubs(request, parent, isMain) {
    if (request === "electron") {
      return {
        app: { on() {} },
        screen,
        ipcMain,
        BrowserWindow: FakeWindow,
        shell: {},
        dialog: {},
        Menu: {},
      };
    }
    if (request === "./debugLogger") return { warn() {}, debug() {}, info() {} };
    if (request === "./linuxWindowInputRegion") return {};
    if (request === "./hotkeyManager") return class {};
    if (request === "./dragManager") return FakeDragManager;
    if (request === "./menuManager" || request === "./dockManager") return {};
    if (request === "./devServerManager") return { DEV_SERVER_PORT: 5173 };
    if (request === "./i18nMain") return { i18nMain: { t: (key) => key } };
    return originalLoad.call(this, request, parent, isMain);
  };
  WindowManager = require("../../src/helpers/windowManager");
} finally {
  Module._load = originalLoad;
}

test.beforeEach(() => {
  displays = structuredClone([LAPTOP, EXTERNAL]);
  cursor = { x: 500, y: 400 };
});

function createManager(t, { position = "center", bounds, listen = false } = {}) {
  const manager = new WindowManager();
  const win = new FakeWindow(bounds || { x: 652, y: 858, ...WINDOW_SIZES.BASE });
  manager.mainWindow = win;
  manager._panelStartPosition = position;
  manager._onboardingActive = false;
  if (listen) manager.registerMainWindowEvents();
  t.after(() => win.close());
  return { manager, win };
}

function assertCentered(win, display) {
  const bounds = win.getBounds();
  const area = display.workArea;
  assert.ok(
    Math.abs(bounds.x + bounds.width / 2 - area.x - area.width / 2) <= 0.5,
    "the visible window must share the work area's horizontal midpoint"
  );
  assert.equal(bounds.y + bounds.height, area.y + area.height - 4);
}

const settlePlacement = () => new Promise((resolve) => setImmediate(resolve));

test("showing a centered pill corrects an offset on the same display", async (t) => {
  const { manager, win } = createManager(t, {
    bounds: { x: 1100, y: 400, ...WINDOW_SIZES.BASE },
  });
  win.visible = false;

  manager.showDictationPanel();
  await settlePlacement();

  assert.equal(win.isVisible(), true);
  assertCentered(win, displays[0]);
  assert.equal(win.changes.length, 1);
});

test("stopping dictation with reposition disabled does not move to another screen", async (t) => {
  const { manager, win } = createManager(t);
  cursor = { x: 100, y: -700 };

  manager.showDictationPanel({ reposition: false });
  await settlePlacement();

  assertCentered(win, displays[0]);
  assert.equal(win.changes.length, 0);
});

test("an already centered pill does not ask the compositor to move", async (t) => {
  const { manager, win } = createManager(t);

  await manager._repositionToActiveDisplay();
  await manager.resizeMainWindow("RECORDING");
  await manager.resizeMainWindow("BASE");

  assertCentered(win, displays[0]);
  assert.equal(win.changes.length, 0);
});

test("dragging in center mode snaps to the drop display and retains automatic placement", async (t) => {
  const { manager, win } = createManager(t);
  await manager.startWindowDrag();
  win.bounds = { x: -200, y: -600, ...WINDOW_SIZES.BASE };

  await manager.stopWindowDrag();

  assertCentered(win, displays[1]);
  assert.equal(win.changes.length, 1);
  // The pointer still belongs to the laptop. A later activation follows it.
  await manager._repositionToActiveDisplay();
  assertCentered(win, displays[0]);
  assert.equal(win.changes.length, 2);
});

for (const position of ["bottom-left", "bottom-right"]) {
  test(`${position} retains the user's dragged position across display changes`, async (t) => {
    const { manager, win } = createManager(t, { position, listen: true });
    await manager.startWindowDrag();
    const dragged = { x: 400, y: 450, ...WINDOW_SIZES.BASE };
    win.bounds = { ...dragged };
    await manager.stopWindowDrag();

    cursor = { x: 100, y: -700 };
    screen.emit("display-metrics-changed", {}, displays[1], ["workArea"]);
    await manager._repositionToActiveDisplay();
    await manager.resizeMainWindow("WITH_MENU");
    await manager.resizeMainWindow("BASE");

    assert.deepEqual(win.getBounds(), dragged);
    assert.equal(win.changes.length, 2, "only the menu's grow and shrink should move the window");
  });
}

test("a click without movement keeps automatic placement available in every position", async (t) => {
  for (const position of ["center", "bottom-left", "bottom-right"]) {
    const { manager, win } = createManager(t, { position });
    await manager.startWindowDrag();
    await manager.stopWindowDrag();
    assert.equal(win.changes.length, 0, `${position} should not move on a click`);

    cursor = { x: 100, y: -700 };
    await manager._repositionToActiveDisplay();
    assert.equal(win.changes.length, 1, `${position} should still follow the active display`);
    assert.ok(win.getBounds().y < 0);
    cursor = { x: 500, y: 400 };
  }
});

test("center follows resolution, monitor layout, and Dock work-area changes", async (t) => {
  const { win } = createManager(t, { listen: true });
  const laptop = displays[0];
  for (const workArea of [
    { x: 0, y: 33, width: 1281, height: 767 },
    { x: -1800, y: 150, width: 1281, height: 767 },
    { x: -1730, y: 150, width: 1211, height: 700 },
  ]) {
    laptop.bounds = { ...workArea };
    laptop.workArea = { ...workArea };
    cursor = { x: workArea.x + 500, y: workArea.y + 100 };
    screen.emit("display-metrics-changed", {}, laptop, ["bounds", "workArea", "scaleFactor"]);
    await settlePlacement();
    assertCentered(win, laptop);
  }
  assert.equal(win.changes.length, 3);
});

test("removing an external display returns a centered pill to the remaining screen", async (t) => {
  const { win } = createManager(t, {
    bounds: { x: 725, y: -149, ...WINDOW_SIZES.BASE },
    listen: true,
  });
  const removed = displays.pop();
  screen.emit("display-removed", {}, removed);
  await settlePlacement();

  assertCentered(win, displays[0]);
  assert.equal(win.changes.length, 1);
});

test("connecting a display corrects the current screen without following the pointer", async (t) => {
  const { win } = createManager(t, {
    bounds: { x: 1000, y: 700, ...WINDOW_SIZES.BASE },
    listen: true,
  });
  const added = displays[1];
  cursor = { x: 100, y: -700 };
  screen.emit("display-added", {}, added);
  await settlePlacement();

  assertCentered(win, displays[0]);
  assert.equal(win.changes.length, 1);
});

test("display metrics changes do not follow the last dictation target on another screen", async (t) => {
  const { manager, win } = createManager(t, {
    bounds: { x: 725, y: -149, ...WINDOW_SIZES.BASE },
    listen: true,
  });
  manager.textEditMonitor = {
    lastTargetPid: 42,
    getTargetWindowBounds: async () => ({ x: 100, y: 100, width: 700, height: 500 }),
  };
  displays[1].workArea = { x: -380, y: -1440, width: 2489, height: 1360 };
  screen.emit("display-metrics-changed", {}, displays[1], ["workArea"]);
  await settlePlacement();

  assertCentered(win, displays[1]);
  assert.equal(win.changes.length, 1);
});

test("growing and restoring center recomputes the midpoint instead of restoring stale bounds", async (t) => {
  const { manager, win } = createManager(t, {
    bounds: { x: 1000, y: 700, ...WINDOW_SIZES.BASE },
  });
  await manager.resizeMainWindow("WITH_MENU");
  assertCentered(win, displays[0]);

  displays[0].workArea = { x: 72, y: 33, width: 1209, height: 820 };
  await manager.resizeMainWindow("BASE");

  assertCentered(win, displays[0]);
  assert.equal(win.getBounds().width, WINDOW_SIZES.BASE.width);
  assert.equal(win.getBounds().height, WINDOW_SIZES.BASE.height);
});

test("choosing center while expanded prevents a later shrink from restoring a dragged position", async (t) => {
  const { manager, win } = createManager(t, {
    position: "bottom-right",
    bounds: { x: 1000, y: 650, ...WINDOW_SIZES.BASE },
  });
  await manager.resizeMainWindow("WITH_MENU");
  await manager.setPanelStartPosition("center");
  assertCentered(win, displays[0]);

  await manager.resizeMainWindow("BASE");
  assertCentered(win, displays[0]);
});

test("choosing center waits for an in-flight resize and keeps later restores centered", async (t) => {
  const initial = { x: 1000, y: 650, ...WINDOW_SIZES.BASE };
  const { manager, win } = createManager(t, { position: "bottom-right", bounds: initial });
  const maskReady = deferred();
  const releaseMask = deferred();
  manager._prepareRendererForMainWindowResize = () => {
    maskReady.resolve();
    return releaseMask.promise;
  };
  const resizing = manager.resizeMainWindow("WITH_MENU");
  await maskReady.promise;

  const choosingCenter = manager.setPanelStartPosition("center");
  await settlePlacement();
  assert.deepEqual(win.getBounds(), initial);
  assert.equal(win.changes.length, 0, "the setting must wait for the pending resize");

  releaseMask.resolve();
  await Promise.all([resizing, choosingCenter]);
  assertCentered(win, displays[0]);
  assert.equal(win.getBounds().width, WINDOW_SIZES.WITH_MENU.width);

  await manager.resizeMainWindow("BASE");
  assertCentered(win, displays[0]);
  assert.equal(win.getBounds().width, WINDOW_SIZES.BASE.width);
});

test("closing the window removes display listeners and cancels an unresolved placement", async (t) => {
  const { manager, win } = createManager(t, { listen: true });
  const targetPid = deferred();
  const pending = manager._repositionToActiveDisplay(targetPid.promise);
  for (const event of DISPLAY_EVENTS) assert.equal(screen.listenerCount(event), 1);

  win.close();
  cursor = { x: 100, y: -700 };
  targetPid.resolve(null);
  await pending;

  for (const event of DISPLAY_EVENTS) assert.equal(screen.listenerCount(event), 0);
  assert.equal(win.changes.length, 0);
  assert.equal(manager.mainWindow, null);
});
