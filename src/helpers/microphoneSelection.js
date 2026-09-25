import { isBuiltInMicrophone } from "../utils/audioDeviceUtils";
import { resolveMicDeviceSelection } from "./micDeviceSelection";

export const MICROPHONE_SELECTION_MODES = ["auto", "system", "built-in", "specific"];

// Chromium lists the system default input again as "default" under the
// device's own label. It is an alias, not a candidate: matching it makes the
// default mic tie with itself.
const CHROMIUM_ALIAS_DEVICE_IDS = new Set(["default"]);

export function getMicrophoneSelectionMode(settings = {}) {
  if (MICROPHONE_SELECTION_MODES.includes(settings.microphoneSelectionMode)) {
    return settings.microphoneSelectionMode;
  }
  if (settings.preferBuiltInMic) return "built-in";
  return settings.selectedMicDeviceId && settings.selectedMicDeviceId !== "default"
    ? "specific"
    : "system";
}

export function normalizeMicrophoneLabel(label = "") {
  return String(label)
    .normalize("NFKC")
    .replace(/^\s*default\s*-\s*/i, "")
    .replace(/\s*\((?:built[ -]?in|default)\)\s*$/i, "")
    .replace(/\s+/g, " ")
    .trim()
    .toLocaleLowerCase();
}

function comparableLabel(label) {
  return normalizeMicrophoneLabel(label).replace(/[^\p{L}\p{N}]+/gu, "");
}

export function resolveSystemDefaultMicDevice(devices, systemDefault) {
  const inputs = devices.filter((device) => device.kind === "audioinput");
  const chromiumDefault = inputs.find((device) => device.deviceId === "default") || null;
  const physicalInputs = inputs.filter((device) => !CHROMIUM_ALIAS_DEVICE_IDS.has(device.deviceId));
  const nativeName = systemDefault?.name?.trim();

  if (nativeName) {
    const target = comparableLabel(nativeName);
    const exactMatches = physicalInputs.filter(
      (device) => target && comparableLabel(device.label) === target
    );
    if (exactMatches.length === 1) {
      return { device: exactMatches[0], status: "native-exact", systemDefault };
    }

    const containedMatches = physicalInputs.filter((device) => {
      const candidate = comparableLabel(device.label);
      return (
        target.length >= 8 &&
        candidate.length >= 8 &&
        (candidate.includes(target) || target.includes(candidate))
      );
    });
    if (containedMatches.length === 1) {
      return { device: containedMatches[0], status: "native-compatible", systemDefault };
    }
  }

  if (chromiumDefault) {
    const matches = physicalInputs.filter((device) =>
      chromiumDefault.groupId
        ? device.groupId === chromiumDefault.groupId
        : normalizeMicrophoneLabel(device.label) &&
          normalizeMicrophoneLabel(device.label) === normalizeMicrophoneLabel(chromiumDefault.label)
    );
    if (matches.length === 1) {
      return { device: matches[0], status: "chromium-physical", systemDefault };
    }
  }

  if (chromiumDefault) {
    return {
      device: chromiumDefault,
      status: nativeName ? "chromium-default-unmatched" : "chromium-default",
      systemDefault,
    };
  }

  return {
    device: null,
    status: nativeName ? "native-unmatched" : "unavailable",
    systemDefault,
  };
}

export function resolveMicrophoneSelection(
  devices,
  settings,
  systemDefault = null,
  lidClosed = null
) {
  const mode = getMicrophoneSelectionMode(settings);
  const inputs = devices.filter((device) => device.kind === "audioinput");

  if (mode === "auto") {
    const physicalInputs = inputs.filter(
      (device) => device.deviceId && !CHROMIUM_ALIAS_DEVICE_IDS.has(device.deviceId)
    );
    // macOS names its internal input explicitly. The generic "Microphone"
    // heuristic also matches custom-named Continuity and USB devices.
    const builtInInputs = physicalInputs.filter((device) =>
      /built[ -]?in|internal|macbook|integrated/i.test(device.label)
    );
    const builtIn = builtInInputs[0];
    const externalInputs = physicalInputs.filter(
      (device) => device.label && !builtInInputs.includes(device)
    );
    const phone = externalInputs.find((device) => /iphone|continuity/i.test(device.label));
    const device = lidClosed === true ? phone || externalInputs[0] : builtIn;
    if (device) {
      return {
        mode,
        device,
        status: lidClosed === true ? "auto-external" : "auto-built-in",
        lidClosed,
      };
    }
    return {
      mode,
      ...resolveSystemDefaultMicDevice(inputs, systemDefault),
      lidClosed,
    };
  }

  if (mode === "system") {
    return { mode, ...resolveSystemDefaultMicDevice(inputs, systemDefault) };
  }

  if (mode === "built-in") {
    const device = inputs.find(
      (candidate) =>
        !CHROMIUM_ALIAS_DEVICE_IDS.has(candidate.deviceId) && isBuiltInMicrophone(candidate.label)
    );
    return { mode, device: device || null, status: device ? "built-in" : "unavailable" };
  }

  const selection = resolveMicDeviceSelection(
    inputs,
    settings.selectedMicDeviceId,
    settings.selectedMicDeviceLabel
  );
  return { mode, ...selection };
}

export function isCacheableMicrophoneResolution(resolution) {
  const deviceId = resolution?.device?.deviceId;
  return Boolean(deviceId && deviceId !== "default");
}

export async function resolvePreferredMicrophone({
  settings,
  mediaDevices = navigator.mediaDevices,
  forceSystemDefault = false,
  refreshSystemDefault = false,
  getSystemDefault = (options) => window.electronAPI?.getSystemDefaultMicrophone?.(options),
  getLidState = () => window.electronAPI?.getLaptopLidState?.(),
}) {
  const effectiveSettings = forceSystemDefault
    ? { ...settings, microphoneSelectionMode: "system", preferBuiltInMic: false }
    : settings;
  const mode = getMicrophoneSelectionMode(effectiveSettings);
  const devices = await mediaDevices.enumerateDevices();
  let systemDefault = null;
  let lidClosed = null;

  if (mode === "auto") {
    try {
      const state = await getLidState?.();
      lidClosed = typeof state === "boolean" ? state : null;
    } catch {
      // If lid detection is unavailable, prefer the built-in input.
    }
  }

  if (mode === "system" || mode === "auto") {
    try {
      systemDefault = (await getSystemDefault?.({ refresh: refreshSystemDefault })) || null;
    } catch {
      // The explicit Chromium "default" device below remains the safe fallback.
    }
  }

  const result = resolveMicrophoneSelection(devices, effectiveSettings, systemDefault, lidClosed);
  if (
    result.mode === "specific" &&
    result.device &&
    (result.status === "remapped" || !settings.selectedMicDeviceLabel)
  ) {
    settings.setSelectedMicDevice?.(result.device.deviceId, result.device.label);
  }
  return result;
}
