//! Learns dictionary terms from a user's edits to pasted transcription text.
//!
//! Lengths are counted in UTF-16 units.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::text::script::{is_han, is_hiragana, is_katakana};
use crate::text::word_segment::word_segments;

// Swapping one everyday word for another ("why" -> "what") is a content edit,
// not vocabulary; learning it would only pad the cleanup prompt. Words under
// three letters are already dropped, so none are listed here.
const COMMON_WORDS_SOURCE: &str =
    "the and for not with you this but his from they say her she will one all would
  there their what out about who get which when make can like time just him know
  take into year your good some could them see other than then now look only come
  over think also back after use two how our work first well way even new want
  because any these give day most are was were been has had did does said went
  made got came took saw knew thought where why here very much many still too again
  off down never every own same another both each few more less last next while
  before through under between should might must being have that its yes okay
  这个 那个 这些 那些 这样 那样 我们 你们 他们 她们 什么 怎么 为什么 因为 所以 但是
  可以 已经 现在 然后 就是 还是 如果 没有 一个 一下 知道 觉得 感觉 需要 应该 时候";

static COMMON_WORDS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| COMMON_WORDS_SOURCE.split_whitespace().collect());

static HAS_LETTER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\p{L}").expect("static regex"));

// A dictionary entry is a name or term, not a clause: longer replacements are
// rewrites even when the rest of the sentence survives.
const MAX_TERM_UNITS: usize = 4;
const MAX_REPLACED_UNITS: usize = 8;

// Scripts written without spaces between words. Spelling distance means
// nothing for their mishearings ("克劳德" -> "Claude"), so they skip that check.
fn is_unspaced_script(c: char) -> bool {
    is_han(c) || is_hiragana(c) || is_katakana(c)
}

fn is_unspaced_script_only(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| is_unspaced_script(c) || c == 'ー' || c.is_whitespace())
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Levenshtein distance over UTF-16 code units.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            cur[j] = if a[i - 1] == b[j - 1] {
                prev[j - 1]
            } else {
                1 + prev[j].min(cur[j - 1]).min(prev[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[derive(Debug, Clone)]
struct Unit {
    text: String,
    key: String,
    /// Byte offsets into the source text.
    start: usize,
    end: usize,
}

/// Word units with source offsets, so a changed run can be sliced back out
/// with its original spacing ("Claude Code"). Unspaced scripts are split
/// into dictionary words.
fn tokenize(text: &str) -> Vec<Unit> {
    word_segments(text)
        .into_iter()
        .map(|seg| {
            let segment = &text[seg.start..seg.end];
            Unit {
                text: segment.to_string(),
                key: segment.to_lowercase(),
                start: seg.start,
                end: seg.end,
            }
        })
        .collect()
}

/// The region of `field_value` that corresponds to the pasted text. A field
/// holding only the pasted text is returned as-is.
fn find_edited_region<'a>(original_text: &'a str, field_value: &'a str) -> &'a str {
    if utf16_len(field_value) as f64 <= utf16_len(original_text) as f64 * 1.5 {
        return field_value;
    }
    if field_value.contains(original_text) {
        return original_text;
    }

    // Sliding window: the region with the highest word overlap.
    let orig_units = tokenize(original_text);
    let field_units = tokenize(field_value);
    let window = orig_units.len();
    if field_units.len() <= window || window == 0 {
        return field_value;
    }

    let mut best_start = 0;
    let mut best_score: i64 = -1;
    for i in 0..=field_units.len() - window {
        let matches = (0..window)
            .filter(|&j| field_units[i + j].key == orig_units[j].key)
            .count() as i64;
        if matches > best_score {
            best_score = matches;
            best_start = i;
        }
    }
    // Require at least 30% word overlap to consider it a match.
    if (best_score as f64) < window as f64 * 0.3 {
        return field_value;
    }
    &field_value[field_units[best_start].start..field_units[best_start + window - 1].end]
}

#[derive(Debug, Default)]
struct Hunk {
    removed: Vec<Unit>,
    added: Vec<Unit>,
}

struct Diff {
    kept_units: usize,
    kept_chars: usize,
    hunks: Vec<Hunk>,
}

/// Word-level LCS diff: units kept (and their characters) plus the changed
/// runs of removed original units and added edited units.
fn diff_units(orig: &[Unit], edited: &[Unit]) -> Diff {
    let (m, n) = (orig.len(), edited.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if orig[i - 1].key == edited[j - 1].key {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut kept_chars = 0;
    let mut current: Option<Hunk> = None;
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && orig[i - 1].key == edited[j - 1].key {
            if let Some(h) = current.take() {
                hunks.push(h);
            }
            kept_chars += utf16_len(&orig[i - 1].text);
            i -= 1;
            j -= 1;
            continue;
        }
        let hunk = current.get_or_insert_with(Hunk::default);
        if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            hunk.added.insert(0, edited[j - 1].clone());
            j -= 1;
        } else {
            hunk.removed.insert(0, orig[i - 1].clone());
            i -= 1;
        }
    }
    if let Some(h) = current.take() {
        hunks.push(h);
    }
    // Hunks were collected back to front.
    hunks.reverse();
    Diff {
        kept_units: dp[m][n],
        kept_chars,
        hunks,
    }
}

fn slice_units<'a>(text: &'a str, units: &[Unit]) -> &'a str {
    match (units.first(), units.last()) {
        (Some(first), Some(last)) => &text[first.start..last.end],
        _ => "",
    }
}

/// Whether a replaced run looks like a misheard term rather than a content edit.
fn is_learnable_term(replaced: &str, term: &str, term_units: &[Unit]) -> bool {
    if !HAS_LETTER_RE.is_match(term) {
        return false;
    }
    if term_units
        .iter()
        .all(|u| COMMON_WORDS.contains(u.key.as_str()))
    {
        return false;
    }
    if is_unspaced_script_only(term) {
        // The segmenter's dictionary already knows a single-unit term (认为,
        // 说得), so it is ordinary vocabulary or a grammar fix. Misheard names
        // fall outside that dictionary and split into several units (章|珊).
        return term_units.len() >= 2;
    }
    if replaced.chars().any(is_unspaced_script) {
        // A Latin term replacing an unspaced-script mishearing ("爱爱" -> "AI").
        return utf16_len(term) >= 2;
    }
    if utf16_len(term) < 3 {
        return false;
    }
    // 0.65 allows phonetic corrections like "Shunade" -> "Sinead" (4/7) while
    // filtering out unrelated word replacements.
    let a = replaced.to_lowercase();
    let b = term.to_lowercase();
    let longest = utf16_len(&a).max(utf16_len(&b)).max(1);
    edit_distance(&a, &b) as f64 / longest as f64 <= 0.65
}

/// Extract corrected terms from a user's edits to pasted transcription text,
/// skipping words already in `existing_dictionary` (case-insensitively).
pub fn extract_corrections<S: AsRef<str>>(
    original_text: &str,
    field_value: &str,
    existing_dictionary: &[S],
) -> Vec<String> {
    if original_text.is_empty() || field_value.is_empty() || original_text == field_value {
        return Vec::new();
    }
    let edited_region = find_edited_region(original_text, field_value);
    if edited_region == original_text {
        return Vec::new();
    }

    let orig_units = tokenize(original_text);
    let edited_units = tokenize(edited_region);
    if orig_units.is_empty() || edited_units.is_empty() {
        return Vec::new();
    }

    // Mostly changed means a rewrite, not corrections. Counting characters as
    // well as words keeps a misheard name in an unspaced script, which splits
    // into one unit per character, from outweighing a short sentence.
    let diff = diff_units(&orig_units, &edited_units);
    let total_chars: usize = orig_units.iter().map(|u| utf16_len(&u.text)).sum();
    if diff.kept_units * 2 < orig_units.len() && diff.kept_chars * 3 < total_chars {
        return Vec::new();
    }

    let dict: HashSet<String> = existing_dictionary
        .iter()
        .map(|w| w.as_ref().to_lowercase())
        .collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut results = Vec::new();

    for Hunk { removed, added } in &diff.hunks {
        // Pure insertions and deletions add or drop content; only a
        // replacement pairs a misheard run with its correction.
        if removed.is_empty() || added.is_empty() {
            continue;
        }
        if added.len() > MAX_TERM_UNITS || removed.len() > MAX_REPLACED_UNITS {
            continue;
        }
        let replaced = slice_units(original_text, removed);
        let term = slice_units(edited_region, added);
        let normalized = term.to_lowercase();
        if dict.contains(&normalized) || seen.contains(&normalized) {
            continue;
        }
        if replaced.to_lowercase() == normalized || !is_learnable_term(replaced, term, added) {
            continue;
        }
        results.push(term.to_string());
        seen.insert(normalized);
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: [&str; 0] = [];

    fn ex(original: &str, edited: &str) -> Vec<String> {
        extract_corrections(original, edited, &NONE)
    }

    #[test]
    fn empty_and_identical_inputs() {
        assert!(ex("", "hello").is_empty());
        assert!(ex("hello", "").is_empty());
        assert!(ex("hello world", "hello world").is_empty());
    }

    #[test]
    fn learns_phonetic_mishearing() {
        assert!(ex("Hey Shunade how are you", "Hey Sinead how are you").contains(&"Sinead".into()));
        assert!(ex("Hey Shunade", "Hey Sinead").contains(&"Sinead".into()));
    }

    #[test]
    fn skips_existing_dictionary_words() {
        let (o, e) = ("Hey Shunade how are you", "Hey Sinead how are you");
        assert!(!extract_corrections(o, e, &["Sinead"]).contains(&"Sinead".into()));
        assert!(!extract_corrections(o, e, &["sinead"]).contains(&"Sinead".into()));
    }

    #[test]
    fn rewrite_is_not_corrections() {
        assert!(ex("the cat sat on the mat", "a dog stood under a rug").is_empty());
    }

    #[test]
    fn short_and_unrelated_replacements_ignored() {
        assert!(!ex("I went to see XX today", "I went to see Al today").contains(&"Al".into()));
        assert!(
            !ex("I saw a cat yesterday", "I saw a elephant yesterday").contains(&"elephant".into())
        );
    }

    #[test]
    fn everyday_word_swaps_ignored() {
        assert!(ex("This is why I'm speaking", "This is what I'm speaking").is_empty());
        assert!(ex("This is why I'm speaking", "This is where I'm speaking").is_empty());
    }

    #[test]
    fn same_correction_learned_once() {
        let result = ex("Shunade said hi to Shunade", "Sinead said hi to Sinead");
        assert!(
            result
                .iter()
                .filter(|w| w.to_lowercase() == "sinead")
                .count()
                <= 1
        );
    }

    #[test]
    fn latin_term_replacing_chinese_transliteration() {
        assert_eq!(
            ex(
                "我们今天用克劳德扣的写代码。",
                "我们今天用Claude Code写代码。"
            ),
            vec!["Claude Code"]
        );
        assert_eq!(
            ex("克劳德扣的写代码", "Claude Code写代码"),
            vec!["Claude Code"]
        );
        assert_eq!(ex("用爱爱写代码", "用AI写代码"), vec!["AI"]);
    }

    #[test]
    fn misheard_chinese_name() {
        assert_eq!(ex("明天和张三开会", "明天和章珊开会"), vec!["章珊"]);
    }

    #[test]
    fn unspaced_english_inside_chinese() {
        assert_eq!(
            ex("我在用wis per做听写", "我在用Whisper做听写"),
            vec!["Whisper"]
        );
        assert_eq!(
            ex("我们用 cloud code 写代码", "我们用 Claude code 写代码"),
            vec!["Claude"]
        );
    }

    #[test]
    fn chinese_grammar_fixes_ignored() {
        assert!(ex("他说的很好", "他说得很好").is_empty());
        assert!(ex("我觉得这个方案可以", "我认为这个方案可以").is_empty());
    }

    #[test]
    fn single_chinese_character_fix_not_learned() {
        assert!(ex("我今天和张山一起吃饭", "我今天和张珊一起吃饭").is_empty());
    }

    #[test]
    fn rewritten_chinese_sentence_ignored() {
        assert!(ex("今天天气很好我们去公园", "明天下雨大家待在家里").is_empty());
    }

    #[test]
    fn learns_from_pasted_region_only() {
        let prefix = "Earlier chat content that was already in the field before dictation. ";
        assert_eq!(
            ex(
                "部署到乌班图服务器上",
                &format!("{prefix}部署到Ubuntu服务器上")
            ),
            vec!["Ubuntu"]
        );
    }

    #[test]
    fn edit_distance_counts_utf16_units() {
        assert_eq!(edit_distance("shunade", "sinead"), 4);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
