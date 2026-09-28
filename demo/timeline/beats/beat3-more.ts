import { BEAT_BOUNDS } from "../bounds.ts";
import { doubleTapCues, doubleTapMoments, LOWER_CAPTION } from "../helpers.ts";
import type { Beat, Caption, Lang, Moments, SoundCue } from "../types.ts";

const [start, end] = BEAT_BOUNDS.more;

/*
 * Beat 3 · More ways to use it. Three paper notes are pinned up in turn, each
 * a drawn app with Whis beside it at the fn key:
 * 1. Snippet: Whis Double taps and says the trigger phrase; Messages gets the
 *    full link, and the red pen circles the phrase and arrows it to the link.
 * 2. Hold mode: Whis holds fn (a blue ring on the pill) and dictates a comment;
 *    on release it lands on the blank line of the code editor, ticked.
 * 3. Upload: three audio files are dropped on Whisper's Upload page and
 *    transcribe one after another, each ticked, until the batch completes.
 * A finished note shrinks to a thumbnail pinned at the top left.
 */
const captions: Caption[] = [
  {
    id: "b3-snippet",
    text: { en: "Say a short phrase, get the full snippet.", "zh-CN": "说出触发词，自动展开完整内容。" },
    start: 31.4,
    end: 35.6,
    placement: LOWER_CAPTION,
  },
  {
    id: "b3-hold",
    text: { en: "Or hold fn to talk, and let go to paste.", "zh-CN": "也可以按住 fn 说话，松开就粘贴。" },
    start: 35.9,
    end: 40.5,
    placement: LOWER_CAPTION,
  },
  {
    id: "b3-upload",
    text: { en: "Drop in audio files to transcribe a batch.", "zh-CN": "拖入音频文件，批量转写。" },
    start: 40.8,
    end: 46.6,
    placement: LOWER_CAPTION,
  },
];

/** Film-only text drawn in this Beat (not Captions). Handwritten in LXGW WenKai. */
export const BEAT3_TEXT = {
  /** Hand-lettered titles of the three pinned notes. */
  snippetTitle: { en: "Snippets", "zh-CN": "片段" },
  holdTitle: { en: "Hold mode", "zh-CN": "按住说话" },
  uploadTitle: { en: "Upload", "zh-CN": "批量上传" },
  /** What Whis says in the Snippet note: the trigger phrase (copy.snippet.trigger). */
  said: { en: "“…cal link”", "zh-CN": "“…我的日程链接”" },
  /** Red-pen notes beside the held fn key. */
  holdNote: { en: "hold fn…", "zh-CN": "按住 fn…" },
  releaseNote: { en: "let go!", "zh-CN": "松开！" },
} as const satisfies Record<string, Record<Lang, string>>;

const tap = doubleTapMoments(32);
/** Hold mode: press and hold fn; the pill opens two frames later, as in the app. */
const HOLD_PRESS = 36.9;

const moments = {
  /** Note 1 (Snippet) is pinned up and the Messages window sketched in. */
  snippetIn: 31.1,
  /** Whis Double taps fn; the pill opens. */
  taps: tap.taps,
  listen: tap.listen,
  /** The spoken trigger phrase is written in Whis's speech bubble. */
  said: 32.5,
  /** Whis stops speaking; the pill thinks. */
  snippetStop: 33.5,
  /** The expanded text lands in the composer; the pill closes. */
  snippetPaste: 33.8,
  /** The red pen circles the trigger phrase... */
  triggerCircle: 34.3,
  /** ...and arrows it to the full link, which is underlined. */
  expandArrow: 34.7,
  /** Note 1 shrinks to its thumbnail. */
  snippetOut: 35.7,
  /** Note 2 (Hold mode) is pinned up with the code editor. */
  holdIn: 36,
  /** fn is pressed and held. */
  holdPress: HOLD_PRESS,
  /** The pill opens with its Hold mode ring. */
  holdListen: Math.round((HOLD_PRESS + 2 / 30) * 1000) / 1000,
  /** "hold fn…" is written by the key. */
  holdNote: 37.2,
  /** fn is released; the pill stops listening. */
  holdRelease: 38.9,
  /** "let go!" is written. */
  releaseNote: 39.05,
  /** The comment lands on the blank line; the pill closes. */
  holdPaste: 39.25,
  /** The red pen ticks the pasted comment. */
  holdTick: 39.8,
  /** Note 2 shrinks to its thumbnail. */
  holdOut: 40.6,
  /** Note 3 (Upload) is pinned up with Whisper's Upload page. */
  uploadIn: 40.9,
  /** Three audio files slide in from the right... */
  filesIn: 41.6,
  /** ...and are dropped on the drop zone; the file list appears. */
  drop: 42.3,
  /** Each file finishes transcribing and is ticked. */
  filesDone: [43.6, 44.3, 45],
  /** The batch is complete: the "3/3 completed" status is circled. */
  uploadComplete: 45.35,
  /** Everything fades out ahead of Beat 4. */
  beatOut: 46.5,
} satisfies Moments;

/** Seconds each file takes to transcribe; they start staggered and finish at filesDone. */
export const FILE_TRANSCRIBE_SECONDS = 1;

const cues: SoundCue[] = [
  ...doubleTapCues(tap),
  { type: "rec_stop", at: moments.snippetStop },
  { type: "paste", at: moments.snippetPaste },
  { type: "pen_scratch", at: moments.triggerCircle, params: { strokes: 3, stroke: 0.1 } },
  { type: "pen_scratch", at: moments.expandArrow, params: { strokes: 3, stroke: 0.08, gain: 0.3 } },
  { type: "key_click", at: moments.holdPress, params: { key: "char" } },
  { type: "rec_start", at: moments.holdListen },
  { type: "pen_scratch", at: moments.holdNote, params: { strokes: 4, stroke: 0.07, gain: 0.3 } },
  { type: "key_click", at: moments.holdRelease, params: { key: "char", gain: 0.26 } },
  { type: "rec_stop", at: moments.holdRelease },
  { type: "pen_scratch", at: moments.releaseNote, params: { strokes: 3, stroke: 0.07, gain: 0.3 } },
  { type: "paste", at: moments.holdPaste },
  { type: "pen_scratch", at: moments.holdTick, params: { strokes: 2, stroke: 0.08 } },
  { type: "file_drop", at: moments.drop, params: { files: 3 } },
  ...moments.filesDone.map((at): SoundCue => ({ type: "pen_scratch", at, params: { strokes: 2, stroke: 0.07, gain: 0.32 } })),
  { type: "pen_scratch", at: moments.uploadComplete, params: { strokes: 3, stroke: 0.1 } },
];

export const beat3 = {
  index: 3,
  key: "more",
  title: "More ways to use it",
  start,
  end,
  captions,
  moments,
  cues,
  reviewFrames: [
    { name: "snippet-done", at: 35.5 },
    { name: "hold-done", at: 40.45 },
    { name: "upload-done", at: 46.3 },
  ],
} satisfies Beat<typeof moments>;
