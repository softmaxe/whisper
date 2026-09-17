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

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

async function mountPanel(
  t,
  { text, copyFallback = "copy", writeClipboard, onPreferredHeightChange } = {}
) {
  let root;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });
  const { window } = installBrowserGlobals(t);
  const container = installHookDom(t);
  const frames = new Map();
  let nextFrame = 1;
  globalThis.requestAnimationFrame = (callback) => {
    const id = nextFrame++;
    frames.set(id, callback);
    return id;
  };
  globalThis.cancelAnimationFrame = (id) => frames.delete(id);
  window.electronAPI.writeClipboard = writeClipboard ?? (async () => ({ success: true }));
  const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: {
      clipboard: {
        writeText: async () => {
          throw new Error("Clipboard unavailable");
        },
      },
    },
  });
  t.after(() => {
    if (originalNavigator) Object.defineProperty(globalThis, "navigator", originalNavigator);
    else delete globalThis.navigator;
  });
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-copy-recovery-test-",
    mockModules: {
      "react-i18next": "export const useTranslation = () => ({ t: (key) => key });",
    },
  });
  const { CopyRecoveryPanel } = await vite.ssrLoadModule(
    "/components/dictation/CopyRecoveryPanel.tsx"
  );
  let tree;
  let closeCount = 0;
  let props = {
    open: true,
    text,
    copyFallback,
    onClose: () => closeCount++,
    onPreferredHeightChange,
  };
  function Probe() {
    tree = CopyRecoveryPanel(props);
    return null;
  }
  await React.act(async () => {
    root = createRoot(container);
    root.render(React.createElement(Probe));
  });
  const find = (predicate) => elements(tree).find(predicate);
  const flushFrame = async () => {
    const currentFrames = [...frames];
    await React.act(async () => {
      for (const [id, callback] of currentFrames) {
        if (!frames.delete(id)) continue;
        callback();
      }
    });
  };
  const measure = async (height = 176) => {
    let pending;
    await React.act(async () => {
      pending = tree.props.onPreferredHeightChange(height, props.text);
    });
    return { pending };
  };
  return {
    find,
    measure,
    flushFrame,
    get shell() {
      return tree;
    },
    get closeCount() {
      return closeCount;
    },
    get copyButton() {
      return find((node) => node.type === "button" && !node.props["aria-label"]);
    },
    get pendingFrameCount() {
      return frames.size;
    },
    async reveal() {
      await measure();
      await flushFrame();
      await flushFrame();
    },
    async copy() {
      await React.act(async () => {
        find((node) => node.type === "button" && !node.props["aria-label"]).props.onClick();
      });
    },
    async rerender(nextProps) {
      props = { ...props, ...nextProps };
      await React.act(async () => root.render(React.createElement(Probe)));
    },
    async unmount() {
      await React.act(async () => root.unmount());
      root = null;
    },
  };
}

const transcript = `${"第一句应从顶部开始，可以选择和复制。 ".repeat(30)}\n\n  Keep spacing intact.  `;

function assertCopiedStatus(panel) {
  assert.equal(panel.copyButton, undefined, "copied text must not offer a redundant copy action");
  const footer = panel.find((node) => node.type === "footer");
  assert.equal(
    elements(footer).some((node) => node.type === "button"),
    false
  );
  assert.ok(panel.find((node) => node.props.children === "transcriptionPreview.recovery.copied"));
  assert.ok(
    panel.find((node) => node.props.children === "transcriptionPreview.recovery.pasteHint")
  );
}

test("already copied recovery keeps selectable text and paste guidance until explicitly closed", async (t) => {
  const writes = [];
  const panel = await mountPanel(t, {
    text: transcript,
    copyFallback: "copied",
    writeClipboard: async (text) => {
      writes.push(text);
      return { success: true };
    },
  });
  await panel.reveal();
  const region = panel.find((node) => node.props.role === "region");
  assert.equal(region.props.children.props.children, transcript);
  assert.equal(region.props.tabIndex, 0);
  assert.equal(region.props.children.props.dir, "auto");
  assert.match(region.props.children.props.className, /\bselect-text\b/);
  assert.equal(
    panel.find((node) => node.props.role === "status").props.children,
    "transcriptionPreview.recovery.pasteFailedCopied"
  );
  assertCopiedStatus(panel);
  assert.deepEqual(writes, []);
  assert.equal(panel.closeCount, 0);
  panel
    .find((node) => node.props["aria-label"] === "transcriptionPreview.recovery.close")
    .props.onClick();
  assert.equal(panel.closeCount, 1);
});

test("manual copy replaces its action with a persistent copied status and resets for the next result", async (t) => {
  const writes = [];
  const panel = await mountPanel(t, {
    text: transcript,
    writeClipboard: async (text) => {
      writes.push(text);
      return { success: writes.length === 1 };
    },
  });
  await panel.reveal();
  assert.ok(panel.copyButton);
  assert.equal(panel.copyButton.props.disabled, false);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  await panel.copy();
  assert.deepEqual(writes, [transcript]);
  assertCopiedStatus(panel);
  await React.act(async () => t.mock.timers.tick(3000));
  assertCopiedStatus(panel);
  assert.equal(panel.closeCount, 0);

  await panel.rerender({ text: "A new result requiring manual copy", copyFallback: "copy" });
  await panel.reveal();
  assert.ok(panel.copyButton, "a fresh result must not inherit the prior clipboard success");
  await panel.copy();
  assert.ok(panel.copyButton, "copy failure must leave its retry action available");
  assert.equal(panel.copyButton.props.disabled, false);
  assert.equal(
    panel.find((node) => node.props.role === "status").props.children,
    "transcriptionPreview.recovery.copyFailed"
  );
});

test("Escape closes recovery once and prevents a second document-level dismissal", async (t) => {
  const panel = await mountPanel(t, { text: transcript });
  await panel.reveal();
  const close = panel.find(
    (node) => node.props["aria-label"] === "transcriptionPreview.recovery.close"
  );
  assert.equal(close.props["aria-keyshortcuts"], "Escape");
  const handled = [];
  const event = {
    key: "Enter",
    preventDefault: () => handled.push("preventDefault"),
    stopPropagation: () => handled.push("stopPropagation"),
  };
  panel.shell.props.onKeyDown(event);
  assert.equal(panel.closeCount, 0);
  assert.deepEqual(handled, []);

  panel.shell.props.onKeyDown({ ...event, key: "Escape" });
  assert.equal(panel.closeCount, 1);
  assert.deepEqual(handled, ["preventDefault", "stopPropagation"]);
});

test("failed copy keeps the result readable and allows a successful retry", async (t) => {
  let attempt = 0;
  const panel = await mountPanel(t, {
    text: transcript,
    writeClipboard: async () => ({ success: ++attempt > 1 }),
  });
  await panel.reveal();
  assert.ok(panel.copyButton);
  await panel.copy();
  assert.equal(
    panel.find((node) => node.props.role === "status").props.children,
    "transcriptionPreview.recovery.copyFailed"
  );
  assert.equal(
    panel.find((node) => node.props.role === "region").props.children.props.children,
    transcript
  );
  assert.equal(panel.closeCount, 0);
  assert.ok(panel.copyButton);
  assert.equal(panel.copyButton.props.disabled, false);
  await panel.copy();
  assert.equal(attempt, 2);
  assert.equal(
    panel.find((node) => node.props.role === "status").props.children,
    "transcriptionPreview.recovery.pasteFailedCopied"
  );
  assertCopiedStatus(panel);
});

test("a new result starts at the top and clears the previous copy failure", async (t) => {
  const panel = await mountPanel(t, {
    text: transcript,
    writeClipboard: async () => ({ success: false }),
  });
  await panel.reveal();
  await panel.copy();
  const scrolling = { scrollTop: 160 };
  panel.find((node) => node.props.role === "region").props.ref.current = scrolling;
  await panel.rerender({ text: "A new transcript" });
  assert.equal(scrolling.scrollTop, 0);
  assertVisible(panel, false);
  assert.equal(
    panel.find((node) => node.props.role === "status").props.children,
    "transcriptionPreview.recovery.pasteFailed"
  );
  await panel.reveal();
  assertVisible(panel, true);
});

function assertVisible(panel, visible) {
  assert.equal(panel.shell.props.open, visible);
  const region = panel.find((node) => node.props.role === "region");
  assert.equal(region.props.tabIndex, visible ? 0 : -1);
  for (const button of elements(panel.shell).filter((node) => node.type === "button")) {
    assert.equal(button.props.disabled, !visible);
    assert.equal(button.props.tabIndex, visible ? 0 : -1);
  }
}

test("recovery stays hidden and noninteractive until measured native sizing and two frames settle", async (t) => {
  const resize = deferred();
  const measurements = [];
  const panel = await mountPanel(t, {
    text: transcript,
    onPreferredHeightChange: (height, revision) => {
      measurements.push({ height, revision });
      return resize.promise;
    },
  });

  assertVisible(panel, false);
  assert.equal(panel.shell.props.measureWhenClosed, true);
  await panel.flushFrame();
  await panel.flushFrame();
  assertVisible(panel, false);

  await panel.measure(420);
  assert.deepEqual(measurements, [{ height: 420, revision: transcript }]);
  await panel.flushFrame();
  await panel.flushFrame();
  assertVisible(panel, false);

  await React.act(async () => resize.resolve({ success: true }));
  assertVisible(panel, false);
  await panel.flushFrame();
  assertVisible(panel, false);
  await panel.flushFrame();
  assertVisible(panel, true);
});

test("closing while native sizing is pending prevents its completion from revealing recovery", async (t) => {
  const resize = deferred();
  const panel = await mountPanel(t, {
    text: transcript,
    onPreferredHeightChange: () => resize.promise,
  });
  await panel.measure();
  await panel.rerender({ open: false });
  await React.act(async () => resize.resolve({ success: true }));
  await panel.flushFrame();
  await panel.flushFrame();
  assertVisible(panel, false);
});

test("reopening the same result waits for a fresh measurement and ignores a prior resize", async (t) => {
  const previousResize = deferred();
  const reopenedResize = deferred();
  let measurements = 0;
  const panel = await mountPanel(t, {
    text: transcript,
    onPreferredHeightChange: () => {
      measurements += 1;
      if (measurements === 1) return Promise.resolve({ success: true });
      return measurements === 2 ? previousResize.promise : reopenedResize.promise;
    },
  });
  await panel.reveal();
  assertVisible(panel, true);
  await panel.measure(200);
  await panel.rerender({ open: false });
  await panel.rerender({ open: true });
  assertVisible(panel, false);
  await React.act(async () => previousResize.resolve({ success: true }));
  await panel.flushFrame();
  await panel.flushFrame();
  assertVisible(panel, false);

  await panel.measure();
  await React.act(async () => reopenedResize.resolve({ success: true }));
  await panel.flushFrame();
  await panel.flushFrame();
  assertVisible(panel, true);
});

test("a replacement result cancels a queued reveal and waits for its own measurement", async (t) => {
  const resizes = [deferred(), deferred()];
  let measurements = 0;
  const panel = await mountPanel(t, {
    text: transcript,
    onPreferredHeightChange: () => resizes[measurements++].promise,
  });
  await panel.measure();
  await React.act(async () => resizes[0].resolve({ success: true }));
  await panel.flushFrame();
  await panel.rerender({ text: "A different result needs different bounds" });
  await panel.flushFrame();
  await panel.flushFrame();
  assertVisible(panel, false);

  await panel.measure();
  await React.act(async () => resizes[1].resolve({ success: true }));
  await panel.flushFrame();
  assertVisible(panel, false);
  await panel.flushFrame();
  assertVisible(panel, true);
});

test("a failed resize still exposes the copyable result after the reveal frames", async (t) => {
  const resize = deferred();
  const writes = [];
  const panel = await mountPanel(t, {
    text: transcript,
    onPreferredHeightChange: () => resize.promise,
    writeClipboard: async (text) => {
      writes.push(text);
      return { success: true };
    },
  });
  await panel.measure();
  await React.act(async () => resize.reject(new Error("Window resize unavailable")));
  await panel.flushFrame();
  assertVisible(panel, false);
  await panel.flushFrame();
  assertVisible(panel, true);
  await panel.copy();
  assert.deepEqual(writes, [transcript]);
});

test("unmounting cancels an already queued recovery reveal", async (t) => {
  const panel = await mountPanel(t, { text: transcript });
  await panel.measure();
  await panel.flushFrame();
  assert.equal(panel.pendingFrameCount, 1);
  await panel.unmount();
  assert.equal(panel.pendingFrameCount, 0);
  await panel.flushFrame();
  assert.equal(panel.shell.props.open, false);
});
