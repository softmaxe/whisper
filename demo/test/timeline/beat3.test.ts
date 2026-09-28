/**
 * Beat 3 · More ways to use it: the Snippet, Hold mode and Upload notes in
 * turn, in story order, with picture and sound on the same times.
 */
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { ROOT } from "../../scripts/paths.ts";
import { COPY } from "../../src/copy.ts";
import { BEAT3_TEXT, beat3, FILE_TRANSCRIBE_SECONDS } from "../../timeline/beats/beat3-more.ts";
import { DOUBLE_TAP_GAP } from "../../timeline/helpers.ts";
import { LANGS } from "../../timeline/index.ts";

const m = beat3.moments;
const cuesOf = (type: string) => beat3.cues.filter((c) => c.type === type).map((c) => c.at);
const caption = (id: string) => beat3.captions.find((c) => c.id === id)!;

describe("Beat 3 story", () => {
  it("has one Caption per note, each showing while its note plays", () => {
    expect(beat3.captions.map((c) => c.id)).toEqual(["b3-snippet", "b3-hold", "b3-upload"]);
    const notes = [
      [m.snippetIn, m.snippetOut],
      [m.holdIn, m.holdOut],
      [m.uploadIn, m.beatOut],
    ];
    beat3.captions.forEach((c, i) => {
      const [noteIn, noteOut] = notes[i];
      expect(c.start, c.id).toBeGreaterThanOrEqual(noteIn - 0.5);
      expect(c.end, c.id).toBeLessThanOrEqual(noteOut + 0.5);
    });
    for (const lang of LANGS) expect(caption("b3-hold").text[lang]).toContain("fn");
  });

  it("Snippet: Double tap, say the trigger, stop, paste the expansion, then circle and arrow it", () => {
    const order = [m.snippetIn, ...m.taps, m.listen, m.said, m.snippetStop, m.snippetPaste, m.triggerCircle, m.expandArrow, m.snippetOut];
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(m.taps[1] - m.taps[0]).toBeCloseTo(DOUBLE_TAP_GAP);
  });

  it("Hold mode: press and hold, listen, release, paste on release, then tick", () => {
    const order = [m.holdIn, m.holdPress, m.holdListen, m.holdNote, m.holdRelease, m.releaseNote, m.holdPaste, m.holdTick, m.holdOut];
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(m.holdListen - m.holdPress).toBeLessThan(0.1);
    // Long enough to see the key held and the pill listening.
    expect(m.holdRelease - m.holdListen).toBeGreaterThanOrEqual(1.5);
    expect(m.holdPaste - m.holdRelease).toBeLessThan(0.5);
  });

  it("Upload: files dropped, each transcribed in turn, then the batch completes", () => {
    const order = [m.uploadIn, m.filesIn, m.drop, ...m.filesDone, m.uploadComplete, m.beatOut];
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(m.filesDone).toHaveLength(COPY.en.whisper.upload.files.length);
    expect(m.filesDone[0] - FILE_TRANSCRIBE_SECONDS).toBeGreaterThan(m.drop);
  });

  it("brings each note in after the previous one leaves", () => {
    expect(m.holdIn).toBeGreaterThanOrEqual(m.snippetOut);
    expect(m.uploadIn).toBeGreaterThanOrEqual(m.holdOut);
    expect(beat3.end - m.beatOut).toBeGreaterThanOrEqual(0.4);
  });
});

describe("Beat 3 sound", () => {
  it("clicks fn for the Double tap, the Hold press and its release", () => {
    expect(cuesOf("key_click")).toEqual([...m.taps, m.holdPress, m.holdRelease]);
  });

  it("chimes when each pill opens and stops listening", () => {
    expect(cuesOf("rec_start")).toEqual([m.listen, m.holdListen]);
    expect(cuesOf("rec_stop")).toEqual([m.snippetStop, m.holdRelease]);
  });

  it("scratches the red pen for every mark", () => {
    expect(cuesOf("pen_scratch")).toEqual([
      m.triggerCircle,
      m.expandArrow,
      m.holdNote,
      m.releaseNote,
      m.holdTick,
      ...m.filesDone,
      m.uploadComplete,
    ]);
  });

  it("pastes with a sound when the snippet and the comment land", () => {
    expect(cuesOf("paste")).toEqual([m.snippetPaste, m.holdPaste]);
  });

  it("pats the files down when they are dropped", () => {
    expect(cuesOf("file_drop")).toEqual([m.drop]);
  });
});

describe("Beat 3 text", () => {
  it("writes only characters in the bundled font subset, in both cuts", () => {
    const charset = new Set(fs.readFileSync(path.join(ROOT, "public/fonts/LXGWWenKai-Regular.subset.charset.txt"), "utf8"));
    for (const [key, text] of Object.entries(BEAT3_TEXT)) {
      for (const lang of LANGS) {
        expect(text[lang].length, `${key} ${lang}`).toBeGreaterThan(0);
        expect([...text[lang]].filter((ch) => !charset.has(ch)), `${key} ${lang}`).toEqual([]);
      }
    }
  });

  it("says the Snippet trigger phrase from the app copy", () => {
    for (const lang of LANGS) expect(BEAT3_TEXT.said[lang]).toContain(COPY[lang].snippet.trigger);
  });
});

describe("Review frames", () => {
  it("catch each note once it is complete, before it leaves", () => {
    const at = Object.fromEntries((beat3.reviewFrames ?? []).map((f) => [f.name, f.at]));
    expect(at["snippet-done"]).toBeGreaterThan(m.expandArrow + 0.6);
    expect(at["snippet-done"]).toBeLessThan(m.snippetOut);
    expect(at["hold-done"]).toBeGreaterThan(m.holdTick + 0.4);
    expect(at["hold-done"]).toBeLessThan(m.holdOut);
    expect(at["upload-done"]).toBeGreaterThan(m.uploadComplete + 0.5);
    expect(at["upload-done"]).toBeLessThan(m.beatOut);
  });
});
