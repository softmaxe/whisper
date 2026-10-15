//! Transcription language registry and cleanup-prompt language instructions.
//!
//! The language data is read from `languageRegistry.json`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Language {
    pub code: String,
    pub label: String,
    pub flag: String,
    /// Hand-written cleanup instruction; others use the generic template.
    #[serde(default)]
    pub instruction: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Registry {
    #[serde(rename = "_genericTemplate", default)]
    generic_template: String,
    languages: Vec<Language>,
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../locales/languageRegistry.json")).unwrap_or_else(
        |err| {
            log::error!("languageRegistry.json is invalid: {err}");
            Registry {
                generic_template: String::new(),
                languages: Vec::new(),
            }
        },
    )
});

static INSTRUCTIONS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    REGISTRY
        .languages
        .iter()
        .filter_map(|l| Some((l.code.as_str(), l.instruction.as_deref()?)))
        .collect()
});

/// Every selectable language, "auto" first, in registry order.
pub fn languages() -> &'static [Language] {
    &REGISTRY.languages
}

/// "zh-CN" -> "zh"; `None` for empty or "auto".
pub fn get_base_language_code(language: &str) -> Option<&str> {
    if language.is_empty() || language == "auto" {
        return None;
    }
    language.split('-').next()
}

/// Cleanup-prompt instruction for a language code; "" when none is given.
pub fn get_language_instruction(language: &str) -> String {
    if language.is_empty() {
        return String::new();
    }
    match INSTRUCTIONS.get(language) {
        Some(instruction) if !instruction.is_empty() => instruction.to_string(),
        _ => REGISTRY.generic_template.replacen("{{code}}", language, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_loads_with_auto_first() {
        let langs = languages();
        assert_eq!(langs[0].code, "auto");
        assert!(langs.iter().any(|l| l.code == "zh-CN"));
    }

    #[test]
    fn base_codes() {
        assert_eq!(get_base_language_code("zh-CN"), Some("zh"));
        assert_eq!(get_base_language_code("en"), Some("en"));
        assert_eq!(get_base_language_code("auto"), None);
        assert_eq!(get_base_language_code(""), None);
    }

    #[test]
    fn instructions() {
        assert_eq!(get_language_instruction(""), "");
        assert!(get_language_instruction("ar").contains("Arabic"));
        assert!(get_language_instruction("auto").contains("auto-detect"));
        let generic = get_language_instruction("af");
        assert!(generic.contains("\"af\""), "{generic}");
        assert!(!generic.contains("{{code}}"));
        assert!(get_language_instruction("xx-YY").contains("\"xx-YY\""));
    }
}
