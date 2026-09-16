const test = require("node:test");
const assert = require("node:assert/strict");
const { loadAudioManager } = require("./harness/audioManager");
const { installMicCaptureGlobals } = require("../lib/rendererTestHarness");

const builtIn = { kind: "audioinput", deviceId: "macbook", label: "MacBook Pro Microphone" };
const phone = { kind: "audioinput", deviceId: "iphone", label: "Rui's iPhone Microphone" };
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function streamFor(deviceId) {
  const track = Object.assign(new EventTarget(), {
    readyState: "live",
    muted: false,
    getSettings: () => ({ deviceId }),
    stop() {
      this.readyState = "ended";
    },
  });
  return { getTracks: () => [track], getAudioTracks: () => [track] };
}

test("auto refreshes cached input and switches a live recording on lid and device changes", async (t) => {
  let manager;
  t.after(() => manager?.cleanup());
  const { window, AudioManager, setSettings } = await loadAudioManager(t, {
    cachePrefix: "whisper-auto-mic-",
    settingsKey: "__autoMicSettings",
    settings: { microphoneSelectionMode: "auto" },
    mockModules: {
      "/stores/settingsStore": `
        export const getSettings = () => globalThis.__autoMicSettings;
        export const useSettingsStore = { subscribe: () => () => {} };
        export const getEffectiveCleanupModel = () => null;
        export const selectResolvedLLMConfig = () => ({ model: null, provider: null });
        export const isCloudCleanupMode = () => false;
        export const isCloudDictationAgentMode = () => false;
        export const isCloudTranslationMode = () => false;
      `,
    },
  });
  let lidClosed = false;
  let lidListener;
  let unsubscribed = false;
  let inputs = [builtIn, phone];
  const acquisitions = [];
  const mediaDevices = Object.assign(new EventTarget(), {
    enumerateDevices: async () => inputs,
    getUserMedia: async (constraints) => {
      const deviceId = constraints.audio.deviceId.exact;
      acquisitions.push(deviceId);
      return streamFor(deviceId);
    },
  });
  installMicCaptureGlobals(t);
  navigator.mediaDevices = mediaDevices;
  window.electronAPI.getLaptopLidState = async () => lidClosed;
  window.electronAPI.getSystemDefaultMicrophone = async () => ({ name: builtIn.label });
  window.electronAPI.onLaptopLidStateChanged = (callback) => {
    lidListener = callback;
    return () => {
      unsubscribed = true;
    };
  };
  manager = new AudioManager();
  manager.micRecovery.debounceMs = 1;
  manager.replaceActiveMic = async (_replacement, previous) => {
    previous.getTracks().forEach((track) => track.stop());
  };

  manager.cachedMicDeviceId = phone.deviceId;
  assert.equal((await manager.getAudioConstraints()).audio.deviceId.exact, builtIn.deviceId);
  lidClosed = true;
  // A missed notification must not pin the next recording to its old cache.
  assert.equal((await manager.getAudioConstraints()).audio.deviceId.exact, phone.deviceId);
  lidClosed = false;
  manager.isRecording = true;
  await manager.beginMicRecovery(streamFor(builtIn.deviceId));

  lidClosed = true;
  lidListener(lidClosed);
  await delay(30);
  assert.equal(manager.micRecovery.track.getSettings().deviceId, phone.deviceId);

  lidClosed = false;
  lidListener(lidClosed);
  await delay(30);
  assert.equal(manager.micRecovery.track.getSettings().deviceId, builtIn.deviceId);

  inputs = [builtIn];
  lidClosed = true;
  lidListener(lidClosed);
  await delay(30);
  assert.equal(manager.micRecovery.track.getSettings().deviceId, builtIn.deviceId);
  inputs = [builtIn, phone];
  mediaDevices.dispatchEvent(new Event("devicechange"));
  await delay(30);
  assert.equal(manager.micRecovery.track.getSettings().deviceId, phone.deviceId);

  inputs = [builtIn];
  mediaDevices.dispatchEvent(new Event("devicechange"));
  await delay(30);
  assert.equal(manager.micRecovery.track.getSettings().deviceId, builtIn.deviceId);
  assert.deepEqual(acquisitions, ["iphone", "macbook", "iphone", "macbook"]);

  setSettings({ microphoneSelectionMode: "specific", selectedMicDeviceId: "macbook" });
  manager.cachedMicDeviceId = "macbook";
  inputs = [builtIn, phone];
  lidListener(true);
  await delay(10);
  assert.equal(manager.cachedMicDeviceId, "macbook");
  assert.equal(acquisitions.length, 4);
  manager.isRecording = false;
  manager.cleanup();
  assert.equal(unsubscribed, true);
  manager = null;
});
