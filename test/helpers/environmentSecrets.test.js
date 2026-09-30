const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const vm = require("node:vm");
const crypto = require("node:crypto");
const dotenv = require("dotenv");

function createFixture(t, { unavailable = false } = {}) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "whisper-environment-secrets-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const accesses = [];
  let storedMasterKey = null;
  const envPath = path.join(directory, ".env");
  const secureDirectory = path.join(directory, "secure-keys");
  const secretPath = path.join(secureDirectory, "CUSTOM_CLEANUP_API_KEY.enc");
  const sentinelPath = path.join(secureDirectory, ".migrated");

  function start() {
    const environment = {};
    const electron = {
      app: { getPath: () => directory },
      safeStorage: {
        isEncryptionAvailable() {
          accesses.push("safeStorage.isEncryptionAvailable");
          return false;
        },
      },
    };
    const dependencies = {
      crypto,
      fs,
      "fs/promises": fs.promises,
      path,
      electron,
      "./debugLogger": { info() {}, warn() {}, error() {} },
      "./i18nMain": { normalizeUiLanguage: (value) => value },
      "@napi-rs/keyring": {
        Entry: class {
          constructor() {
            accesses.push("keyring.Entry");
            if (unavailable) throw new Error("Keychain unavailable in test");
          }
          getPassword() {
            accesses.push("keyring.getPassword");
            return storedMasterKey;
          }
          setPassword(value) {
            accesses.push("keyring.setPassword");
            storedMasterKey = value;
          }
        },
      },
      dotenv: {
        config({ path: filePath, override = false }) {
          if (!fs.existsSync(filePath)) return;
          for (const [key, value] of Object.entries(dotenv.parse(fs.readFileSync(filePath)))) {
            if (override || environment[key] === undefined) environment[key] = value;
          }
        },
      },
    };
    function load(name) {
      const filename = path.resolve(__dirname, "../../src/helpers", name);
      const module = { exports: {} };
      vm.runInNewContext(
        fs.readFileSync(filename, "utf8"),
        {
          module,
          Buffer,
          // Keep all fallback config reads inside the disposable profile.
          __dirname: path.join(directory, "src", "helpers"),
          process: { env: environment, resourcesPath: directory, platform: "darwin" },
          require(request) {
            if (Object.hasOwn(dependencies, request)) return dependencies[request];
            throw new Error(`Unexpected dependency: ${request}`);
          },
        },
        { filename }
      );
      return module.exports;
    }
    dependencies["./secretCrypto"] = load("secretCrypto.js");
    const EnvironmentManager = load("environment.js");
    const manager = new EnvironmentManager();
    const saveSecret = t.mock.method(manager, "_saveSecretKey");
    return {
      manager,
      async saveKey(value) {
        manager.saveCleanupCustomKey(value);
        await Promise.all(saveSecret.mock.calls.map((call) => call.result));
      },
    };
  }

  return { start, accesses, envPath, secureDirectory, secretPath, sentinelPath };
}

test("saving a normal setting without an API key never touches Keychain", async (t) => {
  const fixture = createFixture(t);
  const { manager } = fixture.start();
  await manager.saveMenuBarIconVisible(true);
  assert.match(fs.readFileSync(fixture.envPath, "utf8"), /SHOW_MENU_BAR_ICON=true/);
  assert.deepEqual(fixture.accesses, []);
});

test("startup migration and later settings saves without secrets never touch Keychain", async (t) => {
  const fixture = createFixture(t);
  fs.writeFileSync(fixture.envPath, "UI_LANGUAGE=zh-CN\n");
  const { manager } = fixture.start();
  await manager.init();
  assert.equal(fs.existsSync(fixture.sentinelPath), true);
  assert.deepEqual(fixture.accesses, []);
  await manager.saveMenuBarIconVisible(false);
  await fixture.start().manager.init();
  assert.deepEqual(fixture.accesses, []);
});

test("saving an empty API key never initializes Keychain", async (t) => {
  const fixture = createFixture(t);
  await fixture.start().saveKey("");
  assert.equal(fs.existsSync(fixture.secretPath), false);
  assert.deepEqual(fixture.accesses, []);
});

test("clearing a saved API key deletes its ciphertext without accessing Keychain", async (t) => {
  const fixture = createFixture(t);
  fs.mkdirSync(fixture.secureDirectory);
  fs.writeFileSync(fixture.secretPath, "unread ciphertext fixture");
  const { manager, saveKey } = fixture.start();
  await saveKey("");
  assert.equal(manager.getCleanupCustomKey(), "");
  assert.equal(fs.existsSync(fixture.secretPath), false);
  assert.deepEqual(fixture.accesses, []);
});

test("a nonempty API key stays encrypted and can be loaded after restart", async (t) => {
  const fixture = createFixture(t);
  const { manager, saveKey } = fixture.start();
  await manager.init();
  assert.deepEqual(fixture.accesses, []);
  const value = "test-only-credential";
  await saveKey(value);
  assert.ok(fixture.accesses.includes("keyring.setPassword"));
  assert.equal(fs.readFileSync(fixture.secretPath).includes(Buffer.from(value)), false);
  await manager.saveMenuBarIconVisible(true);
  assert.doesNotMatch(fs.readFileSync(fixture.envPath, "utf8"), /CUSTOM_CLEANUP_API_KEY/);
  fixture.accesses.length = 0;
  const restarted = fixture.start().manager;
  await restarted.init();
  assert.equal(restarted.getCleanupCustomKey(), value);
  assert.ok(fixture.accesses.includes("keyring.getPassword"));
  assert.equal(fixture.accesses.includes("keyring.setPassword"), false);
});

test("legacy plaintext credentials migrate before being removed from config", async (t) => {
  const fixture = createFixture(t);
  fs.writeFileSync(fixture.envPath, "CUSTOM_REASONING_API_KEY=test-only-credential\n");
  const { manager } = fixture.start();
  await manager.init();
  assert.equal(manager.getCleanupCustomKey(), "test-only-credential");
  assert.equal(fs.existsSync(fixture.secretPath), true);
  assert.equal(fs.existsSync(fixture.sentinelPath), true);
  assert.doesNotMatch(fs.readFileSync(fixture.envPath, "utf8"), /API_KEY/);
});

test("failed encryption preserves unmigrated credentials", async (t) => {
  const fixture = createFixture(t, { unavailable: true });
  fs.writeFileSync(fixture.envPath, "CUSTOM_CLEANUP_API_KEY=test-only-credential\n");
  const { manager } = fixture.start();
  await manager.init();
  await manager.saveMenuBarIconVisible(true);
  assert.equal(fs.existsSync(fixture.sentinelPath), false);
  assert.equal(
    dotenv.parse(fs.readFileSync(fixture.envPath)).CUSTOM_CLEANUP_API_KEY,
    "test-only-credential"
  );
});
