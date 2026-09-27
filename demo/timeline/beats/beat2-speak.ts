import { BEAT_BOUNDS } from "../bounds.ts";
import { LOWER_CAPTION } from "../helpers.ts";
import type { Beat, Caption, Moments, SoundCue } from "../types.ts";

const [start, end] = BEAT_BOUNDS.speak;

/*
 * Beat 2 · Speak and it's written (placeholder). Mail: fillers crossed out,
 * punctuation added, cleaned text pasted. Team chat: a misheard product name
 * is corrected and the Dictionary notebook records it.
 */
const captions: Caption[] = [
  {
    id: "b2-speak",
    text: { en: "Just speak. Whisper writes it down.", "zh-CN": "开口说，Whisper 帮你写下来。" },
    start: 9.5,
    end: 13.8,
    placement: LOWER_CAPTION,
  },
  {
    id: "b2-clean",
    text: { en: "Fillers go, punctuation comes in.", "zh-CN": "口头禅删掉，标点补上。" },
    start: 14.2,
    end: 18.6,
    placement: LOWER_CAPTION,
  },
  {
    id: "b2-paste",
    text: { en: "The text lands right where you were typing.", "zh-CN": "文字直接出现在你正在输入的地方。" },
    start: 19,
    end: 23.4,
    placement: LOWER_CAPTION,
  },
  {
    id: "b2-learn",
    text: { en: "Fix a word once, and Whisper remembers it.", "zh-CN": "改过一次，Whisper 就记住了。" },
    start: 24,
    end: 30.4,
    placement: LOWER_CAPTION,
  },
];

const moments = {
  /** The Mail window is sketched in. */
  mailIn: 9.3,
  /** Fillers are crossed out and punctuation added. */
  clean: 14.3,
  /** The cleaned text is pasted into Mail. */
  paste: 19.2,
  /** The misheard word is corrected in the team chat. */
  correct: 24.5,
} satisfies Moments;

const cues: SoundCue[] = [];

export const beat2 = {
  index: 2,
  key: "speak",
  title: "Speak and it's written",
  start,
  end,
  captions,
  moments,
  cues,
} satisfies Beat<typeof moments>;
