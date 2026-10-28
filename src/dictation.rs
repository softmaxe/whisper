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
use futures::FutureExt;
use futures::future::Shared;
use gpui_kit::*;
use tokio_util::sync::CancellationToken;

use crate::audio::capture::{CaptureEvent, RecordedAudio, Recorder};
use crate::audio::cues;
use crate::history::{HistoryStore, Recorded, Recording};
use crate::i18n::t;
use crate::pipeline::{self, PipelineInput, PipelineOutcome, Transcript};
use crate::services::transcription;
use crate::settings::Settings;

use crate::app_state::{AppState, DictationSnapshot};
use crate::backend;

/// The Target app's pid, looked up off the main thread: the lookup can wait
/// on a helper process.
type TargetPid = Shared<Task<Option<i32>>>;

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
    target_pid: Option<TargetPid>,
    started_at: Option<(Instant, DateTime<Utc>)>,
    effects_held: bool,
    /// Whether this recording paused media, so the matching resume does not
    /// depend on a setting changed mid-recording.
    media_paused: bool,
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
                media_paused: false,
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
            self.release_recording_effects();
        }
        self.publish(cx);
    }

    fn publish(&self, cx: &mut Context<Self>) {
        let snapshot = DictationSnapshot {
            is_recording: self.phase == Phase::Recording,
            is_processing: self.phase == Phase::Processing,
            is_preparing: self.phase == Phase::Preparing,
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
        self.media_paused = settings.pause_media_on_dictation;
        if self.media_paused {
            backend::pause_media();
        }
        backend::register_cancel_hotkey();
        if settings.audio_cues_enabled {
            cues::play_start_cue();
        }
    }

    fn release_recording_effects(&mut self) {
        if !self.effects_held {
            return;
        }
        self.effects_held = false;
        backend::unregister_cancel_hotkey();
        if std::mem::take(&mut self.media_paused) {
            backend::resume_media();
        }
    }

    /// Starts looking up the Target app: the frontmost app at the hotkey press.
    fn capture_target(&mut self, cx: &mut Context<Self>) {
        let lookup = cx.background_spawn(async { backend::capture_target_pid() });
        self.target_pid = Some(lookup.shared());
    }

    // ---- Commands ----------------------------------------------------------------------------

    /// Opens the microphone ahead of a likely start (Hold mode key-down).
    pub fn prepare(&mut self, cx: &mut Context<Self>) {
        if self.start_lock || self.phase != Phase::Idle {
            return;
        }
        let device = backend::preferred_input_device(&AppState::settings(cx));
        self.recorder.prepare(device.as_deref());
        self.capture_target(cx);
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
            self.capture_target(cx);
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
                        } else if !matches!(err, crate::audio::AudioError::Cancelled) {
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
            self.started_at = None;
            self.recorder.stop();
            self.set_phase(Phase::Idle, cx);
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
            agent_name: crate::text::DEFAULT_AGENT_NAME.to_string(),
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
        let handle = crate::services::runtime::spawn(pipeline::run(input, cancel));
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
        let recording = Recording {
            wav: &wav,
            duration,
            occurred_at,
        };
        match outcome {
            PipelineOutcome::Cancelled => {
                self.set_phase(Phase::Idle, cx);
            }
            PipelineOutcome::NoAudio => {
                self.set_phase(Phase::Idle, cx);
                cx.emit(DictationNotice::NoAudio);
            }
            PipelineOutcome::Failed {
                message,
                code,
                message_key,
            } => {
                self.set_phase(Phase::Idle, cx);
                let title = match code.as_deref() {
                    Some(transcription::CONNECTION_FAILED_CODE) => {
                        t("dictation.errorTitles.transcriptionConnectionFailed")
                    }
                    Some(transcription::REQUEST_TIMEOUT_CODE) => {
                        t("dictation.errorTitles.transcriptionTimeout")
                    }
                    Some(transcription::RATE_LIMITED_CODE) => {
                        t("dictation.errorTitles.providerRateLimited")
                    }
                    _ => "Transcription Error".to_string(),
                };
                let description = message_key
                    .and_then(|key| crate::i18n::try_t(&key))
                    .unwrap_or_else(|| format!("Transcription failed: {message}"));
                cx.emit(DictationNotice::Error { title, description });
            }
            PipelineOutcome::Transcribed(transcript) => {
                self.deliver(transcript, &recording, cx);
            }
        }
    }

    fn deliver(&mut self, transcript: Transcript, recording: &Recording, cx: &mut Context<Self>) {
        cx.emit(DictationNotice::DismissError);
        record(cx, |history, settings| {
            history.record_delivered(&transcript, recording, settings)
        });
        let settings = AppState::settings(cx);
        let Transcript {
            text,
            raw_text,
            cleanup_failure,
            ..
        } = transcript;

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
            let target = self.target_pid.clone();
            cx.spawn(async move |this, cx| {
                let target_pid = match target {
                    Some(target) => target.await,
                    None => None,
                };
                let pasted = backend::paste_text(text.clone(), target_pid, restore_clipboard).await;
                let _ = this.update(cx, |this, cx| {
                    if generation != this.generation {
                        return;
                    }
                    if pasted {
                        cx.emit(DictationNotice::Delivered);
                        if let Some(failure) = cleanup_failure {
                            let title = failure
                                .message_key
                                .and_then(|key| crate::i18n::try_t(&key))
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
                                backend::monitor_corrections(text.clone(), target_pid, cx);
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
}

/// Runs a History write; a failure is logged and the Dictation goes on.
fn record(
    cx: &mut App,
    write: impl FnOnce(&HistoryStore, &Settings) -> anyhow::Result<Option<Recorded>>,
) {
    let result = AppState::global(cx).update(cx, |s, cx| s.record_history(cx, write));
    if let Err(err) = result {
        log::error!("failed to save the History row: {err:#}");
    }
}

/// Re-transcribes a History row from its saved audio through the same
/// pipeline as a live Dictation.
pub fn retry_transcription(id: i64, cx: &mut App) -> Task<Result<(), String>> {
    let history = AppState::global(cx).read(cx).history_store.clone();
    let Some(audio) = history.retry_audio(id) else {
        return Task::ready(Err(t("controlPanel.history.audioNotFound")));
    };
    // Saved recordings are always WAV.
    let input = Dictation::pipeline_input(audio, "audio/wav", cx);
    let handle = crate::services::runtime::spawn(pipeline::run(input, CancellationToken::new()));
    cx.spawn(async move |cx| {
        let outcome = handle.await.map_err(|e| e.to_string())?;
        match outcome {
            PipelineOutcome::Transcribed(transcript) => cx.update(|cx| {
                AppState::global(cx)
                    .update(cx, |s, cx| {
                        s.record_history(cx, |history, _| history.apply_retry(id, &transcript))
                    })
                    .map_err(|e| e.to_string())?;
                if let Some(failure) = transcript.cleanup_failure {
                    crate::ui::overlays::show_toast(
                        crate::ui::overlays::Toast::new(t("app.toasts.cleanupFailed.title"))
                            .description(
                                failure
                                    .message_key
                                    .and_then(|k| crate::i18n::try_t(&k))
                                    .unwrap_or(failure.message),
                            )
                            .variant(crate::ui::overlays::ToastVariant::Destructive),
                        cx,
                    );
                }
                Ok(())
            }),
            PipelineOutcome::NoAudio => Err(t("dictation.noAudio.title")),
            PipelineOutcome::Failed {
                message,
                message_key,
                ..
            } => Err(message_key
                .and_then(|k| crate::i18n::try_t(&k))
                .unwrap_or(message)),
            PipelineOutcome::Cancelled => Err("Cancelled".into()),
        }
    })
}
