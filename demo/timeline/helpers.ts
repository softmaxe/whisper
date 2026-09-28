import { FILM_FPS } from "./bounds.ts";
import type { CaptionPlacement, Moments, Seconds, SoundCue, Typing } from "./types.ts";

/** Default placement for a narration Caption in the lower part of the paper. */
export const LOWER_CAPTION: CaptionPlacement = {
  x: 960,
  y: 850,
  width: 1560,
  align: "center",
  fontSize: 64,
};

/** `x` rounded to whole milliseconds, so timeline times print and compare cleanly. */
export const roundMs = (x: Seconds): Seconds => Math.round(x * 1000) / 1000;

/** How long after the key press the app opens the Recording pill: two frames. */
export const PILL_OPEN_DELAY: Seconds = 2 / FILM_FPS;

/** Deterministic pseudo-random value in [0, 1) for index `i` (no Math.random: frames render in parallel). */
export function hash01(i: number): number {
  const x = Math.sin(i * 12.9898 + 78.233) * 43758.5453;
  return x - Math.floor(x);
}

/**
 * The time of each keystroke of `typing.keys`, one per character (spaces
 * included), from `typing.start` to `typing.end` inclusive. Gaps are slightly
 * irregular and a little longer around spaces, like real typing. Rounded to ms.
 */
export function keystrokeTimes(typing: Typing): Seconds[] {
  const chars = [...typing.keys];
  if (chars.length === 1) return [typing.start];
  const gaps = chars.slice(1).map((ch, i) => {
    const wordBreak = ch === " " || chars[i] === " " ? 1.5 : 1;
    return wordBreak * (0.7 + 0.6 * hash01(i));
  });
  const total = gaps.reduce((a, b) => a + b, 0);
  const span = typing.end - typing.start;
  let t = typing.start;
  const times = [t];
  for (const g of gaps) {
    t += (g / total) * span;
    times.push(t);
  }
  return times.map(roundMs);
}

/** One `key_click` cue per keystroke; spaces get the deeper space-bar sound. */
export function typingCues(typing: Typing, params: SoundCue["params"] = {}): SoundCue[] {
  const chars = [...typing.keys];
  return keystrokeTimes(typing).map((at, i) => ({
    type: "key_click",
    at,
    params: { ...params, key: chars[i] === " " ? "space" : "char" },
  }));
}

/** Seconds between the two presses of a Double tap. */
export const DOUBLE_TAP_GAP: Seconds = 0.25;

/**
 * The moments of a Double tap on the fn/Globe key whose first press lands at
 * `first`: both presses, and `listen`, when the Recording pill opens (two
 * frames after the second press, as in the app). Spread into a Beat's
 * moments, e.g. `const tap = doubleTapMoments(5.9)` then
 * `moments = { ..., taps: tap.taps, listen: tap.listen }`. Rounded to ms.
 */
export function doubleTapMoments(
  first: Seconds,
  gap: Seconds = DOUBLE_TAP_GAP,
): { taps: readonly [Seconds, Seconds]; listen: Seconds } {
  const second = roundMs(first + gap);
  return { taps: [roundMs(first), second], listen: roundMs(second + PILL_OPEN_DELAY) };
}

/**
 * The sounds of a Double tap that starts a Dictation: a key click per press
 * and the app's recording-start chime when the pill opens.
 */
export function doubleTapCues(tap: { taps: readonly Seconds[]; listen: Seconds }): SoundCue[] {
  return [
    ...tap.taps.map((at): SoundCue => ({ type: "key_click", at, params: { key: "char" } })),
    { type: "rec_start", at: tap.listen },
  ];
}

/** Every moment as `[name, seconds]` in declaration order, with series flattened to `name[i]`. */
export function momentTimes(moments: Moments): [name: string, at: Seconds][] {
  return Object.entries(moments).flatMap(([name, at]): [string, Seconds][] =>
    typeof at === "number" ? [[name, at]] : at.map((t, i): [string, Seconds] => [`${name}[${i}]`, t]),
  );
}
