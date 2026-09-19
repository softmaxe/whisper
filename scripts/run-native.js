const path = require("node:path");
const { spawnSync } = require("node:child_process");
const root = path.resolve(__dirname, "..");
const result = spawnSync(
  "/usr/bin/open",
  [
    "-n",
    path.join(root, "dist/native-development-arm64/Whisper.app"),
    "--args",
    "--profile",
    path.join(root, "native/.development-profile"),
  ],
  { stdio: "inherit" }
);
if (result.error) console.error("Could not launch the native development application.");
process.exitCode = result.status ?? 1;
