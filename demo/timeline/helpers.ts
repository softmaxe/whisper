import type { CaptionPlacement, Moments, Seconds, SoundCue, Typing } from "./types.ts";

/** Default placement for a narration Caption in the lower part of the paper. */
export const LOWER_CAPTION: CaptionPlacement = {
  x: 960,
  y: 850,
  width: 1560,
  align: "center",
  fontSize: 64,
};

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
  return times.map((x) => Math.round(x * 1000) / 1000);
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

/** Every moment as `[name, seconds]` in declaration order, with series flattened to `name[i]`. */
export function momentTimes(moments: Moments): [name: string, at: Seconds][] {
  return Object.entries(moments).flatMap(([name, at]): [string, Seconds][] =>
    typeof at === "number" ? [[name, at]] : at.map((t, i): [string, Seconds] => [`${name}[${i}]`, t]),
  );
}
