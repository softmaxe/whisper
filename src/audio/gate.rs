//! Local speech gate.
//!
//! Every 100 ms of recording the capture thread measures RMS and peak over the
//! most recent 2048 samples, quantized to bytes and normalized back to −1..1.
//! The dictation pipeline skips transcription only when the decision says
//! `skip` with reason `Silence`; `InsufficientSpeech` is reported but still
//! transcribed.

use serde::Serialize;

pub const SILENCE_RMS_THRESHOLD: f64 = 0.002;
pub const SPEECH_WINDOW_RMS_THRESHOLD: f64 = 0.003;
pub const SPEECH_WINDOW_PEAK_THRESHOLD: f64 = 0.02;
pub const STRONG_SPEECH_RMS_THRESHOLD: f64 = 0.006;

/// Samples per gate window.
pub const ANALYSER_WINDOW: usize = 2048;
/// Interval between gate windows.
pub const GATE_INTERVAL_MS: u32 = 100;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SpeechGateState {
    pub peak_rms: f64,
    pub peak_amplitude: f64,
    pub window_count: u32,
    pub speech_window_count: u32,
    pub consecutive_speech_windows: u32,
    pub max_consecutive_speech_windows: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateReason {
    /// No windows were measured (e.g. pre-roll adopted); fail open.
    Unavailable,
    Silence,
    InsufficientSpeech,
    SpeechDetected,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GateMetrics {
    pub peak_rms: f64,
    pub peak_amplitude: f64,
    pub window_count: u32,
    pub speech_window_count: u32,
    pub max_consecutive_speech_windows: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpeechGateDecision {
    pub skip: bool,
    pub reason: GateReason,
    /// Present whenever at least one window was measured.
    pub metrics: Option<GateMetrics>,
}

impl SpeechGateDecision {
    pub fn unavailable() -> Self {
        Self {
            skip: false,
            reason: GateReason::Unavailable,
            metrics: None,
        }
    }

    /// The only outcome the pipeline acts on: skip as silence.
    pub fn is_silence(&self) -> bool {
        self.skip && self.reason == GateReason::Silence
    }
}

impl SpeechGateState {
    pub fn record_window(&mut self, rms: f64, peak: f64) {
        self.window_count += 1;
        self.peak_rms = self.peak_rms.max(rms);
        self.peak_amplitude = self.peak_amplitude.max(peak);
        let is_speech = rms >= SPEECH_WINDOW_RMS_THRESHOLD && peak >= SPEECH_WINDOW_PEAK_THRESHOLD;
        if !is_speech {
            self.consecutive_speech_windows = 0;
            return;
        }
        self.speech_window_count += 1;
        self.consecutive_speech_windows += 1;
        self.max_consecutive_speech_windows = self
            .max_consecutive_speech_windows
            .max(self.consecutive_speech_windows);
    }

    pub fn decision(&self) -> SpeechGateDecision {
        if self.window_count == 0 {
            return SpeechGateDecision::unavailable();
        }
        let metrics = Some(GateMetrics {
            peak_rms: self.peak_rms,
            peak_amplitude: self.peak_amplitude,
            window_count: self.window_count,
            speech_window_count: self.speech_window_count,
            max_consecutive_speech_windows: self.max_consecutive_speech_windows,
        });
        if self.peak_rms < SILENCE_RMS_THRESHOLD {
            return SpeechGateDecision {
                skip: true,
                reason: GateReason::Silence,
                metrics,
            };
        }
        let has_speech =
            self.speech_window_count >= 1 || self.peak_rms >= STRONG_SPEECH_RMS_THRESHOLD;
        SpeechGateDecision {
            skip: !has_speech,
            reason: if has_speech {
                GateReason::SpeechDetected
            } else {
                GateReason::InsufficientSpeech
            },
            metrics,
        }
    }
}

/// Decision for an optional state; `None` (gate torn down) fails open.
pub fn gate_decision(state: Option<&SpeechGateState>) -> SpeechGateDecision {
    state.map_or_else(SpeechGateDecision::unavailable, SpeechGateState::decision)
}

/// A float sample quantized to a byte time-domain value, back in −1..1:
/// `byte = trunc(clamp(128 * (x + 1), 0, 255))`, `v = (byte - 128) / 128`.
#[inline]
pub fn byte_quantize(sample: f32) -> f64 {
    let scaled = (128.0 * (f64::from(sample) + 1.0)).clamp(0.0, 255.0);
    (scaled.trunc() - 128.0) / 128.0
}

/// RMS and peak over the last `window` samples after byte quantization.
/// Fewer samples than the window are zero-padded.
pub fn window_metrics(samples: &[f32], window: usize) -> (f64, f64) {
    if window == 0 {
        return (0.0, 0.0);
    }
    let start = samples.len().saturating_sub(window);
    let mut sum = 0.0;
    let mut peak: f64 = 0.0;
    for &s in &samples[start..] {
        let v = byte_quantize(s);
        sum += v * v;
        peak = peak.max(v.abs());
    }
    ((sum / window as f64).sqrt(), peak)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(windows: &[(f64, f64)]) -> SpeechGateState {
        let mut state = SpeechGateState::default();
        for &(rms, peak) in windows {
            state.record_window(rms, peak);
        }
        state
    }

    #[test]
    fn fails_open_without_windows() {
        assert_eq!(
            SpeechGateState::default().decision(),
            SpeechGateDecision::unavailable()
        );
        assert_eq!(gate_decision(None), SpeechGateDecision::unavailable());
        assert!(!gate_decision(None).skip);
    }

    #[test]
    fn near_silence_is_skippable() {
        let d = state_with(&[(0.0012, 0.01), (0.0016, 0.015), (0.0014, 0.012)]).decision();
        assert!(d.skip);
        assert_eq!(d.reason, GateReason::Silence);
        assert!(d.is_silence());
        assert_eq!(
            d.metrics,
            Some(GateMetrics {
                peak_rms: 0.0016,
                peak_amplitude: 0.015,
                window_count: 3,
                speech_window_count: 0,
                max_consecutive_speech_windows: 0,
            })
        );
    }

    #[test]
    fn isolated_noise_is_insufficient_speech() {
        let d = state_with(&[(0.0025, 0.015), (0.0028, 0.018), (0.0022, 0.014)]).decision();
        assert!(d.skip);
        assert_eq!(d.reason, GateReason::InsufficientSpeech);
        assert!(!d.is_silence());
        let m = d.metrics.unwrap();
        assert_eq!(
            (m.peak_rms, m.peak_amplitude, m.window_count),
            (0.0028, 0.018, 3)
        );
    }

    #[test]
    fn sustained_speech_passes() {
        let d = state_with(&[(0.003, 0.025), (0.0056, 0.06), (0.0061, 0.065)]).decision();
        assert!(!d.skip);
        assert_eq!(d.reason, GateReason::SpeechDetected);
        let m = d.metrics.unwrap();
        assert_eq!(m.speech_window_count, 3);
        assert_eq!(m.max_consecutive_speech_windows, 3);
        assert_eq!(m.peak_rms, 0.0061);
    }

    #[test]
    fn strong_rms_alone_counts_as_speech_and_runs_reset() {
        let d = state_with(&[(0.007, 0.01)]).decision();
        assert_eq!(d.reason, GateReason::SpeechDetected);
        let s = state_with(&[(0.004, 0.03), (0.004, 0.03), (0.001, 0.001), (0.004, 0.03)]);
        assert_eq!(s.max_consecutive_speech_windows, 2);
        assert_eq!(s.consecutive_speech_windows, 1);
    }

    #[test]
    fn quantization_truncates_to_bytes() {
        assert_eq!(byte_quantize(0.0), 0.0);
        // Tiny negative values truncate down to the next byte.
        assert_eq!(byte_quantize(-0.0001), -1.0 / 128.0);
        assert_eq!(byte_quantize(0.0001), 0.0);
        assert_eq!(byte_quantize(1.0), 127.0 / 128.0);
        assert_eq!(byte_quantize(-1.0), -1.0);
        assert_eq!(byte_quantize(5.0), 127.0 / 128.0);
    }

    #[test]
    fn window_metrics_zero_pads_and_uses_the_tail() {
        let digital_silence = vec![0.0f32; 4096];
        assert_eq!(
            window_metrics(&digital_silence, ANALYSER_WINDOW),
            (0.0, 0.0)
        );
        // 1024 samples at -1 then padding: rms = sqrt(1024/2048).
        let half = vec![-1.0f32; 1024];
        let (rms, peak) = window_metrics(&half, ANALYSER_WINDOW);
        assert!((rms - 0.5f64.sqrt()).abs() < 1e-12);
        assert_eq!(peak, 1.0);
        // Only the last window counts.
        let mut tail = vec![-1.0f32; 4096];
        tail.extend(std::iter::repeat_n(0.0, 2048));
        assert_eq!(window_metrics(&tail, ANALYSER_WINDOW), (0.0, 0.0));
    }

    #[test]
    fn speech_like_tone_passes_and_silence_fails() {
        let rate = 48_000.0;
        let tone: Vec<f32> = (0..48_000)
            .map(|i| 0.05 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / rate).sin())
            .collect();
        let mut state = SpeechGateState::default();
        for end in (4800..=tone.len()).step_by(4800) {
            let (rms, peak) = window_metrics(&tone[..end], ANALYSER_WINDOW);
            state.record_window(rms, peak);
        }
        assert_eq!(state.decision().reason, GateReason::SpeechDetected);

        let mut state = SpeechGateState::default();
        let silence = vec![0.0f32; 48_000];
        for end in (4800..=silence.len()).step_by(4800) {
            let (rms, peak) = window_metrics(&silence[..end], ANALYSER_WINDOW);
            state.record_window(rms, peak);
        }
        assert!(state.decision().is_silence());
    }
}
