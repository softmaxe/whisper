//! Dictation start/stop cues.
//!
//! Each cue is two sine notes with this envelope: the gain
//! is set to 0.0001 at the note start, ramps linearly to 0.2 over 15 ms, then
//! exponentially back to 0.0001 at 90 ms; the oscillator stops 10 ms later.
//! Notes are 25 ms apart and the first starts 5 ms after scheduling. The
//! buffer is rendered up front and played on a short-lived output stream on
//! its own thread, so playing never blocks the caller. Callers check
//! `Settings::audio_cues_enabled` first.

use std::f64::consts::PI;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use super::AudioError;

pub const START_NOTES: [f64; 2] = [523.25, 659.25];
pub const STOP_NOTES: [f64; 2] = [587.33, 440.0];
pub const NOTE_DURATION_SECONDS: f64 = 0.09;
pub const NOTE_GAP_SECONDS: f64 = 0.025;
pub const NOTE_ATTACK_SECONDS: f64 = 0.015;
pub const MAX_GAIN: f64 = 0.2;
pub const MIN_GAIN: f64 = 0.0001;
const SCHEDULE_LEAD_SECONDS: f64 = 0.005;
const OSCILLATOR_TAIL_SECONDS: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Start,
    Stop,
}

impl Cue {
    pub fn notes(self) -> &'static [f64] {
        match self {
            Self::Start => &START_NOTES,
            Self::Stop => &STOP_NOTES,
        }
    }
}

/// Gain envelope at `t` seconds after the note start.
fn envelope(t: f64) -> f64 {
    if t < 0.0 {
        0.0
    } else if t < NOTE_ATTACK_SECONDS {
        MIN_GAIN + (MAX_GAIN - MIN_GAIN) * (t / NOTE_ATTACK_SECONDS)
    } else if t < NOTE_DURATION_SECONDS {
        let progress = (t - NOTE_ATTACK_SECONDS) / (NOTE_DURATION_SECONDS - NOTE_ATTACK_SECONDS);
        MAX_GAIN * (MIN_GAIN / MAX_GAIN).powf(progress)
    } else if t < NOTE_DURATION_SECONDS + OSCILLATOR_TAIL_SECONDS {
        MIN_GAIN
    } else {
        0.0
    }
}

/// Total cue length including the scheduling lead.
pub fn cue_duration(notes: &[f64]) -> Duration {
    let count = notes.len().max(1) as f64;
    Duration::from_secs_f64(
        SCHEDULE_LEAD_SECONDS
            + (count - 1.0) * (NOTE_DURATION_SECONDS + NOTE_GAP_SECONDS)
            + NOTE_DURATION_SECONDS
            + OSCILLATOR_TAIL_SECONDS,
    )
}

/// Renders the cue as mono samples at `sample_rate`.
pub fn render_cue(notes: &[f64], sample_rate: u32) -> Vec<f32> {
    let rate = f64::from(sample_rate.max(1));
    let len = (cue_duration(notes).as_secs_f64() * rate).ceil() as usize;
    (0..len)
        .map(|i| {
            let t = i as f64 / rate;
            notes
                .iter()
                .enumerate()
                .map(|(index, &freq)| {
                    let start = SCHEDULE_LEAD_SECONDS
                        + index as f64 * (NOTE_DURATION_SECONDS + NOTE_GAP_SECONDS);
                    let local = t - start;
                    if local < 0.0 {
                        return 0.0;
                    }
                    envelope(local) * (2.0 * PI * freq * local).sin()
                })
                .sum::<f64>() as f32
        })
        .collect()
}

pub fn play_start_cue() {
    play(Cue::Start);
}

pub fn play_stop_cue() {
    play(Cue::Stop);
}

/// Plays a cue on the default output device without blocking.
pub fn play(cue: Cue) {
    let spawned = std::thread::Builder::new()
        .name("whisper-cue".into())
        .spawn(move || {
            if let Err(err) = play_blocking(cue) {
                log::debug!("failed to play dictation cue: {err}");
            }
        });
    if let Err(err) = spawned {
        log::debug!("failed to start cue thread: {err}");
    }
}

/// Plays a cue and returns once it has finished.
pub fn play_blocking(cue: Cue) -> Result<(), AudioError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| AudioError::Output("no output device".into()))?;
    let supported = device
        .default_output_config()
        .map_err(|e| AudioError::Output(e.to_string()))?;
    let config = supported.config();
    let samples = render_cue(cue.notes(), config.sample_rate);
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, samples),
        SampleFormat::F64 => build::<f64>(&device, config, samples),
        SampleFormat::I16 => build::<i16>(&device, config, samples),
        SampleFormat::I32 => build::<i32>(&device, config, samples),
        SampleFormat::U16 => build::<u16>(&device, config, samples),
        SampleFormat::U8 => build::<u8>(&device, config, samples),
        other => Err(AudioError::Output(format!(
            "unsupported sample format {other:?}"
        ))),
    }?;
    stream
        .play()
        .map_err(|e| AudioError::Output(e.to_string()))?;
    std::thread::sleep(cue_duration(cue.notes()) + Duration::from_millis(80));
    drop(stream);
    Ok(())
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    samples: Vec<f32>,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let mut position = 0usize;
    device
        .build_output_stream::<T, _, _>(
            config,
            move |out: &mut [T], _| {
                for frame in out.chunks_mut(channels) {
                    let value = samples.get(position).copied().unwrap_or(0.0);
                    position += 1;
                    for slot in frame {
                        *slot = <T as FromSample<f32>>::from_sample_(value);
                    }
                }
            },
            |err| log::debug!("cue output error: {err}"),
            None,
        )
        .map_err(|e| AudioError::Output(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_matches_the_cue_schedule() {
        let rate = 48_000;
        let samples = render_cue(&START_NOTES, rate);
        // 5 ms lead + 115 ms + 90 ms + 10 ms tail = 220 ms.
        assert_eq!(cue_duration(&START_NOTES), Duration::from_millis(220));
        assert_eq!(samples.len(), (0.22 * 48_000.0f64).ceil() as usize);
        let at = |ms: f64| (ms / 1000.0 * f64::from(rate)) as usize;
        let peak =
            |from: usize, to: usize| samples[from..to].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert_eq!(peak(0, at(5.0)), 0.0, "silent lead");
        assert!(peak(at(15.0), at(25.0)) > 0.15, "first note near full gain");
        assert!(peak(at(96.0), at(120.0)) < 0.001, "gap between notes");
        assert!(peak(at(130.0), at(140.0)) > 0.15, "second note");
        assert!(peak(at(5.0), samples.len()) <= 0.2 + 1e-6);
    }

    #[test]
    fn envelope_shape() {
        assert!((envelope(0.0) - MIN_GAIN).abs() < 1e-12);
        assert!((envelope(NOTE_ATTACK_SECONDS) - MAX_GAIN).abs() < 1e-9);
        assert!((envelope(NOTE_DURATION_SECONDS - 1e-9) - MIN_GAIN).abs() < 1e-6);
        assert_eq!(envelope(0.2), 0.0);
    }

    #[test]
    #[ignore]
    fn live_cue_playback() {
        play_blocking(Cue::Start).unwrap();
        play_blocking(Cue::Stop).unwrap();
    }
}
