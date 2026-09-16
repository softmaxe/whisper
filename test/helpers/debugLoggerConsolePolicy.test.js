const test = require("node:test");
const assert = require("node:assert/strict");
const os = require("node:os");
const Module = require("node:module");

const loggerPath = require.resolve("../../src/helpers/debugLogger.js");
const originalLoad = Module._load;
const originalPlatform = Object.getOwnPropertyDescriptor(process, "platform");

function withRuntime({ platform, argv = [], env = {}, electron }, loadModule) {
  const previousArgv = process.argv;
  const previousEnvironment = {
    LOG_LEVEL: process.env.LOG_LEVEL,
    NODE_ENV: process.env.NODE_ENV,
    OPENWHISPR_LOG_LEVEL: process.env.OPENWHISPR_LOG_LEVEL,
  };

  Object.defineProperty(process, "platform", { value: platform, configurable: true });
  process.argv = ["node", "main.js", ...argv];
  delete process.env.LOG_LEVEL;
  delete process.env.NODE_ENV;
  delete process.env.OPENWHISPR_LOG_LEVEL;
  Object.assign(process.env, env);

  Module._load = function loadWithElectronMock(request, parent, isMain) {
    if (request === "electron") return electron;
    return originalLoad.call(this, request, parent, isMain);
  };

  try {
    return loadModule();
  } finally {
    Module._load = originalLoad;
    Object.defineProperty(process, "platform", originalPlatform);
    process.argv = previousArgv;
    delete process.env.LOG_LEVEL;
    delete process.env.NODE_ENV;
    delete process.env.OPENWHISPR_LOG_LEVEL;
    for (const [name, value] of Object.entries(previousEnvironment)) {
      if (value !== undefined) process.env[name] = value;
    }
  }
}

function loadLogger({
  platform,
  isPackaged,
  argv,
  env,
  isReady = false,
  userDataPath = os.tmpdir(),
}) {
  delete require.cache[loggerPath];
  return withRuntime(
    {
      platform,
      argv,
      env,
      electron: {
        app: {
          getAppPath: () => "/openwhispr",
          getPath: () => userDataPath,
          getVersion: () => "9.9.9-test",
          isPackaged,
          isReady: () => isReady,
        },
      },
    },
    () => require(loggerPath)
  );
}

async function captureConsole(run) {
  const calls = [];
  const originalConsole = {
    error: console.error,
    log: console.log,
    warn: console.warn,
  };

  console.error = (...args) => calls.push(["error", ...args]);
  console.log = (...args) => calls.push(["log", ...args]);
  console.warn = (...args) => calls.push(["warn", ...args]);
  try {
    await run();
  } finally {
    Object.assign(console, originalConsole);
  }
  return calls;
}

test("packaged macOS preserves console logging", async (t) => {
  const cases = [{ name: "packaged macOS", platform: "darwin", isPackaged: true }];

  for (const testCase of cases) {
    await t.test(testCase.name, async () => {
      const logger = loadLogger(testCase);

      const calls = await captureConsole(() => logger.info("Existing console behavior"));

      assert.deepEqual(calls, [["log", "[INFO] Existing console behavior"]]);
    });
  }
});
