const assert = require("node:assert/strict");
const { createHash } = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const signRelease = require("./sign-macos-release");
const {
  APP_IDENTIFIER,
  loadSigningCredentials,
  signingIdentifier,
  verifySignature,
} = require("./lib/macos-signing");
const {
  inside,
  copyPackageResources,
  packageCode,
  assertNativeFiles,
  publishArtifacts,
} = require("./lib/native-packaging");

const root = path.resolve(__dirname, "..");

function run(command, args) {
  const env = { ...process.env };
  // Build tools and dependency configure scripts never need the release identity.
  delete env.WHISPER_SIGNING_CERTIFICATE;
  delete env.WHISPER_SIGNING_PASSWORD;
  const result = spawnSync(command, args, { cwd: root, env, encoding: "utf8" });
  if (result.error || result.status !== 0) {
    throw new Error(
      `${command} failed: ${result.stderr || result.error?.message || result.status}`
    );
  }
  return result.stdout.trim();
}

function verifyNative(app, version, development) {
  const plist = path.join(app, "Contents", "Info.plist");
  for (const [key, expected] of Object.entries({
    CFBundleIdentifier: APP_IDENTIFIER,
    CFBundleName: "Whisper",
    CFBundleDisplayName: "Whisper",
    CFBundleVersion: version,
    CFBundleShortVersionString: version,
    LSMinimumSystemVersion: "27.0",
  })) {
    assert.equal(run("/usr/libexec/PlistBuddy", ["-c", `Print :${key}`, plist]), expected);
  }
  const code = packageCode(app);
  assertNativeFiles(app);
  for (const file of code) {
    if (!fs.statSync(file).isFile()) continue;
    assert.equal(
      run("/usr/bin/lipo", ["-archs", file]),
      "arm64",
      "Native code must be arm64-only."
    );
    const libraries = run("/usr/bin/otool", ["-L", file]);
    assert.doesNotMatch(libraries, /Electron|Chromium|libnode|WebKit|JavaScriptCore/i);
    for (const line of libraries.split("\n").slice(1)) {
      const dependency = line.trim().split(" (compatibility version:")[0];
      if (dependency.startsWith("/")) {
        assert.match(
          dependency,
          /^\/(System\/Library|usr\/lib)\//,
          "Bundled code cannot depend on a build machine's libraries."
        );
      }
    }
  }
  const executable = path.join(app, "Contents", "MacOS", "Whisper");
  assert.match(run("/usr/bin/vtool", ["-show-build", executable]), /minos\s+27\.0(?:\s|$)/);
  run("/usr/bin/codesign", ["--verify", "--deep", "--strict", app]);
  if (!development) verifySignature(app, code);
}

async function main() {
  assert.equal(process.platform, "darwin", "Native packaging requires macOS.");
  assert.equal(process.arch, "arm64", "Native packaging requires Apple Silicon.");
  const arguments_ = process.argv.slice(2);
  assert.ok(
    arguments_.every((argument) => ["--development", "--release"].includes(argument)),
    "Use --development or --release."
  );
  assert.ok(
    !(arguments_.includes("--development") && arguments_.includes("--release")),
    "Select one package mode."
  );
  const development = arguments_.includes("--development");
  // Development never reads credentials. Release cannot fall back to an ad-hoc identity.
  if (!development) loadSigningCredentials();
  const args = ["swift", "build", "--package-path", "native", "-c", "release", "--arch", "arm64"];
  run("/usr/bin/xcrun", args);
  const binaryDirectory = run("/usr/bin/xcrun", [...args, "--show-bin-path"]);
  const version = require("../package.json").version;
  assert.match(version, /^\d+\.\d+\.\d+$/);
  const manifest = JSON.parse(fs.readFileSync(path.join(root, "native", "packaging.json"), "utf8"));
  for (const script of manifest.buildScripts || []) run(process.execPath, [inside(root, script)]);
  fs.mkdirSync(path.join(root, "dist"), { recursive: true });
  const staging = fs.mkdtempSync(path.join(root, "dist", ".native-package-"));
  try {
    const app = path.join(staging, "Whisper.app");
    const contents = path.join(app, "Contents");
    const resources = path.join(contents, "Resources");
    fs.mkdirSync(path.join(contents, "MacOS"), { recursive: true });
    fs.mkdirSync(resources, { recursive: true });
    fs.copyFileSync(path.join(binaryDirectory, "Whisper"), path.join(contents, "MacOS", "Whisper"));
    copyPackageResources(root, resources, manifest);
    for (const entry of fs.readdirSync(binaryDirectory)) {
      if (entry.endsWith(".bundle")) {
        assert.ok(!fs.existsSync(path.join(resources, entry)), "Duplicate Swift resource bundle.");
        fs.cpSync(path.join(binaryDirectory, entry), path.join(resources, entry), {
          recursive: true,
        });
      }
    }
    fs.writeFileSync(
      path.join(contents, "Info.plist"),
      `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>${APP_IDENTIFIER}</string>
<key>CFBundleExecutable</key><string>Whisper</string>
<key>CFBundleName</key><string>Whisper</string>
<key>CFBundleDisplayName</key><string>Whisper</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>${version}</string>
<key>CFBundleShortVersionString</key><string>${version}</string>
<key>CFBundleIconFile</key><string>icon.icns</string>
<key>LSMinimumSystemVersion</key><string>27.0</string>
<key>LSArchitecturePriority</key><array><string>arm64</string></array>
<key>NSHighResolutionCapable</key><true/>
<key>NSMicrophoneUsageDescription</key><string>Whisper needs microphone access to transcribe your speech into text.</string>
<key>NSAccessibilityUsageDescription</key><string>Whisper needs Accessibility permissions to paste transcribed text into other applications.</string>
<key>NSLocalNetworkUsageDescription</key><string>Whisper connects to the speech recognition and text cleanup servers you configure.</string>
<key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
<key>CFBundleDevelopmentRegion</key><string>en</string>
<key>CFBundleLocalizations</key><array><string>en</string><string>zh-CN</string></array>
</dict></plist>`
    );
    const code = packageCode(app);
    assertNativeFiles(app);
    const entitlements = path.join(root, "native", "Resources", "entitlements.plist");
    if (development) {
      for (const file of code) {
        run("/usr/bin/codesign", [
          "--force",
          "--sign",
          "-",
          "--timestamp=none",
          "--options",
          "runtime",
          "--entitlements",
          entitlements,
          "--identifier",
          signingIdentifier(app, file),
          file,
        ]);
      }
    } else {
      await signRelease({
        app,
        platform: "darwin",
        type: "distribution",
        binaries: code.filter((file) => fs.statSync(file).isFile()),
        optionsForFile: () => ({ hardenedRuntime: true, entitlements }),
      });
    }
    verifyNative(app, version, development);
    const archiveName = `whisper-${version}-macos-arm64${development ? "-development" : ""}.zip`;
    const archive = path.join(staging, archiveName);
    run("/usr/bin/ditto", ["-c", "-k", "--keepParent", app, archive]);
    run("/usr/bin/unzip", ["-t", archive]);
    const extracted = path.join(staging, "verify");
    run("/usr/bin/ditto", ["-x", "-k", archive, extracted]);
    verifyNative(path.join(extracted, "Whisper.app"), version, development);
    const digest = createHash("sha256").update(fs.readFileSync(archive)).digest("hex");
    fs.writeFileSync(archive + ".sha256", `${digest}  ${archiveName}\n`);
    const output = path.join(
      root,
      "dist",
      development ? "native-development-arm64" : "native-arm64",
      "Whisper.app"
    );
    publishArtifacts(
      [
        { source: app, destination: output },
        { source: archive, destination: path.join(root, "release", archiveName) },
        {
          source: archive + ".sha256",
          destination: path.join(root, "release", archiveName + ".sha256"),
        },
      ],
      path.join(staging, "previous")
    );
    console.log(
      `Verified native ${development ? "development" : "release"} bundle, archive and checksum: ${path.relative(root, output)}, release/${archiveName}`
    );
    console.log("The installed application and legacy baseline were not changed.");
  } finally {
    fs.rmSync(staging, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
