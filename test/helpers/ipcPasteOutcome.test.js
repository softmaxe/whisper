const test = require("node:test");
const assert = require("node:assert/strict");
const { createAutomaticPasteFixture } = require("../lib/automaticPasteFixture");

test("paste-text forwards unchanged text, options, the Dictation's Target app and the actual sender to AutomaticPaste", async (t) => {
  const f = createAutomaticPasteFixture(t);
  let finish;
  const pending = new Promise((resolve) => {
    finish = resolve;
  });
  const paste = t.mock.method(f.owner.automaticPaste, "paste", () => pending);
  const dictationId = await f.pressDictationHotkey();
  const options = {
    dictationId,
    restoreClipboard: false,
    allowClipboardFallback: true,
    webContents: { id: 999 },
  };
  let settled = false;
  const result = f.invokePaste("original text", options).then((value) => {
    settled = true;
    return value;
  });
  await f.flush();
  assert.equal(settled, false);
  assert.deepEqual(paste.mock.calls[0].arguments, [
    "original text",
    {
      restoreClipboard: false,
      allowClipboardFallback: true,
      targetPid: 42,
      webContents: f.sender,
    },
  ]);
  assert.equal(options.webContents.id, 999);
  finish({ success: true, pasted: true });
  assert.deepEqual(structuredClone(await result), { success: true, pasted: true });
});

test("paste-text preserves a serializable clipboard-only outcome and omitted options", async (t) => {
  const f = createAutomaticPasteFixture(t);
  const paste = t.mock.method(f.owner.automaticPaste, "paste", async () => ({
    success: true,
    pasted: false,
  }));
  const result = await f.invokePaste("manual transcript");
  assert.deepEqual(paste.mock.calls[0].arguments, [
    "manual transcript",
    { targetPid: null, webContents: f.sender },
  ]);
  assert.deepEqual(structuredClone(result), { success: true, pasted: false });
});

test("paste-text propagates AutomaticPaste errors", async (t) => {
  const f = createAutomaticPasteFixture(t);
  const failure = new Error("Paste command failed");
  t.mock.method(f.owner.automaticPaste, "paste", async () => {
    throw failure;
  });
  await assert.rejects(f.invokePaste("manual transcript"), (error) => error === failure);
});
