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

use crate::audio::capture::{CaptureEvent, CaptureId, CaptureStatus, RecordedAudio, Recorder};
use crate::audio::cues;
use crate::history::Recording;
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

/// The recovery an error panel offers besides recording again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ErrorActions {
    /// The recording is kept, so it can be sent again without speaking again.
    pub retry: bool,
    /// The cause lives in Settings → Speech to text.
    pub open_settings: bool,
}

impl ErrorActions {
    /// What helps after a transcription failure with `code`. A slow or
    /// unreachable server is often momentary, so the words are worth
    /// resending, and its address may need fixing; a rate limit only needs
    /// time; an invalid endpoint fails the same way until it is fixed.
    pub fn for_transcription_failure(code: Option<&str>) -> Self {
        match code {
            Some(transcription::CONNECTION_FAILED_CODE | transcription::REQUEST_TIMEOUT_CODE) => {
                Self {
                    retry: true,
                    open_settings: true,
                }
            }
            Some(transcription::CUSTOM_ENDPOINT_INVALID_CODE) => Self {
                retry: false,
                open_settings: true,
            },
            _ => Self {
                retry: true,
                open_settings: false,
            },
        }
    }
}

/// The audio of the last Dictation whose transcription failed, held in
/// memory only so the error panel can send it again. A new Dictation, a
/// resend, or the panel closing drops it.
struct FailedRecording {
    wav: bytes::Bytes,
    duration: Duration,
    occurred_at: DateTime<Utc>,
}

impl FailedRecording {
    fn from_interrupted_audio(
        (audio, duration, occurred_at): (RecordedAudio, Duration, DateTime<Utc>),
    ) -> Option<Self> {
        let wav = audio.to_wav_16k();
        audio.is_usable(wav.len()).then(|| Self {
            wav: wav.into(),
            duration,
            occurred_at,
        })
    }
}

/// What happened, for the pill window to present.
#[derive(Clone, Debug)]
pub enum DictationNotice {
    DismissError,
    Error {
        title: String,
        description: String,
        actions: ErrorActions,
        /// The length of the recording held for a resend, so the panel can
        /// say which words it would send.
        kept: Option<Duration>,
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
    /// Holds text for explicit recovery, with a resend when audio is kept.
    TranscriptRecovery {
        title: String,
        description: String,
        text: String,
        kept: Option<Duration>,
    },
    /// Escape while only the copy-recovery panel is open closes it. The
    /// error card never takes Escape: it can hold the only copy of a
    /// recording, and a stray Escape meant for another app must not lose it.
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
    failed: Option<FailedRecording>,
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
                    let CaptureEvent::DeviceLost {
                        capture_id,
                        message,
                    } = event;
                    if this
                        .update(cx, |this: &mut Dictation, cx| {
                            this.fail_microphone(capture_id, &message, cx)
                        })
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
                failed: None,
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

    pub fn capture_status(&self) -> CaptureStatus {
        self.recorder.capture_status()
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
        // A new Dictation replaces the one that failed.
        self.failed = None;
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
        self.failed = None;
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
                                actions: ErrorActions::default(),
                                kept: None,
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
        if self.cancel_capture() {
            self.set_phase(Phase::Idle, cx);
        }
    }

    fn cancel_capture(&mut self) -> bool {
        // A stale recovery Escape must not invalidate a processing result.
        if !matches!(self.phase, Phase::Preparing | Phase::Recording) {
            return false;
        }
        self.generation += 1;
        self.start_lock = false;
        self.recorder.cancel_prepared();
        if self.phase == Phase::Recording {
            self.started_at = None;
            self.recorder.stop();
        } else {
            self.recorder.cancel();
        }
        true
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

    fn fail_microphone(&mut self, capture_id: CaptureId, message: &str, cx: &mut Context<Self>) {
        if !self.accepts_device_loss(capture_id) {
            return;
        }
        log::warn!("microphone lost: {message}");
        let audio = self.interrupt_capture(capture_id);
        let generation = self.generation;
        self.set_phase(Phase::Processing, cx);
        let conversion = crate::services::runtime::blocking(move || {
            audio.and_then(FailedRecording::from_interrupted_audio)
        });
        cx.spawn(async move |this, cx| {
            let failed = conversion.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                this.failed = failed;
                this.set_phase(Phase::Idle, cx);
                this.show_microphone_error(cx);
            });
        })
        .detach();
    }

    fn accepts_device_loss(&self, capture_id: CaptureId) -> bool {
        matches!(self.phase, Phase::Recording | Phase::Preparing)
            && self.recorder.active_capture_id() == Some(capture_id)
    }

    /// Device failure stops capture without applying the user's discard action.
    fn interrupt_capture(
        &mut self,
        capture_id: CaptureId,
    ) -> Option<(RecordedAudio, Duration, DateTime<Utc>)> {
        if !self.accepts_device_loss(capture_id) {
            return None;
        }
        self.generation += 1;
        self.start_lock = false;
        self.recorder.cancel_prepared();
        let timing = self.started_at.take();
        if self.phase != Phase::Recording {
            self.recorder.cancel();
            return None;
        }
        self.recorder.stop().map(|audio| {
            let (started, occurred_at) = timing.unwrap_or_else(|| (Instant::now(), Utc::now()));
            (audio, started.elapsed(), occurred_at)
        })
    }

    fn show_microphone_error(&self, cx: &mut Context<Self>) {
        cx.emit(DictationNotice::Error {
            title: t("dictation.errorTitles.microphoneUnavailable"),
            description: t("dictation.errorDescriptions.microphoneUnavailable"),
            actions: ErrorActions {
                retry: self.failed.is_some(),
                open_settings: false,
            },
            kept: self.failed.as_ref().map(|failed| failed.duration),
        });
    }

    // ---- Processing --------------------------------------------------------------------------

    fn pipeline_input(
        audio: bytes::Bytes,
        cx: &App,
    ) -> impl std::future::Future<Output = PipelineInput> + Send + 'static {
        let state = AppState::global(cx).read(cx);
        capture_pipeline_input(audio, state.settings.clone(), state.db.clone())
    }

    fn process(&mut self, audio: RecordedAudio, cx: &mut Context<Self>) {
        let (started, occurred_at) = self
            .started_at
            .take()
            .unwrap_or_else(|| (Instant::now(), Utc::now()));
        let duration = started.elapsed();
        let input = Self::pipeline_input(bytes::Bytes::new(), cx);
        let generation = self.generation;
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        let stopped = Instant::now();
        let handle = crate::services::runtime::spawn(async move {
            let conversion_cancel = cancel.clone();
            let wav = crate::services::runtime::blocking(move || {
                if conversion_cancel.is_cancelled() || audio.speech_gate.is_silence() {
                    return None;
                }
                let wav = audio.to_wav_16k();
                audio.is_usable(wav.len()).then(|| bytes::Bytes::from(wav))
            })
            .await;
            let wav = match wav {
                Ok(Some(wav)) => wav,
                Ok(None) => return (PipelineOutcome::NoAudio, bytes::Bytes::new()),
                Err(_) => return (pipeline_task_failed(), bytes::Bytes::new()),
            };
            log::info!(
                "[perf] operation={generation} stage=audio_ready elapsed_ms={:.3} bytes={}",
                stopped.elapsed().as_secs_f64() * 1000.0,
                wav.len()
            );
            if cancel.is_cancelled() {
                return (PipelineOutcome::Cancelled, wav);
            }
            let mut input = input.await;
            input.audio = wav.clone();
            let outcome = run_prepared_pipeline(input, cancel).await;
            log::info!(
                "[perf] operation={generation} stage=pipeline_complete elapsed_ms={:.3}",
                stopped.elapsed().as_secs_f64() * 1000.0
            );
            (outcome, wav)
        });
        cx.spawn(async move |this, cx| {
            let (outcome, wav) = handle
                .await
                .unwrap_or_else(|_| (pipeline_task_failed(), bytes::Bytes::new()));
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

    /// Sends the last failed recording again, as if it had just stopped.
    /// The paste goes to whichever app is frontmost now.
    pub fn retry_failed(&mut self, cx: &mut Context<Self>) {
        if self.start_lock || self.phase != Phase::Idle {
            return;
        }
        let Some(failed) = self.failed.take() else {
            return;
        };
        self.push_force_stopped = false;
        self.generation += 1;
        self.capture_target(cx);
        self.set_phase(Phase::Processing, cx);
        cx.emit(DictationNotice::DismissError);
        self.transcribe(failed.wav, failed.duration, failed.occurred_at, cx);
    }

    /// Forgets the failed recording once its error panel closes.
    pub fn discard_failed(&mut self) {
        self.failed = None;
    }

    fn transcribe(
        &mut self,
        wav: bytes::Bytes,
        duration: Duration,
        occurred_at: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) {
        let generation = self.generation;
        let input = Self::pipeline_input(wav.clone(), cx);
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        let handle = crate::services::runtime::spawn(async move {
            run_prepared_pipeline(input.await, cancel).await
        });
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
        wav: bytes::Bytes,
        duration: Duration,
        occurred_at: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            PipelineOutcome::Cancelled => {
                self.set_phase(Phase::Idle, cx);
            }
            PipelineOutcome::NoAudio => {
                self.set_phase(Phase::Idle, cx);
                cx.emit(DictationNotice::NoAudio);
            }
            PipelineOutcome::PromptEcho { text } => {
                let notice = self.recover_transcript(
                    "dictation.promptEcho",
                    text,
                    wav,
                    duration,
                    occurred_at,
                );
                self.set_phase(Phase::Idle, cx);
                cx.emit(notice);
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
                    _ => t("dictation.errorTitles.transcriptionFailed"),
                };
                let description = message_key
                    .and_then(|key| crate::i18n::try_t(&key))
                    .unwrap_or_else(|| {
                        crate::i18n::t_with(
                            "dictation.errorDescriptions.transcriptionFailed",
                            &[("message", &message)],
                        )
                    });
                let actions = ErrorActions::for_transcription_failure(code.as_deref());
                let kept = actions.retry.then_some(duration);
                self.failed = actions.retry.then_some(FailedRecording {
                    wav,
                    duration,
                    occurred_at,
                });
                cx.emit(DictationNotice::Error {
                    title,
                    description,
                    actions,
                    kept,
                });
            }
            PipelineOutcome::Transcribed(transcript) => {
                self.persist_and_deliver(transcript, wav, duration, occurred_at, cx);
            }
        }
    }

    fn recover_transcript(
        &mut self,
        message: &str,
        text: String,
        wav: bytes::Bytes,
        duration: Duration,
        occurred_at: DateTime<Utc>,
    ) -> DictationNotice {
        self.failed = (!wav.is_empty()).then_some(FailedRecording {
            wav,
            duration,
            occurred_at,
        });
        DictationNotice::TranscriptRecovery {
            title: t(&format!("{message}.title")),
            description: t(&format!("{message}.description")),
            text,
            kept: self.failed.as_ref().map(|failed| failed.duration),
        }
    }

    fn persist_and_deliver(
        &mut self,
        transcript: Transcript,
        wav: bytes::Bytes,
        duration: Duration,
        occurred_at: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) {
        let generation = self.generation;
        let saved_transcript = transcript.clone();
        let save = AppState::global(cx).update(cx, |state, cx| {
            state.record_history(cx, move |history, settings| {
                history.record_delivered(
                    &saved_transcript,
                    &Recording {
                        wav: &wav,
                        duration,
                        occurred_at,
                    },
                    settings,
                )
            })
        });
        let started = Instant::now();
        cx.spawn(async move |this, cx| {
            if let Err(err) = save.await {
                log::error!("failed to save the History row: {err:#}");
            }
            log::info!(
                "[perf] operation={generation} stage=persistence_complete elapsed_ms={:.3}",
                started.elapsed().as_secs_f64() * 1000.0
            );
            let _ = this.update(cx, |this, cx| {
                if generation == this.generation && this.phase == Phase::Processing {
                    this.deliver(transcript, cx);
                }
            });
        })
        .detach();
    }

    fn deliver(&mut self, transcript: Transcript, cx: &mut Context<Self>) {
        cx.emit(DictationNotice::DismissError);
        let settings = AppState::settings(cx);
        let Transcript {
            text,
            cleanup_failure,
            ..
        } = transcript;

        let restore_clipboard = !settings.keep_transcription_in_clipboard;
        if self.push_force_stopped && settings.auto_paste_enabled {
            let kept = backend::write_clipboard(&text);
            self.set_phase(Phase::Idle, cx);
            cx.emit(DictationNotice::PushForceStopped {
                kept_in_clipboard: kept,
                transcript: text,
            });
        } else if settings.auto_paste_enabled {
            let generation = self.generation;
            let target = self.target_pid.clone();
            cx.spawn(async move |this, cx| {
                let target_pid = match target {
                    Some(target) => target.await,
                    None => None,
                };
                let current = this
                    .update(cx, |this, _| {
                        generation == this.generation && this.phase == Phase::Processing
                    })
                    .unwrap_or(false);
                if !current {
                    return;
                }
                let paste_started = Instant::now();
                let pasted = backend::paste_text(text.clone(), target_pid, restore_clipboard).await;
                log::info!(
                    "[perf] operation={generation} stage=paste_complete elapsed_ms={:.3}",
                    paste_started.elapsed().as_secs_f64() * 1000.0
                );
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

fn pipeline_task_failed() -> PipelineOutcome {
    PipelineOutcome::Failed {
        message: "pipeline task failed".into(),
        code: None,
        message_key: None,
    }
}

/// Enqueue at the UI action, before conversion or audio loading can yield.
/// Earlier edits have settled before this read, and later edits cannot change
/// this request's rules. Never send optimistic UI projections to the pipeline.
fn capture_pipeline_input(
    audio: bytes::Bytes,
    settings: Settings,
    db: Arc<crate::db::Database>,
) -> impl std::future::Future<Output = PipelineInput> + Send + 'static {
    crate::services::runtime::storage(move || PipelineInput {
        audio,
        dictionary: db
            .get_dictionary_entries()
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.word)
            .collect(),
        dictionary_priority: db.get_dictionary_by_priority().unwrap_or_default(),
        known_corrections: if settings.auto_learn_corrections {
            db.get_known_corrections().unwrap_or_default()
        } else {
            Vec::new()
        },
        snippets: db.get_snippets().unwrap_or_default(),
        settings,
        cleanup_api_key: None,
    })
}

async fn run_prepared_pipeline(
    mut input: PipelineInput,
    cancel: CancellationToken,
) -> PipelineOutcome {
    if cancel.is_cancelled() {
        return PipelineOutcome::Cancelled;
    }
    let needs_key = pipeline::cleanup_enabled(&input.settings);
    // Keychain access stays off the storage worker.
    let key = crate::services::runtime::blocking(move || {
        needs_key
            .then(backend::cleanup_api_key)
            .filter(|k| !k.is_empty())
    });
    let Ok(key) = key.await else {
        return pipeline_task_failed();
    };
    input.cleanup_api_key = key;
    if cancel.is_cancelled() {
        return PipelineOutcome::Cancelled;
    }
    pipeline::run(input, cancel).await
}

/// Re-transcribes a History row from its saved audio through the same
/// pipeline as a live Dictation.
pub fn retry_transcription(id: i64, cx: &mut App) -> Task<Result<(), String>> {
    let history = AppState::global(cx).read(cx).history_store.clone();
    let input = Dictation::pipeline_input(bytes::Bytes::new(), cx);
    cx.spawn(async move |cx| {
        let audio = crate::services::runtime::storage(move || history.retry_audio(id)).await;
        let Some(audio) = audio else {
            return Err(t("controlPanel.history.audioNotFound"));
        };
        let mut input = input.await;
        input.audio = audio.into();
        let outcome =
            crate::services::runtime::spawn(run_prepared_pipeline(input, CancellationToken::new()))
                .await
                .map_err(|e| e.to_string())?;
        match outcome {
            PipelineOutcome::Transcribed(transcript) => {
                let failure = transcript.cleanup_failure.clone();
                let save = cx.update(|cx| {
                    AppState::global(cx).update(cx, |s, cx| {
                        s.record_history(cx, move |history, _| history.apply_retry(id, &transcript))
                    })
                });
                save.await.map_err(|e| e.to_string())?;
                if let Some(failure) = failure {
                    cx.update(|cx| {
                        crate::ui::overlays::show_toast(
                            crate::ui::overlays::Toast::new(t("app.toasts.cleanupFailed.title"))
                                .description(
                                    failure
                                        .message_key
                                        .and_then(|key| crate::i18n::try_t(&key))
                                        .unwrap_or(failure.message),
                                )
                                .variant(crate::ui::overlays::ToastVariant::Destructive),
                            cx,
                        )
                    });
                }
                Ok(())
            }
            PipelineOutcome::NoAudio => Err(t("dictation.noAudio.title")),
            PipelineOutcome::PromptEcho { text } => {
                // The History row still owns its audio. Keep this candidate
                // copyable without replacing that row or live capture state.
                cx.update(|cx| {
                    Dictation::global(cx).update(cx, |_, cx| {
                        cx.emit(DictationNotice::TranscriptRecovery {
                            title: t("dictation.promptEcho.title"),
                            description: t("dictation.promptEcho.description"),
                            text,
                            kept: None,
                        });
                    });
                });
                Err(t("dictation.promptEcho.title"))
            }
            PipelineOutcome::Failed {
                message,
                message_key,
                ..
            } => Err(message_key
                .and_then(|key| crate::i18n::try_t(&key))
                .unwrap_or(message)),
            PipelineOutcome::Cancelled => Err("Cancelled".into()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{ErrorActions, capture_pipeline_input, run_prepared_pipeline};
    use crate::db::{Database, DictionarySource, Snippet};
    use crate::pipeline::PipelineOutcome;
    use crate::services::transcription::{
        CONNECTION_FAILED_CODE, CUSTOM_ENDPOINT_INVALID_CODE, RATE_LIMITED_CODE,
        REQUEST_TIMEOUT_CODE,
    };
    use crate::services::{
        runtime,
        test_server::{MockServer, Reply, run},
    };
    use crate::settings::Settings;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    fn dictation(phase: super::Phase) -> super::Dictation {
        super::Dictation {
            phase,
            recorder: super::Recorder::new(None),
            generation: 7,
            start_lock: false,
            cancel: None,
            target_pid: None,
            started_at: None,
            effects_held: false,
            media_paused: false,
            push_force_stopped: false,
            failed: None,
            status: super::StatusMirror::default(),
            _device_events: gpui_kit::Task::ready(()),
        }
    }

    #[test]
    fn microphone_loss_keeps_partial_audio_but_user_cancel_discards_it() {
        use super::{FailedRecording, Phase, Recorder};
        let occurred_at = chrono::Utc::now();
        for interrupted in [true, false] {
            let mut dictation = dictation(Phase::Recording);
            dictation.recorder = Recorder::with_test_audio(vec![0.1; 16_000]);
            dictation.started_at = Some((std::time::Instant::now(), occurred_at));
            if interrupted {
                let audio = dictation
                    .interrupt_capture(dictation.recorder.active_capture_id().unwrap())
                    .unwrap();
                let kept = FailedRecording::from_interrupted_audio(audio).unwrap();
                assert_eq!(kept.occurred_at, occurred_at);
                assert!(kept.wav.starts_with(b"RIFF"));
                assert!(kept.wav.len() > 32_000);
            } else {
                assert!(dictation.cancel_capture());
                assert!(dictation.failed.is_none());
            }
            assert_eq!(dictation.generation, 8);
            assert!(dictation.started_at.is_none());
            assert!(!dictation.recorder.is_capture_active());
            assert!(dictation.recorder.stop().is_none());
        }
    }

    #[test]
    fn microphone_loss_without_usable_audio_does_not_offer_resend() {
        use super::{FailedRecording, Phase, Recorder};
        let mut dictation = dictation(Phase::Recording);
        dictation.recorder = Recorder::with_test_audio(vec![0.1; 10]);
        let capture_id = dictation.recorder.active_capture_id().unwrap();
        assert!(
            dictation
                .interrupt_capture(capture_id)
                .and_then(FailedRecording::from_interrupted_audio)
                .is_none()
        );
    }

    #[test]
    fn queued_device_loss_cannot_stop_a_newer_recording() {
        use super::{Phase, Recorder};
        let mut dictation = dictation(Phase::Recording);
        dictation.recorder = Recorder::with_test_audio(vec![0.1; 16_000]);
        let old_id = dictation.recorder.active_capture_id().unwrap();
        assert!(dictation.cancel_capture());
        dictation.recorder = Recorder::with_test_audio(vec![0.2; 16_000]);
        let current_id = dictation.recorder.active_capture_id().unwrap();
        let generation = dictation.generation;
        assert_ne!(old_id, current_id);
        assert!(!dictation.accepts_device_loss(old_id));
        assert!(dictation.interrupt_capture(old_id).is_none());
        assert_eq!(dictation.generation, generation);
        assert!(dictation.recorder.is_recording());
        assert_eq!(dictation.phase, Phase::Recording);
        assert!(dictation.accepts_device_loss(current_id));
        let (audio, _, _) = dictation.interrupt_capture(current_id).unwrap();
        assert_eq!(audio.samples, vec![0.2; 16_000]);
        assert!(!dictation.recorder.is_capture_active());
        assert!(!dictation.accepts_device_loss(current_id));
    }

    #[test]
    fn dictionary_word_echo_keeps_text_and_audio_for_recovery() {
        use super::{DictationNotice, Phase};
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.apply_dictionary_changes(&["Supabase"], &[] as &[&str], DictionarySource::Manual)
            .unwrap();
        let server = MockServer::start(|_| Reply::json(200, r#"{"text":"Supabase"}"#));
        let settings = Settings {
            remote_transcription_url: server.url("/v1"),
            use_cleanup_model: false,
            ..Settings::default()
        };
        let wav = bytes::Bytes::from_static(b"RIFF original audio");
        let outcome = run(async {
            let input = capture_pipeline_input(wav.clone(), settings, db).await;
            run_prepared_pipeline(input, CancellationToken::new()).await
        });
        let PipelineOutcome::PromptEcho { text } = outcome else {
            panic!("unexpected outcome: {outcome:?}");
        };
        let mut dictation = dictation(Phase::Processing);
        let occurred_at = chrono::Utc::now();
        let duration = std::time::Duration::from_secs(3);
        let notice = dictation.recover_transcript(
            "dictation.promptEcho",
            text,
            wav.clone(),
            duration,
            occurred_at,
        );
        let DictationNotice::TranscriptRecovery { text, kept, .. } = notice else {
            panic!("echo must not report NoAudio or Delivered");
        };
        assert_eq!(text, "Supabase");
        assert_eq!(kept, Some(duration));
        let retry = dictation.failed.take().unwrap();
        assert_eq!(retry.wav, wav);
        assert_eq!(retry.duration, duration);
        assert_eq!(retry.occurred_at, occurred_at);
        dictation.discard_failed();
        assert!(dictation.failed.is_none());
    }

    #[test]
    fn recording_cancel_does_not_invalidate_processing_or_idle() {
        use super::{Dictation, Phase, Recorder, StatusMirror};
        for phase in [Phase::Idle, Phase::Processing] {
            let cancel = CancellationToken::new();
            let mut dictation = Dictation {
                phase,
                recorder: Recorder::new(None),
                generation: 7,
                start_lock: false,
                cancel: Some(cancel.clone()),
                target_pid: None,
                started_at: None,
                effects_held: false,
                media_paused: false,
                push_force_stopped: false,
                failed: None,
                status: StatusMirror::default(),
                _device_events: gpui_kit::Task::ready(()),
            };
            assert!(!dictation.cancel_capture());
            assert_eq!(dictation.generation, 7);
            assert_eq!(dictation.phase, phase);
            assert!(!cancel.is_cancelled());
        }
    }

    fn actions(code: Option<&str>) -> (bool, bool) {
        let a = ErrorActions::for_transcription_failure(code);
        (a.retry, a.open_settings)
    }

    #[test]
    fn an_unreachable_or_slow_server_keeps_the_words_and_points_to_settings() {
        assert_eq!(actions(Some(CONNECTION_FAILED_CODE)), (true, true));
        assert_eq!(actions(Some(REQUEST_TIMEOUT_CODE)), (true, true));
    }

    #[test]
    fn a_rate_limit_or_server_error_only_needs_a_resend() {
        assert_eq!(actions(Some(RATE_LIMITED_CODE)), (true, false));
        assert_eq!(actions(None), (true, false));
    }

    #[test]
    fn an_invalid_endpoint_is_fixed_in_settings_not_by_resending() {
        assert_eq!(actions(Some(CUSTOM_ENDPOINT_INVALID_CODE)), (false, true));
    }

    #[test]
    fn failed_edits_never_enter_the_pipeline_even_before_ui_reconciliation() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.lock()
            .execute_batch(
                "CREATE TRIGGER reject_snippet BEFORE INSERT ON snippets
             BEGIN SELECT RAISE(ABORT, 'synthetic snippet failure'); END;
             CREATE TRIGGER reject_word BEFORE INSERT ON custom_dictionary
             BEGIN SELECT RAISE(ABORT, 'synthetic dictionary failure'); END;",
            )
            .unwrap();
        let server = MockServer::start(|_| Reply::json(200, r#"{"text":"hello"}"#));
        let settings = Settings {
            remote_transcription_url: server.url("/v1"),
            use_cleanup_model: false,
            ..Settings::default()
        };
        let (release, blocked) = std::sync::mpsc::channel();
        drop(runtime::storage(move || blocked.recv().unwrap()));
        let writing = {
            let db = db.clone();
            runtime::storage(move || {
                (
                    db.apply_snippet_changes(&[], &[Snippet::new("hello", "rejected replacement")]),
                    db.apply_dictionary_changes(
                        &["hello"],
                        &[] as &[&str],
                        DictionarySource::Manual,
                    ),
                )
            })
        };
        // Capture while the edits are still queued and no UI completion has run.
        let input = capture_pipeline_input(
            bytes::Bytes::from_static(b"RIFF test"),
            settings,
            db.clone(),
        );
        let later_edit = {
            let db = db.clone();
            runtime::storage(move || {
                db.lock()
                    .execute_batch("DROP TRIGGER reject_snippet;")
                    .unwrap();
                db.set_snippets(&[Snippet::new("hello", "later replacement")])
                    .unwrap();
            })
        };
        release.send(()).unwrap();
        run(async {
            later_edit.await;
            let (snippet, dictionary) = writing.await;
            assert!(snippet.is_err());
            assert!(dictionary.is_err());
            let input = input.await;
            assert!(input.snippets.is_empty());
            assert!(input.dictionary.is_empty());
            assert!(input.dictionary_priority.is_empty());
            let outcome = run_prepared_pipeline(input, CancellationToken::new()).await;
            let PipelineOutcome::Transcribed(transcript) = outcome else {
                panic!("unexpected outcome: {outcome:?}");
            };
            assert_eq!(transcript.text, "hello");
        });
    }

    #[test]
    fn pipeline_rules_capture_prior_writes_and_exclude_later_edits() {
        use crate::text::correction_learner::Correction;
        let db = Arc::new(Database::open_in_memory().unwrap());
        let writing = {
            let db = db.clone();
            runtime::storage(move || {
                db.apply_dictionary_changes(
                    &["Claude Code"],
                    &[] as &[&str],
                    DictionarySource::Manual,
                )
                .unwrap();
                db.set_snippets(&[Snippet::new("signature", "Regards")])
                    .unwrap();
                let correction = Correction {
                    misheard: "cloud code".into(),
                    term: "Claude Code".into(),
                    needs_repeat: false,
                };
                db.record_corrections(std::slice::from_ref(&correction))
                    .unwrap();
                db.record_corrections(&[correction]).unwrap();
            })
        };
        let enabled = capture_pipeline_input(bytes::Bytes::new(), Settings::default(), db.clone());
        let disabled = capture_pipeline_input(
            bytes::Bytes::new(),
            Settings {
                auto_learn_corrections: false,
                ..Settings::default()
            },
            db.clone(),
        );
        let clearing = runtime::storage(move || {
            db.clear_dictionary().unwrap();
            db.set_snippets(&[]).unwrap();
        });
        run(async {
            // Poll in reverse order: submission, not polling, fixes the snapshot.
            clearing.await;
            writing.await;
            let input = enabled.await;
            assert_eq!(input.dictionary, vec!["Claude Code"]);
            assert_eq!(input.dictionary_priority, vec!["Claude Code"]);
            assert_eq!(input.snippets, vec![Snippet::new("signature", "Regards")]);
            assert_eq!(input.known_corrections.len(), 1);
            assert!(input.known_corrections[0].applies_automatically());
            let disabled = disabled.await;
            assert!(disabled.known_corrections.is_empty());
            assert_eq!(disabled.dictionary, input.dictionary);
            assert_eq!(disabled.snippets, input.snippets);
        });
    }
}
