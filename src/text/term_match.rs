//! Finds dictionary terms in text, for usage counts and known corrections.

use std::ops::Range;

use crate::text::script::is_unspaced_script;

/// Bytes of `text` matched by the lowercased `needle`, if it starts there.
fn match_len(text: &str, needle: &[char]) -> Option<usize> {
    let mut matched = 0;
    for (offset, c) in text.char_indices() {
        for lower in c.to_lowercase() {
            if needle.get(matched) != Some(&lower) {
                return None;
            }
            matched += 1;
        }
        if matched == needle.len() {
            return Some(offset + c.len_utf8());
        }
    }
    None
}

/// A word in a script written without spaces is not part of a Latin word next
/// to it ("用Claude写").
fn continues_word(c: char) -> bool {
    (c.is_alphanumeric() || c == '_') && !is_unspaced_script(c)
}

/// Case-insensitive, non-overlapping occurrences of `term` in `text`. A term
/// in an unspaced script matches anywhere, since it sits inside a longer run
/// of letters; any other term matches whole words only.
pub fn find_term(text: &str, term: &str) -> Vec<Range<usize>> {
    let needle: Vec<char> = term.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let whole_word = !needle.iter().copied().any(is_unspaced_script);
    let mut found: Vec<Range<usize>> = Vec::new();
    for (start, _) in text.char_indices() {
        if found.last().is_some_and(|last| start < last.end) {
            continue;
        }
        let Some(len) = match_len(&text[start..], &needle) else {
            continue;
        };
        let end = start + len;
        if whole_word
            && (text[..start]
                .chars()
                .next_back()
                .is_some_and(continues_word)
                || text[end..].chars().next().is_some_and(continues_word))
        {
            continue;
        }
        found.push(start..end);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches<'a>(text: &'a str, term: &str) -> Vec<&'a str> {
        find_term(text, term)
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    #[test]
    fn matches_whole_words_ignoring_case() {
        assert_eq!(
            matches("Rust trusted RUST rust_x", "rust"),
            ["Rust", "RUST"]
        );
        assert_eq!(matches("ship C++ today", "c++"), ["C++"]);
    }

    #[test]
    fn latin_terms_match_next_to_unspaced_scripts() {
        assert_eq!(matches("用Claude写代码", "claude"), ["Claude"]);
    }

    #[test]
    fn unspaced_terms_match_inside_longer_runs() {
        assert_eq!(matches("发给张珊，张珊回了", "张珊"), ["张珊", "张珊"]);
    }

    #[test]
    fn an_empty_term_matches_nothing() {
        assert!(matches("anything", "").is_empty());
    }
}
