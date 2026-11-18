//! Translations, read from the bundled JSON locale files.
//!
//! Supports nested keys addressed with dots, `{{name}}` interpolation,
//! `_one`/`_other` plurals selected by a `count` parameter, and English
//! fallback for keys missing from a locale.

use std::sync::{LazyLock, RwLock};

use serde_json::Value;

static EN: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../locales/en.json")));
static ZH_CN: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../locales/zh-CN.json")));
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

fn lookup_form<'a>(bundle: &'a Value, key: &str, count: Option<i64>) -> Option<&'a str> {
    if count == Some(1)
        && let Some(value) = lookup(bundle, &format!("{key}_one"))
    {
        return Some(value);
    }
    if count.is_some()
        && let Some(value) = lookup(bundle, &format!("{key}_other"))
    {
        return Some(value);
    }
    lookup(bundle, key)
}

fn resolve(language: Language, key: &str, count: Option<i64>) -> Option<&'static str> {
    if language != Language::En
        && let Some(value) = lookup_form(language.bundle(), key, count)
    {
        return Some(value);
    }
    lookup_form(Language::En.bundle(), key, count)
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
        assert_eq!(t_in(Language::En, "sidebar.home", &[]), "History");
        assert_eq!(t_in(Language::ZhCn, "sidebar.home", &[]), "历史记录");
    }

    #[test]
    fn catalogs_have_matching_keys_and_interpolation_parameters() {
        use std::collections::BTreeSet;

        let mut keys = BTreeSet::new();
        for bundle in [Language::En.bundle(), Language::ZhCn.bundle()] {
            let mut stack = vec![(String::new(), bundle)];
            while let Some((path, node)) = stack.pop() {
                if let Some(children) = node.as_object() {
                    for (name, child) in children {
                        let key = if path.is_empty() {
                            name.clone()
                        } else {
                            format!("{path}.{name}")
                        };
                        stack.push((key, child));
                    }
                } else {
                    let template = node.as_str().expect("expected a translation string");
                    assert!(!template.trim().is_empty(), "empty translation for {path}");
                    let key = path
                        .strip_suffix("_one")
                        .or_else(|| path.strip_suffix("_other"))
                        .unwrap_or(&path);
                    keys.insert(key.to_owned());
                }
            }
        }

        let parameter = regex::Regex::new(r"\{\{\s*([^,}\s]+)\s*(?:,[^}]*)?\}\}").unwrap();
        let parameters = |template: &str| {
            parameter
                .captures_iter(template)
                .map(|capture| capture[1].to_owned())
                .collect::<BTreeSet<_>>()
        };
        let en = Language::En.bundle();
        let zh = Language::ZhCn.bundle();
        for key in keys {
            let base = lookup(en, &key);
            let one = lookup(en, &format!("{key}_one"));
            let other = lookup(en, &format!("{key}_other"));
            assert!(
                base.is_some() || one.is_some() || other.is_some(),
                "missing English key {key}"
            );
            assert_eq!(
                one.is_some(),
                other.is_some(),
                "incomplete English plural forms for {key}"
            );
            for (count, english) in [
                (None, base),
                (Some(1), one.or(base)),
                (Some(2), other.or(base)),
            ] {
                if let Some(english) = english {
                    let chinese = lookup_form(zh, &key, count)
                        .unwrap_or_else(|| panic!("missing Chinese key {key} for count {count:?}"));
                    assert_eq!(
                        parameters(english),
                        parameters(chinese),
                        "parameters for {key} with count {count:?}"
                    );
                }
            }
        }
    }

    /// Verify literal keys exist locally. For `t_with`, allow a plural form
    /// because this scan does not inspect the interpolation arguments.
    #[test]
    fn every_literal_key_resolves() {
        let pattern = regex::Regex::new(r#"\b(t|t_with|try_t)\(\s*"([A-Za-z][\w.]*)""#).unwrap();
        let mut missing = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src"
        ))];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                for capture in pattern.captures_iter(&source) {
                    let key = &capture[2];
                    if !key.contains('.') {
                        continue;
                    }
                    for language in [Language::En, Language::ZhCn] {
                        let count = (&capture[1] == "t_with").then_some(2);
                        let found = lookup_form(language.bundle(), key, count).is_some();
                        if !found {
                            missing.push(format!("{language:?} {key} ({})", path.display()));
                        }
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "missing locale keys:\n{}",
            missing.join("\n")
        );
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
        for (language, count, expected) in [
            (Language::En, "1", "1 day"),
            (Language::En, "30", "30 days"),
            (Language::ZhCn, "1", "1 天"),
            (Language::ZhCn, "30", "30 天"),
        ] {
            assert_eq!(
                t_in(
                    language,
                    "settings.privacy.retentionDays",
                    &[("count", count)]
                ),
                expected
            );
        }
    }
}
