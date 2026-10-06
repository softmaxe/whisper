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

test("Automatic paste targets the app that had focus at the Dictation hotkey press", async (t) => {
  const f = createAutomaticPasteFixture(t, { autoLearn: false, focusPid: 42 });
  const dictationId = await f.pressDictationHotkey();
  f.windowManager.setDictationLifecycleState("recording", dictationId);

  // Focus moves to a launcher, and a later capture records it as the
  // monitor's last target.
  await f.advance(1000);
  f.state.focusPid = 99;
  await f.monitor.captureTargetPid();
  assert.equal(f.monitor.lastTargetPid, 99);
  f.windowManager.setDictationLifecycleState("processing", dictationId);
  f.events.length = 0;

  const result = f.invokePaste("dictated text", { dictationId });
  await f.flush();
  await f.advance(25);
  await f.advance(120);
  assert.deepEqual(await result, { success: true, pasted: true });
  assert.deepEqual(
    f.events.filter(({ kind }) => ["activate", "probe"].includes(kind)).map(({ pid }) => pid),
    [42, 42]
  );
  assert.equal(f.nativePastes.length, 1);
});

test("a Dictation back at idle no longer names a Target app for Automatic paste", async (t) => {
  const f = createAutomaticPasteFixture(t, { autoLearn: false, focusPid: 42 });
  const dictationId = await f.pressDictationHotkey();
  f.windowManager.setDictationLifecycleState("processing", dictationId);
  f.windowManager.setDictationLifecycleState("idle", dictationId);
  f.events.length = 0;

  assert.deepEqual(await f.invokePaste("manual transcript", { dictationId }), {
    success: true,
    pasted: false,
  });
  assert.equal(
    f.events.some(({ kind }) => ["activate", "probe"].includes(kind)),
    false
  );
  assert.equal(f.clipboard.readText(), "manual transcript ");
  assert.equal(f.nativePastes.length, 0);
});

test("a lifecycle report with a malformed Dictation id names no Dictation", async (t) => {
  const f = createAutomaticPasteFixture(t, { autoLearn: false, focusPid: 42 });
  const dictationId = await f.pressDictationHotkey();
  f.reportLifecycle("recording", { requestId: dictationId });
  f.reportLifecycle("preparing", 7);
  assert.equal(f.windowManager.isDictating(), false);

  f.reportLifecycle("recording", dictationId);
  assert.equal(f.windowManager.isDictating(), true);
  assert.equal(f.windowManager.getDictationTargetPid(dictationId), 42);

  // An idle naming no Dictation is the renderer's reset, so it fails closed.
  f.reportLifecycle("idle", { requestId: dictationId });
  assert.equal(f.windowManager.isDictationActive(), false);
  assert.equal(f.windowManager.getDictationTargetPid(dictationId), null);
});
