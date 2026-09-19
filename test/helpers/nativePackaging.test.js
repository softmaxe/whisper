const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {
  inside,
  copyPackageResources,
  runtimeResourceBundles,
  packageCode,
  assertNativeFiles,
  publishArtifacts,
} = require("../../scripts/lib/native-packaging");

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "whisper-native-package-test-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

test("native resources preserve helper paths and required license material", (t) => {
  const root = fixture(t);
  fs.writeFileSync(path.join(root, "helper"), Buffer.from("cffaedfe0000000000000000", "hex"));
  fs.writeFileSync(path.join(root, "LICENSE"), "synthetic license fixture");
  const app = path.join(root, "Whisper.app");
  const resources = path.join(app, "Contents/Resources");
  copyPackageResources(root, resources, {
    helpers: [
      {
        from: "helper",
        to: "bin/macos-fixture",
        licenses: [{ from: "LICENSE", to: "bin/fixture.LICENSE" }],
      },
    ],
  });
  assert.equal(
    fs.readFileSync(path.join(resources, "bin/fixture.LICENSE"), "utf8"),
    "synthetic license fixture"
  );
  assert.ok(
    packageCode(app).includes(
      path.join(fs.realpathSync(app), "Contents/Resources/bin/macos-fixture")
    )
  );
  assertNativeFiles(app);
});

test("native manifest rejects missing helper license, destination collisions and root escapes", (t) => {
  const root = fixture(t);
  fs.writeFileSync(path.join(root, "resource"), "fixture");
  const destination = path.join(root, "output");
  assert.throws(
    () =>
      copyPackageResources(root, destination, {
        helpers: [{ from: "resource", to: "bin/helper" }],
      }),
    /license/
  );
  assert.throws(() => inside(root, "../outside"), /inside/);
  assert.throws(() => inside(root, "/outside"), /relative/);
  assert.throws(
    () =>
      copyPackageResources(root, destination, {
        resources: [
          { from: "resource", to: "same" },
          { from: "resource", to: "same" },
        ],
      }),
    /Duplicate/
  );
});

test("native package scan rejects escaping symlinks and embedded Web runtimes", (t) => {
  const root = fixture(t);
  const app = path.join(root, "Whisper.app");
  fs.mkdirSync(app);
  fs.writeFileSync(path.join(root, "outside"), "private fixture");
  fs.symlinkSync("../outside", path.join(app, "escape"));
  assert.throws(() => packageCode(app), /cannot leave/);
  fs.unlinkSync(path.join(app, "escape"));
  fs.mkdirSync(path.join(app, "node_modules"));
  assert.throws(() => assertNativeFiles(app), /runtime/);
});

test("failed artifact publication restores earlier artifacts and never removes legacy baselines", (t) => {
  const root = fixture(t);
  const output = path.join(root, "output");
  fs.mkdirSync(output);
  fs.writeFileSync(path.join(output, "Whisper.app"), "previous bundle");
  fs.writeFileSync(path.join(output, "archive.zip"), "previous archive");
  fs.writeFileSync(path.join(output, "whisper-legacy-baseline.zip"), "preserved baseline");
  const staged = path.join(root, "new-app");
  fs.writeFileSync(staged, "new bundle");
  assert.throws(
    () =>
      publishArtifacts(
        [
          { source: staged, destination: path.join(output, "Whisper.app") },
          {
            source: path.join(root, "missing-archive"),
            destination: path.join(output, "archive.zip"),
          },
        ],
        path.join(root, "backups")
      ),
    /ENOENT/
  );
  assert.equal(fs.readFileSync(path.join(output, "Whisper.app"), "utf8"), "previous bundle");
  assert.equal(fs.readFileSync(path.join(output, "archive.zip"), "utf8"), "previous archive");
  assert.equal(
    fs.readFileSync(path.join(output, "whisper-legacy-baseline.zip"), "utf8"),
    "preserved baseline"
  );
});

test("native packaging includes runtime and dependency resources but excludes declared test targets", () => {
  const entries = [
    "WhisperNative_WhisperApp.bundle",
    "WhisperNative_WhisperCore.bundle",
    "WhisperNative_Fixtures.bundle",
    "Dependency_Resources.bundle",
    "Whisper",
    "debug.dSYM",
  ];
  assert.deepEqual(
    runtimeResourceBundles(entries, {
      name: "WhisperNative",
      targets: [
        { name: "WhisperApp", type: "executable" },
        { name: "WhisperCore", type: "library" },
        { name: "Fixtures", c99name: "Fixtures", type: "test" },
      ],
    }),
    [
      "WhisperNative_WhisperApp.bundle",
      "WhisperNative_WhisperCore.bundle",
      "Dependency_Resources.bundle",
    ]
  );
});
