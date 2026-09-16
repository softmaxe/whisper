import enPrompts from "./en/prompts.json";
import zhCNPrompts from "./zh-CN/prompts.json";

export interface PromptBundle {
  cleanupPrompt: string;
  fullPrompt: string;
  dictionarySuffix: string;
  screenContextSuffix: string;
  translatePrompt: string;
}

export const en: PromptBundle = enPrompts;
export const zhCN: PromptBundle = zhCNPrompts;

export const PROMPTS_BY_LOCALE = {
  en,
  "zh-CN": zhCN,
} as const;
