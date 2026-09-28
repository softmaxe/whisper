import { useId } from "react";
import type { Point } from "../../pen/RedPen.tsx";
import { emWidth } from "../../components/captionLayout.ts";
import { HAND_FONT } from "../../fonts.ts";
import { RoughDrawing, useWobbleSeed } from "../../rough/RoughDrawing.tsx";
import { PALETTE } from "../../theme.ts";

/*
 * Pieces of the Beat 5 house scene, all SVG in canvas px. Everything is
 * pencil (graphite) so the red pen stays for the annotations and the data.
 */

const PENCIL = { stroke: PALETTE.graphite, strokeWidth: 4, roughness: 1.1, bowing: 0.9 } as const;
export const MONO = `"JetBrains Mono", Menlo, "SF Mono", monospace`;

/** The house: walls from `left` to `right` and `eave` to `base`, a roof up to `apex`, and a chimney. */
export interface HouseShape {
  left: number;
  right: number;
  eave: number;
  base: number;
  apex: number;
  /** How far the roof overhangs the walls. */
  overhang: number;
}

/** The house outline, drawn roof first, then walls, chimney and a pencilled ground line. */
export const HouseOutline: React.FC<{ house: HouseShape; progress: number; seed: number }> = ({ house, progress, seed }) => {
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  const { left, right, eave, base, apex, overhang } = house;
  const cx = (left + right) / 2;
  // Chimney on the right slope: its foot sits on the roof line.
  const roofY = (x: number) => apex + ((x - cx) / (right + overhang - cx)) * (eave + 20 - apex);
  const chimney = { x0: cx + (right - cx) * 0.52, x1: cx + (right - cx) * 0.64, top: apex + 20 };
  return (
    <RoughDrawing
      seed={boil}
      options={PENCIL}
      progress={progress}
      deps={[left, right, eave, base, apex, overhang]}
      build={(g, o) => [
        g.linearPath(
          [
            [left - overhang, eave + 20],
            [cx, apex],
            [right + overhang, eave + 20],
          ],
          { ...o, strokeWidth: 5 },
        ),
        g.linearPath(
          [
            [left, eave],
            [left, base],
            [right, base],
            [right, eave],
          ],
          o,
        ),
        g.polygon(
          [
            [chimney.x0, roofY(chimney.x0)],
            [chimney.x0, chimney.top],
            [chimney.x1, chimney.top],
            [chimney.x1, roofY(chimney.x1)],
          ],
          { ...o, fill: PALETTE.pencil, fillStyle: "hachure", hachureGap: 9, fillWeight: 1.4 },
        ),
        g.line(left - 24, base + 2, right + overhang + 60, base - 2, { ...o, strokeWidth: 2.4, roughness: 1.6 }),
      ]}
    />
  );
};

/** A desktop Mac: monitor with a white screen, stand and foot. `children` draw on the screen. */
export const Mac: React.FC<{
  x: number;
  y: number;
  w: number;
  h: number;
  progress: number;
  seed: number;
  children?: React.ReactNode;
}> = ({ x, y, w, h, progress, seed, children }) => {
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  const cx = x + w / 2;
  const inset = 16;
  const screenOpacity = Math.min(1, Math.max(0, (progress - 0.5) / 0.5));
  return (
    <g>
      <rect x={x + inset} y={y + inset} width={w - 2 * inset} height={h - 2 * inset} rx={6} fill={PALETTE.whitePaper} opacity={screenOpacity} />
      <RoughDrawing
        seed={boil}
        options={PENCIL}
        progress={progress}
        deps={[x, y, w, h]}
        build={(g, o) => [
          g.rectangle(x, y, w, h, o),
          g.rectangle(x + inset, y + inset, w - 2 * inset, h - 2 * inset, { ...o, strokeWidth: 2.2 }),
          g.linearPath(
            [
              [cx - 36, y + h],
              [cx - 50, y + h + 70],
              [cx + 50, y + h + 70],
              [cx + 36, y + h],
            ],
            o,
          ),
          g.rectangle(cx - 100, y + h + 70, 200, 14, { ...o, fill: PALETTE.pencil, fillStyle: "hachure", hachureGap: 6 }),
        ]}
      />
      <g opacity={screenOpacity}>{children}</g>
    </g>
  );
};

/** A rack server: a white box with vents on the left, two lights on the right and its name written on it. */
export const Server: React.FC<{
  x: number;
  y: number;
  w: number;
  h: number;
  name: string;
  /** 0..1 reveal of the box. */
  progress: number;
  /** 0..1 reveal of the name. */
  nameProgress: number;
  /** 0..1 brightness of the activity light. */
  light: number;
  seed: number;
}> = ({ x, y, w, h, name, progress, nameProgress, light, seed }) => {
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  const surface = Math.min(1, Math.max(0, (progress - 0.4) / 0.6));
  return (
    <g>
      <rect x={x} y={y} width={w} height={h} fill={PALETTE.whitePaper} opacity={surface} />
      <RoughDrawing
        seed={boil}
        options={PENCIL}
        progress={progress}
        deps={[x, y, w, h]}
        build={(g, o) => [
          g.rectangle(x, y, w, h, o),
          ...[0.3, 0.5, 0.7].map((f) => g.line(x + 24, y + h * f, x + 110, y + h * f, { ...o, strokeWidth: 2.4 })),
          g.circle(x + w - 64, y + h / 2, 16, { ...o, strokeWidth: 2.4 }),
          g.circle(x + w - 34, y + h / 2, 16, { ...o, strokeWidth: 2.4 }),
        ]}
      />
      <circle cx={x + w - 64} cy={y + h / 2} r={6} fill={PALETTE.redPen} opacity={surface * (0.25 + 0.75 * light)} />
      <circle cx={x + w - 34} cy={y + h / 2} r={6} fill={PALETTE.pencil} opacity={surface * 0.6} />
      <Written x={x + 136} y={y + h / 2 + 13} progress={nameProgress} fontSize={36} font={`"${HAND_FONT}", serif`} color={PALETTE.ink}>
        {name}
      </Written>
    </g>
  );
};

/**
 * Text revealed left to right by a wipe, as if written: `font` picks the
 * hand (LXGW WenKai) or a clear face (e.g. MONO for endpoints and commands).
 * `width` is a generous estimate of the text's width in px for the wipe.
 */
export const Written: React.FC<{
  x: number;
  y: number;
  progress: number;
  fontSize: number;
  font: string;
  color: string;
  width?: number;
  anchor?: "start" | "middle";
  children: string;
}> = ({ x, y, progress, fontSize, font, color, width, anchor = "start", children }) => {
  const clipId = useId();
  if (progress <= 0) return null;
  // Generous: CJK advances a full em, Latin (and monospace) about 0.6 em.
  const w = width ?? emWidth(children) * fontSize * 1.15 + fontSize;
  const left = anchor === "middle" ? x - w / 2 : x;
  return (
    <g>
      <clipPath id={clipId}>
        <rect x={left - fontSize * 0.3} y={y - fontSize * 1.2} width={w * progress + fontSize * 0.3} height={fontSize * 1.7} />
      </clipPath>
      <text x={x} y={y} textAnchor={anchor} fontFamily={font} fontSize={fontSize} fill={color} clipPath={`url(#${clipId})`}>
        {children}
      </text>
    </g>
  );
};

/** A data arrow as a quadratic curve through the same bowed midpoint PenArrow uses. */
export interface DataPath {
  from: Point;
  to: Point;
  bend: number;
}

/** Point at `u` (0..1) along a data arrow's shaft. */
export function pointOn({ from, to, bend }: DataPath, u: number): Point {
  const [x0, y0] = from;
  const [x1, y1] = to;
  const len = Math.hypot(x1 - x0, y1 - y0) || 1;
  const [nx, ny] = [(y1 - y0) / len, -(x1 - x0) / len];
  const mid = [(x0 + x1) / 2 + nx * bend, (y0 + y1) / 2 + ny * bend];
  // Control point so the curve passes through `mid` at u = 0.5.
  const c = [2 * mid[0] - (x0 + x1) / 2, 2 * mid[1] - (y0 + y1) / 2];
  const a = (1 - u) * (1 - u);
  const b = 2 * u * (1 - u);
  const d = u * u;
  return [a * x0 + b * c[0] + d * x1, a * y0 + b * c[1] + d * y1];
}
