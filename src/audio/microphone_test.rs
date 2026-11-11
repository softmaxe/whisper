//! Memory-only loopback lifecycle. The clock is supplied by the caller so the
//! ten-second limit and cancellation can be exercised without audio hardware.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use parking_lot::Mutex;

use super::AudioError;
use super::capture::{CaptureEvent, RecordedAudio, Recorder};
#[cfg(any(debug_assertions, test))]
use super::gate::SpeechGate;
use super::gate::{ANALYSER_WINDOW, window_rms};
use super::playback::Playback;

pub const RECORDING_LIMIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Permission,
    Device,
    NoSignal,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Closed,
    Recording,
    Ready,
    Playing,
    Error(Problem),
}

struct Clip {
    samples: Arc<Vec<f32>>,
    rate: u32,
    duration: Duration,
    silent: bool,
}

pub struct MicrophoneTest {
    state: State,
    recorder: Recorder,
    events: async_channel::Receiver<CaptureEvent>,
    clip: Option<Clip>,
    playback: Option<Playback>,
    started: Instant,
    generation: u64,
    progress: f32,
}

impl Default for MicrophoneTest {
    fn default() -> Self {
        let (tx, events) = async_channel::unbounded();
        Self {
            state: State::Closed,
            recorder: Recorder::new(Some(Arc::new(move |event| {
                let _ = tx.try_send(event);
            }))),
            events,
            clip: None,
            playback: None,
            started: Instant::now(),
            generation: 0,
            progress: 0.,
        }
    }
}

impl MicrophoneTest {
    pub fn state(&self) -> State {
        self.state
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn level(&self) -> f32 {
        self.recorder.level().unwrap_or(0.)
    }
    pub fn duration(&self) -> Duration {
        self.clip
            .as_ref()
            .map_or(Duration::ZERO, |clip| clip.duration)
    }
    pub fn silent(&self) -> bool {
        self.clip.as_ref().is_some_and(|clip| clip.silent)
    }
    pub fn progress(&self) -> f32 {
        self.playback
            .as_ref()
            .map_or(self.progress, Playback::progress)
    }
    pub fn remaining(&self, now: Instant) -> Duration {
        RECORDING_LIMIT.saturating_sub(now.saturating_duration_since(self.started))
    }

    pub fn start(
        &mut self,
        selector: Option<&str>,
        now: Instant,
    ) -> (u64, BoxFuture<'static, Result<(), AudioError>>) {
        self.begin(now);
        (self.generation, self.recorder.start(selector))
    }

    fn begin(&mut self, now: Instant) {
        self.close();
        self.started = now;
        self.state = State::Recording;
    }

    pub fn capture_started(
        &mut self,
        generation: u64,
        result: Result<(), AudioError>,
        now: Instant,
    ) {
        if generation != self.generation || self.state != State::Recording {
            return;
        }
        match result {
            Ok(()) => self.started = now,
            Err(error) => self.fail(&error),
        }
    }

    pub fn stop(&mut self) {
        if self.state != State::Recording {
            return;
        }
        match self.recorder.stop() {
            Some(audio) => self.finish_audio(audio),
            None => self.fail(&AudioError::FirstAudioUnavailable),
        }
    }

    fn finish_audio(&mut self, mut audio: RecordedAudio) {
        if !audio.received_audio_data || audio.samples.is_empty() || audio.sample_rate == 0 {
            self.fail(&AudioError::FirstAudioUnavailable);
            return;
        }
        // UI polling may stop capture late; every finalization path enforces
        // the sample limit before measuring or retaining the clip.
        let max_samples = audio.sample_rate as usize * RECORDING_LIMIT.as_secs() as usize;
        if audio.samples.len() > max_samples {
            audio.samples.truncate(max_samples);
            audio.speech_gate = Default::default();
        }
        // A short clip can finish before capture has measured its first gate.
        if audio.speech_gate.window_count == 0 {
            for window in audio.samples.chunks(ANALYSER_WINDOW) {
                audio
                    .speech_gate
                    .record_window(window_rms(window, ANALYSER_WINDOW));
            }
        }
        let duration =
            Duration::from_secs_f64(audio.samples.len() as f64 / f64::from(audio.sample_rate));
        self.clip = Some(Clip {
            samples: Arc::new(audio.samples),
            rate: audio.sample_rate,
            duration,
            silent: audio.speech_gate.is_silence(),
        });
        self.state = State::Ready;
        self.progress = 0.;
    }

    pub fn toggle_playback(&mut self) {
        self.toggle_playback_with(|clip| {
            Playback::start(clip.samples.clone(), clip.rate, true).map(Some)
        });
    }

    fn toggle_playback_with(
        &mut self,
        start: impl FnOnce(&Clip) -> Result<Option<Playback>, AudioError>,
    ) {
        if self.state == State::Playing {
            self.playback = None;
            self.state = State::Ready;
            return;
        }
        if self.state != State::Ready {
            return;
        }
        let Some(clip) = &self.clip else {
            return;
        };
        match start(clip) {
            Ok(playback) => {
                self.playback = playback;
                self.state = State::Playing;
                self.progress = 0.;
            }
            Err(error) => self.fail(&error),
        }
    }

    pub fn tick(&mut self, now: Instant) {
        while let Ok(CaptureEvent::DeviceLost { capture_id, .. }) = self.events.try_recv() {
            if self.state == State::Recording
                && self.recorder.active_capture_id() == Some(capture_id)
            {
                self.fail(&AudioError::DeviceLost(String::new()));
            }
        }
        if self.state == State::Recording && self.remaining(now).is_zero() {
            self.stop();
        }
        if self.state == State::Playing && self.playback.as_ref().is_some_and(Playback::is_done) {
            let error = self.playback.as_ref().and_then(Playback::error);
            self.playback = None;
            self.playback_finished(error);
        }
    }

    fn playback_finished(&mut self, error: Option<AudioError>) {
        if let Some(error) = error {
            self.fail(&error);
        } else {
            self.state = State::Ready;
            self.progress = 1.;
        }
    }

    fn fail(&mut self, error: &AudioError) {
        self.recorder.cancel();
        self.playback = None;
        self.state = State::Error(match error {
            AudioError::PermissionDenied => Problem::Permission,
            AudioError::FirstAudioUnavailable => Problem::NoSignal,
            AudioError::Output(_) => Problem::Output,
            _ => Problem::Device,
        });
    }

    pub fn close(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.recorder.cancel();
        self.playback = None;
        self.clip = None;
        self.state = State::Closed;
        self.progress = 0.;
    }

    #[cfg(debug_assertions)]
    pub fn dev_scene(&mut self, scene: &str) {
        self.begin(Instant::now() - Duration::from_secs(3));
        if scene == "ready" {
            self.finish_audio(RecordedAudio {
                samples: vec![0.1; 48_000],
                sample_rate: 16_000,
                received_audio_data: true,
                speech_gate: SpeechGate {
                    peak_rms: 0.1,
                    window_count: 30,
                },
            });
        } else if scene == "error" {
            self.fail(&AudioError::PermissionDenied);
        }
    }
}

impl Drop for MicrophoneTest {
    fn drop(&mut self) {
        self.close();
    }
}

/// Called before dictation opens or adopts a microphone, including prewarming.
pub fn cancel_for_dictation(test: &Weak<Mutex<MicrophoneTest>>) {
    if let Some(test) = test.upgrade() {
        test.lock().close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording(now: Instant, amplitude: f32) -> MicrophoneTest {
        let mut test = MicrophoneTest::default();
        test.begin(now);
        test.recorder = Recorder::with_test_audio(vec![amplitude; 16_000]);
        test
    }

    #[test]
    fn injected_clock_auto_stops_at_ten_seconds_and_manual_stop_is_ready() {
        let now = Instant::now();
        let mut test = recording(now, 0.1);
        test.tick(now + Duration::from_millis(9999));
        assert_eq!(test.state(), State::Recording);
        test.tick(now + RECORDING_LIMIT);
        assert_eq!(test.state(), State::Ready);
        assert_eq!(test.duration(), Duration::from_secs(1));
        assert!(!test.silent());
        let mut test = recording(now, 0.1);
        test.stop();
        assert_eq!(test.state(), State::Ready);
        test.begin(now);
        assert_eq!(test.state(), State::Recording);
        assert!(test.clip.is_none());
    }

    #[test]
    fn delayed_ticks_and_manual_stops_retain_at_most_ten_seconds() {
        let now = Instant::now();
        for auto_stop in [false, true] {
            let mut test = recording(now, 0.1);
            test.recorder = Recorder::with_test_audio(vec![0.1; 25 * 16_000]);
            if auto_stop {
                test.tick(now + Duration::from_secs(90));
            } else {
                test.stop();
            }
            assert_eq!(test.state(), State::Ready);
            let clip = test.clip.as_ref().unwrap();
            assert_eq!(clip.samples.len(), 10 * 16_000);
            assert_eq!(test.duration(), RECORDING_LIMIT);
        }
    }

    #[test]
    fn finalization_limits_native_rate_samples_and_measures_only_retained_audio() {
        for rate in [16_000, 44_100, 48_000] {
            let mut test = recording(Instant::now(), 0.1);
            let mut samples = vec![0.; 10 * rate as usize];
            samples.extend(vec![0.5; rate as usize]);
            test.finish_audio(RecordedAudio {
                samples,
                sample_rate: rate,
                received_audio_data: true,
                speech_gate: SpeechGate {
                    peak_rms: 0.5,
                    window_count: 110,
                },
            });
            let clip = test.clip.as_ref().unwrap();
            assert_eq!(clip.samples.len(), 10 * rate as usize);
            assert_eq!(clip.rate, rate);
            assert_eq!(test.duration(), RECORDING_LIMIT);
            assert!(test.silent());
        }
    }

    #[test]
    fn silent_clips_still_allow_playback_and_replay() {
        let mut test = recording(Instant::now(), 0.);
        test.stop();
        assert!(test.silent());
        test.toggle_playback_with(|_| Ok(None));
        assert_eq!(test.state(), State::Playing);
        test.playback_finished(None);
        assert_eq!(test.state(), State::Ready);
        assert_eq!(test.progress(), 1.);
        test.toggle_playback_with(|_| Ok(None));
        test.toggle_playback();
        assert_eq!(test.state(), State::Ready);
        assert!(test.clip.is_some());
        test.toggle_playback_with(|_| Err(AudioError::Output("disconnected".into())));
        assert_eq!(test.state(), State::Error(Problem::Output));
        test.close();
        assert_eq!(test.state(), State::Closed);
        assert!(test.clip.is_none());
    }

    #[test]
    fn errors_are_named_and_late_capture_results_cannot_reopen_a_test() {
        for (error, problem) in [
            (AudioError::PermissionDenied, Problem::Permission),
            (
                AudioError::DeviceLost("disconnected".into()),
                Problem::Device,
            ),
            (AudioError::FirstAudioUnavailable, Problem::NoSignal),
            (AudioError::Output("unavailable".into()), Problem::Output),
        ] {
            let now = Instant::now();
            let mut test = recording(now, 0.1);
            let generation = test.generation();
            test.capture_started(generation, Err(error), now);
            assert_eq!(test.state(), State::Error(problem));
            assert!(!test.recorder.is_capture_active());
            test.begin(now);
            let generation = test.generation();
            test.close();
            test.capture_started(generation, Ok(()), now);
            assert_eq!(test.state(), State::Closed);
        }
        let mut test = recording(Instant::now(), 0.1);
        test.finish_audio(RecordedAudio {
            samples: Vec::new(),
            sample_rate: 16_000,
            received_audio_data: false,
            speech_gate: SpeechGate::default(),
        });
        assert_eq!(test.state(), State::Error(Problem::NoSignal));
    }

    #[test]
    fn dictation_and_drop_cancel_capture_and_free_the_memory_clip() {
        let now = Instant::now();
        let test = Arc::new(Mutex::new(recording(now, 0.1)));
        let recorder = test.lock().recorder.clone();
        cancel_for_dictation(&Arc::downgrade(&test));
        assert!(!recorder.is_capture_active());
        assert_eq!(test.lock().state(), State::Closed);
        let mut test = recording(now, 0.1);
        let recorder = test.recorder.clone();
        test.stop();
        let clip = Arc::downgrade(&test.clip.as_ref().unwrap().samples);
        drop(test);
        assert!(!recorder.is_capture_active());
        assert!(clip.upgrade().is_none());
    }

    #[test]
    #[ignore]
    fn live_microphone_loopback() {
        let mut test = MicrophoneTest::default();
        let (generation, start) = test.start(None, Instant::now());
        let result = futures::executor::block_on(start);
        test.capture_started(generation, result, Instant::now());
        assert_eq!(test.state(), State::Recording);
        std::thread::sleep(Duration::from_secs(2));
        test.stop();
        assert_eq!(test.state(), State::Ready);
        test.toggle_playback();
        while test.state() == State::Playing {
            test.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(test.state(), State::Ready);
    }
}
