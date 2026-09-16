const test = require("node:test");
const assert = require("node:assert/strict");
const { createDb } = require("./harness/db.js");

test("snippets trim, dedupe, preserve no-ops, and update replacements", (t) => {
  const db = createDb(t);
  if (!db) return;
  db.setSnippets([
    { trigger: "  signoff  ", replacement: "  Regards  " },
    { trigger: "SIGNOFF", replacement: "Ignored duplicate" },
  ]);

  assert.deepEqual(db.getSnippets(), [{ trigger: "signoff", replacement: "Regards" }]);
  db.setSnippets([{ trigger: "signoff", replacement: "Regards" }]);
  assert.deepEqual(db.getSnippets(), [{ trigger: "signoff", replacement: "Regards" }]);

  db.setSnippets([{ trigger: "signoff", replacement: "Best regards" }]);
  assert.deepEqual(db.getSnippets(), [{ trigger: "signoff", replacement: "Best regards" }]);
});

test("snippet removals delete local rows", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setSnippets([{ trigger: "temp", replacement: "Temporary" }]);
  db.setSnippets([]);
  assert.equal(db.db.prepare("SELECT COUNT(*) AS count FROM snippets").get().count, 0);

  assert.deepEqual(db.getSnippets(), []);
});

test("setSnippets drops triggers longer than the supported limit", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.setSnippets([
    { trigger: "x".repeat(101), replacement: "too long" },
    { trigger: "ok", replacement: "fine" },
  ]);

  assert.deepEqual(db.getSnippets(), [{ trigger: "ok", replacement: "fine" }]);
});
