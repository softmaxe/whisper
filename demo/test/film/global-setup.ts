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

/**
 * Everything in demo/ the build reads: timeline, picture, fonts, audio
 * synthesiser, build scripts, dependency locks. App files the demo imports
 * come from appSources().
 */
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
/** Demo directories whose code may import the app's own sources. */
const CODE_DIRS = ["src", "timeline", "scripts"];
const CODE_FILE = /\.(ts|tsx|js|jsx|mjs)$/;
const IMPORT_SPECIFIER = /\b(?:from|import)\s*["']([^"']+)["']/g;
const RESOLVE_SUFFIXES = ["", ".ts", ".tsx", ".js", ".jsx", ".mjs", "/index.ts", "/index.js"];

function codeFiles(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    if (IGNORED_DIRS.has(entry.name) || entry.name.startsWith(".")) return [];
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return codeFiles(full);
    return CODE_FILE.test(entry.name) ? [full] : [];
  });
}

/** The files `file` imports by relative path, resolved like the bundler (extensionless or explicit). */
function relativeImports(file: string): string[] {
  const text = fs.readFileSync(file, "utf8");
  return [...text.matchAll(IMPORT_SPECIFIER)]
    .map((match) => match[1])
    .filter((specifier) => specifier.startsWith("."))
    .flatMap((specifier) => {
      const base = path.resolve(path.dirname(file), specifier);
      const found = RESOLVE_SUFFIXES.map((suffix) => base + suffix).find(
        (candidate) => fs.existsSync(candidate) && fs.statSync(candidate).isFile(),
      );
      return found ? [found] : [];
    });
}

/**
 * App files outside demo/ that the demo's code imports (e.g. the pill's
 * waveform math), followed through their own imports, so a change to the
 * app's pill also marks the built Film stale.
 */
export function appSources(): string[] {
  const inDemo = (file: string) => !path.relative(ROOT, file).startsWith("..");
  const pending = CODE_DIRS.flatMap((dir) => codeFiles(path.join(ROOT, dir)))
    .flatMap(relativeImports)
    .filter((file) => !inDemo(file));
  const found = new Set<string>();
  while (pending.length > 0) {
    const file = pending.pop()!;
    if (found.has(file)) continue;
    found.add(file);
    if (CODE_FILE.test(file)) pending.push(...relativeImports(file));
  }
  return [...found].sort();
}

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
  const sources = [...SOURCES.map((s) => path.join(ROOT, s)), ...appSources()];
  const newest = sources.map(newestSource).reduce((a, b) => (b.mtime > a.mtime ? b : a));
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
