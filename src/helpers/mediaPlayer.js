const { spawn } = require("child_process");
const path = require("path");
const fs = require("fs");
const debugLogger = require("./debugLogger");
const { killProcess } = require("../utils/process");

// spawnSync capped a child's output at 1 MB and killed anything past it. Keep
// that bound: these helpers emit a few KB, and an unbounded buffer would let a
// runaway one grow main-process memory until its deadline.
const MAX_OUTPUT_BYTES = 1024 * 1024;

// Runs `cmd args` asynchronously and resolves with
// { status, stdout, stderr, timedOut }. Times out after `timeout` ms; on
// timeout it kills the child and resolves with the exit code the child already
// reported, or status: null and timedOut: true when it never exited. A spawn
// failure also resolves with status: null but timedOut: false. Output is
// capped at MAX_OUTPUT_BYTES. Never rejects — callers branch on status === 0.
//
// Every media helper goes through here rather than spawnSync: a synchronous
// spawn parks the Electron main thread in a nested libuv loop until the
// child's stdio pipes close, which froze hotkeys, IPC and the dictation
// window mid-recording (#2073).
function spawnAsync(cmd, args, { timeout = 3000 } = {}) {
  return new Promise((resolve) => {
    let child;
    try {
      child = spawn(cmd, args, { stdio: ["ignore", "pipe", "pipe"] });
    } catch (err) {
      resolve({ status: null, stdout: "", stderr: String(err?.message || err), timedOut: false });
      return;
    }

    const chunks = { stdout: [], stderr: [] };
    let bufferedBytes = 0;
    const collect = (stream, chunk) => {
      if (bufferedBytes >= MAX_OUTPUT_BYTES) return;
      bufferedBytes += chunk.length;
      chunks[stream].push(chunk);
    };
    let settled = false;
    const settle = (status, timedOut = false) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        status,
        stdout: Buffer.concat(chunks.stdout).toString("utf8"),
        stderr: Buffer.concat(chunks.stderr).toString("utf8"),
        timedOut,
      });
    };

    const timer = setTimeout(() => {
      // A helper can finish its work and exit while a descendant it spawned
      // keeps the inherited pipes open, so the deadline can arrive with the
      // outcome already known. Honour that exit code: discarding it sends the
      // pause into the media-key fallback, which toggles playback back on
      // mid-dictation and then leaves it paused afterwards (#2073).
      const exitCode = child.exitCode;
      try {
        // A no-op once the child has exited; dropping our own pipe ends is
        // what lets the deadline resolve while a descendant lingers.
        killProcess(child, "SIGKILL");
        child.stdout.destroy();
        child.stderr.destroy();
        debugLogger.warn(
          "Media helper timed out",
          { cmd: path.basename(cmd), timeout, exitCode },
          "media"
        );
      } catch {
        // Teardown and logging are best effort; the deadline must still settle
        // below, or the serialized queue stalls for the rest of the session.
      }
      settle(exitCode, exitCode === null);
    }, timeout);

    child.stdout.on("data", (d) => collect("stdout", d));
    child.stderr.on("data", (d) => collect("stderr", d));
    child.on("error", (err) => {
      // Pushed past the cap on purpose: this is the only diagnostic a failed
      // spawn produces, and it must not be the thing the cap drops.
      chunks.stderr.push(Buffer.from(String(err?.message || err)));
      settle(null);
    });
    child.on("close", (code) => settle(code));
  });
}

class MediaPlayer {
  constructor() {
    this._macBinaryChecked = false;
    this._macBinaryPath = null;
    this._didPause = false; // Whether we sent a pause
    this._adapterChecked = false;
    this._adapterPaths = null; // { perl, script, framework } once resolved
    this._pausedViaAdapter = false; // Whether we paused via the adapter
    // Pause/resume/toggle run one at a time: a quick tap could otherwise run
    // the resume before the pause had recorded what it paused, stranding media
    // until the next dictation ended (#2073).
    this._queue = Promise.resolve();
  }

  // The queue holds a caught copy so a failed operation can't stall the ones
  // behind it, while the caller still gets the raw run.
  _serialize(operation) {
    const run = this._queue.then(operation);
    this._queue = run.catch(() => {});
    return run;
  }

  _resolveMacMediaRemote() {
    if (this._macBinaryChecked) return this._macBinaryPath;
    this._macBinaryChecked = true;

    const candidates = [
      path.join(__dirname, "..", "..", "resources", "bin", "macos-media-remote"),
      path.join(__dirname, "..", "..", "resources", "macos-media-remote"),
    ];

    if (process.resourcesPath) {
      candidates.push(path.join(process.resourcesPath, "bin", "macos-media-remote"));
    }

    for (const candidate of candidates) {
      try {
        if (fs.existsSync(candidate)) {
          fs.accessSync(candidate, fs.constants.X_OK);
          this._macBinaryPath = candidate;
          return candidate;
        }
      } catch {
        continue;
      }
    }
    return null;
  }

  // Resolves the vendored mediaremote-adapter Perl entry point and framework.
  // MediaRemote.framework was closed to unprivileged Mach-O processes on
  // macOS 15.4+; the only working state-aware path is to load our adapter
  // framework via /usr/bin/perl, which is system-entitled to talk to it.
  _resolveMediaRemoteAdapter() {
    if (this._adapterChecked) return this._adapterPaths;
    this._adapterChecked = true;

    const perl = "/usr/bin/perl";
    if (!fs.existsSync(perl)) return null;

    const scriptCandidates = [];
    const frameworkCandidates = [];

    if (process.resourcesPath) {
      scriptCandidates.push(path.join(process.resourcesPath, "bin", "mediaremote-adapter.pl"));
      frameworkCandidates.push(
        path.join(process.resourcesPath, "bin", "MediaRemoteAdapter.framework")
      );
    }

    scriptCandidates.push(
      path.join(
        __dirname,
        "..",
        "..",
        "resources",
        "mediaremote-adapter",
        "bin",
        "mediaremote-adapter.pl"
      )
    );
    frameworkCandidates.push(
      path.join(__dirname, "..", "..", "resources", "bin", "MediaRemoteAdapter.framework")
    );

    const script = scriptCandidates.find((p) => fs.existsSync(p));
    const framework = frameworkCandidates.find((p) => fs.existsSync(p));
    if (!script || !framework) return null;

    this._adapterPaths = { perl, script, framework };
    return this._adapterPaths;
  }

  pauseMedia() {
    return this._serialize(async () => {
      try {
        return await this._pauseMacOS();
      } catch (err) {
        debugLogger.warn("Media pause failed", { error: err.message }, "media");
      }
      return false;
    });
  }

  resumeMedia() {
    return this._serialize(async () => {
      try {
        return await this._resumeMacOS();
      } catch (err) {
        debugLogger.warn("Media resume failed", { error: err.message }, "media");
      }
      return false;
    });
  }

  toggleMedia() {
    return this._serialize(async () => {
      try {
        return await this._sendMacMediaKey();
      } catch (err) {
        debugLogger.warn("Media toggle failed", { error: err.message }, "media");
      }
      return false;
    });
  }

  // --- macOS: MediaRemote-aware pause/resume ---

  async _runAdapter(args, timeout = 3000) {
    const paths = this._resolveMediaRemoteAdapter();
    if (!paths) return null;
    return spawnAsync(paths.perl, [paths.script, paths.framework, ...args], {
      timeout,
    });
  }

  async _pauseMacOS() {
    this._didPause = false;
    this._pausedViaAdapter = false;

    // Primary path: vendored mediaremote-adapter via /usr/bin/perl. Works on
    // macOS 15.4+ where the framework is closed to user processes.
    const probe = await this._runAdapter(["get", "--no-artwork"]);
    if (probe && probe.status === 0) {
      const output = (probe.stdout || "").trim();
      let playing = null;
      if (output && output !== "null") {
        try {
          playing = !!JSON.parse(output).playing;
        } catch {
          playing = null;
        }
      } else if (output === "null") {
        playing = false;
      }

      if (playing === false) {
        debugLogger.debug("Adapter reports no media playing", {}, "media");
        return false;
      }

      if (playing === true) {
        // 1 = kMRAPause
        const pause = await this._runAdapter(["send", "1"]);
        if (pause && pause.status === 0) {
          debugLogger.debug("Media paused via adapter", {}, "media");
          this._pausedViaAdapter = true;
          this._didPause = true;
          return true;
        }
        debugLogger.debug(
          "Adapter send pause failed",
          {
            status: pause?.status,
            stderr: (pause?.stderr || "").trim().slice(0, 200),
          },
          "media"
        );
      }
    } else if (probe) {
      debugLogger.debug(
        "Adapter get failed, falling back to media key",
        {
          status: probe.status,
          stderr: (probe.stderr || "").trim().slice(0, 200),
        },
        "media"
      );
    }

    // Fallback: post a real media-key CGEvent. We don't know whether anything
    // is playing, so this can spuriously start playback — same toggle risk
    // the binary-based path had pre-adapter.
    if (await this._sendMacMediaKey()) {
      this._didPause = true;
      return true;
    }
    return false;
  }

  async _resumeMacOS() {
    if (!this._didPause) return false;
    const usedAdapter = this._pausedViaAdapter;
    this._didPause = false;
    this._pausedViaAdapter = false;

    if (usedAdapter) {
      // 0 = kMRAPlay
      const play = await this._runAdapter(["send", "0"]);
      if (play && play.status === 0) {
        debugLogger.debug("Media resumed via adapter", {}, "media");
        return true;
      }
      debugLogger.debug(
        "Adapter send play failed, falling back to media key",
        {
          status: play?.status,
          stderr: (play?.stderr || "").trim().slice(0, 200),
        },
        "media"
      );
    }

    return this._sendMacMediaKey();
  }

  // Posts a real NX_KEYTYPE_PLAY system-defined NSEvent via the bundled
  // helper. Media apps only respond to that event class — synthetic F-key
  // codes (osascript "key code") are not media keys and land in the focused
  // app as plain keystrokes instead.
  async _sendMacMediaKey() {
    const binary = this._resolveMacMediaRemote();
    if (!binary) return false;

    const result = await spawnAsync(binary, ["--media-key-toggle"], {
      timeout: 3000,
    });
    if (result.status === 0) {
      debugLogger.debug("Media key sent via CGEvent helper", {}, "media");
      return true;
    }
    debugLogger.debug(
      "CGEvent media-key helper failed",
      {
        status: result.status,
        stderr: (result.stderr || "").trim().slice(0, 200),
      },
      "media"
    );
    return false;
  }
}

module.exports = new MediaPlayer();
