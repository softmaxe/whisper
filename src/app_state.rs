//! Application-wide state shared by the windows: settings, History, the
//! Dictionary and Snippets, and the current Dictation snapshot.
//!
//! The control panel and the dictation window stay in sync by observing this
//! one entity.

use std::sync::Arc;

use gpui_kit::*;

use crate::audio_storage::{AudioStorage, RetentionCleanupReport};
use crate::db::{Database, DictionaryEntry, DictionarySource, Snippet, TranscriptionItem};
use crate::history::{HistoryStore, Recorded};
use crate::i18n::{self, Language};
use crate::native::text_monitor::{SessionReport, TextEdit};
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

pub struct AppState {
    pub settings: Settings,
    pub dictation: DictationSnapshot,
    pub db: Arc<Database>,
    pub audio: Arc<AudioStorage>,
    pub history_store: Arc<HistoryStore>,
    pub history: Vec<TranscriptionItem>,
    pub history_loaded: bool,
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
            history: Vec::new(),
            history_loaded: false,
            dictionary,
            dictionary_entries,
            known_corrections,
            snippets,
        });
        cx.set_global(AppStateGlobal(state.clone()));
        state
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
        if let Err(err) = self.settings.save() {
            log::warn!("failed to save settings: {err}");
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
    pub fn load_history(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let result = self.db.get_transcriptions(HISTORY_LIMIT);
        self.history_loaded = true;
        match result {
            Ok(items) => {
                self.history = items;
                cx.emit(AppEvent::HistoryChanged);
                cx.notify();
                Ok(())
            }
            Err(err) => {
                cx.notify();
                Err(err)
            }
        }
    }

    /// Runs a History write and shows the row it returns. Returns the row id,
    /// or `None` when retention kept nothing. The write may count Dictionary
    /// usage, so the Dictionary is reloaded either way.
    pub fn record_history(
        &mut self,
        cx: &mut Context<Self>,
        write: impl FnOnce(&HistoryStore, &Settings) -> anyhow::Result<Option<Recorded>>,
    ) -> anyhow::Result<Option<i64>> {
        let written = write(&self.history_store, &self.settings);
        self.reload_dictionary(cx);
        let Some(recorded) = written? else {
            return Ok(None);
        };
        let id = recorded.item.id;
        self.upsert_history_item(recorded.item, cx);
        if recorded.insights_changed {
            cx.emit(AppEvent::AnalyticsChanged);
        }
        Ok(Some(id))
    }

    /// Inserts or replaces a row, newest first, capped at the limit.
    pub fn upsert_history_item(&mut self, item: TranscriptionItem, cx: &mut Context<Self>) {
        self.history.retain(|existing| existing.id != item.id);
        self.history.push(item);
        self.history.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        self.history.truncate(HISTORY_LIMIT);
        cx.emit(AppEvent::HistoryChanged);
        cx.notify();
    }

    pub fn remove_history_item(&mut self, id: i64, cx: &mut Context<Self>) {
        let before = self.history.len();
        self.history.retain(|item| item.id != id);
        if self.history.len() != before {
            cx.emit(AppEvent::HistoryChanged);
            cx.notify();
        }
    }

    pub fn delete_transcription(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<bool> {
        let deleted = self.db.delete_transcription(id)?;
        if deleted {
            self.audio.delete_audio(id);
            self.remove_history_item(id, cx);
            cx.emit(AppEvent::AnalyticsChanged);
        }
        Ok(deleted)
    }

    pub fn clear_transcriptions(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        self.db.clear_transcriptions()?;
        self.audio.delete_all_audio();
        self.history.clear();
        cx.emit(AppEvent::HistoryChanged);
        cx.emit(AppEvent::AnalyticsChanged);
        cx.notify();
        Ok(())
    }

    /// Shows what a background retention sweep removed: reloads History when
    /// rows or recordings went away, and Insights when its counters changed.
    pub fn apply_retention_cleanup(
        &mut self,
        report: &RetentionCleanupReport,
        cx: &mut Context<Self>,
    ) {
        let history_changed =
            !report.deleted_transcription_ids.is_empty() || report.audio.deleted > 0;
        // An unloaded History loads fresh rows when it opens.
        let reloaded = history_changed
            && self.history_loaded
            && self
                .load_history(cx)
                .inspect_err(|err| log::warn!("failed to reload History: {err}"))
                .is_ok();
        // A History reload already refreshes Insights.
        if !reloaded && (history_changed || report.analytics_purged > 0) {
            cx.emit(AppEvent::AnalyticsChanged);
        }
    }

    /// Deletes every stored recording, keeping the transcripts.
    pub fn delete_all_audio(&mut self, cx: &mut Context<Self>) {
        self.audio.delete_all_audio_and_flags(&self.db);
        if let Err(err) = self.load_history(cx) {
            log::warn!("failed to reload History: {err}");
        }
    }

    /// Adds and removes Dictionary words, then shows the stored list.
    pub fn change_dictionary<A: AsRef<str>, R: AsRef<str>>(
        &mut self,
        add: &[A],
        remove: &[R],
        source: DictionarySource,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let result = self.db.apply_dictionary_changes(add, remove, source);
        self.reload_dictionary(cx);
        result.map(|_| ())
    }

    /// Forgets one way `term` was misheard.
    pub fn remove_correction(&mut self, misheard: &str, term: &str, cx: &mut Context<Self>) {
        if let Err(err) = self.db.remove_correction(misheard, term) {
            log::warn!("failed to remove a correction: {err}");
        }
        self.reload_dictionary(cx);
    }

    /// Empties the Dictionary and what auto-learn knows about it.
    pub fn clear_dictionary(&mut self, cx: &mut Context<Self>) {
        if let Err(err) = self.db.clear_dictionary() {
            log::warn!("failed to clear the dictionary: {err}");
        }
        self.reload_dictionary(cx);
    }

    /// Learns from a finished post-paste session. Returns the words added to
    /// the Dictionary.
    pub fn learn_from_session(
        &mut self,
        report: SessionReport,
        cx: &mut Context<Self>,
    ) -> Vec<String> {
        let SessionReport {
            app,
            readable,
            edit,
        } = report;
        let edited = edit.is_some();
        let learned = edit.map_or_else(Vec::new, |edit| self.learn_corrections(&edit, cx));
        log::info!(
            "[auto-learn] app={app:?} readable={readable} edited={edited} learned={}",
            learned.len()
        );
        if let Some(app) = app
            && let Err(err) =
                self.db
                    .record_auto_learn_session(&app, readable, edited, learned.len())
        {
            log::warn!("failed to save auto-learn stats: {err}");
        }
        learned
    }

    fn learn_corrections(&mut self, edit: &TextEdit, cx: &mut Context<Self>) -> Vec<String> {
        let corrections = crate::text::correction_learner::extract_corrections(
            &edit.baseline,
            &edit.edited,
            &edit.pasted,
        );
        if corrections.is_empty() {
            return Vec::new();
        }
        let learned = self
            .db
            .record_corrections(&corrections)
            .inspect_err(|err| log::warn!("failed to save learned words: {err}"))
            .unwrap_or_default();
        self.reload_dictionary(cx);
        learned
    }

    fn reload_dictionary(&mut self, cx: &mut Context<Self>) {
        self.dictionary_entries = self.db.get_dictionary_entries().unwrap_or_default();
        self.dictionary = self
            .dictionary_entries
            .iter()
            .map(|e| e.word.clone())
            .collect();
        self.known_corrections = self.db.get_known_corrections().unwrap_or_default();
        cx.emit(AppEvent::DictionaryChanged);
        cx.notify();
    }

    pub fn set_snippets(&mut self, snippets: Vec<Snippet>, cx: &mut Context<Self>) {
        if let Err(err) = self.db.set_snippets(&snippets) {
            log::warn!("failed to save snippets: {err}");
        }
        self.snippets = self.db.get_snippets().unwrap_or(snippets);
        cx.emit(AppEvent::SnippetsChanged);
        cx.notify();
    }
}
