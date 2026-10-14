//! Hotkey classification (which path observes a key) and the mapping from
//! accelerator strings to `global-hotkey` key combinations.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};

const RIGHT_SIDE_MODIFIERS: [&str; 10] = [
    "control", "ctrl", "alt", "option", "shift", "command", "cmd", "super", "meta", "win",
];

/// Right-side single modifiers ("RightOption") are watched by the native
/// listener, not registered as shortcuts.
pub fn is_right_side_modifier(hotkey: &str) -> bool {
    right_modifier_base(hotkey).is_some()
}

/// "RightOption" -> "option"; `None` for anything else.
fn right_modifier_base(hotkey: &str) -> Option<String> {
    let base = strip_prefix_ignore_case(hotkey, "right")?;
    RIGHT_SIDE_MODIFIERS
        .iter()
        .any(|name| base.eq_ignore_ascii_case(name))
        .then(|| base.to_ascii_lowercase())
}

fn strip_prefix_ignore_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &value[prefix.len()..])
}

const MODIFIER_NAMES: [&str; 12] = [
    "control",
    "ctrl",
    "alt",
    "option",
    "shift",
    "super",
    "meta",
    "win",
    "command",
    "cmd",
    "commandorcontrol",
    "cmdorctrl",
];

/// Chords made only of modifiers ("Control+Super") have no ordinary key.
pub fn is_modifier_only_hotkey(hotkey: &str) -> bool {
    hotkey.contains('+')
        && hotkey
            .split('+')
            .all(|part| MODIFIER_NAMES.contains(&part.to_ascii_lowercase().as_str()))
}

pub fn is_globe_like_hotkey(hotkey: &str) -> bool {
    hotkey == "GLOBE" || hotkey == "Fn"
}

pub fn is_mouse_button_hotkey(hotkey: &str) -> bool {
    hotkey.eq_ignore_ascii_case("MouseButton4") || hotkey.eq_ignore_ascii_case("MouseButton5")
}

/// Hotkeys the native listener reports (and whose release it reports).
pub fn uses_native_listener(hotkey: &str) -> bool {
    is_globe_like_hotkey(hotkey) || is_mouse_button_hotkey(hotkey) || is_right_side_modifier(hotkey)
}

/// macOS only reports a release for keys the native listener watches and for
/// modifier chords. A lone regular key autorepeats and never reports release.
pub fn lacks_mac_release_signal(hotkey: &str) -> bool {
    !hotkey.contains('+') && !uses_native_listener(hotkey)
}

/// Hold mode needs a key whose release is visible.
pub fn supports_push_to_talk(hotkey: Option<&str>) -> bool {
    !hotkey.is_some_and(lacks_mac_release_signal)
}

pub fn is_unsupported_fn_combination(hotkey: &str) -> bool {
    strip_prefix_ignore_case(hotkey, "fn+").is_some()
}

/// Name the native listener uses for a right-side modifier
/// (`RIGHT_MOD_DOWN:RightOption`), so "RightAlt" matches it too.
pub fn canonical_listener_key(hotkey: &str) -> String {
    if let Some(base) = right_modifier_base(hotkey) {
        let name = match base.as_str() {
            "control" | "ctrl" => "RightControl",
            "alt" | "option" => "RightOption",
            "shift" => "RightShift",
            _ => "RightCommand",
        };
        return name.to_string();
    }
    if is_mouse_button_hotkey(hotkey) {
        return format!("MouseButton{}", &hotkey[hotkey.len() - 1..]);
    }
    if is_globe_like_hotkey(hotkey) {
        return "GLOBE".to_string();
    }
    hotkey.to_string()
}

/// Right-side modifiers register as their generic accelerator modifier.
pub fn normalize_to_accelerator(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(|part| match right_modifier_base(part).as_deref() {
            Some("control" | "ctrl") => "Control".to_string(),
            Some("alt" | "option") => "Alt".to_string(),
            Some("shift") => "Shift".to_string(),
            Some(_) => "Command".to_string(),
            None => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Modifiers whose release ends a Hold mode key chord, as reported by the
/// listener's `MODIFIER_UP:<name>` (and `fn` from `FN_UP`).
pub fn required_modifiers(hotkey: &str) -> Vec<&'static str> {
    let mut required = Vec::new();
    for part in hotkey.split('+').map(str::trim) {
        let modifier = match part {
            "Command" | "Cmd" | "RightCommand" | "RightCmd" | "CommandOrControl" | "Super"
            | "Meta" => "command",
            "Control" | "Ctrl" | "RightControl" | "RightCtrl" => "control",
            "Alt" | "Option" | "RightAlt" | "RightOption" => "option",
            "Shift" | "RightShift" => "shift",
            "Fn" => "fn",
            _ => continue,
        };
        if !required.contains(&modifier) {
            required.push(modifier);
        }
    }
    required
}

fn modifier_flag(token: &str) -> Option<Modifiers> {
    let lowered = token.to_ascii_lowercase().replace(['-', '_', ' '], "");
    let base = lowered
        .strip_prefix("left")
        .or_else(|| lowered.strip_prefix("right"))
        .or_else(|| lowered.strip_suffix("left"))
        .or_else(|| lowered.strip_suffix("right"))
        .unwrap_or(&lowered);
    match base {
        "command" | "cmd" | "super" | "meta" | "win" | "commandorcontrol" | "cmdorctrl"
        | "commandorctrl" | "cmdorcontrol" => Some(Modifiers::SUPER),
        "control" | "ctrl" => Some(Modifiers::CONTROL),
        "alt" | "option" | "altgr" => Some(Modifiers::ALT),
        "shift" => Some(Modifiers::SHIFT),
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
        "DELETE" | "DEL" => Code::Delete,
        "INSERT" | "INS" => Code::Insert,
        "RETURN" | "ENTER" => Code::Enter,
        "UP" | "ARROWUP" => Code::ArrowUp,
        "DOWN" | "ARROWDOWN" => Code::ArrowDown,
        "LEFT" | "ARROWLEFT" => Code::ArrowLeft,
        "RIGHT" | "ARROWRIGHT" => Code::ArrowRight,
        "HOME" => Code::Home,
        "END" => Code::End,
        "PAGEUP" | "PGUP" => Code::PageUp,
        "PAGEDOWN" | "PGDOWN" => Code::PageDown,
        "ESCAPE" | "ESC" => Code::Escape,
        "CAPSLOCK" => Code::CapsLock,
        "NUMLOCK" => Code::NumLock,
        "SCROLLLOCK" => Code::ScrollLock,
        "PAUSE" => Code::Pause,
        "PRINTSCREEN" | "PRINT" => Code::PrintScreen,
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

/// Parses an accelerator string ("CommandOrControl+Shift+Space", "Control+,",
/// "Control++", "F13") into a `global-hotkey` combination.
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

    fn hk(mods: Modifiers, code: Code) -> HotKey {
        HotKey::new(if mods.is_empty() { None } else { Some(mods) }, code)
    }

    #[test]
    fn classifies_hotkeys() {
        assert!(is_right_side_modifier("RightOption"));
        assert!(is_right_side_modifier("rightcmd"));
        assert!(!is_right_side_modifier("Right"));
        assert!(!is_right_side_modifier("RightArrow"));
        assert!(is_modifier_only_hotkey("Control+Alt"));
        assert!(!is_modifier_only_hotkey("Control+Alt+Space"));
        assert!(!is_modifier_only_hotkey("Control"));
        assert!(is_mouse_button_hotkey("MouseButton4"));
        assert!(!is_mouse_button_hotkey("MouseButton3"));
        assert!(is_unsupported_fn_combination("Fn+A"));
        assert!(!is_unsupported_fn_combination("Fn"));
    }

    #[test]
    fn only_keys_whose_release_is_visible_can_be_held() {
        assert!(!supports_push_to_talk(Some("F8")));
        assert!(!supports_push_to_talk(Some("PageDown")));
        assert!(supports_push_to_talk(Some("GLOBE")));
        assert!(supports_push_to_talk(Some("RightOption")));
        assert!(supports_push_to_talk(Some("Control+R")));
        assert!(supports_push_to_talk(Some("MouseButton4")));
        assert!(supports_push_to_talk(None));
    }

    #[test]
    fn canonical_listener_names() {
        assert_eq!(canonical_listener_key("RightAlt"), "RightOption");
        assert_eq!(canonical_listener_key("RightCmd"), "RightCommand");
        assert_eq!(canonical_listener_key("Fn"), "GLOBE");
        assert_eq!(canonical_listener_key("mousebutton5"), "MouseButton5");
    }

    #[test]
    fn required_modifiers_follow_the_chord() {
        assert_eq!(
            required_modifiers("Control+Shift+Space"),
            vec!["control", "shift"]
        );
        assert_eq!(required_modifiers("CommandOrControl+K"), vec!["command"]);
        assert_eq!(required_modifiers("F8"), Vec::<&str>::new());
    }

    #[test]
    fn maps_accelerators() {
        let cases: [(&str, HotKey); 12] = [
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
            ("Ctrl+Up", hk(Modifiers::CONTROL, Code::ArrowUp)),
            ("ControlLeft+K", hk(Modifiers::CONTROL, Code::KeyK)),
            ("Option+`", hk(Modifiers::ALT, Code::Backquote)),
        ];
        for (accelerator, expected) in cases {
            let parsed = parse_accelerator(accelerator).unwrap();
            assert_eq!(
                (parsed.mods, parsed.key),
                (expected.mods, expected.key),
                "{accelerator}"
            );
        }
        assert_eq!(
            normalize_to_accelerator("RightCommand+K"),
            "Command+K".to_string()
        );
    }

    #[test]
    fn rejects_unrepresentable_accelerators() {
        assert!(parse_accelerator("Control+Alt").is_err());
        assert!(parse_accelerator("Control+BAD").is_err());
        assert!(parse_accelerator("").is_err());
        assert!(parse_accelerator("Hyper+K").is_err());
    }
}
