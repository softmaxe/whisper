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
    // A list marker opening the line starts a sentence too.
    if let Some(lead) = text
        .trim_end_matches(horizontal)
        .strip_suffix(['-', '*', '•'])
        && lead
            .trim_end_matches(horizontal)
            .chars()
            .next_back()
            .is_none_or(|c| c == '\n')
    {
        return true;
    }
    text.trim_end_matches([' ', '\t', '"', '\'', '“', '‘', '('])
        .chars()
        .next_back()
        .is_none_or(|c| matches!(c, '.' | '!' | '?' | '\n' | '。' | '！' | '？'))
}

fn protected(text: &str, code: &[(usize, usize)], start: usize, end: usize) -> bool {
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
        || inside_code(code, start)
        || mentioned(before, after)
        || before.ends_with([
            '.', '/', '\\', ':', '_', '-', '=', '#', '$', '%', '&', '+', '~', '@',
        ])
        || after.starts_with(['/', '\\', ':', '_', '-', '=', '@', '(', '['])
        || after
            .strip_prefix('.')
            .is_some_and(|tail| tail.chars().next().is_some_and(char::is_alphanumeric))
        || after.trim_start_matches(horizontal).starts_with('=')
}

/// A filler wrapped in quotes, brackets or emphasis is being talked about,
/// not hesitated over: `He said "uh" today`, `(hmm)`, `*uh*`.
fn mentioned(before: &str, after: &str) -> bool {
    before.ends_with(['"', '\'', '“', '‘', '(', '[', '*'])
        && after
            .trim_start_matches(punctuation)
            .starts_with(['"', '\'', '”', '’', ')', ']', '*'])
}

/// Inline code spans: a run of backticks up to the next run of the same
/// length on that line, as Markdown pairs them. Found once per pass so the
/// scan stays linear on long transcripts.
fn code_spans(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let run_end = |mut at: usize| {
        while at < bytes.len() && bytes[at] == b'`' {
            at += 1;
        }
        at
    };
    let mut spans = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'`' {
            at += 1;
            continue;
        }
        let open = at;
        at = run_end(at);
        let mut scan = at;
        while scan < bytes.len() && bytes[scan] != b'\n' {
            if bytes[scan] != b'`' {
                scan += 1;
                continue;
            }
            let close = run_end(scan);
            if close - scan == at - open {
                spans.push((open, close));
                at = close;
                break;
            }
            scan = close;
        }
    }
    spans
}

fn inside_code(code: &[(usize, usize)], at: usize) -> bool {
    code.get(code.partition_point(|span| span.1 <= at))
        .is_some_and(|span| span.0 <= at)
}

fn punctuation(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | ';' | '!' | '?' | '…' | '。' | '，' | '；' | '！' | '？' | '、'
    )
}

/// Copies surviving text, capitalizing the sentence a removed filler used
/// to open. Only a plain word is recased: code, numbers, and names with their
/// own casing (`iPhone`, `my_var`) are left as written.
fn append_kept(out: &mut String, kept: &str, capitalize: &mut bool) {
    for (at, c) in kept.char_indices() {
        if !*capitalize {
            out.push_str(&kept[at..]);
            return;
        }
        if c.is_alphabetic() {
            let rest = &kept[at + c.len_utf8()..];
            let own_casing = rest
                .chars()
                .take_while(|c| word(*c))
                .any(|c| c.is_uppercase() || c.is_numeric() || c == '_');
            if own_casing {
                out.push(c);
            } else {
                out.extend(c.to_uppercase());
            }
            *capitalize = false;
        } else {
            *capitalize = c.is_whitespace() || matches!(c, '"' | '\'' | '“' | '‘' | '(');
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
    let code = code_spans(text);
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
                || protected(text, &code, start, cursor)
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
                && (UNIVERSAL.contains(&lower.as_str())
                    || specific.contains(&lower.as_str())
                    || (auto && hesitation && ENGLISH.contains(&lower.as_str())))
                && !protected(text, &code, start, cursor)
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
    let mut groups: Vec<Group> = Vec::new();
    for (token_start, token_end, recase) in ranges {
        let mut end = token_end;
        let mut terminal = None;
        // Take every pause mark the filler brought with it, in any order
        // (`uh, ... we go`, `uh—, hello`).
        loop {
            let from = skip_horizontal(text, end);
            let run = punctuation_run(text, from);
            let next = text[run..].chars().next();
            // Punctuation set off by a space is the filler's only when it
            // stands alone (`uh , hello`); `uh .env` keeps its dot.
            if run > from && (from == end || next.is_none_or(char::is_whitespace)) {
                terminal = terminal_in(text, from, run).or(terminal);
                end = run;
                continue;
            }
            let mut rest = text[from..].chars();
            // A hyphen must stand apart to be a pause (`uh-oh` is a word). The
            // long dashes pause even when set tight (`Uh—we go`), but before
            // a digit one may be a minus sign.
            if let Some(dash) = rest.next().filter(|c| matches!(c, '-' | '–' | '—')) {
                let after = rest.next();
                let pause = if dash == '-' {
                    after.is_none_or(char::is_whitespace)
                } else {
                    !after.is_some_and(char::is_numeric)
                };
                if pause {
                    end = from + dash.len_utf8();
                    continue;
                }
            }
            break;
        }
        let start = text[..token_start].trim_end_matches(horizontal).len();
        let end = skip_horizontal(text, end);
        if let Some(previous) = groups.last_mut().filter(|previous| start <= previous.end) {
            previous.end = previous.end.max(end);
            previous.recase |= recase;
            previous.terminal = terminal.or(previous.terminal);
        } else {
            groups.push(Group {
                start,
                end,
                recase,
                terminal,
            });
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut previous = 0;
    let mut capitalize = false;
    for group in groups {
        append_kept(&mut out, &text[previous..group.start], &mut capitalize);
        let right = text[group.end..].chars().next();
        let left = out.chars().next_back();
        // The whole group goes, punctuation included. What stays at the seam
        // is the strongest mark the surviving clause still needs: a terminal
        // spoken after the filler closes the clause before it, replacing that
        // clause's comma; a comma was only ever the filler's own. Punctuation
        // already standing before the group is the text's and always stays.
        let terminal: String = group
            .terminal
            .map(|(start, end)| text[start..end].chars().filter(|c| terminal(*c)).collect())
            .unwrap_or_default();
        match left {
            Some(c) if comma(c) && !terminal.is_empty() => {
                out.pop();
                out.push_str(&terminal);
            }
            Some(c) if word(c) || matches!(c, '"' | '\'' | '”' | '’' | ')' | ']') => {
                out.push_str(&terminal);
            }
            _ => {}
        }
        if let (Some(left), Some(right)) = (out.chars().next_back(), right) {
            let removed = &text[group.start..group.end];
            // Space that led into the filler still separates what surrounds
            // it; space that only trailed it has no place after an opener.
            let separated = removed.starts_with(horizontal)
                || (removed.ends_with(horizontal)
                    && !matches!(left, '"' | '\'' | '“' | '‘' | '(' | '['));
            let joined_words = word(left) && word(right) && !(cjk(left) && cjk(right));
            if !left.is_whitespace() && !right.is_whitespace() && (separated || joined_words) {
                out.push(' ');
            }
        }
        capitalize |= group.recase;
        previous = group.end;
    }
    append_kept(&mut out, &text[previous..], &mut capitalize);
    out
}

/// One or more fillers removed together, with the punctuation and spacing
/// that belonged to them.
struct Group {
    start: usize,
    end: usize,
    recase: bool,
    /// The last sentence-ending punctuation run inside the group.
    terminal: Option<(usize, usize)>,
}

fn comma(c: char) -> bool {
    matches!(c, ',' | ';' | '，' | '；' | '、')
}

fn terminal(c: char) -> bool {
    punctuation(c) && !comma(c)
}

fn cjk(c: char) -> bool {
    ('\u{3400}'..='\u{9fff}').contains(&c)
}

fn skip_horizontal(text: &str, from: usize) -> usize {
    text.len() - text[from..].trim_start_matches(horizontal).len()
}

fn punctuation_run(text: &str, from: usize) -> usize {
    text.len() - text[from..].trim_start_matches(punctuation).len()
}

fn terminal_in(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    text[start..end].contains(terminal).then_some((start, end))
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
    fn seam_keeps_the_punctuation_the_surviving_text_needs() {
        for (text, expected) in [
            ("好的呃。我们走", "好的。我们走"),
            ("好的，呃。", "好的。"),
            ("好的，呃。uh,我们走", "好的。我们走"),
            ("Uh ... we go", "We go"),
            ("uh , hello", "hello"),
            ("so uh... we go", "so... we go"),
            ("I was... uh... going", "I was... going"),
            ("I think so uh.", "I think so."),
            ("Wait, uh", "Wait,"),
            ("Dear John, uh\nThank you.", "Dear John,\nThank you."),
            ("Dear John, uh\r\nThank you.", "Dear John,\r\nThank you."),
            ("Uh, uh, ... we go", "We go"),
            ("uh—, hello", "hello"),
            ("It is uh –5 degrees.", "It is –5 degrees."),
            ("- Uh, `myVariable` is it.", "- `myVariable` is it."),
            ("Uh, iPhone works.", "iPhone works."),
            ("Uh, my_var works.", "my_var works."),
            ("Really? Uh, yes.", "Really? Yes."),
            ("foo uh, bar", "foo bar"),
            ("line one\nuh, line two", "line one\nline two"),
            ("see uh .env here", "see .env here"),
            ("\"Uh, we go\"", "\"We go\""),
            ("- Uh, we go\n- Um, stay", "- We go\n- Stay"),
            ("Uh, 3.5 is right", "3.5 is right"),
            ("uh; bar", "bar"),
            ("Uh—we go", "We go"),
            ("foo uh!uh?bar", "foo? bar"),
        ] {
            assert_eq!(filtered(text, "en"), expected, "{text:?}");
        }
    }

    #[test]
    fn inline_code_is_preserved_whatever_surrounds_the_backticks() {
        for text in [
            "`uh`, hello",
            "(`uh`) hello",
            "`say uh now`",
            "run `hm` then `uh` ok",
            "``uh``",
            "```uh```",
            "``a ` uh``",
        ] {
            assert_eq!(filtered(text, "en"), text);
        }
        for text in [
            "He said \"uh\" today",
            "He said \"uh,\" today",
            "(hmm) hello",
            "*uh* hello",
        ] {
            assert_eq!(filtered(text, "en"), text);
        }
        // An unmatched backtick protects nothing.
        assert_eq!(filtered("it`s uh, fine", "en"), "it`s fine");
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
