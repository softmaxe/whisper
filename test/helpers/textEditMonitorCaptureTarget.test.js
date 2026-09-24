const test = require("node:test");
const assert = require("node:assert/strict");

const TextEditMonitor = require("../../src/helpers/textEditMonitor");

const darwinOnly = { skip: process.platform !== "darwin" };

function stubFrontmostFallback(monitor, pid) {
  // Without the native helper, the frontmost app stands in for keyboard focus.
  monitor.resolveBinary = () => null;
  let invocations = 0;
  let release;
  const gate = new Promise((resolve) => {
    release = () => resolve(pid);
  });
  monitor._readFrontmostPid = () => {
    invocations += 1;
    return gate;
  };
  return { release, count: () => invocations };
}

test("concurrent captures share one target lookup", darwinOnly, async () => {
  const m = new TextEditMonitor();
  const lookup = stubFrontmostFallback(m, 4242);

  const first = m.captureTargetPid();
  const second = m.captureTargetPid();
  lookup.release();

  assert.deepEqual(await Promise.all([first, second]), [4242, 4242]);
  assert.equal(lookup.count(), 1);
  assert.equal(m.lastTargetPid, 4242);
});

test("a just-completed capture is reused instead of respawning osascript", darwinOnly, async () => {
  const m = new TextEditMonitor();
  const lookup = stubFrontmostFallback(m, 4242);

  const first = m.captureTargetPid();
  lookup.release();
  await first;

  assert.equal(await m.captureTargetPid(), 4242);
  assert.equal(lookup.count(), 1);
});

test("a failed capture is retried, not reused", darwinOnly, async () => {
  const m = new TextEditMonitor();
  let invocations = 0;
  m.resolveBinary = () => null;
  m._readFrontmostPid = () => {
    invocations += 1;
    return Promise.resolve(invocations === 1 ? null : 4242);
  };

  assert.equal(await m.captureTargetPid(), null);
  assert.equal(await m.captureTargetPid(), 4242);
  assert.equal(invocations, 2);
});

test("captures refresh once the reuse window has passed", darwinOnly, async () => {
  const m = new TextEditMonitor();
  let invocations = 0;
  m.resolveBinary = () => null;
  m._readFrontmostPid = () => {
    invocations += 1;
    return Promise.resolve(invocations === 1 ? 1111 : 2222);
  };

  assert.equal(await m.captureTargetPid(), 1111);
  m._lastCaptureAt = Date.now() - 10_000;
  assert.equal(await m.captureTargetPid(), 2222);
  assert.equal(m.lastTargetPid, 2222);
});

// A floating launcher such as Raycast holds keyboard focus while the app
// behind it owns the menu bar. The native helper names the focus owner.
function focusHelper(output) {
  return () => ({
    command: "/bin/sh",
    args: ["-c", `[ "$0" = "--focused-app" ] && echo '${output}'`],
  });
}

test("captures the app holding keyboard focus, not the menu bar app", darwinOnly, async () => {
  const m = new TextEditMonitor();
  m.resolveBinary = focusHelper("FOCUSED_PID:4242");
  m._readFrontmostPid = () => Promise.resolve(1111);

  assert.equal(await m.captureTargetPid(), 4242);
  assert.equal(m.lastTargetPid, 4242);
});

test("capture falls back to the frontmost app without a focus verdict", darwinOnly, async () => {
  for (const resolveBinary of [focusHelper("UNKNOWN"), focusHelper("NO_ELEMENT"), () => null]) {
    const m = new TextEditMonitor();
    m.resolveBinary = resolveBinary;
    m._readFrontmostPid = () => Promise.resolve(1111);
    assert.equal(await m.captureTargetPid(), 1111);
  }
});

test("a target already holding keyboard focus is not re-activated", darwinOnly, async () => {
  const m = new TextEditMonitor();
  m.resolveBinary = focusHelper("FOCUSED_PID:4242");
  m._readFrontmostPid = () => Promise.resolve(1111);
  let activations = 0;
  m._activateApp = () => {
    activations += 1;
    return Promise.resolve();
  };

  assert.equal(await m.activatePid(4242), true);
  assert.equal(activations, 0);
});

test("a target without keyboard focus is activated until it holds focus", darwinOnly, async () => {
  const m = new TextEditMonitor();
  let focusOwner = 1111;
  m._readKeyboardFocusPid = () => Promise.resolve(focusOwner);
  m._activateApp = () => {
    focusOwner = 4242;
    return Promise.resolve(true);
  };

  assert.equal(await m.activatePid(4242), true);
});

// A launcher closed during Dictation keeps the transcript in the clipboard:
// activating its app could reopen the launcher and paste into it.
test("a target that cannot be activated reports no focus without polling", darwinOnly, async () => {
  const m = new TextEditMonitor();
  let lookups = 0;
  m._readKeyboardFocusPid = () => {
    lookups += 1;
    return Promise.resolve(1111);
  };
  m._activateApp = () => Promise.resolve(false);

  assert.equal(await m.activatePid(4242), false);
  assert.equal(lookups, 1);
});
