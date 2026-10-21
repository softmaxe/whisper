//! Application-wide state shared by the windows: settings, History, the
//! Dictionary and Snippets, and the current Dictation snapshot.
//!
//! The control panel and the dictation window stay in sync by observing this
//! one entity.

use std::sync::Arc;

use gpui_kit::*;

use crate::audio_storage::AudioStorage;
use crate::db::{Database, DictionarySource, Snippet, TranscriptionItem, TranscriptionStatus};
use crate::history::{HistoryStore, Recorded};
use crate::i18n::{self, Language};
use crate::settings::Settings;

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
    pub include_discarded: bool,
    pub dictionary: Vec<String>,
    pub snippets: Vec<Snippet>,
}

impl EventEmitter<AppEvent> for AppState {}

struct AppStateGlobal(Entity<AppState>);

impl Global for AppStateGlobal {}

impl AppState {
    pub fn init(settings: Settings, db: Arc<Database>, cx: &mut App) -> Entity<AppState> {
        i18n::set_language(Language::from_code(&settings.ui_language));
        theme::init(settings.theme, cx);
        let dictionary = db.get_dictionary().unwrap_or_default();
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
            include_discarded: false,
            dictionary,
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
    pub fn load_history(
        &mut self,
        include_discarded: bool,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.include_discarded = include_discarded;
        let result = self.db.get_transcriptions(HISTORY_LIMIT, include_discarded);
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
    /// or `None` when retention kept nothing.
    pub fn record_history(
        &mut self,
        cx: &mut Context<Self>,
        write: impl FnOnce(&HistoryStore, &Settings) -> anyhow::Result<Option<Recorded>>,
    ) -> anyhow::Result<Option<i64>> {
        let Some(recorded) = write(&self.history_store, &self.settings)? else {
            return Ok(None);
        };
        let id = recorded.item.id;
        self.upsert_history_item(recorded.item, cx);
        if recorded.insights_changed {
            cx.emit(AppEvent::AnalyticsChanged);
        }
        Ok(Some(id))
    }

    /// Inserts or replaces a row: hidden statuses drop out unless shown,
    /// newest first, capped at the limit.
    pub fn upsert_history_item(&mut self, item: TranscriptionItem, cx: &mut Context<Self>) {
        self.history.retain(|existing| existing.id != item.id);
        let hidden = matches!(
            item.status,
            TranscriptionStatus::Failed | TranscriptionStatus::Discarded
        ) && !self.include_discarded;
        if !hidden {
            self.history.push(item);
            self.history.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            self.history.truncate(HISTORY_LIMIT);
        }
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

    /// Deletes every stored recording, keeping the transcripts.
    pub fn delete_all_audio(&mut self, cx: &mut Context<Self>) {
        self.audio.delete_all_audio_and_flags(&self.db);
        if let Err(err) = self.load_history(self.include_discarded, cx) {
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

    /// Learns corrections from an edit the user made right after a paste.
    /// Returns the words added to the Dictionary.
    pub fn learn_corrections(
        &mut self,
        original: &str,
        edited: &str,
        cx: &mut Context<Self>,
    ) -> Vec<String> {
        let learned = crate::text::correction_learner::extract_corrections(
            original,
            edited,
            &self.dictionary,
        );
        if learned.is_empty() {
            return learned;
        }
        let none: [&str; 0] = [];
        match self.change_dictionary(&learned, &none, DictionarySource::Learned, cx) {
            Ok(()) => learned,
            Err(err) => {
                log::warn!("failed to save learned words: {err}");
                Vec::new()
            }
        }
    }

    fn reload_dictionary(&mut self, cx: &mut Context<Self>) {
        self.dictionary = self.db.get_dictionary().unwrap_or_default();
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
