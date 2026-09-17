const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
} = require("../lib/rendererTestHarness");

function elements(node) {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!React.isValidElement(node)) return [];
  return [node, ...elements(node.props.children)];
}

test("empty History keeps an accessible toggle to reveal failed and discarded recordings", async (t) => {
  let root;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });
  installBrowserGlobals(t);
  const container = installHookDom(t);
  const vite = await createRendererServer(t, {
    mockModules: {
      "react-i18next":
        'export const useTranslation = () => ({ t: (key) => key, i18n: { language: "en" } });',
      "/stores/settingsStore":
        "export const useSettingsStore = (selector) => selector({ dataRetentionEnabled: true });",
      "/stores/policyStore":
        'export const usePolicyStore = (selector) => selector({ status: "unmanaged" });',
    },
  });
  const { default: HistoryView } = await vite.ssrLoadModule("/components/HistoryView.tsx");
  let tree;
  function Probe() {
    const [showDiscarded, setShowDiscarded] = React.useState(false);
    tree = HistoryView({
      history: [],
      isLoading: false,
      hotkey: "F8",
      showDiscarded,
      onToggleDiscarded: () => setShowDiscarded((value) => !value),
    });
    return null;
  }
  await React.act(async () => {
    root = createRoot(container);
    root.render(React.createElement(Probe));
  });
  const toggle = () => elements(tree).find((node) => node.type === "button");
  assert.ok(toggle(), "the empty state must offer a way to reveal hidden records");
  assert.equal(toggle().props["aria-pressed"], false);
  assert.ok(
    elements(toggle()).some((node) => node.props.children === "controlPanel.history.discarded.show")
  );
  await React.act(async () => toggle().props.onClick());
  assert.equal(toggle().props["aria-pressed"], true);
  assert.ok(
    elements(toggle()).some((node) => node.props.children === "controlPanel.history.discarded.hide")
  );
});
