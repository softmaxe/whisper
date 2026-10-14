//! PCM helpers: downmix, band-limited resampling, 16-bit WAV encode/decode
//! and segment merging.

use std::f64::consts::PI;

/// Sample rate the transcription server receives.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
pub const WAV_HEADER_BYTES: usize = 44;
pub const WAV_MIME_TYPE: &str = "audio/wav";

/// Averages interleaved channels into mono.
pub fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

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

/// Windowed-sinc (Kaiser) resampler. Downsampling low-passes below the
/// target Nyquist so nothing aliases into the speech band.
pub fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() || from_rate == 0 || to_rate == 0 {
        return input.to_vec();
    }
    let ratio = f64::from(to_rate) / f64::from(from_rate);
    // Cutoff in cycles per input sample; leave a transition band below Nyquist.
    let cutoff = 0.5 * ratio.min(1.0) * 0.94;
    let half_width = KERNEL_ZERO_CROSSINGS / (2.0 * cutoff);
    let table_len = (half_width * TABLE_RESOLUTION as f64).ceil() as usize + 2;
    let i0_beta = bessel_i0(KAISER_BETA);
    let table: Vec<f64> = (0..table_len)
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
    let kernel = |t: f64| -> f64 {
        let pos = t.abs() * TABLE_RESOLUTION as f64;
        let i = pos as usize;
        if i + 1 >= table.len() {
            return 0.0;
        }
        let frac = pos - i as f64;
        table[i] * (1.0 - frac) + table[i + 1] * frac
    };

    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let reach = half_width.ceil() as i64;
    let last = input.len() as i64 - 1;
    let mut out = Vec::with_capacity(out_len);
    for n in 0..out_len {
        let center = n as f64 / ratio;
        let base = center.floor() as i64;
        let mut acc = 0.0;
        let mut weight = 0.0;
        for k in (base - reach)..=(base + reach + 1) {
            let w = kernel(center - k as f64);
            if w == 0.0 {
                continue;
            }
            weight += w;
            if (0..=last).contains(&k) {
                acc += w * f64::from(input[k as usize]);
            }
        }
        // Normalizing by the full kernel sum keeps DC gain at exactly 1 while
        // edges fade naturally against implicit zero padding.
        out.push(if weight.abs() > 1e-12 {
            (acc / weight) as f32
        } else {
            0.0
        });
    }
    out
}

/// Encodes mono samples as 16-bit PCM WAV at `sample_rate`.
pub fn encode_wav_pcm16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(WAV_HEADER_BYTES + samples.len() * 2);
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
    for &s in samples {
        let v = (f64::from(s).clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Resamples mono audio to 16 kHz and encodes it as 16-bit PCM WAV.
pub fn encode_wav_16k(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    encode_wav_pcm16(
        &resample(samples, sample_rate, TARGET_SAMPLE_RATE),
        TARGET_SAMPLE_RATE,
    )
}

/// Concatenates mono segments (each with its own rate) at `target_rate`.
pub fn merge_segments(segments: &[(&[f32], u32)], target_rate: u32) -> Vec<f32> {
    let mut merged = Vec::new();
    for (samples, rate) in segments {
        merged.extend(resample(samples, *rate, target_rate));
    }
    merged
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavFormat {
    pub audio_format: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
}

pub fn is_wav_format(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Walks RIFF chunks; returns `(fmt, data range)` when present.
fn walk_chunks(bytes: &[u8]) -> (Option<WavFormat>, Option<std::ops::Range<usize>>) {
    let mut format = None;
    let mut data = None;
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let Some(size) = u32_at(bytes, offset + 4).map(|s| s as usize) else {
            break;
        };
        let body = offset + 8;
        if id == b"fmt " {
            format = (|| {
                Some(WavFormat {
                    audio_format: u16_at(bytes, body)?,
                    channels: u16_at(bytes, body + 2)?,
                    sample_rate: u32_at(bytes, body + 4)?,
                    bits_per_sample: u16_at(bytes, body + 14)?,
                })
            })();
        } else if id == b"data" {
            data = Some(body..(body + size).min(bytes.len()));
            break;
        }
        offset = body + size + (size & 1);
    }
    (format, data)
}

pub fn parse_wav_format(bytes: &[u8]) -> Option<WavFormat> {
    if !is_wav_format(bytes) {
        return None;
    }
    walk_chunks(bytes).0
}

/// True for the 16 kHz mono PCM16 shape the server receives from dictation.
pub fn is_pcm16_mono_16k_wav(bytes: &[u8]) -> bool {
    parse_wav_format(bytes).is_some_and(|f| {
        f.audio_format == 1 && f.channels == 1 && f.sample_rate == 16_000 && f.bits_per_sample == 16
    })
}

/// Decoded PCM (interleaved when multi-channel).
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedWav {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Decodes 8- or 16-bit PCM WAV to floats.
pub fn decode_wav(bytes: &[u8]) -> Result<DecodedWav, String> {
    if !is_wav_format(bytes) {
        return Err("Buffer is not a valid WAV file".into());
    }
    let (format, data) = walk_chunks(bytes);
    let format = format.ok_or("WAV fmt chunk not found")?;
    let data = data.ok_or("WAV data chunk not found")?;
    let pcm = &bytes[data];
    let samples = match format.bits_per_sample {
        16 => pcm
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| f32::from(i16::from_le_bytes(*c)) / 32768.0)
            .collect(),
        8 => pcm
            .iter()
            .map(|&b| (f32::from(b) - 128.0) / 128.0)
            .collect(),
        bits => return Err(format!("Unsupported WAV bit depth: {bits}")),
    };
    Ok(DecodedWav {
        samples,
        sample_rate: format.sample_rate,
        channels: format.channels,
    })
}

/// Plain RMS of float samples.
pub fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / samples.len() as f64).sqrt() as f32
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
    fn downmix_averages_channels() {
        assert_eq!(downmix_to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix_to_mono(&[0.25], 1), vec![0.25]);
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
        assert!(compute_rms(&out[1000..out.len() - 1000]) < 0.002);
        // Just above the target Nyquist is also removed.
        let out = resample(&sine(8_600.0, 44_100, 0.5, 0.8), 44_100, 16_000);
        assert!(compute_rms(&out[1000..out.len() - 1000]) < 0.01);
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
        assert!(is_wav_format(&bytes));
        assert!(is_pcm16_mono_16k_wav(&bytes));
        assert_eq!(
            parse_wav_format(&bytes),
            Some(WavFormat {
                audio_format: 1,
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16
            })
        );
        let decoded = decode_wav(&bytes).unwrap();
        assert_eq!(decoded.sample_rate, 16_000);
        let expected = [0.0, 0.5, -0.5, 1.0, -1.0, 1.0];
        for (a, b) in decoded.samples.iter().zip(expected) {
            assert!((a - b).abs() < 1e-3);
        }
        // hound agrees with the header.
        let reader = hound::WavReader::new(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.len() as usize, samples.len());
        assert!(!is_wav_format(b"OggS...."));
        assert!(decode_wav(b"nope").is_err());
    }

    #[test]
    fn encode_16k_from_48k() {
        let bytes = encode_wav_16k(&sine(440.0, 48_000, 1.0, 0.3), 48_000);
        assert!(is_pcm16_mono_16k_wav(&bytes));
        assert_eq!(bytes.len(), WAV_HEADER_BYTES + 16_000 * 2);
    }

    // Two 0.25 s segments merge into one decodable ~0.5 s timeline.
    #[test]
    fn merges_segments_into_one_timeline() {
        let a = sine(440.0, 48_000, 0.25, 0.3);
        let b = sine(660.0, 44_100, 0.25, 0.3);
        let merged = merge_segments(&[(&a, 48_000), (&b, 44_100)], 16_000);
        let decoded = decode_wav(&encode_wav_pcm16(&merged, 16_000)).unwrap();
        let seconds = decoded.samples.len() as f64 / 16_000.0;
        assert!(seconds > 0.4 && seconds < 0.7, "duration was {seconds}");
    }
}
