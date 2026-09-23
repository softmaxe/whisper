import {
  formatHotkeyDisplay,
  isGlobeLikeHotkey,
  isMouseButtonHotkey,
  parseHotkeyList,
} from "./hotkeys.ts";

export type ValidationErrorCode =
  | "TOO_MANY_KEYS"
  | "NO_MODIFIER_OR_SPECIAL"
  | "LEFT_RIGHT_MIX"
  | "LEFT_MODIFIER_ONLY"
  | "DUPLICATE"
  | "RESERVED"
  | "FN_COMBINATION_UNSUPPORTED"
  | "MODIFIER_ONLY_UNSUPPORTED";

export interface ValidationResult {
  valid: boolean;
  error?: string;
  errorCode?: ValidationErrorCode;
}

const MODIFIER_ORDER = ["Control", "Command", "Alt", "Shift", "Super", "Fn"];

const MODIFIERS = new Set(MODIFIER_ORDER);

const RIGHT_SIDE_MODIFIERS = new Set([
  "rightcontrol",
  "rightctrl",
  "rightalt",
  "rightoption",
  "rightshift",
  "rightcommand",
  "rightcmd",
  "rightsuper",
  "rightmeta",
  "rightwin",
  "controlright",
  "ctrlright",
  "altright",
  "optionright",
  "shiftright",
  "commandright",
  "cmdright",
  "superright",
  "metaright",
  "winright",
]);

const LEFT_SIDE_MODIFIERS = new Set([
  "leftcontrol",
  "leftctrl",
  "leftalt",
  "leftoption",
  "leftshift",
  "leftcommand",
  "leftcmd",
  "leftsuper",
  "leftmeta",
  "leftwin",
  "controlleft",
  "ctrlleft",
  "altleft",
  "optionleft",
  "shiftleft",
  "commandleft",
  "cmdleft",
  "superleft",
  "metaleft",
  "winleft",
]);

function isRightSideModifier(part: string): boolean {
  const normalized = part.replace(/[-_ ]/g, "").toLowerCase();
  return RIGHT_SIDE_MODIFIERS.has(normalized);
}

function isLeftSideModifier(part: string): boolean {
  const normalized = part.replace(/[-_ ]/g, "").toLowerCase();
  return LEFT_SIDE_MODIFIERS.has(normalized);
}

const SPECIAL_KEYS = new Set(
  [
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
  ].concat(Array.from({ length: 24 }, (_, i) => `F${i + 1}`))
);

const RESERVED_SHORTCUTS = [
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
] as const;

function normalizeModifier(part: string): string | null {
  const trimmed = part.replace(/\s+/g, "");
  const lowered = trimmed.toLowerCase();

  if (lowered === "commandorcontrol" || lowered === "cmdorctrl") {
    return "Command";
  }

  if (lowered === "command" || lowered === "cmd") {
    return "Command";
  }

  if (lowered === "control" || lowered === "ctrl") {
    return "Control";
  }

  if (lowered === "alt" || lowered === "option") {
    return "Alt";
  }

  if (lowered === "shift") {
    return "Shift";
  }

  if (lowered === "super" || lowered === "win" || lowered === "meta") {
    return "Command";
  }

  if (lowered === "fn") {
    return "Fn";
  }

  // Handle right-side modifiers (e.g., RightControl, RightOption)
  // These are valid modifiers but we preserve their "Right" prefix for single-modifier validation
  if (isRightSideModifier(part)) {
    // Return a normalized form but mark it as a modifier
    if (lowered.includes("control") || lowered.includes("ctrl")) return "RightControl";
    if (lowered.includes("alt") || lowered.includes("option")) return "RightOption";
    if (lowered.includes("shift")) return "RightShift";
    if (lowered.includes("command") || lowered.includes("cmd")) return "RightCommand";
    if (lowered.includes("super") || lowered.includes("meta") || lowered.includes("win")) {
      return "RightCommand";
    }
  }

  // Handle left-side modifiers (e.g., LeftControl, ControlLeft, LeftOption)
  if (isLeftSideModifier(part)) {
    if (lowered.includes("control") || lowered.includes("ctrl")) return "LeftControl";
    if (lowered.includes("alt") || lowered.includes("option")) return "LeftOption";
    if (lowered.includes("shift")) return "LeftShift";
    if (lowered.includes("command") || lowered.includes("cmd")) return "LeftCommand";
    if (lowered.includes("super") || lowered.includes("meta") || lowered.includes("win")) {
      return "LeftCommand";
    }
  }

  return null;
}

function normalizeKeyToken(part: string): string {
  const trimmed = part.replace(/\s+/g, "");
  const lowered = trimmed.toLowerCase();

  if (lowered === "arrowleft") return "Left";
  if (lowered === "arrowright") return "Right";
  if (lowered === "arrowup") return "Up";
  if (lowered === "arrowdown") return "Down";
  if (lowered === "escape" || lowered === "esc") return "Esc";
  if (lowered === "printscreen" || lowered === "print") return "PrintScreen";
  if (lowered === "pageup" || lowered === "pgup") return "PageUp";
  if (lowered === "pagedown" || lowered === "pgdown") return "PageDown";
  if (lowered === "scrolllock") return "ScrollLock";
  if (lowered === "numlock") return "NumLock";
  if (lowered === "delete" || lowered === "del") return "Delete";
  if (lowered === "insert" || lowered === "ins") return "Insert";
  if (lowered === "space") return "Space";
  if (lowered === "tab") return "Tab";
  if (lowered === "home") return "Home";
  if (lowered === "end") return "End";
  if (lowered === "backspace") return "Backspace";
  if (lowered === "globe") return "GLOBE";
  if (lowered === "fn") return "Fn";
  if (lowered === "mousebutton4") return "MouseButton4";
  if (lowered === "mousebutton5") return "MouseButton5";

  const functionMatch = lowered.match(/^f(\d{1,2})$/);
  if (functionMatch) {
    return `F${functionMatch[1]}`;
  }

  if (trimmed.length === 1) {
    return trimmed.toUpperCase();
  }

  return trimmed;
}

function isLeftRightMix(parts: string[]): boolean {
  const sidesByModifier = new Map<string, Set<string>>();

  const patterns = [
    {
      regex: /^(left|right)[-_ ]?(ctrl|control|alt|option|shift|command|cmd|super|meta|win)$/i,
      sideIndex: 1,
      modIndex: 2,
    },
    {
      regex: /^(ctrl|control|alt|option|shift|command|cmd|super|meta|win)[-_ ]?(left|right)$/i,
      sideIndex: 2,
      modIndex: 1,
    },
  ];

  for (const rawPart of parts) {
    const part = rawPart.replace(/\s+/g, "");
    for (const { regex, sideIndex, modIndex } of patterns) {
      const match = part.match(regex);
      if (match) {
        const side = match[sideIndex].toLowerCase().includes("left") ? "left" : "right";
        const rawModifier = match[modIndex].toLowerCase();
        const normalizedModifier =
          rawModifier === "ctrl"
            ? "control"
            : rawModifier === "cmd"
              ? "command"
              : rawModifier === "option"
                ? "alt"
                : rawModifier === "win" || rawModifier === "meta"
                  ? "super"
                  : rawModifier;
        const set = sidesByModifier.get(normalizedModifier) ?? new Set<string>();
        set.add(side);
        sidesByModifier.set(normalizedModifier, set);
      }
    }
  }

  for (const set of sidesByModifier.values()) {
    if (set.size > 1) {
      return true;
    }
  }

  return false;
}

export function normalizeHotkey(hotkey: string): string {
  if (!hotkey) return "";

  const parts = hotkey
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean);

  const modifiers: string[] = [];
  const keys: string[] = [];

  for (const part of parts) {
    const normalizedModifier = normalizeModifier(part);
    if (normalizedModifier) {
      modifiers.push(normalizedModifier);
      continue;
    }

    keys.push(normalizeKeyToken(part));
  }

  modifiers.sort((a, b) => MODIFIER_ORDER.indexOf(a) - MODIFIER_ORDER.indexOf(b));

  return [...modifiers, ...keys].join("+");
}

export function getValidationMessage(
  hotkey: string,
  existingHotkeys: string[] = []
): string | null {
  const result = validateHotkey(hotkey, existingHotkeys);
  if (result.valid) return null;

  if (result.errorCode === "RESERVED") {
    const label = formatHotkeyDisplay(hotkey);
    return `${label} is reserved by the system`;
  }

  return result.error || "That shortcut is not supported";
}

export function validateHotkey(hotkey: string, existingHotkeys: string[] = []): ValidationResult {
  if (!hotkey || hotkey.trim() === "") {
    return { valid: false, error: "Please enter a valid shortcut." };
  }

  // A slot may hold several hotkeys as a comma-separated list (#936) — validate
  // each entry independently and return the first failure. parseHotkeyList keeps
  // comma-key hotkeys like "Control+," intact, so a single such hotkey falls
  // through to the regular single-hotkey validation below.
  if (hotkey.includes(",")) {
    const items = parseHotkeyList(hotkey);
    if (items.length === 0) {
      return { valid: false, error: "Please enter a valid shortcut." };
    }
    if (items.length > 1) {
      for (const item of items) {
        const result = validateHotkey(item, existingHotkeys);
        if (!result.valid) return result;
      }
      return { valid: true };
    }
    hotkey = items[0];
  }

  if (isGlobeLikeHotkey(hotkey)) {
    return { valid: true };
  }

  // Electron cannot represent Fn inside an accelerator. Stripping it would
  // register the base key globally, so the native listener supports standalone
  // Globe/Fn only.
  if (/^fn\+/i.test(hotkey)) {
    return {
      valid: false,
      error: "The Globe/Fn key can only be used by itself.",
      errorCode: "FN_COMBINATION_UNSUPPORTED",
    };
  }

  if (isMouseButtonHotkey(hotkey)) {
    return { valid: true };
  }

  // Mouse buttons cannot be combined with keyboard modifiers — they're handled
  // by a separate native event tap, not Electron's globalShortcut.
  if (/mousebutton[45]/i.test(hotkey)) {
    return {
      valid: false,
      error: "Mouse button hotkeys cannot be combined with other keys.",
    };
  }

  const parts = hotkey
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean);

  if (parts.length > 3) {
    return {
      valid: false,
      error: "Shortcuts are limited to three keys.",
      errorCode: "TOO_MANY_KEYS",
    };
  }

  if (isLeftRightMix(parts)) {
    return {
      valid: false,
      error: "Do not mix left and right versions of the same modifier in one shortcut.",
      errorCode: "LEFT_RIGHT_MIX",
    };
  }

  let hasModifier = false;
  let hasSpecialKey = false;

  for (const part of parts) {
    const normalizedModifier = normalizeModifier(part);
    if (normalizedModifier) {
      hasModifier = true;
      continue;
    }

    const normalizedKey = normalizeKeyToken(part);
    if (SPECIAL_KEYS.has(normalizedKey)) {
      hasSpecialKey = true;
    }
  }

  if (!hasModifier && !hasSpecialKey) {
    return {
      valid: false,
      error:
        "Shortcuts must include a modifier or a non-alphanumeric key (like arrows, space, or function keys).",
      errorCode: "NO_MODIFIER_OR_SPECIAL",
    };
  }

  // Check for modifier-only hotkeys: require right-side for single modifier, or 2+ modifiers
  const modifierCount = parts.filter((part) => normalizeModifier(part) !== null).length;
  const hasBaseKey = parts.length > modifierCount;

  // The Globe listener reports Fn, right-side modifiers and mouse buttons,
  // nothing else — and Electron cannot register an accelerator without a key,
  // so a modifier-only chord would be accepted here and then fail to bind.
  if (!hasBaseKey && modifierCount >= 2) {
    return {
      valid: false,
      error:
        "Two-modifier shortcuts are not supported on macOS. Use a right-side modifier on its own (e.g. RightOption), or add a regular key.",
      errorCode: "MODIFIER_ONLY_UNSUPPORTED",
    };
  }

  if (!hasBaseKey && modifierCount === 1) {
    const singleMod = parts[0];
    if (!isRightSideModifier(singleMod)) {
      return {
        valid: false,
        error:
          "Single modifier hotkeys must use the right-side key (e.g., RightOption). Or add a regular key (e.g., Control+Space).",
        errorCode: "LEFT_MODIFIER_ONLY",
      };
    }
  }

  const normalizedHotkey = normalizeHotkey(hotkey);
  const normalizedExisting = existingHotkeys.map((existing) => normalizeHotkey(existing));

  if (normalizedExisting.includes(normalizedHotkey)) {
    return {
      valid: false,
      error: "That shortcut is already in use.",
      errorCode: "DUPLICATE",
    };
  }

  const normalizedReserved = RESERVED_SHORTCUTS.map((entry) => normalizeHotkey(entry));

  if (normalizedReserved.includes(normalizedHotkey)) {
    return {
      valid: false,
      error: "That shortcut is reserved by your system.",
      errorCode: "RESERVED",
    };
  }

  return { valid: true };
}
