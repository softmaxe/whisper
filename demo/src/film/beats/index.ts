import type { BeatKey } from "../../../timeline/types.ts";
import { Beat1Opening } from "./Beat1Opening.tsx";
import { Beat2Speak } from "./Beat2Speak.tsx";
import { Beat3More } from "./Beat3More.tsx";
import { Beat4Review } from "./Beat4Review.tsx";
import { Beat5Servers } from "./Beat5Servers.tsx";

/**
 * Maps each timeline Beat to its visuals component (one file per Beat). Each
 * component imports its own Beat from `timeline/beats/` (typed moments) and is
 * mounted by the Film in a <Sequence> starting at the Beat's start, so its
 * `useCurrentFrame()` is local to the Beat; `useBeatTime(beat)` in ../anim.ts
 * gives the absolute Film time. Captions are drawn by the Film, not the Beat.
 */
export const BEAT_COMPONENTS: Record<BeatKey, React.FC> = {
  opening: Beat1Opening,
  speak: Beat2Speak,
  more: Beat3More,
  review: Beat4Review,
  servers: Beat5Servers,
};
