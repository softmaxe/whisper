const test = require("node:test");
const assert = require("node:assert/strict");

require.cache[require.resolve("electron")] = {
  exports: {
    globalShortcut: {
      register: () => true,
      unregister: () => undefined,
      isRegistered: () => false,
      unregisterAll: () => undefined,
    },
    BrowserWindow: class {
      static getAllWindows() {
        return [];
      }
    },
  },
};

const HotkeyManager = require("../../src/helpers/hotkeyManager");

// globalShortcut never reports a release and re-fires on autorepeat, so a lone
// regular key held down would toggle dictation on and off. Only keys a native
// listener watches, or chords whose modifier release ends the hold, can be held.
test("only keys whose release is visible can be held", () => {
  const manager = new HotkeyManager();

  assert.equal(manager.supportsPushToTalk("F8"), false);
  assert.equal(manager.supportsPushToTalk("PageDown"), false);
  assert.equal(manager.supportsPushToTalk("GLOBE"), true);
  assert.equal(manager.supportsPushToTalk("RightOption"), true);
  assert.equal(manager.supportsPushToTalk("Control+R"), true);
  assert.equal(manager.supportsPushToTalk("MouseButton4"), true);
});
