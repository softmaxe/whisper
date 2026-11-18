//! History recording: turns a delivered Dictation, a History retry or an
//! upload into its History row, its stored recording and its Insights counter.
//! Failed and cancelled Dictations are not kept.
//!
//! Each write applies the retention settings and keeps a row and its audio
//! consistent. `AppState` reloads the latest rows after each write.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, ensure};
use chrono::{DateTime, Utc};

use crate::analytics::AnalyticsEventInput;
use crate::audio_storage::AudioStorage;
use crate::db::{Database, TranscriptionAudioUpdate, TranscriptionItem};
use crate::pipeline::Transcript;
use crate::settings::Settings;

/// A finished recording, saved next to its row when audio retention is on.
#[derive(Debug, Clone, Copy)]
pub struct Recording<'a> {
    /// 16 kHz WAV.
    pub wav: &'a [u8],
    pub duration: Duration,
    pub occurred_at: DateTime<Utc>,
}

/// A saved row ID and the independent changes to Dictionary usage and Insights.
/// Both counters can be committed even when a later History write fails.
pub struct HistoryWrite {
    pub result: anyhow::Result<Option<i64>>,
    pub dictionary_changed: bool,
    pub insights_changed: bool,
}

pub struct HistoryStore {
    db: Arc<Database>,
    audio: Arc<AudioStorage>,
}

impl HistoryStore {
    pub fn new(db: Arc<Database>, audio: Arc<AudioStorage>) -> Self {
        Self { db, audio }
    }

    /// A delivered Dictation. Dictionary usage is counted even with retention
    /// off; the row, its audio and the Insights counter need retention.
    pub fn record_delivered(
        &self,
        transcript: &Transcript,
        recording: &Recording,
        settings: &Settings,
    ) -> HistoryWrite {
        let dictionary_changed = self
            .db
            .record_dictionary_usage(&transcript.spoken_text)
            .inspect_err(|err| log::warn!("failed to record dictionary usage: {err}"))
            .is_ok_and(|updated| updated > 0);
        if !settings.data_retention_enabled {
            return HistoryWrite {
                result: Ok(None),
                dictionary_changed,
                insights_changed: false,
            };
        }
        let insights_changed =
            self.record_insights(transcript, recording.occurred_at, recording.duration);
        let result = (|| {
            let item = self.db.save_transcription(
                &transcript.text,
                Some(&transcript.raw_text),
                Some(recording.occurred_at),
            )?;
            // The transcript row remains available even if saving the audio fails.
            self.store_audio(&item, recording, settings)
                .context("failed to save transcription audio")?;
            Ok(Some(item.id))
        })();
        HistoryWrite {
            result,
            dictionary_changed,
            insights_changed,
        }
    }

    /// An uploaded file's raw transcript. Uploads skip cleanup, Snippets and
    /// Insights.
    pub fn record_upload(&self, text: &str, settings: &Settings) -> HistoryWrite {
        let result = if settings.data_retention_enabled {
            self.db
                .save_transcription(text, None, None)
                .map(|item| Some(item.id))
        } else {
            Ok(None)
        };
        HistoryWrite {
            result,
            dictionary_changed: false,
            insights_changed: false,
        }
    }

    /// The saved recording a retry re-transcribes.
    pub fn retry_audio(&self, id: i64) -> Option<Vec<u8>> {
        self.audio.read_audio(id)
    }

    /// Replaces a row's text with a retry's transcript; its audio is unchanged.
    pub fn apply_retry(&self, id: i64, transcript: &Transcript) -> HistoryWrite {
        let result = (|| {
            let updated = self.db.update_transcription_text(
                id,
                &transcript.text,
                Some(&transcript.raw_text),
            )?;
            ensure!(updated, "History row {id} no longer exists");
            Ok(Some(id))
        })();
        HistoryWrite {
            result,
            dictionary_changed: false,
            insights_changed: false,
        }
    }

    /// Adds the Insights counter; a failure only loses the counter.
    fn record_insights(
        &self,
        transcript: &Transcript,
        occurred_at: DateTime<Utc>,
        duration: Duration,
    ) -> bool {
        let duration_ms = duration.as_millis() as i64;
        let event = AnalyticsEventInput::for_dictation(
            &transcript.text,
            Some(&transcript.raw_text),
            occurred_at,
            (duration_ms > 0).then_some(duration_ms),
        );
        match self.db.record_analytics_event(&event) {
            Ok(recorded) => recorded,
            Err(err) => {
                log::warn!("failed to record the analytics event: {err}");
                false
            }
        }
    }

    /// Saves the recording when audio retention is on and flags the row.
    fn store_audio(
        &self,
        item: &TranscriptionItem,
        recording: &Recording,
        settings: &Settings,
    ) -> anyhow::Result<()> {
        if settings.audio_retention_days == 0 {
            return Ok(());
        }
        self.audio
            .save_audio(item.id, recording.wav, &item.timestamp)?;
        self.db.update_transcription_audio(
            item.id,
            &TranscriptionAudioUpdate {
                has_audio: true,
                audio_duration_ms: Some(recording.duration.as_millis() as i64),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        store: HistoryStore,
        db: Arc<Database>,
        audio: Arc<AudioStorage>,
        _dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open_in_memory().unwrap());
        let audio = Arc::new(AudioStorage::new(dir.path().join("audio")));
        Fixture {
            store: HistoryStore::new(db.clone(), audio.clone()),
            db,
            audio,
            _dir: dir,
        }
    }

    fn transcript(text: &str) -> Transcript {
        Transcript {
            text: text.into(),
            spoken_text: text.into(),
            raw_text: format!("um {text}"),
            cleanup_failure: None,
        }
    }

    const WAV: &[u8] = b"RIFF fake wav";

    fn recording() -> Recording<'static> {
        Recording {
            wav: WAV,
            duration: Duration::from_secs(3),
            occurred_at: Utc::now(),
        }
    }

    fn total_dictations(db: &Database) -> i64 {
        db.get_analytics_summary().unwrap().total_dictations
    }

    #[test]
    fn delivered_dictation_saves_row_audio_and_insights() {
        let f = fixture();
        let recorded =
            f.store
                .record_delivered(&transcript("hello"), &recording(), &Settings::default());
        assert!(recorded.insights_changed);
        assert!(!recorded.dictionary_changed);
        let item =
            f.db.get_transcription_by_id(recorded.result.unwrap().unwrap())
                .unwrap()
                .unwrap();
        assert_eq!(item.text, "hello");
        assert_eq!(item.raw_text.as_deref(), Some("um hello"));
        assert!(item.has_audio);
        assert_eq!(item.audio_duration_ms, Some(3000));
        assert_eq!(f.audio.read_audio(item.id).as_deref(), Some(WAV));
        assert_eq!(total_dictations(&f.db), 1);
    }

    #[test]
    fn retention_off_keeps_nothing_but_still_counts_dictionary_usage() {
        let f = fixture();
        f.db.apply_dictionary_changes(
            &["hello"],
            &[] as &[&str],
            crate::db::DictionarySource::Manual,
        )
        .unwrap();
        let settings = Settings {
            data_retention_enabled: false,
            ..Settings::default()
        };
        let delivered = f
            .store
            .record_delivered(&transcript("hello"), &recording(), &settings)
            .result
            .unwrap();
        assert_eq!(delivered, None);
        assert!(f.db.get_transcriptions(10).unwrap().is_empty());
        assert_eq!(total_dictations(&f.db), 0);
        assert_eq!(f.db.get_dictionary_entries().unwrap()[0].use_count, 1);
        assert_eq!(
            f.store.record_upload("text", &settings).result.unwrap(),
            None
        );
    }

    #[test]
    fn uploads_are_saved_without_insights() {
        let f = fixture();
        let recorded = f.store.record_upload("file text", &Settings::default());
        assert!(!recorded.insights_changed);
        let item =
            f.db.get_transcription_by_id(recorded.result.unwrap().unwrap())
                .unwrap()
                .unwrap();
        assert_eq!(item.text, "file text");
        assert_eq!(item.raw_text, None);
        assert_eq!(total_dictations(&f.db), 0);
    }

    #[test]
    fn retrying_a_row_replaces_its_text_without_counting_it_again() {
        let f = fixture();
        let id = f
            .store
            .record_delivered(&transcript("hello"), &recording(), &Settings::default())
            .result
            .unwrap()
            .unwrap();
        assert_eq!(f.store.retry_audio(id).as_deref(), Some(WAV));

        let recorded = f.store.apply_retry(id, &transcript("again"));
        assert!(!recorded.insights_changed);
        assert_eq!(recorded.result.unwrap(), Some(id));
        let item = f.db.get_transcription_by_id(id).unwrap().unwrap();
        assert_eq!(item.text, "again");
        assert_eq!(item.raw_text.as_deref(), Some("um again"));
        assert!(item.has_audio);
        assert_eq!(item.audio_duration_ms, Some(3000));
        assert_eq!(total_dictations(&f.db), 1);
    }

    #[test]
    fn retrying_a_missing_row_fails() {
        let f = fixture();
        assert!(f.store.apply_retry(42, &transcript("x")).result.is_err());
    }

    #[test]
    fn audio_write_failure_is_reported_while_the_transcript_stays_saved() {
        let f = fixture();
        let audio_dir = f._dir.path().join("audio");
        std::fs::remove_dir(&audio_dir).unwrap();
        // A file at the directory path makes the write fail without changing
        // permissions or touching the installed application's data.
        std::fs::write(&audio_dir, b"blocked").unwrap();
        let write = f.store.record_delivered(
            &transcript("recoverable words"),
            &recording(),
            &Settings::default(),
        );
        assert!(write.result.is_err());
        assert!(write.insights_changed);
        assert_eq!(total_dictations(&f.db), 1);
        let items = f.db.get_transcriptions(10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "recoverable words");
        assert!(!items[0].has_audio);
        assert!(f.audio.read_audio(items[0].id).is_none());
    }

    #[test]
    fn dictionary_refresh_survives_retention_off_and_partial_history_failure() {
        let f = fixture();
        f.db.apply_dictionary_changes(
            &["hello"],
            &[] as &[&str],
            crate::db::DictionarySource::Manual,
        )
        .unwrap();
        let settings = Settings {
            data_retention_enabled: false,
            ..Settings::default()
        };
        let write = f
            .store
            .record_delivered(&transcript("hello"), &recording(), &settings);
        assert!(write.dictionary_changed);
        assert!(write.result.unwrap().is_none());
        assert_eq!(f.db.get_dictionary_entries().unwrap()[0].use_count, 1);

        // Usage is written before History, so a failed insert must still
        // refresh usage counters in the Dictionary view.
        f.db.lock()
            .execute_batch(
                "CREATE TRIGGER reject_history BEFORE INSERT ON transcriptions
             BEGIN SELECT RAISE(FAIL, 'synthetic history failure'); END;",
            )
            .unwrap();
        let write =
            f.store
                .record_delivered(&transcript("hello"), &recording(), &Settings::default());
        assert!(write.dictionary_changed);
        assert!(write.result.is_err());
        assert_eq!(f.db.get_dictionary_entries().unwrap()[0].use_count, 2);
    }

    #[test]
    fn uploads_and_retries_never_request_a_dictionary_refresh() {
        let f = fixture();
        for enabled in [false, true] {
            let settings = Settings {
                data_retention_enabled: enabled,
                ..Settings::default()
            };
            let write = f.store.record_upload("hello", &settings);
            assert!(!write.dictionary_changed);
            if let Some(recorded) = write.result.unwrap() {
                let retry = f.store.apply_retry(recorded, &transcript("again"));
                assert!(!retry.dictionary_changed);
                assert!(retry.result.is_ok());
            }
        }
        let missing = f.store.apply_retry(i64::MAX, &transcript("missing"));
        assert!(!missing.dictionary_changed);
        assert!(missing.result.is_err());
    }
}
