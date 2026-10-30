//! Cleanup prompt resolution and chat-completions request shaping.

use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::text::DEFAULT_AGENT_NAME;
use crate::text::correction_learner::KnownCorrection;
use crate::text::language::get_language_instruction;

// ---------------------------------------------------------------------------
// Prompt bundles

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct PromptBundle {
    cleanup_prompt: String,
    dictionary_suffix: String,
    corrections_suffix: String,
}

fn load_bundle(raw: &str, name: &str) -> PromptBundle {
    serde_json::from_str(raw).unwrap_or_else(|err| {
        log::error!("{name} is invalid: {err}");
        PromptBundle::default()
    })
}

static EN_PROMPTS: LazyLock<PromptBundle> = LazyLock::new(|| {
    load_bundle(
        include_str!("../../locales/prompts.en.json"),
        "prompts.en.json",
    )
});
static ZH_CN_PROMPTS: LazyLock<PromptBundle> = LazyLock::new(|| {
    load_bundle(
        include_str!("../../locales/prompts.zh-CN.json"),
        "prompts.zh-CN.json",
    )
});

/// Chinese language tags (any `zh` variant) map to Simplified Chinese;
/// anything else is English.
pub fn normalize_ui_language(language: &str) -> &'static str {
    let normalized = language.trim().replace('_', "-").to_lowercase();
    if normalized == "zh" || normalized.starts_with("zh-") {
        "zh-CN"
    } else {
        "en"
    }
}

fn bundle(ui_language: &str) -> &'static PromptBundle {
    match normalize_ui_language(ui_language) {
        "zh-CN" => &ZH_CN_PROMPTS,
        _ => &EN_PROMPTS,
    }
}

/// The shipped cleanup prompt for a UI language (English when the bundle
/// lacks it).
pub fn default_cleanup_prompt(ui_language: &str) -> &'static str {
    let text = &bundle(ui_language).cleanup_prompt;
    if text.is_empty() {
        &EN_PROMPTS.cleanup_prompt
    } else {
        text
    }
}

/// Text placed before the comma-joined dictionary words.
pub fn dictionary_suffix(ui_language: &str) -> &'static str {
    let text = &bundle(ui_language).dictionary_suffix;
    if text.is_empty() {
        &EN_PROMPTS.dictionary_suffix
    } else {
        text
    }
}

pub fn append_dictionary_suffix<S: AsRef<str>>(
    prompt: &str,
    custom_dictionary: &[S],
    ui_language: &str,
) -> String {
    if custom_dictionary.is_empty() {
        return prompt.to_string();
    }
    let words: Vec<&str> = custom_dictionary.iter().map(AsRef::as_ref).collect();
    format!(
        "{prompt}{}{}",
        dictionary_suffix(ui_language),
        words.join(", ")
    )
}

/// The most corrections listed in a cleanup prompt.
const MAX_PROMPT_CORRECTIONS: usize = 30;

/// Appends mishearings for the model to judge in context.
pub fn append_corrections_suffix(
    prompt: &str,
    corrections: &[&KnownCorrection],
    ui_language: &str,
) -> String {
    if corrections.is_empty() {
        return prompt.to_string();
    }
    let text = &bundle(ui_language).corrections_suffix;
    let suffix = if text.is_empty() {
        &EN_PROMPTS.corrections_suffix
    } else {
        text
    };
    let pairs: Vec<String> = corrections
        .iter()
        .take(MAX_PROMPT_CORRECTIONS)
        .map(|c| format!("{} → {}", c.misheard, c.term))
        .collect();
    format!("{prompt}{suffix}{}", pairs.join("; "))
}

/// Fill `{{agentName}}`, append the language instruction for `language`
/// (none when empty) and the dictionary suffix.
pub fn resolve_prompt_template<S: AsRef<str>>(
    template: &str,
    language: &str,
    custom_dictionary: &[S],
    ui_language: &str,
) -> String {
    let mut prompt = template.replace("{{agentName}}", DEFAULT_AGENT_NAME);
    let instruction = get_language_instruction(language);
    if !instruction.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(&instruction);
    }
    append_dictionary_suffix(&prompt, custom_dictionary, ui_language)
}

/// The cleanup system prompt: the user's custom prompt unless it is empty,
/// else the default for the UI language.
pub fn resolve_cleanup_prompt<S: AsRef<str>>(
    custom_prompt: &str,
    language: &str,
    custom_dictionary: &[S],
    ui_language: &str,
) -> String {
    let template = match custom_prompt {
        "" => default_cleanup_prompt(ui_language),
        custom => custom,
    };
    resolve_prompt_template(template, language, custom_dictionary, ui_language)
}

/// The user message: the cleanup prompt says input arrives between
/// <transcript> tags, and the trailing line re-anchors the output contract.
pub fn wrap_cleanup_transcript(text: &str) -> String {
    format!("<transcript>\n{text}\n</transcript>\n\nOutput only the cleaned transcript.")
}

// ---------------------------------------------------------------------------
// Model families and thinking suppression

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReasoningEffortConstraint {
    /// Value that best approximates "thinking off" for reasoning effort.
    pub suppress_value: &'static str,
    /// Effort for deterministic transforms such as cleanup.
    pub cleanup_value: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFamilyConstraints {
    pub reasoning_effort: Option<ReasoningEffortConstraint>,
}

const FAMILIES: [(&str, ModelFamilyConstraints); 2] = [
    (
        "gpt-oss",
        ModelFamilyConstraints {
            // gpt-oss accepts low|medium|high only; it has no off switch. Low
            // also cuts hidden reasoning and answering-instead-of-cleaning.
            reasoning_effort: Some(ReasoningEffortConstraint {
                suppress_value: "low",
                cleanup_value: Some("low"),
            }),
        },
    ),
    (
        "qwen",
        ModelFamilyConstraints {
            reasoning_effort: Some(ReasoningEffortConstraint {
                suppress_value: "none",
                cleanup_value: None,
            }),
        },
    ),
];

/// Family constraints for a model id (substring match, case-insensitive).
pub fn get_model_family_constraints(model: &str) -> Option<&'static ModelFamilyConstraints> {
    let id = model.to_lowercase();
    if id.is_empty() {
        return None;
    }
    FAMILIES
        .iter()
        .find(|(needle, _)| id.contains(needle))
        .map(|(_, c)| c)
}

/// Disable thinking through the self-hosted OpenAI-compatible shape: the
/// nested `reasoning` object (Ollama) rather than a flat `reasoning_effort`
/// that vLLM can reject, plus `chat_template_kwargs.enable_thinking`.
pub fn suppress_thinking(body: &mut Map<String, Value>, model: &str) {
    let effort = get_model_family_constraints(model)
        .and_then(|c| c.reasoning_effort)
        .map_or("none", |e| e.suppress_value);
    body.insert("reasoning".into(), json!({ "effort": effort }));
    body.insert(
        "chat_template_kwargs".into(),
        json!({ "enable_thinking": false }),
    );
}

// ---------------------------------------------------------------------------
// Chat-completions request shaping

/// Set `max_tokens`, zero `temperature` (cleanup is a deterministic
/// transform), family `reasoning_effort` and thinking suppression on a
/// chat-completions body. Self-hosted servers speak the classic shape: always
/// `max_tokens`, always `temperature`. Callers own `model`, `messages` and
/// `stream`.
pub fn apply_chat_completions_params(
    body: &mut Map<String, Value>,
    model: &str,
    disable_thinking: bool,
    max_tokens: u32,
) {
    body.insert("max_tokens".into(), json!(max_tokens));
    body.insert("temperature".into(), json!(0.0));

    let cleanup_effort = get_model_family_constraints(model)
        .and_then(|c| c.reasoning_effort)
        .and_then(|e| e.cleanup_value);
    if let Some(effort) = cleanup_effort {
        body.insert("reasoning_effort".into(), json!(effort));
    }

    if disable_thinking {
        suppress_thinking(body, model);
    }
}

/// Finish reasons that mean the output hit the token cap, across providers.
pub fn is_truncated_finish_reason(reason: Option<&str>) -> bool {
    matches!(reason, Some("length" | "max_tokens"))
}

/// Shaped params a backend may reject by name with a 400/422. Only params the
/// shaping layer added are strippable, never `messages` or `model`.
pub const STRIPPABLE_SHAPED_PARAMS: &[&str] =
    &["reasoning_effort", "chat_template_kwargs", "temperature"];

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: [&str; 0] = [];

    #[test]
    fn ui_language_normalization() {
        assert_eq!(normalize_ui_language("zh"), "zh-CN");
        assert_eq!(normalize_ui_language("zh_TW"), "zh-CN");
        assert_eq!(normalize_ui_language("ZH-cn"), "zh-CN");
        assert_eq!(normalize_ui_language("en-US"), "en");
        assert_eq!(normalize_ui_language(""), "en");
    }

    #[test]
    fn default_prompts_per_ui_language() {
        assert!(default_cleanup_prompt("en").contains("{{agentName}}"));
        assert_ne!(
            default_cleanup_prompt("zh-CN"),
            default_cleanup_prompt("en")
        );
        assert!(dictionary_suffix("en").starts_with("\n\nCustom Dictionary"));
        assert_ne!(dictionary_suffix("zh-CN"), dictionary_suffix("en"));
    }

    #[test]
    fn template_resolution() {
        let p = resolve_prompt_template("Hi {{agentName}}, {{agentName}}.", "", &NONE, "en");
        assert_eq!(p, "Hi Whisper, Whisper.");
        let p = resolve_prompt_template("X", "ar", &["Quokka", "Zed"], "en");
        assert!(p.starts_with("X\n\nLANGUAGE CONTEXT"));
        assert!(p.ends_with("use the dictionary spelling): Quokka, Zed"));
    }

    #[test]
    fn corrections_are_listed_after_the_prompt_and_capped() {
        assert_eq!(append_corrections_suffix("X", &[], "en"), "X");
        let correction = KnownCorrection {
            misheard: "cloud".into(),
            term: "Claude".into(),
            count: 1,
        };
        let p = append_corrections_suffix("X", &[&correction], "zh-CN");
        assert!(p.starts_with("X\n\n已知误识别"));
        assert!(p.ends_with("cloud → Claude"));
        let many = vec![&correction; MAX_PROMPT_CORRECTIONS + 5];
        let p = append_corrections_suffix("X", &many, "en");
        assert_eq!(p.matches("→").count(), MAX_PROMPT_CORRECTIONS);
    }

    #[test]
    fn cleanup_prompt_uses_custom_or_default() {
        assert_eq!(
            resolve_cleanup_prompt("Saved prompt", "", &NONE, "en"),
            "Saved prompt"
        );
        let default = resolve_cleanup_prompt("", "", &NONE, "en");
        assert!(default.contains("\"Whisper\""));
    }

    #[test]
    fn wraps_transcript() {
        assert_eq!(
            wrap_cleanup_transcript("hi"),
            "<transcript>\nhi\n</transcript>\n\nOutput only the cleaned transcript."
        );
    }

    #[test]
    fn family_lookup() {
        let effort = |m| get_model_family_constraints(m).and_then(|c| c.reasoning_effort);
        assert_eq!(
            effort("openai/GPT-OSS-120b"),
            Some(ReasoningEffortConstraint {
                suppress_value: "low",
                cleanup_value: Some("low")
            })
        );
        assert!(effort("gpt-oss-safeguard-120b").is_some());
        assert_eq!(
            effort("qwen/qwen3-32b"),
            Some(ReasoningEffortConstraint {
                suppress_value: "none",
                cleanup_value: None
            })
        );
        assert_eq!(effort("gpt-4o"), None);
        assert_eq!(effort(""), None);
    }

    #[test]
    fn thinking_suppression() {
        let mut body = Map::new();
        suppress_thinking(&mut body, "qwen3-8b");
        assert_eq!(
            Value::Object(body),
            json!({"reasoning": {"effort": "none"}, "chat_template_kwargs": {"enable_thinking": false}})
        );
        let mut body = Map::new();
        suppress_thinking(&mut body, "gpt-oss-20b-mxfp4");
        assert_eq!(body["reasoning"]["effort"], "low");
        assert!(!body.contains_key("reasoning_effort"));
        let mut body = Map::new();
        suppress_thinking(&mut body, "GPT-OSS-20B");
        assert_eq!(body["reasoning"]["effort"], "low");
    }

    #[test]
    fn chat_params_for_cleanup() {
        let mut body = Map::new();
        apply_chat_completions_params(&mut body, "gpt-oss-20b", true, 2048);
        assert_eq!(body["max_tokens"], 2048);
        assert_eq!(body["temperature"], 0.0);
        assert_eq!(body["reasoning_effort"], "low");
        assert_eq!(body["reasoning"]["effort"], "low");
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);

        let mut body = Map::new();
        apply_chat_completions_params(&mut body, "qwen3-8b", false, 2048);
        assert!(!body.contains_key("reasoning_effort"));
        assert!(!body.contains_key("reasoning"));
        assert!(!body.contains_key("chat_template_kwargs"));
    }

    #[test]
    fn finish_reasons_and_stripping() {
        assert!(is_truncated_finish_reason(Some("length")));
        assert!(is_truncated_finish_reason(Some("max_tokens")));
        assert!(!is_truncated_finish_reason(Some("stop")));
        assert!(!is_truncated_finish_reason(None));
    }
}
