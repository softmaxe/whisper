/**
 * Time-of-day keyframes for the watercolour wash: the paper moves from dawn
 * through day and dusk to night over the working day the Film shows, then
 * clears back to blank paper for the ending. Pure data and maths (no React),
 * so tests can check the progression without rendering.
 *
 * Keyframes follow the Beat windows in timeline/bounds.ts and Beat 4's
 * `nightfall` moment, so a retimed Beat moves the wash with it.
 */
import { beat4 } from "../../../timeline/beats/beat4-review.ts";
import { BEAT_BOUNDS } from "../../../timeline/bounds.ts";
import { FILM } from "../../../timeline/index.ts";
import type { Seconds } from "../../../timeline/types.ts";
import { PALETTE } from "../theme.ts";

export interface WashKey {
  /** Film seconds. */
  at: Seconds;
  /** Wash colour (multiplied onto the paper). */
  color: string;
  /** Overall strength, 0 = blank paper. */
  opacity: number;
}

const { opening, speak, more, servers } = BEAT_BOUNDS;
const mid = ([a, b]: readonly [number, number]) => (a + b) / 2;

/**
 * Night stops at a strength where ink Captions, graphite pencil, Whis (with
 * its paper rim) and the red pen all stay readable on the washed paper.
 */
export const NIGHT_OPACITY = 0.44;

/**
 * Peak strength of the layers stacked on the base blotches (alpha up to 1):
 * the heavier sky along the top edge and the pigment pooled at the edges.
 */
export const WASH_LAYERS = { skyTop: 0.55, poolEdge: 0.45, grain: 0.35 } as const;

/** When the wash has fully turned to night in Beat 4. */
export const NIGHT_FULL: Seconds = beat4.moments.nightfall + 2.2;

/**
 * When the paper is blank again: at the start of the Film's final fade, so its
 * whole last second is blank paper. The night clears over the 3 s before.
 */
export const CLEAR_END: Seconds = FILM.durationSeconds - FILM.fadeOutSeconds;
export const CLEAR_START: Seconds = CLEAR_END - 3;

export const WASH_KEYS: readonly WashKey[] = [
  { at: opening[0], color: PALETTE.washDawn, opacity: 0.4 },
  { at: mid(opening), color: PALETTE.washDawn, opacity: 0.34 },
  { at: speak[0] + 3, color: PALETTE.washDay, opacity: 0.26 },
  // Midday is the palest; the afternoon warms towards dusk.
  { at: mid(speak), color: PALETTE.washDay, opacity: 0.18 },
  { at: mid(more), color: PALETTE.washDay, opacity: 0.28 },
  { at: more[1] - 1, color: PALETTE.washDusk, opacity: 0.34 },
  { at: beat4.moments.nightfall, color: PALETTE.washDusk, opacity: 0.38 },
  { at: NIGHT_FULL, color: PALETTE.washNight, opacity: NIGHT_OPACITY },
  { at: servers[0], color: PALETTE.washNight, opacity: NIGHT_OPACITY },
  { at: CLEAR_START, color: PALETTE.washNight, opacity: NIGHT_OPACITY * 0.9 },
  { at: CLEAR_END, color: PALETTE.washNight, opacity: 0 },
];

/** Smoothstep easing between keyframes, so the light turns gently. */
const ease = (x: number) => x * x * (3 - 2 * x);

function hexToRgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function rgbToHex([r, g, b]: [number, number, number]): string {
  return `#${[r, g, b].map((c) => Math.round(c).toString(16).padStart(2, "0")).join("")}`;
}

/** The wash colour and strength at Film time `t`. */
export function washAt(t: Seconds, keys: readonly WashKey[] = WASH_KEYS): { color: string; opacity: number } {
  if (t <= keys[0].at) return { color: keys[0].color, opacity: keys[0].opacity };
  const last = keys[keys.length - 1];
  if (t >= last.at) return { color: last.color, opacity: last.opacity };
  const i = keys.findIndex((k) => k.at > t);
  const a = keys[i - 1];
  const b = keys[i];
  const x = ease((t - a.at) / (b.at - a.at));
  const ca = hexToRgb(a.color);
  const cb = hexToRgb(b.color);
  return {
    color: rgbToHex([0, 1, 2].map((j) => ca[j] + (cb[j] - ca[j]) * x) as [number, number, number]),
    opacity: a.opacity + (b.opacity - a.opacity) * x,
  };
}
