import { interpolate } from "remotion";
import { clamp, moveEase, ramp } from "../../anim.ts";
import type { Box } from "../../chrome/AppWindow.tsx";
import { PenNote } from "../../pen/RedPen.tsx";
import { RoughDrawing, useWobbleSeed } from "../../rough/RoughDrawing.tsx";
import { PALETTE } from "../../theme.ts";

/* Composition shared by the three notes (canvas px). Captions are written below y = 850. */

/** The note being shown, large, right of Whis. */
export const NOTE: Box = { x: 470, y: 28, w: 1400, h: 792 };
/** The app window inside each note, in note-local px. */
export const NOTE_WINDOW: Box = { x: 44, y: 116, w: 1312, h: 636 };
/** A finished note shrinks to this scale... */
export const THUMB_SCALE = 0.2;
/** ...and is pinned in one of these slots at the top left (canvas px, degrees). */
export const THUMB_SLOTS = [
  { x: 60, y: 52, rotate: -3 },
  { x: 72, y: 236, rotate: 2.5 },
] as const;

/** Offset of the note's hatched pencil shadow. */
const SHADOW = 14;
const PIN_RED = "#c94a3a";

export interface PinnedNoteProps {
  /** 0..1 sketch-in: the sheet and pin, then the title is written. 0 renders nothing. */
  progress: number;
  /** 0..1 shrink from the large note to its thumbnail slot. */
  move?: number;
  slot?: { x: number; y: number; rotate: number };
  /** The hand-lettered title at the top left. */
  title: string;
  seed: number;
  /** HTML content in note-local px (usually an AppWindow at NOTE_WINDOW). */
  children?: React.ReactNode;
  /** SVG marks drawn over the content, in note-local px. */
  marks?: React.ReactNode;
}

/**
 * A sheet of paper pinned to the page: a wobbly pencil outline with a
 * hatched shadow, a red pushpin at the top and a handwritten title. Its
 * content (an app window) and red-pen marks are in note-local px; when the
 * note is done, `move` shrinks it, content and marks together, to a small
 * thumbnail pinned at the top left. HTML: place it in an AbsoluteFill.
 */
export const PinnedNote: React.FC<PinnedNoteProps> = ({ progress, move = 0, slot, title, seed, children, marks }) => {
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  const { w, h } = NOTE;
  const e = moveEase(ramp(move, 0, 1));
  const to = slot ?? { x: NOTE.x, y: NOTE.y, rotate: 0 };
  const x = NOTE.x + (to.x - NOTE.x) * e;
  const y = NOTE.y + (to.y - NOTE.y) * e;
  const scale = 1 + (THUMB_SCALE - 1) * e;
  const rotate = to.rotate * e;
  const sheetP = interpolate(progress, [0, 0.6], [0, 1], clamp);
  const surfaceP = interpolate(progress, [0, 0.35], [0, 1], clamp);
  const titleP = interpolate(progress, [0.5, 1], [0, 1], clamp);
  // The pin drops in as the sheet lands.
  const pinP = interpolate(progress, [0.35, 0.55], [0, 1], clamp);

  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        top: 0,
        width: w,
        height: h,
        transformOrigin: "0 0",
        transform: `translate(${x}px, ${y}px) rotate(${rotate}deg) scale(${scale})`,
      }}
    >
      <svg width={w + SHADOW + 10} height={h + SHADOW + 10} style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }}>
        <rect x={2} y={2} width={w - 4} height={h - 4} fill={PALETTE.cream} opacity={surfaceP} />
        <RoughDrawing
          seed={boil}
          progress={sheetP}
          options={{ stroke: PALETTE.graphite, strokeWidth: 3, roughness: 1.2, bowing: 1 }}
          build={(g, o) => [
            g.rectangle(0, 0, w, h, o),
            g.polygon(
              [
                [w, SHADOW],
                [w + SHADOW, SHADOW],
                [w + SHADOW, h + SHADOW],
                [SHADOW, h + SHADOW],
                [SHADOW, h],
                [w, h],
              ],
              { ...o, stroke: "none", fill: PALETTE.pencil, fillStyle: "hachure", hachureGap: 8, hachureAngle: -52, fillWeight: 1.3 },
            ),
          ]}
        />
      </svg>
      <div style={{ position: "absolute", inset: 0, opacity: surfaceP }}>{children}</div>
      <svg width={w} height={h} style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }}>
        <PenNote x={56} y={84} text={title} progress={titleP} fontSize={54} rotate={-1.5} color={PALETTE.ink} />
        {marks}
        <Pushpin x={w / 2} y={22} progress={pinP} seed={seed + 5} />
      </svg>
    </div>
  );
};

/** A red pushpin head seen from above, with a pencil shadow. */
const Pushpin: React.FC<{ x: number; y: number; progress: number; seed: number }> = ({ x, y, progress, seed }) => {
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  const drop = (1 - progress) * -30;
  return (
    <g transform={`translate(0 ${drop})`} opacity={progress}>
      <ellipse cx={x + 6} cy={y + 8} rx={20} ry={12} fill={PALETTE.pencil} opacity={0.25} />
      <RoughDrawing
        seed={boil}
        options={{ stroke: PALETTE.graphite, strokeWidth: 2.4, roughness: 0.8, fill: PIN_RED, fillStyle: "solid" }}
        build={(g, o) => [g.circle(x, y, 38, o)]}
      />
      <circle cx={x - 6} cy={y - 6} r={5} fill={PALETTE.whitePaper} opacity={0.7} />
    </g>
  );
};
