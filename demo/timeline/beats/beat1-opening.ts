import { BEAT_BOUNDS } from "../bounds.ts";
import { LOWER_CAPTION, typingCues } from "../helpers.ts";
import type { Beat, Caption, Moments, SoundCue, Typing } from "../types.ts";

const [start, end] = BEAT_BOUNDS.opening;

/*
 * Beat 1 · Opening (placeholder). Typing is slow; the fn/Globe key is circled;
 * a Double tap brings up the Recording pill.
 */
const captions: Caption[] = [
  {
    id: "b1-slow",
    text: { en: "Typing feels slow?", "zh-CN": "打字太慢？" },
    start: 0.5,
    end: 4.2,
    placement: { ...LOWER_CAPTION, fontSize: 72 },
  },
  {
    id: "b1-fn",
    text: { en: "Double tap the fn key and just speak.", "zh-CN": "双击 fn 键，直接开口说。" },
    start: 4.5,
    end: 8.6,
    placement: LOWER_CAPTION,
  },
];

/** Slow hunt-and-peck typing on the keyboard. */
const typing: Typing[] = [{ keys: "hi maya, just", start: 0.8, end: 3.8 }];

const moments = {
  /** The fn/Globe key is circled in red pen. */
  fnCircle: 4.6,
  /** The two taps of the Double tap. */
  taps: [5.6, 5.85],
  /** The Recording pill appears. */
  pill: 6.0,
} satisfies Moments;

const cues: SoundCue[] = [
  ...typing.flatMap((t) => typingCues(t)),
  ...moments.taps.map((at) => ({ type: "key_click", at, params: { key: "char" } })),
];

export const beat1 = {
  index: 1,
  key: "opening",
  title: "Opening",
  start,
  end,
  captions,
  typing,
  moments,
  cues,
  reviewFrames: [{ name: "fn-circled", at: moments.fnCircle + 0.8 }],
} satisfies Beat<typeof moments>;
