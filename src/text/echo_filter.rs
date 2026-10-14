//! Detects Whisper echoing the dictionary prompt back instead of transcribing.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

static NON_WORD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}\s]").expect("static regex"));
static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("static regex"));

const MAX_SHORT_FRAGMENT_CHARACTERS: usize = 30;
// Whisper's echo pathology loops a term many times; saying a word twice is speech.
const LOOPED_WORD_MIN_OCCURRENCES: usize = 3;
// Dictation almost never recites 3+ dictionary entries in the prompt's own order.
const MIN_CONSECUTIVE_TERM_RUN: usize = 3;

// Whisper continues an initial prompt in the prompt's own shape: the hint list
// is joined with ", ", so a continuation arrives as a looped term, a short
// fragment left dangling on its separator ("testing, "), or a run of
// consecutive entries in the prompt's own order. Vocabulary overlap alone
// cannot stand in for that: short dictation is legitimately spelled out of
// dictionary words, and multi-word snippet triggers put common words into the
// prompt, making plain speech look like an echo.
fn is_prompt_delimiter(c: char) -> bool {
    matches!(c, ',' | '、' | '，')
}

fn has_trailing_delimiter(text: &str) -> bool {
    text.trim_end_matches(char::is_whitespace)
        .chars()
        .next_back()
        .is_some_and(is_prompt_delimiter)
}

/// Error code attached to the "No audio detected" error raised for a
/// discarded dictionary echo, so the pipeline can tell it apart from silence.
pub const DICTIONARY_ECHO_CODE: &str = "DICTIONARY_ECHO";
/// Message used for a discarded echo; matches the genuine no-audio message.
pub const DICTIONARY_ECHO_MESSAGE: &str = "No audio detected";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FragmentAnalysis {
    pub is_prompt_fragment: bool,
    pub has_repeated_words: bool,
    pub unique_word_count: usize,
}

fn normalize(s: &str) -> String {
    let lowered = s.to_lowercase();
    let stripped = NON_WORD_RE.replace_all(&lowered, "");
    WHITESPACE_RE.replace_all(&stripped, " ").trim().to_string()
}

fn has_looped_word(text_words: &[&str]) -> bool {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for word in text_words {
        let count = counts.entry(word).or_insert(0);
        *count += 1;
        if *count >= LOOPED_WORD_MIN_OCCURRENCES {
            return true;
        }
    }
    false
}

// True when the text is MIN_CONSECUTIVE_TERM_RUN+ consecutive prompt entries
// in the prompt's own order: a literal continuation of the hint list.
fn matches_consecutive_term_run(text_words: &[&str], dictionary_prompt: &str) -> bool {
    let mut sequence: Vec<(String, usize)> = Vec::new();
    let mut term_index = 0;
    for term in dictionary_prompt.split(is_prompt_delimiter) {
        let normalized = normalize(term);
        if normalized.is_empty() {
            continue;
        }
        for word in normalized.split(' ') {
            sequence.push((word.to_string(), term_index));
        }
        term_index += 1;
    }
    if text_words.is_empty() || text_words.len() > sequence.len() {
        return false;
    }
    (0..=sequence.len() - text_words.len()).any(|start| {
        let window = &sequence[start..start + text_words.len()];
        window.iter().zip(text_words).all(|((w, _), t)| w == t)
            && window[window.len() - 1].1 - window[0].1 + 1 >= MIN_CONSECUTIVE_TERM_RUN
    })
}

pub fn analyze_dictionary_prompt_fragment(text: &str, dictionary_prompt: &str) -> FragmentAnalysis {
    if text.is_empty() || dictionary_prompt.is_empty() {
        return FragmentAnalysis::default();
    }
    let normalized_text = normalize(text);
    let normalized_prompt = normalize(dictionary_prompt);
    if normalized_text.is_empty() || normalized_prompt.is_empty() {
        return FragmentAnalysis::default();
    }

    let text_words: Vec<&str> = normalized_text.split(' ').collect();
    let prompt_words: HashSet<&str> = normalized_prompt.split(' ').collect();
    let unique_text_words: HashSet<&str> = text_words.iter().copied().collect();
    let has_repeated_words = text_words.len() > unique_text_words.len();
    let match_count = unique_text_words
        .iter()
        .filter(|w| prompt_words.contains(*w))
        .count();

    // Shape tests run against the raw text: normalize() strips the separators.
    // The short-fragment length is in UTF-16 units.
    let continues_prompt_shape = has_looped_word(&text_words)
        || (has_trailing_delimiter(text)
            && normalized_text.encode_utf16().count() <= MAX_SHORT_FRAGMENT_CHARACTERS)
        || (text.contains(is_prompt_delimiter)
            && matches_consecutive_term_run(&text_words, dictionary_prompt));

    FragmentAnalysis {
        is_prompt_fragment: match_count as f64 / unique_text_words.len() as f64 >= 0.9
            && continues_prompt_shape,
        has_repeated_words,
        unique_word_count: unique_text_words.len(),
    }
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

    #[test]
    fn fragment_analysis_shapes() {
        let prompt = "Rust, compiler, testing, on my way, Supabase";
        // Plain speech made of dictionary words is not a fragment.
        assert!(!analyze_dictionary_prompt_fragment("Rust compiler", prompt).is_prompt_fragment);
        assert!(!analyze_dictionary_prompt_fragment("on my way", prompt).is_prompt_fragment);
        // Dangling separator.
        assert!(analyze_dictionary_prompt_fragment("testing, ", prompt).is_prompt_fragment);
        // Looped term.
        let looped = analyze_dictionary_prompt_fragment("testing testing testing", prompt);
        assert!(looped.is_prompt_fragment);
        assert!(looped.has_repeated_words);
        assert_eq!(looped.unique_word_count, 1);
        // Consecutive run of three entries in prompt order.
        assert!(
            analyze_dictionary_prompt_fragment("compiler, testing, on my way", prompt)
                .is_prompt_fragment
        );
        // Out of order is not a run.
        assert!(
            !analyze_dictionary_prompt_fragment("testing, compiler, Rust", prompt)
                .is_prompt_fragment
        );
        assert_eq!(
            analyze_dictionary_prompt_fragment("", prompt),
            FragmentAnalysis::default()
        );
    }
}
