import { Composition } from "remotion";
import { FILM, toFrame } from "../../timeline/index.ts";
import { Film } from "./Film.tsx";
import { loadFonts } from "./fonts.ts";

loadFonts();

/**
 * The hand-drawn Film compositions: one per cut, sharing the timeline.
 * Composition ids are listed in scripts/paths.ts for the build.
 */
export const FilmCompositions: React.FC = () => (
  <>
    <Composition
      id="Film-en"
      component={Film}
      durationInFrames={toFrame(FILM.durationSeconds)}
      fps={FILM.fps}
      width={FILM.width}
      height={FILM.height}
      defaultProps={{ lang: "en" as const }}
    />
    <Composition
      id="Film-zh"
      component={Film}
      durationInFrames={toFrame(FILM.durationSeconds)}
      fps={FILM.fps}
      width={FILM.width}
      height={FILM.height}
      defaultProps={{ lang: "zh-CN" as const }}
    />
  </>
);
