const { spawn, execFile } = require("child_process");
const path = require("path");
const EventEmitter = require("events");
const fs = require("fs");
const debugLogger = require("./debugLogger");

const POLL_INTERVAL_MS = 500;
const INITIAL_QUERY_DELAY_MS = 500; // Wait for paste to settle in target app
const INITIAL_QUERY_RETRIES = 4; // Retry if AXValue is empty (paste not yet processed)
const INITIAL_QUERY_RETRY_DELAY_MS = 300;
const ACTIVATE_CONFIRM_RETRIES = 6; // Poll the frontmost app until activation lands
const ACTIVATE_CONFIRM_DELAY_MS = 25;
// One dictation triggers target captures from several call sites (hotkey press,
// toggle path, recording start) within a few hundred ms; reuse the result
// across that burst. Kept short so back-to-back dictations in different apps
// still get a fresh capture.
const TARGET_CAPTURE_FRESHNESS_MS = 250;
// AXError -25212 (kAXErrorNoValue) on the focused-element read is how
// Chromium/Electron apps respond while their AX tree is dormant, making the
// native binary's 5-attempt retry (~1.2s) dead time on every read. A single
// -25212 line can also be transient (the tree may wake mid-run — the binary
// then logs "Got focused element on attempt N"), so only a run where the
// element never resolved and every attempt failed with -25212 marks the app
// as unsupported for this session.
function isPersistentAxNoValueFailure(stderr) {
  const text = stderr || "";
  if (text.includes("Got focused element")) return false;
  const attemptCodes = text.match(/\(error: -?\d+\)/g) || [];
  return attemptCodes.length > 0 && attemptCodes.every((code) => code === "(error: -25212)");
}

// Monitoring is strictly read-only: never write AXEnhancedUserInterface (or any
// AX attribute) on the target app to force its accessibility tree. Flipping that
// flag switches the whole process into screen-reader mode for its lifetime and
// blurs the focused editor in some Chromium apps (Claude Desktop, claude.ai),
// so every dictation after the first pasted into a field that no longer had
// keyboard focus. Modern Chromium builds the tree on demand when our
// reads arrive; where it doesn't, we skip auto-learn for that paste instead.

// AppleScript to read the focused text field value from a specific app by PID.
// Using PID avoids the problem where the Electron overlay is "frontmost".
// Tries AXValue first, then falls back to AXStringForRange for apps that
// implement parameterized text attributes but not AXValue directly.
const MACOS_AX_SCRIPT_BY_PID = (pid) =>
  `tell application "System Events"\n` +
  `\tset targetProc to first application process whose unix id is ${pid}\n` +
  `\tset focAttr to value of attribute "AXFocusedUIElement" of targetProc\n` +
  `\tif focAttr is missing value then return ""\n` +
  `\ttry\n` +
  `\t\tset val to value of attribute "AXValue" of focAttr\n` +
  `\t\tif val is not missing value and val is not "" then return val\n` +
  `\tend try\n` +
  `\ttry\n` +
  `\t\tset charCount to value of attribute "AXNumberOfCharacters" of focAttr\n` +
  `\t\tif charCount is greater than 0 then\n` +
  `\t\t\treturn value of attribute "AXSelectedText" of focAttr\n` +
  `\t\tend if\n` +
  `\tend try\n` +
  `\treturn ""\n` +
  `end tell`;

class TextEditMonitor extends EventEmitter {
  constructor() {
    super();
    this.process = null;
    this.currentOriginalText = null;
    this.timeout = null;
    this._pollInterval = null;
    this._lastValue = null;
    this._stdoutBuffer = "";
    this.lastTargetPid = null;
    this._captureTargetPromise = null;
    this._lastCaptureAt = 0;
    this._windowBounds = null;
    // PIDs whose AX tree never yields a focused element (see
    // isPersistentAxNoValueFailure). A recycled PID only costs a detour via
    // AppleScript polling, which is still correct.
    this._nativeSelectionUnsupportedPids = new Set();
  }

  /**
   * macOS: capture the active app's PID via NSWorkspace before the overlay steals focus.
   * Must be called at hotkey press time, BEFORE showDictationPanel()/mainWindow.show().
   * NSWorkspace.frontmostApplication correctly identifies the key window owner,
   * ignoring panel-type windows like the OpenWhispr overlay.
   *
   * Resolves with the captured PID (or null). At most one lookup runs at a
   * time: concurrent calls share the in-flight lookup, so an older lookup can
   * never overwrite a newer target, and a just-captured result is reused
   * briefly so the press-time and recording-start captures of one dictation
   * cost a single osascript spawn instead of several.
   */
  captureTargetPid() {
    if (this._captureTargetPromise) return this._captureTargetPromise;
    if (
      this.lastTargetPid !== null &&
      Date.now() - this._lastCaptureAt < TARGET_CAPTURE_FRESHNESS_MS
    ) {
      return Promise.resolve(this.lastTargetPid);
    }
    this.lastTargetPid = null;
    this._captureTargetPromise = this._readFrontmostPid().then((pid) => {
      this._captureTargetPromise = null;
      this._lastCaptureAt = Date.now();
      this.lastTargetPid = pid;
      debugLogger.debug("[TextEditMonitor] Captured target PID", { pid });
      return pid;
    });
    return this._captureTargetPromise;
  }

  /**
   * macOS: resolve the frontmost app's PID, or null if it can't be read.
   */
  _readFrontmostPid() {
    return new Promise((resolve) => {
      const script =
        'ObjC.import("AppKit"); $.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier';
      execFile(
        "osascript",
        ["-l", "JavaScript", "-e", script],
        { timeout: 2000 },
        (err, stdout) => {
          const pid = err ? NaN : parseInt(stdout.trim(), 10);
          resolve(isNaN(pid) ? null : pid);
        }
      );
    });
  }

  /**
   * macOS: request activation of the app with the given PID, bringing all its
   * windows forward (AllWindows|IgnoringOtherApps) so one becomes key. Scans
   * runningApplications because NSRunningApplication's PID lookup returns nil under JXA.
   */
  _activateApp(pid) {
    return new Promise((resolve) => {
      const script = `
        ObjC.import("AppKit");
        const apps = $.NSWorkspace.sharedWorkspace.runningApplications;
        for (let i = 0; i < apps.count; i++) {
          const a = apps.objectAtIndex(i);
          if (a.processIdentifier === ${pid}) { a.activateWithOptions(3); break; }
        }
      `;
      execFile("osascript", ["-l", "JavaScript", "-e", script], { timeout: 2000 }, () => resolve());
    });
  }

  /**
   * macOS: make the captured target app frontmost before pasting, so the global
   * Cmd+V lands in its focused field (#668). Resolves true once the target is
   * confirmed frontmost. If it is already frontmost we do nothing: re-activating
   * an already-active Chromium app (e.g. Claude Desktop) drops its field's first
   * responder — the focus loss this fixes — and skipping also avoids a needless
   * activation round-trip. Otherwise we activate and poll until the OS reports the
   * target frontmost.
   */
  async activateTargetPid() {
    if (!this.lastTargetPid) return false;
    return this.activatePid(this.lastTargetPid);
  }

  async activatePid(pid) {
    if (!pid) return false;
    if ((await this._readFrontmostPid()) === pid) return true;

    await this._activateApp(pid);
    for (let i = 0; i < ACTIVATE_CONFIRM_RETRIES; i++) {
      await new Promise((resolve) => setTimeout(resolve, ACTIVATE_CONFIRM_DELAY_MS));
      if ((await this._readFrontmostPid()) === pid) {
        debugLogger.debug("[TextEditMonitor] Activated target PID", { pid });
        return true;
      }
    }
    debugLogger.debug("[TextEditMonitor] Target did not become frontmost", { pid });
    return false;
  }

  async canPasteAtTarget(pid, timeoutMs = 700) {
    // Without capture metadata or a working probe, delivery is unconfirmed.
    // Dictation keeps the transcript available for manual copy in this case.
    if (!pid) return null;
    const resolved = this.resolveBinary();
    if (!resolved) return null;

    return this._probeTarget(
      resolved.command,
      [...resolved.args, "--paste-target", String(pid)],
      (line) => (line === "PASTEABLE" ? true : line === "NOT_PASTEABLE" ? false : null),
      null,
      timeoutMs
    );
  }

  _probeTarget(command, args, parseVerdict, unavailable, timeoutMs) {
    // The verdict is the first output line. A stale binary that predates the
    // probe flag falls into monitor mode instead: it blocks reading stdin,
    // then emits its monitor output and keeps running — closing stdin and
    // resolving on that first line keeps the stale case a fast fallback
    // rather than a hang until the timeout.
    return new Promise((resolve) => {
      const child = spawn(command, args, { stdio: ["pipe", "pipe", "ignore"] });
      let buffered = "";
      let settled = false;
      const settle = (verdict) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        try {
          child.kill();
        } catch {}
        resolve(verdict);
      };
      const timer = setTimeout(() => settle(unavailable), timeoutMs);
      child.stdin.on("error", () => {});
      child.stdin.end();
      child.stdout.on("data", (data) => {
        buffered += data.toString();
        const newline = buffered.indexOf("\n");
        if (newline !== -1) settle(parseVerdict(buffered.slice(0, newline).trim()));
      });
      child.on("error", () => settle(unavailable));
      child.on("close", () => settle(parseVerdict(buffered.trim())));
    });
  }

  /**
   * macOS: the target app's largest on-screen window rect, used to decide which
   * display the user is working on. Resolves to null when the app has no
   * ordinary window, leaving the caller to fall back to the
   * cursor. Cached over the same press-time burst as captureTargetPid, so the
   * dictation panel and the screen-context capture share one spawn.
   */
  async getTargetWindowBounds(pid, timeoutMs = 700) {
    if (!pid) return null;
    if (
      this._windowBounds?.pid === pid &&
      Date.now() - this._windowBounds.at < TARGET_CAPTURE_FRESHNESS_MS
    ) {
      return this._windowBounds.bounds;
    }

    const resolved = this.resolveBinary();
    if (!resolved) return null;

    const bounds = await new Promise((resolve) => {
      execFile(
        resolved.command,
        [...resolved.args, "--window-bounds", String(pid)],
        { timeout: timeoutMs },
        (error, stdout) => {
          const match = stdout?.match(/^BOUNDS:(-?\d+),(-?\d+),(\d+),(\d+)$/m);
          if (!match) {
            resolve(null);
            return;
          }
          resolve({
            x: Number(match[1]),
            y: Number(match[2]),
            width: Number(match[3]),
            height: Number(match[4]),
          });
        }
      );
    });

    this._windowBounds = { pid, bounds, at: Date.now() };
    return bounds;
  }

  /**
   * Start monitoring the focused text field for edits after a paste.
   * Kills any existing monitor before starting a new one.
   * @param {string} originalText - The transcribed text that was pasted
   * @param {number} timeoutMs - How long to monitor (default 30s)
   */
  startMonitoring(originalText, timeoutMs = 30000, options = {}) {
    this.stopMonitoring();
    this.currentOriginalText = originalText;

    const resolved = this.resolveBinary();
    if (resolved) {
      this._startMacOSNative(originalText, timeoutMs, options.targetPid, resolved);
      return;
    }
    this._startMacOSPolling(originalText, timeoutMs, options.targetPid);
  }

  stopMonitoring() {
    if (this.timeout) {
      clearTimeout(this.timeout);
      this.timeout = null;
    }
    if (this._pollInterval) {
      clearInterval(this._pollInterval);
      this._pollInterval = null;
    }
    this._lastValue = null;
    this._stdoutBuffer = "";
    if (this.process) {
      try {
        this.process.kill();
      } catch {
        // ignore
      }
      this.process = null;
    }
    this.currentOriginalText = null;
  }

  _handleProcessStdoutChunk(chunk) {
    this._stdoutBuffer += chunk;
    const lines = this._stdoutBuffer.split(/\r?\n/);
    this._stdoutBuffer = lines.pop() || "";

    for (const rawLine of lines) {
      if (!rawLine) continue;
      this._handleProcessLine(rawLine);
    }
  }

  _decodeBase64Payload(encoded) {
    try {
      return Buffer.from(encoded, "base64").toString("utf8");
    } catch (error) {
      debugLogger.debug("[TextEditMonitor] Failed to decode base64 payload", {
        error: error.message,
      });
      return null;
    }
  }

  _emitTextEdited(newFieldValue) {
    if (typeof newFieldValue !== "string" || this.currentOriginalText === null) {
      return;
    }

    debugLogger.debug("[TextEditMonitor] Text changed", {
      newFieldValue: newFieldValue.substring(0, 80),
    });
    this.emit("text-edited", {
      originalText: this.currentOriginalText,
      newFieldValue,
    });
  }

  _handleProcessLine(line) {
    if (line.startsWith("CHANGED_B64:")) {
      const decoded = this._decodeBase64Payload(line.slice("CHANGED_B64:".length));
      if (decoded !== null) {
        this._emitTextEdited(decoded);
      }
      return;
    }

    if (line.startsWith("CHANGED:")) {
      this._emitTextEdited(line.slice("CHANGED:".length));
      return;
    }

    if (line === "NO_ELEMENT" || line === "NO_VALUE") {
      debugLogger.debug("[TextEditMonitor] No target element", { status: line });
      this.stopMonitoring();
    }
  }

  /**
   * macOS: use the native Swift AXObserver binary for event-based text monitoring.
   * Falls back to osascript polling if the binary fails to start.
   */
  async _startMacOSNative(originalText, timeoutMs, targetPid, resolved) {
    if (!targetPid) {
      debugLogger.debug("[TextEditMonitor] macOS native: no target PID");
      this.stopMonitoring();
      return;
    }

    // The native monitor for these apps always burns its retries and ends in
    // NO_ELEMENT -> stopMonitoring; skip the doomed child process entirely.
    if (this._nativeSelectionUnsupportedPids.has(targetPid)) {
      debugLogger.debug(
        "[TextEditMonitor] macOS native: AX reads unsupported for target this session, skipping monitor",
        { targetPid }
      );
      this.stopMonitoring();
      return;
    }

    debugLogger.debug("[TextEditMonitor] macOS native: starting", {
      targetPid,
      textPreview: originalText.substring(0, 80),
    });

    await new Promise((r) => setTimeout(r, INITIAL_QUERY_DELAY_MS));
    if (this.currentOriginalText === null) return;

    const { command, args } = resolved;

    try {
      fs.accessSync(command, fs.constants.X_OK);
    } catch {
      debugLogger.debug(
        "[TextEditMonitor] macOS native: binary not executable, falling back to polling",
        { command }
      );
      this._startMacOSPolling(originalText, timeoutMs, targetPid);
      return;
    }

    // Per-child verdict state, so overlapping monitor runs can never cache
    // each other's target.
    const axRun = { stderr: "", noElement: false };
    this.process = spawn(command, [...args, String(targetPid)], {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });

    this.process.stdin.write(originalText + "\n");
    this.process.stdin.end();

    this._stdoutBuffer = "";
    this.process.stdout.setEncoding("utf8");
    this.process.stdout.on("data", (chunk) => {
      debugLogger.debug("[TextEditMonitor] stdout", { data: chunk.trim() });
      if (chunk.includes("NO_ELEMENT")) axRun.noElement = true;
      this._handleProcessStdoutChunk(chunk);
    });

    this.process.stderr.setEncoding("utf8");
    this.process.stderr.on("data", (data) => {
      debugLogger.debug("[TextEditMonitor] stderr", { data: data.trim() });
      axRun.stderr += data;
    });

    this.process.on("error", (err) => {
      debugLogger.debug("[TextEditMonitor] macOS native: process error, falling back to polling", {
        error: err.message,
      });
      this.process = null;
      if (this.currentOriginalText === null) return;
      this._startMacOSPolling(originalText, timeoutMs, targetPid);
    });

    this.process.on("exit", (code, signal) => {
      debugLogger.debug("[TextEditMonitor] Process exited", { code, signal });
      this.process = null;
    });

    // "close" fires after both stdio streams have flushed. The monitor child
    // runs the full retry ladder with no exec timeout, so NO_ELEMENT after a
    // uniform -25212 run is a terminal verdict — teach the cache so plain
    // dictation stops spawning doomed monitors.
    this.process.on("close", () => {
      if (axRun.noElement && isPersistentAxNoValueFailure(axRun.stderr)) {
        this._nativeSelectionUnsupportedPids.add(targetPid);
      }
    });

    this.timeout = setTimeout(() => this.stopMonitoring(), timeoutMs);
  }

  /**
   * macOS: query the focused text field value via osascript for a specific app PID.
   * Returns the field value string, or null on error.
   */
  _queryMacOSValue(pid) {
    return new Promise((resolve) => {
      const script = MACOS_AX_SCRIPT_BY_PID(pid);
      execFile("osascript", ["-e", script], { timeout: 3000 }, (err, stdout) => {
        if (err) {
          resolve(null);
        } else {
          resolve(stdout.replace(/\n$/, ""));
        }
      });
    });
  }

  /**
   * macOS: poll the focused text field for changes using osascript.
   * Uses Apple-signed osascript binary which inherits accessibility trust.
   * @param {string} originalText - The pasted text
   * @param {number} timeoutMs - Monitoring timeout
   * @param {number|null} targetPid - PID of the app that received the paste
   */
  _startMacOSPolling(originalText, timeoutMs, targetPid) {
    if (!targetPid) {
      debugLogger.debug("[TextEditMonitor] macOS: no target PID");
      this.stopMonitoring();
      return;
    }

    debugLogger.debug("[TextEditMonitor] macOS: starting osascript polling", {
      targetPid,
      textPreview: originalText.substring(0, 80),
    });

    // Delay before querying to let the paste keystroke be processed.
    setTimeout(
      () => this._queryInitialValue(targetPid, originalText, timeoutMs),
      INITIAL_QUERY_DELAY_MS
    );
  }

  /**
   * Query the initial AXValue with retries. The target app may not have processed
   * the pasted text yet, so an empty value is retried a few times before giving up.
   */
  async _queryInitialValue(targetPid, originalText, timeoutMs, attempt = 1) {
    // Guard against stopMonitoring() being called while we waited
    if (this.currentOriginalText === null) return;

    const initialValue = await this._queryMacOSValue(targetPid);
    if (this.currentOriginalText === null) return;

    if (initialValue === null) {
      debugLogger.debug("[TextEditMonitor] macOS: no focused element");
      this.stopMonitoring();
      return;
    }

    if (!initialValue) {
      if (attempt < INITIAL_QUERY_RETRIES) {
        debugLogger.debug("[TextEditMonitor] macOS: AXValue empty, retrying", {
          attempt,
          maxRetries: INITIAL_QUERY_RETRIES,
        });
        setTimeout(
          () => this._queryInitialValue(targetPid, originalText, timeoutMs, attempt + 1),
          INITIAL_QUERY_RETRY_DELAY_MS
        );
        return;
      }
      debugLogger.debug("[TextEditMonitor] macOS: no text value after retries");
      this.stopMonitoring();
      return;
    }

    this._lastValue = initialValue;
    debugLogger.debug("[TextEditMonitor] macOS: initial value", {
      valuePreview: initialValue.substring(0, 80),
      attempt,
    });

    this._pollInterval = setInterval(async () => {
      const currentValue = await this._queryMacOSValue(targetPid);
      // Guard against stopMonitoring() being called during the query
      if (this.currentOriginalText === null) return;

      if (currentValue === null) {
        debugLogger.debug("[TextEditMonitor] macOS: lost focused element");
        this.stopMonitoring();
        return;
      }

      if (currentValue !== this._lastValue) {
        this._lastValue = currentValue;
        debugLogger.debug("[TextEditMonitor] macOS: text changed", {
          newValuePreview: currentValue.substring(0, 80),
        });
        this.emit("text-edited", {
          originalText: this.currentOriginalText,
          newFieldValue: currentValue,
        });
      }
    }, POLL_INTERVAL_MS);

    this.timeout = setTimeout(() => this.stopMonitoring(), timeoutMs);
  }

  /**
   * Resolve the native monitor binary.
   * Returns { command, args } or null if unavailable.
   */
  resolveBinary() {
    const nativePath = this._findFile("macos-text-monitor");
    return nativePath ? { command: nativePath, args: [] } : null; // PID added at spawn time
  }

  _findFile(fileName) {
    const candidates = new Set([
      path.join(__dirname, "..", "..", "resources", "bin", fileName),
      path.join(__dirname, "..", "..", "resources", fileName),
    ]);

    if (process.resourcesPath) {
      [
        path.join(process.resourcesPath, fileName),
        path.join(process.resourcesPath, "bin", fileName),
        path.join(process.resourcesPath, "resources", fileName),
        path.join(process.resourcesPath, "resources", "bin", fileName),
        path.join(process.resourcesPath, "app.asar.unpacked", "resources", fileName),
        path.join(process.resourcesPath, "app.asar.unpacked", "resources", "bin", fileName),
      ].forEach((c) => candidates.add(c));
    }

    for (const candidate of candidates) {
      try {
        if (fs.statSync(candidate).isFile()) return candidate;
      } catch {
        continue;
      }
    }

    return null;
  }
}

module.exports = TextEditMonitor;
