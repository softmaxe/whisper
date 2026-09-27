import type { Seconds } from "../../../timeline/types.ts";
import { ramp } from "../anim.ts";
import { RoughDrawing, useWobbleSeed } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";

/** How long a tapped key stays down, then how long it takes to spring back. */
const DOWN: Seconds = 0.12;
const UP: Seconds = 0.1;
/** Keycap legends use the Mac's system face, like the real key. */
const KEY_FONT = '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif';

/**
 * 0..1 depth of a key at Film time `t` for presses at `taps` (e.g. the `taps`
 * of doubleTapMoments). With `release`, the key is held from the first press
 * until then (Hold mode) and springs back after.
 */
export function keyDepth(t: Seconds, taps: readonly Seconds[], release?: Seconds): number {
  if (release !== undefined && taps.length > 0 && t >= taps[0]) {
    return t < release ? 1 : 1 - ramp(t, release, release + UP);
  }
  let depth = 0;
  for (const at of taps) {
    if (t < at || t >= at + DOWN + UP) continue;
    depth = Math.max(depth, t < at + DOWN ? 1 : 1 - (t - at - DOWN) / UP);
  }
  return depth;
}

export interface FnKeyProps {
  /** Top-left of the key cap at rest, in the parent SVG's coordinates. */
  x: number;
  y: number;
  /** Key width in px (default 70); the key is 0.86 as tall. */
  size?: number;
  /** 0..1 press depth, e.g. keyDepth(t, m.taps). */
  depth?: number;
  /** 0..1 sketch-in (default 1). */
  progress?: number;
  seed?: number;
}

/**
 * The Mac's fn/Globe key, hand-drawn: a pencil key cap with a front skirt,
 * "fn" at the top right and the Globe glyph at the bottom left. It sinks and
 * darkens as `depth` goes to 1. Renders an SVG <g>.
 */
export const FnKey: React.FC<FnKeyProps> = ({ x, y, size = 70, depth = 0, progress = 1, seed = 71 }) => {
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  const w = size;
  const h = size * 0.86;
  const skirt = size * 0.1;
  const sink = depth * skirt * 0.8;
  const pencil = { stroke: PALETTE.graphite, strokeWidth: Math.max(2, size / 28), roughness: 0.8, bowing: 0.6 };
  const legend = depth > 0.5 ? PALETTE.ink : PALETTE.graphite;
  const g = size / 24; // Globe glyph scale (24-unit viewBox).
  return (
    <g>
      {/* The key's front face, visible below the cap until it is pressed down. */}
      <rect x={x} y={y + h - 4} width={w} height={skirt + 4 - sink} rx={size * 0.12} fill={PALETTE.paperShade} />
      <g transform={`translate(0 ${sink})`}>
        <rect x={x} y={y} width={w} height={h} rx={size * 0.12} fill={depth > 0 ? PALETTE.paperShade : PALETTE.whitePaper} />
        <RoughDrawing
          seed={boil}
          progress={progress}
          options={pencil}
          deps={[x, y, size]}
          build={(r, o) => [
            r.path(roundedRect(x, y, w, h, size * 0.12), o),
            // The Globe glyph (as in the old demo's key cap), bottom left.
            r.circle(x + size * 0.12 + 12 * g * 0.5, y + h - size * 0.12 - 12 * g * 0.5, 18 * g * 0.5, {
              ...o,
              strokeWidth: pencil.strokeWidth * 0.8,
              roughness: 0.5,
            }),
          ]}
        />
        <path
          transform={`translate(${x + size * 0.12} ${y + h - size * 0.12 - 12 * g}) scale(${g * 0.5})`}
          d="M3 12h18M12 3c2.6 2.6 3.6 5.6 3.6 9s-1 6.4-3.6 9c-2.6-2.6-3.6-5.6-3.6-9s1-6.4 3.6-9z"
          fill="none"
          stroke={legend}
          strokeWidth={1.6}
          opacity={progress}
        />
        <text
          x={x + w - size * 0.14}
          y={y + size * 0.34}
          textAnchor="end"
          fontFamily={KEY_FONT}
          fontSize={size * 0.3}
          fontWeight={500}
          fill={legend}
          opacity={progress}
        >
          fn
        </text>
      </g>
    </g>
  );
};

/** A rounded rectangle path (Rough.js draws it as one wobbly stroke). */
export function roundedRect(x: number, y: number, w: number, h: number, r: number): string {
  return `M ${x + r} ${y} H ${x + w - r} Q ${x + w} ${y} ${x + w} ${y + r} V ${y + h - r} Q ${x + w} ${y + h} ${x + w - r} ${y + h} H ${x + r} Q ${x} ${y + h} ${x} ${y + h - r} V ${y + r} Q ${x} ${y} ${x + r} ${y} Z`;
}
