const test = require("node:test");
const assert = require("node:assert/strict");

const mic = (deviceId, label) => ({ kind: "audioinput", deviceId, label });

test("auto follows repeated lid changes even when the system default stays on the phone", async () => {
  const { resolveMicrophoneSelection } = await import("../../src/helpers/microphoneSelection.js");
  const builtIn = mic("macbook", "MacBook Pro Microphone (Built-in)");
  const phone = mic("iphone", "Rui's iPhone 16 Pro Max Microphone");
  const usb = mic("usb", "USB Microphone");
  const devices = [mic("default", phone.label), usb, phone, builtIn];

  for (const lidClosed of [false, true, false, true]) {
    const result = resolveMicrophoneSelection(
      devices,
      { microphoneSelectionMode: "auto" },
      { name: phone.label },
      lidClosed
    );
    assert.equal(result.device, lidClosed ? phone : builtIn);
  }
});

test("auto handles an external microphone arriving and leaving while the lid is closed", async () => {
  const { resolveMicrophoneSelection } = await import("../../src/helpers/microphoneSelection.js");
  const builtIn = mic("macbook", "MacBook Pro Microphone");
  const phone = mic("iphone", "Rui's iPhone Microphone");
  const external = mic("usb", "Studio Microphone");
  const settings = { microphoneSelectionMode: "auto" };
  const resolve = (devices) => resolveMicrophoneSelection(devices, settings, null, true);

  assert.equal(resolve([builtIn, external]).device, external);
  assert.equal(resolve([builtIn, external, phone]).device, phone);
  assert.equal(resolve([builtIn, external]).device, external);
  assert.equal(resolve([builtIn]).device, null);
});

test("auto excludes aliases and uses the system default only when no preferred input exists", async () => {
  const { resolveMicrophoneSelection } = await import("../../src/helpers/microphoneSelection.js");
  const builtIn = mic("macbook", "MacBook Pro Microphone");
  const alias = mic("default", "Default - MacBook Pro Microphone");
  const result = resolveMicrophoneSelection(
    [alias, mic("communications", "Communications - USB Microphone"), builtIn],
    { microphoneSelectionMode: "auto" },
    null,
    true
  );
  assert.equal(result.device, builtIn);
  assert.equal(result.status, "chromium-physical");
});

test("auto with an unknown lid state prefers built-in over a custom-named external mic", async () => {
  const { resolveMicrophoneSelection } = await import("../../src/helpers/microphoneSelection.js");
  const builtIn = mic("macbook", "MacBook Pro Microphone");
  const result = resolveMicrophoneSelection([mic("continuity", "Joshua P Microphone"), builtIn], {
    microphoneSelectionMode: "auto",
  });
  assert.equal(result.device, builtIn);
});

test("manual input stays selected regardless of lid position", async () => {
  const { resolveMicrophoneSelection } = await import("../../src/helpers/microphoneSelection.js");
  const builtIn = mic("macbook", "MacBook Pro Microphone");
  const phone = mic("iphone", "Rui's iPhone Microphone");
  for (const lidClosed of [true, false]) {
    assert.equal(
      resolveMicrophoneSelection(
        [builtIn, phone],
        { microphoneSelectionMode: "specific", selectedMicDeviceId: phone.deviceId },
        null,
        lidClosed
      ).device,
      phone
    );
  }
});

test("preferred microphone lookup reads current lid state and tolerates an unavailable monitor", async () => {
  const { resolvePreferredMicrophone } = await import("../../src/helpers/microphoneSelection.js");
  const builtIn = mic("macbook", "MacBook Pro Microphone");
  const phone = mic("iphone", "Rui's iPhone Microphone");
  let lidClosed = true;
  const options = {
    settings: { microphoneSelectionMode: "auto" },
    mediaDevices: { enumerateDevices: async () => [builtIn, phone] },
    getSystemDefault: async () => ({ name: phone.label }),
    getLidState: async () => lidClosed,
  };
  assert.equal((await resolvePreferredMicrophone(options)).device, phone);
  lidClosed = false;
  assert.equal((await resolvePreferredMicrophone(options)).device, builtIn);
  assert.equal(
    (
      await resolvePreferredMicrophone({
        ...options,
        getLidState: async () => {
          throw new Error("Lid monitor unavailable");
        },
      })
    ).device,
    builtIn
  );
});

test("system mode maps the native default name to an exact Chromium input", async () => {
  const { resolveMicrophoneSelection } = await import("../../src/helpers/microphoneSelection.js");
  const expected = mic("macbook", "MacBook Pro Microphone (Built-in)");
  const result = resolveMicrophoneSelection(
    [mic("default", "Default - MacBook Pro Microphone (Built-in)"), expected],
    { microphoneSelectionMode: "system" },
    { name: "MacBook Pro Microphone" }
  );

  assert.equal(result.device, expected);
  assert.equal(result.status, "native-exact");
});

test("system mode uses Chromium's explicit default device when native mapping is unavailable", async () => {
  const { isCacheableMicrophoneResolution, resolveMicrophoneSelection } =
    await import("../../src/helpers/microphoneSelection.js");
  const expected = mic("default", "Default - Microphone Array");
  const result = resolveMicrophoneSelection([expected, mic("continuity", "Joshua P Microphone")], {
    microphoneSelectionMode: "system",
  });

  assert.equal(result.device, expected);
  assert.equal(result.status, "chromium-default");
  assert.equal(isCacheableMicrophoneResolution(result), false);
});

test("a resolved physical microphone can be cached", async () => {
  const { isCacheableMicrophoneResolution, resolveMicrophoneSelection } =
    await import("../../src/helpers/microphoneSelection.js");
  const result = resolveMicrophoneSelection(
    [mic("default", "Default - USB Microphone"), mic("usb", "USB Microphone")],
    { microphoneSelectionMode: "system" },
    { name: "USB Microphone" }
  );

  assert.equal(isCacheableMicrophoneResolution(result), true);
});

test("system mode never guesses the first physical microphone", async () => {
  const { resolveMicrophoneSelection } = await import("../../src/helpers/microphoneSelection.js");
  const result = resolveMicrophoneSelection(
    [mic("continuity", "Joshua P Microphone"), mic("usb", "USB Microphone")],
    { microphoneSelectionMode: "system" },
    { name: "Missing System Microphone" }
  );

  assert.equal(result.device, null);
  assert.equal(result.status, "native-unmatched");
});

test("legacy microphone preferences retain their behavior", async () => {
  const { getMicrophoneSelectionMode } = await import("../../src/helpers/microphoneSelection.js");

  assert.equal(getMicrophoneSelectionMode({ preferBuiltInMic: true }), "built-in");
  assert.equal(
    getMicrophoneSelectionMode({ preferBuiltInMic: false, selectedMicDeviceId: "usb" }),
    "specific"
  );
  assert.equal(getMicrophoneSelectionMode({ preferBuiltInMic: false }), "system");
});

test("system mode ignores Chromium's Windows 'communications' alias when matching the native default", async () => {
  const { isCacheableMicrophoneResolution, resolveMicrophoneSelection } =
    await import("../../src/helpers/microphoneSelection.js");
  const expected = mic("9f2c1e5a7b3d", "Microphone (Realtek(R) Audio)");
  const result = resolveMicrophoneSelection(
    [
      mic("default", "Default - Microphone (Realtek(R) Audio)"),
      mic("communications", "Communications - Microphone (Realtek(R) Audio)"),
      expected,
    ],
    { microphoneSelectionMode: "system" },
    { name: "Microphone (Realtek(R) Audio)" }
  );

  assert.equal(result.device, expected);
  assert.equal(result.status, "native-exact");
  assert.equal(isCacheableMicrophoneResolution(result), true);
});

test("system mode ignores the alias when the native name is only contained in the Chromium label", async () => {
  const { isCacheableMicrophoneResolution, resolveMicrophoneSelection } =
    await import("../../src/helpers/microphoneSelection.js");
  const expected = mic("9f2c1e5a7b3d", "Microphone (Realtek(R) Audio)");
  const result = resolveMicrophoneSelection(
    [
      mic("default", "Default - Microphone (Realtek(R) Audio)"),
      mic("communications", "Communications - Microphone (Realtek(R) Audio)"),
      expected,
    ],
    { microphoneSelectionMode: "system" },
    { name: "Realtek(R) Audio" }
  );

  assert.equal(result.device, expected);
  assert.equal(result.status, "native-compatible");
  assert.equal(isCacheableMicrophoneResolution(result), true);
});

test("built-in mode pins the physical built-in device, not Chromium's Windows aliases", async () => {
  const { resolveMicrophoneSelection } = await import("../../src/helpers/microphoneSelection.js");
  const expected = mic("9f2c1e5a7b3d", "Microphone Array (Realtek(R) Audio)");
  const result = resolveMicrophoneSelection(
    [
      mic("default", "Default - Microphone Array (Realtek(R) Audio)"),
      mic("communications", "Communications - Microphone Array (Realtek(R) Audio)"),
      expected,
      mic("7a1b", "Jabra Evolve2 65"),
    ],
    { microphoneSelectionMode: "built-in" }
  );

  assert.equal(result.device, expected);
  assert.equal(result.status, "built-in");
});
