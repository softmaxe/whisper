//! Conservative, deterministic hesitation removal, independent of cleanup.

const UNIVERSAL: &[&str] = &[
    "uh", "uhm", "umm", "uhh", "uhhh", "ehh", "ehm", "ahm", "hmm", "hm", "mmm", "хм", "ммм",
];
const ENGLISH: &[&str] = &["um", "ah", "eh"];

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn horizontal(c: char) -> bool {
    matches!(c, ' ' | '\t')
}

fn sentence_start(text: &str) -> bool {
    text.trim_end_matches([' ', '\t', '"', '\'', '“', '‘', '('])
        .chars()
        .next_back()
        .is_none_or(|c| matches!(c, '.' | '!' | '?' | '\n' | '。' | '！' | '？'))
}

/// A missing language is Auto; short English words then require punctuation
/// to avoid deleting ordinary words in other languages (Portuguese `um`).
pub fn remove_filler_words(text: &str, language: &str) -> String {
    let base = language
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_lowercase();
    let auto = base.is_empty() || base == "auto";
    let specific: &[&str] = match base.as_str() {
        "en" => ENGLISH,
        "de" => &["äh", "ähm"],
        "fr" => &["euh"],
        _ => &[],
    };
    let mut current = text.to_string();
    // Removing an adjacent filler can expose the punctuation qualifying an
    // Auto English token. Finish that work here so the pass is idempotent.
    loop {
        let next = remove_pass(&current, specific, auto);
        if next == current {
            return next;
        }
        current = next;
    }
}

fn remove_pass(text: &str, specific: &[&str], auto: bool) -> String {
    let mut ranges: Vec<(usize, usize, bool)> = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let Some(c) = text[cursor..].chars().next() else {
            break;
        };
        if c == '呃' {
            let start = cursor;
            while text[cursor..].starts_with('呃') {
                cursor += '呃'.len_utf8();
            }
            if text[..start].ends_with('打') || text[cursor..].starts_with('逆') {
                continue;
            }
            if let Some(c @ ('，' | '、' | ',')) = text[cursor..].chars().next() {
                cursor += c.len_utf8();
            }
            ranges.push((start, cursor, false));
        } else if word(c) {
            let start = cursor;
            cursor += c.len_utf8();
            while let Some(c) = text[cursor..]
                .chars()
                .next()
                .filter(|c| word(*c) && *c != '呃')
            {
                cursor += c.len_utf8();
            }
            let token = &text[start..cursor];
            let lower = token.to_lowercase();
            let following = text[cursor..].trim_start_matches(horizontal).chars().next();
            let hesitation =
                following.is_none_or(|c| matches!(c, ',' | '.' | '…' | '-' | '–' | '—'));
            let boundary = text[..start].chars().next_back().is_none_or(|c| !word(c))
                && text[cursor..].chars().next().is_none_or(|c| !word(c));
            if boundary
                && (UNIVERSAL.contains(&lower.as_str())
                    || specific.contains(&lower.as_str())
                    || (auto && hesitation && ENGLISH.contains(&lower.as_str())))
            {
                let capitalize = c.is_uppercase() && sentence_start(&text[..start]);
                if text[cursor..].starts_with([',', '.']) {
                    cursor += 1;
                }
                ranges.push((start, cursor, capitalize));
            }
        } else {
            cursor += c.len_utf8();
        }
    }
    if ranges.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut previous = 0;
    let mut capitalize = false;
    let mut after_removal = false;
    for (start, end, recase) in
        ranges
            .into_iter()
            .chain(std::iter::once((text.len(), text.len(), false)))
    {
        let mut kept = &text[previous..start];
        if after_removal {
            let whitespace = kept.len() - kept.trim_start_matches(horizontal).len();
            if out.ends_with(horizontal) {
                let trimmed = out.trim_end_matches(horizontal).len();
                out.truncate(trimmed);
                if !out.is_empty() && !out.ends_with('\n') && whitespace < kept.len() {
                    out.push(' ');
                }
                kept = &kept[whitespace..];
            } else if out.is_empty() || out.ends_with('\n') {
                kept = &kept[whitespace..];
            }
            if out.is_empty() || out.ends_with('\n') {
                kept = kept
                    .trim_start_matches([',', '.', '…', '-', '–', '—', '，', '、'])
                    .trim_start_matches(horizontal);
            }
        }
        for c in kept.chars() {
            if capitalize && c.is_alphabetic() {
                out.extend(c.to_uppercase());
                capitalize = false;
            } else {
                out.push(c);
            }
        }
        capitalize |= recase;
        previous = end;
        after_removal = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn universal_and_language_specific_lists() {
        for language in ["auto", "en", "pt", "de", "fr", "zh-CN"] {
            for token in UNIVERSAL {
                assert_eq!(
                    remove_filler_words(&format!("{token}, hello"), language),
                    "hello",
                    "{token}/{language}"
                );
            }
        }
        for (language, tokens) in [
            ("en-US", ENGLISH),
            ("de-DE", &["äh", "ähm"][..]),
            ("fr", &["euh"][..]),
        ] {
            for token in tokens {
                assert_eq!(
                    remove_filler_words(&format!("{token} hello"), language),
                    "hello"
                );
            }
        }
    }

    #[test]
    fn boundaries_and_auto_preserve_ordinary_words() {
        for text in [
            "umbrella",
            "hmmm-like",
            "ahead",
            "uh_thing",
            "1uh",
            "éuh",
            "um carro",
            "ah bon",
            "euh oui",
        ] {
            assert_eq!(remove_filler_words(text, "auto"), text);
        }
        assert_eq!(remove_filler_words("Um, I think", "auto"), "I think");
        assert_eq!(
            remove_filler_words("Um, we should go", "en"),
            "We should go"
        );
        assert_eq!(
            remove_filler_words("hello. Um, we should go", "auto"),
            "hello. We should go"
        );
        assert_eq!(remove_filler_words("um carro", "pt-BR"), "um carro");
    }

    #[test]
    fn chinese_runs_and_exceptions() {
        for language in ["en", "auto", "zh-CN"] {
            assert_eq!(
                remove_filler_words("呃呃，我们呃、走吧呃,好", language),
                "我们走吧好"
            );
            let text = "打呃 呃逆 嗯 啊 额 那个 然后 就是";
            assert_eq!(remove_filler_words(text, language), text);
        }
    }

    #[test]
    fn whitespace_punctuation_and_idempotence() {
        for (text, expected) in [
            ("a  b\tc", "a  b\tc"),
            ("a  uh, \t b\nUm, we go\n\nuh", "a b\nWe go\n\n"),
            ("Um... we go", "We go"),
            ("um uh,", ""),
            ("uh, uhm. 呃，", ""),
        ] {
            let result = remove_filler_words(text, "auto");
            assert_eq!(result, expected, "{text:?}");
            assert_eq!(remove_filler_words(&result, "auto"), result);
        }
    }
}
