const test = require("node:test");
const assert = require("node:assert/strict");
const { createAutomaticPasteFixture } = require("../lib/automaticPasteFixture");
const { automaticPasteScenarios } = require("../lib/automaticPasteScenarios");

automaticPasteScenarios(createAutomaticPasteFixture);

test("AutomaticPaste accepts a legacy successful clipboard result without an outcome object", async (t) => {
  const f = createAutomaticPasteFixture(t, { autoLearn: false });
  // A narrow compatibility test supplements the real ClipboardManager scenarios.
  t.mock.method(f.owner.clipboardManager, "pasteText", async () => undefined);
  assert.deepEqual(await f.paste("legacy result"), { success: true, pasted: true });
});

test("AutomaticPaste uses the current window when requested and retains it across activation", async (t) => {
  const f = createAutomaticPasteFixture(t, { targetPid: null });
  const currentWindow = {
    isDestroyed: () => false,
    isFocused: () => true,
    hide: t.mock.fn(),
    showInactive: t.mock.fn(),
  };
  f.owner.windowManager.mainWindow = currentWindow;
  const result = f.paste("manual transcript");
  f.owner.windowManager.mainWindow = {
    isDestroyed: () => assert.fail("A later window must not replace this request's window"),
  };
  await f.flush();
  assert.equal(currentWindow.hide.mock.callCount(), 1);
  await f.advance(119);
  assert.equal(currentWindow.showInactive.mock.callCount(), 0);
  await f.advance(1);
  assert.deepEqual(await result, { success: true, pasted: false });
  assert.equal(currentWindow.showInactive.mock.callCount(), 1);
});
