const { contextBridge, ipcRenderer, webUtils } = require("electron");

// BYOK API-key bridges, built once instead of hand-listed per key. Sandboxed
// preloads can't require local modules, so the {base, get, save} tuples are
// inlined here; keep them in sync with the BYOK_API_KEYS manifest in
// src/config/secretKeys.js (the main process derives its plumbing from that).
const BYOK_KEY_BRIDGES = [
  { base: "openai", get: "getOpenAIKey", save: "saveOpenAIKey" },
  { base: "anthropic", get: "getAnthropicKey", save: "saveAnthropicKey" },
  { base: "gemini", get: "getGeminiKey", save: "saveGeminiKey" },
  { base: "groq", get: "getGroqKey", save: "saveGroqKey" },
  { base: "xai", get: "getXaiKey", save: "saveXaiKey" },
  { base: "mistral", get: "getMistralKey", save: "saveMistralKey" },
  { base: "openrouter", get: "getOpenrouterKey", save: "saveOpenrouterKey" },
  { base: "tinfoil", get: "getTinfoilKey", save: "saveTinfoilKey" },
  { base: "corti", get: "getCortiKey", save: "saveCortiKey" },
  { base: "deepgram", get: "getDeepgramKey", save: "saveDeepgramKey" },
  { base: "assemblyai", get: "getAssemblyAIKey", save: "saveAssemblyAIKey" },
  {
    base: "note-formatting-custom",
    get: "getNoteFormattingCustomKey",
    save: "saveNoteFormattingCustomKey",
  },
  { base: "translation-custom", get: "getTranslationCustomKey", save: "saveTranslationCustomKey" },
  {
    base: "dictation-agent-custom",
    get: "getDictationAgentCustomKey",
    save: "saveDictationAgentCustomKey",
  },
  {
    base: "dictation-agent-vision-custom",
    get: "getDictationAgentVisionCustomKey",
    save: "saveDictationAgentVisionCustomKey",
  },
  { base: "chat-agent-custom", get: "getChatAgentCustomKey", save: "saveChatAgentCustomKey" },
];
const secretKeyApi = {};
for (const k of BYOK_KEY_BRIDGES) {
  secretKeyApi[k.get] = () => ipcRenderer.invoke(`get-${k.base}-key`);
  secretKeyApi[k.save] = (key) => ipcRenderer.invoke(`save-${k.base}-key`, key);
}

/**
 * Helper to register an IPC listener and return a cleanup function.
 * Ensures renderer code can easily remove listeners to avoid leaks.
 */
const registerListener = (channel, handlerFactory) => {
  return (callback) => {
    if (typeof callback !== "function") {
      return () => {};
    }

    const listener =
      typeof handlerFactory === "function"
        ? handlerFactory(callback)
        : (event, ...args) => callback(event, ...args);

    ipcRenderer.on(channel, listener);
    return () => {
      ipcRenderer.removeListener(channel, listener);
    };
  };
};

contextBridge.exposeInMainWorld("electronAPI", {
  setOnboardingWindowMode: (mode) => ipcRenderer.invoke("onboarding-set-window-mode", mode),
  setOnboardingActive: (active) => ipcRenderer.invoke("onboarding-set-active", active),
  markMacAccessibilityFeaturesReady: (expectedAccountScope) =>
    expectedAccountScope
      ? ipcRenderer.send("mac-accessibility-features-ready", expectedAccountScope)
      : ipcRenderer.send("mac-accessibility-features-ready"),
  onOnboardingDemoEvent: registerListener(
    "onboarding-demo-event",
    (callback) => (_event, payload) => callback(payload)
  ),
  testProviderConnection: (config) => ipcRenderer.invoke("test-provider-connection", config),
  pasteText: (text, options) => ipcRenderer.invoke("paste-text", text, options),
  captureSelectedText: (options) => ipcRenderer.invoke("capture-selected-text", options),
  replaceSelectedText: (sessionId, text, options) =>
    ipcRenderer.invoke("replace-selected-text", sessionId, text, options),
  pasteAtCapturedTarget: (sessionId, text, options) =>
    ipcRenderer.invoke("paste-at-captured-target", sessionId, text, options),
  hideWindow: () => ipcRenderer.invoke("hide-window"),
  showDictationPanel: () => ipcRenderer.invoke("show-dictation-panel"),
  captureDictationTarget: () => ipcRenderer.invoke("capture-dictation-target"),
  onToggleDictation: registerListener(
    "toggle-dictation",
    (callback) => (_event, options) => callback(options)
  ),
  onToggleVoiceAgent: registerListener(
    "toggle-voice-agent",
    (callback) => (_event, options) => callback(options)
  ),
  onToggleTranslation: registerListener(
    "toggle-translation",
    (callback) => (_event, options) => callback(options)
  ),
  onOpenAssistantPanel: registerListener("open-assistant-panel", (callback) => () => callback()),
  onStartDictation: registerListener(
    "start-dictation",
    (callback) => (_event, options) => callback(options)
  ),
  onStopDictation: registerListener("stop-dictation", (callback) => () => callback()),
  onPrepareDictation: registerListener(
    "prepare-dictation",
    (callback) => (_event, options) => callback(options)
  ),
  onCancelDictationPreparation: registerListener(
    "cancel-dictation-preparation",
    (callback) => () => callback()
  ),
  onCancelDictation: registerListener("cancel-dictation", (callback) => () => callback()),
  onDictationForceStopped: registerListener(
    "dictation-force-stopped",
    (callback) => (_event, payload) => callback(payload)
  ),
  micWarmHoldChanged: (active) => ipcRenderer.send("mic-warm-hold-changed", active),
  dictationLifecycleStateChanged: (state, inputKind) =>
    ipcRenderer.send("dictation-lifecycle-state-changed", state, inputKind),
  dictationAudioLevelChanged: (level) => ipcRenderer.send("dictation-audio-level-changed", level),
  onAgentDictationPillStateChanged: registerListener(
    "agent-dictation-pill-state-changed",
    (callback) => (_event, state) => callback(state)
  ),
  onAgentDictationPillAudioLevelChanged: registerListener(
    "agent-dictation-pill-audio-level-changed",
    (callback) => (_event, level) => callback(level)
  ),
  showAgentDictationFinalTranscript: (text) =>
    ipcRenderer.send("show-agent-dictation-final-transcript", text),
  onAgentDictationPillFinalTranscript: registerListener(
    "agent-dictation-pill-final-transcript",
    (callback) => (_event, text) => callback(text)
  ),

  // Database functions
  saveTranscription: (text, rawText, options) =>
    ipcRenderer.invoke("db-save-transcription", text, rawText, options),
  getTranscriptions: (limit, options) =>
    ipcRenderer.invoke("db-get-transcriptions", limit, options),
  recordAnalyticsEvent: (input) => ipcRenderer.invoke("analytics-record-event", input),
  getAnalyticsSummary: () => ipcRenderer.invoke("analytics-get-summary"),
  clearTranscriptions: () => ipcRenderer.invoke("db-clear-transcriptions"),
  deleteTranscription: (id) => ipcRenderer.invoke("db-delete-transcription", id),

  // Audio storage functions
  saveTranscriptionAudio: (id, audioBuffer, metadata) =>
    ipcRenderer.invoke("save-transcription-audio", id, audioBuffer, metadata),
  getAudioPath: (id) => ipcRenderer.invoke("get-audio-path", id),
  showAudioInFolder: (id) => ipcRenderer.invoke("show-audio-in-folder", id),
  getAudioBuffer: (id) => ipcRenderer.invoke("get-audio-buffer", id),
  deleteTranscriptionAudio: (id) => ipcRenderer.invoke("delete-transcription-audio", id),
  getAudioStorageUsage: () => ipcRenderer.invoke("get-audio-storage-usage"),
  deleteAllAudio: () => ipcRenderer.invoke("delete-all-audio"),
  syncRetentionSettings: (settings) => ipcRenderer.send("retention-settings-changed", settings),
  retryTranscription: (id, settings) => ipcRenderer.invoke("retry-transcription", id, settings),
  updateTranscriptionText: (id, text, rawText) =>
    ipcRenderer.invoke("update-transcription-text", id, text, rawText),
  getTranscriptionById: (id) => ipcRenderer.invoke("get-transcription-by-id", id),

  // Dictionary functions
  getDictionary: () => ipcRenderer.invoke("db-get-dictionary"),
  setDictionary: (words) => ipcRenderer.invoke("db-set-dictionary", words),
  applyDictionaryChanges: (changes) => ipcRenderer.invoke("db-apply-dictionary-changes", changes),
  onDictionaryUpdated: (callback) => {
    const listener = (_event, words) => callback?.(words);
    ipcRenderer.on("dictionary-updated", listener);
    return () => ipcRenderer.removeListener("dictionary-updated", listener);
  },
  getSnippets: () => ipcRenderer.invoke("db-get-snippets"),
  setSnippets: (snippets) => ipcRenderer.invoke("db-set-snippets", snippets),
  onSnippetsUpdated: (callback) => {
    const listener = (_event, snippets) => callback?.(snippets);
    ipcRenderer.on("snippets-updated", listener);
    return () => ipcRenderer.removeListener("snippets-updated", listener);
  },
  setAutoLearnEnabled: (enabled) => ipcRenderer.send("auto-learn-changed", enabled),
  onCorrectionsLearned: (callback) => {
    const listener = (_event, words) => callback?.(words);
    ipcRenderer.on("corrections-learned", listener);
    return () => ipcRenderer.removeListener("corrections-learned", listener);
  },
  undoLearnedCorrections: (words) => ipcRenderer.invoke("undo-learned-corrections", words),

  // Note functions
  saveNote: (title, content, noteType, sourceFile, audioDuration, folderId, spaceId) =>
    ipcRenderer.invoke(
      "db-save-note",
      title,
      content,
      noteType,
      sourceFile,
      audioDuration,
      folderId,
      spaceId
    ),
  exportTranscript: (noteId, format) => ipcRenderer.invoke("export-transcript", noteId, format),
  exportDictionary: (words) => ipcRenderer.invoke("export-dictionary", words),
  onSemanticReindexProgress: (callback) => {
    const listener = (_event, data) => callback?.(data);
    ipcRenderer.on("semantic-reindex-progress", listener);
    return () => ipcRenderer.removeListener("semantic-reindex-progress", listener);
  },
  onActiveAccountScopeChanged: registerListener(
    "active-account-scope-changed",
    (callback) => (_event, scope) => callback(scope)
  ),
  onSpacePurged: (callback) => {
    const listener = (_event, payload) => callback?.(payload);
    ipcRenderer.on("space-purged", listener);
    return () => ipcRenderer.removeListener("space-purged", listener);
  },
  onSpaceSynced: (callback) => {
    const listener = (_event, space) => callback?.(space);
    ipcRenderer.on("space-synced", listener);
    return () => ipcRenderer.removeListener("space-synced", listener);
  },
  selectAudioFile: (options) => ipcRenderer.invoke("select-audio-file", options),
  cancelUploadTranscription: (requestId) =>
    ipcRenderer.invoke("cancel-upload-transcription", requestId),
  transcribeAudioFileByok: (options) => ipcRenderer.invoke("transcribe-audio-file-byok", options),
  getFileSize: (filePath) => ipcRenderer.invoke("get-file-size", filePath),
  getPathForFile: (file) => {
    const filePath = webUtils.getPathForFile(file);
    // Register real dropped-file paths so the main-process audio allowlist accepts them.
    if (filePath) ipcRenderer.send("approve-audio-path", filePath);
    return filePath;
  },
  deleteTempFile: (filePath) => ipcRenderer.invoke("delete-temp-file", filePath),
  onUrlDownloadProgress: registerListener(
    "url-download-progress",
    (callback) => (_event, data) => callback(data)
  ),

  onNoteAdded: (callback) => {
    const listener = (_event, note) => callback?.(note);
    ipcRenderer.on("note-added", listener);
    return () => ipcRenderer.removeListener("note-added", listener);
  },
  onNoteUpdated: (callback) => {
    const listener = (_event, note) => callback?.(note);
    ipcRenderer.on("note-updated", listener);
    return () => ipcRenderer.removeListener("note-updated", listener);
  },
  onNoteDeleted: (callback) => {
    const listener = (_event, data) => callback?.(data);
    ipcRenderer.on("note-deleted", listener);
    return () => ipcRenderer.removeListener("note-deleted", listener);
  },
  onNoteSynced: (callback) => {
    const listener = (_event, note) => callback?.(note);
    ipcRenderer.on("note-synced", listener);
    return () => ipcRenderer.removeListener("note-synced", listener);
  },
  onFolderSynced: (callback) => {
    const listener = (_event, folder) => callback?.(folder);
    ipcRenderer.on("folder-synced", listener);
    return () => ipcRenderer.removeListener("folder-synced", listener);
  },
  onFolderDeleted: (callback) => {
    const listener = (_event, data) => callback?.(data);
    ipcRenderer.on("folder-deleted", listener);
    return () => ipcRenderer.removeListener("folder-deleted", listener);
  },
  onSyncEvent: (callback) => {
    const listener = (_event, data) => callback?.(data);
    ipcRenderer.on("sync-event", listener);
    return () => ipcRenderer.removeListener("sync-event", listener);
  },

  onActionCreated: (callback) => {
    const listener = (_event, action) => callback?.(action);
    ipcRenderer.on("action-created", listener);
    return () => ipcRenderer.removeListener("action-created", listener);
  },
  onActionUpdated: (callback) => {
    const listener = (_event, action) => callback?.(action);
    ipcRenderer.on("action-updated", listener);
    return () => ipcRenderer.removeListener("action-updated", listener);
  },
  onActionDeleted: (callback) => {
    const listener = (_event, data) => callback?.(data);
    ipcRenderer.on("action-deleted", listener);
    return () => ipcRenderer.removeListener("action-deleted", listener);
  },

  onTranscriptionAdded: (callback) => {
    const listener = (_event, transcription) => callback?.(transcription);
    ipcRenderer.on("transcription-added", listener);
    return () => ipcRenderer.removeListener("transcription-added", listener);
  },
  onTranscriptionDeleted: (callback) => {
    const listener = (_event, data) => callback?.(data);
    ipcRenderer.on("transcription-deleted", listener);
    return () => ipcRenderer.removeListener("transcription-deleted", listener);
  },
  onTranscriptionsCleared: (callback) => {
    const listener = (_event, data) => callback?.(data);
    ipcRenderer.on("transcriptions-cleared", listener);
    return () => ipcRenderer.removeListener("transcriptions-cleared", listener);
  },
  onTranscriptionUpdated: (callback) => {
    const listener = (_event, transcription) => callback?.(transcription);
    ipcRenderer.on("transcription-updated", listener);
    return () => ipcRenderer.removeListener("transcription-updated", listener);
  },
  onAnalyticsChanged: (callback) => {
    const listener = () => callback?.();
    ipcRenderer.on("analytics-changed", listener);
    return () => ipcRenderer.removeListener("analytics-changed", listener);
  },

  // BYOK API keys (get/save for every provider in the secretKeys manifest)
  ...secretKeyApi,

  // Clipboard functions
  checkAccessibilityPermission: (silent) =>
    ipcRenderer.invoke("check-accessibility-permission", silent),
  promptAccessibilityPermission: () => ipcRenderer.invoke("prompt-accessibility-permission"),
  readClipboard: () => ipcRenderer.invoke("read-clipboard"),
  writeClipboard: (text) => ipcRenderer.invoke("write-clipboard", text),
  checkPasteTools: () => ipcRenderer.invoke("check-paste-tools"),
  onWhisperDownloadProgress: registerListener("whisper-download-progress"),
  checkFFmpegAvailability: () => ipcRenderer.invoke("check-ffmpeg-availability"),
  onCudaDownloadProgress: registerListener(
    "cuda-download-progress",
    (callback) => (_event, data) => callback(data)
  ),
  onCudaFallbackNotification: registerListener(
    "cuda-fallback-notification",
    (callback) => () => callback()
  ),
  onVulkanWhisperDownloadProgress: registerListener(
    "vulkan-whisper-download-progress",
    (callback) => (_event, data) => callback(data)
  ),
  onGpuFallbackNotification: registerListener(
    "gpu-fallback-notification",
    (callback) => () => callback()
  ),
  onParakeetDownloadProgress: registerListener("parakeet-download-progress"),

  // Window control functions
  windowMinimize: () => ipcRenderer.invoke("window-minimize"),
  windowMaximize: () => ipcRenderer.invoke("window-maximize"),
  windowClose: () => ipcRenderer.invoke("window-close"),
  windowIsMaximized: () => ipcRenderer.invoke("window-is-maximized"),
  getPlatform: () => process.platform,

  relaunchApp: () => ipcRenderer.invoke("relaunch-app"),
  updateHotkey: (hotkey) => ipcRenderer.invoke("update-hotkey", hotkey),
  setHotkeyListeningMode: (enabled) => ipcRenderer.invoke("set-hotkey-listening-mode", enabled),
  getHotkeyModeInfo: (hotkey) => ipcRenderer.invoke("get-hotkey-mode-info", hotkey),
  getHyprlandConfigStatus: () => ipcRenderer.invoke("get-hyprland-config-status"),
  startWindowDrag: () => ipcRenderer.invoke("start-window-drag"),
  stopWindowDrag: () => ipcRenderer.invoke("stop-window-drag"),
  startControlPanelDrag: () => ipcRenderer.invoke("start-control-panel-drag"),
  stopControlPanelDrag: () => ipcRenderer.invoke("stop-control-panel-drag"),
  getMainWindowHorizontalDirection: () =>
    ipcRenderer.invoke("get-main-window-horizontal-direction"),
  onMainWindowHorizontalDirectionChanged: registerListener(
    "main-window-horizontal-direction-changed",
    (callback) => (_event, direction) => callback(direction)
  ),
  onMainWindowWillResize: registerListener(
    "main-window-will-resize",
    (callback) => (_event, resize) => callback(resize)
  ),
  ackMainWindowResizeMask: (token) => ipcRenderer.send("main-window-resize-mask-ready", token),
  setMainWindowInteractivity: (interactive) =>
    ipcRenderer.invoke("set-main-window-interactivity", interactive),
  setMainWindowInputRegion: (region) => ipcRenderer.invoke("set-main-window-input-region", region),
  onMainWindowVisibilityChanged: registerListener(
    "main-window-visibility-changed",
    (callback) => (_event, visible) => callback(visible)
  ),
  resizeMainWindow: (sizeKey) => ipcRenderer.invoke("resize-main-window", sizeKey),
  resizeAssistantWindowToContent: (surfaceHeight) =>
    ipcRenderer.invoke("resize-assistant-window-to-content", surfaceHeight),
  resizeDictationErrorWindowToContent: (surfaceHeight) =>
    ipcRenderer.invoke("resize-dictation-error-window-to-content", surfaceHeight),
  setAssistantPanelOpen: (open) => ipcRenderer.invoke("set-assistant-panel-open", open),
  setAssistantPanelBusy: (busy) => ipcRenderer.invoke("set-assistant-panel-busy", busy),
  getAppVersion: () => ipcRenderer.invoke("get-app-version"),
  getPostMigrationState: () => ipcRenderer.invoke("get-post-migration-state"),
  markBundleMigrated: () => ipcRenderer.invoke("mark-bundle-migrated"),
  markBundleMigrationDismissed: () => ipcRenderer.invoke("mark-bundle-migration-dismissed"),

  // Update event listeners
  onUpdateAvailable: registerListener("update-available"),
  onUpdateNotAvailable: registerListener("update-not-available"),
  onUpdateDownloaded: registerListener("update-downloaded"),
  onUpdateDownloadProgress: registerListener("update-download-progress"),
  onUpdateError: registerListener("update-error"),

  // Audio event listeners
  onCancelHotkeyPressed: registerListener("cancel-hotkey-pressed", (cb) => () => cb()),
  registerCancelHotkey: (key, owner = "recording") =>
    ipcRenderer.invoke("register-cancel-hotkey", key, owner),
  unregisterCancelHotkey: (owner = "recording") =>
    ipcRenderer.invoke("unregister-cancel-hotkey", owner),

  // External link opener
  openExternal: (url) => ipcRenderer.invoke("open-external", url),
  onModelDownloadProgress: registerListener("model-download-progress"),

  getUiLanguage: () => ipcRenderer.invoke("get-ui-language"),
  saveUiLanguage: (language) => ipcRenderer.invoke("save-ui-language", language),
  setUiLanguage: (language) => ipcRenderer.invoke("set-ui-language", language),

  // Custom endpoint API keys
  getCustomTranscriptionKey: () => ipcRenderer.invoke("get-custom-transcription-key"),
  saveCustomTranscriptionKey: (key) => ipcRenderer.invoke("save-custom-transcription-key", key),
  getCleanupCustomKey: () => ipcRenderer.invoke("get-cleanup-custom-key"),
  saveCleanupCustomKey: (key) => ipcRenderer.invoke("save-cleanup-custom-key", key),

  // Dictation key persistence (file-based for reliable startup)
  getDictationKey: () => ipcRenderer.invoke("get-dictation-key"),
  getActiveDictationKey: () => ipcRenderer.invoke("get-active-dictation-key"),
  getEffectiveDefaultHotkey: () => ipcRenderer.invoke("get-effective-default-hotkey"),
  saveDictationKey: (key) => ipcRenderer.invoke("save-dictation-key", key),

  // Activation mode persistence (file-based for reliable startup)
  getActivationMode: () => ipcRenderer.invoke("get-activation-mode"),
  saveActivationMode: (mode) => ipcRenderer.invoke("save-activation-mode", mode),
  syncStartupPreferences: (prefs) => ipcRenderer.invoke("sync-startup-preferences", prefs),
  cancelEnterpriseReasoning: () => ipcRenderer.send("enterprise-reasoning-cancel"),
  onEnterpriseStreamPart: registerListener(
    "enterprise-stream-part",
    (callback) => (_event, payload) => callback(payload)
  ),
  getManagedEnterpriseConfig: (accountId, workspaceId, expectedAuthGeneration, forceRefresh) =>
    ipcRenderer.invoke(
      "get-managed-enterprise-config",
      accountId,
      workspaceId,
      expectedAuthGeneration,
      forceRefresh
    ),
  onManagedEnterpriseConfigChanged: registerListener(
    "managed-enterprise-config-changed",
    (callback) => (_event, snapshot) => callback(snapshot)
  ),
  onLlamaVulkanDownloadProgress: registerListener(
    "llama-vulkan-download-progress",
    (callback) => (_event, data) => callback(data)
  ),

  getLogLevel: () => ipcRenderer.invoke("get-log-level"),
  log: (entry) => ipcRenderer.invoke("app-log", entry),

  // ydotool status check
  getYdotoolStatus: () => ipcRenderer.invoke("get-ydotool-status"),

  // Debug logging management
  getDebugState: () => ipcRenderer.invoke("get-debug-state"),
  setDebugLogging: (enabled) => ipcRenderer.invoke("set-debug-logging", enabled),
  openLogsFolder: () => ipcRenderer.invoke("open-logs-folder"),

  // System settings helpers for microphone/audio permissions
  requestMicrophoneAccess: () => ipcRenderer.invoke("request-microphone-access"),
  checkMicrophoneAccess: () => ipcRenderer.invoke("check-microphone-access"),
  getSystemDefaultMicrophone: (options) =>
    ipcRenderer.invoke("get-system-default-microphone", options),
  getLaptopLidState: () => ipcRenderer.invoke("get-laptop-lid-state"),
  onLaptopLidStateChanged: registerListener(
    "laptop-lid-state-changed",
    (callback) => (_event, lidClosed) => callback(lidClosed)
  ),
  openMicrophoneSettings: () => ipcRenderer.invoke("open-microphone-settings"),
  openSoundInputSettings: () => ipcRenderer.invoke("open-sound-input-settings"),
  openAccessibilitySettings: () => ipcRenderer.invoke("open-accessibility-settings"),
  openLoginItemsSettings: () => ipcRenderer.invoke("open-login-items-settings"),
  toggleMediaPlayback: () => ipcRenderer.invoke("toggle-media-playback"),
  pauseMediaPlayback: () => ipcRenderer.invoke("pause-media-playback"),
  resumeMediaPlayback: () => ipcRenderer.invoke("resume-media-playback"),
  onAuthTokenStateChanged: registerListener(
    "auth-token-state-changed",
    (callback) => (_event, state) => callback(state)
  ),
  cancelCloudTranscription: () => ipcRenderer.send("cloud-transcribe-cancel"),
  cancelCloudReason: () => ipcRenderer.send("cloud-reason-cancel"),
  onWorkspacePolicyChanged: (callback) => {
    const listener = (_event, snapshot) => callback(snapshot);
    ipcRenderer.on("workspace-policy-changed", listener);
    return () => ipcRenderer.removeListener("workspace-policy-changed", listener);
  },
  onUploadTranscriptionProgress: registerListener(
    "upload-transcription-progress",
    (callback) => (_event, data) => callback(data)
  ),
  assemblyAiStreamingSend: (audioBuffer) =>
    ipcRenderer.send("assemblyai-streaming-send", audioBuffer),
  assemblyAiStreamingForceEndpoint: () => ipcRenderer.send("assemblyai-streaming-force-endpoint"),
  onAssemblyAiPartialTranscript: registerListener(
    "assemblyai-partial-transcript",
    (callback) => (_event, text) => callback(text)
  ),
  onAssemblyAiFinalTranscript: registerListener(
    "assemblyai-final-transcript",
    (callback) => (_event, text) => callback(text)
  ),
  onAssemblyAiError: registerListener(
    "assemblyai-error",
    (callback) => (_event, error) => callback(error)
  ),
  onAssemblyAiSessionEnd: registerListener(
    "assemblyai-session-end",
    (callback) => (_event, data) => callback(data)
  ),
  deepgramStreamingSend: (audioBuffer) => ipcRenderer.send("deepgram-streaming-send", audioBuffer),
  deepgramStreamingFinalize: () => ipcRenderer.send("deepgram-streaming-finalize"),
  onDeepgramPartialTranscript: registerListener(
    "deepgram-partial-transcript",
    (callback) => (_event, text) => callback(text)
  ),
  onDeepgramFinalTranscript: registerListener(
    "deepgram-final-transcript",
    (callback) => (_event, text) => callback(text)
  ),
  onDeepgramError: registerListener(
    "deepgram-error",
    (callback) => (_event, error) => callback(error)
  ),
  onDeepgramSessionEnd: registerListener(
    "deepgram-session-end",
    (callback) => (_event, data) => callback(data)
  ),
  geminiStreamingSend: (audioBuffer) => ipcRenderer.send("gemini-streaming-send", audioBuffer),
  geminiStreamingFinalize: () => ipcRenderer.send("gemini-streaming-finalize"),
  onGeminiPartialTranscript: registerListener(
    "gemini-partial-transcript",
    (callback) => (_event, text) => callback(text)
  ),
  onGeminiFinalTranscript: registerListener(
    "gemini-final-transcript",
    (callback) => (_event, text) => callback(text)
  ),
  onGeminiError: registerListener("gemini-error", (callback) => (_event, error) => callback(error)),
  onGeminiSessionEnd: registerListener(
    "gemini-session-end",
    (callback) => (_event, data) => callback(data)
  ),
  cortiStreamingSend: (audioBuffer) => ipcRenderer.send("corti-streaming-send", audioBuffer),
  cortiStreamingFinalize: () => ipcRenderer.send("corti-streaming-finalize"),
  onCortiPartialTranscript: registerListener(
    "corti-partial-transcript",
    (callback) => (_event, text) => callback(text)
  ),
  onCortiFinalTranscript: registerListener(
    "corti-final-transcript",
    (callback) => (_event, text) => callback(text)
  ),
  onCortiError: registerListener("corti-error", (callback) => (_event, error) => callback(error)),
  onCortiSessionEnd: registerListener(
    "corti-session-end",
    (callback) => (_event, data) => callback(data)
  ),
  meetingTranscriptionSend: (buffer, source) =>
    ipcRenderer.send("meeting-transcription-send", buffer, source),
  onMeetingTranscriptionSegment: registerListener(
    "meeting-transcription-segment",
    (callback) => (_event, data) => callback(data)
  ),
  onMeetingSpeakerIdentified: registerListener(
    "meeting-speaker-identified",
    (callback) => (_event, data) => callback(data)
  ),
  onMeetingSpeakersMerged: registerListener(
    "meeting-speakers-merged",
    (callback) => (_event, data) => callback(data)
  ),
  onMeetingSessionSpeakerConfigUpdated: registerListener(
    "meeting-session-speaker-config-updated",
    (callback) => (_event, data) => callback(data)
  ),
  onMeetingTranscriptionError: registerListener(
    "meeting-transcription-error",
    (callback) => (_event, data) => callback(data)
  ),
  onMeetingTranscriptionFatalError: registerListener(
    "meeting-transcription-fatal-error",
    (callback) => (_event, data) => callback(data)
  ),
  onMeetingSystemAudioSilent: registerListener(
    "meeting-system-audio-silent",
    (callback) => (_event, data) => callback(data)
  ),
  onMeetingSystemAudioDegraded: registerListener(
    "meeting-system-audio-degraded",
    (callback) => () => callback()
  ),
  onMeetingSystemAudioInterrupted: registerListener(
    "meeting-system-audio-interrupted",
    (callback) => (_event, data) => callback(data)
  ),
  onMeetingSystemAudioResumed: registerListener(
    "meeting-system-audio-resumed",
    (callback) => () => callback()
  ),
  dictationRealtimeSend: (buffer) => ipcRenderer.send("dictation-realtime-send", buffer),
  onDictationRealtimePartial: registerListener(
    "dictation-realtime-partial",
    (callback) => (_event, data) => callback(data)
  ),
  onDictationRealtimeFinal: registerListener(
    "dictation-realtime-final",
    (callback) => (_event, data) => callback(data)
  ),
  onDictationRealtimeError: registerListener(
    "dictation-realtime-error",
    (callback) => (_event, data) => callback(data)
  ),
  onDictationRealtimeSessionEnd: registerListener(
    "dictation-realtime-session-end",
    (callback) => (_event, data) => callback(data)
  ),

  // Usage limit events (for showing UpgradePrompt in ControlPanel)
  notifyLimitReached: (data) => ipcRenderer.send("limit-reached", data),
  onLimitReached: registerListener("limit-reached", (callback) => (_event, data) => callback(data)),

  // Workspace invitation deep link
  onWorkspaceInvitationToken: registerListener(
    "workspace-invitation-token",
    (callback) => (_event, token) => callback(token)
  ),
  getPendingInvitationToken: () => ipcRenderer.invoke("get-pending-invitation-token"),

  // Globe key listener for hotkey capture (macOS only)
  onGlobeKeyPressed: (callback) => {
    const listener = () => callback?.();
    ipcRenderer.on("globe-key-pressed", listener);
    return () => ipcRenderer.removeListener("globe-key-pressed", listener);
  },
  onGlobeKeyReleased: (callback) => {
    const listener = () => callback?.();
    ipcRenderer.on("globe-key-released", listener);
    return () => ipcRenderer.removeListener("globe-key-released", listener);
  },

  // Hotkey registration events (for notifying user when hotkey fails)
  onHotkeyFallbackUsed: (callback) => {
    const listener = (_event, data) => callback?.(data);
    ipcRenderer.on("hotkey-fallback-used", listener);
    return () => ipcRenderer.removeListener("hotkey-fallback-used", listener);
  },
  onHotkeyRegistrationFailed: (callback) => {
    const listener = (_event, data) => callback?.(data);
    ipcRenderer.on("hotkey-registration-failed", listener);
    return () => ipcRenderer.removeListener("hotkey-registration-failed", listener);
  },
  onSettingUpdated: (callback) => {
    const listener = (_event, data) => callback?.(data);
    ipcRenderer.on("setting-updated", listener);
    return () => ipcRenderer.removeListener("setting-updated", listener);
  },
  onDictationKeyActive: (callback) => {
    const listener = (_event, key) => callback?.(key);
    ipcRenderer.on("dictation-key-active", listener);
    return () => ipcRenderer.removeListener("dictation-key-active", listener);
  },
  onWindowsPushToTalkUnavailable: registerListener("windows-ptt-unavailable"),
  onLinuxPttPermissionDenied: registerListener(
    "linux-ptt-permission-denied",
    (callback) => () => callback()
  ),

  // Settings shortcut (Cmd+, / Ctrl+,)
  onShowSettings: registerListener("show-settings", (callback) => () => callback()),

  // Accessibility permission events (macOS)
  onAccessibilityMissing: (callback) => {
    const listener = () => callback?.();
    ipcRenderer.on("accessibility-missing", listener);
    return () => ipcRenderer.removeListener("accessibility-missing", listener);
  },
  checkAccessibilityTrusted: () => ipcRenderer.invoke("check-accessibility-trusted"),

  // Notify main process of activation mode changes (for Windows Push-to-Talk)
  notifyActivationModeChanged: (mode) => ipcRenderer.send("activation-mode-changed", mode),
  notifyHotkeyChanged: (hotkey) => ipcRenderer.send("hotkey-changed", hotkey),

  // Floating icon auto-hide
  notifyFloatingIconAutoHideChanged: (enabled) =>
    ipcRenderer.send("floating-icon-auto-hide-changed", enabled),
  onFloatingIconAutoHideChanged: registerListener(
    "floating-icon-auto-hide-changed",
    (callback) => (_event, enabled) => callback(enabled)
  ),

  // Panel start position
  notifyPanelStartPositionChanged: (position) =>
    ipcRenderer.send("panel-start-position-changed", position),

  // Start minimized
  notifyStartMinimizedChanged: (enabled) => ipcRenderer.send("start-minimized-changed", enabled),

  // macOS menu bar icon
  getMenuBarIconVisible: () => ipcRenderer.invoke("get-menu-bar-icon-visible"),
  setMenuBarIconVisible: (visible) => ipcRenderer.invoke("set-menu-bar-icon-visible", visible),

  // Auto-start management
  getAutoStartEnabled: () => ipcRenderer.invoke("get-auto-start-enabled"),
  setAutoStartEnabled: (enabled) => ipcRenderer.invoke("set-auto-start-enabled", enabled),
  onPreviewText: registerListener("preview-text", (callback) => (_event, text) => callback(text)),
  onPreviewAppend: registerListener(
    "preview-append",
    (callback) => (_event, text) => callback(text)
  ),
  onPreviewHold: registerListener(
    "preview-hold",
    (callback) => (_event, payload) => callback(payload)
  ),
  onPreviewResult: registerListener(
    "preview-result",
    (callback) => (_event, payload) => callback(payload)
  ),
  onPreviewHide: registerListener("preview-hide", (callback) => () => callback()),
  sendDictationPreviewAudio: (data) => ipcRenderer.send("dictation-preview-audio", data),
  acquireRecordingLock: (pipeline) => ipcRenderer.invoke("acquire-recording-lock", pipeline),
  releaseRecordingLock: (pipeline) => ipcRenderer.invoke("release-recording-lock", pipeline),

  // Agent cloud streaming (event-based for real-time chunks)
  startAgentStream: (requestId, messages, opts) =>
    ipcRenderer.send("cloud-agent-stream-start", requestId, messages, opts),
  cancelAgentStream: (requestId) => ipcRenderer.send("cloud-agent-stream-cancel", requestId),
  onAgentStreamChunk: registerListener(
    "cloud-agent-stream-chunk",
    (callback) => (_event, payload) => callback(payload)
  ),
  onAgentStreamError: registerListener(
    "cloud-agent-stream-error",
    (callback) => (_event, payload) => callback(payload)
  ),
  onAgentStreamEnd: registerListener(
    "cloud-agent-stream-end",
    (callback) => (_event, payload) => callback(payload)
  ),
  acknowledgeNoteCreate: (id, snapshot, cloudId, cloudUpdatedAt, ownerUserId, settleIfUnchanged) =>
    ipcRenderer.invoke(
      "db-acknowledge-note-create",
      id,
      snapshot,
      cloudId,
      cloudUpdatedAt,
      ownerUserId,
      settleIfUnchanged
    ),
  markNoteSyncedIfUnchanged: (id, snapshot, expectedCloudId, cloudUpdatedAt, ownerUserId) =>
    ipcRenderer.invoke(
      "db-mark-note-synced-if-unchanged",
      id,
      snapshot,
      expectedCloudId,
      cloudUpdatedAt,
      ownerUserId
    ),
  acknowledgeFolderCreate: (
    id,
    snapshot,
    expectedCloudId,
    responseClientFolderId,
    cloudId,
    cloudUpdatedAt
  ) =>
    ipcRenderer.invoke(
      "db-acknowledge-folder-create",
      id,
      snapshot,
      expectedCloudId,
      responseClientFolderId,
      cloudId,
      cloudUpdatedAt
    ),
  markSnippetSynced: (id, cloudId, serverUpdatedAt, expectedTrigger, expectedReplacement) =>
    ipcRenderer.invoke(
      "db-mark-snippet-synced",
      id,
      cloudId,
      serverUpdatedAt,
      expectedTrigger,
      expectedReplacement
    ),

  // Google Calendar event listeners
  onGcalConnectionChanged: registerListener(
    "gcal-connection-changed",
    (callback) => (_event, data) => callback(data)
  ),
  onGcalEventsSynced: registerListener(
    "gcal-events-synced",
    (callback) => (_event, data) => callback(data)
  ),

  // Microsoft Calendar event listeners
  onMcalConnectionChanged: registerListener(
    "mcal-connection-changed",
    (callback) => (_event, data) => callback(data)
  ),
  onMcalEventsSynced: registerListener(
    "mcal-events-synced",
    (callback) => (_event, data) => callback(data)
  ),

  // Apple Calendar event listeners
  onAcalConnectionChanged: registerListener(
    "acal-connection-changed",
    (callback) => (_event, data) => callback(data)
  ),
  onAcalEventsSynced: registerListener(
    "acal-events-synced",
    (callback) => (_event, data) => callback(data)
  ),
  getWhisperVadConfig: () => ipcRenderer.invoke("whisper-vad-get-config"),
  setWhisperVadConfig: (config) => ipcRenderer.invoke("whisper-vad-set-config", config),
  onMeetingNotificationData: registerListener(
    "meeting-notification-data",
    (callback) => (_event, data) => callback(data)
  ),
  onMeetingAutoEndRequested: registerListener(
    "meeting-auto-end-requested",
    (callback) => (_event, data) => callback(data)
  ),
  onMeetingNoteNavigationPending: registerListener(
    "meeting-note-navigation-pending",
    (callback) => () => callback()
  ),
  onNoteNavigationPending: registerListener(
    "note-navigation-pending",
    (callback) => () => callback()
  ),
});
