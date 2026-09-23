import React, { createContext, useCallback, useContext, useEffect, useRef } from "react";
import { useSettingsStore, initializeSettings } from "../stores/settingsStore";
import logger from "../utils/logger";
import { useLocalStorage } from "./useLocalStorage";

function useSettingsInternal() {
  const store = useSettingsStore();
  const { applyCustomDictionaryFromExternal } = store;

  // One-time initialization: sync API keys, dictation key, activation mode,
  // UI language, and dictionary from the main process / SQLite.
  const hasInitialized = useRef(false);
  useEffect(() => {
    if (hasInitialized.current) return;
    hasInitialized.current = true;
    initializeSettings().catch((err) => {
      logger.warn(
        "Failed to initialize settings store",
        { error: (err as Error).message },
        "settings"
      );
    });
  }, []);

  // Refresh the in-memory store from main-process broadcasts (auto-learn, sync
  // pulls) without re-triggering a sync — that would loop, since pulls emit the
  // broadcast. Writes that must sync go through setCustomDictionary instead.
  useEffect(() => {
    if (typeof window === "undefined" || !window.electronAPI?.onDictionaryUpdated) return;
    const unsubscribe = window.electronAPI.onDictionaryUpdated((words: string[]) => {
      if (Array.isArray(words)) {
        applyCustomDictionaryFromExternal(words);
      }
    });
    return unsubscribe;
  }, [applyCustomDictionaryFromExternal]);

  // Auto-learn corrections from user edits in external apps
  const [autoLearnCorrections, setAutoLearnCorrectionsRaw] = useLocalStorage(
    "autoLearnCorrections",
    true,
    {
      serialize: String,
      deserialize: (value: string) => value !== "false",
    }
  );

  const setAutoLearnCorrections = useCallback(
    (enabled: boolean) => {
      setAutoLearnCorrectionsRaw(enabled);
      window.electronAPI?.setAutoLearnEnabled?.(enabled);
    },
    [setAutoLearnCorrectionsRaw]
  );

  // Sync auto-learn state to main process on mount
  useEffect(() => {
    window.electronAPI?.setAutoLearnEnabled?.(autoLearnCorrections);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Retention periods are enforced by the main process cleanup sweep. The
  // history switch rides along because the main process reconstructs Insights
  // history from stored transcripts, and that must answer to the same switch.
  const { audioRetentionDays, transcriptRetentionDays, dataRetentionEnabled } = store;
  useEffect(() => {
    window.electronAPI?.syncRetentionSettings?.({
      audioRetentionDays,
      transcriptRetentionDays,
      dataRetentionEnabled,
    });
  }, [audioRetentionDays, transcriptRetentionDays, dataRetentionEnabled]);

  return {
    uiLanguage: store.uiLanguage,
    setUiLanguage: store.setUiLanguage,
    preferredLanguage: store.preferredLanguage,
    chineseScriptPreference: store.chineseScriptPreference,
    remoteTranscriptionUrl: store.remoteTranscriptionUrl,
    setRemoteTranscriptionUrl: store.setRemoteTranscriptionUrl,
    remoteTranscriptionModel: store.remoteTranscriptionModel,
    setRemoteTranscriptionModel: store.setRemoteTranscriptionModel,
    customDictionary: store.customDictionary,
    updateCustomDictionary: store.updateCustomDictionary,
    snippets: store.snippets,
    setSnippets: store.setSnippets,
    useCleanupModel: store.useCleanupModel,
    dictationKey: store.dictationKey,
    setDictationKey: store.setDictationKey,
    activationMode: store.activationMode,
    setActivationMode: store.setActivationMode,
    theme: store.theme,
    setTheme: store.setTheme,
    audioCuesEnabled: store.audioCuesEnabled,
    setAudioCuesEnabled: store.setAudioCuesEnabled,
    pauseMediaOnDictation: store.pauseMediaOnDictation,
    setPauseMediaOnDictation: store.setPauseMediaOnDictation,
    floatingIconAutoHide: store.floatingIconAutoHide,
    setFloatingIconAutoHide: store.setFloatingIconAutoHide,
    startMinimized: store.startMinimized,
    setStartMinimized: store.setStartMinimized,
    showMenuBarIcon: store.showMenuBarIcon,
    setShowMenuBarIcon: store.setShowMenuBarIcon,
    panelStartPosition: store.panelStartPosition,
    setPanelStartPosition: store.setPanelStartPosition,
    microphoneSelectionMode: store.microphoneSelectionMode,
    selectedMicDeviceId: store.selectedMicDeviceId,
    selectedMicDeviceLabel: store.selectedMicDeviceLabel,
    setMicrophoneSelectionMode: store.setMicrophoneSelectionMode,
    setSelectedMicDevice: store.setSelectedMicDevice,
    autoPasteEnabled: store.autoPasteEnabled,
    setAutoPasteEnabled: store.setAutoPasteEnabled,
    keepTranscriptionInClipboard: store.keepTranscriptionInClipboard,
    setKeepTranscriptionInClipboard: store.setKeepTranscriptionInClipboard,
    audioRetentionDays: store.audioRetentionDays,
    setAudioRetentionDays: store.setAudioRetentionDays,
    transcriptRetentionDays: store.transcriptRetentionDays,
    setTranscriptRetentionDays: store.setTranscriptRetentionDays,
    dataRetentionEnabled: store.dataRetentionEnabled,
    setDataRetentionEnabled: store.setDataRetentionEnabled,
    saveDiscardedTranscriptions: store.saveDiscardedTranscriptions,
    setSaveDiscardedTranscriptions: store.setSaveDiscardedTranscriptions,
    updateTranscriptionSettings: store.updateTranscriptionSettings,
    updateCleanupSettings: store.updateCleanupSettings,
    autoLearnCorrections,
    setAutoLearnCorrections,
  };
}

export type SettingsValue = ReturnType<typeof useSettingsInternal>;

const SettingsContext = createContext<SettingsValue | null>(null);

export function SettingsProvider({ children }: { children: React.ReactNode }) {
  const value = useSettingsInternal();
  return React.createElement(SettingsContext.Provider, { value }, children);
}

export function useSettings(): SettingsValue {
  const ctx = useContext(SettingsContext);
  if (!ctx) {
    throw new Error("useSettings must be used within a SettingsProvider");
  }
  return ctx;
}
