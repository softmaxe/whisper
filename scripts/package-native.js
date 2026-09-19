const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const signRelease = require("./sign-macos-release");
const { loadSigningCredentials, verifySignature } = require("./lib/macos-signing");

const root = path.resolve(__dirname, "..");
const app = path.join(root, "dist", "native-arm64", "Whisper.app");

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8" });
  if (result.error || result.status !== 0) {
    throw new Error(
      `${command} failed: ${result.stderr || result.error?.message || result.status}`
    );
  }
  return result.stdout.trim();
}

async function main() {
  assert.equal(process.platform, "darwin", "Native packaging requires macOS.");
  assert.equal(process.arch, "arm64", "Native packaging requires Apple Silicon.");
  // Fail before packaging if the original identity is unavailable. There is no ad-hoc fallback.
  loadSigningCredentials();
  const args = ["swift", "build", "--package-path", "native", "-c", "release", "--arch", "arm64"];
  run("/usr/bin/xcrun", args);
  const binaryDirectory = run("/usr/bin/xcrun", [...args, "--show-bin-path"]);
  const version = require("../package.json").version;
  assert.match(version, /^\d+\.\d+\.\d+$/);
  fs.rmSync(app, { recursive: true, force: true });
  const contents = path.join(app, "Contents");
  fs.mkdirSync(path.join(contents, "MacOS"), { recursive: true });
  fs.mkdirSync(path.join(contents, "Resources"), { recursive: true });
  fs.copyFileSync(path.join(binaryDirectory, "Whisper"), path.join(contents, "MacOS", "Whisper"));
  fs.copyFileSync(
    path.join(root, "src", "assets", "icon.icns"),
    path.join(contents, "Resources", "icon.icns")
  );
  fs.copyFileSync(path.join(root, "LICENSE"), path.join(contents, "Resources", "LICENSE"));
  for (const entry of fs.readdirSync(binaryDirectory)) {
    if (entry.endsWith(".bundle")) {
      fs.cpSync(path.join(binaryDirectory, entry), path.join(contents, "Resources", entry), {
        recursive: true,
      });
    }
  }
  fs.writeFileSync(
    path.join(contents, "Info.plist"),
    `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>local.whisper.desktop</string>
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
<key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
<key>CFBundleDevelopmentRegion</key><string>en</string>
<key>CFBundleLocalizations</key><array><string>en</string><string>zh-CN</string></array>
</dict></plist>
`
  );
  await signRelease({
    app,
    platform: "darwin",
    type: "distribution",
    optionsForFile: () => ({
      hardenedRuntime: true,
      entitlements: path.join(root, "native", "Resources", "entitlements.plist"),
    }),
  });
  verifySignature(app);
  assert.equal(run("/usr/bin/lipo", ["-archs", path.join(contents, "MacOS", "Whisper")]), "arm64");
  assert.equal(
    run("/usr/libexec/PlistBuddy", [
      "-c",
      "Print :LSMinimumSystemVersion",
      path.join(contents, "Info.plist"),
    ]),
    "27.0"
  );
  const linkedLibraries = run("/usr/bin/otool", ["-L", path.join(contents, "MacOS", "Whisper")]);
  assert.doesNotMatch(linkedLibraries, /Electron|Chromium|libnode|WebKit/);
  const files = fs.readdirSync(contents, { recursive: true }).join("\n");
  assert.doesNotMatch(files, /node_modules|\.asar$|Electron Framework|Chromium/m);
  console.log(
    "Built and verified dist/native-arm64/Whisper.app. The installed app was not changed."
  );
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
