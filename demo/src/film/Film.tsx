import { AbsoluteFill, interpolate, Sequence, useCurrentFrame, useVideoConfig } from "remotion";
import { FILM, toFrame } from "../../timeline/index.ts";
import type { Lang } from "../../timeline/types.ts";
import { clamp } from "./anim.ts";
import { BEAT_COMPONENTS } from "./beats/index.ts";
import { Captions } from "./components/Captions.tsx";
import { Paper } from "./components/Paper.tsx";
import { Wash } from "./components/Wash.tsx";
import { LangProvider } from "./lang.tsx";

export type FilmProps = { lang: Lang };

/**
 * The whole Film in one language: static paper at the bottom, the time-of-day
 * wash over it, each Beat's visuals in its own Sequence, Captions on top, and
 * a closing fade back to blank paper.
 */
export const Film: React.FC<FilmProps> = ({ lang }) => {
  const frame = useCurrentFrame();
  const { durationInFrames } = useVideoConfig();
  const fadeFrames = toFrame(FILM.fadeOutSeconds);
  const contentOpacity = interpolate(frame, [durationInFrames - fadeFrames, durationInFrames - 1], [1, 0], clamp);

  return (
    <LangProvider lang={lang}>
      <AbsoluteFill>
        <Paper />
        <Wash />
        <AbsoluteFill style={{ opacity: contentOpacity }}>
          {FILM.beats.map((beat) => {
            const BeatVisuals = BEAT_COMPONENTS[beat.key];
            return (
              <Sequence
                key={beat.key}
                name={`Beat ${beat.index} · ${beat.title}`}
                from={toFrame(beat.start)}
                durationInFrames={toFrame(beat.end) - toFrame(beat.start)}
              >
                <BeatVisuals />
              </Sequence>
            );
          })}
          <Captions />
        </AbsoluteFill>
      </AbsoluteFill>
    </LangProvider>
  );
};
