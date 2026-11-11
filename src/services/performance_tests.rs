//! Opt-in, synthetic-input measurements against the configured live servers.
//!
//! Run with `WHISPER_PERF_AUDIO`, `WHISPER_PERF_SETTINGS_PATH`,
//! `WHISPER_PERF_OUTPUT`, and `WHISPER_PERF_LABEL` set. This probe never opens
//! the application database, accesses the microphone, or pastes text.

use std::path::PathBuf;
use std::time::Instant;

use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::audio::wav::{encode_wav_16k, read_wav_mono};
use crate::pipeline::{self, PipelineInput, PipelineOutcome};
use crate::settings::Settings;

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {name} to run the performance probe"))
}

#[test]
#[ignore]
fn performance_live_pipeline() {
    let audio = std::fs::read(required_path("WHISPER_PERF_AUDIO")).unwrap();
    let (spec, samples) = read_wav_mono(&audio);
    let settings = Settings::load_from(&required_path("WHISPER_PERF_SETTINGS_PATH"));
    assert!(!settings.remote_transcription_url.is_empty());
    let repetitions = std::env::var("WHISPER_PERF_REPETITIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(5);
    let label = std::env::var("WHISPER_PERF_LABEL").unwrap_or_else(|_| "unspecified".into());
    let mut observations = Vec::new();
    // Warm both services before collecting samples; server loading is a
    // separate workload from repeated dictation latency.
    for cleanup in [false, true] {
        if cleanup && !pipeline::cleanup_enabled(&settings) {
            continue;
        }
        for iteration in 0..=repetitions {
            let started = Instant::now();
            let wav = encode_wav_16k(&samples, spec.sample_rate);
            let conversion_ms = started.elapsed().as_secs_f64() * 1000.0;
            let wav_bytes = wav.len();
            let mut run_settings = settings.clone();
            run_settings.preferred_language = "en".into();
            run_settings.use_cleanup_model = cleanup;
            let input = PipelineInput {
                audio: wav.into(),
                settings: run_settings,
                dictionary: Vec::new(),
                dictionary_priority: Vec::new(),
                known_corrections: Vec::new(),
                snippets: Vec::new(),
                cleanup_api_key: None,
                cleanup_stage: None,
            };
            let pipeline_started = Instant::now();
            let outcome = super::runtime::block_on(pipeline::run(
                input,
                CancellationToken::new(),
                CancellationToken::new(),
            ));
            let pipeline_ms = pipeline_started.elapsed().as_secs_f64() * 1000.0;
            let total_ms = started.elapsed().as_secs_f64() * 1000.0;
            let PipelineOutcome::Transcribed(transcript) = outcome else {
                panic!("live pipeline did not produce a transcript");
            };
            assert!(transcript.cleanup_failure.is_none(), "cleanup failed");
            let lower = transcript.text.to_lowercase();
            let phrase_preserved = lower.contains("report") && lower.contains("friday");
            assert!(phrase_preserved, "synthetic phrase was not preserved");
            if iteration > 0 {
                let observation = json!({
                    "label": label,
                    "iteration": iteration,
                    "cleanup": cleanup,
                    "input_rate": spec.sample_rate,
                    "input_samples": samples.len(),
                    "wav_bytes": wav_bytes,
                    "conversion_ms": conversion_ms,
                    "pipeline_ms": pipeline_ms,
                    "total_ms": total_ms,
                    "phrase_preserved": phrase_preserved,
                });
                eprintln!("{observation}");
                observations.push(observation);
            }
        }
    }
    let output = required_path("WHISPER_PERF_OUTPUT");
    std::fs::write(output, serde_json::to_vec_pretty(&observations).unwrap()).unwrap();
}

/// Measures the file transport without retaining the body in the local server.
/// Run each revision in a fresh process under `/usr/bin/time -l` for peak RSS.
#[test]
#[ignore]
fn performance_upload_memory() {
    use std::io;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.wav");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(128 * 1024 * 1024).unwrap();
    drop(file);
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", server.server_addr());
    let receiver = std::thread::spawn(move || {
        let mut request = server.recv().unwrap();
        let read = io::copy(request.as_reader(), &mut io::sink()).unwrap();
        request
            .respond(tiny_http::Response::from_string(
                r#"{"text":"upload benchmark"}"#,
            ))
            .unwrap();
        read
    });
    let started = Instant::now();
    let result = super::runtime::block_on(super::upload::transcribe_audio_file(
        super::upload::FileTranscriptionRequest {
            file_path: path,
            preferred_language: "en".into(),
            remote_transcription_url: url,
            remote_transcription_model: "test".into(),
        },
        CancellationToken::new(),
    ));
    assert_eq!(result.unwrap(), "upload benchmark");
    let received_bytes = receiver.join().unwrap();
    assert!(received_bytes > 128 * 1024 * 1024);
    let observation = json!({
        "label": std::env::var("WHISPER_PERF_LABEL").unwrap_or_default(),
        "file_bytes": 128 * 1024 * 1024,
        "received_bytes": received_bytes,
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
    });
    std::fs::write(
        required_path("WHISPER_PERF_OUTPUT"),
        serde_json::to_vec_pretty(&observation).unwrap(),
    )
    .unwrap();
}

/// Measures startup persistence hydration using a synthetic database. This
/// excludes GPUI initialization, fonts, native helpers and first-frame time.
#[test]
#[ignore]
fn performance_state_hydration() {
    use crate::db::{Database, DictionarySource, Snippet};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.db");
    let db = Database::open(&path).unwrap();
    let words: Vec<String> = (0..5000).map(|index| format!("term{index}")).collect();
    db.apply_dictionary_changes(&words, &[] as &[String], DictionarySource::Manual)
        .unwrap();
    db.set_snippets(&[Snippet::new("signature", "Performance fixture")])
        .unwrap();
    for _ in 0..50 {
        db.save_transcription(&"synthetic transcript ".repeat(20), None, None)
            .unwrap();
    }
    drop(db);
    let mut observations = Vec::new();
    for iteration in 0..=30 {
        let started = Instant::now();
        let db = Database::open(&path).unwrap();
        let opened_ms = started.elapsed().as_secs_f64() * 1000.0;
        let entries = db.get_dictionary_entries().unwrap();
        let words: Vec<String> = entries.iter().map(|entry| entry.word.clone()).collect();
        let corrections = db.get_known_corrections().unwrap();
        let snippets = db.get_snippets().unwrap();
        let history = db.get_transcriptions(50).unwrap();
        std::hint::black_box((&entries, &words, &corrections, &snippets, &history));
        let total_ms = started.elapsed().as_secs_f64() * 1000.0;
        if iteration > 0 {
            observations.push(json!({
                "label": std::env::var("WHISPER_PERF_LABEL").unwrap_or_default(),
                "iteration": iteration,
                "dictionary_entries": entries.len(),
                "history_rows": history.len(),
                "database_open_ms": opened_ms,
                "total_ms": total_ms,
            }));
        }
    }
    std::fs::write(
        required_path("WHISPER_PERF_OUTPUT"),
        serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
}
