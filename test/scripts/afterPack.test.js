const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const afterPack = require("../../scripts/afterPack").default;

function packedApp(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "whisper-pack-test-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const modules = path.join(
    directory,
    "Whisper.app",
    "Contents",
    "Resources",
    "app.asar.unpacked",
    "node_modules"
  );
  const ffmpeg = path.join(modules, "ffmpeg-static", "ffmpeg");
  const sqlite = path.join(modules, "better-sqlite3", "build", "Release", "better_sqlite3.node");
  for (const file of [ffmpeg, sqlite]) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, "native binary fixture");
  }
  const context = {
    electronPlatformName: "darwin",
    appOutDir: directory,
    packager: { appInfo: { productFilename: "Whisper" }, platformSpecificBuildOptions: {} },
  };
  return { context, ffmpeg, sqlite };
}

test("packaging accepts the dictation runtime without retired model workers", async (t) => {
  const { context, ffmpeg, sqlite } = packedApp(t);
  await afterPack(context);
  assert.ok(fs.existsSync(ffmpeg));
  assert.ok(fs.existsSync(sqlite));
});

test("packaging rejects a missing FFmpeg executable needed for file transcription", async (t) => {
  const { context, ffmpeg } = packedApp(t);
  fs.rmSync(ffmpeg);
  await assert.rejects(afterPack(context), /the packed app cannot spawn FFmpeg/);
});

test("packaging rejects a missing SQLite binding needed for History", async (t) => {
  const { context, sqlite } = packedApp(t);
  fs.rmSync(sqlite);
  await assert.rejects(afterPack(context), /the packed app cannot open History/);
});
