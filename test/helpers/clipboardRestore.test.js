const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");
const { EventEmitter } = require("node:events");
const childProcess = require("node:child_process");

const fakeClipboard = {
  text: "",
  html: "",
  rtf: "",
  image: null,
  formats: ["text/plain"],
  writes: [],
  availableFormats() {
    return this.formats;
  },
  readText() {
    return this.text;
  },
  writeText(text) {
    this.text = text;
    this.html = "";
    this.rtf = "";
    this.image = null;
    this.formats = ["text/plain"];
    this.writes.push(["writeText", text]);
  },
  readHTML() {
    return this.html;
  },
  readRTF() {
    return this.rtf;
  },
  write(payload) {
    this.text = payload.text || "";
    this.html = payload.html || "";
    this.rtf = payload.rtf || "";
    this.image = payload.image || null;
    this.formats = [];
    if (Object.hasOwn(payload, "text")) this.formats.push("text/plain");
    if (Object.hasOwn(payload, "html")) this.formats.push("text/html");
    if (Object.hasOwn(payload, "rtf")) this.formats.push("text/rtf");
    if (Object.hasOwn(payload, "image")) this.formats.push("image/png");
    this.writes.push(["write", payload]);
  },
  readImage() {
    return this.image || emptyImage;
  },
  writeImage(image) {
    this.text = "";
    this.html = "";
    this.rtf = "";
    this.image = image;
    this.formats = image && !image.isEmpty() ? ["image/png"] : [];
    this.writes.push(["writeImage", image]);
  },
};

const emptyImage = { isEmpty: () => true };
const nonEmptyImage = { isEmpty: () => false };

const clipboardModulePath = require.resolve("../../src/helpers/clipboard");

const originalLoad = Module._load;

function loadClipboardManager({ spawn } = {}) {
  delete require.cache[clipboardModulePath];

  Module._load = function loadWithMocks(request, parent, isMain) {
    if (request === "electron") {
      return {
        clipboard: fakeClipboard,
        systemPreferences: {
          isTrustedAccessibilityClient: () => true,
        },
      };
    }
    if (request === "child_process" && spawn) {
      return { ...childProcess, spawn };
    }
    return originalLoad.call(this, request, parent, isMain);
  };

  try {
    return require("../../src/helpers/clipboard");
  } finally {
    Module._load = originalLoad;
  }
}

const ClipboardManager = loadClipboardManager();
const initialExitListeners = new Set(process.listeners("exit"));

test.afterEach(() => {
  for (const listener of process.listeners("exit")) {
    if (!initialExitListeners.has(listener)) {
      process.removeListener("exit", listener);
    }
  }
});

function createSuccessfulSpawn(calls) {
  return function successfulSpawn(command, args = []) {
    calls.push({ command, args });
    const pasteProcess = new EventEmitter();
    pasteProcess.stderr = new EventEmitter();
    pasteProcess.stdout = new EventEmitter();
    process.nextTick(() => pasteProcess.emit("close", 0));
    return pasteProcess;
  };
}

function createSpawn(calls, exitCodes, { stdout = [] } = {}) {
  return function mockedSpawn(command, args = []) {
    calls.push({ command, args });
    const pasteProcess = new EventEmitter();
    pasteProcess.stderr = new EventEmitter();
    pasteProcess.stdout = new EventEmitter();
    const code = exitCodes.shift() ?? 0;
    const output = stdout.shift();
    process.nextTick(() => {
      if (output) pasteProcess.stdout.emit("data", output);
      pasteProcess.emit("close", code);
    });
    return pasteProcess;
  };
}

function resetClipboard({
  text = "",
  html = "",
  rtf = "",
  image = null,
  formats = ["text/plain"],
} = {}) {
  fakeClipboard.text = text;
  fakeClipboard.html = html;
  fakeClipboard.rtf = rtf;
  fakeClipboard.image = image;
  fakeClipboard.formats = formats;
  fakeClipboard.writes = [];
}

test("restore preserves rich clipboard formats atomically", () => {
  resetClipboard({
    formats: ["text/html", "text/rtf", "text/plain", "image/png"],
    text: "plain before",
    html: "<b>html before</b>",
    rtf: "{\\rtf1 before}",
    image: nonEmptyImage,
  });
  const manager = new ClipboardManager();

  const snapshot = manager._saveClipboard();
  fakeClipboard.writeText("dictated text");
  manager._restoreClipboard(snapshot);

  assert.deepEqual([...fakeClipboard.availableFormats()].sort(), [
    "image/png",
    "text/html",
    "text/plain",
    "text/rtf",
  ]);
  assert.equal(fakeClipboard.text, "plain before");
  assert.equal(fakeClipboard.html, "<b>html before</b>");
  assert.equal(fakeClipboard.rtf, "{\\rtf1 before}");
  assert.equal(fakeClipboard.image, nonEmptyImage);
  assert.equal(fakeClipboard.writes.at(-1)[0], "write");
});

test("restore runs when clipboard still contains the pasted text", async () => {
  resetClipboard();
  fakeClipboard.text = "dictated text";
  const manager = new ClipboardManager();

  await manager._restoreClipboardAfterDelay(
    { type: "text", data: "previous clipboard" },
    { delayMs: 0, expectedText: "dictated text" }
  );

  assert.equal(fakeClipboard.text, "previous clipboard");
});

test("restore is skipped when another clipboard write wins the race", async () => {
  resetClipboard();
  fakeClipboard.text = "user copied something else";
  const manager = new ClipboardManager();

  await manager._restoreClipboardAfterDelay(
    { type: "text", data: "previous clipboard" },
    { delayMs: 0, expectedText: "dictated text" }
  );

  assert.equal(fakeClipboard.text, "user copied something else");
});

test("pasteText waits for prior clipboard restoration before starting the next paste", async () => {
  const manager = new ClipboardManager();
  const events = [];
  let releaseFirstRestore;

  manager._pasteText = async (text) => {
    events.push(`start:${text}`);
    events.push(`end:${text}`);
    if (text === "first") {
      return {
        restoreComplete: new Promise((resolve) => {
          releaseFirstRestore = resolve;
        }),
      };
    }
    return { restoreComplete: Promise.resolve() };
  };

  await manager.pasteText("first");
  const secondPaste = manager.pasteText("second");
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(events, ["start:first", "end:first"]);

  releaseFirstRestore();
  await secondPaste;
  assert.deepEqual(events, ["start:first", "end:first", "start:second", "end:second"]);
});

for (const fastPaste of [true, false]) {
  for (const verdict of [false, null, undefined, "throws"]) {
    test(
      `unconfirmed paste target (${verdict}) keeps the transcript without invoking ${fastPaste ? "CGEvent" : "osascript"}`,
      { skip: process.platform !== "darwin" },
      async () => {
        resetClipboard({ text: "previous clipboard" });
        const calls = [];
        const TestClipboardManager = loadClipboardManager({ spawn: createSuccessfulSpawn(calls) });
        const manager = new TestClipboardManager();
        manager.resolveFastPasteBinary = () => (fastPaste ? "/tmp/fast-paste" : null);
        manager.checkAccessibilityPermissions = async () => true;
        let restores = 0;
        manager._restoreClipboardAfterDelay = async () => restores++;

        const result = await manager.pasteText("final transcript", {
          restoreClipboard: true,
          checkPasteTarget: async () => {
            assert.equal(fakeClipboard.text, "final transcript", "probe sees the new clipboard");
            if (verdict === "throws") throw new Error("AX unavailable");
            return verdict;
          },
        });
        await result.restoreComplete;

        assert.equal(result.pasted, false);
        assert.equal(fakeClipboard.text, "final transcript");
        assert.equal(restores, 0);
        assert.deepEqual(calls, []);
        // A skipped paste must release the queue for later clipboard work.
        await manager.runClipboardOperation(() => fakeClipboard.writeText("manual copy"));
        assert.equal(fakeClipboard.text, "manual copy");
      }
    );
  }
}

test(
  "confirmed targets and callers without a probe keep the normal paste and restore flow",
  { skip: process.platform !== "darwin" },
  async () => {
    for (const checkPasteTarget of [async () => true, undefined]) {
      resetClipboard({ text: "previous clipboard" });
      const manager = new ClipboardManager();
      manager.resolveFastPasteBinary = () => null;
      manager.checkAccessibilityPermissions = async () => true;
      let pasted = false;
      manager.pasteMacOS = async (originalClipboard, options) => {
        pasted = true;
        assert.equal(options.expectedClipboardText, "final transcript");
        return {
          restoreComplete: manager._restoreClipboardAfterDelay(originalClipboard, {
            delayMs: 0,
            expectedText: options.expectedClipboardText,
          }),
        };
      };

      const result = await manager.pasteText("final transcript", {
        checkPasteTarget,
      });
      await result.restoreComplete;

      assert.equal(pasted, true);
      assert.notEqual(result.pasted, false);
      assert.equal(fakeClipboard.text, "previous clipboard");
    }
  }
);

test("pasteMacOS restores clipboard after the short macOS delay on successful fast paste", async () => {
  const spawnCalls = [];
  const TestClipboardManager = loadClipboardManager({
    spawn: createSuccessfulSpawn(spawnCalls),
  });
  const manager = new TestClipboardManager();
  const originalClipboard = { type: "text", data: "previous clipboard" };
  let restoreCall;

  manager.resolveFastPasteBinary = () => "/tmp/openwhispr-fast-paste";
  manager._restoreClipboardAfterDelay = (original, options) => {
    restoreCall = { original, options };
    return Promise.resolve();
  };

  const result = await manager.pasteMacOS(originalClipboard, {
    expectedClipboardText: "dictated text",
  });
  await result.restoreComplete;

  assert.equal(spawnCalls.length, 1);
  assert.equal(spawnCalls[0].command, "/tmp/openwhispr-fast-paste");
  assert.equal(restoreCall.original, originalClipboard);
  assert.deepEqual(restoreCall.options, {
    delayMs: 450,
    expectedText: "dictated text",
  });
});

test("pasteMacOS leaves text on the clipboard when the keyboard layout cannot be resolved", async () => {
  resetClipboard({ text: "dictated text" });
  const spawnCalls = [];
  const TestClipboardManager = loadClipboardManager({
    spawn: createSpawn(spawnCalls, [3]),
  });
  const manager = new TestClipboardManager();
  manager.fastPastePath = "/tmp/openwhispr-fast-paste";
  manager.fastPasteChecked = true;
  let restoreCalls = 0;
  manager._restoreClipboardAfterDelay = () => {
    restoreCalls++;
    return Promise.resolve();
  };

  await assert.rejects(
    manager.pasteMacOS({ type: "text", data: "previous clipboard" }),
    /could not resolve the active keyboard layout/
  );

  assert.deepEqual(spawnCalls, [{ command: "/tmp/openwhispr-fast-paste", args: [] }]);
  assert.equal(fakeClipboard.text, "dictated text");
  assert.deepEqual(fakeClipboard.writes, []);
  assert.equal(restoreCalls, 0);
  assert.equal(manager.fastPastePath, "/tmp/openwhispr-fast-paste");
  assert.equal(manager.fastPasteChecked, true);
});

test("pasteMacOSWithOsascript fallback uses the short macOS restore delay", async () => {
  const spawnCalls = [];
  const TestClipboardManager = loadClipboardManager({
    spawn: createSuccessfulSpawn(spawnCalls),
  });
  const manager = new TestClipboardManager();
  const originalClipboard = { type: "text", data: "previous clipboard" };
  let restoreCall;

  manager._restoreClipboardAfterDelay = (original, options) => {
    restoreCall = { original, options };
    return Promise.resolve();
  };

  const result = await manager.pasteMacOSWithOsascript(originalClipboard, {
    expectedClipboardText: "dictated text",
  });
  await result.restoreComplete;

  assert.equal(spawnCalls.length, 1);
  assert.equal(spawnCalls[0].command, "osascript");
  assert.deepEqual(spawnCalls[0].args, [
    "-e",
    'tell application "System Events" to key code 9 using command down',
  ]);
  assert.equal(restoreCall.original, originalClipboard);
  assert.deepEqual(restoreCall.options, {
    delayMs: 450,
    expectedText: "dictated text",
  });
});

// Terminal detection now serves two callers: the Linux paste path, which matches
// window classes, and macOS selection capture, which matches localized app names.
