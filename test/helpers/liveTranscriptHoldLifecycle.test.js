const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
} = require("../lib/rendererTestHarness");

const FINAL_HIDE_MS = 4000;
const RECOVERY_HIDE_MS = 5000;

function capturePanelTimers(t) {
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  const timers = [];
  globalThis.setTimeout = (callback, delay) => {
    const timer = { callback, delay, cancelled: false };
    timers.push(timer);
    return timer;
  };
  globalThis.clearTimeout = (timer) => {
    if (timer && typeof timer === "object" && "cancelled" in timer) {
      timer.cancelled = true;
      return;
    }
    originalClearTimeout(timer);
  };
  t.after(() => {
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  });
  return (delay = FINAL_HIDE_MS) =>
    timers.findLast((timer) => timer.delay === delay && !timer.cancelled);
}

async function mountLiveTranscript(t, initialProps = {}, { trackWindowSizes = false } = {}) {
  let root = null;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });
  const previewListeners = new Map();
  const shortcutRegistrations = [];
  const shortcutReleases = [];
  let shownWindows = 0;
  installBrowserGlobals(t, {
    window: {
      electronAPI: {
        ...Object.fromEntries(
          [
            "onPreviewText",
            "onPreviewAppend",
            "onPreviewHold",
            "onPreviewResult",
            "onPreviewHide",
            "onCancelHotkeyPressed",
          ].map((method) => [
            method,
            (listener) => {
              previewListeners.set(method, listener);
              return () => previewListeners.delete(method);
            },
          ])
        ),
        showDictationPanel: () => shownWindows++,
        registerCancelHotkey: async (key, owner) => {
          shortcutRegistrations.push([key, owner]);
          return true;
        },
        unregisterCancelHotkey: async (owner) => {
          shortcutReleases.push(owner);
          return true;
        },
      },
    },
  });
  const container = installHookDom(t);
  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-live-transcript-hold-test-",
    mockModules: {
      "/stores/settingsStore":
        "export const useSettingsStore = { getState: () => ({ floatingIconAutoHide: false }) };",
    },
  });
  const { useLiveTranscriptPanel } = await vite.ssrLoadModule("/hooks/useLiveTranscriptPanel.js");
  const useSizeOwner = trackWindowSizes
    ? (await vite.ssrLoadModule("/hooks/useMainWindowSizeOwner.js")).useMainWindowSizeOwner
    : () => {};
  const windowSizes = [];
  const requestMainWindowSize = async (sizeKey) => {
    windowSizes.push(sizeKey);
  };
  let props = {
    resizeToContent: async () => ({ success: true }),
    assistantOpenRef: { current: false },
    onWillOpen: () => {},
    isRecording: false,
    isProcessing: false,
    isAssistantVoice: false,
    ...initialProps,
  };
  let panel;
  function Harness() {
    panel = useLiveTranscriptPanel(props);
    useSizeOwner({
      requestMainWindowSize,
      dictationErrorActionCount: 0,
      toastCount: 0,
      isCommandMenuOpen: false,
      isCompactPill: false,
      isDictationActive: props.isRecording || props.isProcessing,
      assistantOpen: false,
      assistantMounted: false,
      assistantOpenRef: props.assistantOpenRef,
      liveTranscriptOpen: panel.open,
      liveTranscriptMounted: panel.mounted,
      liveTranscriptOpenRef: panel.openRef,
      liveTranscriptCopyFallback: panel.copyFallback,
    });
    return null;
  }

  root = createRoot(container);
  await React.act(async () => {
    root.render(React.createElement(Harness));
  });
  return {
    getPanel: () => panel,
    windowSizes,
    shortcutRegistrations,
    shortcutReleases,
    hasCancelListener: () => previewListeners.has("onCancelHotkeyPressed"),
    getShownWindows: () => shownWindows,
    emitPreview: async (method, payload) => {
      assert.ok(previewListeners.has(method), `Missing preview listener: ${method}`);
      await React.act(async () => previewListeners.get(method)(payload));
    },
    rerender: async (nextProps) => {
      props = { ...props, ...nextProps };
      await React.act(async () => root.render(React.createElement(Harness)));
    },
    unmount: async () => {
      await React.act(async () => root.unmount());
      root = null;
    },
  };
}

function capturePresentationClock(t) {
  const original = {
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
    requestAnimationFrame: globalThis.requestAnimationFrame,
    cancelAnimationFrame: globalThis.cancelAnimationFrame,
  };
  const timers = new Map();
  let time = 0;
  let nextId = 1;
  globalThis.setTimeout = (callback, delay = 0) => {
    const id = nextId++;
    timers.set(id, { at: time + delay, callback });
    return id;
  };
  globalThis.clearTimeout = (id) => timers.delete(id);
  globalThis.requestAnimationFrame = (callback) => setTimeout(() => callback(time), 16);
  globalThis.cancelAnimationFrame = (id) => clearTimeout(id);
  t.after(() => Object.assign(globalThis, original));

  return async (duration) => {
    const end = time + duration;
    for (let steps = 0; steps < 1000; steps += 1) {
      const next = [...timers]
        .filter(([, timer]) => timer.at <= end)
        .sort((a, b) => a[1].at - b[1].at)[0];
      if (!next) {
        time = end;
        return;
      }
      const [id, timer] = next;
      time = timer.at;
      timers.delete(id);
      await React.act(async () => {
        // Async measurement callbacks can schedule another animation frame;
        // advance that frame below instead of waiting for it inside this step.
        timer.callback();
      });
    }
    assert.fail("Presentation clock did not settle");
  };
}

test("rapid consecutive recordings update an already-visible final transcript panel", async (t) => {
  let entrances = 0;
  const { getPanel, emitPreview, rerender } = await mountLiveTranscript(t, {
    isRecording: true,
    onWillOpen: () => entrances++,
  });
  const advance = capturePresentationClock(t);

  await emitPreview("onPreviewAppend", "first recording");
  await advance(2200);
  assert.equal(getPanel().text, "first recording");
  assert.equal(getPanel().entrancePhase, "content");

  for (const nextText of ["second recording", "third recording"]) {
    await rerender({ isRecording: false });
    await emitPreview("onPreviewResult", { text: "previous final result" });
    await advance(200);
    assert.equal(getPanel().text, "previous final result");

    await rerender({ isRecording: true });
    assert.equal(getPanel().text, "", "a new recording clears the previous transcript");
    await emitPreview("onPreviewText", "");
    await emitPreview("onPreviewAppend", nextText);
    await advance(400);

    assert.equal(getPanel().open, true);
    assert.equal(getPanel().text, nextText);
    assert.equal(entrances, 1, "an open panel does not replay its entrance");
  }
});

test("a new recording during panel entrance keeps text buffered until the surface is ready", async (t) => {
  const { getPanel, emitPreview, rerender } = await mountLiveTranscript(t, { isRecording: true });
  const advance = capturePresentationClock(t);

  await emitPreview("onPreviewAppend", "old recording");
  await advance(100);
  await rerender({ isRecording: false });
  await rerender({ isRecording: true });
  await emitPreview("onPreviewAppend", "fresh recording");
  await advance(400);
  assert.equal(getPanel().text, "", "text must not bypass the panel entrance");

  await advance(1800);
  assert.equal(getPanel().text, "fresh recording");
});

async function showNextFinalAndRunHide(panel, getFinalHideTimer) {
  await React.act(async () => {
    panel.showFinalText("next result");
  });
  const finalHideTimer = getFinalHideTimer();
  assert.ok(finalHideTimer, "fixture setup: the next final result must schedule its hide");
  await React.act(async () => finalHideTimer.callback());
}

test("closing a held final releases the hold before the next final result", async (t) => {
  const { getPanel } = await mountLiveTranscript(t);
  const getFinalHideTimer = capturePanelTimers(t);

  getPanel().holdFinal(true);
  await React.act(async () => getPanel().close({ clear: true }));
  await showNextFinalAndRunHide(getPanel(), getFinalHideTimer);

  assert.equal(getPanel().openRef.current, false);
});

test("replacing a held final with an error releases the hold before the next result", async (t) => {
  const { getPanel } = await mountLiveTranscript(t);
  const getFinalHideTimer = capturePanelTimers(t);

  getPanel().holdFinal(true);
  await React.act(async () => getPanel().dismissForError());
  await showNextFinalAndRunHide(getPanel(), getFinalHideTimer);

  assert.equal(getPanel().openRef.current, false);
});

test("a new recording releases a hold inherited from the prior session", async (t) => {
  const { getPanel, rerender } = await mountLiveTranscript(t);
  const getFinalHideTimer = capturePanelTimers(t);

  getPanel().holdFinal(true);
  await rerender({ isRecording: true });
  await rerender({ isRecording: false });
  await showNextFinalAndRunHide(getPanel(), getFinalHideTimer);

  assert.equal(getPanel().openRef.current, false);
});

test("hovering a final transcript pauses its active hide countdown and leaving restarts it", async (t) => {
  const { getPanel } = await mountLiveTranscript(t);
  const getFinalHideTimer = capturePanelTimers(t);

  await React.act(async () => getPanel().showFinalText("finished transcript"));
  const activeHideTimer = getFinalHideTimer();
  assert.ok(activeHideTimer, "fixture setup: a final result must schedule its hide");

  getPanel().holdFinal(true);

  assert.equal(activeHideTimer.cancelled, true);
  await React.act(async () => activeHideTimer.callback());
  assert.equal(getPanel().openRef.current, true);

  getPanel().holdFinal(false);
  const resumedHideTimer = getFinalHideTimer();
  assert.ok(resumedHideTimer, "leaving the hovered final must schedule a fresh hide");
  assert.notEqual(resumedHideTimer, activeHideTimer);
  assert.equal(resumedHideTimer.delay, FINAL_HIDE_MS);

  await React.act(async () => resumedHideTimer.callback());
  assert.equal(getPanel().openRef.current, false);
});

test("manual-copy recovery ignores late previews and dismisses after five seconds", async (t) => {
  const { getPanel, emitPreview, getShownWindows, shortcutReleases, hasCancelListener } =
    await mountLiveTranscript(t);
  const advance = capturePresentationClock(t);

  await React.act(async () => {
    getPanel().showFinalText("Final expanded text", { copyFallback: "copy" });
  });
  await advance(2200);
  assert.equal(getPanel().text, "Final expanded text");
  assert.equal(getPanel().copyFallback, "copy");
  assert.equal(getShownWindows(), 1, "recovery restores a window hidden during paste");

  await emitPreview("onPreviewText", "late raw text");
  await emitPreview("onPreviewAppend", "late raw chunk");
  await emitPreview("onPreviewHold", { showCleanup: true });
  await emitPreview("onPreviewResult", { text: "late preview result" });
  await emitPreview("onPreviewHide");
  await advance(RECOVERY_HIDE_MS - 2200 - 1);

  assert.equal(getPanel().open, true);
  assert.equal(getPanel().text, "Final expanded text");
  assert.equal(getPanel().phase, "final");
  assert.equal(getPanel().copyFallback, "copy");

  await advance(1);
  assert.equal(getPanel().open, false);
  assert.deepEqual(shortcutReleases, ["copy-recovery"]);
  assert.equal(hasCancelListener(), false);
  assert.equal(getPanel().copyFallback, "copy", "the exit must not flash a streaming panel");
  await advance(400);
  assert.equal(getPanel().mounted, false);
  assert.equal(getPanel().copyFallback, null);
  await emitPreview("onPreviewResult", { text: "late dismissed result" });
  await advance(2200);
  assert.equal(getPanel().open, false);
});

test("copied recovery closes after five seconds and restores the base window", async (t) => {
  const { getPanel, windowSizes } = await mountLiveTranscript(t, {}, { trackWindowSizes: true });
  const advance = capturePresentationClock(t);
  await React.act(async () =>
    getPanel().showFinalText("Copied result", { copyFallback: "copied" })
  );
  await advance(RECOVERY_HIDE_MS - 1);
  assert.equal(getPanel().open, true);
  await advance(1);
  assert.equal(getPanel().open, false);
  await advance(400);
  assert.equal(getPanel().mounted, false);
  assert.equal(getPanel().text, "");
  assert.equal(getPanel().copyFallback, null);
  await advance(200);
  assert.deepEqual(windowSizes, ["BASE", "BASE"]);
});

test("holding recovery pauses dismissal and leaving starts a fresh five seconds", async (t) => {
  const { getPanel } = await mountLiveTranscript(t);
  const advance = capturePresentationClock(t);
  await React.act(async () =>
    getPanel().showFinalText("Copied result", { copyFallback: "copied" })
  );
  await advance(4000);
  getPanel().holdFinal(true);
  await advance(10000);
  assert.equal(getPanel().open, true);
  getPanel().holdFinal(false);
  await advance(RECOVERY_HIDE_MS - 1);
  assert.equal(getPanel().open, true);
  await advance(1);
  assert.equal(getPanel().open, false);
});

test("replacement recovery ignores a stale dismissal and receives a full countdown", async (t) => {
  const { getPanel } = await mountLiveTranscript(t);
  const getHideTimer = capturePanelTimers(t);
  await React.act(async () => getPanel().showFinalText("Same text", { copyFallback: "copied" }));
  const previous = getHideTimer(RECOVERY_HIDE_MS);
  assert.ok(previous);
  await React.act(async () => getPanel().showFinalText("Same text", { copyFallback: "copied" }));
  const replacement = getHideTimer(RECOVERY_HIDE_MS);
  assert.ok(replacement);
  assert.notEqual(previous, replacement);
  assert.equal(previous.cancelled, true);
  await React.act(async () => previous.callback());
  assert.equal(getPanel().open, true);
  await React.act(async () => replacement.callback());
  assert.equal(getPanel().open, false);
});

test("manual dismissal and unmount cancel the recovery countdown", async (t) => {
  const { getPanel, unmount } = await mountLiveTranscript(t);
  const getHideTimer = capturePanelTimers(t);
  await React.act(async () => getPanel().showFinalText("First result", { copyFallback: "copied" }));
  const previous = getHideTimer(RECOVERY_HIDE_MS);
  await React.act(async () => getPanel().close({ suppress: true, clear: true }));
  assert.equal(previous.cancelled, true);
  await React.act(async () => getPanel().showFinalText("Next result", { copyFallback: "copied" }));
  await React.act(async () => previous.callback());
  assert.equal(getPanel().open, true);
  const active = getHideTimer(RECOVERY_HIDE_MS);
  await unmount();
  assert.equal(active.cancelled, true);
});

test("recovery countdown waits for native sizing before starting", async (t) => {
  let finishResize;
  const { getPanel } = await mountLiveTranscript(t, {
    resizeToContent: () => new Promise((resolve) => (finishResize = resolve)),
  });
  const advance = capturePresentationClock(t);
  await React.act(async () =>
    getPanel().showFinalText("Pending result", { copyFallback: "copied" })
  );
  await advance(10000);
  assert.equal(getPanel().mounted, false);
  await React.act(async () => finishResize({ success: true }));
  await advance(RECOVERY_HIDE_MS - 1);
  assert.equal(getPanel().open, true);
  await advance(1);
  assert.equal(getPanel().open, false);
});

test("copy recovery waits for native sizing and shows the complete result without a streaming entrance", async (t) => {
  let finishResize;
  const heights = [];
  const { getPanel } = await mountLiveTranscript(t, {
    resizeToContent: (height) => {
      heights.push(height);
      return new Promise((resolve) => {
        finishResize = resolve;
      });
    },
  });

  await React.act(async () =>
    getPanel().showFinalText("Complete final result", { copyFallback: "copied" })
  );
  assert.deepEqual(heights, [280]);
  assert.equal(getPanel().mounted, false, "do not render inside the old recording pill bounds");
  assert.equal(getPanel().openRef.current, true, "reserve the native size during the resize");
  await React.act(async () => finishResize({ success: true }));
  assert.equal(getPanel().mounted, true);
  assert.equal(getPanel().open, true);
  assert.equal(getPanel().text, "Complete final result");
  assert.equal(getPanel().entrancePhase, "content");
});

test("copy recovery cancels an in-flight streaming entrance", async (t) => {
  const { getPanel, emitPreview } = await mountLiveTranscript(t);
  const advance = capturePresentationClock(t);
  await emitPreview("onPreviewText", "unfinished streaming text");
  await advance(100);
  await React.act(async () =>
    getPanel().showFinalText("Final copyable text", { copyFallback: "copy" })
  );
  await advance(3000);
  assert.equal(getPanel().text, "Final copyable text");
  assert.equal(getPanel().entrancePhase, "content");
  assert.equal(getPanel().copyFallback, "copy");
});

test("copy recovery keeps native ownership when replacing an already open preview", async (t) => {
  let finishResize;
  const { getPanel, emitPreview } = await mountLiveTranscript(t, {
    resizeToContent: (height) =>
      height === 280
        ? new Promise((resolve) => {
            finishResize = resolve;
          })
        : Promise.resolve({ success: true }),
  });
  const advance = capturePresentationClock(t);
  await emitPreview("onPreviewText", "Live preview");
  await advance(2200);
  assert.equal(getPanel().open, true);

  await React.act(async () => getPanel().showFinalText("Final text", { copyFallback: "copy" }));
  assert.equal(getPanel().open, false);
  assert.equal(getPanel().mounted, false);
  assert.equal(getPanel().openRef.current, true, "the size owner must not queue a base resize");
  await React.act(async () => finishResize({ success: true }));
  assert.equal(getPanel().open, true);
  assert.equal(getPanel().text, "Final text");
});

test("closing recovery before its initial resize completes restores the base window", async (t) => {
  let finishResize;
  const { getPanel, windowSizes } = await mountLiveTranscript(
    t,
    {
      resizeToContent: () =>
        new Promise((resolve) => {
          finishResize = resolve;
        }),
    },
    { trackWindowSizes: true }
  );
  const advance = capturePresentationClock(t);
  await React.act(async () => getPanel().showFinalText("Pending result", { copyFallback: "copy" }));
  assert.deepEqual(windowSizes, ["BASE"]);
  await React.act(async () => getPanel().close({ suppress: true, clear: true }));
  await React.act(async () => finishResize({ success: true }));
  await advance(1200);
  assert.equal(getPanel().mounted, false);
  assert.equal(getPanel().open, false);
  assert.equal(getPanel().copyFallback, null);
  assert.deepEqual(
    windowSizes,
    ["BASE", "BASE"],
    "cancelled recovery must restore native pill bounds"
  );
});

test("the next recording clears recovery even when previews stay disabled", async (t) => {
  const { getPanel, rerender, emitPreview, shortcutRegistrations, shortcutReleases } =
    await mountLiveTranscript(t);
  const advance = capturePresentationClock(t);

  await React.act(async () => {
    getPanel().showFinalText("Previous final text", { copyFallback: "copied" });
  });
  await advance(2200);
  assert.equal(getPanel().open, true);
  assert.deepEqual(shortcutRegistrations, [["Escape", "copy-recovery"]]);

  await rerender({ isRecording: true });
  assert.deepEqual(shortcutReleases, ["copy-recovery"]);
  await advance(400);
  assert.equal(getPanel().mounted, false);
  assert.equal(getPanel().text, "");
  assert.equal(getPanel().copyFallback, null);

  await emitPreview("onPreviewText", "Next recording text");
  await advance(2200);
  assert.equal(getPanel().text, "Next recording text");
  await rerender({ isRecording: false });
  await emitPreview("onPreviewResult", { text: "Next final text" });
  await advance(4500);
  assert.equal(getPanel().mounted, false, "ordinary previews retain their automatic close");
});

test("global Escape closes recovery without DOM focus and releases its shortcut lease", async (t) => {
  const { getPanel, emitPreview, shortcutRegistrations, shortcutReleases, hasCancelListener } =
    await mountLiveTranscript(t);
  const advance = capturePresentationClock(t);
  assert.equal(globalThis.document.activeElement, null);

  await React.act(async () =>
    getPanel().showFinalText("Copyable result", { copyFallback: "copied" })
  );
  assert.deepEqual(shortcutRegistrations, [["Escape", "copy-recovery"]]);
  assert.equal(hasCancelListener(), true);
  assert.equal(
    globalThis.document.activeElement,
    null,
    "recovery must not need to focus the floating window"
  );

  await emitPreview("onCancelHotkeyPressed");
  assert.equal(getPanel().open, false);
  assert.deepEqual(shortcutReleases, ["copy-recovery"]);
  assert.equal(hasCancelListener(), false);
  await advance(400);
  assert.equal(getPanel().mounted, false);
  assert.equal(getPanel().text, "");
});

test("unmounting recovery releases its global Escape shortcut and listener", async (t) => {
  const { getPanel, shortcutRegistrations, shortcutReleases, hasCancelListener, unmount } =
    await mountLiveTranscript(t);
  await React.act(async () =>
    getPanel().showFinalText("Copyable result", { copyFallback: "copy" })
  );
  assert.deepEqual(shortcutRegistrations, [["Escape", "copy-recovery"]]);
  await unmount();
  assert.deepEqual(shortcutReleases, ["copy-recovery"]);
  assert.equal(hasCancelListener(), false);
});

test("ordinary live and final previews never claim the recovery Escape shortcut", async (t) => {
  const { getPanel, emitPreview, shortcutRegistrations, shortcutReleases, hasCancelListener } =
    await mountLiveTranscript(t);
  const advance = capturePresentationClock(t);
  await emitPreview("onPreviewText", "Streaming transcript");
  await advance(2200);
  assert.equal(getPanel().open, true);
  await emitPreview("onPreviewResult", { text: "Completed transcript" });
  await advance(4500);
  assert.equal(getPanel().mounted, false);
  assert.deepEqual(shortcutRegistrations, []);
  assert.deepEqual(shortcutReleases, []);
  assert.equal(hasCancelListener(), false);
});
