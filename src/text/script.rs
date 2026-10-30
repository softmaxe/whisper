//! Unicode Script property checks for the scripts the text helpers care about.
//!
//! Rust's `regex` lacks `Script=` lookups for these without bloating
//! patterns, so the assigned ranges (Unicode 16) are listed here directly.

pub(crate) fn is_han(c: char) -> bool {
    matches!(c,
        '\u{2E80}'..='\u{2E99}' | '\u{2E9B}'..='\u{2EF3}' | '\u{2F00}'..='\u{2FD5}'
        | '\u{3005}' | '\u{3007}' | '\u{3021}'..='\u{3029}' | '\u{3038}'..='\u{303B}'
        | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FA6D}'
        | '\u{FA70}'..='\u{FAD9}' | '\u{16FE2}' | '\u{16FE3}' | '\u{16FF0}' | '\u{16FF1}'
        | '\u{20000}'..='\u{2A6DF}' | '\u{2A700}'..='\u{2EE5D}' | '\u{2F800}'..='\u{2FA1D}'
        | '\u{30000}'..='\u{323AF}')
}

/// Scripts written without spaces between words.
pub(crate) fn is_unspaced_script(c: char) -> bool {
    is_han(c) || is_hiragana(c) || is_katakana(c)
}

pub(crate) fn is_hiragana(c: char) -> bool {
    matches!(c,
        '\u{3041}'..='\u{3096}' | '\u{309D}'..='\u{309F}' | '\u{1B001}'..='\u{1B11F}'
        | '\u{1B132}' | '\u{1B150}'..='\u{1B152}' | '\u{1F200}')
}

pub(crate) fn is_katakana(c: char) -> bool {
    matches!(c,
        '\u{30A1}'..='\u{30FA}' | '\u{30FD}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}'
        | '\u{32D0}'..='\u{32FE}' | '\u{3300}'..='\u{3357}' | '\u{FF66}'..='\u{FF6F}'
        | '\u{FF71}'..='\u{FF9D}' | '\u{1AFF0}'..='\u{1AFFE}' | '\u{1B000}'
        | '\u{1B120}'..='\u{1B122}' | '\u{1B155}' | '\u{1B164}'..='\u{1B167}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_cjk_scripts() {
        assert!(is_han('中') && is_han('〇') && is_han('々') && is_han('𠀀'));
        assert!(!is_han('あ') && !is_han('a') && !is_han('ー'));
        assert!(is_hiragana('あ') && !is_hiragana('ア'));
        assert!(is_katakana('ア') && is_katakana('ｱ') && !is_katakana('ー'));
    }
}
