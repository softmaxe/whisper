const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
} = require("../lib/rendererTestHarness");

// Just enough host-node surface for react-dom to commit the toast viewport.
function installHostNodes(document) {
  const node = (props) => ({
    ownerDocument: document,
    childNodes: [],
    style: { setProperty() {}, removeProperty() {} },
    setAttribute() {},
    removeAttribute() {},
    getAttribute: () => null,
    addEventListener() {},
    removeEventListener() {},
    appendChild(child) {
      this.childNodes.push(child);
      return child;
    },
    insertBefore(child) {
      this.childNodes.push(child);
      return child;
    },
    removeChild(child) {
      this.childNodes = this.childNodes.filter((item) => item !== child);
      return child;
    },
    ...props,
  });
  const element = (namespaceURI, tag) =>
    node({
      nodeType: 1,
      nodeName: tag.toUpperCase(),
      tagName: tag.toUpperCase(),
      namespaceURI,
    });
  document.createElement = (tag) => element("http://www.w3.org/1999/xhtml", tag);
  document.createElementNS = element;
  document.createTextNode = (text) => node({ nodeType: 3, nodeValue: text });
}

async function mountToastProvider(t) {
  let root;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });
  installBrowserGlobals(t, { window: { location: { search: "" } } });
  const container = installHookDom(t);
  installHostNodes(globalThis.document);
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-toast-presentation-test-",
    mockModules: {
      "react-i18next": "export const useTranslation = () => ({ t: (key) => key });",
      "/dictation/DictationErrorCard": "export const DictationErrorCard = () => null;",
    },
  });
  const { ToastProvider } = await vite.ssrLoadModule("/components/ui/Toast.tsx");
  const { useToast } = await vite.ssrLoadModule("/components/ui/useToast.ts");
  let context;
  function Probe() {
    context = useToast();
    return null;
  }
  await React.act(async () => {
    root = createRoot(container);
    root.render(React.createElement(ToastProvider, null, React.createElement(Probe)));
  });
  return {
    get context() {
      return context;
    },
    toast: (props) => React.act(async () => context.toast({ duration: 0, ...props })),
  };
}

test("a destructive notice stays a standard toast without the error surface", async (t) => {
  const provider = await mountToastProvider(t);
  await provider.toast({ title: "Microphone disconnected", variant: "destructive" });
  assert.equal(provider.context.toastCount, 1);
  assert.equal(provider.context.dictationErrorActionCount, 0);
});

test("only an explicit dictation-error presentation requests the error surface", async (t) => {
  const provider = await mountToastProvider(t);
  await provider.toast({ title: "Transcription failed", presentation: "dictation-error" });
  assert.equal(provider.context.dictationErrorActionCount, 1);
});
