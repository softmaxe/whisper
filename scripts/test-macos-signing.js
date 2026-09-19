// Exercise real codesign identity continuity without installing an app or requesting permissions.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const signRelease = require("./sign-macos-release");

function codesign(args) {
  const result = spawnSync("/usr/bin/codesign", args, { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout + result.stderr;
}

function requirement(target) {
  const output = codesign(["--display", "-r-", target]);
  const expression = output.match(/^designated => (.+)$/m)?.[1];
  assert.ok(expression, output);
  assert.doesNotMatch(expression, /cdhash/);
  assert.match(expression, /certificate leaf = H"[A-Fa-f0-9]{40}"/);
  return expression;
}

function cdhash(target) {
  return codesign(["--display", "--verbose=4", target]).match(/^CDHash=(.+)$/m)?.[1];
}

async function main() {
  assert.equal(process.platform, "darwin", "Signing integration tests require macOS.");
  const searchList = () =>
    spawnSync("/usr/bin/security", ["list-keychains", "-d", "user"], { encoding: "utf8" }).stdout;
  const signingDirectories = () =>
    fs
      .readdirSync(os.tmpdir())
      .filter((name) => /^whisper-signing-[A-Za-z0-9]{6}$/.test(name))
      .sort();
  const previousSearchList = searchList();
  const previousDirectories = signingDirectories();
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "whisper-signing-upgrade-"));
  try {
    const apps = [];
    const helpers = [];
    for (const version of [1, 2]) {
      const app = path.join(temporary, String(version), "Whisper.app");
      const contents = path.join(app, "Contents");
      const helper = path.join(contents, "Resources", "bin", "macos-fast-paste");
      fs.mkdirSync(path.join(contents, "MacOS"), { recursive: true });
      fs.mkdirSync(path.dirname(helper), { recursive: true });
      fs.writeFileSync(
        path.join(contents, "Info.plist"),
        `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>local.whisper.desktop</string>
<key>CFBundleExecutable</key><string>Whisper</string>
<key>CFBundleName</key><string>Whisper</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>${version}</string>
<key>CFBundleShortVersionString</key><string>1.0.${version}</string>
</dict></plist>`
      );
      // Different Mach-O code as well as different bundle versions.
      const source = version === 1 ? "/usr/bin/true" : "/usr/bin/false";
      fs.copyFileSync(source, path.join(contents, "MacOS", "Whisper"));
      fs.copyFileSync(source, helper);
      fs.symlinkSync("macos-fast-paste", `${helper}-alias`);
      const resource = path.join(contents, "Resources", "locale.pak");
      fs.writeFileSync(resource, Buffer.from([0, 1, 2, 3, 0, 255, 0, 128]));
      fs.linkSync(resource, path.join(contents, "Resources", "locale-copy.pak"));
      await signRelease({
        app,
        platform: "darwin",
        type: "distribution",
        binaries: [helper],
        optionsForFile: () => ({
          hardenedRuntime: true,
          entitlements: path.join(__dirname, "..", "resources", "mac", "entitlements.mac.plist"),
        }),
      });
      assert.equal(requirement(helper), requirement(`${helper}-alias`));
      assert.notEqual(
        spawnSync("/usr/bin/codesign", ["--display", resource]).status,
        0,
        "Resource data belongs in the bundle seal, not an individual code signature."
      );
      apps.push(app);
      helpers.push(helper);
    }
    for (const targets of [apps, helpers]) {
      const first = requirement(targets[0]);
      const second = requirement(targets[1]);
      assert.equal(first, second, "The signing identity changed between versions.");
      assert.ok(cdhash(targets[0]));
      assert.notEqual(cdhash(targets[0]), cdhash(targets[1]), "Test binaries must differ.");
      codesign(["--verify", "--strict", "-R", `=${first}`, targets[1]]);
      codesign(["--verify", "--strict", "-R", `=${second}`, targets[0]]);
    }
    const helperAsApp = spawnSync("/usr/bin/codesign", [
      "--verify",
      "-R",
      `=${requirement(apps[0])}`,
      helpers[1],
    ]);
    assert.notEqual(helperAsApp.status, 0, "A helper must not satisfy the main app's requirement.");
    const wrongCertificate = requirement(apps[0]).replace(
      /H"[A-Fa-f0-9]{40}"/,
      `H"${"0".repeat(40)}"`
    );
    const mismatchedSigner = spawnSync("/usr/bin/codesign", [
      "--verify",
      "-R",
      `=${wrongCertificate}`,
      apps[1],
    ]);
    assert.notEqual(
      mismatchedSigner.status,
      0,
      "A different certificate must not satisfy the requirement."
    );
    const originalCertificate = process.env.WHISPER_SIGNING_CERTIFICATE;
    const originalPassword = process.env.WHISPER_SIGNING_PASSWORD;
    try {
      // Exercise failure after creating/importing into a temporary keychain, without new identities.
      process.env.WHISPER_SIGNING_CERTIFICATE =
        Buffer.from("invalid PKCS12 fixture").toString("base64");
      process.env.WHISPER_SIGNING_PASSWORD = "invalid-password-placeholder";
      await assert.rejects(
        signRelease({ app: apps[0], platform: "darwin", type: "distribution" }),
        /Importing signing identity failed/
      );
    } finally {
      if (originalCertificate === undefined) delete process.env.WHISPER_SIGNING_CERTIFICATE;
      else process.env.WHISPER_SIGNING_CERTIFICATE = originalCertificate;
      if (originalPassword === undefined) delete process.env.WHISPER_SIGNING_PASSWORD;
      else process.env.WHISPER_SIGNING_PASSWORD = originalPassword;
    }
    assert.deepEqual(
      signingDirectories(),
      previousDirectories,
      "Temporary private signing files survived."
    );
    assert.equal(
      searchList(),
      previousSearchList,
      "Temporary signing keychains survived in the search list."
    );
    console.log(
      "Two changed app versions and native helpers retain the same certificate-bound identity."
    );
    console.log(
      "Temporary signing files and keychain entries are cleaned after success and import failure."
    );
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
