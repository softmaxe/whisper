/**
 * Red-pen annotations, drawn stroke by stroke: the Film's shared vocabulary
 * for pointing things out (PenCircle, PenStrike, PenTick, PenArrow, PenNote)
 * plus PenMark, which circles, strikes or underlines a run of HTML text.
 *
 * Common props (PenProps):
 * - `progress` 0..1 draws the mark: 0 = nothing (renders null), 1 = done.
 *   Drive it from timeline moments, e.g. `progress={ramp(t, m.fnCircle, m.fnCircle + 0.45)}`.
 *   Pair each mark with a `pen_scratch` sound cue at the same moment.
 * - `seed` fixes the mark's hand-drawn shape; use a distinct seed per mark.
 * - `wobble` (default true): once fully drawn, the stroke "boils" every 4
 *   frames like the rest of the pencil work. It is held still while drawing.
 * - `options` overrides the Rough.js stroke options (default RED_PEN from theme.ts).
 *
 * All but PenMark render an SVG <g> in the parent SVG's coordinates (usually
 * a full-canvas 1920x1080 <svg>), so place them inside one, after whatever
 * they annotate so they draw on top. PenMark is HTML and goes inline in text.
 */
import { useId, useLayoutEffect, useRef, useState } from "react";
import type { Options } from "roughjs/bin/core";
import { emWidth } from "../components/captionLayout.ts";
import { HAND_FONT } from "../fonts.ts";
import { RoughDrawing, useWobbleSeed, type RoughBuild } from "../rough/RoughDrawing.tsx";
import { PALETTE, RED_PEN } from "../theme.ts";

export type Point = readonly [x: number, y: number];

export interface PenProps {
  /** 0..1 stroke-by-stroke reveal; 0 renders nothing. */
  progress: number;
  /** Fixes the hand-drawn shape. Use a distinct seed per mark. */
  seed: number;
  /** Boil once fully drawn (default true). */
  wobble?: boolean;
  /** Rough.js options over RED_PEN (e.g. `{ strokeWidth: 7 }`). */
  options?: Options;
}

/** Shared renderer: the red pen, a still seed while drawing and a boil after. */
const PenStroke: React.FC<PenProps & { build: RoughBuild; deps: unknown[] }> = ({
  progress,
  seed,
  wobble = true,
  options,
  build,
  deps,
}) => {
  const boil = useWobbleSeed(seed, 4, !wobble || progress < 1);
  if (progress <= 0) return null;
  return <RoughDrawing seed={boil} options={{ ...RED_PEN, ...options }} progress={progress} build={build} deps={deps} />;
};

export interface PenCircleProps extends PenProps {
  /** Centre and radii of the loop, around the thing being circled. */
  cx: number;
  cy: number;
  rx: number;
  ry: number;
  /** How far round the loop goes, in turns (default 1.12: a quick circle that overshoots its start). */
  turns?: number;
}

/**
 * A quick red-pen loop: starts top-right, goes anticlockwise a little more
 * than once and widens slightly, like a hand circling a word.
 */
export const PenCircle: React.FC<PenCircleProps> = ({ cx, cy, rx, ry, turns = 1.12, ...pen }) => (
  <PenStroke
    {...pen}
    deps={[cx, cy, rx, ry, turns]}
    build={(g, o) => [g.curve(loopPoints(cx, cy, rx, ry, turns), { ...o, roughness: Math.min(o.roughness ?? 1, 0.6) })]}
  />
);

/** Points of an overshooting loop (shared by PenCircle and PenMark). */
function loopPoints(cx: number, cy: number, rx: number, ry: number, turns: number): [number, number][] {
  const n = 15;
  return Array.from({ length: n }, (_, i) => {
    const a = -0.9 - (i / (n - 1)) * turns * 2 * Math.PI;
    const grow = 1 + 0.07 * (i / (n - 1));
    return [cx + Math.cos(a) * rx * grow, cy + Math.sin(a) * ry * grow];
  });
}

export interface PenStrikeProps extends PenProps {
  /** Left end and vertical centre of the text being struck, and its width. */
  x: number;
  y: number;
  width: number;
  /** 1 = a single strike-through (default), 2 = out and back, crossing it out firmly. */
  passes?: 1 | 2;
  /** Rise in px from the left end to the right end (default 3, a slight upward slant). */
  rise?: number;
}

/** A red-pen strike-through across text, overshooting both ends a little. */
export const PenStrike: React.FC<PenStrikeProps> = ({ x, y, width, passes = 1, rise = 3, ...pen }) => (
  <PenStroke {...pen} deps={[x, y, width, passes, rise]} build={(g, o) => strikeCurves(g, o, x, y, width, passes, rise)} />
);

function strikeCurves(
  g: Parameters<RoughBuild>[0],
  o: Options,
  x: number,
  y: number,
  width: number,
  passes: 1 | 2,
  rise: number,
) {
  const over = Math.min(12, width * 0.08);
  const line = (y0: number, reverse: boolean) => {
    const pts: [number, number][] = [0, 0.35, 0.7, 1].map((f, i) => [
      x - over + f * (width + 2 * over),
      y0 - f * rise + (i % 2 ? 1.5 : -1),
    ]);
    return g.curve(reverse ? pts.reverse() : pts, o);
  };
  return passes === 2 ? [line(y - 3, false), line(y + 5, true)] : [line(y, false)];
}

export interface PenTickProps extends PenProps {
  /** Centre of the tick. */
  x: number;
  y: number;
  /** Width of the tick in px (default 60). */
  size?: number;
}

/** A confident red-pen tick. Drawn heavier than other marks (strokeWidth 7). */
export const PenTick: React.FC<PenTickProps> = ({ x, y, size = 60, ...pen }) => (
  <PenStroke
    {...pen}
    options={{ strokeWidth: 7, roughness: 0.9, ...pen.options }}
    deps={[x, y, size]}
    build={(g, o) => [
      g.linearPath(
        [
          [x - size / 2, y + size * 0.02],
          [x - size / 6, y + size * 0.4],
          [x + size / 2, y - size * 0.5],
        ],
        o,
      ),
    ]}
  />
);

export interface PenArrowProps extends PenProps {
  from: Point;
  /** The tip. */
  to: Point;
  /** Sideways bow of the shaft in px, to the left of the travel direction (negative = right). Default 30. */
  bend?: number;
  /** Arrowhead barb length in px (default 22). */
  head?: number;
}

/** A curved red-pen arrow; the shaft is drawn first, then the two barbs. */
export const PenArrow: React.FC<PenArrowProps> = ({ from, to, bend = 30, head = 22, ...pen }) => (
  <PenStroke
    {...pen}
    options={{ strokeWidth: 4, ...pen.options }}
    deps={[...from, ...to, bend, head]}
    build={(g, o) => {
      const [x0, y0] = from;
      const [x1, y1] = to;
      const len = Math.hypot(x1 - x0, y1 - y0) || 1;
      // Unit normal to the left of travel; the shaft bows through the offset midpoint.
      const [nx, ny] = [(y1 - y0) / len, -(x1 - x0) / len];
      const mid: [number, number] = [(x0 + x1) / 2 + nx * bend, (y0 + y1) / 2 + ny * bend];
      // The barbs follow the shaft's direction as it arrives at the tip.
      const inAngle = Math.atan2(y1 - mid[1], x1 - mid[0]);
      const barb = (turn: number): [number, number] => [
        x1 - Math.cos(inAngle + turn) * head,
        y1 - Math.sin(inAngle + turn) * head,
      ];
      return [
        g.curve([[x0, y0], mid, [x1, y1]], o),
        g.line(x1, y1, ...barb(0.5), o),
        g.line(x1, y1, ...barb(-0.5), o),
      ];
    }}
  />
);

export interface PenNoteProps {
  /** Anchor point: the baseline at the left edge, centre or right edge (see `align`). */
  x: number;
  y: number;
  text: string;
  /** 0..1: the note is written left to right by a wipe; 0 renders nothing. */
  progress: number;
  /** Font size in px (default 46). */
  fontSize?: number;
  align?: "start" | "middle" | "end";
  /** Tilt in degrees around the anchor (default -2, hand-placed). */
  rotate?: number;
  color?: string;
}

/**
 * A handwritten red note in LXGW WenKai, written left to right. Its
 * characters must be in the bundled font subset (test them like Captions).
 */
export const PenNote: React.FC<PenNoteProps> = ({
  x,
  y,
  text,
  progress,
  fontSize = 46,
  align = "start",
  rotate = -2,
  color = PALETTE.redPen,
}) => {
  const clipId = useId();
  if (progress <= 0) return null;
  // The wipe only needs a generous width estimate, not an exact measurement.
  const width = emWidth(text) * fontSize * 1.1 + fontSize * 0.3;
  const left = align === "start" ? x : align === "middle" ? x - width / 2 : x - width;
  return (
    <g transform={`rotate(${rotate} ${x} ${y})`}>
      <clipPath id={clipId}>
        <rect x={left - fontSize * 0.2} y={y - fontSize * 1.3} width={width * progress + fontSize * 0.2} height={fontSize * 1.8} />
      </clipPath>
      <text
        x={x}
        y={y}
        textAnchor={align}
        fontFamily={`"${HAND_FONT}", serif`}
        fontSize={fontSize}
        fill={color}
        clipPath={`url(#${clipId})`}
      >
        {text}
      </text>
    </g>
  );
};

export interface PenMarkProps extends PenProps {
  /** circle: a loop round the text; strike: a strike-through; underline: a line under it. */
  kind: "circle" | "strike" | "underline";
  /** Extra room round the text in px (default 10). */
  pad?: number;
  /** For `strike`: 2 crosses the text out firmly. */
  passes?: 1 | 2;
  children: React.ReactNode;
}

/**
 * Wraps a run of HTML text (e.g. a filler word in a drawn Mail window) and
 * draws a red-pen mark sized to it. The text is measured after layout, so
 * the mark follows whatever font and wrapping the text ends up with:
 *
 *   <p>Friday works <PenMark kind="strike" progress={p} seed={41}>uh</PenMark> let's launch</p>
 *
 * The mark overflows the span and draws above the text; it does not change layout.
 */
export const PenMark: React.FC<PenMarkProps> = ({ kind, pad = 10, passes = 1, children, ...pen }) => {
  const ref = useRef<HTMLSpanElement>(null);
  const [size, setSize] = useState<[number, number] | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () =>
      setSize((prev) => (prev && prev[0] === el.offsetWidth && prev[1] === el.offsetHeight ? prev : [el.offsetWidth, el.offsetHeight]));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  const [w, h] = size ?? [0, 0];
  const build: RoughBuild =
    kind === "circle"
      ? (g, o) => [g.curve(loopPoints(w / 2, h / 2, w / 2 + pad, h / 2 + pad * 0.6, 1.12), { ...o, roughness: 0.6 })]
      : kind === "strike"
        ? (g, o) => strikeCurves(g, o, 0, h * 0.55, w, passes, 3)
        : (g, o) => strikeCurves(g, o, 0, h + pad * 0.4, w, 1, 2);
  return (
    <span ref={ref} style={{ position: "relative", display: "inline-block" }}>
      {children}
      {size && (
        <svg
          width={w}
          height={h}
          style={{ position: "absolute", left: 0, top: 0, overflow: "visible", pointerEvents: "none" }}
        >
          <PenStroke {...pen} deps={[kind, w, h, pad, passes]} build={build} />
        </svg>
      )}
    </span>
  );
};
