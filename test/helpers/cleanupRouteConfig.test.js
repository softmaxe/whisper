const test = require("node:test");
const assert = require("node:assert/strict");
const { loadAudioManager } = require("./harness/audioManager");

async function loadCleanupConfig(t, cleanupModel = "cleanup-model") {
  const { vite } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-cleanup-route-config-test-",
    settingsKey: "__cleanupRouteConfigSettings",
    mockModules: {
      "/stores/settingsStore": `
        export const getSettings = () => globalThis.__cleanupRouteConfigSettings;
        export const getEffectiveCleanupModel = () => ${JSON.stringify(cleanupModel)};
      `,
    },
  });
  return (await vite.ssrLoadModule("/helpers/audioManager.js")).resolveCleanupConfig;
}

test("cleanup pins temperature 0 and requires complete output", async (t) => {
  const resolveCleanupConfig = await loadCleanupConfig(t);

  const config = resolveCleanupConfig({ useCleanupModel: true, cleanupDisableThinking: true });

  assert.deepEqual(config, {
    inferenceScope: "dictationCleanup",
    disableThinking: true,
    temperature: 0,
    requireCompleteOutput: true,
  });
});

test("cleanup is skipped when it is disabled or has no model", async (t) => {
  const resolveCleanupConfig = await loadCleanupConfig(t, "  ");

  assert.equal(resolveCleanupConfig({ useCleanupModel: true }), null);
  assert.equal(resolveCleanupConfig({ useCleanupModel: false }), null);
});
