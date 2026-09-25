import { createContext, useContext } from "react";
import { COPY, type Copy } from "../copy.ts";

export const CopyContext = createContext<Copy>(COPY.en);
export const useCopy = () => useContext(CopyContext);
