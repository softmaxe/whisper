import { hash01 } from "../../../../timeline/helpers.ts";
import { ramp } from "../../anim.ts";
import { RoughDrawing, useWobbleSeed } from "../../rough/RoughDrawing.tsx";
import { PALETTE } from "../../theme.ts";

/*
 * The app icon (resources/, 1024 px): a black rounded square from 100 to 924
 * with ~185 px corners, and a white waveform W of fifteen rounded bars
 * (centres 261 + 36·i, width ~24) standing on y = 715. Tops trace the W.
 */
const ICON = { size: 1024, left: 100, right: 924, radius: 185 };
const BAR_X0 = 261;
const BAR_STEP = 36;
const BAR_W = 24;
const BAR_BOTTOM = 715;
const BAR_TOPS = [377, 460, 543, 627, 653, 598, 545, 490, 545, 598, 653, 627, 543, 460, 385];

/** A rounded square as an SVG path (for Rough.js), in icon units. */
function roundedSquare(a: number, b: number, r: number): string {
  return [
    `M ${a + r} ${a}`,
    `L ${b - r} ${a}`,
    `Q ${b} ${a} ${b} ${a + r}`,
    `L ${b} ${b - r}`,
    `Q ${b} ${b} ${b - r} ${b}`,
    `L ${a + r} ${b}`,
    `Q ${a} ${b} ${a} ${b - r}`,
    `L ${a} ${a + r}`,
    `Q ${a} ${a} ${a + r} ${a}`,
    "Z",
  ].join(" ");
}

/**
 * The waveform W as one continuous pen path, in icon units: up each bar and
 * back down it, then a small loop under the baseline over to the next bar.
 * `jitter` shifts the points a little, like a hand redrawing it.
 */
export function waveformStroke(jitterSeed = 0, jitter = 0): string {
  const j = (k: number) => (jitter ? (hash01(jitterSeed * 97 + k) - 0.5) * jitter : 0);
  const parts: string[] = [];
  BAR_TOPS.forEach((top, i) => {
    const x = BAR_X0 + BAR_STEP * i;
    if (i === 0) parts.push(`M ${x + j(1)} ${BAR_BOTTOM}`);
    parts.push(`L ${x + j(i * 5 + 2)} ${top + BAR_W / 2 + j(i * 5 + 3)}`);
    parts.push(`L ${x + j(i * 5 + 4)} ${BAR_BOTTOM}`);
    if (i < BAR_TOPS.length - 1) {
      const next = x + BAR_STEP;
      parts.push(`Q ${x + BAR_STEP / 2} ${BAR_BOTTOM + 26 + j(i * 5 + 5)} ${next} ${BAR_BOTTOM}`);
    }
  });
  return parts.join(" ");
}

export interface LogoProps {
  /** Centre of the logo on the canvas. */
  cx: number;
  cy: number;
  /** Side of the icon's 1024 px canvas on screen, in px. */
  size: number;
  /** 0..1: the black rounded square (pencil outline, then crayon fill). */
  square: number;
  /** 0..1: the white W, drawn as one stroke. */
  stroke: number;
  seed?: number;
}

/**
 * The Whisper logo, hand-drawn: a pencilled black rounded square, then the
 * waveform W written in one continuous white stroke. It boils gently once
 * finished, like the rest of the pencil work.
 */
export const Logo: React.FC<LogoProps> = ({ cx, cy, size, square, stroke, seed = 71 }) => {
  const done = square >= 1 && stroke >= 1;
  const boil = useWobbleSeed(seed, 4, !done);
  if (square <= 0) return null;
  const k = size / ICON.size;
  const outline = roundedSquare(ICON.left, ICON.right, ICON.radius);
  const fill = ramp(square, 0.45, 1);
  return (
    <g transform={`translate(${cx - size / 2} ${cy - size / 2}) scale(${k})`}>
      {/* Paper rim, so the black square separates from a dark wash. */}
      <path d={outline} fill={PALETTE.paper} stroke={PALETTE.paper} strokeWidth={24} opacity={Math.min(1, square * 2)} />
      <path d={outline} fill={PALETTE.iconBlack} opacity={fill} />
      <RoughDrawing
        seed={boil}
        options={{ stroke: PALETTE.graphite, strokeWidth: 3.2 / k, roughness: 1.1, bowing: 0.8 }}
        progress={Math.min(1, square / 0.6)}
        build={(g, o) => [g.path(outline, o)]}
      />
      {stroke > 0 && (
        <path
          d={waveformStroke(boil, 5)}
          fill="none"
          stroke={PALETTE.iconWhite}
          strokeWidth={BAR_W + 4}
          strokeLinecap="round"
          strokeLinejoin="round"
          pathLength={1}
          strokeDasharray={stroke < 1 ? "1 1" : undefined}
          strokeDashoffset={stroke < 1 ? 1 - stroke : undefined}
        />
      )}
    </g>
  );
};
