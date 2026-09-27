/**
 * The Film timeline: the single source of every Caption, moment and sound cue.
 *
 * Each Beat lives in its own file under ./beats and reads its window from
 * ./bounds.ts, so work on different Beats touches different files.
 */
import { beat1 } from "./beats/beat1-opening.ts";
import { beat2 } from "./beats/beat2-speak.ts";
import { beat3 } from "./beats/beat3-more.ts";
import { beat4 } from "./beats/beat4-review.ts";
import { beat5 } from "./beats/beat5-servers.ts";
import { FILM_SECONDS } from "./bounds.ts";
import type { Beat, Caption, Film, Lang, Seconds, SoundCue } from "./types.ts";

export type * from "./types.ts";

/** Both cuts, in build order. */
export const LANGS: readonly Lang[] = ["en", "zh-CN"];

export const FILM: Film = {
  durationSeconds: FILM_SECONDS,
  fps: 30,
  width: 1920,
  height: 1080,
  fadeOutSeconds: 1,
  beats: [beat1, beat2, beat3, beat4, beat5],
};

export const allCaptions = (film: Film = FILM): Caption[] => film.beats.flatMap((b) => b.captions);

export const allCues = (film: Film = FILM): SoundCue[] =>
  film.beats.flatMap((b) => b.cues).sort((a, b) => a.at - b.at);

export const beatByKey = (key: Beat["key"], film: Film = FILM): Beat => {
  const beat = film.beats.find((b) => b.key === key);
  if (!beat) throw new Error(`No Beat with key ${key}`);
  return beat;
};

/** Seconds -> frame index at the Film's frame rate. */
export const toFrame = (t: Seconds, fps: number = FILM.fps): number => Math.round(t * fps);
