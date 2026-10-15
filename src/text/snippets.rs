//! Snippet expansion: spoken triggers replaced with saved text.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use fancy_regex::Regex as FancyRegex;
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Snippet {
    pub trigger: String,
    pub replacement: String,
}

static BARE_STRIP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\s\p{P}\p{S}]").expect("static regex"));

/// An insertion-ordered map: re-setting a key keeps its first position,
/// which decides bare-trigger priority.
#[derive(Debug, Default)]
struct OrderedMap {
    entries: Vec<(String, String)>,
    index: HashMap<String, usize>,
}

impl OrderedMap {
    fn set(&mut self, key: String, value: String) {
        if let Some(&i) = self.index.get(&key) {
            self.entries[i].1 = value;
        } else {
            self.index.insert(key.clone(), self.entries.len());
            self.entries.push((key, value));
        }
    }

    fn has(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.index.get(key).map(|&i| self.entries[i].1.as_str())
    }
}

/// Compiled matcher for one snippet list. Build it once per list with
/// [`SnippetMatcher::new`] when expanding many transcripts.
#[derive(Debug)]
pub struct SnippetMatcher {
    regex: FancyRegex,
    replacements: OrderedMap,
    /// Trigger keys with spaces, punctuation and symbols removed.
    bare_triggers: OrderedMap,
}

// Regex case folding can't fold Turkish İ (U+0130) or dotless ı (U+0131), and
// İ lowercases to two chars ("i" + U+0307), so triggers like "İmza" never
// matched. Folding İ to a plain "i" gives keys a canonical form, and matching
// İ/ı explicitly in the pattern lets the regex find them in the transcript.
fn fold_capital_i_dot(value: &str) -> String {
    value.replace('İ', "i")
}

// Cleanup models punctuate and space a spoken trigger freely ("My email,
// address."), which breaks the in-text match; the bare form survives that.
fn strip_to_bare(value: &str) -> String {
    BARE_STRIP_RE.replace_all(value, "").into_owned()
}

/// Escapes a key for the pattern: regex metacharacters get a backslash, i/ı
/// become explicit Turkish classes.
fn escape_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len() * 2);
    for c in key.chars() {
        match c {
            'i' => out.push_str("[iİ]"),
            'ı' => out.push_str("[ıI]"),
            '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\'
            | '#' | '&' | '-' | '~' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

impl SnippetMatcher {
    /// Returns `None` when no snippet has a usable trigger.
    pub fn new(snippets: &[Snippet]) -> Option<Self> {
        let mut replacements = OrderedMap::default();
        for Snippet {
            trigger,
            replacement,
        } in snippets
        {
            let nfc: String = trigger.trim().nfc().collect();
            let folded = fold_capital_i_dot(&nfc);
            let key = folded.to_lowercase();
            if key.is_empty() {
                continue;
            }
            replacements.set(key, replacement.clone());
            // An uppercase I in the trigger may mean Turkish ı as well as
            // English i, so register both readings; an explicit trigger wins.
            let dotless_key = folded.replace('I', "ı").to_lowercase();
            if !replacements.has(&dotless_key) {
                replacements.set(dotless_key, replacement.clone());
            }
        }
        if replacements.entries.is_empty() {
            return None;
        }

        let mut bare_triggers = OrderedMap::default();
        for (key, replacement) in &replacements.entries {
            let bare = strip_to_bare(key);
            if !bare.is_empty() && !bare_triggers.has(&bare) {
                bare_triggers.set(bare, replacement.clone());
            }
        }

        // Longest-first (UTF-16 length, stable) so "investor ask" wins over a
        // shorter "ask" trigger.
        let mut keys: Vec<&str> = replacements
            .entries
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        keys.sort_by_key(|k| std::cmp::Reverse(utf16_len(k)));
        let alternatives: Vec<String> = keys.iter().map(|k| escape_key(k)).collect();
        // Unicode-aware word boundaries: triggers never match inside a word.
        // `(?:^|(?<=…))` keeps the lookbehind fixed-width for fancy-regex.
        let pattern = format!(
            r"(?i)(?:^|(?<=[\s\p{{P}}\p{{S}}]))(?:{})(?=$|[\s\p{{P}}\p{{S}}])",
            alternatives.join("|")
        );
        let regex = match FancyRegex::new(&pattern) {
            Ok(regex) => regex,
            Err(err) => {
                log::warn!("snippet matcher failed to compile: {err}");
                return None;
            }
        };
        Some(Self {
            regex,
            replacements,
            bare_triggers,
        })
    }

    /// Replace every spoken trigger with its saved text in a single pass. A
    /// dictation that is nothing but one trigger becomes exactly the saved
    /// text, whatever spacing and punctuation surrounds or splits it.
    pub fn expand(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        // NFC so a decomposed "I" + U+0307 in the transcript recombines into İ.
        let normalized: String = text.nfc().collect();
        let bare = strip_to_bare(&normalized);
        if !bare.is_empty()
            && let Some(whole) = resolve_replacement(&bare, &self.bare_triggers)
        {
            return whole.to_string();
        }

        let mut out = String::with_capacity(normalized.len());
        let mut last = 0;
        for found in self.regex.find_iter(&normalized) {
            let Ok(m) = found else {
                // Backtrack limit hit: leave the rest of the text untouched.
                break;
            };
            out.push_str(&normalized[last..m.start()]);
            out.push_str(resolve_replacement(m.as_str(), &self.replacements).unwrap_or(m.as_str()));
            last = m.end();
        }
        out.push_str(&normalized[last..]);
        out
    }
}

// The pattern case-folds the Unicode way while the keys are lowercased, so a
// match is not proof of a replacement: "[ıI]mza" matches a plain "imza", which
// is no key at all. Resolving through here keeps a match an actual expansion.
fn resolve_replacement<'a>(matched: &str, replacements: &'a OrderedMap) -> Option<&'a str> {
    let folded = fold_capital_i_dot(matched);
    replacements
        .get(&folded.to_lowercase())
        // An uppercase I can be capital dotless ı as well as English i.
        .or_else(|| replacements.get(&folded.replace('I', "ı").to_lowercase()))
}

type MatcherCache = Option<(Vec<Snippet>, Option<Arc<SnippetMatcher>>)>;

// Memoized against the snippet list content.
static MATCHER_CACHE: LazyLock<Mutex<MatcherCache>> = LazyLock::new(|| Mutex::new(None));

fn cached_matcher(snippets: &[Snippet]) -> Option<Arc<SnippetMatcher>> {
    if snippets.is_empty() {
        return None;
    }
    let mut cache = MATCHER_CACHE.lock();
    if let Some((cached, matcher)) = cache.as_ref()
        && cached.as_slice() == snippets
    {
        return matcher.clone();
    }
    let matcher = SnippetMatcher::new(snippets).map(Arc::new);
    *cache = Some((snippets.to_vec(), matcher.clone()));
    matcher
}

/// Replace every spoken trigger with its saved text. See [`SnippetMatcher::expand`].
pub fn expand_snippets(text: &str, snippets: &[Snippet]) -> String {
    if text.is_empty() {
        return String::new();
    }
    match cached_matcher(snippets) {
        Some(matcher) => matcher.expand(text),
        None => text.to_string(),
    }
}

/// Dictionary words plus snippet triggers: the hint list fed to the STT prompt
/// and the cleanup-model dictionary suffix so triggers survive both.
pub fn get_dictionary_hint_words(dictionary: &[String], snippets: &[Snippet]) -> Vec<String> {
    let mut words = dictionary.to_vec();
    words.extend(
        snippets
            .iter()
            .filter(|s| !s.trigger.is_empty())
            .map(|s| s.trigger.clone()),
    );
    words
}

/// The same hint words ordered highest priority first for the STT prompt:
/// snippet triggers (expansion depends on them), then dictionary words in
/// `priority` order. Words `priority` does not know yet were just added, so
/// they lead the dictionary words.
pub fn get_prioritized_dictionary_hint_words(
    dictionary: &[String],
    snippets: &[Snippet],
    priority: &[String],
) -> Vec<String> {
    let mut rank: HashMap<String, i64> = HashMap::new();
    // Later duplicates overwrite earlier ones.
    for (index, word) in priority.iter().enumerate() {
        rank.insert(word.to_lowercase(), index as i64);
    }
    let mut ranked: Vec<(usize, i64, &String)> = dictionary
        .iter()
        .enumerate()
        .map(|(index, word)| {
            (
                index,
                rank.get(&word.to_lowercase()).copied().unwrap_or(-1),
                word,
            )
        })
        .collect();
    ranked.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));

    let mut words = get_dictionary_hint_words(&[], snippets);
    words.extend(ranked.into_iter().map(|(_, _, w)| w.clone()));
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snip(trigger: &str, replacement: &str) -> Vec<Snippet> {
        vec![Snippet {
            trigger: trigger.into(),
            replacement: replacement.into(),
        }]
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn expands_turkish_capital_i_trigger() {
        let s = snip("İmza", "Best regards,\nUmut");
        assert_eq!(expand_snippets("İmza", &s), "Best regards,\nUmut");
    }

    #[test]
    fn capital_i_dot_trigger_matches_every_casing() {
        let s = snip("İmza", "Best regards,\nUmut");
        for spoken in ["imza", "İmza", "İMZA", "Imza"] {
            assert_eq!(
                expand_snippets(&format!("Meeting is over. {spoken} please."), &s),
                "Meeting is over. Best regards,\nUmut please.",
                "expected {spoken:?} to expand"
            );
        }
    }

    #[test]
    fn lowercase_trigger_matches_capital_i_dot() {
        let s = snip("imza", "Best regards,\nUmut");
        assert_eq!(
            expand_snippets("İmza goes here", &s),
            "Best regards,\nUmut goes here"
        );
        assert_eq!(
            expand_snippets("İMZA goes here", &s),
            "Best regards,\nUmut goes here"
        );
    }

    #[test]
    fn dotless_trigger_matches_capital_i() {
        let s = snip("ışık", "LIGHT");
        for spoken in ["ışık", "Işık", "IŞIK"] {
            assert_eq!(
                expand_snippets(&format!("{spoken} on"), &s),
                "LIGHT on",
                "{spoken}"
            );
        }
    }

    #[test]
    fn capital_i_trigger_matches_both_readings() {
        let s = snip("Işık", "LIGHT");
        for spoken in ["ışık", "Işık", "IŞIK", "işık"] {
            assert_eq!(
                expand_snippets(&format!("{spoken} on"), &s),
                "LIGHT on",
                "{spoken}"
            );
        }
    }

    #[test]
    fn all_caps_english_trigger_with_capital_i() {
        let s = snip("IBAN", "TR00 0000 0000");
        assert_eq!(expand_snippets("iban please", &s), "TR00 0000 0000 please");
        assert_eq!(expand_snippets("IBAN please", &s), "TR00 0000 0000 please");
        assert_eq!(expand_snippets("İBAN please", &s), "TR00 0000 0000 please");
    }

    #[test]
    fn decomposed_capital_i_dot_matches() {
        let s = snip("İmza", "Best regards,\nUmut");
        let nfd: String = "İmza done".nfd().collect();
        assert_eq!(expand_snippets(&nfd, &s), "Best regards,\nUmut done");
    }

    #[test]
    fn respects_word_boundaries() {
        let s = snip("İmza", "SIGNATURE");
        assert_eq!(
            expand_snippets("imzalar are ready", &s),
            "imzalar are ready"
        );
        assert_eq!(
            expand_snippets("(İmza) is required", &s),
            "(SIGNATURE) is required"
        );
    }

    #[test]
    fn ascii_triggers_fold_case() {
        let s = snip("Signoff", "Regards");
        assert_eq!(expand_snippets("SIGNOFF now", &s), "Regards now");
        assert_eq!(expand_snippets("signoff now", &s), "Regards now");
    }

    #[test]
    fn multiple_occurrences_preserved() {
        let s = snip("İmza", "X");
        assert_eq!(expand_snippets("İmza and imza, done", &s), "X and X, done");
    }

    #[test]
    fn no_snippets_leaves_text() {
        assert_eq!(expand_snippets("hello there", &[]), "hello there");
    }

    #[test]
    fn longest_trigger_wins_and_unicode_boundaries() {
        let s = vec![
            Snippet {
                trigger: "ask".into(),
                replacement: "A".into(),
            },
            Snippet {
                trigger: "investor ask".into(),
                replacement: "IA".into(),
            },
        ];
        assert_eq!(
            expand_snippets("the investor ask and ask", &s),
            "the IA and A"
        );
        // Letters from other scripts are word characters too.
        assert_eq!(expand_snippets("an éask now", &s), "an éask now");
        assert_eq!(expand_snippets("say 「ask」 now", &s), "say 「A」 now");
    }

    #[test]
    fn regex_metacharacters_in_triggers_are_literal() {
        let s = snip("c++ (lang)", "CPP");
        assert_eq!(
            expand_snippets("I like c++ (lang) a lot", &s),
            "I like CPP a lot"
        );
        assert_eq!(
            expand_snippets("I like cxx lang a lot", &s),
            "I like cxx lang a lot"
        );
    }

    #[test]
    fn hint_words_empty_inputs() {
        assert!(get_dictionary_hint_words(&[], &[]).is_empty());
    }

    #[test]
    fn hint_words_dictionary_only() {
        let dict = strings(&["Whisper", "Supabase"]);
        assert_eq!(get_dictionary_hint_words(&dict, &[]), dict);
    }

    #[test]
    fn hint_words_triggers_only_and_combined() {
        let snippets = vec![
            Snippet {
                trigger: "my cal".into(),
                replacement: "cal.com/me".into(),
            },
            Snippet {
                trigger: "zoom link".into(),
                replacement: "zoom.us/j/123".into(),
            },
            Snippet {
                trigger: "".into(),
                replacement: "ignored".into(),
            },
        ];
        assert_eq!(
            get_dictionary_hint_words(&[], &snippets),
            strings(&["my cal", "zoom link"])
        );
        assert_eq!(
            get_dictionary_hint_words(&strings(&["Whisper", "Supabase"]), &snippets),
            strings(&["Whisper", "Supabase", "my cal", "zoom link"])
        );
    }

    #[test]
    fn prioritized_hint_words_lead_with_triggers() {
        let dict = strings(&["Alice", "Bob", "Carol", "Dana"]);
        let snippets = snip("my cal", "cal.com/me");
        assert_eq!(
            get_prioritized_dictionary_hint_words(
                &dict,
                &snippets,
                &strings(&["carol", "alice", "bob"])
            ),
            strings(&["my cal", "Dana", "Carol", "Alice", "Bob"])
        );
    }

    #[test]
    fn prioritized_hint_words_keep_order_without_ranking() {
        assert_eq!(
            get_prioritized_dictionary_hint_words(&strings(&["A", "B"]), &[], &[]),
            strings(&["A", "B"])
        );
        assert!(get_prioritized_dictionary_hint_words(&[], &[], &strings(&["a"])).is_empty());
    }

    #[test]
    fn whole_dictation_trigger_becomes_saved_text() {
        let s = snip("my email address", "me@example.com");
        for spoken in [
            "My email address.",
            "my email, address",
            "  My e-mail address!  ",
        ] {
            assert_eq!(expand_snippets(spoken, &s), "me@example.com", "{spoken}");
        }
    }

    #[test]
    fn whole_dictation_keeps_turkish_readings() {
        let s = snip("İmza", "SIGNATURE");
        assert_eq!(expand_snippets("İMZA.", &s), "SIGNATURE");
        assert_eq!(expand_snippets("Imza!", &s), "SIGNATURE");
    }

    #[test]
    fn whole_dictation_ignores_triggers_inside_longer_speech() {
        let s = snip("my email address", "me@example.com");
        assert_eq!(
            expand_snippets("Send it to my email, address please.", &s),
            "Send it to my email, address please."
        );
        assert_eq!(
            expand_snippets("Send it to my email address.", &s),
            "Send it to me@example.com."
        );
    }

    #[test]
    fn punctuation_only_trigger_never_matches_empty_bare() {
        let s = snip(":)", "SMILE");
        assert_eq!(expand_snippets("...", &s), "...");
    }
}
