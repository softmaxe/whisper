/**
 * Film test global setup: the tests must never pass on stale output. If a
 * built cut or any review frame is missing, or older than any source file
 * that goes into it, both cuts are rebuilt (`npm run build`) first.
 */
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { cutPaths, PATHS, ROOT } from "../../scripts/paths.ts";
import { reviewFrameTimes } from "../../scripts/review-frames.ts";
import { LANGS } from "../../timeline/index.ts";

/** Everything the build reads: timeline, picture, fonts, audio synthesiser, build scripts, dependency locks. */
const SOURCES = [
  "timeline",
  "src",
  "public/fonts",
  "audio/whisper_audio",
  "audio/pyproject.toml",
  "audio/uv.lock",
  "scripts",
  "package.json",
  "package-lock.json",
  "tsconfig.json",
];
const IGNORED_DIRS = new Set(["__pycache__", ".venv", "node_modules"]);

function newestSource(entry: string): { file: string; mtime: number } {
  const stat = fs.statSync(entry);
  if (!stat.isDirectory()) return { file: entry, mtime: stat.mtimeMs };
  let newest = { file: entry, mtime: 0 };
  for (const child of fs.readdirSync(entry, { withFileTypes: true })) {
    if (IGNORED_DIRS.has(child.name) || child.name.startsWith(".")) continue;
    const candidate = newestSource(path.join(entry, child.name));
    if (candidate.mtime > newest.mtime) newest = candidate;
  }
  return newest;
}

/** Every file the build must produce. */
export function buildOutputs(): string[] {
  return [
    PATHS.audioWav,
    ...LANGS.flatMap((lang) => {
      const cut = cutPaths(lang);
      return [cut.film, ...reviewFrameTimes().map(({ name }) => path.join(cut.framesDir, `${name}.png`))];
    }),
  ];
}

/** Why the built output can't be trusted, or null if it is up to date. */
export function staleReason(): string | null {
  const outputs = buildOutputs();
  const missing = outputs.find((f) => !fs.existsSync(f));
  if (missing) return `${path.relative(ROOT, missing)} is missing`;
  const oldestOutput = Math.min(...outputs.map((f) => fs.statSync(f).mtimeMs));
  const newest = SOURCES.map((s) => newestSource(path.join(ROOT, s))).reduce((a, b) => (b.mtime > a.mtime ? b : a));
  return newest.mtime > oldestOutput ? `${path.relative(ROOT, newest.file)} changed after the last build` : null;
}

export default function setup(): void {
  const reason = staleReason();
  if (!reason) return;
  console.log(`\nFilm test: ${reason}; rebuilding both cuts before testing them (npm run build)…`);
  execFileSync("npm", ["run", "build"], { cwd: ROOT, stdio: "inherit" });
  const still = staleReason();
  if (still) throw new Error(`Film test: the build did not refresh its output (${still})`);
}
