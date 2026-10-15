//! SQLite persistence: History, Dictionary, Snippets and Insights counters.

mod dictionary;
mod insights;
mod schema;
mod snippets;
pub mod timestamp;

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::{Mutex, MutexGuard};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

pub use dictionary::DictionarySource;
pub use snippets::Snippet;

/// Shared handle to the database; cheap to share behind an `Arc`.
pub struct Database {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionStatus {
    #[default]
    Completed,
    Pending,
    /// Transcription or cleanup failed; kept so the user can retry.
    Failed,
    /// A recording the user cancelled, saved only when they opted in.
    Discarded,
}

impl TranscriptionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Pending => "pending",
            Self::Failed => "failed",
            Self::Discarded => "discarded",
        }
    }

    /// Unknown values read as `Completed`, the column's default, which is
    /// also how History's status filter treats them.
    pub fn parse(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "failed" => Self::Failed,
            "discarded" => Self::Discarded,
            _ => Self::Completed,
        }
    }
}

/// One History row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptionItem {
    pub id: i64,
    pub text: String,
    pub raw_text: Option<String>,
    /// When the dictation was spoken, in the shape [`timestamp`] describes;
    /// read it with [`TranscriptionItem::occurred_at`].
    pub timestamp: String,
    /// When the row was written; the transcript retention window counts from it.
    pub created_at: String,
    pub has_audio: bool,
    pub audio_duration_ms: Option<i64>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub status: TranscriptionStatus,
    pub error_message: Option<String>,
    pub error_code: Option<String>,
    pub route_kind: Option<String>,
}

impl TranscriptionItem {
    pub fn occurred_at(&self) -> Option<DateTime<Utc>> {
        timestamp::parse_db_timestamp(&self.timestamp)
    }
}

const TRANSCRIPTION_COLUMNS: &str = "id, text, raw_text, timestamp, created_at, has_audio, \
     audio_duration_ms, provider, model, status, error_message, error_code, route_kind";

fn transcription_from_row(row: &Row<'_>) -> rusqlite::Result<TranscriptionItem> {
    Ok(TranscriptionItem {
        id: row.get(0)?,
        text: row.get(1)?,
        raw_text: row.get(2)?,
        timestamp: row.get(3)?,
        created_at: row.get(4)?,
        has_audio: row.get(5)?,
        audio_duration_ms: row.get(6)?,
        provider: row.get(7)?,
        model: row.get(8)?,
        status: TranscriptionStatus::parse(&row.get::<_, String>(9)?),
        error_message: row.get(10)?,
        error_code: row.get(11)?,
        route_kind: row.get(12)?,
    })
}

/// Optional fields of [`Database::save_transcription`].
#[derive(Debug, Clone, Default)]
pub struct SaveTranscriptionOptions {
    pub status: TranscriptionStatus,
    pub error_message: Option<String>,
    pub error_code: Option<String>,
    /// Dictation intent such as "translation" or "upload".
    pub route_kind: Option<String>,
    /// When the dictation was spoken; `None` means now.
    pub occurred_at: Option<DateTime<Utc>>,
}

/// Audio metadata written once a recording is stored next to its row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscriptionAudioUpdate {
    pub has_audio: bool,
    pub audio_duration_ms: Option<i64>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// Result of the transcript retention sweep.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExpiredPurge {
    /// Transcriptions removed; delete their audio files.
    pub ids: Vec<i64>,
    /// Insights counters removed alongside; refresh Insights when non-zero.
    pub analytics_purged: usize,
}

impl Database {
    /// Opens (creating if needed) the database file and sets up its schema.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Self::from_connection(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        schema::init(&conn).context("initializing database schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock()
    }

    pub fn save_transcription(
        &self,
        text: &str,
        raw_text: Option<&str>,
        options: SaveTranscriptionOptions,
    ) -> Result<TranscriptionItem> {
        let occurred_at =
            timestamp::format_db_timestamp(&options.occurred_at.unwrap_or_else(Utc::now));
        let conn = self.lock();
        conn.execute(
            "INSERT INTO transcriptions (
               text, raw_text, status, error_message, error_code, route_kind, timestamp
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                text,
                raw_text,
                options.status.as_str(),
                options.error_message,
                options.error_code,
                options.route_kind,
                occurred_at
            ],
        )?;
        let id = conn.last_insert_rowid();
        Ok(conn.query_row(
            &format!("SELECT {TRANSCRIPTION_COLUMNS} FROM transcriptions WHERE id = ?"),
            [id],
            transcription_from_row,
        )?)
    }

    /// Newest first. Failed and discarded rows are hidden unless
    /// `include_discarded`.
    pub fn get_transcriptions(
        &self,
        limit: usize,
        include_discarded: bool,
    ) -> Result<Vec<TranscriptionItem>> {
        let status_filter = if include_discarded {
            ""
        } else {
            " WHERE status NOT IN ('failed', 'discarded')"
        };
        let conn = self.lock();
        let rows = conn
            .prepare(&format!(
                "SELECT {TRANSCRIPTION_COLUMNS} FROM transcriptions{status_filter} ORDER BY timestamp DESC LIMIT ?"
            ))?
            .query_map([limit as i64], transcription_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn get_transcription_by_id(&self, id: i64) -> Result<Option<TranscriptionItem>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {TRANSCRIPTION_COLUMNS} FROM transcriptions WHERE id = ?"),
                [id],
                transcription_from_row,
            )
            .optional()?)
    }

    /// Clears History and Insights. Returns how many transcriptions were removed.
    pub fn clear_transcriptions(&self) -> Result<usize> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let cleared = tx.execute("DELETE FROM transcriptions", [])?;
        tx.execute("DELETE FROM analytics_events", [])?;
        tx.commit()?;
        Ok(cleared)
    }

    /// Deletes transcriptions created, and Insights counters for dictations
    /// spoken, more than `retention_days` ago.
    pub fn delete_transcriptions_expired_before(
        &self,
        retention_days: u32,
    ) -> Result<ExpiredPurge> {
        let cutoff = timestamp::format_db_timestamp(
            &(Utc::now() - Duration::days(i64::from(retention_days))),
        );
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids: Vec<i64> = tx
            .prepare("DELETE FROM transcriptions WHERE created_at < ? RETURNING id")?
            .query_map([&cutoff], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let analytics_purged = insights::purge_before(&tx, &cutoff)?;
        tx.commit()?;
        Ok(ExpiredPurge {
            ids,
            analytics_purged,
        })
    }

    /// Returns false when the row does not exist.
    pub fn delete_transcription(&self, id: i64) -> Result<bool> {
        let changed = self
            .lock()
            .execute("DELETE FROM transcriptions WHERE id = ?", [id])?;
        Ok(changed > 0)
    }

    pub fn update_transcription_audio(
        &self,
        id: i64,
        update: &TranscriptionAudioUpdate,
    ) -> Result<()> {
        self.lock().execute(
            "UPDATE transcriptions SET has_audio = ?, audio_duration_ms = ?, provider = ?, model = ? WHERE id = ?",
            params![update.has_audio, update.audio_duration_ms, update.provider, update.model, id],
        )?;
        Ok(())
    }

    pub fn update_transcription_text(
        &self,
        id: i64,
        text: &str,
        raw_text: Option<&str>,
    ) -> Result<()> {
        self.lock().execute(
            "UPDATE transcriptions SET text = ?, raw_text = ? WHERE id = ?",
            params![text, raw_text, id],
        )?;
        Ok(())
    }

    pub fn update_transcription_status(
        &self,
        id: i64,
        status: TranscriptionStatus,
        error_message: Option<&str>,
        error_code: Option<&str>,
    ) -> Result<()> {
        self.lock().execute(
            "UPDATE transcriptions SET status = ?, error_message = ?, error_code = ? WHERE id = ?",
            params![status.as_str(), error_message, error_code, id],
        )?;
        Ok(())
    }

    pub fn clear_audio_flags(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut stmt = tx.prepare("UPDATE transcriptions SET has_audio = 0 WHERE id = ?")?;
            for id in ids {
                stmt.execute([id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Clears `has_audio` on every row, after all audio files were deleted.
    pub fn clear_all_audio_flags(&self) -> Result<usize> {
        Ok(self.lock().execute(
            "UPDATE transcriptions SET has_audio = 0 WHERE has_audio = 1",
            [],
        )?)
    }
}

#[cfg(test)]
mod tests;
