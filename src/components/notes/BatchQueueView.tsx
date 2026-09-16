import type { JSX } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Check, X, Loader2, Clock, Trash2 } from "../icons";
import { Button } from "../ui/button";
import { cn } from "../lib/utils";
import type { QueueItem } from "../../stores/batchQueueStore";

interface BatchQueueViewProps {
  queue: QueueItem[];
  completedCount: number;
  failedCount: number;
  totalCount: number;
  isProcessing: boolean;
  onRemoveItem: (id: string) => void;
  onCancelAll: () => void;
  onClearQueue: () => void;
  onOpenHistory?: () => void;
  onCopyText: (text: string) => void;
}

function StatusIcon({ status }: { status: QueueItem["status"] }) {
  switch (status) {
    case "done":
      return <Check size={12} className="text-success/70" />;
    case "error":
      return <X size={12} className="text-destructive/70" />;
    case "queued":
      return <Clock size={12} className="text-foreground/45" />;
    default:
      return <Loader2 size={12} className="text-primary/60 animate-spin" />;
  }
}

interface BatchWarningIndicatorProps {
  transcriptionWarning: boolean;
  t: (key: string) => string;
}

export function BatchWarningIndicator({
  transcriptionWarning,
  t,
}: BatchWarningIndicatorProps): JSX.Element | null {
  const messages: string[] = [];
  if (transcriptionWarning) messages.push(t("notes.upload.partialWarning"));
  if (messages.length === 0) return null;

  const label = messages.join(" ");
  return (
    <span className="flex shrink-0" role="img" title={label} aria-label={label}>
      <AlertTriangle size={11} className="text-warning" />
    </span>
  );
}

export default function BatchQueueView({
  queue,
  completedCount,
  failedCount,
  totalCount,
  isProcessing,
  onRemoveItem,
  onCancelAll,
  onClearQueue,
  onOpenHistory,
  onCopyText,
}: BatchQueueViewProps) {
  const { t } = useTranslation();
  const allDone =
    queue.length > 0 && queue.every((i) => i.status === "done" || i.status === "error");
  // Failed items still count as settled so the bar reaches 100% when the run ends.
  const overallProgress =
    totalCount > 0 ? Math.round(((completedCount + failedCount) / totalCount) * 100) : 0;

  return (
    <div style={{ animation: "float-up 0.3s ease-out" }}>
      <div className="mb-3">
        <div className="flex items-center justify-between mb-1.5">
          <p className="text-xs text-foreground/50 font-medium">
            {t("notes.upload.queueProgress", {
              completed: completedCount,
              total: totalCount,
            })}
            {failedCount > 0 && (
              <span className="text-destructive/50 font-normal">
                {" · "}
                {t("notes.upload.queueFailed", { n: failedCount })}
              </span>
            )}
          </p>
          {allDone && (
            <Button
              variant="ghost"
              size="sm"
              onClick={onClearQueue}
              className="h-6 text-[10px] text-foreground/45"
            >
              {t("notes.upload.clearQueue")}
            </Button>
          )}
        </div>
        <div className="w-full h-[3px] rounded-full bg-foreground/5 dark:bg-white/5 overflow-hidden">
          <div
            className="h-full rounded-full bg-primary/50 transition-[width] duration-500 ease-out"
            style={{ width: `${overallProgress}%` }}
          />
        </div>
      </div>

      <div className="space-y-1 max-h-[300px] overflow-y-auto">
        {queue.map((item) => (
          <div
            key={item.id}
            className={cn(
              "flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs",
              "bg-surface-1/30 dark:bg-white/[0.02] border border-foreground/4 dark:border-white/10",
              item.status === "error" && "border-destructive/15"
            )}
          >
            <StatusIcon status={item.status} />
            <span className="flex-1 truncate text-foreground/60">{item.name}</span>

            {item.status === "done" && (
              <BatchWarningIndicator transcriptionWarning={!!item.warning} t={t} />
            )}

            {item.status === "done" && item.transcriptionId != null && onOpenHistory && (
              <button
                onClick={onOpenHistory}
                className="text-[10px] text-primary/50 hover:text-primary/70"
                aria-label={t("controlPanel.history.sectionTitle")}
              >
                {t("controlPanel.history.sectionTitle")}
              </button>
            )}

            {(item.status === "done" || item.status === "error") &&
              item.text &&
              item.transcriptionId == null && (
                <button
                  onClick={() => onCopyText(item.text!)}
                  className="text-[10px] text-primary/50 hover:text-primary/70"
                  aria-label={t("controlPanel.history.copyText")}
                >
                  {t("controlPanel.history.copyText")}
                </button>
              )}

            {item.status === "error" && item.error && (
              <span
                className="text-[10px] text-destructive/50 truncate max-w-20"
                title={t(`notes.upload.${item.error}`, {
                  defaultValue: item.error,
                })}
              >
                {t(`notes.upload.${item.error}`, {
                  defaultValue: item.error,
                })}
              </span>
            )}

            {item.status === "queued" && (
              <button
                onClick={() => onRemoveItem(item.id)}
                className="text-foreground/45 transition-colors"
                aria-label={t("notes.upload.removeFromQueue")}
              >
                <Trash2 size={10} />
              </button>
            )}
          </div>
        ))}
      </div>

      {queue.some((item) => item.status === "done" && item.transcriptionId == null) && (
        <p className="text-[10px] text-foreground/45 mt-2 text-center">
          {t("controlPanel.history.dataRetentionDisabled")}
        </p>
      )}

      {isProcessing && !allDone && (
        <div className="flex justify-center mt-3">
          <Button
            variant="ghost"
            size="sm"
            onClick={onCancelAll}
            className="h-7 text-xs text-foreground/45"
          >
            {t("notes.upload.cancelAll")}
          </Button>
        </div>
      )}
    </div>
  );
}
