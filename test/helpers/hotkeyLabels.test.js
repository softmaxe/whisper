const test = require("node:test");
const assert = require("node:assert/strict");

const load = () => import("../../src/utils/hotkeys.ts");

test("isGlobeLikeHotkey matches exactly GLOBE and Fn, nothing else", async () => {
  const { isGlobeLikeHotkey } = await load();

  assert.equal(isGlobeLikeHotkey("GLOBE"), true);
  assert.equal(isGlobeLikeHotkey("Fn"), true);
  assert.equal(isGlobeLikeHotkey("F1"), false);
  assert.equal(isGlobeLikeHotkey("globe"), false);
  assert.equal(isGlobeLikeHotkey("fn"), false);
  assert.equal(isGlobeLikeHotkey(""), false);
});

test("empty input formats to an empty label", async () => {
  const { formatHotkeyLabelForPlatform } = await load();

  assert.equal(formatHotkeyLabelForPlatform("", "darwin"), "");
  assert.equal(formatHotkeyLabelForPlatform("  ", "darwin"), "");
});

test("globe-like hotkeys display as Globe/Fn on macOS", async () => {
  const { formatHotkeyLabelForPlatform } = await load();

  assert.equal(formatHotkeyLabelForPlatform("GLOBE", "darwin"), "Globe/Fn");
});

test("mouse button hotkeys display with spaces", async () => {
  const { formatHotkeyLabelForPlatform } = await load();

  assert.equal(formatHotkeyLabelForPlatform("MouseButton4", "darwin"), "Mouse Button 4");
  assert.equal(formatHotkeyLabelForPlatform("MouseButton5", "darwin"), "Mouse Button 5");
});

test("CommandOrControl displays as Cmd on macOS", async () => {
  const { formatHotkeyLabelForPlatform } = await load();

  assert.equal(formatHotkeyLabelForPlatform("CommandOrControl+K", "darwin"), "Cmd+K");
});

test("Alt displays as Option on macOS", async () => {
  const { formatHotkeyLabelForPlatform } = await load();

  assert.equal(formatHotkeyLabelForPlatform("Alt+R", "darwin"), "Option+R");
});

test("Super and Meta display as Cmd on macOS", async () => {
  const { formatHotkeyLabelForPlatform } = await load();

  assert.equal(formatHotkeyLabelForPlatform("Super+K", "darwin"), "Cmd+K");
  assert.equal(formatHotkeyLabelForPlatform("Meta+K", "darwin"), "Cmd+K");
});

test("right-side single modifiers get spelled-out platform-aware labels", async () => {
  const { formatHotkeyLabelForPlatform } = await load();

  assert.equal(formatHotkeyLabelForPlatform("RightOption", "darwin"), "Right Option");
  assert.equal(formatHotkeyLabelForPlatform("RightCommand", "darwin"), "Right Cmd");
});

test("left-side modifiers are labelled by side too, so a rejection can name the key pressed", async () => {
  const { formatHotkeyLabelForPlatform } = await load();

  assert.equal(formatHotkeyLabelForPlatform("LeftOption", "darwin"), "Left Option");
  assert.equal(formatHotkeyLabelForPlatform("LeftControl", "darwin"), "Left Ctrl");
  assert.equal(formatHotkeyLabelForPlatform("LeftCommand", "darwin"), "Left Cmd");
});

test("bare modifier tokens format like they do inside a chord", async () => {
  const { formatHotkeyLabelForPlatform } = await load();

  assert.equal(formatHotkeyLabelForPlatform("Alt", "darwin"), "Option");
  assert.equal(formatHotkeyLabelForPlatform("Command", "darwin"), "Cmd");
});

test("sidedModifierToken names the physical key behind a modifier code", async () => {
  const { sidedModifierToken } = await load();

  assert.equal(sidedModifierToken("AltRight", "darwin"), "RightOption");
  assert.equal(sidedModifierToken("AltLeft", "darwin"), "LeftOption");
  assert.equal(sidedModifierToken("MetaLeft", "darwin"), "LeftCommand");
  assert.equal(sidedModifierToken("ShiftRight", "darwin"), "RightShift");
  // Sideless codes have no side to report.
  assert.equal(sidedModifierToken("CapsLock", "darwin"), null);
  assert.equal(sidedModifierToken("KeyK", "darwin"), null);
});

test("single keys pass through unchanged", async () => {
  const { formatHotkeyLabelForPlatform } = await load();

  assert.equal(formatHotkeyLabelForPlatform("`", "darwin"), "`");
});

test("parseHotkey splits modifiers from the base key", async () => {
  const { parseHotkey } = await load();

  assert.deepEqual(parseHotkey("CommandOrControl+Shift+K"), {
    modifiers: ["CommandOrControl", "Shift"],
    baseKey: "K",
  });
  assert.deepEqual(parseHotkey("Alt+R"), { modifiers: ["Alt"], baseKey: "R" });
  assert.deepEqual(parseHotkey("F8"), { modifiers: [], baseKey: "F8" });
  assert.deepEqual(parseHotkey(""), { modifiers: [], baseKey: "" });
  assert.deepEqual(parseHotkey(null), { modifiers: [], baseKey: "" });
});

test("isCompoundHotkey is true only when modifiers are present", async () => {
  const { isCompoundHotkey } = await load();

  assert.equal(isCompoundHotkey("Ctrl+Shift+K"), true);
  assert.equal(isCompoundHotkey("Alt+R"), true);
  assert.equal(isCompoundHotkey("F8"), false);
  assert.equal(isCompoundHotkey("GLOBE"), false);
  assert.equal(isCompoundHotkey(""), false);
  assert.equal(isCompoundHotkey(null), false);
});

test("isValidHotkeyFormat accepts single keys, globe, mouse buttons, and well-formed combos", async () => {
  const { isValidHotkeyFormat } = await load();

  assert.equal(isValidHotkeyFormat("GLOBE"), true);
  assert.equal(isValidHotkeyFormat("Fn"), true);
  assert.equal(isValidHotkeyFormat("MouseButton4"), true);
  assert.equal(isValidHotkeyFormat("`"), true);
  assert.equal(isValidHotkeyFormat("A"), true);
  assert.equal(isValidHotkeyFormat("Ctrl+K"), true);
  assert.equal(isValidHotkeyFormat("Alt+Shift+F9"), true);
});

test("isValidHotkeyFormat rejects empty input and combos with empty parts", async () => {
  const { isValidHotkeyFormat } = await load();

  assert.equal(isValidHotkeyFormat(""), false);
  assert.equal(isValidHotkeyFormat("  "), false);
  assert.equal(isValidHotkeyFormat("Ctrl+"), false);
  assert.equal(isValidHotkeyFormat("+K"), false);
});

test("parseHotkeyList preserves hotkeys ending with '+' when followed by another hotkey", async () => {
  const { parseHotkeyList } = await load();

  assert.deepEqual(parseHotkeyList("Control++,F8"), ["Control++", "F8"]);
  assert.deepEqual(parseHotkeyList("Control+,,F8"), ["Control+,", "F8"]);
  assert.deepEqual(parseHotkeyList("Control++,Control+,"), ["Control++", "Control+,"]);
});
