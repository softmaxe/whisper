import { BEAT_BOUNDS } from "../bounds.ts";
import { LOWER_CAPTION } from "../helpers.ts";
import type { Beat, Caption, Moments, SoundCue } from "../types.ts";

const [start, end] = BEAT_BOUNDS.review;

/*
 * Beat 4 · The day in review (placeholder). The wash turns to night; History
 * search highlights matches; Insights values and activity bars are drawn.
 */
const captions: Caption[] = [
  {
    id: "b4-history",
    text: { en: "Every dictation is kept, and easy to find.", "zh-CN": "每次听写都会保存，随时可以搜索。" },
    start: 47.5,
    end: 52.6,
    placement: LOWER_CAPTION,
  },
  {
    id: "b4-insights",
    text: { en: "Your usage, at a glance.", "zh-CN": "你的使用情况，一目了然。" },
    start: 53,
    end: 58.5,
    placement: LOWER_CAPTION,
  },
];

const moments = {
  /** History is searched. */
  search: 48,
  /** Insights bars are drawn. */
  insights: 53.2,
} satisfies Moments;

const cues: SoundCue[] = [];

export const beat4 = {
  index: 4,
  key: "review",
  title: "The day in review",
  start,
  end,
  captions,
  moments,
  cues,
} satisfies Beat<typeof moments>;
