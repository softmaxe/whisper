const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");

const modulePaths = {
  ipc: require.resolve("../../src/helpers/ipcHandlers"),
  automaticPaste: require.resolve("../../src/helpers/automaticPaste"),
  clipboard: require.resolve("../../src/helpers/clipboard"),
  monitor: require.resolve("../../src/helpers/textEditMonitor"),
};

// Keep production AutomaticPaste wiring, the clipboard queue, native adapters
// and monitor real. Control OS access and unrelated IPC startup services.
function createAutomaticPasteFixture(t, options = {}) {
  const state = {
    targetPid: 42,
    focusPid: 42,
    activationAllowed: true,
    targetVerdict: "PASTEABLE",
    accessibility: true,
    fastPasteAvailable: true,
    monitorAvailable: true,
    windowFocused: false,
    windowDestroyed: false,
    pasteResults: [],
    ...options,
  };
  const events = [];
  const record = (kind, data = {}) => {
    const event = { kind, at: Date.now(), ...data };
    events.push(event);
    return event;
  };
  let contents = { ...(options.clipboard ?? { text: "previous clipboard" }) };
  const emptyImage = { isEmpty: () => true };
  const clipboard = {
    availableFormats: () => {
      record("snapshot");
      return Object.keys(contents).map(
        (key) =>
          ({ text: "text/plain", html: "text/html", rtf: "public.rtf", image: "image/png" })[key]
      );
    },
    readText: () => contents.text || "",
    readHTML: () => contents.html || "",
    readRTF: () => contents.rtf || "",
    readImage: () => contents.image || emptyImage,
    writeText: (text) => {
      contents = { text };
      record("clipboard", { text });
    },
    write: (data) => {
      contents = { ...data };
      record("rich-clipboard", { data });
    },
    writeImage: (image) => {
      contents = { image };
      record("image-clipboard", { image });
    },
  };
  const nativePastes = [];
  const observers = [];
  function childProcess() {
    const child = new EventEmitter();
    child.stdin = new EventEmitter();
    child.stdin.write = (text) => {
      child.input = (child.input || "") + text;
    };
    child.stdin.end = () => {};
    child.stdout = new EventEmitter();
    child.stderr = new EventEmitter();
    child.stdout.setEncoding = child.stderr.setEncoding = () => {};
    child.kill = () => {
      child.killedAt = Date.now();
      child.emit("exit", null, "SIGTERM");
      return true;
    };
    return child;
  }
  function spawn(command, args = []) {
    const child = childProcess();
    if (args.includes("--focused-app")) {
      record("focus", { pid: state.focusPid });
      queueMicrotask(() => child.stdout.emit("data", `FOCUSED_PID:${state.focusPid}\n`));
    } else if (args.includes("--paste-target")) {
      record("probe", { pid: Number(args.at(-1)), text: clipboard.readText() });
      if (state.targetVerdict instanceof Error) throw state.targetVerdict;
      queueMicrotask(() => child.stdout.emit("data", `${state.targetVerdict}\n`));
    } else if (path.basename(command) === "macos-text-monitor") {
      observers.push({ child, ...record("observe", { pid: Number(args.at(-1)) }) });
    } else if (command === "osascript" && args[1]?.startsWith("display dialog")) {
      record("permission-dialog");
      queueMicrotask(() => child.emit("close", 1));
    } else {
      assert.ok(
        path.basename(command) === "macos-fast-paste" ||
          (command === "osascript" && args[1]?.includes("key code 9")),
        `Unexpected native command: ${command} ${args.join(" ")}`
      );
      nativePastes.push({ child, ...record("paste", { command, text: clipboard.readText() }) });
      const result = state.pasteResults.shift() ?? 0;
      if (result !== "pending") {
        queueMicrotask(() => {
          if (result instanceof Error) child.emit("error", result);
          else child.emit("close", result);
        });
      }
    }
    return child;
  }
  function execFile(command, args, _options, callback) {
    assert.equal(command, "osascript");
    const script = args.at(-1);
    if (script.includes("runningApplications")) {
      const pid = Number(script.match(/processIdentifier === (\d+)/)[1]);
      record("activate", { pid });
      if (state.activationAllowed) state.focusPid = pid;
      queueMicrotask(() => callback(null, state.activationAllowed ? "ACTIVATED" : "SKIPPED"));
    } else {
      assert.ok(script.includes("frontmostApplication"), "Unexpected AppleScript query");
      queueMicrotask(() => callback(null, String(state.focusPid)));
    }
  }
  const nativeFs = {
    ...fs,
    statSync: (file) => {
      const name = path.basename(file);
      if (name === "macos-fast-paste" || name === "macos-text-monitor") {
        const available =
          name === "macos-fast-paste" ? state.fastPasteAvailable : state.monitorAvailable;
        if (!available)
          throw Object.assign(new Error("Missing test executable"), { code: "ENOENT" });
        return { isFile: () => true };
      }
      return fs.statSync(file);
    },
    accessSync: () => {},
  };
  const handlers = new Map();
  const listeners = new Map();
  const electron = {
    clipboard,
    app: {
      getPath: () => require("node:os").tmpdir(),
      getName: () => "test",
      getVersion: () => "0.0.0",
      on: () => {},
    },
    ipcMain: {
      handle: (name, handler) => handlers.set(name, handler),
      on: (name, listener) => listeners.set(name, listener),
    },
    systemPreferences: {
      isTrustedAccessibilityClient: () => {
        record("permission", { text: clipboard.readText() });
        return state.accessibility;
      },
    },
  };
  const originalLoad = Module._load;
  for (const file of Object.values(modulePaths)) delete require.cache[file];
  Module._load = function loadExternalSeams(request, parent, isMain) {
    if (request === "electron") return electron;
    if (request === "./debugLogger") return { debug: () => {} };
    if (parent?.filename === modulePaths.ipc) {
      // Constructor services outside Automatic paste must not touch disk or
      // query microphone hardware while these scenarios exercise real wiring.
      if (request === "./audioStorage") return class AudioStorageManager {};
      if (request === "./systemDefaultMicrophone") {
        return { resolveSystemDefaultMicrophone: async () => null };
      }
    }
    if ([modulePaths.clipboard, modulePaths.monitor].includes(parent?.filename)) {
      if (request === "child_process") return { spawn, execFile };
      if (request === "fs") return nativeFs;
    }
    return originalLoad.call(this, request, parent, isMain);
  };
  let IPCHandlers;
  let ClipboardManager;
  let TextEditMonitor;
  try {
    IPCHandlers = require(modulePaths.ipc);
    ClipboardManager = require(modulePaths.clipboard);
    TextEditMonitor = require(modulePaths.monitor);
  } finally {
    Module._load = originalLoad;
  }
  t.mock.timers.enable({ apis: ["setTimeout", "setInterval", "Date"], now: 10000 });
  const monitor = new TextEditMonitor();
  monitor.lastTargetPid = state.targetPid;
  // This spy delegates to the real monitor, including its native adapter.
  const monitorStart = t.mock.method(monitor, "startMonitoring");
  const window = {
    isDestroyed: () => state.windowDestroyed,
    isFocused: () => state.windowFocused,
    hide: () => record("hide"),
    showInactive: () => record("show-inactive"),
  };
  const owner = new IPCHandlers({
    clipboardManager: new ClipboardManager(),
    textEditMonitor: state.missingMonitor ? null : monitor,
    windowManager: { mainWindow: window },
  });
  owner._autoLearnEnabled = options.autoLearn !== false;
  const sender = { id: 1 };
  const flush = () => new Promise((resolve) => setImmediate(resolve));
  t.after(() => {
    owner._cleanupTextEditMonitor();
    monitor.stopMonitoring();
    t.mock.timers.reset();
    for (const file of Object.values(modulePaths)) delete require.cache[file];
  });
  return {
    state,
    events,
    clipboard,
    nativePastes,
    observers,
    monitor,
    monitorStart,
    owner,
    sender,
    paste: (text, pasteOptions) =>
      owner.automaticPaste.paste(text, { ...pasteOptions, webContents: sender }),
    invokePaste: (text, pasteOptions) => handlers.get("paste-text")({ sender }, text, pasteOptions),
    writeClipboard: (text) => handlers.get("write-clipboard")({ sender }, text),
    emit: (channel, ...args) => listeners.get(channel)({ sender }, ...args),
    flush,
    advance: async (ms) => {
      t.mock.timers.tick(ms);
      await flush();
    },
  };
}

module.exports = { createAutomaticPasteFixture };
