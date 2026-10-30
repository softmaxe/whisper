//! Saved dictation audio and the retention sweep.
//!
//! Files are named `Whisper-YYYY-MM-DD-HH-MM-SS-<id>.wav` in local wall-clock
//! time, or `Whisper-<id>.wav` when the row has no usable timestamp. Lookups
//! match the trailing `-<id>` of the file stem.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{Local, TimeZone};

use crate::db::{Database, timestamp::parse_db_timestamp};
use crate::paths;
use crate::settings::Settings;

/// Extension of saved recordings; other files in the audio directory are
/// ignored.
const AUDIO_EXTENSION: &str = "wav";

const DAY: Duration = Duration::from_secs(86_400);

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(AUDIO_EXTENSION))
}

/// The transcription id a saved file belongs to: the part after the last `-`
/// of `Whisper-...-<id>`.
fn id_from_stem(stem: &str) -> Option<&str> {
    stem.rsplit_once('-').map(|(_, id)| id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StorageUsage {
    pub file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioCleanup {
    pub deleted: usize,
    pub kept: usize,
}

pub struct AudioStorage {
    dir: PathBuf,
}

impl Default for AudioStorage {
    fn default() -> Self {
        Self::new(paths::audio_dir())
    }
}

impl AudioStorage {
    pub fn new(dir: PathBuf) -> Self {
        if let Err(err) = fs::create_dir_all(&dir) {
            log::error!("Failed to create audio directory {}: {err}", dir.display());
        }
        Self { dir }
    }

    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// File name for a recording, named in the user's local wall clock. The
    /// stored timestamp is resolved first: a bare SQLite value is UTC.
    fn build_filename(transcription_id: i64, timestamp: &str) -> String {
        Self::build_filename_in(transcription_id, timestamp, &Local)
    }

    fn build_filename_in<Tz: TimeZone>(transcription_id: i64, timestamp: &str, tz: &Tz) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
        match parse_db_timestamp(timestamp) {
            Some(instant) => {
                let local = instant.with_timezone(tz);
                format!(
                    "Whisper-{}-{transcription_id}.{AUDIO_EXTENSION}",
                    local.format("%Y-%m-%d-%H-%M-%S")
                )
            }
            None => format!("Whisper-{transcription_id}.{AUDIO_EXTENSION}"),
        }
    }

    /// Writes a WAV recording for a transcription. `timestamp` is the row's
    /// stored `timestamp`.
    pub fn save_audio(
        &self,
        transcription_id: i64,
        bytes: &[u8],
        timestamp: &str,
    ) -> anyhow::Result<PathBuf> {
        let path = self
            .dir
            .join(Self::build_filename(transcription_id, timestamp));
        fs::create_dir_all(&self.dir)?;
        fs::write(&path, bytes)?;
        log::debug!("Audio saved: id={transcription_id} size={}", bytes.len());
        Ok(path)
    }

    fn audio_files(&self) -> std::io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.is_file() && is_audio_file(&path) {
                files.push(path);
            }
        }
        Ok(files)
    }

    /// The saved recording of a transcription.
    pub fn audio_path(&self, transcription_id: i64) -> Option<PathBuf> {
        let id = transcription_id.to_string();
        let mut files = self.audio_files().ok()?;
        files.sort();
        files.into_iter().find(|path| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|stem| id_from_stem(stem) == Some(id.as_str()))
        })
    }

    pub fn read_audio(&self, transcription_id: i64) -> Option<Vec<u8>> {
        let path = self.audio_path(transcription_id)?;
        fs::read(&path)
            .map_err(|err| log::error!("Failed to read audio {}: {err}", path.display()))
            .ok()
    }

    /// Deletes a transcription's recording; succeeds when there was none.
    pub fn delete_audio(&self, transcription_id: i64) -> bool {
        let Some(path) = self.audio_path(transcription_id) else {
            return true;
        };
        match fs::remove_file(&path) {
            Ok(()) => true,
            Err(err) => {
                log::error!("Failed to delete audio {}: {err}", path.display());
                false
            }
        }
    }

    /// Deletes recordings older than `retention_days` (by file mtime) and
    /// clears their rows' `has_audio` flags.
    pub fn cleanup_expired_audio(&self, retention_days: u32, db: &Database) -> AudioCleanup {
        let cutoff = SystemTime::now()
            .checked_sub(DAY * retention_days)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let files = match self.audio_files() {
            Ok(files) => files,
            Err(err) => {
                log::error!("Audio cleanup failed: {err}");
                return AudioCleanup::default();
            }
        };
        let mut expired_ids = Vec::new();
        let mut result = AudioCleanup::default();
        for path in files {
            let modified = match fs::metadata(&path).and_then(|m| m.modified()) {
                Ok(modified) => modified,
                Err(err) => {
                    log::error!("Failed to stat {} during cleanup: {err}", path.display());
                    continue;
                }
            };
            if modified >= cutoff {
                result.kept += 1;
                continue;
            }
            if let Err(err) = fs::remove_file(&path) {
                log::error!("Failed to delete {} during cleanup: {err}", path.display());
                continue;
            }
            result.deleted += 1;
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if let Some(Ok(id)) = id_from_stem(stem).map(str::parse::<i64>) {
                expired_ids.push(id);
            }
        }
        if !expired_ids.is_empty()
            && let Err(err) = db.clear_audio_flags(&expired_ids)
        {
            log::error!("Failed to clear audio flags after cleanup: {err:#}");
        }
        log::info!(
            "Audio cleanup complete: deleted={} kept={} retention_days={retention_days}",
            result.deleted,
            result.kept
        );
        result
    }

    /// Deletes every saved recording. Returns how many files were found.
    pub fn delete_all_audio(&self) -> usize {
        let files = match self.audio_files() {
            Ok(files) => files,
            Err(err) => {
                log::error!("Failed to delete all audio: {err}");
                return 0;
            }
        };
        for path in &files {
            if let Err(err) = fs::remove_file(path) {
                log::error!("Failed to delete audio file {}: {err}", path.display());
            }
        }
        log::info!("All audio deleted: count={}", files.len());
        files.len()
    }

    /// Deletes every recording and clears every row's `has_audio` flag.
    pub fn delete_all_audio_and_flags(&self, db: &Database) -> usize {
        let deleted = self.delete_all_audio();
        if let Err(err) = db.clear_all_audio_flags() {
            log::error!("Failed to clear audio flags after delete-all: {err:#}");
        }
        deleted
    }

    pub fn storage_usage(&self) -> StorageUsage {
        let files = match self.audio_files() {
            Ok(files) => files,
            Err(err) => {
                log::error!("Failed to get storage usage: {err}");
                return StorageUsage::default();
            }
        };
        let total_bytes = files
            .iter()
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .sum();
        StorageUsage {
            file_count: files.len(),
            total_bytes,
        }
    }
}

/// Retention preferences the sweep acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionSettings {
    /// Days to keep recordings; 0 disables audio cleanup.
    pub audio_retention_days: u32,
    /// Days to keep transcripts; 0 keeps them forever.
    pub transcript_retention_days: u32,
}

impl From<&Settings> for RetentionSettings {
    fn from(settings: &Settings) -> Self {
        Self {
            audio_retention_days: settings.audio_retention_days,
            transcript_retention_days: settings.transcript_retention_days,
        }
    }
}

/// What a retention sweep removed, so the caller can refresh views.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetentionCleanupReport {
    /// History rows removed; each needs a "transcription deleted" refresh.
    pub deleted_transcription_ids: Vec<i64>,
    /// Insights counters removed; refresh Insights when non-zero.
    pub analytics_purged: usize,
    pub audio: AudioCleanup,
}

/// The retention sweep: failed and discarded rows left by older versions,
/// expired transcripts with their audio and counters, then expired
/// recordings. A retention of 0 days disables that half.
pub fn run_retention_cleanup(
    db: &Database,
    storage: &AudioStorage,
    settings: RetentionSettings,
) -> RetentionCleanupReport {
    let mut report = RetentionCleanupReport::default();
    match db.delete_hidden_transcriptions() {
        Ok(ids) => {
            for id in &ids {
                storage.delete_audio(*id);
            }
            report.deleted_transcription_ids = ids;
        }
        Err(err) => log::error!("Failed to remove failed and discarded rows: {err:#}"),
    }
    if settings.transcript_retention_days > 0 {
        match db.delete_transcriptions_expired_before(settings.transcript_retention_days) {
            Ok(purge) => {
                for id in &purge.ids {
                    storage.delete_audio(*id);
                }
                report.deleted_transcription_ids.extend(purge.ids);
                report.analytics_purged = purge.analytics_purged;
            }
            Err(err) => log::error!("Retention cleanup failed: {err:#}"),
        }
    }
    if settings.audio_retention_days > 0 {
        report.audio = storage.cleanup_expired_audio(settings.audio_retention_days, db);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TranscriptionAudioUpdate;
    use chrono::FixedOffset;

    fn storage() -> (tempfile::TempDir, AudioStorage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = AudioStorage::new(dir.path().join("audio"));
        (dir, storage)
    }

    fn set_mtime_days_ago(path: &Path, days: u64) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - DAY * days as u32)
            .unwrap();
    }

    #[test]
    fn names_audio_in_local_time_with_or_without_a_zone() {
        // 12:00 UTC is 05:00 in Los Angeles in September.
        let la = FixedOffset::west_opt(7 * 3600).unwrap();
        for ts in ["2026-09-10 12:00:00.000Z", "2026-09-10 12:00:00"] {
            assert_eq!(
                AudioStorage::build_filename_in(42, ts, &la),
                "Whisper-2026-09-10-05-00-00-42.wav"
            );
        }
    }

    #[test]
    fn falls_back_to_the_bare_name() {
        assert_eq!(AudioStorage::build_filename(42, ""), "Whisper-42.wav");
        assert_eq!(
            AudioStorage::build_filename(42, "not a date"),
            "Whisper-42.wav"
        );
    }

    #[test]
    fn finds_audio_by_trailing_id() {
        let (_tmp, storage) = storage();
        let wav = storage
            .save_audio(42, b"RIFF", "2026-09-10 12:00:00")
            .unwrap();
        let bare = storage.save_audio(7, b"bare", "").unwrap();
        fs::write(storage.dir().join("142.wav"), b"x").unwrap();
        fs::write(storage.dir().join("notes-42.txt"), b"not audio").unwrap();

        assert_eq!(storage.audio_path(42), Some(wav));
        assert_eq!(storage.audio_path(7), Some(bare));
        assert_eq!(storage.read_audio(7).as_deref(), Some(&b"bare"[..]));
        assert_eq!(
            storage.audio_path(142),
            None,
            "a stem without `-<id>` is not a recording"
        );
        assert_eq!(storage.audio_path(4), None);
        assert_eq!(
            storage.storage_usage(),
            StorageUsage {
                file_count: 3,
                total_bytes: 4 + 4 + 1
            }
        );

        assert!(storage.delete_audio(42));
        assert_eq!(storage.audio_path(42), None);
        assert!(storage.delete_audio(42), "deleting a missing file succeeds");
        assert_eq!(storage.delete_all_audio(), 2);
        assert_eq!(storage.storage_usage(), StorageUsage::default());
        assert!(storage.dir().join("notes-42.txt").exists());
    }

    #[test]
    fn expired_audio_is_deleted_and_flags_cleared() {
        let (_tmp, storage) = storage();
        let db = Database::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for text in ["old", "new"] {
            let row = db.save_transcription(text, None, None).unwrap();
            db.update_transcription_audio(
                row.id,
                &TranscriptionAudioUpdate {
                    has_audio: true,
                    ..Default::default()
                },
            )
            .unwrap();
            storage.save_audio(row.id, b"data", &row.timestamp).unwrap();
            ids.push(row.id);
        }
        let old_path = storage.audio_path(ids[0]).unwrap();
        set_mtime_days_ago(&old_path, 10);

        let result = storage.cleanup_expired_audio(7, &db);
        assert_eq!(
            result,
            AudioCleanup {
                deleted: 1,
                kept: 1
            }
        );
        assert!(!old_path.exists());
        assert!(
            !db.get_transcription_by_id(ids[0])
                .unwrap()
                .unwrap()
                .has_audio
        );
        assert!(
            db.get_transcription_by_id(ids[1])
                .unwrap()
                .unwrap()
                .has_audio
        );
    }

    #[test]
    fn retention_sweep_removes_expired_transcripts_with_their_audio() {
        let (_tmp, storage) = storage();
        let db = Database::open_in_memory().unwrap();
        let old = db.save_transcription("old", None, None).unwrap();
        db.lock()
            .execute(
                "UPDATE transcriptions SET created_at = datetime('now', '-5 days') WHERE id = ?",
                [old.id],
            )
            .unwrap();
        let fresh = db.save_transcription("fresh", None, None).unwrap();
        storage.save_audio(old.id, b"a", &old.timestamp).unwrap();
        storage
            .save_audio(fresh.id, b"b", &fresh.timestamp)
            .unwrap();

        let disabled = run_retention_cleanup(
            &db,
            &storage,
            RetentionSettings {
                audio_retention_days: 0,
                transcript_retention_days: 0,
            },
        );
        assert_eq!(disabled, RetentionCleanupReport::default());
        assert_eq!(storage.storage_usage().file_count, 2);

        let report = run_retention_cleanup(
            &db,
            &storage,
            RetentionSettings {
                audio_retention_days: 30,
                transcript_retention_days: 1,
            },
        );
        assert_eq!(report.deleted_transcription_ids, [old.id]);
        assert_eq!(
            report.audio,
            AudioCleanup {
                deleted: 0,
                kept: 1
            }
        );
        assert!(storage.audio_path(old.id).is_none());
        assert!(storage.audio_path(fresh.id).is_some());
    }

    #[test]
    fn retention_sweep_removes_failed_and_discarded_rows_with_their_audio() {
        let (_tmp, storage) = storage();
        let db = Database::open_in_memory().unwrap();
        // Older versions wrote failed and discarded rows.
        let hidden: Vec<i64> = ["failed", "discarded"]
            .into_iter()
            .map(|status| {
                let row = db.save_transcription("", None, None).unwrap();
                db.lock()
                    .execute(
                        "UPDATE transcriptions SET status = ? WHERE id = ?",
                        rusqlite::params![status, row.id],
                    )
                    .unwrap();
                storage.save_audio(row.id, b"a", &row.timestamp).unwrap();
                row.id
            })
            .collect();
        let kept = db.save_transcription("kept", None, None).unwrap();
        storage.save_audio(kept.id, b"b", &kept.timestamp).unwrap();

        let report = run_retention_cleanup(
            &db,
            &storage,
            RetentionSettings {
                audio_retention_days: 0,
                transcript_retention_days: 0,
            },
        );
        assert_eq!(report.deleted_transcription_ids, hidden);
        assert!(hidden.iter().all(|id| storage.audio_path(*id).is_none()));
        assert!(storage.audio_path(kept.id).is_some());
        let remaining: Vec<i64> = db
            .lock()
            .prepare("SELECT id FROM transcriptions")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(remaining, [kept.id]);
    }

    #[test]
    fn delete_all_audio_clears_every_flag() {
        let (_tmp, storage) = storage();
        let db = Database::open_in_memory().unwrap();
        let row = db.save_transcription("x", None, None).unwrap();
        db.update_transcription_audio(
            row.id,
            &TranscriptionAudioUpdate {
                has_audio: true,
                ..Default::default()
            },
        )
        .unwrap();
        storage.save_audio(row.id, b"a", &row.timestamp).unwrap();
        assert_eq!(storage.delete_all_audio_and_flags(&db), 1);
        assert!(
            !db.get_transcription_by_id(row.id)
                .unwrap()
                .unwrap()
                .has_audio
        );
    }
}
