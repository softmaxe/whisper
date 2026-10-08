const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");
const http = require("node:http");
const { once, EventEmitter } = require("node:events");
const { execFileSync, spawn } = require("node:child_process");
const { createUploadCancelRegistry } = require("../../src/helpers/uploadCancelRegistry");
const uploadCancelRegistry = createUploadCancelRegistry();

const handlersModulePath = require.resolve("../../src/helpers/ipcHandlers");
const ffmpegModulePath = require.resolve("../../src/helpers/ffmpegUtils");
const originalLoad = Module._load;
let conversionSpawn = null;

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
  if (request === "child_process" && parent?.filename === ffmpegModulePath) {
    return {
      ...originalLoad.call(this, request, parent, isMain),
      spawn: (...args) => (conversionSpawn || spawn)(...args),
    };
  }
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
    _uploadCancelRegistry: uploadCancelRegistry,
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
    requestId: "completed-upload",
    language: "",
    remoteTranscriptionUrl: "https://stt.internal.example.com",
    remoteTranscriptionModel: "tiny",
  });
  assert.equal(result.success, true);
  assert.equal(fetches[0].url, "https://stt.internal.example.com/audio/transcriptions");
  assert.deepEqual(await handlers.get("cancel-upload-transcription")({}, "completed-upload"), {
    success: false,
  });
});

test("upload: a missing self-hosted endpoint fails without a request", async () => {
  fetches.length = 0;
  const result = await invokeUpload({ requestId: "invalid-upload", remoteTranscriptionUrl: "" });
  assert.equal(result.success, false);
  assert.equal(fetches.length, 0);
  assert.deepEqual(await handlers.get("cancel-upload-transcription")({}, "invalid-upload"), {
    success: false,
  });
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
      requestId: "failed-upload",
      remoteTranscriptionUrl: "http://localhost:8000/v1",
      remoteTranscriptionModel: "test-asr",
    });
    assert.equal(result.success, false);
    assert.match(result.error, /ASR unavailable/);
    assert.deepEqual(await handlers.get("cancel-upload-transcription")({}, "failed-upload"), {
      success: false,
    });
  } finally {
    fetchResponse = previous;
  }
});

for (const extension of ["webm", "aiff"]) {
  test(
    `upload cancellation aborts the HTTP request for ${extension}`,
    { timeout: 5000 },
    async (t) => {
      const { getFFmpegPath } = require("../../src/helpers/ffmpegUtils");
      const source = pathNode.join(uploadTempDir, `cancel.${extension}`);
      if (extension === "aiff")
        execFileSync(getFFmpegPath(), [
          "-loglevel",
          "error",
          "-f",
          "lavfi",
          "-i",
          "sine=duration=0.1",
          "-y",
          source,
        ]);
      else fsNode.writeFileSync(source, "test audio");
      const received = Promise.withResolvers();
      const disconnected = Promise.withResolvers();
      const server = http.createServer((request, response) => {
        request.resume();
        request.on("end", () => received.resolve());
        response.on("close", () => disconnected.resolve());
      });
      server.listen(0, "127.0.0.1");
      await once(server, "listening");
      const previous = fetchResponse;
      fetchResponse = (url, init) => fetch(url, init);
      fetches.length = 0;
      let pending;
      let convertedPath;
      t.after(async () => {
        server.closeAllConnections();
        await new Promise((resolve) => server.close(resolve));
        await pending;
        fetchResponse = previous;
        if (convertedPath) fsNode.rmSync(convertedPath, { force: true });
      });
      pending = invokeUpload({
        filePath: source,
        requestId: "cancel-upload",
        remoteTranscriptionUrl: `http://127.0.0.1:${server.address().port}/v1`,
        remoteTranscriptionModel: "test-asr",
      });
      await received.promise;
      const filename = fetches[0].init.body.toString().match(/filename="([^"]+)"/)[1];
      if (extension === "aiff") {
        convertedPath = pathNode.join(osNode.tmpdir(), filename);
        assert.ok(fsNode.existsSync(convertedPath));
      }
      const cancelled = await handlers.get("cancel-upload-transcription")({}, "cancel-upload");
      let timeout;
      const outcome = await Promise.race([
        Promise.all([pending, disconnected.promise]),
        new Promise((resolve) => {
          timeout = setTimeout(() => resolve(null), 1000);
        }),
      ]);
      clearTimeout(timeout);
      assert.ok(
        outcome,
        "cancel must settle the upload and close the HTTP connection without a server response"
      );
      assert.equal(cancelled.success, true);
      assert.deepEqual(await handlers.get("cancel-upload-transcription")({}, "cancel-upload"), {
        success: false,
      });
      assert.equal(outcome[0].success, false);
      assert.equal(outcome[0].code, "UPLOAD_CANCELLED");
      if (convertedPath) {
        assert.equal(
          fsNode.existsSync(convertedPath),
          false,
          "the temporary upload must be removed"
        );
      }
      assert.ok(fsNode.existsSync(source), "the user's source must be preserved");
    }
  );
}

test("upload cancellation during conversion removes the partial file without sending audio", async (t) => {
  const source = pathNode.join(uploadTempDir, "partial.aiff");
  fsNode.writeFileSync(source, "source audio");
  const started = Promise.withResolvers();
  let partialPath;
  let killed = false;
  conversionSpawn = (_command, args) => {
    partialPath = args.at(-1);
    fsNode.writeFileSync(partialPath, "partial MP3");
    const child = new EventEmitter();
    child.stderr = new EventEmitter();
    child.kill = () => {
      killed = true;
      queueMicrotask(() => child.emit("close", null));
    };
    started.resolve();
    return child;
  };
  t.after(() => {
    conversionSpawn = null;
    if (partialPath) fsNode.rmSync(partialPath, { force: true });
  });
  fetches.length = 0;
  const pending = invokeUpload({
    filePath: source,
    requestId: "cancel-conversion",
    remoteTranscriptionUrl: "http://localhost:8000/v1",
  });
  await started.promise;
  await handlers.get("cancel-upload-transcription")({}, "cancel-conversion");
  const result = await pending;
  assert.equal(killed, true);
  assert.equal(result.code, "UPLOAD_CANCELLED");
  assert.equal(fetches.length, 0);
  assert.equal(fsNode.existsSync(partialPath), false, "cancel must remove the partial MP3");
  assert.ok(fsNode.existsSync(source));
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
