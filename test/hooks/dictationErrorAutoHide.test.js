const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
} = require("../lib/rendererTestHarness");

// Exercise native visibility through the real size and feedback owner.
async function mountWithErrorCard(t, initialProps = {}) {
  // Registered before the browser globals so React still has `window` when the
  // harness tears the root down.
  let root = null;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });

  const hideCalls = [];
  const showCalls = [];
  installBrowserGlobals(t, {
    window: {
      electronAPI: {
        hideWindow: async () => {
          hideCalls.push("hide");
        },
        showDictationPanel: async (options) => {
          showCalls.push(options);
        },
      },
    },
  });
  const container = installHookDom(t);
  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-dictation-error-auto-hide-",
  });
  const { useMainWindowSizeOwner } = await vite.ssrLoadModule("/hooks/useMainWindowSizeOwner.js");

  const closed = { current: false };
  let props = {
    requestMainWindowSize: async () => ({ success: true }),
    dictationErrorActionCount: 1,
    toastCount: 0,
    isCommandMenuOpen: false,
    isCompactPill: false,
    isDictationActive: false,
    liveTranscriptOpen: false,
    liveTranscriptMounted: false,
    liveTranscriptOpenRef: closed,
    ...initialProps,
  };
  let result;
  function Harness() {
    result = useMainWindowSizeOwner(props);
    return null;
  }

  root = createRoot(container);

  const render = async (next = {}) => {
    props = { ...props, ...next };
    await React.act(async () => {
      root.render(React.createElement(Harness));
      await Promise.resolve();
    });
  };
  // The reveal waits on the native resize and then on compositor frames, both
  // bounded by VISUAL_FRAME_TIMEOUT_MS.
  const settle = async () => {
    await React.act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 900));
    });
  };

  await render();
  return { hideCalls, showCalls, render, settle, state: () => result };
}

test("Retry keeps the pill window on screen while the new dictation runs", async (t) => {
  const { hideCalls, render, settle, state } = await mountWithErrorCard(t);

  // Retry starts recording, and that start dismisses the error card (#2141).
  await render({ dictationErrorActionCount: 0, isDictationActive: true });
  await settle();

  assert.deepEqual(hideCalls, [], "auto-hide must not hide a window that is still recording");
  assert.equal(state().dictationErrorPillHandoffActive, false, "the pill must be revealed again");
});

test("an error card dismissed while idle still auto-hides the window", async (t) => {
  const { hideCalls, render, settle } = await mountWithErrorCard(t);

  await render({ dictationErrorActionCount: 0, isDictationActive: false });
  await settle();

  assert.deepEqual(hideCalls, ["hide"]);
});

test("idle without a pending finish hides immediately without a grace period", async (t) => {
  const h = await mountWithErrorCard(t, { dictationErrorActionCount: 0 });
  assert.deepEqual(h.hideCalls, ["hide"]);
  assert.deepEqual(h.showCalls, []);

  await h.render({ isDictationActive: true });
  assert.equal(h.hideCalls.length, 1, "preparing, recording and processing keep their window");
  await h.render({ isDictationActive: false });
  assert.equal(h.hideCalls.length, 2, "completion hides without waiting for a timer");
});

test("a late error reopens the hidden window without moving the pill", async (t) => {
  const h = await mountWithErrorCard(t, { dictationErrorActionCount: 0 });
  await h.render({ toastCount: 1, dictationErrorActionCount: 1 });
  assert.deepEqual(h.showCalls, [{ reposition: false }]);
  assert.deepEqual(h.hideCalls, ["hide"]);
  await h.render({ toastCount: 0, dictationErrorActionCount: 0 });
  await h.settle();
  assert.deepEqual(h.hideCalls, ["hide", "hide"]);
});

test("ordinary notifications remain visible until dismissed", async (t) => {
  const h = await mountWithErrorCard(t, { dictationErrorActionCount: 0, toastCount: 1 });
  assert.equal(h.showCalls.length, 1);
  assert.deepEqual(h.hideCalls, []);
  await h.render({ toastCount: 0 });
  assert.deepEqual(h.hideCalls, ["hide"]);
});

for (const feedback of [
  { liveTranscriptMounted: true, liveTranscriptOpen: true },
  { liveTranscriptMounted: true, liveTranscriptOpen: false },
  { liveTranscriptCopyFallback: { reason: "paste-failed" } },
  { toastCount: 1 },
]) {
  test(`error dismissal preserves pending feedback ${JSON.stringify(feedback)}`, async (t) => {
    const h = await mountWithErrorCard(t);
    await h.render({ dictationErrorActionCount: 0, ...feedback });
    await h.settle();
    assert.deepEqual(h.hideCalls, [], "the error handoff must not hide its successor");
    await h.render({
      liveTranscriptMounted: false,
      liveTranscriptOpen: false,
      liveTranscriptCopyFallback: null,
      toastCount: 0,
    });
    await h.settle();
    assert.deepEqual(h.hideCalls, ["hide"]);
  });
}
