import { create } from "zustand";
import { PROMPT_KIND_LIST, type PromptKind } from "../config/prompts/registry";
import { sweepRetiredPromptOverrides } from "../config/retiredPrompts";
import { chooseDictionaryStartupAction } from "../helpers/dictionaryStartup";
import i18n, { normalizeUiLanguage } from "../i18n";
import type { ChineseScriptPreference } from "../types/electron";
import { normalizeChineseScriptPreference } from "../utils/chineseScript";
import logger from "../utils/logger";
import type { Snippet } from "../utils/snippets";

// Requires localStorage as well as window: the module-scope migrations below
// dereference the bare localStorage global, and test harnesses import this
// store with partial window stubs that don't define it.
const isBrowser = typeof window !== "undefined" && typeof localStorage !== "undefined";

export type MicrophoneSelectionMode = "auto" | "system" | "built-in" | "specific";
export type PanelStartPosition = "bottom-right" | "center" | "bottom-left";
export type ThemePreference = "light" | "dark" | "auto";
export type ActivationMode = "tap" | "push";

export interface SettingsState {
  uiLanguage: string;
  preferredLanguage: string;
  /** When transcription language is Auto, force Chinese output script. See #975. */
  chineseScriptPreference: ChineseScriptPreference;
  remoteTranscriptionUrl: string;
  remoteTranscriptionModel: string;
  customDictionary: string[];
  snippets: Snippet[];

  useCleanupModel: boolean;
  cleanupModel: string;
  cleanupRemoteUrl: string;
  cleanupCustomApiKey: string;
  cleanupDisableThinking: boolean;
  customPrompts: Record<PromptKind, string>;

  dictationKey: string;
  /** Hotkey actually registered by the main process. Display-only. */
  activeDictationKey: string | null;
  activationMode: ActivationMode;

  microphoneSelectionMode: MicrophoneSelectionMode;
  preferBuiltInMic: boolean;
  selectedMicDeviceId: string;
  selectedMicDeviceLabel: string;

  theme: ThemePreference;
  audioRetentionDays: number;
  transcriptRetentionDays: number;
  dataRetentionEnabled: boolean;
  saveDiscardedTranscriptions: boolean;
  audioCuesEnabled: boolean;
  pauseMediaOnDictation: boolean;
  floatingIconAutoHide: boolean;
  startMinimized: boolean;
  showMenuBarIcon: boolean;
  panelStartPosition: PanelStartPosition;
  autoPasteEnabled: boolean;
  keepTranscriptionInClipboard: boolean;

  setUiLanguage: (language: string) => void;
  setPreferredLanguage: (language: string) => void;
  setChineseScriptPreference: (value: ChineseScriptPreference) => void;
  setRemoteTranscriptionUrl: (url: string) => void;
  setRemoteTranscriptionModel: (model: string) => void;
  setCustomDictionary: (words: string[]) => void;
  updateCustomDictionary: (changes: { add?: string[]; remove?: string[] }) => void;
  applyCustomDictionaryFromExternal: (words: string[]) => void;
  setSnippets: (snippets: Snippet[]) => void;

  setUseCleanupModel: (value: boolean) => void;
  setCleanupModel: (model: string) => void;
  setCleanupRemoteUrl: (url: string) => void;
  setCleanupCustomApiKey: (key: string) => void;
  setCleanupDisableThinking: (value: boolean) => void;
  setCustomPrompt: (kind: PromptKind, value: string) => void;

  setDictationKey: (key: string) => void;
  setActivationMode: (mode: ActivationMode) => void;

  setMicrophoneSelectionMode: (mode: MicrophoneSelectionMode) => void;
  setPreferBuiltInMic: (value: boolean) => void;
  setSelectedMicDevice: (deviceId: string, label: string) => void;

  setTheme: (value: ThemePreference) => void;
  setAudioRetentionDays: (days: number) => void;
  setTranscriptRetentionDays: (days: number) => void;
  setDataRetentionEnabled: (value: boolean) => void;
  setSaveDiscardedTranscriptions: (value: boolean) => void;
  setAudioCuesEnabled: (value: boolean) => void;
  setPauseMediaOnDictation: (value: boolean) => void;
  setFloatingIconAutoHide: (enabled: boolean) => void;
  setStartMinimized: (enabled: boolean) => void;
  setShowMenuBarIcon: (visible: boolean) => void;
  setPanelStartPosition: (position: PanelStartPosition) => void;
  setAutoPasteEnabled: (value: boolean) => void;
  setKeepTranscriptionInClipboard: (value: boolean) => void;

  updateTranscriptionSettings: (
    settings: Partial<Pick<SettingsState, "preferredLanguage" | "chineseScriptPreference">>
  ) => void;
  updateCleanupSettings: (
    settings: Partial<Pick<SettingsState, "useCleanupModel" | "cleanupModel" | "cleanupRemoteUrl">>
  ) => void;
}

function readString(key: string, fallback: string): string {
  if (!isBrowser) return fallback;
  return localStorage.getItem(key) ?? fallback;
}

function readBoolean(key: string, fallback: boolean): boolean {
  if (!isBrowser) return fallback;
  const stored = localStorage.getItem(key);
  if (stored === null) return fallback;
  if (fallback === true) return stored !== "false";
  return stored === "true";
}

function readNumber(key: string, fallback: number): number {
  if (!isBrowser) return fallback;
  const parsed = parseInt(localStorage.getItem(key) ?? "", 10);
  return isNaN(parsed) ? fallback : parsed;
}

function readJsonArray<T>(key: string, fallback: T[]): T[] {
  if (!isBrowser) return fallback;
  const stored = localStorage.getItem(key);
  if (stored === null) return fallback;
  try {
    const parsed = JSON.parse(stored);
    return Array.isArray(parsed) ? (parsed as T[]) : fallback;
  } catch {
    return fallback;
  }
}

// Idle microphone capture is no longer supported. Remove only its retired preference.
if (isBrowser) localStorage.removeItem("micWarmHoldSeconds");

function migrateMicrophoneSelectionMode() {
  if (!isBrowser) return;
  const current = localStorage.getItem("microphoneSelectionMode");
  if (current === "auto" || current === "built-in" || current === "specific") return;

  if (current === "system") {
    localStorage.setItem("microphoneSelectionMode", "auto");
    localStorage.setItem("preferBuiltInMic", "false");
    return;
  }

  const selectedDeviceId = localStorage.getItem("selectedMicDeviceId") || "";
  const legacyBuiltIn = localStorage.getItem("preferBuiltInMic");
  const mode: MicrophoneSelectionMode =
    legacyBuiltIn === "true"
      ? "built-in"
      : selectedDeviceId && selectedDeviceId !== "default"
        ? "specific"
        : "auto";
  localStorage.setItem("microphoneSelectionMode", mode);
}

migrateMicrophoneSelectionMode();

const LANGUAGE_MIGRATIONS: Record<string, string> = { zh: "zh-CN" };

function migratePreferredLanguage() {
  if (!isBrowser) return;
  const stored = localStorage.getItem("preferredLanguage");
  if (stored && LANGUAGE_MIGRATIONS[stored]) {
    localStorage.setItem("preferredLanguage", LANGUAGE_MIGRATIONS[stored]);
  }
}

migratePreferredLanguage();

const BOOLEAN_SETTINGS = new Set([
  "useCleanupModel",
  "cleanupDisableThinking",
  "preferBuiltInMic",
  "dataRetentionEnabled",
  "saveDiscardedTranscriptions",
  "audioCuesEnabled",
  "pauseMediaOnDictation",
  "floatingIconAutoHide",
  "startMinimized",
  "showMenuBarIcon",
  "autoPasteEnabled",
  "keepTranscriptionInClipboard",
]);

const ARRAY_SETTINGS = new Set(["customDictionary", "snippets"]);

const NUMERIC_SETTINGS = new Set(["audioRetentionDays", "transcriptRetentionDays"]);

function persist(key: string, value: string) {
  if (isBrowser) localStorage.setItem(key, value);
}

function createStringSetter(key: keyof SettingsState) {
  return (value: string) => {
    persist(key, value);
    useSettingsStore.setState({ [key]: value });
  };
}

function createBooleanSetter(key: keyof SettingsState) {
  return (value: boolean) => {
    persist(key, String(value));
    useSettingsStore.setState({ [key]: value });
  };
}

function createNumberSetter(key: keyof SettingsState) {
  return (value: number) => {
    persist(key, String(value));
    useSettingsStore.setState({ [key]: value });
  };
}

// The cleanup server key is a secret kept in the OS secure store, never in
// localStorage; saves are debounced so typing does not hit it per keystroke.
let cleanupKeySaveTimer: ReturnType<typeof setTimeout> | null = null;
function debouncedSaveCleanupKey(key: string) {
  if (!isBrowser) return;
  if (cleanupKeySaveTimer) clearTimeout(cleanupKeySaveTimer);
  cleanupKeySaveTimer = setTimeout(() => {
    window.electronAPI?.saveCleanupCustomKey?.(key)?.catch((err) => {
      logger.warn("Failed to persist secret", { error: (err as Error).message }, "settings");
    });
  }, 250);
}

function normalizeMicrophoneSelectionMode(mode: string): MicrophoneSelectionMode {
  return mode === "built-in" || mode === "specific" ? mode : "auto";
}

export const useSettingsStore = create<SettingsState>()((set, get) => ({
  uiLanguage: normalizeUiLanguage(
    isBrowser ? localStorage.getItem("uiLanguage") || i18n.language : null
  ),
  preferredLanguage: readString("preferredLanguage", "auto"),
  chineseScriptPreference: normalizeChineseScriptPreference(
    readString("chineseScriptPreference", "as-transcribed")
  ),
  remoteTranscriptionUrl: readString("remoteTranscriptionUrl", ""),
  remoteTranscriptionModel: readString("remoteTranscriptionModel", ""),
  customDictionary: readJsonArray<string>("customDictionary", []),
  snippets: readJsonArray<Snippet>("snippets", []),

  useCleanupModel: readBoolean("useCleanupModel", true),
  cleanupModel: readString("cleanupModel", ""),
  cleanupRemoteUrl: readString("cleanupRemoteUrl", ""),
  // Hydrated from the main process in initializeSettings, never from localStorage.
  cleanupCustomApiKey: "",
  cleanupDisableThinking: readBoolean("cleanupDisableThinking", true),
  customPrompts: PROMPT_KIND_LIST.reduce(
    (acc, kind) => ({ ...acc, [kind]: readString(`customPrompt.${kind}`, "") }),
    {} as Record<PromptKind, string>
  ),

  dictationKey: readString("dictationKey", ""),
  activeDictationKey: null,
  activationMode: readString("activationMode", "tap") === "push" ? "push" : "tap",

  microphoneSelectionMode: normalizeMicrophoneSelectionMode(
    readString("microphoneSelectionMode", "auto")
  ),
  preferBuiltInMic: readString("microphoneSelectionMode", "auto") === "built-in",
  selectedMicDeviceId: readString("selectedMicDeviceId", ""),
  selectedMicDeviceLabel: readString("selectedMicDeviceLabel", ""),

  theme: (() => {
    const v = readString("theme", "auto");
    return v === "light" || v === "dark" ? v : "auto";
  })(),
  audioRetentionDays: readNumber("audioRetentionDays", 30),
  transcriptRetentionDays: readNumber("transcriptRetentionDays", 0),
  dataRetentionEnabled: readBoolean("dataRetentionEnabled", true),
  saveDiscardedTranscriptions: readBoolean("saveDiscardedTranscriptions", false),
  audioCuesEnabled: readBoolean("audioCuesEnabled", true),
  pauseMediaOnDictation: readBoolean("pauseMediaOnDictation", false),
  floatingIconAutoHide: readBoolean("floatingIconAutoHide", false),
  startMinimized: readBoolean("startMinimized", false),
  showMenuBarIcon: readBoolean("showMenuBarIcon", true),
  panelStartPosition: (() => {
    const v = readString("panelStartPosition", "bottom-right");
    return v === "center" || v === "bottom-left" ? v : "bottom-right";
  })(),
  autoPasteEnabled: readBoolean("autoPasteEnabled", true),
  keepTranscriptionInClipboard: readBoolean("keepTranscriptionInClipboard", false),

  setUiLanguage: (language: string) => {
    const normalized = normalizeUiLanguage(language);
    persist("uiLanguage", normalized);
    set({ uiLanguage: normalized });
    void i18n.changeLanguage(normalized);
    if (isBrowser && window.electronAPI?.setUiLanguage) {
      window.electronAPI.setUiLanguage(normalized).catch((err) => {
        logger.warn(
          "Failed to sync UI language to main process",
          { error: (err as Error).message },
          "settings"
        );
      });
    }
  },
  setPreferredLanguage: createStringSetter("preferredLanguage"),
  setChineseScriptPreference: (value: ChineseScriptPreference) =>
    createStringSetter("chineseScriptPreference")(normalizeChineseScriptPreference(value)),
  setRemoteTranscriptionUrl: createStringSetter("remoteTranscriptionUrl"),
  setRemoteTranscriptionModel: createStringSetter("remoteTranscriptionModel"),

  // Replaces the whole dictionary: anything absent from `words` is deleted.
  // Editing specific words wants updateCustomDictionary instead (#1295).
  setCustomDictionary: (words: string[]) => {
    persist("customDictionary", JSON.stringify(words));
    set({ customDictionary: words });
    window.electronAPI?.setDictionary(words).catch((err) => {
      logger.warn(
        "Failed to sync dictionary to SQLite",
        { error: (err as Error).message },
        "settings"
      );
    });
  },

  updateCustomDictionary: ({ add = [], remove = [] }) => {
    const removeLower = new Set(remove.map((w) => w.toLowerCase()));
    const addLower = new Set(add.map((w) => w.toLowerCase()));
    // Optimistic so the UI updates immediately; the stored list replaces it below.
    const optimistic = [
      ...get().customDictionary.filter((w) => {
        const lower = w.toLowerCase();
        return !removeLower.has(lower) && !addLower.has(lower);
      }),
      ...add,
    ];
    persist("customDictionary", JSON.stringify(optimistic));
    set({ customDictionary: optimistic });

    const api = window.electronAPI;
    if (!api) return;
    // Older preloads have no delta channel; fall back rather than drop the edit.
    const written = api.applyDictionaryChanges
      ? api.applyDictionaryChanges({ add, remove })
      : api.setDictionary(optimistic);

    written
      .then(async () => {
        // SQLite owns ordering, casing and dedupe — adopt what it stored.
        const stored = await api.getDictionary?.();
        if (stored) {
          persist("customDictionary", JSON.stringify(stored));
          set({ customDictionary: stored });
        }
      })
      .catch((err) => {
        logger.warn(
          "Failed to apply dictionary changes to SQLite",
          { error: (err as Error).message },
          "settings"
        );
      });
  },

  // For broadcasts from main process — DB is already authoritative, only update UI.
  applyCustomDictionaryFromExternal: (words: string[]) => {
    persist("customDictionary", JSON.stringify(words));
    set({ customDictionary: words });
  },

  setSnippets: (snippets: Snippet[]) => {
    persist("snippets", JSON.stringify(snippets));
    set({ snippets });
    window.electronAPI?.setSnippets?.(snippets).catch((err) => {
      logger.warn(
        "Failed to sync snippets to SQLite",
        { error: (err as Error).message },
        "settings"
      );
    });
  },

  setUseCleanupModel: createBooleanSetter("useCleanupModel"),
  setCleanupModel: createStringSetter("cleanupModel"),
  setCleanupRemoteUrl: createStringSetter("cleanupRemoteUrl"),
  setCleanupCustomApiKey: (key: string) => {
    set({ cleanupCustomApiKey: key });
    debouncedSaveCleanupKey(key);
  },
  setCleanupDisableThinking: createBooleanSetter("cleanupDisableThinking"),
  setCustomPrompt: (kind, value) => {
    persist(`customPrompt.${kind}`, value);
    set((s) => ({ customPrompts: { ...s.customPrompts, [kind]: value } }));
  },

  setDictationKey: (key: string) => {
    persist("dictationKey", key);
    set({ dictationKey: key });
    if (isBrowser) {
      window.electronAPI?.notifyHotkeyChanged?.(key);
      window.electronAPI?.saveDictationKey?.(key);
    }
  },
  setActivationMode: (mode: ActivationMode) => {
    persist("activationMode", mode);
    set({ activationMode: mode });
    if (isBrowser) window.electronAPI?.notifyActivationModeChanged?.(mode);
  },

  setMicrophoneSelectionMode: (mode: MicrophoneSelectionMode) => {
    const normalized = normalizeMicrophoneSelectionMode(mode);
    const preferBuiltInMic = normalized === "built-in";
    persist("microphoneSelectionMode", normalized);
    persist("preferBuiltInMic", String(preferBuiltInMic));
    set({ microphoneSelectionMode: normalized, preferBuiltInMic });
  },
  setPreferBuiltInMic: (value: boolean) => {
    const mode: MicrophoneSelectionMode = value ? "built-in" : "auto";
    persist("preferBuiltInMic", String(value));
    persist("microphoneSelectionMode", mode);
    set({ preferBuiltInMic: value, microphoneSelectionMode: mode });
  },
  setSelectedMicDevice: (deviceId: string, label: string) => {
    persist("selectedMicDeviceLabel", label);
    persist("selectedMicDeviceId", deviceId);
    set({ selectedMicDeviceId: deviceId, selectedMicDeviceLabel: label });
  },

  setTheme: (value: ThemePreference) => {
    persist("theme", value);
    set({ theme: value });
  },
  setAudioRetentionDays: createNumberSetter("audioRetentionDays"),
  setTranscriptRetentionDays: createNumberSetter("transcriptRetentionDays"),
  setDataRetentionEnabled: (value: boolean) => {
    persist("dataRetentionEnabled", String(value));
    set({ dataRetentionEnabled: value });
    logger.info(
      value
        ? "Data retention enabled — transcriptions and audio will be saved"
        : "Data retention disabled — transcriptions and audio will not be saved",
      {},
      "settings"
    );
  },
  setSaveDiscardedTranscriptions: createBooleanSetter("saveDiscardedTranscriptions"),
  setAudioCuesEnabled: createBooleanSetter("audioCuesEnabled"),
  setPauseMediaOnDictation: createBooleanSetter("pauseMediaOnDictation"),

  setFloatingIconAutoHide: (enabled: boolean) => {
    if (get().floatingIconAutoHide === enabled) return;
    persist("floatingIconAutoHide", String(enabled));
    set({ floatingIconAutoHide: enabled });
    if (isBrowser) window.electronAPI?.notifyFloatingIconAutoHideChanged?.(enabled);
  },
  setStartMinimized: (enabled: boolean) => {
    if (get().startMinimized === enabled) return;
    persist("startMinimized", String(enabled));
    set({ startMinimized: enabled });
    if (isBrowser) window.electronAPI?.notifyStartMinimizedChanged?.(enabled);
  },
  setShowMenuBarIcon: (visible: boolean) => {
    if (get().showMenuBarIcon === visible) return;
    persist("showMenuBarIcon", String(visible));
    set({ showMenuBarIcon: visible });
    if (isBrowser) {
      window.electronAPI?.setMenuBarIconVisible?.(visible).catch((error) => {
        logger.warn("Failed to save menu bar visibility", { error: error.message }, "settings");
      });
    }
  },
  setPanelStartPosition: (position: PanelStartPosition) => {
    if (get().panelStartPosition === position) return;
    persist("panelStartPosition", position);
    set({ panelStartPosition: position });
    if (isBrowser) window.electronAPI?.notifyPanelStartPositionChanged?.(position);
  },
  setAutoPasteEnabled: createBooleanSetter("autoPasteEnabled"),
  setKeepTranscriptionInClipboard: createBooleanSetter("keepTranscriptionInClipboard"),

  updateTranscriptionSettings: (settings) => {
    const s = get();
    if (settings.preferredLanguage !== undefined)
      s.setPreferredLanguage(settings.preferredLanguage);
    if (settings.chineseScriptPreference !== undefined)
      s.setChineseScriptPreference(settings.chineseScriptPreference);
  },
  updateCleanupSettings: (settings) => {
    const s = get();
    if (settings.useCleanupModel !== undefined) s.setUseCleanupModel(settings.useCleanupModel);
    if (settings.cleanupModel !== undefined) s.setCleanupModel(settings.cleanupModel);
    if (settings.cleanupRemoteUrl !== undefined) s.setCleanupRemoteUrl(settings.cleanupRemoteUrl);
  },
}));

// Overrides that byte-match a retired shipped default were persisted defaults,
// not user customizations; clear them so current defaults apply again.
function sweepRetiredCustomPrompts() {
  if (!isBrowser) return;
  void sweepRetiredPromptOverrides(localStorage, PROMPT_KIND_LIST)
    .then((swept) => {
      if (swept.length === 0) return;
      useSettingsStore.setState((s) => ({
        customPrompts: {
          ...s.customPrompts,
          ...Object.fromEntries(swept.map((kind) => [kind, ""])),
        },
      }));
      logger.info("Cleared retired default prompt overrides", { kinds: swept }, "settings");
    })
    .catch((error) => {
      logger.warn(
        "Retired prompt sweep failed",
        { error: error instanceof Error ? error.message : String(error) },
        "settings"
      );
    });
}

sweepRetiredCustomPrompts();

export function getSettings(): SettingsState {
  return useSettingsStore.getState();
}

export function getEffectiveCleanupModel(): string {
  return getSettings().cleanupModel;
}

// --- Initialization ---

let hasInitialized = false;

export async function initializeSettings(): Promise<void> {
  if (hasInitialized) return;
  hasInitialized = true;

  if (!isBrowser) return;

  const state = useSettingsStore.getState();

  if (window.electronAPI) {
    try {
      const cleanupKey = await window.electronAPI.getCleanupCustomKey?.();
      useSettingsStore.setState({ cleanupCustomApiKey: cleanupKey || "" });
    } catch (err) {
      logger.warn(
        "Failed to hydrate secrets from main process",
        { error: (err as Error).message },
        "settings"
      );
    }

    // localStorage holds the user's preferred hotkey. Only populate from .env
    // when localStorage is empty (fresh install / cleared data).
    try {
      if (!state.dictationKey) {
        const envKey = await window.electronAPI.getDictationKey?.();
        if (envKey) createStringSetter("dictationKey")(envKey);
      }
    } catch (err) {
      logger.warn(
        "Failed to sync dictation key on startup",
        { error: (err as Error).message },
        "settings"
      );
    }

    // Track what is actually registered, separately from the editable
    // dictationKey preference so partial registrations never get persisted.
    try {
      const activeKey = await window.electronAPI.getActiveDictationKey?.();
      if (activeKey) useSettingsStore.setState({ activeDictationKey: activeKey });
    } catch (err) {
      logger.warn(
        "Failed to sync active dictation key on startup",
        { error: (err as Error).message },
        "settings"
      );
    }

    try {
      const envMode = await window.electronAPI.getActivationMode?.();
      if (envMode && envMode !== state.activationMode) {
        persist("activationMode", envMode);
        useSettingsStore.setState({ activationMode: envMode });
      }
    } catch (err) {
      logger.warn(
        "Failed to sync activation mode on startup",
        { error: (err as Error).message },
        "settings"
      );
    }

    // Main-process storage also controls the icon before renderer startup.
    try {
      const visible = await window.electronAPI.getMenuBarIconVisible?.();
      if (typeof visible === "boolean") {
        persist("showMenuBarIcon", String(visible));
        useSettingsStore.setState({ showMenuBarIcon: visible });
      }
    } catch (err) {
      logger.warn(
        "Failed to sync menu bar visibility on startup",
        { error: (err as Error).message },
        "settings"
      );
    }

    try {
      const envLanguage = await window.electronAPI.getUiLanguage?.();
      const resolved = normalizeUiLanguage(envLanguage || state.uiLanguage);
      if (resolved !== state.uiLanguage) {
        persist("uiLanguage", resolved);
        useSettingsStore.setState({ uiLanguage: resolved });
      }
      await i18n.changeLanguage(resolved);
    } catch (err) {
      logger.warn(
        "Failed to sync UI language on startup",
        { error: (err as Error).message },
        "settings"
      );
      void i18n.changeLanguage(normalizeUiLanguage(state.uiLanguage));
    }

    const migratedLang = localStorage.getItem("preferredLanguage");
    if (migratedLang && migratedLang !== state.preferredLanguage) {
      useSettingsStore.setState({ preferredLanguage: migratedLang });
    }

    // Prefer SQLite whenever it has entries (same policy as snippets). A stale
    // cache used to win when both sides were non-empty; ensureAgentNameInDictionary
    // then wrote that cache through setDictionary and wiped newer DB words (#1295).
    try {
      if (window.electronAPI.getDictionary) {
        const currentDictionary = useSettingsStore.getState().customDictionary;
        const dbWords = await window.electronAPI.getDictionary();
        const decision = chooseDictionaryStartupAction(dbWords, currentDictionary);
        if (decision.action === "push-local-to-db") {
          await window.electronAPI.setDictionary(decision.words);
        } else if (decision.action === "pull-db-to-local") {
          persist("customDictionary", JSON.stringify(decision.words));
          useSettingsStore.setState({ customDictionary: decision.words });
        }
      }
    } catch (err) {
      logger.warn(
        "Failed to sync dictionary on startup",
        { error: (err as Error).message },
        "settings"
      );
    }

    try {
      if (window.electronAPI.getSnippets) {
        const currentSnippets = useSettingsStore.getState().snippets;
        const dbSnippets = await window.electronAPI.getSnippets();
        if (dbSnippets.length === 0 && currentSnippets.length > 0) {
          await window.electronAPI.setSnippets?.(currentSnippets);
          const normalizedSnippets = await window.electronAPI.getSnippets();
          persist("snippets", JSON.stringify(normalizedSnippets));
          useSettingsStore.setState({ snippets: normalizedSnippets });
        } else if (dbSnippets.length > 0) {
          persist("snippets", JSON.stringify(dbSnippets));
          useSettingsStore.setState({ snippets: dbSnippets });
        }
      }
    } catch (err) {
      logger.warn(
        "Failed to sync snippets on startup",
        { error: (err as Error).message },
        "settings"
      );
    }
  }

  // Sync Zustand store when another window writes to localStorage
  window.addEventListener("storage", (event) => {
    if (!event.key || event.storageArea !== localStorage || event.newValue === null) return;

    const { key, newValue } = event;

    if (key.startsWith("customPrompt.")) {
      const kind = key.slice("customPrompt.".length) as PromptKind;
      if (!PROMPT_KIND_LIST.includes(kind)) return;
      useSettingsStore.setState((s) => ({
        customPrompts: { ...s.customPrompts, [kind]: newValue },
      }));
      return;
    }

    const current = useSettingsStore.getState() as unknown as Record<string, unknown>;
    if (!(key in current) || typeof current[key] === "function") return;

    let value: unknown;
    if (BOOLEAN_SETTINGS.has(key)) {
      value = newValue === "true";
    } else if (ARRAY_SETTINGS.has(key)) {
      try {
        const parsed = JSON.parse(newValue);
        value = Array.isArray(parsed) ? parsed : [];
      } catch {
        value = [];
      }
    } else if (NUMERIC_SETTINGS.has(key)) {
      const parsed = Number(newValue);
      if (Number.isNaN(parsed)) {
        value = key === "audioRetentionDays" ? 30 : current[key];
      } else if (key === "audioRetentionDays") {
        value = Math.round(parsed);
      } else {
        value = parsed;
      }
    } else {
      value = newValue;
    }

    useSettingsStore.setState({ [key]: value });

    if (key === "uiLanguage" && typeof value === "string") {
      void i18n.changeLanguage(value);
    }
  });

  // Active hotkey updates from backend — display state, never persisted.
  window.electronAPI?.onDictationKeyActive?.((key: string) => {
    useSettingsStore.setState({ activeDictationKey: key });
  });

  // Sync settings pushed from main process (e.g., hotkey changed in control panel)
  window.electronAPI?.onSettingUpdated?.((data: { key: string; value: unknown }) => {
    const current = useSettingsStore.getState() as unknown as Record<string, unknown>;
    if (data.key in current && typeof current[data.key] !== "function") {
      localStorage.setItem(
        data.key,
        typeof data.value === "string" ? data.value : JSON.stringify(data.value)
      );
      useSettingsStore.setState({ [data.key]: data.value });
    }
  });
}
