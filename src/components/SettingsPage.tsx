import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useClipboard } from "../hooks/useClipboard";
import { useDialogs } from "../hooks/useDialogs";
import { usePermissions } from "../hooks/usePermissions";
import { useSettings } from "../hooks/useSettings";
import SelfHostedPanel from "./SelfHostedPanel";
import {
  AlertTriangle,
  BookOpen,
  CircleCheck,
  CircleX,
  Copy,
  FileAudio,
  Info,
  Languages,
  MessageSquare,
  Mic,
  Monitor,
  Moon,
  Network,
  RotateCw,
  Shield,
  Sparkles,
  Sun,
  Upload,
  UserCircle,
  Wand2,
} from "./icons";
import { BIDI_VALUE_TOKEN, BidiInterpolatedText } from "./ui/BidiInterpolatedText";
import MicPermissionWarning from "./ui/MicPermissionWarning";
import MicrophoneSettings from "./ui/MicrophoneSettings";
import NixOsPasteInfo from "./ui/NixOsPasteInfo";
import PasteToolsInfo from "./ui/PasteToolsInfo";
import PermissionCard from "./ui/PermissionCard";
import { Alert, AlertDescription, AlertTitle } from "./ui/alert";
import { Button } from "./ui/button";
import {
  AlertDialog,
  ConfirmDialog,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";

import { useHotkeyModeInfo } from "../hooks/useHotkeyModeInfo";
import { useHotkeyRegistration } from "../hooks/useHotkeyRegistration";
import { useLocalStorage } from "../hooks/useLocalStorage";
import { usePolicySnapshot } from "../hooks/usePolicy";
import { useTheme } from "../hooks/useTheme";
import {
  effectiveAudioRetentionDays,
  effectiveLocalHistoryEnabled,
  isAgentAllowed,
  lockedLocalHistoryValue,
  maxAudioRetentionDays,
} from "../stores/policyRules";
import { usePolicyStore } from "../stores/policyStore";
import type {
  ChineseScriptPreference,
  InferenceMode,
  LocalTranscriptionProvider,
} from "../types/electron";
import { formatBytes } from "../utils/formatBytes";
import { validateHotkeyForSlot } from "../utils/hotkeyValidation";
import { formatHotkeyLabel } from "../utils/hotkeys";
import {
  getLinuxPasteInstallCommands,
  needsLinuxPasteToolGuidance,
} from "../utils/linuxPasteTools";
import logger from "../utils/logger";
import { getCachedPlatform } from "../utils/platform";
import { cn } from "./lib/utils";
import InferenceConfigEditor from "./settings/InferenceConfigEditor";
import { UploadTranscriptionPanel } from "./settings/UploadSettings";
import { ActivationModeSelector } from "./ui/ActivationModeSelector";
import { HotkeyListInput } from "./ui/HotkeyListInput";
import LanguageSelector from "./ui/LanguageSelector";
import LinuxPttSetupInfo from "./ui/LinuxPttSetupInfo";
import PromptStudio from "./ui/PromptStudio";
import { ProviderTabs } from "./ui/ProviderTabs";
import { InferenceModeSelector, SettingsRow } from "./ui/SettingsSection";
import { GRADIENT_CIRCLE } from "./ui/gradientCircle";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { Toggle } from "./ui/toggle";
import { useSettingsLayout } from "./ui/useSettingsLayout";
import { useToast } from "./ui/useToast";

export type SettingsSectionType =
  | "account"
  | "plansBilling"
  | "workspace"
  | "general"
  | "hotkeys"
  | "speechToText"
  | "llms"
  | "privacyData"
  | "system";

interface SettingsPageProps {
  activeSection?: SettingsSectionType;
  onNavigateToSection?: (section: SettingsSectionType) => void;
  /** When a legacy section ID was used (e.g. `meetings`), land on the matching sub-tab. */
  initialSubTab?: string;
}

const UI_LANGUAGE_OPTIONS: import("./ui/LanguageSelector").LanguageOption[] = [
  { value: "en", label: "English", flag: "🇺🇸" },
  { value: "ar", label: "العربية", flag: "🇦🇪" },
  { value: "es", label: "Español", flag: "🇪🇸" },
  { value: "fr", label: "Français", flag: "🇫🇷" },
  { value: "de", label: "Deutsch", flag: "🇩🇪" },
  { value: "pt", label: "Português", flag: "🇵🇹" },
  { value: "it", label: "Italiano", flag: "🇮🇹" },
  { value: "ru", label: "Русский", flag: "🇷🇺" },
  { value: "ja", label: "日本語", flag: "🇯🇵" },
  { value: "zh-CN", label: "简体中文", flag: "🇨🇳" },
  { value: "zh-TW", label: "繁體中文", flag: "🇹🇼" },
];

const RETENTION_DAY_OPTIONS = [1, 7, 14, 30, 60, 90];

const RETENTION_SELECT_CLASS =
  "h-7 rounded border border-border/70 bg-surface-1/80 px-2.5 text-xs font-medium text-foreground shadow-sm backdrop-blur-sm hover:border-border-hover hover:bg-surface-2/70 focus:outline-none focus:ring-2 focus:ring-ring/30 focus:ring-offset-1 disabled:cursor-not-allowed disabled:opacity-50 transition-colors duration-200";

function SettingsPanel({
  children,
  className = "",
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={`rounded-lg border border-border/70 dark:border-border-subtle/70 bg-card/50 dark:bg-surface-2/50 backdrop-blur-sm divide-y divide-border/60 dark:divide-border-subtle/50 ${className}`}
    >
      {children}
    </div>
  );
}

function SettingsPanelRow({
  children,
  className = "",
}: {
  children: React.ReactNode;
  className?: string;
}) {
  const { isCompact } = useSettingsLayout();

  return (
    <div className={`${isCompact ? "px-3 py-2.5" : "px-4 py-3"} ${className}`}>{children}</div>
  );
}

function SectionHeader({
  title,
  description,
  note,
}: {
  title: string;
  description?: string;
  note?: string;
}) {
  return (
    <div className="mb-3">
      <h3 className="text-xs font-semibold text-foreground tracking-tight">{title}</h3>
      {description && (
        <p className="text-xs text-muted-foreground/80 mt-0.5 leading-relaxed">{description}</p>
      )}
      {note && <p className="text-xs text-muted-foreground/80 mt-0.5 leading-relaxed">{note}</p>}
    </div>
  );
}

interface TranscriptionSectionProps {
  isSignedIn: boolean;
  startOnboarding: () => void;
  cloudTranscriptionMode: string;
  setCloudTranscriptionMode: (mode: string) => void;
  useLocalWhisper: boolean;
  setUseLocalWhisper: (value: boolean) => void;
  updateTranscriptionSettings: (settings: { useLocalWhisper: boolean }) => void;
  cloudTranscriptionProvider: string;
  setCloudTranscriptionProvider: (provider: string) => void;
  cloudTranscriptionModel: string;
  setCloudTranscriptionModel: (model: string) => void;
  localTranscriptionProvider: string;
  setLocalTranscriptionProvider: (provider: LocalTranscriptionProvider) => void;
  whisperModel: string;
  setWhisperModel: (model: string) => void;
  parakeetModel: string;
  setParakeetModel: (model: string) => void;
  cohereModel: string;
  setCohereModel: (model: string) => void;
  cloudTranscriptionBaseUrl?: string;
  setCloudTranscriptionBaseUrl: (url: string) => void;
  transcriptionMode: InferenceMode;
  setTranscriptionMode: (mode: InferenceMode) => void;
  remoteTranscriptionUrl: string;
  setRemoteTranscriptionUrl: (url: string) => void;
  remoteTranscriptionModel: string;
  setRemoteTranscriptionModel: (model: string) => void;
  showTranscriptionPreview: boolean;
  setShowTranscriptionPreview: (value: boolean) => void;
  toast: (opts: {
    title: string;
    description: string;
    variant?: "default" | "destructive" | "success";
    duration?: number;
  }) => void;
}

function TranscriptionSection({
  setTranscriptionMode,
  remoteTranscriptionUrl,
  setRemoteTranscriptionUrl,
  remoteTranscriptionModel,
  setRemoteTranscriptionModel,
}: TranscriptionSectionProps) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4">
      <InferenceModeSelector
        modes={[
          {
            id: "self-hosted",
            label: t("settingsPage.transcription.modes.selfHosted"),
            description: t("settingsPage.transcription.modes.selfHostedDesc"),
            icon: <Network className="w-4 h-4" />,
          },
        ]}
        activeMode="self-hosted"
        onSelect={setTranscriptionMode}
      />
      <SelfHostedPanel
        service="transcription"
        url={remoteTranscriptionUrl}
        onUrlChange={setRemoteTranscriptionUrl}
        model={remoteTranscriptionModel}
        onModelChange={setRemoteTranscriptionModel}
      />
    </div>
  );
}

interface AiModelsSectionProps {
  useCleanupModel: boolean;
  setUseCleanupModel: (value: boolean) => void;
  toast: (opts: {
    title: string;
    description: string;
    variant?: "default" | "destructive" | "success";
    duration?: number;
  }) => void;
}

const CLEANUP_MODE_TOAST_KEY: Record<InferenceMode, string> = {
  openwhispr: "switchedCloud",
  providers: "switchedProviders",
  local: "switchedLocal",
  "self-hosted": "switchedSelfHosted",
  enterprise: "switchedEnterprise",
};

function AiModelsSection({ useCleanupModel, setUseCleanupModel, toast }: AiModelsSectionProps) {
  const { t } = useTranslation();

  const handleCleanupModeChange = (mode: InferenceMode) => {
    const toastKey = CLEANUP_MODE_TOAST_KEY[mode];
    toast({
      title: t(`settingsPage.aiModels.toasts.${toastKey}.title`),
      description: t(`settingsPage.aiModels.toasts.${toastKey}.description`),
      variant: "success",
      duration: 3000,
    });
  };

  return (
    <div className="space-y-4">
      <SettingsPanel>
        <SettingsPanelRow>
          <SettingsRow
            label={t("settingsPage.aiModels.enableTextCleanup")}
            description={t("settingsPage.aiModels.enableTextCleanupDescription")}
          >
            <Toggle checked={useCleanupModel} onChange={setUseCleanupModel} />
          </SettingsRow>
        </SettingsPanelRow>
      </SettingsPanel>

      {useCleanupModel && (
        <>
          <InferenceConfigEditor scope="dictationCleanup" onModeChange={handleCleanupModeChange} />
        </>
      )}
    </div>
  );
}

type SpeechTab = "dictation" | "noteRecording" | "upload";
type LlmTab =
  | "dictationCleanup"
  | "dictationAgent"
  | "dictationTranslation"
  | "noteFormatting"
  | "chatIntelligence";

const SPEECH_TABS: SpeechTab[] = ["dictation", "upload"];
const LLM_TABS: LlmTab[] = ["dictationCleanup"];
const AGENT_LLM_TABS = new Set<LlmTab>(["dictationAgent", "chatIntelligence"]);

function useSubTab<T extends string>(storageKey: string, options: readonly T[], initial?: T) {
  const [tab, setTab] = useLocalStorage<T>(storageKey, initial ?? options[0]);
  useEffect(() => {
    if (initial && initial !== tab) setTab(initial);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initial]);
  const safeTab = options.includes(tab) ? tab : options[0];
  return [safeTab, setTab] as const;
}

function TabPanel({ active, children }: { active: boolean; children: React.ReactNode }) {
  return <div className={active ? undefined : "hidden"}>{children}</div>;
}

// "Gabriel Stein" → "GS"; single names fall back to their first letter.
function nameInitials(name: string): string {
  const parts = name.trim().split(/\s+/);
  const first = parts[0]?.[0] ?? "";
  const last = parts.length > 1 ? (parts[parts.length - 1][0] ?? "") : "";
  return (first + last).toUpperCase();
}

export function AccountAvatar({ image, name }: { image?: string | null; name: string }) {
  // Same stale-URL fallback as MemberAvatar: OAuth-hosted images expire, and a
  // bare <img> would render the broken-image glyph instead of the initials.
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const initials = nameInitials(name);
  return (
    <div
      className={cn(
        "w-10 h-10 rounded-full flex items-center justify-center shrink-0 overflow-hidden",
        GRADIENT_CIRCLE
      )}
    >
      {image && image !== failedSrc ? (
        <img
          src={image}
          alt={name}
          loading="lazy"
          referrerPolicy="no-referrer"
          onError={() => setFailedSrc(image)}
          className="w-10 h-10 rounded-full object-cover"
        />
      ) : initials ? (
        <span className="text-[13px] font-semibold leading-none select-none">{initials}</span>
      ) : (
        <UserCircle className="w-5 h-5" />
      )}
    </div>
  );
}

function SpeechToTextTabs({
  initialTab,
  renderDictation,
}: {
  initialTab?: SpeechTab;
  renderDictation: () => React.ReactNode;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useSubTab<SpeechTab>("settings.speechToTextTab", SPEECH_TABS, initialTab);

  const subTabs = [
    { id: "dictation", name: t("settingsPage.speechToText.tabs.dictation") },
    { id: "upload", name: t("settingsPage.speechToText.tabs.upload") },
  ];

  return (
    <div className="space-y-4">
      <SectionHeader title={t("settingsPage.speechToText.title")} />
      <ProviderTabs
        providers={subTabs}
        selectedId={tab}
        onSelect={(id) => setTab(id as SpeechTab)}
        renderIcon={(id) =>
          id === "dictation" ? (
            <Mic className="w-3.5 h-3.5" />
          ) : id === "upload" ? (
            <Upload className="w-3.5 h-3.5" />
          ) : (
            <FileAudio className="w-3.5 h-3.5" />
          )
        }
      />
      <TabPanel active={tab === "dictation"}>{renderDictation()}</TabPanel>
      <TabPanel active={tab === "upload"}>
        <div className="space-y-6">
          <UploadTranscriptionPanel />
        </div>
      </TabPanel>
    </div>
  );
}

function LlmsTabs({
  initialTab,
  renderDictationCleanup,
}: {
  initialTab?: LlmTab;
  renderDictationCleanup: () => React.ReactNode;
}) {
  const { t } = useTranslation();
  const agentAllowed = usePolicyStore(isAgentAllowed);
  const visibleTabIds = agentAllowed
    ? LLM_TABS
    : LLM_TABS.filter((tabId) => !AGENT_LLM_TABS.has(tabId));
  const [tab, setTab] = useSubTab<LlmTab>("settings.llmsTab", visibleTabIds, initialTab);

  const subTabs = [
    { id: "dictationCleanup", name: t("settingsPage.llms.tabs.dictationCleanup") },
  ].filter((item) => visibleTabIds.includes(item.id as LlmTab));

  return (
    <div className="space-y-4">
      <SectionHeader title={t("settingsPage.llms.title")} />
      <ProviderTabs
        providers={subTabs}
        selectedId={tab}
        onSelect={(id) => setTab(id as LlmTab)}
        renderIcon={(id) => {
          if (id === "dictationCleanup") return <Wand2 className="w-3.5 h-3.5" />;
          if (id === "dictationAgent") return <Sparkles className="w-3.5 h-3.5" />;
          if (id === "dictationTranslation") return <Languages className="w-3.5 h-3.5" />;
          if (id === "noteFormatting") return <BookOpen className="w-3.5 h-3.5" />;
          return <MessageSquare className="w-3.5 h-3.5" />;
        }}
      />
      <TabPanel active={tab === "dictationCleanup"}>{renderDictationCleanup()}</TabPanel>
    </div>
  );
}

export default function SettingsPage({
  activeSection = "general",
  initialSubTab,
}: SettingsPageProps) {
  const { isCompact } = useSettingsLayout();
  const {
    confirmDialog,
    alertDialog,
    showConfirmDialog,
    showAlertDialog,
    hideConfirmDialog,
    hideAlertDialog,
  } = useDialogs();

  const {
    useLocalWhisper,
    whisperModel,
    localTranscriptionProvider,
    parakeetModel,
    cohereModel,
    uiLanguage,
    preferredLanguage,
    chineseScriptPreference,
    cloudTranscriptionProvider,
    cloudTranscriptionModel,
    cloudTranscriptionBaseUrl,
    useCleanupModel,
    dictationKey,
    activationMode,
    setActivationMode,
    microphoneSelectionMode,
    selectedMicDeviceId,
    selectedMicDeviceLabel,
    micWarmHoldSeconds,
    setMicrophoneSelectionMode,
    setSelectedMicDevice,
    setMicWarmHoldSeconds,
    setUseLocalWhisper,
    setUiLanguage,
    setWhisperModel,
    setLocalTranscriptionProvider,
    setParakeetModel,
    setCohereModel,
    setCloudTranscriptionProvider,
    setCloudTranscriptionModel,
    setCloudTranscriptionBaseUrl,
    setDictationKey,
    autoLearnCorrections,
    setAutoLearnCorrections,
    updateTranscriptionSettings,
    updateCleanupSettings,
    cloudTranscriptionMode,
    setCloudTranscriptionMode,
    transcriptionMode,
    setTranscriptionMode,
    remoteTranscriptionUrl,
    setRemoteTranscriptionUrl,
    remoteTranscriptionModel,
    setRemoteTranscriptionModel,
    audioCuesEnabled,
    setAudioCuesEnabled,
    pauseMediaOnDictation,
    setPauseMediaOnDictation,
    showTranscriptionPreview,
    setShowTranscriptionPreview,
    autoPasteEnabled,
    setAutoPasteEnabled,
    keepTranscriptionInClipboard,
    setKeepTranscriptionInClipboard,
    floatingIconAutoHide,
    setFloatingIconAutoHide,
    startMinimized,
    setStartMinimized,
    showMenuBarIcon,
    setShowMenuBarIcon,
    panelStartPosition,
    setPanelStartPosition,
    audioRetentionDays,
    setAudioRetentionDays,
    transcriptRetentionDays,
    setTranscriptRetentionDays,
    dataRetentionEnabled,
    setDataRetentionEnabled,
    saveDiscardedTranscriptions,
    setSaveDiscardedTranscriptions,
  } = useSettings();

  const settingsPolicyState = usePolicySnapshot();
  const historyLockedByPolicy = lockedLocalHistoryValue(settingsPolicyState) !== null;
  const effectiveDataRetentionEnabled = effectiveLocalHistoryEnabled(
    settingsPolicyState,
    dataRetentionEnabled
  );
  const audioRetentionCap = maxAudioRetentionDays(settingsPolicyState);
  const enforcedAudioRetentionDays = effectiveAudioRetentionDays(
    settingsPolicyState,
    audioRetentionDays
  );

  const { t } = useTranslation();
  const { toast } = useToast();
  const permissionsHook = usePermissions(showAlertDialog);
  useClipboard(showAlertDialog);
  const [audioStorageUsage, setAudioStorageUsage] = useState<{
    fileCount: number;
    totalBytes: number;
  }>({ fileCount: 0, totalBytes: 0 });

  useEffect(() => {
    if (activeSection !== "privacyData") return;
    window.electronAPI
      ?.getAudioStorageUsage?.()
      .then((usage: { fileCount: number; totalBytes: number }) => {
        if (usage) setAudioStorageUsage(usage);
      })
      .catch(() => {});
  }, [activeSection]);

  // Lazy keep-alive: mount AI sections only after the user has visited them once,
  // then keep them mounted so model-download progress and IPC listeners survive
  // section switches. The setState-during-render pattern flips the flag in the
  // same commit as the section change, so there's no blank frame on first visit.
  const [hasMountedSpeechToText, setHasMountedSpeechToText] = useState(
    activeSection === "speechToText"
  );
  const [hasMountedLlms, setHasMountedLlms] = useState(activeSection === "llms");
  if (activeSection === "speechToText" && !hasMountedSpeechToText) {
    setHasMountedSpeechToText(true);
  }
  if (activeSection === "llms" && !hasMountedLlms) {
    setHasMountedLlms(true);
  }

  const handleClearAllAudio = async () => {
    if (!window.electronAPI?.deleteAllAudio) return;
    try {
      await window.electronAPI.deleteAllAudio();
      setAudioStorageUsage({ fileCount: 0, totalBytes: 0 });
      toast({ title: t("settingsPage.privacy.clearAllAudio"), variant: "default" });
    } catch {
      // silent fail
    }
  };

  // Wayland paste tool status for diagnostics.
  const [ydotoolStatus, setYdotoolStatus] = useState<{
    isLinux: boolean;
    isWayland: boolean;
    hasYdotool: boolean;
    hasYdotoold: boolean;
    hasWtype: boolean;
    daemonRunning: boolean;
    hasService: boolean;
    hasUinput: boolean;
    hasUdevRule: boolean;
    hasGroup: boolean;
    isKde: boolean;
    isWlroots: boolean;
    hasXclip: boolean;
    hasXsel: boolean;
    isNixOS: boolean;
  } | null>(null);
  const [ydotoolGuideKey, setYdotoolGuideKey] = useState<string | null>(null);

  const refreshYdotoolStatus = useCallback(async () => {
    try {
      const status = await window.electronAPI?.getYdotoolStatus?.();
      if (status) setYdotoolStatus(status);
    } catch {}
  }, []);

  useEffect(() => {
    refreshYdotoolStatus();
  }, [refreshYdotoolStatus]);

  const { theme, setTheme } = useTheme();

  const { registerHotkey, isRegistering: isHotkeyRegistering } = useHotkeyRegistration({
    onSuccess: (registeredHotkey) => {
      setDictationKey(registeredHotkey);
    },
    showSuccessToast: false,
    showErrorToast: true,
    showAlert: showAlertDialog,
  });

  const validateDictationHotkey = useCallback(
    (hotkey: string) => validateHotkeyForSlot(hotkey, {}, t),
    [t]
  );

  const {
    isUsingNativeShortcut,
    isUsingHyprland,
    hyprlandConfigStatus,
    supportsPushToTalk,
    pushToTalkUnavailableReason,
  } = useHotkeyModeInfo("settings", dictationKey);
  const [effectiveDefaultHotkey, setEffectiveDefaultHotkey] = useState<string | null>(null);
  const [linuxPttAvailable, setLinuxPttAvailable] = useState(true);

  const platform = getCachedPlatform();

  const [autoStartEnabled, setAutoStartEnabled] = useState(false);
  const [autoStartNeedsApproval, setAutoStartNeedsApproval] = useState(false);
  const [autoStartLoading, setAutoStartLoading] = useState(true);

  const readAutoStartState = useCallback(async () => {
    if (!window.electronAPI?.getAutoStartEnabled) return;
    try {
      const state = await window.electronAPI.getAutoStartEnabled();
      setAutoStartEnabled(state.enabled);
      setAutoStartNeedsApproval(state.requiresApproval);
    } catch (error) {
      logger.error("Failed to get auto-start status", error, "settings");
    }
  }, []);

  useEffect(() => {
    readAutoStartState().finally(() => setAutoStartLoading(false));
  }, [readAutoStartState]);

  const handleAutoStartChange = async (enabled: boolean) => {
    if (!window.electronAPI?.setAutoStartEnabled) return;
    try {
      setAutoStartLoading(true);
      const result = await window.electronAPI.setAutoStartEnabled(enabled);
      // Read the state back rather than assuming: on Windows the OS can have the
      // item disabled out from under us, and on macOS it can need approval first.
      if (result.success) await readAutoStartState();
    } catch (error) {
      logger.error("Failed to set auto-start", error, "settings");
    } finally {
      setAutoStartLoading(false);
    }
  };

  useEffect(() => {
    const loadEffectiveDefaultHotkey = async () => {
      try {
        const key = await window.electronAPI?.getEffectiveDefaultHotkey?.();
        if (key) setEffectiveDefaultHotkey(key);
      } catch (error) {
        logger.error("Failed to get effective default hotkey", error, "settings");
      }
    };
    loadEffectiveDefaultHotkey();
  }, []);

  useEffect(() => {
    const cleanup = window.electronAPI?.onLinuxPttPermissionDenied?.(() => {
      setLinuxPttAvailable(false);
      toast({
        title: t("settingsPage.general.hotkey.linuxPttPermissionTitle"),
        description: t("settingsPage.general.hotkey.linuxPttPermissionDescription"),
        variant: "destructive",
        duration: 15000,
      });
      setActivationMode("tap");
    });
    return () => cleanup?.();
  }, [toast, t, setActivationMode]);

  const resetAccessibilityPermissions = () => {
    const message = t("settingsPage.permissions.resetAccessibility.description");

    showConfirmDialog({
      title: t("settingsPage.permissions.resetAccessibility.title"),
      description: message,
      onConfirm: () => {
        permissionsHook.requestAccessibilityPermission();
      },
    });
  };

  const renderSectionContent = () => {
    switch (activeSection) {
      case "general":
        return (
          <div className="space-y-6">
            {/* Appearance */}
            <div>
              <SectionHeader
                title={t("settingsPage.general.appearance.title")}
                description={t("settingsPage.general.appearance.description")}
              />
              <SettingsPanel>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.general.appearance.theme")}
                    description={t("settingsPage.general.appearance.themeDescription")}
                  >
                    <div className="inline-flex items-center gap-px p-0.5 bg-muted/60 dark:bg-surface-2 rounded-md">
                      {(
                        [
                          {
                            value: "light",
                            icon: Sun,
                            label: t("settingsPage.general.appearance.light"),
                          },
                          {
                            value: "dark",
                            icon: Moon,
                            label: t("settingsPage.general.appearance.dark"),
                          },
                          {
                            value: "auto",
                            icon: Monitor,
                            label: t("settingsPage.general.appearance.auto"),
                          },
                        ] as const
                      ).map((option) => {
                        const Icon = option.icon;
                        const isSelected = theme === option.value;
                        return (
                          <button
                            key={option.value}
                            onClick={() => setTheme(option.value)}
                            className={`
                              flex items-center gap-1 px-2.5 py-1 rounded-[5px] text-xs font-medium
                              transition-colors duration-100
                              ${
                                isSelected
                                  ? "bg-background dark:bg-surface-raised text-foreground shadow-sm"
                                  : "text-muted-foreground hover:text-foreground"
                              }
                            `}
                          >
                            <Icon className={`w-3 h-3 ${isSelected ? "text-primary" : ""}`} />
                            {option.label}
                          </button>
                        );
                      })}
                    </div>
                  </SettingsRow>
                </SettingsPanelRow>
                {platform === "darwin" && (
                  <SettingsPanelRow>
                    <SettingsRow
                      label={t("settingsPage.general.appearance.showMenuBarIcon")}
                      description={t("settingsPage.general.appearance.showMenuBarIconDescription")}
                    >
                      <Toggle checked={showMenuBarIcon} onChange={setShowMenuBarIcon} />
                    </SettingsRow>
                  </SettingsPanelRow>
                )}
              </SettingsPanel>
            </div>

            {/* Sound Effects */}
            <div>
              <SectionHeader title={t("settingsPage.general.soundEffects.title")} />
              <SettingsPanel>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.general.soundEffects.dictationSounds")}
                    description={t("settingsPage.general.soundEffects.dictationSoundsDescription")}
                  >
                    <Toggle checked={audioCuesEnabled} onChange={setAudioCuesEnabled} />
                  </SettingsRow>
                </SettingsPanelRow>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.general.soundEffects.pauseMedia")}
                    description={t("settingsPage.general.soundEffects.pauseMediaDescription")}
                  >
                    <Toggle checked={pauseMediaOnDictation} onChange={setPauseMediaOnDictation} />
                  </SettingsRow>
                </SettingsPanelRow>
              </SettingsPanel>
            </div>

            {/* Notifications */}

            {/* Clipboard */}
            <div>
              <SectionHeader title={t("settingsPage.general.clipboard.title")} />
              <SettingsPanel>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.general.clipboard.autoPaste")}
                    description={t("settingsPage.general.clipboard.autoPasteDescription")}
                  >
                    <Toggle checked={autoPasteEnabled} onChange={setAutoPasteEnabled} />
                  </SettingsRow>
                </SettingsPanelRow>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.general.clipboard.keepInClipboard")}
                    description={t("settingsPage.general.clipboard.keepInClipboardDescription")}
                  >
                    <Toggle
                      checked={keepTranscriptionInClipboard}
                      onChange={setKeepTranscriptionInClipboard}
                    />
                  </SettingsRow>
                </SettingsPanelRow>
              </SettingsPanel>
            </div>

            {/* Save Notes as Files */}

            {/* Import from Granola */}

            {/* Floating Icon */}
            <div>
              <SectionHeader
                title={t("settingsPage.general.floatingIcon.title")}
                description={t("settingsPage.general.floatingIcon.description")}
              />
              <SettingsPanel>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.general.floatingIcon.autoHide")}
                    description={t("settingsPage.general.floatingIcon.autoHideDescription")}
                  >
                    <Toggle checked={floatingIconAutoHide} onChange={setFloatingIconAutoHide} />
                  </SettingsRow>
                </SettingsPanelRow>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.general.floatingIcon.startPosition")}
                    description={t("settingsPage.general.floatingIcon.startPositionDescription")}
                  >
                    <select
                      value={panelStartPosition}
                      onChange={(e) =>
                        setPanelStartPosition(
                          e.target.value as "bottom-right" | "center" | "bottom-left"
                        )
                      }
                      className="h-7 rounded border border-border/70 bg-surface-1/80 px-2.5 text-xs font-medium text-foreground shadow-sm backdrop-blur-sm hover:border-border-hover hover:bg-surface-2/70 focus:outline-none focus:ring-2 focus:ring-ring/30 focus:ring-offset-1 transition-colors duration-200"
                    >
                      <option value="bottom-right">
                        {t("settingsPage.general.floatingIcon.bottomRight")}
                      </option>
                      <option value="center">
                        {t("settingsPage.general.floatingIcon.center")}
                      </option>
                      <option value="bottom-left">
                        {t("settingsPage.general.floatingIcon.bottomLeft")}
                      </option>
                    </select>
                  </SettingsRow>
                </SettingsPanelRow>
              </SettingsPanel>
            </div>

            {/* Language */}
            <div>
              <SectionHeader
                title={t("settings.language.sectionTitle")}
                description={t("settings.language.sectionDescription")}
              />
              <SettingsPanel>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settings.language.uiLabel")}
                    description={t("settings.language.uiDescription")}
                  >
                    <LanguageSelector
                      value={uiLanguage}
                      onChange={setUiLanguage}
                      options={UI_LANGUAGE_OPTIONS}
                      className="min-w-32"
                    />
                  </SettingsRow>
                </SettingsPanelRow>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settings.language.transcriptionLabel")}
                    description={t("settings.language.transcriptionDescription")}
                  >
                    <LanguageSelector
                      value={preferredLanguage}
                      onChange={(value) =>
                        updateTranscriptionSettings({ preferredLanguage: value })
                      }
                    />
                  </SettingsRow>
                </SettingsPanelRow>
                {preferredLanguage === "auto" && (
                  <SettingsPanelRow>
                    <SettingsRow
                      label={t("settings.language.chineseScriptLabel")}
                      description={t("settings.language.chineseScriptDescription")}
                    >
                      <Select
                        value={chineseScriptPreference}
                        onValueChange={(value: ChineseScriptPreference) =>
                          updateTranscriptionSettings({ chineseScriptPreference: value })
                        }
                      >
                        <SelectTrigger className="h-7 w-44 text-xs rounded-lg px-2.5 [&>svg]:h-3 [&>svg]:w-3">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="as-transcribed">
                            {t("settings.language.chineseScriptAsTranscribed")}
                          </SelectItem>
                          <SelectItem value="simplified">
                            {t("settings.language.chineseScriptSimplified")}
                          </SelectItem>
                          <SelectItem value="traditional">
                            {t("settings.language.chineseScriptTraditional")}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </SettingsRow>
                  </SettingsPanelRow>
                )}
              </SettingsPanel>
            </div>

            {/* Startup */}
            <div>
              <SectionHeader
                title={t("settingsPage.general.startup.title")}
                description={t("settingsPage.general.startup.description")}
              />
              <SettingsPanel>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.general.startup.launchAtLogin")}
                    description={t("settingsPage.general.startup.launchAtLoginDescription")}
                  >
                    <Toggle
                      checked={autoStartEnabled}
                      onChange={(checked: boolean) => handleAutoStartChange(checked)}
                      disabled={autoStartLoading}
                    />
                  </SettingsRow>
                </SettingsPanelRow>
                {autoStartNeedsApproval && (
                  <SettingsPanelRow>
                    <Alert
                      variant="warning"
                      className="dark:bg-amber-950/50 dark:border-amber-800 dark:text-amber-200 dark:[&>svg]:text-amber-400"
                    >
                      <AlertTriangle className="h-4 w-4" />
                      <AlertTitle>
                        {t("settingsPage.general.startup.needsApproval.title")}
                      </AlertTitle>
                      <AlertDescription className="space-y-2">
                        <p>{t("settingsPage.general.startup.needsApproval.description")}</p>
                        <Button
                          onClick={() => void window.electronAPI?.openLoginItemsSettings?.()}
                          variant="outline"
                          size="sm"
                        >
                          {t("settingsPage.general.startup.needsApproval.action")}
                        </Button>
                      </AlertDescription>
                    </Alert>
                  </SettingsPanelRow>
                )}
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.general.startup.startMinimized")}
                    description={t("settingsPage.general.startup.startMinimizedDescription")}
                  >
                    <Toggle checked={startMinimized} onChange={setStartMinimized} />
                  </SettingsRow>
                </SettingsPanelRow>
              </SettingsPanel>
            </div>

            {/* Microphone */}
            <div>
              <SectionHeader
                title={t("settingsPage.general.microphone.title")}
                description={t("settingsPage.general.microphone.description")}
              />
              <SettingsPanel>
                <SettingsPanelRow>
                  <MicrophoneSettings
                    microphoneSelectionMode={microphoneSelectionMode}
                    selectedMicDeviceId={selectedMicDeviceId}
                    selectedMicDeviceLabel={selectedMicDeviceLabel}
                    micWarmHoldSeconds={micWarmHoldSeconds}
                    onSelectionModeChange={setMicrophoneSelectionMode}
                    onDeviceSelect={setSelectedMicDevice}
                    onMicWarmHoldSecondsChange={setMicWarmHoldSeconds}
                  />
                </SettingsPanelRow>
              </SettingsPanel>
            </div>

            {/* Dictionary */}
            <div>
              <SectionHeader
                title={t("settingsPage.dictionary.autoLearnTitle", {
                  defaultValue: "Auto-learn from corrections",
                })}
              />
              <SettingsPanel>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.dictionary.autoLearnTitle", {
                      defaultValue: "Auto-learn from corrections",
                    })}
                    description={t("settingsPage.dictionary.autoLearnDescription", {
                      defaultValue:
                        "When you correct a transcription in the target app, the corrected word is automatically added to your dictionary.",
                    })}
                  >
                    <Toggle checked={autoLearnCorrections} onChange={setAutoLearnCorrections} />
                  </SettingsRow>
                </SettingsPanelRow>
              </SettingsPanel>
            </div>

            {/* Wayland Paste Diagnostics — only on Linux + Wayland */}
            {ydotoolStatus?.isLinux && ydotoolStatus?.isWayland && (
              <div>
                <SectionHeader
                  title={t("settingsPage.general.waylandPaste.title", {
                    defaultValue: "Wayland Paste Setup",
                  })}
                  description={t("settingsPage.general.waylandPaste.description", {
                    defaultValue:
                      "Auto-paste on Wayland uses ydotool or wtype. wtype is preferred on wlroots compositors.",
                  })}
                />
                {(() => {
                  if (ydotoolStatus.isNixOS) {
                    return (
                      <NixOsPasteInfo status={ydotoolStatus} onRecheck={refreshYdotoolStatus} />
                    );
                  }
                  const checks = [
                    {
                      key: "hasWtype",
                      label: "wtype",
                      ok: ydotoolStatus.hasWtype,
                      required: ydotoolStatus.isWlroots,
                      desc: t("settingsPage.general.waylandPaste.wtypeDesc"),
                      steps: [
                        {
                          title: t("settingsPage.general.waylandPaste.guide.wtype.step1Title"),
                          desc: t("settingsPage.general.waylandPaste.guide.wtype.step1Desc"),
                          cmds: getLinuxPasteInstallCommands(t, "wtype"),
                        },
                        {
                          title: t("settingsPage.general.waylandPaste.guide.wtype.step2Title"),
                          cmds: [{ cmd: "which wtype" }],
                        },
                      ],
                    },
                    {
                      key: "hasYdotool",
                      label: "ydotool",
                      ok: ydotoolStatus.hasYdotool,
                      required: !ydotoolStatus.isWlroots,
                      desc: t("settingsPage.general.waylandPaste.ydotoolDesc", {
                        defaultValue: "Input automation tool for Wayland",
                      }),
                      steps: [
                        {
                          title: t("settingsPage.general.waylandPaste.guide.ydotool.step1Title", {
                            defaultValue: "Install ydotool",
                          }),
                          desc: t("settingsPage.general.waylandPaste.guide.ydotool.step1Desc", {
                            defaultValue:
                              "Use your distribution's package manager to install ydotool.",
                          }),
                          cmds: getLinuxPasteInstallCommands(t, "ydotool"),
                        },
                        {
                          title: t("settingsPage.general.waylandPaste.guide.ydotool.step2Title", {
                            defaultValue: "Verify installation",
                          }),
                          desc: t("settingsPage.general.waylandPaste.guide.ydotool.step2Desc", {
                            defaultValue: "Check that ydotool is available in your PATH.",
                          }),
                          cmds: [{ cmd: "which ydotool" }],
                        },
                      ],
                    },
                    {
                      key: "hasYdotoold",
                      label: "ydotoold",
                      ok: ydotoolStatus.hasYdotoold,
                      required: !ydotoolStatus.isWlroots,
                      desc: t("settingsPage.general.waylandPaste.ydotooldDesc", {
                        defaultValue: "Daemon for ydotool (separate package on Ubuntu/Pop!_OS)",
                      }),
                      steps: [
                        {
                          title: t("settingsPage.general.waylandPaste.guide.ydotoold.step1Title", {
                            defaultValue: "Install ydotoold",
                          }),
                          desc: t("settingsPage.general.waylandPaste.guide.ydotoold.step1Desc", {
                            defaultValue:
                              "On Ubuntu and Pop!_OS, ydotoold is a separate package. On Fedora, it's included with ydotool.",
                          }),
                          cmds: [
                            {
                              label: "Ubuntu / Pop!_OS / Debian",
                              cmd: "sudo apt install ydotoold",
                            },
                            { label: "Fedora", cmd: "# Already included in the ydotool package" },
                            { label: "Arch Linux", cmd: "# Included in the ydotool package" },
                          ],
                        },
                      ],
                    },
                    {
                      key: "hasUinput",
                      label: "/dev/uinput",
                      ok: ydotoolStatus.hasUinput,
                      required: !ydotoolStatus.isWlroots,
                      desc: t("settingsPage.general.waylandPaste.uinputDesc", {
                        defaultValue: "Kernel input device access",
                      }),
                      note: !ydotoolStatus.hasUinput
                        ? ydotoolStatus.hasUdevRule
                          ? t("settingsPage.general.waylandPaste.uinputRuleFound", {
                              defaultValue: "Rule present but not active. A reboot should fix it.",
                            })
                          : t("settingsPage.general.waylandPaste.uinputRuleMissing", {
                              defaultValue: "no udev rule found",
                            })
                        : undefined,
                      steps:
                        ydotoolStatus.hasUdevRule && !ydotoolStatus.hasUinput
                          ? [
                              {
                                title: t(
                                  "settingsPage.general.waylandPaste.guide.uinput.ruleFoundTitle",
                                  {
                                    defaultValue: "udev rule already configured",
                                  }
                                ),
                                desc: t(
                                  "settingsPage.general.waylandPaste.guide.uinput.ruleFoundDesc",
                                  {
                                    defaultValue:
                                      "The udev rule for /dev/uinput is already on your system but hasn't taken effect. Try reloading:",
                                  }
                                ),
                                cmds: [
                                  {
                                    cmd: "sudo udevadm control --reload-rules && sudo udevadm trigger /dev/uinput",
                                  },
                                ],
                              },
                              {
                                title: t(
                                  "settingsPage.general.waylandPaste.guide.uinput.rebootTitle",
                                  {
                                    defaultValue: "If reloading didn't help, reboot",
                                  }
                                ),
                                desc: t(
                                  "settingsPage.general.waylandPaste.guide.uinput.rebootDesc",
                                  {
                                    defaultValue:
                                      "On some distros, udev changes only apply after a full reboot. Restart your computer and come back to re-check.",
                                  }
                                ),
                              },
                            ]
                          : [
                              {
                                title: t(
                                  "settingsPage.general.waylandPaste.guide.uinput.step1Title",
                                  {
                                    defaultValue: "Create a udev rule",
                                  }
                                ),
                                desc: t(
                                  "settingsPage.general.waylandPaste.guide.uinput.step1Desc",
                                  {
                                    defaultValue:
                                      "This rule grants access to /dev/uinput for users in the input group.",
                                  }
                                ),
                                cmds: [
                                  {
                                    cmd: 'echo \'KERNEL=="uinput", GROUP="input", MODE="0660", TAG+="uaccess"\' | sudo tee /etc/udev/rules.d/70-uinput.rules',
                                  },
                                ],
                              },
                              {
                                title: t(
                                  "settingsPage.general.waylandPaste.guide.uinput.step2Title",
                                  {
                                    defaultValue: "Reload udev rules",
                                  }
                                ),
                                desc: t(
                                  "settingsPage.general.waylandPaste.guide.uinput.step2Desc",
                                  {
                                    defaultValue: "Apply the new rule without rebooting.",
                                  }
                                ),
                                cmds: [
                                  {
                                    cmd: "sudo udevadm control --reload-rules && sudo udevadm trigger /dev/uinput",
                                  },
                                ],
                              },
                            ],
                    },
                    {
                      key: "hasGroup",
                      label: t("settingsPage.general.waylandPaste.inputGroup", {
                        defaultValue: "input group",
                      }),
                      ok: ydotoolStatus.hasGroup,
                      required: !ydotoolStatus.isWlroots,
                      desc: t("settingsPage.general.waylandPaste.inputGroupDesc", {
                        defaultValue: "User must be in the input group (requires re-login)",
                      }),
                      steps: [
                        {
                          title: t("settingsPage.general.waylandPaste.guide.group.step1Title", {
                            defaultValue: "Add your user to the input group",
                          }),
                          cmds: [{ cmd: "sudo usermod -aG input $USER" }],
                        },
                        {
                          title: t("settingsPage.general.waylandPaste.guide.group.step2Title", {
                            defaultValue: "Log out and back in",
                          }),
                          desc: t("settingsPage.general.waylandPaste.guide.group.step2Desc", {
                            defaultValue:
                              "Group changes only take effect after a new login session. Log out of your desktop and log back in, then reopen OpenWhispr.",
                          }),
                        },
                      ],
                    },
                    {
                      key: "hasService",
                      label: t("settingsPage.general.waylandPaste.service", {
                        defaultValue: "systemd service",
                      }),
                      ok: ydotoolStatus.hasService,
                      required: !ydotoolStatus.isWlroots,
                      desc: t("settingsPage.general.waylandPaste.serviceDesc", {
                        defaultValue: "User service file for auto-starting ydotoold",
                      }),
                      steps: [
                        {
                          title: t("settingsPage.general.waylandPaste.guide.service.step1Title", {
                            defaultValue: "Create the service directory",
                          }),
                          cmds: [{ cmd: "mkdir -p ~/.config/systemd/user" }],
                        },
                        {
                          title: t("settingsPage.general.waylandPaste.guide.service.step2Title", {
                            defaultValue: "Create the service file",
                          }),
                          desc: t("settingsPage.general.waylandPaste.guide.service.step2Desc", {
                            defaultValue:
                              "This creates a user-level systemd service that starts ydotoold automatically when you log in.",
                          }),
                          cmds: [
                            {
                              cmd: `cat > ~/.config/systemd/user/ydotoold.service << 'EOF'
[Unit]
Description=ydotoold - ydotool daemon
After=graphical-session.target
PartOf=graphical-session.target

[Service]
ExecStart=/usr/bin/ydotoold
Restart=on-failure
RestartSec=1s

[Install]
WantedBy=graphical-session.target
EOF`,
                            },
                          ],
                        },
                        {
                          title: t("settingsPage.general.waylandPaste.guide.service.step3Title", {
                            defaultValue: "Reload and enable",
                          }),
                          cmds: [
                            {
                              cmd: "systemctl --user daemon-reload && systemctl --user enable ydotoold",
                            },
                          ],
                        },
                      ],
                    },
                    {
                      key: "daemonRunning",
                      label: t("settingsPage.general.waylandPaste.daemon", {
                        defaultValue: "ydotoold daemon",
                      }),
                      ok: ydotoolStatus.daemonRunning,
                      required: !ydotoolStatus.isWlroots,
                      desc: t("settingsPage.general.waylandPaste.daemonDesc", {
                        defaultValue: "Background service must be running",
                      }),
                      steps: [
                        {
                          title: t("settingsPage.general.waylandPaste.guide.daemon.step1Title", {
                            defaultValue: "Start the daemon",
                          }),
                          desc: t("settingsPage.general.waylandPaste.guide.daemon.step1Desc", {
                            defaultValue: "Start ydotoold and enable it so it runs on every login.",
                          }),
                          cmds: [
                            {
                              cmd: "systemctl --user enable ydotoold && systemctl --user start ydotoold",
                            },
                            {
                              label: "Arch Linux (service is named ydotool.service)",
                              cmd: "systemctl --user enable --now ydotool.service",
                            },
                          ],
                        },
                        {
                          title: t("settingsPage.general.waylandPaste.guide.daemon.step2Title", {
                            defaultValue: "Verify it's running",
                          }),
                          cmds: [
                            { cmd: "systemctl --user status ydotoold" },
                            {
                              label: "Arch Linux",
                              cmd: "systemctl --user status ydotool.service",
                            },
                          ],
                        },
                      ],
                    },
                  ];

                  if (ydotoolStatus.isKde) {
                    checks.push({
                      key: "hasXclip",
                      label: "xclip",
                      ok: ydotoolStatus.hasXclip || ydotoolStatus.hasXsel || false,
                      required: true,
                      desc: t("settingsPage.general.waylandPaste.xclipDesc", {
                        defaultValue: "Clipboard tool for KDE Wayland paste (xclip or xsel)",
                      }),
                      steps: [
                        {
                          title: t("settingsPage.general.waylandPaste.guide.xclip.step1Title", {
                            defaultValue: "Install xclip",
                          }),
                          cmds: [
                            { cmd: "sudo dnf install xclip  # Fedora" },
                            { cmd: "sudo apt install xclip  # Debian/Ubuntu" },
                          ],
                        },
                      ],
                    });
                  }

                  const allOk = checks.filter((c) => c.required).every((c) => c.ok);
                  const activeGuide = checks.find((c) => c.key === ydotoolGuideKey);

                  return (
                    <>
                      {allOk ? (
                        <SettingsPanel>
                          <SettingsPanelRow>
                            <div className="flex items-center justify-between">
                              <div className="flex items-center gap-2">
                                <CircleCheck className="h-4 w-4 text-emerald-500" />
                                <span className="text-sm">
                                  {t("settingsPage.general.waylandPaste.allGoodDesc", {
                                    defaultValue: "Auto-paste is ready to go.",
                                  })}
                                </span>
                              </div>
                              <button
                                onClick={refreshYdotoolStatus}
                                className="shrink-0 text-muted-foreground hover:text-foreground transition-colors"
                              >
                                <RotateCw className="w-3.5 h-3.5" />
                              </button>
                            </div>
                          </SettingsPanelRow>
                        </SettingsPanel>
                      ) : (
                        <>
                          <div className="rounded-xl border border-border overflow-hidden">
                            <div className="divide-y divide-border">
                              {checks.map((item) => (
                                <div key={item.key} className="px-4 py-3">
                                  <div className="flex items-center gap-2.5">
                                    {item.ok ? (
                                      <CircleCheck className="h-4 w-4 shrink-0 text-emerald-500" />
                                    ) : (
                                      <CircleX className="h-4 w-4 shrink-0 text-red-500" />
                                    )}
                                    <div className="flex-1 min-w-0">
                                      <span className="text-sm font-medium">{item.label}</span>
                                      <span className="text-xs text-muted-foreground ms-2">
                                        {item.desc}
                                      </span>
                                      {item.note && (
                                        <p className="text-[11px] text-amber-600 dark:text-amber-400 mt-0.5">
                                          {item.note}
                                        </p>
                                      )}
                                    </div>
                                    {!item.ok && (
                                      <button
                                        onClick={() => setYdotoolGuideKey(item.key)}
                                        className="shrink-0 flex items-center gap-1 text-xs px-2.5 py-1 rounded-md border border-border hover:bg-muted transition-colors text-foreground"
                                      >
                                        <BookOpen className="w-3 h-3" />
                                        {t("settingsPage.general.waylandPaste.guide.open", {
                                          defaultValue: "Guide",
                                        })}
                                      </button>
                                    )}
                                  </div>
                                </div>
                              ))}
                            </div>
                          </div>
                          <button
                            onClick={refreshYdotoolStatus}
                            className="flex items-center gap-1.5 mt-3 text-xs text-muted-foreground hover:text-foreground transition-colors"
                          >
                            <RotateCw className="w-3 h-3" />
                            {t("settingsPage.general.waylandPaste.recheck", {
                              defaultValue: "Re-check",
                            })}
                          </button>
                        </>
                      )}

                      {/* Step-by-step guide dialog */}
                      <Dialog
                        open={!!activeGuide}
                        onOpenChange={(open) => !open && setYdotoolGuideKey(null)}
                      >
                        <DialogContent className="sm:max-w-lg max-h-[80vh] overflow-y-auto">
                          {activeGuide && (
                            <>
                              <DialogHeader>
                                <DialogTitle className="flex items-center gap-2">
                                  <BookOpen className="w-4 h-4" />
                                  {activeGuide.label}
                                </DialogTitle>
                                <DialogDescription>{activeGuide.desc}</DialogDescription>
                              </DialogHeader>
                              <div className="space-y-5 mt-2">
                                {activeGuide.steps.map((step, i) => (
                                  <div key={i}>
                                    <div className="flex items-start gap-3">
                                      <span className="shrink-0 w-6 h-6 rounded-full bg-primary text-primary-foreground flex items-center justify-center text-xs font-semibold">
                                        {i + 1}
                                      </span>
                                      <div className="flex-1 min-w-0">
                                        <p className="text-sm font-medium">{step.title}</p>
                                        {step.desc && (
                                          <p className="text-xs text-muted-foreground mt-0.5">
                                            {step.desc}
                                          </p>
                                        )}
                                        {step.cmds && step.cmds.length > 0 && (
                                          <div className="mt-2 space-y-2">
                                            {step.cmds.map((c, j) => (
                                              <div key={j}>
                                                {c.label && (
                                                  <p className="text-[11px] text-muted-foreground mb-1">
                                                    {c.label}
                                                  </p>
                                                )}
                                                <div className="flex items-start gap-1.5">
                                                  <pre
                                                    dir="ltr"
                                                    className="flex-1 text-[11px] bg-muted/60 rounded-md px-3 py-2 font-mono whitespace-pre-wrap break-all select-all overflow-x-auto"
                                                  >
                                                    {c.cmd}
                                                  </pre>
                                                  <button
                                                    onClick={() =>
                                                      navigator.clipboard.writeText(c.cmd)
                                                    }
                                                    className="shrink-0 p-1.5 rounded-md hover:bg-muted transition-colors text-muted-foreground hover:text-foreground"
                                                    title={t(
                                                      "settingsPage.general.waylandPaste.copy",
                                                      { defaultValue: "Copy" }
                                                    )}
                                                  >
                                                    <Copy className="w-3.5 h-3.5" />
                                                  </button>
                                                </div>
                                              </div>
                                            ))}
                                          </div>
                                        )}
                                      </div>
                                    </div>
                                  </div>
                                ))}
                              </div>
                            </>
                          )}
                        </DialogContent>
                      </Dialog>
                    </>
                  );
                })()}
              </div>
            )}
          </div>
        );

      case "hotkeys":
        return (
          <div className="space-y-6">
            {isUsingHyprland && hyprlandConfigStatus && !hyprlandConfigStatus.canWrite && (
              <Alert>
                <Info className="h-4 w-4" />
                <AlertTitle>
                  {t("settingsPage.general.hotkey.hyprlandConfigWriteWarningTitle")}
                </AlertTitle>
                <AlertDescription>
                  <BidiInterpolatedText
                    text={t("settingsPage.general.hotkey.hyprlandConfigWriteWarningDescription", {
                      path: BIDI_VALUE_TOKEN,
                    })}
                    value={hyprlandConfigStatus.path}
                  />
                </AlertDescription>
              </Alert>
            )}
            {/* Dictation Hotkey */}
            <div>
              <SectionHeader
                title={t("settingsPage.general.hotkey.title")}
                description={t("settingsPage.general.hotkey.description")}
                note={isUsingHyprland && t("settingsPage.general.hotkey.hyprlandUnbindDescription")}
              />
              <SettingsPanel>
                <SettingsPanelRow>
                  <HotkeyListInput
                    value={dictationKey}
                    onChange={(list) => registerHotkey(list)}
                    validate={validateDictationHotkey}
                    disabled={isHotkeyRegistering}
                    maxHotkeys={isUsingNativeShortcut ? 1 : undefined}
                    required
                    footerEnd={
                      effectiveDefaultHotkey &&
                      dictationKey &&
                      dictationKey !== effectiveDefaultHotkey ? (
                        <button
                          onClick={() => registerHotkey(effectiveDefaultHotkey)}
                          disabled={isHotkeyRegistering}
                          className="text-xs text-muted-foreground/70 hover:text-foreground transition-colors disabled:opacity-50"
                        >
                          <BidiInterpolatedText
                            text={t("settingsPage.general.hotkey.resetToDefault", {
                              hotkey: BIDI_VALUE_TOKEN,
                            })}
                            value={formatHotkeyLabel(effectiveDefaultHotkey)}
                          />
                        </button>
                      ) : null
                    }
                  />
                </SettingsPanelRow>

                {(!isUsingNativeShortcut || getCachedPlatform() === "linux") && (
                  <SettingsPanelRow>
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-xs text-muted-foreground/80">
                        {t("settingsPage.general.hotkey.activationMode")}
                      </span>
                      <ActivationModeSelector
                        value={activationMode}
                        onChange={setActivationMode}
                        pushDisabledReason={
                          !supportsPushToTalk
                            ? pushToTalkUnavailableReason || t("windows.pttUnavailable")
                            : undefined
                        }
                      />
                    </div>
                    {getCachedPlatform() === "linux" && activationMode === "push" && (
                      <LinuxPttSetupInfo isAvailable={linuxPttAvailable} />
                    )}
                  </SettingsPanelRow>
                )}
              </SettingsPanel>
            </div>

            {/* Voice Agent Hotkey */}

            {/* Translation Hotkey */}

            {/* Meeting Mode Hotkey */}
          </div>
        );

      case "speechToText":
      case "llms":
        return null;

      case "privacyData":
        return (
          <div className="space-y-6">
            {/* Privacy */}

            {/* Audio Retention */}
            <div className="border-t border-border/70 pt-6">
              <SectionHeader
                title={t("settingsPage.privacy.audioRetention")}
                description={t("settingsPage.privacy.audioRetentionDescription")}
              />

              <SettingsPanel>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.privacy.audioRetention")}
                    description={t("settingsPage.privacy.audioRetentionDescription")}
                  >
                    <select
                      value={enforcedAudioRetentionDays}
                      onChange={(e) => {
                        const days = parseInt(e.target.value, 10);
                        if (audioRetentionCap !== null && days > audioRetentionCap) return;
                        setAudioRetentionDays(days);
                      }}
                      className={RETENTION_SELECT_CLASS}
                    >
                      <option value={0}>{t("settingsPage.privacy.audioRetentionDisabled")}</option>
                      {enforcedAudioRetentionDays > 0 &&
                        !RETENTION_DAY_OPTIONS.includes(enforcedAudioRetentionDays) && (
                          <option value={enforcedAudioRetentionDays}>
                            {t("settingsPage.privacy.retentionDays", {
                              count: enforcedAudioRetentionDays,
                            })}
                          </option>
                        )}
                      {RETENTION_DAY_OPTIONS.map((days) => (
                        <option
                          key={days}
                          value={days}
                          disabled={audioRetentionCap !== null && days > audioRetentionCap}
                        >
                          {t("settingsPage.privacy.retentionDays", { count: days })}
                        </option>
                      ))}
                    </select>
                  </SettingsRow>
                </SettingsPanelRow>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.privacy.audioStorageUsage")}
                    description={
                      audioStorageUsage.fileCount > 0
                        ? t("settingsPage.privacy.audioStorageFiles", {
                            count: audioStorageUsage.fileCount,
                            size: formatBytes(audioStorageUsage.totalBytes),
                          })
                        : t("settingsPage.privacy.audioStorageEmpty")
                    }
                  >
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-7 text-xs"
                      disabled={audioStorageUsage.fileCount === 0}
                      onClick={handleClearAllAudio}
                    >
                      {t("settingsPage.privacy.clearAllAudio")}
                    </Button>
                  </SettingsRow>
                </SettingsPanelRow>
              </SettingsPanel>
            </div>

            {/* Data Retention */}
            <div className="border-t border-border/70 pt-6">
              <SettingsPanel>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.privacy.dataRetention")}
                    description={
                      historyLockedByPolicy
                        ? t("common.managedByOrg")
                        : t("settingsPage.privacy.dataRetentionDescription")
                    }
                  >
                    <Toggle
                      checked={effectiveDataRetentionEnabled}
                      disabled={historyLockedByPolicy}
                      onChange={setDataRetentionEnabled}
                    />
                  </SettingsRow>
                </SettingsPanelRow>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.privacy.transcriptRetention")}
                    description={t("settingsPage.privacy.transcriptRetentionDescription")}
                  >
                    <select
                      value={transcriptRetentionDays}
                      disabled={!effectiveDataRetentionEnabled}
                      onChange={(e) => setTranscriptRetentionDays(parseInt(e.target.value, 10))}
                      className={RETENTION_SELECT_CLASS}
                    >
                      <option value={0}>
                        {t("settingsPage.privacy.transcriptRetentionForever")}
                      </option>
                      {RETENTION_DAY_OPTIONS.map((days) => (
                        <option key={days} value={days}>
                          {t("settingsPage.privacy.retentionDays", { count: days })}
                        </option>
                      ))}
                    </select>
                  </SettingsRow>
                </SettingsPanelRow>
                <SettingsPanelRow>
                  <SettingsRow
                    label={t("settingsPage.privacy.saveDiscarded")}
                    description={t("settingsPage.privacy.saveDiscardedDescription")}
                  >
                    <Toggle
                      checked={saveDiscardedTranscriptions}
                      disabled={!effectiveDataRetentionEnabled || enforcedAudioRetentionDays === 0}
                      onChange={setSaveDiscardedTranscriptions}
                    />
                  </SettingsRow>
                </SettingsPanelRow>
              </SettingsPanel>
            </div>

            {/* Permissions */}
            <div className="border-t border-border/70 pt-6">
              <SectionHeader
                title={t("settingsPage.permissions.title")}
                description={t("settingsPage.permissions.description")}
              />

              <div className="space-y-3">
                <PermissionCard
                  icon={Mic}
                  title={t("settingsPage.permissions.microphoneTitle")}
                  description={t("settingsPage.permissions.microphoneDescription")}
                  granted={permissionsHook.micPermissionGranted}
                  onRequest={permissionsHook.requestMicPermission}
                  buttonText={t("settingsPage.permissions.grantAccess")}
                />

                {platform === "darwin" && (
                  <>
                    {platform === "darwin" && (
                      <PermissionCard
                        icon={Shield}
                        title={t("settingsPage.permissions.accessibilityTitle")}
                        description={t("settingsPage.permissions.accessibilityDescription")}
                        granted={permissionsHook.accessibilityPermissionGranted}
                        onRequest={permissionsHook.requestAccessibilityPermission}
                        buttonText={t("settingsPage.permissions.grantAccess")}
                      />
                    )}
                  </>
                )}
              </div>

              {!permissionsHook.micPermissionGranted && permissionsHook.micPermissionError && (
                <MicPermissionWarning
                  error={permissionsHook.micPermissionError}
                  onOpenSoundSettings={permissionsHook.openSoundInputSettings}
                  onOpenPrivacySettings={permissionsHook.openMicPrivacySettings}
                />
              )}

              {platform === "linux" &&
                permissionsHook.pasteToolsInfo &&
                needsLinuxPasteToolGuidance(permissionsHook.pasteToolsInfo) && (
                  <PasteToolsInfo
                    pasteToolsInfo={permissionsHook.pasteToolsInfo}
                    isChecking={permissionsHook.isCheckingPasteTools}
                    onCheck={permissionsHook.checkPasteToolsAvailability}
                  />
                )}

              {platform === "darwin" && (
                <div className="mt-5">
                  <p className="text-xs font-medium text-foreground mb-3">
                    {t("settingsPage.permissions.troubleshootingTitle")}
                  </p>
                  <SettingsPanel>
                    <SettingsPanelRow>
                      <SettingsRow
                        label={t("settingsPage.permissions.resetAccessibility.label")}
                        description={t(
                          "settingsPage.permissions.resetAccessibility.rowDescription"
                        )}
                      >
                        <Button
                          onClick={resetAccessibilityPermissions}
                          variant="ghost"
                          size="sm"
                          className="text-foreground/70 hover:text-foreground"
                        >
                          {t("settingsPage.permissions.troubleshoot")}
                        </Button>
                      </SettingsRow>
                    </SettingsPanelRow>
                  </SettingsPanel>
                </div>
              )}
            </div>
          </div>
        );

      default:
        return null;
    }
  };

  return (
    <>
      <ConfirmDialog
        open={confirmDialog.open}
        onOpenChange={(open) => !open && hideConfirmDialog()}
        title={confirmDialog.title}
        description={confirmDialog.description}
        onConfirm={confirmDialog.onConfirm}
        variant={confirmDialog.variant}
        confirmText={confirmDialog.confirmText}
        cancelText={confirmDialog.cancelText}
      />

      <AlertDialog
        open={alertDialog.open}
        onOpenChange={(open) => !open && hideAlertDialog()}
        title={alertDialog.title}
        description={alertDialog.description}
        onOk={() => {}}
      />

      {/* Mounted on first visit and kept alive so model-download progress and IPC listeners survive section switches. */}
      {hasMountedSpeechToText && (
        <TabPanel active={activeSection === "speechToText"}>
          <SpeechToTextTabs
            initialTab={
              activeSection === "speechToText"
                ? (initialSubTab as SpeechTab | undefined)
                : undefined
            }
            renderDictation={() => (
              <div className="space-y-6">
                <TranscriptionSection
                  isSignedIn={false}
                  startOnboarding={() => {}}
                  cloudTranscriptionMode={cloudTranscriptionMode}
                  setCloudTranscriptionMode={setCloudTranscriptionMode}
                  useLocalWhisper={useLocalWhisper}
                  setUseLocalWhisper={setUseLocalWhisper}
                  updateTranscriptionSettings={updateTranscriptionSettings}
                  cloudTranscriptionProvider={cloudTranscriptionProvider}
                  setCloudTranscriptionProvider={setCloudTranscriptionProvider}
                  cloudTranscriptionModel={cloudTranscriptionModel}
                  setCloudTranscriptionModel={setCloudTranscriptionModel}
                  localTranscriptionProvider={localTranscriptionProvider}
                  setLocalTranscriptionProvider={setLocalTranscriptionProvider}
                  whisperModel={whisperModel}
                  setWhisperModel={setWhisperModel}
                  parakeetModel={parakeetModel}
                  setParakeetModel={setParakeetModel}
                  cohereModel={cohereModel}
                  setCohereModel={setCohereModel}
                  cloudTranscriptionBaseUrl={cloudTranscriptionBaseUrl}
                  setCloudTranscriptionBaseUrl={setCloudTranscriptionBaseUrl}
                  transcriptionMode={transcriptionMode}
                  setTranscriptionMode={setTranscriptionMode}
                  remoteTranscriptionUrl={remoteTranscriptionUrl}
                  setRemoteTranscriptionUrl={setRemoteTranscriptionUrl}
                  remoteTranscriptionModel={remoteTranscriptionModel}
                  setRemoteTranscriptionModel={setRemoteTranscriptionModel}
                  showTranscriptionPreview={showTranscriptionPreview}
                  setShowTranscriptionPreview={setShowTranscriptionPreview}
                  toast={toast}
                />
              </div>
            )}
          />
        </TabPanel>
      )}
      {hasMountedLlms && (
        <TabPanel active={activeSection === "llms"}>
          <LlmsTabs
            initialTab={
              activeSection === "llms" ? (initialSubTab as LlmTab | undefined) : undefined
            }
            renderDictationCleanup={() => (
              <div className="space-y-6">
                <AiModelsSection
                  useCleanupModel={useCleanupModel}
                  setUseCleanupModel={(value) => {
                    updateCleanupSettings({ useCleanupModel: value });
                  }}
                  toast={toast}
                />
                <div className="border-t border-border/70 pt-6">
                  <SectionHeader title={t("settingsPage.prompts.title")} />
                  <PromptStudio />
                </div>
              </div>
            )}
          />
        </TabPanel>
      )}
      {renderSectionContent()}
    </>
  );
}
