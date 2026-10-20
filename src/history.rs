//! History recording: turns a delivered, failed or discarded Dictation, a
//! History retry or an upload into its History row, its stored recording and
//! its Insights counter.
//!
//! Each write applies the retention settings and keeps a row and its audio
//! consistent. `AppState` publishes the rows returned here to the windows.

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use chrono::{DateTime, Utc};

use crate::analytics::AnalyticsEventInput;
use crate::audio_storage::AudioStorage;
use crate::db::{
    Database, SaveTranscriptionOptions, TranscriptionAudioUpdate, TranscriptionItem,
    TranscriptionStatus,
};
use crate::pipeline::Transcript;
use crate::settings::Settings;

/// Recordings shorter than this are not kept as discarded. See #907.
const MIN_DISCARDED_DURATION: Duration = Duration::from_secs(1);

/// A finished recording, saved next to its row when audio retention is on.
#[derive(Debug, Clone, Copy)]
pub struct Recording<'a> {
    /// 16 kHz WAV.
    pub wav: &'a [u8],
    pub duration: Duration,
    pub occurred_at: DateTime<Utc>,
}

/// A row a write created or changed, for the windows to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Recorded {
    pub item: TranscriptionItem,
    /// An Insights counter was added.
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
    ) -> anyhow::Result<Option<Recorded>> {
        if let Err(err) = self.db.record_dictionary_usage(&transcript.spoken_text) {
            log::warn!("failed to record dictionary usage: {err}");
        }
        if !settings.data_retention_enabled {
            return Ok(None);
        }
        let insights_changed =
            self.record_insights(transcript, recording.occurred_at, recording.duration);
        let options = SaveTranscriptionOptions {
            occurred_at: Some(recording.occurred_at),
            ..Default::default()
        };
        let item =
            self.db
                .save_transcription(&transcript.text, Some(&transcript.raw_text), options)?;
        let audio = AudioMeta {
            provider: Some(&transcript.source),
            model: transcript.model.as_deref(),
        };
        if let Err(err) = self.store_audio(&item, recording, audio, settings) {
            // The transcript is still useful without its recording.
            log::warn!("failed to save transcription audio: {err}");
        }
        Ok(Some(Recorded {
            item: self.reread(item),
            insights_changed,
        }))
    }

    /// A Dictation whose transcription failed, kept so it can be retried.
    pub fn record_failed(
        &self,
        recording: &Recording,
        message: &str,
        code: Option<&str>,
        settings: &Settings,
    ) -> anyhow::Result<Option<Recorded>> {
        if !settings.data_retention_enabled {
            return Ok(None);
        }
        let options = SaveTranscriptionOptions {
            status: TranscriptionStatus::Failed,
            error_message: Some(message.to_string()),
            error_code: code.map(str::to_string),
            occurred_at: Some(recording.occurred_at),
            ..Default::default()
        };
        let item = self.db.save_transcription("", None, options)?;
        if let Err(err) = self.store_audio(&item, recording, AudioMeta::default(), settings) {
            log::warn!("failed to save transcription audio: {err}");
        }
        Ok(Some(Recorded {
            item: self.reread(item),
            insights_changed: false,
        }))
    }

    /// Whether a cancelled recording would be kept, checked before encoding it.
    pub fn keeps_discarded(duration: Duration, settings: &Settings) -> bool {
        settings.save_discarded_transcriptions
            && settings.data_retention_enabled
            && settings.audio_retention_days > 0
            && duration >= MIN_DISCARDED_DURATION
    }

    /// A recording the user cancelled. Its audio is the only way to recover
    /// it, so no row is kept without it (#907).
    pub fn record_discarded(
        &self,
        recording: &Recording,
        settings: &Settings,
    ) -> anyhow::Result<Option<Recorded>> {
        if !Self::keeps_discarded(recording.duration, settings) {
            return Ok(None);
        }
        let options = SaveTranscriptionOptions {
            status: TranscriptionStatus::Discarded,
            occurred_at: Some(recording.occurred_at),
            ..Default::default()
        };
        let item = self.db.save_transcription("", None, options)?;
        if let Err(err) = self.store_audio(&item, recording, AudioMeta::default(), settings) {
            let _ = self.db.delete_transcription(item.id);
            return Err(err);
        }
        Ok(Some(Recorded {
            item: self.reread(item),
            insights_changed: false,
        }))
    }

    /// An uploaded file's raw transcript. Uploads skip cleanup, Snippets and
    /// Insights.
    pub fn record_upload(
        &self,
        text: &str,
        settings: &Settings,
    ) -> anyhow::Result<Option<Recorded>> {
        if !settings.data_retention_enabled {
            return Ok(None);
        }
        let options = SaveTranscriptionOptions {
            route_kind: Some("upload".into()),
            ..Default::default()
        };
        let item = self.db.save_transcription(text, None, options)?;
        Ok(Some(Recorded {
            item,
            insights_changed: false,
        }))
    }

    /// The saved recording a retry re-transcribes.
    pub fn retry_audio(&self, id: i64) -> Option<Vec<u8>> {
        self.audio.read_audio(id)
    }

    /// Replaces a row's text with a retry's transcript. A failed or discarded
    /// row becomes a delivered Dictation, so it is counted in Insights and
    /// Dictionary usage once; a completed row only gets the new text.
    pub fn apply_retry(
        &self,
        id: i64,
        transcript: &Transcript,
        settings: &Settings,
    ) -> anyhow::Result<Option<Recorded>> {
        let before = self
            .db
            .get_transcription_by_id(id)?
            .ok_or_else(|| anyhow!("History row {id} no longer exists"))?;
        let recovered = before.status != TranscriptionStatus::Completed;
        self.db
            .update_transcription_text(id, &transcript.text, Some(&transcript.raw_text))?;
        self.db
            .update_transcription_status(id, TranscriptionStatus::Completed, None, None)?;
        self.db.update_transcription_audio(
            id,
            &TranscriptionAudioUpdate {
                has_audio: before.has_audio,
                audio_duration_ms: before.audio_duration_ms,
                provider: Some(transcript.source.clone()),
                model: transcript.model.clone(),
            },
        )?;
        let mut insights_changed = false;
        if recovered {
            if let Err(err) = self.db.record_dictionary_usage(&transcript.spoken_text) {
                log::warn!("failed to record dictionary usage: {err}");
            }
            if settings.data_retention_enabled {
                let duration =
                    Duration::from_millis(before.audio_duration_ms.unwrap_or(0).max(0) as u64);
                let occurred_at = before.occurred_at().unwrap_or_else(Utc::now);
                insights_changed = self.record_insights(transcript, occurred_at, duration);
            }
        }
        let item = self.reread(before);
        Ok(Some(Recorded {
            item,
            insights_changed,
        }))
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
            Some(&transcript.source),
            transcript.model.as_deref(),
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
        meta: AudioMeta,
        settings: &Settings,
    ) -> anyhow::Result<()> {
        if settings.audio_retention_days == 0 {
            return Ok(());
        }
        self.audio
            .save_audio(item.id, recording.wav, Some(&item.timestamp), "wav")?;
        self.db.update_transcription_audio(
            item.id,
            &TranscriptionAudioUpdate {
                has_audio: true,
                audio_duration_ms: Some(recording.duration.as_millis() as i64),
                provider: meta.provider.map(str::to_string),
                model: meta.model.map(str::to_string),
            },
        )
    }

    /// The row as stored now, or `item` when it cannot be read back.
    fn reread(&self, item: TranscriptionItem) -> TranscriptionItem {
        self.db
            .get_transcription_by_id(item.id)
            .ok()
            .flatten()
            .unwrap_or(item)
    }
}

#[derive(Default)]
struct AudioMeta<'a> {
    provider: Option<&'a str>,
    model: Option<&'a str>,
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
            source: "self-hosted".into(),
            model: Some("whisper-1".into()),
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
        let recorded = f
            .store
            .record_delivered(&transcript("hello"), &recording(), &Settings::default())
            .unwrap()
            .unwrap();
        assert!(recorded.insights_changed);
        let item = recorded.item;
        assert_eq!(item.text, "hello");
        assert_eq!(item.raw_text.as_deref(), Some("um hello"));
        assert!(item.has_audio);
        assert_eq!(item.audio_duration_ms, Some(3000));
        assert_eq!(item.provider.as_deref(), Some("self-hosted"));
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
            .unwrap();
        assert_eq!(delivered, None);
        assert!(f.db.get_transcriptions(10, true).unwrap().is_empty());
        assert_eq!(total_dictations(&f.db), 0);
        assert_eq!(f.db.get_dictionary_by_priority().unwrap(), vec!["hello"]);
        assert_eq!(
            f.store
                .record_failed(&recording(), "boom", None, &settings)
                .unwrap(),
            None
        );
        assert_eq!(f.store.record_upload("text", &settings).unwrap(), None);
    }

    #[test]
    fn audio_retention_off_saves_the_row_without_audio() {
        let f = fixture();
        let settings = Settings {
            audio_retention_days: 0,
            ..Settings::default()
        };
        let item = f
            .store
            .record_failed(&recording(), "boom", Some("CODE"), &settings)
            .unwrap()
            .unwrap()
            .item;
        assert_eq!(item.status, TranscriptionStatus::Failed);
        assert_eq!(item.error_code.as_deref(), Some("CODE"));
        assert!(!item.has_audio);
        assert_eq!(f.audio.read_audio(item.id), None);
    }

    #[test]
    fn discarded_recordings_need_the_option_retention_and_a_second_of_audio() {
        let f = fixture();
        let opted_in = Settings {
            save_discarded_transcriptions: true,
            ..Settings::default()
        };
        assert_eq!(
            f.store
                .record_discarded(&recording(), &Settings::default())
                .unwrap(),
            None
        );
        let short = Recording {
            duration: Duration::from_millis(500),
            ..recording()
        };
        assert_eq!(f.store.record_discarded(&short, &opted_in).unwrap(), None);
        let no_audio = Settings {
            audio_retention_days: 0,
            ..opted_in.clone()
        };
        assert_eq!(
            f.store.record_discarded(&recording(), &no_audio).unwrap(),
            None
        );
        let item = f
            .store
            .record_discarded(&recording(), &opted_in)
            .unwrap()
            .unwrap()
            .item;
        assert_eq!(item.status, TranscriptionStatus::Discarded);
        assert!(item.has_audio);
    }

    #[test]
    fn discarded_row_is_removed_when_its_audio_cannot_be_saved() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("audio");
        let db = Arc::new(Database::open_in_memory().unwrap());
        let store = HistoryStore::new(db.clone(), Arc::new(AudioStorage::new(blocked.clone())));
        // A file where the directory should be makes every save fail.
        std::fs::remove_dir_all(&blocked).unwrap();
        std::fs::write(&blocked, b"").unwrap();
        let settings = Settings {
            save_discarded_transcriptions: true,
            ..Settings::default()
        };
        assert!(store.record_discarded(&recording(), &settings).is_err());
        assert!(db.get_transcriptions(10, true).unwrap().is_empty());
    }

    #[test]
    fn uploads_are_saved_without_insights() {
        let f = fixture();
        let recorded = f
            .store
            .record_upload("file text", &Settings::default())
            .unwrap()
            .unwrap();
        assert!(!recorded.insights_changed);
        assert_eq!(recorded.item.route_kind.as_deref(), Some("upload"));
        assert_eq!(total_dictations(&f.db), 0);
    }

    #[test]
    fn retrying_a_failed_row_counts_it_once() {
        let f = fixture();
        let settings = Settings::default();
        let failed = f
            .store
            .record_failed(&recording(), "boom", Some("CODE"), &settings)
            .unwrap()
            .unwrap()
            .item;
        let id = failed.id;
        assert_eq!(f.store.retry_audio(id).as_deref(), Some(WAV));

        let recorded = f
            .store
            .apply_retry(id, &transcript("recovered"), &settings)
            .unwrap()
            .unwrap();
        assert!(recorded.insights_changed);
        let item = recorded.item;
        assert_eq!(item.text, "recovered");
        assert_eq!(item.status, TranscriptionStatus::Completed);
        assert_eq!(item.error_code, None);
        assert!(item.has_audio);
        assert_eq!(item.audio_duration_ms, Some(3000));
        assert_eq!(item.model.as_deref(), Some("whisper-1"));
        assert_eq!(total_dictations(&f.db), 1);

        // Retrying the now-completed row only replaces the text.
        let again = f
            .store
            .apply_retry(id, &transcript("again"), &settings)
            .unwrap()
            .unwrap();
        assert!(!again.insights_changed);
        assert_eq!(again.item.text, "again");
        assert_eq!(total_dictations(&f.db), 1);
    }

    #[test]
    fn retrying_a_missing_row_fails() {
        let f = fixture();
        assert!(
            f.store
                .apply_retry(42, &transcript("x"), &Settings::default())
                .is_err()
        );
    }
}
