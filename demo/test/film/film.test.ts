/**
 * The built Film cuts, inspected with ffprobe. Run with `pnpm run test:film`:
 * the global setup (test/film/global-setup.ts) rebuilds both cuts first
 * whenever out/ is missing or older than any source file.
 */
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { cutPaths, PATHS } from "../../scripts/paths.ts";
import { reviewFrameTimes } from "../../scripts/review-frames.ts";
import { FILM, LANGS } from "../../timeline/index.ts";

interface Stream {
  codec_type: string;
  codec_name: string;
  width?: number;
  height?: number;
  r_frame_rate?: string;
  duration: string;
}

function probe(file: string): { streams: Stream[]; duration: number } {
  const out = JSON.parse(
    execFileSync("ffprobe", ["-v", "error", "-show_streams", "-show_format", "-of", "json", file], { encoding: "utf8" }),
  );
  return { streams: out.streams, duration: Number(out.format.duration) };
}

/** Mean and max volume (dBFS) of `file`'s audio, optionally from `start` for `length` seconds. */
function volume(file: string, start?: number, length?: number): { mean: number; max: number } {
  const window = start === undefined ? [] : ["-ss", String(start), "-t", String(length)];
  const { stderr } = spawnSync(
    "ffmpeg",
    ["-hide_banner", "-nostats", ...window, "-i", file, "-af", "volumedetect", "-vn", "-f", "null", "-"],
    { encoding: "utf8" },
  );
  const read = (name: string) => {
    const match = new RegExp(`${name}_volume: (-?[\\d.]+|-inf) dB`).exec(stderr);
    if (!match) throw new Error(`ffmpeg volumedetect printed no ${name} volume for ${file}`);
    return match[1] === "-inf" ? -Infinity : Number(match[1]);
  };
  return { mean: read("mean"), max: read("max") };
}

const D = FILM.durationSeconds;

describe.each(LANGS)("the %s cut", (lang) => {
  const cut = cutPaths(lang);
  const { streams, duration } = probe(cut.film);
  const video = streams.filter((s) => s.codec_type === "video");
  const audio = streams.filter((s) => s.codec_type === "audio");

  it(`lasts the Film's ${D} s (± 0.1 s)`, () => {
    expect(Math.abs(duration - D)).toBeLessThanOrEqual(0.1);
  });

  it("has exactly one video and one audio stream", () => {
    expect(streams).toHaveLength(2);
    expect(video).toHaveLength(1);
    expect(audio).toHaveLength(1);
  });

  it(`is H.264 ${FILM.width}x${FILM.height} at ${FILM.fps} fps`, () => {
    expect(video[0]).toMatchObject({
      codec_name: "h264",
      width: FILM.width,
      height: FILM.height,
      r_frame_rate: `${FILM.fps}/1`,
    });
  });

  it("has AAC audio as long as the picture (± 0.1 s)", () => {
    expect(audio[0].codec_name).toBe("aac");
    expect(Math.abs(Number(audio[0].duration) - Number(video[0].duration))).toBeLessThanOrEqual(0.1);
  });

  it("has audible music with headroom, still playing near the end", () => {
    const whole = volume(cut.film);
    expect(whole.max).toBeGreaterThan(-20);
    expect(whole.max).toBeLessThan(-0.5);
    expect(volume(cut.film, D - 2, 0.5).mean).toBeGreaterThan(-50);
  });

  it("fades out below -40 dB in the final second", () => {
    expect(volume(cut.film, D - 1, 1).mean).toBeLessThan(-40);
  });

  it("comes with every PNG review frame", () => {
    for (const { name } of reviewFrameTimes()) {
      expect(fs.existsSync(path.join(cut.framesDir, `${name}.png`)), name).toBe(true);
    }
  });
});

describe("the shared soundtrack", () => {
  it("is a WAV of the Film's length", () => {
    expect(Math.abs(probe(PATHS.audioWav).duration - D)).toBeLessThanOrEqual(0.01);
  });
});
