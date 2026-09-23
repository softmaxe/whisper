const crypto = require("crypto");
const { spawn } = require("child_process");
const debugLogger = require("./debugLogger");

const SESSION_TTL_MS = 5 * 60 * 1000;
const MAX_SELECTION_EDIT_CODE_POINTS = 6000;
// Ceiling for one synthetic-copy round trip. A no-selection agent command
// pays this in full, and replaceSelectedText spends a second round trip
// re-verifying — the accepted cost of failing closed rather than pasting blind.
const COPY_TIMEOUT_MS = 1200;
const CLIPBOARD_POLL_MS = 20;

// Editors that copy the whole current line when Ctrl+C (⌘C on macOS) lands with
// an empty selection (VS Code's editor.emptySelectionClipboard, Scintilla,
// JetBrains, Visual Studio), making a bare caret look like a selection to the
// synthetic-copy capture. Matched against the localized app name, so the
// JetBrains IDEs need both spellings.
const LINE_COPY_EDITOR_SIGNATURES = [
  "code", // VS Code and forks (VSCodium, code-oss)
  "cursor",
  "windsurf",
  "notepad++",
  "sublime",
  "jetbrains",
  "idea64",
  "intellij", // macOS app name for idea64
  "pycharm",
  "webstorm",
  "phpstorm",
  "rider64",
  "rider", // macOS app name for rider64
  "android studio", // macOS app name for studio64
  "clion",
  "goland",
  "rubymine",
  "datagrip",
  "dataspell",
  "studio64", // Android Studio
  "devenv", // Visual Studio
];

function runSpawn(command, args, options = {}) {
  return new Promise((resolve) => {
    const child = spawn(command, args, {
      stdio: ["ignore", "pipe", "pipe"],
      ...options,
    });
    let stdout = "";
    let stderr = "";
    let settled = false;
    const finish = (success) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({ success, stdout, stderr });
    };
    child.stdout?.on("data", (chunk) => (stdout += chunk.toString()));
    child.stderr?.on("data", (chunk) => (stderr += chunk.toString()));
    child.on("error", (error) => {
      stderr += error.message;
      finish(false);
    });
    child.on("close", (code) => finish(code === 0));
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      finish(false);
    }, options.timeout || COPY_TIMEOUT_MS);
  });
}

class SelectionManager {
  constructor({ clipboardManager, textEditMonitor, now = Date.now } = {}) {
    this.clipboardManager = clipboardManager;
    this.textEditMonitor = textEditMonitor;
    this.now = now;
    this.sessions = new Map();
  }

  async captureSelectedText(options = {}) {
    // The caller knows whether a caret capture could ever be used (auto-paste
    // on); without it the probe's binary spawn would be pure waste.
    const probeEditable = options.probeEditable === true;
    return this.clipboardManager.runClipboardOperation(async () => {
      this._pruneSessions();
      const expectedTarget = this.textEditMonitor?.lastTargetPid
        ? { kind: "mac-pid", pid: this.textEditMonitor.lastTargetPid }
        : null;
      if (!expectedTarget) {
        return { status: "unavailable", code: "target_unavailable" };
      }
      const capture = await this._readCurrentSelection(expectedTarget, { probeEditable });
      if (capture.status === "editable") {
        const sessionId = crypto.randomUUID();
        this.sessions.set(sessionId, {
          kind: "caret",
          target: capture.target,
          expiresAt: this.now() + SESSION_TTL_MS,
        });
        return { status: "editable", sessionId };
      }
      if (capture.status !== "selected") return capture;

      const characterCount = [...capture.text].length;
      if (characterCount > MAX_SELECTION_EDIT_CODE_POINTS) {
        return {
          status: "too_large",
          characterCount,
          maxCharacters: MAX_SELECTION_EDIT_CODE_POINTS,
        };
      }

      const sessionId = crypto.randomUUID();
      this.sessions.set(sessionId, {
        kind: "selection",
        text: capture.text,
        target: capture.target,
        expiresAt: this.now() + SESSION_TTL_MS,
      });
      return {
        status: "selected",
        sessionId,
        text: capture.text,
        characterCount,
      };
    });
  }

  async replaceSelectedText(sessionId, replacement, options = {}) {
    if (typeof replacement !== "string" || replacement.length === 0) {
      return { success: false, code: "invalid_replacement" };
    }

    return this.clipboardManager.runClipboardOperation(async () => {
      this._pruneSessions();
      const session = this.sessions.get(sessionId);
      this.sessions.delete(sessionId);
      if (!session || session.kind !== "selection") {
        return { success: false, code: "session_expired" };
      }

      const current = await this._readCurrentSelection(session.target, { activate: true });
      if (current.status === "target_changed") {
        return { success: false, code: "target_changed" };
      }
      if (current.status === "unavailable") {
        return { success: false, code: "selection_unavailable" };
      }
      if (current.status !== "selected" || current.text !== session.text) {
        return { success: false, code: "selection_changed" };
      }

      try {
        const pasteResult = await this.clipboardManager._pasteText(replacement, {
          ...options,
          restoreClipboard: options.restoreClipboard !== false,
        });
        await pasteResult?.restoreComplete;
        if (pasteResult?.pasted === false) {
          return { success: false, code: "paste_failed" };
        }
        return { success: true };
      } catch (error) {
        debugLogger.warn(
          "Selection replacement paste failed",
          { error: error.message },
          "clipboard"
        );
        return { success: false, code: "paste_failed", error: error.message };
      }
    });
  }

  async pasteAtCapturedTarget(sessionId, text, options = {}) {
    if (typeof text !== "string" || text.length === 0) {
      return { success: false, code: "invalid_replacement" };
    }

    return this.clipboardManager.runClipboardOperation(async () => {
      this._pruneSessions();
      const session = this.sessions.get(sessionId);
      this.sessions.delete(sessionId);
      if (!session || session.kind !== "caret") {
        return { success: false, code: "session_expired" };
      }

      const current = await this._readCurrentSelection(session.target, { probeEditable: true });
      if (current.status !== "editable") {
        return { success: false, code: "target_changed" };
      }

      try {
        const pasteResult = await this.clipboardManager._pasteText(text, {
          ...options,
          restoreClipboard: options.restoreClipboard !== false,
        });
        await pasteResult?.restoreComplete;
        if (pasteResult?.pasted === false) {
          return { success: false, code: "paste_failed" };
        }
        return { success: true };
      } catch (error) {
        debugLogger.warn("Assistant response paste failed", { error: error.message }, "clipboard");
        return { success: false, code: "paste_failed", error: error.message };
      }
    });
  }

  _pruneSessions() {
    const now = this.now();
    for (const [id, session] of this.sessions) {
      if (session.expiresAt <= now) this.sessions.delete(id);
    }
  }

  async _readCurrentSelection(expectedTarget = null, options = {}) {
    return this._readMacSelection(expectedTarget, options);
  }

  async _readMacSelection(expectedTarget, { activate = false, probeEditable = false } = {}) {
    const pid = expectedTarget?.pid || this.textEditMonitor?.lastTargetPid;
    if (!pid || !this.textEditMonitor?.getSelectedText) {
      return { status: "unavailable", code: "target_unavailable" };
    }
    const frontmostPid = await this.textEditMonitor._readFrontmostPid?.();
    if (frontmostPid && frontmostPid !== pid) {
      return { status: "target_changed" };
    }
    if (activate && !(await this.textEditMonitor.activatePid(pid))) {
      return { status: "unavailable", code: "activation_failed" };
    }

    const result = await this.textEditMonitor.getSelectedText(pid);
    if (result.state === "selected") {
      return { status: "selected", text: result.text, target: { kind: "mac-pid", pid } };
    }
    if (result.state === "none") {
      const editable = probeEditable && result.editable && !(await this._isTerminalPid(pid));
      return {
        status: editable ? "editable" : "none",
        target: { kind: "mac-pid", pid },
      };
    }
    // Chromium browsers (and any app whose accessibility tree stays dormant)
    // never resolve a focused element, so the read above cannot tell a selection
    // from an empty field. A synthetic copy still can.
    return this._readMacSelectionViaClipboard(pid, expectedTarget, probeEditable);
  }

  async _readMacSelectionViaClipboard(pid, expectedTarget, probeEditable) {
    const binary = this.clipboardManager.resolveFastPasteBinary?.();
    if (!binary) return { status: "unavailable", code: "accessibility_unavailable" };

    const capture = await this._captureViaClipboard(
      async () => {
        const result = await this._runCopyHelper(binary, ["--copy"]);
        const match = result.stdout.match(/COPY_OK\s+(\d+)\s*(.*)/);
        if (!result.success || !match) return { success: false };
        return {
          success: true,
          target: { kind: "mac-pid", pid: Number(match[1]), appName: match[2].trim() || null },
        };
      },
      expectedTarget || { kind: "mac-pid", pid }
    );

    // A copy that never landed leaves the selection as unknown as the
    // accessibility read did, so it gets the same non-fatal treatment.
    if (capture.status === "unavailable" && capture.code === "copy_failed") {
      return { status: "unavailable", code: "accessibility_unavailable" };
    }
    // Replacement text typed into a shell executes on its embedded newlines, so
    // terminal selections are declined. GPU-rendered
    // terminals (Ghostty, Alacritty, kitty) have no accessibility tree, so this
    // path is the only way they reach selection editing at all.
    if (
      capture.status === "selected" &&
      this.clipboardManager.isTerminalSignature?.(capture.target?.appName)
    ) {
      return { status: "none", target: capture.target };
    }
    return this._markEditableCaret(capture, capture.target || expectedTarget, probeEditable);
  }

  _runCopyHelper(binary, args) {
    return runSpawn(binary, args, { timeout: COPY_TIMEOUT_MS });
  }

  async _markEditableCaret(capture, target, probeEditable) {
    if (!probeEditable || capture.status !== "none" || !target) return capture;
    // Generated text pasted into a shell executes on its embedded newlines, so
    // a terminal target never becomes a caret destination — the same rule the
    // selection paths apply.
    if (this._isTerminalTarget(capture.target, target)) return capture;
    const editable = await this.textEditMonitor?.isFocusedEditable?.(target);
    return editable ? { status: "editable", target } : capture;
  }

  _isTerminalTarget(...targets) {
    return targets.some(
      (target) =>
        target?.isTerminal === true ||
        this.clipboardManager.isTerminalSignature?.(this._targetSignature(target))
    );
  }

  // macOS AX targets carry only a pid; resolve the executable so terminal apps
  // can be recognized before their empty prompt reads as a writable caret.
  async _isTerminalPid(pid) {
    if (!this.clipboardManager.isTerminalSignature) return false;
    const executablePath = await this._readExecutablePath(pid);
    if (!executablePath) return false;
    // Match the bundle and executable names, not the whole path — segments
    // like "/System/" would collide with short signatures such as "st".
    const bundleName = executablePath.match(/\/([^/]+)\.app\//)?.[1] ?? "";
    const executableName = executablePath.split("/").pop() ?? "";
    return this.clipboardManager.isTerminalSignature(`${bundleName} ${executableName}`);
  }

  async _readExecutablePath(pid) {
    const result = await runSpawn("ps", ["-p", String(pid), "-o", "comm="], { timeout: 500 });
    return result.success ? result.stdout.trim() : "";
  }

  async _captureViaClipboard(sendCopy, expectedTarget) {
    const original = this.clipboardManager._saveClipboard();
    const beforeWrite = this.clipboardManager._readClipboardTextAll();
    const sentinel = `__OPENWHISPR_SELECTION_${crypto.randomUUID()}__`;
    this.clipboardManager._writeClipboardTextAll(sentinel);
    // Snapshot the clipboard so stale text can't be mistaken for the copied
    // selection. Known limitation: a clipboard that
    // already held exactly the selected text reads as "no selection". The
    // command then falls back to the Assistant panel — never to a caret paste,
    // because the editable probe reads the focused element's own selection
    // state and refuses a field with a live selection.
    const baseline = new Set([...beforeWrite, ...this.clipboardManager._readClipboardTextAll()]);

    const copyResult = await sendCopy();
    if (!copyResult?.success || !copyResult.target) {
      this._restoreClipboardIfOurs(original, [sentinel], baseline);
      return { status: "unavailable", code: "copy_failed" };
    }
    if (expectedTarget && !this._sameTarget(copyResult.target, expectedTarget)) {
      this._restoreClipboardIfOurs(original, [sentinel], baseline);
      return { status: "target_changed" };
    }

    const deadline = Date.now() + COPY_TIMEOUT_MS;
    let copiedText = null;
    while (Date.now() < deadline) {
      copiedText =
        this.clipboardManager
          ._readClipboardTextAll()
          .find((text) => text.length > 0 && text !== sentinel && !baseline.has(text)) ?? null;
      if (copiedText !== null) break;
      await new Promise((resolve) => setTimeout(resolve, CLIPBOARD_POLL_MS));
    }

    this._restoreClipboardIfOurs(original, [sentinel, copiedText], baseline);
    if (copiedText === null) {
      return { status: "none", target: copyResult.target };
    }
    // A line copy from an empty-selection Ctrl+C is exactly one line with a
    // trailing terminator; treat that shape from a known line-copy editor as
    // "no selection" so a bare caret never gets its line rewritten.
    if (
      /^[^\n]*\r?\n$/.test(copiedText) &&
      this._isLineCopyEditor(expectedTarget, copyResult.target)
    ) {
      return { status: "none", target: copyResult.target };
    }
    return { status: "selected", text: copiedText, target: copyResult.target };
  }

  _targetSignature(target) {
    return `${target?.exeName || ""} ${target?.windowClass || ""} ${target?.appName || ""}`.trim();
  }

  // The copy helper reports the app name, so both targets are checked.
  _isLineCopyEditor(...targets) {
    const signature = targets
      .map((target) => this._targetSignature(target))
      .join(" ")
      .toLowerCase();
    if (!signature.trim()) return false;
    return LINE_COPY_EDITOR_SIGNATURES.some((editor) => signature.includes(editor));
  }

  _restoreClipboardIfOurs(original, writtenTexts, baseline = new Set()) {
    const written = writtenTexts.filter((text) => typeof text === "string" && text.length > 0);
    try {
      const current = this.clipboardManager._readClipboardTextAll();
      const userClipboardText = current.find(
        (text) => text.length > 0 && !written.includes(text) && !baseline.has(text)
      );
      if (userClipboardText) {
        // The user copied something while capture was in flight. Prefer their
        // new clipboard over restoring our snapshot.
        this.clipboardManager._writeClipboardTextAll(userClipboardText);
        return;
      }
      if (!current.some((text) => written.includes(text))) return;
      if (original?.type === "text") {
        this.clipboardManager._writeClipboardTextAll(original.data);
      } else {
        this.clipboardManager._restoreClipboard(original);
      }
    } catch {}
  }

  _sameTarget(a, b) {
    return !!a && !!b && a.kind === b.kind && String(a.id ?? a.pid) === String(b.id ?? b.pid);
  }
}

module.exports = SelectionManager;
module.exports.SESSION_TTL_MS = SESSION_TTL_MS;
