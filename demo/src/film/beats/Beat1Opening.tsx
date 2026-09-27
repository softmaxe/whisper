import { beat1 } from "../../../timeline/beats/beat1-opening.ts";
import { PALETTE, RED_PEN } from "../theme.ts";
import { PlaceholderBeat } from "./Placeholder.tsx";

const m = beat1.moments;

/** Beat 1 · Opening (placeholder): a keyboard, the fn key circled in red pen, the Recording pill. */
export const Beat1Opening: React.FC = () => (
  <PlaceholderBeat
    beat={beat1}
    sketches={[
      {
        at: beat1.start + 0.1,
        seed: 11,
        build: (g, o) => [
          g.rectangle(560, 380, 800, 300, { ...o, fill: PALETTE.cream, fillStyle: "hachure", hachureGap: 10, hachureAngle: -52 }),
          ...Array.from({ length: 3 }, (_, row) =>
            Array.from({ length: 10 }, (_, col) => g.rectangle(600 + col * 74, 420 + row * 74, 60, 58, o)),
          ).flat(),
          g.rectangle(600, 642, 60, 30, { ...o, strokeWidth: 4 }),
        ],
      },
      { at: m.fnCircle, seed: 12, draw: 0.6, options: RED_PEN, build: (g, o) => [g.ellipse(630, 657, 130, 80, o)] },
      {
        at: m.pill,
        seed: 13,
        draw: 0.5,
        options: { stroke: PALETTE.whisBlack },
        build: (g, o) => [
          g.rectangle(840, 220, 240, 80, { ...o, fill: PALETTE.whisBlack, fillStyle: "solid" }),
          ...Array.from({ length: 10 }, (_, i) =>
            g.line(880 + i * 18, 260 - 6 - (i % 3) * 6, 880 + i * 18, 260 + 6 + (i % 3) * 6, { ...o, stroke: PALETTE.whisWhite }),
          ),
        ],
      },
    ]}
  />
);
