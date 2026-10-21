//! The mapping from accelerators ([`Hotkey::accelerator`]) to `global-hotkey`
//! key combinations.
//!
//! [`Hotkey::accelerator`]: crate::text::hotkeys::Hotkey::accelerator

use global_hotkey::hotkey::{Code, HotKey, Modifiers};

fn modifier_flag(token: &str) -> Option<Modifiers> {
    match token {
        "Command" => Some(Modifiers::SUPER),
        "Control" => Some(Modifiers::CONTROL),
        "Alt" => Some(Modifiers::ALT),
        "Shift" => Some(Modifiers::SHIFT),
        _ => None,
    }
}

fn punctuation_code(c: char) -> Option<(Code, bool)> {
    // (code, needs shift) on the US layout.
    let mapped = match c {
        ',' => (Code::Comma, false),
        '.' => (Code::Period, false),
        '/' => (Code::Slash, false),
        ';' => (Code::Semicolon, false),
        '\'' => (Code::Quote, false),
        '[' => (Code::BracketLeft, false),
        ']' => (Code::BracketRight, false),
        '\\' => (Code::Backslash, false),
        '`' => (Code::Backquote, false),
        '-' => (Code::Minus, false),
        '=' => (Code::Equal, false),
        '<' => (Code::Comma, true),
        '>' => (Code::Period, true),
        '?' => (Code::Slash, true),
        ':' => (Code::Semicolon, true),
        '"' => (Code::Quote, true),
        '{' => (Code::BracketLeft, true),
        '}' => (Code::BracketRight, true),
        '|' => (Code::Backslash, true),
        '~' => (Code::Backquote, true),
        '_' => (Code::Minus, true),
        '+' => (Code::Equal, true),
        '!' => (Code::Digit1, true),
        '@' => (Code::Digit2, true),
        '#' => (Code::Digit3, true),
        '$' => (Code::Digit4, true),
        '%' => (Code::Digit5, true),
        '^' => (Code::Digit6, true),
        '&' => (Code::Digit7, true),
        '*' => (Code::Digit8, true),
        '(' => (Code::Digit9, true),
        ')' => (Code::Digit0, true),
        _ => return None,
    };
    Some(mapped)
}

fn key_code(token: &str) -> Option<(Code, bool)> {
    let mut chars = token.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphabetic() {
            return letter_code(c.to_ascii_uppercase()).map(|code| (code, false));
        }
        if let Some(digit) = c.to_digit(10) {
            return Some((digit_code(digit), false));
        }
        return punctuation_code(c);
    }

    let upper = token.to_ascii_uppercase();
    if let Some(n) = upper.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
        return function_code(n).map(|code| (code, false));
    }
    if let Some(n) = upper
        .strip_prefix("NUMPAD")
        .or_else(|| upper.strip_prefix("NUM"))
        .and_then(|n| n.parse::<u32>().ok())
    {
        let code = match n {
            0 => Code::Numpad0,
            1 => Code::Numpad1,
            2 => Code::Numpad2,
            3 => Code::Numpad3,
            4 => Code::Numpad4,
            5 => Code::Numpad5,
            6 => Code::Numpad6,
            7 => Code::Numpad7,
            8 => Code::Numpad8,
            9 => Code::Numpad9,
            _ => return None,
        };
        return Some((code, false));
    }
    if let Some(name) = upper.strip_prefix("KEY").filter(|n| n.len() == 1) {
        return name
            .chars()
            .next()
            .and_then(letter_code)
            .map(|c| (c, false));
    }
    if let Some(n) = upper
        .strip_prefix("DIGIT")
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n <= 9)
    {
        return Some((digit_code(n), false));
    }

    let code = match upper.as_str() {
        "PLUS" => return Some((Code::Equal, true)),
        "SPACE" => Code::Space,
        "TAB" => Code::Tab,
        "BACKSPACE" => Code::Backspace,
        "DELETE" => Code::Delete,
        "INSERT" => Code::Insert,
        "ENTER" => Code::Enter,
        "UP" => Code::ArrowUp,
        "DOWN" => Code::ArrowDown,
        "LEFT" => Code::ArrowLeft,
        "RIGHT" => Code::ArrowRight,
        "HOME" => Code::Home,
        "END" => Code::End,
        "PAGEUP" => Code::PageUp,
        "PAGEDOWN" => Code::PageDown,
        "ESC" => Code::Escape,
        "CAPSLOCK" => Code::CapsLock,
        "NUMLOCK" => Code::NumLock,
        "SCROLLLOCK" => Code::ScrollLock,
        "PAUSE" => Code::Pause,
        "PRINTSCREEN" => Code::PrintScreen,
        "VOLUMEUP" | "AUDIOVOLUMEUP" => Code::AudioVolumeUp,
        "VOLUMEDOWN" | "AUDIOVOLUMEDOWN" => Code::AudioVolumeDown,
        "VOLUMEMUTE" | "AUDIOVOLUMEMUTE" => Code::AudioVolumeMute,
        "MEDIAPLAYPAUSE" => Code::MediaPlayPause,
        "MEDIANEXTTRACK" | "MEDIATRACKNEXT" => Code::MediaTrackNext,
        "MEDIAPREVIOUSTRACK" | "MEDIATRACKPREVIOUS" => Code::MediaTrackPrevious,
        "MEDIASTOP" => Code::MediaStop,
        "NUMDEC" | "NUMPADDECIMAL" => Code::NumpadDecimal,
        "NUMADD" | "NUMPADADD" => Code::NumpadAdd,
        "NUMSUB" | "NUMPADSUBTRACT" => Code::NumpadSubtract,
        "NUMMULT" | "NUMPADMULTIPLY" => Code::NumpadMultiply,
        "NUMDIV" | "NUMPADDIVIDE" => Code::NumpadDivide,
        "COMMA" => Code::Comma,
        "PERIOD" => Code::Period,
        "SLASH" => Code::Slash,
        "SEMICOLON" => Code::Semicolon,
        "QUOTE" => Code::Quote,
        "BRACKETLEFT" => Code::BracketLeft,
        "BRACKETRIGHT" => Code::BracketRight,
        "BACKSLASH" => Code::Backslash,
        "BACKQUOTE" => Code::Backquote,
        "MINUS" => Code::Minus,
        "EQUAL" => Code::Equal,
        _ => return None,
    };
    Some((code, false))
}

fn letter_code(c: char) -> Option<Code> {
    let code = match c {
        'A' => Code::KeyA,
        'B' => Code::KeyB,
        'C' => Code::KeyC,
        'D' => Code::KeyD,
        'E' => Code::KeyE,
        'F' => Code::KeyF,
        'G' => Code::KeyG,
        'H' => Code::KeyH,
        'I' => Code::KeyI,
        'J' => Code::KeyJ,
        'K' => Code::KeyK,
        'L' => Code::KeyL,
        'M' => Code::KeyM,
        'N' => Code::KeyN,
        'O' => Code::KeyO,
        'P' => Code::KeyP,
        'Q' => Code::KeyQ,
        'R' => Code::KeyR,
        'S' => Code::KeyS,
        'T' => Code::KeyT,
        'U' => Code::KeyU,
        'V' => Code::KeyV,
        'W' => Code::KeyW,
        'X' => Code::KeyX,
        'Y' => Code::KeyY,
        'Z' => Code::KeyZ,
        _ => return None,
    };
    Some(code)
}

fn digit_code(n: u32) -> Code {
    match n {
        0 => Code::Digit0,
        1 => Code::Digit1,
        2 => Code::Digit2,
        3 => Code::Digit3,
        4 => Code::Digit4,
        5 => Code::Digit5,
        6 => Code::Digit6,
        7 => Code::Digit7,
        8 => Code::Digit8,
        _ => Code::Digit9,
    }
}

fn function_code(n: u8) -> Option<Code> {
    let code = match n {
        1 => Code::F1,
        2 => Code::F2,
        3 => Code::F3,
        4 => Code::F4,
        5 => Code::F5,
        6 => Code::F6,
        7 => Code::F7,
        8 => Code::F8,
        9 => Code::F9,
        10 => Code::F10,
        11 => Code::F11,
        12 => Code::F12,
        13 => Code::F13,
        14 => Code::F14,
        15 => Code::F15,
        16 => Code::F16,
        17 => Code::F17,
        18 => Code::F18,
        19 => Code::F19,
        20 => Code::F20,
        21 => Code::F21,
        22 => Code::F22,
        23 => Code::F23,
        24 => Code::F24,
        _ => return None,
    };
    Some(code)
}

/// Maps an accelerator ("Command+Shift+Space", "Control+,", "Control++",
/// "F13") to a `global-hotkey` combination.
pub fn parse_accelerator(accelerator: &str) -> Result<HotKey, String> {
    let trimmed = accelerator.trim();
    if trimmed.is_empty() {
        return Err("empty accelerator".into());
    }
    // The key is the last token; "+" as the key leaves "++" at the end.
    let (modifier_part, key_token) = if trimmed == "+" {
        ("", "+")
    } else if let Some(prefix) = trimmed.strip_suffix("++") {
        (prefix, "+")
    } else {
        match trimmed.rfind('+') {
            Some(index) => (&trimmed[..index], &trimmed[index + 1..]),
            None => ("", trimmed),
        }
    };

    let mut modifiers = Modifiers::empty();
    if !modifier_part.is_empty() {
        for token in modifier_part.split('+') {
            let token = token.trim();
            match modifier_flag(token) {
                Some(flag) => modifiers |= flag,
                None => {
                    return Err(format!(
                        "\"{token}\" is not a modifier in \"{accelerator}\""
                    ));
                }
            }
        }
    }

    let key_token = key_token.trim();
    if modifier_flag(key_token).is_some() {
        return Err(format!("\"{accelerator}\" has no key besides modifiers"));
    }
    let (code, shifted) =
        key_code(key_token).ok_or_else(|| format!("unsupported key \"{key_token}\""))?;
    if shifted {
        modifiers |= Modifiers::SHIFT;
    }
    Ok(HotKey::new(
        if modifiers.is_empty() {
            None
        } else {
            Some(modifiers)
        },
        code,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::hotkeys::Hotkey;

    fn hk(mods: Modifiers, code: Code) -> HotKey {
        HotKey::new(if mods.is_empty() { None } else { Some(mods) }, code)
    }

    fn parse(hotkey: &str) -> Result<HotKey, String> {
        let accelerator = Hotkey::parse(hotkey)
            .and_then(|h| h.accelerator())
            .ok_or_else(|| format!("{hotkey} has no accelerator"))?;
        parse_accelerator(&accelerator)
    }

    #[test]
    fn maps_hotkey_accelerators() {
        let cases: [(&str, HotKey); 15] = [
            (
                "CommandOrControl+Shift+Space",
                hk(Modifiers::SUPER | Modifiers::SHIFT, Code::Space),
            ),
            (
                "Control+Alt+D",
                hk(Modifiers::CONTROL | Modifiers::ALT, Code::KeyD),
            ),
            ("F13", hk(Modifiers::empty(), Code::F13)),
            ("Control+,", hk(Modifiers::CONTROL, Code::Comma)),
            (
                "Control++",
                hk(Modifiers::CONTROL | Modifiers::SHIFT, Code::Equal),
            ),
            (
                "Command+Plus",
                hk(Modifiers::SUPER | Modifiers::SHIFT, Code::Equal),
            ),
            ("Alt+/", hk(Modifiers::ALT, Code::Slash)),
            (
                "Control+Shift+;",
                hk(Modifiers::CONTROL | Modifiers::SHIFT, Code::Semicolon),
            ),
            ("Escape", hk(Modifiers::empty(), Code::Escape)),
            ("Ctrl+ArrowUp", hk(Modifiers::CONTROL, Code::ArrowUp)),
            ("Ctrl+Return", hk(Modifiers::CONTROL, Code::Enter)),
            ("ControlLeft+K", hk(Modifiers::CONTROL, Code::KeyK)),
            ("RightCommand+K", hk(Modifiers::SUPER, Code::KeyK)),
            ("AltGr+`", hk(Modifiers::ALT, Code::Backquote)),
            ("Control+Num5", hk(Modifiers::CONTROL, Code::Numpad5)),
        ];
        for (hotkey, expected) in cases {
            let parsed = parse(hotkey).unwrap();
            assert_eq!(
                (parsed.mods, parsed.key),
                (expected.mods, expected.key),
                "{hotkey}"
            );
        }
    }

    #[test]
    fn rejects_unrepresentable_accelerators() {
        assert!(parse("Control+Alt").is_err());
        assert!(parse("Control+BAD").is_err());
        assert!(parse("Hyper+K").is_err());
        assert!(parse_accelerator("").is_err());
    }
}
