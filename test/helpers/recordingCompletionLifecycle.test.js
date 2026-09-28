const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
const { renderToStaticMarkup } = require("react-dom/server");
const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
  installMicCaptureGlobals,
} = require("../lib/rendererTestHarness");

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

async function setup(t, { cleanup = false, retain = true, delayedSave = false } = {}) {
  t.mock.timers.enable({ apis: ["setInterval", "setTimeout"] });
  let root;
  let hook;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });
  const requests = [];
  const pastes = [];
  const saves = [];
  const audioSaves = [];
  const writes = [];
  const recoveries = [];
  const errors = [];
  const lifecycle = [];
  let hidden = 0;
  const noopDispose = () => () => {};
  installBrowserGlobals(t, {
    initialStorage: { onboardingCompleted: "true" },
    window: {
      electronAPI: {
        getLogLevel: async () => "error",
        log: async () => {},
        captureDictationTarget: async () => {},
        onToggleDictation: noopDispose,
        onLaptopLidStateChanged: noopDispose,
        dictationLifecycleStateChanged: (state) => lifecycle.push(state),
        completeDictationPreview: () => {},
        hideDictationPreview: () => hidden++,
        recordAnalyticsEvent: async () => {},
        saveTranscription: (...args) => {
          const result = deferred();
          saves.push({ args, ...result });
          if (!delayedSave) result.resolve({ id: saves.length });
          return result.promise;
        },
        saveTranscriptionAudio: async (...args) => audioSaves.push(args),
        writeClipboard: async (text) => {
          writes.push(text);
          return { success: true };
        },
        pasteText: (text) => {
          const result = deferred();
          pastes.push({ text, ...result });
          return result.promise;
        },
      },
    },
  });
  const container = installHookDom(t);
  const media = installMicCaptureGlobals(t);
  media.track.addEventListener = () => {};
  media.track.removeEventListener = () => {};
  let releases = 0;
  media.track.stop = () => releases++;
  const recorders = [];
  const originalRecorder = globalThis.MediaRecorder;
  globalThis.MediaRecorder = class {
    constructor(stream) {
      this.stream = stream;
      this.state = "inactive";
      this.mimeType = "audio/webm";
      recorders.push(this);
    }
    start() {
      this.state = "recording";
    }
    stop() {
      this.state = "inactive";
    }
    finish() {
      return this.onstop?.();
    }
    data(value = 1) {
      this.ondataavailable?.({ data: new Blob([new Uint8Array(4096).fill(value)]) });
    }
  };
  t.after(() => {
    if (originalRecorder === undefined) delete globalThis.MediaRecorder;
    else globalThis.MediaRecorder = originalRecorder;
  });
  t.mock.method(globalThis, "fetch", (url, options) => {
    const result = deferred();
    requests.push({ url, options, ...result });
    return result.promise;
  });
  const vite = await createRendererServer(t);
  const { useSettingsStore } = await vite.ssrLoadModule("/stores/settingsStore.ts");
  useSettingsStore.setState({
    microphoneSelectionMode: "system",
    audioCuesEnabled: false,
    pauseMediaOnDictation: false,
    dataRetentionEnabled: retain,
    audioRetentionDays: 7,
    autoPasteEnabled: true,
    keepTranscriptionInClipboard: false,
    remoteTranscriptionUrl: "http://localhost:8000/v1/audio/transcriptions",
    remoteTranscriptionModel: "test-asr",
    preferredLanguage: "en",
    useCleanupModel: cleanup,
    cleanupModel: "test-cleanup",
    cleanupRemoteUrl: "http://localhost:8001/v1",
    snippets: [],
  });
  const { useAudioRecording } = await vite.ssrLoadModule("/hooks/useAudioRecording.js");
  const { VoicePill } = await vite.ssrLoadModule("/components/dictation/VoicePill.tsx");
  const { DictationErrorCard } = await vite.ssrLoadModule(
    "/components/dictation/DictationErrorCard.tsx"
  );
  const { resolveRecordingPillState } = await vite.ssrLoadModule(
    "/helpers/voicePillPresentation.js"
  );
  let activeError = null;
  const toast = (error) => {
    errors.push(error);
    if (error.presentation === "dictation-error") activeError = error;
  };
  const dismissDictationError = () => {
    activeError = null;
  };
  const showTranscript = (text, options) => recoveries.push({ text, options });
  function Harness() {
    hook = useAudioRecording(toast, { onShowTranscript: showTranscript, dismissDictationError });
    return null;
  }
  root = createRoot(container);
  await React.act(async () => root.render(React.createElement(Harness)));
  const act = (callback) =>
    React.act(async () => {
      await callback();
    });
  return {
    requests,
    pastes,
    saves,
    audioSaves,
    writes,
    recoveries,
    errors,
    recovery: () => activeError,
    recoveryMarkup: () =>
      activeError
        ? renderToStaticMarkup(
            React.createElement(DictationErrorCard, {
              ...activeError,
              onAction: (action) => action.onClick(),
            })
          )
        : "",
    lifecycle,
    hidden: () => hidden,
    releases: () => releases,
    hook: () => hook,
    settings: useSettingsStore,
    act,
    start: async (value = 1) => {
      await act(() => hook.startRecording());
      assert.equal(hook.isRecording, true);
      recorders.at(-1).data(value);
      return recorders.at(-1);
    },
    stop: async () => {
      await act(() => hook.stopRecording());
      const recorder = recorders.at(-1);
      let completion;
      await act(() => {
        completion = recorder.finish();
      });
      return { completion };
    },
    resolveRequest: async (index, payload, status = 200) =>
      act(() => requests[index].resolve(new Response(JSON.stringify(payload), { status }))),
    presentation: () => {
      const state = resolveRecordingPillState(hook);
      const markup = renderToStaticMarkup(
        React.createElement(VoicePill, {
          variant: "floating",
          state,
          getAudioLevel: hook.getAudioLevel,
        })
      );
      return {
        motion: markup.match(/data-motion="([^"]+)"/)?.[1] ?? null,
        width: Number(markup.match(/width:(\d+)px/)?.[1]),
      };
    },
    unmount: async () =>
      act(() => {
        root.unmount();
        root = null;
      }),
  };
}

function assertPending(harness) {
  assert.equal(harness.hook().isProcessing, true);
  assert.equal(harness.lifecycle.at(-1), "processing");
  assert.deepEqual(harness.presentation(), { motion: "wave", width: 84 });
}

test("transcription failure presents an actionable retry that starts a new Dictation", async (t) => {
  const h = await setup(t, { retain: false });
  await h.start();
  await h.stop();
  assertPending(h);
  await h.resolveRequest(0, { error: { message: "Service unavailable" } }, 401);
  assert.equal(h.hook().isProcessing, false);
  assert.equal(h.lifecycle.at(-1), "idle");
  assert.match(h.recoveryMarkup(), /role="alert"/);
  assert.match(h.recoveryMarkup(), /Retry/);
  assert.ok(h.releases() > 0);
  assert.deepEqual(h.recoveries, []);
  assert.equal(h.pastes.length, 0);
  await h.act(() => h.recovery().actions[0].onClick());
  assert.equal(h.recovery(), null);
  assert.equal(h.hook().isRecording, true);
  assert.equal(h.presentation().motion, "live");
  await h.act(() => h.hook().cancelRecording());
  assert.equal(h.recovery(), null);
  assert.equal(h.presentation().motion, null);
});

for (const cleanup of [false, true, "fallback"]) {
  test(`real completion waits through ${cleanup || "disabled"} cleanup and deferred paste`, async (t) => {
    const h = await setup(t, { cleanup: Boolean(cleanup) });
    await h.start();
    await h.stop();
    assertPending(h);
    assert.ok(h.releases() > 0, "capture is released before network work settles");
    await h.resolveRequest(0, { text: "raw transcript" });
    if (cleanup) {
      assertPending(h);
      assert.equal(h.pastes.length, 0);
      assert.equal(h.requests.length, 2);
      await h.resolveRequest(
        1,
        cleanup === "fallback"
          ? { error: { message: "Cleanup rejected" } }
          : { choices: [{ message: { content: "Clean transcript." }, finish_reason: "stop" }] },
        cleanup === "fallback" ? 401 : 200
      );
    }
    const finalText = cleanup === true ? "Clean transcript." : "raw transcript";
    assert.equal(h.pastes[0].text, finalText);
    assertPending(h);
    assert.equal(h.hidden(), 0);
    assert.deepEqual(h.saves[0].args.slice(0, 2), [finalText, "raw transcript"]);
    assert.equal(await h.hook().startRecording(), false, "busy starts cannot invalidate delivery");
    await h.act(() => h.pastes[0].resolve({ pasted: true }));
    assert.equal(h.hook().isProcessing, false);
    assert.equal(h.lifecycle.at(-1), "idle");
    assert.deepEqual(h.presentation(), { motion: null, width: 38 });
    assert.equal(h.hidden(), 1);
    assert.deepEqual(h.recoveries, []);
    assert.deepEqual(h.lifecycle, ["idle", "preparing", "recording", "processing", "idle"]);
  });
}

for (const reject of [false, true]) {
  test(`real completion retains manual-copy recovery after paste ${reject ? "rejection" : "failure"}`, async (t) => {
    const h = await setup(t, { retain: false });
    await h.start();
    await h.stop();
    await h.resolveRequest(0, { text: "recoverable result" });
    assertPending(h);
    await h.act(() =>
      reject
        ? h.pastes[0].reject(new Error("IPC unavailable"))
        : h.pastes[0].resolve({ pasted: false })
    );
    assert.equal(h.hook().isProcessing, false);
    assert.deepEqual(h.writes, ["recoverable result"]);
    assert.deepEqual(h.recoveries, [
      { text: "recoverable result", options: { copyFallback: "copied" } },
    ]);
    assert.equal(h.hidden(), 0);
    assert.equal(h.saves.length, 0);
  });
}

test("cancelling queued recorder finalization prevents an old stop from starting processing", async (t) => {
  const h = await setup(t, { retain: false });
  const oldRecorder = await h.start();
  await h.act(() => h.hook().stopRecording());
  assertPending(h);
  await h.act(() => h.hook().cancelProcessing());
  await h.start();
  await h.act(() => oldRecorder.finish());
  assert.equal(h.hook().isRecording, true);
  assert.equal(h.hook().isProcessing, false);
  assert.equal(h.requests.length, 0);
});

for (const stage of ["transcription", "cleanup", "paste"]) {
  for (const outcome of ["success", "failure"]) {
    test(`late ${stage} ${outcome} cannot change a subsequent recording`, async (t) => {
      const h = await setup(t, { cleanup: stage === "cleanup", retain: false });
      await h.start();
      await h.stop();
      if (stage !== "transcription") await h.resolveRequest(0, { text: "previous result" });
      assertPending(h);
      await h.act(() => h.hook().cancelProcessing());
      assert.deepEqual(h.presentation(), { motion: null, width: 38 });
      await h.start();
      const hidden = h.hidden();
      if (stage === "paste") {
        await h.act(() => h.pastes[0].resolve({ pasted: outcome === "success" }));
      } else if (stage === "cleanup") {
        await h.resolveRequest(
          1,
          outcome === "success"
            ? { choices: [{ message: { content: "late cleanup" }, finish_reason: "stop" }] }
            : { error: { message: "old failure" } },
          outcome === "success" ? 200 : 401
        );
      } else {
        await h.resolveRequest(
          0,
          outcome === "success" ? { text: "late result" } : {},
          outcome === "success" ? 200 : 500
        );
      }
      assert.equal(h.hook().isRecording, true);
      assert.equal(h.hook().isProcessing, false);
      assert.equal(h.lifecycle.at(-1), "recording");
      assert.equal(h.hidden(), hidden);
      assert.deepEqual(h.recoveries, []);
      assert.deepEqual(h.writes, []);
      assert.deepEqual(h.errors, []);
      assert.equal(h.pastes.length, stage === "paste" ? 1 : 0);
    });
  }
}

test("teardown prevents pending delivery from reopening recovery", async (t) => {
  const h = await setup(t, { retain: false });
  await h.start();
  await h.stop();
  await h.resolveRequest(0, { text: "ended result" });
  await h.unmount();
  await h.act(() => h.pastes[0].resolve({ pasted: false }));
  assert.deepEqual(h.recoveries, []);
  assert.deepEqual(h.writes, []);
});

test("cancelled delivery persistence retains its own recording audio after a subsequent result", async (t) => {
  const h = await setup(t, { delayedSave: true });
  await h.start(1);
  await h.stop();
  await h.resolveRequest(0, { text: "first result" });
  await h.act(() => h.hook().cancelProcessing());
  await h.start(2);
  await h.stop();
  await h.resolveRequest(1, { text: "second result" });
  await h.act(() => h.saves[0].resolve({ id: 1 }));
  await h.act(() => h.saves[1].resolve({ id: 2 }));
  assert.deepEqual(
    h.audioSaves.map(([id, buffer]) => [id, new Uint8Array(buffer)[0]]),
    [
      [1, 1],
      [2, 2],
    ]
  );
  assertPending(h);
  await h.act(() => h.pastes[0].resolve({ pasted: true }));
  assertPending(h);
  await h.act(() => h.pastes[1].resolve({ pasted: true }));
  assert.equal(h.hook().isProcessing, false);
});
