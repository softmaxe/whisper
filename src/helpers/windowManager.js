const { app, screen, BrowserWindow, dialog, ipcMain, shell } = require("electron");
const debugLogger = require("./debugLogger");
const { randomUUID } = require("node:crypto");
const HotkeyManager = require("./hotkeyManager");
const { isGlobeLikeHotkey } = HotkeyManager;
const DragManager = require("./dragManager");
const MainWindowPlacementCoordinator = require("./mainWindowPlacementCoordinator");
const MenuManager = require("./menuManager");
const DevServerManager = require("./devServerManager");
const { isAllowedAppNavigation, isExternalBrowserUrl } = require("./navigationGuard");
const { pathToFileURL } = require("url");
const dockManager = require("./dockManager");
const { i18nMain } = require("./i18nMain");
const {
  DICTATION_LIFECYCLE,
  normalizeDictationLifecycle,
  shouldIgnoreDictationHotkey,
  isDictationRecording,
} = require("./dictationLifecycle");
const { DEV_SERVER_PORT } = DevServerManager;
const DRAG_MOVE_TOLERANCE_PX = 2;
const {
  MAIN_WINDOW_CONFIG,
  CONTROL_PANEL_CONFIG,
  ONBOARDING_WINDOW_SIZES,
  fitAssistantContentWindowToWorkArea,
  fitDictationErrorContentWindowToWorkArea,
  fitDictationErrorWindowToWorkArea,
  resolveHorizontalWindowDirection,
  WINDOW_SIZES,
  WindowPositionUtil,
} = require("./windowConfig");
// A hidden control panel still holds a full renderer process. Keep it briefly
// so reopening right away stays instant, then release it.
const CONTROL_PANEL_RELEASE_DELAY_MS = 60_000;
const { centeredBounds, clampedBounds } = require("./onboardingWindowBounds");
const { createHotkeyRepeatGate } = require("./hotkeyRepeatGate");

class WindowManager {
  constructor() {
    this.mainWindow = null;
    this.controlPanelWindow = null;
    this._resizeMaskTokenCounter = 0;
    this._controlPanelVisibilityTimer = null;
    this._controlPanelReleaseTimer = null;
    // The control panel renderer reports work that a release would lose.
    this._controlPanelRetained = false;
    // Set while an idle release closes the panel, and until its replacement
    // loads, so neither step re-gates dictation like a real app reset.
    this._controlPanelReleased = false;
    this._onboardingRestoreBounds = null;
    this._onboardingWindowMode = null;
    this._onboardingWindowState = null;
    // Fail closed until AppRouter has resolved persisted onboarding state and
    // committed the normal app. This covers the startup gap before React mounts.
    this._onboardingActive = true;
    // Set by main.js so the tray's listen item rebuilds with dictation state.
    this.onDictationStateChanged = null;
    this.tray = null;
    this.hotkeyManager = new HotkeyManager();
    this.dragManager = new DragManager();
    this._mainWindowPlacementCoordinator = new MainWindowPlacementCoordinator();
    this.isQuitting = false;
    this.loadErrorShown = false;
    this.macCompoundPushState = null;
    this._cachedActivationMode = "tap";
    this._floatingIconAutoHide = false;
    this._panelStartPosition = "bottom-right";
    this._activeHorizontalDirection = null;
    this._isDictatingToggle = false;
    this._dictationLifecycleState = DICTATION_LIFECYCLE.IDLE;

    app.on("before-quit", () => {
      this.isQuitting = true;
      this.hotkeyManager.unregisterAll();
    });
  }

  async createMainWindow() {
    const cursorPos = screen.getCursorScreenPoint();
    const display = screen.getDisplayNearestPoint(cursorPos);
    const position = WindowPositionUtil.getMainWindowPosition(
      display,
      null,
      this._panelStartPosition
    );

    this.mainWindow = new BrowserWindow({
      ...MAIN_WINDOW_CONFIG,
      ...position,
    });

    this.setMainWindowInteractivity(false);
    this.registerMainWindowEvents();

    // Register load event handlers BEFORE loading to catch all events
    this.mainWindow.webContents.on(
      "did-fail-load",
      async (_event, errorCode, errorDescription, validatedURL, isMainFrame) => {
        if (!isMainFrame) {
          return;
        }
        if (
          process.env.NODE_ENV === "development" &&
          validatedURL &&
          validatedURL.includes(`localhost:${DEV_SERVER_PORT}`)
        ) {
          setTimeout(async () => {
            const isReady = await DevServerManager.waitForDevServer();
            if (isReady) {
              this.mainWindow.reload();
            }
          }, 2000);
        } else {
          this.showLoadFailureDialog("Dictation panel", errorCode, errorDescription, validatedURL);
        }
      }
    );

    this.mainWindow.webContents.on("did-finish-load", () => {
      // A reload has not resolved its route yet. AppRouter releases this gate
      // after it renders the normal app; fresh onboarding keeps it active.
      this.setOnboardingActive(true);
      this.mainWindow.setTitle(i18nMain.t("window.voiceRecorderTitle"));
      this.enforceMainWindowOnTop();
      this._notifyMainWindowHorizontalDirection();
    });

    await this.loadMainWindow();
    await this.initializeHotkey();
    this.dragManager.setTargetWindow(this.mainWindow);
    MenuManager.setupMainMenu(() => this.openSettings());
  }

  setMainWindowInteractivity(shouldCapture) {
    if (!this.mainWindow || this.mainWindow.isDestroyed()) {
      return;
    }

    if (shouldCapture) {
      this.mainWindow.setIgnoreMouseEvents(false);
    } else {
      this.mainWindow.setIgnoreMouseEvents(true, { forward: true });
    }
  }

  resizeMainWindow(sizeKey) {
    if (!this.mainWindow || this.mainWindow.isDestroyed()) {
      return { success: false, message: "Window not available" };
    }
    return this._enqueueMainWindowMutation(() => this._performMainWindowResize(sizeKey));
  }

  resizeAssistantWindowToContent(surfaceHeight) {
    if (!this.mainWindow || this.mainWindow.isDestroyed()) {
      return { success: false, message: "Window not available" };
    }

    return this._enqueueMainWindowMutation(() =>
      this._performMainWindowResize("ASSISTANT_CONTENT", { surfaceHeight })
    );
  }

  resizeDictationErrorWindowToContent(surfaceHeight) {
    if (!this.mainWindow || this.mainWindow.isDestroyed()) {
      return { success: false, message: "Window not available" };
    }

    return this._enqueueMainWindowMutation(() =>
      this._performMainWindowResize("DICTATION_ERROR_CONTENT", { surfaceHeight })
    );
  }

  // Deliberate moves are not anchored resizes: the renderer must drop any live
  // resize mask rather than hold a translation against invalidated bounds.
  _clearRendererResizeMask() {
    if (!this.mainWindow || this.mainWindow.isDestroyed()) return;
    this.mainWindow.webContents.send("main-window-will-resize", { anchor: "none" });
  }

  async _prepareRendererForMainWindowResize(bounds, anchor) {
    if (!this.mainWindow || this.mainWindow.isDestroyed()) return;

    // The renderer must install screen-space anchor compensation before
    // setBounds reaches the OS compositor. Without this handshake, macOS can
    // paint the new viewport size one frame before the
    // corresponding window position, which visibly kicks the pill or panel.
    // A fixed sleep loses that race whenever the renderer is mid-task (an
    // entrance commit, mic warm-up), so wait for its explicit ack; the
    // timeout only covers an unresponsive or torn-down renderer.
    const token = ++this._resizeMaskTokenCounter;
    const ackPromise = new Promise((resolve) => {
      const listener = (_event, ackToken) => {
        if (ackToken !== token) return;
        ipcMain.removeListener("main-window-resize-mask-ready", listener);
        clearTimeout(timeout);
        resolve();
      };
      const timeout = setTimeout(() => {
        ipcMain.removeListener("main-window-resize-mask-ready", listener);
        resolve();
      }, 60);
      ipcMain.on("main-window-resize-mask-ready", listener);
    });
    this.mainWindow.webContents.send("main-window-will-resize", { bounds, anchor, token });
    await ackPromise;
  }

  _enqueueMainWindowMutation(run) {
    // Renderer voice requests are latest-wins, while errors and other overlays
    // and active-display placement can also mutate the native bounds. Serialize
    // once more at the native boundary so setBounds calls cannot interleave.
    this._mainWindowResizeQueue = (this._mainWindowResizeQueue || Promise.resolve()).then(run, run);
    return this._mainWindowResizeQueue;
  }

  // The pill is bottom-anchored, so the display that owns its bottom-center
  // point is the one that must keep it through resizes and repositions.
  _getMainWindowDisplayFor(bounds) {
    return screen.getDisplayNearestPoint({
      x: bounds.x + bounds.width / 2,
      y: bounds.y + bounds.height,
    });
  }

  _resolveMainWindowSize(sizeKey, workArea, request) {
    switch (sizeKey) {
      case "DICTATION_ERROR":
      case "DICTATION_ERROR_WITH_TRANSCRIPT":
        return fitDictationErrorWindowToWorkArea(WINDOW_SIZES[sizeKey], workArea);
      case "ASSISTANT_CONTENT":
        return fitAssistantContentWindowToWorkArea(request?.surfaceHeight, workArea);
      case "DICTATION_ERROR_CONTENT":
        return fitDictationErrorContentWindowToWorkArea(request?.surfaceHeight, workArea);
      default:
        return WINDOW_SIZES[sizeKey] || WINDOW_SIZES.BASE;
    }
  }

  async _performMainWindowResize(sizeKey, request) {
    // The queue can drain after the window is gone (quit, recreate); the
    // caller's guard ran before enqueueing.
    if (!this.mainWindow || this.mainWindow.isDestroyed()) {
      return { success: false, error: "Main window not available" };
    }
    // Bounds, display and the work-area fit are all sampled inside the queue:
    // a queued cross-display move would otherwise leave a fit computed at
    // enqueue time describing the display the window is about to leave.
    const currentBounds = this.mainWindow.getBounds();
    const display = this._getMainWindowDisplayFor(currentBounds);
    const newSize = this._resolveMainWindowSize(
      sizeKey,
      display.workArea || display.bounds,
      request
    );

    // A window moved since the last resize (dragged) means the captured BASE
    // bounds no longer describe where the user wants the pill — drop them.
    // Tolerate a couple of pixels: fractional DPI scaling can round setBounds
    // values, and treating that as a drag would defeat the restore forever.
    const MOVE_TOLERANCE_PX = 2;
    if (
      this._lastResizeBounds &&
      (Math.abs(currentBounds.x - this._lastResizeBounds.x) > MOVE_TOLERANCE_PX ||
        Math.abs(currentBounds.y - this._lastResizeBounds.y) > MOVE_TOLERANCE_PX)
    ) {
      this._baseBoundsBeforeResize = null;
      this._activeHorizontalDirection = null;
    }

    // Edge positions restore the exact pre-grow bounds on return to BASE. Anchoring the
    // shrink on the grown bounds instead would re-anchor on whatever the
    // work-area clamp did on the way up, walking the pill away from where the
    // user put it a little more on every grow/shrink cycle.
    if (sizeKey === "BASE" && this._baseBoundsBeforeResize) {
      // The work area can shrink while the window is grown (dock/taskbar
      // reappearing, resolution change) — clamp the restore so the pill
      // cannot come back off-screen.
      const restored =
        this._panelStartPosition === "center"
          ? WindowPositionUtil.getMainWindowPosition(
              display,
              this._baseBoundsBeforeResize,
              "center"
            )
          : {
              ...this._baseBoundsBeforeResize,
              ...WindowPositionUtil.clampToWorkArea(this._baseBoundsBeforeResize, display),
            };
      const restoreAnchor =
        this._panelStartPosition === "center"
          ? "center"
          : `bottom-${this._activeHorizontalDirection || this.getMainWindowHorizontalDirection()}`;
      this._baseBoundsBeforeResize = null;
      if (
        restored.x === currentBounds.x &&
        restored.y === currentBounds.y &&
        restored.width === currentBounds.width &&
        restored.height === currentBounds.height
      ) {
        // Nothing moved (BASE and the grown size share bounds) — skip the mask
        // handshake and setBounds so the restore cannot perturb the renderer.
        this._lastResizeBounds = restored;
        this._activeHorizontalDirection = null;
        this._notifyMainWindowHorizontalDirection();
        return { success: true, bounds: restored, changed: false };
      }
      await this._prepareRendererForMainWindowResize(restored, restoreAnchor);
      if (!this.mainWindow || this.mainWindow.isDestroyed()) {
        return { success: false, message: "Window not available" };
      }
      this._lastResizeBounds = restored;
      this.mainWindow.setBounds(restored);
      this._activeHorizontalDirection = null;
      this._notifyMainWindowHorizontalDirection();
      return { success: true, bounds: restored, changed: true };
    }

    if (
      sizeKey !== "BASE" &&
      !this._baseBoundsBeforeResize &&
      currentBounds.width === WINDOW_SIZES.BASE.width &&
      currentBounds.height === WINDOW_SIZES.BASE.height
    ) {
      this._baseBoundsBeforeResize = { ...currentBounds };
      this._activeHorizontalDirection = resolveHorizontalWindowDirection(
        currentBounds,
        display,
        this._panelStartPosition
      );
    }

    if (sizeKey !== "BASE" && !this._activeHorizontalDirection) {
      this._activeHorizontalDirection = resolveHorizontalWindowDirection(
        currentBounds,
        display,
        this._panelStartPosition
      );
    }
    const position =
      this._panelStartPosition === "center"
        ? "center"
        : `bottom-${this._activeHorizontalDirection || this.getMainWindowHorizontalDirection()}`;

    let newX, newY;

    if (position === "bottom-left") {
      // Anchor bottom-left corner: keep x, expand rightward and upward
      newX = currentBounds.x;
      newY = currentBounds.y + currentBounds.height - newSize.height;
    } else if (position === "center") {
      // Recompute from the display so stale or dragged bounds cannot shift
      // the pill's center through a grow/shrink cycle.
      const centered = WindowPositionUtil.getMainWindowPosition(display, newSize, "center");
      newX = centered.x;
      newY = centered.y;
    } else {
      // bottom-right (default): anchor bottom-right corner, expand leftward and upward
      const bottomRightX = currentBounds.x + currentBounds.width;
      newX = bottomRightX - newSize.width;
      newY = currentBounds.y + currentBounds.height - newSize.height;
    }

    const clamped = WindowPositionUtil.clampToWorkArea({ x: newX, y: newY, ...newSize }, display);
    const newBounds = { ...clamped, ...newSize };

    // Opening a voice mode and the size-priority effect can request the same
    // footprint in adjacent ticks. Avoid asking the OS compositor to rebuild
    // an unchanged transparent window surface.
    if (
      currentBounds.x === newBounds.x &&
      currentBounds.y === newBounds.y &&
      currentBounds.width === newBounds.width &&
      currentBounds.height === newBounds.height
    ) {
      this._lastResizeBounds = { ...currentBounds };
      if (sizeKey === "BASE") {
        this._activeHorizontalDirection = null;
        this._notifyMainWindowHorizontalDirection();
      }
      return { success: true, bounds: currentBounds, changed: false };
    }

    await this._prepareRendererForMainWindowResize(newBounds, position);
    if (!this.mainWindow || this.mainWindow.isDestroyed()) {
      return { success: false, message: "Window not available" };
    }
    this.mainWindow.setBounds(newBounds);
    this._lastResizeBounds = newBounds;
    if (sizeKey === "BASE") {
      this._activeHorizontalDirection = null;
      this._notifyMainWindowHorizontalDirection();
    }

    return { success: true, bounds: newBounds, changed: true };
  }

  async loadWindowContent(window, isControlPanel = false) {
    if (process.env.NODE_ENV === "development") {
      const appUrl = DevServerManager.getAppUrl(isControlPanel);
      await DevServerManager.waitForDevServer();
      await window.loadURL(appUrl);
    } else {
      const fileInfo = DevServerManager.getAppFilePath(isControlPanel);
      if (!fileInfo) {
        throw new Error("Failed to get app file path");
      }

      const fs = require("fs");
      if (!fs.existsSync(fileInfo.path)) {
        throw new Error(`HTML file not found: ${fileInfo.path}`);
      }

      await window.loadFile(fileInfo.path, { query: fileInfo.query });
    }
  }

  async loadMainWindow() {
    await this.loadWindowContent(this.mainWindow, false);
  }

  createHotkeyCallback() {
    const isPress = createHotkeyRepeatGate();

    // globalShortcut registrations pass the hotkey that fired.
    return async (triggeredHotkey) => {
      if (this.hotkeyManager.isInListeningMode()) {
        return;
      }
      if (this.isDictationProcessing()) {
        return;
      }

      const activationMode = this.getActivationMode();
      const currentHotkey = triggeredHotkey || this.hotkeyManager.getCurrentHotkey?.();

      if (
        activationMode === "push" &&
        currentHotkey &&
        !isGlobeLikeHotkey(currentHotkey) &&
        currentHotkey.includes("+")
      ) {
        this.startMacCompoundPushToTalk(currentHotkey);
        return;
      }

      if (!isPress()) return;
      this.sendToggleDictation();
    };
  }

  startMacCompoundPushToTalk(hotkey) {
    if (this._onboardingActive) return;
    if (this.macCompoundPushState?.active || this.isDictationProcessing()) {
      return;
    }

    const requiredModifiers = this.getMacRequiredModifiers(hotkey);
    if (requiredModifiers.size === 0) {
      return;
    }

    const MIN_HOLD_DURATION_MS = 150;
    const MAX_PUSH_DURATION_MS = 300000; // 5 minutes max recording
    const downTime = Date.now();

    const startupRequest = this.createRecordingStartupRequest();
    const targetPidPromise = this.textEditMonitor?.captureTargetPid?.();
    this.showDictationPanel({ reposition: true, targetPidPromise });
    this.sendPrepareDictation({ startupRequest });

    const safetyTimeoutId = setTimeout(() => {
      if (this.macCompoundPushState?.active) {
        debugLogger.warn("Compound PTT safety timeout", undefined, "ptt");
        this.forceStopMacCompoundPush("timeout");
      }
    }, MAX_PUSH_DURATION_MS);

    this.macCompoundPushState = {
      active: true,
      downTime,
      isRecording: false,
      requiredModifiers,
      safetyTimeoutId,
    };

    setTimeout(() => {
      if (!this.macCompoundPushState || this.macCompoundPushState.downTime !== downTime) {
        return;
      }

      if (!this.macCompoundPushState.isRecording) {
        this.macCompoundPushState.isRecording = true;
        this.sendStartDictation();
      }
    }, MIN_HOLD_DURATION_MS);
  }

  handleMacPushModifierUp(modifier) {
    if (!this.macCompoundPushState?.active) {
      return;
    }

    if (!this.macCompoundPushState.requiredModifiers.has(modifier)) {
      return;
    }

    if (this.macCompoundPushState.safetyTimeoutId) {
      clearTimeout(this.macCompoundPushState.safetyTimeoutId);
    }

    const wasRecording = this.macCompoundPushState.isRecording;
    this.macCompoundPushState = null;

    if (wasRecording) {
      this.sendStopDictation();
    } else {
      this.sendCancelDictationPreparation();
      this.hideDictationPanel();
    }
  }

  // A push that ends without a physical release leaves the trigger keys down, so
  // an injected paste shortcut lands in a modifier state the target app cannot
  // interpret and the transcript is lost. Tell the renderer to hold the text
  // back instead of pasting it.
  _notifyPushForceStopped(reason) {
    if (this.mainWindow && !this.mainWindow.isDestroyed()) {
      this.mainWindow.webContents.send("dictation-force-stopped", { reason });
    }
  }

  forceStopMacCompoundPush(reason = "manual") {
    if (!this.macCompoundPushState) {
      return;
    }

    if (this.macCompoundPushState.safetyTimeoutId) {
      clearTimeout(this.macCompoundPushState.safetyTimeoutId);
    }

    const wasRecording = this.macCompoundPushState.isRecording;
    this.macCompoundPushState = null;

    this._notifyPushForceStopped(reason);

    if (wasRecording) {
      this.sendStopDictation();
    } else {
      this.sendCancelDictationPreparation();
      this.hideDictationPanel();
    }
  }

  getMacRequiredModifiers(hotkey) {
    const required = new Set();
    const parts = hotkey.split("+").map((part) => part.trim());

    for (const part of parts) {
      switch (part) {
        case "Command":
        case "Cmd":
        case "RightCommand":
        case "RightCmd":
        case "CommandOrControl":
        case "Super":
        case "Meta":
          required.add("command");
          break;
        case "Control":
        case "Ctrl":
        case "RightControl":
        case "RightCtrl":
          required.add("control");
          break;
        case "Alt":
        case "Option":
        case "RightAlt":
        case "RightOption":
          required.add("option");
          break;
        case "Shift":
        case "RightShift":
          required.add("shift");
          break;
        case "Fn":
          required.add("fn");
          break;
        default:
          break;
      }
    }

    return required;
  }

  setDictationLifecycleState(state) {
    const nextState = normalizeDictationLifecycle(state);
    if (nextState === this._dictationLifecycleState) return;

    this._dictationLifecycleState = nextState;
    this._isDictatingToggle = isDictationRecording(nextState);
    this.onDictationStateChanged?.();
  }

  // The tray's listen item is a toggle over this state, like the pill's.
  isDictating() {
    return this._isDictatingToggle;
  }

  isDictationProcessing() {
    return shouldIgnoreDictationHotkey(this._dictationLifecycleState);
  }

  // A Tap mode key combination: the renderer owns the real recording state and
  // may decline the toggle (mic error, Esc cancel), so each press captures the
  // paste target and the start guess only pre-warms the mic.
  sendToggleDictation() {
    if (this._onboardingActive) return;
    if (this.hotkeyManager.isInListeningMode()) {
      return;
    }
    if (shouldIgnoreDictationHotkey(this._dictationLifecycleState)) {
      return;
    }
    if (this.mainWindow && !this.mainWindow.isDestroyed()) {
      const isStarting = !this._isDictatingToggle;
      const startupRequest = isStarting ? this.createRecordingStartupRequest() : undefined;
      const targetPidPromise = this.textEditMonitor?.captureTargetPid?.();
      if (!isStarting) {
        this._mainWindowPlacementCoordinator.cancelPending();
      }
      this.showDictationPanel({ reposition: isStarting, targetPidPromise });
      if (isStarting) {
        this.sendPrepareDictation({ startupRequest });
      }
      this._preparedStartupRequest = null;
      this.mainWindow.webContents.send(
        "toggle-dictation",
        startupRequest ? { startupRequest } : undefined
      );
    }
  }

  sendStartDictation() {
    if (this._onboardingActive) return;
    if (this.hotkeyManager.isInListeningMode()) {
      return;
    }
    if (shouldIgnoreDictationHotkey(this._dictationLifecycleState)) {
      return;
    }
    if (this.mainWindow && !this.mainWindow.isDestroyed()) {
      const startupRequest = this._preparedStartupRequest ?? this.createRecordingStartupRequest();
      this._preparedStartupRequest = null;
      const targetPidPromise = this.textEditMonitor?.captureTargetPid?.();
      this.showDictationPanel({ reposition: true, targetPidPromise });
      this.mainWindow.webContents.send("start-dictation", { startupRequest });
    }
  }

  sendStopDictation() {
    this._preparedStartupRequest = null;
    if (this.hotkeyManager.isInListeningMode()) {
      return;
    }
    if (this.mainWindow && !this.mainWindow.isDestroyed()) {
      this.mainWindow.webContents.send("stop-dictation");
    }
  }

  createRecordingStartupRequest(acceptedAt = performance.timeOrigin + performance.now()) {
    const request = { requestId: randomUUID(), acceptedAt };
    debugLogger.info(
      "Recording startup",
      {
        ...request,
        sequence: 0,
        stages: { requestAccepted: 0 },
        outcome: "pending",
        totalMs: null,
      },
      "audio"
    );
    return request;
  }

  sendPrepareDictation({ startupRequest } = {}) {
    if (this._onboardingActive) return;
    if (this.hotkeyManager.isInListeningMode()) {
      return;
    }
    if (shouldIgnoreDictationHotkey(this._dictationLifecycleState)) {
      return;
    }
    if (this.mainWindow && !this.mainWindow.isDestroyed()) {
      startupRequest ??= this.createRecordingStartupRequest();
      this._preparedStartupRequest = startupRequest;
      this.mainWindow.webContents.send("prepare-dictation", { startupRequest });
    }
  }

  sendCancelDictationPreparation() {
    this._preparedStartupRequest = null;
    if (this.mainWindow && !this.mainWindow.isDestroyed()) {
      this.mainWindow.webContents.send("cancel-dictation-preparation");
    }
  }

  sendCancelDictation() {
    this._preparedStartupRequest = null;
    if (this.hotkeyManager.isInListeningMode()) {
      return;
    }
    if (this.mainWindow && !this.mainWindow.isDestroyed()) {
      this.mainWindow.webContents.send("cancel-dictation-preparation");
      this.mainWindow.webContents.send("cancel-hotkey-pressed");
    }
  }

  getActivationMode() {
    return this._cachedActivationMode;
  }

  async setActivationModeCache(mode) {
    const nextMode = mode === "push" ? "push" : "tap";
    const success = await this.hotkeyManager.setActivationMode(nextMode);
    if (!success) return false;
    this._cachedActivationMode = nextMode;
    return true;
  }

  setFloatingIconAutoHide(enabled) {
    this._floatingIconAutoHide = Boolean(enabled);
  }

  getMainWindowHorizontalDirection() {
    if (!this.mainWindow || this.mainWindow.isDestroyed()) {
      return this._panelStartPosition === "bottom-left" ? "left" : "right";
    }
    const bounds = this.mainWindow.getBounds();
    const display = this._getMainWindowDisplayFor(bounds);
    return resolveHorizontalWindowDirection(bounds, display, this._panelStartPosition);
  }

  _notifyMainWindowHorizontalDirection() {
    if (!this.mainWindow || this.mainWindow.isDestroyed()) return;
    this.mainWindow.webContents.send(
      "main-window-horizontal-direction-changed",
      this.getMainWindowHorizontalDirection()
    );
  }

  setPanelStartPosition(position) {
    this._panelStartPosition = position || "bottom-right";
    this._mainWindowPlacementCoordinator.resetManualPosition();
    this._activeHorizontalDirection = null;
    return this._enqueueMainWindowMutation(() => {
      if (!this.mainWindow || this.mainWindow.isDestroyed()) return;
      const currentBounds = this.mainWindow.getBounds();
      const display = this._getMainWindowDisplayFor(currentBounds);
      const newPos = WindowPositionUtil.getMainWindowPosition(
        display,
        { width: currentBounds.width, height: currentBounds.height },
        this._panelStartPosition
      );
      this._clearRendererResizeMask();
      this.mainWindow.setBounds(newPos);
      this._baseBoundsBeforeResize = null;
      this._lastResizeBounds = { ...newPos };
      this._activeHorizontalDirection = null;
      this._notifyMainWindowHorizontalDirection();
    });
  }

  setHotkeyListeningMode(enabled) {
    this.hotkeyManager.setListeningMode(enabled);
  }

  async initializeHotkey() {
    await this.hotkeyManager.initializeHotkey(this.mainWindow, this.createHotkeyCallback());
  }

  async updateHotkey(hotkey) {
    return await this.hotkeyManager.updateHotkey(hotkey, this.createHotkeyCallback());
  }

  // The control panel is transparent on macOS, where Electron ignores
  // `-webkit-app-region: drag` entirely — the renderer recreates its titlebar
  // drag through the shared DragManager instead (useControlPanelWindowDrag).
  // No pill bookkeeping: position ownership below is main-window-only.
  async startControlPanelDrag() {
    if (!this.controlPanelWindow || this.controlPanelWindow.isDestroyed()) {
      return { success: false, message: "Window not available" };
    }
    return await this.dragManager.startWindowDrag(this.controlPanelWindow);
  }

  async stopControlPanelDrag() {
    return await this.dragManager.stopWindowDrag();
  }

  async startWindowDrag() {
    // A lookup started by a prior hotkey must never land while the user is
    // taking ownership of the panel position.
    this._mainWindowPlacementCoordinator.cancelPending();
    this._dragStartBounds =
      this.mainWindow && !this.mainWindow.isDestroyed() ? this.mainWindow.getBounds() : null;
    return await this.dragManager.startWindowDrag();
  }

  async stopWindowDrag() {
    const result = await this.dragManager.stopWindowDrag();
    let recenter = false;
    if (result.success && this.mainWindow && !this.mainWindow.isDestroyed()) {
      const draggedBounds = this.mainWindow.getBounds();
      const start = this._dragStartBounds;
      // Every pill click goes through start/stopWindowDrag. Only an actual
      // move changes placement; center mode snaps back on the drop display.
      const moved =
        !start ||
        Math.abs(draggedBounds.x - start.x) > DRAG_MOVE_TOLERANCE_PX ||
        Math.abs(draggedBounds.y - start.y) > DRAG_MOVE_TOLERANCE_PX;
      if (moved) {
        if (this._panelStartPosition === "center") {
          this._mainWindowPlacementCoordinator.resetManualPosition();
          recenter = true;
        } else {
          this._mainWindowPlacementCoordinator.markManuallyPositioned();
        }
        this._baseBoundsBeforeResize = null;
        this._lastResizeBounds = { ...draggedBounds };
      }
    }
    this._dragStartBounds = null;
    this._activeHorizontalDirection = null;
    this._notifyMainWindowHorizontalDirection();
    if (recenter) {
      await this._recenterMainWindow();
    }
    return result;
  }

  openExternalUrl(url, showError = true) {
    shell.openExternal(url).catch((error) => {
      if (showError) {
        dialog.showErrorBox(
          i18nMain.t("dialog.openLink.title"),
          i18nMain.t("dialog.openLink.message", { url, error: error.message })
        );
      }
    });
  }

  async createControlPanelWindow() {
    if (this.controlPanelWindow && !this.controlPanelWindow.isDestroyed()) {
      if (this.controlPanelWindow.isMinimized()) {
        this.controlPanelWindow.restore();
      }
      if (!this.controlPanelWindow.isVisible()) {
        this.controlPanelWindow.show();
      }
      this.controlPanelWindow.focus();
      dockManager.setControlPanelVisible(true);
      return;
    }

    this.controlPanelWindow = new BrowserWindow(CONTROL_PANEL_CONFIG);
    this._onboardingRestoreBounds = null;
    this._onboardingWindowMode = null;
    this._onboardingWindowState = null;

    this.controlPanelWindow.webContents.on("will-navigate", (event, url) => {
      // getAppUrl() is null in packaged builds; exactly one of the two is set.
      const appUrl =
        DevServerManager.getAppUrl(true) ??
        pathToFileURL(DevServerManager.getAppFilePath(true).path).href;

      if (isAllowedAppNavigation(url, appUrl)) {
        return;
      }

      event.preventDefault();
      if (isExternalBrowserUrl(url)) {
        this.openExternalUrl(url);
      } else {
        debugLogger.debug("Blocked untrusted navigation", { url }, "window");
      }
    });

    this.controlPanelWindow.webContents.setWindowOpenHandler(({ url }) => {
      this.openExternalUrl(url);
      return { action: "deny" };
    });

    this.controlPanelWindow.webContents.on("did-create-window", (childWindow, details) => {
      childWindow.close();
      if (details.url && !details.url.startsWith("devtools://")) {
        this.openExternalUrl(details.url, false);
      }
    });

    // Nothing else shows this window: ready-to-show deliberately doesn't, so the
    // renderer can pick the onboarding size first and avoid a visible
    // expanded → compact flash on fresh installs. That makes this the only
    // backstop if the renderer never gets that far — it loads but throws, a lazy
    // chunk fails, or auth/policy resolution never settles — so it must outlive
    // did-finish-load. Only a real show cancels it.
    this._controlPanelVisibilityTimer = setTimeout(() => {
      this._showControlPanel();
    }, 10000);

    this.controlPanelWindow.on("close", (event) => {
      if (!this.isQuitting) {
        event.preventDefault();
        this.hideControlPanelToTray();
      }
    });

    this.controlPanelWindow.on("closed", () => {
      this._clearControlPanelVisibilityTimer();
      this._clearControlPanelReleaseTimer();
      this._controlPanelRetained = false;
      this.controlPanelWindow = null;
      if (!this._controlPanelReleased) {
        this._onboardingActive = true;
        this._hideNormalAppSurfaces();
      }
      this._onboardingRestoreBounds = null;
      this._onboardingWindowMode = null;
      this._onboardingWindowState = null;
      dockManager.setControlPanelVisible(false);
    });

    MenuManager.setupControlPanelMenu(() => this.openSettings());

    this.controlPanelWindow.webContents.on("did-finish-load", () => {
      // Every fresh document starts unresolved. AppRouter releases the gate
      // only after it commits the normal app, so reloads cannot expose the
      // dictation pill or hotkeys in between. A panel recreated after an idle
      // release replaces a committed app, so it must not cancel a dictation
      // that is already running.
      if (this._controlPanelReleased) {
        this._controlPanelReleased = false;
      } else {
        this.setOnboardingActive(true);
      }
      this.controlPanelWindow.setTitle(i18nMain.t("window.controlPanelTitle"));
    });

    this.controlPanelWindow.webContents.on(
      "did-fail-load",
      (_event, errorCode, errorDescription, validatedURL, isMainFrame) => {
        if (!isMainFrame) {
          return;
        }
        if (process.env.NODE_ENV !== "development") {
          this.showLoadFailureDialog("Control panel", errorCode, errorDescription, validatedURL);
        }
        // Show it regardless: a failed load can't reach the renderer path that
        // normally does, and a hidden window leaves the failure invisible.
        this._showControlPanel();
      }
    );

    this.controlPanelWindow.webContents.on("render-process-gone", (_event, details) => {
      if (details.reason === "crashed" || details.reason === "killed" || details.reason === "oom") {
        debugLogger.error(
          "Control panel renderer process gone",
          { reason: details.reason, exitCode: details.exitCode },
          "window"
        );
        setTimeout(() => this.loadControlPanel(), 1000);
      }
    });

    this.controlPanelWindow.on("show", () => {
      this._clearControlPanelReleaseTimer();
      if (this.controlPanelWindow.webContents.isCrashed()) {
        debugLogger.error("Control panel crashed, reloading on show", undefined, "window");
        this.loadControlPanel();
      }
    });

    await this.loadControlPanel();
  }

  async loadControlPanel() {
    await this.loadWindowContent(this.controlPanelWindow, true);
  }

  // The display the user is working on is the one showing the app being dictated
  // into, which on a multi-monitor desk is often not the one the mouse rests on.
  // Falls back to the cursor when the target has no readable window (no target
  // captured yet, or an app with no ordinary window).
  async _resolveActiveDisplay(targetPidPromise) {
    let pid = this.textEditMonitor?.lastTargetPid;
    if (targetPidPromise) {
      try {
        pid = await targetPidPromise;
      } catch {
        pid = null;
      }
    }
    const bounds = pid ? await this.textEditMonitor.getTargetWindowBounds(pid) : null;
    return bounds
      ? screen.getDisplayMatching(bounds)
      : screen.getDisplayNearestPoint(screen.getCursorScreenPoint());
  }

  _repositionToActiveDisplay(targetPidPromise) {
    return this._repositionMainWindow(() => this._resolveActiveDisplay(targetPidPromise));
  }

  _recenterMainWindow(display = null) {
    if (
      this._panelStartPosition !== "center" ||
      !this.mainWindow ||
      this.mainWindow.isDestroyed()
    ) {
      return Promise.resolve({ applied: false, reason: "unavailable" });
    }
    return this._repositionMainWindow(
      () => display || this._getMainWindowDisplayFor(this.mainWindow.getBounds())
    );
  }

  _repositionMainWindow(resolveDisplay) {
    return this._mainWindowPlacementCoordinator.request(
      resolveDisplay,
      (activeDisplay, isCurrent) =>
        this._enqueueMainWindowMutation(() =>
          this._performActiveDisplayReposition(activeDisplay, isCurrent)
        )
    );
  }

  _performActiveDisplayReposition(activeDisplay, isCurrent) {
    if (
      !isCurrent() ||
      this.dragManager.isDragActive() ||
      !this.mainWindow ||
      this.mainWindow.isDestroyed()
    ) {
      return { applied: false, reason: "superseded" };
    }

    const currentBounds = this.mainWindow.getBounds();
    const currentDisplay = this._getMainWindowDisplayFor(currentBounds);

    if (currentDisplay.id === activeDisplay.id && this._panelStartPosition !== "center") {
      // Nearest-display math can't tell "on this display" from "just past its
      // edge", so a rearranged monitor or a drag that ended over another
      // display can leave the panel stranded in dead space, looking like the
      // overlay vanished. Pull it back before showing it.
      const clamped = WindowPositionUtil.clampToWorkArea(currentBounds, currentDisplay);
      if (clamped.x !== currentBounds.x || clamped.y !== currentBounds.y) {
        const clampedBounds = { ...currentBounds, ...clamped };
        this._clearRendererResizeMask();
        this.mainWindow.setBounds(clampedBounds);
        this._lastResizeBounds = { ...clampedBounds };
        this._baseBoundsBeforeResize = null;
        return { applied: true, bounds: clampedBounds };
      }
      return { applied: false, reason: "same-display" };
    }

    const newPos = WindowPositionUtil.getMainWindowPosition(
      activeDisplay,
      { width: currentBounds.width, height: currentBounds.height },
      this._panelStartPosition
    );
    if (newPos.x === currentBounds.x && newPos.y === currentBounds.y) {
      return { applied: false, reason: "same-position" };
    }
    debugLogger.debug(
      "[WindowManager] Repositioning dictation panel",
      { from: currentBounds, to: newPos, displayId: activeDisplay.id },
      "window"
    );
    this._clearRendererResizeMask();
    this.mainWindow.setBounds(newPos);
    // This is an intentional native move, not a drag. Keep resize restoration
    // from treating the old display's bounds as the user's desired base state.
    this._lastResizeBounds = { ...newPos };
    this._baseBoundsBeforeResize = null;
    this._activeHorizontalDirection = null;
    this._notifyMainWindowHorizontalDirection();
    return { applied: true, bounds: newPos };
  }

  showDictationPanel(options = {}) {
    if (this._onboardingActive) return;
    const { focus = false, reposition = false, targetPidPromise } = options;
    if (!this.mainWindow || this.mainWindow.isDestroyed()) return;
    if (reposition) {
      void this._repositionToActiveDisplay(targetPidPromise);
    } else if (options.reposition === undefined) {
      void this._recenterMainWindow();
    }
    if (this.mainWindow.isMinimized()) {
      this.mainWindow.restore();
    }
    if (!this.mainWindow.isVisible()) {
      if (typeof this.mainWindow.showInactive === "function") {
        this.mainWindow.showInactive();
      } else {
        this.mainWindow.show();
      }
    }
    if (focus) {
      this.mainWindow.focus();
    }
  }

  setOnboardingActive(active) {
    const nextActive = active === true;
    if (nextActive === this._onboardingActive) {
      if (nextActive) this._hideNormalAppSurfaces();
      return true;
    }

    if (nextActive) {
      this._onboardingActive = true;
      this.sendCancelDictation();
      this._hideNormalAppSurfaces();
      return true;
    }

    this.sendCancelDictation();
    this.hideDictationPanel();
    this._onboardingActive = false;
    if (!this._floatingIconAutoHide) this.showDictationPanel();
    return true;
  }

  _hideNormalAppSurfaces() {
    this.hideDictationPanel();
  }

  _clearControlPanelVisibilityTimer() {
    clearTimeout(this._controlPanelVisibilityTimer);
    this._controlPanelVisibilityTimer = null;
  }

  _showControlPanel() {
    const win = this.controlPanelWindow;
    if (!win || win.isDestroyed()) return;
    // Cancel the backstop either way: once the window has been shown on purpose,
    // a later timer firing could pull it back out of the tray.
    this._clearControlPanelVisibilityTimer();
    if (win.isVisible()) return;
    win.show();
    win.focus();
    dockManager.setControlPanelVisible(true);
  }

  // Compact onboarding starts at smaller bounds, but both modes expose the
  // complete window-control contract so a frameless window never traps the
  // user in setup.
  _applyOnboardingWindowChrome(win, mode) {
    const expanded = mode === "expanded";
    win.setResizable(true);
    win.setMinimizable(true);
    win.setMaximizable(true);
    win.setClosable(true);
    win.setFullScreenable(false);
    // Floor at the mode's canonical size so no step renders below the bounds
    // it was designed for — clamped to the work area, or a 1366x768-class
    // display could never fit (and setContentBounds would fight the minimum).
    const floor = expanded ? ONBOARDING_WINDOW_SIZES.EXPANDED : ONBOARDING_WINDOW_SIZES.COMPACT;
    const { workArea } = screen.getDisplayMatching(win.getBounds());
    win.setMinimumSize(
      Math.min(floor.width, workArea.width),
      Math.min(floor.height, workArea.height)
    );
    if (typeof win.setWindowButtonVisibility === "function") {
      win.setWindowButtonVisibility(true);
    }
  }

  setOnboardingWindowMode(mode) {
    const win = this.controlPanelWindow;
    if (!win || win.isDestroyed()) return false;
    if (!new Set(["compact", "expanded", "restore"]).has(mode)) return false;
    if (mode !== "restore" && (win.isFullScreen() || win.isMaximized())) {
      // Entering onboarding from a maximized/fullscreen control panel must not
      // refuse: each mode is applied at its canonical centered bounds, which
      // only take effect from a normal window state. Both modes stay
      // maximizable, so the user can simply maximize again afterwards.
      if (win.isFullScreen()) win.setFullScreen(false);
      if (win.isMaximized()) win.unmaximize();
    }

    const current = win.getContentBounds();
    const { workArea } = screen.getDisplayMatching(win.getBounds());

    if (mode === "restore") {
      // A maximized/fullscreen window the user made keeps its bounds, but the
      // chrome state below must still be restored and the tracking cleared —
      // refusing outright left setFullScreenable(false) and onboarding's
      // minimum-size floor on the control panel for the rest of its life.
      if (!win.isFullScreen() && !win.isMaximized() && this._onboardingRestoreBounds) {
        win.setContentBounds(clampedBounds(this._onboardingRestoreBounds, workArea), true);
      }
      const state = this._onboardingWindowState;
      if (state) {
        win.setResizable(state.resizable);
        win.setMinimizable(state.minimizable);
        win.setMaximizable(state.maximizable);
        win.setClosable(state.closable);
        win.setFullScreenable(state.fullscreenable);
        if (state.minimumSize) win.setMinimumSize(...state.minimumSize);
      }
      if (typeof win.setWindowButtonVisibility === "function") {
        win.setWindowButtonVisibility(true);
      }
      this._onboardingRestoreBounds = null;
      this._onboardingWindowMode = null;
      this._onboardingWindowState = null;
      this._showControlPanel();
      return true;
    }

    if (!this._onboardingRestoreBounds) {
      this._onboardingRestoreBounds = current;
      this._onboardingWindowState = {
        resizable: win.isResizable(),
        minimizable: win.isMinimizable(),
        maximizable: win.isMaximizable(),
        closable: win.isClosable(),
        fullscreenable: win.isFullScreenable(),
        minimumSize: win.getMinimumSize(),
      };
    }

    this._applyOnboardingWindowChrome(win, mode);

    if (this._onboardingWindowMode === mode) {
      this._showControlPanel();
      return true;
    }

    const target =
      mode === "compact" ? ONBOARDING_WINDOW_SIZES.COMPACT : ONBOARDING_WINDOW_SIZES.EXPANDED;
    const next = centeredBounds(current, target, workArea);
    if (
      current.x === next.x &&
      current.y === next.y &&
      current.width === next.width &&
      current.height === next.height
    ) {
      this._onboardingWindowMode = mode;
      this._showControlPanel();
      return true;
    }

    win.setContentBounds(next, true);
    this._onboardingWindowMode = mode;
    this._showControlPanel();
    return true;
  }

  hideControlPanelToTray() {
    if (!this.controlPanelWindow || this.controlPanelWindow.isDestroyed()) {
      return;
    }

    // An explicit hide is authoritative: the visibility backstop exists to
    // rescue a window that never got shown, and letting it fire now would
    // pull the panel (and the Dock icon) back out of the tray.
    this._clearControlPanelVisibilityTimer();
    this.controlPanelWindow.hide();
    dockManager.setControlPanelVisible(false);
    this._scheduleControlPanelRelease();
  }

  setControlPanelRetained(retained) {
    this._controlPanelRetained = retained === true;
    if (this._controlPanelRetained) {
      this._clearControlPanelReleaseTimer();
    } else {
      this._scheduleControlPanelRelease();
    }
  }

  _clearControlPanelReleaseTimer() {
    clearTimeout(this._controlPanelReleaseTimer);
    this._controlPanelReleaseTimer = null;
  }

  _canReleaseControlPanel() {
    const win = this.controlPanelWindow;
    return (
      !!win &&
      !win.isDestroyed() &&
      !win.isVisible() &&
      !this._controlPanelRetained &&
      !this.isQuitting
    );
  }

  _scheduleControlPanelRelease() {
    this._clearControlPanelReleaseTimer();
    if (!this._canReleaseControlPanel()) return;
    this._controlPanelReleaseTimer = setTimeout(() => {
      this._controlPanelReleaseTimer = null;
      this._releaseHiddenControlPanel();
    }, CONTROL_PANEL_RELEASE_DELAY_MS);
  }

  _releaseHiddenControlPanel() {
    if (!this._canReleaseControlPanel()) return;
    debugLogger.debug("Releasing hidden control panel", undefined, "window");
    this._controlPanelReleased = true;
    // destroy() skips the close handler that would only hide the window again.
    this.controlPanelWindow.destroy();
  }

  hideDictationPanel() {
    this._mainWindowPlacementCoordinator.cancelPending();
    if (this.mainWindow && !this.mainWindow.isDestroyed()) this.mainWindow.hide();
  }

  isDictationPanelVisible() {
    if (!this.mainWindow || this.mainWindow.isDestroyed()) {
      return false;
    }

    if (this.mainWindow.isMinimized && this.mainWindow.isMinimized()) {
      return false;
    }

    return this.mainWindow.isVisible();
  }

  registerMainWindowEvents() {
    if (!this.mainWindow) {
      return;
    }

    // Safety timeout: force show the window if ready-to-show doesn't fire within 10 seconds
    const showTimeout = setTimeout(() => {
      if (
        this.mainWindow &&
        !this.mainWindow.isDestroyed() &&
        !this.mainWindow.isVisible() &&
        !this._floatingIconAutoHide
      ) {
        this.showDictationPanel();
      }
    }, 10000);

    this.mainWindow.once("ready-to-show", () => {
      clearTimeout(showTimeout);
      this.enforceMainWindowOnTop();
      if (!this.mainWindow.isVisible() && !this._floatingIconAutoHide) {
        this.showDictationPanel();
      }
    });

    this.mainWindow.on("show", () => {
      this.enforceMainWindowOnTop();
    });

    this.mainWindow.on("focus", () => {
      this.enforceMainWindowOnTop();
    });

    let mainWindowDisplayId = this._getMainWindowDisplayFor(this.mainWindow.getBounds()).id;
    this.mainWindow.on("move", () => {
      mainWindowDisplayId = this._getMainWindowDisplayFor(this.mainWindow.getBounds()).id;
    });

    const displayEvents = ["display-added", "display-removed", "display-metrics-changed"];
    const recenterOnDisplayChange = () => {
      if (this._panelStartPosition !== "center") return;
      // A rearranged display may no longer cover the old window coordinates.
      // Keep its identity until it disconnects, then use the nearest display.
      const display = screen.getAllDisplays().find(({ id }) => id === mainWindowDisplayId);
      void this._recenterMainWindow(display).catch((error) => {
        debugLogger.warn("Failed to recenter dictation panel", { error: error.message }, "window");
      });
    };
    for (const event of displayEvents) screen.on(event, recenterOnDisplayChange);

    this.mainWindow.on("closed", () => {
      clearTimeout(showTimeout);
      for (const event of displayEvents) screen.removeListener(event, recenterOnDisplayChange);
      this._mainWindowPlacementCoordinator.cancelPending();
      this.dragManager.cleanup();
      this.mainWindow = null;
    });
  }

  enforceMainWindowOnTop() {
    if (this.mainWindow && !this.mainWindow.isDestroyed()) {
      WindowPositionUtil.setupAlwaysOnTop(this.mainWindow);
    }
  }

  refreshLocalizedUi() {
    MenuManager.setupMainMenu(() => this.openSettings());

    if (this.controlPanelWindow && !this.controlPanelWindow.isDestroyed()) {
      MenuManager.setupControlPanelMenu(() => this.openSettings());
      this.controlPanelWindow.setTitle(i18nMain.t("window.controlPanelTitle"));
    }

    if (this.mainWindow && !this.mainWindow.isDestroyed()) {
      this.mainWindow.setTitle(i18nMain.t("window.voiceRecorderTitle"));
    }
  }

  async openSettings() {
    await this.createControlPanelWindow();
    if (this.controlPanelWindow && !this.controlPanelWindow.isDestroyed()) {
      this.controlPanelWindow.webContents.send("show-settings");
    }
  }

  showLoadFailureDialog(windowName, errorCode, errorDescription, validatedURL) {
    if (this.loadErrorShown) {
      return;
    }
    this.loadErrorShown = true;
    const detailLines = [
      i18nMain.t("dialog.loadFailure.detail.window", { windowName }),
      i18nMain.t("dialog.loadFailure.detail.error", { errorCode, errorDescription }),
      validatedURL ? i18nMain.t("dialog.loadFailure.detail.url", { url: validatedURL }) : null,
      i18nMain.t("dialog.loadFailure.detail.hint"),
    ].filter(Boolean);
    dialog.showMessageBox({
      type: "error",
      title: i18nMain.t("dialog.loadFailure.title"),
      message: i18nMain.t("dialog.loadFailure.message"),
      detail: detailLines.join("\n"),
    });
  }
}

module.exports = WindowManager;
