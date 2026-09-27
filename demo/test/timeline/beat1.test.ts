/**
 * Beat 1 · Opening: slow typing, the circled fn key, the Double tap and the
 * Recording pill, in story order, with picture and sound on the same times.
 */
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { ROOT } from "../../scripts/paths.ts";
import { BEAT1_TEXT, beat1 } from "../../timeline/beats/beat1-opening.ts";
import { DOUBLE_TAP_GAP, doubleTapMoments, keystrokeTimes } from "../../timeline/helpers.ts";
import { LANGS } from "../../timeline/index.ts";

const m = beat1.moments;
const cuesOf = (type: string) => beat1.cues.filter((c) => c.type === type).map((c) => c.at);

describe("Beat 1 story", () => {
  it("asks whether typing is too slow, then tells the viewer to Double tap fn", () => {
    expect(beat1.captions.map((c) => c.id)).toEqual(["b1-slow", "b1-fn"]);
    const [slow, fn] = beat1.captions;
    const [typing] = beat1.typing;
    expect(slow.start).toBeLessThanOrEqual(typing.start);
    expect(fn.start).toBeLessThanOrEqual(m.fnCircle);
    expect(fn.end).toBeGreaterThan(m.listen);
    for (const lang of LANGS) expect(fn.text[lang]).toContain("fn");
  });

  it("types, gives up, circles fn, labels it, Double taps, then listens", () => {
    const [typing] = beat1.typing;
    const order = [typing.start, typing.end, m.giveUp, m.fnCircle, m.fnNote, ...m.taps, m.listen, m.beatOut];
    expect([...order].sort((a, b) => a - b)).toEqual(order);
  });

  it("leaves the Recording pill on screen long enough to watch the bars move", () => {
    expect(m.beatOut - m.listen).toBeGreaterThanOrEqual(2);
  });
});

describe("Beat 1 sound", () => {
  it("clicks a key on every keystroke and both taps of the Double tap", () => {
    const [typing] = beat1.typing;
    expect(cuesOf("key_click")).toEqual([...keystrokeTimes(typing), ...m.taps]);
  });

  it("scratches the red pen as the circle and the label are drawn", () => {
    expect(cuesOf("pen_scratch")).toEqual([m.fnCircle, m.fnNote]);
  });

  it("plays the app's recording-start chime when the pill opens", () => {
    expect(cuesOf("rec_start")).toEqual([m.listen]);
  });
});

describe("Beat 1 text", () => {
  it("has typed text and the key label in both cuts, in the bundled font subset", () => {
    const charset = new Set(fs.readFileSync(path.join(ROOT, "public/fonts/LXGWWenKai-Regular.subset.charset.txt"), "utf8"));
    for (const lang of LANGS) {
      expect(BEAT1_TEXT.typed[lang].length).toBeGreaterThan(0);
      // The label is handwritten in LXGW WenKai; the typed text uses the system font.
      expect([...BEAT1_TEXT.fnNote[lang]].filter((ch) => !charset.has(ch)), lang).toEqual([]);
    }
  });
});

describe("Review frames", () => {
  it("catch the circled key once labelled, and the pill once listening", () => {
    const at = Object.fromEntries((beat1.reviewFrames ?? []).map((f) => [f.name, f.at]));
    expect(at["fn-circled"]).toBeGreaterThan(m.fnNote + 0.8);
    expect(at["fn-circled"]).toBeLessThan(m.taps[0]);
    expect(at["recording-pill"]).toBeGreaterThan(m.listen + 0.5);
    expect(at["recording-pill"]).toBeLessThan(m.beatOut);
  });
});

describe("doubleTapMoments", () => {
  it("spaces the two presses and opens the pill just after the second", () => {
    const tap = doubleTapMoments(2);
    expect(tap.taps).toEqual([2, 2 + DOUBLE_TAP_GAP]);
    expect(tap.listen).toBeGreaterThan(tap.taps[1]);
    expect(tap.listen - tap.taps[1]).toBeLessThan(0.1);
  });
});
