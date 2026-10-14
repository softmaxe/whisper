//! Validation of a hotkey (or comma-separated list) before it is saved.

use super::keys::{is_globe_like_hotkey, is_mouse_button_hotkey};
use super::list::parse_hotkey_list;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationErrorCode {
    Empty,
    TooManyKeys,
    NoModifierOrSpecial,
    LeftRightMix,
    LeftModifierOnly,
    Duplicate,
    Reserved,
    FnCombinationUnsupported,
    ModifierOnlyUnsupported,
    MouseButtonCombination,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ValidationError {
    pub code: ValidationErrorCode,
    pub message: &'static str,
}

fn fail(code: ValidationErrorCode, message: &'static str) -> Result<(), ValidationError> {
    Err(ValidationError { code, message })
}

const MODIFIER_ORDER: [&str; 6] = ["Control", "Command", "Alt", "Shift", "Super", "Fn"];

const SIDE_MODIFIERS: [&str; 10] = [
    "control", "ctrl", "alt", "option", "shift", "command", "cmd", "super", "meta", "win",
];

fn compact_lower(part: &str) -> String {
    part.chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .collect::<String>()
        .to_lowercase()
}

fn side_modifier(part: &str, side: &str) -> bool {
    let normalized = compact_lower(part);
    SIDE_MODIFIERS
        .iter()
        .any(|m| normalized == format!("{side}{m}") || normalized == format!("{m}{side}"))
}

fn is_right_side(part: &str) -> bool {
    side_modifier(part, "right")
}

fn is_left_side(part: &str) -> bool {
    side_modifier(part, "left")
}

fn side_name(lowered: &str, prefix: &str) -> &'static str {
    let named = |control, alt, shift, command| -> &'static str {
        if lowered.contains("control") || lowered.contains("ctrl") {
            control
        } else if lowered.contains("alt") || lowered.contains("option") {
            alt
        } else if lowered.contains("shift") {
            shift
        } else {
            command
        }
    };
    if prefix == "Right" {
        named("RightControl", "RightOption", "RightShift", "RightCommand")
    } else {
        named("LeftControl", "LeftOption", "LeftShift", "LeftCommand")
    }
}

fn normalize_modifier(part: &str) -> Option<&'static str> {
    let lowered: String = part.split_whitespace().collect::<String>().to_lowercase();
    match lowered.as_str() {
        "commandorcontrol" | "cmdorctrl" | "command" | "cmd" | "super" | "win" | "meta" => {
            return Some("Command");
        }
        "control" | "ctrl" => return Some("Control"),
        "alt" | "option" => return Some("Alt"),
        "shift" => return Some("Shift"),
        "fn" => return Some("Fn"),
        _ => {}
    }
    if is_right_side(part) {
        return Some(side_name(&lowered, "Right"));
    }
    if is_left_side(part) {
        return Some(side_name(&lowered, "Left"));
    }
    None
}

fn normalize_key_token(part: &str) -> String {
    let trimmed: String = part.split_whitespace().collect();
    let lowered = trimmed.to_lowercase();
    let named = match lowered.as_str() {
        "arrowleft" => Some("Left"),
        "arrowright" => Some("Right"),
        "arrowup" => Some("Up"),
        "arrowdown" => Some("Down"),
        "escape" | "esc" => Some("Esc"),
        "printscreen" | "print" => Some("PrintScreen"),
        "pageup" | "pgup" => Some("PageUp"),
        "pagedown" | "pgdown" => Some("PageDown"),
        "scrolllock" => Some("ScrollLock"),
        "numlock" => Some("NumLock"),
        "delete" | "del" => Some("Delete"),
        "insert" | "ins" => Some("Insert"),
        "space" => Some("Space"),
        "tab" => Some("Tab"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "backspace" => Some("Backspace"),
        "globe" => Some("GLOBE"),
        "fn" => Some("Fn"),
        "mousebutton4" => Some("MouseButton4"),
        "mousebutton5" => Some("MouseButton5"),
        _ => None,
    };
    if let Some(name) = named {
        return name.to_string();
    }
    if let Some(digits) = lowered.strip_prefix('f')
        && (1..=2).contains(&digits.len())
        && digits.chars().all(|c| c.is_ascii_digit())
    {
        return format!("F{digits}");
    }
    if trimmed.chars().count() == 1 {
        return trimmed.to_uppercase();
    }
    trimmed
}

fn is_special_key(key: &str) -> bool {
    const SPECIAL: [&str; 20] = [
        "GLOBE",
        "Fn",
        "Esc",
        "Tab",
        "Space",
        "Backspace",
        "Insert",
        "Delete",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Left",
        "Right",
        "Up",
        "Down",
        "PrintScreen",
        "Pause",
        "ScrollLock",
        "NumLock",
    ];
    if SPECIAL.contains(&key) {
        return true;
    }
    key.strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n))
}

const RESERVED_SHORTCUTS: [&str; 48] = [
    "Command+C",
    "Command+V",
    "Command+X",
    "Command+Z",
    "Command+Shift+Z",
    "Command+A",
    "Command+Q",
    "Command+W",
    "Command+R",
    "Command+T",
    "Command+S",
    "Command+P",
    "Command+N",
    "Command+M",
    "Command+H",
    "Command+F",
    "Command+G",
    "Command+Shift+G",
    "Command+,",
    "Command+Left",
    "Command+Right",
    "Command+Up",
    "Command+Down",
    "Command+Shift+Left",
    "Command+Shift+Right",
    "Command+Shift+Up",
    "Command+Shift+Down",
    "Command+Control+F",
    "Command+Space",
    "Command+Alt+Space",
    "Command+Shift+3",
    "Command+Shift+4",
    "Command+Shift+5",
    "Command+Alt+Esc",
    "Command+Alt+D",
    "Command+Delete",
    "Command+Shift+Delete",
    "Command+Shift+Q",
    "Command+B",
    "Command+I",
    "Command+U",
    "Command+Shift+T",
    "Command+=",
    "Command+-",
    "Command+Alt+F",
    "Command+Shift+F",
    "Fn+F11",
    "Fn+F12",
];

fn is_left_right_mix(parts: &[&str]) -> bool {
    let mut sides: Vec<(String, &'static str)> = Vec::new();
    for part in parts {
        let compact: String = part.split_whitespace().collect::<String>().to_lowercase();
        let compact = compact.replace(['-', '_'], "");
        for modifier in SIDE_MODIFIERS {
            for side in ["left", "right"] {
                if compact == format!("{side}{modifier}") || compact == format!("{modifier}{side}")
                {
                    let normalized = match modifier {
                        "ctrl" => "control",
                        "cmd" => "command",
                        "option" => "alt",
                        "win" | "meta" => "super",
                        other => other,
                    };
                    let side: &'static str = if side == "left" { "left" } else { "right" };
                    sides.push((normalized.to_string(), side));
                }
            }
        }
    }
    sides.iter().any(|(modifier, side)| {
        sides
            .iter()
            .any(|(other, other_side)| other == modifier && other_side != side)
    })
}

/// Canonical form for duplicate and reserved-shortcut comparison.
pub fn normalize_hotkey(hotkey: &str) -> String {
    let mut modifiers: Vec<&'static str> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for part in hotkey.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match normalize_modifier(part) {
            Some(modifier) => modifiers.push(modifier),
            None => keys.push(normalize_key_token(part)),
        }
    }
    let rank = |m: &str| {
        MODIFIER_ORDER
            .iter()
            .position(|o| *o == m)
            .map_or(-1, |p| p as i32)
    };
    modifiers.sort_by_key(|m| rank(m));
    modifiers
        .into_iter()
        .map(str::to_string)
        .chain(keys)
        .collect::<Vec<_>>()
        .join("+")
}

/// Validates a hotkey or comma-separated hotkey list against the macOS rules.
pub fn validate_hotkey(hotkey: &str, existing: &[&str]) -> Result<(), ValidationError> {
    if hotkey.trim().is_empty() {
        return fail(ValidationErrorCode::Empty, "Please enter a valid shortcut.");
    }

    let mut hotkey = hotkey.to_string();
    if hotkey.contains(',') {
        let items = parse_hotkey_list(&hotkey);
        if items.is_empty() {
            return fail(ValidationErrorCode::Empty, "Please enter a valid shortcut.");
        }
        if items.len() > 1 {
            for item in &items {
                validate_hotkey(item, existing)?;
            }
            return Ok(());
        }
        hotkey = items[0].clone();
    }
    let hotkey = hotkey.as_str();

    if is_globe_like_hotkey(hotkey) {
        return Ok(());
    }
    if hotkey.len() >= 3
        && hotkey
            .get(..3)
            .is_some_and(|p| p.eq_ignore_ascii_case("fn+"))
    {
        return fail(
            ValidationErrorCode::FnCombinationUnsupported,
            "The Globe/Fn key can only be used by itself.",
        );
    }
    if is_mouse_button_hotkey(hotkey) {
        return Ok(());
    }
    let lowered = hotkey.to_lowercase();
    if lowered.contains("mousebutton4") || lowered.contains("mousebutton5") {
        return fail(
            ValidationErrorCode::MouseButtonCombination,
            "Mouse button hotkeys cannot be combined with other keys.",
        );
    }

    let parts: Vec<&str> = hotkey
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();

    if parts.len() > 3 {
        return fail(
            ValidationErrorCode::TooManyKeys,
            "Shortcuts are limited to three keys.",
        );
    }
    if is_left_right_mix(&parts) {
        return fail(
            ValidationErrorCode::LeftRightMix,
            "Do not mix left and right versions of the same modifier in one shortcut.",
        );
    }

    let mut has_modifier = false;
    let mut has_special = false;
    for part in &parts {
        if normalize_modifier(part).is_some() {
            has_modifier = true;
        } else if is_special_key(&normalize_key_token(part)) {
            has_special = true;
        }
    }
    if !has_modifier && !has_special {
        return fail(
            ValidationErrorCode::NoModifierOrSpecial,
            "Shortcuts must include a modifier or a non-alphanumeric key (like arrows, space, or function keys).",
        );
    }

    let modifier_count = parts
        .iter()
        .filter(|p| normalize_modifier(p).is_some())
        .count();
    let has_base_key = parts.len() > modifier_count;
    if !has_base_key && modifier_count >= 2 {
        return fail(
            ValidationErrorCode::ModifierOnlyUnsupported,
            "Two-modifier shortcuts are not supported on macOS. Use a right-side modifier on its own (e.g. RightOption), or add a regular key.",
        );
    }
    if !has_base_key && modifier_count == 1 && !is_right_side(parts[0]) {
        return fail(
            ValidationErrorCode::LeftModifierOnly,
            "Single modifier hotkeys must use the right-side key (e.g., RightOption). Or add a regular key (e.g., Control+Space).",
        );
    }

    let normalized = normalize_hotkey(hotkey);
    if existing.iter().any(|e| normalize_hotkey(e) == normalized) {
        return fail(
            ValidationErrorCode::Duplicate,
            "That shortcut is already in use.",
        );
    }
    if RESERVED_SHORTCUTS
        .iter()
        .any(|r| normalize_hotkey(r) == normalized)
    {
        return fail(
            ValidationErrorCode::Reserved,
            "That shortcut is reserved by your system.",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(hotkey: &str) -> Option<ValidationErrorCode> {
        validate_hotkey(hotkey, &[]).err().map(|e| e.code)
    }

    #[test]
    fn reports_each_validation_error_code() {
        assert!(validate_hotkey("", &[]).is_err());
        assert!(validate_hotkey("  ", &[]).is_err());
        assert_eq!(code("GLOBE"), None);
        assert_eq!(code("Fn"), None);
        assert_eq!(code("MouseButton4"), None);
        assert_eq!(
            code("Control+MouseButton4"),
            Some(ValidationErrorCode::MouseButtonCombination)
        );
        assert_eq!(
            code("Ctrl+Alt+Shift+K"),
            Some(ValidationErrorCode::TooManyKeys)
        );
        assert_eq!(code("K"), Some(ValidationErrorCode::NoModifierOrSpecial));
        assert_eq!(code("F8"), None);
        assert_eq!(code("Space"), None);
        assert_eq!(code("Esc"), None);
        assert_eq!(
            code("LeftCtrl+RightCtrl+K"),
            Some(ValidationErrorCode::LeftRightMix)
        );
        assert_eq!(code("Control"), Some(ValidationErrorCode::LeftModifierOnly));
        assert_eq!(code("RightOption"), None);
        assert_eq!(code("RightAlt"), None);
        assert_eq!(
            code("Control+Alt"),
            Some(ValidationErrorCode::ModifierOnlyUnsupported)
        );
        assert_eq!(code("Control+Alt+Space"), None);
        assert_eq!(
            code("Fn+A"),
            Some(ValidationErrorCode::FnCombinationUnsupported)
        );
        assert_eq!(
            validate_hotkey("Ctrl+K", &["Control+K"])
                .err()
                .map(|e| e.code),
            Some(ValidationErrorCode::Duplicate)
        );
        assert_eq!(code("Command+C"), Some(ValidationErrorCode::Reserved));
        assert_eq!(code("Cmd+c"), Some(ValidationErrorCode::Reserved));
        assert_eq!(code("Control+Shift+K"), None);
        assert_eq!(code("Alt+F7"), None);
        assert_eq!(code("F8,Control+Shift+K"), None);
        assert_eq!(
            code("F8,Fn+A"),
            Some(ValidationErrorCode::FnCombinationUnsupported)
        );
    }

    #[test]
    fn normalizes_for_comparison() {
        assert_eq!(normalize_hotkey("CommandOrControl+K"), "Command+K");
        assert_eq!(normalize_hotkey("CmdOrCtrl+K"), "Command+K");
        assert_eq!(normalize_hotkey("Option+R"), "Alt+R");
        assert_eq!(normalize_hotkey("Super+K"), "Command+K");
        assert_eq!(normalize_hotkey("Shift+Control+K"), "Control+Shift+K");
        assert_eq!(
            normalize_hotkey("Shift+Alt+Control+K"),
            "Control+Alt+Shift+K"
        );
        assert_eq!(normalize_hotkey("Ctrl+ArrowLeft"), "Control+Left");
        assert_eq!(normalize_hotkey("Ctrl+Escape"), "Control+Esc");
        assert_eq!(normalize_hotkey("Ctrl+pgup"), "Control+PageUp");
        assert_eq!(normalize_hotkey("Ctrl+k"), "Control+K");
        assert_eq!(normalize_hotkey("Ctrl+f9"), "Control+F9");
        assert_eq!(normalize_hotkey(""), "");
        assert_eq!(normalize_hotkey("ControlLeft+K"), "LeftControl+K");
        assert_eq!(normalize_hotkey("ShiftLeft+Space"), "LeftShift+Space");
    }

    #[test]
    fn left_side_modifiers_need_a_key_and_cannot_mix_sides() {
        assert_eq!(code("ControlLeft+K"), None);
        assert_eq!(code("ShiftLeft+Space"), None);
        assert_eq!(
            code("ControlLeft"),
            Some(ValidationErrorCode::LeftModifierOnly)
        );
        for mix in [
            "LeftControl+RightControl",
            "ControlLeft+ControlRight",
            "LeftControl+ControlRight",
        ] {
            assert_eq!(code(mix), Some(ValidationErrorCode::LeftRightMix), "{mix}");
        }
    }
}
