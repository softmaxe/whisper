const assert = require("node:assert/strict");
const test = require("node:test");
const { createHotkeyGesture } = require("../../src/helpers/hotkeyGesture");

// Drives the gesture with a controlled clock and records the Dictation commands
// it issues. Dictation state is whatever the test says it is, since the real
// state is reported asynchronously by the renderer.
function setup({ mode = "tap" } = {}) {
  let time = 1000;
  let timers = [];
  const commands = [];
  const state = { mode, active: false, processing: false };

  const gesture = createHotkeyGesture({
    getActivationMode: () => state.mode,
    isDictationActive: () => state.active,
    isDictationProcessing: () => state.processing,
    start: () => {
      commands.push("start");
      state.active = true;
    },
    stop: () => {
      commands.push("stop");
      state.active = false;
    },
    cancel: () => {
      commands.push("cancel");
      state.active = false;
    },
    now: () => time,
    setTimeout: (callback, delay) => {
      const timer = { callback, at: time + delay };
      timers.push(timer);
      return timer;
    },
    clearTimeout: (timer) => {
      timers = timers.filter((candidate) => candidate !== timer);
    },
  });

  const advance = (ms) => {
    const target = time + ms;
    for (;;) {
      const due = timers.filter((timer) => timer.at <= target).sort((a, b) => a.at - b.at)[0];
      if (!due) break;
      timers = timers.filter((timer) => timer !== due);
      time = due.at;
      due.callback();
    }
    time = target;
  };

  const tap = (key = "RightCommand", heldMs = 80) => {
    gesture.press(key);
    advance(heldMs);
    gesture.release(key);
  };

  return { gesture, commands, state, advance, tap };
}

test("Tap mode: a single tap does not start Dictation", () => {
  const { commands, advance, tap } = setup();
  tap();
  advance(2000);
  assert.deepEqual(commands, []);
});

test("Tap mode: a Double tap starts Dictation on the second press, before its release", () => {
  const { gesture, commands, advance, tap } = setup();
  tap();
  advance(200);
  gesture.press("RightCommand");
  assert.deepEqual(commands, ["start"]);
});

test("Tap mode: a first press held longer than a tap does not count toward a Double tap", () => {
  const { gesture, commands, advance, tap } = setup();
  tap("RightCommand", 350);
  advance(100);
  gesture.press("RightCommand");
  assert.deepEqual(commands, []);
});

test("Tap mode: taps too far apart are two single taps", () => {
  const { gesture, commands, advance, tap } = setup();
  tap();
  advance(450);
  gesture.press("RightCommand");
  assert.deepEqual(commands, []);
});

function startHandsFree(harness) {
  harness.tap();
  harness.advance(150);
  harness.tap();
  harness.advance(1000);
  harness.commands.length = 0;
}

test("Tap mode: Hands-free dictation outlives the Double tap and ends when the next press is released", () => {
  const harness = setup();
  startHandsFree(harness);
  assert.deepEqual(harness.commands, []);

  harness.gesture.press("RightCommand");
  harness.advance(2000);
  assert.deepEqual(harness.commands, [], "a stop press of any length waits for its release");
  harness.gesture.release("RightCommand");
  assert.deepEqual(harness.commands, ["stop"]);
});

test("Tap mode: a third tap right after the Double tap does not end Dictation", () => {
  const { commands, advance, tap } = setup();
  tap();
  advance(150);
  tap();
  advance(100);
  tap();
  advance(1000);
  assert.deepEqual(commands, ["start"]);
});

test("Tap mode: a press used as a modifier, such as Command+C, does not count as a tap", () => {
  const { gesture, commands, advance, tap } = setup();
  gesture.press("RightCommand");
  advance(60);
  gesture.interrupt();
  advance(40);
  gesture.release("RightCommand");
  advance(150);
  tap();
  advance(1000);
  assert.deepEqual(commands, []);
});

test("Tap mode: an interrupted second press cancels the Dictation it started", () => {
  const { gesture, commands, advance, tap } = setup();
  tap();
  advance(150);
  gesture.press("RightCommand");
  advance(60);
  gesture.interrupt();
  gesture.release("RightCommand");
  assert.deepEqual(commands, ["start", "cancel"]);
});

test("Tap mode: Command+V during Hands-free dictation does not end it", () => {
  const harness = setup();
  startHandsFree(harness);
  harness.gesture.press("RightCommand");
  harness.advance(60);
  harness.gesture.interrupt();
  harness.gesture.release("RightCommand");
  assert.deepEqual(harness.commands, []);

  harness.advance(500);
  harness.tap();
  assert.deepEqual(harness.commands, ["stop"]);
});

test("Hold mode: Dictation starts once the hotkey is held long enough and ends on release", () => {
  const { gesture, commands, advance } = setup({ mode: "push" });
  gesture.press("GLOBE");
  advance(149);
  assert.deepEqual(commands, [], "nothing opens before the hold is recognized");
  advance(1);
  assert.deepEqual(commands, ["start"]);
  advance(3000);
  gesture.release("GLOBE");
  assert.deepEqual(commands, ["start", "stop"]);
});

test("Hold mode: a short press or a shortcut issues no Dictation command", () => {
  const { gesture, commands, advance, tap } = setup({ mode: "push" });
  tap("RightCommand", 100);
  advance(500);
  gesture.press("RightCommand");
  advance(100);
  gesture.interrupt();
  advance(400);
  gesture.release("RightCommand");
  assert.deepEqual(commands, []);
});

test("Hold mode: a shortcut pressed during a hold cancels Dictation instead of transcribing it", () => {
  const { gesture, commands, advance } = setup({ mode: "push" });
  gesture.press("RightCommand");
  advance(800);
  gesture.interrupt();
  gesture.release("RightCommand");
  assert.deepEqual(commands, ["start", "cancel"]);
});

test("presses are ignored while the previous Dictation is still being transcribed", () => {
  for (const mode of ["tap", "push"]) {
    const { gesture, commands, state, advance, tap } = setup({ mode });
    state.processing = true;
    tap();
    advance(150);
    gesture.press("RightCommand");
    advance(1000);
    state.processing = false;
    gesture.release("RightCommand");
    assert.deepEqual(commands, [], mode);
  }
});

test("pressing another Dictation hotkey during a hold voids the hold", () => {
  const { gesture, commands, advance } = setup({ mode: "push" });
  gesture.press("GLOBE");
  advance(500);
  gesture.press("RightCommand");
  advance(500);
  gesture.release("RightCommand");
  gesture.release("GLOBE");
  assert.deepEqual(commands, ["start", "cancel"]);
});

test("reset forgets a half-finished gesture", () => {
  const tapHarness = setup();
  tapHarness.tap();
  tapHarness.gesture.reset();
  tapHarness.advance(100);
  tapHarness.tap();
  assert.deepEqual(tapHarness.commands, []);

  const holdHarness = setup({ mode: "push" });
  holdHarness.gesture.press("GLOBE");
  holdHarness.gesture.reset();
  holdHarness.advance(1000);
  holdHarness.gesture.release("GLOBE");
  assert.deepEqual(holdHarness.commands, []);
});

test("Tap mode: only two taps of the same hotkey in a row make a Double tap", () => {
  const differentKeys = setup();
  differentKeys.tap("GLOBE");
  differentKeys.advance(150);
  differentKeys.gesture.press("RightCommand");
  assert.deepEqual(differentKeys.commands, [], "taps on different hotkeys do not combine");

  const voidedBetween = setup();
  voidedBetween.tap();
  voidedBetween.advance(100);
  voidedBetween.gesture.press("GLOBE");
  voidedBetween.gesture.interrupt();
  voidedBetween.gesture.release("GLOBE");
  voidedBetween.advance(100);
  voidedBetween.gesture.press("RightCommand");
  assert.deepEqual(voidedBetween.commands, [], "any press in between breaks the sequence");
});

test("a press whose release was never reported does not block the next press", () => {
  const { gesture, commands, advance } = setup({ mode: "push" });
  gesture.press("MouseButton4");
  advance(500);
  gesture.interrupt();
  advance(5000);
  gesture.press("MouseButton4");
  advance(200);
  gesture.release("MouseButton4");
  assert.deepEqual(commands, ["start", "cancel", "start", "stop"]);
});

test("Hold mode: the next hold right after a short one starts normally", () => {
  const { gesture, commands, advance } = setup({ mode: "push" });
  gesture.press("GLOBE");
  advance(200);
  gesture.release("GLOBE");
  advance(100);
  gesture.press("GLOBE");
  advance(200);
  gesture.release("GLOBE");
  assert.deepEqual(commands, ["start", "stop", "start", "stop"]);
});
