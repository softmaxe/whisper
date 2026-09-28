/**
 * The Film timeline: generic rules every Beat must keep. Per-Beat story checks
 * (texts, moments, cues) live in beatN.test.ts, so this file names no Caption,
 * count or Beat time.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { exportTimeline } from "../../scripts/export-timeline.ts";
import { ROOT } from "../../scripts/paths.ts";
import { reviewFrameTimes } from "../../scripts/review-frames.ts";
import { CAPTION_LINE_HEIGHT, emWidth, layoutLines } from "../../src/film/components/captionLayout.ts";
import { BEAT_BOUNDS } from "../../timeline/bounds.ts";
import { keystrokeTimes, momentTimes } from "../../timeline/helpers.ts";
import { allCaptions, allCues, FILM, LANGS } from "../../timeline/index.ts";

const EPS = 1e-9;
const D = FILM.durationSeconds;
/** The pencilled paper margin; Captions stay inside it. */
const MARGIN = 36;
const SFX_DIR = path.join(ROOT, "audio/whisper_audio/sfx");

describe("Film format", () => {
  it("is 1920x1080 at 30 fps with a closing fade", () => {
    expect(FILM).toMatchObject({ width: 1920, height: 1080, fps: 30 });
    expect(FILM.fadeOutSeconds).toBeGreaterThan(0);
  });
});

describe("Beats", () => {
  it("are the five spec Beats, in order, with the keys the audio reads", () => {
    expect(FILM.beats.map((b) => b.index)).toEqual([1, 2, 3, 4, 5]);
    expect(FILM.beats.map((b) => b.key)).toEqual(["opening", "speak", "more", "review", "servers"]);
  });

  it("use their windows from bounds.ts", () => {
    for (const b of FILM.beats) expect([b.start, b.end], b.key).toEqual(BEAT_BOUNDS[b.key]);
  });

  it("cover the Film from 0 with no gaps", () => {
    expect(FILM.beats[0].start).toBe(0);
    expect(FILM.beats.at(-1)!.end).toBe(D);
    FILM.beats.forEach((b, i) => {
      expect(b.end, `Beat ${b.index} has positive length`).toBeGreaterThan(b.start);
      if (i > 0) expect(b.start, `Beat ${b.index} starts where the previous ends`).toBe(FILM.beats[i - 1].end);
    });
  });

  it("keep every named moment inside the Beat, in story order", () => {
    for (const b of FILM.beats) {
      const times = momentTimes(b.moments);
      for (const [name, at] of times) {
        expect(at, `Beat ${b.index} ${name}`).toBeGreaterThanOrEqual(b.start);
        expect(at, `Beat ${b.index} ${name}`).toBeLessThan(b.end);
      }
      for (let i = 1; i < times.length; i++) {
        expect(times[i][1], `Beat ${b.index}: ${times[i][0]} after ${times[i - 1][0]}`).toBeGreaterThanOrEqual(times[i - 1][1]);
      }
    }
  });

  it("keep on-screen typing inside the Beat", () => {
    for (const b of FILM.beats) {
      for (const t of b.typing ?? []) {
        expect(t.end, `Beat ${b.index} typing`).toBeGreaterThan(t.start);
        expect(t.start).toBeGreaterThanOrEqual(b.start);
        expect(t.end).toBeLessThan(b.end);
      }
    }
  });
});

describe("Captions", () => {
  const captions = allCaptions();

  it("have unique ids and text in both languages", () => {
    expect(new Set(captions.map((c) => c.id)).size).toBe(captions.length);
    for (const c of captions) for (const lang of LANGS) expect(c.text[lang]?.trim(), `${c.id} ${lang}`).toBeTruthy();
  });

  it("every Beat has at least one Caption", () => {
    for (const b of FILM.beats) expect(b.captions.length, b.key).toBeGreaterThan(0);
  });

  it("each last at least 2.5 s", () => {
    for (const c of captions) expect(c.end - c.start, c.id).toBeGreaterThanOrEqual(2.5 - EPS);
  });

  it("do not overlap", () => {
    const sorted = [...captions].sort((a, b) => a.start - b.start);
    for (let i = 1; i < sorted.length; i++) {
      expect(sorted[i].start, `${sorted[i].id} after ${sorted[i - 1].id}`).toBeGreaterThanOrEqual(sorted[i - 1].end - EPS);
    }
  });

  it("lie inside their own Beat", () => {
    for (const b of FILM.beats) {
      for (const c of b.captions) {
        expect(c.start, c.id).toBeGreaterThanOrEqual(b.start);
        expect(c.end, c.id).toBeLessThanOrEqual(b.end);
      }
    }
  });

  for (const lang of LANGS) {
    it(`fit inside the paper margin in the ${lang} cut`, () => {
      for (const { id, text, placement: p } of captions) {
        const where = `${id} (${lang})`;
        const left = p.align === "center" ? p.x - p.width / 2 : p.align === "right" ? p.x - p.width : p.x;
        expect(left, where).toBeGreaterThanOrEqual(MARGIN);
        expect(left + p.width, where).toBeLessThanOrEqual(FILM.width - MARGIN);
        expect(p.y, where).toBeGreaterThanOrEqual(MARGIN);
        const lines = layoutLines(text[lang], p.fontSize, p.width);
        expect(p.y + lines.length * p.fontSize * CAPTION_LINE_HEIGHT, `${where}: ${lines.length} lines`).toBeLessThanOrEqual(
          FILM.height - MARGIN,
        );
        for (const line of lines) expect(emWidth(line) * p.fontSize, `${where}: "${line}"`).toBeLessThanOrEqual(p.width);
      }
    });
  }
  // font-subset.test.ts checks Captions, with every other handwritten string, against the font subset.
});

describe("Sound cues", () => {
  it("lie inside their Beat, before the closing fade", () => {
    for (const b of FILM.beats) {
      for (const cue of b.cues) {
        const where = `${cue.type}@${cue.at} in Beat ${b.index}`;
        expect(cue.at, where).toBeGreaterThanOrEqual(b.start);
        expect(cue.at, where).toBeLessThan(Math.min(b.end, D - FILM.fadeOutSeconds));
      }
    }
  });

  it("each have a synthesiser module named after the cue type", () => {
    for (const type of new Set(allCues().map((c) => c.type))) {
      expect(type, "cue types are snake_case Python module names").toMatch(/^[a-z][a-z0-9_]*$/);
      expect(fs.existsSync(path.join(SFX_DIR, `${type}.py`)), `audio/whisper_audio/sfx/${type}.py`).toBe(true);
    }
  });

  // Picture and sound share the timeline's times: every cue lands on something
  // the Beat names — a moment, a Caption start or a keystroke.
  it("each sit on a named time of their Beat", () => {
    for (const b of FILM.beats) {
      const named = new Set([
        ...momentTimes(b.moments).map(([, at]) => at),
        ...b.captions.map((c) => c.start),
        ...(b.typing ?? []).flatMap(keystrokeTimes),
      ]);
      for (const cue of b.cues) expect(named.has(cue.at), `Beat ${b.index}: ${cue.type}@${cue.at}`).toBe(true);
    }
  });
});

describe("Review frames", () => {
  it("have unique names and lie inside the Film", () => {
    const frames = reviewFrameTimes();
    expect(new Set(frames.map((f) => f.name)).size).toBe(frames.length);
    for (const f of frames) {
      expect(f.name).toMatch(/^[a-z0-9-]+$/);
      expect(f.at, f.name).toBeGreaterThanOrEqual(0);
      expect(f.at, f.name).toBeLessThan(D);
    }
  });
});

describe("JSON export for the audio synthesiser", () => {
  it("round-trips the whole timeline", () => {
    const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "timeline-")), "timeline.json");
    exportTimeline(file);
    expect(JSON.parse(fs.readFileSync(file, "utf8"))).toEqual(FILM);
  });
});
