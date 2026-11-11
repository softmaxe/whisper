//! The processing half of a Dictation: one recording in, the text to deliver
//! out. Live dictation and History retry both go through [`run`], so they
//! produce the same text.
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
use crate::text::correction_learner::{KnownCorrection, apply_known_corrections};
use crate::text::echo_filter::matches_dictionary_prompt;
use crate::text::prompt_cap::{CLEANUP_PROMPT_CHARS, WHISPER_PROMPT_CHARS, fit_dictionary_prompt};
use crate::text::snippets::{
    Snippet, expand_snippets, get_prioritized_dictionary_hint_words, whole_snippet_replacement,
};

/// Everything the pipeline needs, captured when the recording stops so a
/// settings change mid-request cannot mix two configurations.
#[derive(Debug, Clone)]
pub struct PipelineInput {
    /// WAV audio (16 kHz for dictation, the stored recording for retry).
    pub audio: bytes::Bytes,
    pub settings: Settings,
    pub dictionary: Vec<String>,
    /// Dictionary words most-used first.
    pub dictionary_priority: Vec<String>,
    /// What the user has corrected before, most repeated first.
    pub known_corrections: Vec<KnownCorrection>,
    pub snippets: Vec<Snippet>,
    pub cleanup_api_key: Option<String>,
    pub cleanup_stage: Option<async_channel::Sender<bool>>,
}

/// Why cleanup did not run on a transcript that was still delivered raw.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupFailure {
    pub message: String,
    pub message_key: Option<String>,
}

/// The text a Dictation delivers, and what it was made from.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// Cleaned, script-converted and with Snippets expanded: what is pasted
    /// and saved.
    pub text: String,
    /// `text` before Snippet expansion: the words actually spoken, so saved
    /// Snippet text is not counted as Dictionary usage.
    pub spoken_text: String,
    /// The server's transcript before cleanup.
    pub raw_text: String,
    pub cleanup_failure: Option<CleanupFailure>,
    pub cleanup_skipped: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineOutcome {
    /// The server heard nothing usable.
    NoAudio,
    /// Text alone cannot distinguish a prompt echo from spoken hint words.
    /// Keep the candidate for explicit recovery instead of delivering it.
    PromptEcho {
        text: String,
    },
    Transcribed(Transcript),
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
    let full = merge_whisper_prompt(fitted.as_deref(), target);
    (fitted, full)
}

/// Whether cleanup is configured: enabled, with a model and a server.
pub fn cleanup_enabled(settings: &Settings) -> bool {
    settings.use_cleanup_model
        && !settings.cleanup_model.trim().is_empty()
        && !settings.cleanup_remote_url.trim().is_empty()
}

pub fn cleanup_system_prompt(input: &PipelineInput) -> String {
    let s = &input.settings;
    let words = get_prioritized_dictionary_hint_words(
        &input.dictionary,
        &input.snippets,
        &input.dictionary_priority,
    );
    let hints = fit_dictionary_prompt(&words, CLEANUP_PROMPT_CHARS);
    let prompt = crate::text::prompts::resolve_cleanup_prompt(
        &s.custom_prompt_cleanup,
        &resolve_cleanup_language(&s.preferred_language),
        hints.as_slice(),
        &s.ui_language,
    );
    // Corrections that apply automatically never reach the model as a question.
    let judged: Vec<&KnownCorrection> = input
        .known_corrections
        .iter()
        .filter(|c| !c.applies_automatically())
        .collect();
    crate::text::prompts::append_corrections_suffix(&prompt, &judged, &s.ui_language)
}

/// Cleans a transcript. A failure or blank reply keeps the transcript and
/// reports why.
pub async fn clean_up(
    text: &str,
    input: &PipelineInput,
    cancel: &CancellationToken,
) -> (String, Option<CleanupFailure>) {
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
    match cleanup::process_text(normalized, &config, cancel).await {
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

fn filter_fillers(text: &str, settings: &Settings) -> String {
    if settings.remove_filler_words {
        crate::text::filler_words::remove_filler_words(text, &settings.preferred_language)
    } else {
        text.to_string()
    }
}

/// The deterministic Chinese script pass, run on the cleaned text.
pub fn finalize_script(text: &str, settings: &Settings) -> String {
    let target = resolve_chinese_script_target(
        &settings.preferred_language,
        settings.chinese_script_preference,
        Some(text),
    );
    apply_chinese_script(text, target)
}

/// Transcribes `input.audio`, filters dictionary echoes, cleans up, converts
/// the Chinese script and expands Snippets. Cancelling `cancel` aborts only
/// this run.
pub async fn run(
    input: PipelineInput,
    cancel: CancellationToken,
    skip_cleanup: CancellationToken,
) -> PipelineOutcome {
    let route = match transcription::resolve_transcription_route(&input.settings) {
        Ok(route) => route,
        Err(err) => return PipelineOutcome::failed_from(&TranscriptionError::Route(err)),
    };
    let (dictionary_prompt, whisper_prompt) = build_whisper_prompt(&input);
    let request = TranscriptionRequest::new(
        &route,
        input.audio.clone(),
        "audio/wav",
        whisper_prompt.clone(),
    );
    let response = match transcription::transcribe(request, cancel.clone()).await {
        Ok(response) => response,
        Err(TranscriptionError::Cancelled) => return PipelineOutcome::Cancelled,
        Err(TranscriptionError::EmptyText) => {
            return PipelineOutcome::NoAudio;
        }
        Err(err) => return PipelineOutcome::failed_from(&err),
    };
    let raw = response.text;
    if cancel.is_cancelled() {
        return PipelineOutcome::Cancelled;
    }
    // A whole trigger is intentional input even when it equals the ASR hint.
    // Deliver saved text verbatim, without letting cleanup rewrite the trigger.
    // Check raw input first so filler tokens in saved triggers keep working.
    if let Some(text) = whole_snippet_replacement(&raw, &input.snippets) {
        return PipelineOutcome::Transcribed(Transcript {
            text,
            spoken_text: raw.trim().to_string(),
            raw_text: raw,
            cleanup_failure: None,
            cleanup_skipped: false,
        });
    }
    let filtered = filter_fillers(&raw, &input.settings);
    let fillers_removed = filtered != raw;
    if filtered.trim().is_empty()
        || (fillers_removed && !filtered.chars().any(char::is_alphanumeric))
    {
        return PipelineOutcome::NoAudio;
    }
    if let Some(text) = whole_snippet_replacement(&filtered, &input.snippets) {
        return PipelineOutcome::Transcribed(Transcript {
            text,
            spoken_text: filtered.trim().to_string(),
            raw_text: raw,
            cleanup_failure: None,
            cleanup_skipped: false,
        });
    }
    // Check the dictionary alone as well as the full prompt: a script bias
    // counts as one more word and can hide a short dictionary's echo.
    let echo = dictionary_prompt
        .as_deref()
        .is_some_and(|p| matches_dictionary_prompt(&raw, p))
        || whisper_prompt
            .as_deref()
            .is_some_and(|p| matches_dictionary_prompt(&raw, p));
    if echo {
        return PipelineOutcome::PromptEcho { text: raw };
    }
    if cancel.is_cancelled() {
        return PipelineOutcome::Cancelled;
    }
    let mut cleanup_skipped = false;
    let (cleaned, cleanup_failure) = if cleanup_enabled(&input.settings) {
        // A skip issued during ASR cannot arm a future cleanup stage.
        let can_skip = !skip_cleanup.is_cancelled();
        let stage = CleanupStage::new(input.cleanup_stage.as_ref());
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return PipelineOutcome::Cancelled,
            _ = skip_cleanup.cancelled(), if can_skip => {
                cleanup_skipped = true;
                (filtered.clone(), None)
            }
            result = clean_up(&filtered, &input, &cancel) => result,
        };
        drop(stage);
        result
    } else {
        (filtered.clone(), None)
    };
    if cancel.is_cancelled() {
        return PipelineOutcome::Cancelled;
    }
    // Mishearings were learned from delivered text, so they match it here.
    let filtered_cleaned = filter_fillers(&cleaned, &input.settings);
    if (fillers_removed || filtered_cleaned != cleaned)
        && !filtered_cleaned.chars().any(char::is_alphanumeric)
    {
        return PipelineOutcome::NoAudio;
    }
    let spoken_text = apply_known_corrections(
        &finalize_script(&filtered_cleaned, &input.settings),
        &input.known_corrections,
    );
    if spoken_text.trim().is_empty() {
        return PipelineOutcome::NoAudio;
    }
    PipelineOutcome::Transcribed(Transcript {
        text: expand_snippets(&spoken_text, &input.snippets),
        spoken_text,
        raw_text: raw,
        cleanup_failure,
        cleanup_skipped,
    })
}

struct CleanupStage<'a>(Option<&'a async_channel::Sender<bool>>);

impl<'a> CleanupStage<'a> {
    fn new(sender: Option<&'a async_channel::Sender<bool>>) -> Self {
        if let Some(sender) = sender {
            let _ = sender.try_send(true);
        }
        Self(sender)
    }
}

impl Drop for CleanupStage<'_> {
    fn drop(&mut self) {
        if let Some(sender) = self.0 {
            let _ = sender.try_send(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_server::{MockServer, Reply, run as run_test};
    use serde_json::json;
    use std::time::Duration;

    async fn run(input: PipelineInput, cancel: CancellationToken) -> PipelineOutcome {
        super::run(input, cancel, CancellationToken::new()).await
    }

    fn input(dictionary: &[&str]) -> PipelineInput {
        PipelineInput {
            audio: bytes::Bytes::new(),
            settings: Settings::default(),
            dictionary: dictionary.iter().map(|s| s.to_string()).collect(),
            dictionary_priority: Vec::new(),
            known_corrections: Vec::new(),
            snippets: Vec::new(),
            cleanup_api_key: None,
            cleanup_stage: None,
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

        // The bias survives a dictionary that overflows the budget.
        let words: Vec<String> = (0..400).map(|n| format!("term{n}")).collect();
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let mut i = input(&words);
        i.settings.preferred_language = "zh-CN".into();
        let full = build_whisper_prompt(&i).1.unwrap();
        assert!(full.starts_with("以下是简体中文"));
        assert!(full.chars().count() <= WHISPER_PROMPT_CHARS);
        assert!(full.ends_with("term0"));
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

    fn asr(text: &'static str) -> MockServer {
        MockServer::start(move |req| {
            if req.method == "HEAD" {
                Reply::status(405)
            } else {
                Reply::json(200, &json!({ "text": text }).to_string())
            }
        })
    }

    fn llm(reply: Reply) -> MockServer {
        MockServer::start(move |_| reply.clone())
    }

    fn completion(content: &str) -> Reply {
        Reply::json(
            200,
            &json!({"choices": [{"message": {"content": content}, "finish_reason": "stop"}]})
                .to_string(),
        )
    }

    fn served(asr: &MockServer, llm: Option<&MockServer>) -> PipelineInput {
        let mut i = input(&[]);
        i.settings.remote_transcription_url = asr.url("/v1");
        if let Some(llm) = llm {
            i.settings.cleanup_remote_url = llm.url("/v1");
            i.settings.cleanup_model = "m".into();
        }
        i
    }

    fn transcribed(outcome: PipelineOutcome) -> Transcript {
        match outcome {
            PipelineOutcome::Transcribed(t) => t,
            other => panic!("unexpected outcome {other:?}"),
        }
    }

    #[test]
    fn delivers_cleaned_script_converted_text_with_snippets_expanded() {
        let asr = asr("um the 軟體 sig");
        let llm = llm(completion("The 軟體 sig."));
        let mut i = served(&asr, Some(&llm));
        i.settings.preferred_language = "zh-CN".into();
        i.snippets = vec![Snippet {
            trigger: "sig".into(),
            replacement: "Best regards".into(),
        }];
        let t = transcribed(run_test(run(i, CancellationToken::new())));
        assert_eq!(t.raw_text, "um the 軟體 sig");
        assert_eq!(t.spoken_text, "The 软件 sig.");
        assert_eq!(t.text, "The 软件 Best regards.");
        assert_eq!(t.cleanup_failure, None);
    }

    fn corrections() -> Vec<KnownCorrection> {
        [("克劳德", "Claude", 2), ("cloud", "Claude", 5)]
            .map(|(misheard, term, count)| KnownCorrection {
                misheard: misheard.into(),
                term: term.into(),
                count,
            })
            .to_vec()
    }

    #[test]
    fn delivered_text_carries_known_corrections_and_raw_text_does_not() {
        let asr = asr("我用克劳德写 cloud 代码");
        let mut i = served(&asr, None);
        i.known_corrections = corrections();
        let t = transcribed(run_test(run(i, CancellationToken::new())));
        assert_eq!(t.raw_text, "我用克劳德写 cloud 代码");
        assert_eq!(t.text, "我用Claude写 cloud 代码");
        assert_eq!(t.cleanup_failure, None);
    }

    #[test]
    fn the_cleanup_prompt_lists_only_corrections_that_need_context() {
        let mut i = input(&["Claude"]);
        i.known_corrections = corrections();
        let prompt = cleanup_system_prompt(&i);
        assert!(prompt.contains("cloud → Claude"));
        assert!(!prompt.contains("克劳德 →"));
    }

    #[test]
    fn the_cleanup_prompt_keeps_the_most_important_words_of_a_long_dictionary() {
        let mut i = input(&[]);
        i.dictionary = (0..1000).map(|n| format!("filler-term-{n}")).collect();
        i.dictionary_priority = i.dictionary.iter().rev().cloned().collect();
        let prompt = cleanup_system_prompt(&i);
        assert!(prompt.contains("filler-term-999"));
        assert!(!prompt.contains("filler-term-500"));
        assert!(prompt.len() < 12_000);
    }

    #[test]
    fn cleanup_failure_still_delivers_the_raw_transcript() {
        let asr = asr("hello there");
        let llm = llm(Reply::json(401, r#"{"error":"bad key"}"#));
        let t = transcribed(run_test(run(
            served(&asr, Some(&llm)),
            CancellationToken::new(),
        )));
        assert_eq!(t.text, "hello there");
        assert_eq!(t.cleanup_failure.unwrap().message, "bad key");
    }

    #[test]
    fn standalone_snippet_survives_prompt_echo_filtering() {
        for spoken in ["sig", "Sig."] {
            let asr = asr(spoken);
            let mut i = served(&asr, None);
            i.snippets = vec![Snippet {
                trigger: "sig".into(),
                replacement: "Best regards".into(),
            }];
            let t = transcribed(run_test(run(i, CancellationToken::new())));
            assert_eq!(t.text, "Best regards");
            assert_eq!(t.raw_text, spoken);
        }
    }

    #[test]
    fn whole_snippets_keep_saved_text_without_cleanup() {
        let asr = asr("My email, address.");
        let llm = llm(completion("Email address."));
        let mut i = served(&asr, Some(&llm));
        i.snippets = vec![Snippet {
            trigger: "my email address".into(),
            replacement: "me@example.com".into(),
        }];
        let t = transcribed(run_test(run(i, CancellationToken::new())));
        assert_eq!(t.text, "me@example.com");
        assert_eq!(t.spoken_text, "My email, address.");
        assert!(llm.requests().is_empty());
    }

    #[test]
    fn partial_snippets_do_not_exempt_multi_entry_prompt_echoes_from_review() {
        let asr = asr("sig, Supabase, Tauri");
        let mut i = served(&asr, None);
        i.dictionary = vec!["Supabase".into(), "Tauri".into()];
        i.snippets = vec![Snippet {
            trigger: "sig".into(),
            replacement: "Best regards".into(),
        }];
        assert_eq!(
            run_test(run(i, CancellationToken::new())),
            PipelineOutcome::PromptEcho {
                text: "sig, Supabase, Tauri".into()
            }
        );
    }

    #[test]
    fn invalid_asr_schema_is_a_recoverable_failure() {
        let asr = MockServer::start(|_| Reply::json(200, r#"{"text":null}"#));
        assert!(matches!(
            run_test(run(served(&asr, None), CancellationToken::new())),
            PipelineOutcome::Failed { .. }
        ));
    }

    #[test]
    fn valid_empty_asr_text_is_still_no_audio() {
        let asr = asr("   ");
        assert_eq!(
            run_test(run(served(&asr, None), CancellationToken::new())),
            PipelineOutcome::NoAudio
        );
    }

    #[test]
    fn filler_only_transcripts_are_no_audio_without_cleanup() {
        let asr = asr("Um, uh, 呃，");
        assert_eq!(
            run_test(run(served(&asr, None), CancellationToken::new())),
            PipelineOutcome::NoAudio
        );
    }

    #[test]
    fn punctuation_only_is_no_audio_only_when_fillers_were_removed() {
        for raw in ["uh!", "呃。", "Uh... ", "uh ; !"] {
            let asr = asr(raw);
            assert_eq!(
                run_test(run(served(&asr, None), CancellationToken::new())),
                PipelineOutcome::NoAudio,
                "{raw:?}"
            );
        }
        let asr = asr("; !");
        let t = transcribed(run_test(run(served(&asr, None), CancellationToken::new())));
        assert_eq!(t.text, "; !");
        let asr = self::asr("uh!");
        let mut input = served(&asr, None);
        input.settings.remove_filler_words = false;
        let t = transcribed(run_test(run(input, CancellationToken::new())));
        assert_eq!(t.text, "uh!");
        let asr = self::asr("hello");
        let llm = llm(completion("uh ; !"));
        assert_eq!(
            run_test(run(served(&asr, Some(&llm)), CancellationToken::new())),
            PipelineOutcome::NoAudio
        );
    }

    #[test]
    fn whole_snippets_match_raw_before_filtered_transcripts() {
        for (raw, trigger, spoken) in [
            ("uh oh", "uh oh", "uh oh"),
            ("Um, my address", "my address", "My address"),
            ("hmm", "hmm", "hmm"),
        ] {
            let asr = asr(raw);
            let llm = llm(completion("must not run"));
            let mut input = served(&asr, Some(&llm));
            input.snippets = vec![Snippet::new(trigger, "Saved text")];
            if raw == "uh oh" {
                input
                    .snippets
                    .push(Snippet::new("oh", "Wrong filtered match"));
            }
            let t = transcribed(run_test(run(input, CancellationToken::new())));
            assert_eq!(t.text, "Saved text");
            assert_eq!(t.raw_text, raw);
            assert_eq!(t.spoken_text, spoken);
            assert!(llm.requests().is_empty());
        }
    }

    #[test]
    fn filler_removal_preserves_raw_text_and_can_be_disabled() {
        for enabled in [true, false] {
            let asr = asr("Um, we should go");
            let mut input = served(&asr, None);
            input.settings.remove_filler_words = enabled;
            let t = transcribed(run_test(run(input, CancellationToken::new())));
            assert_eq!(t.raw_text, "Um, we should go");
            assert_eq!(
                t.text,
                if enabled {
                    "We should go"
                } else {
                    "Um, we should go"
                }
            );
        }
    }

    #[test]
    fn filler_removal_wraps_cleanup_and_precedes_whole_snippet_matching() {
        let asr = asr("Um, we should go");
        let llm = llm(completion("Uh, we should go."));
        let t = transcribed(run_test(run(
            served(&asr, Some(&llm)),
            CancellationToken::new(),
        )));
        assert_eq!(t.text, "We should go.");
        assert!(String::from_utf8_lossy(&llm.requests()[0].body).contains("We should go"));
        let asr = self::asr("Um, sig.");
        let mut input = served(&asr, None);
        input.snippets = vec![Snippet {
            trigger: "sig".into(),
            replacement: "Best regards".into(),
        }];
        let t = transcribed(run_test(run(input, CancellationToken::new())));
        assert_eq!(t.text, "Best regards");
        assert_eq!(t.raw_text, "Um, sig.");
    }

    #[test]
    fn dictionary_echo_keeps_candidate_text_without_cleanup_or_delivery() {
        for (spoken, dictionary) in [
            ("Supabase", vec!["Supabase".into()]),
            ("Supabase, Tauri", vec!["Supabase".into(), "Tauri".into()]),
        ] {
            let asr = asr(spoken);
            let llm = llm(completion("rewritten"));
            let mut i = served(&asr, Some(&llm));
            i.dictionary = dictionary;
            assert_eq!(
                run_test(run(i, CancellationToken::new())),
                PipelineOutcome::PromptEcho {
                    text: spoken.into()
                }
            );
            assert!(llm.requests().is_empty());
        }
    }

    #[test]
    fn cancelling_during_cleanup_stops_the_run() {
        let asr = asr("hello there");
        let llm = llm(completion("late").delayed(Duration::from_secs(3)));
        let input = served(&asr, Some(&llm));
        let started = std::time::Instant::now();
        let outcome = run_test(async move {
            let cancel = CancellationToken::new();
            let canceller = cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                canceller.cancel();
            });
            run(input, cancel).await
        });
        assert_eq!(outcome, PipelineOutcome::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn skipping_cleanup_is_prompt_and_finishes_deterministic_steps_once() {
        let asr = asr("Uh, 電腦 克劳德 sig");
        let llm = llm(completion("late reply").delayed(Duration::from_secs(3)));
        let mut input = served(&asr, Some(&llm));
        input.settings.preferred_language = "zh-CN".into();
        input.settings.chinese_script_preference =
            crate::settings::ChineseScriptPreference::Simplified;
        input.known_corrections = corrections();
        input.snippets = vec![Snippet {
            trigger: "sig".into(),
            replacement: "uh saved text".into(),
        }];
        let (tx, rx) = async_channel::unbounded();
        input.cleanup_stage = Some(tx);
        let skip = CancellationToken::new();
        let started = std::time::Instant::now();
        let t = transcribed(run_test(async {
            let run = super::run(input, CancellationToken::new(), skip.clone());
            let skipping = async {
                assert_eq!(rx.recv().await, Ok(true));
                tokio::time::sleep(Duration::from_millis(30)).await;
                skip.cancel();
            };
            let (outcome, ()) = tokio::join!(run, skipping);
            outcome
        }));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(t.raw_text, "Uh, 電腦 克劳德 sig");
        assert!(t.text.contains("电脑"), "{}", t.text);
        assert!(t.text.contains("Claude"), "{}", t.text);
        assert!(t.text.ends_with("uh saved text"), "{}", t.text);
        assert!(t.cleanup_skipped);
        assert_eq!(t.cleanup_failure, None);
        assert_eq!(rx.try_recv(), Ok(false));
        assert!(rx.try_recv().is_err());
        assert_eq!(llm.requests().len(), 1);
    }

    #[test]
    fn early_and_late_cleanup_skips_are_harmless() {
        for early in [true, false] {
            let asr = asr("hello there");
            let llm = llm(completion("Hello there."));
            let mut input = served(&asr, Some(&llm));
            let (tx, rx) = async_channel::unbounded();
            input.cleanup_stage = Some(tx);
            let skip = CancellationToken::new();
            if early {
                skip.cancel();
            }
            let t = transcribed(run_test(super::run(
                input,
                CancellationToken::new(),
                skip.clone(),
            )));
            skip.cancel();
            assert_eq!(t.text, "Hello there.");
            assert!(!t.cleanup_skipped);
            assert_eq!(rx.try_recv(), Ok(true));
            assert_eq!(rx.try_recv(), Ok(false));
        }
        let asr = asr("hello there");
        let mut input = served(&asr, None);
        let (tx, rx) = async_channel::unbounded();
        input.cleanup_stage = Some(tx);
        let _ = run_test(run(input, CancellationToken::new()));
        assert!(rx.try_recv().is_err());
    }

    /// Live check against real servers (see [`crate::services::live_tests`]):
    /// `cargo test -- --ignored pipeline`.
    #[test]
    #[ignore]
    fn live_pipeline_transcribes_and_cleans() {
        use crate::audio::wav::{encode_wav_16k, read_wav_mono};
        use crate::services::live_tests::{asr, llm, synthesize};

        let dir = tempfile::tempdir().unwrap();
        let wav = synthesize(dir.path(), "um so please send the report by friday");
        let (spec, mono) = read_wav_mono(&std::fs::read(&wav).unwrap());
        let mut i = input(&[]);
        i.audio = encode_wav_16k(&mono, spec.sample_rate).into();
        (
            i.settings.remote_transcription_url,
            i.settings.remote_transcription_model,
        ) = asr();
        (i.settings.cleanup_remote_url, i.settings.cleanup_model) = llm();
        let outcome = crate::services::runtime::block_on(run(i, CancellationToken::new()));
        match outcome {
            PipelineOutcome::Transcribed(t) => {
                println!("raw: {}\nclean: {}", t.raw_text, t.text);
                assert!(t.cleanup_failure.is_none(), "{:?}", t.cleanup_failure);
                assert!(t.text.to_lowercase().contains("friday"));
            }
            other => panic!("unexpected outcome {other:?}"),
        }
    }
}
