// PROTOTYPE — throwaway. Four minimalist date layouts for the History page, switchable via
// ?variant= on the existing control panel route. "current" keeps the shipped layout.
// Delete this file (and its hook-up in HistoryView) once a layout wins.
import { useState, type ReactNode } from "react";
import type { TranscriptionItem } from "../../types/electron";
import { normalizeDbDate } from "../../utils/dateFormatting";
import { Copy } from "../icons";
import { cn } from "../lib/utils";

export const PROTOTYPE_VARIANTS = [
  { key: "current", name: "Current" },
  { key: "A", name: "Gutter" },
  { key: "B", name: "Journal" },
  { key: "C", name: "Stream" },
  { key: "D", name: "Compact" },
] as const;

export type PrototypeVariantKey = (typeof PROTOTYPE_VARIANTS)[number]["key"];

interface DayGroup {
  key: string;
  date: Date;
  daysAgo: number;
  items: TranscriptionItem[];
}

function startOfDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function groupByDay(items: TranscriptionItem[]): DayGroup[] {
  const today = startOfDay(new Date()).getTime();
  const groups: DayGroup[] = [];
  for (const item of items) {
    const date = normalizeDbDate(item.timestamp);
    const day = startOfDay(date);
    const key = day.toDateString();
    const last = groups[groups.length - 1];
    if (last?.key === key) last.items.push(item);
    else
      groups.push({
        key,
        date: day,
        daysAgo: Math.round((today - day.getTime()) / 86400000),
        items: [item],
      });
  }
  return groups;
}

function time(item: TranscriptionItem, locale: string) {
  return normalizeDbDate(item.timestamp).toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function relativeLabel(
  group: DayGroup,
  locale: string,
  labels: { today: string; yesterday: string }
) {
  if (group.daysAgo === 0) return labels.today;
  if (group.daysAgo === 1) return labels.yesterday;
  const sameYear = group.date.getFullYear() === new Date().getFullYear();
  if (group.daysAgo < 7) return group.date.toLocaleDateString(locale, { weekday: "long" });
  return group.date.toLocaleDateString(locale, {
    month: "short",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  });
}

function RowText({ item, className }: { item: TranscriptionItem; className?: string }) {
  if (item.status === "failed")
    return <p className={cn("text-sm text-destructive", className)}>Transcription failed</p>;
  if (item.status === "discarded")
    return <p className={cn("text-sm text-muted-foreground", className)}>Discarded recording</p>;
  return (
    <p
      dir="auto"
      className={cn("text-base leading-normal whitespace-pre-wrap wrap-break-word", className)}
    >
      {item.text}
    </p>
  );
}

function CopyButton({ item, onCopy }: { item: TranscriptionItem; onCopy: (text: string) => void }) {
  return (
    <button
      onClick={() => onCopy(item.text)}
      aria-label="Copy"
      className="grid h-7 w-7 shrink-0 place-items-center rounded-full text-muted-foreground/70 opacity-0 transition-opacity group-hover/row:opacity-100 hover:bg-foreground/6 hover:text-foreground focus-visible:opacity-100"
    >
      <Copy size={13} />
    </button>
  );
}

interface VariantProps {
  items: TranscriptionItem[];
  locale: string;
  labels: { today: string; yesterday: string };
  actions: ReactNode;
  onCopy: (text: string) => void;
}

// A — Gutter: no cards; times sit in a left gutter, dates are a label with a hairline.
function VariantGutter({ items, locale, labels, actions, onCopy }: VariantProps) {
  return (
    <div className="group">
      {groupByDay(items).map((group, index) => (
        <section key={group.key} className={index > 0 ? "mt-8" : ""}>
          <div className="sticky -top-1 z-10 -mx-4 flex items-center gap-3 bg-background px-4 pt-2 pb-2">
            <span className="text-xs font-medium text-muted-foreground">
              {relativeLabel(group, locale, labels)}
            </span>
            <span className="h-px flex-1 bg-border/70" />
            {index === 0 && actions}
          </div>
          {group.items.map((item) => (
            <div
              key={item.id}
              className="group/row -mx-3 flex gap-4 rounded-xl px-3 py-2.5 transition-colors hover:bg-muted/30 dark:hover:bg-white/3"
            >
              <span className="w-16 shrink-0 whitespace-nowrap pt-[3px] text-xs tabular-nums text-muted-foreground/70">
                {time(item, locale)}
              </span>
              <RowText item={item} className="min-w-0 flex-1" />
              <CopyButton item={item} onCopy={onCopy} />
            </div>
          ))}
        </section>
      ))}
    </div>
  );
}

// B — Journal: the date is a sticky left column; entries flow on the right with trailing times.
function VariantJournal({ items, locale, labels, actions, onCopy }: VariantProps) {
  return (
    <div className="group">
      <div className="flex justify-end pb-2">{actions}</div>
      {groupByDay(items).map((group, index) => {
        const relative = relativeLabel(group, locale, labels);
        const absolute = group.date.toLocaleDateString(locale, { month: "short", day: "numeric" });
        return (
          <section
            key={group.key}
            className={cn(
              "grid grid-cols-[120px_1fr] gap-6",
              index > 0 && "mt-4 border-t border-border/60 pt-4"
            )}
          >
            <div className="sticky top-3 self-start pt-2.5">
              <div className="text-sm font-medium text-foreground/90">{relative}</div>
              {relative !== absolute && (
                <div className="mt-0.5 text-xs text-muted-foreground/70">{absolute}</div>
              )}
            </div>
            <div className="divide-y divide-border/50">
              {group.items.map((item) => (
                <div key={item.id} className="group/row flex items-start gap-4 py-2.5">
                  <RowText item={item} className="min-w-0 flex-1" />
                  <div className="flex shrink-0 items-center gap-1 pt-[3px]">
                    <CopyButton item={item} onCopy={onCopy} />
                    <span className="w-16 whitespace-nowrap text-end text-xs tabular-nums text-muted-foreground/60">
                      {time(item, locale)}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );
}

function streamTime(item: TranscriptionItem, group: DayGroup, locale: string) {
  if (group.daysAgo !== 0) return time(item, locale);
  const minutes = Math.round((Date.now() - normalizeDbDate(item.timestamp).getTime()) / 60000);
  const rtf = new Intl.RelativeTimeFormat(locale, { style: "narrow", numeric: "auto" });
  if (minutes < 60) return rtf.format(-Math.max(minutes, 0), "minute");
  return rtf.format(-Math.floor(minutes / 60), "hour");
}

// C — Stream: one continuous list. Today needs no header; later days are a quiet centered divider.
function VariantStream({ items, locale, labels, actions, onCopy }: VariantProps) {
  const groups = groupByDay(items);
  return (
    <div className="group">
      <div className="flex justify-end pb-1">{actions}</div>
      {groups.map((group, index) => (
        <section key={group.key}>
          {(index > 0 || group.daysAgo !== 0) && (
            <div className="flex items-center gap-3 py-5">
              <span className="h-px flex-1 bg-border/60" />
              <span className="text-[11px] tracking-wide text-muted-foreground/70">
                {relativeLabel(group, locale, labels)}
              </span>
              <span className="h-px flex-1 bg-border/60" />
            </div>
          )}
          <div className="space-y-1">
            {group.items.map((item) => (
              <div
                key={item.id}
                className="group/row -mx-3 flex items-start gap-3 rounded-xl px-3 py-3 transition-colors hover:bg-muted/30 dark:hover:bg-white/3"
              >
                <RowText item={item} className="min-w-0 flex-1" />
                <CopyButton item={item} onCopy={onCopy} />
                <span className="shrink-0 whitespace-nowrap pt-[3px] text-xs tabular-nums text-muted-foreground/60">
                  {streamTime(item, group, locale)}
                </span>
              </div>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

// D — Compact: one line per dictation, click to expand. Headers carry a count.
function VariantCompact({ items, locale, labels, actions, onCopy }: VariantProps) {
  const [expanded, setExpanded] = useState<number | null>(null);
  return (
    <div className="group">
      {groupByDay(items).map((group, index) => (
        <section key={group.key} className={index > 0 ? "mt-5" : ""}>
          <div className="sticky -top-1 z-10 -mx-4 flex items-center justify-between bg-background px-4 pt-2 pb-1.5">
            <span className="text-xs text-muted-foreground">
              <span className="font-medium text-foreground/80">
                {relativeLabel(group, locale, labels)}
              </span>
              <span className="mx-1.5 text-muted-foreground/50">·</span>
              {group.items.length}
            </span>
            {index === 0 && actions}
          </div>
          {group.items.map((item) => {
            const open = expanded === item.id;
            return (
              <div
                key={item.id}
                onClick={() => setExpanded(open ? null : item.id)}
                className="group/row -mx-3 flex cursor-default items-start gap-4 rounded-lg px-3 py-1.5 transition-colors hover:bg-muted/30 dark:hover:bg-white/3"
              >
                <span className="w-16 shrink-0 whitespace-nowrap pt-[2px] text-xs tabular-nums text-muted-foreground/60">
                  {time(item, locale)}
                </span>
                <RowText
                  item={item}
                  className={cn("min-w-0 flex-1 text-sm", !open && "truncate whitespace-nowrap")}
                />
                <div onClick={(event) => event.stopPropagation()} className="-my-1">
                  <CopyButton item={item} onCopy={onCopy} />
                </div>
              </div>
            );
          })}
        </section>
      ))}
    </div>
  );
}

export function PrototypeHistoryVariant({
  variant,
  ...props
}: VariantProps & { variant: PrototypeVariantKey }) {
  if (variant === "A") return <VariantGutter {...props} />;
  if (variant === "B") return <VariantJournal {...props} />;
  if (variant === "C") return <VariantStream {...props} />;
  if (variant === "D") return <VariantCompact {...props} />;
  return null;
}

// Sample rows for previewing in a plain browser, where there is no Electron history.
function localStamp(daysAgo: number, hours: number, minutes: number) {
  const now = new Date();
  const date = new Date(now.getFullYear(), now.getMonth(), now.getDate() - daysAgo, hours, minutes);
  return date.toISOString().replace("T", " ").slice(0, 19);
}

function minutesAgo(minutes: number) {
  return new Date(Date.now() - minutes * 60000).toISOString().replace("T", " ").slice(0, 19);
}

const SAMPLE_ROWS: [string, string, TranscriptionItem["status"]?][] = [
  [
    minutesAgo(4),
    "Speak your thoughts. Whisper turns your voice into clear text, adds punctuation, and pastes it into the app you are using.",
  ],
  [minutesAgo(38), "今天的计划：先把界面设计收尾，然后看一下用户反馈，最后更新项目文档。"],
  [
    minutesAgo(131),
    "For next week's demo, cover voice input, the custom dictionary, and audio or video file transcription. Prepare a few examples in advance.",
  ],
  [
    localStamp(1, 16, 24),
    "The goal is to make writing feel natural. Keep the original meaning, remove repetition, and make the text easy to read.",
  ],
  [localStamp(1, 11, 5), "", "failed"],
  [localStamp(1, 9, 47), "帮我回复一下张珊：周四下午三点可以，会议室我来订。"],
  [
    localStamp(3, 20, 12),
    "Reminder: the release checklist needs a smoke test after the Homebrew upgrade, not just the signature check.",
  ],
  [localStamp(9, 14, 30), "把字典里的专有名词再整理一遍，尤其是产品名和人名，避免识别成同音字。"],
  [localStamp(9, 10, 2), "Short one."],
  [
    localStamp(26, 18, 40),
    "Draft for the blog post: dictation should feel like thinking out loud, and the text should already be clean by the time it lands in your editor.",
  ],
];

export const PROTOTYPE_SAMPLE_HISTORY: TranscriptionItem[] = SAMPLE_ROWS.map(
  ([timestamp, text, status], index) => ({
    id: 10_000 + index,
    text,
    raw_text: null,
    timestamp,
    created_at: timestamp,
    has_audio: 0,
    audio_duration_ms: null,
    provider: null,
    model: null,
    status: status ?? "completed",
    error_message: status === "failed" ? "Connection refused" : null,
    error_code: null,
    route_kind: null,
    client_transcription_id: `prototype-${index}`,
    cloud_id: null,
    sync_status: "synced",
    deleted_at: null,
  })
);
