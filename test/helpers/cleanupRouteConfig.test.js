const test = require("node:test");
const assert = require("node:assert/strict");
const { loadAudioManager } = require("./harness/audioManager");

async function loadRouteResolver(t, provider = "custom", mode = "self-hosted") {
  const { vite } = await loadAudioManager(t, {
    cachePrefix: "openwhispr-cleanup-route-config-test-",
    settingsKey: "__cleanupRouteConfigSettings",
    mockModules: {
      "/stores/settingsStore": `
        export const getSettings = () => globalThis.__cleanupRouteConfigSettings;
        export const getEffectiveCleanupModel = () => "cleanup-model";
        export const selectResolvedLLMConfig = () => ({ model: "cleanup-model", provider: ${JSON.stringify(provider)}, mode: ${JSON.stringify(mode)} });
        export const isCloudCleanupMode = () => false;
        export const isCloudDictationAgentMode = () => false;
        export const isCloudTranslationMode = () => false;
      `,
      "/dictationAgentInference": `
        export const resolveDictationAgentInference = () => ({
          reachable: true,
          model: "agent-model",
          displayProvider: "test",
          config: { provider: "test" },
        });
        export const resolveDictationAgentVisionInference = () => ({
          active: false,
          model: "",
          config: {},
        });
      `,
      "/dictationTranslationInference": `
        export const resolveDictationTranslationInference = () => ({
          reachable: true,
          model: "translate-model",
          displayProvider: "test",
          config: { provider: "test" },
        });
      `,
      "/config/prompts": `
        export const resolvePrompt = () => "route prompt";
        export const appendScreenContextSuffix = (prompt) => prompt;
        export const wrapCleanupTranscript = (text) => text;
        export const getCleanupSystemPrompt = () => "cleanup prompt";
      `,
    },
  });
  const settings = { useCleanupModel: true, cleanupDisableThinking: true };
  const resolveReasoningRoute = (await vite.ssrLoadModule("/helpers/audioManager.js"))
    .resolveReasoningRoute;
  return (text, { voiceAgentRequested = false, translationRequested = false } = {}) =>
    resolveReasoningRoute(text, settings, "Jarvis", voiceAgentRequested, translationRequested);
}

test("the cleanup route pins temperature 0 and requires complete output", async (t) => {
  const resolveRoute = await loadRouteResolver(t);

  const route = resolveRoute("so um clean this up");

  assert.equal(route.kind, "cleanup");
  assert.equal(route.config.inferenceScope, "dictationCleanup");
  assert.equal(route.config.temperature, 0);
  assert.equal(route.config.requireCompleteOutput, true);
});
