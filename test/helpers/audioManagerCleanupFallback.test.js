const test = require("node:test");
const assert = require("node:assert/strict");
const { loadAudioManager } = require("./harness/audioManager");

test("cleanup failure details ride the raw result instead of notifying before paste", async (t) => {
  globalThis.__cleanupFallbackImmediateNotifications = [];
  t.after(() => delete globalThis.__cleanupFallbackImmediateNotifications);

  const { createManager } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-audio-cleanup-fallback-",
    settingsKey: "__audioCleanupFallbackSettings",
    settings: {
      useCleanupModel: true,
      cleanupProvider: "custom",
      cleanupMode: "self-hosted",
      cleanupDisableThinking: true,
      preferredLanguage: "en",
      cleanupRemoteUrl: "http://localhost:8000/v1",
    },
    mockModules: {
      "/stores/settingsStore": `
        export const getSettings = () => globalThis.__audioCleanupFallbackSettings;
        export const getEffectiveCleanupModel = () => "cleanup-model";
        export const useSettingsStore = { subscribe: () => () => {} };
      `,
      "/stores/cleanupFailureStore": `
        export const recordCleanupFailure = (failure) => {
          globalThis.__cleanupFallbackImmediateNotifications.push(failure);
        };
      `,
    },
  });

  const technicalDetails = {
    status: 503,
    underlyingError: "Cleanup server unavailable",
  };
  const failure = Object.assign(new Error("Cleanup server is temporarily unavailable."), {
    messageKey: "reasoning.custom.endpointError",
    action: "Check the cleanup server URL and status.",
    actionKey: "reasoning.custom.endpointHelp",
    copyCommand: "curl -I http://localhost:8000/v1/models",
    technicalDetails,
  });
  const manager = createManager({
    pendingCleanupFailure: null,
    isReasoningAvailable: async () => true,
    processWithReasoningModel: async () => {
      throw failure;
    },
  });

  const text = await manager.processTranscriptionCore("original dictation", "self-hosted");

  assert.equal(text, "original dictation");
  assert.deepEqual(globalThis.__cleanupFallbackImmediateNotifications, []);
  assert.deepEqual(manager._takePendingResultExtras(), {
    cleanupFailure: {
      message: failure.message,
      messageKey: failure.messageKey,
      action: failure.action,
      actionKey: failure.actionKey,
      copyCommand: failure.copyCommand,
      technicalDetails,
    },
  });
  assert.deepEqual(manager._takePendingResultExtras(), {});
});

test("safePaste returns false when the preload reports that no text was pasted", async (t) => {
  const { createManager, window } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-audio-cleanup-paste-outcome-",
    settingsKey: "__audioCleanupPasteOutcomeSettings",
  });
  const manager = createManager({
    onError: () => assert.fail("a resolved no-op is not a paste error"),
  });
  window.electronAPI.pasteText = async () => ({ success: true, pasted: false });

  assert.equal(await manager.safePaste("onboarding transcript"), false);
});

test("safePaste returns true only when the preload reports a completed paste", async (t) => {
  const { createManager, window } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-audio-cleanup-paste-success-",
    settingsKey: "__audioCleanupPasteSuccessSettings",
  });
  const manager = createManager({
    onError: () => assert.fail("a completed paste must not report an error"),
  });
  window.electronAPI.pasteText = async () => ({ success: true, pasted: true });

  assert.equal(await manager.safePaste("completed transcript"), true);
});

test("safePaste lets copy recovery handle a rejected paste without a competing error", async (t) => {
  const { createManager, window } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-audio-paste-copy-recovery-",
    settingsKey: "__audioPasteCopyRecoverySettings",
  });
  const manager = createManager({
    onError: () => assert.fail("manual-copy recovery owns the error presentation"),
  });
  window.electronAPI.pasteText = async (text, options) => {
    assert.equal(text, "recoverable transcript");
    assert.deepEqual(options, { restoreClipboard: true });
    throw new Error("paste bridge unavailable");
  };

  assert.equal(
    await manager.safePaste("recoverable transcript", {
      restoreClipboard: true,
      suppressError: true,
    }),
    false
  );
});
