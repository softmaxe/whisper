import { useMemo } from "react";
import { useCurrentFrame } from "remotion";
import rough from "roughjs";
import type { Drawable, Options } from "roughjs/bin/core";
import type { RoughGenerator } from "roughjs/bin/generator";

/**
 * Renders roughjs shapes as SVG paths with an optional stroke-by-stroke reveal.
 * Copied from the Pelican Test Film (softmaxe/animate-test).
 *
 * Usage:
 *   <svg width={1920} height={1080}>
 *     <RoughDrawing
 *       seed={useWobbleSeed(3)}
 *       build={(g, o) => [g.rectangle(100, 100, 300, 200, o), g.circle(500, 200, 80, o)]}
 *       options={{ stroke: PALETTE.graphite, strokeWidth: 3 }}
 *       progress={interpolate(frame, [0, 45], [0, 1], clamp)}
 *     />
 *   </svg>
 *
 * `seed` must be a pure function of the frame: Remotion renders frames in
 * parallel, so every render of a frame has to generate the identical wobble.
 * `build` is only re-run when `seed`/`options`/`deps` change, so keep it free
 * of per-frame values and animate via `progress`.
 */

export type RoughBuild = (g: RoughGenerator, options: Options) => Drawable[];

interface RoughDrawingProps {
  build: RoughBuild;
  seed: number;
  options?: Options;
  /** 0 = nothing drawn, 1 = fully drawn. Paths are revealed in order. */
  progress?: number;
  /** Extra values that should trigger regeneration of the shapes. */
  deps?: unknown[];
}

interface RevealPath {
  d: string;
  stroke: string;
  strokeWidth: number;
  fill?: string;
}

const generator = rough.generator();

/**
 * A seed that changes every `every` frames (7.5 Hz at 30 fps by default), so
 * pencil strokes "boil" gently like hand-drawn animation while staying
 * deterministic per frame. Keep the Paper on a fixed seed. Hachure path counts
 * can change with the seed, so hold `base` fixed while a stroke-by-stroke
 * reveal is still running (pass `still` = true) to avoid a jump.
 */
export function useWobbleSeed(base: number, every = 4, still = false): number {
  const frame = useCurrentFrame();
  return still ? base : base + Math.floor(frame / every) * 101;
}

export const RoughDrawing: React.FC<RoughDrawingProps> = ({ build, seed, options, progress = 1, deps = [] }) => {
  const paths = useMemo<RevealPath[]>(() => {
    const opts: Options = { roughness: 1.2, bowing: 1, ...options, seed };
    return build(generator, opts).flatMap((d) => generator.toPaths(d));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seed, JSON.stringify(options), ...deps]);

  const count = paths.length;
  return (
    <g>
      {paths.map((p, i) => {
        // Each path owns an equal slice of the overall progress.
        const local = Math.min(1, Math.max(0, progress * count - i));
        if (local <= 0) return null;
        const isFill = p.fill && p.fill !== "none";
        return (
          <path
            key={i}
            d={p.d}
            stroke={p.stroke}
            strokeWidth={p.strokeWidth}
            fill={p.fill ?? "none"}
            fillOpacity={isFill ? local : undefined}
            strokeLinecap="round"
            strokeLinejoin="round"
            pathLength={1}
            strokeDasharray={local < 1 ? "1 1" : undefined}
            strokeDashoffset={local < 1 ? 1 - local : undefined}
          />
        );
      })}
    </g>
  );
};
