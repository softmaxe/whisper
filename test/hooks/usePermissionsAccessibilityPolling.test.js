const assert = require("node:assert/strict");
const test = require("node:test");
const React = require("react");
const { createRoot } = require("react-dom/client");
const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
} = require("../lib/rendererTestHarness");

test("accessibility permission is re-checked on mount and polled until granted", async (t) => {
  let root = null;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });

  let accessibilityChecks = 0;
  installBrowserGlobals(t, {
    window: {
      electronAPI: {
        getPlatform: () => "darwin",
        checkAccessibilityPermission: async () => {
          accessibilityChecks += 1;
          return false;
        },
        checkMicrophoneAccess: async () => ({ granted: false }),
        checkPasteTools: async () => ({
          platform: "darwin",
          available: true,
          method: "cgevent",
          requiresPermission: true,
          tools: [],
        }),
      },
    },
  });
  const container = installHookDom(t);

  const originalSetInterval = globalThis.setInterval;
  const originalClearInterval = globalThis.clearInterval;
  const activeIntervals = new Set();
  let nextIntervalId = 1;
  globalThis.setInterval = () => {
    const id = nextIntervalId++;
    activeIntervals.add(id);
    return id;
  };
  globalThis.clearInterval = (id) => activeIntervals.delete(id);
  t.after(() => {
    globalThis.setInterval = originalSetInterval;
    globalThis.clearInterval = originalClearInterval;
  });

  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-permissions-accessibility-polling-",
    noExternal: ["react-i18next"],
    mockModules: {
      "react-i18next": `
        const t = (key) => key;
        export function useTranslation() { return { t }; }
      `,
    },
  });
  const { usePermissions } = await vite.ssrLoadModule("/hooks/usePermissions.ts");

  function Harness() {
    usePermissions();
    return null;
  }

  root = createRoot(container);
  await React.act(async () => {
    root.render(React.createElement(Harness));
    await Promise.resolve();
  });
  assert.equal(accessibilityChecks, 1);
  assert.equal(activeIntervals.size, 1);

  await React.act(async () => root.unmount());
  root = null;
  assert.equal(activeIntervals.size, 0);
});
