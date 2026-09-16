const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { createRendererServer } = require("../lib/rendererTestHarness");

const repoRoot = path.join(__dirname, "../..");

function source(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function sharedFieldInventory() {
  return Object.keys(EXPECTED_SHARED_FIELD_DIRECTIONS).flatMap((file) => {
    const absolutePath = path.join(repoRoot, file);
    const text = fs.readFileSync(absolutePath, "utf8");
    return [...text.matchAll(/<(Input|Textarea)\b[\s\S]*?\/>/g)].map((match) => ({
      file: path.relative(repoRoot, absolutePath).split(path.sep).join("/"),
      direction: match[0].match(/\bdir="([^"]+)"/)?.[1] ?? "inherit",
    }));
  });
}

function nativeFieldInventory() {
  return Object.keys(EXPECTED_NATIVE_FIELD_DIRECTIONS).flatMap((file) => {
    const absolutePath = path.join(repoRoot, file);
    const text = fs.readFileSync(absolutePath, "utf8");
    return [...text.matchAll(/<(input|textarea)\b[\s\S]*?\/>/g)].map((match) => ({
      file,
      direction: match[0].match(/\bdir="([^"]+)"/)?.[1] ?? "inherit",
    }));
  });
}

function inventoryByFile(inventory) {
  return inventory.reduce((result, field) => {
    (result[field.file] ??= []).push(field.direction);
    return result;
  }, {});
}

// Cover fields in the fork's dictation, History, vocabulary, and settings UI.
// Source order distinguishes multiple fields in the same component.
const EXPECTED_SHARED_FIELD_DIRECTIONS = {
  "src/components/DictionaryView.tsx": ["auto", "auto", "auto"],
  "src/components/OpenAICompatiblePanel.tsx": ["ltr"],
  "src/components/SelfHostedPanel.tsx": ["ltr", "ltr"],
  "src/components/SnippetsView.tsx": ["auto", "auto", "auto", "auto"],
  "src/components/ui/ApiKeyInput.tsx": ["ltr"],
  "src/components/ui/PromptStudio.tsx": ["auto", "auto"],
  "src/components/ui/SearchableModelList.tsx": ["ltr"],
};

const EXPECTED_NATIVE_FIELD_DIRECTIONS = {
  "src/components/CommandSearch.tsx": ["auto"],
  "src/components/ui/LanguageSelector.tsx": ["auto"],
};

test("shared field primitives inherit unless a consumer declares its content direction", async (t) => {
  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-field-direction-primitives-",
  });
  const { Input } = await vite.ssrLoadModule("/components/ui/input.tsx");
  const { Textarea } = await vite.ssrLoadModule("/components/ui/textarea.tsx");

  const inherited = renderToStaticMarkup(
    React.createElement(Input, { value: "مرحبا OpenWhispr 2.0", readOnly: true })
  );
  assert.doesNotMatch(inherited, /\sdir=/);
  assert.match(inherited, /value="مرحبا OpenWhispr 2.0"/);

  const technicalPassword = renderToStaticMarkup(
    React.createElement(Input, {
      dir: "ltr",
      type: "password",
      value: "سر-sk_ABC/123",
      readOnly: true,
    })
  );
  assert.match(technicalPassword, /\sdir="ltr"/);
  assert.match(technicalPassword, /value="سر-sk_ABC\/123"/);

  const prose = renderToStaticMarkup(
    React.createElement(Textarea, {
      dir: "auto",
      value: "مرحبا OpenWhispr 2.0",
      readOnly: true,
    })
  );
  assert.match(prose, /\sdir="auto"/);
  assert.match(prose, />مرحبا OpenWhispr 2.0<\/textarea>/);
});

test("supported text fields preserve their content direction", () => {
  const inventory = sharedFieldInventory();
  assert.deepEqual(inventoryByFile(inventory), EXPECTED_SHARED_FIELD_DIRECTIONS);
});

test("native text fields use the same reviewed direction policy", () => {
  const inventory = nativeFieldInventory();
  assert.deepEqual(inventoryByFile(inventory), EXPECTED_NATIVE_FIELD_DIRECTIONS);
});

test("dictionary, credentials, hotkeys, and errors keep their direction policy", () => {
  assert.match(
    source("src/components/DictionaryView.tsx"),
    /<Input\s+dir="auto"\s+ref=\{addInputRef\}/
  );
  assert.match(
    source("src/components/ui/ApiKeyInput.tsx"),
    /<span\s+dir="ltr"[\s\S]*?\{maskKey\(apiKey\)\}/
  );
  assert.match(
    source("src/components/ui/ApiKeyInput.tsx"),
    /<Input\s+dir="ltr"[\s\S]*?value=\{draft\}/
  );
  assert.match(source("src/components/ui/HotkeyInput.tsx"), /<div\s+dir="ltr"/);
  assert.match(source("src/components/ErrorBoundary.tsx"), /<pre\s+dir="ltr"/);
});
