/**
 * Small animation helpers shared by the Beat visuals.
 */
import { Easing, interpolate, useCurrentFrame, useVideoConfig } from "remotion";
import type { Beat, Seconds } from "../../timeline/types.ts";

/** `interpolate()` options that hold the end values outside the input range. */
export const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;

/** Clamped 0..1 ramp of `t` over [a, b]. */
export const ramp = (t: number, a: number, b: number): number => interpolate(t, [a, b], [0, 1], clamp);

/** Slow-in, gentle-out easing for paper moving across the desk (Beat 3's notes shrinking to thumbnails). */
export const moveEase = Easing.bezier(0.45, 0, 0.2, 1);

/**
 * Time inside a Beat's visuals, which are mounted in a <Sequence> starting at
 * `beat.start`: the Beat-local `frame` (0 = Beat start), the `fps`, and `t`,
 * the absolute Film time in seconds that timeline moments are written in.
 */
export function useBeatTime(beat: Pick<Beat, "start">): { frame: number; fps: number; t: Seconds } {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  return { frame, fps, t: beat.start + frame / fps };
}
