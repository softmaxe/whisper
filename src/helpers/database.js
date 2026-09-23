const Database = require("better-sqlite3");
const path = require("path");
const { randomUUID } = require("crypto");
const debugLogger = require("./debugLogger");
// An explicit zone marks an instant this app captured at dictation time. A
// naive timestamp may instead be a sync artifact: upsertTranscriptionFromCloud
// keeps the cloud created_at but lets timestamp default to the local pull, so
// a naive value must never outrank created_at when dating a historical row.
const { hasExplicitTimeZone, parseDbTimestamp, toDbTimestamp } = require("./dbTimestamp");
const {
  ANALYTICS_COUNTER_VERSION,
  ANALYTICS_HISTORY_BACKFILL_VERSION,
  ANALYTICS_HISTORICAL_COUNTER_VERSION,
  countSpokenWords,
  inferHistoricalAnalyticsMode,
  localDateKey,
  summarizeAnalyticsDays,
} = require("./analytics");
const { app } = require("electron");

// Server-enforced trigger cap (openwhispr-api); enforced here so one oversized
// trigger can't 400 the whole sync batch.
const MAX_SNIPPET_TRIGGER_LENGTH = 100;

class DatabaseManager {
  constructor() {
    this.db = null;
    this.activeAccountId = null;
    this.initDatabase();
  }

  initDatabase() {
    try {
      const dbFileName =
        process.env.NODE_ENV === "development" ? "transcriptions-dev.db" : "transcriptions.db";

      const dbPath = path.join(app.getPath("userData"), dbFileName);

      this.db = new Database(dbPath);
      this.db.pragma("journal_mode = WAL");

      this.db.exec(`
        CREATE TABLE IF NOT EXISTS transcriptions (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          text TEXT NOT NULL,
          timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
          created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
      `);

      // Audio retention columns
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN raw_text TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN has_audio INTEGER NOT NULL DEFAULT 0");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN audio_duration_ms INTEGER");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN provider TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN model TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec(
          "ALTER TABLE transcriptions ADD COLUMN status TEXT NOT NULL DEFAULT 'completed'"
        );
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN error_message TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN error_code TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      // Records the dictation intent (e.g. "translation") so retry/recover re-runs the same route.
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN route_kind TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }

      this.db.exec(`
        CREATE TABLE IF NOT EXISTS custom_dictionary (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          word TEXT NOT NULL UNIQUE,
          created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
      `);

      this.db.exec(`
        CREATE TABLE IF NOT EXISTS snippets (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          trigger TEXT NOT NULL,
          replacement TEXT NOT NULL,
          client_snippet_id TEXT,
          cloud_id TEXT,
          sync_status TEXT DEFAULT 'pending',
          deleted_at TEXT,
          created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
          updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
      `);

      // Sync columns for transcriptions
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN client_transcription_id TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN cloud_id TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN sync_status TEXT DEFAULT 'pending'");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE transcriptions ADD COLUMN deleted_at TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }

      // Sync columns for custom_dictionary
      try {
        this.db.exec("ALTER TABLE custom_dictionary ADD COLUMN client_dict_id TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE custom_dictionary ADD COLUMN cloud_id TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec(
          "ALTER TABLE custom_dictionary ADD COLUMN source TEXT NOT NULL DEFAULT 'manual'"
        );
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE custom_dictionary ADD COLUMN sync_status TEXT DEFAULT 'pending'");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE custom_dictionary ADD COLUMN deleted_at TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      try {
        this.db.exec("ALTER TABLE custom_dictionary ADD COLUMN updated_at DATETIME");
        this.db.exec(
          "UPDATE custom_dictionary SET updated_at = created_at WHERE updated_at IS NULL"
        );
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }

      // Backfill client IDs for existing rows
      const syncTables = [
        { table: "transcriptions", col: "client_transcription_id" },
        { table: "custom_dictionary", col: "client_dict_id" },
        { table: "snippets", col: "client_snippet_id" },
      ];
      for (const { table, col } of syncTables) {
        const rows = this.db.prepare(`SELECT id FROM ${table} WHERE ${col} IS NULL`).all();
        const stmt = this.db.prepare(`UPDATE ${table} SET ${col} = ? WHERE id = ?`);
        for (const row of rows) {
          stmt.run(randomUUID(), row.id);
        }
      }

      this.db.exec(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_transcriptions_client_id ON transcriptions(client_transcription_id)"
      );

      this.db.exec(`
        CREATE TABLE IF NOT EXISTS analytics_events (
          event_id TEXT PRIMARY KEY,
          account_id TEXT,
          occurred_at TEXT NOT NULL,
          local_date TEXT NOT NULL,
          word_count INTEGER NOT NULL CHECK (word_count > 0),
          spoken_duration_ms INTEGER,
          mode TEXT NOT NULL,
          provider TEXT,
          model TEXT,
          counter_version INTEGER NOT NULL DEFAULT 1,
          sync_status TEXT NOT NULL DEFAULT 'pending',
          deleted_at TEXT,
          created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_analytics_events_account_date
          ON analytics_events(account_id, local_date);
        CREATE TABLE IF NOT EXISTS analytics_clear_requests (
          account_id TEXT PRIMARY KEY,
          cleared_through TEXT NOT NULL,
          synced INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS analytics_device_clear_state (
          id INTEGER PRIMARY KEY CHECK (id = 1),
          cleared_through TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS analytics_history_backfill_state (
          id INTEGER PRIMARY KEY CHECK (id = 1),
          version INTEGER NOT NULL,
          scanned_through_transcription_id INTEGER NOT NULL DEFAULT 0
            CHECK (scanned_through_transcription_id >= 0)
        );
      `);
      // Repair databases created before analytics deletion tombstones were
      // introduced. SQLite has no ADD COLUMN IF NOT EXISTS syntax.
      try {
        this.db.exec("ALTER TABLE analytics_events ADD COLUMN deleted_at TEXT");
      } catch (err) {
        if (!err.message.includes("duplicate column")) throw err;
      }
      this.db.exec(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_custom_dictionary_client_id ON custom_dictionary(client_dict_id)"
      );
      // Cloud batch-create matches responses by client_dict_id, so a row
      // without one can never be marked synced and re-uploads every pass. Rows
      // written straight to SQLite don't set it (#1295), so the schema does.
      this.db.exec(`
        CREATE TRIGGER IF NOT EXISTS custom_dictionary_client_id_default
        AFTER INSERT ON custom_dictionary
        WHEN new.client_dict_id IS NULL
        BEGIN
          UPDATE custom_dictionary SET client_dict_id =
            lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' ||
            substr(lower(hex(randomblob(2))), 2) || '-' ||
            substr('89ab', abs(random()) % 4 + 1, 1) || substr(lower(hex(randomblob(2))), 2) ||
            '-' || lower(hex(randomblob(6)))
          WHERE id = new.id;
        END
      `);
      this.db.exec(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_snippets_client_id ON snippets(client_snippet_id) WHERE client_snippet_id IS NOT NULL"
      );
      this.db.exec(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_snippets_trigger_lower_active ON snippets(lower(trigger)) WHERE deleted_at IS NULL"
      );
      this.db.exec(
        "CREATE INDEX IF NOT EXISTS idx_snippets_pending_sync ON snippets(sync_status) WHERE sync_status = 'pending'"
      );

      return true;
    } catch (error) {
      debugLogger.error("Database initialization failed", { error: error.message }, "database");
      throw error;
    }
  }

  saveTranscription(
    text,
    rawText = null,
    {
      status = "completed",
      errorMessage = null,
      errorCode = null,
      routeKind = null,
      clientTranscriptionId = randomUUID(),
      analyticsOccurredAt = null,
    } = {}
  ) {
    try {
      if (!this.db) {
        throw new Error("Database not initialized");
      }
      // With an occurrence time this column carries when the dictation was
      // spoken rather than when the row was written -- earlier by the length
      // of the recording plus transcription. History reads it through
      // normalizeDbDate, which already branches on a trailing zone.
      // Keep the existing SQLite-friendly separator so mixed old/new rows
      // continue to sort chronologically, while the trailing Z marks this as
      // an exact client-captured instant for clear-state reconciliation.
      const occurredAt = toDbTimestamp(analyticsOccurredAt);
      const stmt = this.db.prepare(
        `INSERT INTO transcriptions (
           text, raw_text, status, error_message, error_code, route_kind,
           client_transcription_id, timestamp
         ) VALUES (?, ?, ?, ?, ?, ?, ?, COALESCE(?, CURRENT_TIMESTAMP))`
      );
      const result = stmt.run(
        text,
        rawText,
        status,
        errorMessage,
        errorCode,
        routeKind,
        clientTranscriptionId,
        occurredAt
      );

      const fetchStmt = this.db.prepare("SELECT * FROM transcriptions WHERE id = ?");
      const transcription = fetchStmt.get(result.lastInsertRowid);

      return { id: result.lastInsertRowid, success: true, transcription };
    } catch (error) {
      debugLogger.error("Error saving transcription", { error: error.message }, "database");
      throw error;
    }
  }

  _ensureAnalyticsHistoryBackfillState(version) {
    this.db
      .prepare(
        `INSERT INTO analytics_history_backfill_state (
           id, version, scanned_through_transcription_id
         ) VALUES (1, ?, 0)
         ON CONFLICT(id) DO UPDATE SET
           version = excluded.version,
           scanned_through_transcription_id = 0
         WHERE analytics_history_backfill_state.version <> excluded.version`
      )
      .run(version);
    return this.db
      .prepare(
        `SELECT version, scanned_through_transcription_id
         FROM analytics_history_backfill_state WHERE id = 1`
      )
      .get();
  }

  getAnalyticsHistoryBackfillState(version = ANALYTICS_HISTORY_BACKFILL_VERSION) {
    try {
      if (!this.db) throw new Error("Database not initialized");
      const safeVersion = Math.max(1, Math.trunc(Number(version)) || 1);
      return this.db.transaction(() => {
        const state = this._ensureAnalyticsHistoryBackfillState(safeVersion);
        const target = this.db
          .prepare("SELECT COALESCE(MAX(id), 0) AS id FROM transcriptions")
          .get();
        return {
          version: safeVersion,
          scannedThroughId: Number(state.scanned_through_transcription_id),
          targetId: Number(target.id),
        };
      })();
    } catch (error) {
      debugLogger.error(
        "Error reading analytics history backfill state",
        { error: error.message },
        "database"
      );
      throw error;
    }
  }

  _invalidateAnalyticsHistoryFromTranscription(id) {
    const resumeBeforeId = Math.max(0, Math.trunc(Number(id)) - 1);
    this.db
      .prepare(
        `INSERT INTO analytics_history_backfill_state (
           id, version, scanned_through_transcription_id
         ) VALUES (1, ?, 0)
         ON CONFLICT(id) DO UPDATE SET
           version = excluded.version,
           scanned_through_transcription_id = CASE
             WHEN analytics_history_backfill_state.version = excluded.version
             THEN MIN(
               analytics_history_backfill_state.scanned_through_transcription_id,
               ?
             )
             ELSE 0
           END`
      )
      .run(ANALYTICS_HISTORY_BACKFILL_VERSION, resumeBeforeId);
  }

  backfillAnalyticsHistoryBatch({
    afterId = 0,
    throughId = null,
    checkpointVersion = null,
    limit = 250,
  } = {}) {
    try {
      if (!this.db) throw new Error("Database not initialized");
      const safeLimit = Math.max(1, Math.min(Math.trunc(Number(limit)) || 250, 1_000));
      const safeAfterId = Math.max(0, Math.trunc(Number(afterId)) || 0);
      const safeThroughId =
        throughId === null || throughId === undefined
          ? null
          : Math.max(0, Math.trunc(Number(throughId)) || 0);
      const safeCheckpointVersion =
        checkpointVersion === null || checkpointVersion === undefined
          ? null
          : Math.max(1, Math.trunc(Number(checkpointVersion)) || 1);

      return this.db.transaction(() => {
        const checkpoint =
          safeCheckpointVersion === null
            ? null
            : this._ensureAnalyticsHistoryBackfillState(safeCheckpointVersion);
        const effectiveAfterId = checkpoint
          ? Number(checkpoint.scanned_through_transcription_id)
          : safeAfterId;
        if (safeThroughId !== null && effectiveAfterId >= safeThroughId) {
          return {
            complete: true,
            nextCursor: effectiveAfterId,
            scanned: 0,
            inserted: 0,
            skipped: 0,
          };
        }

        const clearState = this.db
          .prepare("SELECT cleared_through FROM analytics_device_clear_state WHERE id = 1")
          .get();
        // Legacy SQLite timestamps are completion times without an offset. Once
        // the user has cleared Insights, only a client-captured occurrence time
        // can prove that a historical row happened afterward, so an ambiguous
        // legacy row stays out rather than reviving a cleared counter. That is
        // the eligibility rule below; the boundary on the instant actually
        // written is enforced in the loop, where the chosen value is known.
        const rows = this.db
          .prepare(
            `SELECT transcription.id, transcription.client_transcription_id,
                    transcription.text, transcription.raw_text, transcription.timestamp,
                    transcription.created_at,
                    audio_duration_ms, provider, model
             FROM transcriptions transcription
             WHERE transcription.id > ?
               AND (? IS NULL OR transcription.id <= ?)
               AND transcription.deleted_at IS NULL
               AND transcription.status = 'completed'
               AND COALESCE(transcription.route_kind, '') != 'upload'
               AND TRIM(COALESCE(NULLIF(TRIM(transcription.raw_text), ''), transcription.text, '')) != ''
               AND NOT EXISTS (
                 SELECT 1 FROM analytics_events event
                 WHERE event.event_id = TRIM(transcription.client_transcription_id)
               )
               AND (
                 ? IS NULL
                 OR (
                   (TRIM(transcription.timestamp) LIKE '%Z'
                    OR SUBSTR(TRIM(transcription.timestamp), -6, 1) IN ('+', '-'))
                   AND JULIANDAY(transcription.timestamp) > JULIANDAY(?)
                 )
               )
             ORDER BY transcription.id ASC
             LIMIT ?`
          )
          .all(
            effectiveAfterId,
            safeThroughId,
            safeThroughId,
            clearState?.cleared_through ?? null,
            clearState?.cleared_through ?? null,
            safeLimit
          );

        let inserted = 0;
        let skipped = 0;
        const clearedThrough = clearState ? Date.parse(clearState.cleared_through) : null;
        const insert = this.db.prepare(
          `INSERT INTO analytics_events (
             event_id, account_id, occurred_at, local_date, word_count,
             spoken_duration_ms, mode, provider, model, counter_version, created_at
           ) VALUES (?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, DATETIME(?))
           ON CONFLICT(event_id) DO NOTHING`
        );
        const assignClientId = this.db.prepare(
          `UPDATE transcriptions SET client_transcription_id = ?
           WHERE id = ? AND (client_transcription_id IS NULL OR TRIM(client_transcription_id) = '')`
        );

        for (const row of rows) {
          const sourceText = row.raw_text?.trim() ? row.raw_text : row.text;
          const wordCount = countSpokenWords(sourceText);
          if (wordCount === 0) {
            skipped += 1;
            continue;
          }

          const createdAt = parseDbTimestamp(row.created_at);
          // A naive timestamp can be a sync artifact rather than an occurrence
          // time, so it is never the answer: created_at carries the cloud row's
          // own instant, while timestamp defaulted to the moment of the pull.
          const occurredAt =
            (hasExplicitTimeZone(row.timestamp) ? parseDbTimestamp(row.timestamp) : null) ??
            createdAt;
          // Guessing a date would put an old dictation on today, inflating
          // today's counters and manufacturing a current streak out of a row
          // whose age we could not read. It stays out instead.
          if (!occurredAt) {
            skipped += 1;
            continue;
          }
          // Not a restatement of the query's clear filter: that one decides
          // eligibility from transcription.timestamp, while this guards the
          // instant actually chosen, which may be created_at. It also catches
          // what the SQL shape test cannot -- a bare YYYY-MM-DD reads as zoned
          // there, its day hyphen sitting six characters from the end.
          if (clearedThrough !== null && occurredAt.getTime() <= clearedThrough) {
            skipped += 1;
            continue;
          }

          const eventId = row.client_transcription_id?.trim() || randomUUID();
          if (!row.client_transcription_id?.trim()) assignClientId.run(eventId, row.id);
          const result = insert.run(
            eventId,
            occurredAt.toISOString(),
            localDateKey(occurredAt),
            wordCount,
            Number(row.audio_duration_ms) > 0 ? Number(row.audio_duration_ms) : null,
            inferHistoricalAnalyticsMode(row.provider),
            row.provider || null,
            row.model || null,
            ANALYTICS_HISTORICAL_COUNTER_VERSION,
            (createdAt ?? occurredAt).toISOString()
          );
          if (result.changes > 0) inserted += 1;
          else skipped += 1;
        }

        const complete = rows.length < safeLimit;
        const lastCandidateId =
          rows.length > 0 ? Number(rows[rows.length - 1].id) : effectiveAfterId;
        const nextCursor = complete && safeThroughId !== null ? safeThroughId : lastCandidateId;
        if (checkpoint) {
          this.db
            .prepare(
              `UPDATE analytics_history_backfill_state
               SET scanned_through_transcription_id = ?
               WHERE id = 1 AND version = ?`
            )
            .run(nextCursor, safeCheckpointVersion);
        }

        return {
          complete,
          nextCursor,
          scanned: rows.length,
          inserted,
          skipped,
        };
      })();
    } catch (error) {
      debugLogger.error(
        "Error backfilling analytics history",
        { error: error.message },
        "database"
      );
      throw error;
    }
  }

  recordAnalyticsEvent({
    eventId,
    wordCount,
    occurredAt,
    localDate,
    spokenDurationMs = null,
    mode = "unknown",
    provider = null,
    model = null,
  }) {
    try {
      if (!this.db) throw new Error("Database not initialized");
      if (wordCount === 0) return { success: true, ignored: true };
      // Clear History is device-wide, so an in-flight recording must stay
      // cleared even if its account changes before this late write lands.
      const cleared = this.db
        .prepare(
          `SELECT 1 FROM analytics_device_clear_state
           WHERE id = 1 AND ? <= cleared_through`
        )
        .get(occurredAt);
      if (cleared) return { success: true, ignored: true };
      this.db
        .prepare(
          `INSERT INTO analytics_events (
             event_id, account_id, occurred_at, local_date, word_count,
             spoken_duration_ms, mode, provider, model, counter_version
           ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(event_id) DO UPDATE SET
             account_id = COALESCE(analytics_events.account_id, excluded.account_id),
             occurred_at = excluded.occurred_at,
             local_date = excluded.local_date,
             word_count = excluded.word_count,
             spoken_duration_ms = COALESCE(
               excluded.spoken_duration_ms,
               analytics_events.spoken_duration_ms
             ),
             mode = excluded.mode,
             provider = COALESCE(excluded.provider, analytics_events.provider),
             model = COALESCE(excluded.model, analytics_events.model),
             counter_version = excluded.counter_version,
             sync_status = 'pending'
           WHERE analytics_events.deleted_at IS NULL`
        )
        .run(
          eventId,
          this.activeAccountId,
          occurredAt,
          localDate,
          wordCount,
          Number(spokenDurationMs) > 0 ? Number(spokenDurationMs) : null,
          mode,
          provider,
          model,
          ANALYTICS_COUNTER_VERSION
        );
      return { success: true, eventId };
    } catch (error) {
      debugLogger.error("Error recording analytics event", { error: error.message }, "database");
      throw error;
    }
  }

  getAnalyticsSummary() {
    try {
      if (!this.db) throw new Error("Database not initialized");
      // Grouped in SQL so the row count is bounded by distinct days rather than
      // by dictations; summarizeAnalyticsDays still owns every derived figure.
      // Device-scoped by design: account_id only attributes rows for cloud
      // sync, so filtering on it here would blank the view on every sign-out.
      const days = this.db
        .prepare(
          `SELECT local_date AS date,
                  SUM(word_count) AS words,
                  COUNT(*) AS dictations,
                  SUM(CASE WHEN spoken_duration_ms > 0 THEN spoken_duration_ms ELSE 0 END)
                    AS spokenDurationMs,
                  SUM(CASE WHEN spoken_duration_ms > 0 THEN word_count ELSE 0 END)
                    AS coveredWords
           FROM analytics_events
           WHERE deleted_at IS NULL
           GROUP BY local_date`
        )
        .all();
      return summarizeAnalyticsDays(days);
    } catch (error) {
      debugLogger.error("Error reading analytics summary", { error: error.message }, "database");
      throw error;
    }
  }

  getTranscriptions(limit = 50, { includeDiscarded = false } = {}) {
    try {
      if (!this.db) {
        throw new Error("Database not initialized");
      }
      const statusFilter = includeDiscarded ? "" : " AND status NOT IN ('failed', 'discarded')";
      const stmt = this.db.prepare(
        `SELECT * FROM transcriptions WHERE deleted_at IS NULL${statusFilter} ORDER BY timestamp DESC LIMIT ?`
      );
      const transcriptions = stmt.all(limit);
      return transcriptions;
    } catch (error) {
      debugLogger.error("Error getting transcriptions", { error: error.message }, "database");
      throw error;
    }
  }

  clearTranscriptions() {
    try {
      if (!this.db) {
        throw new Error("Database not initialized");
      }
      const tombstone = this.db.prepare(
        "UPDATE transcriptions SET deleted_at = datetime('now'), sync_status = 'pending' WHERE cloud_id IS NOT NULL AND deleted_at IS NULL"
      );
      const hardDelete = this.db.prepare("DELETE FROM transcriptions WHERE cloud_id IS NULL");
      // One rule decides every row: a counter the cloud never received is
      // erased outright, and only a counter it did receive leaves a tombstone
      // behind for the delete pusher.
      //
      // It matters in both directions. Tombstoning a row that was never
      // uploaded sent its event id to the server on the next pass — for an
      // account that never turned Insights sync on, that was the only
      // analytics traffic it ever produced, and the server stores a row per id
      // it is asked to delete. Hard-deleting a row that *was* uploaded stranded
      // it in the cloud with nothing left on the device to erase it, which is
      // what signing out before clearing used to do.
      //
      // Scope follows the credentials: only the active account can be erased
      // remotely, so another account's synced rows keep their tombstones until
      // that account signs in here again.
      const hardDeleteLocalAnalytics = this.db.prepare(
        `DELETE FROM analytics_events
         WHERE account_id IS NULL OR (sync_status <> 'synced' AND deleted_at IS NULL)`
      );
      const tombstoneSyncedAnalytics = this.db.prepare(
        `UPDATE analytics_events
         SET deleted_at = ?, sync_status = 'pending'
         WHERE sync_status = 'synced' AND deleted_at IS NULL`
      );
      const countSyncedAnalytics = this.db.prepare(
        "SELECT COUNT(*) AS count FROM analytics_events WHERE account_id = ? AND sync_status = 'synced'"
      );
      const queueAnalyticsClear = this.db.prepare(
        `INSERT INTO analytics_clear_requests (account_id, cleared_through, synced)
         VALUES (?, ?, 0)
         ON CONFLICT(account_id) DO UPDATE SET
           cleared_through = MAX(analytics_clear_requests.cleared_through, excluded.cleared_through),
           synced = 0`
      );
      const updateDeviceClearState = this.db.prepare(
        `INSERT INTO analytics_device_clear_state (id, cleared_through)
         VALUES (1, ?)
         ON CONFLICT(id) DO UPDATE SET
           cleared_through = MAX(analytics_device_clear_state.cleared_through, excluded.cleared_through)`
      );
      const clearedThrough = new Date().toISOString();
      const clearAll = this.db.transaction(() => {
        const cleared = tombstone.run().changes + hardDelete.run().changes;
        updateDeviceClearState.run(clearedThrough);
        // The account-wide cutoff is what erases rows this device no longer
        // has — another device's uploads. It is only meaningful once this
        // account has actually put something in the cloud; queueing it for an
        // account that never synced would be a bare request to the analytics
        // API from a user who never opted in.
        const hasSyncedRows =
          this.activeAccountId && countSyncedAnalytics.get(this.activeAccountId).count > 0;
        tombstoneSyncedAnalytics.run(clearedThrough);
        hardDeleteLocalAnalytics.run();
        if (hasSyncedRows) {
          queueAnalyticsClear.run(this.activeAccountId, clearedThrough);
        }
        return cleared;
      });
      return { cleared: clearAll(), success: true };
    } catch (error) {
      debugLogger.error("Error clearing transcriptions", { error: error.message }, "database");
      throw error;
    }
  }

  /** Purges transcriptions and their Insights counters older than the retention window.
   *  Returns the affected transcription ids so callers can drop the matching audio files.
   *  Runs even with no expired transcriptions: counters outlive tombstoned rows. */
  deleteTranscriptionsExpiredBefore(retentionDays) {
    try {
      if (!this.db) {
        throw new Error("Database not initialized");
      }
      // Resolve the cutoff once so the ids we report are exactly the rows we purge.
      const cutoff = this.db
        .prepare("SELECT datetime('now', ?) AS cutoff")
        .get(`-${retentionDays} days`).cutoff;
      const expired = this.db
        .prepare("SELECT id FROM transcriptions WHERE deleted_at IS NULL AND created_at < ?")
        .all(cutoff)
        .map((row) => row.id);

      const tombstone = this.db.prepare(
        "UPDATE transcriptions SET deleted_at = datetime('now'), sync_status = 'pending' WHERE cloud_id IS NOT NULL AND deleted_at IS NULL AND created_at < ?"
      );
      const hardDelete = this.db.prepare(
        "DELETE FROM transcriptions WHERE cloud_id IS NULL AND created_at < ?"
      );
      // Counters follow the transcripts they describe, on the same cutoff and
      // in the same transaction. Matched on created_at, never occurred_at:
      // created_at uses the same SQLite timestamp format as the cutoff.
      //
      // Same rule as clearTranscriptions: only a row the cloud actually holds
      // leaves a tombstone. Attribution alone is not enough — every dictation
      // made while signed in carries an account_id whether or not Insights
      // sync was ever turned on, so tombstoning on that basis shipped the event
      // ids of a user who never opted in.
      const tombstoneSyncedAnalytics = this.db.prepare(
        `UPDATE analytics_events
         SET deleted_at = datetime('now'), sync_status = 'pending'
         WHERE sync_status = 'synced' AND deleted_at IS NULL AND created_at < ?`
      );
      const purgeUnsyncedAnalytics = this.db.prepare(
        `DELETE FROM analytics_events
         WHERE (account_id IS NULL OR sync_status <> 'synced')
           AND deleted_at IS NULL AND created_at < ?`
      );
      let analyticsPurged = 0;
      this.db.transaction(() => {
        tombstone.run(cutoff);
        hardDelete.run(cutoff);
        analyticsPurged =
          tombstoneSyncedAnalytics.run(cutoff).changes + purgeUnsyncedAnalytics.run(cutoff).changes;
      })();
      return { ids: expired, analyticsPurged };
    } catch (error) {
      debugLogger.error(
        "Error purging expired transcriptions",
        { error: error.message },
        "database"
      );
      throw error;
    }
  }

  deleteTranscription(id) {
    try {
      if (!this.db) {
        throw new Error("Database not initialized");
      }
      const row = this.db
        .prepare("SELECT cloud_id, deleted_at FROM transcriptions WHERE id = ?")
        .get(id);
      if (!row || row.deleted_at) return { success: false, id };
      const stmt = row.cloud_id
        ? this.db.prepare(
            "UPDATE transcriptions SET deleted_at = datetime('now'), sync_status = 'pending' WHERE id = ? AND deleted_at IS NULL"
          )
        : this.db.prepare("DELETE FROM transcriptions WHERE id = ?");
      const result = stmt.run(id);
      return { success: result.changes > 0, id };
    } catch (error) {
      debugLogger.error("Error deleting transcription", { error: error.message }, "database");
      throw error;
    }
  }

  updateTranscriptionAudio(id, { hasAudio, audioDurationMs, provider, model }) {
    try {
      if (!this.db) throw new Error("Database not initialized");
      const stmt = this.db.prepare(
        "UPDATE transcriptions SET has_audio = ?, audio_duration_ms = ?, provider = ?, model = ? WHERE id = ?"
      );
      stmt.run(hasAudio, audioDurationMs, provider, model, id);
      return { success: true };
    } catch (error) {
      debugLogger.error("Error updating transcription audio", { error: error.message }, "database");
      throw error;
    }
  }

  updateTranscriptionText(id, text, rawText) {
    try {
      if (!this.db) throw new Error("Database not initialized");
      const stmt = this.db.prepare("UPDATE transcriptions SET text = ?, raw_text = ? WHERE id = ?");
      this.db.transaction(() => {
        const existing = this.db
          .prepare("SELECT text, raw_text FROM transcriptions WHERE id = ?")
          .get(id);
        if (existing && (existing.text !== text || existing.raw_text !== rawText)) {
          this._invalidateAnalyticsHistoryFromTranscription(id);
        }
        stmt.run(text, rawText, id);
      })();
      return { success: true };
    } catch (error) {
      debugLogger.error("Error updating transcription text", { error: error.message }, "database");
      throw error;
    }
  }

  updateTranscriptionStatus(id, status, errorMessage = null, errorCode = null) {
    try {
      if (!this.db) throw new Error("Database not initialized");
      const stmt = this.db.prepare(
        "UPDATE transcriptions SET status = ?, error_message = ?, error_code = ? WHERE id = ?"
      );
      this.db.transaction(() => {
        const existing = this.db.prepare("SELECT status FROM transcriptions WHERE id = ?").get(id);
        if (existing && existing.status !== status && status === "completed") {
          this._invalidateAnalyticsHistoryFromTranscription(id);
        }
        stmt.run(status, errorMessage, errorCode, id);
      })();
      return { success: true };
    } catch (error) {
      debugLogger.error(
        "Error updating transcription status",
        { error: error.message },
        "database"
      );
      throw error;
    }
  }

  getTranscriptionById(id) {
    try {
      if (!this.db) throw new Error("Database not initialized");
      const stmt = this.db.prepare("SELECT * FROM transcriptions WHERE id = ?");
      return stmt.get(id) || null;
    } catch (error) {
      debugLogger.error("Error getting transcription by id", { error: error.message }, "database");
      throw error;
    }
  }

  clearAudioFlags(ids) {
    try {
      if (!this.db) throw new Error("Database not initialized");
      if (!ids || ids.length === 0) return { success: true };
      const transaction = this.db.transaction((idList) => {
        const stmt = this.db.prepare("UPDATE transcriptions SET has_audio = 0 WHERE id = ?");
        for (const id of idList) {
          stmt.run(id);
        }
      });
      transaction(ids);
      return { success: true };
    } catch (error) {
      debugLogger.error("Error clearing audio flags", { error: error.message }, "database");
      throw error;
    }
  }

  getDictionary() {
    try {
      if (!this.db) {
        throw new Error("Database not initialized");
      }
      const rows = this.db
        .prepare("SELECT word FROM custom_dictionary WHERE deleted_at IS NULL ORDER BY id ASC")
        .all();
      return rows.map((row) => row.word);
    } catch (error) {
      debugLogger.error("Error getting dictionary", { error: error.message }, "database");
      throw error;
    }
  }

  // Every dictionary mutation rule lives here once, so the whole-list and
  // delta write paths cannot drift apart.
  _dictionaryWriteStatements() {
    return {
      tombstone: this.db.prepare(
        "UPDATE custom_dictionary SET deleted_at = datetime('now'), updated_at = datetime('now'), sync_status = 'pending' WHERE id = ? AND deleted_at IS NULL"
      ),
      hardDelete: this.db.prepare(
        "DELETE FROM custom_dictionary WHERE id = ? AND cloud_id IS NULL"
      ),
      restore: this.db.prepare(
        "UPDATE custom_dictionary SET deleted_at = NULL, source = CASE WHEN source = 'learned' AND ? = 'manual' THEN 'manual' ELSE source END, word = ?, updated_at = datetime('now'), sync_status = 'pending' WHERE id = ?"
      ),
      promoteSource: this.db.prepare(
        "UPDATE custom_dictionary SET word = ?, source = 'manual', updated_at = datetime('now'), sync_status = 'pending' WHERE id = ? AND source = 'learned'"
      ),
      // Guarded on word != ? so an unchanged row keeps its sync_status.
      updateWord: this.db.prepare(
        "UPDATE custom_dictionary SET word = ?, updated_at = datetime('now'), sync_status = 'pending' WHERE id = ? AND word != ?"
      ),
      // INSERT OR IGNORE in case a legacy case-variant row collides on the
      // case-sensitive UNIQUE(word) that the lowercase index didn't catch.
      insert: this.db.prepare(
        "INSERT OR IGNORE INTO custom_dictionary (word, source, client_dict_id, sync_status, updated_at) VALUES (?, ?, ?, 'pending', datetime('now'))"
      ),
    };
  }

  // Dedupe by lower(word), keeping the first occurrence's casing, so no caller
  // can present two spellings of the same word to a write loop.
  _normalizeDictionaryWords(words) {
    const byLower = new Map();
    for (const raw of Array.isArray(words) ? words : []) {
      if (typeof raw !== "string") continue;
      const trimmed = raw.trim();
      if (!trimmed) continue;
      const lower = trimmed.toLowerCase();
      if (!byLower.has(lower)) byLower.set(lower, trimmed);
    }
    return byLower;
  }

  _dictionaryRows() {
    const rows = this.db
      .prepare("SELECT id, word, source, deleted_at FROM custom_dictionary")
      .all();
    return { rows, byLower: new Map(rows.map((r) => [r.word.toLowerCase(), r])) };
  }

  // Returns true when the word became present, so callers can report how many
  // words they actually added rather than how many they asked for.
  _upsertDictionaryWord(stmts, word, existing, source) {
    if (!existing) {
      return stmts.insert.run(word, source, randomUUID()).changes > 0;
    }
    if (existing.deleted_at) {
      stmts.restore.run(source, word, existing.id);
      return true;
    }
    if (source === "manual" && existing.source === "learned") {
      stmts.promoteSource.run(word, existing.id);
    } else {
      stmts.updateWord.run(word, existing.id, word);
    }
    return false;
  }

  // Hard-delete when the row never reached the cloud, else tombstone so the
  // next push tells the server about the deletion.
  _deleteDictionaryRow(stmts, existing) {
    if (!existing || existing.deleted_at) return false;
    const hardResult = stmts.hardDelete.run(existing.id);
    if (hardResult.changes === 0) stmts.tombstone.run(existing.id);
    return true;
  }

  // Add and/or remove specific words, leaving every other row untouched.
  // Prefer this over setDictionary, which deletes whatever the caller omitted
  // and so lets a stale snapshot destroy the rest (#1295).
  // `source` tags additions ('manual' for user-typed, 'learned' for auto-learn).
  applyDictionaryChanges({ add = [], remove = [] } = {}, source = "manual") {
    try {
      if (!this.db) {
        throw new Error("Database not initialized");
      }
      const additions = this._normalizeDictionaryWords(add);
      const removals = this._normalizeDictionaryWords(remove);
      // A word on both sides is a rename to itself; adding wins.
      for (const lower of additions.keys()) removals.delete(lower);
      if (additions.size === 0 && removals.size === 0) {
        return { success: true, added: 0, removed: 0 };
      }

      const { byLower } = this._dictionaryRows();
      const stmts = this._dictionaryWriteStatements();
      let added = 0;
      let removed = 0;

      this.db.transaction(() => {
        for (const lower of removals.keys()) {
          if (this._deleteDictionaryRow(stmts, byLower.get(lower))) removed += 1;
        }
        for (const [lower, word] of additions) {
          if (this._upsertDictionaryWord(stmts, word, byLower.get(lower), source)) added += 1;
        }
      })();

      return { success: true, added, removed };
    } catch (error) {
      debugLogger.error("Error applying dictionary changes", { error: error.message }, "database");
      throw error;
    }
  }

  // Replace the entire dictionary: anything absent from `words` is deleted.
  // Only for deliberate replace-everything callers (settings restore, clear
  // all, first write into an empty database). Everything else wants
  // applyDictionaryChanges.
  //
  // Diff-based so unchanged rows keep their source/created_at/cloud_id.
  setDictionary(words, sourceForNewWords = "manual") {
    try {
      if (!this.db) {
        throw new Error("Database not initialized");
      }
      const incomingByLower = this._normalizeDictionaryWords(words);
      const { rows, byLower } = this._dictionaryRows();
      const stmts = this._dictionaryWriteStatements();

      this.db.transaction(() => {
        for (const existing of rows) {
          if (incomingByLower.has(existing.word.toLowerCase())) continue;
          this._deleteDictionaryRow(stmts, existing);
        }
        for (const [lower, word] of incomingByLower) {
          this._upsertDictionaryWord(stmts, word, byLower.get(lower), sourceForNewWords);
        }
      })();

      return { success: true };
    } catch (error) {
      debugLogger.error("Error setting dictionary", { error: error.message }, "database");
      throw error;
    }
  }

  getSnippets() {
    try {
      if (!this.db) {
        throw new Error("Database not initialized");
      }
      return this.db
        .prepare(
          "SELECT trigger, replacement FROM snippets WHERE deleted_at IS NULL ORDER BY id ASC"
        )
        .all();
    } catch (error) {
      debugLogger.error("Error getting snippets", { error: error.message }, "database");
      throw error;
    }
  }

  setSnippets(snippets) {
    try {
      if (!this.db) {
        throw new Error("Database not initialized");
      }

      const incomingByLower = new Map();
      for (const raw of Array.isArray(snippets) ? snippets : []) {
        if (!raw || typeof raw !== "object") continue;
        const trigger = typeof raw.trigger === "string" ? raw.trigger.trim() : "";
        const replacement = typeof raw.replacement === "string" ? raw.replacement.trim() : "";
        if (!trigger || !replacement) continue;
        if (trigger.length > MAX_SNIPPET_TRIGGER_LENGTH) continue;
        const lower = trigger.toLowerCase();
        if (!incomingByLower.has(lower)) incomingByLower.set(lower, { trigger, replacement });
      }
      const cleaned = Array.from(incomingByLower.values());
      const incomingLower = new Set(incomingByLower.keys());

      const existingRows = this.db.prepare("SELECT * FROM snippets").all();
      const existingByLower = new Map();
      for (const row of existingRows) {
        const lower = row.trigger.toLowerCase();
        const current = existingByLower.get(lower);
        if (!current || (current.deleted_at && !row.deleted_at)) existingByLower.set(lower, row);
      }

      const tombstone = this.db.prepare(
        "UPDATE snippets SET deleted_at = datetime('now'), updated_at = datetime('now'), sync_status = 'pending' WHERE id = ? AND deleted_at IS NULL"
      );
      const hardDelete = this.db.prepare("DELETE FROM snippets WHERE id = ? AND cloud_id IS NULL");
      const restore = this.db.prepare(
        "UPDATE snippets SET deleted_at = NULL, trigger = ?, replacement = ?, updated_at = datetime('now'), sync_status = 'pending' WHERE id = ?"
      );
      const updateActive = this.db.prepare(
        "UPDATE snippets SET trigger = ?, replacement = ?, updated_at = datetime('now'), sync_status = 'pending' WHERE id = ? AND (trigger != ? OR replacement != ?)"
      );
      const insert = this.db.prepare(
        "INSERT OR IGNORE INTO snippets (trigger, replacement, client_snippet_id, sync_status, updated_at) VALUES (?, ?, ?, 'pending', datetime('now'))"
      );

      this.db.transaction(() => {
        for (const existing of existingRows) {
          if (incomingLower.has(existing.trigger.toLowerCase())) continue;
          if (existing.deleted_at) continue;
          const hardResult = hardDelete.run(existing.id);
          if (hardResult.changes === 0) tombstone.run(existing.id);
        }

        for (const snippet of cleaned) {
          const existing = existingByLower.get(snippet.trigger.toLowerCase());
          if (existing) {
            if (existing.deleted_at) {
              restore.run(snippet.trigger, snippet.replacement, existing.id);
            } else {
              updateActive.run(
                snippet.trigger,
                snippet.replacement,
                existing.id,
                snippet.trigger,
                snippet.replacement
              );
            }
            continue;
          }
          insert.run(snippet.trigger, snippet.replacement, randomUUID());
        }
      })();

      return { success: true };
    } catch (error) {
      debugLogger.error("Error setting snippets", { error: error.message }, "database");
      throw error;
    }
  }
}

module.exports = DatabaseManager;
