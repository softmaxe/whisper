import { beat3 } from "../../../timeline/beats/beat3-more.ts";
import { PALETTE } from "../theme.ts";
import { PlaceholderBeat } from "./Placeholder.tsx";

const NOTE_X = [330, 810, 1290];

/** Beat 3 · More ways to use it (placeholder): three pinned notes sketched in turn. */
export const Beat3More: React.FC = () => (
  <PlaceholderBeat
    beat={beat3}
    sketches={beat3.moments.notes.map((at, i) => ({
      at,
      seed: 31 + i,
      build: (g, o) => [
        g.rectangle(NOTE_X[i], 200, 300, 360, {
          ...o,
          fill: PALETTE.whitePaper,
          fillStyle: "hachure",
          hachureGap: 12,
          hachureAngle: 60,
          fillWeight: 1,
        }),
        g.circle(NOTE_X[i] + 150, 215, 26, { ...o, stroke: PALETTE.redPen, fill: PALETTE.redPen, fillStyle: "solid" }),
      ],
    }))}
  />
);
