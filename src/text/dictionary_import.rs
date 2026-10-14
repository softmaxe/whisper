//! Dictionary import/export text formats.
//!
//! Exports are one word per line, UTF-8, no trailing newline, default file
//! name `dictionary.txt`.

/// Default file name offered by the export dialog.
pub const DICTIONARY_EXPORT_FILE_NAME: &str = "dictionary.txt";

/// Split a bulk-import paste or file into dictionary words. Accepts
/// comma-separated values, one word per line, or a mix of both.
pub fn parse_dictionary_import_text(text: &str) -> Vec<String> {
    text.split([',', '\n'])
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// The exported file's content: words joined with "\n".
pub fn format_dictionary_export<S: AsRef<str>>(words: &[S]) -> String {
    words
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comma_separated() {
        assert_eq!(
            parse_dictionary_import_text("Alice, Bob, Carol"),
            ["Alice", "Bob", "Carol"]
        );
    }

    #[test]
    fn one_per_line() {
        assert_eq!(
            parse_dictionary_import_text("Alice\nBob\nCarol"),
            ["Alice", "Bob", "Carol"]
        );
        assert_eq!(
            parse_dictionary_import_text("Alice\r\nBob\r\n"),
            ["Alice", "Bob"]
        );
    }

    #[test]
    fn mixed_and_blank() {
        assert_eq!(
            parse_dictionary_import_text("Alice,\n Bob\n\nCarol, "),
            ["Alice", "Bob", "Carol"]
        );
        assert!(parse_dictionary_import_text("").is_empty());
    }

    #[test]
    fn export_round_trips() {
        let words = ["Alice", "Claude Code", "章珊"];
        let exported = format_dictionary_export(&words);
        assert_eq!(exported, "Alice\nClaude Code\n章珊");
        assert_eq!(parse_dictionary_import_text(&exported), words);
    }
}
