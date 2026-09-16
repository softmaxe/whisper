import React, { useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Upload, FileAudio, X, AlertCircle, Settings } from "../icons";
import { Button } from "../ui/button";
import { cn } from "../lib/utils";
import { transcriptionErrorKey } from "./shared";
import { useSettingsStore } from "../../stores/settingsStore";
import { useBatchQueue } from "../../stores/batchQueueStore";
import { transcribeFile } from "../../services/fileTranscription";
import type { FileTranscriptionConfig } from "../../services/fileTranscription";
import BatchQueueView from "./BatchQueueView";
import { getBaseLanguageCode } from "../../utils/languageSupport";
import { saveUploadTranscription } from "../../services/uploadNotes";
import { UploadCompleteWarnings, UploadModelSettingsButton } from "./UploadAudioFeedback";
import { isSupportedUploadFile } from "../../utils/uploadAudioFormats";

type UploadState = "idle" | "selected" | "transcribing" | "complete" | "error";

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface UploadAudioViewProps {
  onOpenHistory?: () => void;
  onOpenSettings?: (section: string) => void;
  onCopyText: (text: string) => void;
}

export default function UploadAudioView({
  onOpenHistory,
  onOpenSettings,
  onCopyText,
}: UploadAudioViewProps) {
  const { t } = useTranslation();
  const [state, setState] = useState<UploadState>("idle");
  const [file, setFile] = useState<{
    name: string;
    path: string;
    size: string;
    sizeBytes: number;
  } | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [partialWarning, setPartialWarning] = useState<{ failed: number; total: number } | null>(
    null
  );
  const [transcriptionId, setTranscriptionId] = useState<number | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isDragOver, setIsDragOver] = useState(false);
  const [progress, setProgress] = useState(0);
  const progressRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const runIdRef = useRef(0);
  const activeRequestIdRef = useRef<string | null>(null);
  const [skippedNotice, setSkippedNotice] = useState<string | null>(null);
  const batch = useBatchQueue();
  const remoteTranscriptionUrl = useSettingsStore((s) => s.remoteTranscriptionUrl);
  const remoteTranscriptionModel = useSettingsStore((s) => s.remoteTranscriptionModel);
  const customTranscriptionApiKey = useSettingsStore((s) => s.customTranscriptionApiKey);
  const preferredLanguage = useSettingsStore((s) => s.preferredLanguage);
  const providerReady = !!remoteTranscriptionUrl.trim();

  useEffect(
    () => () => {
      if (progressRef.current) clearInterval(progressRef.current);
    },
    []
  );

  const getActiveModelLabel = (): string => {
    const name = t("settingsPage.transcription.modes.selfHosted");
    return remoteTranscriptionModel ? `${name} · ${remoteTranscriptionModel}` : name;
  };

  const buildTranscriptionConfig = (): FileTranscriptionConfig => ({
    useLocalWhisper: false,
    localTranscriptionProvider: "whisper",
    whisperModel: "",
    parakeetModel: "",
    cohereModel: "",
    isOpenWhisprCloud: false,
    getApiKey: () => customTranscriptionApiKey,
    cloudTranscriptionProvider: "custom",
    cloudTranscriptionBaseUrl: remoteTranscriptionUrl,
    cloudTranscriptionModel: remoteTranscriptionModel,
    language: getBaseLanguageCode(preferredLanguage) || "",
    transcriptionMode: "self-hosted",
    remoteTranscriptionUrl,
    remoteTranscriptionModel,
  });

  const handleBrowse = async () => {
    const res = await window.electronAPI.selectAudioFile({ multiple: true });
    if (res.canceled) return;

    const filePaths: string[] = res.filePaths || (res.filePath ? [res.filePath] : []);
    if (filePaths.length === 0) return;

    // While a batch runs (or a queue exists), new files join the queue.
    if (filePaths.length === 1 && !batch.isProcessing && !batch.hasQueue) {
      const fp = filePaths[0];
      const name = fp.split(/[/\\]/).pop() || "audio";
      const sizeBytes = (await window.electronAPI.getFileSize?.(fp)) ?? 0;
      setFile({ name, path: fp, size: sizeBytes ? formatFileSize(sizeBytes) : "", sizeBytes });
      setState("selected");
      setError(null);
      return;
    }

    const items: Array<{ name: string; path: string; sizeBytes: number }> = [];
    for (const fp of filePaths) {
      const name = fp.split(/[/\\]/).pop() || "audio";
      const sizeBytes = (await window.electronAPI.getFileSize?.(fp)) ?? 0;
      items.push({ name, path: fp, sizeBytes });
    }
    batch.addFiles(items);
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(false);
    const files = e.dataTransfer.files;
    if (!files || files.length === 0) return;

    const validFiles: Array<{ name: string; path: string; sizeBytes: number }> = [];
    const skippedNames: string[] = [];
    for (let i = 0; i < files.length; i++) {
      const f = files[i];
      if (!isSupportedUploadFile(f.name)) {
        skippedNames.push(f.name);
        continue;
      }
      const filePath = window.electronAPI.getPathForFile(f);
      if (filePath) {
        validFiles.push({ name: f.name, path: filePath, sizeBytes: f.size });
      }
    }

    setSkippedNotice(
      skippedNames.length > 0
        ? t("notes.upload.unsupportedFiles", { names: skippedNames.join(", ") })
        : null
    );
    if (validFiles.length === 0) return;

    if (validFiles.length === 1 && !batch.isProcessing && !batch.hasQueue) {
      const f = validFiles[0];
      setFile({
        name: f.name,
        path: f.path,
        size: formatFileSize(f.sizeBytes),
        sizeBytes: f.sizeBytes,
      });
      setState("selected");
      setError(null);
    } else {
      batch.addFiles(validFiles);
    }
  };

  const reset = () => {
    if (progressRef.current) clearInterval(progressRef.current);
    setState("idle");
    setFile(null);
    setResult(null);
    setPartialWarning(null);
    setTranscriptionId(null);
    setSaveError(null);
    setError(null);
    setProgress(0);
    setSkippedNotice(null);
  };

  const cancelTranscription = () => {
    if (activeRequestIdRef.current) {
      window.electronAPI.cancelUploadTranscription?.(activeRequestIdRef.current);
      activeRequestIdRef.current = null;
    }
    runIdRef.current++;
    reset();
  };

  const handleTranscribe = async () => {
    if (!file || batch.isProcessing) return;
    const runId = ++runIdRef.current;
    const requestId = crypto.randomUUID();
    activeRequestIdRef.current = requestId;
    setState("transcribing");
    setError(null);
    setSaveError(null);
    setProgress(0);
    progressRef.current = setInterval(() => {
      setProgress((prev) => {
        if (prev >= 90) {
          if (progressRef.current) clearInterval(progressRef.current);
          return prev;
        }
        return prev + Math.random() * 6;
      });
    }, 500);

    try {
      const res = await transcribeFile(file.path, buildTranscriptionConfig(), false, {
        requestId,
      }).finally(() => {
        if (activeRequestIdRef.current === requestId) activeRequestIdRef.current = null;
      });
      if (runId !== runIdRef.current) return;
      if (progressRef.current) clearInterval(progressRef.current);

      if (res.success && res.text) {
        setProgress(100);
        setResult(res.text);
        setPartialWarning(
          res.failedChunks && res.totalChunks
            ? { failed: res.failedChunks, total: res.totalChunks }
            : null
        );
        try {
          const saved = await saveUploadTranscription(res.text);
          if (runId !== runIdRef.current) return;
          if (saved.success) setTranscriptionId(saved.id);
          else setSaveError(t("notes.upload.errorOccurred"));
        } catch {
          if (runId !== runIdRef.current) return;
          setSaveError(t("notes.upload.errorOccurred"));
        }
        setState("complete");
      } else {
        setProgress(0);
        const errorKey = transcriptionErrorKey(res);
        setError(
          errorKey
            ? t(`notes.upload.${errorKey}`)
            : res.messageKey
              ? t(res.messageKey)
              : res.error || t("notes.upload.transcriptionFailed")
        );
        setState("error");
      }
    } catch (err) {
      if (runId !== runIdRef.current) return;
      if (progressRef.current) clearInterval(progressRef.current);
      setProgress(0);
      const errorKey = transcriptionErrorKey(err);
      setError(
        errorKey
          ? t(`notes.upload.${errorKey}`)
          : err instanceof Error
            ? err.message
            : t("notes.upload.errorOccurred")
      );
      setState("error");
    }
  };

  const handleRetry = () => {
    if (file) void handleTranscribe();
    else reset();
  };

  const startBatchProcessing = () => {
    if (state === "transcribing") return;
    setSkippedNotice(null);
    batch.processQueue({ transcription: buildTranscriptionConfig() });
  };

  const getTranscribingLabel = (): string =>
    t("notes.upload.transcribingProvider", {
      provider: t("settingsPage.transcription.modes.selfHosted"),
    });

  return (
    <div className="flex flex-col items-center h-full overflow-y-auto px-6">
      <div
        className="w-full max-w-md shrink-0 my-auto"
        style={{ animation: "float-up 0.4s ease-out" }}
      >
        <div className="max-w-[320px] mx-auto">
          {state === "idle" && !providerReady && (
            <NoProviderView t={t} onOpenSettings={() => onOpenSettings?.("uploadTranscription")} />
          )}
          {state === "idle" && providerReady && (
            <IdleView
              t={t}
              getActiveModelLabel={getActiveModelLabel}
              handleDrop={handleDrop}
              handleBrowse={handleBrowse}
              isDragOver={isDragOver}
              setIsDragOver={setIsDragOver}
              onOpenSettings={onOpenSettings}
            />
          )}
          {skippedNotice && (
            <p className="text-[10px] text-amber-500/60 mt-2 text-center">{skippedNotice}</p>
          )}
          {batch.hasQueue && (
            <div className="mt-3">
              <BatchQueueView
                queue={batch.queue}
                completedCount={batch.completedCount}
                failedCount={batch.failedCount}
                totalCount={batch.totalCount}
                isProcessing={batch.isProcessing}
                onRemoveItem={batch.removeItem}
                onCancelAll={batch.cancelAll}
                onClearQueue={() => {
                  setSkippedNotice(null);
                  batch.clearQueue();
                }}
                onOpenHistory={onOpenHistory}
                onCopyText={onCopyText}
              />
              {!batch.isProcessing && batch.queue.some((i) => i.status === "queued") && (
                <div className="mt-3 space-y-2">
                  <div className="flex justify-center">
                    <Button
                      variant="default"
                      size="sm"
                      onClick={startBatchProcessing}
                      disabled={!providerReady || state === "transcribing"}
                      className="h-8 text-xs px-5"
                    >
                      {t("notes.upload.transcribe")}
                    </Button>
                  </div>
                </div>
              )}
            </div>
          )}
          {state === "selected" && file && (
            <SelectedView
              t={t}
              file={file}
              getActiveModelLabel={getActiveModelLabel}
              reset={reset}
              handleTranscribe={handleTranscribe}
              transcribeDisabled={batch.isProcessing || !providerReady}
              onOpenSettings={onOpenSettings}
            />
          )}
          {state === "transcribing" && (
            <TranscribingView
              t={t}
              progress={progress}
              getTranscribingLabel={getTranscribingLabel}
              file={file}
              onCancel={cancelTranscription}
            />
          )}
          {state === "complete" && result && (
            <CompleteView
              t={t}
              result={result}
              partialWarning={partialWarning}
              transcriptionId={transcriptionId}
              saveError={saveError}
              onOpenHistory={onOpenHistory}
              onCopyText={onCopyText}
              reset={reset}
            />
          )}
          {state === "error" && error && (
            <ErrorView t={t} error={error} reset={reset} onRetry={handleRetry} />
          )}
        </div>
      </div>
    </div>
  );
}

interface NoProviderViewProps {
  t: (key: string, options?: Record<string, unknown>) => string;
  onOpenSettings: () => void;
}

function NoProviderView({ t, onOpenSettings }: NoProviderViewProps) {
  return (
    <div
      className="flex flex-col items-center gap-4 py-2"
      style={{ animation: "float-up 0.4s ease-out" }}
    >
      <div className="w-10 h-10 rounded-[10px] bg-linear-to-b from-foreground/5 to-foreground/2 dark:from-white/8 dark:to-white/3 border border-foreground/8 dark:border-white/10 flex items-center justify-center">
        <Settings
          size={17}
          strokeWidth={1.5}
          className="text-foreground/45 dark:text-foreground/45"
        />
      </div>
      <div className="text-center">
        <h2 className="text-xs font-semibold text-foreground mb-1">
          {t("notes.upload.noProviderTitle")}
        </h2>
        <p className="text-xs text-foreground/45 leading-relaxed max-w-60">
          {t("notes.upload.customEndpointInvalid")}
        </p>
      </div>
      <Button variant="default" size="sm" className="h-7 text-xs px-4" onClick={onOpenSettings}>
        {t("notes.upload.noProviderAction")}
      </Button>
    </div>
  );
}

interface IdleViewProps {
  t: (key: string, options?: Record<string, unknown>) => string;
  getActiveModelLabel: () => string;
  handleDrop: (e: React.DragEvent) => void;
  handleBrowse: () => void;
  isDragOver: boolean;
  setIsDragOver: (v: boolean) => void;
  onOpenSettings?: (section: string) => void;
}

function IdleView({
  t,
  getActiveModelLabel,
  handleDrop,
  handleBrowse,
  isDragOver,
  setIsDragOver,
  onOpenSettings,
}: IdleViewProps) {
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      handleBrowse();
    }
  };

  return (
    <>
      <div className="flex flex-col items-center mb-5">
        <div className="w-10 h-10 rounded-[10px] bg-linear-to-b from-foreground/5 to-foreground/[0.02] dark:from-white/8 dark:to-white/3 border border-foreground/8 dark:border-white/10 flex items-center justify-center mb-4">
          <Upload
            size={17}
            strokeWidth={1.5}
            className="text-foreground/45 dark:text-foreground/45"
          />
        </div>
        <h2 className="text-xs font-semibold text-foreground mb-1">{t("notes.upload.title")}</h2>
        <UploadModelSettingsButton
          label={t("notes.upload.using", { model: getActiveModelLabel() })}
          actionLabel={t("notes.upload.noProviderAction")}
          onOpenSettings={onOpenSettings}
          className="text-xs text-foreground/70"
        />
      </div>

      <div
        role="button"
        tabIndex={0}
        aria-label={t("notes.upload.dropOrBrowse")}
        onDrop={handleDrop}
        onDragOver={(e) => {
          e.preventDefault();
          setIsDragOver(true);
        }}
        onDragLeave={(e) => {
          e.preventDefault();
          setIsDragOver(false);
        }}
        onClick={handleBrowse}
        onKeyDown={handleKeyDown}
        className={cn(
          "relative rounded-lg p-8 text-center cursor-pointer transition-[background-color,border-color,transform] duration-300 group",
          "bg-surface-1/40 dark:bg-white/[0.03] backdrop-blur-sm",
          "border border-foreground/6 dark:border-white/10",
          "hover:bg-surface-1/60 dark:hover:bg-white/[0.05] hover:border-foreground/12 dark:hover:border-white/10",
          "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary/30",
          isDragOver && "border-primary/30 bg-primary/[0.04] dark:bg-primary/[0.06] scale-[1.01]"
        )}
        style={isDragOver ? { animation: "drag-pulse 1.5s ease-in-out infinite" } : undefined}
      >
        <div className="absolute inset-0 rounded-lg overflow-hidden pointer-events-none opacity-0 group-hover:opacity-100 transition-opacity duration-500">
          <div
            className="absolute inset-0 bg-gradient-to-r from-transparent via-foreground/[0.02] dark:via-white/[0.03] to-transparent"
            style={{ animation: "shimmer-slide 3s ease-in-out infinite" }}
          />
        </div>

        {!isDragOver ? (
          <div className="flex flex-col items-center gap-2 relative">
            <div className="w-8 h-8 rounded-full bg-foreground/[0.03] dark:bg-white/[0.04] flex items-center justify-center mb-1">
              <Upload
                size={14}
                className="text-foreground/45 dark:text-foreground/45 transition-colors"
              />
            </div>
            <p className="text-xs text-foreground/45 group-hover:text-foreground/50 transition-colors">
              {t("notes.upload.dropOrBrowse")}
            </p>
            <p className="text-xs text-foreground/45 tracking-wide">
              {t("notes.upload.supportedFormats")}
            </p>
          </div>
        ) : (
          <div className="flex flex-col items-center gap-2 relative">
            <Upload size={18} className="text-primary/60" />
            <p className="text-xs text-primary/60 font-medium">{t("notes.upload.dropToUpload")}</p>
          </div>
        )}
      </div>
    </>
  );
}

interface SelectedViewProps {
  t: (key: string, options?: Record<string, unknown>) => string;
  file: { name: string; path: string; size: string; sizeBytes: number };
  getActiveModelLabel: () => string;
  reset: () => void;
  handleTranscribe: () => void;
  transcribeDisabled: boolean;
  onOpenSettings?: (section: string) => void;
}

function SelectedView({
  t,
  file,
  getActiveModelLabel,
  reset,
  handleTranscribe,
  transcribeDisabled,
  onOpenSettings,
}: SelectedViewProps) {
  return (
    <div style={{ animation: "float-up 0.3s ease-out" }}>
      <div className="rounded-lg border border-foreground/8 dark:border-white/10 bg-surface-1/40 dark:bg-white/[0.03] backdrop-blur-sm p-4 mb-3">
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-[8px] bg-primary/8 dark:bg-primary/12 border border-primary/10 dark:border-primary/15 flex items-center justify-center shrink-0">
            <FileAudio size={15} className="text-primary/60" />
          </div>
          <div className="min-w-0 flex-1">
            <p dir="ltr" className="text-xs text-foreground/70 truncate font-medium">
              {file.name}
            </p>
            {file.size && <p className="text-xs text-foreground/45 mt-0.5">{file.size}</p>}
            <UploadModelSettingsButton
              label={getActiveModelLabel()}
              actionLabel={t("notes.upload.noProviderAction")}
              onOpenSettings={onOpenSettings}
              className="block max-w-full truncate text-start text-xs text-foreground/70 mt-0.5"
            />
          </div>
          <button onClick={reset} className="text-foreground/45 transition-colors p-1 rounded">
            <X size={12} />
          </button>
        </div>
      </div>

      <div className="flex items-center gap-2 justify-center flex-wrap">
        <Button
          variant="default"
          size="sm"
          onClick={handleTranscribe}
          disabled={transcribeDisabled}
          className="h-8 text-xs px-5"
        >
          {t("notes.upload.transcribe")}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={reset}
          className="h-8 text-xs text-foreground/45"
        >
          {t("notes.upload.cancel")}
        </Button>
      </div>
    </div>
  );
}

interface TranscribingViewProps {
  t: (key: string, options?: Record<string, unknown>) => string;
  progress: number;
  getTranscribingLabel: () => string;
  file: { name: string; path: string; size: string; sizeBytes: number } | null;
  onCancel: () => void;
}

function TranscribingView({
  t,
  progress,
  getTranscribingLabel,
  file,
  onCancel,
}: TranscribingViewProps) {
  return (
    <div className="flex flex-col items-center" style={{ animation: "float-up 0.3s ease-out" }}>
      <div className="flex items-end justify-center gap-[3px] h-10 mb-5">
        {[0, 1, 2, 3, 4, 5, 6].map((i) => (
          <div
            key={i}
            className="w-[3px] rounded-full bg-primary/40 dark:bg-primary/50 origin-bottom"
            style={{
              height: "100%",
              animation: `waveform-bar ${0.8 + i * 0.12}s ease-in-out infinite`,
              animationDelay: `${i * 0.08}s`,
            }}
          />
        ))}
      </div>

      <div className="w-full max-w-[200px] h-[3px] rounded-full bg-foreground/5 dark:bg-white/5 overflow-hidden mb-3">
        <div
          className="h-full rounded-full bg-primary/50 transition-[width] duration-500 ease-out"
          style={{ width: `${Math.min(progress, 100)}%` }}
        />
      </div>

      <p className="text-xs text-foreground/50 font-medium">{getTranscribingLabel()}</p>
      {file && (
        <p dir="ltr" className="text-xs text-foreground/45 mt-1 truncate max-w-50">
          {file.name}
        </p>
      )}
      <Button
        variant="ghost"
        size="sm"
        onClick={onCancel}
        className="h-7 text-xs text-foreground/45 mt-4"
      >
        {t("notes.upload.cancelTranscription")}
      </Button>
    </div>
  );
}

interface CompleteViewProps {
  t: (key: string, options?: Record<string, unknown>) => string;
  result: string;
  partialWarning: { failed: number; total: number } | null;
  transcriptionId: number | null;
  saveError: string | null;
  onOpenHistory?: () => void;
  onCopyText: (text: string) => void;
  reset: () => void;
}

function CompleteView({
  t,
  result,
  partialWarning,
  transcriptionId,
  saveError,
  onOpenHistory,
  onCopyText,
  reset,
}: CompleteViewProps) {
  return (
    <div className="flex flex-col items-center" style={{ animation: "float-up 0.3s ease-out" }}>
      <div className="relative w-12 h-12 mb-4">
        <svg className="w-12 h-12 -rotate-90" viewBox="0 0 36 36">
          <circle
            cx="18"
            cy="18"
            r="15"
            fill="none"
            strokeWidth="1.5"
            className="stroke-success/15"
          />
          <circle
            cx="18"
            cy="18"
            r="15"
            fill="none"
            strokeWidth="1.5"
            className="stroke-success/60"
            strokeDasharray="94.25"
            strokeLinecap="round"
            style={{ animation: "ring-fill 0.8s ease-out forwards" }}
          />
        </svg>
        <div className="absolute inset-0 flex items-center justify-center">
          <svg className="w-5 h-5 text-success/70" viewBox="0 0 24 24" fill="none">
            <path
              d="M5 13l4 4L19 7"
              stroke="currentColor"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeDasharray="24"
              strokeDashoffset="24"
              style={{ animation: "draw-check 0.4s ease-out 0.5s forwards" }}
            />
          </svg>
        </div>
      </div>

      <p className="text-xs text-foreground/60 font-medium mb-1">
        {t("notes.upload.transcriptionComplete")}
      </p>
      <p className="text-xs text-foreground/45 max-w-[240px] text-center line-clamp-2 mb-4">
        {result.slice(0, 150)}
      </p>

      <UploadCompleteWarnings partialWarning={partialWarning} diarizationWarning={false} t={t} />

      {saveError && (
        <p
          className="text-xs text-destructive/50 max-w-[240px] text-center mb-4 -mt-2"
          role="status"
        >
          {saveError}
        </p>
      )}
      {transcriptionId == null && !saveError && (
        <p className="text-xs text-foreground/45 max-w-[240px] text-center mb-4 -mt-2">
          {t("controlPanel.history.dataRetentionDisabled")}
        </p>
      )}

      <div className="flex items-center gap-2">
        {transcriptionId != null && onOpenHistory && (
          <Button variant="default" size="sm" onClick={onOpenHistory} className="h-8 text-xs">
            {t("controlPanel.history.sectionTitle")}
          </Button>
        )}
        {transcriptionId == null && (
          <Button
            variant="default"
            size="sm"
            onClick={() => onCopyText(result)}
            className="h-8 text-xs"
          >
            {t("controlPanel.history.copyText")}
          </Button>
        )}
        <Button
          variant="ghost"
          size="sm"
          onClick={reset}
          className="h-8 text-xs text-foreground/45"
        >
          {t("notes.upload.uploadAnother")}
        </Button>
      </div>
    </div>
  );
}

interface ErrorViewProps {
  t: (key: string) => string;
  error: string;
  reset: () => void;
  onRetry: () => void;
}

function ErrorView({ t, error, reset, onRetry }: ErrorViewProps) {
  return (
    <div style={{ animation: "float-up 0.3s ease-out" }}>
      <div className="rounded-lg border border-destructive/15 dark:border-destructive/20 bg-destructive/[0.03] dark:bg-destructive/[0.05] backdrop-blur-sm p-4 mb-4">
        <div className="flex items-start gap-2.5">
          <AlertCircle size={14} className="text-destructive/50 shrink-0 mt-0.5" />
          <p className="flex-1 text-xs text-destructive/70 leading-relaxed">{error}</p>
          <button
            onClick={reset}
            className="text-foreground/45 transition-colors shrink-0 p-0.5 rounded"
          >
            <X size={11} />
          </button>
        </div>
      </div>

      <div className="flex items-center gap-2 justify-center">
        <Button
          variant="ghost"
          size="sm"
          onClick={onRetry}
          className="h-7 text-xs text-foreground/45"
        >
          {t("notes.upload.retry")}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={reset}
          className="h-7 text-xs text-foreground/45"
        >
          {t("notes.upload.startOver")}
        </Button>
      </div>
    </div>
  );
}
