/**
 * One-command build of both Film cuts:
 *   timeline JSON -> audio synthesis (Python/uv) -> bundle (Remotion)
 *   -> for each cut: render video -> mux with the audio (ffmpeg) -> review frames (ffmpeg).
 *
 * Usage: npm run build [-- --concurrency=N]
 * Output (git-ignored): out/whisper-film-en.mp4, out/whisper-film-zh-CN.mp4,
 * out/audio.wav and out/frames/<lang>/*.png.
 */
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { bundle } from "@remotion/bundler";
import { renderMedia, selectComposition } from "@remotion/renderer";
import { FILM, LANGS } from "../timeline/index.ts";
import { exportTimeline } from "./export-timeline.ts";
import { cutPaths, FILM_COMPOSITION_IDS, OUT_DIR, PATHS, ROOT } from "./paths.ts";
import { reviewFrameTimes } from "./review-frames.ts";

function run(cmd: string, args: string[], cwd = ROOT): void {
  execFileSync(cmd, args, { cwd, stdio: "inherit" });
}

async function step<T>(name: string, fn: () => T | Promise<T>): Promise<T> {
  const t0 = Date.now();
  console.log(`\n▶ ${name}`);
  const value = await fn();
  console.log(`✔ ${name} (${((Date.now() - t0) / 1000).toFixed(1)}s)`);
  return value;
}

/** `--concurrency=N` (a positive integer), or undefined when not given. */
function parseConcurrency(): number | undefined {
  const arg = process.argv.find((a) => a.startsWith("--concurrency="));
  if (!arg) return undefined;
  const value = arg.slice("--concurrency=".length);
  if (!/^[1-9]\d*$/.test(value)) {
    throw new Error(`--concurrency must be a positive integer (e.g. --concurrency=4), got "${value}"`);
  }
  return Number(value);
}

async function main(): Promise<void> {
  const t0 = Date.now();
  const concurrency = parseConcurrency() ?? Math.max(1, Math.floor(os.cpus().length * 0.75));
  fs.mkdirSync(OUT_DIR, { recursive: true });

  await step("Export timeline", () => exportTimeline(PATHS.timelineJson));

  await step("Synthesise audio", () =>
    run("uv", ["run", "--project", PATHS.audioProject, "python", "-m", "whisper_audio", PATHS.timelineJson, PATHS.audioWav]),
  );

  const serveUrl = await step("Bundle", () => bundle({ entryPoint: PATHS.videoEntry, publicDir: PATHS.publicDir }));

  for (const lang of LANGS) {
    const cut = cutPaths(lang);

    await step(`Render ${lang} video`, async () => {
      const composition = await selectComposition({ serveUrl, id: FILM_COMPOSITION_IDS[lang] });
      let lastPct = -1;
      await renderMedia({
        serveUrl,
        composition,
        codec: "h264",
        crf: 18,
        pixelFormat: "yuv420p",
        muted: true,
        imageFormat: "jpeg",
        jpegQuality: 92,
        concurrency,
        outputLocation: cut.videoOnly,
        onProgress: ({ progress }) => {
          const pct = Math.floor(progress * 10) * 10;
          if (pct !== lastPct) {
            lastPct = pct;
            console.log(`  render ${lang} ${pct}%`);
          }
        },
      });
    });

    await step(`Mux ${lang} video + audio`, () =>
      run("ffmpeg", [
        "-y", "-loglevel", "error",
        "-i", cut.videoOnly,
        "-i", PATHS.audioWav,
        "-map", "0:v:0", "-map", "1:a:0",
        "-c:v", "copy",
        "-c:a", "aac", "-b:a", "192k",
        "-t", String(FILM.durationSeconds),
        "-movflags", "+faststart",
        cut.film,
      ]),
    );

    await step(`Extract ${lang} review frames`, () => {
      fs.rmSync(cut.framesDir, { recursive: true, force: true });
      fs.mkdirSync(cut.framesDir, { recursive: true });
      for (const { name, at } of reviewFrameTimes()) {
        run("ffmpeg", [
          "-y", "-loglevel", "error",
          "-ss", at.toFixed(3),
          "-i", cut.film,
          "-frames:v", "1",
          path.join(cut.framesDir, `${name}.png`),
        ]);
      }
    });

    console.log(`Film (${lang}): ${path.relative(ROOT, cut.film)}`);
  }

  console.log(`\nReview frames: ${path.relative(ROOT, PATHS.framesRoot)}/<lang>/`);
  console.log(`Total build time: ${((Date.now() - t0) / 1000).toFixed(1)}s`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((err) => {
    console.error(err);
    process.exit(1);
  });
}
