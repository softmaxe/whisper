const test = require("node:test");
const assert = require("node:assert/strict");

const TextEditMonitor = require("../../src/helpers/textEditMonitor");

test("activateTargetPid resolves false when no target PID was captured", async () => {
  const m = new TextEditMonitor();
  m.lastTargetPid = null;
  assert.equal(await m.activateTargetPid(), false);
});

test("activateTargetPid resolves false for an unmapped PID", async () => {
  const m = new TextEditMonitor();
  // Non-darwin short-circuits; darwin finds no app to activate for a
  // non-existent PID. Both resolve quickly to false rather than reporting a
  // target that never held keyboard focus as active.
  m.lastTargetPid = 99999999;
  const start = Date.now();
  const result = await m.activateTargetPid();
  assert.equal(result, false);
  assert.ok(Date.now() - start < 3000);
});

const darwinOnly = { skip: process.platform !== "darwin" };

test(
  "canPasteAtTarget distinguishes unavailable probes from an explicit no-target verdict",
  darwinOnly,
  async () => {
    for (const [output, expected] of [
      ["PASTEABLE", true],
      ["NOT_PASTEABLE", false],
      ["UNKNOWN", null],
      ["NO_ELEMENT", null],
      ["INITIAL_VALUE:stale", null],
    ]) {
      const monitor = new TextEditMonitor();
      monitor.resolveBinary = () => ({ command: "/bin/sh", args: ["-c", `echo '${output}'`] });
      assert.equal(await monitor.canPasteAtTarget(42), expected, output);
    }

    const monitor = new TextEditMonitor();
    monitor.resolveBinary = () => null;
    assert.equal(await monitor.canPasteAtTarget(null), null);
    assert.equal(await monitor.canPasteAtTarget(42), null);
    monitor.resolveBinary = () => ({ command: "/nonexistent/paste-probe", args: [] });
    assert.equal(await monitor.canPasteAtTarget(42), null);
    monitor.resolveBinary = () => ({ command: "/bin/sh", args: ["-c", "exec sleep 2"] });
    assert.equal(await monitor.canPasteAtTarget(42, 25), null);
  }
);

test(
  "canPasteAtTarget probes the captured PID and accepts only its first verdict",
  darwinOnly,
  async () => {
    const monitor = new TextEditMonitor();
    monitor.lastTargetPid = 99;
    monitor.resolveBinary = () => ({
      command: "/bin/sh",
      args: [
        "-c",
        '[ "$0" = "--paste-target" ] && [ "$1" = "42" ] && printf "NOT_PASTEABLE\\nPASTEABLE\\n"',
      ],
    });
    assert.equal(await monitor.canPasteAtTarget(42), false);
  }
);

test("startMonitoring stops immediately without a target PID", darwinOnly, () => {
  const m = new TextEditMonitor();
  m.startMonitoring("pasted text", 5000, { targetPid: null });
  assert.equal(m.currentOriginalText, null);
  assert.equal(m.process, null);
});

test(
  "startMonitoring self-terminates when the target has no accessible focused element",
  darwinOnly,
  async () => {
    const m = new TextEditMonitor();
    // Auto-learn must degrade by giving up (NO_ELEMENT → stopMonitoring), never by
    // writing AX attributes like AXEnhancedUserInterface onto the target app —
    // that flag blurs the focused editor in some Chromium apps (see module comment).
    m.startMonitoring("pasted text", 4000, { targetPid: 99999999 });
    const deadline = Date.now() + 6000;
    while (m.currentOriginalText !== null && Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, 200));
    }
    assert.equal(m.currentOriginalText, null);
    assert.equal(m.process, null);
    assert.equal(m._pollInterval, null);
  }
);

// AXError -25212 (kAXErrorNoValue) marks apps whose AX tree never yields a
// focused element to us (Chromium/Electron); the native binary's retry loop is
// deterministic dead time there, so one failure skips the native monitor for
// the rest of the session.
test(
  "a monitor run ending in NO_ELEMENT with uniform -25212 teaches the cache",
  darwinOnly,
  async () => {
    const m = new TextEditMonitor();
    m.resolveBinary = () => ({
      command: "/bin/sh",
      args: [
        "-c",
        'echo "Attempt 5/5: Cannot get focused element for PID 4242 (error: -25212)" >&2; echo "NO_ELEMENT"',
      ],
    });

    m.startMonitoring("pasted text", 4000, { targetPid: 4242 });
    // The verdict lands on the child's "close" event, after monitoring stops.
    const deadline = Date.now() + 6000;
    while (!m._nativeSelectionUnsupportedPids.has(4242) && Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, 100));
    }

    assert.equal(m.currentOriginalText, null);
    assert.ok(m._nativeSelectionUnsupportedPids.has(4242));
  }
);

test("startMonitoring skips the native monitor for a cached PID", darwinOnly, () => {
  const m = new TextEditMonitor();
  m.resolveBinary = () => ({ command: "/bin/echo", args: [] });
  m._nativeSelectionUnsupportedPids.add(42);
  m.startMonitoring("pasted text", 4000, { targetPid: 42 });
  // The cache check runs before the initial settle delay, so monitoring ends
  // synchronously — no doomed child process is ever spawned.
  assert.equal(m.currentOriginalText, null);
  assert.equal(m.process, null);
});
