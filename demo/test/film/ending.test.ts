/**
 * The Film ends on blank paper: throughout its last second the picture
 * already matches the final frame, where every Beat, Caption and closing fade
 * has finished and only the paper (under the time-of-day wash) is left.
 */
import { execFileSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import { cutPaths } from "../../scripts/paths.ts";
import { FILM, LANGS } from "../../timeline/index.ts";

const W = 192;
const H = 108;

/** One frame at `at` seconds, shrunk to W×H greyscale bytes. */
function frameAt(file: string, at: number): Buffer {
  return execFileSync(
    "ffmpeg",
    ["-v", "error", "-ss", at.toFixed(3), "-i", file, "-frames:v", "1", "-vf", `scale=${W}:${H}`, "-pix_fmt", "gray", "-f", "rawvideo", "-"],
    { maxBuffer: W * H * 4 },
  );
}

/** Mean absolute difference of two greyscale frames, 0..255. */
function meanDiff(a: Buffer, b: Buffer): number {
  let sum = 0;
  for (let i = 0; i < a.length; i++) sum += Math.abs(a[i] - b[i]);
  return sum / a.length;
}

const D = FILM.durationSeconds;
const frame = 1 / FILM.fps;

describe.each(LANGS)("the %s cut's last second", (lang) => {
  const { film } = cutPaths(lang);
  const last = frameAt(film, D - 1.5 * frame);

  it("is blank paper from its first frame to the end", () => {
    for (const at of [D - FILM.fadeOutSeconds, D - FILM.fadeOutSeconds / 2]) {
      expect(meanDiff(frameAt(film, at), last), `${at.toFixed(2)} s`).toBeLessThan(2);
    }
  });

  it("differs from a frame with the Beat still on screen (the check can fail)", () => {
    expect(meanDiff(frameAt(film, D - 3), last)).toBeGreaterThan(2);
  });
});
