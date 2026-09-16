const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

test("clean removes only project build output, regardless of the working directory", (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "whisper-clean-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const project = path.join(root, "project");
  const elsewhere = path.join(root, "elsewhere");
  const homeDir = path.join(root, "home");
  const outputs = ["dist", "src/dist", "node_modules/.cache"];
  const protectedFiles = [
    path.join(elsewhere, "dist", "keep.txt"),
    path.join(project, "src", "keep.txt"),
    ...["whisper", "open-whispr"].map((app) =>
      path.join(homeDir, "Library", "Application Support", app, "transcriptions-dev.db")
    ),
  ];
  for (const file of protectedFiles) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, "keep");
  }
  for (const dir of outputs) fs.mkdirSync(path.join(project, dir), { recursive: true });
  const script = path.join(project, "cleanup.js");
  fs.copyFileSync(path.join(__dirname, "../../cleanup.js"), script);
  execFileSync(process.execPath, [script], {
    cwd: elsewhere,
    env: { ...process.env, HOME: homeDir, USERPROFILE: homeDir },
  });
  for (const dir of outputs) assert.equal(fs.existsSync(path.join(project, dir)), false);
  for (const file of protectedFiles) assert.equal(fs.readFileSync(file, "utf8"), "keep");
});
