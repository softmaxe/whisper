const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

const options = { transcription: { getApiKey: () => "fixture-key" } };
const file = (name) => ({ name, path: `/tmp/${name}`, sizeBytes: 1024 });
const tick = () => new Promise((resolve) => setImmediate(resolve));

async function loadQueue(t, transcribe, save = async () => ({ success: true, id: 42 })) {
  const { window } = installBrowserGlobals(t);
  globalThis.__uploadTestTranscribe = transcribe;
  globalThis.__uploadTestSave = save;
  t.after(() => {
    delete globalThis.__uploadTestTranscribe;
    delete globalThis.__uploadTestSave;
  });
  const vite = await createRendererServer(t, {
    cachePrefix: "whisper-batch-queue-test-",
    mockModules: {
      "/services/fileTranscription":
        "export const transcribeFile = (...args) => globalThis.__uploadTestTranscribe(...args);",
      "/services/uploadNotes":
        "export const saveUploadTranscription = (...args) => globalThis.__uploadTestSave(...args);",
      "/components/notes/shared": "export const transcriptionErrorKey = () => null;",
    },
  });
  return { window, ...(await vite.ssrLoadModule("/stores/batchQueueStore.ts")) };
}

async function waitForQueueCompletion(store) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (!store.getState().isProcessing) return;
    await tick();
  }
  throw new Error("Batch queue did not finish");
}

test("cancelled batches discard late transcription and never save it", async (t) => {
  let finish;
  const saved = [];
  const queue = await loadQueue(
    t,
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
    async (text) => {
      saved.push(text);
      return { success: true, id: 42 };
    }
  );
  queue.addFiles([file("first.wav"), file("second.wav")]);
  queue.processBatchQueue(options);
  queue.cancelBatch();
  assert.equal(queue.useBatchQueueStore.getState().isProcessing, false);
  finish({ success: true, text: "Late transcript" });
  await tick();
  assert.deepEqual(saved, []);
  assert.ok(
    queue.useBatchQueueStore.getState().queue.every((item) => item.error === "batchCancelled")
  );
});

test("an old run cannot overwrite a new batch after clearing", async (t) => {
  let finish;
  let count = 0;
  const saved = [];
  const queue = await loadQueue(
    t,
    async () => {
      if (++count === 1)
        return new Promise((resolve) => {
          finish = resolve;
        });
      return { success: true, text: "Current transcript" };
    },
    async (text) => {
      saved.push(text);
      return { success: true, id: 8 };
    }
  );
  queue.addFiles([file("old.wav")]);
  queue.processBatchQueue(options);
  queue.clearBatchQueue();
  queue.addFiles([file("new.wav")]);
  queue.processBatchQueue(options);
  await waitForQueueCompletion(queue.useBatchQueueStore);
  finish({ success: true, text: "Stale transcript" });
  await tick();
  assert.deepEqual(saved, ["Current transcript"]);
  assert.equal(queue.useBatchQueueStore.getState().queue[0].name, "new.wav");
  assert.equal(queue.useBatchQueueStore.getState().queue[0].transcriptionId, 8);
});

test("transcription and save failures leave later queue items runnable", async (t) => {
  let count = 0;
  const queue = await loadQueue(
    t,
    async () => {
      const current = ++count;
      return current === 1
        ? { success: false, error: "Server unavailable" }
        : {
            success: true,
            text: `Transcript ${current}`,
            warning: current === 3 ? "partial" : undefined,
          };
    },
    async (text) => ({ success: text !== "Transcript 2", id: 9 })
  );
  queue.addFiles([file("first.wav"), file("second.wav"), file("third.wav")]);
  queue.processBatchQueue(options);
  await waitForQueueCompletion(queue.useBatchQueueStore);
  const items = queue.useBatchQueueStore.getState().queue;
  assert.deepEqual(
    items.map((item) => item.status),
    ["error", "error", "done"]
  );
  assert.equal(items[0].error, "Server unavailable");
  assert.equal(items[1].text, "Transcript 2", "failed saves remain copyable");
  assert.equal(items[1].transcriptionId, undefined);
  assert.equal(items[2].warning, true);
});

test("batch snapshots credentials and includes files added during the run", async (t) => {
  let finish;
  let key = "first-key";
  const seenKeys = [];
  const queue = await loadQueue(t, async (_path, config, diarize) => {
    assert.equal(diarize, false);
    seenKeys.push(config.getApiKey());
    if (seenKeys.length === 1)
      return new Promise((resolve) => {
        finish = resolve;
      });
    return { success: true, text: "Second transcript" };
  });
  queue.addFiles([file("first.wav")]);
  queue.processBatchQueue({ transcription: { getApiKey: () => key } });
  key = "changed-key";
  queue.addFiles([file("second.wav")]);
  finish({ success: true, text: "First transcript" });
  await waitForQueueCompletion(queue.useBatchQueueStore);
  assert.deepEqual(seenKeys, ["first-key", "first-key"]);
  assert.ok(queue.useBatchQueueStore.getState().queue.every((item) => item.status === "done"));
});
