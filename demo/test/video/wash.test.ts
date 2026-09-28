/**
 * The time-of-day wash: it moves from dawn to night across the five Beats,
 * clears to blank paper for the ending, and never gets so dark that ink, the
 * red pen or Clawd stop reading on the washed paper.
 */
import { describe, expect, it } from "vitest";
import { CLEAR_END, NIGHT_FULL, NIGHT_OPACITY, WASH_KEYS, WASH_LAYERS, washAt } from "../../src/film/components/washKeys.ts";
import { PALETTE } from "../../src/film/theme.ts";
import { beat4 } from "../../timeline/beats/beat4-review.ts";
import { FILM } from "../../timeline/index.ts";

const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
const hex = (c: number[]) => `#${c.map((x) => Math.round(x * 255).toString(16).padStart(2, "0")).join("")}`;

/**
 * The paper under the wash: multiply layers of `color` at each alpha in
 * `layers`, the whole group at `opacity` (as Wash.tsx stacks them).
 */
const washed = (color: string, opacity: number, layers: number[]) =>
  hex(
    rgb(PALETTE.paper).map((p, i) => {
      const c = rgb(color)[i];
      const group = layers.reduce((m, a) => m * (1 - a * (1 - c)), 1);
      return p * (1 - opacity * (1 - group));
    }),
  );

function luminance(color: string): number {
  const lin = rgb(color).map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}
const contrast = (a: string, b: string) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

describe("Wash", () => {
  it("has keyframes in time order inside the Film", () => {
    const times = WASH_KEYS.map((k) => k.at);
    expect([...times].sort((a, b) => a - b)).toEqual(times);
    expect(times[0]).toBe(0);
    expect(times.at(-1)).toBeLessThanOrEqual(FILM.durationSeconds);
  });

  it("changes across all five Beats", () => {
    const mids = FILM.beats.map((b) => washAt((b.start + b.end) / 2));
    for (let i = 1; i < mids.length; i++) expect(mids[i], `Beat ${i + 1}`).not.toEqual(mids[i - 1]);
    // Each Beat also moves within itself: the light is never frozen for a whole Beat.
    for (const b of FILM.beats) expect(washAt(b.start + 0.01), b.key).not.toEqual(washAt(b.end - 0.01));
  });

  it("goes from dawn in the Opening to night in Beat 4", () => {
    expect(washAt(0).color).toBe(PALETTE.washDawn);
    expect(washAt(beat4.moments.nightfall).color).not.toBe(PALETTE.washNight);
    expect(washAt(NIGHT_FULL)).toEqual({ color: PALETTE.washNight, opacity: NIGHT_OPACITY });
    expect(NIGHT_FULL).toBeLessThan(beat4.moments.highlight);
  });

  it("clears to blank paper before the Film ends", () => {
    expect(CLEAR_END).toBeLessThanOrEqual(FILM.durationSeconds);
    expect(washAt(FILM.durationSeconds - 0.01 / FILM.fps).opacity).toBe(0);
    // The whole last second, where the Film fades out, is blank paper.
    expect(washAt(FILM.durationSeconds - FILM.fadeOutSeconds).opacity).toBe(0);
  });

  it("keeps ink and the red pen readable on the darkest washed paper", () => {
    const strongest = Math.max(...WASH_KEYS.map((k) => k.opacity));
    expect(strongest).toBe(NIGHT_OPACITY);
    // Worst case: a full blotch under the heaviest sky, pooled edge and grain (the top corners).
    const { skyTop, poolEdge, grain } = WASH_LAYERS;
    const corner = washed(PALETTE.washNight, NIGHT_OPACITY, [1, skyTop, poolEdge, grain]);
    expect(contrast(PALETTE.ink, corner)).toBeGreaterThanOrEqual(3.5);
    // Where the Beats draw: a full blotch with the fading sky and a little grain.
    const body = washed(PALETTE.washNight, NIGHT_OPACITY, [1, skyTop * 0.2, grain * 0.3]);
    expect(contrast(PALETTE.ink, body)).toBeGreaterThanOrEqual(4.5);
    // Luminance contrast understates the orange-on-blue hue contrast of the red pen, so the bar is lower.
    expect(contrast(PALETTE.redPen, body)).toBeGreaterThanOrEqual(1.5);
  });
});
