const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
const { createInstance } = require("i18next");
const translations = require("../../src/locales/en/translation.json");
const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
  installMicCaptureGlobals,
} = require("../lib/rendererTestHarness");

const builtIn = {
  kind: "audioinput",
  deviceId: "built-in",
  label: "MacBook Pro Microphone (Built-in)",
};
const iphone = { kind: "audioinput", deviceId: "iphone", label: "Rui's iPhone Microphone" };
const alias = { ...builtIn, deviceId: "default" };

function elements(node) {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!React.isValidElement(node)) return [];
  return [node, ...elements(node.props.children)];
}

async function mountSettings(
  t,
  { devices = [alias, builtIn, iphone], props = {}, getLid = async () => false } = {}
) {
  let root;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });
  const { window } = installBrowserGlobals(t);
  const container = installHookDom(t);
  installMicCaptureGlobals(t);
  let available = devices;
  let lidListener;
  let deviceListener;
  let lidUnsubscribed = false;
  const changes = [];
  navigator.mediaDevices.enumerateDevices = async () => available;
  navigator.mediaDevices.getUserMedia = async () => {
    throw new Error("Unexpected permission prompt");
  };
  navigator.mediaDevices.addEventListener = (event, callback) => {
    if (event === "devicechange") deviceListener = callback;
  };
  navigator.mediaDevices.removeEventListener = (event) => {
    if (event === "devicechange") deviceListener = null;
  };
  window.electronAPI.getSystemDefaultMicrophone = async () => ({
    name: builtIn.label,
    platform: "darwin",
    source: "system",
  });
  window.electronAPI.getLaptopLidState = getLid;
  window.electronAPI.onLaptopLidStateChanged = (callback) => {
    lidListener = callback;
    return () => {
      lidUnsubscribed = true;
    };
  };
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-microphone-ui-",
    mockModules: {},
  });
  const { MicrophoneSettings } = await vite.ssrLoadModule("/components/ui/MicrophoneSettings.tsx");
  const { I18nextProvider } = await import("react-i18next");
  const select = await vite.ssrLoadModule("/components/ui/select.tsx");
  let tree;
  const settingsProps = {
    microphoneSelectionMode: "auto",
    selectedMicDeviceId: "",
    selectedMicDeviceLabel: "",
    onSelectionModeChange: (mode) => changes.push(["mode", mode]),
    onDeviceSelect: (id, label) => changes.push(["device", id, label]),
    ...props,
  };
  function Probe() {
    tree = MicrophoneSettings(settingsProps);
    return null;
  }
  const i18n = createInstance();
  await i18n.init({
    lng: "en",
    interpolation: { escapeValue: false },
    resources: { en: { translation: translations } },
  });
  await React.act(async () => {
    root = createRoot(container);
    root.render(React.createElement(I18nextProvider, { i18n }, React.createElement(Probe)));
  });
  return {
    changes,
    input: () => elements(tree).find((node) => node.type === select.Select),
    options: () =>
      elements(elements(tree).find((node) => node.type === select.Select)).filter(
        (node) => node.type === select.SelectItem
      ),
    status: () =>
      elements(tree).find((node) => node.props["aria-live"] === "polite")?.props.children,
    async lidChanged(closed) {
      await React.act(async () => lidListener(closed));
    },
    async devicesChanged(next) {
      available = next;
      await React.act(async () => deviceListener());
    },
    async unmount() {
      await React.act(async () => root.unmount());
      root = null;
    },
    listenersRemoved: () => lidUnsubscribed && deviceListener === null,
  };
}

test("picker offers Auto and physical inputs, with one built-in label and manual selection", async (t) => {
  const view = await mountSettings(t);
  assert.deepEqual(
    view.options().map((option) => option.props.value),
    ["__auto__", "built-in", "iphone"]
  );
  assert.equal(view.options()[1].props.children, builtIn.label);
  assert.equal(view.input().props.value, "__auto__");
  view.input().props.onValueChange("iphone");
  view.input().props.onValueChange("__auto__");
  assert.deepEqual(view.changes, [
    ["device", "iphone", iphone.label],
    ["mode", "specific"],
    ["mode", "auto"],
  ]);
});

test("Auto display follows lid changes and a phone arriving while closed", async (t) => {
  const view = await mountSettings(t);
  assert.equal(view.status(), `Preferred: ${builtIn.label}`);
  await view.lidChanged(true);
  assert.equal(view.status(), `Preferred: ${iphone.label}`);
  await view.devicesChanged([alias, builtIn]);
  assert.equal(view.status(), `Preferred: ${builtIn.label}`);
  await view.devicesChanged([alias, builtIn, iphone]);
  assert.equal(view.status(), `Preferred: ${iphone.label}`);
  await view.lidChanged(false);
  assert.equal(view.status(), `Preferred: ${builtIn.label}`);
  await view.unmount();
  assert.equal(view.listenersRemoved(), true);
});

test("a stale initial lid query cannot replace a newer close event", async (t) => {
  let resolveLid;
  const view = await mountSettings(t, {
    getLid: () =>
      new Promise((resolve) => {
        resolveLid = resolve;
      }),
  });
  await view.lidChanged(true);
  await React.act(async () => resolveLid(false));
  assert.equal(view.status(), `Preferred: ${iphone.label}`);
});

test("legacy built-in mode selects the physical entry and preserves an unavailable selection", async (t) => {
  const view = await mountSettings(t, { props: { microphoneSelectionMode: "built-in" } });
  assert.equal(view.input().props.value, "built-in");
  await view.devicesChanged([iphone]);
  assert.equal(view.input().props.value, "__unavailable__");
  const unavailable = view.options().find((option) => option.props.value === "__unavailable__");
  assert.equal(unavailable.props.disabled, true);
  assert.equal(unavailable.props.children, "Built-in Microphone (Unavailable)");
});

test("manual phone selection stays fixed when the lid changes and remembers a disconnected phone", async (t) => {
  const view = await mountSettings(t, {
    props: {
      microphoneSelectionMode: "specific",
      selectedMicDeviceId: iphone.deviceId,
      selectedMicDeviceLabel: iphone.label,
    },
  });
  await view.lidChanged(true);
  await view.lidChanged(false);
  assert.equal(view.input().props.value, "iphone");
  assert.deepEqual(view.changes, []);
  await view.devicesChanged([alias, builtIn]);
  assert.equal(view.input().props.value, "__unavailable__");
  const unavailable = view.options().find((option) => option.props.value === "__unavailable__");
  assert.equal(unavailable.props.children, `${iphone.label} (Unavailable)`);
  assert.equal(unavailable.props.disabled, true);
});

test("device refresh cannot replace a missing selection solely by matching its label", async (t) => {
  const view = await mountSettings(t, {
    props: {
      microphoneSelectionMode: "specific",
      selectedMicDeviceId: iphone.deviceId,
      selectedMicDeviceLabel: iphone.label,
    },
  });
  await view.devicesChanged([builtIn, { ...iphone, deviceId: "different-phone" }]);
  assert.equal(view.input().props.value, "__unavailable__");
  assert.deepEqual(view.changes, []);
  view.input().props.onValueChange("different-phone");
  assert.deepEqual(view.changes, [
    ["device", "different-phone", iphone.label],
    ["mode", "specific"],
  ]);
});
