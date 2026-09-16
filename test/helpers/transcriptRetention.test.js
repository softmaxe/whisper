const test = require("node:test");
const assert = require("node:assert/strict");
const { createDb } = require("./harness/db.js");

function insert(db, text, ageDays) {
  const { lastInsertRowid } = db.db
    .prepare("INSERT INTO transcriptions (text, created_at) VALUES (?, datetime('now', ?))")
    .run(text, `-${ageDays} days`);
  return lastInsertRowid;
}

test("purges local transcriptions past the retention window and keeps the rest", (t) => {
  const db = createDb(t);
  if (!db) return;

  const stale = insert(db, "two days old", 2);
  const fresh = insert(db, "a few hours old", 0);

  const { ids } = db.deleteTranscriptionsExpiredBefore(1);

  assert.deepEqual(ids, [stale]);
  const remaining = db.db
    .prepare("SELECT id FROM transcriptions")
    .all()
    .map((r) => r.id);
  assert.deepEqual(remaining, [fresh]);
});

function insertAnalytics(db, eventId, ageDays) {
  db.recordAnalyticsEvent({
    eventId,
    wordCount: 4,
    occurredAt: "2026-08-30T10:00:00.000Z",
    localDate: "2026-08-30",
    spokenDurationMs: 2_000,
    mode: "self_hosted",
    provider: "self-hosted",
    model: "whisper-1",
  });
  // Backdated explicitly on created_at: that is the column the purge compares,
  // and it is the only one whose format matches the cutoff (see the purge).
  db.db
    .prepare("UPDATE analytics_events SET created_at = datetime('now', ?) WHERE event_id = ?")
    .run(`-${ageDays} days`, eventId);
}

test("retention purges analytics counters on the same schedule as transcripts", (t) => {
  const db = createDb(t);
  if (!db) return;

  insertAnalytics(db, "stale", 10);
  insertAnalytics(db, "fresh", 0);

  const { analyticsPurged } = db.deleteTranscriptionsExpiredBefore(7);

  assert.equal(analyticsPurged, 1);
  assert.deepEqual(
    db.db
      .prepare("SELECT event_id FROM analytics_events WHERE deleted_at IS NULL")
      .all()
      .map((row) => row.event_id),
    ["fresh"],
    "only counters inside the retention window remain"
  );
  assert.equal(db.getAnalyticsSummary().totalDictations, 1);
});
