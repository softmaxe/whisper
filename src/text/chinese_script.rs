//! Chinese script preference helpers for Whisper-family STT.
//!
//! Whisper only exposes "zh", so zh-CN, zh-TW and auto share one STT language
//! hint; the Simplified/Traditional choice is applied deterministically after
//! transcription. An explicit zh-CN or zh-TW also biases the Whisper prompt;
//! auto never does, because the bias would skew the language detection it
//! depends on.

use std::sync::OnceLock;

use ferrous_opencc::OpenCC;
use ferrous_opencc::config::BuiltinConfig;

use crate::settings::ChineseScriptPreference;
use crate::text::script::is_han;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChineseScriptTarget {
    Simplified,
    Traditional,
}

// Kana and Hangul never appear in Chinese, so either one rules the text out.
fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30ff}' | '\u{31f0}'..='\u{31ff}' | '\u{ff66}'..='\u{ff9f}')
}

fn is_hangul(c: char) -> bool {
    matches!(c, '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}' | '\u{ac00}'..='\u{d7af}')
}

// Auto mode must be conservative: Han is shared by Chinese and Japanese. These
// signals use forms that differ from modern Japanese. Shared forms such as 開,
// 電, 設定 and 資料 are deliberately absent.
const SIMPLIFIED_CHINESE_VARIANTS: &str = "这们吗简软网语汉习说发东车门问间书见长爱据实认让给还过边达选进运远违连迟适应际标亲亿优仅从众气请谢听读卖产业电备复历压类总处线证验权转导报记试计机传云丰动务场图库录页价关规办觉坏变删乐广设";
const TRADITIONAL_CHINESE_VARIANTS: &str = "這們嗎體說發據實讓邊遲應氣條聽讀寫賣兒經廣樂觀號國學會產歷壓總處證驗權轉傳豐圖錄價關辦覺壞變刪";

/// Whether text has positive evidence of being Chinese rather than merely
/// containing Han. Kana/Hangul rule it out; text with no Chinese-specific
/// variants stays ambiguous and is left untouched.
pub fn is_chinese_text(text: &str) -> bool {
    if text.is_empty() || text.chars().any(|c| is_kana(c) || is_hangul(c)) {
        return false;
    }
    text.chars().any(|c| {
        SIMPLIFIED_CHINESE_VARIANTS.contains(c) || TRADITIONAL_CHINESE_VARIANTS.contains(c)
    })
}

/// Resolve the script target from the preferred language and the auto-detect
/// preference. zh-CN / zh-TW always win. On auto (or empty) the preference
/// applies only to text that looks Chinese. Pass `None` for `text` when no
/// transcript exists yet (prompt building): then only an explicit language counts.
pub fn resolve_chinese_script_target(
    preferred_language: &str,
    preference: ChineseScriptPreference,
    text: Option<&str>,
) -> Option<ChineseScriptTarget> {
    match preferred_language {
        "zh-CN" => return Some(ChineseScriptTarget::Simplified),
        "zh-TW" => return Some(ChineseScriptTarget::Traditional),
        _ => {}
    }
    let is_auto = preferred_language.is_empty() || preferred_language == "auto";
    if is_auto && text.is_some_and(is_chinese_text) {
        return match preference {
            ChineseScriptPreference::Simplified => Some(ChineseScriptTarget::Simplified),
            ChineseScriptPreference::Traditional => Some(ChineseScriptTarget::Traditional),
            ChineseScriptPreference::AsTranscribed => None,
        };
    }
    None
}

/// Language code passed to cleanup prompts. Auto stays auto: naming zh-CN or
/// zh-TW would make cleanup answer in Chinese before the language is known.
pub fn resolve_cleanup_language(preferred_language: &str) -> String {
    if !preferred_language.is_empty() && preferred_language != "auto" {
        preferred_language.to_string()
    } else {
        "auto".to_string()
    }
}

/// Short Whisper prompt bias so the model prefers the target character set.
pub fn get_chinese_script_prompt_bias(target: Option<ChineseScriptTarget>) -> Option<&'static str> {
    match target? {
        ChineseScriptTarget::Simplified => Some("以下是简体中文。语言、学习、软件、网络。"),
        ChineseScriptTarget::Traditional => Some("以下是繁體中文。語言、學習、軟體、網路。"),
    }
}

/// Merge dictionary words with an optional Chinese script bias for Whisper prompts.
pub fn merge_whisper_prompt(
    dictionary_prompt: Option<&str>,
    target: Option<ChineseScriptTarget>,
) -> Option<String> {
    let bias = get_chinese_script_prompt_bias(target);
    let dict = dictionary_prompt.map(str::trim).unwrap_or("");
    match (bias, dict.is_empty()) {
        (Some(bias), false) => Some(format!("{bias} {dict}")),
        (Some(bias), true) => Some(bias.to_string()),
        (None, false) => Some(dict.to_string()),
        (None, true) => None,
    }
}

static TO_SIMPLIFIED: OnceLock<Option<OpenCC>> = OnceLock::new();
static TO_TRADITIONAL: OnceLock<Option<OpenCC>> = OnceLock::new();

// Built lazily per direction: loading the dictionaries costs time a user who
// never dictates Chinese should not pay. tw2sp includes Taiwan phrase variants
// (軟體) that plain tw2s misses; s2twp produces them.
fn converter(target: ChineseScriptTarget) -> Option<&'static OpenCC> {
    let (cell, config) = match target {
        ChineseScriptTarget::Simplified => (&TO_SIMPLIFIED, BuiltinConfig::Tw2sp),
        ChineseScriptTarget::Traditional => (&TO_TRADITIONAL, BuiltinConfig::S2twp),
    };
    cell.get_or_init(|| match OpenCC::from_config(config) {
        Ok(cc) => Some(cc),
        Err(err) => {
            log::error!("failed to load OpenCC {config:?}: {err}");
            None
        }
    })
    .as_ref()
}

/// Convert `text` to the target script. Text without Han, or no target, is
/// returned unchanged. If the converter cannot load, the text passes through.
pub fn apply_chinese_script(text: &str, target: Option<ChineseScriptTarget>) -> String {
    let Some(target) = target else {
        return text.to_string();
    };
    if text.is_empty() || !text.chars().any(is_han) {
        return text.to_string();
    }
    match converter(target) {
        Some(cc) => cc.convert(text),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::prompt_cap::{WHISPER_PROMPT_CHARS, fit_dictionary_prompt};
    use ChineseScriptPreference as P;
    use ChineseScriptTarget::{Simplified, Traditional};

    #[test]
    fn explicit_language_overrides_preference() {
        assert_eq!(
            resolve_chinese_script_target("zh-CN", P::Traditional, None),
            Some(Simplified)
        );
        assert_eq!(
            resolve_chinese_script_target("zh-TW", P::Simplified, None),
            Some(Traditional)
        );
        assert_eq!(
            resolve_chinese_script_target("en", P::Simplified, None),
            None
        );
    }

    #[test]
    fn detects_chinese_vs_japanese_korean() {
        for t in [
            "这是简体中文",
            "這是繁體中文",
            "这是 OpenAI 的 API",
            "今天天气很好",
            "今天天氣很好",
        ] {
            assert!(is_chinese_text(t), "{t}");
        }
        for t in [
            "Hello 世界",
            "Meet at 東京駅",
            "東京駅",
            "会議資料",
            "開発資料",
            "設定変更",
            "電気",
            "資料確認",
            "導入試験",
            "会議の資料を確認してください",
            "今日は時間がありますか",
            "한국어 漢字 텍스트",
            "hello world",
            "",
        ] {
            assert!(!is_chinese_text(t), "{t}");
        }
    }

    #[test]
    fn auto_applies_preference_only_to_chinese() {
        assert_eq!(
            resolve_chinese_script_target("auto", P::Simplified, Some("這是中文")),
            Some(Simplified)
        );
        assert_eq!(
            resolve_chinese_script_target("auto", P::Traditional, Some("这是中文")),
            Some(Traditional)
        );
        assert_eq!(
            resolve_chinese_script_target("auto", P::AsTranscribed, Some("这是中文")),
            None
        );
        assert_eq!(
            resolve_chinese_script_target("", P::Simplified, Some("这是中文")),
            Some(Simplified)
        );
        for t in [
            "会議の資料",
            "会議資料",
            "Meet at 東京駅",
            "한국어 漢字",
            "hello world",
        ] {
            assert_eq!(
                resolve_chinese_script_target("auto", P::Simplified, Some(t)),
                None,
                "{t}"
            );
        }
    }

    #[test]
    fn without_text_only_explicit_language_applies() {
        assert_eq!(
            resolve_chinese_script_target("zh-CN", P::AsTranscribed, None),
            Some(Simplified)
        );
        assert_eq!(
            resolve_chinese_script_target("zh-TW", P::AsTranscribed, None),
            Some(Traditional)
        );
        assert_eq!(
            resolve_chinese_script_target("auto", P::Simplified, None),
            None
        );
        assert_eq!(
            resolve_chinese_script_target("auto", P::Traditional, None),
            None
        );
    }

    #[test]
    fn cleanup_language_keeps_auto() {
        assert_eq!(resolve_cleanup_language("auto"), "auto");
        assert_eq!(resolve_cleanup_language(""), "auto");
        assert_eq!(resolve_cleanup_language("zh-CN"), "zh-CN");
        assert_eq!(resolve_cleanup_language("zh-TW"), "zh-TW");
        assert_eq!(resolve_cleanup_language("ja"), "ja");
    }

    #[test]
    fn converts_traditional_to_simplified() {
        assert_eq!(
            apply_chinese_script("這是繁體中文軟體", Some(Simplified)),
            "这是繁体中文软件"
        );
        assert_eq!(apply_chinese_script("網路", Some(Simplified)), "网络");
    }

    #[test]
    fn converts_simplified_to_taiwan_traditional() {
        assert_eq!(
            apply_chinese_script("这是简体中文软件", Some(Traditional)),
            "這是簡體中文軟體"
        );
        assert_eq!(apply_chinese_script("软件", Some(Traditional)), "軟體");
    }

    #[test]
    fn leaves_non_cjk_and_empty_alone() {
        assert_eq!(
            apply_chinese_script("hello world", Some(Simplified)),
            "hello world"
        );
        assert_eq!(apply_chinese_script("", Some(Simplified)), "");
        assert_eq!(apply_chinese_script("mixed 軟體 ok", None), "mixed 軟體 ok");
    }

    #[test]
    fn conversion_is_idempotent() {
        let once = apply_chinese_script("這是繁體中文軟體", Some(Simplified));
        assert_eq!(apply_chinese_script(&once, Some(Simplified)), once);
        for input in ["干净的头发", "他很干练", "面条和方便面", "皇后在后面"] {
            let once = apply_chinese_script(input, Some(Traditional));
            assert_eq!(
                apply_chinese_script(&once, Some(Traditional)),
                once,
                "not idempotent: {input}"
            );
        }
    }

    #[test]
    fn japanese_and_korean_untouched_when_target_none() {
        for t in ["会議の資料を確認してください", "한국어 漢字 텍스트"] {
            let target = resolve_chinese_script_target("auto", P::Simplified, Some(t));
            assert_eq!(target, None);
            assert_eq!(apply_chinese_script(t, target), t);
        }
    }

    #[test]
    fn prompt_bias_and_merge() {
        assert!(
            get_chinese_script_prompt_bias(Some(Simplified))
                .unwrap()
                .contains("简体")
        );
        assert!(
            get_chinese_script_prompt_bias(Some(Traditional))
                .unwrap()
                .contains("繁體")
        );
        assert_eq!(get_chinese_script_prompt_bias(None), None);
        assert!(
            merge_whisper_prompt(Some("foo, bar"), Some(Simplified))
                .unwrap()
                .contains("foo, bar")
        );
        assert!(
            merge_whisper_prompt(None, Some(Simplified))
                .unwrap()
                .contains("简体")
        );
        assert_eq!(
            merge_whisper_prompt(Some("foo"), None).as_deref(),
            Some("foo")
        );
        assert_eq!(merge_whisper_prompt(Some("  "), None), None);
    }

    #[test]
    fn bias_survives_fitted_dictionary() {
        let bias = get_chinese_script_prompt_bias(Some(Simplified)).unwrap();
        let words: Vec<String> = (0..400).map(|i| format!("term{i}")).collect();
        let bias_len = bias.encode_utf16().count();
        let fitted = fit_dictionary_prompt(&words, WHISPER_PROMPT_CHARS - bias_len - 1);
        let prompt = merge_whisper_prompt(fitted.as_deref(), Some(Simplified)).unwrap();
        assert!(prompt.starts_with(bias));
        assert!(prompt.encode_utf16().count() <= WHISPER_PROMPT_CHARS);
        assert!(prompt.ends_with("term0"));
    }
}
