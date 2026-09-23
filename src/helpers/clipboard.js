const { clipboard, systemPreferences } = require("electron");
const { spawn } = require("child_process");
const { killProcess } = require("../utils/process");
const path = require("path");
const fs = require("fs");
const debugLogger = require("./debugLogger");

// isTrustedAccessibilityClient() is a cheap synchronous syscall, so the cache
// only exists to debounce the dialog shown on denial.
const ACCESSIBILITY_CHECK_TTL_MS = 5000;

const PASTE_DELAY_MS = 120;
const RESTORE_DELAY_MS = 450;

class ClipboardManager {
  constructor() {
    this.accessibilityCache = { value: null, expiresAt: 0 };
    this.fastPastePath = null;
    this.fastPasteChecked = false;
    this.pasteQueue = Promise.resolve();
  }

  _resolveNativeBinary(binaryName, cacheKeyChecked, cacheKeyPath) {
    if (this[cacheKeyChecked]) {
      return this[cacheKeyPath];
    }
    this[cacheKeyChecked] = true;

    const candidates = new Set([
      path.join(__dirname, "..", "..", "resources", "bin", binaryName),
      path.join(__dirname, "..", "..", "resources", binaryName),
    ]);

    if (process.resourcesPath) {
      [
        path.join(process.resourcesPath, binaryName),
        path.join(process.resourcesPath, "bin", binaryName),
        path.join(process.resourcesPath, "resources", binaryName),
        path.join(process.resourcesPath, "resources", "bin", binaryName),
        path.join(process.resourcesPath, "app.asar.unpacked", "resources", binaryName),
        path.join(process.resourcesPath, "app.asar.unpacked", "resources", "bin", binaryName),
      ].forEach((candidate) => candidates.add(candidate));
    }

    for (const candidate of candidates) {
      try {
        const stats = fs.statSync(candidate);
        if (stats.isFile()) {
          try {
            fs.accessSync(candidate, fs.constants.X_OK);
          } catch {
            fs.chmodSync(candidate, 0o755);
          }
          this[cacheKeyPath] = candidate;
          return candidate;
        }
      } catch {
        continue;
      }
    }

    return null;
  }

  resolveFastPasteBinary() {
    return this._resolveNativeBinary("macos-fast-paste", "fastPasteChecked", "fastPastePath");
  }

  _saveClipboard() {
    const formats = clipboard.availableFormats();
    const data = {};

    const text = clipboard.readText();
    if (text) data.text = text;

    if (formats.includes("text/html")) {
      const html = clipboard.readHTML();
      if (html) data.html = html;
    }

    if (formats.includes("text/rtf") || formats.includes("public.rtf")) {
      const rtf = clipboard.readRTF();
      if (rtf) data.rtf = rtf;
    }

    if (formats.some((f) => f.startsWith("image/"))) {
      const image = clipboard.readImage();
      if (image && !image.isEmpty()) data.image = image;
    }

    const keys = Object.keys(data);
    if (keys.length === 1 && keys[0] === "image") {
      return { type: "image", data: data.image };
    }
    if (keys.length === 1 && keys[0] === "text") {
      return { type: "text", data: data.text };
    }
    if (keys.length > 0) return { type: "formats", data };

    return { type: "text", data: text };
  }

  _restoreClipboard(original) {
    if (!original) return;
    if (original.type === "formats") {
      clipboard.write(original.data);
    } else if (original.type === "image") {
      clipboard.writeImage(original.data);
    } else {
      clipboard.writeText(original.data);
    }
    this.safeLog("🔄 Clipboard restored");
  }

  async _restoreClipboardAfterDelay(original, { delayMs, expectedText, restore } = {}) {
    if (!original) return;
    await new Promise((resolve) => setTimeout(resolve, delayMs));

    if (typeof expectedText === "string") {
      let currentText = null;
      try {
        currentText = clipboard.readText();
      } catch {}

      if (currentText !== expectedText) {
        debugLogger.debug(
          "Skipping clipboard restore because clipboard changed",
          {
            expectedLength: expectedText.length,
            currentLength: typeof currentText === "string" ? currentText.length : null,
          },
          "clipboard"
        );
        return;
      }
    }

    if (restore) {
      restore();
      return;
    }

    this._restoreClipboard(original);
  }

  safeLog(...args) {
    if (process.env.NODE_ENV === "development") {
      try {
        console.log(...args);
      } catch (error) {
        // Silently ignore EPIPE errors in logging
        if (error.code !== "EPIPE") {
          process.stderr.write(`Log error: ${error.message}\n`);
        }
      }
    }
  }

  async pasteText(text, options = {}) {
    return this._runClipboardOperation(
      () => this._pasteText(text, options),
      (result) => result?.restoreComplete
    );
  }

  async runClipboardOperation(operation) {
    return this._runClipboardOperation(operation);
  }

  async _runClipboardOperation(operation, completionForResult = null) {
    const previousPaste = this.pasteQueue.catch(() => {});
    let markRestoreComplete;
    const restoreGate = new Promise((resolve) => {
      markRestoreComplete = resolve;
    });

    this.pasteQueue = previousPaste.then(() => restoreGate).catch(() => {});
    await previousPaste;

    try {
      const result = await operation();
      const completion = completionForResult ? completionForResult(result) : null;
      Promise.resolve(completion).then(markRestoreComplete, markRestoreComplete);
      return result;
    } catch (error) {
      markRestoreComplete();
      throw error;
    }
  }

  async _pasteText(text, options = {}) {
    const startTime = Date.now();
    let method = "unknown";
    const allowClipboardFallback = options.allowClipboardFallback === true;

    try {
      const shouldRestore = options.restoreClipboard !== false;
      const originalClipboard = shouldRestore ? this._saveClipboard() : null;
      if (shouldRestore) {
        this.safeLog("💾 Saved original clipboard:", originalClipboard.type);
      }

      clipboard.writeText(text);
      this.safeLog("📋 Text copied to clipboard:", text.substring(0, 50) + "...");

      method = this.resolveFastPasteBinary() ? "cgevent" : "applescript";
      this.safeLog("🔍 Checking accessibility permissions for paste operation...");
      const hasPermissions = await this.checkAccessibilityPermissions(allowClipboardFallback);

      if (!hasPermissions) {
        this.safeLog("⚠️ No accessibility permissions - text copied to clipboard only");
        if (allowClipboardFallback) {
          this.safeLog("✅ Clipboard fallback used (manual paste required)");
          return { restoreComplete: Promise.resolve(), pasted: false };
        }
        const errorMsg =
          "Accessibility permissions required for automatic pasting. Text has been copied to clipboard - please paste manually with Cmd+V.";
        throw new Error(errorMsg);
      }

      // Probe after writing the transcript so target apps validate their
      // Paste menu against this clipboard. A requested probe must confirm
      // the target; posting Command-V alone does not establish delivery.
      let canPaste = null;
      try {
        canPaste = (await options.checkPasteTarget?.()) ?? null;
      } catch (error) {
        this.safeLog("Paste target check unavailable", { error: error.message });
      }
      if (options.checkPasteTarget && canPaste !== true) {
        this.safeLog("Paste target not confirmed; keeping transcription in clipboard");
        return { restoreComplete: Promise.resolve(), pasted: false };
      }

      this.safeLog("✅ Permissions granted, attempting to paste...");
      let pasteResult;
      try {
        pasteResult = await this.pasteMacOS(originalClipboard, {
          ...options,
          expectedClipboardText: text,
        });
      } catch (firstError) {
        this.safeLog("⚠️ First paste attempt failed, retrying...", firstError?.message);
        clipboard.writeText(text);
        await new Promise((r) => setTimeout(r, 200));
        pasteResult = await this.pasteMacOS(originalClipboard, {
          ...options,
          expectedClipboardText: text,
        });
      }

      this.safeLog("✅ Paste operation complete", {
        method,
        elapsedMs: Date.now() - startTime,
        textLength: text.length,
      });
      return pasteResult || { restoreComplete: Promise.resolve() };
    } catch (error) {
      this.safeLog("❌ Paste operation failed", {
        method,
        elapsedMs: Date.now() - startTime,
        error: error.message,
      });
      throw error;
    }
  }

  async pasteMacOS(originalClipboard, options = {}) {
    const fastPasteBinary = this.resolveFastPasteBinary();
    const useFastPaste = !!fastPasteBinary;
    const pasteDelay = PASTE_DELAY_MS;

    return new Promise((resolve, reject) => {
      setTimeout(() => {
        const pasteProcess = useFastPaste
          ? spawn(fastPasteBinary)
          : spawn("osascript", [
              "-e",
              'tell application "System Events" to key code 9 using command down',
            ]);

        let errorOutput = "";
        let hasTimedOut = false;

        pasteProcess.stderr.on("data", (data) => {
          errorOutput += data.toString();
        });

        pasteProcess.on("close", (code) => {
          if (hasTimedOut) return;
          clearTimeout(timeoutId);
          pasteProcess.removeAllListeners();

          if (code === 0) {
            this.safeLog(`Text pasted successfully via ${useFastPaste ? "CGEvent" : "osascript"}`);
            if (originalClipboard != null) {
              resolve({
                restoreComplete: this._restoreClipboardAfterDelay(originalClipboard, {
                  delayMs: RESTORE_DELAY_MS,
                  expectedText: options.expectedClipboardText,
                }),
              });
            } else {
              resolve({ restoreComplete: Promise.resolve() });
            }
          } else if (useFastPaste && code === 3) {
            this.safeLog("CGEvent paste could not resolve the active keyboard layout", {
              stderr: errorOutput.trim(),
            });
            reject(
              new Error(
                "Paste could not resolve the active keyboard layout. Text is copied to clipboard - please paste manually with Cmd+V."
              )
            );
          } else if (useFastPaste) {
            this.safeLog(
              code === 2
                ? "CGEvent binary lacks accessibility trust, falling back to osascript"
                : `CGEvent paste failed (code ${code}), falling back to osascript`,
              { stderr: errorOutput.trim() }
            );
            this.fastPasteChecked = true;
            this.fastPastePath = null;
            this.pasteMacOSWithOsascript(originalClipboard, options).then(resolve).catch(reject);
          } else {
            this.accessibilityCache = { value: null, expiresAt: 0 };
            const stderr = errorOutput.trim();
            const errorMsg = `Paste failed (code ${code}${stderr ? `: ${stderr}` : ""}). Text is copied to clipboard - please paste manually with Cmd+V.`;
            reject(new Error(errorMsg));
          }
        });

        pasteProcess.on("error", (error) => {
          if (hasTimedOut) return;
          clearTimeout(timeoutId);
          pasteProcess.removeAllListeners();

          if (useFastPaste) {
            this.safeLog("CGEvent paste error, falling back to osascript");
            this.fastPasteChecked = true;
            this.fastPastePath = null;
            this.pasteMacOSWithOsascript(originalClipboard, options).then(resolve).catch(reject);
          } else {
            const errorMsg = `Paste command failed: ${error.message}. Text is copied to clipboard - please paste manually with Cmd+V.`;
            reject(new Error(errorMsg));
          }
        });

        const timeoutId = setTimeout(() => {
          hasTimedOut = true;
          killProcess(pasteProcess, "SIGKILL");
          pasteProcess.removeAllListeners();
          const errorMsg =
            "Paste operation timed out. Text is copied to clipboard - please paste manually with Cmd+V.";
          reject(new Error(errorMsg));
        }, 3000);
      }, pasteDelay);
    });
  }

  async pasteMacOSWithOsascript(originalClipboard, options = {}) {
    return new Promise((resolve, reject) => {
      const pasteProcess = spawn("osascript", [
        "-e",
        'tell application "System Events" to key code 9 using command down',
      ]);

      let hasTimedOut = false;

      pasteProcess.on("close", (code) => {
        if (hasTimedOut) return;
        clearTimeout(timeoutId);
        pasteProcess.removeAllListeners();

        if (code === 0) {
          this.safeLog("Text pasted successfully via osascript fallback");
          if (originalClipboard != null) {
            resolve({
              restoreComplete: this._restoreClipboardAfterDelay(originalClipboard, {
                delayMs: RESTORE_DELAY_MS,
                expectedText: options.expectedClipboardText,
              }),
            });
          } else {
            resolve({ restoreComplete: Promise.resolve() });
          }
        } else {
          this.accessibilityCache = { value: null, expiresAt: 0 };
          const errorMsg = `Paste failed (code ${code}). Text is copied to clipboard - please paste manually with Cmd+V.`;
          reject(new Error(errorMsg));
        }
      });

      pasteProcess.on("error", (error) => {
        if (hasTimedOut) return;
        clearTimeout(timeoutId);
        pasteProcess.removeAllListeners();
        const errorMsg = `Paste command failed: ${error.message}. Text is copied to clipboard - please paste manually with Cmd+V.`;
        reject(new Error(errorMsg));
      });

      const timeoutId = setTimeout(() => {
        hasTimedOut = true;
        killProcess(pasteProcess, "SIGKILL");
        pasteProcess.removeAllListeners();
        reject(
          new Error(
            "Paste operation timed out. Text is copied to clipboard - please paste manually with Cmd+V."
          )
        );
      }, 3000);
    });
  }

  async checkAccessibilityPermissions(silent = false) {
    if (!silent) {
      const now = Date.now();
      if (now < this.accessibilityCache.expiresAt && this.accessibilityCache.value !== null) {
        return this.accessibilityCache.value;
      }
    }

    const allowed = systemPreferences.isTrustedAccessibilityClient(false);

    if (!silent) {
      this.accessibilityCache = {
        value: allowed,
        expiresAt: Date.now() + ACCESSIBILITY_CHECK_TTL_MS,
      };

      if (!allowed) {
        this.showAccessibilityDialog("not allowed assistive access");
      }
    }

    return allowed;
  }

  showAccessibilityDialog(testError) {
    const isStuckPermission =
      testError.includes("not allowed assistive access") ||
      testError.includes("(-1719)") ||
      testError.includes("(-25006)");

    let dialogMessage;
    if (isStuckPermission) {
      dialogMessage = `🔒 whisper needs Accessibility permissions, but it looks like you may have OLD PERMISSIONS from a previous version.

❗ COMMON ISSUE: If you've rebuilt/reinstalled whisper, the old permissions may be "stuck" and preventing new ones.

🔧 To fix this:
1. Open System Settings → Privacy & Security → Accessibility
2. Look for ANY old "whisper" entries and REMOVE them (click the - button)
3. Also remove any entries that say "Electron" or have unclear names
4. Click the + button and manually add the NEW whisper app
5. Make sure the checkbox is enabled
6. Restart whisper

⚠️ This is especially common during development when rebuilding the app.

📝 Without this permission, text will only copy to clipboard (no automatic pasting).

Would you like to open System Settings now?`;
    } else {
      dialogMessage = `🔒 whisper needs Accessibility permissions to paste text into other applications.

📋 Current status: Clipboard copy works, but pasting (Cmd+V simulation) fails.

🔧 To fix this:
1. Open System Settings (or System Preferences on older macOS)
2. Go to Privacy & Security → Accessibility
3. Click the lock icon and enter your password
4. Add whisper to the list and check the box
5. Restart whisper

⚠️ Without this permission, dictated text will only be copied to clipboard but won't paste automatically.

💡 In production builds, this permission is required for full functionality.

Would you like to open System Settings now?`;
    }

    const permissionDialog = spawn("osascript", [
      "-e",
      `display dialog "${dialogMessage}" buttons {"Cancel", "Open System Settings"} default button "Open System Settings"`,
    ]);

    permissionDialog.on("close", (dialogCode) => {
      if (dialogCode === 0) {
        this.openSystemSettings();
      }
    });

    permissionDialog.on("error", () => {});
  }

  openSystemSettings() {
    const settingsCommands = [
      ["open", ["x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"]],
      ["open", ["-b", "com.apple.systempreferences"]],
      ["open", ["/System/Library/PreferencePanes/Security.prefPane"]],
    ];

    let commandIndex = 0;
    const tryNextCommand = () => {
      if (commandIndex < settingsCommands.length) {
        const [cmd, args] = settingsCommands[commandIndex];
        const settingsProcess = spawn(cmd, args);

        settingsProcess.on("error", (error) => {
          commandIndex++;
          tryNextCommand();
        });

        settingsProcess.on("close", (settingsCode) => {
          if (settingsCode !== 0) {
            commandIndex++;
            tryNextCommand();
          }
        });
      } else {
        spawn("open", ["-a", "System Preferences"]).on("error", () => {
          spawn("open", ["-a", "System Settings"]).on("error", () => {});
        });
      }
    };

    tryNextCommand();
  }

  preWarmAccessibility() {
    this.checkAccessibilityPermissions(true).catch(() => {});
    this.resolveFastPasteBinary();
  }

  async readClipboard() {
    return clipboard.readText();
  }

  async writeClipboard(text) {
    clipboard.writeText(text);
    return { success: true };
  }

  checkPasteTools() {
    const fastPaste = this.resolveFastPasteBinary();
    return {
      platform: "darwin",
      available: true,
      method: fastPaste ? "cgevent" : "applescript",
      requiresPermission: true,
      tools: [],
    };
  }
}

module.exports = ClipboardManager;
