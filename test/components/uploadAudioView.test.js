const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
const { renderToStaticMarkup } = require("react-dom/server");
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
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

async function mountUpload(
  t,
  {
    transcribe = async () => ({ success: true, text: "Transcript" }),
    save = async () => ({ success: true, id: 42 }),
    copy = () => {},
  } = {}
) {
  let root;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });
  const { window } = installBrowserGlobals(t);
  const container = installHookDom(t);
  const cancelled = [];
  const historyVisits = [];
  window.electronAPI.selectAudioFile = async () => ({ filePaths: ["/tmp/design-review.m4a"] });
  window.electronAPI.getFileSize = async () => 1024;
  window.electronAPI.cancelUploadTranscription = async (requestId) => {
    cancelled.push(requestId);
    return { success: true };
  };
  window.__uploadViewTest = {
    transcribe,
    save,
    settings: {
      remoteTranscriptionUrl: "http://localhost:8178/v1",
      remoteTranscriptionModel: "Whisper-Large-v3-Turbo",
      customTranscriptionApiKey: "",
      preferredLanguage: "auto",
    },
    batch: { hasQueue: false, isProcessing: false },
  };
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-upload-view-test-",
    mockModules: {
      "react-i18next": "export const useTranslation = () => ({ t: (key) => key });",
      "/stores/settingsStore":
        "export const useSettingsStore = (selector) => selector(window.__uploadViewTest.settings);",
      "/stores/batchQueueStore":
        "export const useBatchQueue = () => window.__uploadViewTest.batch;",
      "/services/fileTranscription":
        "export const transcribeFile = (...args) => window.__uploadViewTest.transcribe(...args);",
      "/services/uploadNotes":
        "export const saveUploadTranscription = (...args) => window.__uploadViewTest.save(...args);",
      "./BatchQueueView": "export default function BatchQueueView() { return null; }",
      "./shared": "export const transcriptionErrorKey = () => null;",
    },
  });
  const { default: UploadAudioView } = await vite.ssrLoadModule(
    "/components/notes/UploadAudioView.tsx"
  );
  let tree;
  function Probe() {
    tree = UploadAudioView({
      onOpenHistory: () => historyVisits.push(true),
      onCopyText: copy,
    });
    return null;
  }
  await React.act(async () => {
    root = createRoot(container);
    root.render(React.createElement(Probe));
  });
  const current = (name) => elements(tree).find((node) => node.type.name === name);
  return {
    current,
    cancelled,
    historyVisits,
    async selectFile() {
      await React.act(async () => current("IdleView").props.handleBrowse());
    },
    async start() {
      let pending;
      await React.act(async () => {
        pending = current("SelectedView").props.handleTranscribe();
      });
      return { pending };
    },
    completion() {
      const view = current("CompleteView");
      return view ? view.type(view.props) : null;
    },
  };
}

test("a cancelled single upload ignores late results and never saves them", async (t) => {
  const response = deferred();
  const saved = [];
  const view = await mountUpload(t, {
    transcribe: () => response.promise,
    save: async (text) => {
      saved.push(text);
      return { success: true, id: 9 };
    },
  });
  await view.selectFile();
  const { pending } = await view.start();
  const progress = view.current("TranscribingView");
  const markup = renderToStaticMarkup(progress);
  assert.match(markup, /role="status"/);
  assert.match(markup, /design-review\.m4a/);
  assert.doesNotMatch(markup, /aria-valuenow|role="progressbar"/);
  await React.act(async () => progress.props.onCancel());
  assert.equal(view.cancelled.length, 1);
  assert.ok(view.current("IdleView"));
  await React.act(async () => {
    response.resolve({ success: true, text: "A late result" });
    await pending;
  });
  assert.deepEqual(saved, []);
  assert.equal(view.current("CompleteView"), undefined);
  assert.ok(view.current("IdleView"));
});

const fullText = `${"A long transcript that must remain readable and copyable. ".repeat(8)}\n\n最后一段，保留 API 和产品名称。`;

for (const scenario of [
  { name: "saved", save: async () => ({ success: true, id: 42 }), history: true },
  { name: "retention disabled", save: async () => ({ success: true, id: null }), history: false },
  { name: "save failed", save: async () => ({ success: false, id: null }), history: false },
]) {
  test(`a ${scenario.name} upload exposes the entire result and copies it without opening history`, async (t) => {
    const copies = [];
    const view = await mountUpload(t, {
      transcribe: async () => ({ success: true, text: fullText }),
      save: scenario.save,
      copy: (text) => copies.push(text),
    });
    await view.selectFile();
    const { pending } = await view.start();
    await React.act(async () => pending);
    const nodes = elements(view.completion());
    const transcript = nodes.find((node) => node.props.role === "region");
    assert.ok(transcript, "the complete transcript must have an accessible reading region");
    assert.equal(transcript.props.tabIndex, 0, "keyboard users can focus the scrolling text");
    assert.equal(transcript.props.children.props.children, fullText);
    assert.equal(transcript.props.children.props.dir, "auto");
    const copy = nodes.find((node) => node.props.children === "notes.upload.copyFullText");
    assert.ok(copy, "Copy is available regardless of history persistence");
    copy.props.onClick();
    assert.deepEqual(copies, [fullText]);
    assert.deepEqual(view.historyVisits, []);
    const history = nodes.find((node) => node.props.children === "notes.upload.viewInHistory");
    assert.equal(Boolean(history), scenario.history);
    assert.equal(
      nodes.some((node) => node.props.children === "notes.upload.savedToHistory"),
      scenario.history
    );
    if (history) {
      history.props.onClick();
      assert.deepEqual(view.historyVisits, [true]);
    }
  });
}

test("a failed transcription can be retried without losing the selected file", async (t) => {
  const paths = [];
  const view = await mountUpload(t, {
    transcribe: async (path) => {
      paths.push(path);
      return paths.length === 1
        ? { success: false, error: "Service unavailable" }
        : { success: true, text: "Retry succeeded" };
    },
  });
  await view.selectFile();
  const { pending } = await view.start();
  await React.act(async () => pending);
  assert.equal(view.current("ErrorView").props.error, "Service unavailable");
  await React.act(async () => view.current("ErrorView").props.onRetry());
  assert.deepEqual(paths, ["/tmp/design-review.m4a", "/tmp/design-review.m4a"]);
  assert.equal(view.current("CompleteView").props.result, "Retry succeeded");
});
