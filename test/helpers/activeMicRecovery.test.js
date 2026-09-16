const test = require("node:test");
const assert = require("node:assert/strict");

class FakeTrack extends EventTarget {
  constructor({ deviceId = "mic-1", groupId = "group-1", muted = false } = {}) {
    super();
    this.readyState = "live";
    this.muted = muted;
    this.settings = { deviceId, groupId };
    this.stopped = false;
  }

  getSettings() {
    return this.settings;
  }

  stop() {
    this.stopped = true;
    this.readyState = "ended";
  }
}

class FakeStream {
  constructor(track) {
    this.track = track;
  }

  getAudioTracks() {
    return this.track ? [this.track] : [];
  }

  getTracks() {
    return this.getAudioTracks();
  }
}

class FakeMediaDevices extends EventTarget {
  constructor(devices) {
    super();
    this.devices = devices;
  }

  async enumerateDevices() {
    return this.devices;
  }

  change(devices) {
    this.devices = devices;
    this.dispatchEvent(new Event("devicechange"));
  }
}

const mic = (deviceId, groupId = deviceId, label = deviceId) => ({
  kind: "audioinput",
  deviceId,
  groupId,
  label,
});

const speaker = (deviceId) => ({
  kind: "audiooutput",
  deviceId,
  groupId: deviceId,
  label: deviceId,
});
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const deferred = () => {
  let resolve;
  const promise = new Promise((fulfill) => (resolve = fulfill));
  return { promise, resolve };
};

test("filters output-only changes and keeps a healthy pinned microphone", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const mediaDevices = new FakeMediaDevices([mic("mic-1")]);
  const original = new FakeStream(new FakeTrack());
  let acquired = 0;
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    acquire: async () => {
      acquired += 1;
      return new FakeStream(new FakeTrack({ deviceId: "mic-2" }));
    },
    onRecovered: async () => {},
    debounceMs: 1,
  });
  await controller.start(original, { followDefault: false });
  mediaDevices.change([mic("mic-1"), speaker("speaker-2")]);
  await delay(10);
  assert.equal(acquired, 0);
  controller.stop();
});

test("recovers when the system default input changes", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const mediaDevices = new FakeMediaDevices([mic("default", "group-1", "Mic One")]);
  const original = new FakeStream(new FakeTrack());
  const replacement = new FakeStream(new FakeTrack({ deviceId: "mic-2", groupId: "group-2" }));
  let recovered = null;
  const statuses = [];
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    acquire: async () => replacement,
    onRecovered: async (stream) => {
      recovered = stream;
    },
    onStatusChange: (status) => statuses.push(status),
    debounceMs: 1,
  });
  await controller.start(original, { followDefault: true });
  mediaDevices.change([mic("default", "group-2", "Mic Two")]);
  await delay(10);
  assert.equal(recovered, replacement);
  assert.deepEqual(statuses.slice(-2), ["reconnecting", "active"]);
  controller.stop();
});

test("track ended triggers recovery without waiting for devicechange", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const mediaDevices = new FakeMediaDevices([mic("mic-1")]);
  const track = new FakeTrack();
  let acquired = 0;
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    acquire: async () => {
      acquired += 1;
      return new FakeStream(new FakeTrack({ deviceId: "mic-2" }));
    },
    onRecovered: async () => {},
  });
  await controller.start(new FakeStream(track));
  track.readyState = "ended";
  track.dispatchEvent(new Event("ended"));
  await delay(1);
  assert.equal(acquired, 1);
  controller.stop();
});

test("an unavailable microphone retries and later recovers", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const mediaDevices = new FakeMediaDevices([mic("mic-1")]);
  const track = new FakeTrack();
  let attempts = 0;
  const statuses = [];
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    acquire: async () => {
      attempts += 1;
      if (attempts === 1) throw new Error("no mic");
      return new FakeStream(new FakeTrack({ deviceId: "mic-2" }));
    },
    onRecovered: async () => {},
    onStatusChange: (status) => statuses.push(status),
    retryMs: 5,
  });
  await controller.start(new FakeStream(track));
  track.readyState = "ended";
  track.dispatchEvent(new Event("ended"));
  await delay(20);
  assert.equal(attempts, 2);
  assert.ok(statuses.includes("unavailable"));
  assert.equal(statuses.at(-1), "active");
  controller.stop();
});

test("start() with an already-ended track recovers instead of reporting active", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const mediaDevices = new FakeMediaDevices([mic("mic-1")]);
  const track = new FakeTrack();
  track.readyState = "ended";
  let acquired = 0;
  const statuses = [];
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    acquire: async () => {
      acquired += 1;
      return new FakeStream(new FakeTrack({ deviceId: "mic-2" }));
    },
    onRecovered: async () => {},
    onStatusChange: (status) => statuses.push(status),
  });
  await controller.start(new FakeStream(track));
  await delay(5);
  assert.equal(acquired, 1);
  assert.equal(statuses.at(-1), "active");
  controller.stop();
});

test("a stale recovery settling late cannot clobber a newer in-flight recovery", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const mediaDevices = new FakeMediaDevices([mic("mic-1")]);
  const track1 = new FakeTrack();
  const track2 = new FakeTrack({ deviceId: "mic-2" });
  const pendingAcquires = [];
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    acquire: () => new Promise((resolve) => pendingAcquires.push(resolve)),
    onRecovered: async () => {},
  });
  await controller.start(new FakeStream(track1));
  track1.readyState = "ended";
  track1.dispatchEvent(new Event("ended"));
  assert.equal(pendingAcquires.length, 1);

  controller.stop();
  await controller.start(new FakeStream(track2));
  track2.readyState = "ended";
  track2.dispatchEvent(new Event("ended"));
  assert.equal(pendingAcquires.length, 2);

  pendingAcquires[0](new FakeStream(new FakeTrack({ deviceId: "stale" })));
  await delay(1);
  // The newer recovery is still in flight; another trigger must dedupe onto it.
  track2.dispatchEvent(new Event("ended"));
  await delay(1);
  assert.equal(pendingAcquires.length, 2);

  pendingAcquires[1](new FakeStream(new FakeTrack({ deviceId: "mic-3" })));
  await delay(1);
  controller.stop();
});

test("stop invalidates an in-flight acquisition and stops its late stream", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const mediaDevices = new FakeMediaDevices([mic("mic-1")]);
  const track = new FakeTrack();
  const replacementTrack = new FakeTrack({ deviceId: "mic-2" });
  let resolveAcquire;
  let recovered = false;
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    acquire: () => new Promise((resolve) => (resolveAcquire = resolve)),
    onRecovered: async () => {
      recovered = true;
    },
  });
  await controller.start(new FakeStream(track));
  track.readyState = "ended";
  track.dispatchEvent(new Event("ended"));
  controller.stop();
  resolveAcquire(new FakeStream(replacementTrack));
  await delay(1);
  assert.equal(recovered, false);
  assert.equal(replacementTrack.stopped, true);
});

test("preferred input changes replace a healthy microphone and ignore output changes", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const builtIn = mic("built-in");
  const phone = mic("phone");
  const mediaDevices = new FakeMediaDevices([builtIn, phone]);
  let preferred = builtIn;
  const reasons = [];
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    resolvePreferredDevice: async () => preferred,
    acquire: async (reason) => {
      reasons.push(reason);
      return new FakeStream(new FakeTrack({ deviceId: preferred.deviceId }));
    },
    onRecovered: async () => {},
    debounceMs: 1,
  });
  await controller.start(new FakeStream(new FakeTrack({ deviceId: "built-in" })), {
    followDefault: false,
  });
  preferred = phone;
  controller.scheduleEvaluation();
  await delay(10);
  assert.equal(controller.track.getSettings().deviceId, "phone");
  assert.deepEqual(reasons, ["preferred-change"]);

  mediaDevices.change([builtIn, phone, speaker("new-output")]);
  await delay(10);
  assert.equal(reasons.length, 1);

  preferred = builtIn;
  controller.scheduleEvaluation();
  await delay(10);
  assert.equal(controller.track.getSettings().deviceId, "built-in");
  assert.deepEqual(reasons, ["preferred-change", "preferred-change"]);
  controller.stop();
});

test("start rechecks the preferred device after initial capture opens", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const phone = mic("phone");
  const mediaDevices = new FakeMediaDevices([mic("built-in"), phone]);
  let recovered = false;
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    resolvePreferredDevice: async () => phone,
    acquire: async () => new FakeStream(new FakeTrack({ deviceId: "phone" })),
    onRecovered: async () => {
      recovered = true;
    },
  });
  await controller.start(new FakeStream(new FakeTrack({ deviceId: "built-in" })), {
    followDefault: false,
  });
  await delay(1);
  assert.equal(recovered, true);
  assert.equal(controller.track.getSettings().deviceId, "phone");
  controller.stop();
});

for (const latestTarget of ["built-in", "phone"]) {
  test(`pending replacement settles on latest preferred input: ${latestTarget}`, async () => {
    const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
    const inputs = [mic("built-in"), mic("phone")];
    const mediaDevices = new FakeMediaDevices(inputs);
    let preferred = inputs[0];
    const pendingAcquires = [];
    const replacements = [];
    const controller = new ActiveMicRecoveryController({
      mediaDevices,
      resolvePreferredDevice: async () => preferred,
      acquire: () => {
        const pending = deferred();
        pendingAcquires.push(pending);
        return pending.promise;
      },
      onRecovered: async (stream) => replacements.push(stream.track.getSettings().deviceId),
      debounceMs: 1,
    });
    await controller.start(new FakeStream(new FakeTrack({ deviceId: "built-in" })), {
      followDefault: false,
    });
    preferred = inputs[1];
    await controller.evaluateDeviceChange();
    assert.equal(pendingAcquires.length, 1);

    preferred = inputs[0];
    controller.scheduleEvaluation();
    await delay(5);
    if (latestTarget === "phone") {
      preferred = inputs[1];
      controller.scheduleEvaluation();
      await delay(5);
    }
    assert.equal(pendingAcquires.length, 1);
    pendingAcquires[0].resolve(new FakeStream(new FakeTrack({ deviceId: "phone" })));
    await delay(10);
    if (latestTarget === "built-in") {
      assert.equal(pendingAcquires.length, 2);
      pendingAcquires[1].resolve(new FakeStream(new FakeTrack({ deviceId: "built-in" })));
      await delay(5);
      assert.deepEqual(replacements, ["phone", "built-in"]);
    } else {
      assert.equal(pendingAcquires.length, 1);
      assert.deepEqual(replacements, ["phone"]);
    }
    assert.equal(controller.track.getSettings().deviceId, latestTarget);
    controller.stop();
  });
}

test("stop and restart invalidate an asynchronous preferred-device lookup", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const mediaDevices = new FakeMediaDevices([mic("built-in"), mic("phone")]);
  const lookup = deferred();
  let pendingLookup = false;
  let acquired = 0;
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    resolvePreferredDevice: () =>
      pendingLookup ? lookup.promise : Promise.resolve(mic("built-in")),
    acquire: async () => {
      acquired += 1;
      return new FakeStream(new FakeTrack({ deviceId: "phone" }));
    },
    onRecovered: async () => {},
  });
  await controller.start(new FakeStream(new FakeTrack({ deviceId: "built-in" })), {
    followDefault: false,
  });
  pendingLookup = true;
  const evaluation = controller.evaluateDeviceChange();
  await delay(1);
  controller.stop();
  pendingLookup = false;
  await controller.start(new FakeStream(new FakeTrack({ deviceId: "built-in" })), {
    followDefault: false,
  });
  lookup.resolve(mic("phone"));
  await evaluation;
  assert.equal(acquired, 0);
  assert.equal(controller.track.getSettings().deviceId, "built-in");
  controller.stop();
});

test("a newer preferred-device evaluation invalidates an older policy result", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const mediaDevices = new FakeMediaDevices([mic("built-in"), mic("phone")]);
  const lookup = deferred();
  let pendingLookup = false;
  let acquired = 0;
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    resolvePreferredDevice: () =>
      pendingLookup ? lookup.promise : Promise.resolve(mic("built-in")),
    acquire: async () => {
      acquired += 1;
      return new FakeStream(new FakeTrack({ deviceId: "phone" }));
    },
    onRecovered: async () => {},
  });
  await controller.start(new FakeStream(new FakeTrack({ deviceId: "built-in" })), {
    followDefault: false,
  });
  pendingLookup = true;
  const staleEvaluation = controller.evaluateDeviceChange();
  await delay(1);
  pendingLookup = false;
  await controller.evaluateDeviceChange();
  lookup.resolve(mic("phone"));
  await staleEvaluation;
  assert.equal(acquired, 0);
  controller.stop();
});

test("superseded policy lookups do not consume a system default change", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const mediaDevices = new FakeMediaDevices([mic("mic-1"), mic("mic-2")]);
  const lookup = deferred();
  let pendingLookup = false;
  let acquired = 0;
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    resolvePreferredDevice: () => (pendingLookup ? lookup.promise : Promise.resolve(null)),
    acquire: async () => {
      acquired += 1;
      return new FakeStream(new FakeTrack({ deviceId: "mic-2" }));
    },
    onRecovered: async () => {},
  });
  await controller.start(new FakeStream(new FakeTrack({ deviceId: "mic-1" })));
  mediaDevices.devices = [mic("mic-2"), mic("mic-1")];
  pendingLookup = true;
  const staleEvaluation = controller.evaluateDeviceChange();
  await delay(1);
  pendingLookup = false;
  await controller.evaluateDeviceChange();
  lookup.resolve(null);
  await staleEvaluation;
  assert.equal(acquired, 1);
  controller.stop();
});

test("a healthy fallback retries the preferred device only after input or policy changes", async () => {
  const { ActiveMicRecoveryController } = await import("../../src/helpers/activeMicRecovery.js");
  const builtIn = mic("built-in");
  const phone = mic("phone");
  const mediaDevices = new FakeMediaDevices([builtIn, phone]);
  let preferred = builtIn;
  let acquired = 0;
  const controller = new ActiveMicRecoveryController({
    mediaDevices,
    resolvePreferredDevice: async () => preferred,
    acquire: async () => {
      acquired += 1;
      return new FakeStream(new FakeTrack({ deviceId: "built-in" }));
    },
    onRecovered: async () => {},
    debounceMs: 1,
  });
  await controller.start(new FakeStream(new FakeTrack({ deviceId: "built-in" })), {
    followDefault: false,
  });
  preferred = phone;
  await controller.evaluateDeviceChange();
  await delay(1);
  assert.equal(acquired, 1);
  mediaDevices.change([builtIn, phone, speaker("output")]);
  await delay(10);
  assert.equal(acquired, 1);
  assert.equal(controller.status, "active");

  mediaDevices.change([builtIn, phone, mic("usb"), speaker("output")]);
  await delay(10);
  assert.equal(acquired, 2);

  preferred = builtIn;
  await controller.evaluateDeviceChange();
  preferred = phone;
  await controller.evaluateDeviceChange();
  await delay(1);
  assert.equal(acquired, 3);
  controller.stop();
});
