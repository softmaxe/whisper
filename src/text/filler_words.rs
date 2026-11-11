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

fn protected(text: &str, start: usize, end: usize) -> bool {
    let before = &text[..start];
    let after = &text[end..];
    let span_start = before
        .rfind(char::is_whitespace)
        .map_or(0, |i| i + text[i..].chars().next().unwrap().len_utf8());
    let span_end = after
        .find(char::is_whitespace)
        .map_or(text.len(), |i| end + i);
    let span = &text[span_start..span_end];
    span.contains("://")
        || span.contains('@')
        || (span.starts_with('`') && span.ends_with('`'))
        || before.ends_with([
            '.', '/', '\\', ':', '_', '-', '=', '#', '$', '%', '&', '+', '~', '@',
        ])
        || after.starts_with(['/', '\\', ':', '_', '-', '=', '@', '(', '['])
        || after
            .strip_prefix('.')
            .is_some_and(|tail| tail.chars().next().is_some_and(char::is_alphanumeric))
        || after.trim_start_matches(horizontal).starts_with('=')
}

fn punctuation(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | '!' | '?' | '…' | '。' | '，' | '！' | '？' | '、'
    )
}

fn append_kept(out: &mut String, kept: &str, capitalize: &mut bool) {
    for c in kept.chars() {
        if *capitalize && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            *capitalize = false;
        } else {
            out.push(c);
        }
    }
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
            if text[..start].ends_with('打')
                || text[cursor..].starts_with('逆')
                || protected(text, start, cursor)
            {
                continue;
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
                following.is_none_or(|c| punctuation(c) || matches!(c, '-' | '–' | '—'));
            let boundary = text[..start].chars().next_back().is_none_or(|c| !word(c))
                && text[cursor..].chars().next().is_none_or(|c| !word(c));
            if boundary
                && !protected(text, start, cursor)
                && (UNIVERSAL.contains(&lower.as_str())
                    || specific.contains(&lower.as_str())
                    || (auto && hesitation && ENGLISH.contains(&lower.as_str())))
            {
                let capitalize = c.is_uppercase() && sentence_start(&text[..start]);
                ranges.push((start, cursor, capitalize));
            }
        } else {
            cursor += c.len_utf8();
        }
    }
    if ranges.is_empty() {
        return text.to_string();
    }
    let mut merged: Vec<(usize, usize, bool)> = Vec::new();
    for (mut start, mut end, recase) in ranges {
        // Consume the filler-owned punctuation as a run, including ellipses.
        end = text.len() - text[end..].trim_start_matches(punctuation).len();
        let after_spaces = text[end..].trim_start_matches(horizontal);
        if after_spaces.starts_with(['-', '–', '—'])
            && after_spaces.chars().nth(1).is_none_or(char::is_whitespace)
        {
            end = text.len() - after_spaces.trim_start_matches(['-', '–', '—']).len();
        }
        start = text[..start].trim_end_matches(horizontal).len();
        end = text.len() - text[end..].trim_start_matches(horizontal).len();
        if let Some(previous) = merged.last_mut().filter(|previous| start <= previous.1) {
            previous.1 = previous.1.max(end);
            previous.2 |= recase;
        } else {
            merged.push((start, end, recase));
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut previous = 0;
    let mut capitalize = false;
    for (start, end, recase) in merged {
        append_kept(&mut out, &text[previous..start], &mut capitalize);
        let removed = &text[start..end];
        let right = text[end..].chars().next();
        // A terminal after a filler still closes the preceding clause when
        // that clause ended in a comma (e.g. 好的，呃。我们走).
        if out.ends_with([',', '，', '、'])
            && right.is_some_and(char::is_alphanumeric)
            && let Some(terminal @ ('.' | '!' | '?' | '。' | '！' | '？' | '…')) =
                removed.trim_end_matches(horizontal).chars().next_back()
        {
            out.pop();
            out.push(terminal);
        }
        if let (Some(left), Some(right)) = (out.chars().next_back(), right) {
            let separated = removed.chars().any(horizontal);
            let joined_words = word(left)
                && word(right)
                && !(('\u{3400}'..='\u{9fff}').contains(&left)
                    && ('\u{3400}'..='\u{9fff}').contains(&right));
            if !left.is_whitespace()
                && !right.is_whitespace()
                && !punctuation(right)
                && (separated || joined_words)
            {
                out.push(' ');
            }
        }
        capitalize |= recase;
        previous = end;
    }
    append_kept(&mut out, &text[previous..], &mut capitalize);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filtered(text: &str, language: &str) -> String {
        let result = remove_filler_words(text, language);
        assert_eq!(
            remove_filler_words(&result, language),
            result,
            "{text:?}/{language}"
        );
        result
    }

    #[test]
    fn adjacent_removals_and_orphan_punctuation() {
        for (text, expected) in [
            ("foo uh,uh,bar", "foo bar"),
            ("foo uh uh bar", "foo bar"),
            ("foo, uh, um, bar", "foo, bar"),
            ("uh uh uh", ""),
            ("foo uh\nbar", "foo\nbar"),
            ("呃。", ""),
            ("uh!", ""),
            ("Uh... ", ""),
            ("hello. Um... we go", "hello. We go"),
            ("好的，呃。我们走", "好的。我们走"),
            ("Wait, uh, what?", "Wait, what?"),
        ] {
            assert_eq!(filtered(text, "en"), expected, "{text:?}");
        }
        for punctuation in [".", ",", "!", "?", "…", "...", "。", "，", "！", "？", "、"] {
            for token in ["uh", "呃", "um"] {
                assert_eq!(filtered(&format!("{token}{punctuation}"), "en"), "");
                assert_eq!(filtered(&format!("{token}{punctuation}"), "auto"), "");
            }
        }
    }

    #[test]
    fn urls_emails_and_code_are_preserved() {
        for text in [
            "https://uh.edu/a",
            "https://example.com/uh?x",
            "user@hm.com",
            "visit uh.edu today",
            "hm.value",
            "uh_count",
            "uh-oh",
            "um-hum",
            "let uh = 1;",
            "let hm=1;",
            "`uh`",
            "`uh,`",
        ] {
            assert_eq!(filtered(text, "en"), text);
        }
        for prefix in [
            '.', '/', '\\', ':', '_', '-', '=', '#', '$', '%', '&', '+', '~', '@',
        ] {
            let text = format!("{prefix}uh");
            assert_eq!(filtered(&text, "en"), text);
        }
        for suffix in [
            "/", "\\", ":", "_", "-", "=", "@", "(", "[", ".a", ".1", "  = 1",
        ] {
            let text = format!("uh{suffix}");
            assert_eq!(filtered(&text, "en"), text);
        }
        assert_eq!(filtered("um - I think", "auto"), "I think");
        assert_eq!(filtered("Uh, I think so.", "auto"), "I think so.");
    }

    #[test]
    fn universal_and_language_specific_lists() {
        for language in ["auto", "en", "pt", "de", "fr", "zh-CN"] {
            for token in UNIVERSAL {
                assert_eq!(
                    filtered(&format!("{token}, hello"), language),
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
                assert_eq!(filtered(&format!("{token} hello"), language), "hello");
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
            assert_eq!(filtered(text, "auto"), text);
        }
        assert_eq!(filtered("Um, I think", "auto"), "I think");
        assert_eq!(filtered("Um, we should go", "en"), "We should go");
        assert_eq!(
            filtered("hello. Um, we should go", "auto"),
            "hello. We should go"
        );
        assert_eq!(filtered("um carro", "pt-BR"), "um carro");
    }

    #[test]
    fn chinese_runs_and_exceptions() {
        for language in ["en", "auto", "zh-CN"] {
            assert_eq!(filtered("呃呃，我们呃、走吧呃,好", language), "我们走吧好");
            let text = "打呃 呃逆 嗯 啊 额 那个 然后 就是";
            assert_eq!(filtered(text, language), text);
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
            let result = filtered(text, "auto");
            assert_eq!(result, expected, "{text:?}");
            assert_eq!(filtered(&result, "auto"), result);
        }
    }
}
