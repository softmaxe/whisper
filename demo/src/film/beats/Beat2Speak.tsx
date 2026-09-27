import { beat2 } from "../../../timeline/beats/beat2-speak.ts";
import { useCopy } from "../../lib/copy-context.tsx";
import { ramp, useBeatTime } from "../anim.ts";
import { PALETTE, RED_PEN } from "../theme.ts";
import { PlaceholderBeat } from "./Placeholder.tsx";

const m = beat2.moments;

/** Beat 2 · Speak and it's written (placeholder): a Mail window, a red-pen strike, pasted lines. */
export const Beat2Speak: React.FC = () => {
  const copy = useCopy();
  const { t } = useBeatTime(beat2);
  return (
    <PlaceholderBeat
      beat={beat2}
      sketches={[
        {
          at: m.mailIn,
          seed: 21,
          build: (g, o) => [
            g.rectangle(460, 150, 1000, 600, { ...o, fill: PALETTE.whitePaper, fillStyle: "solid" }),
            g.line(460, 210, 1460, 210, o),
            ...[0, 1, 2].map((i) => g.circle(495 + i * 30, 180, 16, o)),
          ],
        },
        { at: m.clean, seed: 22, draw: 0.5, options: RED_PEN, build: (g, o) => [g.line(540, 330, 700, 322, o)] },
        {
          at: m.paste,
          seed: 23,
          options: { stroke: PALETTE.pencil, strokeWidth: 2 },
          build: (g, o) => [0, 1, 2, 3].map((i) => g.line(540, 420 + i * 60, 1360 - (i % 2) * 220, 420 + i * 60, o)),
        },
        { at: m.correct, seed: 24, draw: 0.6, options: RED_PEN, build: (g, o) => [g.ellipse(1220, 330, 220, 90, o)] },
      ]}
    >
      <div
        style={{
          position: "absolute",
          left: 580,
          top: 162,
          fontFamily: copy.font,
          fontSize: 30,
          color: PALETTE.ink,
          opacity: ramp(t, m.mailIn + 0.6, m.mailIn + 1.2),
        }}
      >
        {copy.mail.app}
      </div>
    </PlaceholderBeat>
  );
};
