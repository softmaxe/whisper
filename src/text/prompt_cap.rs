//! Fits the dictionary hint list into the Whisper prompt budget.

/// Whisper-family decoders read at most 223 prompt tokens and silently keep
/// the TAIL of anything longer. 900 chars bounds the request, while
/// [`fit_dictionary_prompt`] decides which words land in the kept tail.
pub const WHISPER_PROMPT_CHARS: usize = 900;

/// What the decoder's 223-token window is worth in characters for a
/// comma-separated list of names (~2.3-2.9 chars/token). Used to warn in the
/// UI, not to trim.
pub const WHISPER_DECODER_PROMPT_CHARS: usize = 550;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FittedPrompt {
    pub prompt: Option<String>,
    pub omitted: usize,
}

/// Fills the budget from `priority_words` (most important first), skipping any
/// entry that would overflow so none is sent half-spelled, then joins the
/// picked entries least important first: decoders keep the tail of a long
/// prompt. Lengths are UTF-16 units. Callers must
/// classify dictionary echoes against the returned prompt.
pub fn fit_dictionary_prompt<S: AsRef<str>>(
    priority_words: &[S],
    max_chars: usize,
) -> FittedPrompt {
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
    let omitted = priority_words.len() - picked.len();
    picked.reverse();
    FittedPrompt {
        prompt: (!picked.is_empty()).then(|| picked.join(", ")),
        omitted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_important_word_last() {
        assert_eq!(
            fit_dictionary_prompt(&["first", "second", "third"], 900),
            FittedPrompt {
                prompt: Some("third, second, first".into()),
                omitted: 0
            }
        );
    }

    #[test]
    fn drops_overflowing_words_whole() {
        let result = fit_dictionary_prompt(&["alpha", "beta", "gamma-long", "pi"], 15);
        assert_eq!(
            result,
            FittedPrompt {
                prompt: Some("pi, beta, alpha".into()),
                omitted: 1
            }
        );
        assert!(result.prompt.unwrap().len() <= 15);
    }

    #[test]
    fn no_words_no_prompt() {
        let empty: [&str; 0] = [];
        assert_eq!(
            fit_dictionary_prompt(&empty, 900),
            FittedPrompt {
                prompt: None,
                omitted: 0
            }
        );
    }
}
