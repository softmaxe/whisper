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

async function setup(t) {
  t.mock.timers.enable({ apis: ["setInterval", "setTimeout"] });
  let root;
  let hook;
  t.after(async () => {
    if (root)
      await React.act(async () => {
        await hook.cancelRecording();
        root.unmount();
      });
  });
  const events = {};
  const logs = [];
  const lifecycle = [];
  const target = deferred();
  const electronAPI = {
    getLogLevel: async () => "info",
    log: async (entry) => logs.push(entry),
    captureDictationTarget: () => target.promise,
    onLaptopLidStateChanged: (callback) => {
      events.LidChanged = callback;
      return () => delete events.LidChanged;
    },
    dictationLifecycleStateChanged: (state) => lifecycle.push(state),
  };
  for (const name of [
    "ToggleDictation",
    "StartDictation",
    "PrepareDictation",
    "CancelDictationPreparation",
    "StopDictation",
  ]) {
    electronAPI[`on${name}`] = (callback) => {
      events[name] = callback;
      return () => delete events[name];
    };
  }
  installBrowserGlobals(t, { window: { electronAPI } });
  const container = installHookDom(t);
  const media = installMicCaptureGlobals(t);
  media.track.addEventListener = () => {};
  media.track.removeEventListener = () => {};
  const frames = [];
  globalThis.requestAnimationFrame = (callback) => frames.push(callback);
  let now = 1000;
  t.mock.method(performance, "now", () => now);
  const recorders = [];
  class Recorder {
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
      this.onstop?.();
    }
  }
  const originalRecorder = globalThis.MediaRecorder;
  const originalProcessor = globalThis.MediaStreamTrackProcessor;
  const readers = new Map();
  globalThis.MediaStreamTrackProcessor = class {
    constructor({ track }) {
      const pending = deferred();
      const reader = {
        pending,
        cancelled: false,
        read: () => pending.promise,
        cancel: async () => {
          reader.cancelled = true;
          pending.resolve({ done: true });
        },
        releaseLock() {},
      };
      readers.set(track, reader);
      this.readable = { getReader: () => reader };
    }
  };
  globalThis.MediaRecorder = Recorder;
  t.after(() => {
    if (originalRecorder === undefined) delete globalThis.MediaRecorder;
    else globalThis.MediaRecorder = originalRecorder;
    if (originalProcessor === undefined) delete globalThis.MediaStreamTrackProcessor;
    else globalThis.MediaStreamTrackProcessor = originalProcessor;
  });
  const vite = await createRendererServer(t);
  const { useSettingsStore } = await vite.ssrLoadModule("/stores/settingsStore.ts");
  useSettingsStore.setState({
    transcriptionMode: "self-hosted",
    useLocalWhisper: false,
    microphoneSelectionMode: "system",
    micWarmHoldSeconds: 0,
    audioCuesEnabled: false,
    dataRetentionEnabled: false,
  });
  const { useAudioRecording } = await vite.ssrLoadModule("/hooks/useAudioRecording.js");
  const toasts = [];
  const toast = (value) => toasts.push(value);
  function Harness() {
    hook = useAudioRecording(toast);
    return null;
  }
  root = createRoot(container);
  await React.act(async () => root.render(React.createElement(Harness)));
  return {
    events,
    electronAPI,
    toasts,
    settings: useSettingsStore,
    logs,
    lifecycle,
    target,
    media,
    recorders,
    readers,
    hook: () => hook,
    advance: (ms) => {
      now += ms;
    },
    request: {
      requestId: "00000000-0000-4000-8000-000000000023",
      acceptedAt: performance.timeOrigin + now - 20,
    },
    paint: async () =>
      React.act(async () => {
        frames.splice(0).forEach((callback) => callback(now));
        frames.splice(0).forEach((callback) => callback(now));
      }),
    timing: () =>
      logs.filter((entry) => entry.message === "Recording startup").map((entry) => entry.meta),
    deliver: async (track = media.track) => {
      let closed = false;
      await React.act(async () =>
        readers.get(track)?.pending.resolve({
          done: false,
          value: {
            numberOfFrames: 480,
            close: () => {
              closed = true;
            },
          },
        })
      );
      return closed;
    },
  };
}

test("Dictation timing includes IPC and visual waiting before capture and overlapping target capture", async (t) => {
  const h = await setup(t);
  const acquisition = deferred();
  const opens = [];
  h.media.mediaDevices.getUserMedia = (constraints) => {
    opens.push(constraints);
    return acquisition.promise;
  };
  await React.act(async () => h.events.ToggleDictation({ startupRequest: h.request }));
  assert.equal(h.hook().isPreparing, true);
  assert.equal(opens.length, 0);
  assert.equal(h.timing().at(-1)?.stages.preparationEntered, 20);
  h.advance(40);
  await h.paint();
  assert.equal(opens.length, 1);
  assert.equal(h.timing().at(-1).stages.acquisitionRequested, 60);
  h.advance(100);
  await React.act(async () => acquisition.resolve(h.media.stream));
  assert.equal(h.hook().isRecording, false);
  h.advance(30);
  await React.act(async () => h.target.resolve());
  assert.equal(h.hook().isRecording, true);
  const trace = h.timing().at(-1);
  assert.equal(trace.requestId, h.request.requestId);
  assert.equal(trace.stages.acquisitionCompleted, 160);
  assert.equal(trace.stages.readyFeedback, 190);
  assert.equal(trace.stages.firstAudio, undefined);
  assert.equal(trace.outcome, "pending");
  await React.act(async () => h.hook().cancelRecording());
});

test("a silent input frame completes startup timing after the existing ready feedback", async (t) => {
  const h = await setup(t);
  await React.act(async () => h.events.ToggleDictation({ startupRequest: h.request }));
  h.advance(40);
  await h.paint();
  h.advance(10);
  await React.act(async () => h.target.resolve());
  assert.equal(h.hook().isRecording, true);
  assert.equal(h.timing().at(-1).outcome, "pending");
  assert.equal(h.timing().at(-1).stages.firstAudio, undefined);
  h.advance(100);
  assert.equal(await h.deliver(), true);
  const trace = h.timing().at(-1);
  assert.equal(trace.stages.firstAudio, 170);
  assert.equal(trace.stages.readyFeedback, 70);
  assert.equal(trace.totalMs, 170);
  assert.equal(trace.outcome, "completed");
  assert.equal(h.readers.get(h.media.track).cancelled, true);
  assert.equal(h.media.track.readyState, "live");
  assert.equal(h.recorders.length, 1);
  assert.equal(h.recorders[0].state, "recording");
});

test("preparation and start share one capture and retain first audio observed before ready feedback", async (t) => {
  const h = await setup(t);
  let opens = 0;
  h.media.mediaDevices.getUserMedia = async () => {
    opens += 1;
    return h.media.stream;
  };
  await React.act(async () => {
    void h.events.PrepareDictation({ startupRequest: h.request });
  });
  h.advance(40);
  await h.paint();
  h.advance(30);
  assert.equal(await h.deliver(), true);
  const recorder = h.recorders[0];
  assert.equal(recorder.state, "recording");
  await React.act(async () => h.events.StartDictation({ startupRequest: h.request }));
  h.advance(40);
  await h.paint();
  h.advance(50);
  await React.act(async () => h.target.resolve());
  assert.equal(h.hook().isRecording, true);
  assert.equal(opens, 1);
  assert.deepEqual(h.recorders, [recorder]);
  assert.equal(recorder.state, "recording");
  const trace = h.timing().at(-1);
  assert.equal(trace.stages.firstAudio, 90);
  assert.equal(trace.stages.readyFeedback, 180);
  assert.equal(trace.totalMs, 180);
  assert.equal(trace.outcome, "completed");
});

test("a rejected microphone request reports failure with absent audio stages and no sensitive timing metadata", async (t) => {
  const h = await setup(t);
  h.media.track.label = "Private Person's iPhone";
  h.media.mediaDevices.getUserMedia = async () => {
    throw Object.assign(new Error("secret-token https://private-server.invalid Private Person"), {
      name: "NotAllowedError",
    });
  };
  await React.act(async () => h.events.ToggleDictation({ startupRequest: h.request }));
  h.advance(40);
  await h.paint();
  await React.act(async () => h.target.resolve());
  assert.equal(h.hook().isRecording, false);
  assert.equal(h.hook().isPreparing, false);
  const trace = h.timing().at(-1);
  assert.equal(trace.outcome, "failed");
  assert.equal(trace.totalMs, null);
  assert.equal(trace.stages.acquisitionCompleted, undefined);
  assert.equal(trace.stages.trackReady, undefined);
  assert.equal(trace.stages.firstAudio, undefined);
  assert.equal(trace.stages.readyFeedback, undefined);
  assert.doesNotMatch(
    JSON.stringify(h.timing()),
    /Private|secret-token|private-server|transcript|audioBuffer/
  );
});

test("cancelling preparation preserves the old identity when its acquisition resolves during a new request", async (t) => {
  const h = await setup(t);
  const old = deferred();
  const next = deferred();
  let opens = 0;
  h.media.mediaDevices.getUserMedia = () => (++opens === 1 ? old.promise : next.promise);
  await React.act(async () => {
    void h.events.PrepareDictation({ startupRequest: h.request });
  });
  await h.paint();
  await React.act(async () => h.events.CancelDictationPreparation());
  assert.equal(h.hook().isPreparing, false);
  const cancelled = h.timing().at(-1);
  assert.equal(cancelled.outcome, "cancelled");
  assert.equal(cancelled.stages.acquisitionCompleted, undefined);
  assert.equal(cancelled.totalMs, null);
  h.advance(100);
  const nextRequest = {
    requestId: "00000000-0000-4000-8000-000000000024",
    acceptedAt: h.request.acceptedAt + 120,
  };
  await React.act(async () => {
    void h.events.PrepareDictation({ startupRequest: nextRequest });
  });
  await h.paint();
  let oldStopped = false;
  const oldTrack = {
    ...h.media.track,
    stop: () => {
      oldStopped = true;
    },
  };
  const oldStream = { getAudioTracks: () => [oldTrack], getTracks: () => [oldTrack] };
  await React.act(async () => old.resolve(oldStream));
  assert.equal(oldStopped, true);
  assert.equal(h.hook().isPreparing, true);
  const oldLate = h
    .timing()
    .filter((entry) => entry.requestId === h.request.requestId)
    .at(-1);
  assert.equal(oldLate.outcome, "cancelled");
  assert.equal(oldLate.lateStage, "acquisitionCompleted");
  assert.equal(oldLate.stages.acquisitionCompleted, undefined);
  assert.equal(
    h
      .timing()
      .filter((entry) => entry.requestId === nextRequest.requestId)
      .at(-1).stages.acquisitionCompleted,
    undefined
  );
  h.advance(70);
  await React.act(async () => next.resolve(h.media.stream));
  await h.deliver();
  await React.act(async () => h.events.StartDictation({ startupRequest: nextRequest }));
  await h.paint();
  await React.act(async () => h.target.resolve());
  const completed = h.timing().at(-1);
  assert.equal(completed.requestId, nextRequest.requestId);
  assert.equal(completed.outcome, "completed");
  assert.equal(completed.stages.acquisitionCompleted, 70);
  assert.equal(completed.stages.firstAudio, 70);
  assert.equal(opens, 2);
});

test("cancelling during visual waiting leaves acquisition and ready stages absent", async (t) => {
  const h = await setup(t);
  await React.act(async () => h.events.ToggleDictation({ startupRequest: h.request }));
  await React.act(async () => h.hook().cancelRecording());
  await h.paint();
  const trace = h.timing().at(-1);
  assert.equal(trace.outcome, "cancelled");
  assert.equal(trace.totalMs, null);
  assert.equal(trace.stages.acquisitionRequested, undefined);
  assert.equal(trace.stages.readyFeedback, undefined);
  assert.equal(h.hook().isPreparing, false);
  assert.equal(h.hook().isRecording, false);
});

test("stop during acquisition releases late capture without changing a completed retry trace", async (t) => {
  const h = await setup(t);
  const old = deferred();
  const next = deferred();
  let opens = 0;
  h.media.mediaDevices.getUserMedia = () => (++opens === 1 ? old.promise : next.promise);
  await React.act(async () => h.events.StartDictation({ startupRequest: h.request }));
  await h.paint();
  await React.act(async () => h.target.resolve());
  await React.act(async () => h.events.StopDictation());
  assert.equal(h.hook().isPreparing, false);
  assert.equal(h.hook().isRecording, false);
  assert.equal(h.timing().at(-1).outcome, "incomplete");

  const nextRequest = { ...h.request, requestId: "00000000-0000-4000-8000-000000000025" };
  await React.act(async () => h.events.StartDictation({ startupRequest: nextRequest }));
  await h.paint();
  assert.equal(opens, 2);
  await React.act(async () => next.resolve(h.media.stream));
  await h.deliver();
  const completed = h.timing().at(-1);
  assert.equal(completed.requestId, nextRequest.requestId);
  assert.equal(completed.outcome, "completed");

  let stopped = false;
  const track = {
    ...h.media.track,
    stop: () => {
      stopped = true;
    },
  };
  await React.act(async () =>
    old.resolve({ getAudioTracks: () => [track], getTracks: () => [track] })
  );
  assert.equal(stopped, true);
  assert.equal(h.hook().isRecording, true);
  assert.equal(h.recorders.length, 1);
  assert.equal(h.lifecycle.at(-1), "recording");
  const late = h
    .timing()
    .filter((entry) => entry.requestId === h.request.requestId)
    .at(-1);
  assert.equal(late.outcome, "incomplete");
  assert.equal(late.lateStage, "acquisitionCompleted");
  assert.equal(late.stages.firstAudio, undefined);
  assert.deepEqual(
    h
      .timing()
      .filter((entry) => entry.requestId === nextRequest.requestId)
      .at(-1),
    completed
  );
});

test("an observation timeout is incomplete and does not stop or reclassify an ongoing recording", async (t) => {
  const h = await setup(t);
  await React.act(async () => h.events.ToggleDictation({ startupRequest: h.request }));
  await h.paint();
  await React.act(async () => h.target.resolve());
  assert.equal(h.hook().isRecording, true);
  await React.act(async () => t.mock.timers.tick(30000));
  const trace = h.timing().at(-1);
  assert.equal(trace.outcome, "incomplete");
  assert.equal(trace.reason, "observation_timeout");
  assert.equal(trace.totalMs, null);
  assert.equal(trace.stages.firstAudio, undefined);
  assert.equal(h.readers.get(h.media.track).cancelled, true);
  assert.equal(h.hook().isRecording, true);
  assert.equal(h.recorders[0].state, "recording");
});

test("expired preparation cannot supply first audio for the replacement recording input", async (t) => {
  const h = await setup(t);
  const replacementTrack = { ...h.media.track };
  const replacement = {
    getAudioTracks: () => [replacementTrack],
    getTracks: () => [replacementTrack],
  };
  let opens = 0;
  h.media.mediaDevices.getUserMedia = async () => (++opens === 1 ? h.media.stream : replacement);
  await React.act(async () => h.events.ToggleDictation({ startupRequest: h.request }));
  await h.paint();
  await h.deliver();
  h.advance(10001);
  await React.act(async () => t.mock.timers.tick(10001));
  await React.act(async () => h.target.resolve());
  assert.equal(opens, 2);
  assert.equal(h.hook().isRecording, true);
  assert.equal(h.recorders.at(-1).stream, replacement);
  assert.equal(h.timing().at(-1).outcome, "pending");
  assert.equal(h.timing().at(-1).stages.firstAudio, undefined);
  h.advance(80);
  await h.deliver(replacementTrack);
  assert.equal(h.timing().at(-1).outcome, "completed");
  assert.equal(h.timing().at(-1).stages.firstAudio, 10101);
});

test("a new preparation request observes a reused capture without opening another microphone", async (t) => {
  const h = await setup(t);
  let opens = 0;
  h.media.mediaDevices.getUserMedia = async () => {
    opens += 1;
    return h.media.stream;
  };
  await React.act(async () => {
    void h.events.PrepareDictation({ startupRequest: h.request });
  });
  await h.paint();
  h.advance(50);
  const nextRequest = {
    requestId: "00000000-0000-4000-8000-000000000024",
    acceptedAt: h.request.acceptedAt + 70,
  };
  await React.act(async () => {
    void h.events.PrepareDictation({ startupRequest: nextRequest });
  });
  await h.paint();
  await React.act(async () => h.events.StartDictation({ startupRequest: nextRequest }));
  await h.paint();
  await React.act(async () => h.target.resolve());
  await h.deliver();
  assert.equal(opens, 1);
  assert.equal(h.recorders.length, 1);
  assert.equal(h.hook().isRecording, true);
  const trace = h.timing().at(-1);
  assert.equal(trace.requestId, nextRequest.requestId);
  assert.equal(trace.outcome, "completed");
  assert.equal(trace.captureSource, "prepared");
  assert.equal(
    h
      .timing()
      .filter((entry) => entry.requestId === h.request.requestId)
      .at(-1).outcome,
    "incomplete"
  );
});

const selectedPhone = { kind: "audioinput", deviceId: "phone", label: "iPhone Microphone" };
const alternativeMic = { kind: "audioinput", deviceId: "builtin", label: "MacBook Microphone" };

function selectPhone(h) {
  h.settings.setState({
    microphoneSelectionMode: "specific",
    selectedMicDeviceId: selectedPhone.deviceId,
    selectedMicDeviceLabel: selectedPhone.label,
  });
  h.media.mediaDevices.enumerateDevices = async () => [selectedPhone, alternativeMic];
}

test("a rejected selected microphone never opens the healthy alternative, including on retry", async (t) => {
  const h = await setup(t);
  selectPhone(h);
  const opens = [];
  h.media.mediaDevices.getUserMedia = async (constraints) => {
    const id = constraints.audio.deviceId?.exact;
    opens.push(id);
    if (id === "phone")
      throw Object.assign(new Error("Device unavailable"), { name: "OverconstrainedError" });
    return h.media.stream;
  };
  for (let attempt = 0; attempt < 2; attempt += 1) {
    await React.act(async () => h.events.StartDictation());
    await h.paint();
    await React.act(async () => h.target.resolve());
    assert.equal(h.hook().isRecording, false);
    assert.equal(h.hook().isPreparing, false);
    assert.ok(h.toasts.length > 0);
  }
  assert.ok(opens.length >= 2);
  assert.ok(opens.every((id) => id === "phone"));
});

test("an ended selected input releases capture without acquiring a healthy alternative", async (t) => {
  const h = await setup(t);
  selectPhone(h);
  const opens = [];
  let stopped = false;
  h.media.track.readyState = "ended";
  h.media.track.stop = () => {
    stopped = true;
  };
  h.media.mediaDevices.getUserMedia = async (constraints) => {
    opens.push(constraints.audio.deviceId?.exact);
    if (constraints.audio.deviceId?.exact === "phone") return h.media.stream;
    return {
      getAudioTracks: () => [{ readyState: "live", getSettings: () => ({}) }],
      getTracks: () => [],
    };
  };
  await React.act(async () => h.events.StartDictation());
  await h.paint();
  await React.act(async () => h.target.resolve());
  assert.equal(h.hook().isRecording, false);
  assert.equal(stopped, true);
  assert.ok(opens.every((id) => id === "phone"));
  assert.ok(h.toasts.length > 0);
});

test("preparation keeps its device when preferences change and the next Dictation uses the new choice", async (t) => {
  const h = await setup(t);
  selectPhone(h);
  const opens = [];
  h.media.mediaDevices.getUserMedia = async (constraints) => {
    opens.push(constraints.audio.deviceId?.exact);
    return h.media.stream;
  };
  await React.act(async () => {
    void h.events.PrepareDictation();
  });
  await h.paint();
  await React.act(async () =>
    h.settings.setState({
      selectedMicDeviceId: "builtin",
      selectedMicDeviceLabel: alternativeMic.label,
    })
  );
  await React.act(async () => h.events.StartDictation());
  await h.paint();
  await React.act(async () => h.target.resolve());
  assert.equal(h.hook().isRecording, true);
  assert.deepEqual(opens, ["phone"]);
  await React.act(async () => h.hook().cancelRecording());
  await React.act(async () => h.events.StartDictation());
  await h.paint();
  assert.equal(h.hook().isRecording, true);
  assert.deepEqual(opens, ["phone", "builtin"]);
});

test("a selected microphone disconnect ends Dictation and reports failure without replacement", async (t) => {
  const h = await setup(t);
  selectPhone(h);
  const target = new EventTarget();
  h.media.track.addEventListener = target.addEventListener.bind(target);
  h.media.track.removeEventListener = target.removeEventListener.bind(target);
  let stopped = false;
  h.media.track.stop = () => {
    stopped = true;
  };
  const opens = [];
  h.media.mediaDevices.getUserMedia = async (constraints) => {
    opens.push(constraints.audio.deviceId?.exact);
    return h.media.stream;
  };
  await React.act(async () => h.events.StartDictation());
  await h.paint();
  await React.act(async () => h.target.resolve());
  assert.equal(h.hook().isRecording, true);
  await React.act(async () => {
    h.media.track.readyState = "ended";
    target.dispatchEvent(new Event("ended"));
  });
  assert.equal(h.hook().isRecording, false);
  assert.equal(stopped, true);
  assert.deepEqual(opens, ["phone"]);
  assert.match(h.toasts.at(-1).description, /Reconnect it/);
});

test("a stale selected ID with only a label match fails instead of guessing physical identity", async (t) => {
  const h = await setup(t);
  selectPhone(h);
  h.media.mediaDevices.enumerateDevices = async () => [
    { ...selectedPhone, deviceId: "new-phone" },
    alternativeMic,
  ];
  const opens = [];
  h.media.mediaDevices.getUserMedia = async (constraints) => {
    opens.push(constraints.audio.deviceId?.exact);
    return h.media.stream;
  };
  await React.act(async () => h.events.StartDictation());
  await h.paint();
  await React.act(async () => h.target.resolve());
  assert.equal(h.hook().isRecording, false);
  assert.deepEqual(opens, []);
  assert.equal(h.settings.getState().selectedMicDeviceId, "phone");
});

test("the microphone choice is fixed at request acceptance before visual preparation", async (t) => {
  const h = await setup(t);
  selectPhone(h);
  const opens = [];
  h.media.mediaDevices.getUserMedia = async (constraints) => {
    opens.push(constraints.audio.deviceId.exact);
    return h.media.stream;
  };
  await React.act(async () => h.events.StartDictation());
  h.settings.setState({
    selectedMicDeviceId: "builtin",
    selectedMicDeviceLabel: alternativeMic.label,
  });
  await h.paint();
  await React.act(async () => h.target.resolve());
  assert.deepEqual(opens, ["phone"]);
});

function installSelectedInputs(h) {
  let inputs = [selectedPhone, alternativeMic];
  const devices = new EventTarget();
  h.media.mediaDevices.addEventListener = devices.addEventListener.bind(devices);
  h.media.mediaDevices.removeEventListener = devices.removeEventListener.bind(devices);
  h.media.mediaDevices.enumerateDevices = async () => inputs;
  const opens = [];
  const tracks = [];
  h.media.mediaDevices.getUserMedia = async (constraints) => {
    const deviceId = constraints.audio.deviceId.exact;
    opens.push(deviceId);
    const track = Object.assign(new EventTarget(), {
      readyState: "live",
      muted: false,
      getSettings: () => ({ deviceId }),
      stop() {
        this.readyState = "ended";
      },
    });
    tracks.push(track);
    return { getAudioTracks: () => [track], getTracks: () => [track] };
  };
  return {
    opens,
    tracks,
    changeInputs: (next) => {
      inputs = next;
      devices.dispatchEvent(new Event("devicechange"));
    },
  };
}

for (const mode of ["auto", "system"]) {
  test(`${mode} keeps its input through lid, default, and device changes until the next request`, async (t) => {
    const h = await setup(t);
    const input = installSelectedInputs(h);
    let lidClosed = true;
    let defaultDevice = selectedPhone;
    h.electronAPI.getLaptopLidState = async () => lidClosed;
    h.electronAPI.getSystemDefaultMicrophone = async () => ({ name: defaultDevice.label });
    h.settings.setState({ microphoneSelectionMode: mode });
    await React.act(async () => {
      void h.events.PrepareDictation();
    });
    await h.paint();
    await React.act(async () => {
      lidClosed = false;
      defaultDevice = alternativeMic;
      h.events.LidChanged(false);
      input.changeInputs([alternativeMic, selectedPhone]);
      t.mock.timers.tick(250);
    });
    await React.act(async () => h.events.StartDictation());
    await h.paint();
    await React.act(async () => h.target.resolve());
    assert.equal(h.hook().isRecording, true);
    assert.deepEqual(input.opens, ["phone"]);
    await React.act(async () => {
      input.changeInputs([
        alternativeMic,
        selectedPhone,
        { ...alternativeMic, deviceId: "usb", label: "USB Microphone" },
      ]);
      t.mock.timers.tick(250);
    });
    assert.equal(h.hook().isRecording, true);
    assert.deepEqual(input.opens, ["phone"]);
    await React.act(async () => h.hook().cancelRecording());
    assert.equal(input.tracks[0].readyState, "ended");
    await React.act(async () => h.events.StartDictation());
    await h.paint();
    assert.equal(h.hook().isRecording, true);
    assert.deepEqual(input.opens, ["phone", "builtin"]);
  });
}

for (const failure of ["mute", "missing", "recorder-error", "recorder-stop"]) {
  test(`${failure} during recording ends capture with an actionable error and no replacement`, async (t) => {
    const h = await setup(t);
    selectPhone(h);
    const input = installSelectedInputs(h);
    await React.act(async () => h.events.StartDictation());
    await h.paint();
    await React.act(async () => h.target.resolve());
    assert.equal(h.hook().isRecording, true);
    await React.act(async () => {
      if (failure === "mute") {
        input.tracks[0].muted = true;
        input.tracks[0].dispatchEvent(new Event("mute"));
        t.mock.timers.tick(800);
      } else if (failure === "missing") {
        input.changeInputs([alternativeMic]);
        t.mock.timers.tick(250);
      } else if (failure === "recorder-error") {
        h.recorders[0].onerror(new Event("error"));
      } else {
        h.recorders[0].stop();
      }
    });
    assert.equal(h.hook().isRecording, false);
    assert.equal(h.hook().isProcessing, false);
    assert.equal(input.tracks[0].readyState, "ended");
    assert.deepEqual(input.opens, ["phone"]);
    assert.match(h.toasts.at(-1).description, /Reconnect it/);
  });
}

for (const failure of ["muted", "missing", "ambiguous", "NotAllowedError", "NotReadableError"]) {
  test(`${failure} selected input fails preparation without using the available alternative`, async (t) => {
    const h = await setup(t);
    selectPhone(h);
    const opens = [];
    let stopped = false;
    h.media.track.stop = () => {
      stopped = true;
    };
    if (failure === "muted") h.media.track.muted = true;
    if (failure === "missing") h.media.mediaDevices.enumerateDevices = async () => [alternativeMic];
    if (failure === "ambiguous")
      h.media.mediaDevices.enumerateDevices = async () => [
        { ...selectedPhone, deviceId: "phone-1" },
        { ...selectedPhone, deviceId: "phone-2" },
        alternativeMic,
      ];
    h.media.mediaDevices.getUserMedia = async (constraints) => {
      opens.push(constraints.audio.deviceId?.exact);
      if (failure.endsWith("Error"))
        throw Object.assign(new Error("Unavailable"), { name: failure });
      return h.media.stream;
    };
    await React.act(async () => {
      void h.events.PrepareDictation();
    });
    await h.paint();
    await React.act(async () => t.mock.timers.tick(600));
    assert.equal(h.hook().isPreparing, false);
    assert.equal(h.hook().isRecording, false);
    assert.equal(h.lifecycle.at(-1), "idle");
    assert.ok(opens.every((id) => id === "phone"));
    if (failure === "muted") assert.equal(stopped, true);
    assert.match(h.toasts.at(-1).description, /Reconnect it/);
  });
}

test("a delayed selected input remains the only acquisition while a healthy alternative is available", async (t) => {
  const h = await setup(t);
  selectPhone(h);
  const pending = deferred();
  const opens = [];
  h.media.mediaDevices.getUserMedia = (constraints) => {
    opens.push(constraints.audio.deviceId.exact);
    return constraints.audio.deviceId.exact === "phone"
      ? pending.promise
      : Promise.resolve(h.media.stream);
  };
  await React.act(async () => h.events.StartDictation());
  await h.paint();
  await React.act(async () => h.target.resolve());
  await React.act(async () => t.mock.timers.tick(5000));
  assert.equal(h.hook().isPreparing, true);
  assert.equal(h.hook().isRecording, false);
  assert.deepEqual(opens, ["phone"]);
  await React.act(async () => pending.resolve(h.media.stream));
  assert.equal(h.hook().isRecording, true);
  assert.deepEqual(opens, ["phone"]);
});

test("a new request after preparation expiry applies the updated microphone selection", async (t) => {
  const h = await setup(t);
  selectPhone(h);
  const input = installSelectedInputs(h);
  await React.act(async () => {
    void h.events.PrepareDictation();
  });
  await h.paint();
  await React.act(async () => t.mock.timers.tick(10001));
  assert.equal(input.tracks[0].readyState, "ended");
  h.settings.setState({
    selectedMicDeviceId: "builtin",
    selectedMicDeviceLabel: alternativeMic.label,
  });
  await React.act(async () => h.events.StartDictation());
  await h.paint();
  await React.act(async () => h.target.resolve());
  assert.equal(h.hook().isRecording, true);
  assert.deepEqual(input.opens, ["phone", "builtin"]);
});
