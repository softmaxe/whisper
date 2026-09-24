import { Fragment, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useUiLocale } from "../hooks/useUiLocale";
import { useSettingsStore } from "../stores/settingsStore";
import type { TranscriptionItem as TranscriptionItemType } from "../types/electron";
import { formatDateGroup } from "../utils/dateFormatting";
import { formatHotkeyLabel, parseHotkeyList } from "../utils/hotkeys";
import { Archive, Loader2, Mic, Trash2 } from "./icons";
import { cn } from "./lib/utils";
import EmptyStateCard from "./ui/EmptyStateCard";
import TranscriptionItem from "./ui/TranscriptionItem";
// PROTOTYPE: date layout variants, remove once one wins.
import {
  PROTOTYPE_SAMPLE_HISTORY,
  PROTOTYPE_VARIANTS,
  PrototypeHistoryVariant,
} from "./prototype/HistoryDateLayouts.prototype";
import { PrototypeSwitcher, usePrototypeVariant } from "./prototype/PrototypeSwitcher";

const EMPTY_PREVIEW_WIDTHS = ["w-full", "w-4/5", "w-3/5"];

interface HistoryViewProps {
  history: TranscriptionItemType[];
  isLoading: boolean;
  hotkey: string;
  copyToClipboard: (text: string) => void;
  deleteTranscription: (id: number) => void;
  clearAllTranscriptions: () => void;
  onOpenSettings: (section?: string) => void;
  onShowAudioInFolder: (id: number) => void;
  onRetryTranscription: (id: number, options?: { isRecover?: boolean }) => Promise<void>;
  showDiscarded: boolean;
  onToggleDiscarded: () => void;
}

export default function HistoryView({
  history: historyProp,
  isLoading,
  hotkey,

  copyToClipboard,
  deleteTranscription,
  clearAllTranscriptions,
  onOpenSettings,

  onShowAudioInFolder,
  onRetryTranscription,
  showDiscarded,
  onToggleDiscarded,
}: HistoryViewProps) {
  const { t } = useTranslation();
  const locale = useUiLocale();
  const dataRetentionEnabled = useSettingsStore((s) => s.dataRetentionEnabled);
  const [variant, selectVariant] = usePrototypeVariant(PROTOTYPE_VARIANTS);
  // PROTOTYPE: a plain browser has no Electron history, so show sample rows there.
  const history =
    historyProp.length === 0 && !window.electronAPI ? PROTOTYPE_SAMPLE_HISTORY : historyProp;

  const groupedHistory = useMemo(() => {
    if (history.length === 0) return [];

    const groups: { label: string; items: TranscriptionItemType[] }[] = [];
    let currentLabel: string | null = null;

    for (const item of history) {
      const label = formatDateGroup(item.timestamp, t, locale);

      if (label !== currentLabel) {
        groups.push({ label, items: [item] });
        currentLabel = label;
      } else {
        groups[groups.length - 1].items.push(item);
      }
    }

    return groups;
  }, [history, t, locale]);

  const discardedToggle = (
    <button
      onClick={onToggleDiscarded}
      aria-pressed={showDiscarded}
      className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] text-muted-foreground/70 hover:!text-foreground hover:!bg-black/5 dark:hover:!bg-white/5 active:scale-[0.98] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring/30 transition-[color,background-color,transform] duration-150"
    >
      <Archive size={11} />
      <span>
        {showDiscarded
          ? t("controlPanel.history.discarded.hide")
          : t("controlPanel.history.discarded.show")}
      </span>
    </button>
  );

  return (
    <div className="px-4 pt-4 pb-6">
      <div className="mx-auto max-w-5xl">
        <div className="flex gap-8">
          <div className="min-w-0 flex-1">
            {!dataRetentionEnabled && (
              <div className="mb-3 rounded-lg border border-amber-500/30 bg-amber-500/5 dark:bg-amber-500/10 px-3.5 py-2.5 flex items-center gap-2.5">
                <span className="text-amber-600 dark:text-amber-400 shrink-0 text-sm">⊘</span>
                <p className="text-xs text-amber-700 dark:text-amber-300/90 leading-relaxed">
                  {t("controlPanel.history.dataRetentionDisabled")}
                </p>
              </div>
            )}
            {isLoading && history.length === 0 ? (
              <div className="rounded-2xl border border-border/70 bg-card/50 dark:border-white/10 dark:bg-surface-2/60">
                <div className="flex items-center justify-center gap-2 py-10">
                  <Loader2 size={14} className="animate-spin text-primary" />
                  <span className="text-sm text-muted-foreground">{t("controlPanel.loading")}</span>
                </div>
              </div>
            ) : history.length === 0 ? (
              <>
                <div className="flex items-center justify-between pt-2 pb-2.5">
                  <p className="text-sm text-muted-foreground">
                    {t("controlPanel.history.sectionTitle")}
                  </p>
                  {discardedToggle}
                </div>
                <EmptyStateCard
                  icon={Mic}
                  title={t("controlPanel.history.empty")}
                  description={t("controlPanel.history.emptyDescription")}
                >
                  {/* Ghost rows preview the list this card becomes. */}
                  <div aria-hidden="true" className="mb-1 w-56 space-y-2">
                    {EMPTY_PREVIEW_WIDTHS.map((width) => (
                      <span
                        key={width}
                        className={cn(
                          "block h-2 rounded-full bg-foreground/6 dark:bg-white/8",
                          width
                        )}
                      />
                    ))}
                  </div>
                  <span className="inline-flex h-[30px] items-center gap-1.5 rounded-full bg-surface-3 px-3 text-xs font-medium text-foreground/70 dark:bg-surface-3">
                    {t("controlPanel.history.press")}
                    <span dir="ltr" className="inline-flex items-center gap-1">
                      {parseHotkeyList(hotkey).map((hk, index) => (
                        <Fragment key={hk}>
                          {index > 0 && <span className="text-foreground/45">/</span>}
                          <kbd className="rounded-md bg-background px-1.5 py-px font-sans text-[11px] font-medium text-foreground/80 shadow-sm dark:bg-surface-2">
                            {formatHotkeyLabel(hk)}
                          </kbd>
                        </Fragment>
                      ))}
                    </span>
                    {t("controlPanel.history.toStart")}
                  </span>
                </EmptyStateCard>
              </>
            ) : variant !== "current" ? (
              <PrototypeHistoryVariant
                variant={variant}
                items={history}
                locale={locale}
                labels={{
                  today: t("controlPanel.history.dateGroups.today"),
                  yesterday: t("controlPanel.history.dateGroups.yesterday"),
                }}
                onCopy={copyToClipboard}
                actions={
                  <div className="flex items-center gap-1.5 opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity duration-200">
                    {discardedToggle}
                    <button
                      onClick={clearAllTranscriptions}
                      className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] text-muted-foreground/70 hover:!text-destructive hover:!bg-destructive/8"
                    >
                      <Trash2 size={11} />
                      <span>{t("controlPanel.history.clearAll")}</span>
                    </button>
                  </div>
                }
              />
            ) : (
              <div className="group">
                {groupedHistory.map((group, index) => (
                  <div key={group.label} className={index > 0 ? "mt-6" : ""}>
                    <div className="sticky -top-1 z-10 -mx-4 px-4 pt-2 pb-2.5 bg-background flex items-center justify-between">
                      <span className="text-sm text-muted-foreground">{group.label}</span>
                      {index === 0 && (
                        <div className="flex items-center gap-1.5 opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity duration-200">
                          {discardedToggle}
                          <button
                            onClick={clearAllTranscriptions}
                            className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] text-muted-foreground/70 hover:!text-destructive hover:!bg-destructive/8 dark:hover:!bg-destructive/10 active:scale-[0.98] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring/30 transition-[color,background-color,transform] duration-150"
                          >
                            <Trash2 size={11} />
                            <span>{t("controlPanel.history.clearAll")}</span>
                          </button>
                        </div>
                      )}
                    </div>
                    <div className="relative z-0 overflow-clip rounded-2xl border border-border/70 bg-card/50 divide-y divide-border/60 dark:border-white/10 dark:bg-surface-2/60">
                      {group.items.map((item) => (
                        <TranscriptionItem
                          key={item.id}
                          item={item}
                          onCopy={copyToClipboard}
                          onDelete={deleteTranscription}
                          onShowAudioInFolder={onShowAudioInFolder}
                          onRetryTranscription={onRetryTranscription}
                          onOpenSettings={() => onOpenSettings("transcription")}
                        />
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
      <PrototypeSwitcher variants={PROTOTYPE_VARIANTS} current={variant} onSelect={selectVariant} />
    </div>
  );
}
