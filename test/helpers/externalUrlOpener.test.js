const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");

const openerModulePath = require.resolve("../../src/helpers/externalUrlOpener");

function loadOpener(t) {
  const originalLoad = Module._load;
  const originalPlatform = Object.getOwnPropertyDescriptor(process, "platform");
  const originalCache = require.cache[openerModulePath];
  const openExternalCalls = [];
  Object.defineProperty(process, "platform", { value: "darwin", configurable: true });
  delete require.cache[openerModulePath];
  t.after(() => {
    Object.defineProperty(process, "platform", originalPlatform);
    delete require.cache[openerModulePath];
    if (originalCache) require.cache[openerModulePath] = originalCache;
  });

  Module._load = function loadWithMocks(request, parent, isMain) {
    if (request === "electron") {
      return {
        shell: {
          openExternal: async (url) => openExternalCalls.push(url),
        },
      };
    }
    if (request === "child_process") {
      return { spawn: () => assert.fail("macOS must open URLs through Electron") };
    }
    return originalLoad.call(this, request, parent, isMain);
  };

  try {
    return { ...require(openerModulePath), openExternalCalls };
  } finally {
    Module._load = originalLoad;
  }
}

test("macOS opens external URLs through Electron", async (t) => {
  const { openExternalUrl, openExternalCalls } = loadOpener(t);
  await openExternalUrl("https://github.com/softmaxe/whisper");
  assert.deepEqual(openExternalCalls, ["https://github.com/softmaxe/whisper"]);
});

test("invalid URLs reject without opening anything", async (t) => {
  const { openExternalUrl, openExternalCalls } = loadOpener(t);
  await assert.rejects(openExternalUrl("not a url"), TypeError);
  assert.deepEqual(openExternalCalls, []);
});
