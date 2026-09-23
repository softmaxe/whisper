const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");
const { EventEmitter } = require("node:events");
const path = require("node:path");
const childProcess = require("node:child_process");
const fs = require("node:fs");

const modulePath = require.resolve("../../src/helpers/mediaPlayer");
// mediaPlayer kills timed-out helpers through this shared utility, so it has to
// re-bind to each test's stub too.
const processUtilPath = require.resolve("../../src/utils/process");
const originalLoad = Module._load;
const originalPlatform = process.platform;

function setPlatform(platform) {
  Object.defineProperty(process, "platform", { value: platform, configurable: true });
}
test.afterEach(() => {
  setPlatform(originalPlatform);
  Module._load = originalLoad;
});

// A child whose pipes only close when the test says so, so a helper that never
// finishes can be observed. Records what the timeout branch does to it.
function createFakeChild() {
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  child.stdoutDestroyed = false;
  child.stderrDestroyed = false;
  child.stdout.destroy = () => {
    child.stdoutDestroyed = true;
  };
  child.stderr.destroy = () => {
    child.stderrDestroyed = true;
  };
  // killProcess skips a child that has already exited.
  child.exitCode = null;
  child.pid = 4242;
  child.killSignals = [];
  child.kill = (signal) => {
    child.killSignals.push(signal);
    return true;
  };
  // child_process delivers "error" and then "close" when the executable is
  // missing; spawnAsync must treat that like a failure rather than hang.
  child.fail = (err) => {
    child.emit("error", err);
    child.emit("close", -2);
  };
  child.finish = (status, stdout = "", stderr = "") => {
    if (stdout) child.stdout.emit("data", Buffer.from(stdout));
    if (stderr) child.stderr.emit("data", Buffer.from(stderr));
    child.emit("close", status);
  };
  // A helper that printed its result and exited, but whose pipes a descendant
  // still holds open. Node sets exitCode at "exit"; "close" is what's stuck,
  // so the deadline fires with the outcome already known.
  child.exitLeavingPipesOpen = (status, stdout = "") => {
    if (stdout) child.stdout.emit("data", Buffer.from(stdout));
    child.exitCode = status;
    child.emit("exit", status, null);
  };
  return child;
}

// Loads a fresh MediaPlayer singleton for `platform`. Every synchronous
// child_process entry point throws, so a regression back to a blocking call
// fails every test in this file (#2073). The stub stays installed for the whole
// test rather than just the initial require, so a call added inside a function
// is caught too, and it only answers requires made by the two modules under
// test, so node:test's own requires are untouched.
function loadMediaPlayer(platform, { existingPaths = () => false, warnThrows = false } = {}) {
  delete require.cache[modulePath];
  delete require.cache[processUtilPath];
  setPlatform(platform);
  const calls = [];
  const logs = [];
  const spawn = (cmd, args, options) => {
    const child = createFakeChild();
    calls.push({ cmd: path.basename(cmd), args, options, child });
    return child;
  };
  const syncSpawn = () => {
    throw new Error(
      "no synchronous child-process call may run on the media pause/resume path (#2073)"
    );
  };
  const mockedRequesters = new Set([modulePath, processUtilPath]);
  Module._load = function loadWithMocks(request, parent, isMain) {
    if (!mockedRequesters.has(parent?.filename)) {
      return originalLoad.call(this, request, parent, isMain);
    }
    // A "node:"-prefixed require reaches the same builtin, so it must not be
    // the way a blocking call slips past the stub.
    const builtin = request.startsWith("node:") ? request.slice(5) : request;
    if (request === "./debugLogger") {
      // The real logger requires electron at load time.
      return {
        debug: (message, meta) => logs.push({ level: "debug", message, meta }),
        info() {},
        warn: (message, meta) => {
          logs.push({ level: "warn", message, meta });
          // debugLogger.write ends in an unguarded logStream.write, so a
          // broken sink throws straight back into the caller.
          if (warnThrows) throw new Error("log stream is broken");
        },
        error() {},
      };
    }
    if (builtin === "child_process") {
      return {
        ...childProcess,
        spawn,
        spawnSync: syncSpawn,
        execSync: syncSpawn,
        execFileSync: syncSpawn,
      };
    }
    if (builtin === "fs") {
      // Binary resolution hits the real filesystem; pin it so the host's
      // downloaded binaries can't change which fallback runs. Only existsSync
      // is stubbed, which covers the mediaremote adapter; macos-media-remote
      // also calls accessSync and still sees the real filesystem.
      return { ...fs, existsSync: (p) => existingPaths(String(p)) };
    }
    return originalLoad.call(this, request, parent, isMain);
  };
  return { mediaPlayer: require(modulePath), calls, logs };
}

// Awaits until the module has spawned call #index. The chain between one
// child's close and the next spawn is microtask-only, so a single immediate
// suffices; the bound is just a guard against a hang.
async function waitForCall(calls, index) {
  for (let i = 0; i < 50 && calls.length <= index; i += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
  assert.ok(calls.length > index, `expected spawn call #${index + 1}, saw ${calls.length}`);
  return calls[index];
}

// _pauseMacOS only sets _didPause after two perl spawns, so a resume arriving
// in that window must queue behind the pause rather than return early and leave
// media paused until the next dictation ended (#2073).
const MAC_ADAPTER_PATHS = (p) =>
  p === "/usr/bin/perl" ||
  p.endsWith("mediaremote-adapter.pl") ||
  p.endsWith("MediaRemoteAdapter.framework");

test("darwin: resume waits for an in-flight pause instead of no-oping on _didPause", async () => {
  const { mediaPlayer, calls } = loadMediaPlayer("darwin", { existingPaths: MAC_ADAPTER_PATHS });

  const pausing = mediaPlayer.pauseMedia();
  const resuming = mediaPlayer.resumeMedia();

  const probe = await waitForCall(calls, 0);
  assert.equal(probe.cmd, "perl");
  assert.deepEqual(probe.args.slice(-2), ["get", "--no-artwork"]);
  probe.child.finish(0, '{"playing":true}');

  const pause = await waitForCall(calls, 1);
  assert.deepEqual(pause.args.slice(-2), ["send", "1"]); // kMRAPause
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(calls.length, 2, "resume must not spawn until the pause has settled");
  pause.child.finish(0);

  assert.equal(await pausing, true);

  const play = await waitForCall(calls, 2);
  assert.deepEqual(play.args.slice(-2), ["send", "0"]); // kMRAPlay
  play.child.finish(0);

  assert.equal(await resuming, true);
  assert.equal(mediaPlayer._didPause, false);
});
