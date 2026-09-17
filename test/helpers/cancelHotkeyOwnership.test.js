const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");

const preloadPath = require.resolve("../../preload");
const handlersPath = require.resolve("../../src/helpers/ipcHandlers");

function anything() {
  return new Proxy(function () {}, {
    get: (_target, property) => {
      if (property === Symbol.toPrimitive || property === "toString") return () => "";
      if (property === "then") return undefined;
      return anything();
    },
    apply: () => anything(),
  });
}

function loadCancelBridge(t, registration) {
  const handlers = new Map();
  const exposed = new Map();
  const calls = [];
  const sent = [];
  let pressCancel;
  const electronStub = {
    app: {
      getPath: () => "/tmp",
      getName: () => "test",
      getVersion: () => "0.0.0",
      isPackaged: false,
      on: () => {},
      requestSingleInstanceLock: () => true,
    },
    contextBridge: { exposeInMainWorld: (name, api) => exposed.set(name, api) },
    ipcRenderer: {
      invoke: async (channel, ...args) => {
        const handler = handlers.get(channel);
        assert.equal(typeof handler, "function", `No main-process handler for ${channel}`);
        return handler({ sender: { id: 1 } }, ...args);
      },
    },
    ipcMain: {
      handle: (channel, handler) => handlers.set(channel, handler),
      on: () => {},
      removeHandler: () => {},
    },
    net: { fetch: async () => ({ ok: true, status: 200, json: async () => ({}) }) },
    BrowserWindow: class BrowserWindow {
      static getAllWindows() {
        return [];
      }
      static fromWebContents() {
        return null;
      }
    },
    shell: {},
    dialog: {},
    screen: { getPrimaryDisplay: () => ({ workAreaSize: { width: 0, height: 0 } }) },
    systemPreferences: { getMediaAccessStatus: () => "granted" },
    session: { fromPartition: () => ({}) },
    clipboard: {},
    nativeImage: {},
    globalShortcut: {},
    utilityProcess: {},
    MessageChannelMain: class {},
  };
  const originalLoad = Module._load;
  Module._load = function loadWithElectronStub(request, parent, isMain) {
    if (request === "electron") return electronStub;
    if (parent?.filename === handlersPath && request === "./debugLogger") {
      return new Proxy({}, { get: () => () => {} });
    }
    return originalLoad.call(this, request, parent, isMain);
  };
  t.after(() => {
    Module._load = originalLoad;
    delete require.cache[preloadPath];
    delete require.cache[handlersPath];
  });

  const windowManager = {
    mainWindow: { webContents: { send: (...args) => sent.push(args) } },
    hotkeyManager: {
      registerSlot: async (slot, key, callback) => {
        calls.push(["register", slot, key]);
        const result = registration ? await registration(key) : { success: true, hotkey: key };
        if (result.success) pressCancel = callback;
        return result;
      },
      unregisterSlot: (slot) => {
        calls.push(["unregister", slot]);
        pressCancel = null;
      },
    },
  };
  delete require.cache[preloadPath];
  delete require.cache[handlersPath];
  require(preloadPath);
  const IPCHandlers = require(handlersPath);
  const Ctor = IPCHandlers.default || IPCHandlers;
  const target = { windowManager, sessionId: "test-session" };
  Ctor.prototype.setupHandlers.call(
    new Proxy(target, {
      get: (value, property) => (property in value ? value[property] : anything()),
    })
  );

  return {
    api: exposed.get("electronAPI"),
    handlers,
    calls,
    sent,
    pressCancel: () => {
      assert.equal(typeof pressCancel, "function", "Escape must remain registered");
      pressCancel();
    },
  };
}

test("legacy cancel callers still register, dispatch, and release Escape", async (t) => {
  const { api, handlers, calls, sent, pressCancel } = loadCancelBridge(t);
  assert.deepEqual(await handlers.get("register-cancel-hotkey")({}, "Escape"), {
    success: true,
    hotkey: "Escape",
  });
  pressCancel();
  assert.deepEqual(sent, [["cancel-hotkey-pressed"]]);
  await api.unregisterCancelHotkey();
  assert.deepEqual(calls, [
    ["register", "cancel", "Escape"],
    ["unregister", "cancel"],
  ]);
});

test("late recording release cannot remove copy recovery's Escape binding", async (t) => {
  const { api, calls, pressCancel } = loadCancelBridge(t);
  await api.registerCancelHotkey("Escape");
  await api.registerCancelHotkey("Escape", "copy-recovery");
  await api.unregisterCancelHotkey();
  await api.unregisterCancelHotkey();
  pressCancel();
  assert.deepEqual(calls, [["register", "cancel", "Escape"]]);

  await api.unregisterCancelHotkey("copy-recovery");
  assert.deepEqual(calls.at(-1), ["unregister", "cancel"]);
});

test("copy recovery cleanup cannot remove a new recording's Escape binding", async (t) => {
  const { api, calls, pressCancel } = loadCancelBridge(t);
  await api.registerCancelHotkey("Escape", "copy-recovery");
  await api.registerCancelHotkey("Escape");
  await api.unregisterCancelHotkey("copy-recovery");
  pressCancel();
  assert.deepEqual(calls, [["register", "cancel", "Escape"]]);

  await api.unregisterCancelHotkey();
  assert.deepEqual(calls.at(-1), ["unregister", "cancel"]);
});

test("failed registration does not retain an owner or block a later registration", async (t) => {
  let attempts = 0;
  const failure = { success: false, error: "Shortcut unavailable" };
  const { api, calls } = loadCancelBridge(t, async () =>
    ++attempts === 1 ? failure : { success: true, hotkey: "Escape" }
  );
  assert.deepEqual(await api.registerCancelHotkey("Escape", "copy-recovery"), failure);
  await api.unregisterCancelHotkey("copy-recovery");
  assert.deepEqual(calls, [["register", "cancel", "Escape"]]);

  assert.equal((await api.registerCancelHotkey("Escape")).success, true);
  await api.unregisterCancelHotkey();
  assert.deepEqual(calls.at(-1), ["unregister", "cancel"]);
});

test("release waits for an in-flight registration so the global binding cannot leak", async (t) => {
  const registration = Promise.withResolvers();
  const { api, calls } = loadCancelBridge(t, () => registration.promise);
  const register = api.registerCancelHotkey("Escape", "copy-recovery");
  const unregister = api.unregisterCancelHotkey("copy-recovery");
  await Promise.resolve();
  assert.deepEqual(calls, [["register", "cancel", "Escape"]]);

  registration.resolve({ success: true, hotkey: "Escape" });
  await Promise.all([register, unregister]);
  assert.deepEqual(calls, [
    ["register", "cancel", "Escape"],
    ["unregister", "cancel"],
  ]);
});

test("a rejected registration does not prevent later cancel updates", async (t) => {
  let attempts = 0;
  const { api, calls } = loadCancelBridge(t, async () => {
    if (++attempts === 1) throw new Error("Registration failed");
    return { success: true, hotkey: "Escape" };
  });
  await assert.rejects(api.registerCancelHotkey("Escape", "copy-recovery"), /Registration failed/);
  await api.registerCancelHotkey("Escape");
  await api.unregisterCancelHotkey("copy-recovery");
  assert.equal(calls.length, 2);
  await api.unregisterCancelHotkey();
  assert.deepEqual(calls.at(-1), ["unregister", "cancel"]);
});

test("invalid owners and conflicting keys cannot replace an active recovery binding", async (t) => {
  const { api, calls, pressCancel } = loadCancelBridge(t);
  await api.registerCancelHotkey("Escape", "copy-recovery");
  assert.equal((await api.registerCancelHotkey("Escape", "invalid")).success, false);
  assert.equal((await api.unregisterCancelHotkey("invalid")).success, false);
  assert.equal((await api.registerCancelHotkey("F12")).success, false);
  await api.unregisterCancelHotkey();
  pressCancel();
  assert.deepEqual(calls, [["register", "cancel", "Escape"]]);
});
