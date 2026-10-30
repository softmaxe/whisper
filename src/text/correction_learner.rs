//! Learns dictionary terms and how they get misheard from a user's edits to
//! pasted transcription text.
//!
//! Lengths are counted in UTF-16 units.

use std::collections::HashSet;
use std::ops::Range;
use std::sync::LazyLock;

use pinyin::ToPinyin;
use regex::Regex;

use crate::text::script::{is_han, is_unspaced_script};
use crate::text::term_match::find_term;
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

/// A field this large (a terminal buffer, a long document) is compared only
/// after trimming what stayed the same; past this it is not worth the memory.
const MAX_DIFF_CELLS: usize = 4_000_000;

// One-character fixes of these are grammar (的 -> 得, 在 -> 再), and they never
// belong to a name.
const FUNCTION_CHARS: &str =
    "的地得了着过在再和与及或是不也都就还很又把被给对向从到这那哪他她它我你们吗呢吧啊";

/// Largest pinyin edit distance, as a share of the longer reading, for two
/// Chinese spellings to count as the same sounds.
const MAX_PINYIN_DISTANCE: f64 = 0.34;

/// A replaced run of the delivered text and what the user changed it to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    pub misheard: String,
    pub term: String,
    /// Too weak on its own: wait for a second sighting before the Dictionary
    /// takes the term.
    pub needs_repeat: bool,
}

/// A stored correction whose term is in the Dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownCorrection {
    pub misheard: String,
    pub term: String,
    /// How many times the user made this correction.
    pub count: i64,
}

impl KnownCorrection {
    /// Whether the delivered text is rewritten without asking the cleanup model.
    pub fn applies_automatically(&self) -> bool {
        self.count >= 2 && is_context_free(&self.misheard, &self.term)
    }
}

/// Whether replacing `misheard` everywhere is safe. A single Latin word may be
/// a real word ("cloud" -> "Claude"), so only the cleanup model, which sees
/// the context, may act on it.
fn is_context_free(misheard: &str, term: &str) -> bool {
    if misheard.to_lowercase() == term.to_lowercase() {
        return utf16_len(misheard) >= 4 && has_inner_capital(term);
    }
    if is_unspaced_script_only(misheard) {
        return misheard.chars().filter(|c| !c.is_whitespace()).count() >= 2;
    }
    misheard.split_whitespace().count() >= 2 || misheard.chars().any(is_unspaced_script)
}

/// Rewrites the corrections safe to apply without context, longest first so a
/// phrase wins over a word inside it. Text already spelled as the term is left
/// alone.
pub fn apply_known_corrections(text: &str, corrections: &[KnownCorrection]) -> String {
    let mut automatic: Vec<&KnownCorrection> = corrections
        .iter()
        .filter(|c| c.applies_automatically())
        .collect();
    automatic.sort_by_key(|c| std::cmp::Reverse(utf16_len(&c.misheard)));

    let mut result = text.to_string();
    for correction in automatic {
        let spelled_right = find_term(&result, &correction.term);
        let hits: Vec<Range<usize>> = find_term(&result, &correction.misheard)
            .into_iter()
            .filter(|hit| {
                !spelled_right
                    .iter()
                    .any(|r| hit.start < r.end && r.start < hit.end)
            })
            .collect();
        for hit in hits.into_iter().rev() {
            result.replace_range(hit, &correction.term);
        }
    }
    result
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

/// Toneless pinyin with the distinctions speech recognition and many speakers
/// blur merged (zh/z, l/n, ang/an). `None` when a character has no reading.
fn fuzzy_pinyin(text: &str) -> Option<String> {
    let mut reading = String::new();
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        let plain = c.to_pinyin()?.plain();
        let initial = [("zh", "z"), ("ch", "c"), ("sh", "s"), ("l", "n")]
            .into_iter()
            .find_map(|(from, to)| plain.strip_prefix(from).map(|rest| (to, rest)));
        let (start, rest) = initial.unwrap_or(("", plain));
        reading.push_str(start);
        let nasal = ["ang", "eng", "ing"]
            .iter()
            .any(|ending| rest.ends_with(ending));
        reading.push_str(if nasal { &rest[..rest.len() - 1] } else { rest });
    }
    Some(reading)
}

/// `None` when the spellings sound different: a mishearing keeps the sound, so
/// that is a content edit. `Some(needs_repeat)` otherwise, with a reading
/// missing (kana) leaving the sound unknown.
fn compare_sound(misheard: &str, term: &str) -> Option<bool> {
    let (Some(a), Some(b)) = (fuzzy_pinyin(misheard), fuzzy_pinyin(term)) else {
        return Some(true);
    };
    let longest = utf16_len(&a).max(utf16_len(&b)).max(1);
    (edit_distance(&a, &b) as f64 / longest as f64 <= MAX_PINYIN_DISTANCE).then_some(false)
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

/// Where the pasted texts sit in the field. An app that reformats a paste
/// (smart quotes, rich text) leaves no exact copy; a field about that long is
/// then taken to be the paste.
fn pasted_regions<S: AsRef<str>>(baseline: &str, pasted: &[S]) -> Vec<Range<usize>> {
    let found: Vec<Range<usize>> = pasted
        .iter()
        .map(AsRef::as_ref)
        .filter(|text| !text.is_empty())
        .filter_map(|text| baseline.rfind(text).map(|start| start..start + text.len()))
        .collect();
    if !found.is_empty() {
        return found;
    }
    let longest = pasted
        .iter()
        .map(|text| utf16_len(text.as_ref()))
        .max()
        .unwrap_or(0);
    if longest > 0 && utf16_len(baseline) as f64 <= longest as f64 * 1.5 {
        std::iter::once(0..baseline.len()).collect()
    } else {
        Vec::new()
    }
}

/// A changed run, as index ranges into the baseline and edited units.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    removed: Range<usize>,
    added: Range<usize>,
}

/// Word-level LCS diff. Units equal but for case stay in step and form hunks
/// of their own, so "github" -> "GitHub" is a change and "github actions" ->
/// "GitHub Actions" is one change.
fn diff_units(baseline: &[Unit], edited: &[Unit]) -> Vec<Hunk> {
    let prefix = baseline
        .iter()
        .zip(edited)
        .take_while(|(a, b)| a.text == b.text)
        .count();
    let suffix = baseline[prefix..]
        .iter()
        .rev()
        .zip(edited[prefix..].iter().rev())
        .take_while(|(a, b)| a.text == b.text)
        .count();
    let orig = &baseline[prefix..baseline.len() - suffix];
    let edit = &edited[prefix..edited.len() - suffix];
    let (m, n) = (orig.len(), edit.len());
    if m.saturating_mul(n) > MAX_DIFF_CELLS {
        return Vec::new();
    }

    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if orig[i - 1].key == edit[j - 1].key {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current: Option<Hunk> = None;
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && orig[i - 1].key == edit[j - 1].key {
            if orig[i - 1].text == edit[j - 1].text {
                hunks.extend(current.take());
            } else {
                let hunk = current.get_or_insert(Hunk {
                    removed: i..i,
                    added: j..j,
                });
                hunk.removed.start = i - 1;
                hunk.added.start = j - 1;
            }
            i -= 1;
            j -= 1;
            continue;
        }
        let hunk = current.get_or_insert(Hunk {
            removed: i..i,
            added: j..j,
        });
        if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            j -= 1;
            hunk.added.start = j;
        } else {
            i -= 1;
            hunk.removed.start = i;
        }
    }
    hunks.extend(current);
    // Hunks were collected back to front.
    hunks.reverse();
    let shift = |r: &Range<usize>| r.start + prefix..r.end + prefix;
    hunks
        .iter()
        .map(|h| Hunk {
            removed: shift(&h.removed),
            added: shift(&h.added),
        })
        .collect()
}

/// Whether the user rewrote this pasted region rather than correcting it.
/// Counting characters as well as words keeps a misheard name in an unspaced
/// script, which splits into one unit per character, from outweighing a short
/// sentence.
fn is_rewritten(region: &Range<usize>, units: &[Unit], hunks: &[Hunk]) -> bool {
    let inside = |unit: &&Unit| region.start <= unit.start && unit.end <= region.end;
    let measure = |units: &[Unit]| {
        units
            .iter()
            .filter(inside)
            .fold((0usize, 0usize), |(count, chars), unit| {
                (count + 1, chars + utf16_len(&unit.text))
            })
    };
    let (total_units, total_chars) = measure(units);
    let (changed_units, changed_chars) = hunks
        .iter()
        .map(|hunk| measure(&units[hunk.removed.clone()]))
        .fold((0usize, 0usize), |(a, b), (c, d)| (a + c, b + d));
    (total_units - changed_units) * 2 < total_units
        && (total_chars - changed_chars) * 3 < total_chars
}

fn slice_units<'a>(text: &'a str, units: &[Unit]) -> &'a str {
    match (units.first(), units.last()) {
        (Some(first), Some(last)) => &text[first.start..last.end],
        _ => "",
    }
}

/// The character of a single-character Han unit that may belong to a name.
fn name_char(unit: &Unit) -> Option<char> {
    let mut chars = unit.text.chars();
    let first = chars.next()?;
    (chars.next().is_none() && is_han(first) && !FUNCTION_CHARS.contains(first)).then_some(first)
}

/// A one-character fix to a name ("张山" -> "张珊") leaves one unit of each
/// side, which says nothing. Takes in the unchanged character next to it when
/// the two characters sound the same.
fn widen_name_fix(hunk: &Hunk, baseline: &[Unit], edited: &[Unit]) -> Option<Hunk> {
    let ([from], [to]) = (&baseline[hunk.removed.clone()], &edited[hunk.added.clone()]) else {
        return None;
    };
    let (from_char, to_char) = (name_char(from)?, name_char(to)?);
    if fuzzy_pinyin(&from_char.to_string())? != fuzzy_pinyin(&to_char.to_string())? {
        return None;
    }
    if let Some(left) = baseline[..hunk.removed.start].last()
        && left.end == from.start
        && name_char(left).is_some()
    {
        return Some(Hunk {
            removed: hunk.removed.start - 1..hunk.removed.end,
            added: hunk.added.start - 1..hunk.added.end,
        });
    }
    let right = baseline.get(hunk.removed.end)?;
    (right.start == from.end && name_char(right).is_some()).then(|| Hunk {
        removed: hunk.removed.start..hunk.removed.end + 1,
        added: hunk.added.start..hunk.added.end + 1,
    })
}

/// Capitals past the first letter of a word ("GitHub", "iOS").
fn has_inner_capital(term: &str) -> bool {
    term.split_whitespace()
        .any(|word| word.chars().skip(1).any(char::is_uppercase))
}

/// Whether `text` starts a sentence at byte `start`, where a capital says
/// nothing about the word.
fn at_sentence_start(text: &str, start: usize, region: &Range<usize>) -> bool {
    start == region.start
        || text[..start]
            .trim_end_matches(' ')
            .chars()
            .next_back()
            .is_none_or(|c| ".!?。！？\n".contains(c))
}

/// A capitalization fix: `Some(needs_repeat)` when it names a term.
fn classify_case_fix(term: &str, sentence_start: bool) -> Option<bool> {
    if !term.chars().any(char::is_uppercase) {
        return None;
    }
    if has_inner_capital(term) {
        // Short ones collide with everyday words ("us" -> "US").
        return Some(utf16_len(term) < 3);
    }
    let words = term.split_whitespace().count();
    if words >= 2 {
        return Some(false);
    }
    (!sentence_start).then_some(true)
}

fn stem(word: &str) -> &str {
    ["ing", "ers", "er", "ed", "es", "ly", "s"]
        .iter()
        .find_map(|suffix| word.strip_suffix(*suffix).filter(|rest| rest.len() >= 3))
        .unwrap_or(word)
}

/// Whether two lowercase words differ only by an English ending
/// ("work" -> "working"), which fixes grammar rather than a mishearing.
fn is_inflection(a: &str, b: &str) -> bool {
    let (a, b) = (stem(a), stem(b));
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    long.starts_with(short) && long.len() - short.len() <= 1
}

/// `Some(needs_repeat)` when a replaced run looks like a misheard term rather
/// than a content edit.
fn classify(misheard: &str, term: &str, term_units: &[Unit], sentence_start: bool) -> Option<bool> {
    if !HAS_LETTER_RE.is_match(term) {
        return None;
    }
    if term_units
        .iter()
        .all(|u| COMMON_WORDS.contains(u.key.as_str()))
    {
        return None;
    }
    let (a, b) = (misheard.to_lowercase(), term.to_lowercase());
    if a == b {
        return classify_case_fix(term, sentence_start);
    }
    if is_unspaced_script_only(term) {
        // The segmenter's dictionary already knows a single-unit term (认为,
        // 说得), so it is ordinary vocabulary or a grammar fix. Misheard names
        // fall outside that dictionary and split into several units (章|珊).
        if term_units.len() < 2 {
            return None;
        }
        return if is_unspaced_script_only(misheard) {
            compare_sound(misheard, term)
        } else {
            Some(false)
        };
    }
    // Names and products are capitalized or carry digits; a lowercase word
    // may just be a better word choice ("affect" -> "effect").
    let weak = !term.chars().any(|c| c.is_uppercase() || c.is_ascii_digit());
    if misheard.chars().any(is_unspaced_script) {
        // A Latin term replacing an unspaced-script mishearing ("爱爱" -> "AI").
        return (utf16_len(term) >= 2).then_some(weak);
    }
    if utf16_len(term) < 3 || (weak && is_inflection(&a, &b)) {
        return None;
    }
    // 0.65 allows phonetic corrections like "Shunade" -> "Sinead" (4/7) while
    // filtering out unrelated word replacements.
    let longest = utf16_len(&a).max(utf16_len(&b)).max(1);
    (edit_distance(&a, &b) as f64 / longest as f64 <= 0.65).then_some(weak)
}

/// Corrections the user made to `pasted` text: the differences between the
/// field right after the paste (`baseline`) and its last value (`edited`)
/// that fall inside a pasted region.
pub fn extract_corrections<S: AsRef<str>>(
    baseline: &str,
    edited: &str,
    pasted: &[S],
) -> Vec<Correction> {
    if baseline.is_empty() || edited.is_empty() || baseline == edited {
        return Vec::new();
    }
    let base_units = tokenize(baseline);
    let edited_units = tokenize(edited);
    if base_units.is_empty() || edited_units.is_empty() {
        return Vec::new();
    }
    let hunks = diff_units(&base_units, &edited_units);
    let regions: Vec<Range<usize>> = pasted_regions(baseline, pasted)
        .into_iter()
        .filter(|region| !is_rewritten(region, &base_units, &hunks))
        .collect();

    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut results = Vec::new();
    for hunk in &hunks {
        // Pure insertions and deletions add or drop content; only a
        // replacement pairs a misheard run with its correction.
        if hunk.removed.is_empty() || hunk.added.is_empty() {
            continue;
        }
        if hunk.added.len() > MAX_TERM_UNITS || hunk.removed.len() > MAX_REPLACED_UNITS {
            continue;
        }
        let (start, end) = (
            base_units[hunk.removed.start].start,
            base_units[hunk.removed.end - 1].end,
        );
        let Some(region) = regions.iter().find(|r| r.start <= start && end <= r.end) else {
            continue;
        };
        let widened = widen_name_fix(hunk, &base_units, &edited_units);
        let hunk = widened.as_ref().unwrap_or(hunk);
        let misheard = slice_units(baseline, &base_units[hunk.removed.clone()]);
        let term = slice_units(edited, &edited_units[hunk.added.clone()]);
        let sentence_start = at_sentence_start(baseline, start, region);
        let Some(needs_repeat) = classify(
            misheard,
            term,
            &edited_units[hunk.added.clone()],
            sentence_start,
        ) else {
            continue;
        };
        if seen.insert((misheard.to_lowercase(), term.to_lowercase())) {
            results.push(Correction {
                misheard: misheard.to_string(),
                term: term.to_string(),
                needs_repeat: needs_repeat || widened.is_some(),
            });
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corrections(original: &str, edited: &str) -> Vec<Correction> {
        extract_corrections(original, edited, &[original])
    }

    fn ex(original: &str, edited: &str) -> Vec<String> {
        corrections(original, edited)
            .into_iter()
            .map(|c| c.term)
            .collect()
    }

    /// Terms the Dictionary takes at once.
    fn sure(original: &str, edited: &str) -> Vec<String> {
        corrections(original, edited)
            .into_iter()
            .filter(|c| !c.needs_repeat)
            .map(|c| c.term)
            .collect()
    }

    fn known(misheard: &str, term: &str, count: i64) -> KnownCorrection {
        KnownCorrection {
            misheard: misheard.into(),
            term: term.into(),
            count,
        }
    }

    #[test]
    fn empty_and_identical_inputs() {
        assert!(ex("", "hello").is_empty());
        assert!(ex("hello", "").is_empty());
        assert!(ex("hello world", "hello world").is_empty());
    }

    #[test]
    fn learns_phonetic_mishearing() {
        assert_eq!(
            corrections("Hey Shunade how are you", "Hey Sinead how are you"),
            [Correction {
                misheard: "Shunade".into(),
                term: "Sinead".into(),
                needs_repeat: false,
            }]
        );
        assert_eq!(sure("Hey Shunade", "Hey Sinead"), ["Sinead"]);
    }

    #[test]
    fn rewrite_is_not_corrections() {
        assert!(ex("the cat sat on the mat", "a dog stood under a rug").is_empty());
    }

    #[test]
    fn short_and_unrelated_replacements_ignored() {
        assert!(ex("I went to see XX today", "I went to see Al today").is_empty());
        assert!(ex("I saw a cat yesterday", "I saw a elephant yesterday").is_empty());
    }

    #[test]
    fn everyday_word_swaps_ignored() {
        assert!(ex("This is why I'm speaking", "This is what I'm speaking").is_empty());
        assert!(ex("This is why I'm speaking", "This is where I'm speaking").is_empty());
    }

    #[test]
    fn inflections_are_grammar_not_terms() {
        assert!(ex("I am work on it", "I am working on it").is_empty());
        assert!(ex("she walk to town", "she walked to town").is_empty());
        assert!(ex("we fixed two test", "we fixed two tests").is_empty());
        assert!(ex("he is run fast", "he is running fast").is_empty());
    }

    #[test]
    fn capitalized_look_alikes_are_not_inflections() {
        assert_eq!(
            sure("we run postgre here", "we run Postgres here"),
            ["Postgres"]
        );
    }

    #[test]
    fn lowercase_terms_wait_for_a_second_sighting() {
        assert_eq!(
            corrections("this will affect it", "this will effect it"),
            [Correction {
                misheard: "affect".into(),
                term: "effect".into(),
                needs_repeat: true,
            }]
        );
    }

    #[test]
    fn same_correction_learned_once() {
        assert_eq!(
            ex("Shunade said hi to Shunade", "Sinead said hi to Sinead"),
            ["Sinead"]
        );
    }

    #[test]
    fn latin_term_replacing_chinese_transliteration() {
        assert_eq!(
            sure(
                "我们今天用克劳德扣的写代码。",
                "我们今天用Claude Code写代码。"
            ),
            ["Claude Code"]
        );
        assert_eq!(
            sure("克劳德扣的写代码", "Claude Code写代码"),
            ["Claude Code"]
        );
        assert_eq!(sure("用爱爱写代码", "用AI写代码"), ["AI"]);
    }

    #[test]
    fn misheard_chinese_name() {
        assert_eq!(sure("明天和张三开会", "明天和章珊开会"), ["章珊"]);
    }

    #[test]
    fn unspaced_english_inside_chinese() {
        assert_eq!(
            sure("我在用wis per做听写", "我在用Whisper做听写"),
            ["Whisper"]
        );
        assert_eq!(
            sure("我们用 cloud code 写代码", "我们用 Claude code 写代码"),
            ["Claude"]
        );
    }

    #[test]
    fn chinese_grammar_fixes_ignored() {
        assert!(ex("他说的很好", "他说得很好").is_empty());
        assert!(ex("我觉得这个方案可以", "我认为这个方案可以").is_empty());
        assert!(ex("他跑的快", "他跑得快").is_empty());
    }

    #[test]
    fn one_character_name_fix_widens_and_waits() {
        assert_eq!(
            corrections("我今天和张山一起吃饭", "我今天和张珊一起吃饭"),
            [Correction {
                misheard: "张山".into(),
                term: "张珊".into(),
                needs_repeat: true,
            }]
        );
    }

    #[test]
    fn a_one_character_fix_that_changes_the_sound_is_content() {
        assert!(ex("我今天和张山一起吃饭", "我今天和张海一起吃饭").is_empty());
    }

    #[test]
    fn chinese_rewording_is_content_not_a_mishearing() {
        assert!(ex("今天开会讨论预算问题", "今天开会讨论人员安排").is_empty());
        assert!(ex("这个方法非常好用", "这个办法相当好用").is_empty());
    }

    #[test]
    fn rewritten_chinese_sentence_ignored() {
        assert!(ex("今天天气很好我们去公园", "明天下雨大家待在家里").is_empty());
    }

    #[test]
    fn capitalization_fixes_name_terms() {
        assert_eq!(
            sure("push it to github today", "push it to GitHub today"),
            ["GitHub"]
        );
        assert_eq!(sure("the ios app", "the iOS app"), ["iOS"]);
        assert_eq!(
            sure("use claude code here", "use Claude Code here"),
            ["Claude Code"]
        );
    }

    #[test]
    fn a_capital_alone_needs_a_second_sighting_and_a_mid_sentence_word() {
        assert_eq!(
            corrections("we use python here", "we use Python here"),
            [Correction {
                misheard: "python".into(),
                term: "Python".into(),
                needs_repeat: true,
            }]
        );
        assert!(ex("hello there. python rules", "hello there. Python rules").is_empty());
        assert!(ex("hello there", "Hello there").is_empty());
        assert!(ex("We use Python here", "we use python here").is_empty());
    }

    // The BASELINE and CHANGED lines macos-text-monitor reported from TextEdit.
    #[test]
    fn learns_both_fixes_from_one_session() {
        assert_eq!(
            sure(
                "明天和张三开会，用 cloud code 写代码",
                "明天和章珊开会，用 Claude Code 写代码"
            ),
            ["章珊", "Claude Code"]
        );
    }

    #[test]
    fn learns_only_inside_the_pasted_region() {
        let prefix = "Earlier chat content that was already in the field before dictation. ";
        let pasted = "部署到乌班图服务器上";
        let found = extract_corrections(
            &format!("{prefix}{pasted}"),
            &format!("{prefix}部署到Ubuntu服务器上"),
            &[pasted],
        );
        assert_eq!(found[0].term, "Ubuntu");
        assert!(
            extract_corrections(
                "Shunade wrote this. pasted words here",
                "Sinead wrote this. pasted words here",
                &["pasted words here"],
            )
            .is_empty()
        );
    }

    #[test]
    fn learns_from_any_recent_paste() {
        let (first, second) = ("部署到乌班图服务器上", "然后重启服务");
        let found = extract_corrections(
            &format!("{first}。{second}"),
            &format!("部署到Ubuntu服务器上。{second}"),
            &[first, second],
        );
        assert_eq!(found[0].term, "Ubuntu");
    }

    #[test]
    fn a_rewritten_region_is_skipped_even_in_a_long_field() {
        let pasted = "the cat sat on the mat";
        let prefix = "lots of unchanged words stay in this field ".repeat(5);
        assert!(
            extract_corrections(
                &format!("{prefix}{pasted}"),
                &format!("{prefix}a dog stood under a rug"),
                &[pasted],
            )
            .is_empty()
        );
    }

    #[test]
    fn a_reformatted_paste_counts_when_the_field_is_about_its_size() {
        let found = extract_corrections(
            "Hey \u{201c}Shunade\u{201d} there",
            "Hey \u{201c}Sinead\u{201d} there",
            &["Hey \"Shunade\" there"],
        );
        assert_eq!(found[0].term, "Sinead");
    }

    #[test]
    fn a_field_with_no_trace_of_the_paste_teaches_nothing() {
        let field = "something else entirely that was never pasted here at all";
        assert!(
            extract_corrections(field, &field.replace("never", "nearly"), &["short"]).is_empty()
        );
    }

    #[test]
    fn huge_changed_runs_are_left_alone() {
        let a: String = (0..3000).map(|i| format!("alpha{i} ")).collect();
        let b: String = (0..3000).map(|i| format!("beta{i} ")).collect();
        assert!(ex(&a, &b).is_empty());
    }

    #[test]
    fn edit_distance_counts_utf16_units() {
        assert_eq!(edit_distance("shunade", "sinead"), 4);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn fuzzy_pinyin_merges_common_confusions() {
        assert_eq!(fuzzy_pinyin("张三"), fuzzy_pinyin("章珊"));
        assert_eq!(fuzzy_pinyin("老"), fuzzy_pinyin("脑"));
        assert_ne!(fuzzy_pinyin("山"), fuzzy_pinyin("海"));
        assert_eq!(fuzzy_pinyin("ka"), None);
    }

    #[test]
    fn phrases_and_chinese_apply_automatically_after_a_repeat() {
        assert!(known("cloud code", "Claude Code", 2).applies_automatically());
        assert!(known("克劳德", "Claude", 2).applies_automatically());
        assert!(known("github", "GitHub", 2).applies_automatically());
        assert!(!known("cloud code", "Claude Code", 1).applies_automatically());
    }

    #[test]
    fn single_latin_words_are_left_to_the_cleanup_model() {
        assert!(!known("cloud", "Claude", 9).applies_automatically());
        assert!(!known("us", "US", 9).applies_automatically());
        assert!(!known("python", "Python", 9).applies_automatically());
    }

    #[test]
    fn applies_longest_mishearing_first_and_ignores_case() {
        let text = apply_known_corrections(
            "Cloud Code 和克劳德扣的都能用",
            &[
                known("cloud code", "Claude Code", 2),
                known("克劳德", "Claude", 2),
                known("克劳德扣的", "Claude Code", 2),
            ],
        );
        assert_eq!(text, "Claude Code 和Claude Code都能用");
    }

    #[test]
    fn a_term_containing_its_mishearing_is_left_alone() {
        let text = apply_known_corrections("克劳德 和 克劳", &[known("克劳", "克劳德", 2)]);
        assert_eq!(text, "克劳德 和 克劳德");
    }

    #[test]
    fn unconfirmed_and_context_dependent_corrections_change_nothing() {
        let corrections = [known("克劳德", "Claude", 1), known("cloud", "Claude", 5)];
        assert_eq!(
            apply_known_corrections("克劳德 cloud", &corrections),
            "克劳德 cloud"
        );
    }
}
