const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer } = require("../lib/rendererTestHarness");

function textContent(node) {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string") return node;
  if (Array.isArray(node)) return node.map(textContent).join("");
  return textContent(node.props?.children);
}

function findNode(node, predicate) {
  if (!node) return null;
  if (Array.isArray(node)) {
    for (const child of node) {
      const match = findNode(child, predicate);
      if (match) return match;
    }
    return null;
  }
  if (predicate(node)) return node;
  return findNode(node.props?.children, predicate);
}

async function loadPromptStudio(t) {
  const state = {
    uiLanguage: "en",
    useCleanupModel: true,
    cleanupModel: "cleanup-model",
    cleanupRemoteUrl: "http://localhost:8080",
    cleanupDisableThinking: true,
    customPrompts: { cleanup: "Saved prompt" },
    preferredLanguage: "en",
    dictionary: ["Quokka"],
  };
  const harness = { state, calls: [], writes: [], values: [], cursor: 0, error: null };
  state.setCustomPrompt = (kind, value) => {
    harness.writes.push([kind, value]);
    state.customPrompts[kind] = value;
  };
  globalThis.__promptStudioCleanup = harness;
  t.after(() => delete globalThis.__promptStudioCleanup);

  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-prompt-studio-",
    noExternal: true,
    mockModules: {
      react: `
        export function useState(initial) {
          const h = globalThis.__promptStudioCleanup;
          const index = h.cursor++;
          if (!(index in h.values)) {
            h.values[index] = typeof initial === "function" ? initial() : initial;
          }
          return [h.values[index], (value) => { h.values[index] = value; }];
        }
      `,
      "react/jsx-dev-runtime": `
        export function jsxDEV(type, props, key) { return { type, props, key }; }
        export const jsx = jsxDEV;
        export const jsxs = jsxDEV;
      `,
      "react/jsx-runtime": `
        export function jsx(type, props, key) { return { type, props, key }; }
        export const jsxs = jsx;
      `,
      "react-i18next": `
        export function useTranslation() {
          return { t: (key, values) => values?.error ? key + ": " + values.error : key };
        }
      `,
      "zustand/react/shallow": "export const useShallow = (selector) => selector;",
      "/ui/button": "export function Button() {}",
      "./button": "export function Button() {}",
      "./textarea": "export function Textarea() {}",
      "./dialog": "export function AlertDialog() {}",
      "../icons": [
        "Eye",
        "Edit3",
        "Play",
        "Save",
        "RotateCcw",
        "Copy",
        "TestTube",
        "AlertTriangle",
        "Check",
      ]
        .map((name) => `export const ${name} = () => null;`)
        .join("\n"),
      "/hooks/useDialogs": `
        export function useDialogs() {
          return {
            alertDialog: { open: false, title: "", description: "" },
            showAlertDialog() {},
            hideAlertDialog() {},
          };
        }
      `,
      "/hooks/usePolicy": "export const usePolicySnapshot = () => ({});",
      "/utils/agentName": `export const useAgentName = () => ({ agentName: "Whisper" });`,
      "/services/ReasoningService": `
        export default {
          async processText(...args) {
            const h = globalThis.__promptStudioCleanup;
            h.calls.push(args);
            if (h.error) throw h.error;
            return "Cleaned transcript";
          },
        };
      `,
      "/utils/logger": "export default { error() {} };",
      "/stores/settingsStore": `
        export const useSettingsStore = (selector) => selector(globalThis.__promptStudioCleanup.state);
        useSettingsStore.getState = () => globalThis.__promptStudioCleanup.state;
        export const selectPolicyEffectiveSettings = (state) => state;
      `,
      "/utils/snippets": "export const getDictionaryHintWords = (state) => state.dictionary;",
      "/i18n": `
        export const normalizeUiLanguage = (value) => value;
        export default { getFixedT: () => (key, options) => options.defaultValue };
      `,
    },
  });
  const { default: PromptStudio } = await vite.ssrLoadModule("/components/ui/PromptStudio.tsx");
  const render = () => {
    harness.cursor = 0;
    return PromptStudio({});
  };
  const action = (label) => {
    const node = findNode(
      render(),
      (candidate) =>
        typeof candidate.props?.onClick === "function" && textContent(candidate) === label
    );
    assert.ok(node, `expected action ${label}`);
    return node.props;
  };
  const edit = (value) => {
    const field = findNode(render(), (candidate) => candidate.props?.rows === 16);
    assert.ok(field, "expected the prompt editor");
    field.props.onChange({ target: { value } });
  };
  return { ...harness, render, action, edit, harness, vite };
}

test("cleanup prompt tests use the configured server and unsaved prompt without changing settings", async (t) => {
  const { state, calls, writes, render, action, edit } = await loadPromptStudio(t);
  action("promptStudio.tabs.customize").onClick();
  edit("Clean this for {{agentName}}.");
  action("promptStudio.tabs.test").onClick();
  const preview = textContent(render());
  assert.ok(preview.includes(state.cleanupRemoteUrl));
  assert.ok(preview.includes(state.cleanupModel));

  await action("promptStudio.test.run").onClick();

  assert.equal(calls.length, 1);
  const [input, model, agentName, config] = calls[0];
  assert.match(
    input,
    /^<transcript>\n[\s\S]*<\/transcript>\n\nOutput only the cleaned transcript\.$/
  );
  assert.equal(model, state.cleanupModel);
  assert.equal(agentName, "Whisper");
  assert.equal(config.provider, "lan");
  assert.equal(config.lanUrl, state.cleanupRemoteUrl);
  assert.equal(config.inferenceScope, "dictationCleanup");
  assert.equal(config.disableThinking, true);
  assert.equal(config.temperature, 0);
  assert.equal(config.requireCompleteOutput, true);
  assert.match(config.systemPrompt, /^Clean this for Whisper\./);
  assert.match(config.systemPrompt, /English/);
  assert.match(config.systemPrompt, /Quokka/);
  assert.deepEqual(writes, []);
  assert.equal(state.customPrompts.cleanup, "Saved prompt");
  assert.ok(textContent(render()).includes("Cleaned transcript"));
});

test("cleanup prompt tests explain missing configuration and keep the draft after failures", async (t) => {
  const { state, calls, writes, render, action, harness } = await loadPromptStudio(t);
  action("promptStudio.tabs.test").onClick();

  for (const [field, value, message] of [
    ["useCleanupModel", false, "promptStudio.test.disabledReasoning"],
    ["cleanupRemoteUrl", " ", "promptStudio.test.noEndpoint"],
    ["cleanupModel", " ", "promptStudio.test.noModelSelected"],
  ]) {
    const previous = state[field];
    state[field] = value;
    await action("promptStudio.test.run").onClick();
    assert.ok(textContent(render()).includes(message));
    assert.equal(calls.length, 0);
    state[field] = previous;
  }

  harness.error = new Error("Server unavailable");
  await action("promptStudio.test.run").onClick();
  assert.ok(textContent(render()).includes("Server unavailable"));
  assert.equal(action("promptStudio.test.run").disabled, false);
  assert.deepEqual(writes, []);
  assert.equal(state.customPrompts.cleanup, "Saved prompt");
});

test("cleanup prompt save and reset preserve default prompt updates", async (t) => {
  const { state, writes, render, action, edit } = await loadPromptStudio(t);
  action("promptStudio.tabs.customize").onClick();
  edit("Custom cleanup instructions");
  action("promptStudio.common.save").onClick();
  assert.equal(state.customPrompts.cleanup, "Custom cleanup instructions");

  action("promptStudio.common.reset").onClick();
  const field = findNode(render(), (candidate) => candidate.props?.rows === 16);
  assert.notEqual(field.props.value, "Custom cleanup instructions");
  assert.ok(field.props.value.length > 0);
  assert.equal(state.customPrompts.cleanup, "");

  action("promptStudio.common.save").onClick();
  assert.deepEqual(writes, [
    ["cleanup", "Custom cleanup instructions"],
    ["cleanup", ""],
    ["cleanup", ""],
  ]);
});
