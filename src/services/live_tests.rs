//! Live compatibility checks against the self-hosted servers. Ignored by
//! default; run with `cargo test --lib live_ -- --ignored`. Override the
//! servers with `WHISPER_LIVE_ASR_URL` / `WHISPER_LIVE_ASR_MODEL` /
//! `WHISPER_LIVE_LLM_URL` / `WHISPER_LIVE_LLM_MODEL`.

use std::path::Path;
use std::process::Command;

use tokio_util::sync::CancellationToken;

use crate::audio::wav::{encode_wav_16k, is_pcm16_mono_16k_wav, read_wav_mono};
use crate::services::cleanup::{CleanupRequestConfig, list_models, process_text};
use crate::services::ffmpeg::ffmpeg_path;
use crate::services::test_server::run;
use crate::services::transcription::{TranscriptionRequest, resolve_route, transcribe};
use crate::services::upload::{FileTranscriptionRequest, transcribe_audio_file};

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

/// Server URLs come from the environment so no address is checked in.
fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name} to run the live tests"))
}

fn asr() -> (String, String) {
    (
        required_env("WHISPER_LIVE_ASR_URL"),
        env_or("WHISPER_LIVE_ASR_MODEL", "Qwen/Qwen3-ASR-1.7B"),
    )
}

fn llm() -> (String, String) {
    (
        required_env("WHISPER_LIVE_LLM_URL"),
        env_or("WHISPER_LIVE_LLM_MODEL", "qwen3.5:9b"),
    )
}

/// Speaks `text` with `say` and converts it to a 48 kHz stereo WAV, so the
/// app's own downmix + resampler produce the uploaded 16 kHz file.
fn synthesize(dir: &Path, text: &str) -> std::path::PathBuf {
    let aiff = dir.join("speech.aiff");
    let wav = dir.join("speech-48k.wav");
    assert!(
        Command::new("say")
            .arg("-o")
            .arg(&aiff)
            .arg(text)
            .status()
            .unwrap()
            .success()
    );
    let ffmpeg = ffmpeg_path().expect("ffmpeg");
    let status = Command::new(ffmpeg)
        .arg("-i")
        .arg(&aiff)
        .args(["-ar", "48000", "-ac", "2", "-c:a", "pcm_s16le", "-y"])
        .arg(&wav)
        .output()
        .unwrap()
        .status;
    assert!(status.success());
    wav
}

fn words(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect()
}

#[test]
#[ignore]
fn live_transcribe_and_clean_up() {
    let dir = tempfile::tempdir().unwrap();
    let wav = synthesize(dir.path(), "hello world, this is a dictation test");
    let (spec, mono) = read_wav_mono(&std::fs::read(&wav).unwrap());
    let upload = encode_wav_16k(&mono, spec.sample_rate);
    assert!(is_pcm16_mono_16k_wav(&upload));

    let (asr_url, asr_model) = asr();
    let route = resolve_route(&asr_url, &asr_model, "en", None).unwrap();
    let request = TranscriptionRequest::new(&route, upload, "audio/wav", Some("Whisper".into()));
    let response = run(transcribe(request, CancellationToken::new())).unwrap();
    eprintln!("transcript: {:?}", response.text);
    let heard = words(&response.text);
    assert!(
        heard.contains("hello") && heard.contains("world"),
        "{heard}"
    );

    let (llm_url, llm_model) = llm();
    let config = CleanupRequestConfig::dictation(
        &llm_url,
        None,
        &llm_model,
        "You clean up dictated text. Fix punctuation and capitalization, remove filler words, \
         and keep the meaning. Reply with the cleaned text only.",
        true,
    );
    let cleaned = run(process_text(
        &response.text,
        &config,
        &CancellationToken::new(),
    ))
    .unwrap();
    eprintln!("cleaned: {cleaned:?}");
    assert!(!cleaned.contains("<think>"));
    assert!(words(&cleaned).contains("hello"), "{cleaned}");
}

#[test]
#[ignore]
fn live_list_models() {
    let (asr_url, asr_model) = asr();
    let (base, models) = run(list_models(&asr_url, None)).unwrap();
    assert_eq!(base, asr_url);
    assert!(models.iter().any(|m| m.value == asr_model), "{models:?}");

    let (llm_url, llm_model) = llm();
    // Bare origin falls back to the /v1 sibling.
    let origin = llm_url.trim_end_matches("/v1");
    let (base, models) = run(list_models(origin, None)).unwrap();
    eprintln!("cleanup base {base}, {} models", models.len());
    assert!(models.iter().any(|m| m.value == llm_model));
}

#[test]
#[ignore]
fn live_upload_reencodes_aiff() {
    let dir = tempfile::tempdir().unwrap();
    let aiff = dir.path().join("memo.aiff");
    assert!(
        Command::new("say")
            .arg("-o")
            .arg(&aiff)
            .arg("upload test one two three")
            .status()
            .unwrap()
            .success()
    );
    let (asr_url, asr_model) = asr();
    let text = run(transcribe_audio_file(FileTranscriptionRequest {
        file_path: aiff,
        language: "en".into(),
        remote_transcription_url: asr_url,
        remote_transcription_model: asr_model,
        request_id: Some("live-upload".into()),
    }))
    .unwrap();
    eprintln!("upload transcript: {text:?}");
    assert!(words(&text).contains("upload"), "{text}");
}
