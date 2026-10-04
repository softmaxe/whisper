const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");

let loginItemSettings = {};
const writes = [];
const electronStub = {
  app: {
    getLoginItemSettings: () => loginItemSettings,
    setLoginItemSettings: (settings) => writes.push(settings),
  },
};

const originalLoad = Module._load;
Module._load = function loadWithElectronStub(request, parent, isMain) {
  if (request === "electron") return electronStub;
  return originalLoad.call(this, request, parent, isMain);
};
const autoStart = require("../../src/helpers/autoStart.js");
Module._load = originalLoad;

test("an item awaiting approval reads as off, with the reason surfaced", () => {
  loginItemSettings = { openAtLogin: false, status: "requires-approval" };
  assert.deepEqual(autoStart.getAutoStartState(), { enabled: false, requiresApproval: true });
});

test("macOS reports an approved item without prompting for approval", () => {
  loginItemSettings = { openAtLogin: true, status: "enabled" };
  assert.deepEqual(autoStart.getAutoStartState(), { enabled: true, requiresApproval: false });
});

test("a disabled macOS item is neither enabled nor awaiting approval", () => {
  loginItemSettings = { openAtLogin: false, status: "not-registered" };
  assert.deepEqual(autoStart.getAutoStartState(), { enabled: false, requiresApproval: false });
});

test("macOS detects a login launch from wasOpenedAtLogin", () => {
  loginItemSettings = { wasOpenedAtLogin: true };
  assert.equal(autoStart.wasLaunchedAtLoginHidden(), true);
  loginItemSettings = { wasOpenedAtLogin: false };
  assert.equal(autoStart.wasLaunchedAtLoginHidden(), false);
});

test("enabling launch at login registers the login item", () => {
  writes.length = 0;
  autoStart.setAutoStartEnabled(true);
  autoStart.setAutoStartEnabled(false);
  assert.deepEqual(writes, [{ openAtLogin: true }, { openAtLogin: false }]);
});
