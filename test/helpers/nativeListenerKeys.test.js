const test = require("node:test");
const assert = require("node:assert/strict");
const HotkeyManager = require("../../src/helpers/hotkeyManager.js");

function makeManager(hotkeys) {
  const manager = new HotkeyManager();
  manager.slots.clear();
  manager.slots.set("dictation", { hotkeys, callback: null, accelerators: [] });
  return manager;
}

test("Globe and Fn request native suppression when bound to dictation", () => {
  for (const hotkey of ["GLOBE", "Fn"]) {
    const manager = makeManager(["Control+Shift+R", hotkey]);
    assert.deepEqual(manager.getMacNativeListenerConfig(["dictation"]), {
      mouseButtons: [],
      suppressGlobeAction: true,
    });
  }
});

test("dictation mouse hotkeys are collected once for the macOS listener", () => {
  const manager = makeManager(["MouseButton4", "MouseButton5", "MouseButton4"]);
  assert.deepEqual(manager.getMacNativeListenerConfig(["dictation"]), {
    mouseButtons: ["MouseButton4", "MouseButton5"],
    suppressGlobeAction: false,
  });
});

test("ordinary dictation accelerators need no Globe or mouse suppression", () => {
  assert.deepEqual(makeManager(["Control+Shift+R"]).getMacNativeListenerConfig(["dictation"]), {
    mouseButtons: [],
    suppressGlobeAction: false,
  });
});

test("dictation lookup handles primary and secondary shortcuts", () => {
  const manager = makeManager(["GLOBE", "Control+Shift+R"]);
  assert.equal(manager.slotHasHotkey("dictation", "Control+Shift+R"), true);
  assert.equal(manager.slotHasHotkey("dictation", "F12"), false);
  assert.deepEqual(manager.getSlotHotkeys("dictation"), ["GLOBE", "Control+Shift+R"]);
});
