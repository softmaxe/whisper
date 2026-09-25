import React, { useEffect, useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Upload, FileAudio, X, AlertCircle, Settings, Loader2 } from "../icons";
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
import { setControlPanelHold } from "../../utils/controlPanelRetention";

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
  const runIdRef = useRef(0);
  const activeRequestIdRef = useRef<string | null>(null);
  const [skippedNotice, setSkippedNotice] = useState<string | null>(null);
  const batch = useBatchQueue();

  // A selected file, a running request or its result exist only in this view.
  useEffect(() => {
    setControlPanelHold("upload", state !== "idle");
    return () => setControlPanelHold("upload", false);
  }, [state]);
  const remoteTranscriptionUrl = useSettingsStore((s) => s.remoteTranscriptionUrl);
  const remoteTranscriptionModel = useSettingsStore((s) => s.remoteTranscriptionModel);
  const preferredLanguage = useSettingsStore((s) => s.preferredLanguage);
  const providerReady = !!remoteTranscriptionUrl.trim();

  const getActiveModelLabel = (): string => {
    const name = t("settingsPage.transcription.modes.selfHosted");
    return remoteTranscriptionModel ? `${name} · ${remoteTranscriptionModel}` : name;
  };

  const buildTranscriptionConfig = (): FileTranscriptionConfig => ({
    remoteTranscriptionUrl,
    remoteTranscriptionModel,
    language: getBaseLanguageCode(preferredLanguage) || "",
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
    setState("idle");
    setFile(null);
    setResult(null);
    setPartialWarning(null);
    setTranscriptionId(null);
    setSaveError(null);
    setError(null);
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

    try {
      const res = await transcribeFile(file.path, buildTranscriptionConfig()).finally(() => {
        if (activeRequestIdRef.current === requestId) activeRequestIdRef.current = null;
      });
      if (runId !== runIdRef.current) return;
      if (res.success && res.text) {
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

  return (
    <div className="flex flex-col items-center h-full overflow-y-auto px-6">
      <div
        className={cn(
          "w-full shrink-0 my-auto",
          state === "complete" ? "max-w-2xl py-6" : "max-w-md"
        )}
        style={{ animation: "float-up 0.4s ease-out" }}
      >
        <div className={cn("mx-auto", state !== "complete" && "max-w-[320px]")}>
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
            <TranscribingView t={t} file={file} onCancel={cancelTranscription} />
          )}
          {state === "complete" && result && (
            <CompleteView
              t={t}
              result={result}
              fileName={file?.name}
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
  file: { name: string; path: string; size: string; sizeBytes: number } | null;
  onCancel: () => void;
}

function TranscribingView({ t, file, onCancel }: TranscribingViewProps) {
  return (
    <div className="flex flex-col items-center" style={{ animation: "float-up 0.3s ease-out" }}>
      <Loader2
        size={32}
        aria-hidden="true"
        className="text-primary mb-5 motion-safe:animate-spin"
      />
      <p className="text-xs text-foreground font-medium" role="status">
        {t("notes.upload.transcribing")}
      </p>
      {file && (
        <p dir="ltr" className="text-xs text-muted-foreground mt-1 truncate max-w-50">
          {file.name}
        </p>
      )}
      <p className="text-xs text-muted-foreground mt-4 text-center leading-relaxed">
        {t("notes.upload.completionHint")}
      </p>
      <Button variant="ghost" size="sm" onClick={onCancel} className="mt-4">
        {t("notes.upload.cancelTranscription")}
      </Button>
    </div>
  );
}

interface CompleteViewProps {
  t: (key: string, options?: Record<string, unknown>) => string;
  result: string;
  fileName?: string;
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
  fileName,
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

      <p className="text-xs text-foreground font-medium mb-1" role="status">
        {t("notes.upload.transcriptionComplete")}
      </p>
      {transcriptionId != null && (
        <p className="text-xs text-muted-foreground mb-4">{t("notes.upload.savedToHistory")}</p>
      )}

      <div className="w-full rounded-lg border border-border bg-surface-1 mb-4 overflow-hidden">
        {fileName && (
          <div className="flex items-center gap-2 border-b border-border px-4 py-2.5">
            <FileAudio size={14} aria-hidden="true" className="shrink-0 text-muted-foreground" />
            <p dir="ltr" className="text-xs text-muted-foreground truncate">
              {fileName}
            </p>
          </div>
        )}
        <div
          role="region"
          aria-label={t("notes.upload.transcriptLabel")}
          tabIndex={0}
          className="max-h-[min(40vh,320px)] overflow-y-auto p-4 outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/30"
        >
          <p
            dir="auto"
            className="text-sm text-foreground leading-relaxed whitespace-pre-wrap break-words select-text"
          >
            {result}
          </p>
        </div>
      </div>

      <UploadCompleteWarnings partialWarning={partialWarning} t={t} />

      {saveError && (
        <p className="text-xs text-destructive text-center mb-4" role="status">
          {saveError}
        </p>
      )}
      {transcriptionId == null && !saveError && (
        <p className="text-xs text-muted-foreground text-center mb-4">
          {t("controlPanel.history.dataRetentionDisabled")}
        </p>
      )}

      <div className="flex items-center justify-center gap-2 flex-wrap">
        <Button variant="default" size="sm" onClick={() => onCopyText(result)}>
          {t("notes.upload.copyFullText")}
        </Button>
        {transcriptionId != null && onOpenHistory && (
          <Button variant="ghost" size="sm" onClick={onOpenHistory}>
            {t("notes.upload.viewInHistory")}
          </Button>
        )}
        <Button variant="ghost" size="sm" onClick={reset}>
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
