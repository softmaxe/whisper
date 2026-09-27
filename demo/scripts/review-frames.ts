import { FILM } from "../timeline/index.ts";
import { keystrokeTimes } from "../timeline/helpers.ts";
import type { Film } from "../timeline/types.ts";

/**
 * Review frames extracted from each cut: every Beat midpoint, a moment after
 * each run of on-screen typing, the end of every Caption window (fully
 * written, just before it lifts off), and each Beat's own story frames
 * (`beat.reviewFrames`). Names are unique and file-name safe.
 */
export function reviewFrameTimes(film: Film = FILM): { name: string; at: number }[] {
  return film.beats.flatMap((b) => {
    const prefix = `beat-${b.index}-${b.key}`;
    return [
      { name: `${prefix}-mid`, at: (b.start + b.end) / 2 },
      ...(b.typing ?? []).map((t, i) => ({ name: `${prefix}-typed-${i + 1}`, at: keystrokeTimes(t).at(-1)! + 0.3 })),
      ...b.captions.map((c) => ({ name: `caption-${c.id}`, at: c.end - 0.4 })),
      ...(b.reviewFrames ?? []).map((f) => ({ name: `${prefix}-${f.name}`, at: f.at })),
    ];
  });
}
