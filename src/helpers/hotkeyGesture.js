const HOLD_THRESHOLD_MS = 150;
const TAP_MAX_HOLD_MS = 300;
const DOUBLE_TAP_GAP_MS = 400;
// Presses this soon after a Double tap are a third tap, not a stop.
const START_SETTLE_MS = 300;

// Decides what each press of a Dictation hotkey that reports its own release
// does: Hold mode holds to talk, and Tap mode starts Hands-free dictation on a
// Double tap and ends it on the next Clean press.
function createHotkeyGesture({
  getActivationMode,
  isDictationActive,
  isDictationProcessing,
  start,
  stop,
  cancel,
  now = Date.now,
  setTimeout = global.setTimeout,
  clearTimeout = global.clearTimeout,
}) {
  // The press currently held down and what its release will do:
  //   tap     - may become the first half of a Double tap
  //   start   - the second half of a Double tap; Dictation already started
  //   pending - a Hold mode press not yet held long enough
  //   held    - a Hold mode press that started Dictation
  //   stop    - ends the active Dictation
  //   ignored - does nothing
  let heldPress = null;
  // The last Clean tap that could open a Double tap; any later press uses it up.
  let lastTap = null;
  let doubleTapAt = -Infinity;

  const clearHeldPress = () => {
    if (heldPress?.timer) clearTimeout(heldPress.timer);
    const current = heldPress;
    heldPress = null;
    return current;
  };

  const gesture = {
    press(key) {
      if (heldPress && heldPress.key !== key) {
        // Another Dictation hotkey pressed during a hold is just another key.
        gesture.interrupt();
        return;
      }
      // Pressed again without a release: the release was lost, so start over.
      clearHeldPress();

      const at = now();
      const previousTap = lastTap;
      lastTap = null;

      if (isDictationProcessing() || at - doubleTapAt < START_SETTLE_MS) {
        heldPress = { key, kind: "ignored", at };
      } else if (isDictationActive()) {
        heldPress = { key, kind: "stop", at };
      } else if (getActivationMode() === "push") {
        const pending = { key, kind: "pending", at };
        pending.timer = setTimeout(() => {
          if (heldPress !== pending) return;
          pending.kind = "held";
          pending.timer = null;
          start();
        }, HOLD_THRESHOLD_MS);
        heldPress = pending;
      } else if (previousTap?.key === key && at - previousTap.at <= DOUBLE_TAP_GAP_MS) {
        doubleTapAt = at;
        heldPress = { key, kind: "start", at };
        start();
      } else {
        heldPress = { key, kind: "tap", at };
      }
    },

    release(key) {
      if (heldPress?.key !== key) return;
      const current = clearHeldPress();
      const releasedAt = now();
      if (current.kind === "stop" || current.kind === "held") {
        stop();
      } else if (current.kind === "tap" && releasedAt - current.at <= TAP_MAX_HOLD_MS) {
        lastTap = { key, at: releasedAt };
      }
    },

    // Another key or mouse button went down while the hotkey was held: the
    // press was part of a shortcut, so it is void.
    interrupt() {
      const current = clearHeldPress();
      if (!current) return;
      heldPress = { ...current, kind: "ignored", timer: null };
      if (current.kind === "start" || current.kind === "held") cancel();
    },

    reset() {
      clearHeldPress();
      lastTap = null;
      doubleTapAt = -Infinity;
    },
  };
  return gesture;
}

module.exports = { createHotkeyGesture };
