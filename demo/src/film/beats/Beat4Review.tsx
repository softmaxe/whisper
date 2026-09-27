import { beat4 } from "../../../timeline/beats/beat4-review.ts";
import { PALETTE } from "../theme.ts";
import { PlaceholderBeat } from "./Placeholder.tsx";

const m = beat4.moments;
const BARS = [120, 220, 160, 300, 260, 380, 200];

/** Beat 4 · The day in review (placeholder): a History list and an activity chart. */
export const Beat4Review: React.FC = () => (
  <PlaceholderBeat
    beat={beat4}
    sketches={[
      {
        at: m.search,
        seed: 41,
        build: (g, o) => [
          g.rectangle(260, 180, 620, 560, { ...o, fill: PALETTE.whitePaper, fillStyle: "solid" }),
          ...[0, 1, 2, 3, 4].map((i) => g.line(300, 260 + i * 100, 820 - (i % 2) * 160, 260 + i * 100, { ...o, strokeWidth: 2 })),
        ],
      },
      {
        at: m.insights,
        seed: 42,
        draw: 1.6,
        build: (g, o) => [
          g.line(1040, 720, 1680, 720, o),
          ...BARS.map((h, i) =>
            g.rectangle(1070 + i * 86, 720 - h, 56, h, { ...o, fill: PALETTE.pencil, fillStyle: "hachure", hachureGap: 8 }),
          ),
        ],
      },
    ]}
  />
);
