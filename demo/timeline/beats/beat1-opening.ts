import { BEAT_BOUNDS } from "../bounds.ts";
import { doubleTapCues, doubleTapMoments, LOWER_CAPTION, typingCues } from "../helpers.ts";
import type { Beat, Caption, Lang, Moments, SoundCue, Typing } from "../types.ts";

const [start, end] = BEAT_BOUNDS.opening;

/*
 * Beat 1 · Opening. Whis pecks out the start of a Mail reply, key by key, and
 * gives up with a scratch of the head: typing is slow. The fn/Globe key is
 * circled in red pen and labelled; Whis Double taps it and the Recording pill
 * opens, its bars following Whis's voice.
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

/** Slow hunt-and-peck typing on the keyboard: the reply's greeting and a false start. */
const typing: Typing[] = [{ keys: "Hi Maya, Fri", start: 1.1, end: 3.7 }];

/** Film-only text drawn in this Beat (not Captions). */
export const BEAT1_TEXT = {
  /** What the keystrokes put in the Mail draft, revealed in step with them. */
  typed: { en: "Hi Maya, Fri", "zh-CN": "陈雅你好，周五" },
  /** The red-pen label beside the circled key. */
  fnNote: { en: "the fn / Globe key", "zh-CN": "fn / 地球仪键" },
} as const satisfies Record<string, Record<Lang, string>>;

const tap = doubleTapMoments(6.1);

const moments = {
  /** The Mail window, keyboard and Whis start being sketched in. */
  sketchIn: 0.05,
  /** Whis stops typing and scratches its head. */
  giveUp: 3.9,
  /** The fn/Globe key is circled in red pen. */
  fnCircle: 4.6,
  /** The label is written beside it, with an arrow to the circle. */
  fnNote: 4.95,
  /** The two presses of the Double tap. */
  taps: tap.taps,
  /** The Recording pill opens and starts listening; Whis speaks. */
  listen: tap.listen,
  /** Everything fades out ahead of Beat 2. */
  beatOut: 8.6,
} satisfies Moments;

const cues: SoundCue[] = [
  ...typing.flatMap((t) => typingCues(t)),
  { type: "pen_scratch", at: moments.fnCircle, params: { strokes: 3, stroke: 0.1 } },
  { type: "pen_scratch", at: moments.fnNote, params: { strokes: 4, stroke: 0.07, gain: 0.3 } },
  ...doubleTapCues(tap),
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
  reviewFrames: [
    { name: "fn-circled", at: 5.95 },
    { name: "recording-pill", at: 7.9 },
  ],
} satisfies Beat<typeof moments>;
