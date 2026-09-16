const test = require("node:test");
const assert = require("node:assert/strict");

const { focusWindowsHotkeyCaptureWindow } = require("../../src/helpers/hotkeyCaptureFocus");

function fakeWindow({ destroyed = false, minimized = false, visible = true } = {}) {
  const calls = [];
  return {
    calls,
    isDestroyed: () => destroyed,
    isMinimized: () => minimized,
    isVisible: () => visible,
    restore: () => calls.push("restore"),
    show: () => calls.push("show"),
    focus: () => calls.push("window-focus"),
    webContents: { focus: () => calls.push("renderer-focus") },
  };
}

test("hotkey capture never steals native focus on macOS", () => {
  for (const platform of ["darwin"]) {
    const win = fakeWindow({ minimized: true, visible: false });

    assert.equal(focusWindowsHotkeyCaptureWindow(win, platform), false);
    assert.deepEqual(win.calls, []);
  }
});
