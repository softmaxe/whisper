import * as DialogPrimitive from "@radix-ui/react-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useUiLocale } from "../hooks/useUiLocale";
import type { NoteItem, SpaceItem, TranscriptionItem } from "../types/electron.js";
import { formatRelativeTime } from "../utils/dateFormatting";
import { Mic, Search } from "./icons";
import { cn } from "./lib/utils";
import { useDismissGuard } from "./ui/useDismissGuard";

interface ConversationResult {
  id: number;
  title: string;
  last_message?: string;
  updated_at: string;
}

interface JumpTarget {
  key: string;
  spaceId: number;
  folderId: number | null;
  label: string;
  space: SpaceItem | undefined;
}

export interface CommandSearchProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mode?: "all" | "conversations";
  transcriptions?: TranscriptionItem[];
  onNoteSelect?: (noteId: number, folderId: number | null, spaceId?: number) => void;
  onContainerSelect?: (spaceId: number, folderId: number | null) => void;
  onTranscriptSelect?: (transcriptId: number) => void;
  onConversationSelect?: (conversationId: number) => void;
}

type FlatItem =
  | { kind: "container"; target: JumpTarget }
  | { kind: "note"; note: NoteItem }
  | { kind: "transcript"; transcript: TranscriptionItem }
  | { kind: "conversation"; conversation: ConversationResult };

export default function CommandSearch({
  open,
  onOpenChange,
  transcriptions = [],
  onTranscriptSelect,
}: CommandSearchProps) {
  const { t } = useTranslation();
  const locale = useUiLocale();
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [prevOpen, setPrevOpen] = useState(open);
  const [prevQuery, setPrevQuery] = useState(query);

  if (open && !prevOpen) {
    setPrevOpen(open);
    setQuery("");
    setSelectedIndex(0);
  } else if (open !== prevOpen) {
    setPrevOpen(open);
  }

  if (query !== prevQuery) {
    setPrevQuery(query);
    setSelectedIndex(0);
  }

  const filteredTranscripts = useMemo(() => {
    const slice = query.trim()
      ? transcriptions.filter((tr) => tr.text.toLowerCase().includes(query.toLowerCase()))
      : transcriptions;
    return slice.slice(0, 5);
  }, [transcriptions, query]);

  const flatItems = useMemo<FlatItem[]>(
    () => filteredTranscripts.map((transcript) => ({ kind: "transcript", transcript })),
    [filteredTranscripts]
  );

  const selectItem = useCallback(
    (item: FlatItem) => {
      if (item.kind === "transcript") onTranscriptSelect?.(item.transcript.id);
      onOpenChange(false);
    },
    [onTranscriptSelect, onOpenChange]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIndex((i) => Math.min(i + 1, flatItems.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIndex((i) => Math.max(i - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const item = flatItems[selectedIndex];
        if (item) selectItem(item);
      }
    },
    [flatItems, selectedIndex, selectItem]
  );

  useEffect(() => {
    const el = listRef.current?.querySelector(`[data-idx="${selectedIndex}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  const hasResults = flatItems.length > 0;
  const { registerContent, shouldBlockDismiss } = useDismissGuard<HTMLDivElement>();

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        {/* No open/close animation: the palette is keyboard-first (Cmd/Ctrl+K) and
            used repeatedly, so it must appear and disappear instantly. */}
        <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-black/50 backdrop-blur-sm" />
        <DialogPrimitive.Content
          ref={registerContent}
          onInteractOutside={(e) => {
            // The filter dropdown makes this panel inert while it is open, so
            // the click that closes it lands on the overlay — see useDismissGuard.
            if (shouldBlockDismiss(e)) e.preventDefault();
          }}
          className={cn(
            "fixed left-[50%] top-[18%] z-50 w-full max-w-xl translate-x-[-50%]",
            "rounded-xl border border-border/70 bg-card shadow-2xl overflow-hidden",
            "dark:bg-surface-2 dark:border-border dark:shadow-modal"
          )}
        >
          <DialogPrimitive.Title className="sr-only">
            {t("commandSearch.title")}
          </DialogPrimitive.Title>
          <DialogPrimitive.Description className="sr-only">
            {t("commandSearch.description")}
          </DialogPrimitive.Description>

          {/* Search input */}
          <div className="flex items-center gap-2.5 px-3.5 py-3 border-b border-border/70">
            <Search size={14} className="shrink-0 text-muted-foreground/70" />

            <input
              dir="auto"
              ref={inputRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={t("commandSearch.sections.transcripts")}
              autoFocus
              className="flex-1 text-sm text-foreground placeholder:text-muted-foreground/70"
              style={{
                background: "transparent",
                border: "none",
                outline: "none",
                boxShadow: "none",
                padding: 0,
              }}
            />
            {query && (
              <button
                onClick={() => setQuery("")}
                className="text-[11px] text-muted-foreground/70 hover:text-muted-foreground transition-colors outline-none"
              >
                ✕
              </button>
            )}
          </div>

          {/* Results list */}
          <div ref={listRef} className="overflow-y-auto max-h-[340px] p-1.5">
            {!hasResults ? (
              <div className="flex items-center justify-center py-10">
                <p className="text-xs text-muted-foreground/70">
                  {query.trim() ? t("commandSearch.noResults") : t("commandSearch.emptyState")}
                </p>
              </div>
            ) : (
              <>
                {filteredTranscripts.length > 0 && (
                  <div>
                    <SectionHeader
                      icon={<Mic size={11} />}
                      label={t("commandSearch.sections.transcripts")}
                    />
                    {filteredTranscripts.map((transcript) => {
                      const idx = flatItems.findIndex(
                        (fi) => fi.kind === "transcript" && fi.transcript.id === transcript.id
                      );
                      return (
                        <TranscriptRow
                          key={transcript.id}
                          transcript={transcript}
                          idx={idx}
                          isSelected={selectedIndex === idx}
                          onSelect={() => selectItem({ kind: "transcript", transcript })}
                          onHover={() => setSelectedIndex(idx)}
                          t={t}
                          locale={locale}
                        />
                      );
                    })}
                  </div>
                )}
              </>
            )}
          </div>

          {/* Footer */}
          <div className="flex items-center gap-4 px-3.5 py-2 border-t border-border/70 bg-muted/15">
            <FooterHint keys={["↑", "↓"]} label={t("commandSearch.footer.navigate")} />
            <FooterHint keys={["↵"]} label={t("commandSearch.footer.open")} />
            <FooterHint keys={["Esc"]} label={t("commandSearch.footer.dismiss")} />
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

function SectionHeader({ icon, label }: { icon: React.ReactNode; label: string }) {
  return (
    <div className="flex items-center gap-1.5 px-2.5 pt-2 pb-1">
      <span className="text-muted-foreground/70">{icon}</span>
      <span className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/70">
        {label}
      </span>
    </div>
  );
}

function TranscriptRow({
  transcript,
  idx,
  isSelected,
  onSelect,
  onHover,
  t,
  locale,
}: {
  transcript: TranscriptionItem;
  idx: number;
  isSelected: boolean;
  onSelect: () => void;
  onHover: () => void;
  t: (key: string, opts?: Record<string, unknown>) => string;
  locale?: string;
}) {
  return (
    <button
      type="button"
      data-idx={idx}
      onClick={onSelect}
      onMouseEnter={onHover}
      className={cn(
        "group flex items-center gap-2.5 w-full px-2.5 py-2 rounded-lg text-start transition-colors duration-100 outline-none",
        isSelected
          ? "bg-primary/8 dark:bg-primary/10"
          : "hover:bg-foreground/4 dark:hover:bg-white/4"
      )}
    >
      <Mic
        size={13}
        className={cn(
          "shrink-0 mt-px transition-colors",
          isSelected ? "text-primary" : "text-muted-foreground/70"
        )}
      />
      <p dir="auto" className="flex-1 text-xs text-foreground/75 truncate min-w-0">
        {transcript.text}
      </p>
      <span className="text-[10px] text-muted-foreground/70 tabular-nums shrink-0">
        {formatRelativeTime(transcript.created_at, t, locale)}
      </span>
    </button>
  );
}

function FooterHint({ keys, label }: { keys: string[]; label: string }) {
  return (
    <div className="flex items-center gap-1">
      <span dir="ltr" className="inline-flex items-center gap-1">
        {keys.map((k) => (
          <kbd
            key={k}
            className="text-[10px] px-1 py-px rounded border border-border/70 bg-muted/50 text-muted-foreground/55 font-mono leading-tight"
          >
            {k}
          </kbd>
        ))}
      </span>
      <span className="text-[10px] text-muted-foreground/70 ms-0.5">{label}</span>
    </div>
  );
}
