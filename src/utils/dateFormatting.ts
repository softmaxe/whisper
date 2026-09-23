export function normalizeDbDate(dateStr: string): Date {
  if (typeof dateStr !== "string" || !dateStr.trim()) return new Date(NaN);
  const trimmed = dateStr.trim();
  const hasExplicitZone = /(?:Z|[+-]\d{2}:?\d{2})$/i.test(trimmed);
  const source = hasExplicitZone ? trimmed : `${trimmed}Z`;
  return new Date(source);
}

export function formatShortDate(dateStr: string, locale?: string): string {
  if (!dateStr) return "";
  const date = normalizeDbDate(dateStr);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleDateString(locale, { month: "short", day: "numeric" });
}

export function formatRelativeTime(
  dateStr: string,
  t: (key: string, options?: Record<string, unknown>) => string,
  locale?: string
): string {
  if (!dateStr) return "";
  const date = normalizeDbDate(dateStr);
  if (Number.isNaN(date.getTime())) return "";
  const diff = Date.now() - date.getTime();
  if (diff < 0) return formatShortDate(dateStr, locale);
  const minutes = Math.floor(diff / 60000);
  const hours = Math.floor(diff / 3600000);
  const days = Math.floor(diff / 86400000);
  if (minutes < 1) return t("notes.list.timeNow");
  if (minutes < 60) return t("notes.list.minutesAgo", { count: minutes });
  if (hours < 24) return t("notes.list.hoursAgo", { count: hours });
  if (days < 7) return t("notes.list.daysAgo", { count: days });
  return formatShortDate(dateStr, locale);
}

export function formatDateGroup(
  date: Date | string,
  t: (key: string) => string,
  locale?: string
): string {
  if (!date) return "";
  const d = typeof date === "string" ? normalizeDbDate(date) : date;
  if (!(d instanceof Date) || Number.isNaN(d.getTime())) return "";
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const target = new Date(d.getFullYear(), d.getMonth(), d.getDate());

  if (target.getTime() === today.getTime()) return t("controlPanel.history.dateGroups.today");
  if (target.getTime() === yesterday.getTime())
    return t("controlPanel.history.dateGroups.yesterday");
  return d.toLocaleDateString(locale, { month: "short", day: "numeric", year: "numeric" });
}
