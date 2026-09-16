const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");

const handlersModulePath = require.resolve("../../src/helpers/ipcHandlers");
const originalLoad = Module._load;

// Captures every ipcMain.handle/on registration so the registered handler
// closures can be invoked directly against a fake `this`, mirroring the
// scaffolding in test/helpers/retryTranscriptionHandler.test.js.
const handlers = new Map();
const onHandlers = new Map();

const electronStub = {
  app: {
    getPath: () => "/tmp",
    getName: () => "test",
    getVersion: () => "0.0.0",
    isPackaged: false,
    on: () => {},
    requestSingleInstanceLock: () => true,
  },
  ipcMain: {
    handle: (channel, fn) => handlers.set(channel, fn),
    on: (channel, fn) => onHandlers.set(channel, fn),
    removeHandler: () => {},
  },
  net: {
    fetch: async () => ({
      ok: true,
      status: 200,
      json: async () => ({}),
      text: async () => "{}",
    }),
  },
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

// Kept installed for the whole file: some helpers are require()d lazily at
// handler invocation time, not at module load.
Module._load = function loadWithMocks(request, parent, isMain) {
  if (request === "electron") return electronStub;
  if (parent?.filename === handlersModulePath) {
    if (request === "./cortiTranscription") {
      return { transcribeAudio: async () => ({ text: "corti text" }) };
    }
    if (request === "./tinfoilTranscription") {
      return {
        transcribeWithTinfoil: async () => ({ text: "tinfoil text", model: "tinfoil-model" }),
        getTinfoilChatModels: () => [],
      };
    }
    if (request === "./windowBroadcast") {
      return { broadcastToWindows: () => {} };
    }
  }
  return originalLoad.call(this, request, parent, isMain);
};

// A permissive `this` for setupHandlers: registration only stores closures, so
// any manager not exercised by the pill handlers can be an inert stub.
function anything() {
  return new Proxy(function () {}, {
    get: (t, prop) => {
      if (prop === Symbol.toPrimitive || prop === "toString") return () => "";
      if (prop === "then") return undefined;
      return anything();
    },
    apply: () => anything(),
  });
}

// `target` stays a plain object (not the Proxy) so tests can reassign
// `target.windowManager` between cases and have the already-registered
// handler closures observe the swap on their next invocation — they read
// `this.windowManager` fresh each call, they don't capture it at registration.
let target;
function buildFakeThis() {
  target = {
    sessionId: "test-session",
  };
  return new Proxy(target, {
    get: (t, prop) => (prop in t ? t[prop] : anything()),
  });
}

function installWindowManager(stub) {
  target.windowManager = stub;
}

function makeWindowManagerStub() {
  const calls = { toggle: 0, cancel: 0, lifecycle: [], levels: [] };
  const mainContents = {};
  const pillContents = {};
  const stub = {
    mainWindow: { isDestroyed: () => false, webContents: mainContents },
    agentDictationPillWindow: { isDestroyed: () => false, webContents: pillContents },
    getAgentDictationPillState: () => ({
      lifecycle: "idle",
      interactive: true,
      horizontalDirection: "left",
    }),
    sendToggleDictation: () => {
      calls.toggle += 1;
    },
    sendCancelActiveDictation: () => {
      calls.cancel += 1;
    },
    setDictationLifecycleState: (state, kind) => {
      calls.lifecycle.push([state, kind]);
    },
    setDictationAudioLevel: (level) => {
      calls.levels.push(level);
    },
    resizeAgentDictationPillToContent: () => ({ success: true }),
    setAgentDictationPillInteractivity: () => undefined,
  };
  return { stub, calls, mainContents, pillContents };
}

test.before(() => {
  delete require.cache[handlersModulePath];
  const IPCHandlers = require(handlersModulePath);
  const Ctor = IPCHandlers.default || IPCHandlers;
  Ctor.prototype.setupHandlers.call(buildFakeThis());
  assert.ok(
    onHandlers.get("dictation-lifecycle-state-changed"),
    "dictation-lifecycle-state-changed must be registered"
  );
  assert.ok(
    onHandlers.get("dictation-audio-level-changed"),
    "dictation-audio-level-changed must be registered"
  );
});

test("only the main renderer can change its window's input region", async () => {
  const { stub, mainContents, pillContents } = makeWindowManagerStub();
  const regions = [];
  stub.setMainWindowInputRegion = async (region) => {
    regions.push(region);
    return true;
  };
  installWindowManager(stub);
  const handler = handlers.get("set-main-window-input-region");
  const region = { viewportWidth: 208, viewportHeight: 120, x: 156, y: 68, width: 40, height: 40 };
  assert.equal(await handler({ sender: mainContents }, region), true);
  assert.equal(handler({ sender: pillContents }, null), null);
  assert.equal(handler({ sender: {} }, null), null);
  stub.mainWindow = null;
  assert.equal(handler({ sender: mainContents }, null), null);
  assert.deepEqual(regions, [region]);
});

test.after(() => {
  Module._load = originalLoad;
});

test("audio levels are accepted only from the dictation renderer", () => {
  const { stub, calls, mainContents } = makeWindowManagerStub();
  installWindowManager(stub);
  const levelHandler = onHandlers.get("dictation-audio-level-changed");

  levelHandler({ sender: {} }, 0.5);
  assert.deepEqual(calls.levels, []);
  levelHandler({ sender: mainContents }, 0.5);
  assert.deepEqual(calls.levels, [0.5]);
});

test("lifecycle reports are accepted only from the dictation renderer", () => {
  const { stub, calls, mainContents } = makeWindowManagerStub();
  installWindowManager(stub);
  const lifecycleHandler = onHandlers.get("dictation-lifecycle-state-changed");

  lifecycleHandler({ sender: {} }, "recording", "dictation");
  assert.deepEqual(calls.lifecycle, []);
  lifecycleHandler({ sender: mainContents }, "recording", "dictation");
  assert.deepEqual(calls.lifecycle, [["recording", "dictation"]]);
});
