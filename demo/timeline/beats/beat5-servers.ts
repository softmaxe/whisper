import { BEAT_BOUNDS } from "../bounds.ts";
import { LOWER_CAPTION } from "../helpers.ts";
import type { Beat, Caption, Moments, SoundCue } from "../types.ts";

const [start, end] = BEAT_BOUNDS.servers;

/*
 * Beat 5 · Your servers (placeholder). A house outline holds the Mac and the
 * servers; the logo is drawn, then the install command and repository link;
 * the Film's own closing fade returns to blank paper.
 */
const captions: Caption[] = [
  {
    id: "b5-servers",
    text: { en: "It all runs on your own servers.", "zh-CN": "一切都在你自己的服务器上运行。" },
    start: 59.5,
    end: 64.8,
    placement: LOWER_CAPTION,
  },
  {
    id: "b5-home",
    text: { en: "Your voice never leaves home.", "zh-CN": "你的声音，从不离开家。" },
    start: 65.2,
    end: 69.2,
    placement: LOWER_CAPTION,
  },
  {
    id: "b5-install",
    text: { en: "Install it with Homebrew.", "zh-CN": "用 Homebrew 安装。" },
    start: 69.6,
    end: 73.8,
    placement: LOWER_CAPTION,
    variant: "title",
  },
];

const moments = {
  /** The house outline is drawn around the Mac and the servers. */
  house: 59.3,
  /** The logo is drawn. */
  logo: 69.4,
} satisfies Moments;

const cues: SoundCue[] = [];

export const beat5 = {
  index: 5,
  key: "servers",
  title: "Your servers",
  start,
  end,
  captions,
  moments,
  cues,
} satisfies Beat<typeof moments>;
