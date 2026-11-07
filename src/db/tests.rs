//! Database tests: transcriptions, transcript retention, Insights counters and
//! the schema.

use super::*;
use crate::analytics::AnalyticsEventInput;

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn count(db: &Database, sql: &str) -> i64 {
    db.lock().query_row(sql, [], |r| r.get(0)).unwrap()
}

fn event(occurred_at: &str) -> AnalyticsEventInput {
    AnalyticsEventInput {
        word_count: 4,
        occurred_at: occurred_at.into(),
        local_date: occurred_at[..10].into(),
        spoken_duration_ms: Some(2_000),
    }
}

fn at(iso: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .with_timezone(&Utc)
}

fn days_ago(days: i64) -> String {
    timestamp::format_db_timestamp(&(Utc::now() - Duration::days(days)))
}

fn save(db: &Database, text: &str, occurred: &str) -> TranscriptionItem {
    db.save_transcription(text, None, Some(at(occurred)))
        .unwrap()
}

/// A row with `status`, as older versions wrote failed and discarded rows.
fn save_with_status(db: &Database, text: &str, status: &str) {
    let row = db.save_transcription(text, None, None).unwrap();
    db.lock()
        .execute(
            "UPDATE transcriptions SET status = ? WHERE id = ?",
            params![status, row.id],
        )
        .unwrap();
}

// ---- transcriptions ----

#[test]
fn saves_and_lists_transcriptions_newest_first() {
    let db = db();
    let first = save(&db, "first", "2026-09-01T10:00:00.000Z");
    let second = save(&db, "second", "2026-09-02T10:00:00.000Z");
    save_with_status(&db, "oops", "failed");
    save_with_status(&db, "", "discarded");

    let visible: Vec<i64> = db
        .get_transcriptions(50)
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(visible, [second.id, first.id]);
    assert_eq!(db.get_transcriptions(1).unwrap()[0].id, second.id);
    let now = db.save_transcription("now", None, None).unwrap();
    assert_eq!(
        db.get_transcriptions(1).unwrap()[0].id,
        now.id,
        "saved without a time means now"
    );
}

#[test]
fn rows_store_timestamps_in_one_shape() {
    let db = db();
    let row = save(&db, "saved words", "2026-09-01T09:58:00.000Z");
    assert_eq!(row.timestamp, "2026-09-01 09:58:00.000Z");
    assert_eq!(row.occurred_at(), Some(at("2026-09-01T09:58:00Z")));
    let plain = db.save_transcription("x", None, None).unwrap();
    for value in [&plain.timestamp, &plain.created_at] {
        let parsed = timestamp::parse_db_timestamp(value).unwrap();
        assert_eq!(&timestamp::format_db_timestamp(&parsed), value);
    }
}

#[test]
fn updates_text_and_audio() {
    let db = db();
    let row = save(&db, "text", "2026-09-01T10:00:00.000Z");
    db.update_transcription_text(row.id, "new", Some("raw"))
        .unwrap();
    db.update_transcription_audio(
        row.id,
        &TranscriptionAudioUpdate {
            has_audio: true,
            audio_duration_ms: Some(1234),
        },
    )
    .unwrap();
    let got = db.get_transcription_by_id(row.id).unwrap().unwrap();
    assert_eq!(
        (got.text.as_str(), got.raw_text.as_deref()),
        ("new", Some("raw"))
    );
    assert!(got.has_audio);
    assert_eq!(got.audio_duration_ms, Some(1234));

    assert_eq!(db.clear_all_audio_flags().unwrap(), 1);
    assert!(
        !db.get_transcription_by_id(row.id)
            .unwrap()
            .unwrap()
            .has_audio
    );
    assert!(db.get_transcription_by_id(9999).unwrap().is_none());
}

#[test]
fn deletes_rows() {
    let db = db();
    let row = save(&db, "local", "2026-09-01T10:00:00.000Z");
    assert!(db.delete_transcription(row.id).unwrap());
    assert!(db.get_transcription_by_id(row.id).unwrap().is_none());
    assert!(!db.delete_transcription(row.id).unwrap(), "already deleted");
    assert_eq!(count(&db, "SELECT COUNT(*) FROM transcriptions"), 0);
}

// ---- transcript retention ----

fn insert_aged(db: &Database, text: &str, age_days: i64) -> i64 {
    let row = db.save_transcription(text, None, None).unwrap();
    db.lock()
        .execute(
            "UPDATE transcriptions SET created_at = ? WHERE id = ?",
            params![days_ago(age_days), row.id],
        )
        .unwrap();
    row.id
}

#[test]
fn purges_transcriptions_past_the_retention_window() {
    let db = db();
    let stale = insert_aged(&db, "two days old", 2);
    let fresh = insert_aged(&db, "a few hours old", 0);
    let purge = db.delete_transcriptions_expired_before(1).unwrap();
    assert_eq!(purge.ids, [stale]);
    let remaining: Vec<i64> = db
        .lock()
        .prepare("SELECT id FROM transcriptions")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(remaining, [fresh]);
}

#[test]
fn retention_purges_counters_on_the_same_schedule() {
    let db = db();
    db.record_analytics_event(&event(&days_ago(10))).unwrap();
    db.record_analytics_event(&event(&days_ago(0))).unwrap();
    let purge = db.delete_transcriptions_expired_before(7).unwrap();
    assert_eq!(purge.analytics_purged, 1);
    assert!(purge.ids.is_empty());
    assert_eq!(db.get_analytics_summary().unwrap().total_dictations, 1);
}

// ---- Insights counters ----

#[test]
fn zero_word_events_are_ignored() {
    let db = db();
    let mut empty = event("2026-08-30 10:00:00.000Z");
    empty.word_count = 0;
    assert!(!db.record_analytics_event(&empty).unwrap());
    assert_eq!(count(&db, "SELECT COUNT(*) FROM analytics_events"), 0);
}

#[test]
fn counters_outlive_a_deleted_transcription() {
    let db = db();
    let row = save(&db, "one two three four", "2026-08-30T10:00:00.000Z");
    db.record_analytics_event(&event("2026-08-30 10:00:00.000Z"))
        .unwrap();
    db.delete_transcription(row.id).unwrap();
    assert_eq!(db.get_analytics_summary().unwrap().total_dictations, 1);
}

#[test]
fn clearing_history_erases_transcriptions_and_counters() {
    let db = db();
    save(&db, "words", "2026-08-30T10:00:00.000Z");
    db.record_analytics_event(&event("2026-08-30 10:00:00.000Z"))
        .unwrap();
    assert_eq!(db.clear_transcriptions().unwrap(), 1);
    let summary = db.get_analytics_summary().unwrap();
    assert_eq!((summary.total_words, summary.total_dictations), (0, 0));
    assert_eq!(count(&db, "SELECT COUNT(*) FROM analytics_events"), 0);
    assert!(db.get_transcriptions(50).unwrap().is_empty());
    assert!(
        db.record_analytics_event(&event("2026-08-31 10:00:00.000Z"))
            .unwrap()
    );
    assert_eq!(db.get_analytics_summary().unwrap().total_dictations, 1);
}

#[test]
fn summary_reports_wpm_coverage_and_streaks() {
    let db = db();
    let mut a = event("2026-08-29 10:00:00.000Z");
    a.word_count = 100;
    a.spoken_duration_ms = Some(60_000);
    let mut b = event("2026-08-30 10:00:00.000Z");
    b.word_count = 25;
    b.spoken_duration_ms = None;
    db.record_analytics_event(&a).unwrap();
    db.record_analytics_event(&b).unwrap();
    let summary = db.get_analytics_summary_for("2026-08-30").unwrap();
    assert_eq!(summary.total_words, 125);
    assert_eq!(summary.average_wpm, Some(100));
    assert_eq!(summary.wpm_coverage_percent, 80);
    assert_eq!(
        (summary.current_streak_days, summary.longest_streak_days),
        (2, 2)
    );
    assert_eq!(summary.daily.len(), 2);
}

// ---- schema ----

fn column_names(db: &Database, table: &str) -> Vec<String> {
    db.lock()
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn schema_matches_the_expected_layout() {
    let db = db();
    assert_eq!(
        column_names(&db, "transcriptions"),
        [
            "id",
            "text",
            "raw_text",
            "timestamp",
            "created_at",
            "has_audio",
            "audio_duration_ms",
            "status",
        ]
    );
    assert_eq!(
        column_names(&db, "custom_dictionary"),
        [
            "id",
            "word",
            "source",
            "use_count",
            "last_used_at",
            "created_at",
            "updated_at",
        ]
    );
    assert_eq!(
        column_names(&db, "dictionary_corrections"),
        ["id", "misheard", "term", "count", "updated_at"]
    );
    assert_eq!(column_names(&db, "dictionary_rejected_words"), ["word"]);
    assert_eq!(
        column_names(&db, "auto_learn_stats"),
        ["app", "sessions", "readable", "edited", "learned"]
    );
    assert_eq!(
        column_names(&db, "snippets"),
        ["id", "trigger", "replacement", "created_at"]
    );
    assert_eq!(
        column_names(&db, "analytics_events"),
        [
            "id",
            "occurred_at",
            "local_date",
            "word_count",
            "spoken_duration_ms",
        ]
    );
    assert_eq!(count(&db, "PRAGMA user_version"), 3);
}

fn legacy_connection(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE transcriptions (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           text TEXT NOT NULL,
           raw_text TEXT,
           timestamp TEXT NOT NULL,
           created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%fZ', 'now')),
           has_audio INTEGER NOT NULL DEFAULT 0,
           audio_duration_ms INTEGER,
           provider TEXT,
           model TEXT,
           status TEXT NOT NULL DEFAULT 'completed',
           error_message TEXT,
           error_code TEXT,
           route_kind TEXT
         );
         CREATE INDEX idx_transcriptions_timestamp ON transcriptions(timestamp);
         CREATE TABLE analytics_events (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           occurred_at TEXT NOT NULL,
           local_date TEXT NOT NULL,
           word_count INTEGER NOT NULL CHECK (word_count > 0),
           spoken_duration_ms INTEGER CHECK (spoken_duration_ms > 0),
           provider TEXT,
           model TEXT
         );
         CREATE INDEX idx_analytics_events_local_date ON analytics_events(local_date);
         INSERT INTO transcriptions (
           id, text, raw_text, timestamp, created_at, has_audio, audio_duration_ms,
           provider, model, status, error_message, error_code, route_kind
         ) VALUES (
           41, 'delivered', 'raw words', '2026-09-01 10:00:00.000Z',
           '2026-09-01 10:00:01.000Z', 1, 2000,
           'old provider', 'old model', 'completed', NULL, NULL, 'old route'
         ), (
           42, 'failed words', NULL, '2026-09-01 10:01:00.000Z',
           '2026-09-01 10:01:01.000Z', 1, 1000,
           'old provider', 'old model', 'failed', 'old error', 'OLD_ERROR', 'old route'
         );
         INSERT INTO analytics_events VALUES (
           17, '2026-09-01 10:00:00.000Z', '2026-09-01', 4, 2000,
           'old provider', 'old model'
         );
         PRAGMA user_version = 2;",
    )
    .unwrap();
    conn
}

#[test]
fn upgrades_legacy_metadata_without_losing_history_or_counters() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    drop(legacy_connection(&path));
    let expected_layout = db();
    let upgraded = Database::open(&path).unwrap();
    for table in ["transcriptions", "analytics_events"] {
        assert_eq!(
            column_names(&upgraded, table),
            column_names(&expected_layout, table)
        );
    }
    let rows = upgraded.get_transcriptions(10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 41);
    assert_eq!(rows[0].text, "delivered");
    assert_eq!(rows[0].raw_text.as_deref(), Some("raw words"));
    assert_eq!(rows[0].timestamp, "2026-09-01 10:00:00.000Z");
    assert_eq!(rows[0].created_at, "2026-09-01 10:00:01.000Z");
    assert!(rows[0].has_audio);
    assert_eq!(rows[0].audio_duration_ms, Some(2000));
    let summary = upgraded.get_analytics_summary_for("2026-09-01").unwrap();
    assert_eq!(summary.total_words, 4);
    assert_eq!(summary.total_dictations, 1);
    assert_eq!(summary.average_wpm, Some(120));
    assert_eq!(count(&upgraded, "SELECT id FROM analytics_events"), 17);
    assert_eq!(count(&upgraded, "PRAGMA user_version"), 3);
    assert_eq!(
        count(
            &upgraded,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_transcriptions_timestamp', 'idx_analytics_events_local_date')"
        ),
        2
    );
    drop(upgraded);

    let reopened = Database::open(&path).unwrap();
    assert_eq!(reopened.get_transcriptions(10).unwrap(), rows);
    assert_eq!(
        reopened.get_analytics_summary_for("2026-09-01").unwrap(),
        summary
    );
    assert_eq!(reopened.delete_hidden_transcriptions().unwrap(), [42]);
    assert!(reopened.save_transcription("new", None, None).unwrap().id > 42);
    reopened
        .record_analytics_event(&event("2026-09-01 10:02:00.000Z"))
        .unwrap();
    assert_eq!(
        reopened
            .get_analytics_summary_for("2026-09-01")
            .unwrap()
            .total_dictations,
        2
    );
}

#[test]
fn failed_upgrade_rolls_back_schema_and_version() {
    let conn = legacy_connection(Path::new(":memory:"));
    // An unexpected dependent index makes DROP COLUMN fail after earlier
    // columns were removed. The entire upgrade must roll back.
    conn.execute_batch("CREATE INDEX custom_error_index ON transcriptions(error_code);")
        .unwrap();
    assert!(schema::init(&conn).is_err());
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let legacy: (String, String) = conn
        .query_row(
            "SELECT provider, route_kind FROM transcriptions WHERE id = 41",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(legacy, ("old provider".into(), "old route".into()));
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM transcriptions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn refuses_a_newer_schema_without_downgrading_it() {
    let db = db();
    let conn = db.lock();
    conn.pragma_update(None, "user_version", 4).unwrap();
    assert!(schema::init(&conn).is_err());
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
}

#[test]
fn a_reopened_database_keeps_its_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("whisper.db");
    let rows = {
        let db = Database::open(&path).unwrap();
        save(&db, "persist me", "2026-09-01T10:00:00.000Z");
        db.record_analytics_event(&event("2026-09-01 10:00:00.000Z"))
            .unwrap();
        db.get_transcriptions(10).unwrap()
    };
    let reopened = Database::open(&path).unwrap();
    assert_eq!(reopened.get_transcriptions(10).unwrap(), rows);
    assert_eq!(
        reopened.get_analytics_summary().unwrap().total_dictations,
        1
    );
}
