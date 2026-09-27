import path from "node:path";
import { fileURLToPath } from "node:url";
import type { Lang } from "../timeline/types.ts";

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const OUT_DIR = path.join(ROOT, "out");

/** Build inputs and outputs shared by both cuts. */
export const PATHS = {
  timelineJson: path.join(OUT_DIR, "timeline.json"),
  audioWav: path.join(OUT_DIR, "audio.wav"),
  framesRoot: path.join(OUT_DIR, "frames"),
  videoEntry: path.join(ROOT, "src", "index.ts"),
  publicDir: path.join(ROOT, "public"),
  audioProject: path.join(ROOT, "audio"),
};

/** The Remotion composition id of each cut. */
export const FILM_COMPOSITION_IDS: Record<Lang, string> = { en: "Film-en", "zh-CN": "Film-zh" };

/** Per-cut outputs: the silent render, the muxed Film and its review frames. */
export function cutPaths(lang: Lang): { videoOnly: string; film: string; framesDir: string } {
  return {
    videoOnly: path.join(OUT_DIR, `film-video-only-${lang}.mp4`),
    film: path.join(OUT_DIR, `whisper-film-${lang}.mp4`),
    framesDir: path.join(PATHS.framesRoot, lang),
  };
}
