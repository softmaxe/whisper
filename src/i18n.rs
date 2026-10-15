//! Translations, read from the bundled JSON locale files.
//!
//! Supports nested keys addressed with dots, `{{name}}` interpolation,
//! `_one`/`_other` plurals selected by a `count` parameter, and English
//! fallback for keys missing from a locale.

use std::sync::RwLock;

use once_cell::sync::Lazy;
use serde_json::Value;

static EN: Lazy<Value> = Lazy::new(|| parse(include_str!("../locales/en.json")));
static ZH_CN: Lazy<Value> = Lazy::new(|| parse(include_str!("../locales/zh-CN.json")));
static CURRENT: RwLock<Language> = RwLock::new(Language::En);

fn parse(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or(Value::Null)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    ZhCn,
}

impl Language {
    /// Maps a stored UI language to a supported one; anything Chinese is zh-CN.
    pub fn from_code(code: &str) -> Self {
        if code.to_ascii_lowercase().starts_with("zh") {
            Self::ZhCn
        } else {
            Self::En
        }
    }

    fn bundle(self) -> &'static Value {
        match self {
            Self::En => &EN,
            Self::ZhCn => &ZH_CN,
        }
    }
}

/// The UI language used by `t`.
pub fn language() -> Language {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

pub fn set_language(language: Language) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = language;
}

fn lookup<'a>(bundle: &'a Value, key: &str) -> Option<&'a str> {
    let mut node = bundle;
    for part in key.split('.') {
        node = node.get(part)?;
    }
    node.as_str()
}

fn resolve(language: Language, key: &str, count: Option<i64>) -> Option<&'static str> {
    let bundles = [language.bundle(), Language::En.bundle()];
    for bundle in bundles {
        if let Some(count) = count {
            let suffix = if count == 1 { "_one" } else { "_other" };
            if let Some(value) = lookup(bundle, &format!("{key}{suffix}")) {
                return Some(value);
            }
            // Chinese has a single plural form.
            if let Some(value) = lookup(bundle, &format!("{key}_other")) {
                return Some(value);
            }
        }
        if let Some(value) = lookup(bundle, key) {
            return Some(value);
        }
    }
    None
}

fn interpolate(template: &str, params: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = after[..end].trim();
        // Formatting hints after a comma ("{{count, number}}") are ignored.
        let name = name.split(',').next().unwrap_or(name).trim();
        match params.iter().find(|(k, _)| *k == name) {
            Some((_, value)) => out.push_str(value),
            None => out.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Translates `key` in the current UI language. Missing keys render as the key.
pub fn t(key: &str) -> String {
    t_with(key, &[])
}

/// Translates `key` with interpolation parameters; a `count` parameter also
/// selects the plural form.
pub fn t_with(key: &str, params: &[(&str, &str)]) -> String {
    t_in(language(), key, params)
}

pub fn t_in(language: Language, key: &str, params: &[(&str, &str)]) -> String {
    let count = params
        .iter()
        .find(|(k, _)| *k == "count")
        .and_then(|(_, v)| v.parse::<i64>().ok());
    match resolve(language, key, count) {
        Some(template) => interpolate(template, params),
        None => key.to_string(),
    }
}

/// Like `t`, but returns `None` for missing keys.
pub fn try_t(key: &str) -> Option<String> {
    resolve(language(), key, None).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_nested_keys_in_both_languages() {
        assert_eq!(t_in(Language::En, "sidebar.home", &[]), "Home");
        assert_ne!(t_in(Language::ZhCn, "sidebar.home", &[]), "sidebar.home");
    }

    #[test]
    fn missing_keys_fall_back_to_key() {
        assert_eq!(t_in(Language::En, "no.such.key", &[]), "no.such.key");
    }

    #[test]
    fn interpolates_and_pluralizes() {
        assert_eq!(
            interpolate("Hello {{name}}!", &[("name", "Ada")]),
            "Hello Ada!"
        );
        assert_eq!(interpolate("{{missing}}", &[]), "{{missing}}");
        let one = t_in(
            Language::En,
            "settings.privacy.retentionDays",
            &[("count", "1")],
        );
        let many = t_in(
            Language::En,
            "settings.privacy.retentionDays",
            &[("count", "30")],
        );
        assert!(one.contains('1'), "{one}");
        assert!(many.contains("30"), "{many}");
        assert_ne!(one.replace('1', ""), many.replace("30", ""));
    }
}
