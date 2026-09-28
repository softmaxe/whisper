/**
 * Beat 2 · Speak and it's written: the Mail cleanup and paste, then the team
 * chat correction recorded in the Dictionary, in story order, with picture
 * and sound on the same times.
 */
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { ROOT } from "../../scripts/paths.ts";
import { COPY } from "../../src/copy.ts";
import { beat1 } from "../../timeline/beats/beat1-opening.ts";
import { BEAT2_TEXT, beat2 } from "../../timeline/beats/beat2-speak.ts";
import { LANGS } from "../../timeline/index.ts";

const m = beat2.moments;
const cuesOf = (type: string) => beat2.cues.filter((c) => c.type === type).map((c) => c.at);
const charset = new Set(fs.readFileSync(path.join(ROOT, "public/fonts/LXGWWenKai-Regular.subset.charset.txt"), "utf8"));
const missing = (text: string) => [...text].filter((ch) => !charset.has(ch));

describe("Beat 2 story", () => {
  it("dictates, cleans up and pastes into Mail, then corrects a word in team chat and learns it", () => {
    const order = [
      m.sketchIn,
      m.wordsIn,
      m.wordsEnd,
      m.stop,
      ...m.strikes,
      ...m.punctuation,
      m.paste,
      m.pasteNote,
      m.mailOut,
      m.chatIn,
      ...m.chatTaps,
      m.chatListen,
      m.chatStop,
      m.chatPaste,
      m.circle,
      m.fix,
      m.notebookIn,
      m.entry,
      m.tick,
      m.beatOut,
    ];
    expect([...order].sort((a, b) => a - b)).toEqual(order);
  });

  it("picks up Beat 1's Dictation without starting a new one", () => {
    expect(beat1.moments.listen).toBeLessThan(m.sketchIn);
    expect(cuesOf("rec_start")).toEqual([m.chatListen]);
  });

  it("strikes every filler and adds a mark for each punctuation gap, in both cuts", () => {
    for (const lang of LANGS) {
      expect(COPY[lang].mail.spoken.filter((tok) => tok.filler)).toHaveLength(m.strikes.length);
    }
    expect(m.punctuation).toHaveLength(3);
  });

  it("uses a misheard product name that appears in the dictated reply, corrected to Supabase", () => {
    for (const lang of LANGS) {
      const chat = COPY[lang].chat;
      expect(chat.right).toBe("Supabase");
      expect(chat.heard).toContain(chat.wrong);
      expect(chat.toast).toContain(chat.right);
    }
  });

  it("gives the viewer time to read the cleaned Mail and the Dictionary entry", () => {
    expect(m.mailOut - m.paste).toBeGreaterThanOrEqual(2.5);
    expect(m.beatOut - m.tick).toBeGreaterThanOrEqual(2.5);
  });

  it("narrates each step in step with the picture", () => {
    const at = Object.fromEntries(beat2.captions.map((c) => [c.id, c]));
    expect(at["b2-clean"].start).toBeLessThanOrEqual(m.strikes[0]);
    expect(at["b2-paste"].start).toBeLessThanOrEqual(m.paste);
    expect(at["b2-misheard"].start).toBeLessThanOrEqual(m.circle);
    expect(at["b2-learn"].start).toBeLessThanOrEqual(m.entry);
    expect(at["b2-learn"].end).toBeGreaterThan(m.tick + 2);
  });
});

describe("Beat 2 sound", () => {
  it("stops recording as each Dictation ends", () => {
    expect(cuesOf("rec_stop")).toEqual([m.stop, m.chatStop]);
  });

  it("pastes with a sound when the text lands in Mail and in the chat", () => {
    expect(cuesOf("paste")).toEqual([m.paste, m.chatPaste]);
  });

  it("clicks both taps of the Double tap", () => {
    expect(cuesOf("key_click")).toEqual([...m.chatTaps]);
  });

  it("scratches the red pen for every mark and ticks the Dictionary entry", () => {
    expect(cuesOf("pen_scratch")).toEqual([...m.strikes, ...m.punctuation, m.pasteNote, m.circle, m.fix, m.entry]);
    expect(cuesOf("tick")).toEqual([m.tick]);
  });
});

describe("Beat 2 text", () => {
  it("handwrites notes, the correction and the Dictionary entry with characters in the font subset", () => {
    for (const lang of LANGS) {
      const copy = COPY[lang];
      const handwritten = [BEAT2_TEXT.pasted[lang], copy.chat.wrong, copy.chat.right, copy.whisper.nav.dictionary, "。，.,"];
      for (const text of handwritten) expect(missing(text), `${lang}: ${text}`).toEqual([]);
    }
  });

  it("keeps the corrected chat reply and the Mail reply in History", () => {
    for (const lang of LANGS) {
      const copy = COPY[lang];
      const texts = copy.whisper.history.map((e) => e.text);
      expect(texts, lang).toContain(copy.chat.heard.replace(copy.chat.wrong, copy.chat.right));
      expect(texts, lang).toContain(copy.mail.cleaned);
    }
  });
});

describe("Review frames", () => {
  it("catch the cleaned Mail once pasted and labelled, and the ticked Dictionary entry", () => {
    const at = Object.fromEntries((beat2.reviewFrames ?? []).map((f) => [f.name, f.at]));
    expect(at["mail-cleaned"]).toBeGreaterThan(m.pasteNote + 1);
    expect(at["mail-cleaned"]).toBeLessThan(m.mailOut);
    expect(at["dictionary-entry"]).toBeGreaterThan(m.tick + 1);
    expect(at["dictionary-entry"]).toBeLessThan(m.beatOut);
  });
});
