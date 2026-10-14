//! Dictation: the recording → transcription → cleanup → delivery state
//! machine, plus History retry.
//!
//! One entity owns each Dictation's state, locks and side effects. The
//! hotkey layer reads a lock-free status mirror and sends commands; the pill
//! window renders the snapshot published to `AppState` and the notices
//! emitted here.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use gpui_kit::*;
use tokio_util::sync::CancellationToken;

use whisper::analytics::AnalyticsEventInput;
use whisper::audio::capture::{CaptureEvent, RecordedAudio, Recorder};
use whisper::audio::cues;
use whisper::db::{
    SaveTranscriptionOptions, TranscriptionAudioUpdate, TranscriptionItem, TranscriptionStatus,
};
use whisper::i18n::t;
use whisper::pipeline::{self, PipelineInput, PipelineOutcome};
use whisper::settings::Settings;

use crate::app_state::{AppEvent, AppState, DictationSnapshot, MicCaptureStatus};
use crate::backend;

/// Recordings shorter than this are not kept as discarded. See #907.
const MIN_DISCARDED_DURATION: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Preparing,
    Recording,
    Processing,
}

/// What happened, for the pill window to present (`onNotice`).
#[derive(Clone, Debug)]
pub enum DictationNotice {
    DismissError,
    Error {
        title: String,
        description: String,
    },
    NoAudio,
    PushForceStopped {
        kept_in_clipboard: bool,
        transcript: String,
    },
    DeliveryFallback {
        text: String,
        copied: bool,
    },
    /// Escape while the copy-recovery panel is open closes it.
    DismissRecovery,
    CleanupFailed {
        title: String,
        description: Option<String>,
    },
    LearnedWords(Vec<String>),
    /// A clean press that did nothing; explains how the mode starts Dictation.
    HotkeyHint {
        hold: bool,
    },
    /// The transcript reached its destination: pasted, or saved when
    /// auto-paste is off. Sent just before the phase returns to idle.
    Delivered,
}

/// Lock-free mirror of the phase for the hotkey layer.
#[derive(Clone, Default)]
pub struct StatusMirror(Arc<AtomicU8>);

impl StatusMirror {
    fn set(&self, phase: Phase) {
        self.0.store(phase as u8, Ordering::SeqCst);
    }

    /// Requested, opening the microphone, or recording.
    pub fn is_active(&self) -> bool {
        matches!(self.0.load(Ordering::SeqCst), 1 | 2)
    }

    pub fn is_processing(&self) -> bool {
        self.0.load(Ordering::SeqCst) == 3
    }
}

pub struct Dictation {
    phase: Phase,
    recorder: Recorder,
    generation: u64,
    start_lock: bool,
    cancel: Option<CancellationToken>,
    target_pid: Option<i32>,
    started_at: Option<(Instant, DateTime<Utc>)>,
    effects_held: bool,
    push_force_stopped: bool,
    status: StatusMirror,
    _device_events: Task<()>,
}

impl EventEmitter<DictationNotice> for Dictation {}

struct DictationGlobal(Entity<Dictation>);
impl Global for DictationGlobal {}

impl Dictation {
    pub fn init(cx: &mut App) -> Entity<Dictation> {
        let entity = cx.new(|cx| {
            let (tx, rx) = async_channel::unbounded::<CaptureEvent>();
            let recorder = Recorder::new(Some(Arc::new(move |event| {
                let _ = tx.try_send(event);
            })));
            let device_events = cx.spawn(async move |this, cx| {
                while let Ok(event) = rx.recv().await {
                    let CaptureEvent::DeviceLost { message } = event;
                    log::warn!("microphone lost: {message}");
                    if this
                        .update(cx, |this: &mut Dictation, cx| this.fail_microphone(cx))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            Self {
                phase: Phase::Idle,
                recorder,
                generation: 0,
                start_lock: false,
                cancel: None,
                target_pid: None,
                started_at: None,
                effects_held: false,
                push_force_stopped: false,
                status: StatusMirror::default(),
                _device_events: device_events,
            }
        });
        cx.set_global(DictationGlobal(entity.clone()));
        entity
    }

    pub fn global(cx: &App) -> Entity<Dictation> {
        cx.global::<DictationGlobal>().0.clone()
    }

    pub fn status(&self) -> StatusMirror {
        self.status.clone()
    }

    /// Current input level (RMS) for the waveform, `None` when not recording.
    pub fn level(&self) -> Option<f32> {
        (self.phase == Phase::Recording)
            .then(|| self.recorder.level())
            .flatten()
    }

    fn set_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        let previous = self.phase;
        self.phase = phase;
        self.status.set(phase);
        if phase != Phase::Preparing {
            backend::clear_hotkey_start_request();
        }
        if previous == Phase::Recording && phase != Phase::Recording {
            self.release_recording_effects(cx);
        }
        self.publish(cx);
    }

    fn publish(&self, cx: &mut Context<Self>) {
        let snapshot = DictationSnapshot {
            is_recording: self.phase == Phase::Recording,
            is_processing: self.phase == Phase::Processing,
            is_preparing: self.phase == Phase::Preparing,
            is_stopping: false,
            mic_capture_status: if self.phase == Phase::Recording {
                MicCaptureStatus::Active
            } else {
                MicCaptureStatus::Inactive
            },
            transcript: String::new(),
        };
        AppState::global(cx).update(cx, |s, cx| {
            if s.dictation != snapshot {
                s.dictation = snapshot;
                cx.notify();
            }
        });
        cx.notify();
    }

    fn hold_recording_effects(&mut self, settings: &Settings) {
        self.effects_held = true;
        if settings.pause_media_on_dictation {
            backend::pause_media();
        }
        backend::register_cancel_hotkey();
        if settings.audio_cues_enabled {
            cues::play_start_cue();
        }
    }

    fn release_recording_effects(&mut self, cx: &mut Context<Self>) {
        if !self.effects_held {
            return;
        }
        self.effects_held = false;
        backend::unregister_cancel_hotkey();
        if AppState::settings(cx).pause_media_on_dictation {
            backend::resume_media();
        }
    }

    // ---- Commands ----------------------------------------------------------------------------

    /// Opens the microphone ahead of a likely start (Hold mode key-down).
    pub fn prepare(&mut self, cx: &mut Context<Self>) {
        if self.start_lock || self.phase != Phase::Idle {
            return;
        }
        let device = backend::preferred_input_device(&AppState::settings(cx));
        self.recorder.prepare(device.as_deref());
        self.target_pid = backend::capture_target_pid();
        self.set_phase(Phase::Preparing, cx);
        cx.emit(DictationNotice::DismissError);
    }

    pub fn cancel_preparation(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.start_lock = false;
        self.recorder.cancel_prepared();
        if self.phase == Phase::Preparing {
            self.set_phase(Phase::Idle, cx);
        }
    }

    /// Hotkey toggle: a start still opening the mic counts as active.
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.start_lock || self.phase == Phase::Recording {
            self.stop(cx);
        } else if matches!(self.phase, Phase::Idle | Phase::Preparing) {
            self.start(cx);
        }
    }

    /// Pill click: any visible preparation is something to stop.
    pub fn toggle_listening(&mut self, cx: &mut Context<Self>) {
        match self.phase {
            Phase::Preparing | Phase::Recording => self.stop(cx),
            Phase::Idle => self.start(cx),
            Phase::Processing => {}
        }
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.start_lock || matches!(self.phase, Phase::Recording | Phase::Processing) {
            return;
        }
        self.start_lock = true;
        self.push_force_stopped = false;
        self.generation += 1;
        let generation = self.generation;
        let settings = AppState::settings(cx);
        // The pill window never takes focus, so the frontmost app is still the
        // user's editing target. A hotkey press already captured it.
        if self.phase != Phase::Preparing || self.target_pid.is_none() {
            self.target_pid = backend::capture_target_pid();
        }
        self.set_phase(Phase::Preparing, cx);
        cx.emit(DictationNotice::DismissError);
        let device = backend::preferred_input_device(&settings);
        let start = self.recorder.start(device.as_deref());
        cx.spawn(async move |this, cx| {
            let result = start.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                this.start_lock = false;
                match result {
                    Ok(()) => {
                        this.started_at = Some((Instant::now(), Utc::now()));
                        this.set_phase(Phase::Recording, cx);
                        this.hold_recording_effects(&settings);
                    }
                    Err(err) => {
                        this.set_phase(Phase::Idle, cx);
                        if err.is_no_audio() {
                            cx.emit(DictationNotice::NoAudio);
                        } else if !matches!(err, whisper::audio::AudioError::Cancelled) {
                            cx.emit(DictationNotice::Error {
                                title: t("dictation.errorTitles.microphoneUnavailable"),
                                description: t("dictation.errorDescriptions.microphoneUnavailable"),
                            });
                        }
                    }
                }
            });
        })
        .detach();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if self.phase == Phase::Preparing || (self.start_lock && self.phase != Phase::Recording) {
            self.generation += 1;
            self.start_lock = false;
            self.recorder.cancel();
            self.set_phase(Phase::Idle, cx);
            return;
        }
        if self.phase != Phase::Recording {
            return;
        }
        let Some(audio) = self.recorder.stop() else {
            self.set_phase(Phase::Idle, cx);
            return;
        };
        if AppState::settings(cx).audio_cues_enabled {
            cues::play_stop_cue();
        }
        self.set_phase(Phase::Processing, cx);
        self.process(audio, cx);
    }

    /// Marks that the hold ceiling ended the recording with keys still down.
    pub fn note_force_stopped(&mut self) {
        self.push_force_stopped = true;
    }

    pub fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.start_lock = false;
        self.recorder.cancel_prepared();
        if self.phase == Phase::Recording {
            let started = self.started_at.take();
            let audio = self.recorder.stop();
            self.set_phase(Phase::Idle, cx);
            if let (Some(audio), Some((_, occurred_at))) = (audio, started) {
                self.save_discarded(audio, occurred_at, cx);
            }
        } else {
            self.recorder.cancel();
            if self.phase == Phase::Preparing {
                self.set_phase(Phase::Idle, cx);
            }
        }
    }

    pub fn cancel_processing(&mut self, cx: &mut Context<Self>) {
        if self.phase != Phase::Processing {
            return;
        }
        self.generation += 1;
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        whisper::services::cleanup::cancel_all();
        self.set_phase(Phase::Idle, cx);
    }

    fn fail_microphone(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.phase, Phase::Recording | Phase::Preparing) {
            return;
        }
        self.cancel_recording(cx);
        cx.emit(DictationNotice::Error {
            title: t("dictation.errorTitles.microphoneUnavailable"),
            description: t("dictation.errorDescriptions.microphoneUnavailable"),
        });
    }

    // ---- Processing --------------------------------------------------------------------------

    fn pipeline_input(audio: Vec<u8>, mime: &str, cx: &App) -> PipelineInput {
        let state = AppState::global(cx).read(cx);
        PipelineInput {
            audio,
            mime_type: mime.to_string(),
            settings: state.settings.clone(),
            dictionary: state.dictionary.clone(),
            dictionary_priority: state.db.get_dictionary_by_priority().unwrap_or_default(),
            snippets: state.snippets.clone(),
            cleanup_api_key: Some(backend::cleanup_api_key()).filter(|k| !k.is_empty()),
            agent_name: whisper::text::DEFAULT_AGENT_NAME.to_string(),
        }
    }

    fn process(&mut self, audio: RecordedAudio, cx: &mut Context<Self>) {
        let generation = self.generation;
        let (started, occurred_at) = self
            .started_at
            .take()
            .unwrap_or_else(|| (Instant::now(), Utc::now()));
        let duration = started.elapsed();
        let wav = audio.to_wav_16k();
        let check = audio.check(wav.len());
        if !check.usable || audio.speech_gate.is_silence() {
            log::info!(
                "dropping recording before transcription (usable={}, gate={:?})",
                check.usable,
                audio.speech_gate.reason
            );
            self.set_phase(Phase::Idle, cx);
            cx.emit(DictationNotice::NoAudio);
            return;
        }
        let input = Self::pipeline_input(wav.clone(), "audio/wav", cx);
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        let handle = whisper::services::runtime::spawn(pipeline::run(input, cancel));
        cx.spawn(async move |this, cx| {
            let outcome = handle.await.unwrap_or(PipelineOutcome::Failed {
                message: "pipeline task failed".into(),
                code: None,
                message_key: None,
            });
            let _ = this.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                this.cancel = None;
                this.finish(outcome, wav, duration, occurred_at, cx);
            });
        })
        .detach();
    }

    fn finish(
        &mut self,
        outcome: PipelineOutcome,
        wav: Vec<u8>,
        duration: Duration,
        occurred_at: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            PipelineOutcome::Cancelled => {
                self.set_phase(Phase::Idle, cx);
            }
            PipelineOutcome::NoAudio { dictionary_echo } => {
                if dictionary_echo {
                    // Keep the recording so the user can retry the dictation (#1547).
                    save_failed(
                        wav,
                        duration,
                        occurred_at,
                        "No audio detected",
                        Some(pipeline::ECHO_CODE),
                        cx,
                    );
                }
                self.set_phase(Phase::Idle, cx);
                cx.emit(DictationNotice::NoAudio);
            }
            PipelineOutcome::Failed {
                message,
                code,
                message_key,
            } => {
                save_failed(wav, duration, occurred_at, &message, code.as_deref(), cx);
                self.set_phase(Phase::Idle, cx);
                let title = match code.as_deref() {
                    Some("TRANSCRIPTION_CONNECTION_FAILED") => {
                        t("dictation.errorTitles.transcriptionConnectionFailed")
                    }
                    Some("TRANSCRIPTION_REQUEST_TIMEOUT") => {
                        t("dictation.errorTitles.transcriptionTimeout")
                    }
                    Some("PROVIDER_RATE_LIMITED") => t("dictation.errorTitles.providerRateLimited"),
                    _ => "Transcription Error".to_string(),
                };
                let description = message_key
                    .and_then(|key| whisper::i18n::try_t(&key))
                    .unwrap_or_else(|| format!("Transcription failed: {message}"));
                cx.emit(DictationNotice::Error { title, description });
            }
            PipelineOutcome::Transcribed {
                text,
                raw_text,
                source,
                model,
                cleanup_failure,
            } => {
                self.deliver(
                    text,
                    raw_text,
                    source,
                    model,
                    cleanup_failure,
                    wav,
                    duration,
                    occurred_at,
                    cx,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn deliver(
        &mut self,
        text: String,
        raw_text: String,
        source: String,
        model: Option<String>,
        cleanup_failure: Option<pipeline::CleanupFailure>,
        wav: Vec<u8>,
        duration: Duration,
        occurred_at: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DictationNotice::DismissError);
        if text.trim().is_empty() {
            self.set_phase(Phase::Idle, cx);
            cx.emit(DictationNotice::NoAudio);
            return;
        }
        let state = AppState::global(cx);
        let (db, snippets, settings) = {
            let s = state.read(cx);
            (s.db.clone(), s.snippets.clone(), s.settings.clone())
        };
        // Counted before snippet expansion so saved text is not mistaken for speech.
        let _ = db.record_dictionary_usage(&text);
        let text = whisper::text::snippets::expand_snippets(&text, &snippets);

        save_transcription(
            &text,
            &raw_text,
            &source,
            model.as_deref(),
            &wav,
            duration,
            occurred_at,
            cx,
        );

        let restore_clipboard = !settings.keep_transcription_in_clipboard;
        if self.push_force_stopped && settings.auto_paste_enabled {
            let kept = backend::write_clipboard(&text);
            self.set_phase(Phase::Idle, cx);
            cx.emit(DictationNotice::PushForceStopped {
                kept_in_clipboard: kept,
                transcript: raw_text,
            });
        } else if settings.auto_paste_enabled {
            let generation = self.generation;
            let paste = backend::paste_text(text.clone(), self.target_pid, restore_clipboard);
            cx.spawn(async move |this, cx| {
                let pasted = paste.await;
                let _ = this.update(cx, |this, cx| {
                    if generation != this.generation {
                        return;
                    }
                    if pasted {
                        cx.emit(DictationNotice::Delivered);
                        if let Some(failure) = cleanup_failure {
                            let title = failure
                                .message_key
                                .and_then(|key| whisper::i18n::try_t(&key))
                                .unwrap_or_else(|| {
                                    if failure.message.is_empty() {
                                        t("app.toasts.cleanupFailed.title")
                                    } else {
                                        failure.message.clone()
                                    }
                                });
                            cx.emit(DictationNotice::CleanupFailed {
                                title,
                                description: Some(t("app.toasts.cleanupFailed.description")),
                            });
                        }
                        let s = AppState::settings(cx);
                        if s.auto_learn_corrections {
                            let learned =
                                backend::monitor_corrections(text.clone(), this.target_pid, cx);
                            cx.spawn(async move |this, cx| {
                                let words = learned.await;
                                if !words.is_empty() {
                                    let _ = this.update(cx, |_, cx| {
                                        cx.emit(DictationNotice::LearnedWords(words))
                                    });
                                }
                            })
                            .detach();
                        }
                    } else {
                        let copied = backend::write_clipboard(&text);
                        cx.emit(DictationNotice::DeliveryFallback {
                            text: text.clone(),
                            copied,
                        });
                    }
                    this.set_phase(Phase::Idle, cx);
                });
            })
            .detach();
        } else {
            if settings.keep_transcription_in_clipboard {
                backend::write_clipboard(&text);
            }
            cx.emit(DictationNotice::Delivered);
            self.set_phase(Phase::Idle, cx);
        }
    }

    fn save_discarded(
        &mut self,
        audio: RecordedAudio,
        occurred_at: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) {
        let s = AppState::settings(cx);
        let duration = audio.duration;
        if !s.save_discarded_transcriptions
            || !s.data_retention_enabled
            || s.audio_retention_days == 0
            || duration < MIN_DISCARDED_DURATION
            || !audio.received_audio_data
        {
            return;
        }
        let wav = audio.to_wav_16k();
        let state = AppState::global(cx);
        let (db, storage) = {
            let s = state.read(cx);
            (s.db.clone(), s.audio.clone())
        };
        let options = SaveTranscriptionOptions {
            status: TranscriptionStatus::Discarded,
            occurred_at: Some(occurred_at),
            ..Default::default()
        };
        let Ok(item) = db.save_transcription("", None, options) else {
            return;
        };
        // A discarded row is only recoverable through its audio (#907).
        if storage
            .save_audio(item.id, &wav, Some(&item.timestamp), "wav")
            .is_err()
        {
            let _ = db.delete_transcription(item.id);
            return;
        }
        let _ = db.update_transcription_audio(
            item.id,
            &TranscriptionAudioUpdate {
                has_audio: true,
                audio_duration_ms: Some(duration.as_millis() as i64),
                provider: None,
                model: None,
            },
        );
        if let Ok(Some(item)) = db.get_transcription_by_id(item.id) {
            state.update(cx, |s, cx| s.upsert_history_item(item, cx));
        }
    }
}

fn store_audio(
    item: &TranscriptionItem,
    wav: &[u8],
    duration: Duration,
    provider: Option<&str>,
    model: Option<&str>,
    cx: &mut App,
) {
    let state = AppState::global(cx);
    let (db, storage, retention) = {
        let s = state.read(cx);
        (
            s.db.clone(),
            s.audio.clone(),
            s.settings.audio_retention_days,
        )
    };
    if retention == 0 {
        return;
    }
    match storage.save_audio(item.id, wav, Some(&item.timestamp), "wav") {
        Ok(_) => {
            let _ = db.update_transcription_audio(
                item.id,
                &TranscriptionAudioUpdate {
                    has_audio: true,
                    audio_duration_ms: Some(duration.as_millis() as i64),
                    provider: provider.map(str::to_string),
                    model: model.map(str::to_string),
                },
            );
        }
        Err(err) => log::warn!("failed to save transcription audio: {err}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn save_transcription(
    text: &str,
    raw_text: &str,
    source: &str,
    model: Option<&str>,
    wav: &[u8],
    duration: Duration,
    occurred_at: DateTime<Utc>,
    cx: &mut App,
) {
    let state = AppState::global(cx);
    let (db, retention) = {
        let s = state.read(cx);
        (s.db.clone(), s.settings.data_retention_enabled)
    };
    if !retention {
        return;
    }
    let event = AnalyticsEventInput::for_dictation(
        text,
        Some(raw_text),
        occurred_at,
        Some(duration.as_millis() as i64),
        Some(source),
        model,
    );
    if let Err(err) = db.record_analytics_event(&event) {
        log::warn!("failed to record the analytics event: {err}");
    }
    let options = SaveTranscriptionOptions {
        occurred_at: Some(occurred_at),
        ..Default::default()
    };
    match db.save_transcription(text, Some(raw_text), options) {
        Ok(item) => {
            store_audio(&item, wav, duration, Some(source), model, cx);
            let item = db
                .get_transcription_by_id(item.id)
                .ok()
                .flatten()
                .unwrap_or(item);
            state.update(cx, |s, cx| {
                s.upsert_history_item(item, cx);
                cx.emit(AppEvent::AnalyticsChanged);
            });
        }
        Err(err) => log::error!("failed to persist transcription: {err}"),
    }
}

fn save_failed(
    wav: Vec<u8>,
    duration: Duration,
    occurred_at: DateTime<Utc>,
    message: &str,
    code: Option<&str>,
    cx: &mut App,
) {
    let state = AppState::global(cx);
    let (db, retention) = {
        let s = state.read(cx);
        (s.db.clone(), s.settings.data_retention_enabled)
    };
    if !retention {
        return;
    }
    let options = SaveTranscriptionOptions {
        status: TranscriptionStatus::Failed,
        error_message: Some(message.to_string()),
        error_code: code.map(str::to_string),
        occurred_at: Some(occurred_at),
        ..Default::default()
    };
    match db.save_transcription("", None, options) {
        Ok(item) => {
            store_audio(&item, &wav, duration, None, None, cx);
            let item = db
                .get_transcription_by_id(item.id)
                .ok()
                .flatten()
                .unwrap_or(item);
            state.update(cx, |s, cx| s.upsert_history_item(item, cx));
        }
        Err(err) => log::error!("failed to save the failed transcription: {err}"),
    }
}

/// Re-transcribes a History row from its saved audio (`retry-transcription`
/// plus the control panel's cleanup and script passes).
pub fn retry_transcription(id: i64, cx: &mut App) -> Task<Result<(), String>> {
    let state = AppState::global(cx);
    let (db, storage) = {
        let s = state.read(cx);
        (s.db.clone(), s.audio.clone())
    };
    let Some(path) = storage.audio_path(id) else {
        return Task::ready(Err(t("controlPanel.history.audioNotFound")));
    };
    let audio = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => return Task::ready(Err(err.to_string())),
    };
    // Saved recordings are always WAV.
    let input = Dictation::pipeline_input(audio, "audio/wav", cx);
    let handle = whisper::services::runtime::spawn(pipeline::run(input, CancellationToken::new()));
    cx.spawn(async move |cx| {
        let outcome = handle.await.map_err(|e| e.to_string())?;
        match outcome {
            PipelineOutcome::Transcribed {
                text,
                raw_text,
                cleanup_failure,
                ..
            } => {
                db.update_transcription_text(id, &text, Some(&raw_text))
                    .map_err(|e| e.to_string())?;
                let _ =
                    db.update_transcription_status(id, TranscriptionStatus::Completed, None, None);
                cx.update(|cx| {
                    if let Ok(Some(item)) = db.get_transcription_by_id(id) {
                        AppState::global(cx).update(cx, |s, cx| s.upsert_history_item(item, cx));
                    }
                    if let Some(failure) = cleanup_failure {
                        crate::ui::overlays::show_toast(
                            crate::ui::overlays::Toast::new(t("app.toasts.cleanupFailed.title"))
                                .description(
                                    failure
                                        .message_key
                                        .and_then(|k| whisper::i18n::try_t(&k))
                                        .unwrap_or(failure.message),
                                )
                                .variant(crate::ui::overlays::ToastVariant::Destructive),
                            cx,
                        );
                    }
                });
                Ok(())
            }
            PipelineOutcome::NoAudio { .. } => Err(t("dictation.noAudio.title")),
            PipelineOutcome::Failed {
                message,
                message_key,
                ..
            } => Err(message_key
                .and_then(|k| whisper::i18n::try_t(&k))
                .unwrap_or(message)),
            PipelineOutcome::Cancelled => Err("Cancelled".into()),
        }
    })
}
