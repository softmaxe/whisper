//! Detects Whisper echoing the dictionary prompt back instead of transcribing.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

static NON_WORD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}\s]").expect("static regex"));
static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("static regex"));

fn normalize(s: &str) -> String {
    let lowered = s.to_lowercase();
    let stripped = NON_WORD_RE.replace_all(&lowered, "");
    WHITESPACE_RE.replace_all(&stripped, " ").trim().to_string()
}

pub fn matches_dictionary_prompt(text: &str, dictionary_prompt: &str) -> bool {
    if text.is_empty() || dictionary_prompt.is_empty() {
        return false;
    }
    let normalized_text = normalize(text);
    let normalized_prompt = normalize(dictionary_prompt);
    if normalized_text.is_empty() || normalized_prompt.is_empty() {
        return false;
    }
    if normalized_text == normalized_prompt {
        return true;
    }

    let dict_words: HashSet<&str> = normalized_prompt.split(' ').collect();
    let unique_text_words: HashSet<&str> = normalized_text.split(' ').collect();
    let match_count = unique_text_words
        .iter()
        .filter(|w| dict_words.contains(*w))
        .count() as f64;

    let text_composition = match_count / unique_text_words.len() as f64;
    let dictionary_usage = match_count / dict_words.len() as f64;
    text_composition >= 0.9 && dictionary_usage >= 0.7
}

#[cfg(test)]
mod tests {
    use super::*;

    const DICT: &str = "Whisper, Parakeet, Alcahest";

    #[test]
    fn verbatim_echo() {
        assert!(matches_dictionary_prompt(DICT, DICT));
        assert!(matches_dictionary_prompt(
            "Whisper, Parakeet, Alcahest.",
            DICT
        ));
        assert!(matches_dictionary_prompt(
            "whisper, parakeet, alcahest",
            DICT
        ));
        assert!(matches_dictionary_prompt("Whisper Parakeet Alcahest", DICT));
        assert!(matches_dictionary_prompt(
            "Whisper,  Parakeet,  Alcahest",
            DICT
        ));
    }

    #[test]
    fn legitimate_speech_is_kept() {
        assert!(!matches_dictionary_prompt(
            "I just installed Whisper and it works great",
            DICT
        ));
        assert!(!matches_dictionary_prompt("Whisper, Parakeet", DICT));
        assert!(!matches_dictionary_prompt(
            "The quick brown fox jumps over the lazy dog",
            DICT
        ));
    }

    #[test]
    fn empty_inputs() {
        assert!(!matches_dictionary_prompt("some text", ""));
        assert!(!matches_dictionary_prompt("", "Whisper"));
        assert!(!matches_dictionary_prompt("", ""));
    }

    #[test]
    fn single_word_dictionary() {
        assert!(matches_dictionary_prompt("Whisper", "Whisper"));
        assert!(!matches_dictionary_prompt("Whisper is great", "Whisper"));
    }

    #[test]
    fn unicode_and_cjk() {
        assert!(matches_dictionary_prompt(
            "Müller, François, José",
            "Müller, François, José"
        ));
        assert!(!matches_dictionary_prompt(
            "muller francois jose",
            "Müller, François, José"
        ));
        assert!(matches_dictionary_prompt("東京, 大阪", "東京, 大阪"));
    }

    #[test]
    fn looped_and_filler_echoes() {
        let repeated = "Whisper, Parakeet, Alcahest, Whisper, Parakeet, Alcahest";
        assert!(matches_dictionary_prompt(repeated, DICT));
        let dict = "Alpha, Bravo, Charlie, Delta, Echo, Foxtrot, Golf, Hotel, India, Juliet";
        let echo = "Alpha Bravo Charlie Delta Echo Foxtrot Golf Hotel India Juliet the";
        assert!(matches_dictionary_prompt(echo, dict));
    }

    #[test]
    fn punctuation_only_never_matches() {
        assert!(!matches_dictionary_prompt("...", "..."));
        assert!(!matches_dictionary_prompt("!!!", ",,,"));
        assert!(!matches_dictionary_prompt("???", "Whisper, Parakeet"));
        assert!(!matches_dictionary_prompt("Whisper, Parakeet", "!!!"));
    }
}
