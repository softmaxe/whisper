const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
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

async function mountCapture(
  t,
  { warmHold = "900", captureTarget, firstFrame, failRecorder = false, initialStorage = {} } = {}
) {
  t.mock.timers.enable({ apis: ["setInterval"] });
  let root;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });
  const events = {};
  const lifecycle = [];
  const toasts = [];
  const pastes = [];
  const saved = [];
  const savedAudio = [];
  const targetCaptures = [];
  const requests = [];
  const recorders = [];
  const streams = [];
  const contexts = [];
  const transcription = deferred();
  const audioPayloads = [];
  const { window, storage } = installBrowserGlobals(t, {
    initialStorage: {
      onboardingCompleted: "true",
      transcriptionMode: "self-hosted",
      remoteTranscriptionUrl: "http://localhost:8178/v1",
      remoteTranscriptionModel: "test-model",
      microphoneSelectionMode: "specific",
      selectedMicDeviceId: "external",
      selectedMicDeviceLabel: "External microphone",
      micWarmHoldSeconds: warmHold,
      autoPasteEnabled: "true",
      audioCuesEnabled: "false",
      pauseMediaOnDictation: "false",
      showTranscriptionPreview: "false",
      useReasoningModel: "false",
      keepTranscriptionInClipboard: "false",
      ...initialStorage,
    },
  });
  const container = installHookDom(t);
  globalThis.requestAnimationFrame = (callback) => {
    callback();
    return 1;
  };
  installMicCaptureGlobals(t);
  if (firstFrame) {
    globalThis.MediaStreamTrackProcessor = class {
      constructor() {
        this.readable = {
          getReader: () => ({
            read: () => firstFrame.promise,
            cancel: async () => firstFrame.resolve({ done: true }),
            releaseLock() {},
          }),
        };
      }
    };
  }
  const BaseAudioContext = globalThis.AudioContext;
  globalThis.AudioContext = class extends BaseAudioContext {
    constructor() {
      super();
      contexts.push(this);
    }
    async close() {
      this.state = "closed";
    }
    createAnalyser() {
      return { connect() {}, disconnect() {}, getByteTimeDomainData: (a) => a.fill(128) };
    }
  };
  navigator.mediaDevices.enumerateDevices = async () => [
    { kind: "audioinput", deviceId: "external", label: "External microphone" },
  ];
  navigator.mediaDevices.getUserMedia = (constraints) => {
    const request = deferred();
    requests.push({ ...request, constraints });
    return request.promise;
  };
  globalThis.MediaRecorder = class {
    constructor(stream) {
      if (failRecorder) throw new Error("Recorder setup failed");
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
    data(text = "speech".repeat(500)) {
      this.ondataavailable?.({ data: new Blob([text], { type: this.mimeType }) });
    }
    async finish() {
      this.data();
      await this.onstop?.();
    }
  };
  t.after(() => delete globalThis.MediaRecorder);
  for (const name of [
    "onToggleDictation",
    "onStartDictation",
    "onPrepareDictation",
    "onCancelDictationPreparation",
    "onStopDictation",
  ]) {
    window.electronAPI[name] = (callback) => {
      events[name] = callback;
      return () => delete events[name];
    };
  }
  Object.assign(window.electronAPI, {
    captureDictationTarget: async () => {
      targetCaptures.push("editor");
      return captureTarget?.();
    },
    dictationLifecycleStateChanged: (state) => lifecycle.push(state),
    pasteText: async (text) => {
      pastes.push(text);
      return { success: true, pasted: true };
    },
    saveTranscription: async (...args) => {
      saved.push(args);
      return { id: saved.length };
    },
    saveTranscriptionAudio: async (...args) => {
      savedAudio.push(args);
      return { success: true };
    },
    recordAnalyticsEvent: async () => ({ success: true }),
    getSystemDefaultMicrophone: async () => ({ name: "External microphone" }),
    getLaptopLidState: async () => false,
  });
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (_url, options) => {
    audioPayloads.push(options.body);
    return transcription.promise;
  };
  t.after(() => {
    globalThis.fetch = originalFetch;
  });
  const vite = await createRendererServer(t);
  const { useAudioRecording } = await vite.ssrLoadModule("/hooks/useAudioRecording.js");
  const { useSettingsStore } = await vite.ssrLoadModule("/stores/settingsStore.ts");
  useSettingsStore.setState({
    transcriptionMode: "self-hosted",
    useLocalWhisper: false,
    reasoningMode: "disabled",
    useReasoningModel: false,
  });
  let api;
  const toast = (entry) => toasts.push(entry);
  function Harness() {
    api = useAudioRecording(toast);
    return null;
  }
  root = createRoot(container);
  const flush = () =>
    React.act(async () => {
      await new Promise(setImmediate);
    });
  await React.act(async () => root.render(React.createElement(Harness)));
  return {
    api: () => api,
    events,
    lifecycle,
    toasts,
    pastes,
    saved,
    savedAudio,
    targetCaptures,
    requests,
    recorders,
    streams,
    contexts,
    storage,
    audioPayloads,
    transcription,
    flush,
    async act(fn) {
      await React.act(async () => {
        fn();
        await new Promise(setImmediate);
      });
    },
    async unmount() {
      await React.act(async () => root.unmount());
      root = null;
    },
    resolveMic(index, { muted = false } = {}) {
      const track = Object.assign(new EventTarget(), {
        label: "External microphone",
        readyState: "live",
        muted,
        getSettings: () => ({ deviceId: "external" }),
        stop() {
          this.readyState = "ended";
        },
      });
      const stream = { getTracks: () => [track], getAudioTracks: () => [track] };
      stream.clone = () => {
        const clone = Object.assign(new EventTarget(), {
          readyState: "live",
          muted: false,
          label: track.label,
          getSettings: track.getSettings,
          stop: track.stop,
        });
        const value = { getTracks: () => [clone], getAudioTracks: () => [clone] };
        streams.push(value);
        return value;
      };
      streams.push(stream);
      requests[index].resolve(stream);
      return track;
    },
  };
}

test("normal stop releases capture before recorder delivery or transcription with an old idle hold", async (t) => {
  const h = await mountCapture(t);
  let start;
  await h.act(() => {
    start = h.api().startRecording();
  });
  assert.equal(h.requests.length, 1);
  await h.act(() => h.resolveMic(0));
  assert.equal(await start, true, JSON.stringify(h.toasts));
  await h.act(() => h.api().stopRecording());
  assert.ok(
    h.streams.every((stream) => stream.getTracks().every((track) => track.readyState === "ended"))
  );
  assert.equal(h.audioPayloads.length, 0);
  let finished;
  await h.act(() => {
    finished = h.recorders[0].finish();
  });
  assert.equal(h.audioPayloads.length, 1);
  assert.ok(
    h.streams.every((stream) => stream.getTracks().every((track) => track.readyState === "ended"))
  );
  await h.act(() =>
    h.transcription.resolve({
      ok: true,
      status: 200,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify({ text: "A completed dictation." }),
    })
  );
  await finished;
  await h.flush();
  assert.deepEqual(h.pastes, ["A completed dictation."]);
  assert.equal(h.saved[0][0], "A completed dictation.");
  assert.equal(h.savedAudio.length, 1);
  assert.deepEqual(h.targetCaptures, ["editor"]);
});

test("push-to-talk release during acquisition disposes the late stream without recording or output", async (t) => {
  const h = await mountCapture(t);
  await h.act(() => h.events.onStartDictation());
  assert.equal(h.requests.length, 1);
  await h.act(() => h.events.onStopDictation());
  assert.equal(h.api().isPreparing, false);
  await h.act(() => h.resolveMic(0));
  assert.ok(
    h.streams.every((stream) => stream.getTracks().every((track) => track.readyState === "ended"))
  );
  assert.equal(h.lifecycle.includes("recording"), false);
  assert.equal(h.recorders.length, 0);
  assert.equal(h.audioPayloads.length, 0);
  assert.deepEqual(h.pastes, []);
});

test("retired idle-hold preferences are removed without changing microphone selection", async (t) => {
  const h = await mountCapture(t);
  assert.equal(h.storage.getItem("micWarmHoldSeconds"), null);
  assert.equal(h.storage.getItem("selectedMicDeviceId"), "external");
  assert.equal(h.storage.getItem("microphoneSelectionMode"), "specific");
  assert.equal(h.requests.length, 0, "mounting must not pre-warm capture");
});

test("a retry records independently while the cancelled device open is still pending", async (t) => {
  const h = await mountCapture(t);
  let first;
  let second;
  await h.act(() => {
    first = h.api().startRecording();
  });
  await h.act(() => h.api().cancelRecording());
  await h.act(() => {
    second = h.api().startRecording();
  });
  assert.equal(h.requests.length, 2);
  let currentTrack;
  await h.act(() => {
    currentTrack = h.resolveMic(1);
  });
  assert.equal(await second, true);
  let lateTrack;
  await h.act(() => {
    lateTrack = h.resolveMic(0);
  });
  assert.equal(await first, false);
  assert.equal(lateTrack.readyState, "ended");
  assert.equal(currentTrack.readyState, "live");
  assert.equal(h.api().isRecording, true);
  assert.equal(h.lifecycle.at(-1), "recording");
  await h.act(() => h.api().cancelRecording());
  assert.equal(currentTrack.readyState, "ended");
  assert.deepEqual(h.pastes, []);
});

test("teardown after stop ignores queued recorder completion", async (t) => {
  const h = await mountCapture(t);
  await h.act(() => h.api().startRecording());
  await h.act(() => h.resolveMic(0));
  await h.act(() => h.api().stopRecording());
  await h.unmount();
  await h.act(() => h.recorders[0].finish());
  assert.equal(h.audioPayloads.length, 0);
  assert.deepEqual(h.pastes, []);
  assert.ok(h.contexts.every((context) => context.state === "closed"));
});

test("toggle stop releases prepared capture while target capture is pending", async (t) => {
  const target = deferred();
  const h = await mountCapture(t, { captureTarget: () => target.promise });
  await h.act(() => h.events.onToggleDictation());
  await h.act(() => h.resolveMic(0));
  assert.equal(h.recorders[0].state, "recording");
  assert.equal(h.api().isRecording, false);
  await h.act(() => h.events.onToggleDictation());
  assert.equal(h.streams[0].getTracks()[0].readyState, "ended");
  assert.equal(h.recorders[0].state, "inactive");
  await h.act(() => target.resolve());
  assert.equal(h.lifecycle.includes("recording"), false);
  assert.equal(h.audioPayloads.length, 0);
});

test("cancellation releases acquired capture even while track preparation is pending", async (t) => {
  const h = await mountCapture(t);
  await h.act(() => h.api().startRecording());
  let track;
  await h.act(() => {
    track = h.resolveMic(0, { muted: true });
  });
  await h.act(() => h.api().cancelRecording());
  assert.equal(track.readyState, "ended");
  assert.equal(h.lifecycle.includes("recording"), false);
  assert.equal(h.requests.length, 1, "cancel must not retry the muted input");
  assert.equal(h.recorders.length, 0);
  assert.ok(h.contexts.every((context) => context.state === "closed"));
});

test("recorder startup failure releases its acquired stream and audio resources", async (t) => {
  const h = await mountCapture(t, { failRecorder: true });
  let start;
  await h.act(() => {
    start = h.api().startRecording();
  });
  await h.act(() => h.resolveMic(0));
  assert.equal(await start, false);
  assert.equal(h.streams[0].getTracks()[0].readyState, "ended");
  assert.ok(h.contexts.every((context) => context.state === "closed"));
  assert.equal(h.lifecycle.at(-1), "idle");
  assert.equal(h.toasts.length, 1);
  assert.equal(h.audioPayloads.length, 0);
});

test("teardown releases a late acquisition without creating a recorder", async (t) => {
  const h = await mountCapture(t);
  await h.act(() => h.api().startRecording());
  await h.unmount();
  await h.act(() => h.resolveMic(0));
  assert.equal(h.streams[0].getTracks()[0].readyState, "ended");
  assert.equal(h.recorders.length, 0);
  assert.equal(h.audioPayloads.length, 0);
  assert.deepEqual(h.pastes, []);
});

test("repeated cancellation and queued old recorder events cannot stop a newer recording", async (t) => {
  const h = await mountCapture(t);
  await h.act(() => h.api().startRecording());
  await h.act(() => h.resolveMic(0));
  const oldRecorder = h.recorders[0];
  await h.act(() => h.api().cancelRecording());
  await h.act(() => h.api().cancelRecording());
  await h.act(() => h.api().stopRecording());
  assert.equal(h.streams[0].getTracks()[0].readyState, "ended");
  await h.act(() => h.api().startRecording());
  let currentTrack;
  await h.act(() => {
    currentTrack = h.resolveMic(1);
  });
  await h.act(() => oldRecorder.finish());
  assert.equal(h.api().isRecording, true);
  assert.equal(currentTrack.readyState, "live");
  assert.equal(h.audioPayloads.length, 0);
  await h.unmount();
  assert.equal(currentTrack.readyState, "ended");
  assert.ok(h.contexts.every((context) => context.state === "closed"));
});

test("stop preserves an active recording while startup device bookkeeping is still pending", async (t) => {
  const h = await mountCapture(t);
  await h.act(() => h.api().startRecording());
  const devices = deferred();
  navigator.mediaDevices.enumerateDevices = () => devices.promise;
  await h.act(() => h.resolveMic(0));
  assert.equal(h.api().isRecording, true);
  await h.act(() => h.api().stopRecording());
  await h.act(() => h.recorders[0].finish());
  assert.equal(h.audioPayloads.length, 1, "normal stop must retain the captured dictation");
  await h.act(() => devices.resolve([]));
  assert.equal(h.api().isRecording, false);
});

for (const ending of ["stop", "cancel", "teardown"]) {
  test(`${ending} releases capture while a replacement microphone waits for recorder delivery`, async (t) => {
    const h = await mountCapture(t);
    await h.act(() => h.api().startRecording());
    let track;
    await h.act(() => {
      track = h.resolveMic(0);
    });
    await h.act(() => {
      track.readyState = "ended";
      track.dispatchEvent(new Event("ended"));
    });
    assert.equal(h.requests.length, 2);
    let replacement;
    await h.act(() => {
      replacement = h.resolveMic(1);
    });
    const oldRecorder = h.recorders[0];
    assert.equal(oldRecorder.state, "inactive");
    if (ending === "teardown") await h.unmount();
    else
      await h.act(() => (ending === "stop" ? h.api().stopRecording() : h.api().cancelRecording()));
    assert.equal(replacement.readyState, "ended");
    if (ending === "cancel") {
      await h.act(() => h.api().startRecording());
      await h.act(() => h.resolveMic(2));
    }
    await h.act(() => oldRecorder.finish());
    if (ending === "stop") assert.equal(h.audioPayloads.length, 1);
    else assert.equal(h.audioPayloads.length, 0);
    if (ending === "cancel") {
      assert.equal(h.api().isRecording, true);
      assert.equal(h.streams[2].getTracks()[0].readyState, "live");
    }
  });
}

test("a cancelled acquisition cannot consume a retry's prepared but unadopted stream", async (t) => {
  const target = deferred();
  let targets = 0;
  const h = await mountCapture(t, {
    captureTarget: () => (++targets === 2 ? target.promise : undefined),
  });
  let first;
  let second;
  await h.act(() => {
    first = h.api().startRecording();
  });
  await h.act(() => h.api().cancelRecording());
  await h.act(() => {
    second = h.api().startRecording();
  });
  let currentTrack;
  await h.act(() => {
    currentTrack = h.resolveMic(1);
  });
  await h.act(() => h.resolveMic(0));
  assert.equal(await first, false);
  assert.equal(h.api().isPreparing, true);
  assert.equal(currentTrack.readyState, "live");
  await h.act(() => target.resolve());
  assert.equal(await second, true);
  assert.equal(h.requests.length, 2);
  assert.equal(h.api().isRecording, true);
});

test("cancelling preparation before visual frames arrive never opens the microphone", async (t) => {
  const h = await mountCapture(t);
  const frames = [];
  globalThis.requestAnimationFrame = (callback) => {
    frames.push(callback);
    return frames.length;
  };
  await h.act(() => h.events.onPrepareDictation());
  await h.act(() => h.events.onCancelDictationPreparation());
  await h.act(() => {
    while (frames.length) frames.shift()();
  });
  assert.equal(h.requests.length, 0);
  assert.equal(h.api().isPreparing, false);
  assert.equal(h.lifecycle.at(-1), "idle");
});

test("cancelling after a dead recorder delivered its data preserves discarded History", async (t) => {
  const h = await mountCapture(t, { initialStorage: { saveDiscardedTranscriptions: "true" } });
  const realNow = Date.now;
  let now = realNow();
  t.mock.method(Date, "now", () => now);
  await h.act(() => h.api().startRecording());
  let track;
  await h.act(() => {
    track = h.resolveMic(0);
  });
  now += 5000;
  await h.act(() => {
    track.readyState = "ended";
    track.dispatchEvent(new Event("ended"));
    h.recorders[0].stop();
    void h.recorders[0].finish();
  });
  assert.equal(h.requests.length, 2);
  await h.act(() => h.api().cancelRecording());
  assert.equal(h.saved.length, 1);
  assert.equal(h.saved[0][2].status, "discarded");
  assert.equal(h.savedAudio.length, 1);
  await h.act(() => h.resolveMic(1));
  assert.equal(h.streams[1].getTracks()[0].readyState, "ended");
  assert.equal(h.audioPayloads.length, 0);
  assert.deepEqual(h.pastes, []);
});

test("normal stop waits for final recorder data after the input track ends", async (t) => {
  const h = await mountCapture(t);
  await h.act(() => h.api().startRecording());
  let track;
  await h.act(() => {
    track = h.resolveMic(0);
  });
  await h.act(() => {
    track.readyState = "ended";
    track.dispatchEvent(new Event("ended"));
    h.recorders[0].stop();
  });
  await h.act(() => h.api().stopRecording());
  assert.equal(h.api().isProcessing, true);
  assert.equal(h.audioPayloads.length, 0);
  await h.act(() => h.recorders[0].finish());
  assert.equal(h.audioPayloads.length, 1);
  await h.act(() => h.resolveMic(1));
  assert.equal(h.streams[1].getTracks()[0].readyState, "ended");
});

test("prepared audio and speech immediately after readiness reach transcription in order", async (t) => {
  const target = deferred();
  const firstFrame = deferred();
  const h = await mountCapture(t, { captureTarget: () => target.promise, firstFrame });
  await h.act(() => h.api().startRecording());
  await h.act(() => h.resolveMic(0));
  h.recorders[0].data("opening ");
  await h.act(() =>
    firstFrame.resolve({ done: false, value: { numberOfFrames: 480, close() {} } })
  );
  assert.equal(h.api().isPreparing, true);
  await h.act(() => target.resolve());
  assert.equal(h.api().isRecording, true);
  h.recorders[0].data("after ready ");
  await h.act(() => h.api().stopRecording());
  await h.act(() => h.recorders[0].finish());
  assert.equal(h.requests.length, 1);
  assert.equal(h.recorders.length, 1);
  assert.equal(h.audioPayloads.length, 1);
  assert.match(await h.audioPayloads[0].get("file").text(), /^opening after ready speech/);
});
