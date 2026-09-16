const test = require("node:test");
const assert = require("node:assert/strict");
const { createDb } = require("./harness/db.js");
const loadStartup = () => import("../../src/helpers/dictionaryStartup.js");

test("setDictionary replaces the full dictionary (why a stale cache wipe was destructive)", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setDictionary(["OpenWhispr", "Alice", "Bob"]);
  assert.deepEqual(db.getDictionary(), ["OpenWhispr", "Alice", "Bob"]);

  // Writing only the cached subset is exactly what startup used to do.
  db.setDictionary(["OpenWhispr"]);
  assert.deepEqual(db.getDictionary(), ["OpenWhispr"]);
});

test("startup reconcile preserves DB words that a stale renderer cache omitted (#1295)", async (t) => {
  const db = createDb(t);
  if (!db) return;
  const { chooseDictionaryStartupAction } = await loadStartup();

  db.setDictionary(["OpenWhispr", "Alice", "Bob", "Imported Term"]);
  const staleCache = ["OpenWhispr"];
  const decision = chooseDictionaryStartupAction(db.getDictionary(), staleCache);
  assert.equal(decision.action, "pull-db-to-local");

  // Rewriting the reconciled list must preserve every stored word.
  db.setDictionary(decision.words);
  assert.deepEqual(db.getDictionary(), ["OpenWhispr", "Alice", "Bob", "Imported Term"]);
});

// applyDictionaryChanges is the delta write path. Its whole purpose is that a
// caller can only affect the words it names, so the wipe in #1295 stops being
// expressible rather than merely guarded against.

test("adding words leaves every other row untouched", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setDictionary(["OpenWhispr", "Alice", "Bob"]);
  const result = db.applyDictionaryChanges({ add: ["Carol"] });

  assert.equal(result.added, 1);
  assert.deepEqual(db.getDictionary(), ["OpenWhispr", "Alice", "Bob", "Carol"]);
});

test("a caller holding a stale one-word view cannot delete anything (#1295)", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setDictionary(["OpenWhispr", "Alice", "Bob", "Imported Term"]);
  // The exact call startup used to make, but expressed as a delta.
  db.applyDictionaryChanges({ add: ["OpenWhispr"] });

  assert.deepEqual(db.getDictionary(), ["OpenWhispr", "Alice", "Bob", "Imported Term"]);
});

test("a manual add promotes a learned word, matching setDictionary", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.applyDictionaryChanges({ add: ["Kubernetes"] }, "learned");
  assert.equal(
    db.db.prepare("SELECT source FROM custom_dictionary WHERE word = 'Kubernetes'").get().source,
    "learned"
  );

  db.applyDictionaryChanges({ add: ["Kubernetes"] }, "manual");
  assert.equal(
    db.db.prepare("SELECT source FROM custom_dictionary WHERE word = 'Kubernetes'").get().source,
    "manual"
  );
});

test("an edit is a remove plus an add in one transaction", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setDictionary(["OpenWhispr", "Kubernets", "Alice"]);
  db.applyDictionaryChanges({ add: ["Kubernetes"], remove: ["Kubernets"] });

  assert.deepEqual(db.getDictionary(), ["OpenWhispr", "Alice", "Kubernetes"]);
});

test("a word on both sides is kept, not deleted", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setDictionary(["OpenWhispr", "Alice"]);
  const result = db.applyDictionaryChanges({ add: ["Alice"], remove: ["Alice"] });

  assert.equal(result.removed, 0);
  assert.ok(db.getDictionary().includes("Alice"));
});

// Removing via setDictionary meant re-presenting every surviving word with the
// default 'manual' source, which promoted unrelated auto-learned words and
// marked them pending. A delta names only what it changes.
test("removing a word leaves other learned words untouched", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.applyDictionaryChanges({ add: ["Kubernetes", "Postgres"] }, "learned");
  const before = db.db
    .prepare("SELECT word, source FROM custom_dictionary WHERE word = 'Postgres'")
    .get();
  assert.equal(before.source, "learned");

  db.applyDictionaryChanges({ remove: ["Kubernetes"] });

  const after = db.db
    .prepare("SELECT word, source FROM custom_dictionary WHERE word = 'Postgres'")
    .get();
  assert.equal(after.source, "learned");
});

test("counts report words that changed, not words requested", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setDictionary(["OpenWhispr", "Alice"]);

  // Already present, so nothing was actually added.
  assert.equal(db.applyDictionaryChanges({ add: ["Alice"] }).added, 0);
  // Never present, so nothing was actually removed.
  assert.equal(db.applyDictionaryChanges({ remove: ["Nobody"] }).removed, 0);

  const mixed = db.applyDictionaryChanges({ add: ["Alice", "Carol"], remove: ["Nobody", "Alice"] });
  assert.equal(mixed.added, 1);
  assert.equal(mixed.removed, 0);
});

test("an empty delta touches nothing", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setDictionary(["OpenWhispr", "Alice"]);
  const synced = db.getPendingDictionary().find((row) => row.word === "Alice");
  db.markDictionaryEntrySynced(synced.id, "cloud-alice");

  const result = db.applyDictionaryChanges({});
  assert.deepEqual(result, { success: true, added: 0, removed: 0 });

  // Still synced: an empty delta must not mark rows pending again.
  const after = db.db.prepare("SELECT * FROM custom_dictionary WHERE word = 'Alice'").get();
  assert.equal(after.sync_status, "synced");
  assert.deepEqual(db.getDictionary(), ["OpenWhispr", "Alice"]);
});

test("removing a word that isn't there is a no-op", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setDictionary(["OpenWhispr"]);
  const result = db.applyDictionaryChanges({ remove: ["Nonexistent"] });
  assert.equal(result.removed, 0);
  assert.deepEqual(db.getDictionary(), ["OpenWhispr"]);
});

test("case-variant adds do not create duplicate rows", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setDictionary(["OpenWhispr", "Alice"]);
  db.applyDictionaryChanges({ add: ["alice", "ALICE"] });

  const rows = db.db
    .prepare("SELECT * FROM custom_dictionary WHERE lower(word) = 'alice' AND deleted_at IS NULL")
    .all();
  assert.equal(rows.length, 1);
});
