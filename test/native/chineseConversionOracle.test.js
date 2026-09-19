const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { createHash } = require("node:crypto");
const { pathToFileURL } = require("node:url");

const root = path.resolve(__dirname, "../..");
const resource = path.join(root, "native/Sources/WhisperCore/Resources");
const dictionaries = path.join(resource, "OpenCC");
const fixtures = JSON.parse(
  fs.readFileSync(
    path.join(root, "native/Tests/WhisperCoreTests/Resources/ChineseOracle.json"),
    "utf8"
  )
);

test("native Chinese workflow fixtures match the pinned existing converter", async () => {
  const pkg = JSON.parse(
    fs.readFileSync(path.join(root, "node_modules/opencc-js/package.json"), "utf8")
  );
  assert.equal(pkg.version, fixtures.openccJSVersion);
  const { applyChineseScript, resolveChineseScriptTarget } =
    await import("../../src/utils/chineseScript.js");
  for (const fixture of fixtures.cases) {
    const target = resolveChineseScriptTarget(fixture.language, fixture.preference, fixture.input);
    assert.equal(await applyChineseScript(fixture.input, target), fixture.expected, fixture.name);
  }
});

test("vendored OpenCC source data matches all twelve installed dictionaries", async () => {
  const provenance = JSON.parse(
    fs.readFileSync(path.join(dictionaries, "provenance.json"), "utf8")
  );
  assert.equal(provenance.openccJSVersion, fixtures.openccJSVersion);
  assert.equal(Object.keys(provenance.files).length, 12);
  for (const [name, metadata] of Object.entries(provenance.files)) {
    const source = fs.readFileSync(path.join(dictionaries, name));
    assert.equal(createHash("sha256").update(source).digest("hex"), metadata.sha256, name);
    const entries = source
      .toString("utf8")
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line && !line.startsWith("#"))
      .map((line) => {
        const [key, values] = line.split("\t");
        return [key, values.split(" ")[0]];
      })
      .filter(([key, value]) => key !== value || key.length > 1)
      .map(([key, value]) => `${key} ${value}`)
      .join("|");
    const moduleURL = pathToFileURL(
      path.join(root, "node_modules/opencc-js/dist/esm-lib/dict", name.replace(/\.txt$/, ".js"))
    );
    const reference = await import(moduleURL.href);
    assert.equal(entries, reference.default, name);
  }
});

test("native language choices retain the existing supported registry", () => {
  assert.deepEqual(
    JSON.parse(fs.readFileSync(path.join(resource, "languageRegistry.json"), "utf8")),
    JSON.parse(fs.readFileSync(path.join(root, "src/config/languageRegistry.json"), "utf8"))
  );
});
