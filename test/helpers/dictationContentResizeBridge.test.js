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

function loadResizeBridge(t, windowManager) {
  const handlers = new Map();
  const exposed = new Map();
  const invocations = [];
  const electronStub = {
    app: {
      getPath: () => "/tmp",
      getName: () => "test",
      getVersion: () => "0.0.0",
      isPackaged: false,
      on: () => {},
      requestSingleInstanceLock: () => true,
    },
    contextBridge: {
      exposeInMainWorld: (name, api) => exposed.set(name, api),
    },
    ipcRenderer: {
      invoke: async (channel, ...args) => {
        invocations.push({ channel, args });
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

  return { api: exposed.get("electronAPI"), invocations };
}

test("dictation content sizing reaches the native window through the exposed preload API", async (t) => {
  const resizedHeights = [];
  const resizeResult = {
    success: true,
    changed: true,
    bounds: { x: 800, y: 600, width: 466, height: 344 },
  };
  const { api, invocations } = loadResizeBridge(t, {
    resizeAssistantWindowToContent: async (height) => {
      resizedHeights.push(height);
      return resizeResult;
    },
  });

  assert.equal(typeof api?.resizeAssistantWindowToContent, "function");
  const result = await api.resizeAssistantWindowToContent(320);

  assert.deepEqual(invocations, [{ channel: "resize-assistant-window-to-content", args: [320] }]);
  assert.deepEqual(resizedHeights, [320]);
  assert.deepEqual(result, resizeResult);
});
