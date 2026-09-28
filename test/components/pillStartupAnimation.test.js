const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { spawn } = require("node:child_process");

test(
  "the Recording pill keeps animating while microphone startup blocks the renderer",
  { timeout: 60000 },
  async (t) => {
    const directory = await fs.mkdtemp(path.join(os.tmpdir(), "whisper-pill-animation-"));
    let child;
    t.after(async () => {
      if (child && child.exitCode === null && child.signalCode === null) {
        await new Promise((resolve) => {
          child.once("exit", resolve);
          child.kill("SIGKILL");
        });
      }
      await fs.rm(directory, { recursive: true, force: true });
    });
    const [{ build }, { default: react }, { default: tailwindcss }] = await Promise.all([
      import("vite"),
      import("@vitejs/plugin-react"),
      import("@tailwindcss/vite"),
    ]);
    const fixture = path.resolve(__dirname, "../fixtures/pill-startup");
    await build({
      root: fixture,
      configFile: false,
      cacheDir: path.join(directory, "cache"),
      plugins: [react(), tailwindcss()],
      base: "./",
      logLevel: "silent",
      build: { outDir: path.join(directory, "renderer") },
    });
    const env = { ...process.env };
    delete env.ELECTRON_RUN_AS_NODE;
    let output = "";
    const exitCode = await new Promise((resolve, reject) => {
      child = spawn(require("electron"), [path.join(fixture, "electron.cjs"), directory], {
        env,
        stdio: ["ignore", "pipe", "pipe"],
      });
      child.stdout.on("data", (data) => {
        output += data;
      });
      child.stderr.on("data", (data) => {
        output += data;
      });
      child.on("error", reject);
      child.on("exit", resolve);
    });
    assert.equal(exitCode, 0, output);
    const result = output.split("\n").find((line) => line.startsWith('{"samples":'));
    assert.ok(result, "Electron must report the completed animation checks");
    t.diagnostic(result);
  }
);
