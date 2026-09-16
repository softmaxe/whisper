const i18next = require("i18next");

const enTranslation = require("../locales/en/translation.json");
const zhCNTranslation = require("../locales/zh-CN/translation.json");

const enPrompts = require("../locales/en/prompts.json");
const zhCNPrompts = require("../locales/zh-CN/prompts.json");

const SUPPORTED_UI_LANGUAGES = ["en", "zh-CN"];

function normalizeUiLanguage(language) {
  // Keep Chinese OS/browser tags and legacy preferences on Simplified Chinese.
  // Keep this normalization in sync between the renderer and main process.
  const normalized = (language || "").trim().replace(/_/g, "-").toLowerCase();
  return normalized === "zh" || normalized.startsWith("zh-") ? "zh-CN" : "en";
}

const i18nMain = i18next.createInstance();

void i18nMain.init({
  initAsync: false,
  resources: {
    en: {
      translation: enTranslation,
      prompts: enPrompts,
    },
    "zh-CN": {
      translation: zhCNTranslation,
      prompts: zhCNPrompts,
    },
  },
  lng: normalizeUiLanguage(process.env.UI_LANGUAGE),
  fallbackLng: "en",
  supportedLngs: [...SUPPORTED_UI_LANGUAGES],
  ns: ["translation", "prompts"],
  defaultNS: "translation",
  interpolation: {
    escapeValue: false,
  },
  returnEmptyString: false,
  returnNull: false,
});

function changeLanguage(language) {
  const normalized = normalizeUiLanguage(language);

  if (i18nMain.language !== normalized) {
    void i18nMain.changeLanguage(normalized);
  }

  return normalized;
}

module.exports = {
  i18nMain,
  changeLanguage,
  normalizeUiLanguage,
  SUPPORTED_UI_LANGUAGES,
};
