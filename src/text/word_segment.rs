//! Word segmentation matching ICU4C's word break iterator closely enough for
//! the correction learner.
//!
//! ICU4X's dictionary segmenter splits Chinese/Japanese greedily by longest
//! match ("我认为" -> "我认|为"), while ICU4C picks the
//! lowest-cost path through the same cjdict word costs. The learner's rules
//! depend on that difference (认为 must be one unit, an unknown name must split
//! per character), so CJ runs are re-segmented here with ICU4C's algorithm
//! (`CjkBreakEngine::divideUpDictionaryRange`) over ICU4X's bundled cjdict.

use std::sync::LazyLock;

use icu_collections::char16trie::{Char16Trie, TrieResult};
use icu_provider::prelude::*;
use icu_segmenter::WordSegmenter;
use icu_segmenter::options::WordBreakInvariantOptions;
use icu_segmenter::provider::{Baked, SegmenterDictionaryAutoV1};

use crate::text::script::{is_han, is_hiragana, is_katakana};

/// A word-like segment with byte offsets into the source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WordSegment {
    pub start: usize,
    pub end: usize,
}

// Baked data is static, so the trie can be shared across threads.
static CJ_DICTIONARY: LazyLock<Option<Char16Trie<'static>>> = LazyLock::new(|| {
    let request = DataRequest {
        id: DataIdentifierBorrowed::for_marker_attributes(DataMarkerAttributes::from_str_or_panic(
            "cjdict",
        )),
        ..Default::default()
    };
    match DataProvider::<SegmenterDictionaryAutoV1>::load(&Baked, request) {
        Ok(response) => response
            .payload
            .get_static()
            .map(|data| Char16Trie::new(data.trie_data.clone())),
        Err(err) => {
            log::warn!("cjdict unavailable, falling back to ICU4X segmentation: {err}");
            None
        }
    }
});

// ICU4C dictbe.cpp constants.
const MAX_WORD_SIZE: usize = 20;
const MAX_SNLP: u32 = 255;
const MAX_KATAKANA_LENGTH: usize = 8;
const MAX_KATAKANA_GROUP_LENGTH: usize = 20;
const KATAKANA_COSTS: [u32; 9] = [8192, 984, 408, 240, 204, 252, 300, 372, 480];

fn katakana_cost(len: usize) -> u32 {
    if len > MAX_KATAKANA_LENGTH {
        8192
    } else {
        KATAKANA_COSTS[len]
    }
}

// ICU's CJ engine katakana test: Katakana block, prolonged sound mark and
// halfwidth forms.
fn is_icu_katakana(c: char) -> bool {
    matches!(c, '\u{30A1}'..='\u{30FE}' | '\u{FF66}'..='\u{FF9F}') && c != '\u{30FB}'
}

/// Characters the CJ dictionary engine handles.
fn is_cj(c: char) -> bool {
    is_han(c)
        || is_hiragana(c)
        || is_katakana(c)
        || matches!(c, '\u{30FC}' | '\u{FF70}' | '\u{FF9E}' | '\u{FF9F}')
}

/// Lowest-cost split of a run of CJ characters; returns break offsets in chars.
fn divide_cj_run(trie: &Char16Trie<'_>, chars: &[char]) -> Vec<usize> {
    let n = chars.len();
    let mut best = vec![u32::MAX; n + 1];
    let mut prev = vec![usize::MAX; n + 1];
    best[0] = 0;
    let mut is_prev_katakana = false;

    for i in 0..n {
        if best[i] == u32::MAX {
            is_prev_katakana = is_icu_katakana(chars[i]);
            continue;
        }
        // Every dictionary word starting at i, with its cost.
        let mut candidates: Vec<(usize, u32)> = Vec::new();
        let mut iter = trie.iter();
        for (k, &c) in chars[i..].iter().take(MAX_WORD_SIZE.min(n - i)).enumerate() {
            match iter.next(c) {
                TrieResult::FinalValue(v) => {
                    candidates.push((k + 1, v.max(0) as u32));
                    break;
                }
                TrieResult::Intermediate(v) => candidates.push((k + 1, v.max(0) as u32)),
                TrieResult::NoValue => {}
                TrieResult::NoMatch => break,
            }
        }
        // No single-character word: treat the character as one at max cost.
        if candidates.first().is_none_or(|&(len, _)| len != 1) {
            candidates.push((1, MAX_SNLP));
        }
        for (len, cost) in candidates {
            let snlp = best[i].saturating_add(cost);
            if snlp < best[i + len] {
                best[i + len] = snlp;
                prev[i + len] = i;
            }
        }

        // Single-character Katakana words are rare, so a whole Katakana run
        // is a candidate word with a length-based default cost.
        let is_kata = is_icu_katakana(chars[i]);
        if !is_prev_katakana && is_kata {
            let mut j = i + 1;
            while j < n && j - i < MAX_KATAKANA_GROUP_LENGTH && is_icu_katakana(chars[j]) {
                j += 1;
            }
            if j - i < MAX_KATAKANA_GROUP_LENGTH {
                let snlp = best[i].saturating_add(katakana_cost(j - i));
                if snlp < best[j] {
                    best[j] = snlp;
                    prev[j] = i;
                }
            }
        }
        is_prev_katakana = is_kata;
    }

    let mut breaks = Vec::new();
    let mut at = n;
    if best[n] == u32::MAX {
        return (1..=n).collect();
    }
    while at > 0 {
        breaks.push(at);
        at = prev[at];
    }
    breaks.reverse();
    breaks
}

/// Word-like segments of `text`, in order.
pub(crate) fn word_segments(text: &str) -> Vec<WordSegment> {
    let segmenter = WordSegmenter::new_dictionary(WordBreakInvariantOptions::default());
    let mut raw: Vec<WordSegment> = Vec::new();
    let mut prev = 0;
    for (index, word_type) in segmenter.segment_str(text).iter_with_word_type() {
        if index == 0 {
            continue;
        }
        if word_type.is_word_like() {
            raw.push(WordSegment {
                start: prev,
                end: index,
            });
        }
        prev = index;
    }

    let Some(trie) = CJ_DICTIONARY.as_ref() else {
        return raw;
    };
    let all_cj = |s: &WordSegment| text[s.start..s.end].chars().all(is_cj);

    // Merge abutting all-CJ segments back into runs and re-split them.
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if !all_cj(&raw[i]) {
            out.push(raw[i].clone());
            i += 1;
            continue;
        }
        let start = raw[i].start;
        let mut end = raw[i].end;
        i += 1;
        while i < raw.len() && raw[i].start == end && all_cj(&raw[i]) {
            end = raw[i].end;
            i += 1;
        }
        let run = &text[start..end];
        let offsets: Vec<usize> = run
            .char_indices()
            .map(|(o, _)| o)
            .chain([run.len()])
            .collect();
        let chars: Vec<char> = run.chars().collect();
        let mut from = 0;
        for to in divide_cj_run(trie, &chars) {
            out.push(WordSegment {
                start: start + offsets[from],
                end: start + offsets[to],
            });
            from = to;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<&str> {
        word_segments(text)
            .into_iter()
            .map(|s| &text[s.start..s.end])
            .collect()
    }

    #[test]
    fn segments_like_icu4c() {
        assert_eq!(
            words("我认为这个方案可以"),
            ["我", "认为", "这个", "方案", "可以"]
        );
        assert_eq!(words("他说得很好"), ["他", "说得", "很好"]);
        assert_eq!(words("Hello, world 42"), ["Hello", "world", "42"]);
    }

    // Expected values captured from ICU4C's word break iterator.
    #[test]
    fn matches_icu4c_word_breaks() {
        let cases: [(&str, &[&str]); 8] = [
            (
                "我在用Whisper做听写",
                &["我在", "用", "Whisper", "做", "听写"],
            ),
            ("明天和章珊开会", &["明天", "和", "章", "珊", "开会"]),
            (
                "我今天和张山一起吃饭",
                &["我", "今天", "和", "张", "山", "一起", "吃饭"],
            ),
            (
                "我今天和张珊一起吃饭",
                &["我", "今天", "和", "张", "珊", "一起", "吃饭"],
            ),
            (
                "部署到乌班图服务器上",
                &["部署", "到", "乌", "班", "图", "服务", "器", "上"],
            ),
            (
                "我们今天用克劳德扣的写代码。",
                &[
                    "我们", "今天", "用", "克", "劳", "德", "扣", "的", "写", "代码",
                ],
            ),
            (
                "会議の資料を確認してください",
                &[
                    "会議", "の", "資料", "を", "確認", "し", "て", "くだ", "さい",
                ],
            ),
            ("コンピューターを使う", &["コンピューター", "を", "使う"]),
        ];
        for (text, expected) in cases {
            assert_eq!(words(text), expected, "{text}");
        }
    }
}
