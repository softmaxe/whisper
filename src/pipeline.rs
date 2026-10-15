//! The processing half of a Dictation: one recording in, final text out.
//!
//! Recording, delivery and persistence stay with the caller; this module only
//! talks to the servers, so it can run on the Tokio runtime and be tested end
//! to end.

use tokio_util::sync::CancellationToken;

use crate::services::cleanup::{self, CleanupError, CleanupRequestConfig};
use crate::services::transcription::{self, TranscriptionError, TranscriptionRequest};
use crate::settings::Settings;
use crate::text::chinese_script::{
    apply_chinese_script, merge_whisper_prompt, resolve_chinese_script_target,
    resolve_cleanup_language,
};
use crate::text::echo_filter::matches_dictionary_prompt;
use crate::text::prompt_cap::{WHISPER_PROMPT_CHARS, fit_dictionary_prompt};
use crate::text::snippets::{
    Snippet, get_dictionary_hint_words, get_prioritized_dictionary_hint_words,
};

pub const SELF_HOSTED_SOURCE: &str = "self-hosted";

/// Everything the pipeline needs, captured when the recording stops so a
/// settings change mid-request cannot mix two configurations.
#[derive(Debug, Clone)]
pub struct PipelineInput {
    /// Encoded audio (16 kHz WAV for dictation, the stored file for retry).
    pub audio: Vec<u8>,
    pub mime_type: String,
    pub settings: Settings,
    pub dictionary: Vec<String>,
    /// Dictionary words most-used first.
    pub dictionary_priority: Vec<String>,
    pub snippets: Vec<Snippet>,
    pub cleanup_api_key: Option<String>,
    pub agent_name: String,
}

/// Why cleanup did not run on a transcript that was still delivered raw.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupFailure {
    pub message: String,
    pub message_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineOutcome {
    /// The server heard nothing usable, or echoed the dictionary prompt.
    NoAudio {
        dictionary_echo: bool,
    },
    Transcribed {
        text: String,
        raw_text: String,
        /// "self-hosted" or "self-hosted-reasoned", as stored with the row.
        source: String,
        model: Option<String>,
        cleanup_failure: Option<CleanupFailure>,
    },
    Failed {
        message: String,
        code: Option<String>,
        message_key: Option<String>,
    },
    Cancelled,
}

impl PipelineOutcome {
    pub fn failed_from(error: &TranscriptionError) -> Self {
        Self::Failed {
            message: error.to_string(),
            code: error.code().map(str::to_string),
            message_key: error.message_key().map(str::to_string),
        }
    }
}

/// The STT prompt: the most important hint words that fit the request
/// budget, behind the Chinese script bias. Returns `(dictionary, full)`.
pub fn build_whisper_prompt(input: &PipelineInput) -> (Option<String>, Option<String>) {
    let s = &input.settings;
    // No transcript exists yet, so only an explicit zh-CN/zh-TW biases the prompt.
    let target =
        resolve_chinese_script_target(&s.preferred_language, s.chinese_script_preference, None);
    let bias = merge_whisper_prompt(None, target);
    let budget = WHISPER_PROMPT_CHARS
        .saturating_sub(bias.as_ref().map(|b| b.chars().count() + 1).unwrap_or(0));
    let words = get_prioritized_dictionary_hint_words(
        &input.dictionary,
        &input.snippets,
        &input.dictionary_priority,
    );
    let fitted = fit_dictionary_prompt(&words, budget);
    let full = merge_whisper_prompt(fitted.prompt.as_deref(), target);
    (fitted.prompt, full)
}

/// Whether cleanup is configured: enabled, with a model and a server.
pub fn cleanup_enabled(settings: &Settings) -> bool {
    settings.use_cleanup_model
        && !settings.cleanup_model.trim().is_empty()
        && !settings.cleanup_remote_url.trim().is_empty()
}

pub fn cleanup_system_prompt(input: &PipelineInput) -> String {
    let s = &input.settings;
    let hints = get_dictionary_hint_words(&input.dictionary, &input.snippets);
    crate::text::prompts::resolve_cleanup_prompt(
        &s.custom_prompt_cleanup,
        Some(input.agent_name.as_str()),
        &resolve_cleanup_language(&s.preferred_language),
        &hints,
        &s.ui_language,
    )
}

/// Cleans a transcript. A failure or blank reply keeps the transcript and
/// reports why.
pub async fn clean_up(text: &str, input: &PipelineInput) -> (String, Option<CleanupFailure>) {
    let normalized = text.trim();
    if normalized.is_empty() || !cleanup_enabled(&input.settings) {
        return (normalized.to_string(), None);
    }
    let s = &input.settings;
    let config = CleanupRequestConfig::dictation(
        s.cleanup_remote_url.trim(),
        input
            .cleanup_api_key
            .as_deref()
            .filter(|k| !k.trim().is_empty()),
        s.cleanup_model.trim(),
        &cleanup_system_prompt(input),
        s.cleanup_disable_thinking,
    );
    match cleanup::process_text(normalized, &config).await {
        Ok(result) if !result.trim().is_empty() => (result, None),
        Ok(_) => (normalized.to_string(), None),
        Err(err) => (normalized.to_string(), Some(cleanup_failure(&err))),
    }
}

fn cleanup_failure(err: &CleanupError) -> CleanupFailure {
    CleanupFailure {
        message: err.message.clone(),
        message_key: err.message_key.map(str::to_string),
    }
}

/// The deterministic Chinese script pass, run last on the delivered text.
pub fn finalize_script(text: &str, settings: &Settings) -> String {
    let target = resolve_chinese_script_target(
        &settings.preferred_language,
        settings.chinese_script_preference,
        Some(text),
    );
    apply_chinese_script(text, target)
}

/// Transcribes `input.audio`, filters dictionary echoes, then cleans up.
pub async fn run(input: PipelineInput, cancel: CancellationToken) -> PipelineOutcome {
    let route = match transcription::resolve_transcription_route(&input.settings) {
        Ok(route) => route,
        Err(err) => return PipelineOutcome::failed_from(&TranscriptionError::Route(err)),
    };
    let (dictionary_prompt, whisper_prompt) = build_whisper_prompt(&input);
    let request = TranscriptionRequest::new(
        &route,
        input.audio.clone(),
        &input.mime_type,
        whisper_prompt.clone(),
    );
    let response = match transcription::transcribe(request, cancel.clone()).await {
        Ok(response) => response,
        Err(TranscriptionError::Cancelled) => return PipelineOutcome::Cancelled,
        Err(TranscriptionError::EmptyText) => {
            return PipelineOutcome::NoAudio {
                dictionary_echo: false,
            };
        }
        Err(err) => return PipelineOutcome::failed_from(&err),
    };
    let raw = response.text;
    // Check the dictionary alone as well as the full prompt: a script bias
    // counts as one more word and can hide a short dictionary's echo.
    let echo = dictionary_prompt
        .as_deref()
        .is_some_and(|p| matches_dictionary_prompt(&raw, p))
        || whisper_prompt
            .as_deref()
            .is_some_and(|p| matches_dictionary_prompt(&raw, p));
    if echo {
        return PipelineOutcome::NoAudio {
            dictionary_echo: true,
        };
    }
    if cancel.is_cancelled() {
        return PipelineOutcome::Cancelled;
    }
    let reasoned = cleanup_enabled(&input.settings);
    let (cleaned, cleanup_failure) = clean_up(&raw, &input).await;
    if cancel.is_cancelled() {
        return PipelineOutcome::Cancelled;
    }
    let text = finalize_script(&cleaned, &input.settings);
    PipelineOutcome::Transcribed {
        text,
        raw_text: raw,
        source: if reasoned {
            format!("{SELF_HOSTED_SOURCE}-reasoned")
        } else {
            SELF_HOSTED_SOURCE.to_string()
        },
        model: route.model.clone(),
        cleanup_failure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(dictionary: &[&str]) -> PipelineInput {
        PipelineInput {
            audio: Vec::new(),
            mime_type: "audio/wav".into(),
            settings: Settings::default(),
            dictionary: dictionary.iter().map(|s| s.to_string()).collect(),
            dictionary_priority: Vec::new(),
            snippets: Vec::new(),
            cleanup_api_key: None,
            agent_name: "Whisper".into(),
        }
    }

    #[test]
    fn prompt_has_no_bias_on_auto_and_lists_words() {
        let (dict, full) = build_whisper_prompt(&input(&["Supabase", "Tauri"]));
        assert_eq!(dict.as_deref(), full.as_deref());
        let dict = dict.unwrap();
        assert!(dict.contains("Supabase") && dict.contains("Tauri"));
    }

    #[test]
    fn explicit_simplified_chinese_biases_the_prompt() {
        let mut i = input(&["张珊"]);
        i.settings.preferred_language = "zh-CN".into();
        let (_, full) = build_whisper_prompt(&i);
        assert!(full.unwrap().starts_with("以下是简体中文"));
    }

    #[test]
    fn cleanup_requires_model_and_server() {
        let mut s = Settings::default();
        assert!(!cleanup_enabled(&s));
        s.cleanup_model = "m".into();
        s.cleanup_remote_url = "http://localhost:1/v1".into();
        assert!(cleanup_enabled(&s));
        s.use_cleanup_model = false;
        assert!(!cleanup_enabled(&s));
    }

    #[test]
    fn script_pass_converts_only_for_explicit_targets() {
        let mut s = Settings::default();
        assert_eq!(finalize_script("軟體", &s), "軟體");
        s.preferred_language = "zh-CN".into();
        assert_eq!(finalize_script("軟體", &s), "软件");
    }

    /// Live check against real servers (`WHISPER_LIVE_{ASR,LLM}_{URL,MODEL}`):
    /// `cargo test -- --ignored pipeline`.
    #[test]
    #[ignore]
    fn live_pipeline_transcribes_and_cleans() {
        let dir = std::env::temp_dir().join("whisper-pipeline-live");
        std::fs::create_dir_all(&dir).unwrap();
        let aiff = dir.join("p.aiff");
        let wav = dir.join("p.wav");
        assert!(
            std::process::Command::new("say")
                .args([
                    "-o",
                    aiff.to_str().unwrap(),
                    "um so please send the report by friday"
                ])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("ffmpeg")
                .args([
                    "-loglevel",
                    "error",
                    "-y",
                    "-i",
                    aiff.to_str().unwrap(),
                    "-ar",
                    "16000",
                    "-ac",
                    "1",
                    wav.to_str().unwrap()
                ])
                .status()
                .unwrap()
                .success()
        );
        let mut i = input(&[]);
        i.audio = std::fs::read(&wav).unwrap();
        let env = |name: &str| {
            std::env::var(name).unwrap_or_else(|_| panic!("set {name} to run the live tests"))
        };
        i.settings.remote_transcription_url = env("WHISPER_LIVE_ASR_URL");
        i.settings.remote_transcription_model = env("WHISPER_LIVE_ASR_MODEL");
        i.settings.cleanup_remote_url = env("WHISPER_LIVE_LLM_URL");
        i.settings.cleanup_model = env("WHISPER_LIVE_LLM_MODEL");
        let outcome = crate::services::runtime::block_on(run(i, CancellationToken::new()));
        match outcome {
            PipelineOutcome::Transcribed {
                text,
                raw_text,
                source,
                cleanup_failure,
                ..
            } => {
                println!("raw: {raw_text}\nclean: {text}");
                assert!(cleanup_failure.is_none(), "{cleanup_failure:?}");
                assert_eq!(source, "self-hosted-reasoned");
                assert!(text.to_lowercase().contains("friday"));
            }
            other => panic!("unexpected outcome {other:?}"),
        }
    }
}
