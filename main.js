// Chromium picks the display backend before JS runs, so appendSwitch is too
// late — the flag has to come from a relaunch.
const { XWAYLAND_FLAG, shouldForceXWayland } = require("./src/helpers/xwayland");

if (shouldForceXWayland(process.argv)) {
  const { spawn } = require("child_process");
  spawn(process.execPath, [...process.argv.slice(1), XWAYLAND_FLAG], {
    stdio: "inherit",
    detached: true,
  }).unref();
  process.exit(0);
}

const {
  app,
  desktopCapturer,
  globalShortcut,
  BrowserWindow,
  dialog,
  ipcMain,
  session,
  systemPreferences,
} = require("electron");
const path = require("path");

const tls = require("tls");
require("dotenv").config({ path: path.join(__dirname, ".env") });

// Extend Node's TLS trust with the OS store so ws and https.get see corporate
// CAs that Chromium already trusts.
try {
  const currentCAs = tls.getCACertificates();
  const systemCAs = tls.getCACertificates("system");
  if (systemCAs?.length) {
    tls.setDefaultCACertificates([...currentCAs, ...systemCAs]);
  }
} catch (err) {
  require("./src/helpers/debugLogger").warn("System CA merge failed; using existing CA list", {
    error: err?.message,
  });
}

const VALID_CHANNELS = new Set(["development", "staging", "production"]);
const BASE_WINDOWS_APP_ID = "local.whisper.desktop";

function isElectronBinaryExec() {
  const execPath = (process.execPath || "").toLowerCase();
  return (
    execPath.includes("/electron.app/contents/macos/electron") ||
    execPath.endsWith("/electron") ||
    execPath.endsWith("\\electron.exe")
  );
}

function inferDefaultChannel() {
  if (process.env.NODE_ENV === "development" || process.defaultApp || isElectronBinaryExec()) {
    return "development";
  }
  return "production";
}

function resolveAppChannel() {
  const rawChannel = (process.env.OPENWHISPR_CHANNEL || process.env.VITE_OPENWHISPR_CHANNEL || "")
    .trim()
    .toLowerCase();

  if (VALID_CHANNELS.has(rawChannel)) {
    return rawChannel;
  }

  return inferDefaultChannel();
}

const APP_CHANNEL = resolveAppChannel();
process.env.OPENWHISPR_CHANNEL = APP_CHANNEL;

function configureChannelUserDataPath() {
  const isolatedPath = process.env.OPENWHISPR_USER_DATA_DIR
    ? path.resolve(process.env.OPENWHISPR_USER_DATA_DIR)
    : path.join(app.getPath("appData"), "whisper");
  app.setPath("userData", isolatedPath);
}

configureChannelUserDataPath();

// Load userData .env (contains DICTATION_KEY, API keys, etc.) early — before
// hotkey registration, which needs DICTATION_KEY before the renderer loads.
require("dotenv").config({
  path: path.join(app.getPath("userData"), ".env"),
  override: false,
});

// Chromium's Windows-only occlusion tracker misclassifies the always-on-top
// transparent pill as occluded, throttling its renderer and jittering animations.
if (process.platform === "win32") {
  app.commandLine.appendSwitch("disable-features", "CalculateNativeWinOcclusion");
}

// Fix transparent window flickering on Linux: --enable-transparent-visuals requires
// the compositor to set up an ARGB visual before any windows are created.
// --disable-gpu-compositing prevents GPU compositing conflicts with the compositor.
if (process.platform === "linux") {
  app.commandLine.appendSwitch("gtk-version", "3");
  app.commandLine.appendSwitch("enable-transparent-visuals");
  app.commandLine.appendSwitch("disable-gpu-compositing");
}

// Wayland: packaged builds use the wrapper script (scripts/afterPack.js) to
// force --ozone-platform=x11 before Electron starts. appendSwitch below is a
// best-effort fallback for unpackaged dev mode (may not take effect on E39+).
if (process.platform === "linux" && process.env.XDG_SESSION_TYPE === "wayland") {
  app.commandLine.appendSwitch("enable-features", "WaylandWindowDecorations");
}

// Set desktop filename so Wayland compositors can match windows to the .desktop entry.
// This allows XDG portals (e.g. PipeWire) to persist permissions across sessions.
if (process.platform === "linux") {
  app.setDesktopName("whisper.desktop");
}

// Group all windows under single taskbar entry on Windows
if (process.platform === "win32") {
  const windowsAppId =
    APP_CHANNEL === "production" ? BASE_WINDOWS_APP_ID : `${BASE_WINDOWS_APP_ID}.${APP_CHANNEL}`;
  app.setAppUserModelId(windowsAppId);
}

const OAUTH_PROTOCOL = "openwhispr-selfhosted";
const protocolRegistered = false;

const gotSingleInstanceLock = app.requestSingleInstanceLock();

if (!gotSingleInstanceLock) {
  app.exit(0);
}

const isLiveWindow = (window) => window && !window.isDestroyed();

// Ensure macOS menus use the proper casing for the app name
if (process.platform === "darwin" && app.getName() !== "Whisper") {
  app.setName("Whisper");
}

// Add global error handling for uncaught exceptions
process.on("uncaughtException", (error) => {
  console.error("Uncaught Exception:", error);
  // Don't exit the process for EPIPE errors as they're harmless
  if (error.code === "EPIPE") {
    return;
  }
  // For other errors, log and continue
  console.error("Error stack:", error.stack);
});

process.on("unhandledRejection", (reason, promise) => {
  console.error("Unhandled Rejection at:", promise, "reason:", reason);
});

// Import helper module classes (but don't instantiate yet - wait for app.whenReady())
const EnvironmentManager = require("./src/helpers/environment");
const WindowManager = require("./src/helpers/windowManager");
const DatabaseManager = require("./src/helpers/database");
const ClipboardManager = require("./src/helpers/clipboard");
const WhisperManager = require("./src/helpers/whisper");
const ParakeetManager = require("./src/helpers/parakeet");

const TrayManager = require("./src/helpers/tray");
const dockManager = require("./src/helpers/dockManager");
const autoStart = require("./src/helpers/autoStart");
const IPCHandlers = require("./src/helpers/ipcHandlers");

const GlobeKeyManager = require("./src/helpers/globeKeyManager");

const WindowsKeyManager = require("./src/helpers/windowsKeyManager");
const LinuxKeyManager = require("./src/helpers/linuxKeyManager");
const TextEditMonitor = require("./src/helpers/textEditMonitor");
const SelectionManager = require("./src/helpers/selectionManager");
const WhisperCudaManager = require("./src/helpers/whisperCudaManager");
const WhisperVulkanManager = require("./src/helpers/whisperVulkanManager");
const { migrateLegacyBinDir, detectOrphanedGpuPacks } = require("./src/helpers/gpuBinaryManager");

const { i18nMain, changeLanguage } = require("./src/helpers/i18nMain");
const { ensureYdotool } = require("./src/helpers/ensureYdotool");
const sidecarRegistry = require("./src/helpers/sidecarRegistry");
const { reapStaleSidecars } = require("./src/helpers/sidecarReaper");
const { laptopLidMonitor } = require("./src/helpers/laptopLidMonitor");

// Manager instances - initialized after app.whenReady()
let debugLogger = null;
let environmentManager = null;
let windowManager = null;
let hotkeyManager = null;
let databaseManager = null;
let clipboardManager = null;
let whisperManager = null;
let parakeetManager = null;
let trayManager = null;
let globeKeyManager = null;
let windowsKeyManager = null;
let linuxKeyManager = null;
let textEditMonitor = null;
let selectionManager = null;
let whisperCudaManager = null;
let whisperVulkanManager = null;
let googleCalendarManager = null;
let microsoftCalendarManager = null;
let appleCalendarManager = null;
let calendarReminderScheduler = null;
let meetingDetectionEngine = null;
let audioTapManager = null;
let linuxPortalAudioManager = null;
let windowsLoopbackAudioManager = null;
let meetingAecManager = null;
let ipcHandlers = null;
let cliBridge = null;
let globeKeyAlertShown = false;
let macAccessibilityFeaturesReady = false;
let startMacAccessibilityFeatures = null;
let authBridgeServer = null;

let wakeRewarmTimer = null;

// Set up PATH for production builds to find system tools (whisper.cpp, ffmpeg)
function setupProductionPath() {
  if (process.platform === "darwin" && process.env.NODE_ENV !== "development") {
    const commonPaths = [
      "/usr/local/bin",
      "/opt/homebrew/bin",
      "/usr/bin",
      "/bin",
      "/usr/sbin",
      "/sbin",
    ];

    const currentPath = process.env.PATH || "";
    const pathsToAdd = commonPaths.filter((p) => !currentPath.includes(p));

    if (pathsToAdd.length > 0) {
      process.env.PATH = `${currentPath}:${pathsToAdd.join(":")}`;
    }
  }
}

// Phase 1: Initialize managers + IPC handlers before window content loads
// Best-effort cleanup of the orphaned portal restore-token file older builds wrote. See PR #904.

function syncAutoStartEntry() {
  try {
    if (autoStart.syncAutoStartEntry()) {
      debugLogger.info("Re-pointed the launch-at-login entry at the current executable");
    }
  } catch (error) {
    debugLogger.warn("Failed to sync the launch-at-login entry", { error: error?.message });
  }
}

// Reading the login item touches the OS, and failing to answer "did the session
// start us?" must not stop the app from starting at all. Falling back to false
// just shows the window, which is what every launch did before.
function wasLaunchedAtLoginHidden() {
  try {
    return autoStart.wasLaunchedAtLoginHidden();
  } catch (error) {
    if (debugLogger) debugLogger.warn("Failed to detect a login launch", { error: error?.message });
    return false;
  }
}

function initializeCoreManagers() {
  setupProductionPath();

  debugLogger = require("./src/helpers/debugLogger");
  debugLogger.ensureFileLogging();

  environmentManager = new EnvironmentManager();
  const uiLanguage = environmentManager.getUiLanguage(app.getLocale());
  changeLanguage(uiLanguage);
  debugLogger.refreshLogLevel();

  windowManager = new WindowManager();
  hotkeyManager = windowManager.hotkeyManager;
  databaseManager = new DatabaseManager();
  // Restore the last validated account scope before any window, IPC handler,
  // or meeting flow can read or create notes. Offline launches keep the
  // account's data visible; a stale or rotated credential fails the hash
  // check and restores nothing.
  const accountScopeBinding = require("./src/helpers/accountScopeBinding");
  const bootAccountId = accountScopeBinding.resolveBootAccountScope({
    token: require("./src/helpers/tokenStore").get(),
    binding: accountScopeBinding.read(),
  });
  if (bootAccountId) databaseManager.setActiveAccountId(bootAccountId);
  clipboardManager = new ClipboardManager();
  whisperManager = new WhisperManager();
  if (process.platform !== "darwin") {
    whisperCudaManager = new WhisperCudaManager();
    whisperVulkanManager = new WhisperVulkanManager();
    // Heal installs from before GPU packs got per-pack directories; must run
    // before startup pre-warm resolves any GPU binary path.
    const LlamaVulkanManager = require("./src/helpers/llamaVulkanManager");
    const llamaVulkanManager = new LlamaVulkanManager();
    const clearedPacks = migrateLegacyBinDir([
      whisperCudaManager,
      whisperVulkanManager,
      llamaVulkanManager,
    ]);
    if (clearedPacks.length > 0) {
      // No window exists yet — persist the notice; a control panel window
      // shows it as a toast and clears it. See #1606.
      require("./src/helpers/gpuPackMigrationNotice").record(clearedPacks);
    }
    // The 1.8.3 migration deleted lib-carrying packs without recording that
    // notice, leaving those users on a silent CPU fallback: an enabled flag
    // with no pack on disk only happens via such data loss. recordOnce gates
    // each pack to one notice so a dismissed toast doesn't return every launch.
    const orphanedPacks = detectOrphanedGpuPacks([
      { manager: whisperCudaManager, enabledEnvVar: "WHISPER_CUDA_ENABLED" },
      { manager: whisperVulkanManager, enabledEnvVar: "WHISPER_VULKAN_ENABLED" },
      { manager: llamaVulkanManager, enabledEnvVar: "LLAMA_VULKAN_ENABLED" },
    ]);
    if (orphanedPacks.length > 0) {
      require("./src/helpers/gpuPackMigrationNotice").recordOnce(orphanedPacks);
    }
    // Lets every server start resolve its GPU backend from installed packs
    whisperManager.setGpuBinaryManagers({ cuda: whisperCudaManager, vulkan: whisperVulkanManager });
  }
  parakeetManager = new ParakeetManager();
  windowsKeyManager = new WindowsKeyManager();
  linuxKeyManager = new LinuxKeyManager();
  textEditMonitor = new TextEditMonitor();
  selectionManager = new SelectionManager({ clipboardManager, textEditMonitor });
  syncAutoStartEntry();
  windowManager.textEditMonitor = textEditMonitor;
  windowManager.selectionManager = selectionManager;
  windowManager.windowsKeyManager = windowsKeyManager;
  windowManager.linuxKeyManager = linuxKeyManager;

  // IPC handlers must be registered before window content loads
  ipcHandlers = new IPCHandlers({
    environmentManager,
    databaseManager,
    clipboardManager,
    whisperManager,
    parakeetManager,
    windowManager,
    windowsKeyManager,
    linuxKeyManager,
    textEditMonitor,
    selectionManager,
    whisperCudaManager,
    whisperVulkanManager,
    googleCalendarManager,
    microsoftCalendarManager,
    appleCalendarManager,
    meetingDetectionEngine,
    audioTapManager,
    linuxPortalAudioManager,
    windowsLoopbackAudioManager,
    meetingAecManager,
    getTrayManager: () => trayManager,
    oauthProtocolRegistered: protocolRegistered,
    oauthProtocol: OAUTH_PROTOCOL,
  });
}

// Phase 2: Non-critical setup after windows are visible
function initializeDeferredManagers() {
  ensureYdotool().catch((err) => {
    require("./src/helpers/debugLogger").warn(
      "ydotool setup error",
      { error: err?.message },
      "clipboard"
    );
  });
  if (process.platform !== "darwin") {
    clipboardManager.preWarmAccessibility();
  }
  trayManager = new TrayManager();
  globeKeyManager = new GlobeKeyManager({
    // Lets the listener put the user's macOS Globe action back after a crash.
    preferenceStatePath: path.join(app.getPath("userData"), "globe-preference-state.json"),
  });

  if (process.platform === "darwin") {
    globeKeyManager.on("error", (error) => {
      if (globeKeyAlertShown) {
        return;
      }
      globeKeyAlertShown = true;

      const detailLines = [
        error?.message || i18nMain.t("startup.globeHotkey.details.unknown"),
        i18nMain.t("startup.globeHotkey.details.fallback"),
      ];

      if (process.env.NODE_ENV === "development") {
        detailLines.push(i18nMain.t("startup.globeHotkey.details.devHint"));
      } else {
        detailLines.push(i18nMain.t("startup.globeHotkey.details.reinstallHint"));
      }

      dialog.showMessageBox({
        type: "warning",
        title: i18nMain.t("startup.globeHotkey.title"),
        message: i18nMain.t("startup.globeHotkey.message"),
        detail: detailLines.join("\n\n"),
      });
    });
  }
}

// Deep links can arrive before windowManager exists (cold start) or before the
// renderer has mounted its listener. The token is stashed here and the renderer
// pulls it via `get-pending-invitation-token` on mount; the push below is a
// best-effort fast path for an already-running app.
let pendingInvitationDeepLinkToken = null;

ipcMain.handle("get-pending-invitation-token", () => {
  const token = pendingInvitationDeepLinkToken;
  pendingInvitationDeepLinkToken = null;
  return token;
});

// Main application startup
async function startApp() {
  // Await so a stale sidecar is confirmed dead before new ones can spawn and
  // contend for its port or storage lock.
  await reapStaleSidecars();
  laptopLidMonitor.start();

  // Phase 1: Core managers + IPC handlers before windows
  initializeCoreManagers();
  await environmentManager.init();
  await windowManager.setActivationModeCache(environmentManager.getActivationMode());
  windowManager.setFloatingIconAutoHide(environmentManager.getFloatingIconAutoHide());
  windowManager.setPanelStartPosition(environmentManager.getPanelStartPosition());

  let activationModeChangeQueue = Promise.resolve();
  ipcMain.on("activation-mode-changed", (_event, mode) => {
    activationModeChangeQueue = activationModeChangeQueue
      .then(async () => {
        const success = await windowManager.setActivationModeCache(mode);
        const effectiveMode = windowManager.getActivationMode();
        if (success) {
          environmentManager.saveActivationMode(effectiveMode);
        } else {
          for (const browserWindow of BrowserWindow.getAllWindows()) {
            if (!browserWindow.isDestroyed()) {
              browserWindow.webContents.send("setting-updated", {
                key: "activationMode",
                value: effectiveMode,
              });
            }
          }
        }
        windowManager.resetWindowsPushState();
        windowManager.reconcileNativeKeyListeners();
      })
      .catch((err) => {
        debugLogger.error("Failed to change activation mode", { error: err.message }, "hotkey");
      });
  });

  ipcMain.on("floating-icon-auto-hide-changed", (_event, enabled) => {
    windowManager.setFloatingIconAutoHide(enabled);
    environmentManager.saveFloatingIconAutoHide(enabled);
    // Relay to the floating icon window so it can react immediately
    if (windowManager.mainWindow && !windowManager.mainWindow.isDestroyed()) {
      windowManager.mainWindow.webContents.send("floating-icon-auto-hide-changed", enabled);
    }
  });

  ipcMain.on("start-minimized-changed", (_event, enabled) => {
    if (debugLogger) debugLogger.info("Start minimized changed", { enabled });
    environmentManager.saveStartMinimized(enabled);
  });

  ipcMain.on("panel-start-position-changed", (_event, position) => {
    windowManager.setPanelStartPosition(position);
    environmentManager.savePanelStartPosition(position);
  });

  dockManager.init();

  // In development, wait for Vite dev server to be ready
  if (process.env.NODE_ENV === "development") {
    await new Promise((resolve) => setTimeout(resolve, 500));
  }

  // Create windows FIRST so the user sees UI as soon as possible.
  // A login launch goes to the tray whatever the preference says: the user asked
  // the OS to start us, not to put a window in front of them at every login.
  const launchedHidden = wasLaunchedAtLoginHidden();
  const startMinimized = environmentManager.getStartMinimized() || launchedHidden;
  if (debugLogger) debugLogger.info("Start minimized", { enabled: startMinimized, launchedHidden });
  await windowManager.createMainWindow();
  // The activation mode was cached before the hotkey was registered, so a saved
  // Hold could not be checked against its key until now.
  if (
    windowManager.getActivationMode() === "push" &&
    !windowManager.hotkeyManager.supportsPushToTalk()
  ) {
    await windowManager.setActivationModeCache("tap");
    environmentManager.saveActivationMode("tap");
    for (const browserWindow of BrowserWindow.getAllWindows()) {
      if (!browserWindow.isDestroyed()) {
        browserWindow.webContents.send("setting-updated", { key: "activationMode", value: "tap" });
      }
    }
  }
  if (!startMinimized) {
    await windowManager.createControlPanelWindow();
  }

  // Windows/Linux cold start delivers protocol URLs via argv (macOS uses
  // open-url); without this scan a deep link that launches the app is lost.
  await hotkeyManager.hyprlandRegistrationReady;

  // Phase 2: Initialize remaining managers after windows are visible
  initializeDeferredManagers();
  if (process.platform === "darwin") {
    // Restore a Globe preference marker left by a crash without starting any
    // Accessibility-protected event monitors during onboarding.
    await globeKeyManager.restoreLeftoverSystemPreference();
  }

  if (process.platform === "win32") {
    const nircmdStatus = clipboardManager.getNircmdStatus();
    debugLogger.debug("Windows paste tool status", nircmdStatus);
  }

  trayManager.setWindows(windowManager.mainWindow, windowManager.controlPanelWindow);
  trayManager.setWindowManager(windowManager);
  // The tray's listen item is a toggle, so it has to rebuild when dictation
  // starts or stops.
  windowManager.onDictationStateChanged = () => trayManager.updateTrayMenu();
  trayManager.setCreateControlPanelCallback(() => windowManager.createControlPanelWindow());
  await trayManager.setVisible(
    process.platform !== "darwin" || environmentManager.getMenuBarIconVisible()
  );

  if (process.platform === "darwin") {
    const { isGlobeLikeHotkey, isMouseButtonHotkey } = require("./src/helpers/hotkeyManager");
    let globeKeyDownTime = 0;
    let globeKeyIsRecording = false;
    let globeLastStopTime = 0;
    const MIN_HOLD_DURATION_MS = 150;
    const POST_STOP_COOLDOWN_MS = 300;

    globeKeyManager.on("globe-down", async () => {
      const currentHotkey = hotkeyManager.getCurrentHotkey && hotkeyManager.getCurrentHotkey();
      const mainWindowLive = isLiveWindow(windowManager.mainWindow);
      debugLogger?.debug("[Globe] globe-down received", {
        currentHotkey,
        mainWindowLive,
        activationMode: mainWindowLive ? windowManager.getActivationMode() : "n/a",
      });

      // Forward to control panel for hotkey capture
      if (isLiveWindow(windowManager.controlPanelWindow)) {
        windowManager.controlPanelWindow.webContents.send("globe-key-pressed");
      }

      // Handle dictation if Globe/Fn is one of the dictation hotkeys
      const dictationUsesGlobe = hotkeyManager.getSlotHotkeys("dictation").some(isGlobeLikeHotkey);
      if (dictationUsesGlobe) {
        if (mainWindowLive && windowManager.isDictationProcessing()) {
          debugLogger?.debug("[Globe] Ignored — dictation processing");
        } else if (mainWindowLive) {
          // Capture target app PID BEFORE showing the overlay
          if (textEditMonitor) textEditMonitor.captureTargetPid();
          const activationMode = windowManager.getActivationMode();
          if (activationMode === "push") {
            const now = Date.now();
            if (now - globeLastStopTime < POST_STOP_COOLDOWN_MS) {
              debugLogger?.debug("[Globe] Ignored — cooldown active");
              return;
            }
            windowManager.showDictationPanel();
            windowManager.sendPrepareDictation();
            const pressTime = now;
            globeKeyDownTime = pressTime;
            globeKeyIsRecording = false;
            setTimeout(async () => {
              if (globeKeyDownTime === pressTime && !globeKeyIsRecording) {
                globeKeyIsRecording = true;
                debugLogger?.debug("[Globe] Starting dictation (push hold)");
                windowManager.sendStartDictation();
              }
            }, MIN_HOLD_DURATION_MS);
          } else {
            windowManager.sendToggleDictation();
          }
        } else {
          debugLogger?.debug("[Globe] Ignored — mainWindow not live");
        }
      }

      // Check voice agent slot for Globe/Fn key
      const voiceAgentUsesGlobe = hotkeyManager
        .getSlotHotkeys("voiceAgent")
        .some(isGlobeLikeHotkey);
      const translationUsesGlobe = hotkeyManager
        .getSlotHotkeys("translation")
        .some(isGlobeLikeHotkey);
      if (voiceAgentUsesGlobe) {
        windowManager.sendToggleVoiceAgent();
      }
      if (translationUsesGlobe) {
        windowManager.sendToggleTranslation();
      }
      if (!voiceAgentUsesGlobe && !translationUsesGlobe && !dictationUsesGlobe) {
        debugLogger?.debug("[Globe] Ignored — hotkey is not GLOBE", { currentHotkey });
      }
    });

    globeKeyManager.on("globe-up", async () => {
      debugLogger?.debug("[Globe] globe-up received", { wasRecording: globeKeyIsRecording });

      // Forward to control panel for hotkey capture (Fn key released)
      if (isLiveWindow(windowManager.controlPanelWindow)) {
        windowManager.controlPanelWindow.webContents.send("globe-key-released");
      }

      if (hotkeyManager.getSlotHotkeys("dictation").some(isGlobeLikeHotkey)) {
        const activationMode = windowManager.getActivationMode();
        if (activationMode === "push") {
          if (globeKeyDownTime === 0 && !globeKeyIsRecording) {
            // The press was ignored (dictation was processing); releasing it
            // must not cancel preparation or hide the thinking pill.
            debugLogger?.debug("[Globe] Release without a registered press — ignored");
          } else {
            globeKeyDownTime = 0;
            globeLastStopTime = Date.now();
            if (globeKeyIsRecording) {
              globeKeyIsRecording = false;
              debugLogger?.debug("[Globe] Stopping dictation (push release)");
              windowManager.sendStopDictation();
            } else {
              windowManager.sendCancelDictationPreparation();
              windowManager.hideDictationPanel();
            }
          }
        }
      }

      // Fn release also stops compound push-to-talk for Fn+F-key hotkeys
      windowManager.handleMacPushModifierUp("fn");
    });

    // Another key was pressed while Fn was held — user is using Fn as a
    // navigation modifier (Fn+Arrow → Home, Fn+Backspace → Forward Delete, etc.).
    // Cancel any bare-Fn push-to-talk in progress instead of transcribing noise.
    // Only the bare-Fn path uses globeKeyDownTime/globeKeyIsRecording, so compound
    // Fn-hotkey push-to-talk and tap mode are untouched.
    globeKeyManager.on("globe-interrupted", () => {
      if (globeKeyDownTime === 0 && !globeKeyIsRecording) {
        return;
      }
      const wasRecording = globeKeyIsRecording;
      debugLogger?.debug("[Globe] Fn+key interrupted push-to-talk", { wasRecording });
      globeKeyDownTime = 0;
      globeKeyIsRecording = false;
      globeLastStopTime = Date.now();
      if (wasRecording) {
        windowManager.sendCancelDictation();
      } else {
        windowManager.sendCancelDictationPreparation();
        windowManager.hideDictationPanel();
      }
    });

    globeKeyManager.on("modifier-up", (modifier) => {
      if (windowManager?.handleMacPushModifierUp) {
        windowManager.handleMacPushModifierUp(modifier);
      }
    });

    // Right-side single modifier handling (e.g., RightOption as hotkey)
    let rightModDownTime = 0;
    let rightModIsRecording = false;
    let rightModLastStopTime = 0;
    let rightModActiveKey = null;

    globeKeyManager.on("right-modifier-down", async (modifier) => {
      // Check voice agent slot for right-modifier
      if (hotkeyManager.slotHasHotkey("voiceAgent", modifier)) {
        windowManager.sendToggleVoiceAgent();
      }
      if (hotkeyManager.slotHasHotkey("translation", modifier)) {
        windowManager.sendToggleTranslation();
      }

      if (!hotkeyManager.slotHasHotkey("dictation", modifier)) return;
      if (!isLiveWindow(windowManager.mainWindow)) return;
      if (windowManager.isDictationProcessing()) return;

      const activationMode = windowManager.getActivationMode();
      if (textEditMonitor) textEditMonitor.captureTargetPid();
      if (activationMode === "push") {
        if (rightModActiveKey && rightModActiveKey !== modifier) return;
        const now = Date.now();
        if (now - rightModLastStopTime < POST_STOP_COOLDOWN_MS) return;
        windowManager.showDictationPanel();
        windowManager.sendPrepareDictation();
        const pressTime = now;
        rightModActiveKey = modifier;
        rightModDownTime = pressTime;
        rightModIsRecording = false;
        setTimeout(() => {
          if (rightModDownTime === pressTime && !rightModIsRecording) {
            rightModIsRecording = true;
            windowManager.sendStartDictation();
          }
        }, MIN_HOLD_DURATION_MS);
      } else {
        windowManager.sendToggleDictation();
      }
    });

    globeKeyManager.on("right-modifier-up", async (modifier) => {
      if (hotkeyManager.slotHasHotkey("dictation", modifier)) {
        if (!isLiveWindow(windowManager.mainWindow)) return;

        const activationMode = windowManager.getActivationMode();
        if (activationMode === "push" && (!rightModActiveKey || rightModActiveKey === modifier)) {
          if (rightModDownTime === 0 && !rightModIsRecording) {
            // The press was ignored (dictation was processing); releasing it
            // must not cancel preparation or hide the thinking pill.
            debugLogger?.debug("[RightMod] Release without a registered press — ignored");
          } else {
            rightModActiveKey = null;
            rightModDownTime = 0;
            rightModLastStopTime = Date.now();
            if (rightModIsRecording) {
              rightModIsRecording = false;
              windowManager.sendStopDictation();
            } else {
              windowManager.sendCancelDictationPreparation();
              windowManager.hideDictationPanel();
            }
          }
        }
      }

      const rightModToBase = {
        RightCommand: "command",
        RightOption: "option",
        RightControl: "control",
        RightShift: "shift",
      };
      const baseMod = rightModToBase[modifier];
      if (baseMod && windowManager?.handleMacPushModifierUp) {
        windowManager.handleMacPushModifierUp(baseMod);
      }
    });

    const MAC_NATIVE_HOTKEY_SLOTS = ["dictation", "voiceAgent", "translation"];
    const syncMacNativeHotkeyConfiguration = () => {
      globeKeyManager.setConfiguration(
        hotkeyManager.getMacNativeListenerConfig(MAC_NATIVE_HOTKEY_SLOTS)
      );
    };

    // Mouse Button 4/5 handling (e.g., Logitech MX Master side buttons)
    let mouseButtonDownTime = 0;
    let mouseButtonIsRecording = false;
    let mouseButtonLastStopTime = 0;
    let mouseButtonActiveButton = null;

    globeKeyManager.on("mouse-button-down", async (button) => {
      if (hotkeyManager.isInListeningMode && hotkeyManager.isInListeningMode()) return;
      if (!isMouseButtonHotkey(button)) return;

      if (hotkeyManager.slotHasHotkey("voiceAgent", button)) {
        windowManager.sendToggleVoiceAgent();
      }
      if (hotkeyManager.slotHasHotkey("translation", button)) {
        windowManager.sendToggleTranslation();
      }

      if (!hotkeyManager.slotHasHotkey("dictation", button)) return;
      if (!isLiveWindow(windowManager.mainWindow)) return;
      if (windowManager.isDictationProcessing()) return;

      const activationMode = windowManager.getActivationMode();
      if (textEditMonitor) textEditMonitor.captureTargetPid();

      if (activationMode === "push") {
        if (mouseButtonActiveButton && mouseButtonActiveButton !== button) return;
        const now = Date.now();
        if (now - mouseButtonLastStopTime < POST_STOP_COOLDOWN_MS) return;
        windowManager.showDictationPanel();
        windowManager.sendPrepareDictation();
        const pressTime = now;
        mouseButtonActiveButton = button;
        mouseButtonDownTime = pressTime;
        mouseButtonIsRecording = false;
        setTimeout(() => {
          if (mouseButtonDownTime === pressTime && !mouseButtonIsRecording) {
            mouseButtonIsRecording = true;
            windowManager.sendStartDictation();
          }
        }, MIN_HOLD_DURATION_MS);
      } else {
        windowManager.sendToggleDictation();
      }
    });

    globeKeyManager.on("mouse-button-up", async (button) => {
      if (hotkeyManager.isInListeningMode && hotkeyManager.isInListeningMode()) return;
      if (!isMouseButtonHotkey(button)) return;

      if (!hotkeyManager.slotHasHotkey("dictation", button)) return;
      if (!isLiveWindow(windowManager.mainWindow)) return;

      const activationMode = windowManager.getActivationMode();
      if (
        activationMode === "push" &&
        (!mouseButtonActiveButton || mouseButtonActiveButton === button)
      ) {
        if (mouseButtonDownTime === 0 && !mouseButtonIsRecording) {
          // The press was ignored (dictation was processing); releasing it
          // must not cancel preparation or hide the thinking pill.
          debugLogger?.debug("[MouseButton] Release without a registered press — ignored");
        } else {
          mouseButtonActiveButton = null;
          mouseButtonDownTime = 0;
          mouseButtonLastStopTime = Date.now();
          if (mouseButtonIsRecording) {
            mouseButtonIsRecording = false;
            windowManager.sendStopDictation();
          } else {
            windowManager.sendCancelDictationPreparation();
            windowManager.hideDictationPanel();
          }
        }
      }
    });

    // If accessibility is missing, notify the normal control panel after the
    // protected macOS features have started. During onboarding the permissions
    // screen owns this guidance, so its event has no ControlPanel listener.
    const checkAndNotifyAccessibility = () => {
      if (!systemPreferences.isTrustedAccessibilityClient(false)) {
        debugLogger.info("[Accessibility] macOS accessibility not trusted — notifying renderers");
        if (isLiveWindow(windowManager.controlPanelWindow)) {
          windowManager.controlPanelWindow.webContents.send("accessibility-missing");
        }
      }
    };

    let accessibilityFeaturesStarted = false;
    startMacAccessibilityFeatures = () => {
      if (accessibilityFeaturesStarted) return;
      accessibilityFeaturesStarted = true;
      clipboardManager.preWarmAccessibility();
      syncMacNativeHotkeyConfiguration();
      globeKeyManager.start();
      setTimeout(checkAndNotifyAccessibility, 3000);
    };

    if (macAccessibilityFeaturesReady) {
      startMacAccessibilityFeatures();
    }

    hotkeyManager.on("hotkey-loaded", syncMacNativeHotkeyConfiguration);

    ipcMain.on("hotkey-listening-mode-changed", (_event, enabled) => {
      if (enabled) {
        startMacAccessibilityFeatures();
        // Let mouse buttons through so they can be captured, but keep macOS's
        // Globe action down so choosing Globe cannot flash the emoji viewer.
        globeKeyManager.setConfiguration({ mouseButtons: [], suppressGlobeAction: true });
      } else {
        syncMacNativeHotkeyConfiguration();
      }
    });

    // Allow renderer to request an accessibility check (e.g. on sign-in).
    // Also sends accessibility-missing events if untrusted.
    ipcMain.handle("check-accessibility-trusted", () => {
      const trusted = systemPreferences.isTrustedAccessibilityClient(false);
      if (!trusted) {
        checkAndNotifyAccessibility();
      }
      return trusted;
    });

    // Reset native key state when hotkey changes
    ipcMain.on("hotkey-changed", (_event, _newHotkey) => {
      globeKeyDownTime = 0;
      globeKeyIsRecording = false;
      globeLastStopTime = 0;
      rightModDownTime = 0;
      rightModIsRecording = false;
      rightModLastStopTime = 0;
      mouseButtonDownTime = 0;
      mouseButtonIsRecording = false;
      mouseButtonLastStopTime = 0;
      syncMacNativeHotkeyConfiguration();
    });
  }

  // Windows and Linux share the same native low-level key listener model: one hook
  // process per watched key (Electron globalShortcut can't see modifier-only or
  // right-side-modifier combos), routed to the owning slot here. macOS is handled
  // separately above via globeKeyManager.
  if (process.platform === "win32" || process.platform === "linux") {
    const isWindows = process.platform === "win32";
    const nativeKeyManager = isWindows ? windowsKeyManager : linuxKeyManager;
    debugLogger.debug("[Push-to-Talk] Native key listener setup starting");

    // Dictation supports push-to-talk and needs the overlay window; meeting
    // drives other windows (matching their globalShortcut callbacks and macOS).
    const dispatchNativeKeyDown = (key) => {
      if (hotkeyManager.slotHasHotkey("dictation", key)) {
        if (!isLiveWindow(windowManager.mainWindow)) return;
        if (windowManager.getActivationMode() === "push") {
          windowManager.startWindowsPushToTalk(key);
        } else {
          windowManager.sendToggleDictation();
        }
        return;
      }
      if (hotkeyManager.slotHasHotkey("voiceAgent", key)) {
        windowManager.sendToggleVoiceAgent();
      } else if (hotkeyManager.slotHasHotkey("translation", key)) {
        windowManager.sendToggleTranslation();
      } else if (hotkeyManager.slotHasHotkey("meeting", key)) {
        windowManager.startManualMeeting();
      }
    };

    // Only dictation drives push-to-talk, so only its key-up matters.
    const dispatchNativeKeyUp = (key) => {
      if (!hotkeyManager.slotHasHotkey("dictation", key)) return;
      if (windowManager.winPushState?.active) {
        windowManager.handleWindowsPushKeyUp(key);
      } else if (
        isLiveWindow(windowManager.mainWindow) &&
        windowManager.getActivationMode() === "push"
      ) {
        windowManager.handleWindowsPushKeyUp(key);
      }
    };

    nativeKeyManager.on("key-down", dispatchNativeKeyDown);
    nativeKeyManager.on("key-up", dispatchNativeKeyUp);

    nativeKeyManager.on("error", (error) => {
      debugLogger.warn("[Push-to-Talk] Native key listener error", { error: error.message });
      if (isWindows && isLiveWindow(windowManager.mainWindow)) {
        windowManager.mainWindow.webContents.send("windows-ptt-unavailable", {
          reason: "error",
          message: error.message,
        });
      }
    });

    nativeKeyManager.on("unavailable", () => {
      debugLogger.debug(
        "[Push-to-Talk] Native key listener unavailable - falling back to toggle mode"
      );
      if (isWindows && isLiveWindow(windowManager.mainWindow)) {
        windowManager.mainWindow.webContents.send("windows-ptt-unavailable", {
          reason: "binary_not_found",
          message: i18nMain.t("windows.pttUnavailable"),
        });
      }
    });

    nativeKeyManager.on("ready", () => {
      debugLogger.debug("[Push-to-Talk] Native key listener ready and listening");
    });

    if (!isWindows) {
      nativeKeyManager.on("permission-denied", () => {
        debugLogger.warn(
          "[Push-to-Talk] Linux key listener has no permission to access input devices"
        );
        if (isLiveWindow(windowManager.mainWindow)) {
          windowManager.mainWindow.webContents.send("linux-ptt-permission-denied");
        }
      });
    }

    const STARTUP_DELAY_MS = 3000;
    setTimeout(() => windowManager.reconcileNativeKeyListeners(), STARTUP_DELAY_MS);

    ipcMain.on("hotkey-changed", () => {
      windowManager.resetWindowsPushState();
      windowManager.reconcileNativeKeyListeners();
    });
  }
}

ipcMain.on("mac-accessibility-features-ready", (_event, expectedAccountScope) => {
  if (process.platform !== "darwin") return;
  if (expectedAccountScope) {
    const accountScopeBinding = require("./src/helpers/accountScopeBinding");
    const currentAccountScope = accountScopeBinding.resolveActiveAccountScope({
      ...require("./src/helpers/tokenStore").getState(),
      binding: accountScopeBinding.read(),
    });
    if (!accountScopeBinding.matchesActiveAccountScope(expectedAccountScope, currentAccountScope)) {
      debugLogger.info("[Accessibility] Ignoring stale account-scoped readiness signal");
      return;
    }
  }
  macAccessibilityFeaturesReady = true;
  startMacAccessibilityFeatures?.();
});

// Listen for usage limit reached from dictation overlay, forward to control panel
ipcMain.on("limit-reached", (_event, data) => {
  if (isLiveWindow(windowManager?.controlPanelWindow)) {
    windowManager.controlPanelWindow.webContents.send("limit-reached", data);
  }
});

// App event handlers
if (gotSingleInstanceLock) {
  app.on("second-instance", async (_event) => {
    await app.whenReady();
    if (!windowManager) {
      return;
    }

    if (isLiveWindow(windowManager.controlPanelWindow)) {
      if (windowManager.controlPanelWindow.isMinimized()) {
        windowManager.controlPanelWindow.restore();
      }
      windowManager.controlPanelWindow.show();
      windowManager.controlPanelWindow.focus();
      dockManager.setControlPanelVisible(true);
      if (windowManager.controlPanelWindow.webContents.isCrashed()) {
        windowManager.loadControlPanel();
      }
    } else {
      windowManager.createControlPanelWindow();
    }

    if (isLiveWindow(windowManager.mainWindow)) {
      windowManager.enforceMainWindowOnTop();
    } else {
      windowManager.createMainWindow();
    }
  });

  app
    .whenReady()
    .then(() => {
      // On Linux, --enable-transparent-visuals requires a short delay before creating
      // windows to allow the compositor to set up the ARGB visual correctly.
      // Without this delay, transparent windows flicker on both X11 and Wayland.
      const delay = process.platform === "linux" ? 300 : 0;
      return new Promise((resolve) => setTimeout(resolve, delay));
    })
    .then(() => {
      if (process.platform === "win32") {
        session.defaultSession.setDisplayMediaRequestHandler((_request, callback) => {
          // Only the loopback audio track is used; the video source is
          // discarded by the renderer, so skip thumbnail generation.
          desktopCapturer
            .getSources({ types: ["screen"], thumbnailSize: { width: 0, height: 0 } })
            .then((sources) => {
              if (sources.length > 0) {
                callback({ video: sources[0], audio: "loopback" });
              } else {
                callback(null);
              }
            })
            .catch((error) => {
              console.error("Display media request failed:", error);
              callback(null);
            });
        });
      }

      startApp().catch((error) => {
        console.error("Failed to start app:", error);
        dialog.showErrorBox(
          i18nMain.t("startup.error.title"),
          i18nMain.t("startup.error.message", { error: error.message })
        );
        app.exit(1);
      });
    });

  app.on("window-all-closed", () => {
    // Don't quit on macOS when all windows are closed
    // The app should stay in the dock/menu bar
    if (process.platform !== "darwin") {
      app.quit();
    }
    // On macOS, keep the app running even without windows
  });

  app.on("browser-window-focus", (event, window) => {
    // Only apply always-on-top to the dictation window, not the control panel
    if (windowManager && isLiveWindow(windowManager.mainWindow)) {
      // Check if the focused window is the dictation window
      if (window === windowManager.mainWindow) {
        windowManager.enforceMainWindowOnTop();
      }
    }

    // Control panel doesn't need any special handling on focus
    // It should behave like a normal window
  });

  app.on("activate", () => {
    // On macOS, re-create windows when dock icon is clicked
    if (BrowserWindow.getAllWindows().length === 0) {
      if (windowManager) {
        windowManager.createMainWindow();
        windowManager.createControlPanelWindow();
      }
    } else {
      // Show control panel when dock icon is clicked (most common user action)
      if (windowManager && isLiveWindow(windowManager.controlPanelWindow)) {
        if (windowManager.controlPanelWindow.isMinimized()) {
          windowManager.controlPanelWindow.restore();
        }
        windowManager.controlPanelWindow.show();
        windowManager.controlPanelWindow.focus();
        dockManager.setControlPanelVisible(true);
      } else if (windowManager) {
        // If control panel doesn't exist, create it
        windowManager.createControlPanelWindow();
      }

      // Ensure dictation panel maintains its always-on-top status
      if (windowManager && isLiveWindow(windowManager.mainWindow)) {
        windowManager.enforceMainWindowOnTop();
      }
    }
  });

  let isShuttingDown = false;
  app.on("before-quit", (event) => {
    if (isShuttingDown) return;
    isShuttingDown = true;
    event.preventDefault();
    performSyncTeardown();
    sidecarRegistry.shutdownAll().finally(() => app.exit(0));
  });
}

function performSyncTeardown() {
  laptopLidMonitor.stop();
  if (wakeRewarmTimer) {
    clearTimeout(wakeRewarmTimer);
    wakeRewarmTimer = null;
  }
  if (authBridgeServer) {
    authBridgeServer.close();
    authBridgeServer = null;
  }
  if (cliBridge) {
    cliBridge.stop().catch(() => {});
    cliBridge = null;
  }
  if (hotkeyManager) {
    hotkeyManager.unregisterAll();
  } else {
    globalShortcut.unregisterAll();
  }
  if (globeKeyManager) globeKeyManager.stop();
  if (windowsKeyManager) windowsKeyManager.stop();
  if (linuxKeyManager) linuxKeyManager.stop();
  if (meetingDetectionEngine) meetingDetectionEngine.stop();
  if (googleCalendarManager) googleCalendarManager.stop();
  if (microsoftCalendarManager) microsoftCalendarManager.stop();
  if (appleCalendarManager) appleCalendarManager.stop();
  if (calendarReminderScheduler) calendarReminderScheduler.stop();
  if (audioTapManager) audioTapManager.stop().catch(() => {});
  if (linuxPortalAudioManager) linuxPortalAudioManager.stop().catch(() => {});
  if (windowsLoopbackAudioManager) windowsLoopbackAudioManager.stop().catch(() => {});
  if (meetingAecManager) meetingAecManager.stop().catch(() => {});
  if (ipcHandlers) ipcHandlers._cleanupTextEditMonitor();
  if (textEditMonitor) textEditMonitor.stopMonitoring();
}
