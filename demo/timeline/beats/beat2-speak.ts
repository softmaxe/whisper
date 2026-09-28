import { BEAT_BOUNDS } from "../bounds.ts";
import { doubleTapCues, doubleTapMoments, LOWER_CAPTION } from "../helpers.ts";
import type { Beat, Caption, Lang, Moments, SoundCue } from "../types.ts";

const [start, end] = BEAT_BOUNDS.speak;

/*
 * Beat 2 · Speak and it's written. Mail: Whis's words (still being dictated
 * from Beat 1) appear on a "You said" card with their fillers; the pill stops
 * listening, the red pen strikes the fillers and adds punctuation, and the
 * cleaned text lands in the Mail reply by Automatic paste. Team chat: Whis
 * dictates a reply whose product name is misheard; the red pen circles it and
 * writes Supabase, and the Dictionary notebook records it with a tick.
 */
const captions: Caption[] = [
  {
    id: "b2-speak",
    text: { en: "Just speak. Whisper writes it down.", "zh-CN": "开口说，Whisper 帮你写下来。" },
    start: 9.2,
    end: 12.4,
    placement: LOWER_CAPTION,
  },
  {
    id: "b2-clean",
    text: { en: "Fillers go, punctuation comes in.", "zh-CN": "口头禅删掉，标点补上。" },
    start: 12.7,
    end: 15.7,
    placement: LOWER_CAPTION,
  },
  {
    id: "b2-paste",
    text: { en: "The text lands right where you were typing.", "zh-CN": "文字直接出现在你正在输入的地方。" },
    start: 15.9,
    end: 18.9,
    placement: LOWER_CAPTION,
  },
  {
    id: "b2-any-app",
    text: { en: "It works in any app, like your team chat.", "zh-CN": "在任何应用里都能用，比如团队聊天。" },
    start: 19.6,
    end: 22.5,
    placement: LOWER_CAPTION,
  },
  {
    id: "b2-misheard",
    text: { en: "Misheard a product name?", "zh-CN": "产品名听错了？" },
    start: 22.8,
    end: 25.5,
    placement: LOWER_CAPTION,
  },
  {
    id: "b2-learn",
    text: { en: "Fix a word once, and Whisper remembers it.", "zh-CN": "改过一次，Whisper 就记住了。" },
    start: 25.8,
    end: 30.4,
    placement: LOWER_CAPTION,
  },
];

/** Film-only text drawn in this Beat (not Captions); all handwritten in LXGW WenKai. */
export const BEAT2_TEXT = {
  /** The red-pen note beside the pasted Mail text. */
  pasted: { en: "pasted for you", "zh-CN": "自动粘贴好了" },
} as const satisfies Record<string, Record<Lang, string>>;

/** The Double tap that starts the team-chat Dictation. */
const chatTap = doubleTapMoments(20.6);

const moments = {
  /** The Mail window, the "You said" card, the pill and Whis fade in, still dictating. */
  sketchIn: 9,
  /** The spoken words start appearing on the card, fillers and all. */
  wordsIn: 9.2,
  /** The last spoken word appears. */
  wordsEnd: 12.1,
  /** Whis stops speaking; the pill turns to its thinking wave. */
  stop: 12.4,
  /** The red pen strikes out each filler (the copy module has three per cut). */
  strikes: [12.8, 13.25, 13.7],
  /** The red pen adds each punctuation mark with a proofreader's caret. */
  punctuation: [14.2, 14.6, 15],
  /** The pill closes and the cleaned text lands in the Mail reply by Automatic paste. */
  paste: 15.9,
  /** A red-pen note and arrow point out that it was pasted for you. */
  pasteNote: 16.5,
  /** The Mail scene fades out. */
  mailOut: 18.9,
  /** The team chat window is sketched in, with a question waiting. */
  chatIn: 19.4,
  /** Whis Double taps fn to answer. */
  chatTaps: chatTap.taps,
  /** The pill opens and Whis dictates the reply. */
  chatListen: chatTap.listen,
  /** Whis stops speaking. */
  chatStop: 22.7,
  /** The reply lands in the message box by Automatic paste, with a misheard product name. */
  chatPaste: 23.1,
  /** The red pen circles the misheard word. */
  circle: 23.9,
  /** ...and writes the right one above it. */
  fix: 24.5,
  /** The Dictionary notebook is sketched in beside the chat. */
  notebookIn: 25.3,
  /** The correction is written into the notebook. */
  entry: 25.9,
  /** A red-pen tick: learned. The app's toast confirms it. */
  tick: 26.9,
  /** Everything fades out ahead of Beat 3. */
  beatOut: 30.4,
} satisfies Moments;

const cues: SoundCue[] = [
  { type: "rec_stop", at: moments.stop },
  ...moments.strikes.map((at): SoundCue => ({ type: "pen_scratch", at, params: { strokes: 2, stroke: 0.1 } })),
  ...moments.punctuation.map((at): SoundCue => ({ type: "pen_scratch", at, params: { strokes: 2, stroke: 0.06, pitch: 1.2, gain: 0.32 } })),
  { type: "paste", at: moments.paste },
  { type: "pen_scratch", at: moments.pasteNote, params: { strokes: 4, stroke: 0.07, gain: 0.3 } },
  ...doubleTapCues({ taps: moments.chatTaps, listen: moments.chatListen }),
  { type: "rec_stop", at: moments.chatStop },
  { type: "paste", at: moments.chatPaste },
  { type: "pen_scratch", at: moments.circle, params: { strokes: 3, stroke: 0.1 } },
  { type: "pen_scratch", at: moments.fix, params: { strokes: 4, stroke: 0.08, gain: 0.32 } },
  { type: "pen_scratch", at: moments.entry, params: { strokes: 4, stroke: 0.08, gain: 0.32 } },
  { type: "tick", at: moments.tick },
];

export const beat2 = {
  index: 2,
  key: "speak",
  title: "Speak and it's written",
  start,
  end,
  captions,
  moments,
  cues,
  reviewFrames: [
    { name: "mail-cleaned", at: 18.3 },
    { name: "dictionary-entry", at: 29.5 },
  ],
} satisfies Beat<typeof moments>;
