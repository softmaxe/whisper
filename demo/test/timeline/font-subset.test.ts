/**
 * Everything the Film writes by hand is set in the bundled LXGW WenKai subset,
 * which only draws the characters listed in its charset file. A missing
 * character falls back to a system face and breaks the hand-drawn look, so
 * every handwritten string must stay inside the subset, in both cuts.
 */
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { ROOT } from "../../scripts/paths.ts";
import { COPY } from "../../src/copy.ts";
import { BEAT1_TEXT } from "../../timeline/beats/beat1-opening.ts";
import { BEAT2_TEXT } from "../../timeline/beats/beat2-speak.ts";
import { BEAT3_TEXT } from "../../timeline/beats/beat3-more.ts";
import { BEAT5_TEXT } from "../../timeline/beats/beat5-servers.ts";
import { allCaptions, LANGS } from "../../timeline/index.ts";
import type { Lang } from "../../timeline/types.ts";

const FONT_DIR = path.join(ROOT, "public/fonts");
const charset = new Set(fs.readFileSync(path.join(FONT_DIR, "LXGWWenKai-Regular.subset.charset.txt"), "utf8"));
const missing = (text: string) => [...new Set(text)].filter((ch) => ch !== "\n" && !charset.has(ch));

/** Film-only text of every Beat: notes, titles, labels and speech bubbles. */
const BEAT_TEXTS: Record<string, Record<string, Record<Lang, string>>> = {
  beat1: BEAT1_TEXT,
  beat2: BEAT2_TEXT,
  beat3: BEAT3_TEXT,
  beat5: BEAT5_TEXT,
};

/** App copy the Film writes by hand (red-pen notes, notebook entries, pencilled labels and values). */
function handwrittenCopy(lang: Lang): Record<string, string> {
  const copy = COPY[lang];
  const { insights, nav, settings, today } = copy.whisper;
  const v = insights.values;
  return {
    saidLabel: copy.saidLabel,
    "chat.wrong": copy.chat.wrong,
    "chat.right": copy.chat.right,
    "nav.dictionary": nav.dictionary,
    today,
    "insights.values": [v.words, v.wpm, v.dictations].map((n) => n.toLocaleString(lang)).join(" "),
    "insights.days": insights.days(v.streak),
    "settings.speechToText": settings.speechToText,
    "settings.cleanup": settings.cleanup,
    "outro.link": copy.outro.link,
  };
}

/** Source files of the picture, with comments removed, for text written literally in components. */
function pictureSources(dir = path.join(ROOT, "src/film")): { file: string; code: string }[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return pictureSources(full);
    if (!/\.tsx?$/.test(entry.name)) return [];
    const code = fs
      .readFileSync(full, "utf8")
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/(^|\s)\/\/.*$/gm, "");
    return [{ file: path.relative(ROOT, full), code }];
  });
}

/** Literal characters the picture sets in the app's system font, not by hand: the chat toast's tick. */
const SYSTEM_FONT_LITERALS = new Set(["✓"]);

const HINT = "re-run scripts/subset_font.py with these characters";

describe("Handwritten text uses only characters in the bundled font subset", () => {
  it.each(LANGS)("Captions (%s)", (lang) => {
    for (const c of allCaptions()) expect(missing(c.text[lang]), `${c.id}: ${HINT}`).toEqual([]);
  });

  it.each(LANGS)("Beat notes, titles and labels (%s)", (lang) => {
    for (const [beat, texts] of Object.entries(BEAT_TEXTS)) {
      for (const [key, text] of Object.entries(texts)) expect(missing(text[lang]), `${beat}.${key}: ${HINT}`).toEqual([]);
    }
  });

  it.each(LANGS)("app copy written by hand (%s)", (lang) => {
    for (const [key, text] of Object.entries(handwrittenCopy(lang))) expect(missing(text), `${key}: ${HINT}`).toEqual([]);
  });

  it("text written literally in the picture's components (punctuation marks, Whis's marks, the model sheet)", () => {
    for (const { file, code } of pictureSources()) {
      const literal = [...code].filter(
        (ch) => ch.codePointAt(0)! > 0x7f && !SYSTEM_FONT_LITERALS.has(ch) && !/\p{Extended_Pictographic}/u.test(ch),
      );
      expect(missing(literal.join("")), `${file}: ${HINT}`).toEqual([]);
    }
  });
});
