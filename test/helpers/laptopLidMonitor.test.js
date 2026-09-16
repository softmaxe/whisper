const test = require("node:test");
const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");
const { createLaptopLidMonitor, parseLidState } = require("../../src/helpers/laptopLidMonitor");

function setup(options = {}) {
  let clock = 100;
  const timers = new Map();
  const children = [];
  const broadcasts = [];
  const errors = [];
  const monitor = createLaptopLidMonitor({
    platform: "darwin",
    resolveBinary: () => "/helper/macos-mic-listener",
    spawnHelper: (file, args, spawnOptions) => {
      assert.equal(file, "/helper/macos-mic-listener");
      assert.deepEqual(args, ["--watch-lid-state"]);
      assert.deepEqual(spawnOptions.stdio, ["ignore", "pipe", "ignore"]);
      const child = new EventEmitter();
      child.stdout = new EventEmitter();
      child.stdout.setEncoding = () => {};
      child.exitCode = null;
      child.kills = 0;
      child.kill = () => child.kills++;
      children.push(child);
      return child;
    },
    broadcast: (value) => broadcasts.push(value),
    logError: (error) => errors.push(error.message),
    now: () => clock,
    schedule: (callback, delay) => {
      const handle = { unref() {} };
      timers.set(handle, { callback, at: clock + delay });
      return handle;
    },
    cancel: (timer) => timers.delete(timer),
    ...options,
  });

  function advance(ms, runTimers = true) {
    clock += ms;
    if (!runTimers) return;
    for (const [handle, timer] of [...timers]) {
      if (timer.at <= clock && timers.delete(handle)) timer.callback();
    }
  }

  return { monitor, children, broadcasts, errors, timers, advance };
}

test("only accepts explicit boolean or unknown lid states", () => {
  assert.equal(parseLidState('{"lidClosed":true}'), true);
  assert.equal(parseLidState('{"lidClosed":false}'), false);
  assert.equal(parseLidState('{"lidClosed":null}'), null);
  for (const value of ['{"lidClosed":0}', '{"lidClosed":"true"}', "{}", "null", "bad"]) {
    assert.equal(parseLidState(value), undefined);
  }
});

test("non-macOS hosts return unknown without starting a helper", async () => {
  const { monitor, children, timers } = setup({ platform: "linux" });
  monitor.start();
  assert.equal(await monitor.getState(), null);
  assert.equal(children.length, 0);
  assert.equal(timers.size, 0);
});

test("concurrent lookups share a helper and parse partial and combined lines", async () => {
  const { monitor, children, broadcasts } = setup();
  const first = monitor.getState();
  const second = monitor.getState();
  assert.equal(children.length, 1);
  children[0].stdout.emit("data", '{"lidClo');
  children[0].stdout.emit("data", 'sed":false}\n{"lidClosed":false}\n');
  assert.deepEqual(await Promise.all([first, second]), [false, false]);
  assert.deepEqual(broadcasts, [false]);
  children[0].stdout.emit("data", '{"lidClosed":true}\n{"lidClosed":null}\n');
  assert.deepEqual(broadcasts, [false, true, null]);
  assert.equal(await monitor.getState(), null);
  monitor.stop();
});

test("initial lookup has a bounded wait when the helper produces no valid sample", async () => {
  const { monitor, children, advance } = setup();
  const result = monitor.getState();
  children[0].stdout.emit("data", 'CAPABILITY PID\n{"lidClosed":1}\n');
  advance(1500);
  assert.equal(await result, null);
  monitor.stop();
});

test("helper failure clears cached state, restarts once, and ignores retired output", async () => {
  const { monitor, children, broadcasts, advance } = setup();
  monitor.start();
  const original = children[0];
  original.stdout.emit("data", '{"lidClosed":true}\n');
  assert.equal(await monitor.getState(), true);
  original.emit("error", new Error("process failed"));
  original.emit("exit", 1);
  assert.deepEqual(broadcasts, [true, null]);
  advance(2000);
  assert.equal(children.length, 2);
  original.stdout.emit("data", '{"lidClosed":true}\n');
  children[1].stdout.emit("data", '{"lidClosed":false}\n');
  assert.equal(await monitor.getState(), false);
  assert.deepEqual(broadcasts, [true, null, false]);
  monitor.stop();
});

test("heartbeats refresh freshness without duplicate broadcasts", async () => {
  const { monitor, children, broadcasts, advance } = setup();
  monitor.start();
  children[0].stdout.emit("data", '{"lidClosed":false}\n');
  advance(4000);
  children[0].stdout.emit("data", '{"lidClosed":false}\n');
  advance(4000);
  assert.equal(await monitor.getState(), false);
  assert.deepEqual(broadcasts, [false]);
  advance(1000);
  assert.deepEqual(broadcasts, [false, null]);
  assert.equal(children[0].kills, 1);
  advance(2000);
  assert.equal(children.length, 2);
  monitor.stop();
});

test("a delayed timer after sleep cannot expose a stale cached lid state", async () => {
  const { monitor, children, broadcasts, advance } = setup();
  monitor.start();
  children[0].stdout.emit("data", '{"lidClosed":true}\n');
  advance(10000, false);
  const result = monitor.getState();
  assert.deepEqual(broadcasts, [true, null]);
  assert.equal(children[0].kills, 1);
  advance(1500);
  assert.equal(await result, null);
  advance(500);
  assert.equal(children.length, 2);
  monitor.stop();
});

test("stop resolves pending reads and cancels retries and heartbeat timers", async () => {
  const { monitor, children, timers, advance } = setup();
  const result = monitor.getState();
  monitor.stop();
  assert.equal(await result, null);
  assert.equal(children[0].kills, 1);
  assert.equal(timers.size, 0);
  children[0].emit("exit", 0);
  advance(10000);
  assert.equal(children.length, 1);
});

test("missing helpers are retried without blocking or persisting a known state", async () => {
  let lookups = 0;
  const { monitor, advance, timers } = setup({
    resolveBinary: () => {
      lookups++;
      return null;
    },
  });
  const result = monitor.getState();
  advance(1500);
  assert.equal(await result, null);
  advance(500);
  assert.equal(lookups, 2);
  monitor.stop();
  assert.equal(timers.size, 0);
});

test("excessive unterminated helper output is retired and clears known state", () => {
  const { monitor, children, broadcasts } = setup();
  monitor.start();
  children[0].stdout.emit("data", '{"lidClosed":true}\n');
  children[0].stdout.emit("data", "x".repeat(4097));
  assert.deepEqual(broadcasts, [true, null]);
  assert.equal(children[0].kills, 1);
  monitor.stop();
});
