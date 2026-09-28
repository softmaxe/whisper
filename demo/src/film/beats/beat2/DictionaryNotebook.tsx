import { ramp } from "../../anim.ts";
import type { Box } from "../../chrome/AppWindow.tsx";
import { HAND_FONT } from "../../fonts.ts";
import { PenArrow, PenNote, PenStrike, PenTick } from "../../pen/RedPen.tsx";
import { RoughDrawing, useWobbleSeed } from "../../rough/RoughDrawing.tsx";
import { PALETTE } from "../../theme.ts";

/** Faint blue pencil for the notebook's ruled lines. */
const RULE = "#9fb3cf";
const RULE_GAP = 72;

export interface DictionaryNotebookProps {
  box: Box;
  /** The notebook's title: the app's Dictionary label. */
  title: string;
  /** The misheard word, pencilled then struck out. */
  wrong: string;
  /** The corrected word, written in red pen. */
  right: string;
  /** 0..1 sketch-in of the notebook and its title. */
  progress: number;
  /** 0..1 writing of the entry: the misheard word, its strike, the arrow and the correction. */
  entry: number;
  /** 0..1 drawing of the tick. */
  tick: number;
  seed: number;
}

/**
 * A spiral-bound notebook for the Dictionary, sketched in pencil on the
 * paper. Its entry records a correction: the misheard word pencilled and
 * struck out, a red arrow, the right word in red pen, and a tick. Renders an
 * SVG <g>; place it inside a full-canvas <svg>.
 */
export const DictionaryNotebook: React.FC<DictionaryNotebookProps> = ({ box, title, wrong, right, progress, entry, tick, seed }) => {
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  const { x, y, w, h } = box;
  const rings = 7;
  const firstRule = y + 176;
  const rules = Math.floor((y + h - 30 - firstRule) / RULE_GAP) + 1;
  const left = x + 70;
  const row1 = firstRule - 14;
  const row2 = firstRule + RULE_GAP - 14;
  const seg = (a: number, b: number) => ramp(entry, a, b);
  const wrongWidth = [...wrong].reduce((sum, ch) => sum + (ch.charCodeAt(0) > 0x2e80 ? 1 : 0.5), 0) * 46;
  return (
    <g transform={`rotate(-2 ${x + w / 2} ${y + h / 2})`}>
      <rect x={x + 3} y={y + 3} width={w - 6} height={h - 6} fill={PALETTE.whitePaper} opacity={Math.min(1, progress * 1.5)} />
      <RoughDrawing
        seed={boil}
        progress={progress}
        options={{ stroke: PALETTE.graphite, strokeWidth: 2.8, roughness: 1.2, bowing: 0.9 }}
        deps={[x, y, w, h, rules]}
        build={(g, o) => [
          g.rectangle(x, y, w, h, o),
          // A hatched pencil shadow down the right and along the bottom.
          g.polygon(
            [
              [x + w, y + 12],
              [x + w + 12, y + 12],
              [x + w + 12, y + h + 12],
              [x + 12, y + h + 12],
              [x + 12, y + h],
              [x + w, y + h],
            ],
            { ...o, stroke: "none", fill: PALETTE.pencil, fillStyle: "hachure", hachureGap: 7, hachureAngle: -52, fillWeight: 1.3 },
          ),
          ...Array.from({ length: rings }, (_, i) => g.ellipse(x + 50 + (i * (w - 100)) / (rings - 1), y, 20, 34, { ...o, strokeWidth: 2.2 })),
          ...Array.from({ length: rules }, (_, i) =>
            g.line(x + 22, firstRule + i * RULE_GAP, x + w - 22, firstRule + i * RULE_GAP, { ...o, stroke: RULE, strokeWidth: 1.6, roughness: 0.6 }),
          ),
          g.line(x + 52, y + 36, x + 52, y + h - 16, { ...o, stroke: "#d9938a", strokeWidth: 1.6, roughness: 0.6 }),
        ]}
      />
      <text
        x={x + w / 2}
        y={y + 92}
        textAnchor="middle"
        fontFamily={`"${HAND_FONT}", serif`}
        fontSize={52}
        fill={PALETTE.graphite}
        opacity={ramp(progress, 0.5, 1)}
      >
        {title}
      </text>
      <PenNote x={left} y={row1} text={wrong} progress={seg(0, 0.3)} fontSize={46} rotate={0} color={PALETTE.graphite} />
      <PenStrike x={left} y={row1 - 15} width={wrongWidth} progress={seg(0.3, 0.45)} seed={seed + 1} options={{ strokeWidth: 4 }} />
      <PenArrow from={[left + 4, row1 + 14]} to={[left + 70, row2 - 16]} bend={24} head={16} progress={seg(0.45, 0.62)} seed={seed + 2} />
      <PenNote x={left + 92} y={row2} text={right} progress={seg(0.6, 1)} fontSize={56} rotate={-2} />
      <PenTick x={left + 92 + right.length * 31 + 52} y={row2 - 20} size={66} progress={tick} seed={seed + 3} />
    </g>
  );
};
