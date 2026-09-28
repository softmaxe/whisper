/**
 * Remotion composition ids, shared by the registry (FilmCompositions.tsx)
 * and the build (scripts/build.ts). Plain data so Node can import it.
 */
import type { Lang } from "../../timeline/types.ts";

/** The composition id of each cut. */
export const FILM_COMPOSITION_IDS: Record<Lang, string> = { en: "Film-en", "zh-CN": "Film-zh" };

/** The Clawd model sheet composition and the frame exported as its review still. */
export const CLAWD_SHEET = { id: "ClawdSheet", frame: 45 } as const;
