//! Transcription language registry and cleanup-prompt language instructions.
//!
//! The language data is read from `languageRegistry.json`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// Pickers with more entries than this get a search field.
pub const LIST_SEARCH_THRESHOLD: usize = 12;

/// Flag shown when no language is selected.
pub const DEFAULT_LANGUAGE_FLAG: &str = "\u{1F310}";

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

pub fn find_language(code: &str) -> Option<&'static Language> {
    REGISTRY.languages.iter().find(|l| l.code == code)
}

/// Selector search: label or code contains the query, case-insensitively.
pub fn filter_languages(query: &str) -> Vec<&'static Language> {
    let q = query.to_lowercase();
    REGISTRY
        .languages
        .iter()
        .filter(|l| l.label.to_lowercase().contains(&q) || l.code.to_lowercase().contains(&q))
        .collect()
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
        assert!(langs.len() > LIST_SEARCH_THRESHOLD);
        assert_eq!(langs[0].code, "auto");
        assert_eq!(langs[0].flag, DEFAULT_LANGUAGE_FLAG);
        assert!(find_language("zh-CN").is_some());
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

    #[test]
    fn search() {
        assert!(filter_languages("arab").iter().any(|l| l.code == "ar"));
        assert!(
            filter_languages("ZH")
                .iter()
                .any(|l| l.code.starts_with("zh"))
        );
        assert_eq!(filter_languages("").len(), languages().len());
    }
}
