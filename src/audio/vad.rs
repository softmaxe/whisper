//! Local Silero speech probabilities and conservative frame retention.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::ensure;
use tract_onnx::prelude::*;

pub const RATE: u32 = 16_000;
pub const FRAME: usize = 512;
pub const NO_SPEECH_CODE: &str = "LOCAL_NO_SPEECH";
const BUFFER: usize = 15;
const THRESHOLD: f32 = 0.3;
const MODEL: &[u8] = include_bytes!("models/silero_vad_openvino_16k.onnx");

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub speech_duration: Duration,
    pub removed_duration: Duration,
    pub found_speech: bool,
}

#[derive(Debug)]
pub struct FilteredAudio {
    pub samples: Vec<f32>,
    /// No summary means inference failed open; the samples are the original.
    pub summary: Option<Summary>,
}

impl FilteredAudio {
    pub fn no_speech(&self) -> bool {
        self.summary.as_ref().is_some_and(|s| !s.found_speech)
    }
}

fn model() -> Result<Arc<TypedRunnableModel>, String> {
    static MODEL_PLAN: OnceLock<Result<Arc<TypedRunnableModel>, String>> = OnceLock::new();
    MODEL_PLAN
        .get_or_init(|| {
            (|| -> TractResult<Arc<TypedRunnableModel>> {
                let mut model = tract_onnx::onnx()
                    .model_for_read(&mut std::io::Cursor::new(MODEL))?
                    .with_input_fact(0, f32::fact([1, 576]).into())?
                    .with_input_fact(1, f32::fact([2, 1, 128]).into())?;
                // The official export contains anonymous constant names that
                // collide in tract's debug graph validation. Names do not
                // participate in inference; preserve all edges and weights.
                for node in model.nodes_mut() {
                    node.name = format!("silero_{}", node.id);
                }
                model.into_optimized()?.into_runnable()
            })()
            .map_err(|error| format!("Silero model: {error:#}"))
        })
        .clone()
}

struct Silero {
    session: TypedSimpleState,
    recurrent: Tensor,
    context: [f32; 64],
}

impl Silero {
    fn new() -> Result<Self, String> {
        let plan = model()?;
        Ok(Self {
            session: plan.spawn().map_err(|e| e.to_string())?,
            recurrent: Tensor::zero::<f32>(&[2, 1, 128]).map_err(|e| e.to_string())?,
            context: [0.; 64],
        })
    }

    fn probability(&mut self, frame: &[f32; FRAME]) -> Result<f32, String> {
        let result = (|| -> TractResult<f32> {
            let mut input = [0.; 576];
            input[..64].copy_from_slice(&self.context);
            input[64..].copy_from_slice(frame);
            self.context.copy_from_slice(&input[512..]);
            let outputs = self.session.run(tvec!(
                Tensor::from_shape(&[1, 576], &input)?.into(),
                self.recurrent.clone().into(),
            ))?;
            ensure!(outputs.len() == 2, "unexpected Silero output count");
            ensure!(
                outputs[1].shape() == [2, 1, 128],
                "unexpected Silero state shape"
            );
            self.recurrent = outputs[1].clone().into_tensor();
            outputs[0].cast_to_scalar::<f32>()
        })();
        result.map_err(|e| format!("Silero inference: {e:#}"))
    }
}

/// Runs off the UI thread. A fresh recurrent state belongs to each recording;
/// only the immutable execution plan is shared between retries and dictations.
pub fn filter(samples: &[f32]) -> FilteredAudio {
    fail_open(samples, || {
        let mut detector = Silero::new()?;
        filter_with_detector(samples, |frame| detector.probability(frame))
    })
}

/// Creates only the upload copy. The caller keeps the original recording.
pub fn upload_samples(samples: &[f32], rate: u32) -> Option<Vec<u8>> {
    let mono = super::wav::resample(samples, rate, RATE);
    let filtered = filter(&mono);
    if filtered.summary.is_none() {
        return Some(super::wav::encode_wav_16k(samples, rate));
    }
    (!filtered.no_speech()).then(|| super::wav::encode_wav_pcm16(&filtered.samples, RATE))
}

/// History and recovery contain the untrimmed PCM WAV, so they reapply the
/// same detector with a fresh state when sent again.
pub fn upload_wav(original: bytes::Bytes) -> Option<bytes::Bytes> {
    match decode_wav(&original) {
        Ok((samples, rate)) => {
            let mono = super::wav::resample(&samples, rate, RATE);
            let filtered = filter(&mono);
            if filtered.summary.is_none() {
                return Some(original);
            }
            (!filtered.no_speech())
                .then(|| super::wav::encode_wav_pcm16(&filtered.samples, RATE).into())
        }
        Err(error) => {
            warn_once(&error);
            Some(original)
        }
    }
}

fn decode_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32), String> {
    let mut reader =
        hound::WavReader::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.channels == 0
        || spec.sample_rate == 0
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("unsupported stored WAV format".into());
    }
    let samples = reader
        .samples::<i16>()
        .map(|sample| sample.map(|sample| f32::from(sample) / 32768.))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let channels = usize::from(spec.channels);
    if samples.len() % channels != 0 {
        return Err("incomplete stored WAV frame".into());
    }
    Ok((
        samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect(),
        spec.sample_rate,
    ))
}

fn warn_once(error: &str) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        log::warn!("silence filtering failed open; sending original audio: {error}");
    }
}

fn fail_open(
    samples: &[f32],
    work: impl FnOnce() -> Result<FilteredAudio, String>,
) -> FilteredAudio {
    // A runtime panic is also an inference failure, never a lost recording.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(Ok(audio)) => audio,
        failure => {
            let error = match failure {
                Ok(Err(error)) => error,
                _ => "inference panicked".into(),
            };
            warn_once(&error);
            FilteredAudio {
                samples: samples.to_vec(),
                summary: None,
            }
        }
    }
}

fn duration(samples: usize) -> Duration {
    Duration::from_secs_f64(samples as f64 / f64::from(RATE))
}

/// The production frame algorithm, with an injectable detector for exact
/// retention and failure tests. Partial final frames are padded for inference
/// but only their original samples can be retained.
fn filter_with_detector(
    samples: &[f32],
    mut probability: impl FnMut(&[f32; FRAME]) -> Result<f32, String>,
) -> Result<FilteredAudio, String> {
    if samples.len() < FRAME {
        return Err("recording is shorter than a VAD frame".into());
    }
    if samples.iter().any(|s| !s.is_finite()) {
        return Err("non-finite audio sample".into());
    }
    let mut speech = Vec::with_capacity(samples.len().div_ceil(FRAME));
    for chunk in samples.chunks(FRAME) {
        let mut frame = [0.; FRAME];
        frame[..chunk.len()].copy_from_slice(chunk);
        let p = probability(&frame)?;
        if !p.is_finite() || !(0.0..=1.0).contains(&p) {
            return Err("invalid speech probability".into());
        }
        speech.push(p > THRESHOLD);
    }
    let mut keep = vec![false; speech.len()];
    let mut consecutive = 0;
    let mut tail_end: Option<usize> = None;
    for (index, &spoken) in speech.iter().enumerate() {
        if tail_end.is_some_and(|end| index > end) {
            tail_end = None;
        }
        if spoken {
            consecutive += 1;
            if tail_end.is_some() || consecutive >= 2 {
                if tail_end.is_none() {
                    keep[index.saturating_sub(1 + BUFFER)..=index].fill(true);
                }
                tail_end = Some(index + BUFFER);
            }
        } else {
            consecutive = 0;
        }
        if tail_end.is_some() {
            keep[index] = true;
        }
    }
    if consecutive == 1 && tail_end.is_none() {
        let last = speech.len() - 1;
        keep[last.saturating_sub(BUFFER)..=last].fill(true);
    }
    let mut output = Vec::with_capacity(samples.len());
    let mut speech_samples = 0;
    for (index, chunk) in samples.chunks(FRAME).enumerate() {
        if keep[index] {
            output.extend_from_slice(chunk);
            if speech[index] {
                speech_samples += chunk.len();
            }
        }
    }
    let found_speech = speech_samples > 0;
    if found_speech && output.is_empty() || output.len() > samples.len() {
        return Err("implausible retained frame result".into());
    }
    let removed_duration = duration(samples.len() - output.len());
    Ok(FilteredAudio {
        samples: output,
        summary: Some(Summary {
            speech_duration: duration(speech_samples),
            removed_duration,
            found_speech,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripted(probabilities: &[f32]) -> Vec<usize> {
        let samples: Vec<f32> = (0..probabilities.len())
            .flat_map(|i| std::iter::repeat_n(i as f32, FRAME))
            .collect();
        let mut probabilities = probabilities.iter();
        filter_with_detector(&samples, |_| Ok(*probabilities.next().unwrap()))
            .unwrap()
            .samples
            .chunks(FRAME)
            .map(|frame| frame[0] as usize)
            .collect()
    }

    #[test]
    fn onset_keeps_fifteen_preceding_frames_and_fifteen_tail_frames() {
        let mut p = vec![0.; 60];
        p[20..22].fill(0.9);
        assert_eq!(scripted(&p), (5..37).collect::<Vec<_>>());
    }

    #[test]
    fn speech_inside_the_tail_refills_it_without_duplication() {
        let mut p = vec![0.; 70];
        p[20..22].fill(0.9);
        p[30] = 0.9;
        assert_eq!(scripted(&p), (5..46).collect::<Vec<_>>());
        p[50..52].fill(0.9);
        assert_eq!(scripted(&p), (5..67).collect::<Vec<_>>());
    }

    #[test]
    fn final_unconfirmed_burst_all_silence_and_all_speech() {
        let mut p = vec![0.; 40];
        p[39] = 0.9;
        assert_eq!(scripted(&p), (24..40).collect::<Vec<_>>());
        assert!(scripted(&[0.; 40]).is_empty());
        assert!(scripted(&[THRESHOLD; 40]).is_empty());
        assert_eq!(scripted(&[0.9; 40]), (0..40).collect::<Vec<_>>());
        let audio = filter_with_detector(&vec![0.1; FRAME + 13], |_| Ok(0.9)).unwrap();
        assert_eq!(audio.samples.len(), FRAME + 13);
    }

    #[test]
    fn detector_errors_invalid_probabilities_and_short_audio_fail_open() {
        let original = vec![0.25; FRAME * 3];
        for probability in [f32::NAN, -0.1, 1.1] {
            let audio = fail_open(&original, || {
                filter_with_detector(&original, |_| Ok(probability))
            });
            assert_eq!(audio.samples, original);
            assert!(audio.summary.is_none());
        }
        let audio = fail_open(&original, || {
            filter_with_detector(&original, |_| Err("injected failure".into()))
        });
        assert_eq!(audio.samples, original);
        assert!(!audio.no_speech());
        let short = vec![0.25; FRAME - 1];
        assert_eq!(filter(&short).samples, short);
        let short_wav: bytes::Bytes = super::super::wav::encode_wav_pcm16(&short, RATE).into();
        assert_eq!(upload_wav(short_wav.clone()), Some(short_wav));
        let invalid = bytes::Bytes::from_static(b"invalid WAV");
        assert_eq!(upload_wav(invalid.clone()), Some(invalid));
    }

    fn speech_fixture() -> Vec<f32> {
        let mut wav = hound::WavReader::new(std::io::Cursor::new(include_bytes!(
            "../../tests/fixtures/vad-speech.wav"
        )))
        .unwrap();
        wav.samples::<i16>()
            .map(|s| f32::from(s.unwrap()) / 32768.)
            .collect()
    }

    #[test]
    fn tract_probabilities_match_the_independent_onnx_runtime_reference() {
        let reference: Vec<f32> =
            serde_json::from_str(include_str!("../../tests/fixtures/vad-probabilities.json"))
                .unwrap();
        let speech = speech_fixture();
        let mut detector = Silero::new().unwrap();
        for (index, chunk) in speech.chunks(FRAME).enumerate() {
            let mut frame = [0.; FRAME];
            frame[..chunk.len()].copy_from_slice(chunk);
            let probability = detector.probability(&frame).unwrap();
            assert!(
                (probability - reference[index]).abs() < 0.00002,
                "frame {index}: {probability} vs {}",
                reference[index]
            );
        }
    }

    fn noise(length: usize) -> Vec<f32> {
        let mut random = 123456789u32;
        (0..length)
            .map(|_| {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                (random as f32 / u32::MAX as f32 - 0.5) * 0.1
            })
            .collect()
    }

    #[test]
    fn embedded_silero_keeps_real_speech_surrounded_by_silence_or_noise() {
        let speech = speech_fixture();
        for surrounding in [vec![0.; RATE as usize], noise(RATE as usize)] {
            let mut samples = surrounding.clone();
            samples.extend_from_slice(&speech);
            samples.extend_from_slice(&surrounding);
            let audio = filter(&samples);
            let summary = audio.summary.unwrap();
            assert!(summary.found_speech);
            assert!(summary.speech_duration > Duration::from_secs(1));
            assert!(summary.removed_duration > Duration::from_millis(500));
            assert!(audio.samples.len() >= speech.len());
        }
        for samples in [vec![0.; RATE as usize * 3], noise(RATE as usize * 3)] {
            let audio = filter(&samples);
            assert!(audio.no_speech());
            assert!(audio.samples.is_empty());
        }
    }

    #[test]
    fn sixty_second_recording_timing() {
        let speech = speech_fixture();
        let samples: Vec<f32> = speech
            .iter()
            .copied()
            .cycle()
            .take(RATE as usize * 60)
            .collect();
        let start = std::time::Instant::now();
        let audio = filter(&samples);
        eprintln!(
            "Silero 60-second recording: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.
        );
        assert!(audio.summary.is_some());
        assert!(!audio.no_speech());
    }
}
