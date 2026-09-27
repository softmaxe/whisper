import { beat5 } from "../../../timeline/beats/beat5-servers.ts";
import { PALETTE } from "../theme.ts";
import { PlaceholderBeat } from "./Placeholder.tsx";

const m = beat5.moments;

/** Beat 5 · Your servers (placeholder): a house outline around the Mac and two servers, then the logo. */
export const Beat5Servers: React.FC = () => (
  <PlaceholderBeat
    beat={beat5}
    sketches={[
      {
        at: m.house,
        seed: 51,
        draw: 1.6,
        build: (g, o) => [
          g.polygon(
            [
              [420, 380],
              [960, 130],
              [1500, 380],
              [1500, 760],
              [420, 760],
            ],
            o,
          ),
          g.rectangle(560, 470, 320, 210, { ...o, fill: PALETTE.cream, fillStyle: "hachure", hachureGap: 10 }),
          g.rectangle(1060, 440, 300, 110, { ...o, fill: PALETTE.pencil, fillStyle: "hachure", hachureGap: 9 }),
          g.rectangle(1060, 590, 300, 110, { ...o, fill: PALETTE.pencil, fillStyle: "hachure", hachureGap: 9 }),
        ],
      },
      {
        at: m.logo,
        seed: 52,
        draw: 0.8,
        options: { stroke: PALETTE.whisBlack },
        build: (g, o) => [g.rectangle(880, 260, 80, 80, { ...o, fill: PALETTE.whisBlack, fillStyle: "solid" })],
      },
    ]}
  />
);
