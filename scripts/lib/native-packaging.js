const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { isSignableCode } = require("./macos-signing");

function inside(root, relative) {
  assert.equal(typeof relative, "string", "Package paths must be strings.");
  assert.ok(relative && !path.isAbsolute(relative), "Package paths must be relative.");
  const resolved = path.resolve(root, relative);
  const local = path.relative(root, resolved);
  assert.ok(
    local && !local.startsWith("..") && !path.isAbsolute(local),
    "Package paths must stay inside their root."
  );
  return resolved;
}

function copyEntry(root, destination, entry) {
  const source = inside(root, entry.from);
  const target = inside(destination, entry.to);
  assert.ok(!fs.existsSync(target), `Duplicate package destination: ${entry.to}`);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.cpSync(source, target, { recursive: true, dereference: false, verbatimSymlinks: true });
}

function copyPackageResources(root, destination, manifest) {
  for (const entry of manifest.resources || []) copyEntry(root, destination, entry);
  for (const helper of manifest.helpers || []) {
    assert.ok(
      helper.to.startsWith("bin/"),
      "Native helpers retain their Resources/bin identity path."
    );
    assert.ok(helper.licenses?.length, "Every helper must include its redistribution license.");
    copyEntry(root, destination, helper);
    for (const license of helper.licenses) copyEntry(root, destination, license);
  }
}

function runtimeResourceBundles(entries, packageDescription) {
  const tests = new Set();
  for (const target of packageDescription.targets) {
    if (target.type !== "test") continue;
    for (const name of [target.name, target.c99name].filter(Boolean)) {
      tests.add(`${packageDescription.name}_${name}.bundle`);
    }
  }
  return entries.filter((name) => name.endsWith(".bundle") && !tests.has(name));
}

function packageCode(app) {
  const canonicalApp = fs.realpathSync(app);
  const code = new Set();
  const walked = new Set();
  function visit(file) {
    const canonical = fs.realpathSync(file);
    const relative = path.relative(canonicalApp, canonical);
    assert.ok(
      !relative.startsWith("..") && !path.isAbsolute(relative),
      "Package symlinks cannot leave the app."
    );
    if (walked.has(canonical)) return;
    walked.add(canonical);
    if (isSignableCode(canonical)) code.add(canonical);
    if (fs.statSync(canonical).isDirectory()) {
      for (const name of fs.readdirSync(canonical)) visit(path.join(canonical, name));
    }
  }
  visit(app);
  return [...code].sort((a, b) => b.split(path.sep).length - a.split(path.sep).length);
}

function assertNativeFiles(app) {
  const files = fs.readdirSync(app, { recursive: true }).join("\n");
  assert.doesNotMatch(
    files,
    /(?:^|\/)node_modules(?:\/|$)|\.asar$|Electron Framework|Chromium|(?:^|\/)node(?:\.exe)?$/im,
    "A native package cannot contain a Web or Node.js application runtime."
  );
}

// Verified artifacts replace their exact destinations together; unrelated dist baselines stay untouched.
function publishArtifacts(entries, backupDirectory) {
  fs.mkdirSync(backupDirectory, { recursive: true });
  const changes = [];
  try {
    for (const [index, { source, destination }] of entries.entries()) {
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      const change = {
        destination,
        backup: path.join(backupDirectory, String(index)),
        replaced: false,
        installed: false,
      };
      changes.push(change);
      if (fs.existsSync(destination)) {
        fs.renameSync(destination, change.backup);
        change.replaced = true;
      }
      fs.renameSync(source, destination);
      change.installed = true;
    }
  } catch (error) {
    for (const change of changes.reverse()) {
      if (change.installed) fs.rmSync(change.destination, { recursive: true, force: true });
      if (change.replaced) fs.renameSync(change.backup, change.destination);
    }
    throw error;
  }
}

module.exports = {
  inside,
  copyPackageResources,
  runtimeResourceBundles,
  packageCode,
  assertNativeFiles,
  publishArtifacts,
};
