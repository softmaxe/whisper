import { Composition } from "remotion";
import { FILM, LANGS, toFrame } from "../../timeline/index.ts";
import { WhisSheet, WHIS_SHEET_FRAMES } from "./characters/WhisSheet.tsx";
import { FILM_COMPOSITION_IDS, WHIS_SHEET } from "./compositionIds.ts";
import { Film } from "./Film.tsx";
import { loadFonts } from "./fonts.ts";

loadFonts();

/**
 * The hand-drawn Film compositions: one per cut, sharing the timeline.
 * WhisSheet is the mascot model sheet; the build exports one still of it.
 */
export const FilmCompositions: React.FC = () => (
  <>
    {LANGS.map((lang) => (
      <Composition
        key={lang}
        id={FILM_COMPOSITION_IDS[lang]}
        component={Film}
        durationInFrames={toFrame(FILM.durationSeconds)}
        fps={FILM.fps}
        width={FILM.width}
        height={FILM.height}
        defaultProps={{ lang }}
      />
    ))}
    <Composition
      id={WHIS_SHEET.id}
      component={WhisSheet}
      durationInFrames={WHIS_SHEET_FRAMES}
      fps={FILM.fps}
      width={FILM.width}
      height={FILM.height}
    />
  </>
);
