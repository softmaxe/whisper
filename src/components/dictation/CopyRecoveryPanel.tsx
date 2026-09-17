import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Copy, X } from "../icons";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { ExpandingPanelShell } from "./ExpandingPanelShell";

interface CopyRecoveryPanelProps {
  open: boolean;
  text: string;
  copyFallback: "copied" | "copy";
  onClose: () => void;
  onPreferredHeightChange?: (
    height: number,
    measurementRevision?: string | number | null
  ) => void | Promise<unknown>;
}

const TRANSCRIPT_CONTAINER_CLASS =
  "agent-chat-scroll overflow-y-auto overscroll-contain px-4 py-3 [scrollbar-gutter:stable]";
const TRANSCRIPT_CLASS =
  "select-text whitespace-pre-wrap break-words text-[15px] leading-6 text-foreground";

export function CopyRecoveryPanel({
  open,
  text,
  copyFallback,
  onClose,
  onPreferredHeightChange,
}: CopyRecoveryPanelProps) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const activeTextRef = useRef(text);
  const revealGenerationRef = useRef(0);
  const revealFrameRef = useRef(0);
  const [readyText, setReadyText] = useState<string | null>(null);
  const visible = open && readyText === text;
  const [copying, setCopying] = useState(false);
  const [copyResult, setCopyResult] = useState<{ text: string; success: boolean } | null>(null);
  const { copyText } = useCopyFeedback(text);
  const result = copyResult?.text === text ? copyResult : null;
  const copyFailed = result?.success === false;
  const readyToPaste = !copyFailed && (result?.success || copyFallback === "copied");
  const noticeKey = copyFailed ? "copyFailed" : readyToPaste ? "pasteFailedCopied" : "pasteFailed";

  useLayoutEffect(() => {
    activeTextRef.current = text;
    setReadyText(null);
    revealGenerationRef.current += 1;
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
    return () => {
      revealGenerationRef.current += 1;
      cancelAnimationFrame(revealFrameRef.current);
    };
  }, [open, text]);

  const handlePreferredHeightChange = useCallback(
    async (height: number, revision?: string | number | null) => {
      const generation = ++revealGenerationRef.current;
      cancelAnimationFrame(revealFrameRef.current);
      try {
        await onPreferredHeightChange?.(height, revision);
      } catch {
        // Keep the transcript accessible even if native resizing fails.
      }
      if (!open || generation !== revealGenerationRef.current) return;
      // Match the upstream panel entrance: paint the closed surface at its
      // measured native size before transitioning it into view.
      revealFrameRef.current = requestAnimationFrame(() => {
        revealFrameRef.current = requestAnimationFrame(() => {
          if (generation === revealGenerationRef.current) setReadyText(text);
        });
      });
    },
    [onPreferredHeightChange, open, text]
  );

  const handleCopy = async () => {
    if (copying || readyToPaste || !text.trim()) return;
    setCopying(true);
    const success = await copyText(text);
    if (activeTextRef.current === text) {
      setCopyResult({ text, success });
    }
    setCopying(false);
  };

  return (
    <ExpandingPanelShell
      open={visible}
      measureWhenClosed={open}
      preferredHeightCap={420}
      measurementKey={open ? "copy-recovery" : null}
      measurementRevision={text}
      onPreferredHeightChange={handlePreferredHeightChange}
      data-panel-mode="copy-recovery"
      className="font-sans"
      aria-label={t("transcriptionPreview.recovery.title")}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <header className="flex shrink-0 items-start gap-3 border-b border-border/40 px-4 pb-3 pt-4">
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold leading-5 text-foreground">
            {t("transcriptionPreview.recovery.title")}
          </h2>
          <p
            role="status"
            className={`mt-1 text-xs leading-5 ${copyFailed ? "text-destructive" : "text-muted-foreground"}`}
          >
            {t(`transcriptionPreview.recovery.${noticeKey}`)}
          </p>
        </div>
        <div className="-me-1 -mt-1 flex shrink-0 flex-col items-center gap-0.5">
          <button
            type="button"
            onClick={onClose}
            disabled={!visible}
            tabIndex={visible ? 0 : -1}
            className="inline-flex size-7 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-surface-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={t("transcriptionPreview.recovery.close")}
            aria-keyshortcuts="Escape"
          >
            <X className="size-4" aria-hidden="true" />
          </button>
          <kbd className="font-sans text-[10px] leading-3 text-muted-foreground" aria-hidden="true">
            Esc
          </kbd>
        </div>
      </header>

      <div
        ref={scrollRef}
        role="region"
        aria-label={t("transcriptionPreview.recovery.text")}
        tabIndex={visible ? 0 : -1}
        data-panel-scroll-region
        className={`min-h-0 flex-auto ${TRANSCRIPT_CONTAINER_CLASS} focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring`}
      >
        <p dir="auto" className={TRANSCRIPT_CLASS}>
          {text}
        </p>
      </div>

      <footer className="flex shrink-0 items-center justify-between gap-3 border-t border-border/40 px-4 py-3">
        <span className="text-xs leading-5 text-muted-foreground">
          {readyToPaste ? t("transcriptionPreview.recovery.pasteHint") : ""}
        </span>
        {readyToPaste ? (
          <span className="inline-flex h-8 shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
            <Check className="size-3.5 text-success" aria-hidden="true" />
            <span>{t("transcriptionPreview.recovery.copied")}</span>
          </span>
        ) : (
          <button
            type="button"
            onClick={() => void handleCopy()}
            disabled={!visible || copying || !text.trim()}
            tabIndex={visible ? 0 : -1}
            className="inline-flex h-8 shrink-0 items-center justify-center gap-2 rounded-lg border border-border/80 bg-surface-3 px-3 text-xs font-medium text-foreground shadow-[var(--shadow-metallic-light)] transition-[background-color,border-color,box-shadow] duration-150 hover:border-border-hover hover:bg-surface-raised active:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-surface-0 disabled:pointer-events-none disabled:opacity-50"
          >
            <Copy className="size-4" aria-hidden="true" />
            <span>{t("transcriptionPreview.recovery.copy")}</span>
          </button>
        )}
      </footer>

      <div
        data-panel-size-source
        className={`pointer-events-none invisible absolute inset-x-0 top-0 ${TRANSCRIPT_CONTAINER_CLASS}`}
        aria-hidden="true"
      >
        <p dir="auto" className={TRANSCRIPT_CLASS}>
          {text}
        </p>
      </div>
    </ExpandingPanelShell>
  );
}
