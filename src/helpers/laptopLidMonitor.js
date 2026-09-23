const { spawn } = require("child_process");

const INITIAL_WAIT_MS = 1500;
const STALE_STATE_MS = 5000;
const RESTART_DELAY_MS = 2000;
const MAX_BUFFER_LENGTH = 4096;

function parseLidState(line) {
  try {
    const value = JSON.parse(line)?.lidClosed;
    return value === null || typeof value === "boolean" ? value : undefined;
  } catch {
    return undefined;
  }
}

function createLaptopLidMonitor({
  spawnHelper = spawn,
  resolveBinary = () =>
    require("./binaryResolver").resolveBundledBinary("macos-mic-listener", "audio"),
  broadcast = (state) =>
    require("./windowBroadcast").broadcastToWindows("laptop-lid-state-changed", state),
  logError = (error) =>
    require("./debugLogger").warn("Laptop lid monitor unavailable", { error: error?.message }),
  now = Date.now,
  schedule = setTimeout,
  cancel = clearTimeout,
} = {}) {
  let running = false;
  let child = null;
  let restartTimer = null;
  let staleTimer = null;
  let state = null;
  let receivedState = false;
  let lastSampleAt = 0;
  const waiters = new Set();

  function finishWaiters(value) {
    for (const finish of [...waiters]) finish(value);
  }

  function updateState(value) {
    if (state !== value) {
      state = value;
      broadcast(value);
    }
  }

  function invalidateState() {
    receivedState = false;
    lastSampleAt = 0;
    if (staleTimer) cancel(staleTimer);
    staleTimer = null;
    updateState(null);
  }

  function scheduleRestart() {
    if (!running || restartTimer) return;
    restartTimer = schedule(() => {
      restartTimer = null;
      launch();
    }, RESTART_DELAY_MS);
    restartTimer.unref?.();
  }

  function retireChild(processToRetire, error) {
    if (child !== processToRetire) return;
    child = null;
    invalidateState();
    finishWaiters(null);
    if (error) logError(error);
    if (processToRetire.exitCode === null) processToRetire.kill();
    scheduleRestart();
  }

  function armStaleTimer(processToWatch) {
    if (staleTimer) cancel(staleTimer);
    staleTimer = schedule(() => {
      staleTimer = null;
      retireChild(processToWatch, new Error("Lid state heartbeat timed out"));
    }, STALE_STATE_MS);
    staleTimer.unref?.();
  }

  function launch() {
    if (!running || child) return;
    let helper;
    try {
      helper = resolveBinary();
      if (!helper) {
        scheduleRestart();
        return;
      }
      child = spawnHelper(helper, ["--watch-lid-state"], {
        stdio: ["ignore", "pipe", "ignore"],
        windowsHide: true,
      });
    } catch (error) {
      logError(error);
      scheduleRestart();
      return;
    }

    const currentChild = child;
    let buffer = "";
    currentChild.stdout.setEncoding("utf8");
    currentChild.stdout.on("data", (chunk) => {
      if (child !== currentChild || !running) return;
      buffer += chunk;
      if (buffer.length > MAX_BUFFER_LENGTH) {
        retireChild(currentChild, new Error("Invalid lid state output"));
        return;
      }
      let newline;
      while ((newline = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        const value = parseLidState(line);
        if (value === undefined) continue;
        lastSampleAt = now();
        receivedState = true;
        updateState(value);
        finishWaiters(value);
        armStaleTimer(currentChild);
      }
    });
    currentChild.on("error", (error) => retireChild(currentChild, error));
    currentChild.on("exit", () => retireChild(currentChild));
    armStaleTimer(currentChild);
  }

  function start() {
    if (running) return;
    running = true;
    launch();
  }

  function getState() {
    start();
    if (receivedState && now() - lastSampleAt < STALE_STATE_MS) {
      return Promise.resolve(state);
    }
    // Timers can be delayed by sleep; never return a pre-sleep cached value.
    if (receivedState) retireChild(child, new Error("Cached lid state expired"));
    return new Promise((resolve) => {
      const finish = (value) => {
        cancel(timer);
        waiters.delete(finish);
        resolve(value);
      };
      const timer = schedule(() => finish(null), INITIAL_WAIT_MS);
      waiters.add(finish);
    });
  }

  function stop() {
    running = false;
    if (restartTimer) cancel(restartTimer);
    restartTimer = null;
    const previousChild = child;
    child = null;
    invalidateState();
    finishWaiters(null);
    previousChild?.kill();
  }

  return { start, getState, stop };
}

module.exports = {
  createLaptopLidMonitor,
  parseLidState,
  laptopLidMonitor: createLaptopLidMonitor(),
};
