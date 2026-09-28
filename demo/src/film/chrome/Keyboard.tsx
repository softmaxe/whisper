import type { Box } from "./AppWindow.tsx";
import { FnKey } from "./FnKey.tsx";
import { RoughDrawing, useWobbleSeed } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";

const PAD = 18;
const GAP = 7;
/** Key widths per row in key units (each row adds up to 14.5), Mac layout. */
const ROWS: readonly (readonly number[])[] = [
  [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1.5],
  [1.5, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
  [1.75, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1.75],
  [2.25, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2.25],
  // fn, control, option, command, space, command, option, arrows.
  [1, 1, 1, 1.25, 5, 1.25, 1, 3],
];

/** Key cap rectangles of a keyboard drawn in `box`; `fn` is the bottom-left key. */
export function keyboardLayout(box: Box): { keys: Box[]; fn: Box } {
  const unit = (box.w - 2 * PAD) / 14.5;
  const pitch = (box.h - 2 * PAD) / ROWS.length;
  const keys = ROWS.flatMap((row, r) => {
    let x = box.x + PAD;
    return row.map((units) => {
      const key = { x: x + GAP / 2, y: box.y + PAD + r * pitch + GAP / 2, w: units * unit - GAP, h: pitch - GAP };
      x += units * unit;
      return key;
    });
  });
  const fn = keys[keys.length - ROWS[4].length];
  return { keys: keys.filter((k) => k !== fn), fn };
}

export interface KeyboardProps {
  /** The keyboard's outline, seen from above, in canvas px (about 700x250 reads well). */
  box: Box;
  /** 0..1 sketch-in. */
  progress?: number;
  /** Index of the key being struck (any integer; wraps round the letter keys) and 0..1 strike. */
  pressedKey?: number;
  press?: number;
  /** 0..1 press depth of the fn/Globe key (see keyDepth in FnKey.tsx). */
  fnDepth?: number;
  seed?: number;
}

/**
 * A Mac keyboard lying flat, sketched in pencil, with a working fn/Globe key
 * at the bottom left (use keyboardLayout(box).fn to circle it). A struck key
 * shades in for a moment. Renders an SVG <g>.
 */
export const Keyboard: React.FC<KeyboardProps> = ({ box, progress = 1, pressedKey = 0, press = 0, fnDepth = 0, seed = 81 }) => {
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  const { keys, fn } = keyboardLayout(box);
  // Strike only the letter rows' single-width keys.
  const letters = keys.filter((k) => Math.abs(k.w - keys[1].w) < 1 && k.y < fn.y);
  const struck = letters[((pressedKey % letters.length) + letters.length) % letters.length];
  return (
    <g>
      <rect x={box.x} y={box.y} width={box.w} height={box.h} rx={16} fill={PALETTE.cream} opacity={progress} />
      <RoughDrawing
        seed={boil}
        progress={progress}
        options={{ stroke: PALETTE.graphite, strokeWidth: 3, roughness: 1.1, bowing: 0.8 }}
        deps={[box.x, box.y, box.w, box.h]}
        build={(g, o) => [
          g.rectangle(box.x, box.y, box.w, box.h, o),
          // Hatched front edge, so it reads as a slab lying on the desk.
          g.rectangle(box.x + 6, box.y + box.h, box.w - 12, 14, {
            ...o,
            strokeWidth: 2,
            fill: PALETTE.pencil,
            fillStyle: "hachure",
            hachureGap: 8,
            hachureAngle: 20,
            fillWeight: 1.2,
          }),
        ]}
      />
      <RoughDrawing
        seed={boil + 1}
        progress={progress}
        options={{ stroke: PALETTE.pencil, strokeWidth: 1.8, roughness: 0.8, bowing: 0.5 }}
        deps={[box.x, box.y, box.w, box.h]}
        build={(g, o) => keys.map((k) => g.rectangle(k.x, k.y, k.w, k.h, o))}
      />
      {press > 0 && struck && (
        <rect x={struck.x + 2} y={struck.y + 2} width={struck.w - 4} height={struck.h - 4} rx={4} fill={PALETTE.graphite} opacity={0.6 * press} />
      )}
      <FnKey x={fn.x} y={fn.y} size={fn.w} depth={fnDepth} progress={progress} seed={seed + 3} />
    </g>
  );
};
