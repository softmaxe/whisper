import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { PROMPTS_BY_LOCALE } from "./locales/prompts";
import { TRANSLATIONS_BY_LOCALE } from "./locales/translations";

export const SUPPORTED_UI_LANGUAGES = ["en", "zh-CN"] as const;
export type UiLanguage = (typeof SUPPORTED_UI_LANGUAGES)[number];

export function normalizeUiLanguage(language: string | null | undefined): UiLanguage {
  // Keep Chinese OS/browser tags and legacy preferences on Simplified Chinese.
  // Keep this normalization in sync between the renderer and main process.
  const normalized = (language || "").trim().replace(/_/g, "-").toLowerCase();
  return normalized === "zh" || normalized.startsWith("zh-") ? "zh-CN" : "en";
}

const resources = {
  en: {
    translation: TRANSLATIONS_BY_LOCALE.en,
    prompts: PROMPTS_BY_LOCALE.en,
  },
  "zh-CN": {
    translation: TRANSLATIONS_BY_LOCALE["zh-CN"],
    prompts: PROMPTS_BY_LOCALE["zh-CN"],
  },
} as const;

const browserLanguage =
  typeof navigator !== "undefined" ? navigator.language || navigator.languages?.[0] : undefined;

const storageLanguage =
  typeof window !== "undefined" ? window.localStorage?.getItem("uiLanguage") : undefined;

const initialLanguage = normalizeUiLanguage(storageLanguage || browserLanguage || "en");

void i18n.use(initReactI18next).init({
  resources,
  lng: initialLanguage,
  fallbackLng: "en",
  supportedLngs: [...SUPPORTED_UI_LANGUAGES],
  ns: ["translation", "prompts"],
  defaultNS: "translation",
  interpolation: {
    escapeValue: false,
  },
  returnEmptyString: true,
  returnNull: false,
});

export default i18n;
