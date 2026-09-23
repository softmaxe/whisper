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
const lock = require(path.join(root, "package-lock.json"));
const nodeMajor = fs.readFileSync(path.join(root, ".nvmrc"), "utf8").trim();

assert.equal(os.machine(), "arm64", "Whisper builds require an arm64 runner");
assert.equal(process.versions.node.split(".")[0], nodeMajor, "Node.js must match .nvmrc");
assert.match(pkg.version, /^[0-9]+\.[0-9]+\.[0-9]+$/);
assert.equal(lock.version, pkg.version);
assert.equal(lock.packages[""].version, pkg.version);

if (process.env.RELEASE_TAG) {
  assert.equal(process.env.RELEASE_TAG, "v" + pkg.version);
}

if (process.env.GITHUB_OUTPUT) {
  fs.appendFileSync(
    process.env.GITHUB_OUTPUT,
    "version=" + pkg.version + "\narchive=whisper-" + pkg.version + "-macos-arm64.zip\n"
  );
}
