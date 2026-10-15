//! Hotkey list parsing, display labels, validation and capture policy.
//!
//! Hotkeys are accelerator strings ("Control+Shift+K", "RightOption",
//! "GLOBE", "MouseButton4") because that is what settings store.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::settings::DEFAULT_HOTKEY;

const HOTKEY_LIST_SEPARATOR: char = ',';

pub fn is_globe_like_hotkey(hotkey: &str) -> bool {
    hotkey == "GLOBE" || hotkey == "Fn"
}

pub fn is_mouse_button_hotkey(hotkey: &str) -> bool {
    hotkey.eq_ignore_ascii_case("MouseButton4") || hotkey.eq_ignore_ascii_case("MouseButton5")
}

// The comma KEY is itself a valid hotkey ("Control+,"): no accelerator ends
// with "+", so a segment ending in "+" followed by an empty segment (the
// comma key consumed by the split) gets its comma restored.
fn collect_hotkeys(raw: &[&str]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for (i, segment) in raw.iter().enumerate() {
        let mut hotkey = segment.trim().to_string();
        if hotkey.ends_with('+') && i + 1 < raw.len() && raw[i + 1].trim().is_empty() {
            hotkey.push(HOTKEY_LIST_SEPARATOR);
        }
        if hotkey.is_empty() || !seen.insert(hotkey.clone()) {
            continue;
        }
        result.push(hotkey);
    }
    result
}

/// Parse a comma-separated hotkey list (a single hotkey is a one-item list):
/// trimmed, de-duplicated, empties removed, order preserved.
pub fn parse_hotkey_list(value: &str) -> Vec<String> {
    let raw: Vec<&str> = value.split(HOTKEY_LIST_SEPARATOR).collect();
    collect_hotkeys(&raw)
}

/// Like [`parse_hotkey_list`] for a list whose items may themselves hold
/// comma-separated hotkeys.
pub fn parse_hotkey_items<S: AsRef<str>>(items: &[S]) -> Vec<String> {
    let raw: Vec<&str> = items
        .iter()
        .flat_map(|item| item.as_ref().split(HOTKEY_LIST_SEPARATOR))
        .collect();
    collect_hotkeys(&raw)
}

/// Serialize a hotkey list into the canonical comma-separated storage string.
pub fn serialize_hotkey_list<S: AsRef<str>>(items: &[S]) -> String {
    parse_hotkey_items(items).join(",")
}

static SIDE_MODIFIER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(Right|Left)(Option|Alt|Command|Cmd|Control|Ctrl|Shift|Super|Meta|Win)$")
        .expect("static regex")
});

/// "RightOption" → "Right Option", "LeftControl" → "Left Ctrl"; `None` when
/// the token carries no side.
fn format_side_modifier_part(part: &str) -> Option<String> {
    let caps = SIDE_MODIFIER_RE.captures(part)?;
    let key = &caps[2];
    let key = if key == "Option" { "Alt" } else { key };
    Some(format!("{} {}", &caps[1], format_modifier_part(key)))
}

fn format_modifier_part(part: &str) -> &str {
    match part {
        "CommandOrControl" | "Command" | "Cmd" | "Super" | "Meta" => "Cmd",
        "Control" | "Ctrl" => "Ctrl",
        "Alt" | "Option" => "Option",
        "Shift" => "Shift",
        "Win" => "Super",
        "Fn" => "Fn",
        other => other,
    }
}

/// User-facing label; an empty or missing hotkey shows the default hotkey.
pub fn format_hotkey_label(hotkey: Option<&str>) -> String {
    let resolved = match hotkey {
        Some(h) if !h.trim().is_empty() => h,
        _ => DEFAULT_HOTKEY,
    };
    format_hotkey_display(resolved)
}

/// Like [`format_hotkey_label`], but an empty hotkey formats as "".
pub fn format_hotkey_display(hotkey: &str) -> String {
    if hotkey.trim().is_empty() {
        return String::new();
    }
    if is_globe_like_hotkey(hotkey) {
        return "Globe/Fn".into();
    }
    if is_mouse_button_hotkey(hotkey) {
        // Case-insensitive, so "mousebutton4" reads as 4, not 5.
        return if hotkey.eq_ignore_ascii_case("MouseButton4") {
            "Mouse Button 4".into()
        } else {
            "Mouse Button 5".into()
        };
    }
    hotkey
        .split('+')
        .map(|part| {
            format_side_modifier_part(part)
                .unwrap_or_else(|| format_modifier_part(part).to_string())
        })
        .collect::<Vec<_>>()
        .join("+")
}

// ---------------------------------------------------------------------------
// Validation

/// Why a hotkey was rejected. `message_key()` is an i18n key for the UI and
/// `default_message()` the English text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyValidationError {
    Empty,
    FnCombinationUnsupported,
    MouseButtonCombination,
    TooManyKeys,
    LeftRightMix,
    NoModifierOrSpecial,
    ModifierOnlyUnsupported,
    LeftModifierOnly,
    Duplicate,
    Reserved,
}

impl HotkeyValidationError {
    pub fn message_key(self) -> &'static str {
        match self {
            Self::Empty => "hotkey.validation.empty",
            Self::FnCombinationUnsupported => "hotkey.errors.fnCombinationUnsupported",
            Self::MouseButtonCombination => "hotkey.validation.mouseButtonCombination",
            Self::TooManyKeys => "hotkey.validation.tooManyKeys",
            Self::LeftRightMix => "hotkey.validation.leftRightMix",
            Self::NoModifierOrSpecial => "hotkey.validation.noModifierOrSpecial",
            Self::ModifierOnlyUnsupported => "hotkey.validation.modifierOnlyUnsupported",
            Self::LeftModifierOnly => "hotkey.validation.leftModifierOnly",
            Self::Duplicate => "hotkey.validation.duplicate",
            Self::Reserved => "hotkey.validation.reserved",
        }
    }

    pub fn default_message(self) -> &'static str {
        match self {
            Self::Empty => "Please enter a valid shortcut.",
            Self::FnCombinationUnsupported => "The Globe/Fn key can only be used by itself.",
            Self::MouseButtonCombination => {
                "Mouse button hotkeys cannot be combined with other keys."
            }
            Self::TooManyKeys => "Shortcuts are limited to three keys.",
            Self::LeftRightMix => {
                "Do not mix left and right versions of the same modifier in one shortcut."
            }
            Self::NoModifierOrSpecial => {
                "Shortcuts must include a modifier or a non-alphanumeric key (like arrows, space, or function keys)."
            }
            Self::ModifierOnlyUnsupported => {
                "Two-modifier shortcuts are not supported on macOS. Use a right-side modifier on its own (e.g. RightOption), or add a regular key."
            }
            Self::LeftModifierOnly => {
                "Single modifier hotkeys must use the right-side key (e.g., RightOption). Or add a regular key (e.g., Control+Space)."
            }
            Self::Duplicate => "That shortcut is already in use.",
            Self::Reserved => "That shortcut is reserved by your system.",
        }
    }
}

/// A user-facing validation message: i18n `key` with interpolation `params`
/// (`{{name}}` style), plus the English text as a fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationMessage {
    pub key: &'static str,
    pub params: Vec<(&'static str, String)>,
    pub default_text: String,
}

const MODIFIER_ORDER: [&str; 6] = ["Control", "Command", "Alt", "Shift", "Super", "Fn"];

const SIDED_MODIFIER_NAMES: [&str; 10] = [
    "control", "ctrl", "alt", "option", "shift", "command", "cmd", "super", "meta", "win",
];

fn compact_side_token(part: &str) -> String {
    part.chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .collect::<String>()
        .to_lowercase()
}

fn is_sided_modifier(part: &str, side: &str) -> bool {
    let normalized = compact_side_token(part);
    SIDED_MODIFIER_NAMES.iter().any(|name| {
        normalized.strip_prefix(side) == Some(name) || normalized.strip_suffix(side) == Some(name)
    })
}

fn is_right_side_modifier(part: &str) -> bool {
    is_sided_modifier(part, "right")
}

fn is_left_side_modifier(part: &str) -> bool {
    is_sided_modifier(part, "left")
}

const SPECIAL_KEYS: [&str; 20] = [
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

fn is_special_key(key: &str) -> bool {
    SPECIAL_KEYS.contains(&key)
        || key
            .strip_prefix('F')
            .and_then(|n| {
                n.parse::<u32>()
                    .ok()
                    .filter(|_| !n.starts_with('0') && !n.starts_with('+'))
            })
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

static NORMALIZED_RESERVED: LazyLock<Vec<String>> = LazyLock::new(|| {
    RESERVED_SHORTCUTS
        .iter()
        .map(|s| normalize_hotkey(s))
        .collect()
});

fn strip_whitespace(part: &str) -> String {
    part.chars().filter(|c| !c.is_whitespace()).collect()
}

fn normalize_modifier(part: &str) -> Option<&'static str> {
    let lowered = strip_whitespace(part).to_lowercase();
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
    // Side-qualified modifiers keep their side for single-modifier validation.
    let sided = |side: &'static [&'static str; 5]| -> Option<&'static str> {
        let has = |s: &str| lowered.contains(s);
        if has("control") || has("ctrl") {
            Some(side[0])
        } else if has("alt") || has("option") {
            Some(side[1])
        } else if has("shift") {
            Some(side[2])
        } else if has("command") || has("cmd") {
            Some(side[3])
        } else if has("super") || has("meta") || has("win") {
            Some(side[4])
        } else {
            None
        }
    };
    if is_right_side_modifier(part)
        && let Some(m) = sided(&[
            "RightControl",
            "RightOption",
            "RightShift",
            "RightCommand",
            "RightCommand",
        ])
    {
        return Some(m);
    }
    if is_left_side_modifier(part)
        && let Some(m) = sided(&[
            "LeftControl",
            "LeftOption",
            "LeftShift",
            "LeftCommand",
            "LeftCommand",
        ])
    {
        return Some(m);
    }
    None
}

fn normalize_key_token(part: &str) -> String {
    let trimmed = strip_whitespace(part);
    let lowered = trimmed.to_lowercase();
    let fixed = match lowered.as_str() {
        "arrowleft" => "Left",
        "arrowright" => "Right",
        "arrowup" => "Up",
        "arrowdown" => "Down",
        "escape" | "esc" => "Esc",
        "printscreen" | "print" => "PrintScreen",
        "pageup" | "pgup" => "PageUp",
        "pagedown" | "pgdown" => "PageDown",
        "scrolllock" => "ScrollLock",
        "numlock" => "NumLock",
        "delete" | "del" => "Delete",
        "insert" | "ins" => "Insert",
        "space" => "Space",
        "tab" => "Tab",
        "home" => "Home",
        "end" => "End",
        "backspace" => "Backspace",
        "globe" => "GLOBE",
        "fn" => "Fn",
        "mousebutton4" => "MouseButton4",
        "mousebutton5" => "MouseButton5",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    if let Some(digits) = lowered.strip_prefix('f')
        && (1..=2).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
    {
        return format!("F{digits}");
    }
    // Single-unit keys (one UTF-16 unit) are uppercased.
    if trimmed.encode_utf16().count() == 1 {
        return trimmed.to_uppercase();
    }
    trimmed
}

static LEFT_RIGHT_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(left|right)[-_ ]?(ctrl|control|alt|option|shift|command|cmd|super|meta|win)$",
    )
    .expect("static regex")
});
static LEFT_RIGHT_SUFFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(ctrl|control|alt|option|shift|command|cmd|super|meta|win)[-_ ]?(left|right)$",
    )
    .expect("static regex")
});

fn is_left_right_mix(parts: &[&str]) -> bool {
    let mut sides: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    for raw in parts {
        let part = strip_whitespace(raw);
        let matches = [
            LEFT_RIGHT_PREFIX_RE
                .captures(&part)
                .map(|c| (c[1].to_lowercase(), c[2].to_lowercase())),
            LEFT_RIGHT_SUFFIX_RE
                .captures(&part)
                .map(|c| (c[2].to_lowercase(), c[1].to_lowercase())),
        ];
        for (side, modifier) in matches.into_iter().flatten() {
            let side = if side.contains("left") {
                "left"
            } else {
                "right"
            };
            let modifier: &'static str = match modifier.as_str() {
                "ctrl" | "control" => "control",
                "cmd" | "command" => "command",
                "option" | "alt" => "alt",
                "win" | "meta" | "super" => "super",
                _ => "shift",
            };
            sides.entry(modifier).or_default().insert(side);
        }
    }
    sides.values().any(|set| set.len() > 1)
}

fn split_parts(hotkey: &str) -> Vec<&str> {
    hotkey
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

/// Canonical form for comparison: modifiers normalized and ordered
/// (Control, Command, Alt, Shift, Super, Fn; sided ones first), keys after.
pub fn normalize_hotkey(hotkey: &str) -> String {
    let mut modifiers: Vec<&str> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for part in split_parts(hotkey) {
        match normalize_modifier(part) {
            Some(m) => modifiers.push(m),
            None => keys.push(normalize_key_token(part)),
        }
    }
    // indexOf returns -1 for sided modifiers, sorting them first (stable).
    let order = |m: &str| {
        MODIFIER_ORDER
            .iter()
            .position(|o| *o == m)
            .map_or(-1, |i| i as i64)
    };
    modifiers.sort_by_key(|m| order(m));
    modifiers
        .into_iter()
        .map(str::to_string)
        .chain(keys)
        .collect::<Vec<_>>()
        .join("+")
}

/// Validate a hotkey (or a comma-separated list, each entry independently,
/// first failure wins) against macOS constraints and `existing_hotkeys`.
pub fn validate_hotkey<S: AsRef<str>>(
    hotkey: &str,
    existing_hotkeys: &[S],
) -> Result<(), HotkeyValidationError> {
    use HotkeyValidationError as E;
    if hotkey.trim().is_empty() {
        return Err(E::Empty);
    }

    // parse_hotkey_list keeps comma-key hotkeys like "Control+," intact, so a
    // single such hotkey falls through to single-hotkey validation.
    let single;
    let mut hotkey = hotkey;
    if hotkey.contains(',') {
        let items = parse_hotkey_list(hotkey);
        match items.len() {
            0 => return Err(E::Empty),
            1 => {
                single = items.into_iter().next().unwrap_or_default();
                hotkey = &single;
            }
            _ => {
                for item in &items {
                    validate_hotkey(item, existing_hotkeys)?;
                }
                return Ok(());
            }
        }
    }

    if is_globe_like_hotkey(hotkey) {
        return Ok(());
    }
    // Fn cannot be part of an accelerator; only standalone Globe/Fn has a
    // native path.
    if hotkey
        .get(..3)
        .is_some_and(|p| p.eq_ignore_ascii_case("fn+"))
    {
        return Err(E::FnCombinationUnsupported);
    }
    if is_mouse_button_hotkey(hotkey) {
        return Ok(());
    }
    // Mouse buttons come from a separate event tap and cannot be combined.
    let lowered = hotkey.to_lowercase();
    if lowered.contains("mousebutton4") || lowered.contains("mousebutton5") {
        return Err(E::MouseButtonCombination);
    }

    let parts = split_parts(hotkey);
    if parts.len() > 3 {
        return Err(E::TooManyKeys);
    }
    if is_left_right_mix(&parts) {
        return Err(E::LeftRightMix);
    }

    let mut has_modifier = false;
    let mut has_special_key = false;
    for part in &parts {
        if normalize_modifier(part).is_some() {
            has_modifier = true;
        } else if is_special_key(&normalize_key_token(part)) {
            has_special_key = true;
        }
    }
    if !has_modifier && !has_special_key {
        return Err(E::NoModifierOrSpecial);
    }

    let modifier_count = parts
        .iter()
        .filter(|p| normalize_modifier(p).is_some())
        .count();
    let has_base_key = parts.len() > modifier_count;
    // The Globe listener reports Fn, right-side modifiers and mouse buttons
    // only, and an accelerator needs a key, so modifier-only chords can't bind.
    if !has_base_key && modifier_count >= 2 {
        return Err(E::ModifierOnlyUnsupported);
    }
    if !has_base_key && modifier_count == 1 && !is_right_side_modifier(parts[0]) {
        return Err(E::LeftModifierOnly);
    }

    let normalized = normalize_hotkey(hotkey);
    if existing_hotkeys
        .iter()
        .any(|e| normalize_hotkey(e.as_ref()) == normalized)
    {
        return Err(E::Duplicate);
    }
    if NORMALIZED_RESERVED.contains(&normalized) {
        return Err(E::Reserved);
    }
    Ok(())
}

/// The message to show for an invalid hotkey, or `None` when it is valid.
/// A reserved shortcut is named in the message.
pub fn get_validation_message<S: AsRef<str>>(
    hotkey: &str,
    existing_hotkeys: &[S],
) -> Option<ValidationMessage> {
    let err = validate_hotkey(hotkey, existing_hotkeys).err()?;
    if err == HotkeyValidationError::Reserved {
        let label = format_hotkey_display(hotkey);
        return Some(ValidationMessage {
            key: "hotkey.validation.reservedNamed",
            default_text: format!("{label} is reserved by the system"),
            params: vec![("label", label)],
        });
    }
    Some(ValidationMessage {
        key: err.message_key(),
        params: Vec::new(),
        default_text: err.default_message().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use HotkeyValidationError as E;

    const NONE: [&str; 0] = [];

    fn v(h: &str) -> Result<(), E> {
        validate_hotkey(h, &NONE)
    }

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // Labels

    #[test]
    fn globe_like() {
        assert!(is_globe_like_hotkey("GLOBE"));
        assert!(is_globe_like_hotkey("Fn"));
        for h in ["F1", "globe", "fn", ""] {
            assert!(!is_globe_like_hotkey(h));
        }
    }

    #[test]
    fn display_labels() {
        assert_eq!(format_hotkey_display(""), "");
        assert_eq!(format_hotkey_display("  "), "");
        assert_eq!(format_hotkey_display("GLOBE"), "Globe/Fn");
        assert_eq!(format_hotkey_display("MouseButton4"), "Mouse Button 4");
        assert_eq!(format_hotkey_display("MouseButton5"), "Mouse Button 5");
        assert_eq!(format_hotkey_display("CommandOrControl+K"), "Cmd+K");
        assert_eq!(format_hotkey_display("Alt+R"), "Option+R");
        assert_eq!(format_hotkey_display("Super+K"), "Cmd+K");
        assert_eq!(format_hotkey_display("Meta+K"), "Cmd+K");
        assert_eq!(format_hotkey_display("RightOption"), "Right Option");
        assert_eq!(format_hotkey_display("RightCommand"), "Right Cmd");
        assert_eq!(format_hotkey_display("LeftOption"), "Left Option");
        assert_eq!(format_hotkey_display("LeftControl"), "Left Ctrl");
        assert_eq!(format_hotkey_display("LeftCommand"), "Left Cmd");
        assert_eq!(format_hotkey_display("Alt"), "Option");
        assert_eq!(format_hotkey_display("Command"), "Cmd");
        assert_eq!(format_hotkey_display("`"), "`");
        assert_eq!(
            format_hotkey_display("Control+RightShift+K"),
            "Ctrl+Right Shift+K"
        );
    }

    #[test]
    fn labels_fall_back_to_default() {
        assert_eq!(format_hotkey_label(None), "Globe/Fn");
        assert_eq!(format_hotkey_label(Some(" ")), "Globe/Fn");
    }

    // List parsing

    #[test]
    fn list_parsing() {
        assert_eq!(
            parse_hotkey_list("Control+Shift+R"),
            list(&["Control+Shift+R"])
        );
        assert_eq!(parse_hotkey_list("GLOBE"), list(&["GLOBE"]));
        assert_eq!(
            parse_hotkey_list("GLOBE,Control+Shift+R"),
            list(&["GLOBE", "Control+Shift+R"])
        );
        assert_eq!(
            parse_hotkey_list(" GLOBE , Control+Shift+R "),
            list(&["GLOBE", "Control+Shift+R"])
        );
        assert_eq!(parse_hotkey_list("F8,,F8,F9,"), list(&["F8", "F9"]));
        assert!(parse_hotkey_list("").is_empty());
    }

    #[test]
    fn list_items_parsing() {
        assert_eq!(
            parse_hotkey_items(&["GLOBE", "Control+Shift+R"]),
            list(&["GLOBE", "Control+Shift+R"])
        );
        assert_eq!(
            parse_hotkey_items(&["GLOBE,Control+Alt", " F8 "]),
            list(&["GLOBE", "Control+Alt", "F8"])
        );
    }

    #[test]
    fn list_serialization() {
        assert_eq!(
            serialize_hotkey_list(&["GLOBE", "Control+Shift+R"]),
            "GLOBE,Control+Shift+R"
        );
        assert_eq!(serialize_hotkey_list(&["F8, F9 ,F8"]), "F8,F9");
        assert_eq!(serialize_hotkey_list(&NONE), "");
        let once = serialize_hotkey_list(&[" GLOBE , Control+Shift+R , MouseButton4 "]);
        assert_eq!(once, "GLOBE,Control+Shift+R,MouseButton4");
        assert_eq!(
            parse_hotkey_list(&once),
            list(&["GLOBE", "Control+Shift+R", "MouseButton4"])
        );
    }

    #[test]
    fn comma_key_hotkeys() {
        assert_eq!(parse_hotkey_list("Control+,"), list(&["Control+,"]));
        assert_eq!(
            parse_hotkey_list("Command+Shift+,"),
            list(&["Command+Shift+,"])
        );
        assert_eq!(
            parse_hotkey_list("GLOBE,Control+,"),
            list(&["GLOBE", "Control+,"])
        );
        assert_eq!(
            parse_hotkey_list("Control+,,F8"),
            list(&["Control+,", "F8"])
        );
        assert_eq!(
            parse_hotkey_items(&["Control+,", "F8"]),
            list(&["Control+,", "F8"])
        );
        let l = ["Control+,", "F8", "GLOBE"];
        let serialized = serialize_hotkey_list(&l);
        assert_eq!(serialized, "Control+,,F8,GLOBE");
        assert_eq!(parse_hotkey_list(&serialized), list(&l));
    }

    #[test]
    fn plus_key_hotkeys() {
        assert_eq!(
            parse_hotkey_list("Control++,F8"),
            list(&["Control++", "F8"])
        );
        assert_eq!(
            parse_hotkey_items(&["Control++", "F8"]),
            list(&["Control++", "F8"])
        );
        assert_eq!(
            parse_hotkey_list("Control++,,F8"),
            list(&["Control++,", "F8"])
        );
        assert_eq!(
            parse_hotkey_list("Command++,Control+,,"),
            list(&["Command++", "Control+,"])
        );
        assert_eq!(
            parse_hotkey_list("Control++,Control+,"),
            list(&["Control++", "Control+,"])
        );
    }

    // Validation

    #[test]
    fn validation_basics() {
        assert_eq!(v(""), Err(E::Empty));
        assert_eq!(v("  "), Err(E::Empty));
        assert_eq!(v("GLOBE"), Ok(()));
        assert_eq!(v("Fn"), Ok(()));
        assert_eq!(v("MouseButton4"), Ok(()));
        assert_eq!(v("Control+MouseButton4"), Err(E::MouseButtonCombination));
        assert_eq!(v("Ctrl+Alt+Shift+K"), Err(E::TooManyKeys));
        assert_eq!(v("K"), Err(E::NoModifierOrSpecial));
        for h in ["F8", "Space", "Esc"] {
            assert_eq!(v(h), Ok(()), "{h}");
        }
        assert_eq!(v("LeftCtrl+RightCtrl+K"), Err(E::LeftRightMix));
        assert_eq!(v("Control"), Err(E::LeftModifierOnly));
        assert_eq!(v("RightOption"), Ok(()));
        assert_eq!(v("RightAlt"), Ok(()));
        assert_eq!(v("Control+Alt"), Err(E::ModifierOnlyUnsupported));
        assert_eq!(v("Control+Alt+Space"), Ok(()));
        assert_eq!(v("Fn+A"), Err(E::FnCombinationUnsupported));
        assert_eq!(validate_hotkey("Ctrl+K", &["Control+K"]), Err(E::Duplicate));
        assert_eq!(v("Command+C"), Err(E::Reserved));
        assert_eq!(v("Cmd+c"), Err(E::Reserved));
        assert_eq!(v("Control+Shift+K"), Ok(()));
        assert_eq!(v("Alt+F7"), Ok(()));
        assert_eq!(v("F8,Control+Shift+K"), Ok(()));
        assert_eq!(v("F8,Fn+A"), Err(E::FnCombinationUnsupported));
        assert_eq!(v("Command+,"), Err(E::Reserved));
        assert_eq!(v("F25"), Err(E::NoModifierOrSpecial));
    }

    #[test]
    fn normalization() {
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
    }

    #[test]
    fn left_side_modifiers() {
        assert_eq!(normalize_hotkey("ControlLeft+K"), "LeftControl+K");
        assert_eq!(normalize_hotkey("ShiftLeft+Space"), "LeftShift+Space");
        assert_eq!(v("ControlLeft+K"), Ok(()));
        assert_eq!(v("ShiftLeft+Space"), Ok(()));
        assert_eq!(v("ControlLeft"), Err(E::LeftModifierOnly));
    }

    #[test]
    fn left_right_mix_formats() {
        assert_eq!(v("LeftControl+RightControl"), Err(E::LeftRightMix));
        assert_eq!(v("ControlLeft+ControlRight"), Err(E::LeftRightMix));
        assert_eq!(v("LeftControl+ControlRight"), Err(E::LeftRightMix));
    }

    #[test]
    fn validation_messages() {
        assert_eq!(get_validation_message("F8", &NONE), None);
        let reserved = get_validation_message("Command+C", &NONE).unwrap();
        assert_eq!(reserved.default_text, "Cmd+C is reserved by the system");
        assert_eq!(reserved.params, vec![("label", "Cmd+C".to_string())]);
        let dup = get_validation_message("Ctrl+K", &["Control+K"]).unwrap();
        assert_eq!(dup.key, "hotkey.validation.duplicate");
        assert_eq!(dup.default_text, "That shortcut is already in use.");
    }
}
