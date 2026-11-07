//! PCM helpers: band-limited resampling and 16-bit WAV encoding.

use std::f64::consts::PI;
use std::sync::{Arc, Mutex, OnceLock};

/// Sample rate the transcription server receives.
const TARGET_SAMPLE_RATE: u32 = 16_000;
const WAV_HEADER_BYTES: usize = 44;

/// Zeros of the lowpass kernel on each side of the centre.
const KERNEL_ZERO_CROSSINGS: f64 = 24.0;
/// Kernel table resolution per input sample.
const TABLE_RESOLUTION: usize = 512;
const KAISER_BETA: f64 = 8.6;

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half = x / 2.0;
    for k in 1..64 {
        term *= (half / k as f64) * (half / k as f64);
        sum += term;
        if term < sum * 1e-16 {
            break;
        }
    }
    sum
}

/// Cached rates are bounded so uncommon device formats cannot grow resident
/// filter storage without limit. Common 48/44.1 kHz conversions use one/160
/// phases, with the same interpolated Kaiser kernel as the scalar path.
const FILTER_CACHE_SIZE: usize = 4;
const MAX_PHASE_COEFFICIENTS: usize = 100_000;
const MAX_CACHED_FILTER_BYTES: usize = 1024 * 1024;

struct Phase {
    weights: Vec<f64>,
    total: f64,
}

struct Filter {
    reach: i64,
    table: Vec<f64>,
    phases: Vec<Phase>,
}

impl Filter {
    fn new(from_rate: u32, to_rate: u32) -> Self {
        let ratio = f64::from(to_rate) / f64::from(from_rate);
        // Leave the original transition band below the target Nyquist.
        let cutoff = 0.5 * ratio.min(1.0) * 0.94;
        let half_width = KERNEL_ZERO_CROSSINGS / (2.0 * cutoff);
        let table_len = (half_width * TABLE_RESOLUTION as f64).ceil() as usize + 2;
        let i0_beta = bessel_i0(KAISER_BETA);
        let table = (0..table_len)
            .map(|i| {
                let t = i as f64 / TABLE_RESOLUTION as f64;
                if t > half_width {
                    return 0.0;
                }
                let x = 2.0 * cutoff * t;
                let sinc = if x.abs() < 1e-12 {
                    1.0
                } else {
                    (PI * x).sin() / (PI * x)
                };
                let r = t / half_width;
                let window = bessel_i0(KAISER_BETA * (1.0 - r * r).max(0.0).sqrt()) / i0_beta;
                2.0 * cutoff * sinc * window
            })
            .collect();
        let mut filter = Self {
            reach: half_width.ceil() as i64,
            table,
            phases: Vec::new(),
        };
        let mut a = from_rate;
        let mut b = to_rate;
        while b != 0 {
            (a, b) = (b, a % b);
        }
        let phase_count = (to_rate / a) as usize;
        let taps = (2 * filter.reach + 2) as usize;
        if phase_count.saturating_mul(taps) <= MAX_PHASE_COEFFICIENTS {
            filter.phases = (0..phase_count)
                .map(|phase| {
                    let remainder = (phase as u64 * u64::from(from_rate)) % u64::from(to_rate);
                    let fraction = remainder as f64 / f64::from(to_rate);
                    let weights: Vec<f64> = (-filter.reach..=filter.reach + 1)
                        .map(|offset| filter.weight(fraction - offset as f64))
                        .collect();
                    let total = weights.iter().sum();
                    Phase { weights, total }
                })
                .collect();
            // The precomputed phases contain everything the hot loop needs.
            filter.table = Vec::new();
        }
        filter
    }

    fn weight(&self, t: f64) -> f64 {
        let pos = t.abs() * TABLE_RESOLUTION as f64;
        let i = pos as usize;
        if i + 1 >= self.table.len() {
            return 0.0;
        }
        let fraction = pos - i as f64;
        self.table[i] * (1.0 - fraction) + self.table[i + 1] * fraction
    }

    fn sample(&self, input: &[f32], n: usize, from_rate: u32, to_rate: u32) -> f32 {
        let (acc, total) = if self.phases.is_empty() {
            let center = n as f64 / (f64::from(to_rate) / f64::from(from_rate));
            let base = center.floor() as i64;
            let (mut acc, mut total) = (0.0, 0.0);
            for k in base - self.reach..=base + self.reach + 1 {
                let w = self.weight(center - k as f64);
                total += w;
                if k >= 0 && (k as usize) < input.len() {
                    acc += w * f64::from(input[k as usize]);
                }
            }
            (acc, total)
        } else {
            let position = n as u64 * u64::from(from_rate);
            let base = (position / u64::from(to_rate)) as i64;
            let phase = &self.phases[n % self.phases.len()];
            let start = base - self.reach;
            // Clip only the input span. The denominator always includes the
            // full filter, preserving the original zero-padded edge behavior.
            let first = start.max(0) as usize;
            let end = (start + phase.weights.len() as i64).clamp(0, input.len() as i64) as usize;
            let skip = (first as i64 - start) as usize;
            let acc = dot_product(&input[first..end], &phase.weights[skip..skip + end - first]);
            (acc, phase.total)
        };
        if total.abs() > 1e-12 {
            (acc / total) as f32
        } else {
            0.0
        }
    }
}

/// Independent accumulators avoid one long floating-point dependency chain.
/// Accumulation remains f64 before the original f32/PCM16 quantization.
fn dot_product(samples: &[f32], weights: &[f64]) -> f64 {
    let mut sums = [0.0; 4];
    let (sample_chunks, sample_tail) = samples.as_chunks::<4>();
    let (weight_chunks, weight_tail) = weights.as_chunks::<4>();
    for (samples, weights) in sample_chunks.iter().zip(weight_chunks) {
        for i in 0..4 {
            sums[i] += f64::from(samples[i]) * weights[i];
        }
    }
    let mut sum = sums.into_iter().sum::<f64>();
    for (&sample, &weight) in sample_tail.iter().zip(weight_tail) {
        sum += f64::from(sample) * weight;
    }
    sum
}

type FilterCache = Vec<((u32, u32), Arc<Filter>)>;

fn cached_filter(from_rate: u32, to_rate: u32) -> Arc<Filter> {
    static CACHE: OnceLock<Mutex<FilterCache>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let key = (from_rate, to_rate);
    if let Some(index) = cache.iter().position(|(rates, _)| *rates == key) {
        let entry = cache.remove(index);
        let filter = Arc::clone(&entry.1);
        cache.push(entry);
        return filter;
    }
    let filter = Arc::new(Filter::new(from_rate, to_rate));
    let coefficient_bytes = (filter.table.len()
        + filter
            .phases
            .iter()
            .map(|phase| phase.weights.len())
            .sum::<usize>())
        * std::mem::size_of::<f64>();
    let phase_bytes = filter.phases.len() * std::mem::size_of::<Phase>();
    if coefficient_bytes + phase_bytes > MAX_CACHED_FILTER_BYTES {
        return filter;
    }
    if cache.len() == FILTER_CACHE_SIZE {
        cache.remove(0);
    }
    cache.push((key, Arc::clone(&filter)));
    filter
}

fn resampled_len(input_len: usize, from_rate: u32, to_rate: u32) -> usize {
    (input_len as f64 * (f64::from(to_rate) / f64::from(from_rate))).round() as usize
}

/// Windowed-sinc (Kaiser) resampler. Downsampling low-passes below the
/// target Nyquist so nothing aliases into the speech band.
#[cfg(test)]
pub fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() || from_rate == 0 || to_rate == 0 {
        return input.to_vec();
    }
    let filter = cached_filter(from_rate, to_rate);
    (0..resampled_len(input.len(), from_rate, to_rate))
        .map(|n| filter.sample(input, n, from_rate, to_rate))
        .collect()
}

/// Encodes mono samples as 16-bit PCM WAV at `sample_rate`.
pub fn encode_wav_pcm16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let mut out = wav_header(samples.len(), sample_rate);
    for &sample in samples {
        append_pcm16(&mut out, sample);
    }
    out
}

fn wav_header(sample_count: usize, sample_rate: u32) -> Vec<u8> {
    let data_len = (sample_count * 2) as u32;
    let mut out = Vec::with_capacity(WAV_HEADER_BYTES + sample_count * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out
}

fn append_pcm16(out: &mut Vec<u8>, sample: f32) {
    let value = (f64::from(sample).clamp(-1.0, 1.0) * 32767.0).round() as i16;
    out.extend_from_slice(&value.to_le_bytes());
}

/// Resamples mono audio to 16 kHz and encodes it as 16-bit PCM WAV.
pub fn encode_wav_16k(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    if sample_rate == TARGET_SAMPLE_RATE || samples.is_empty() || sample_rate == 0 {
        return encode_wav_pcm16(samples, TARGET_SAMPLE_RATE);
    }
    let filter = cached_filter(sample_rate, TARGET_SAMPLE_RATE);
    let len = resampled_len(samples.len(), sample_rate, TARGET_SAMPLE_RATE);
    let mut out = wav_header(len, TARGET_SAMPLE_RATE);
    for n in 0..len {
        append_pcm16(
            &mut out,
            filter.sample(samples, n, sample_rate, TARGET_SAMPLE_RATE),
        );
    }
    out
}

/// Reads a 16-bit PCM WAV with `hound`, downmixed to mono floats.
#[cfg(test)]
pub(crate) fn read_wav_mono(bytes: &[u8]) -> (hound::WavSpec, Vec<f32>) {
    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes)).expect("valid WAV");
    let spec = reader.spec();
    assert_eq!(spec.bits_per_sample, 16, "expected PCM16");
    let interleaved: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| f32::from(s.unwrap()) / 32768.0)
        .collect();
    let channels = usize::from(spec.channels.max(1));
    let mono = interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    (spec, mono)
}

/// True for the 16 kHz mono PCM16 shape the server receives from dictation.
#[cfg(test)]
pub(crate) fn is_pcm16_mono_16k_wav(bytes: &[u8]) -> bool {
    hound::WavReader::new(std::io::Cursor::new(bytes)).is_ok_and(|r| {
        let spec = r.spec();
        spec.sample_format == hound::SampleFormat::Int
            && spec.channels == 1
            && spec.sample_rate == 16_000
            && spec.bits_per_sample == 16
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, seconds: f64, amp: f32) -> Vec<f32> {
        let n = (f64::from(rate) * seconds) as usize;
        (0..n)
            .map(|i| amp * (2.0 * PI * freq * i as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
        (sum / samples.len() as f64).sqrt() as f32
    }

    /// Amplitude of `freq` via correlation (Goertzel-like) over the middle.
    fn tone_amplitude(samples: &[f32], rate: u32, freq: f64) -> f64 {
        let mid = &samples[samples.len() / 4..samples.len() * 3 / 4];
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &s) in mid.iter().enumerate() {
            let phase = 2.0 * PI * freq * i as f64 / f64::from(rate);
            re += f64::from(s) * phase.cos();
            im += f64::from(s) * phase.sin();
        }
        2.0 * (re * re + im * im).sqrt() / mid.len() as f64
    }

    #[test]
    fn resample_preserves_in_band_tones() {
        for from in [48_000, 44_100, 24_000, 8_000] {
            let input = sine(1000.0, from, 0.5, 0.5);
            let out = resample(&input, from, 16_000);
            let expected = (input.len() as f64 * 16_000.0 / f64::from(from)).round() as usize;
            assert_eq!(out.len(), expected, "{from}");
            let amp = tone_amplitude(&out, 16_000, 1000.0);
            assert!((amp - 0.5).abs() < 0.01, "{from}: amplitude {amp}");
        }
    }

    #[test]
    fn resample_rejects_aliasing() {
        // 12 kHz would alias to 4 kHz at 16 kHz without a lowpass.
        let input = sine(12_000.0, 48_000, 0.5, 0.8);
        let out = resample(&input, 48_000, 16_000);
        assert!(tone_amplitude(&out, 16_000, 4000.0) < 0.002);
        assert!(rms(&out[1000..out.len() - 1000]) < 0.002);
        // Just above the target Nyquist is also removed.
        let out = resample(&sine(8_600.0, 44_100, 0.5, 0.8), 44_100, 16_000);
        assert!(rms(&out[1000..out.len() - 1000]) < 0.01);
    }

    #[test]
    fn dc_gain_is_unity() {
        let out = resample(&vec![0.25; 9600], 48_000, 16_000);
        for &s in &out[200..out.len() - 200] {
            assert!((s - 0.25).abs() < 1e-4);
        }
    }

    #[test]
    fn wav_round_trip() {
        let samples = vec![0.0, 0.5, -0.5, 1.0, -1.0, 2.0];
        let bytes = encode_wav_pcm16(&samples, 16_000);
        assert_eq!(bytes.len(), WAV_HEADER_BYTES + samples.len() * 2);
        assert!(is_pcm16_mono_16k_wav(&bytes));
        let (spec, decoded) = read_wav_mono(&bytes);
        assert_eq!(spec.sample_rate, 16_000);
        let expected = [0.0, 0.5, -0.5, 1.0, -1.0, 1.0];
        assert_eq!(decoded.len(), expected.len());
        for (a, b) in decoded.iter().zip(expected) {
            assert!((a - b).abs() < 1e-3);
        }
        assert!(!is_pcm16_mono_16k_wav(b"OggS...."));
    }

    #[test]
    fn encode_16k_from_48k() {
        let bytes = encode_wav_16k(&sine(440.0, 48_000, 1.0, 0.3), 48_000);
        assert!(is_pcm16_mono_16k_wav(&bytes));
        assert_eq!(bytes.len(), WAV_HEADER_BYTES + 16_000 * 2);
    }

    #[test]
    fn short_recordings_keep_scalar_filter_edge_behavior() {
        // Reference values from the original scalar Kaiser implementation.
        let input = [0.7, -0.2, 0.6, 0.1, -0.4];
        for (rate, expected) in [
            (44_100, [0.2907555, 0.09786659]),
            (48_000, [0.27860528, 0.06889108]),
        ] {
            let actual = resample(&input, rate, 16_000);
            assert_eq!(actual.len(), expected.len());
            for (&actual, expected) in actual.iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-7, "{rate}: {actual}");
            }
        }
        assert!(resample(&[], 48_000, 16_000).is_empty());
        assert_eq!(resample(&input, 16_000, 16_000), input);
    }

    #[test]
    fn direct_encoding_preserves_quantization_and_output_length() {
        for rate in [8_000, 16_000, 44_100, 44_101, 48_000, 96_000] {
            let input = sine(3210.0, rate, 0.025, 0.8);
            let expected = encode_wav_pcm16(&resample(&input, rate, 16_000), 16_000);
            assert_eq!(encode_wav_16k(&input, rate), expected, "{rate}");
        }
    }
}
