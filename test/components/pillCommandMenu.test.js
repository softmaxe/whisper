const test = require("node:test");
const assert = require("node:assert/strict");
const { createElement } = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

// The harness renders i18n keys verbatim (no i18next instance is initialized),
// so assertions match on the raw translation key rather than resolved copy.
async function renderMenu(t, props) {
  installBrowserGlobals(t);
  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-pill-command-menu-test-",
  });
  const mod = await vite.ssrLoadModule("/components/dictation/PillCommandMenu.tsx");
  return renderToStaticMarkup(
    createElement(mod.PillCommandMenu, {
      buttonRef: { current: null },
      isRecording: false,
      isHovered: false,
      setWindowInteractivity: () => {},
      onToggleListening: () => {},
      onHide: () => {},
      onClose: () => {},
      ...props,
    })
  );
}

test("the command menu offers listening and hiding", async (t) => {
  const idleMarkup = await renderMenu(t, { isRecording: false });
  assert.match(idleMarkup, /startListening/);
  assert.match(idleMarkup, /hideForNow/);
  const recordingMarkup = await renderMenu(t, { isRecording: true });
  assert.match(recordingMarkup, /stopListening/);
});
