const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");

const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
} = require("../lib/rendererTestHarness");

test("cleanup toast localizes server recovery guidance and keeps fallback status quieter", async (t) => {
  let root = null;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
    delete globalThis.__cleanupFailureToasts;
  });
  globalThis.__cleanupFailureToasts = [];
  installBrowserGlobals(t);
  const container = installHookDom(t);
  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-cleanup-failure-toast-",
    mockModules: {
      "/ui/useToast": `
        export const useToast = () => ({
          toast: (props) => globalThis.__cleanupFailureToasts.push(props)
        });
      `,
      "/utils/windowContext": `
        export const isDictationPanelWindow = () => false;
      `,
    },
  });
  const { default: CleanupFailureToastListener } = await vite.ssrLoadModule(
    "/components/CleanupFailureToastListener.tsx"
  );
  const { default: i18n } = await vite.ssrLoadModule("/i18n.ts");
  await i18n.changeLanguage("es");
  const { recordCleanupFailure, useCleanupFailureStore } = await vite.ssrLoadModule(
    "/stores/cleanupFailureStore.ts"
  );
  useCleanupFailureStore.setState({ pending: 0, lastMessage: "", lastFailure: null });

  root = createRoot(container);
  await React.act(async () => root.render(React.createElement(CleanupFailureToastListener)));

  const failure = {
    message:
      "Endpoint rejected the request (401/403). Add an API key or adjust server auth settings.",
    messageKey: "reasoning.custom.endpointUnauthorized",
    action: "Optional. Sent as a Bearer token to your text cleanup server.",
    actionKey: "reasoning.custom.apiKeyHelp",
    copyCommand: "curl -I https://server.example/v1/models",
    technicalDetails: {
      status: 401,
      underlyingError: "Unauthorized",
    },
  };
  await React.act(async () => recordCleanupFailure(failure));

  assert.equal(globalThis.__cleanupFailureToasts.length, 1);
  assert.deepEqual(globalThis.__cleanupFailureToasts[0], {
    title:
      "El endpoint rechazó la solicitud (401/403). Agrega una clave API o ajusta la autenticación del servidor.",
    description: "Opcional. Se envía como token Bearer al servidor de corrección de texto.",
    secondaryDescription: "Tu dictado se pegó sin limpieza con IA.",
    copyCommand: "curl -I https://server.example/v1/models",
    technicalDetails: failure.technicalDetails,
    variant: "destructive",
    duration: 10_000,
  });
});
