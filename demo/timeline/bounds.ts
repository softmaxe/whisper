import type { BeatKey, Seconds } from "./types.ts";

/** Length of the whole Film. */
export const FILM_SECONDS: Seconds = 75;

/** The Film's frame rate (FILM.fps). */
export const FILM_FPS = 30;

/**
 * Each Beat's [start, end) window. Beat modules read their own window from
 * here, so work on one Beat never edits a neighbour's file. Changing a window
 * is a deliberate, coordinated edit: the Beats on both sides move with it.
 */
export const BEAT_BOUNDS = {
  opening: [0, 9],
  speak: [9, 31],
  more: [31, 47],
  review: [47, 59],
  servers: [59, FILM_SECONDS],
} as const satisfies Record<BeatKey, readonly [Seconds, Seconds]>;
