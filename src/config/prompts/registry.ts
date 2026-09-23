import { en as enPrompts, type PromptBundle } from "../../locales/prompts";

export const PROMPT_KINDS = {
  cleanup: {
    i18nKey: "cleanupPrompt" as const,
    fallback: enPrompts.cleanupPrompt,
  },
} as const satisfies Record<string, { i18nKey: keyof PromptBundle | null; fallback: string }>;

export type PromptKind = keyof typeof PROMPT_KINDS;
export const PROMPT_KIND_LIST = Object.keys(PROMPT_KINDS) as readonly PromptKind[];
