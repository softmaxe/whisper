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
        provider: Some("self-hosted".into()),
        model: Some("whisper-1".into()),
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
    db.save_transcription(
        text,
        None,
        SaveTranscriptionOptions {
            occurred_at: Some(at(occurred)),
            ..Default::default()
        },
    )
    .unwrap()
}

// ---- transcriptions ----

#[test]
fn saves_and_lists_transcriptions_newest_first() {
    let db = db();
    let first = save(&db, "first", "2026-09-01T10:00:00.000Z");
    let second = save(&db, "second", "2026-09-02T10:00:00.000Z");
    let failed = db
        .save_transcription(
            "oops",
            Some("oops raw"),
            SaveTranscriptionOptions {
                status: TranscriptionStatus::Failed,
                error_message: Some("boom".into()),
                error_code: Some("NETWORK".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(failed.status, TranscriptionStatus::Failed);
    assert_eq!(failed.error_code.as_deref(), Some("NETWORK"));
    assert_eq!(failed.raw_text.as_deref(), Some("oops raw"));

    let visible: Vec<i64> = db
        .get_transcriptions(50, false)
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(visible, [second.id, first.id]);
    let all = db.get_transcriptions(50, true).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].id, failed.id, "saved without a time means now");
    assert_eq!(db.get_transcriptions(1, false).unwrap()[0].id, second.id);
}

#[test]
fn rows_store_timestamps_in_one_shape() {
    let db = db();
    let row = save(&db, "saved words", "2026-09-01T09:58:00.000Z");
    assert_eq!(row.timestamp, "2026-09-01 09:58:00.000Z");
    assert_eq!(row.occurred_at(), Some(at("2026-09-01T09:58:00Z")));
    let plain = db
        .save_transcription("x", None, SaveTranscriptionOptions::default())
        .unwrap();
    for value in [&plain.timestamp, &plain.created_at] {
        let parsed = timestamp::parse_db_timestamp(value).unwrap();
        assert_eq!(&timestamp::format_db_timestamp(&parsed), value);
    }
}

#[test]
fn upload_rows_list_with_their_route() {
    let db = db();
    let upload = db
        .save_transcription(
            "Imported audio transcript",
            None,
            SaveTranscriptionOptions {
                route_kind: Some("upload".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let rows = db.get_transcriptions(50, false).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, upload.id);
    assert_eq!(rows[0].route_kind.as_deref(), Some("upload"));
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
            provider: Some("self-hosted".into()),
            model: Some("whisper-1".into()),
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
    assert_eq!(got.provider.as_deref(), Some("self-hosted"));

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
    let row = db
        .save_transcription(text, None, SaveTranscriptionOptions::default())
        .unwrap();
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
fn counters_are_content_free() {
    let db = db();
    assert!(
        db.record_analytics_event(&event("2026-08-30 10:00:00.000Z"))
            .unwrap()
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM pragma_table_info('analytics_events') WHERE name = 'text'"
        ),
        0
    );
    let summary = db.get_analytics_summary().unwrap();
    assert_eq!((summary.total_words, summary.total_dictations), (4, 1));
}

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
    assert!(db.get_transcriptions(50, true).unwrap().is_empty());
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
            "provider",
            "model",
            "status",
            "error_message",
            "error_code",
            "route_kind",
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
            "provider",
            "model",
        ]
    );
    assert_eq!(count(&db, "PRAGMA user_version"), 1);
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
        db.get_transcriptions(10, true).unwrap()
    };
    let reopened = Database::open(&path).unwrap();
    assert_eq!(reopened.get_transcriptions(10, true).unwrap(), rows);
    assert_eq!(
        reopened.get_analytics_summary().unwrap().total_dictations,
        1
    );
}
