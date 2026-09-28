import { createContext, useContext } from "react";
import type { Lang } from "../../timeline/types.ts";
import { COPY } from "../copy.ts";
import { CopyContext } from "../lib/copy-context.tsx";

const LangContext = createContext<Lang>("en");

/** The cut being rendered; Captions pick their text by it. */
export const useLang = (): Lang => useContext(LangContext);

/** Provides the cut's language, and its app copy through `useCopy()`. */
export const LangProvider: React.FC<{ lang: Lang; children: React.ReactNode }> = ({ lang, children }) => (
  <LangContext.Provider value={lang}>
    <CopyContext.Provider value={COPY[lang]}>{children}</CopyContext.Provider>
  </LangContext.Provider>
);
