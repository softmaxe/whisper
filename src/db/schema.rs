//! Schema setup. Every statement is idempotent, so opening a database runs the
//! whole script; `user_version` records which schema the file holds.

use anyhow::Result;
use rusqlite::Connection;

use super::timestamp::SQL_NOW;

const SCHEMA_VERSION: i64 = 2;

pub(super) fn init(conn: &Connection) -> Result<()> {
    // Wait briefly rather than fail when another connection holds the lock.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;

    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS transcriptions (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          text TEXT NOT NULL,
          raw_text TEXT,
          -- When the dictation was spoken.
          timestamp TEXT NOT NULL,
          created_at TEXT NOT NULL DEFAULT ({SQL_NOW}),
          has_audio INTEGER NOT NULL DEFAULT 0,
          audio_duration_ms INTEGER,
          -- provider, model, error_message, error_code and route_kind are no
          -- longer written; they stay so older databases keep working.
          provider TEXT,
          model TEXT,
          status TEXT NOT NULL DEFAULT 'completed',
          error_message TEXT,
          error_code TEXT,
          route_kind TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_transcriptions_timestamp ON transcriptions(timestamp);

        CREATE TABLE IF NOT EXISTS custom_dictionary (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          word TEXT NOT NULL UNIQUE COLLATE NOCASE,
          source TEXT NOT NULL DEFAULT 'manual',
          use_count INTEGER NOT NULL DEFAULT 0,
          last_used_at TEXT,
          created_at TEXT NOT NULL DEFAULT ({SQL_NOW}),
          updated_at TEXT NOT NULL DEFAULT ({SQL_NOW})
        );

        -- How the user corrected delivered text: what was misheard and the term meant.
        CREATE TABLE IF NOT EXISTS dictionary_corrections (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          misheard TEXT NOT NULL COLLATE NOCASE,
          term TEXT NOT NULL COLLATE NOCASE,
          count INTEGER NOT NULL DEFAULT 1,
          updated_at TEXT NOT NULL DEFAULT ({SQL_NOW}),
          UNIQUE (misheard, term)
        );

        -- Words the user removed; auto-learn must not bring them back.
        CREATE TABLE IF NOT EXISTS dictionary_rejected_words (
          word TEXT PRIMARY KEY COLLATE NOCASE
        );

        -- Per-app auto-learn outcomes, keyed by bundle identifier: no text.
        CREATE TABLE IF NOT EXISTS auto_learn_stats (
          app TEXT PRIMARY KEY,
          sessions INTEGER NOT NULL DEFAULT 0,
          readable INTEGER NOT NULL DEFAULT 0,
          edited INTEGER NOT NULL DEFAULT 0,
          learned INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS snippets (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          trigger TEXT NOT NULL UNIQUE COLLATE NOCASE,
          replacement TEXT NOT NULL,
          created_at TEXT NOT NULL DEFAULT ({SQL_NOW})
        );

        CREATE TABLE IF NOT EXISTS analytics_events (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          occurred_at TEXT NOT NULL,
          local_date TEXT NOT NULL,
          word_count INTEGER NOT NULL CHECK (word_count > 0),
          spoken_duration_ms INTEGER CHECK (spoken_duration_ms > 0),
          -- provider and model are no longer written.
          provider TEXT,
          model TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_analytics_events_local_date ON analytics_events(local_date);

        PRAGMA user_version = {SCHEMA_VERSION};"
    ))?;
    Ok(())
}
