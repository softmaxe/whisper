const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {
  APP_IDENTIFIER,
  certificateFingerprint,
  createSigningIgnore,
  designatedRequirement,
  isSignableCode,
  loadSigningCredentials,
  run,
  signingIdentifier,
} = require("../../scripts/lib/macos-signing");

function credentialsDirectory(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "whisper-signing-test-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  fs.writeFileSync(path.join(directory, "identity.p12"), "local identity");
  fs.writeFileSync(path.join(directory, "password"), "local-password\n");
  return directory;
}

test("release signing uses explicit credentials before a local backup", (t) => {
  const directory = credentialsDirectory(t);
  const credentials = loadSigningCredentials(
    {
      WHISPER_SIGNING_CERTIFICATE: Buffer.from("release identity").toString("base64"),
      WHISPER_SIGNING_PASSWORD: "release-password",
    },
    directory
  );
  assert.equal(credentials.certificate.toString(), "release identity");
  assert.equal(credentials.password, "release-password");
  assert.equal(loadSigningCredentials({}, directory).certificate.toString(), "local identity");
});

test("partial or malformed release secrets never fall back to a local identity", (t) => {
  const directory = credentialsDirectory(t);
  for (const env of [
    { WHISPER_SIGNING_PASSWORD: "password" },
    { WHISPER_SIGNING_CERTIFICATE: "aWRlbnRpdHk=" },
    { WHISPER_SIGNING_CERTIFICATE: "", WHISPER_SIGNING_PASSWORD: "password" },
    { WHISPER_SIGNING_CERTIFICATE: "%%%", WHISPER_SIGNING_PASSWORD: "password" },
  ]) {
    assert.throws(() => loadSigningCredentials(env, directory), /WHISPER_SIGNING_/);
  }
  fs.rmSync(path.join(directory, "identity.p12"));
  assert.throws(() => loadSigningCredentials({}, directory), /Signing identity is missing/);
});

test("certificate requirements reject ad-hoc signing and bind the exact public certificate", () => {
  const fingerprint = certificateFingerprint();
  assert.match(fingerprint, /^[A-F0-9]{40}$/);
  assert.equal(
    designatedRequirement(APP_IDENTIFIER, fingerprint),
    `identifier "${APP_IDENTIFIER}" and certificate leaf = H"${fingerprint}"`
  );
  assert.throws(() => designatedRequirement(APP_IDENTIFIER, "-"), /fingerprint/);
  assert.throws(() => designatedRequirement('test" or true', fingerprint), /identifier/);
});

test("loose executable identities survive new build directories and distinguish paths", () => {
  const plist = (_bundle, key) => (key === "CFBundleExecutable" ? "Whisper" : APP_IDENTIFIER);
  const first = signingIdentifier(
    "/build/one/Whisper.app",
    "/build/one/Whisper.app/Contents/Resources/bin/helper",
    plist
  );
  const second = signingIdentifier(
    "/build/two/Whisper.app",
    "/build/two/Whisper.app/Contents/Resources/bin/helper",
    plist
  );
  assert.equal(first, second);
  assert.notEqual(
    first,
    signingIdentifier(
      "/build/one/Whisper.app",
      "/build/one/Whisper.app/Contents/Resources/bin/other-helper",
      plist
    )
  );
  assert.throws(
    () => signingIdentifier("/build/one/Whisper.app", "/outside/helper", plist),
    /inside/
  );
});

test("bundle executables keep their bundle identifiers when the enclosing bundle is signed", () => {
  const app = "/build/Whisper.app";
  const plist = (bundle, key) => {
    const helper = bundle.endsWith("Helper.app");
    return key === "CFBundleExecutable"
      ? helper
        ? "Helper"
        : "Whisper"
      : helper
        ? `${APP_IDENTIFIER}.helper`
        : APP_IDENTIFIER;
  };
  assert.equal(signingIdentifier(app, app, plist), APP_IDENTIFIER);
  assert.equal(signingIdentifier(app, `${app}/Contents/MacOS/Whisper`, plist), APP_IDENTIFIER);
  const helper = `${app}/Contents/Frameworks/Helper.app`;
  assert.equal(signingIdentifier(app, helper, plist), `${APP_IDENTIFIER}.helper`);
  assert.equal(
    signingIdentifier(app, `${helper}/Contents/MacOS/Helper`, plist),
    `${APP_IDENTIFIER}.helper`
  );
  assert.throws(
    () => signingIdentifier(app, app, () => "changed.identifier"),
    /must keep CFBundleIdentifier/
  );
});

test("signing subprocess errors omit arguments and stderr that can contain secrets", () => {
  let error;
  try {
    run(
      process.execPath,
      ["-e", 'console.error("secret-password");process.exit(1)'],
      "Importing identity"
    );
  } catch (caught) {
    error = caught;
  }
  assert.equal(error.message, "Importing identity failed (1).");
  assert.ok(!error.stack.includes("secret-password"));
});

test("code signing skips binary resources and deduplicates symlink aliases", (t) => {
  const directory = credentialsDirectory(t);
  const app = path.join(directory, "Whisper.app");
  fs.mkdirSync(app);
  const binary = path.join(app, "helper");
  fs.writeFileSync(binary, Buffer.from("cffaedfe0000000000000000", "hex"));
  const alias = path.join(app, "helper-alias");
  fs.symlinkSync("helper", alias);
  const resource = path.join(app, "locale.pak");
  fs.writeFileSync(resource, Buffer.alloc(24));
  const java = path.join(app, "Example.class");
  fs.writeFileSync(java, Buffer.from("cafebabe0000003400010000", "hex"));
  assert.equal(isSignableCode(binary), true);
  assert.equal(isSignableCode(resource), false);
  assert.equal(isSignableCode(java), false);
  const ignore = createSigningIgnore();
  assert.equal(ignore(binary), false);
  assert.equal(ignore(alias), true);
  assert.equal(ignore(resource), true);
  assert.equal(ignore(app), false);
  assert.equal(createSigningIgnore((file) => file === binary)(binary), true);
  assert.equal(createSigningIgnore([/helper$/])(binary), true);
  const plist = (_bundle, key) => (key === "CFBundleExecutable" ? "Whisper" : APP_IDENTIFIER);
  assert.equal(signingIdentifier(app, binary, plist), signingIdentifier(app, alias, plist));
});
