const test = require("node:test");
const assert = require("node:assert/strict");

const { createDb } = require("./harness/db.js");
const DatabaseManager = require("../../src/helpers/database.js");
const { ANALYTICS_HISTORY_BACKFILL_VERSION } = require("../../src/helpers/analytics.js");

function reconcileThroughCheckpoint(db, limit = 250) {
  const state = db.getAnalyticsHistoryBackfillState(ANALYTICS_HISTORY_BACKFILL_VERSION);
  const batches = [];
  do {
    batches.push(
      db.backfillAnalyticsHistoryBatch({
        throughId: state.targetId,
        checkpointVersion: ANALYTICS_HISTORY_BACKFILL_VERSION,
        limit,
      })
    );
  } while (!batches[batches.length - 1].complete);
  return batches;
}

function recordEvent(db, eventId, overrides = {}) {
  db.recordAnalyticsEvent({
    eventId,
    wordCount: 4,
    occurredAt: "2026-08-30T10:00:00.000Z",
    localDate: "2026-08-30",
    spokenDurationMs: 2_000,
    mode: "self_hosted",
    provider: "self-hosted",
    model: "whisper-1",
    ...overrides,
  });
}

test("historical transcriptions reconcile in restart-safe local batches", (t) => {
  const db = createDb(t);
  if (!db) return;

  const insertTranscription = db.db.prepare(
    `INSERT INTO transcriptions (
       text, raw_text, status, client_transcription_id, timestamp, created_at,
       audio_duration_ms, provider, model, deleted_at
     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`
  );
  insertTranscription.run(
    "enhanced text",
    "one two three",
    "completed",
    "legacy-local",
    "2026-09-01T09:59:00.000Z",
    "2026-09-01 10:00:00",
    2_000,
    "self-hosted",
    "whisper-1",
    null
  );
  insertTranscription.run(
    "four five",
    null,
    "completed",
    "legacy-ambiguous",
    "2026-09-02T09:59:00.000Z",
    "2026-09-02 10:00:00",
    null,
    "unknown-provider",
    null,
    null
  );
  insertTranscription.run(
    "ambiguous post-clear words",
    null,
    "completed",
    "legacy-post-clear-ambiguous",
    "2026-09-03 10:00:00",
    "2026-09-03 10:00:00",
    null,
    null,
    null,
    null
  );
  insertTranscription.run(
    "cleared words",
    null,
    "completed",
    "legacy-cleared",
    "2026-08-01T09:59:00.000Z",
    "2026-08-01 10:00:00",
    null,
    null,
    null,
    null
  );
  insertTranscription.run(
    "failed words",
    null,
    "failed",
    "legacy-failed",
    "2026-09-03 10:00:00",
    "2026-09-03 10:00:00",
    null,
    null,
    null,
    null
  );
  insertTranscription.run(
    "timestamp unavailable",
    null,
    "completed",
    "legacy-invalid-time",
    "not-a-time",
    "not-a-time",
    null,
    null,
    null,
    null
  );
  insertTranscription.run(
    "deleted words",
    null,
    "completed",
    "legacy-deleted",
    "2026-09-03 10:00:00",
    "2026-09-03 10:00:00",
    null,
    null,
    null,
    "2026-09-03 11:00:00"
  );
  insertTranscription.run(
    "existing words",
    null,
    "completed",
    "legacy-existing",
    "2026-09-04 10:00:00",
    "2026-09-04 10:00:00",
    null,
    null,
    null,
    null
  );
  db.db
    .prepare(
      "INSERT INTO analytics_device_clear_state (id, cleared_through) VALUES (1, '2026-08-15T00:00:00.000Z')"
    )
    .run();
  recordEvent(db, "legacy-existing", {
    wordCount: 2,
    occurredAt: "2026-09-04T10:00:00.000Z",
  });
  db.db
    .prepare(
      "UPDATE analytics_events SET deleted_at = '2026-09-04T11:00:00.000Z' WHERE event_id = 'legacy-existing'"
    )
    .run();

  const batches = [];
  let afterId = 0;
  do {
    const batch = db.backfillAnalyticsHistoryBatch({ afterId, limit: 1 });
    batches.push(batch);
    afterId = batch.nextCursor;
  } while (!batches[batches.length - 1].complete);

  assert.equal(
    batches.reduce((total, batch) => total + batch.inserted, 0),
    2
  );
  assert.equal(db.getAnalyticsSummary().totalWords, 5);
  assert.equal(db.getAnalyticsSummary().totalDictations, 2);
  assert.deepEqual(
    db.db
      .prepare(
        `SELECT event_id, account_id, word_count, spoken_duration_ms, mode,
                counter_version, created_at
         FROM analytics_events WHERE deleted_at IS NULL ORDER BY event_id`
      )
      .all(),
    [
      {
        event_id: "legacy-ambiguous",
        account_id: null,
        word_count: 2,
        spoken_duration_ms: null,
        mode: "unknown",
        counter_version: 0,
        created_at: "2026-09-02 10:00:00",
      },
      {
        event_id: "legacy-local",
        account_id: null,
        word_count: 3,
        spoken_duration_ms: 2_000,
        mode: "self_hosted",
        counter_version: 0,
        created_at: "2026-09-01 10:00:00",
      },
    ]
  );
  assert.deepEqual(db.backfillAnalyticsHistoryBatch(), {
    complete: true,
    nextCursor: 0,
    scanned: 0,
    inserted: 0,
    skipped: 0,
  });
});

test("uploaded audio remains in History without contributing to dictation Insights", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.saveTranscription("Spoken dictation", null, {
    clientTranscriptionId: "spoken-dictation",
    analyticsOccurredAt: "2026-09-01T10:00:00.000Z",
  });
  const upload = db.saveTranscription("Imported audio transcript", null, {
    clientTranscriptionId: "uploaded-audio",
    analyticsOccurredAt: "2026-09-01T11:00:00.000Z",
    routeKind: "upload",
  });

  reconcileThroughCheckpoint(db, 1);

  assert.equal(db.getTranscriptions().length, 2);
  assert.equal(db.getAnalyticsSummary().totalDictations, 1);
  assert.equal(db.getAnalyticsSummary().totalWords, 2);
  assert.equal(
    db.getAnalyticsHistoryBackfillState(ANALYTICS_HISTORY_BACKFILL_VERSION).scannedThroughId,
    upload.id
  );
  assert.equal(
    db.db
      .prepare("SELECT COUNT(*) AS count FROM analytics_events WHERE event_id = ?")
      .get("uploaded-audio").count,
    0
  );
});

test("analytics reconciliation picks up later eligibility and usable processed text", (t) => {
  const db = createDb(t);
  if (!db) return;

  const insert = db.db.prepare(
    `INSERT INTO transcriptions (
       text, raw_text, status, client_transcription_id, timestamp, created_at
     ) VALUES (?, ?, ?, ?, ?, ?)`
  );
  insert.run(
    "processed text wins",
    "   ",
    "completed",
    "empty-raw",
    "2026-09-01 10:00:00",
    "2026-09-01 10:00:00"
  );
  insert.run(
    "retry succeeds later",
    null,
    "failed",
    "retried-later",
    "2026-09-02 10:00:00",
    "2026-09-02 10:00:00"
  );

  assert.equal(db.backfillAnalyticsHistoryBatch().inserted, 1);
  assert.equal(db.getAnalyticsSummary().totalWords, 3);

  db.db
    .prepare("UPDATE transcriptions SET status = 'completed' WHERE client_transcription_id = ?")
    .run("retried-later");
  assert.equal(db.backfillAnalyticsHistoryBatch().inserted, 1);
  assert.equal(db.getAnalyticsSummary().totalWords, 6);

  insert.run(
    "pulled after startup",
    null,
    "completed",
    "pulled-later",
    "2025-01-01 10:00:00",
    "2025-01-01 10:00:00"
  );
  assert.equal(db.backfillAnalyticsHistoryBatch().inserted, 1);
  assert.equal(db.backfillAnalyticsHistoryBatch().scanned, 0);
});

test("analytics history checkpoint persists progress and scans only the new tail", (t) => {
  const db = createDb(t);
  if (!db) return;

  const insert = db.db.prepare(
    `INSERT INTO transcriptions (
       text, status, client_transcription_id, timestamp, created_at
     ) VALUES (?, 'completed', ?, ?, ?)`
  );
  insert.run("one two", "checkpoint-1", "2026-09-01T10:00:00.000Z", "2026-09-01 10:00:00");
  insert.run("three four", "checkpoint-2", "2026-09-02T10:00:00.000Z", "2026-09-02 10:00:00");

  assert.deepEqual(db.getAnalyticsHistoryBackfillState(), {
    version: ANALYTICS_HISTORY_BACKFILL_VERSION,
    scannedThroughId: 0,
    targetId: 2,
  });
  assert.equal(
    reconcileThroughCheckpoint(db, 1).reduce((total, batch) => total + batch.inserted, 0),
    2
  );
  assert.deepEqual(db.getAnalyticsHistoryBackfillState(), {
    version: ANALYTICS_HISTORY_BACKFILL_VERSION,
    scannedThroughId: 2,
    targetId: 2,
  });
  assert.deepEqual(reconcileThroughCheckpoint(db), [
    {
      complete: true,
      nextCursor: 2,
      scanned: 0,
      inserted: 0,
      skipped: 0,
    },
  ]);

  insert.run("five six", "checkpoint-3", "2026-09-03T10:00:00.000Z", "2026-09-03 10:00:00");
  assert.deepEqual(db.getAnalyticsHistoryBackfillState(), {
    version: ANALYTICS_HISTORY_BACKFILL_VERSION,
    scannedThroughId: 2,
    targetId: 3,
  });
  assert.equal(reconcileThroughCheckpoint(db)[0].inserted, 1);
  assert.equal(db.getAnalyticsSummary().totalDictations, 3);

  assert.deepEqual(db.getAnalyticsHistoryBackfillState(ANALYTICS_HISTORY_BACKFILL_VERSION + 1), {
    version: ANALYTICS_HISTORY_BACKFILL_VERSION + 1,
    scannedThroughId: 0,
    targetId: 3,
  });
});

test("a reopened database reuses its completed analytics history checkpoint", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.saveTranscription("persist this checkpoint", null, {
    clientTranscriptionId: "checkpoint-reopen",
    analyticsOccurredAt: "2026-09-01T10:00:00.000Z",
  });
  reconcileThroughCheckpoint(db);
  db.db.close();

  const reopened = new DatabaseManager();
  try {
    assert.deepEqual(reopened.getAnalyticsHistoryBackfillState(), {
      version: ANALYTICS_HISTORY_BACKFILL_VERSION,
      scannedThroughId: 1,
      targetId: 1,
    });
    assert.equal(reconcileThroughCheckpoint(reopened)[0].scanned, 0);
  } finally {
    reopened.db.close();
  }
});

test("schema setup adds the checkpoint table to an existing database without a migration", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.db.exec("DROP TABLE analytics_history_backfill_state");
  db.db.close();

  const reopened = new DatabaseManager();
  try {
    assert.deepEqual(reopened.getAnalyticsHistoryBackfillState(), {
      version: ANALYTICS_HISTORY_BACKFILL_VERSION,
      scannedThroughId: 0,
      targetId: 0,
    });
  } finally {
    reopened.db.close();
  }
});

test("older eligibility changes rewind the durable analytics checkpoint", (t) => {
  const db = createDb(t);
  if (!db) return;

  const failed = db.db
    .prepare(
      `INSERT INTO transcriptions (
         text, status, client_transcription_id, timestamp, created_at
       ) VALUES ('retry these words', 'failed', 'checkpoint-retry',
                 '2026-09-01T10:00:00.000Z', '2026-09-01 10:00:00')`
    )
    .run();
  db.db
    .prepare(
      `INSERT INTO transcriptions (
         text, status, client_transcription_id, timestamp, created_at
       ) VALUES ('already completed', 'completed', 'checkpoint-complete',
                 '2026-09-02T10:00:00.000Z', '2026-09-02 10:00:00')`
    )
    .run();
  const empty = db.db
    .prepare(
      `INSERT INTO transcriptions (
         text, status, client_transcription_id, timestamp, created_at
       ) VALUES ('', 'completed', 'checkpoint-empty',
                 '2026-09-03T10:00:00.000Z', '2026-09-03 10:00:00')`
    )
    .run();
  reconcileThroughCheckpoint(db);
  assert.equal(db.getAnalyticsHistoryBackfillState().scannedThroughId, 3);

  db.updateTranscriptionStatus(Number(failed.lastInsertRowid), "completed");
  assert.equal(db.getAnalyticsHistoryBackfillState().scannedThroughId, 0);
  assert.equal(reconcileThroughCheckpoint(db)[0].inserted, 1);

  db.updateTranscriptionText(Number(empty.lastInsertRowid), "now usable", null);
  assert.equal(db.getAnalyticsHistoryBackfillState().scannedThroughId, 2);
  assert.equal(reconcileThroughCheckpoint(db)[0].inserted, 1);
});

test("analytics history checkpoint advances atomically with each batch", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.db
    .prepare(
      `INSERT INTO transcriptions (
         text, status, client_transcription_id, timestamp, created_at
       ) VALUES
         ('first row', 'completed', 'atomic-1',
          '2026-09-01T10:00:00.000Z', '2026-09-01 10:00:00'),
         ('second row', 'completed', 'atomic-2',
          '2026-09-02T10:00:00.000Z', '2026-09-02 10:00:00')`
    )
    .run();
  const state = db.getAnalyticsHistoryBackfillState();
  const first = db.backfillAnalyticsHistoryBatch({
    throughId: state.targetId,
    checkpointVersion: ANALYTICS_HISTORY_BACKFILL_VERSION,
    limit: 1,
  });
  assert.equal(first.nextCursor, 1);
  db.db.exec("DROP TABLE analytics_events");

  assert.throws(
    () =>
      db.backfillAnalyticsHistoryBatch({
        throughId: state.targetId,
        checkpointVersion: ANALYTICS_HISTORY_BACKFILL_VERSION,
        limit: 1,
      }),
    /analytics_events/
  );
  assert.equal(db.getAnalyticsHistoryBackfillState().scannedThroughId, 1);
});

test("clearing history keeps old counters gone and admits later transcription IDs", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.saveTranscription("old checkpoint words", null, {
    clientTranscriptionId: "checkpoint-before-clear",
    analyticsOccurredAt: "2026-09-01T10:00:00.000Z",
  });
  reconcileThroughCheckpoint(db);
  assert.equal(db.getAnalyticsSummary().totalDictations, 1);

  db.clearTranscriptions();
  assert.equal(db.getAnalyticsSummary().totalDictations, 0);
  const afterClear = new Date(Date.now() + 60_000).toISOString();
  db.saveTranscription("new checkpoint words", null, {
    clientTranscriptionId: "checkpoint-after-clear",
    analyticsOccurredAt: afterClear,
  });
  assert.equal(reconcileThroughCheckpoint(db)[0].inserted, 1);
  assert.equal(db.getAnalyticsSummary().totalDictations, 1);
  assert.equal(
    db.db
      .prepare("SELECT COUNT(*) AS count FROM analytics_events WHERE event_id = ?")
      .get("checkpoint-before-clear").count,
    0
  );
});

test("a dictation whose time cannot be read stays out instead of landing on today", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.db
    .prepare(
      `INSERT INTO transcriptions (
         text, status, client_transcription_id, timestamp, created_at
       ) VALUES ('undateable words', 'completed', 'no-usable-time', 'not-a-time', 'not-a-time')`
    )
    .run();

  assert.deepEqual(db.backfillAnalyticsHistoryBatch(), {
    complete: true,
    nextCursor: 1,
    scanned: 1,
    inserted: 0,
    skipped: 1,
  });
  const summary = db.getAnalyticsSummary();
  assert.equal(summary.totalDictations, 0, "an undateable row must not become today's dictation");
  assert.equal(summary.currentStreakDays, 0, "and must not manufacture a streak");
});

test("history the user cleared cannot come back through a mis-shaped timestamp", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.db
    .prepare(
      `INSERT INTO transcriptions (
         text, status, client_transcription_id, timestamp, created_at
       ) VALUES ('cleared words', 'completed', 'shape-bypass', '2027-01-01',
                 '2026-01-02 03:04:05')`
    )
    .run();
  db.db
    .prepare(
      "INSERT INTO analytics_device_clear_state (id, cleared_through) VALUES (1, '2026-08-15T00:00:00.000Z')"
    )
    .run();

  assert.deepEqual(db.backfillAnalyticsHistoryBatch(), {
    complete: true,
    nextCursor: 1,
    scanned: 1,
    inserted: 0,
    skipped: 1,
  });
  assert.equal(db.getAnalyticsSummary().totalDictations, 0);
});

test("backfill preserves the transcription creation time for retention", (t) => {
  const db = createDb(t);
  if (!db) return;

  db.db
    .prepare(
      `INSERT INTO transcriptions (
         text, status, client_transcription_id, timestamp, created_at
       ) VALUES ('old words', 'completed', 'old-retained',
                 '2020-01-01T09:59:00.000Z', '2020-01-01 10:00:00')`
    )
    .run();

  assert.equal(db.backfillAnalyticsHistoryBatch().inserted, 1);
  assert.equal(
    db.db.prepare("SELECT created_at FROM analytics_events WHERE event_id = 'old-retained'").get()
      .created_at,
    "2020-01-01 10:00:00"
  );
  assert.equal(db.deleteTranscriptionsExpiredBefore(30).analyticsPurged, 1);
  assert.equal(db.getAnalyticsSummary().totalDictations, 0);
});

test("new transcription rows retain the original analytics occurrence time", (t) => {
  const db = createDb(t);
  if (!db) return;

  const occurredAt = "2026-09-01T09:58:00.000Z";
  const result = db.saveTranscription("saved words", "saved words", {
    analyticsOccurredAt: occurredAt,
  });
  assert.equal(result.transcription.timestamp, occurredAt.replace("T", " "));
});

test("local Insights counters are content-free and idempotent", (t) => {
  const db = createDb(t);
  if (!db) return;

  recordEvent(db, "event-1");
  recordEvent(db, "event-1", { wordCount: 5 });

  const columns = db.db.prepare("PRAGMA table_info(analytics_events)").all();
  assert.equal(
    columns.some((column) => column.name === "text"),
    false
  );
  assert.equal(db.getAnalyticsSummary().totalWords, 5);
  assert.equal(db.getAnalyticsSummary().totalDictations, 1);
});

test("clearing local History also erases Insights counters", (t) => {
  const db = createDb(t);
  if (!db) return;

  recordEvent(db, "event-1");
  db.clearTranscriptions();

  assert.equal(db.getAnalyticsSummary().totalWords, 0);
  assert.equal(db.getAnalyticsSummary().totalDictations, 0);
  assert.equal(db.db.prepare("SELECT COUNT(*) AS count FROM analytics_events").get().count, 0);
});
