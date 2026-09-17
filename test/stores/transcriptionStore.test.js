const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
const { createDb } = require("../helpers/harness/db");
const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
} = require("../lib/rendererTestHarness");

function insert(db, id, status, deleted = false) {
  db.db
    .prepare(
      `INSERT INTO transcriptions (id, text, status, timestamp, deleted_at, error_message)
       VALUES (?, ?, ?, ?, ?, ?)`
    )
    .run(
      id,
      status === "completed" ? `Transcript ${id}` : "",
      status,
      `2026-09-17T10:${String(id).padStart(2, "0")}:00.000Z`,
      deleted ? "2026-09-17T11:00:00.000Z" : null,
      status === "failed" ? "No text transcribed" : null
    );
  return db.getTranscriptionById(id);
}

const ids = (items) => items.map((item) => item.id);

async function mountHistoryStore(t, db) {
  let root;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
  });
  const { window } = installBrowserGlobals(t);
  const container = installHookDom(t);
  const events = {};
  window.electronAPI.getTranscriptions = async (...args) => db.getTranscriptions(...args);
  for (const event of ["Added", "Updated", "Deleted", "sCleared"]) {
    window.electronAPI[`onTranscription${event}`] = (callback) => {
      events[event] = callback;
      return () => {};
    };
  }
  const vite = await createRendererServer(t);
  const store = await vite.ssrLoadModule("/stores/transcriptionStore.ts");
  let snapshot;
  function Probe() {
    snapshot = {
      items: store.useTranscriptions(),
      includeDiscarded: store.useShowDiscarded(),
    };
    return null;
  }
  await React.act(async () => {
    root = createRoot(container);
    root.render(React.createElement(Probe));
    await store.initializeTranscriptions();
  });
  return {
    snapshot: () => snapshot,
    toggle: (includeDiscarded) =>
      React.act(async () => store.initializeTranscriptions(undefined, includeDiscarded)),
    emit: (event, item) => React.act(async () => events[event](item)),
  };
}

test("History filters failed and discarded rows before applying the limit", (t) => {
  const db = createDb(t);
  if (!db) return;
  insert(db, 1, "completed");
  insert(db, 2, "completed");
  insert(db, 3, "failed");
  insert(db, 4, "discarded");
  insert(db, 5, "pending");
  insert(db, 6, "failed", true);

  assert.deepEqual(ids(db.getTranscriptions(3)), [5, 2, 1]);
  assert.deepEqual(ids(db.getTranscriptions(3, { includeDiscarded: false })), [5, 2, 1]);
  assert.deepEqual(ids(db.getTranscriptions(10, { includeDiscarded: true })), [5, 4, 3, 2, 1]);
  assert.equal(db.getTranscriptionById(3).error_message, "No text transcribed");
});

test("History toggle controls saved rows and live additions without deleting errors", async (t) => {
  const db = createDb(t);
  if (!db) return;
  insert(db, 1, "completed");
  insert(db, 2, "failed");
  insert(db, 3, "discarded");
  const history = await mountHistoryStore(t, db);
  assert.deepEqual(ids(history.snapshot().items), [1]);

  await history.emit("Added", insert(db, 4, "failed"));
  await history.emit("Added", insert(db, 5, "discarded"));
  await history.emit("Added", insert(db, 6, "completed"));
  assert.deepEqual(ids(history.snapshot().items), [6, 1]);

  await history.toggle(true);
  assert.equal(history.snapshot().includeDiscarded, true);
  assert.deepEqual(ids(history.snapshot().items), [6, 5, 4, 3, 2, 1]);
  await history.emit("Added", insert(db, 7, "failed"));
  await history.emit("Added", insert(db, 8, "discarded"));
  assert.deepEqual(ids(history.snapshot().items), [8, 7, 6, 5, 4, 3, 2, 1]);

  await history.toggle(false);
  assert.equal(history.snapshot().includeDiscarded, false);
  assert.deepEqual(ids(history.snapshot().items), [6, 1]);
  assert.equal(db.getTranscriptions(50, { includeDiscarded: true }).length, 8);
});

test("live status changes respect the filter and recovered rows keep their recording order", async (t) => {
  const db = createDb(t);
  if (!db) return;
  insert(db, 1, "completed");
  insert(db, 2, "failed");
  insert(db, 3, "discarded");
  insert(db, 4, "completed");
  const history = await mountHistoryStore(t, db);

  db.updateTranscriptionStatus(4, "failed", "Server unavailable");
  await history.emit("Updated", db.getTranscriptionById(4));
  assert.deepEqual(ids(history.snapshot().items), [1]);

  for (const id of [2, 3]) {
    db.updateTranscriptionText(id, `Recovered ${id}`, null);
    db.updateTranscriptionStatus(id, "completed");
    await history.emit("Updated", db.getTranscriptionById(id));
  }
  assert.deepEqual(ids(history.snapshot().items), [3, 2, 1]);
  await history.emit("Updated", db.getTranscriptionById(2));
  assert.deepEqual(ids(history.snapshot().items), [3, 2, 1]);
  assert.equal(history.snapshot().items[1].text, "Recovered 2");

  await history.toggle(true);
  assert.deepEqual(ids(history.snapshot().items), [4, 3, 2, 1]);
  assert.equal(history.snapshot().items[0].error_message, "Server unavailable");
});
