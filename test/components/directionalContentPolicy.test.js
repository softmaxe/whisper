const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const repoRoot = path.join(__dirname, "../..");

function source(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

test("dynamic prose and identity values keep their own direction", () => {
  const expectations = [
    [
      "src/components/dictation/LiveTranscriptPanel.tsx",
      /<p\s+dir="auto"[\s\S]*?\{shimmerParts\.settled\}/,
    ],
    ["src/components/ui/TranscriptionItem.tsx", /<p\s+dir="auto"[\s\S]*?\{item\.text\}/],
    ["src/components/ui/TranscriptionItem.tsx", /<p\s+dir="auto"[^>]*>\s*\{rawText\}/],
    ["src/components/CommandSearch.tsx", /<p\s+dir="auto"[^>]*>\s*\{transcript\.text\}/],
    ["src/components/DictionaryView.tsx", /<span\s+dir="auto"[^>]*>\s*\{word\}/],
    ["src/components/SnippetsView.tsx", /<span\s+dir="auto"[^>]*>\s*\{snippet\.trigger\}/],
    ["src/components/SnippetsView.tsx", /<span\s+dir="auto"[^>]*>\s*\{snippet\.replacement\}/],
  ];

  for (const [file, pattern] of expectations) {
    assert.match(source(file), pattern, `${file} lost its content-direction policy`);
  }
});

test("technical output values remain LTR inside an Arabic document", () => {
  const expectations = [
    ["src/components/ui/TechnicalErrorDetails.tsx", /<pre\s+dir="ltr"[\s\S]*?\{text\}/],
    ["src/components/ui/NixOsPasteInfo.tsx", /<div\s+dir="ltr"[\s\S]*?<pre/],
    [
      "src/components/dictation/AssistantPanel.tsx",
      /<kbd\s+dir="ltr"[\s\S]*?\{readableVoiceHotkey\}/,
    ],
    [
      "src/components/notes/UploadAudioView.tsx",
      /<p\s+dir="ltr"[^>]*font-medium[^>]*>\s*\{file\.name\}/,
    ],
    [
      "src/components/notes/UploadAudioView.tsx",
      /<p\s+dir="ltr"[^>]*max-w-50[^>]*>\s*\{file\.name\}/,
    ],
    ["src/components/ui/SidebarModal.tsx", /<span\s+dir="ltr"[\s\S]*?v\{version\}/],
    ["src/components/ui/ModelCardList.tsx", /<span\s+dir="ltr"[\s\S]*?\{model\.label\}/],
  ];

  for (const [file, pattern] of expectations) {
    assert.match(source(file), pattern, `${file} lost its LTR technical-output isolation`);
  }
});

test("localized sentences isolate technical interpolations without changing word order", () => {
  const expectations = [
    [
      "src/components/SettingsPage.tsx",
      /<BidiInterpolatedText[\s\S]*?hyprlandConfigWriteWarningDescription[\s\S]*?value=\{hyprlandConfigStatus\.path\}/,
    ],
    [
      "src/components/SettingsPage.tsx",
      /<BidiInterpolatedText[\s\S]*?resetToDefault[\s\S]*?value=\{formatHotkeyLabel\(effectiveDefaultHotkey\)\}/,
    ],
  ];

  for (const [file, pattern] of expectations) {
    const text = source(file);
    assert.match(text, pattern, `${file} lost a bidi-isolated technical interpolation`);
    assert.match(text, /BIDI_VALUE_TOKEN/, `${file} must interpolate with the stable marker`);
  }
});

test("direction-sensitive transient motion mirrors in RTL", () => {
  assert.match(
    source("src/components/dictation/LiveTranscriptPanel.tsx"),
    /pointer-events-none translate-x-2 rtl:-translate-x-2 opacity-0/,
    "LiveTranscriptPanel controls must retreat toward the document end"
  );
});
