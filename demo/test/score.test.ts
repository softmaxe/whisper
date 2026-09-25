import { describe, expect, it } from "vitest";
import { INSTRUMENT_RANGES, SPARKLE_CUES, composeScore } from "../src/music/score.ts";
import { TOTAL_BEATS, cueBeat, sceneStartBeat } from "../src/timeline.ts";

const D_FLAT_MAJOR = new Set([1, 3, 5, 6, 8, 10, 0]);
const score = composeScore();

describe("score", () => {
  it("stays inside the video", () => {
    for (const note of score) {
      expect(note.beat).toBeGreaterThanOrEqual(0);
      expect(note.beat).toBeLessThan(TOTAL_BEATS);
      expect(note.beats).toBeGreaterThan(0);
    }
  });

  it("stays in D-flat major", () => {
    const outside = score.filter((note) => !D_FLAT_MAJOR.has(note.midi % 12));
    expect(outside).toEqual([]);
  });

  it("keeps each instrument within its sampled range", () => {
    for (const note of score) {
      const [low, high] = INSTRUMENT_RANGES[note.instrument];
      expect(note.midi).toBeGreaterThanOrEqual(low);
      expect(note.midi).toBeLessThanOrEqual(high);
    }
  });

  it("holds the bowed strings back until the working day starts", () => {
    const bowed = new Set(["cello", "viola", "violins", "pizz"]);
    const early = score.filter(
      (note) => bowed.has(note.instrument) && note.beat < sceneStartBeat("chat")
    );
    expect(early).toEqual([]);
  });

  it("rings a sparkle on every paste and learned word", () => {
    for (const [scene, cue] of SPARKLE_CUES) {
      const beat = cueBeat(scene, cue as never);
      expect(score.some((note) => note.instrument === "glock" && note.beat === beat)).toBe(true);
    }
  });

  it("resolves to the tonic at the end", () => {
    const lastBass = score
      .filter((note) => note.instrument === "cello")
      .sort((a, b) => b.beat - a.beat)[0];
    expect(lastBass.midi % 12).toBe(1);
  });
});
