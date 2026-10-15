//! Microphone capture on a dedicated thread per capture session.
//!
//! A [`CaptureSession`] owns one cpal input stream on its own thread (cpal
//! streams are `!Send` on macOS) and talks to it through a command channel
//! plus shared state. The audio callback downmixes to mono, buffers samples,
//! measures speech-gate windows and publishes the live level through an
//! atomic. The thread ticks every 50 ms to enforce the first-audio deadline,
//! pre-roll and prepared-capture expiry, and stall/device-loss detection.
//!
//! [`Recorder`] runs the prepared-capture flow: `prepare()` opens the mic early (hotkey down) and buffers pre-roll;
//! `start()` adopts that pre-roll when it is at most [`PRE_ROLL_MAX_AGE`] old
//! and resolves once the source has delivered its first frames.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use parking_lot::Mutex;

use super::AudioError;
use super::devices::{describe, find_input_device};
use super::gate::{
    ANALYSER_WINDOW, GATE_INTERVAL_MS, SpeechGateDecision, SpeechGateState, gate_decision,
    window_metrics,
};
use super::validation::{RecordingCheck, evaluate_finished_recording};
use super::wav::encode_wav_16k;

/// A prepared capture nobody takes within this window is closed, so a
/// declined hotkey guess never holds the mic open.
pub const PREPARED_MAX_AGE: Duration = Duration::from_millis(10_000);
/// Pre-roll older than this predates the user's intent and is discarded.
pub const PRE_ROLL_MAX_AGE: Duration = Duration::from_millis(2_000);
/// How long `start` waits for the source to deliver its first frames.
pub const FIRST_AUDIO_TIMEOUT: Duration = Duration::from_millis(10_000);
/// No frames for this long during a recording means the device is gone.
pub const STALL_TIMEOUT: Duration = Duration::from_millis(1_500);
const TICK: Duration = Duration::from_millis(50);
const LEVEL_WINDOW: usize = ANALYSER_WINDOW;

/// RMS of the last [`LEVEL_WINDOW`] float samples for the waveform. Unlike
/// the gate's byte-quantized metrics, quiet input keeps its resolution.
fn level_rms(samples: &[f32]) -> f32 {
    let tail = &samples[samples.len().saturating_sub(LEVEL_WINDOW)..];
    if tail.is_empty() {
        return 0.0;
    }
    (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
}

/// Events from a live capture, delivered on the capture thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureEvent {
    /// The device errored or vanished mid-recording. The recording is
    /// cancelled and "Microphone unavailable" is shown
    /// ([`super::MICROPHONE_CAPTURE_ERROR_CODE`]).
    DeviceLost { message: String },
}

pub type EventSink = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

/// A finished recording.
#[derive(Debug, Clone)]
pub struct RecordedAudio {
    /// Mono samples at `sample_rate` (the device rate).
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    /// Wall-clock duration from the recording start (the prepared stream's
    /// start when pre-roll was adopted) to stop.
    pub duration: Duration,
    /// Whether any audio frame was captured.
    pub received_audio_data: bool,
    /// Speech-gate decision; `Unavailable` (fail open) when pre-roll was adopted.
    pub speech_gate: SpeechGateDecision,
}

impl RecordedAudio {
    /// Length of the captured audio itself.
    #[cfg(test)]
    pub fn audio_duration(&self) -> Duration {
        if self.sample_rate == 0 {
            return Duration::ZERO;
        }
        Duration::from_secs_f64(self.samples.len() as f64 / f64::from(self.sample_rate))
    }

    /// 16 kHz mono PCM16 WAV for upload and storage.
    pub fn to_wav_16k(&self) -> Vec<u8> {
        encode_wav_16k(&self.samples, self.sample_rate)
    }

    /// Validates the recording against the encoded WAV size.
    pub fn check(&self, encoded_len: usize) -> RecordingCheck {
        evaluate_finished_recording(Some(encoded_len), self.received_audio_data)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prepared,
    Recording,
    Finished,
}

type Waiter = oneshot::Sender<Result<(), AudioError>>;

struct CoreState {
    opened_at: Instant,
    sample_rate: u32,
    channels: u16,
    phase: Phase,
    samples: Vec<f32>,
    pre_roll_stale: bool,
    gate: Option<SpeechGateState>,
    frames_since_window: usize,
    first_audio_at: Option<Instant>,
    last_frames_at: Option<Instant>,
    waiter: Option<(Waiter, Instant)>,
    recording_started_at: Option<Instant>,
    open_error: Option<AudioError>,
    device_error: Option<String>,
    device_error_reported: bool,
    expired: bool,
}

/// State shared between the audio callback, the capture thread and callers.
pub(crate) struct Core {
    state: Mutex<CoreState>,
    level_bits: AtomicU32,
    recording: AtomicBool,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct TickOutcome {
    report: Option<String>,
    stop: bool,
}

fn resolve(waiter: Option<(Waiter, Instant)>, result: Result<(), AudioError>) {
    if let Some((tx, _)) = waiter {
        let _ = tx.send(result);
    }
}

impl Core {
    fn new(opened_at: Instant) -> Self {
        Self {
            state: Mutex::new(CoreState {
                opened_at,
                sample_rate: 0,
                channels: 1,
                phase: Phase::Prepared,
                samples: Vec::new(),
                pre_roll_stale: false,
                gate: None,
                frames_since_window: 0,
                first_audio_at: None,
                last_frames_at: None,
                waiter: None,
                recording_started_at: None,
                open_error: None,
                device_error: None,
                device_error_reported: false,
                expired: false,
            }),
            level_bits: AtomicU32::new(0),
            recording: AtomicBool::new(false),
        }
    }

    fn set_format(&self, sample_rate: u32, channels: u16) {
        let mut st = self.state.lock();
        st.sample_rate = sample_rate;
        st.channels = channels.max(1);
        st.samples.reserve(sample_rate as usize * 30);
    }

    /// Audio callback body: interleaved samples in, mono buffered.
    fn push<I: Iterator<Item = f32>>(&self, interleaved: I, now: Instant) {
        let mut guard = self.state.lock();
        let st = &mut *guard;
        let channels = usize::from(st.channels.max(1));
        let keep = match st.phase {
            Phase::Recording => true,
            Phase::Prepared => !st.pre_roll_stale,
            Phase::Finished => false,
        };
        let mut frames = 0usize;
        let (mut acc, mut n) = (0.0f32, 0usize);
        for s in interleaved {
            acc += s;
            n += 1;
            if n == channels {
                frames += 1;
                if keep {
                    st.samples.push(acc / channels as f32);
                }
                acc = 0.0;
                n = 0;
            }
        }
        if frames == 0 {
            return;
        }
        st.last_frames_at = Some(now);
        st.first_audio_at.get_or_insert(now);
        if st.phase != Phase::Recording {
            return;
        }
        // Source delivery, silence included, establishes readiness.
        resolve(st.waiter.take(), Ok(()));
        if let Some(gate) = st.gate.as_mut() {
            let window_frames = (st.sample_rate * GATE_INTERVAL_MS / 1000).max(1) as usize;
            st.frames_since_window += frames;
            while st.frames_since_window >= window_frames {
                st.frames_since_window -= window_frames;
                let (rms, peak) = window_metrics(&st.samples, ANALYSER_WINDOW);
                gate.record_window(rms, peak);
            }
        }
        self.level_bits
            .store(level_rms(&st.samples).to_bits(), Ordering::Relaxed);
    }

    /// Switches to recording. `Ok(None)` when audio has already arrived.
    fn begin(
        &self,
        adopt_pre_roll: bool,
        first_audio_timeout: Duration,
        now: Instant,
    ) -> Result<Option<oneshot::Receiver<Result<(), AudioError>>>, AudioError> {
        let mut st = self.state.lock();
        if let Some(err) = st.open_error.clone() {
            return Err(err);
        }
        if let Some(message) = st.device_error.clone() {
            return Err(AudioError::DeviceLost(message));
        }
        match st.phase {
            Phase::Prepared => {}
            Phase::Recording => return Err(AudioError::Busy),
            Phase::Finished => return Err(AudioError::Cancelled),
        }
        let age = now.saturating_duration_since(st.opened_at);
        let adopt = adopt_pre_roll
            && !st.pre_roll_stale
            && age <= PRE_ROLL_MAX_AGE
            && !st.samples.is_empty();
        if adopt {
            // The gate never measured the pre-roll; fail it open rather than
            // discard an utterance spoken entirely before start.
            st.gate = None;
            st.recording_started_at = Some(st.opened_at);
        } else {
            st.samples.clear();
            st.gate = Some(SpeechGateState::default());
            st.recording_started_at = Some(now);
        }
        st.frames_since_window = 0;
        st.phase = Phase::Recording;
        self.level_bits.store(0, Ordering::Relaxed);
        self.recording.store(true, Ordering::Relaxed);
        if st.first_audio_at.is_some() {
            return Ok(None);
        }
        let (tx, rx) = oneshot::channel();
        st.waiter = Some((tx, now + first_audio_timeout));
        Ok(Some(rx))
    }

    fn fail_open(&self, error: AudioError) {
        let mut st = self.state.lock();
        st.open_error = Some(error.clone());
        resolve(st.waiter.take(), Err(error));
    }

    fn on_stream_error(&self, error: &cpal::Error) {
        match error.kind() {
            cpal::ErrorKind::Xrun
            | cpal::ErrorKind::DeviceChanged
            | cpal::ErrorKind::RealtimeDenied => {
                log::debug!("input stream notice: {error}");
            }
            _ => {
                log::warn!("input stream error: {error}");
                self.state
                    .lock()
                    .device_error
                    .get_or_insert_with(|| error.to_string());
            }
        }
    }

    fn tick(&self, now: Instant) -> TickOutcome {
        let mut st = self.state.lock();
        let mut outcome = TickOutcome::default();
        let age = now.saturating_duration_since(st.opened_at);
        if st.phase == Phase::Prepared {
            if !st.pre_roll_stale && age > PRE_ROLL_MAX_AGE {
                st.pre_roll_stale = true;
                st.samples = Vec::new();
            }
            if age > PREPARED_MAX_AGE {
                st.expired = true;
                st.phase = Phase::Finished;
                outcome.stop = true;
                return outcome;
            }
        }
        if st.phase == Phase::Recording
            && st.device_error.is_none()
            && let Some(last) = st.last_frames_at
            && now.saturating_duration_since(last) > STALL_TIMEOUT
        {
            st.device_error = Some("Microphone stopped delivering audio".into());
        }
        if st.phase == Phase::Recording
            && !st.device_error_reported
            && let Some(message) = st.device_error.clone()
        {
            st.device_error_reported = true;
            resolve(
                st.waiter.take(),
                Err(AudioError::DeviceLost(message.clone())),
            );
            outcome.report = Some(message);
        }
        if st
            .waiter
            .as_ref()
            .is_some_and(|(_, deadline)| now >= *deadline)
        {
            resolve(st.waiter.take(), Err(AudioError::FirstAudioUnavailable));
        }
        outcome
    }

    fn finish(&self, now: Instant) -> RecordedAudio {
        let mut st = self.state.lock();
        self.recording.store(false, Ordering::Relaxed);
        resolve(st.waiter.take(), Err(AudioError::Cancelled));
        let was_recording = st.phase == Phase::Recording;
        st.phase = Phase::Finished;
        let samples = if was_recording {
            std::mem::take(&mut st.samples)
        } else {
            Vec::new()
        };
        let duration = st
            .recording_started_at
            .map(|t| now.saturating_duration_since(t))
            .unwrap_or_default();
        RecordedAudio {
            received_audio_data: !samples.is_empty(),
            samples,
            sample_rate: st.sample_rate,
            duration,
            speech_gate: gate_decision(st.gate.as_ref()),
        }
    }

    fn discard(&self) {
        let mut st = self.state.lock();
        self.recording.store(false, Ordering::Relaxed);
        resolve(st.waiter.take(), Err(AudioError::Cancelled));
        st.phase = Phase::Finished;
        st.samples = Vec::new();
    }

    fn level(&self) -> Option<f32> {
        self.recording
            .load(Ordering::Relaxed)
            .then(|| f32::from_bits(self.level_bits.load(Ordering::Relaxed)))
    }
}

enum Command {
    Shutdown,
}

/// One open microphone stream on its own thread.
pub struct CaptureSession {
    core: Arc<Core>,
    commands: mpsc::Sender<Command>,
    selector: Option<String>,
}

fn normalize_selector(selector: Option<&str>) -> Option<String> {
    selector
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl CaptureSession {
    /// Starts opening `selector` (cpal id or name; `None` = system default)
    /// on a new thread and returns immediately. Buffering starts as soon as
    /// the stream runs; open errors surface from [`Self::begin_recording`].
    pub fn open(selector: Option<&str>, events: Option<EventSink>) -> Result<Self, AudioError> {
        let selector = normalize_selector(selector);
        let core = Arc::new(Core::new(Instant::now()));
        let (tx, rx) = mpsc::channel();
        let thread_core = core.clone();
        let thread_selector = selector.clone();
        std::thread::Builder::new()
            .name("whisper-mic".into())
            .spawn(move || capture_thread(thread_selector, thread_core, rx, events))
            .map_err(|e| AudioError::Open(e.to_string()))?;
        Ok(Self {
            core,
            commands: tx,
            selector,
        })
    }

    pub fn selector(&self) -> Option<&str> {
        self.selector.as_deref()
    }

    /// Starts recording. With `adopt_pre_roll`, audio buffered since `open`
    /// becomes the recording's opening when it is at most
    /// [`PRE_ROLL_MAX_AGE`] old. Resolves once the source has delivered
    /// frames; [`AudioError::FirstAudioUnavailable`] after `first_audio_timeout`.
    pub fn begin_recording(
        &self,
        adopt_pre_roll: bool,
        first_audio_timeout: Duration,
    ) -> BoxFuture<'static, Result<(), AudioError>> {
        match self
            .core
            .begin(adopt_pre_roll, first_audio_timeout, Instant::now())
        {
            Err(err) => futures::future::ready(Err(err)).boxed(),
            Ok(None) => futures::future::ready(Ok(())).boxed(),
            Ok(Some(rx)) => rx.map(|r| r.unwrap_or(Err(AudioError::Cancelled))).boxed(),
        }
    }

    pub fn level(&self) -> Option<f32> {
        self.core.level()
    }

    /// The prepared stream outlived [`PREPARED_MAX_AGE`] and was closed.
    pub fn is_expired(&self) -> bool {
        self.core.state.lock().expired
    }

    /// Stops and returns the recording; the stream closes on its thread.
    pub fn finish(self) -> RecordedAudio {
        self.core.finish(Instant::now())
    }

    /// Stops and drops all buffered audio.
    pub fn discard(self) {
        self.core.discard();
    }
}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        self.core.discard();
        let _ = self.commands.send(Command::Shutdown);
    }
}

fn capture_thread(
    selector: Option<String>,
    core: Arc<Core>,
    commands: mpsc::Receiver<Command>,
    events: Option<EventSink>,
) {
    let stream = match open_stream(selector.as_deref(), &core) {
        Ok(stream) => stream,
        Err(err) => {
            log::warn!("microphone open failed: {err}");
            core.fail_open(err);
            return;
        }
    };
    loop {
        match commands.recv_timeout(TICK) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let outcome = core.tick(Instant::now());
        if let (Some(message), Some(sink)) = (outcome.report, events.as_ref()) {
            sink(CaptureEvent::DeviceLost { message });
        }
        if outcome.stop {
            log::debug!("prepared microphone capture expired");
            break;
        }
    }
    drop(stream);
}

fn map_open_error(error: cpal::Error) -> AudioError {
    match error.kind() {
        cpal::ErrorKind::PermissionDenied => AudioError::PermissionDenied,
        _ => AudioError::Open(error.to_string()),
    }
}

fn open_stream(selector: Option<&str>, core: &Arc<Core>) -> Result<cpal::Stream, AudioError> {
    let host = cpal::default_host();
    let device = find_input_device(&host, selector)?;
    let supported = device.default_input_config().map_err(map_open_error)?;
    let config = supported.config();
    core.set_format(config.sample_rate, config.channels);
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, core),
        SampleFormat::F64 => build::<f64>(&device, config, core),
        SampleFormat::I8 => build::<i8>(&device, config, core),
        SampleFormat::I16 => build::<i16>(&device, config, core),
        SampleFormat::I24 => build::<cpal::I24>(&device, config, core),
        SampleFormat::I32 => build::<i32>(&device, config, core),
        SampleFormat::U8 => build::<u8>(&device, config, core),
        SampleFormat::U16 => build::<u16>(&device, config, core),
        SampleFormat::U32 => build::<u32>(&device, config, core),
        other => Err(AudioError::Open(format!(
            "unsupported sample format {other:?}"
        ))),
    }?;
    stream.play().map_err(map_open_error)?;
    log::info!(
        "recording stream opened: device={} rate={} channels={} format={:?}",
        describe(&device),
        config.sample_rate,
        config.channels,
        supported.sample_format()
    );
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    core: &Arc<Core>,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let data_core = core.clone();
    let error_core = core.clone();
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _| {
                data_core.push(
                    data.iter()
                        .map(|s| <f32 as FromSample<T>>::from_sample_(*s)),
                    Instant::now(),
                );
            },
            move |err| error_core.on_stream_error(&err),
            None,
        )
        .map_err(map_open_error)
}

struct ActiveCapture {
    session: CaptureSession,
    recording: bool,
    generation: u64,
}

#[derive(Default)]
struct RecorderInner {
    prepared: Option<CaptureSession>,
    active: Option<ActiveCapture>,
    generation: u64,
}

/// Dictation recorder: prepared capture with pre-roll, start/stop/cancel.
/// Cheap to clone; all clones share one state.
#[derive(Clone)]
pub struct Recorder {
    inner: Arc<Mutex<RecorderInner>>,
    events: Option<EventSink>,
}

impl Recorder {
    /// `events` receives device-loss notifications for live recordings.
    pub fn new(events: Option<EventSink>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RecorderInner::default())),
            events,
        }
    }

    /// Opens the microphone early (hotkey down) and buffers pre-roll.
    /// No-op while a recording is starting or live. Returns whether a
    /// prepared capture is now pending.
    pub fn prepare(&self, device: Option<&str>) -> bool {
        let selector = normalize_selector(device);
        let mut inner = self.inner.lock();
        if inner.active.is_some() {
            return false;
        }
        if inner
            .prepared
            .as_ref()
            .is_some_and(|p| !p.is_expired() && p.selector() == selector.as_deref())
        {
            return true;
        }
        inner.prepared = None;
        match CaptureSession::open(selector.as_deref(), self.events.clone()) {
            Ok(session) => {
                inner.prepared = Some(session);
                true
            }
            Err(err) => {
                log::warn!("microphone prepare failed: {err}");
                false
            }
        }
    }

    /// Closes a prepared capture that no recording took.
    pub fn cancel_prepared(&self) {
        self.inner.lock().prepared = None;
    }

    /// Starts recording on `device`, adopting a matching prepared capture.
    /// The future resolves once first audio arrives; on error the capture is
    /// closed. [`AudioError::FirstAudioUnavailable`] is the soft "no audio"
    /// outcome; [`AudioError::Cancelled`] means stop/cancel won the race.
    pub fn start(&self, device: Option<&str>) -> BoxFuture<'static, Result<(), AudioError>> {
        let selector = normalize_selector(device);
        let (first_audio, generation) = {
            let mut inner = self.inner.lock();
            if inner.active.is_some() {
                return futures::future::ready(Err(AudioError::Busy)).boxed();
            }
            let prepared = inner
                .prepared
                .take()
                .filter(|p| !p.is_expired() && p.selector() == selector.as_deref());
            let (session, adopt) = match prepared {
                Some(session) => (session, true),
                None => match CaptureSession::open(selector.as_deref(), self.events.clone()) {
                    Ok(session) => (session, false),
                    Err(err) => return futures::future::ready(Err(err)).boxed(),
                },
            };
            let first_audio = session.begin_recording(adopt, FIRST_AUDIO_TIMEOUT);
            inner.generation += 1;
            let generation = inner.generation;
            inner.active = Some(ActiveCapture {
                session,
                recording: false,
                generation,
            });
            (first_audio, generation)
        };
        let inner = self.inner.clone();
        async move {
            let result = first_audio.await;
            let mut guard = inner.lock();
            let current = guard
                .active
                .as_ref()
                .is_some_and(|a| a.generation == generation);
            if !current {
                return Err(AudioError::Cancelled);
            }
            match result {
                Ok(()) => {
                    if let Some(active) = guard.active.as_mut() {
                        active.recording = true;
                    }
                    Ok(())
                }
                Err(err) => {
                    guard.active = None;
                    Err(err)
                }
            }
        }
        .boxed()
    }

    /// Stops the live recording and returns it. A stop during start cancels
    /// instead and returns `None`.
    pub fn stop(&self) -> Option<RecordedAudio> {
        let active = self.inner.lock().active.take()?;
        if active.recording {
            Some(active.session.finish())
        } else {
            active.session.discard();
            None
        }
    }

    /// Discards the recording (or start in progress) and any prepared
    /// capture. Returns whether anything was open.
    pub fn cancel(&self) -> bool {
        let mut inner = self.inner.lock();
        let had_active = inner.active.take().is_some();
        let had_prepared = inner.prepared.take().is_some();
        had_active || had_prepared
    }

    /// Live input level while recording.
    pub fn level(&self) -> Option<f32> {
        self.inner.lock().active.as_ref()?.session.level()
    }

    #[cfg(test)]
    pub fn is_recording(&self) -> bool {
        self.inner
            .lock()
            .active
            .as_ref()
            .is_some_and(|a| a.recording)
    }

    /// The microphone is open (prepared, starting or recording).
    #[cfg(test)]
    pub fn is_capture_active(&self) -> bool {
        let inner = self.inner.lock();
        inner.active.is_some() || inner.prepared.as_ref().is_some_and(|p| !p.is_expired())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::gate::GateReason;
    use futures::executor::block_on;

    fn make_core(rate: u32, channels: u16, opened_at: Instant) -> Core {
        let core = Core::new(opened_at);
        core.set_format(rate, channels);
        core
    }

    fn tone(n: usize, amp: f32) -> Vec<f32> {
        (0..n).map(|i| amp * ((i as f32) * 0.05).sin()).collect()
    }

    #[test]
    fn level_keeps_quiet_input_below_byte_resolution() {
        assert_eq!(level_rms(&[]), 0.0);
        // A constant 0.003 is under one byte step (1/128) and quantizes away.
        let quiet = vec![0.003; LEVEL_WINDOW];
        assert!((level_rms(&quiet) - 0.003).abs() < 1e-6);
        // Only the trailing window counts.
        let mut tail = vec![1.0; LEVEL_WINDOW];
        tail.extend(vec![0.5; LEVEL_WINDOW]);
        assert!((level_rms(&tail) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn adopts_fresh_pre_roll_and_fails_the_gate_open() {
        let t0 = Instant::now();
        let core = make_core(48_000, 1, t0);
        core.push(tone(4800, 0.3).into_iter(), t0 + Duration::from_millis(100));
        let rx = core
            .begin(true, FIRST_AUDIO_TIMEOUT, t0 + Duration::from_millis(500))
            .unwrap();
        assert!(rx.is_none(), "first audio already arrived");
        core.push(tone(4800, 0.3).into_iter(), t0 + Duration::from_millis(600));
        let audio = core.finish(t0 + Duration::from_millis(1500));
        assert_eq!(audio.samples.len(), 9600);
        assert_eq!(audio.speech_gate.reason, GateReason::Unavailable);
        assert!(!audio.speech_gate.skip);
        assert_eq!(
            audio.duration,
            Duration::from_millis(1500),
            "duration starts at prepare"
        );
        assert!(audio.received_audio_data);
    }

    #[test]
    fn stale_pre_roll_is_discarded() {
        let t0 = Instant::now();
        let core = make_core(48_000, 1, t0);
        core.push(tone(4800, 0.3).into_iter(), t0 + Duration::from_millis(100));
        assert_eq!(
            core.tick(t0 + Duration::from_millis(2100)),
            TickOutcome::default()
        );
        core.begin(true, FIRST_AUDIO_TIMEOUT, t0 + Duration::from_millis(2200))
            .unwrap();
        core.push(
            vec![0.0; 48_000].into_iter(),
            t0 + Duration::from_millis(3200),
        );
        let audio = core.finish(t0 + Duration::from_millis(3200));
        assert_eq!(audio.samples.len(), 48_000);
        assert_eq!(audio.duration, Duration::from_millis(1000));
        // Ten 100 ms windows of digital silence.
        assert!(audio.speech_gate.is_silence());
        assert_eq!(audio.speech_gate.metrics.unwrap().window_count, 10);
    }

    #[test]
    fn direct_start_measures_the_gate() {
        let t0 = Instant::now();
        let core = make_core(48_000, 2, t0);
        let rx = core.begin(false, FIRST_AUDIO_TIMEOUT, t0).unwrap().unwrap();
        // Stereo frames are averaged: (0.4 + 0.0) / 2.
        let stereo: Vec<f32> = (0..48_000)
            .flat_map(|i| [0.4 * ((i as f32) * 0.05).sin(), 0.0])
            .collect();
        core.push(stereo.into_iter(), t0 + Duration::from_millis(50));
        assert_eq!(block_on(rx).unwrap(), Ok(()));
        let level = core.level().unwrap();
        assert!(level > 0.1 && level < 0.2, "level {level}");
        let audio = core.finish(t0 + Duration::from_secs(1));
        assert_eq!(audio.samples.len(), 48_000);
        assert_eq!(audio.speech_gate.reason, GateReason::SpeechDetected);
        assert_eq!(audio.speech_gate.metrics.unwrap().window_count, 10);
        assert_eq!(core.level(), None, "level rests after stop");
    }

    #[test]
    fn first_audio_timeout_is_distinct() {
        let t0 = Instant::now();
        let core = make_core(16_000, 1, t0);
        let rx = core
            .begin(false, Duration::from_millis(300), t0)
            .unwrap()
            .unwrap();
        core.tick(t0 + Duration::from_millis(200));
        core.tick(t0 + Duration::from_millis(400));
        let err = block_on(rx).unwrap().unwrap_err();
        assert!(err.is_no_audio());
    }

    #[test]
    fn open_failure_resolves_the_waiter() {
        let t0 = Instant::now();
        let core = make_core(16_000, 1, t0);
        let rx = core.begin(false, FIRST_AUDIO_TIMEOUT, t0).unwrap().unwrap();
        core.fail_open(AudioError::DeviceNotFound("USB".into()));
        assert_eq!(
            block_on(rx).unwrap(),
            Err(AudioError::DeviceNotFound("USB".into()))
        );

        let late = make_core(16_000, 1, t0);
        late.fail_open(AudioError::PermissionDenied);
        assert_eq!(
            late.begin(false, FIRST_AUDIO_TIMEOUT, t0).unwrap_err(),
            AudioError::PermissionDenied
        );
    }

    #[test]
    fn stall_during_recording_reports_device_loss_once() {
        let t0 = Instant::now();
        let core = make_core(48_000, 1, t0);
        core.begin(false, FIRST_AUDIO_TIMEOUT, t0).unwrap();
        core.push(tone(480, 0.1).into_iter(), t0 + Duration::from_millis(10));
        assert_eq!(core.tick(t0 + Duration::from_millis(1000)).report, None);
        let outcome = core.tick(t0 + Duration::from_millis(1600));
        assert_eq!(
            outcome.report.as_deref(),
            Some("Microphone stopped delivering audio")
        );
        assert_eq!(core.tick(t0 + Duration::from_millis(1700)).report, None);
    }

    #[test]
    fn device_error_before_start_fails_start() {
        let t0 = Instant::now();
        let core = make_core(48_000, 1, t0);
        core.state.lock().device_error = Some("gone".into());
        assert_eq!(
            core.tick(t0).report,
            None,
            "prepared captures do not report"
        );
        assert_eq!(
            core.begin(true, FIRST_AUDIO_TIMEOUT, t0).unwrap_err(),
            AudioError::DeviceLost("gone".into())
        );
    }

    #[test]
    fn prepared_capture_expires() {
        let t0 = Instant::now();
        let core = make_core(48_000, 1, t0);
        assert!(!core.tick(t0 + Duration::from_secs(9)).stop);
        assert!(core.tick(t0 + Duration::from_millis(10_001)).stop);
        assert!(core.state.lock().expired);
        assert_eq!(
            core.begin(true, FIRST_AUDIO_TIMEOUT, t0).unwrap_err(),
            AudioError::Cancelled
        );
    }

    #[test]
    fn discard_cancels_a_pending_start() {
        let t0 = Instant::now();
        let core = make_core(48_000, 1, t0);
        let rx = core.begin(false, FIRST_AUDIO_TIMEOUT, t0).unwrap().unwrap();
        core.discard();
        assert_eq!(block_on(rx).unwrap(), Err(AudioError::Cancelled));
        let audio = core.finish(t0);
        assert!(audio.samples.is_empty());
        assert!(!audio.received_audio_data);
    }

    #[test]
    fn recorded_audio_helpers() {
        let audio = RecordedAudio {
            samples: vec![0.1; 48_000],
            sample_rate: 48_000,
            duration: Duration::from_secs(1),
            received_audio_data: true,
            speech_gate: SpeechGateDecision::unavailable(),
        };
        assert_eq!(audio.audio_duration(), Duration::from_secs(1));
        let wav = audio.to_wav_16k();
        assert!(crate::audio::wav::is_pcm16_mono_16k_wav(&wav));
        assert!(audio.check(wav.len()).usable);
        assert!(!audio.check(44).usable);
    }

    #[test]
    fn unknown_device_fails_start() {
        let recorder = Recorder::new(None);
        let err = block_on(recorder.start(Some("no-such-microphone-xyz"))).unwrap_err();
        assert!(
            matches!(err, AudioError::DeviceNotFound(_) | AudioError::Open(_)),
            "{err:?}"
        );
        assert!(!recorder.is_recording());
        assert!(!recorder.is_capture_active());
    }

    /// Records ~1 s from the default microphone (needs mic permission).
    #[test]
    #[ignore]
    fn live_microphone_capture() {
        let recorder = Recorder::new(None);
        assert!(recorder.prepare(None));
        std::thread::sleep(Duration::from_millis(300));
        block_on(recorder.start(None)).expect("first audio");
        assert!(recorder.is_recording());
        std::thread::sleep(Duration::from_millis(1000));
        assert!(recorder.level().is_some());
        let audio = recorder.stop().expect("recording");
        assert!(audio.received_audio_data);
        assert!(audio.audio_duration() > Duration::from_millis(1000));
        eprintln!(
            "captured {:?} at {} Hz, gate {:?}",
            audio.audio_duration(),
            audio.sample_rate,
            audio.speech_gate.reason
        );
    }
}
