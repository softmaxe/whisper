// Build only redistributable code; the npm ffmpeg-static binary contains nonfree components.
const assert = require("node:assert/strict");
const { createHash } = require("node:crypto");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const SOURCES = {
  ffmpeg: {
    version: "9.0.2",
    name: "ffmpeg-9.0.2.tar.xz",
    url: "https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz",
    sha256: "8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e",
  },
  lame: {
    version: "4.0",
    name: "lame-4.0.tar.gz",
    url: "https://downloads.sourceforge.net/project/lame/lame/4.0/lame-4.0.tar.gz",
    sha256: "3df5124d5ad3a98312ffd7ba6a9b36230e4f8a3e66d3ce0f425e336c32d216eb",
  },
};
const root = path.resolve(__dirname, "..");
function argument(name, fallback) {
  const index = process.argv.indexOf(name);
  if (index < 0) return fallback;
  assert.ok(
    process.argv[index + 1] && !process.argv[index + 1].startsWith("--"),
    `Missing ${name} value.`
  );
  return path.resolve(process.argv[index + 1]);
}
const work = argument("--work-directory", path.join(root, ".cache/native-codecs"));
const sources = argument("--source-directory", path.join(work, "sources"));
const output = argument("--output-directory", path.join(root, "resources/bin"));
function run(command, args, cwd = work, env = process.env) {
  const result = spawnSync(command, args, {
    cwd,
    env,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error || result.status !== 0)
    throw new Error(
      `${path.basename(command)} failed: ${result.stderr || result.stdout || result.error?.message}${command === "./configure" && fs.existsSync(path.join(cwd, "ffbuild/config.log")) ? "\n" + fs.readFileSync(path.join(cwd, "ffbuild/config.log"), "utf8").split("\n").slice(-45).join("\n") : ""}`
    );
  return result.stdout;
}
function digest(file) {
  return createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}
function verify(binary) {
  assert.equal(run("/usr/bin/lipo", ["-archs", binary]).trim(), "arm64");
  const license = run(binary, ["-L"]);
  assert.match(license, /GNU Lesser General Public\s+License/);
  assert.doesNotMatch(license, /not legally redistributable/);
  const configuration = run(binary, ["-buildconf"]);
  assert.doesNotMatch(configuration, /--enable-(?:nonfree|gpl|version3)/);
  assert.match(configuration, /--disable-autodetect/);
  const dependencies = run("/usr/bin/otool", ["-L", binary]);
  assert.doesNotMatch(dependencies, /\/opt\/homebrew|\/usr\/local|libmp3lame|libavcodec/);
}
function main() {
  assert.equal(process.platform, "darwin");
  assert.equal(process.arch, "arm64");
  fs.mkdirSync(work, { recursive: true });
  fs.mkdirSync(sources, { recursive: true });
  fs.mkdirSync(output, { recursive: true });
  const binary = path.join(output, "ffmpeg");
  const licenses = path.join(output, "ffmpeg-licenses");
  const stamp = path.join(output, ".native-ffmpeg.json");
  const recipe = digest(__filename);
  if (
    fs.existsSync(binary) &&
    fs.existsSync(stamp) &&
    fs.existsSync(path.join(licenses, "sources.json"))
  ) {
    const saved = JSON.parse(fs.readFileSync(stamp, "utf8"));
    const completeSources = Object.values(SOURCES).every((source) => {
      const file = path.join(licenses, "sources", source.name);
      return fs.existsSync(file) && digest(file) === source.sha256;
    });
    const licenseFiles = [
      "FFmpeg-LICENSE.md",
      "FFmpeg-COPYING.LGPLv2.1",
      "LAME-COPYING",
      "BUILD.md",
      "build-native-ffmpeg.js",
    ];
    if (
      saved.recipe === recipe &&
      saved.binary === digest(binary) &&
      completeSources &&
      licenseFiles.every((file) => fs.existsSync(path.join(licenses, file)))
    ) {
      verify(binary);
      console.log("Verified cached redistributable FFmpeg.");
      return;
    }
  }
  for (const source of Object.values(SOURCES)) {
    const file = path.join(sources, source.name);
    if (!fs.existsSync(file)) {
      const partial = file + ".download";
      try {
        run("/usr/bin/curl", [
          "--fail",
          "--location",
          "--retry",
          "3",
          "--output",
          partial,
          source.url,
        ]);
        assert.equal(digest(partial), source.sha256, `Unexpected checksum for ${source.name}`);
        fs.renameSync(partial, file);
      } finally {
        fs.rmSync(partial, { force: true });
      }
    }
    assert.equal(digest(file), source.sha256, `Unexpected checksum for ${source.name}`);
  }
  const build = fs.mkdtempSync(path.join(work, "build-"));
  try {
    const cc = run("/usr/bin/xcrun", ["--find", "clang"]).trim();
    const env = {
      ...process.env,
      PATH: path.dirname(cc) + path.delimiter + process.env.PATH,
      CFLAGS: "-O2 -arch arm64 -mmacosx-version-min=27.0",
      LDFLAGS: "-arch arm64 -mmacosx-version-min=27.0",
      ac_cv_prog_cc_c23: "no",
      SDKROOT: run("/usr/bin/xcrun", ["--sdk", "macosx", "--show-sdk-path"]).trim(),
    };
    const jobs = String(Math.max(1, Math.min(8, os.availableParallelism() - 2)));
    for (const source of Object.values(SOURCES))
      run("/usr/bin/tar", ["-xf", path.join(sources, source.name), "-C", build]);
    const lame = path.join(build, `lame-${SOURCES.lame.version}`);
    const ffmpeg = path.join(build, `ffmpeg-${SOURCES.ffmpeg.version}`);
    console.log("Building static LAME from verified source.");
    run(
      "./configure",
      [
        "--prefix=" + path.join(build, "prefix"),
        "--disable-shared",
        "--enable-static",
        "--disable-frontend",
        "--disable-decoder",
        "--disable-dependency-tracking",
      ],
      lame,
      env
    );
    run("/usr/bin/make", ["-j", jobs], lame, env);
    run("/usr/bin/make", ["install"], lame, env);
    console.log("Building LGPL FFmpeg with built-in decoders and libmp3lame.");
    run(
      "./configure",
      [
        "--cc=clang",
        "--arch=aarch64",
        "--target-os=darwin",
        "--disable-autodetect",
        "--disable-gpl",
        "--disable-version3",
        "--disable-nonfree",
        "--disable-shared",
        "--enable-static",
        "--disable-doc",
        "--disable-debug",
        "--disable-ffplay",
        "--disable-ffprobe",
        "--disable-network",
        "--disable-indevs",
        "--enable-indev=lavfi",
        "--disable-outdevs",
        "--enable-libmp3lame",
        "--enable-zlib",
        "--enable-bzlib",
        "--extra-cflags=-arch arm64 -mmacosx-version-min=27.0 -I../prefix/include",
        "--extra-ldflags=-arch arm64 -mmacosx-version-min=27.0 -L../prefix/lib",
      ],
      ffmpeg,
      env
    );
    run("/usr/bin/make", ["-j", jobs, "ffmpeg"], ffmpeg, env);
    verify(path.join(ffmpeg, "ffmpeg"));
    fs.copyFileSync(path.join(ffmpeg, "ffmpeg"), binary);
    fs.chmodSync(binary, 0o755);
    fs.rmSync(licenses, { recursive: true, force: true });
    fs.mkdirSync(path.join(licenses, "sources"), { recursive: true });
    fs.copyFileSync(path.join(ffmpeg, "LICENSE.md"), path.join(licenses, "FFmpeg-LICENSE.md"));
    fs.copyFileSync(
      path.join(ffmpeg, "COPYING.LGPLv2.1"),
      path.join(licenses, "FFmpeg-COPYING.LGPLv2.1")
    );
    fs.copyFileSync(path.join(lame, "COPYING"), path.join(licenses, "LAME-COPYING"));
    for (const source of Object.values(SOURCES))
      fs.copyFileSync(path.join(sources, source.name), path.join(licenses, "sources", source.name));
    fs.copyFileSync(__filename, path.join(licenses, "build-native-ffmpeg.js"));
    fs.writeFileSync(path.join(licenses, "sources.json"), JSON.stringify(SOURCES, null, 2) + "\n");
    fs.writeFileSync(
      path.join(licenses, "BUILD.md"),
      "# Rebuild the bundled decoder\n\nFFmpeg and LAME are statically linked from the complete, unmodified source archives in `sources/`. The bundled executable uses LGPL code; nonfree/GPL/version3 components and external-library autodetection are disabled. The exact build recipe is included.\n\nOn Apple Silicon with macOS 27, Xcode 27 and Node.js 24, run:\n\n```sh\nDEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer node build-native-ffmpeg.js --source-directory ./sources --work-directory /tmp/whisper-codec-build --output-directory /tmp/whisper-codec-output\n```\n\nThe rebuilt standalone executable can replace `Contents/Resources/bin/ffmpeg` in your own copy of the application. Any local bundle modification requires re-signing your copy. The application's source and development signing commands are available at https://github.com/softmaxe/whisper. The separate FFmpeg executable communicates over files and standard process I/O.\n"
    );
    fs.writeFileSync(stamp, JSON.stringify({ recipe, binary: digest(binary) }) + "\n");
    console.log(
      "Built redistributable arm64 FFmpeg with complete corresponding source and licenses."
    );
  } finally {
    fs.rmSync(build, { recursive: true, force: true });
  }
}
try {
  main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
