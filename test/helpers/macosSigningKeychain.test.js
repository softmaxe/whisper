const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const script = path.resolve(__dirname, "../../scripts/sign-macos-release.js");
const source = fs.readFileSync(script, "utf8");
const fingerprint = "A".repeat(40);
const directory = "/tmp/whisper-signing-fixture";
const keychain = path.join(directory, "signing.keychain-db");
const originalSearchList = [
  "/Users/fixture/Library/Keychains/login.keychain-db",
  "/Library/Keychains/System.keychain",
];

function signingHarness({ autoAdded = false, failAt, identity, duringSigning } = {}) {
  const state = {
    searchList: [...originalSearchList],
    files: new Set(),
    commands: [],
    signed: false,
    verified: false,
    removed: false,
  };
  const module = { exports: {} };
  const mockFs = {
    mkdtempSync: () => directory,
    realpathSync: (value) => value,
    existsSync: (value) => state.files.has(value),
    writeFileSync: (file, _value, options) => {
      assert.equal(options.mode, 0o600);
      state.files.add(file);
    },
    rmSync: (value, options) => {
      assert.equal(value, directory);
      assert.deepEqual({ ...options }, { recursive: true, force: true });
      state.files.clear();
      state.removed = true;
    },
  };
  function run(command, args, description) {
    assert.equal(command, "/usr/bin/security");
    const operation = args[0];
    state.commands.push(operation);
    if (operation === "create-keychain") {
      state.files.add(keychain);
      if (autoAdded) state.searchList.push(keychain);
    }
    if (operation === failAt) throw new Error(`${description} failed.`);
    if (operation === "list-keychains") {
      if (args.includes("-s")) state.searchList = Array.from(args.slice(args.indexOf("-s") + 1));
      return state.searchList.map((entry) => `    "${entry}"`).join("\n");
    }
    if (operation === "find-identity") {
      assert.deepEqual(Array.from(args), ["find-identity", "-p", "codesigning", keychain]);
      return identity ?? `1) ${fingerprint} "Whisper Release" (CSSMERR_TP_NOT_TRUSTED)`;
    }
    if (operation === "delete-keychain") {
      assert.equal(args[1], keychain);
      state.files.delete(keychain);
    }
    return "";
  }
  const mocks = {
    "node:crypto": { randomBytes: () => Buffer.from("test-only-placeholder") },
    "node:fs": mockFs,
    "node:os": { tmpdir: () => "/tmp" },
    "node:path": path,
    "./lib/macos-signing": {
      certificateFingerprint: () => fingerprint,
      loadSigningCredentials: () => ({
        certificate: Buffer.from("not-a-real-PKCS12"),
        password: "test-only-placeholder",
      }),
      createSigningIgnore: () => () => false,
      designatedRequirement: () => "pinned requirement",
      signingIdentifier: () => "local.whisper.desktop",
      verifySignature: () => {
        assert.ok(state.searchList.includes(keychain));
        state.verified = true;
        if (failAt === "verify") throw new Error("Signature verification failed.");
      },
      run,
    },
    "@electron/osx-sign": {
      signAsync: async (options) => {
        state.signed = true;
        assert.equal(options.identity, fingerprint);
        assert.equal(options.keychain, keychain);
        assert.deepEqual(state.searchList, [...originalSearchList, keychain]);
        assert.equal(
          options.optionsForFile(options.app).requirements,
          "=designated => pinned requirement"
        );
        duringSigning?.(state);
        if (failAt === "sign") throw new Error("Code signing failed.");
      },
    },
  };
  // Run the real orchestration with all filesystem, credential and process
  // boundaries replaced. These regressions never create or open a keychain.
  vm.runInNewContext(
    source,
    {
      module,
      process: { platform: "darwin" },
      console: { log() {} },
      require: (name) => {
        assert.ok(Object.hasOwn(mocks, name), `Unexpected module: ${name}`);
        return mocks[name];
      },
    },
    { filename: script }
  );
  return { state, sign: () => module.exports({ app: "/build/Whisper.app" }) };
}

for (const autoAdded of [false, true]) {
  test(`temporary identity remains searchable when create-keychain ${autoAdded ? "does" : "does not"} add it`, async () => {
    const { state, sign } = signingHarness({ autoAdded });
    await sign();
    assert.equal(state.signed, true);
    assert.equal(state.verified, true);
    assert.deepEqual(state.searchList, originalSearchList);
    assert.equal(state.files.size, 0);
    assert.equal(state.removed, true);
  });
}

test("cleanup preserves unrelated search-list changes made while signing", async () => {
  const otherKeychain = "/tmp/another-build.keychain-db";
  const { state, sign } = signingHarness({
    duringSigning: (state) => {
      state.searchList = [otherKeychain, ...state.searchList.slice(1)];
    },
  });
  await sign();
  assert.deepEqual(state.searchList, [otherKeychain, originalSearchList[1]]);
  assert.equal(state.files.size, 0);
});

for (const failAt of ["create-keychain", "import", "sign", "verify", "delete-keychain"]) {
  test(`temporary signing files and search-list entries are removed after ${failAt} fails`, async () => {
    const { state, sign } = signingHarness({ autoAdded: true, failAt });
    await assert.rejects(sign(), /failed/);
    assert.deepEqual(state.searchList, originalSearchList);
    assert.equal(state.files.size, 0);
    assert.equal(state.removed, true);
  });
}

for (const identity of ["0 identities found", `1) ${"B".repeat(40)} "Other identity"`]) {
  test(`an unavailable pinned certificate/private-key pair fails before codesign: ${identity.slice(0, 12)}`, async () => {
    const { state, sign } = signingHarness({ identity });
    await assert.rejects(sign(), /does not match the pinned certificate or has no private key/);
    assert.equal(state.signed, false);
    assert.equal(state.verified, false);
    assert.deepEqual(state.searchList, originalSearchList);
    assert.equal(state.files.size, 0);
  });
}
