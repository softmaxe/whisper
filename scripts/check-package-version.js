#!/usr/bin/env node

// Validates the CI runner and package version shared by the CI, Build, and Release workflows.
// Set RELEASE_TAG to require a matching `v<version>` tag. When GITHUB_OUTPUT is set, the
// version and release archive name are written as step outputs.

const assert = require("node:assert/strict");
const fs = require("fs");
const os = require("os");
const path = require("path");

const root = path.join(__dirname, "..");
const pkg = require(path.join(root, "package.json"));
const miseConfig = fs.readFileSync(path.join(root, "mise.toml"), "utf8");
const nodeMajor = miseConfig.match(/^node\s*=\s*"(\d+)"\s*$/m)?.[1];

assert.ok(nodeMajor, "mise.toml must pin a Node.js major version");

assert.equal(os.machine(), "arm64", "Whisper builds require an arm64 runner");
assert.equal(process.versions.node.split(".")[0], nodeMajor, "Node.js must match mise.toml");
assert.match(pkg.version, /^[0-9]+\.[0-9]+\.[0-9]+$/);

if (process.env.RELEASE_TAG) {
  assert.equal(process.env.RELEASE_TAG, "v" + pkg.version);
}

if (process.env.GITHUB_OUTPUT) {
  fs.appendFileSync(
    process.env.GITHUB_OUTPUT,
    "version=" + pkg.version + "\narchive=whisper-" + pkg.version + "-macos-arm64.zip\n"
  );
}
