const debugLogger = require("./debugLogger");
const { applySmartSpacing } = require("./smartSpacing");

class AutomaticPaste {
  constructor({ clipboardManager, getMainWindow, getTextEditMonitor, isAutoLearnEnabled }) {
    this.clipboardManager = clipboardManager;
    this.getMainWindow = getMainWindow;
    this.getTextEditMonitor = getTextEditMonitor;
    this.isAutoLearnEnabled = isAutoLearnEnabled;
  }

  async paste(text, options = {}) {
    const mainWindow = this.getMainWindow();
    const monitor = this.getTextEditMonitor();
    const targetPid = monitor?.lastTargetPid || null;

    // Target capture belongs to Dictation startup. Activation stays outside the
    // clipboard queue, while this request's PID survives later target captures.
    let activated = false;
    if (monitor) {
      activated = await monitor.activateTargetPid();
    }

    if (!activated && mainWindow && !mainWindow.isDestroyed() && mainWindow.isFocused()) {
      mainWindow.hide();
      await new Promise((resolve) => setTimeout(resolve, 120));
      mainWindow.showInactive();
    }

    // ClipboardManager writes before probing and holds later pastes until its
    // delayed restoration settles. Always request confirmation, even without a
    // monitor or captured target, so unconfirmed text remains available to copy.
    const pasteResult = await this.clipboardManager.pasteText(applySmartSpacing(text), {
      ...options,
      checkPasteTarget: () => this.getTextEditMonitor()?.canPasteAtTarget(targetPid) ?? null,
    });
    const pasted = pasteResult?.pasted !== false;
    debugLogger.debug("[AutoLearn] Paste completed", {
      autoLearnEnabled: this.isAutoLearnEnabled(),
      hasMonitor: !!this.getTextEditMonitor(),
      targetPid,
      pasted,
    });
    if (pasted && this.getTextEditMonitor() && this.isAutoLearnEnabled()) {
      setTimeout(() => {
        try {
          debugLogger.debug("[AutoLearn] Starting monitoring", {
            textPreview: text.substring(0, 80),
          });
          this.getTextEditMonitor().startMonitoring(text, 30000, { targetPid });
        } catch (err) {
          debugLogger.debug("[AutoLearn] Failed to start monitoring", { error: err.message });
        }
      }, 500);
    }

    // Native success means a confirmed target and a completed paste command,
    // not observed insertion or completed restoration. Older successful results
    // omit pasted; only explicit false means clipboard-only delivery. Keep the
    // restoration promise inside ClipboardManager, away from the IPC boundary.
    return { success: true, pasted };
  }
}

module.exports = AutomaticPaste;
