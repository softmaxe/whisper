const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
} = require("../lib/rendererTestHarness");

const FAKE_AUDIO_MANAGER_SOURCE = `
export default class FakeAudioManager {
  constructor() {
    globalThis.__clipboardPersistenceAudioManager = this;
  }
  getState() {
    return {};
  }
  setCallbacks(callbacks) {
    this.callbacks = callbacks;
  }
  complete(result) {
    return this.callbacks.onTranscriptionComplete(result);
  }
  saveTranscription(...args) {
    return globalThis.__saveClipboardPersistenceTranscription(...args);
  }
  safePaste(...args) {
    return globalThis.__clipboardPersistencePaste(...args);
  }
  cleanup() {}
}
`;

const SETTINGS_STORE_SOURCE = `
export const getSettings = () => globalThis.__clipboardPersistenceSettings;
`;

const POLICY_STORE_SOURCE = `
export const usePolicyStore = {
  getState: () => ({}),
  subscribe: () => () => {},
};
`;

const LOGGER_SOURCE = `
const record = (level, message, meta, scope) => {
  globalThis.__clipboardPersistenceLogs.push({ level, message, meta, scope });
};
export default {
  trace: (message, meta, scope) => record("trace", message, meta, scope),
  debug: (message, meta, scope) => record("debug", message, meta, scope),
  info: (message, meta, scope) => record("info", message, meta, scope),
  warn: (message, meta, scope) => record("warn", message, meta, scope),
  error: (message, meta, scope) => record("error", message, meta, scope),
  fatal: (message, meta, scope) => record("fatal", message, meta, scope),
  logReasoning: (message, meta) => record("debug", message, meta, "reasoning"),
};
`;

const TRANSLATION_SOURCE = `
const translate = (key) => key;
export const useTranslation = () => ({ t: translate });
`;

const NOOP = () => {};

async function mountCompletionHarness(
  t,
  {
    settings,
    writeClipboard = async () => ({ success: true }),
    replaceSelectedText = async () => ({ success: true }),
    saveTranscription = async () => true,
    safePaste = async () => true,
    onboardingCompleted = true,
  }
) {
  let root = null;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });

  const navigatorWrites = [];
  const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", {
    value: {
      clipboard: {
        async writeText(text) {
          navigatorWrites.push(text);
          throw new Error("renderer clipboard should not be used");
        },
      },
    },
    configurable: true,
    writable: true,
  });
  t.after(() => {
    if (originalNavigator) Object.defineProperty(globalThis, "navigator", originalNavigator);
    else delete globalThis.navigator;
  });

  const bridgeWrites = [];
  const saves = [];
  const pastes = [];
  const recoveryPanels = [];
  const toasts = [];
  let hiddenPreviews = 0;
  let prepareDictation;
  const noopDispose = () => () => {};
  installBrowserGlobals(t, {
    initialStorage: { onboardingCompleted: String(onboardingCompleted) },
    window: {
      electronAPI: {
        onToggleDictation: noopDispose,
        onToggleVoiceAgent: noopDispose,
        onToggleTranslation: noopDispose,
        onStartDictation: noopDispose,
        onPrepareDictation: (listener) => {
          prepareDictation = listener;
          return NOOP;
        },
        onCancelDictationPreparation: noopDispose,
        onStopDictation: noopDispose,
        dictationLifecycleStateChanged: NOOP,
        completeDictationPreview: NOOP,
        hideDictationPreview: () => hiddenPreviews++,
        setScreenContextEnabled: NOOP,
        async writeClipboard(text) {
          bridgeWrites.push(text);
          return writeClipboard(text);
        },
        replaceSelectedText,
      },
    },
  });
  const container = installHookDom(t);

  globalThis.__clipboardPersistenceSettings = {
    autoPasteEnabled: false,
    keepTranscriptionInClipboard: true,
    showTranscriptionPreview: false,
    snippets: [],
    useLocalWhisper: false,
    pauseMediaOnDictation: false,
    ...settings,
  };
  globalThis.__clipboardPersistenceLogs = [];
  globalThis.__saveClipboardPersistenceTranscription = async (...args) => {
    saves.push(args);
    return saveTranscription(...args);
  };
  globalThis.__clipboardPersistencePaste = async (...args) => {
    pastes.push(args);
    return safePaste(...args);
  };
  t.after(() => {
    delete globalThis.__clipboardPersistenceAudioManager;
    delete globalThis.__clipboardPersistenceSettings;
    delete globalThis.__clipboardPersistenceLogs;
    delete globalThis.__saveClipboardPersistenceTranscription;
    delete globalThis.__clipboardPersistencePaste;
  });

  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-audio-recording-clipboard-persistence-",
    noExternal: ["react-i18next"],
    mockModules: {
      "/helpers/audioManager": FAKE_AUDIO_MANAGER_SOURCE,
      "/stores/settingsStore": SETTINGS_STORE_SOURCE,
      "/stores/policyStore": POLICY_STORE_SOURCE,
      "/utils/logger": LOGGER_SOURCE,
      "/utils/visualFrame": `export const waitForVisualFrames = async () => {};`,
      "react-i18next": TRANSLATION_SOURCE,
    },
  });
  const { useAudioRecording } = await vite.ssrLoadModule("/hooks/useAudioRecording.js");

  function Harness() {
    useAudioRecording((toast) => toasts.push(toast), {
      onDemoEvent: NOOP,
      onShowTranscript: (text, options) => recoveryPanels.push({ text, options }),
    });
    return null;
  }

  root = createRoot(container);
  await React.act(async () => {
    root.render(React.createElement(Harness));
  });

  return {
    bridgeWrites,
    complete: async (result) => {
      await React.act(async () => {
        await globalThis.__clipboardPersistenceAudioManager.complete(result);
      });
    },
    logs: globalThis.__clipboardPersistenceLogs,
    navigatorWrites,
    pastes,
    recoveryPanels,
    toasts,
    getHiddenPreviews: () => hiddenPreviews,
    prepareDictation: () => prepareDictation(),
    saves,
  };
}

test("clipboard-only rejection cannot cancel non-preview transcription persistence", async (t) => {
  const harness = await mountCompletionHarness(t, {
    writeClipboard: async () => {
      throw new Error("main-process clipboard rejected");
    },
  });

  await harness.complete({
    success: true,
    text: "Final non-preview text",
    rawText: "Raw non-preview text",
    clientTranscriptionId: "client-non-preview",
    source: "openai",
  });

  assert.deepEqual(harness.saves, [
    [
      "Final non-preview text",
      "Raw non-preview text",
      { clientTranscriptionId: "client-non-preview" },
    ],
  ]);
  assert.ok(
    harness.logs.some(
      ({ level, message, meta, scope }) =>
        level === "warn" &&
        message === "Failed to keep transcription in clipboard" &&
        meta.delivery === "clipboard-only" &&
        scope === "clipboard"
    )
  );
  assert.deepEqual(harness.navigatorWrites, []);
});

test("clipboard-only unsuccessful bridge response is logged without cancelling persistence", async (t) => {
  const harness = await mountCompletionHarness(t, {
    writeClipboard: async () => ({ success: false }),
  });

  await harness.complete({
    success: true,
    text: "Text with an unsuccessful clipboard response",
    rawText: "Raw text with an unsuccessful clipboard response",
    clientTranscriptionId: "client-clipboard-unsuccessful",
    source: "openai",
  });

  assert.deepEqual(harness.saves, [
    [
      "Text with an unsuccessful clipboard response",
      "Raw text with an unsuccessful clipboard response",
      { clientTranscriptionId: "client-clipboard-unsuccessful" },
    ],
  ]);
  assert.ok(
    harness.logs.some(
      ({ level, message, meta, scope }) =>
        level === "warn" &&
        message === "Failed to keep transcription in clipboard" &&
        meta.delivery === "clipboard-only" &&
        scope === "clipboard"
    )
  );
  assert.deepEqual(harness.navigatorWrites, []);
});

test("clipboard-only delivery uses the main-process bridge", async (t) => {
  const harness = await mountCompletionHarness(t, {
    settings: { showTranscriptionPreview: true },
  });

  await harness.complete({
    success: true,
    text: "Final streaming text",
    rawText: "Raw streaming text",
    clientTranscriptionId: "client-streaming",
    source: "self-hosted",
  });

  assert.deepEqual(harness.bridgeWrites, ["Final streaming text"]);
  assert.deepEqual(harness.saves, [
    ["Final streaming text", "Raw streaming text", { clientTranscriptionId: "client-streaming" }],
  ]);
  assert.deepEqual(harness.navigatorWrites, []);
});

test("a false persistence result is logged instead of silently ignored", async (t) => {
  const harness = await mountCompletionHarness(t, {
    saveTranscription: async () => false,
  });

  await harness.complete({
    success: true,
    text: "Text with a storage failure",
    rawText: "Raw text with a storage failure",
    clientTranscriptionId: "client-storage-failure",
    source: "openai",
  });

  assert.equal(harness.saves.length, 1);
  assert.ok(
    harness.logs.some(
      ({ level, message, scope }) =>
        level === "error" && message === "Failed to persist transcription" && scope === "audio"
    )
  );
  assert.deepEqual(harness.navigatorWrites, []);
});

test("the Insights timestamp rides along on the persistence call", async (t) => {
  // Persistence moved ahead of the paste so history survives a failed
  // clipboard delivery (#1979). The analytics timestamp has to travel with it:
  // recordAnalyticsEvent reads it from these options, and losing it here would
  // silently backdate every counter to the write time instead of the moment
  // the recording started.
  const harness = await mountCompletionHarness(t, {});

  await harness.complete({
    success: true,
    text: "Final text",
    rawText: "Raw text",
    clientTranscriptionId: "client-analytics",
    analyticsOccurredAt: "2026-09-04T09:00:00.000Z",
    source: "openai",
  });

  assert.deepEqual(harness.saves, [
    [
      "Final text",
      "Raw text",
      {
        clientTranscriptionId: "client-analytics",
        analyticsOccurredAt: "2026-09-04T09:00:00.000Z",
      },
    ],
  ]);
});

test("a failed dictation paste exposes the final expanded text with previews and history disabled", async (t) => {
  const harness = await mountCompletionHarness(t, {
    settings: {
      autoPasteEnabled: true,
      keepTranscriptionInClipboard: false,
      showTranscriptionPreview: false,
      dataRetentionEnabled: false,
      snippets: [{ trigger: "signature", replacement: "Kind regards, Alex" }],
    },
    safePaste: async () => false,
  });

  await harness.complete({
    success: true,
    text: "Thanks. signature",
    rawText: "thanks um signature",
    clientTranscriptionId: "no-editable-target",
    source: "openai",
  });

  assert.deepEqual(harness.recoveryPanels, [
    { text: "Thanks. Kind regards, Alex", options: { copyFallback: "copied" } },
  ]);
  assert.deepEqual(harness.bridgeWrites, ["Thanks. Kind regards, Alex"]);
  assert.deepEqual(harness.saves, [
    [
      "Thanks. Kind regards, Alex",
      "thanks um signature",
      { clientTranscriptionId: "no-editable-target" },
    ],
  ]);
  assert.equal(harness.pastes[0][1].suppressError, true);
  assert.equal(harness.getHiddenPreviews(), 0);
  assert.deepEqual(harness.toasts, []);
});

for (const [failure, safePaste] of [
  ["returns false", async () => false],
  [
    "throws",
    async () => {
      throw new Error("paste bridge unavailable");
    },
  ],
]) {
  for (const [clipboardFailure, writeClipboard] of [
    ["returns failure", async () => ({ success: false })],
    [
      "throws",
      async () => {
        throw new Error("clipboard unavailable");
      },
    ],
  ]) {
    test(`paste recovery survives when paste ${failure} and clipboard ${clipboardFailure}`, async (t) => {
      const harness = await mountCompletionHarness(t, {
        settings: { autoPasteEnabled: true, keepTranscriptionInClipboard: false },
        safePaste,
        writeClipboard,
      });

      await harness.complete({
        success: true,
        text: "Final recoverable text",
        rawText: "raw recoverable text",
        source: "self-hosted",
      });

      assert.deepEqual(harness.recoveryPanels, [
        { text: "Final recoverable text", options: { copyFallback: "copy" } },
      ]);
      assert.equal(harness.saves.length, 1);
      assert.deepEqual(harness.saves[0].slice(0, 2), [
        "Final recoverable text",
        "raw recoverable text",
      ]);
      assert.deepEqual(harness.toasts, []);
    });
  }
}

test("a successful paste closes the preview without opening copy recovery", async (t) => {
  const harness = await mountCompletionHarness(t, {
    settings: { autoPasteEnabled: true },
  });

  await harness.complete({ success: true, text: "Delivered text", source: "openai" });

  assert.deepEqual(harness.recoveryPanels, []);
  assert.deepEqual(harness.bridgeWrites, []);
  assert.equal(harness.getHiddenPreviews(), 1);
});

test("an onboarding demo does not open copy recovery for its intentionally skipped paste", async (t) => {
  const harness = await mountCompletionHarness(t, {
    settings: { autoPasteEnabled: true },
    safePaste: async () => false,
    onboardingCompleted: false,
  });

  await harness.complete({ success: true, text: "Demo text", source: "openai" });

  assert.deepEqual(harness.recoveryPanels, []);
  assert.deepEqual(harness.bridgeWrites, []);
});

test("a late clipboard write cannot reopen recovery after the next recording starts preparing", async (t) => {
  const harness = await mountCompletionHarness(t, {
    settings: { autoPasteEnabled: true },
    safePaste: async () => false,
    writeClipboard: async () => {
      await harness.prepareDictation();
      return { success: true };
    },
  });

  await harness.complete({ success: true, text: "Previous result", source: "openai" });

  assert.deepEqual(harness.recoveryPanels, []);
  assert.equal(harness.saves.length, 1);
});
