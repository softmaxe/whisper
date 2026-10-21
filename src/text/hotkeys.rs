//! The [`Hotkey`] value: parsing, the canonical stored spelling, what
//! observes a hotkey, display labels and validation.
//!
//! Settings store hotkeys as strings ("Control+Shift+K", "RightOption",
//! "GLOBE", "MouseButton4"), comma-separated for a list. [`Hotkey::parse`]
//! reads every spelling settings, the capture field and the native listener
//! use, and `to_string()` writes the canonical one, which parses back to the
//! same value. Callers ask the value instead of inspecting strings.

use std::fmt;
use std::sync::LazyLock;

use crate::settings::DEFAULT_HOTKEY;

const HOTKEY_LIST_SEPARATOR: char = ',';

/// A modifier key. The declaration order is the order within a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Modifier {
    Control,
    Command,
    Option,
    Shift,
    Fn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    Left,
    Right,
}

/// Every accepted modifier spelling, lowercase without spaces, dashes or
/// underscores. Sided ones also take a Left/Right prefix or suffix
/// ("RightAlt", "Option-Right", "right_cmd").
const MODIFIER_NAMES: [(&str, Modifier, bool); 16] = [
    ("control", Modifier::Control, true),
    ("ctrl", Modifier::Control, true),
    ("command", Modifier::Command, true),
    ("cmd", Modifier::Command, true),
    ("super", Modifier::Command, true),
    ("meta", Modifier::Command, true),
    ("win", Modifier::Command, true),
    ("commandorcontrol", Modifier::Command, false),
    ("cmdorctrl", Modifier::Command, false),
    ("commandorctrl", Modifier::Command, false),
    ("cmdorcontrol", Modifier::Command, false),
    ("alt", Modifier::Option, true),
    ("option", Modifier::Option, true),
    ("altgr", Modifier::Option, false),
    ("shift", Modifier::Shift, true),
    ("fn", Modifier::Fn, false),
];

impl Modifier {
    /// The listener's name in `MODIFIER_UP:<name>` ("command", "option", ...).
    pub fn from_listener_name(name: &str) -> Option<Self> {
        match parse_modifier(name)? {
            (modifier, None) => Some(modifier),
            _ => None,
        }
    }

    /// Stored spelling: "Control", "Alt", "RightOption", "LeftCommand".
    fn token(self, side: Option<Side>) -> String {
        let Some(side) = side else {
            return match self {
                Self::Control => "Control",
                Self::Command => "Command",
                Self::Option => "Alt",
                Self::Shift => "Shift",
                Self::Fn => "Fn",
            }
            .to_string();
        };
        let side = match side {
            Side::Left => "Left",
            Side::Right => "Right",
        };
        let name = match self {
            Self::Control => "Control",
            Self::Command => "Command",
            Self::Option => "Option",
            Self::Shift => "Shift",
            Self::Fn => "Fn",
        };
        format!("{side}{name}")
    }

    /// User-facing name: "Ctrl", "Option", "Right Cmd".
    pub fn label(self, side: Option<Side>) -> String {
        let name = match self {
            Self::Control => "Ctrl",
            Self::Command => "Cmd",
            Self::Option => "Option",
            Self::Shift => "Shift",
            Self::Fn => "Fn",
        };
        match side {
            None => name.to_string(),
            Some(Side::Left) => format!("Left {name}"),
            Some(Side::Right) => format!("Right {name}"),
        }
    }
}

fn parse_modifier(token: &str) -> Option<(Modifier, Option<Side>)> {
    let compact: String = token
        .chars()
        .filter(|c| !matches!(c, '-' | '_') && !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let lookup = |name: &str, sided: bool| {
        MODIFIER_NAMES
            .iter()
            .find(|(n, _, can_side)| *n == name && (*can_side || !sided))
            .map(|(_, modifier, _)| *modifier)
    };
    if let Some(modifier) = lookup(&compact, false) {
        return Some((modifier, None));
    }
    [(Side::Left, "left"), (Side::Right, "right")]
        .into_iter()
        .find_map(|(side, word)| {
            let base = compact
                .strip_prefix(word)
                .or_else(|| compact.strip_suffix(word))?;
            lookup(base, true).map(|modifier| (modifier, Some(side)))
        })
}

/// Named keys by every accepted spelling (lowercase) and their stored name.
const KEY_NAMES: [(&str, &str); 30] = [
    ("space", "Space"),
    ("tab", "Tab"),
    ("enter", "Enter"),
    ("return", "Enter"),
    ("backspace", "Backspace"),
    ("escape", "Esc"),
    ("esc", "Esc"),
    ("delete", "Delete"),
    ("del", "Delete"),
    ("insert", "Insert"),
    ("ins", "Insert"),
    ("home", "Home"),
    ("end", "End"),
    ("pageup", "PageUp"),
    ("pgup", "PageUp"),
    ("pagedown", "PageDown"),
    ("pgdown", "PageDown"),
    ("up", "Up"),
    ("arrowup", "Up"),
    ("down", "Down"),
    ("arrowdown", "Down"),
    ("left", "Left"),
    ("arrowleft", "Left"),
    ("right", "Right"),
    ("arrowright", "Right"),
    ("printscreen", "PrintScreen"),
    ("print", "PrintScreen"),
    ("pause", "Pause"),
    ("scrolllock", "ScrollLock"),
    ("numlock", "NumLock"),
];

fn normalize_key(token: &str) -> String {
    let trimmed: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    let lowered = trimmed.to_lowercase();
    if let Some((_, name)) = KEY_NAMES.iter().find(|(alias, _)| *alias == lowered) {
        return name.to_string();
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

/// F1 to F24.
fn is_function_key(key: &str) -> bool {
    key.strip_prefix('F')
        .filter(|n| !n.starts_with('0'))
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n))
}

fn mouse_button(name: &str) -> Option<u8> {
    if name.eq_ignore_ascii_case("MouseButton4") {
        Some(4)
    } else if name.eq_ignore_ascii_case("MouseButton5") {
        Some(5)
    } else {
        None
    }
}

/// A key the native listener watches. Unlike a Carbon shortcut, the listener
/// reports its release too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListenerKey {
    /// Globe/Fn.
    Globe,
    /// A right-side modifier on its own; never [`Modifier::Fn`].
    RightModifier(Modifier),
    /// Side mouse button 4 or 5.
    MouseButton(u8),
}

impl ListenerKey {
    /// Reads a listener key name ("GLOBE", "RightOption", "MouseButton4") or
    /// any other spelling of one.
    pub fn parse(name: &str) -> Option<Self> {
        Hotkey::parse(name)?.listener_key()
    }

    fn from_token(token: &str) -> Option<Self> {
        if token.eq_ignore_ascii_case("GLOBE") || token.eq_ignore_ascii_case("Fn") {
            return Some(Self::Globe);
        }
        if let Some(button) = mouse_button(token) {
            return Some(Self::MouseButton(button));
        }
        match parse_modifier(token)? {
            (modifier, Some(Side::Right)) if modifier != Modifier::Fn => {
                Some(Self::RightModifier(modifier))
            }
            _ => None,
        }
    }

    /// The modifier this key is, as Hold mode chords see its release: Globe
    /// is Fn and a right-side modifier is its modifier.
    pub fn modifier(self) -> Option<Modifier> {
        match self {
            Self::Globe => Some(Modifier::Fn),
            Self::RightModifier(modifier) => Some(modifier),
            Self::MouseButton(_) => None,
        }
    }
}

/// The listener's name for the key, also its stored spelling.
impl fmt::Display for ListenerKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Globe => f.write_str("GLOBE"),
            Self::RightModifier(modifier) => f.write_str(&modifier.token(Some(Side::Right))),
            Self::MouseButton(button) => write!(f, "MouseButton{button}"),
        }
    }
}

/// A parsed Dictation hotkey.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hotkey(Repr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Repr {
    Listener(ListenerKey),
    /// Modifiers sorted and de-duplicated, then the other keys in order.
    Chord {
        modifiers: Vec<(Modifier, Option<Side>)>,
        keys: Vec<String>,
    },
}

impl Hotkey {
    /// Reads one hotkey in any accepted spelling; `None` when empty. Unknown
    /// keys are kept, so a hotkey nothing can register still round-trips.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        // The "+" key leaves "++" at the end ("Control++") or stands alone.
        let (head, plus_key) = match value.strip_suffix("++") {
            Some(head) => (head, true),
            None if value == "+" => ("", true),
            None => (value, false),
        };
        let tokens: Vec<&str> = head
            .split('+')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        if let [token] = tokens[..]
            && !plus_key
            && let Some(key) = ListenerKey::from_token(token)
        {
            return Some(key.into());
        }
        let mut modifiers = Vec::new();
        let mut keys = Vec::new();
        for token in tokens {
            match parse_modifier(token) {
                Some(modifier) => modifiers.push(modifier),
                None => keys.push(normalize_key(token)),
            }
        }
        if plus_key {
            keys.push("+".to_string());
        }
        (!modifiers.is_empty() || !keys.is_empty()).then(|| Self::chord(modifiers, keys))
    }

    /// A hotkey pressed in the capture field: `modifiers` held with `key`, as
    /// the UI toolkit names it ("space", "f13", "k"), or the modifiers alone.
    /// `None` for a key a hotkey cannot use.
    pub fn capture(modifiers: &[Modifier], key: Option<&str>) -> Option<Self> {
        let keys = match key {
            Some(key) => {
                let key = normalize_key(key);
                let usable = KEY_NAMES.iter().any(|(_, name)| *name == key)
                    || is_function_key(&key)
                    || (key.len() == 1 && key.bytes().all(|b| b.is_ascii_graphic()));
                if !usable {
                    return None;
                }
                vec![key]
            }
            None => Vec::new(),
        };
        let modifiers: Vec<_> = modifiers.iter().map(|m| (*m, None)).collect();
        (!modifiers.is_empty() || !keys.is_empty()).then(|| Self::chord(modifiers, keys))
    }

    /// This hotkey pressed with Fn held, which nothing can observe.
    pub fn with_fn(self) -> Self {
        let (mut modifiers, keys) = match self.0 {
            Repr::Listener(ListenerKey::RightModifier(modifier)) => {
                (vec![(modifier, Some(Side::Right))], Vec::new())
            }
            Repr::Listener(key) => return key.into(),
            Repr::Chord { modifiers, keys } => (modifiers, keys),
        };
        modifiers.push((Modifier::Fn, None));
        Self::chord(modifiers, keys)
    }

    fn chord(mut modifiers: Vec<(Modifier, Option<Side>)>, keys: Vec<String>) -> Self {
        modifiers.sort();
        modifiers.dedup();
        Self(Repr::Chord { modifiers, keys })
    }

    /// The key when the native listener observes this hotkey.
    pub fn listener_key(&self) -> Option<ListenerKey> {
        match self.0 {
            Repr::Listener(key) => Some(key),
            Repr::Chord { .. } => None,
        }
    }

    pub fn is_globe(&self) -> bool {
        self.listener_key() == Some(ListenerKey::Globe)
    }

    fn has_fn(&self) -> bool {
        matches!(&self.0, Repr::Chord { modifiers, .. }
            if modifiers.iter().any(|(m, _)| *m == Modifier::Fn))
    }

    /// The accelerator to register as a Carbon global shortcut, with modifier
    /// sides dropped ("RightCommand+K" -> "Command+K"). `None` for keys the
    /// native listener observes and for chords with Fn, which cannot be part
    /// of an accelerator.
    pub fn accelerator(&self) -> Option<String> {
        let Repr::Chord { modifiers, keys } = &self.0 else {
            return None;
        };
        if self.has_fn() {
            return None;
        }
        let modifiers = modifiers.iter().map(|(m, _)| (*m, None)).collect();
        Some(Self::chord(modifiers, keys.clone()).to_string())
    }

    /// Modifiers whose release ends a Hold mode press of this chord, as the
    /// listener reports them. Empty for listener keys, which report their
    /// own release.
    pub fn release_modifiers(&self) -> Vec<Modifier> {
        let Repr::Chord { modifiers, .. } = &self.0 else {
            return Vec::new();
        };
        let mut released: Vec<Modifier> = modifiers.iter().map(|(m, _)| *m).collect();
        released.dedup();
        released
    }

    /// Hold mode needs a key whose release macOS reports: the listener's keys
    /// and chords with a modifier. A lone regular key autorepeats and never
    /// reports its release.
    pub fn supports_hold(&self) -> bool {
        self.listener_key().is_some() || !self.release_modifiers().is_empty()
    }

    /// User-facing label: "Ctrl+Right Shift+K", "Globe/Fn".
    pub fn label(&self) -> String {
        self.label_parts().join("+")
    }

    /// The label's parts, one per key.
    pub fn label_parts(&self) -> Vec<String> {
        match &self.0 {
            Repr::Listener(ListenerKey::Globe) => vec!["Globe/Fn".to_string()],
            Repr::Listener(ListenerKey::RightModifier(modifier)) => {
                vec![modifier.label(Some(Side::Right))]
            }
            Repr::Listener(ListenerKey::MouseButton(button)) => {
                vec![format!("Mouse Button {button}")]
            }
            Repr::Chord { modifiers, keys } => modifiers
                .iter()
                .map(|(modifier, side)| modifier.label(*side))
                .chain(keys.iter().cloned())
                .collect(),
        }
    }
}

impl From<ListenerKey> for Hotkey {
    fn from(key: ListenerKey) -> Self {
        Self(Repr::Listener(key))
    }
}

/// The canonical stored spelling.
impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Listener(key) => key.fmt(f),
            Repr::Chord { modifiers, keys } => {
                let parts: Vec<String> = modifiers
                    .iter()
                    .map(|(modifier, side)| modifier.token(*side))
                    .chain(keys.iter().cloned())
                    .collect();
                f.write_str(&parts.join("+"))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Lists

/// Parses a stored hotkey list ("GLOBE,Control+Shift+R"; a single hotkey is a
/// one-item list): empties and duplicates removed, order kept.
pub fn parse_hotkey_list(value: &str) -> Vec<Hotkey> {
    let raw: Vec<&str> = value.split(HOTKEY_LIST_SEPARATOR).collect();
    let mut hotkeys = Vec::new();
    for (i, segment) in raw.iter().enumerate() {
        let mut item = segment.trim().to_string();
        // The comma key is itself a key ("Control+,"): no hotkey ends with
        // "+", so a segment ending in "+" followed by an empty segment (the
        // comma consumed by the split) gets its comma back.
        if item.ends_with('+') && raw.get(i + 1).is_some_and(|next| next.trim().is_empty()) {
            item.push(HOTKEY_LIST_SEPARATOR);
        }
        if let Some(hotkey) = Hotkey::parse(&item)
            && !hotkeys.contains(&hotkey)
        {
            hotkeys.push(hotkey);
        }
    }
    hotkeys
}

/// The stored spelling of a hotkey list.
pub fn serialize_hotkey_list(hotkeys: &[Hotkey]) -> String {
    hotkeys
        .iter()
        .map(Hotkey::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// User-facing label for a stored hotkey list; an empty list shows the
/// default hotkey.
pub fn format_hotkey_list_label(value: &str) -> String {
    let mut hotkeys = parse_hotkey_list(value);
    if hotkeys.is_empty() {
        hotkeys = parse_hotkey_list(DEFAULT_HOTKEY);
    }
    hotkeys
        .iter()
        .map(Hotkey::label)
        .collect::<Vec<_>>()
        .join(" / ")
}

// ---------------------------------------------------------------------------
// Validation

/// Why a hotkey was rejected. `message_key()` is an i18n key for the UI and
/// `default_message()` the English text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyValidationError {
    FnCombinationUnsupported,
    MouseButtonCombination,
    TooManyKeys,
    LeftRightMix,
    NoModifierOrSpecial,
    ModifierOnlyUnsupported,
    LeftModifierOnly,
    Reserved,
}

impl HotkeyValidationError {
    pub fn message_key(self) -> &'static str {
        match self {
            Self::FnCombinationUnsupported => "hotkey.errors.fnCombinationUnsupported",
            Self::MouseButtonCombination => "hotkey.validation.mouseButtonCombination",
            Self::TooManyKeys => "hotkey.validation.tooManyKeys",
            Self::LeftRightMix => "hotkey.validation.leftRightMix",
            Self::NoModifierOrSpecial => "hotkey.validation.noModifierOrSpecial",
            Self::ModifierOnlyUnsupported => "hotkey.validation.modifierOnlyUnsupported",
            Self::LeftModifierOnly => "hotkey.validation.leftModifierOnly",
            Self::Reserved => "hotkey.validation.reserved",
        }
    }

    pub fn default_message(self) -> &'static str {
        match self {
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

const SPECIAL_KEYS: [&str; 18] = [
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
    SPECIAL_KEYS.contains(&key) || is_function_key(key)
}

const RESERVED_SHORTCUTS: [&str; 46] = [
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
];

static RESERVED: LazyLock<Vec<Hotkey>> = LazyLock::new(|| {
    RESERVED_SHORTCUTS
        .iter()
        .filter_map(|s| Hotkey::parse(s))
        .collect()
});

impl Hotkey {
    /// Checks the hotkey against macOS constraints and reserved shortcuts.
    pub fn validate(&self) -> Result<(), HotkeyValidationError> {
        use HotkeyValidationError as E;
        let Repr::Chord { modifiers, keys } = &self.0 else {
            return Ok(());
        };
        // Only standalone Globe/Fn has a native path.
        if self.has_fn() {
            return Err(E::FnCombinationUnsupported);
        }
        // Mouse buttons come from a separate event tap and cannot be combined.
        if keys.iter().any(|key| mouse_button(key).is_some()) {
            return Err(E::MouseButtonCombination);
        }
        if modifiers.len() + keys.len() > 3 {
            return Err(E::TooManyKeys);
        }
        let mixed = modifiers.iter().any(|(modifier, side)| {
            *side == Some(Side::Left) && modifiers.contains(&(*modifier, Some(Side::Right)))
        });
        if mixed {
            return Err(E::LeftRightMix);
        }
        if modifiers.is_empty() && !keys.iter().any(|key| is_special_key(key)) {
            return Err(E::NoModifierOrSpecial);
        }
        // The listener reports Fn, right-side modifiers and mouse buttons
        // only, and an accelerator needs a key, so modifier-only chords can't
        // bind.
        if keys.is_empty() {
            return Err(if modifiers.len() >= 2 {
                E::ModifierOnlyUnsupported
            } else {
                E::LeftModifierOnly
            });
        }
        if RESERVED.contains(self) {
            return Err(E::Reserved);
        }
        Ok(())
    }

    /// The message to show for an invalid hotkey, or `None` when it is valid.
    /// A reserved shortcut is named in the message.
    pub fn validation_message(&self) -> Option<ValidationMessage> {
        let err = self.validate().err()?;
        if err == HotkeyValidationError::Reserved {
            let label = self.label();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use HotkeyValidationError as E;
    use Modifier::{Command, Control, Fn, Option as Opt, Shift};

    fn hk(value: &str) -> Hotkey {
        Hotkey::parse(value).unwrap_or_else(|| panic!("{value:?} does not parse"))
    }

    fn strings(hotkeys: &[Hotkey]) -> Vec<String> {
        hotkeys.iter().map(Hotkey::to_string).collect()
    }

    #[test]
    fn parses_every_spelling_to_its_stored_form() {
        let cases: &[(&str, &str)] = &[
            // Globe/Fn
            ("GLOBE", "GLOBE"),
            ("globe", "GLOBE"),
            ("Fn", "GLOBE"),
            ("fn", "GLOBE"),
            // Mouse buttons
            ("MouseButton4", "MouseButton4"),
            ("mousebutton4", "MouseButton4"),
            ("MOUSEBUTTON5", "MouseButton5"),
            // Right-side modifiers: prefix or suffix, any separator or case
            ("RightOption", "RightOption"),
            ("RightAlt", "RightOption"),
            ("rightalt", "RightOption"),
            ("Right_Alt", "RightOption"),
            ("right-option", "RightOption"),
            ("Right Option", "RightOption"),
            ("OptionRight", "RightOption"),
            ("Option-Right", "RightOption"),
            ("alt_right", "RightOption"),
            ("RightCommand", "RightCommand"),
            ("RightCmd", "RightCommand"),
            ("right_cmd", "RightCommand"),
            ("CmdRight", "RightCommand"),
            ("RightSuper", "RightCommand"),
            ("RightMeta", "RightCommand"),
            ("RightWin", "RightCommand"),
            ("RightControl", "RightControl"),
            ("RightCtrl", "RightControl"),
            ("ControlRight", "RightControl"),
            ("ctrl-right", "RightControl"),
            ("RightShift", "RightShift"),
            ("ShiftRight", "RightShift"),
            // Not modifiers
            ("Right", "Right"),
            ("ArrowRight", "Right"),
            ("RightArrow", "RightArrow"),
            // Chords: modifier aliases and order
            ("CommandOrControl+K", "Command+K"),
            ("CmdOrCtrl+K", "Command+K"),
            ("Cmd+k", "Command+K"),
            ("Super+K", "Command+K"),
            ("Meta+K", "Command+K"),
            ("Win+K", "Command+K"),
            ("Option+R", "Alt+R"),
            ("AltGr+R", "Alt+R"),
            ("Shift+Control+K", "Control+Shift+K"),
            ("Shift+Alt+Control+K", "Control+Alt+Shift+K"),
            (" Control + Shift + Space ", "Control+Shift+Space"),
            ("Ctrl+Ctrl+K", "Control+K"),
            // Chords: key aliases
            ("Ctrl+ArrowLeft", "Control+Left"),
            ("Ctrl+up", "Control+Up"),
            ("Ctrl+Escape", "Control+Esc"),
            ("Ctrl+pgup", "Control+PageUp"),
            ("Ctrl+Return", "Control+Enter"),
            ("Ctrl+f9", "Control+F9"),
            ("Escape", "Esc"),
            ("Command+Plus", "Command+Plus"),
            ("Control+BAD", "Control+BAD"),
            // Punctuation keys, including the list separator and "+"
            ("Control+,", "Control+,"),
            ("Control++", "Control++"),
            ("Option+`", "Alt+`"),
            // Sided modifiers in a chord keep their side
            ("ControlLeft+K", "LeftControl+K"),
            ("ShiftLeft+Space", "LeftShift+Space"),
            ("Control+RightShift+K", "Control+RightShift+K"),
            ("RightCommand+K", "RightCommand+K"),
            ("LeftControl", "LeftControl"),
            ("Control", "Control"),
            // Fn in a chord
            ("Fn+A", "Fn+A"),
            ("Control+Fn+K", "Control+Fn+K"),
        ];
        for (input, stored) in cases {
            let hotkey = hk(input);
            assert_eq!(hotkey.to_string(), *stored, "{input}");
            assert_eq!(hk(stored), hotkey, "{stored} round-trips");
        }
        for empty in ["", "  "] {
            assert_eq!(Hotkey::parse(empty), None);
        }
    }

    #[test]
    fn answers_how_a_hotkey_is_observed() {
        // (hotkey, listener key, accelerator, release modifiers, holdable)
        let cases: &[(&str, Option<&str>, Option<&str>, &[Modifier], bool)] = &[
            ("GLOBE", Some("GLOBE"), None, &[], true),
            ("Fn", Some("GLOBE"), None, &[], true),
            ("mousebutton5", Some("MouseButton5"), None, &[], true),
            ("RightAlt", Some("RightOption"), None, &[], true),
            ("rightcmd", Some("RightCommand"), None, &[], true),
            (
                "Control+Shift+Space",
                None,
                Some("Control+Shift+Space"),
                &[Control, Shift],
                true,
            ),
            ("RightCommand+K", None, Some("Command+K"), &[Command], true),
            (
                "LeftControl+RightControl+K",
                None,
                Some("Control+K"),
                &[Control],
                true,
            ),
            (
                "CommandOrControl+K",
                None,
                Some("Command+K"),
                &[Command],
                true,
            ),
            ("Win+K", None, Some("Command+K"), &[Command], true),
            (
                "Control+Alt",
                None,
                Some("Control+Alt"),
                &[Control, Opt],
                true,
            ),
            ("F8", None, Some("F8"), &[], false),
            ("PageDown", None, Some("PageDown"), &[], false),
            ("Escape", None, Some("Esc"), &[], false),
            ("Fn+A", None, None, &[Fn], true),
        ];
        for (input, listener, accelerator, released, holdable) in cases {
            let hotkey = hk(input);
            assert_eq!(
                hotkey.listener_key().map(|k| k.to_string()).as_deref(),
                *listener,
                "{input}"
            );
            assert_eq!(hotkey.accelerator().as_deref(), *accelerator, "{input}");
            assert_eq!(hotkey.release_modifiers(), *released, "{input}");
            assert_eq!(hotkey.supports_hold(), *holdable, "{input}");
        }
        assert!(hk("Fn").is_globe());
        assert!(!hk("F1").is_globe());
    }

    #[test]
    fn reads_listener_names() {
        let keys = [
            ("GLOBE", ListenerKey::Globe, Some(Fn)),
            ("RightOption", ListenerKey::RightModifier(Opt), Some(Opt)),
            (
                "RightCommand",
                ListenerKey::RightModifier(Command),
                Some(Command),
            ),
            (
                "RightControl",
                ListenerKey::RightModifier(Control),
                Some(Control),
            ),
            ("RightShift", ListenerKey::RightModifier(Shift), Some(Shift)),
            ("MouseButton4", ListenerKey::MouseButton(4), None),
        ];
        for (name, key, modifier) in keys {
            assert_eq!(ListenerKey::parse(name), Some(key), "{name}");
            assert_eq!(key.to_string(), name);
            assert_eq!(key.modifier(), modifier, "{name}");
        }
        for name in ["MouseButton3", "LeftOption", "Control+K", ""] {
            assert_eq!(ListenerKey::parse(name), None, "{name}");
        }
        let modifiers = [
            ("command", Command),
            ("control", Control),
            ("option", Opt),
            ("shift", Shift),
            ("fn", Fn),
        ];
        for (name, modifier) in modifiers {
            assert_eq!(Modifier::from_listener_name(name), Some(modifier));
        }
        assert_eq!(Modifier::from_listener_name("rightshift"), None);
    }

    #[test]
    fn captures_toolkit_key_presses() {
        let cases: &[(&[Modifier], Option<&str>, Option<&str>)] = &[
            (&[Control], Some("k"), Some("Control+K")),
            (
                &[Shift, Control],
                Some("space"),
                Some("Control+Shift+Space"),
            ),
            (&[], Some("escape"), Some("Esc")),
            (&[], Some("enter"), Some("Enter")),
            (&[], Some("up"), Some("Up")),
            (&[], Some("pagedown"), Some("PageDown")),
            (&[], Some("f13"), Some("F13")),
            (&[Command, Shift], Some(","), Some("Command+Shift+,")),
            (&[Opt], Some("`"), Some("Alt+`")),
            (&[Control, Opt], None, Some("Control+Alt")),
            (&[], Some("f25"), None),
            (&[], Some("shift"), None),
            (&[], Some("capslock"), None),
            (&[], Some("ü"), None),
            (&[], None, None),
        ];
        for (modifiers, key, expected) in cases {
            assert_eq!(
                Hotkey::capture(modifiers, *key).map(|h| h.to_string()),
                expected.map(str::to_string),
                "{modifiers:?} {key:?}"
            );
        }
        assert_eq!(hk("RightOption").with_fn().to_string(), "RightOption+Fn");
        assert_eq!(hk("Control+K").with_fn().to_string(), "Control+Fn+K");
        assert_eq!(hk("GLOBE").with_fn(), hk("GLOBE"));
    }

    #[test]
    fn labels() {
        let cases = [
            ("GLOBE", "Globe/Fn"),
            ("Fn", "Globe/Fn"),
            ("MouseButton4", "Mouse Button 4"),
            ("mousebutton5", "Mouse Button 5"),
            ("CommandOrControl+K", "Cmd+K"),
            ("Alt+R", "Option+R"),
            ("Super+K", "Cmd+K"),
            ("RightOption", "Right Option"),
            ("RightCommand", "Right Cmd"),
            ("LeftOption", "Left Option"),
            ("LeftControl", "Left Ctrl"),
            ("Alt", "Option"),
            ("`", "`"),
            ("Control+RightShift+K", "Ctrl+Right Shift+K"),
        ];
        for (hotkey, label) in cases {
            assert_eq!(hk(hotkey).label(), label, "{hotkey}");
        }
        assert_eq!(hk("Control++").label_parts(), ["Ctrl", "+"]);
        assert_eq!(Control.label(Some(Side::Left)), "Left Ctrl");
        assert_eq!(format_hotkey_list_label(""), "Globe/Fn");
        assert_eq!(
            format_hotkey_list_label("GLOBE,Control+Shift+R"),
            "Globe/Fn / Ctrl+Shift+R"
        );
    }

    #[test]
    fn list_parsing() {
        let cases: &[(&str, &[&str])] = &[
            ("Control+Shift+R", &["Control+Shift+R"]),
            ("GLOBE,Control+Shift+R", &["GLOBE", "Control+Shift+R"]),
            (" GLOBE , Control+Shift+R ", &["GLOBE", "Control+Shift+R"]),
            ("F8,,F8,F9,", &["F8", "F9"]),
            ("GLOBE,Fn", &["GLOBE"]),
            ("Ctrl+K,Control+K", &["Control+K"]),
            ("", &[]),
            // The comma key
            ("Control+,", &["Control+,"]),
            ("Command+Shift+,", &["Command+Shift+,"]),
            ("GLOBE,Control+,", &["GLOBE", "Control+,"]),
            ("Control+,,F8", &["Control+,", "F8"]),
            // The plus key
            ("Control++,F8", &["Control++", "F8"]),
            ("Command++,Control+,,", &["Command++", "Control+,"]),
            ("Control++,Control+,", &["Control++", "Control+,"]),
        ];
        for (value, expected) in cases {
            assert_eq!(strings(&parse_hotkey_list(value)), *expected, "{value:?}");
        }
    }

    #[test]
    fn stored_lists_round_trip() {
        let stored = [
            "GLOBE",
            "GLOBE,Control+Shift+R,MouseButton4",
            "RightCommand,Control+Shift+Space",
            "Control+,,F8,GLOBE",
            "Control++,Alt+`",
        ];
        for value in stored {
            assert_eq!(serialize_hotkey_list(&parse_hotkey_list(value)), value);
        }
        // Older spellings are rewritten once, then stay put.
        let legacy = " Fn , CommandOrControl+Shift+Space , RightAlt , mousebutton5 ";
        let once = serialize_hotkey_list(&parse_hotkey_list(legacy));
        assert_eq!(once, "GLOBE,Command+Shift+Space,RightOption,MouseButton5");
        assert_eq!(serialize_hotkey_list(&parse_hotkey_list(&once)), once);
        assert_eq!(serialize_hotkey_list(&[]), "");
    }

    #[test]
    fn validation() {
        let cases: &[(&str, Result<(), E>)] = &[
            ("GLOBE", Ok(())),
            ("Fn", Ok(())),
            ("MouseButton4", Ok(())),
            ("RightOption", Ok(())),
            ("RightAlt", Ok(())),
            ("Option-Right", Ok(())),
            ("F8", Ok(())),
            ("Space", Ok(())),
            ("Esc", Ok(())),
            ("Control+Shift+K", Ok(())),
            ("Control+Alt+Space", Ok(())),
            ("Alt+F7", Ok(())),
            ("ControlLeft+K", Ok(())),
            ("ShiftLeft+Space", Ok(())),
            ("Fn+A", Err(E::FnCombinationUnsupported)),
            ("RightOption+Fn", Err(E::FnCombinationUnsupported)),
            ("Control+MouseButton4", Err(E::MouseButtonCombination)),
            ("Ctrl+Alt+Shift+K", Err(E::TooManyKeys)),
            ("K", Err(E::NoModifierOrSpecial)),
            ("F25", Err(E::NoModifierOrSpecial)),
            ("LeftCtrl+RightCtrl+K", Err(E::LeftRightMix)),
            ("LeftControl+RightControl", Err(E::LeftRightMix)),
            ("ControlLeft+ControlRight", Err(E::LeftRightMix)),
            ("LeftControl+ControlRight", Err(E::LeftRightMix)),
            ("Control+Alt", Err(E::ModifierOnlyUnsupported)),
            ("Control", Err(E::LeftModifierOnly)),
            ("ControlLeft", Err(E::LeftModifierOnly)),
            ("Command+C", Err(E::Reserved)),
            ("Cmd+c", Err(E::Reserved)),
            ("Command+,", Err(E::Reserved)),
        ];
        for (hotkey, expected) in cases {
            assert_eq!(hk(hotkey).validate(), *expected, "{hotkey}");
        }
    }

    #[test]
    fn validation_messages() {
        assert_eq!(hk("F8").validation_message(), None);
        let reserved = hk("Command+C").validation_message().unwrap();
        assert_eq!(reserved.default_text, "Cmd+C is reserved by the system");
        assert_eq!(reserved.params, vec![("label", "Cmd+C".to_string())]);
        let fn_combo = hk("Fn+A").validation_message().unwrap();
        assert_eq!(fn_combo.key, "hotkey.errors.fnCombinationUnsupported");
    }
}
