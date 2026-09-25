const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");

const handlersModulePath = require.resolve("../../src/helpers/ipcHandlers");
const originalLoad = Module._load;

// Captures every ipcMain.handle registration and every net.fetch request so the
// registered handler closures can be invoked directly against a fake `this`.
const handlers = new Map();
const fetches = [];
const databaseWrites = [];
let fetchResponse = () => ({
  ok: true,
  status: 200,
  json: async () => ({ text: "transcribed" }),
  text: async () => JSON.stringify({ text: "transcribed" }),
});

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
    on: () => {},
    removeHandler: () => {},
  },
  net: {
    fetch: async (url, init) => {
      fetches.push({ url: String(url), init });
      return fetchResponse(String(url), init);
    },
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

// Kept installed for the whole file so the handler module sees the stubs.
Module._load = function loadWithMocks(request, parent, isMain) {
  if (request === "electron") return electronStub;
  if (parent?.filename === handlersModulePath) {
    if (request === "./windowBroadcast") {
      return { broadcastToWindows: () => {} };
    }
  }
  return originalLoad.call(this, request, parent, isMain);
};

// A permissive `this` for setupHandlers: registration only stores closures, so
// any manager not exercised by the retry handler can be an inert stub.
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

function buildFakeThis() {
  const dbRows = new Map([[7, { id: 7, audio_duration_ms: 1200 }]]);
  const target = {
    audioStorageManager: { getAudioBuffer: (id) => (id === 7 ? Buffer.from([1, 2, 3]) : null) },
    databaseManager: {
      updateTranscriptionText: (...args) => databaseWrites.push(["text", ...args]),
      updateTranscriptionStatus: (...args) => databaseWrites.push(["status", ...args]),
      updateTranscriptionAudio: (...args) => databaseWrites.push(["audio", ...args]),
      getTranscriptionById: (id) => dbRows.get(id),
    },
  };
  return new Proxy(target, {
    get: (t, prop) => (prop in t ? t[prop] : anything()),
  });
}

let retryHandler;
test.before(() => {
  delete require.cache[handlersModulePath];
  const IPCHandlers = require(handlersModulePath);
  const Ctor = IPCHandlers.default || IPCHandlers;
  Ctor.prototype.setupHandlers.call(buildFakeThis());
  retryHandler = handlers.get("retry-transcription");
  assert.ok(retryHandler, "retry-transcription must be registered");
});

test.after(() => {
  Module._load = originalLoad;
});

const fsNode = require("node:fs");
const osNode = require("node:os");
const pathNode = require("node:path");

const uploadTempDir = fsNode.mkdtempSync(pathNode.join(osNode.tmpdir(), "whisper-upload-handler-"));
const uploadTempFile = pathNode.join(uploadTempDir, "audio.webm");
test.after(() => fsNode.rmSync(uploadTempDir, { recursive: true, force: true }));

const invokeUpload = (payload) => {
  const uploadHandler = handlers.get("transcribe-audio-file");
  assert.ok(uploadHandler, "transcribe-audio-file must be registered");
  fsNode.writeFileSync(uploadTempFile, Buffer.from([1, 2, 3, 4]));
  return uploadHandler({ sender: {} }, { filePath: uploadTempFile, ...payload });
};

test("upload: a file goes to the configured self-hosted endpoint", async () => {
  fetches.length = 0;
  const result = await invokeUpload({
    language: "",
    remoteTranscriptionUrl: "https://stt.internal.example.com",
    remoteTranscriptionModel: "tiny",
  });
  assert.equal(result.success, true);
  assert.equal(fetches[0].url, "https://stt.internal.example.com/audio/transcriptions");
});

test("upload: a missing self-hosted endpoint fails without a request", async () => {
  fetches.length = 0;
  const result = await invokeUpload({ remoteTranscriptionUrl: "" });
  assert.equal(result.success, false);
  assert.equal(fetches.length, 0);
});

test("upload: server failures surface without a successful transcript", async () => {
  fetches.length = 0;
  const previous = fetchResponse;
  fetchResponse = () => ({
    ok: false,
    status: 503,
    text: async () => JSON.stringify({ error: { message: "ASR unavailable" } }),
  });
  try {
    const result = await invokeUpload({
      remoteTranscriptionUrl: "http://localhost:8000/v1",
      remoteTranscriptionModel: "test-asr",
    });
    assert.equal(result.success, false);
    assert.match(result.error, /ASR unavailable/);
  } finally {
    fetchResponse = previous;
  }
});

test("retry sends retained audio to the current self-hosted endpoint and updates History", async () => {
  fetches.length = 0;
  databaseWrites.length = 0;
  const result = await retryHandler({ sender: {} }, 7, {
    remoteTranscriptionUrl: "http://localhost:8000/v1",
    remoteTranscriptionModel: "test-asr",
    preferredLanguage: "zh-CN",
  });
  assert.equal(result.success, true);
  assert.equal(fetches[0].url, "http://localhost:8000/v1/audio/transcriptions");
  assert.equal(fetches[0].init.body.get("model"), "test-asr");
  assert.equal(fetches[0].init.body.get("language"), "zh");
  assert.deepEqual(databaseWrites, [
    ["text", 7, "transcribed", "transcribed"],
    ["status", 7, "completed"],
    [
      "audio",
      7,
      { hasAudio: 1, audioDurationMs: 1200, provider: "self-hosted", model: "test-asr" },
    ],
  ]);
});

test("retry preserves History when the self-hosted request fails or retained audio is missing", async () => {
  databaseWrites.length = 0;
  const previous = fetchResponse;
  fetchResponse = () => ({ ok: false, status: 503, text: async () => "ASR unavailable" });
  try {
    const settings = {
      remoteTranscriptionUrl: "http://localhost:8000/v1",
      remoteTranscriptionModel: "test-asr",
    };
    const failed = await retryHandler({ sender: {} }, 7, settings);
    assert.equal(failed.success, false);
    assert.match(failed.error, /ASR unavailable/);
    const missing = await retryHandler({ sender: {} }, 99, settings);
    assert.deepEqual(missing, { success: false, error: "Audio file not found" });
    assert.deepEqual(databaseWrites, []);
  } finally {
    fetchResponse = previous;
  }
});
