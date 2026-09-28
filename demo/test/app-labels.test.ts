/**
 * The Film shows Whisper's own UI, so every app label in src/copy.ts must read
 * exactly as the app's locale files say, in both cuts.
 */
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { ROOT } from "../scripts/paths.ts";
import { COPY, type Copy } from "../src/copy.ts";
import { LANGS } from "../timeline/index.ts";
import type { Lang } from "../timeline/types.ts";

type Params = Record<string, string | number>;

/** Each app label: how to read it from the copy, its i18n key, and the parameters the app passes. */
const LABELS: { label: string; key: string; read: (copy: Copy) => string; params?: Params }[] = [
  { label: "nav.home", key: "sidebar.home", read: (c) => c.whisper.nav.home },
  { label: "nav.insights", key: "sidebar.insights", read: (c) => c.whisper.nav.insights },
  { label: "nav.upload", key: "sidebar.upload", read: (c) => c.whisper.nav.upload },
  { label: "nav.dictionary", key: "sidebar.dictionary", read: (c) => c.whisper.nav.dictionary },
  { label: "nav.settings", key: "sidebar.settings", read: (c) => c.whisper.nav.settings },
  { label: "search", key: "commandSearch.sections.transcripts", read: (c) => c.whisper.search },
  { label: "today", key: "controlPanel.history.dateGroups.today", read: (c) => c.whisper.today },
  { label: "showDiscarded", key: "controlPanel.history.discarded.show", read: (c) => c.whisper.showDiscarded },
  { label: "clearAll", key: "controlPanel.history.clearAll", read: (c) => c.whisper.clearAll },
  { label: "copied", key: "controlPanel.history.copiedTitle", read: (c) => c.whisper.copied },
  { label: "upload.title", key: "notes.upload.title", read: (c) => c.whisper.upload.title },
  { label: "upload.drop", key: "notes.upload.dropOrBrowse", read: (c) => c.whisper.upload.drop },
  { label: "upload.formats", key: "notes.upload.supportedFormats", read: (c) => c.whisper.upload.formats },
  {
    label: "upload.progress",
    key: "notes.upload.queueProgress",
    read: (c) => c.whisper.upload.progress(2, 3),
    params: { completed: 2, total: 3 },
  },
  { label: "upload.complete", key: "notes.upload.transcriptionComplete", read: (c) => c.whisper.upload.complete },
  { label: "upload.saved", key: "notes.upload.savedToHistory", read: (c) => c.whisper.upload.saved },
  { label: "insights.title", key: "insights.yourUsage", read: (c) => c.whisper.insights.title },
  { label: "insights.words", key: "insights.wordsSpoken", read: (c) => c.whisper.insights.words },
  { label: "insights.streak", key: "insights.currentStreak", read: (c) => c.whisper.insights.streak },
  { label: "insights.wpm", key: "insights.wordsPerMinute", read: (c) => c.whisper.insights.wpm },
  { label: "insights.dictations", key: "insights.dictations", read: (c) => c.whisper.insights.dictations },
  { label: "insights.allTime", key: "insights.allTime", read: (c) => c.whisper.insights.allTime },
  { label: "insights.days", key: "insights.days_other", read: (c) => c.whisper.insights.days(12), params: { count: 12 } },
  { label: "insights.activity", key: "insights.activity", read: (c) => c.whisper.insights.activity },
  { label: "insights.onDevice", key: "insights.onDevice", read: (c) => c.whisper.insights.onDevice },
  { label: "settings.speechToText", key: "settingsPage.speechToText.title", read: (c) => c.whisper.settings.speechToText },
  {
    label: "settings.shared",
    key: "settingsPage.speechToText.sharedDescription",
    read: (c) => c.whisper.settings.shared,
  },
  { label: "settings.endpoint", key: "settingsPage.selfHosted.serverUrl", read: (c) => c.whisper.settings.endpoint },
  { label: "settings.model", key: "common.model", read: (c) => c.whisper.settings.model },
  { label: "settings.cleanup", key: "settingsPage.llms.title", read: (c) => c.whisper.settings.cleanup },
  {
    label: "settings.enableCleanup",
    key: "settingsPage.aiModels.enableTextCleanup",
    read: (c) => c.whisper.settings.enableCleanup,
  },
  {
    label: "chat.toast",
    key: "app.toasts.addedToDict",
    read: (c) => c.chat.toast,
    // The app quotes each learned word (useMainProcessNotifications).
    params: { words: "“Supabase”" },
  },
];

function locale(lang: Lang): unknown {
  const file = path.join(ROOT, "..", "src", "locales", lang, "translation.json");
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

/** The app's string for `key`, with `{{name}}` placeholders filled like i18next. */
function appString(lang: Lang, key: string, params: Params = {}): string {
  const value = key.split(".").reduce<unknown>((node, part) => (node as Record<string, unknown>)?.[part], locale(lang));
  if (typeof value !== "string") throw new Error(`${lang}: no locale string at ${key}`);
  return value.replace(/\{\{\s*(\w+)\s*\}\}/g, (_, name: string) => {
    if (!(name in params)) throw new Error(`${lang}: ${key} needs {{${name}}}`);
    return String(params[name]);
  });
}

describe.each(LANGS)("app labels in the %s cut", (lang) => {
  it.each(LABELS)("$label matches $key", ({ key, read, params }) => {
    expect(read(COPY[lang])).toBe(appString(lang, key, params));
  });
});
