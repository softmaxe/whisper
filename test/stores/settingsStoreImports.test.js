const test = require("node:test");
const assert = require("node:assert/strict");

// Deliberately partial: no setInterval, no dispatchEvent, no electronAPI, and
// localStorage exists only as a window property, never as a bare global.
const windowStub = {
  addEventListener() {},
  removeEventListener() {},
  localStorage: {
    getItem: () => null,
    setItem() {},
    removeItem() {},
  },
  location: { origin: "https://harness.whisper.test" },
};

Object.defineProperty(globalThis, "window", {
  value: windowStub,
  configurable: true,
  writable: true,
});

test("settingsStore imports without a bare localStorage global", async () => {
  assert.equal(typeof localStorage, "undefined");
  const store = await import("../../src/stores/settingsStore.ts");
  assert.equal(typeof store.useSettingsStore.getState, "function");
});
