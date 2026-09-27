import type { Options } from "roughjs/bin/core";
import { AbsoluteFill } from "remotion";
import type { Beat, Seconds } from "../../../timeline/types.ts";
import { ramp, useBeatTime } from "../anim.ts";
import { HAND_FONT } from "../fonts.ts";
import { RoughDrawing, useWobbleSeed, type RoughBuild } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";

/** One rough drawing sketched in from `at` over `draw` seconds, then wobbling gently. */
export interface Sketch {
  at: Seconds;
  seed: number;
  build: RoughBuild;
  options?: Options;
  /** Seconds to sketch it in (default 1.2). */
  draw?: Seconds;
}

interface Props {
  beat: Beat;
  sketches: Sketch[];
  /** HTML layered over the drawings (labels in the clear typeface, etc.). */
  children?: React.ReactNode;
}

/**
 * Placeholder Beat visuals for the tracer build: rough sketches revealed
 * stroke by stroke, a pencilled Beat label, and a fade in and out at the Beat
 * edges. Each Beat ticket replaces its Beat component and stops using this.
 */
export const PlaceholderBeat: React.FC<Props> = ({ beat, sketches, children }) => {
  const { t } = useBeatTime(beat);
  const wobble = useWobbleSeed(0);
  const opacity = ramp(t, beat.start, beat.start + 0.4) * (1 - ramp(t, beat.end - 0.4, beat.end));
  return (
    <AbsoluteFill style={{ opacity }}>
      <svg width={1920} height={1080} style={{ position: "absolute", inset: 0 }}>
        {sketches.map((s, i) => {
          const progress = ramp(t, s.at, s.at + (s.draw ?? 1.2));
          return (
            <RoughDrawing
              key={i}
              seed={progress < 1 ? s.seed : s.seed + wobble}
              build={s.build}
              options={{ stroke: PALETTE.graphite, strokeWidth: 3, ...s.options }}
              progress={progress}
            />
          );
        })}
      </svg>
      <div
        style={{
          position: "absolute",
          left: 80,
          top: 64,
          fontFamily: `"${HAND_FONT}", serif`,
          fontSize: 34,
          color: PALETTE.pencil,
          transform: "rotate(-2deg)",
        }}
      >
        {`Beat ${beat.index} · ${beat.title}`}
      </div>
      {children}
    </AbsoluteFill>
  );
};
