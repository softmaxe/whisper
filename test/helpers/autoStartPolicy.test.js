const test = require("node:test");
const assert = require("node:assert/strict");

const {
  HIDDEN_LAUNCH_FLAG,
  getLoginItemArgs,
  resolveAutoStartState,
  wasLaunchedHidden,
} = require("../../src/helpers/autoStartPolicy.js");

test("macOS login items pass no args", () => {
  assert.deepEqual(getLoginItemArgs("darwin"), []);
});

test("an item awaiting approval reads as off, with the reason surfaced", () => {
  const state = resolveAutoStartState({
    platform: "darwin",
    loginItemSettings: { openAtLogin: false, status: "requires-approval" },
  });
  assert.deepEqual(state, { enabled: false, requiresApproval: true });
});

test("macOS reports an approved item without prompting for approval", () => {
  const state = resolveAutoStartState({
    platform: "darwin",
    loginItemSettings: { openAtLogin: true, status: "enabled" },
  });
  assert.deepEqual(state, { enabled: true, requiresApproval: false });
});

test("a disabled macOS item is neither enabled nor awaiting approval", () => {
  const state = resolveAutoStartState({
    platform: "darwin",
    loginItemSettings: { openAtLogin: false, status: "not-registered" },
  });
  assert.deepEqual(state, { enabled: false, requiresApproval: false });
});

test("macOS detects a login launch from wasOpenedAtLogin, not from argv", () => {
  assert.equal(
    wasLaunchedHidden({
      platform: "darwin",
      argv: ["OpenWhispr"],
      loginItemSettings: { wasOpenedAtLogin: true },
    }),
    true
  );
  assert.equal(
    wasLaunchedHidden({
      platform: "darwin",
      argv: ["OpenWhispr", HIDDEN_LAUNCH_FLAG],
      loginItemSettings: { wasOpenedAtLogin: false },
    }),
    false
  );
});
