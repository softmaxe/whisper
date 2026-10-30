//! Fits the dictionary hint list into the Whisper prompt budget.

/// Whisper-family decoders read at most 223 prompt tokens and silently keep
/// the TAIL of anything longer. 900 chars bounds the request, while
/// [`fit_dictionary_prompt`] decides which words land in the kept tail.
pub const WHISPER_PROMPT_CHARS: usize = 900;

/// What the decoder's 223-token window is worth in characters for a
/// comma-separated list of names (~2.3-2.9 chars/token). Used to warn in the
/// UI, not to trim.
pub const WHISPER_DECODER_PROMPT_CHARS: usize = 550;

/// Fills the budget from `priority_words` (most important first), skipping any
/// entry that would overflow so none is sent half-spelled, then joins the
/// picked entries least important first: decoders keep the tail of a long
/// prompt. Lengths are UTF-16 units; `None` when no word fits. Callers must
/// classify dictionary echoes against the returned prompt.
pub fn fit_dictionary_prompt<S: AsRef<str>>(
    priority_words: &[S],
    max_chars: usize,
) -> Option<String> {
    let mut picked: Vec<&str> = Vec::new();
    let mut length = 0usize;
    for word in priority_words {
        let word = word.as_ref();
        if word.is_empty() {
            continue;
        }
        let cost = word.encode_utf16().count() + if picked.is_empty() { 0 } else { 2 };
        if length + cost > max_chars {
            continue;
        }
        picked.push(word);
        length += cost;
    }
    picked.reverse();
    (!picked.is_empty()).then(|| picked.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_important_word_last() {
        assert_eq!(
            fit_dictionary_prompt(&["first", "second", "third"], 900).as_deref(),
            Some("third, second, first")
        );
    }

    #[test]
    fn drops_overflowing_words_whole() {
        let result = fit_dictionary_prompt(&["alpha", "beta", "gamma-long", "pi"], 15);
        assert_eq!(result.as_deref(), Some("pi, beta, alpha"));
        assert!(result.unwrap().len() <= 15);
    }

    #[test]
    fn no_words_no_prompt() {
        let empty: [&str; 0] = [];
        assert_eq!(fit_dictionary_prompt(&empty, 900), None);
    }
}
