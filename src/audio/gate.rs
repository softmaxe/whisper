//! Local speech gate.
//!
//! Every 100 ms of recording the capture thread measures RMS over the most
//! recent 2048 samples, quantized to bytes and normalized back to −1..1. The
//! dictation pipeline skips transcription when every measured window stayed
//! under [`SILENCE_RMS_THRESHOLD`].

pub const SILENCE_RMS_THRESHOLD: f64 = 0.002;

/// Samples per gate window.
pub const ANALYSER_WINDOW: usize = 2048;
/// Interval between gate windows.
pub const GATE_INTERVAL_MS: u32 = 100;

/// The loudest window of a recording. No windows (e.g. pre-roll adopted)
/// fails open.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpeechGate {
    pub peak_rms: f64,
    pub window_count: u32,
}

impl SpeechGate {
    pub fn record_window(&mut self, rms: f64) {
        self.window_count += 1;
        self.peak_rms = self.peak_rms.max(rms);
    }

    /// Whether the recording is silence and should not be transcribed.
    pub fn is_silence(&self) -> bool {
        self.window_count > 0 && self.peak_rms < SILENCE_RMS_THRESHOLD
    }
}

/// A float sample quantized to a byte time-domain value, back in −1..1:
/// `byte = trunc(clamp(128 * (x + 1), 0, 255))`, `v = (byte - 128) / 128`.
#[inline]
pub fn byte_quantize(sample: f32) -> f64 {
    let scaled = (128.0 * (f64::from(sample) + 1.0)).clamp(0.0, 255.0);
    (scaled.trunc() - 128.0) / 128.0
}

/// RMS over the last `window` samples after byte quantization. Fewer samples
/// than the window are zero-padded.
pub fn window_rms(samples: &[f32], window: usize) -> f64 {
    if window == 0 {
        return 0.0;
    }
    let start = samples.len().saturating_sub(window);
    let mut sum = 0.0;
    for &s in &samples[start..] {
        let v = byte_quantize(s);
        sum += v * v;
    }
    (sum / window as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate_with(windows: &[f64]) -> SpeechGate {
        let mut gate = SpeechGate::default();
        for &rms in windows {
            gate.record_window(rms);
        }
        gate
    }

    #[test]
    fn fails_open_without_windows() {
        assert!(!SpeechGate::default().is_silence());
    }

    #[test]
    fn near_silence_is_skippable() {
        let gate = gate_with(&[0.0012, 0.0016, 0.0014]);
        assert!(gate.is_silence());
        assert_eq!(
            gate,
            SpeechGate {
                peak_rms: 0.0016,
                window_count: 3,
            }
        );
    }

    #[test]
    fn one_window_at_the_threshold_is_not_silence() {
        assert!(!gate_with(&[0.0012, SILENCE_RMS_THRESHOLD, 0.0014]).is_silence());
        assert!(!gate_with(&[0.0025, 0.0028, 0.0022]).is_silence());
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
    fn window_rms_zero_pads_and_uses_the_tail() {
        assert_eq!(window_rms(&[], ANALYSER_WINDOW), 0.0);
        let digital_silence = vec![0.0f32; 4096];
        assert_eq!(window_rms(&digital_silence, ANALYSER_WINDOW), 0.0);
        // 1024 samples at -1 then padding: rms = sqrt(1024/2048).
        let half = vec![-1.0f32; 1024];
        let rms = window_rms(&half, ANALYSER_WINDOW);
        assert!((rms - 0.5f64.sqrt()).abs() < 1e-12);
        // Only the last window counts.
        let mut tail = vec![-1.0f32; 4096];
        tail.extend(std::iter::repeat_n(0.0, 2048));
        assert_eq!(window_rms(&tail, ANALYSER_WINDOW), 0.0);
    }

    #[test]
    fn speech_like_tone_passes_and_silence_fails() {
        let rate = 48_000.0;
        let tone: Vec<f32> = (0..48_000)
            .map(|i| 0.05 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / rate).sin())
            .collect();
        let mut gate = SpeechGate::default();
        for end in (4800..=tone.len()).step_by(4800) {
            gate.record_window(window_rms(&tone[..end], ANALYSER_WINDOW));
        }
        assert!(!gate.is_silence());

        let mut gate = SpeechGate::default();
        let silence = vec![0.0f32; 48_000];
        for end in (4800..=silence.len()).step_by(4800) {
            gate.record_window(window_rms(&silence[..end], ANALYSER_WINDOW));
        }
        assert!(gate.is_silence());
    }
}
