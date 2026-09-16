import enTranslation from "./en/translation.json";
import zhCNTranslation from "./zh-CN/translation.json";

export const TRANSLATIONS_BY_LOCALE = {
  en: enTranslation,
  "zh-CN": zhCNTranslation,
} as const;
