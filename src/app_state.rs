//! Application-wide state shared by the windows: settings, History, the
//! Dictionary and Snippets, and the current Dictation snapshot.
//!
//! The control panel and the dictation window stay in sync by observing this
//! one entity.

mod dictionary_snapshot;

use dictionary_snapshot::DictionarySnapshot;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::*;

use crate::audio_storage::{AudioStorage, RetentionCleanupReport};
use crate::db::{Database, DictionaryEntry, DictionarySource, Snippet, TranscriptionItem};
use crate::history::{HistoryStore, HistoryWrite};
use crate::i18n::{self, Language};
use crate::native::text_monitor::SessionReport;
use crate::settings::Settings;
use crate::text::correction_learner::KnownCorrection;

use crate::ui::theme;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DictationSnapshot {
    pub is_recording: bool,
    pub is_processing: bool,
    pub is_preparing: bool,
}

impl DictationSnapshot {
    pub fn is_active(&self) -> bool {
        self.is_recording || self.is_processing || self.is_preparing
    }
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    SettingsChanged,
    HistoryChanged,
    DictionaryChanged,
    SnippetsChanged,
    AnalyticsChanged,
}

/// How many History rows the Home view keeps loaded.
pub const HISTORY_LIMIT: usize = 50;

/// A failed read is retryable; an empty successful read is still Loaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryLoadState {
    #[default]
    Unloaded,
    Loading,
    Loaded,
    Failed,
}

impl HistoryLoadState {
    pub fn needs_load(self) -> bool {
        matches!(self, Self::Unloaded | Self::Failed)
    }

    /// A late read cannot replace a newer History mutation or its status.
    fn finish(&mut self, requested: u64, current: u64, succeeded: bool) -> bool {
        if requested != current {
            return false;
        }
        *self = if succeeded {
            Self::Loaded
        } else {
            Self::Failed
        };
        true
    }
}

pub struct AppState {
    pub settings: Settings,
    pub dictation: DictationSnapshot,
    pub db: Arc<Database>,
    pub audio: Arc<AudioStorage>,
    pub history_store: Arc<HistoryStore>,
    pub history: Arc<Vec<TranscriptionItem>>,
    pub history_load: HistoryLoadState,
    history_revision: u64,
    dictionary_revision: Arc<AtomicU64>,
    dictionary_write_revision: u64,
    dictionary_snapshot_sequence: u64,
    snippets_revision: u64,
    pub dictionary: Vec<String>,
    /// The same words as `dictionary`, with source and usage.
    pub dictionary_entries: Vec<DictionaryEntry>,
    pub known_corrections: Vec<KnownCorrection>,
    pub snippets: Vec<Snippet>,
}

impl EventEmitter<AppEvent> for AppState {}

struct AppStateGlobal(Entity<AppState>);

impl Global for AppStateGlobal {}

impl AppState {
    pub fn init(settings: Settings, db: Arc<Database>, cx: &mut App) -> Entity<AppState> {
        i18n::set_language(Language::from_code(&settings.ui_language));
        theme::init(settings.theme, cx);
        let dictionary_entries = db.get_dictionary_entries().unwrap_or_default();
        let dictionary = dictionary_entries.iter().map(|e| e.word.clone()).collect();
        let known_corrections = db.get_known_corrections().unwrap_or_default();
        let snippets = db.get_snippets().unwrap_or_default();
        let audio = Arc::new(AudioStorage::default());
        let state = cx.new(|_| AppState {
            settings,
            dictation: DictationSnapshot::default(),
            history_store: Arc::new(HistoryStore::new(db.clone(), audio.clone())),
            db,
            audio,
            history: Arc::new(Vec::new()),
            history_load: HistoryLoadState::Unloaded,
            history_revision: 0,
            dictionary_revision: Arc::new(AtomicU64::new(0)),
            dictionary_write_revision: 0,
            dictionary_snapshot_sequence: 0,
            snippets_revision: 0,
            dictionary,
            dictionary_entries,
            known_corrections,
            snippets,
        });
        cx.set_global(AppStateGlobal(state.clone()));
        state
    }

    pub fn invalidate_learning(&mut self) {
        self.dictionary_revision.fetch_add(1, Ordering::SeqCst);
    }

    pub fn global(cx: &App) -> Entity<AppState> {
        cx.global::<AppStateGlobal>().0.clone()
    }

    pub fn settings(cx: &App) -> Settings {
        cx.global::<AppStateGlobal>().0.read(cx).settings.clone()
    }

    /// Applies a settings change, persists it, and runs app-wide side effects
    /// (theme, UI language).
    pub fn update_settings(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Settings)) {
        let before = self.settings.clone();
        f(&mut self.settings);
        if self.settings == before {
            return;
        }
        let settings = self.settings.clone();
        let save = crate::services::runtime::storage(move || settings.save());
        cx.spawn(async move |_, _| {
            if let Err(err) = save.await {
                log::warn!("failed to save settings: {err}");
            }
        })
        .detach();
        if before.auto_learn_corrections != self.settings.auto_learn_corrections {
            self.dictionary_revision.fetch_add(1, Ordering::SeqCst);
        }
        if before.theme != self.settings.theme {
            theme::apply(self.settings.theme, cx);
        }
        if before.ui_language != self.settings.ui_language {
            i18n::set_language(Language::from_code(&self.settings.ui_language));
            cx.refresh_windows();
        }
        cx.emit(AppEvent::SettingsChanged);
        cx.notify();
    }
}

impl AppState {
    /// Loads the newest rows. Failed and discarded rows stay hidden.
    pub fn load_history(&mut self, cx: &mut Context<Self>) -> Task<anyhow::Result<()>> {
        self.history_load = HistoryLoadState::Loading;
        cx.notify();
        self.history_revision += 1;
        let revision = self.history_revision;
        let db = self.db.clone();
        let job = crate::services::runtime::storage(move || db.get_transcriptions(HISTORY_LIMIT));
        cx.spawn(async move |this, cx| {
            let rows = job.await;
            let result = rows
                .as_ref()
                .map(|_| ())
                .map_err(|err| anyhow::anyhow!(err.to_string()));
            this.update(cx, |this, cx| this.apply_history_read(revision, rows, cx))?;
            result
        })
    }

    fn apply_history_read(
        &mut self,
        revision: u64,
        rows: anyhow::Result<Vec<TranscriptionItem>>,
        cx: &mut Context<Self>,
    ) {
        if !self
            .history_load
            .finish(revision, self.history_revision, rows.is_ok())
        {
            return;
        }
        match rows {
            Ok(rows) => {
                self.history = Arc::new(rows);
                cx.emit(AppEvent::HistoryChanged);
            }
            Err(err) => log::warn!("failed to load History: {err}"),
        }
        cx.notify();
    }

    /// Serializes a History write with deletion and retention. Accepted writes
    /// finish even if their caller stops awaiting; stale snapshots never replace
    /// a more recent UI mutation.
    pub fn record_history(
        &mut self,
        cx: &mut Context<Self>,
        write: impl FnOnce(&HistoryStore, &Settings) -> HistoryWrite + Send + 'static,
    ) -> Task<anyhow::Result<Option<i64>>> {
        self.history_revision += 1;
        let revision = self.history_revision;
        let dictionary_revision = self.dictionary_revision.load(Ordering::SeqCst);
        let (store, settings, db) = (
            self.history_store.clone(),
            self.settings.clone(),
            self.db.clone(),
        );
        let job = crate::services::runtime::storage(move || {
            let written = write(&store, &settings);
            let rows = db.get_transcriptions(HISTORY_LIMIT);
            let dictionary = written
                .dictionary_changed
                .then(|| DictionarySnapshot::read(&db));
            (written.result, rows, dictionary)
        });
        cx.spawn(async move |this, cx| {
            let (written, rows, dictionary) = job.await;
            let id = written
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref())
                .map(|r| r.item.id);
            this.update(cx, |this, cx| {
                this.apply_history_read(revision, rows, cx);
                if this.dictionary_revision.load(Ordering::SeqCst) == dictionary_revision
                    && let Some(dictionary) = dictionary
                {
                    this.apply_dictionary_snapshot(dictionary, cx);
                }
                if written
                    .as_ref()
                    .ok()
                    .and_then(|r| r.as_ref())
                    .is_some_and(|r| r.insights_changed)
                {
                    cx.emit(AppEvent::AnalyticsChanged);
                }
            })?;
            written?;
            Ok(id)
        })
    }

    pub fn remove_history_item(&mut self, id: i64, cx: &mut Context<Self>) {
        let before = self.history.len();
        Arc::make_mut(&mut self.history).retain(|item| item.id != id);
        if self.history.len() != before {
            cx.emit(AppEvent::HistoryChanged);
            cx.notify();
        }
    }

    pub fn delete_transcription(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<bool>> {
        self.history_revision += 1;
        self.remove_history_item(id, cx);
        let (db, audio) = (self.db.clone(), self.audio.clone());
        let job = crate::services::runtime::storage(move || {
            let deleted = db.delete_transcription(id)?;
            if deleted {
                audio.delete_audio(id);
            }
            Ok(deleted)
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                this.load_history(cx).detach();
                cx.emit(AppEvent::AnalyticsChanged);
            })?;
            result
        })
    }

    pub fn clear_transcriptions(&mut self, cx: &mut Context<Self>) -> Task<anyhow::Result<()>> {
        self.history_revision += 1;
        self.history = Arc::new(Vec::new());
        cx.emit(AppEvent::HistoryChanged);
        cx.notify();
        let (db, audio) = (self.db.clone(), self.audio.clone());
        let job = crate::services::runtime::storage(move || -> anyhow::Result<()> {
            db.clear_transcriptions()?;
            audio.delete_all_audio();
            Ok(())
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                this.load_history(cx).detach();
                cx.emit(AppEvent::AnalyticsChanged);
            })?;
            result
        })
    }

    /// Reconciles the UI after the serialized retention job completes.
    pub fn apply_retention_cleanup(
        &mut self,
        report: &RetentionCleanupReport,
        cx: &mut Context<Self>,
    ) {
        if !report.deleted_transcription_ids.is_empty() || report.audio.deleted > 0 {
            if self.history_load != HistoryLoadState::Unloaded {
                self.load_history(cx).detach();
            }
            cx.emit(AppEvent::AnalyticsChanged);
        } else if report.analytics_purged > 0 {
            cx.emit(AppEvent::AnalyticsChanged);
        }
    }

    pub fn delete_all_audio(&mut self, cx: &mut Context<Self>) -> Task<()> {
        self.history_revision += 1;
        let (db, audio) = (self.db.clone(), self.audio.clone());
        let job = crate::services::runtime::storage(move || audio.delete_all_audio_and_flags(&db));
        cx.spawn(async move |this, cx| {
            job.await;
            let _ = this.update(cx, |this, cx| {
                this.load_history(cx).detach();
            });
        })
    }

    /// Publishes the edit immediately for consecutive UI actions, then persists
    /// it on the storage worker and reconciles with the authoritative rows.
    pub fn change_dictionary<A: AsRef<str>, R: AsRef<str>>(
        &mut self,
        add: &[A],
        remove: &[R],
        source: DictionarySource,
        cx: &mut Context<Self>,
    ) {
        DictionaryEntry::project_changes(&mut self.dictionary_entries, add, remove, source);
        // SQLite's NOCASE collation folds ASCII only. Match its correction
        // joins, including case-only respellings and newly protected words.
        let entries: std::collections::HashMap<String, &str> = self
            .dictionary_entries
            .iter()
            .map(|entry| (entry.word.to_ascii_lowercase(), entry.word.as_str()))
            .collect();
        self.known_corrections.retain_mut(|correction| {
            let Some(&word) = entries.get(&correction.term.to_ascii_lowercase()) else {
                return false;
            };
            correction.term = word.to_string();
            entries
                .get(&correction.misheard.to_ascii_lowercase())
                .is_none_or(|other| *other == word)
        });
        let add: Vec<String> = add.iter().map(|word| word.as_ref().to_string()).collect();
        let remove: Vec<String> = remove
            .iter()
            .map(|word| word.as_ref().to_string())
            .collect();
        self.persist_dictionary_edit(cx, "failed to update the dictionary", move |db| {
            db.apply_dictionary_changes(&add, &remove, source)
        });
    }

    /// Forgets one way `term` was misheard.
    pub fn remove_correction(&mut self, misheard: &str, term: &str, cx: &mut Context<Self>) {
        self.known_corrections.retain(|correction| {
            !(correction.misheard.eq_ignore_ascii_case(misheard)
                && correction.term.eq_ignore_ascii_case(term))
        });
        let (misheard, term) = (misheard.to_string(), term.to_string());
        self.persist_dictionary_edit(cx, "failed to remove a correction", move |db| {
            db.remove_correction(&misheard, &term)
        });
    }

    /// Empties the Dictionary and what auto-learn knows about it.
    pub fn clear_dictionary(&mut self, cx: &mut Context<Self>) {
        self.dictionary_entries.clear();
        self.known_corrections.clear();
        self.persist_dictionary_edit(
            cx,
            "failed to clear the dictionary",
            Database::clear_dictionary,
        );
    }

    fn persist_dictionary_edit(
        &mut self,
        cx: &mut Context<Self>,
        error_context: &'static str,
        write: impl FnOnce(&Database) -> anyhow::Result<()> + Send + 'static,
    ) {
        self.dictionary_revision.fetch_add(1, Ordering::SeqCst);
        self.dictionary_write_revision += 1;
        let revision = self.dictionary_write_revision;
        self.publish_dictionary(cx);
        let db = self.db.clone();
        let job = crate::services::runtime::storage(move || {
            if let Err(err) = write(&db) {
                log::warn!("{error_context}: {err}");
            }
            // Read even after failure, so an optimistic edit cannot remain as
            // phantom data. Keep the old reload behavior on read failure.
            DictionarySnapshot::read(&db).default_on_read_failure()
        });
        cx.spawn(async move |this, cx| {
            let snapshot = job.await;
            let _ = this.update(cx, |this, cx| {
                if this.dictionary_write_revision == revision {
                    this.apply_dictionary_snapshot(snapshot, cx);
                }
            });
        })
        .detach();
    }

    /// Learns from a finished post-paste session. Returns the words added to
    /// the Dictionary.
    pub fn learn_from_session(
        &mut self,
        report: SessionReport,
        cx: &mut Context<Self>,
    ) -> Task<Vec<String>> {
        let revision = self.dictionary_revision.load(Ordering::SeqCst);
        let token = self.dictionary_revision.clone();
        cx.spawn(async move |this, cx| {
            let SessionReport { edit, .. } = report;
            let corrections = if let Some(edit) = edit {
                crate::services::runtime::blocking(move || {
                    crate::text::correction_learner::extract_corrections(
                        &edit.baseline,
                        &edit.edited,
                        &edit.pasted,
                    )
                })
                .await
                .unwrap_or_default()
            } else {
                Vec::new()
            };
            let Ok(Some(db)) = this.update(cx, |this, _| {
                (this.dictionary_revision.load(Ordering::SeqCst) == revision
                    && this.settings.auto_learn_corrections)
                    .then(|| this.db.clone())
            }) else {
                return Vec::new();
            };
            let (learned, dictionary) = crate::services::runtime::storage(move || {
                let learned = db
                    .record_corrections_if_current(&corrections, &token, revision)
                    .inspect_err(|err| log::warn!("failed to save learned words: {err}"))
                    .unwrap_or_default();
                let dictionary = (!corrections.is_empty()).then(|| DictionarySnapshot::read(&db));
                (learned, dictionary)
            })
            .await;
            this.update(cx, |this, cx| {
                if this.dictionary_revision.load(Ordering::SeqCst) != revision {
                    return Vec::new();
                }
                if let Some(dictionary) = dictionary {
                    this.apply_dictionary_snapshot(dictionary, cx);
                }
                learned
            })
            .unwrap_or_default()
        })
    }

    fn apply_dictionary_snapshot(&mut self, snapshot: DictionarySnapshot, cx: &mut Context<Self>) {
        if snapshot.apply(
            &mut self.dictionary_snapshot_sequence,
            &mut self.dictionary_entries,
            &mut self.known_corrections,
        ) {
            self.publish_dictionary(cx);
        }
    }

    fn publish_dictionary(&mut self, cx: &mut Context<Self>) {
        self.dictionary = self
            .dictionary_entries
            .iter()
            .map(|e| e.word.clone())
            .collect();
        cx.emit(AppEvent::DictionaryChanged);
        cx.notify();
    }

    pub fn set_snippets(&mut self, snippets: Vec<Snippet>, cx: &mut Context<Self>) {
        self.snippets_revision += 1;
        let revision = self.snippets_revision;
        let before = self.snippets.clone();
        self.snippets = Snippet::project_list(&before, &snippets);
        cx.emit(AppEvent::SnippetsChanged);
        cx.notify();
        let db = self.db.clone();
        let job = crate::services::runtime::storage(move || {
            if let Err(err) = db.apply_snippet_changes(&before, &snippets) {
                log::warn!("failed to save snippets: {err}");
            }
            db.get_snippets().unwrap_or(snippets)
        });
        cx.spawn(async move |this, cx| {
            let snippets = job.await;
            let _ = this.update(cx, |this, cx| {
                if this.snippets_revision == revision {
                    this.snippets = snippets;
                    cx.emit(AppEvent::SnippetsChanged);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod history_load_tests {
    use super::HistoryLoadState;

    #[test]
    fn failed_load_can_be_retried_and_an_empty_success_is_loaded() {
        let mut state = HistoryLoadState::default();
        assert!(state.needs_load());
        state = HistoryLoadState::Loading;
        assert!(!state.needs_load());
        assert!(state.finish(1, 1, false));
        assert_eq!(state, HistoryLoadState::Failed);
        assert!(state.needs_load());
        state = HistoryLoadState::Loading;
        assert!(state.finish(2, 2, true));
        assert_eq!(state, HistoryLoadState::Loaded);
        assert!(!state.needs_load());
    }

    #[test]
    fn stale_reads_do_not_replace_a_newer_load_or_write_status() {
        let mut state = HistoryLoadState::Loading;
        assert!(!state.finish(1, 2, true));
        assert_eq!(state, HistoryLoadState::Loading);
        assert!(state.finish(2, 2, true));
        assert!(!state.finish(1, 2, false));
        assert_eq!(state, HistoryLoadState::Loaded);
    }
}
