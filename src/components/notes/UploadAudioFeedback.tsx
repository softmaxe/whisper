import React from "react";
import { cn } from "../lib/utils";

// Both completion warnings sit under the transcript preview and share its
// column, so they share one recipe; only the tone differs.
const COMPLETION_WARNING = "text-xs max-w-[240px] text-center mb-4 -mt-2";

interface UploadModelSettingsButtonProps {
  label: string;
  actionLabel: string;
  onOpenSettings?: (section: string) => void;
  className?: string;
}

export function UploadModelSettingsButton({
  label,
  actionLabel,
  onOpenSettings,
  className,
}: UploadModelSettingsButtonProps): React.JSX.Element {
  return (
    <button
      type="button"
      onClick={() => onOpenSettings?.("uploadTranscription")}
      // WCAG 2.5.3: keep the visible label inside the accessible name, so the
      // model stays announced and voice control can still target the button.
      aria-label={`${label}, ${actionLabel}`}
      title={actionLabel}
      disabled={!onOpenSettings}
      className={cn(
        "rounded-sm underline underline-offset-2 decoration-foreground/30 transition-colors",
        "hover:text-foreground hover:decoration-foreground/50 active:text-foreground/50",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30 focus-visible:ring-offset-1",
        "disabled:pointer-events-none disabled:no-underline disabled:opacity-50",
        className
      )}
    >
      {label}
    </button>
  );
}

interface UploadModelFooterButtonProps {
  label: string;
  changeLabel: string;
  actionLabel: string;
  onOpenSettings?: (section: string) => void;
}

// The idle upload card's footer: the active model with a visible Change action.
// Both visible strings make up the accessible name (WCAG 2.5.3).
export function UploadModelFooterButton({
  label,
  changeLabel,
  actionLabel,
  onOpenSettings,
}: UploadModelFooterButtonProps): React.JSX.Element {
  return (
    <button
      type="button"
      onClick={() => onOpenSettings?.("uploadTranscription")}
      title={actionLabel}
      disabled={!onOpenSettings}
      className={cn(
        "group/model flex w-full items-center justify-between gap-3 py-3 ps-5 pe-4 text-start transition-colors",
        "border-t border-foreground/8 dark:border-white/8 bg-foreground/[0.02] dark:bg-white/[0.02]",
        "hover:bg-foreground/[0.04] dark:hover:bg-white/[0.04]",
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring/40",
        "disabled:pointer-events-none disabled:opacity-50"
      )}
    >
      <span className="min-w-0 truncate text-[13px] text-foreground/85">{label}</span>
      <span className="shrink-0 rounded-md border border-foreground/12 dark:border-white/12 px-2.5 py-1 text-xs text-foreground/85 transition-colors group-hover/model:text-foreground group-hover/model:border-foreground/20 dark:group-hover/model:border-white/20">
        {changeLabel}
      </span>
    </button>
  );
}

interface UploadCompleteWarningsProps {
  partialWarning: { failed: number; total: number } | null;
  t: (key: string, options?: Record<string, unknown>) => string;
}

export function UploadCompleteWarnings({
  partialWarning,
  t,
}: UploadCompleteWarningsProps): React.JSX.Element | null {
  if (!partialWarning) return null;

  return (
    <p className={cn(COMPLETION_WARNING, "text-destructive/50")}>
      {t("notes.upload.partialWarningCount", {
        failed: partialWarning.failed,
        total: partialWarning.total,
      })}
    </p>
  );
}
