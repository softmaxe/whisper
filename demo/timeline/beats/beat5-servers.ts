import { BEAT_BOUNDS } from "../bounds.ts";
import { LOWER_CAPTION } from "../helpers.ts";
import type { Beat, Caption, Lang, Moments, SoundCue } from "../types.ts";

const [start, end] = BEAT_BOUNDS.servers;

/*
 * Beat 5 · Your servers. A house outline is drawn; inside it the Mac and the
 * speech-to-text and cleanup servers, each labelled with its local endpoint.
 * Data arrows join them and data circulates along them without ever leaving
 * the house, while a Dictation runs on the Mac. The house lifts off; the
 * Whisper logo is drawn (its waveform W in one stroke), then the `brew
 * install` command and the repository link are written and Whis waves. The
 * Beat fades out before the Film's last second, which is blank paper.
 */
const captions: Caption[] = [
  {
    id: "b5-servers",
    text: { en: "It all runs on your own servers.", "zh-CN": "一切都在你自己的服务器上运行。" },
    start: 59.5,
    end: 63.4,
    placement: LOWER_CAPTION,
  },
  {
    id: "b5-home",
    text: { en: "Your voice never leaves home.", "zh-CN": "你的声音，从不离开家。" },
    start: 63.8,
    end: 68.4,
    placement: LOWER_CAPTION,
  },
  {
    id: "b5-install",
    text: { en: "Install it with Homebrew.", "zh-CN": "用 Homebrew 安装。" },
    start: 69.2,
    end: 73.4,
    placement: LOWER_CAPTION,
    variant: "title",
  },
];

/** Film-only text drawn in this Beat (not Captions). */
export const BEAT5_TEXT = {
  /** Pencilled under the Mac. */
  mac: { en: "your Mac", "zh-CN": "你的 Mac" },
  /** The red-pen note written in the roof once the data is flowing. */
  home: { en: "never leaves the house", "zh-CN": "数据不出家门" },
} as const satisfies Record<string, Record<Lang, string>>;

const moments = {
  /** The house outline starts being drawn. */
  sketchIn: 59.1,
  /** The Mac is sketched inside the house. */
  mac: 59.6,
  /** The speech-to-text server, then the cleanup server. */
  servers: [60.0, 60.35],
  /** Each server's name and local endpoint is written under it. */
  labels: [60.9, 61.5],
  /** Data arrows: Mac → speech-to-text → cleanup → Mac. */
  arrows: [62.1, 62.6, 63.1],
  /** A Dictation starts on the Mac: the pill listens and data starts to circulate. */
  listen: 63.6,
  /** The Dictation stops; the pill thinks, then closes. */
  stop: 65.0,
  /** The red-pen note in the roof: the data never leaves the house. Whis is proud. */
  home: 65.7,
  /** The house scene lifts off the paper. */
  houseOut: 68.0,
  /** The logo's black rounded square is drawn. */
  logo: 68.9,
  /** The waveform W is drawn in one continuous white stroke. */
  logoStroke: 69.3,
  /** Whis waves goodbye. */
  wave: 70.6,
  /** The `brew install` command is written. */
  install: 70.8,
  /** The repository link is written in red pen. */
  link: 71.9,
  /** Everything fades out, leaving blank paper well before the Film's last second. */
  beatOut: 73.0,
} satisfies Moments;

/** Seconds the house takes to lift off, the logo stroke takes, and the Beat's own fade-out takes. */
export const BEAT5_DURATIONS = { houseOut: 0.7, logoStroke: 1.3, beatOut: 0.8 } as const;

const cues: SoundCue[] = [
  { type: "pen_scratch", at: moments.sketchIn, params: { strokes: 5, stroke: 0.16, pitch: 0.8 } },
  { type: "pen_scratch", at: moments.labels[0], params: { strokes: 4, stroke: 0.07 } },
  { type: "pen_scratch", at: moments.labels[1], params: { strokes: 4, stroke: 0.07 } },
  ...moments.arrows.map((at): SoundCue => ({ type: "pen_scratch", at, params: { strokes: 2, stroke: 0.12 } })),
  { type: "rec_start", at: moments.listen },
  { type: "rec_stop", at: moments.stop },
  { type: "pen_scratch", at: moments.home, params: { strokes: 5, stroke: 0.08 } },
  { type: "pen_scratch", at: moments.logoStroke, params: { strokes: 6, stroke: 0.19, pitch: 1.2 } },
  { type: "pen_scratch", at: moments.install, params: { strokes: 6, stroke: 0.1, pitch: 0.9 } },
  { type: "pen_scratch", at: moments.link, params: { strokes: 5, stroke: 0.09 } },
];

export const beat5 = {
  index: 5,
  key: "servers",
  title: "Your servers",
  start,
  end,
  captions,
  moments,
  cues,
  reviewFrames: [
    { name: "house", at: 67.6 },
    { name: "logo-install", at: 72.8 },
    { name: "blank-paper", at: 74.5 },
  ],
} satisfies Beat<typeof moments>;
