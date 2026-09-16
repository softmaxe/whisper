const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const ipcHandlersSource = fs.readFileSync(
  path.join(__dirname, "../../src/helpers/ipcHandlers.js"),
  "utf8"
);

const preloadSource = fs.readFileSync(path.join(__dirname, "../../preload.js"), "utf8");

test("the fork does not expose cleanup that erases the original application's shared caches", () => {
  assert.doesNotMatch(ipcHandlersSource, /ipcMain\.handle\("cleanup-app"/);
  assert.doesNotMatch(preloadSource, /cleanupApp|invoke\("cleanup-app"/);
  assert.doesNotMatch(ipcHandlersSource, /path\.join\(os\.homedir\(\), "\.cache", "openwhispr"\)/);
});
