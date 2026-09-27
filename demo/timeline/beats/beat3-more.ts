import { BEAT_BOUNDS } from "../bounds.ts";
import { LOWER_CAPTION } from "../helpers.ts";
import type { Beat, Caption, Moments, SoundCue } from "../types.ts";

const [start, end] = BEAT_BOUNDS.more;

/*
 * Beat 3 · More ways to use it (placeholder). Three pinned notes in turn:
 * Snippet expansion, Hold mode in a code editor, batch Upload.
 */
const captions: Caption[] = [
  {
    id: "b3-snippet",
    text: { en: "Say a short phrase, get the full snippet.", "zh-CN": "说出触发词，自动展开完整内容。" },
    start: 31.5,
    end: 35.7,
    placement: LOWER_CAPTION,
  },
  {
    id: "b3-hold",
    text: { en: "Or hold the key to talk, and let go to paste.", "zh-CN": "也可以按住说话，松开就粘贴。" },
    start: 36.1,
    end: 40.6,
    placement: LOWER_CAPTION,
  },
  {
    id: "b3-upload",
    text: { en: "Drop in audio files to transcribe a batch.", "zh-CN": "拖入音频文件，批量转写。" },
    start: 41,
    end: 46.5,
    placement: LOWER_CAPTION,
  },
];

const moments = {
  /** Each pinned note is sketched in: Snippet, Hold mode, Upload. */
  notes: [31.3, 36, 40.9],
} satisfies Moments;

const cues: SoundCue[] = [];

export const beat3 = {
  index: 3,
  key: "more",
  title: "More ways to use it",
  start,
  end,
  captions,
  moments,
  cues,
} satisfies Beat<typeof moments>;
