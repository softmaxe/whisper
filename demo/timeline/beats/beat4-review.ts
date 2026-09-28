import { BEAT_BOUNDS } from "../bounds.ts";
import { LOWER_CAPTION, typingCues } from "../helpers.ts";
import type { Beat, Caption, Moments, SoundCue, Typing } from "../types.ts";

const [start, end] = BEAT_BOUNDS.review;

/*
 * Beat 4 · The day in review. The wash turns to night. The Whisper main
 * window is sketched in on History, today's Dictations listed; Whis types a
 * word into search, the entries that don't match fade back, and the matches are
 * highlighted and underlined in red pen. The window switches to Insights: the
 * usage values are written in by hand one after another, the activity bars
 * are drawn, and today's bar is circled while Whis looks proud.
 */
const captions: Caption[] = [
  {
    id: "b4-history",
    text: { en: "Every dictation is kept, and easy to find.", "zh-CN": "每次听写都会保存，随时可以搜索。" },
    start: 47.5,
    end: 52.3,
    placement: LOWER_CAPTION,
  },
  {
    id: "b4-insights",
    text: { en: "Your usage, at a glance.", "zh-CN": "你的使用情况，一目了然。" },
    start: 52.7,
    end: 58.5,
    placement: LOWER_CAPTION,
  },
];

/**
 * The search query typed into History. The pattern matches the English query
 * ("launch"); the Chinese cut reveals its own query (发布) in step with the
 * same keystrokes.
 */
const typing: Typing[] = [{ keys: "launch", start: 48.5, end: 49.3 }];

const moments = {
  /** The wash starts turning from dusk to night. */
  nightfall: 47,
  /** The Whisper main window (History) and Whis are sketched in. */
  sketchIn: 47.15,
  /** The search field is focused, ready for the query. */
  searchOpen: 48.1,
  /** Entries that don't match fade back. */
  filter: 49.6,
  /** The matches are swept with a highlighter and underlined in red pen. */
  highlight: 49.95,
  /** The window switches to Insights. */
  insights: 52.4,
  /** Each usage value is written in by hand: words, streak, words per minute, Dictations. */
  values: [53.1, 53.6, 54.1, 54.6],
  /** The activity bars are drawn, one after another. */
  bars: 55.2,
  /** Today's bar is circled in red pen; Whis looks proud. */
  today: 57.1,
  /** Everything fades out ahead of Beat 5. */
  beatOut: 58.4,
} satisfies Moments;

/** How long the bars take to draw, from `bars`. */
export const BARS_DRAW_SECONDS = 1.6;

const cues: SoundCue[] = [
  ...typing.flatMap((t) => typingCues(t)),
  { type: "pen_scratch", at: moments.highlight, params: { strokes: 2, stroke: 0.14, gain: 0.35 } },
  ...moments.values.map((at): SoundCue => ({ type: "pen_scratch", at, params: { strokes: 3, stroke: 0.06, pitch: 1.2, gain: 0.3 } })),
  { type: "pen_scratch", at: moments.bars, params: { strokes: 7, stroke: 0.16, pitch: 0.85, gain: 0.3 } },
  { type: "pen_scratch", at: moments.today, params: { strokes: 3, stroke: 0.1 } },
];

export const beat4 = {
  index: 4,
  key: "review",
  title: "The day in review",
  start,
  end,
  captions,
  typing,
  moments,
  cues,
  reviewFrames: [
    { name: "history-highlight", at: 51.8 },
    { name: "insights-chart", at: 58.1 },
  ],
} satisfies Beat<typeof moments>;
