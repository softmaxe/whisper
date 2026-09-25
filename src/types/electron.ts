export interface RecordingRequestOptions {
  startupRequest?: { requestId: string; acceptedAt: number };
}

export type ChineseScriptPreference = "simplified" | "traditional" | "as-transcribed";

export type TranscriptionStatus = "completed" | "failed" | "pending" | "discarded";

export type TranscriptionErrorCode =
  | "TIMEOUT"
  | "NETWORK"
  | "SERVER_ERROR"
  | "OFFLINE"
  | "AUTH_EXPIRED"
  | "AUTH_REQUIRED"
  | "LIMIT_REACHED"
  | "PROVIDER_RATE_LIMITED"
  | "API_KEY_MISSING"
  | "INVALID_KEY"
  | "MODEL_NOT_AVAILABLE"
  | "CUSTOM_ENDPOINT_INVALID"
  | null;

export interface TranscriptionItem {
  id: number;
  text: string;
  raw_text: string | null;
  timestamp: string;
  created_at: string;
  has_audio: number;
  audio_duration_ms: number | null;
  provider: string | null;
  model: string | null;
  status: TranscriptionStatus;
  error_message: string | null;
  error_code: TranscriptionErrorCode;
  route_kind?: string | null;
  client_transcription_id: string;
  cloud_id: string | null;
  sync_status: "synced" | "pending" | "error";
  deleted_at: string | null;
}

export type AnalyticsMode = "local" | "openwhispr_cloud" | "byok" | "self_hosted" | "unknown";

export interface AnalyticsEventInput {
  eventId: string;
  wordCount: number;
  occurredAt: string;
  localDate: string;
  spokenDurationMs?: number | null;
  mode: AnalyticsMode;
  provider?: string | null;
  model?: string | null;
}

export interface AnalyticsDailyBucket {
  date: string;
  words: number;
  dictations: number;
  spokenDurationMs: number;
}

export interface AnalyticsSummary {
  totalWords: number;
  totalDictations: number;
  totalSpokenDurationMs: number;
  averageWpm: number | null;
  currentStreakDays: number;
  longestStreakDays: number;
  wpmCoveragePercent: number;
  daily: AnalyticsDailyBucket[];
  historyBackfillRetryRequired?: boolean;
}

export interface PasteToolsResult {
  platform: "darwin";
  available: boolean;
  method: string | null;
  requiresPermission: boolean;
  tools?: string[];
}

declare global {
  interface Window {
    electronAPI: {
      // Basic window operations
      setOnboardingWindowMode?: (mode: "compact" | "expanded" | "restore") => Promise<boolean>;
      setOnboardingActive?: (active: boolean) => Promise<boolean>;
      /** Keeps the hidden control panel renderer alive while it owns unfinished work. */
      setControlPanelRetained?: (retained: boolean) => void;
      pasteText: (
        text: string,
        options?: {
          restoreClipboard?: boolean;
          allowClipboardFallback?: boolean;
        }
      ) => Promise<{ success: true; pasted: boolean }>;
      hideWindow: () => Promise<void>;
      showDictationPanel: () => Promise<void>;
      captureDictationTarget?: () => Promise<{ success: boolean; pid: number | null }>;
      onToggleDictation: (callback: (options?: RecordingRequestOptions) => void) => () => void;
      onStartDictation?: (callback: (options?: RecordingRequestOptions) => void) => () => void;
      onStopDictation?: (callback: () => void) => () => void;
      onPrepareDictation?: (
        callback: (
          options?: RecordingRequestOptions & {
            inputKind?: "dictation" | "assistant" | "translation";
          }
        ) => void
      ) => () => void;
      onCancelDictationPreparation?: (callback: () => void) => () => void;
      onCancelHotkeyPressed?: (callback: () => void) => () => void;
      registerCancelHotkey?: (
        key: string,
        owner?: "recording" | "copy-recovery"
      ) => Promise<{ success: boolean; error?: string }>;
      unregisterCancelHotkey?: (
        owner?: "recording" | "copy-recovery"
      ) => Promise<{ success: boolean; error?: string }>;
      onDictationForceStopped?: (
        callback: (payload?: { reason?: "timeout" | "reset" | "manual" }) => void
      ) => () => void;
      dictationLifecycleStateChanged: (
        state: "idle" | "preparing" | "recording" | "processing"
      ) => void;

      // Database operations
      saveTranscription: (
        text: string,
        rawText?: string | null,
        options?: {
          status?: TranscriptionStatus;
          errorMessage?: string | null;
          errorCode?: TranscriptionErrorCode;
          routeKind?: string | null;
          clientTranscriptionId?: string;
          analyticsOccurredAt?: string;
        }
      ) => Promise<{ id: number; success: boolean; transcription?: TranscriptionItem }>;
      getTranscriptions: (
        limit?: number,
        options?: { includeDiscarded?: boolean }
      ) => Promise<TranscriptionItem[]>;
      recordAnalyticsEvent: (
        input: AnalyticsEventInput
      ) => Promise<{ success: boolean; eventId?: string; ignored?: boolean }>;
      getAnalyticsSummary: () => Promise<AnalyticsSummary>;
      clearTranscriptions: () => Promise<{ cleared: number; success: boolean }>;
      deleteTranscription: (id: number) => Promise<{ success: boolean }>;
      getTranscriptionById: (id: number) => Promise<TranscriptionItem | null>;

      // Audio retention operations
      saveTranscriptionAudio: (
        id: number,
        audioBuffer: ArrayBuffer,
        metadata?: { durationMs?: number; provider?: string; model?: string }
      ) => Promise<{ success: boolean; path?: string }>;
      getAudioPath: (id: number) => Promise<string | null>;
      showAudioInFolder: (id: number) => Promise<{ success: boolean }>;
      getAudioBuffer: (id: number) => Promise<ArrayBuffer | null>;
      getAudioStorageUsage: () => Promise<{ fileCount: number; totalBytes: number }>;
      deleteAllAudio: () => Promise<{ deleted: number }>;
      syncRetentionSettings?: (settings: {
        audioRetentionDays: number;
        transcriptRetentionDays: number;
        dataRetentionEnabled: boolean;
      }) => void;
      retryTranscription: (
        id: number,
        settings?: {
          preferredLanguage?: string;
          remoteTranscriptionUrl?: string;
          remoteTranscriptionModel?: string;
        }
      ) => Promise<{
        success: boolean;
        transcription?: TranscriptionItem;
        error?: string;
        code?: TranscriptionErrorCode;
        messageKey?: string;
      }>;
      updateTranscriptionText: (
        id: number,
        text: string,
        rawText: string
      ) => Promise<{ success: boolean; transcription?: TranscriptionItem; error?: string }>;

      // Dictionary operations
      getDictionary: () => Promise<string[]>;
      /** Replaces the whole dictionary — omitted words are deleted. Prefer applyDictionaryChanges. */
      setDictionary: (words: string[]) => Promise<{ success: boolean }>;
      applyDictionaryChanges?: (changes: {
        add?: string[];
        remove?: string[];
      }) => Promise<{ success: boolean; added: number; removed: number }>;
      onDictionaryUpdated?: (callback: (words: string[]) => void) => () => void;
      getSnippets?: () => Promise<Array<{ trigger: string; replacement: string }>>;
      setSnippets?: (
        snippets: Array<{ trigger: string; replacement: string }>
      ) => Promise<{ success: boolean }>;
      setAutoLearnEnabled?: (enabled: boolean) => void;
      onCorrectionsLearned?: (callback: (words: string[]) => void) => () => void;
      undoLearnedCorrections?: (words: string[]) => Promise<{ success: boolean }>;

      exportDictionary: (words: string[]) => Promise<{ success: boolean; error?: string }>;

      selectAudioFile: (options?: { multiple?: boolean }) => Promise<{
        canceled: boolean;
        filePath?: string;
        filePaths?: string[];
      }>;
      getFileSize?: (filePath: string) => Promise<number>;
      getPathForFile: (file: File) => string;

      // Database event listeners
      onTranscriptionAdded?: (callback: (item: TranscriptionItem) => void) => () => void;
      onTranscriptionUpdated?: (callback: (item: TranscriptionItem) => void) => () => void;
      onTranscriptionDeleted?: (callback: (payload: { id: number }) => void) => () => void;
      onTranscriptionsCleared?: (callback: (payload: { cleared: number }) => void) => () => void;
      onAnalyticsChanged?: (callback: () => void) => () => void;

      getUiLanguage: () => Promise<string>;
      saveUiLanguage: (language: string) => Promise<{ success: boolean; language: string }>;
      setUiLanguage: (language: string) => Promise<{ success: boolean; language: string }>;

      // Clipboard operations
      checkAccessibilityPermission: (silent?: boolean) => Promise<boolean>;
      readClipboard: () => Promise<string>;
      writeClipboard: (text: string) => Promise<{ success: boolean }>;
      checkPasteTools: () => Promise<PasteToolsResult>;

      // Audio

      startWindowDrag: () => Promise<void>;
      stopWindowDrag: () => Promise<void>;
      startControlPanelDrag: () => Promise<void>;
      stopControlPanelDrag: () => Promise<void>;
      setMainWindowInteractivity: (interactive: boolean) => Promise<void>;
      resizeMainWindow: (
        sizeKey:
          | "BASE"
          | "RECORDING"
          | "DICTATION_ERROR"
          | "DICTATION_ERROR_WITH_TRANSCRIPT"
          | "WITH_MENU"
          | "WITH_TOAST"
          | "EXPANDED"
          | "ASSISTANT"
      ) => Promise<{
        success: boolean;
        bounds?: Electron.Rectangle;
        message?: string;
        changed?: boolean;
      }>;
      resizeAssistantWindowToContent: (surfaceHeight: number) => Promise<{
        success: boolean;
        bounds?: Electron.Rectangle;
        message?: string;
        changed?: boolean;
      }>;
      resizeDictationErrorWindowToContent: (
        surfaceHeight: number
      ) => Promise<{ success: boolean; bounds?: Electron.Rectangle; message?: string }>;

      openExternal: (url: string) => Promise<{ success: boolean; error?: string }>;

      // Hotkey management
      updateHotkey: (key: string) => Promise<{ success: boolean; message: string }>;
      setHotkeyListeningMode?: (enabled: boolean) => Promise<{ success: boolean }>;
      getHotkeyModeInfo?: (hotkey?: string) => Promise<{
        supportsPushToTalk: boolean;
        pushToTalkUnavailableReason: string | null;
      }>;

      // Globe key listener for hotkey capture (macOS only)
      onGlobeKeyPressed?: (callback: () => void) => () => void;
      onGlobeKeyReleased?: (callback: () => void) => () => void;

      // Hotkey registration events
      onHotkeyRegistrationFailed?: (
        callback: (data: { hotkey: string; error: string; suggestions: string[] }) => void
      ) => () => void;
      onSettingUpdated?: (callback: (data: { key: string; value: unknown }) => void) => () => void;
      onDictationKeyActive?: (callback: (key: string) => void) => () => void;

      // Settings shortcut (Cmd+, / Ctrl+,)
      onShowSettings?: (callback: () => void) => () => void;

      // Accessibility permission events (macOS)
      markMacAccessibilityFeaturesReady?: () => void;
      onAccessibilityMissing?: (callback: () => void) => () => void;

      getCleanupCustomKey?: () => Promise<string | null>;
      saveCleanupCustomKey?: (key: string) => Promise<void>;

      // Dictation key persistence (file-based for reliable startup)
      getDictationKey?: () => Promise<string | null>;
      getActiveDictationKey?: () => Promise<string>;
      getEffectiveDefaultHotkey?: () => Promise<string>;
      saveDictationKey?: (key: string) => Promise<void>;

      // Activation mode persistence (file-based for reliable startup)
      getActivationMode?: () => Promise<"tap" | "push">;
      saveActivationMode?: (mode: "tap" | "push") => Promise<void>;

      // Debug logging
      getLogLevel?: () => Promise<string>;
      log?: (entry: {
        level: string;
        message: string;
        meta?: any;
        scope?: string;
        source?: string;
      }) => Promise<void>;

      // System settings helpers
      requestMicrophoneAccess?: () => Promise<{ granted: boolean }>;
      checkMicrophoneAccess?: () => Promise<{ granted: boolean; status: string }>;
      getSystemDefaultMicrophone?: (options?: { refresh?: boolean }) => Promise<{
        name: string;
        nativeId?: string;
        source: "system" | "unavailable";
      }>;
      getLaptopLidState?: () => Promise<boolean | null>;
      onLaptopLidStateChanged?: (callback: (lidClosed: boolean | null) => void) => () => void;
      openMicrophoneSettings?: () => Promise<{ success: boolean; error?: string }>;
      openSoundInputSettings?: () => Promise<{ success: boolean; error?: string }>;
      openAccessibilitySettings?: () => Promise<{ success: boolean; error?: string }>;
      openLoginItemsSettings?: () => Promise<{ success: boolean; error?: string }>;
      pauseMediaPlayback?: () => Promise<boolean>;
      resumeMediaPlayback?: () => Promise<boolean>;

      // Windows Push-to-Talk notifications
      notifyActivationModeChanged?: (mode: "tap" | "push") => void;
      notifyHotkeyChanged?: (hotkey: string) => void;
      notifyFloatingIconAutoHideChanged?: (enabled: boolean) => void;
      onFloatingIconAutoHideChanged?: (callback: (enabled: boolean) => void) => () => void;
      notifyStartMinimizedChanged?: (enabled: boolean) => void;
      getMenuBarIconVisible?: () => Promise<boolean>;
      setMenuBarIconVisible?: (visible: boolean) => Promise<boolean>;
      notifyPanelStartPositionChanged?: (position: string) => void;
      getMainWindowHorizontalDirection?: () => Promise<"left" | "right">;
      onMainWindowHorizontalDirectionChanged?: (
        callback: (direction: "left" | "right") => void
      ) => () => void;
      onMainWindowWillResize?: (
        callback: (resize: {
          bounds: Electron.Rectangle;
          anchor: "bottom-left" | "bottom-right" | "center";
        }) => void
      ) => () => void;

      // Auto-start at login. requiresApproval is macOS-only: SMAppService can
      // register the login item and still leave it awaiting approval in System
      // Settings, which otherwise looks like a toggle that will not stick.
      getAutoStartEnabled?: () => Promise<{ enabled: boolean; requiresApproval: boolean }>;
      setAutoStartEnabled?: (enabled: boolean) => Promise<{ success: boolean; error?: string }>;

      cancelUploadTranscription?: (requestId: string) => Promise<{ success: boolean }>;

      // BYOK audio file transcription
      transcribeAudioFile?: (options: {
        filePath: string;
        language?: string;
        remoteTranscriptionUrl?: string;
        remoteTranscriptionModel?: string;
      }) => Promise<{
        success: boolean;
        text?: string;
        error?: string;
        code?: string;
        messageKey?: string;
      }>;

      onPreviewText?: (callback: (text: string) => void) => () => void;
      onPreviewAppend?: (callback: (text: string) => void) => () => void;
      onPreviewHold?: (callback: (payload: { showCleanup: boolean }) => void) => () => void;
      onPreviewResult?: (callback: (payload: { text: string }) => void) => () => void;
      onPreviewHide?: (callback: () => void) => () => void;
    };

    api?: {};
  }
}
