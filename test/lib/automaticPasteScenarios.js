const test = require("node:test");
const assert = require("node:assert/strict");

// All requests use fixture.paste so the same behavioral baseline can move from
// IPC to AutomaticPaste without changing the retained collaborators or cases.
function automaticPasteScenarios(createFixture) {
  async function completePaste(fixture, text = "dictated text", options) {
    const result = fixture.paste(text, options);
    await fixture.flush();
    await fixture.advance(120);
    return result;
  }

  test("confirmed paste returns a serializable outcome before restoration and starts real monitoring", async (t) => {
    const f = createFixture(t);
    assert.deepEqual(await completePaste(f), { success: true, pasted: true });
    assert.equal(f.clipboard.readText(), "dictated text ");
    assert.deepEqual(
      f.events.map(({ kind }) => kind),
      ["focus", "snapshot", "clipboard", "permission", "probe", "paste"]
    );
    assert.deepEqual(
      f.events.find(({ kind }) => kind === "probe"),
      { kind: "probe", at: 10000, pid: 42, text: "dictated text " }
    );
    assert.equal(f.nativePastes.length, 1);
    // No editor value has been reported. Success means command completion.
    assert.equal(f.observers.length, 0);
    await f.advance(449);
    assert.equal(f.clipboard.readText(), "dictated text ");
    await f.advance(1);
    assert.equal(f.clipboard.readText(), "previous clipboard");
    await f.advance(49);
    assert.equal(f.monitorStart.mock.callCount(), 0);
    f.monitor.lastTargetPid = 99;
    await f.advance(1);
    assert.deepEqual(f.monitorStart.mock.calls[0].arguments, [
      "dictated text",
      30000,
      { targetPid: 42 },
    ]);
    await f.advance(500);
    assert.equal(f.observers.length, 1);
    const observer = f.observers[0];
    assert.equal(observer.pid, 42);
    assert.equal(observer.child.input, "dictated text\n");
    await f.advance(29999);
    assert.equal(observer.child.killedAt, undefined);
    await f.advance(1);
    assert.equal(observer.child.killedAt, observer.at + 30000);
  });

  for (const [text, expected] of [
    ["Latin sentence.", "Latin sentence. "],
    ["한글", "한글 "],
    ["中文", "中文"],
    ["かな", "かな"],
    ["カナ", "カナ"],
    ["句。", "句。"],
    ["句！", "句！"],
    ["句︒", "句︒"],
    ["句︿", "句︿"],
    ["already spaced ", "already spaced "],
    ["line\n", "line\n"],
  ]) {
    test(`paste spacing preserves the original monitoring text for ${JSON.stringify(text)}`, async (t) => {
      const f = createFixture(t);
      await completePaste(f, text, { restoreClipboard: false });
      assert.equal(f.clipboard.readText(), expected);
      assert.equal(f.nativePastes[0].text, expected);
      await f.advance(500);
      assert.deepEqual(f.monitorStart.mock.calls[0].arguments, [text, 30000, { targetPid: 42 }]);
      await f.advance(500);
      assert.equal(f.observers[0].child.input, `${text}\n`);
    });
  }

  for (const [name, options] of [
    ["negative", { targetVerdict: "NOT_PASTEABLE" }],
    ["unknown", { targetVerdict: "UNKNOWN" }],
    ["non-boolean native output", { targetVerdict: "true" }],
    ["missing target", { targetPid: null }],
    ["missing native monitor", { monitorAvailable: false }],
    ["missing monitor", { missingMonitor: true }],
    ["throwing probe", { targetVerdict: new Error("Target process unavailable") }],
  ]) {
    test(`${name} retains a copyable transcript without native paste, restoration or monitoring`, async (t) => {
      const f = createFixture(t, options);
      assert.deepEqual(await f.paste("manual transcript"), { success: true, pasted: false });
      await f.advance(2000);
      assert.equal(f.clipboard.readText(), "manual transcript ");
      assert.deepEqual(
        f.events.filter(({ kind }) => kind.includes("clipboard")).map(({ text }) => text),
        ["manual transcript "]
      );
      assert.equal(f.nativePastes.length, 0);
      assert.equal(f.monitorStart.mock.callCount(), 0);
    });
  }

  for (const allowClipboardFallback of [true, false]) {
    test(`Accessibility denial preserves clipboard fallback ${allowClipboardFallback ? "when allowed" : "as an error when disallowed"}`, async (t) => {
      const f = createFixture(t, { accessibility: false });
      const paste = f.paste("manual transcript", { allowClipboardFallback });
      if (allowClipboardFallback) {
        assert.deepEqual(await paste, { success: true, pasted: false });
      } else {
        await assert.rejects(paste, {
          message:
            "Accessibility permissions required for automatic pasting. Text has been copied to clipboard - please paste manually with Cmd+V.",
        });
      }
      await f.advance(2000);
      assert.equal(f.clipboard.readText(), "manual transcript ");
      assert.equal(
        f.events.filter(({ kind }) => kind === "permission-dialog").length,
        allowClipboardFallback ? 0 : 1
      );
      assert.equal(
        f.events.some(({ kind }) => kind === "probe"),
        false
      );
      assert.equal(f.nativePastes.length, 0);
      assert.equal(f.monitorStart.mock.callCount(), 0);
    });
  }

  test("successful paste restores supported rich clipboard data together", async (t) => {
    const image = { isEmpty: () => false };
    const original = { text: "plain", html: "<b>rich</b>", rtf: "{\\rtf1 rich}", image };
    const f = createFixture(t, { clipboard: original, autoLearn: false });
    await completePaste(f);
    await f.advance(450);
    assert.equal(f.clipboard.readText(), original.text);
    assert.equal(f.clipboard.readHTML(), original.html);
    assert.equal(f.clipboard.readRTF(), original.rtf);
    assert.equal(f.clipboard.readImage(), image);
    assert.deepEqual(f.events.at(-1).data, original);
  });

  test("disabled restoration leaves the transcript and takes no clipboard snapshot", async (t) => {
    const f = createFixture(t, { autoLearn: false });
    await completePaste(f, "keep this", { restoreClipboard: false });
    await f.advance(2000);
    assert.equal(f.clipboard.readText(), "keep this ");
    assert.equal(
      f.events.some(({ kind }) => kind === "snapshot"),
      false
    );
  });

  for (const [name, original] of [
    ["empty", { text: "" }],
    ["image-only", { image: { isEmpty: () => false } }],
  ]) {
    test(`successful paste restores an ${name} clipboard`, async (t) => {
      const f = createFixture(t, { clipboard: original, autoLearn: false });
      await completePaste(f);
      await f.advance(450);
      assert.equal(f.clipboard.readText(), "");
      if (original.image) assert.equal(f.clipboard.readImage(), original.image);
      else assert.equal(f.clipboard.readImage().isEmpty(), true);
    });
  }

  for (const ordinaryWrite of [false, true]) {
    test(`${ordinaryWrite ? "an ordinary clipboard write completes immediately and" : "a newer user copy"} survives the pending restoration`, async (t) => {
      const f = createFixture(t, { autoLearn: false });
      await completePaste(f);
      if (ordinaryWrite) assert.deepEqual(await f.writeClipboard("new copy"), { success: true });
      else f.clipboard.writeText("new copy");
      assert.equal(f.clipboard.readText(), "new copy");
      await f.advance(450);
      assert.equal(f.clipboard.readText(), "new copy");
      assert.equal(f.events.filter(({ kind }) => kind === "clipboard").length, 2);
    });
  }

  test("successive requests activate before the queue but wait for the prior restoration before writing", async (t) => {
    const f = createFixture(t, { autoLearn: false });
    await completePaste(f, "first");
    f.monitor.lastTargetPid = 84;
    const second = f.paste("second");
    await f.flush();
    assert.equal(f.events.find(({ kind }) => kind === "activate").pid, 84);
    await f.advance(25);
    assert.equal(f.clipboard.readText(), "first ");
    assert.equal(f.nativePastes.length, 1);
    f.monitor.lastTargetPid = 126;
    await f.advance(424);
    assert.equal(f.clipboard.readText(), "first ");
    await f.advance(1);
    assert.equal(f.clipboard.readText(), "second ");
    assert.deepEqual(
      f.events.filter(({ kind }) => kind === "clipboard").map(({ text }) => text),
      ["first ", "previous clipboard", "second "]
    );
    assert.deepEqual(
      f.events.filter(({ kind }) => kind === "probe").map(({ pid }) => pid),
      [42, 84]
    );
    await f.advance(120);
    assert.deepEqual(await second, { success: true, pasted: true });
    await f.advance(450);
    assert.equal(f.clipboard.readText(), "previous clipboard");
  });

  for (const retrySucceeds of [true, false]) {
    test(`native rejection retries after 200 ms and ${retrySucceeds ? "succeeds" : "releases the queue after failure"}`, async (t) => {
      const f = createFixture(t, {
        fastPasteAvailable: false,
        pasteResults: retrySucceeds ? [1, 0] : [1, 1, 0],
        autoLearn: false,
      });
      const first = f.paste("first");
      const firstOutcome = retrySucceeds ? first : assert.rejects(first, /Paste failed \(code 1\)/);
      await f.flush();
      await f.advance(120);
      assert.equal(f.nativePastes.length, 1);
      const queued = retrySucceeds ? null : f.paste("recovery");
      await f.flush();
      assert.equal(f.clipboard.readText(), "first ");
      await f.advance(199);
      assert.equal(f.nativePastes.length, 1);
      await f.advance(1);
      await f.advance(119);
      assert.equal(f.nativePastes.length, 1);
      await f.advance(1);
      await firstOutcome;
      assert.equal(f.nativePastes.length, 2);
      assert.equal(f.nativePastes[1].at - f.nativePastes[0].at, 320);
      if (retrySucceeds) {
        assert.deepEqual(await first, { success: true, pasted: true });
        await f.advance(450);
        assert.equal(f.clipboard.readText(), "previous clipboard");
      } else {
        assert.equal(f.clipboard.readText(), "recovery ");
        await f.advance(120);
        assert.deepEqual(await queued, { success: true, pasted: true });
        await f.advance(450);
        assert.equal(f.clipboard.readText(), "first ");
      }
    });
  }

  test("an unsuccessful fast-paste command uses the retained AppleScript fallback", async (t) => {
    const f = createFixture(t, { pasteResults: [2, 0], autoLearn: false });
    assert.deepEqual(await completePaste(f), { success: true, pasted: true });
    assert.equal(f.nativePastes.length, 2);
    assert.ok(f.nativePastes[0].command.endsWith("macos-fast-paste"));
    assert.equal(f.nativePastes[1].command, "osascript");
    await f.advance(450);
    assert.equal(f.clipboard.readText(), "previous clipboard");
  });

  test("AutoLearn is read when native paste completes and existing scheduled monitoring is retained", async (t) => {
    const f = createFixture(t, { pasteResults: ["pending"], autoLearn: false });
    const paste = f.paste("original");
    await f.flush();
    await f.advance(120);
    f.owner._autoLearnEnabled = true;
    f.nativePastes[0].child.emit("close", 0);
    await paste;
    f.owner._autoLearnEnabled = false;
    await f.advance(500);
    assert.equal(f.monitorStart.mock.callCount(), 1);
  });

  test("AutoLearn disabled at native completion does not schedule monitoring", async (t) => {
    const f = createFixture(t, { pasteResults: ["pending"] });
    const paste = f.paste("original");
    await f.flush();
    await f.advance(120);
    f.owner._autoLearnEnabled = false;
    f.nativePastes[0].child.emit("close", 0);
    await paste;
    f.owner._autoLearnEnabled = true;
    await f.advance(1000);
    assert.equal(f.monitorStart.mock.callCount(), 0);
  });

  test("a completed native paste without monitoring support does not schedule AutoLearn", async (t) => {
    const f = createFixture(t, { pasteResults: ["pending"] });
    const paste = f.paste("original");
    await f.flush();
    await f.advance(120);
    f.owner.textEditMonitor = null;
    f.nativePastes[0].child.emit("close", 0);
    assert.deepEqual(await paste, { success: true, pasted: true });
    await f.advance(1000);
    assert.equal(f.monitorStart.mock.callCount(), 0);
  });

  test("a monitoring-start failure cannot change the completed paste or block later requests", async (t) => {
    const f = createFixture(t);
    // Only this failure-isolation case injects a synchronous monitor failure.
    // Clipboard execution, target confirmation and restoration stay real.
    t.mock.method(f.monitor, "startMonitoring", () => {
      throw new Error("Monitor unavailable");
    });
    assert.deepEqual(await completePaste(f), { success: true, pasted: true });
    await f.advance(500);
    assert.equal(f.clipboard.readText(), "previous clipboard");
    assert.deepEqual(await completePaste(f, "next"), { success: true, pasted: true });
  });

  test("an already focused Target app is not activated again", async (t) => {
    const f = createFixture(t);
    await completePaste(f);
    assert.equal(
      f.events.some(({ kind }) => kind === "activate"),
      false
    );
    assert.equal(
      f.events.some(({ kind }) => kind === "hide"),
      false
    );
  });

  test("a closed launcher is not reactivated and the focused pill yields for 120 ms", async (t) => {
    const f = createFixture(t, {
      focusPid: 7,
      activationAllowed: false,
      windowFocused: true,
      targetVerdict: "NOT_PASTEABLE",
    });
    const paste = f.paste("manual");
    await f.flush();
    assert.deepEqual(
      f.events.map(({ kind }) => kind),
      ["focus", "activate", "hide"]
    );
    await f.advance(119);
    assert.equal(f.clipboard.readText(), "previous clipboard");
    await f.advance(1);
    assert.deepEqual(await paste, { success: true, pasted: false });
    assert.equal(f.state.focusPid, 7);
    assert.deepEqual(
      f.events.filter(({ kind }) => ["hide", "show-inactive"].includes(kind)),
      [
        { kind: "hide", at: 10000 },
        { kind: "show-inactive", at: 10120 },
      ]
    );
    assert.equal(f.clipboard.readText(), "manual ");
    assert.equal(f.nativePastes.length, 0);
  });
}

module.exports = { automaticPasteScenarios };
