const test = require("node:test");
const assert = require("node:assert/strict");

const {
  resolveAutoStartState,
  wasLaunchedHidden,
  getRelaunchArgs,
} = require("../../src/helpers/autoStartPolicy.js");

test("an item awaiting approval reads as off, with the reason surfaced", () => {
  const state = resolveAutoStartState({
    loginItemSettings: { openAtLogin: false, status: "requires-approval" },
  });
  assert.deepEqual(state, { enabled: false, requiresApproval: true });
});

test("macOS reports an approved item without prompting for approval", () => {
  const state = resolveAutoStartState({
    loginItemSettings: { openAtLogin: true, status: "enabled" },
  });
  assert.deepEqual(state, { enabled: true, requiresApproval: false });
});

test("a disabled macOS item is neither enabled nor awaiting approval", () => {
  const state = resolveAutoStartState({
    loginItemSettings: { openAtLogin: false, status: "not-registered" },
  });
  assert.deepEqual(state, { enabled: false, requiresApproval: false });
});

test("macOS detects a login launch from wasOpenedAtLogin", () => {
  assert.equal(wasLaunchedHidden({ loginItemSettings: { wasOpenedAtLogin: true } }), true);
  assert.equal(wasLaunchedHidden({ loginItemSettings: { wasOpenedAtLogin: false } }), false);
});

test("a relaunch drops cold-start deep links", () => {
  assert.deepEqual(
    getRelaunchArgs({
      argv: ["Whisper", "--flag", "openwhispr-selfhosted://auth"],
      protocol: "openwhispr-selfhosted",
    }),
    ["--flag"]
  );
});
