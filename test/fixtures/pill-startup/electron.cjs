const { app, BrowserWindow } = require("electron");
const assert = require("node:assert/strict");
const path = require("node:path");
const { createHash } = require("node:crypto");
const { setTimeout: delay } = require("node:timers/promises");

const directory = process.argv[2];
app.setPath("userData", path.join(directory, "profile"));
const timeout = setTimeout(() => {
  console.error("Recording pill animation check timed out");
  app.exit(1);
}, 20000);

app.whenReady().then(async () => {
  const win = new BrowserWindow({
    width: 200,
    height: 80,
    show: false,
    webPreferences: { offscreen: true, backgroundThrottling: false },
  });
  const run = (code) => win.webContents.executeJavaScript(`window.pillTest.${code}`);
  const samples = [];
  let blockedAt = null;
  let frames = [];
  win.webContents.setFrameRate(60);
  win.webContents.on("paint", (_event, _dirty, image) => {
    if (blockedAt === null) return;
    const elapsed = performance.now() - blockedAt;
    // Ignore dispatch and unblock boundaries. These frames must arrive while
    // JavaScript cannot run, not just before or after the expensive operation.
    if (elapsed < 150 || elapsed > 1350) return;
    frames.push({
      elapsed,
      hash: createHash("sha256").update(image.toBitmap()).digest("hex"),
    });
  });
  const checkBlockedAnimation = async (name) => {
    await run('render("processing")');
    await delay(100);
    frames = [];
    blockedAt = performance.now();
    const blockedMs = await run("block(1500)");
    blockedAt = null;
    const uniqueFrames = new Set(frames.map((frame) => frame.hash)).size;
    const boundaries = [150, ...frames.map((frame) => frame.elapsed), 1350];
    const longestGap = Math.max(...boundaries.slice(1).map((time, i) => time - boundaries[i]));
    samples.push({ name, blockedMs, uniqueFrames, longestGap });
    assert.ok(uniqueFrames >= 20, `${name}: only ${uniqueFrames} changing frames during startup`);
    assert.ok(longestGap < 250, `${name}: animation stopped for ${longestGap.toFixed(0)} ms`);
  };
  try {
    await win.loadFile(path.join(directory, "renderer", "index.html"));
    await checkBlockedAnimation("first appearance");
    assert.equal((await run("sample()")).reads, 0, "connection feedback must not sample audio");

    await run('render("finishing")');
    await delay(100);
    const frozen = (await win.webContents.capturePage()).toBitmap();
    await delay(200);
    assert.deepEqual((await win.webContents.capturePage()).toBitmap(), frozen);
    await checkBlockedAnimation("restart during finish");

    await run('render("recording", 0)');
    await delay(150);
    const silence = await run("sample()");
    assert.ok(silence.reads > 0);
    assert.equal(silence.animations, 0, "ready audio must replace the connection animation");
    await run('render("recording", 0.1)');
    await delay(150);
    assert.ok((await run("sample()")).height > silence.height, "speech must raise the live bars");

    await run('render("idle")');
    assert.equal((await run("sample()")).animations, 0);
    await checkBlockedAnimation("new appearance after idle");
    assert.equal(await run("unmount()"), true, "unmount must cancel compositor animations");
    console.log(JSON.stringify({ samples }));
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  } finally {
    clearTimeout(timeout);
    win.destroy();
    app.exit(process.exitCode || 0);
  }
});
