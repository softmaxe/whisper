import { AbsoluteFill, useVideoConfig } from "remotion";
import { RoughDrawing } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";

/**
 * Cream paper background: fine grain and soft fibres from SVG turbulence,
 * warm vignette, and a pencil-drawn margin. Static across frames (fixed
 * seeds, no per-frame wobble). Copied from the Pelican Test Film.
 */
export const Paper: React.FC = () => {
  const { width, height } = useVideoConfig();
  const margin = 36;
  return (
    <AbsoluteFill style={{ backgroundColor: PALETTE.paper }}>
      <svg width={width} height={height} style={{ position: "absolute", inset: 0 }}>
        <defs>
          <filter id="paper-grain" x="0" y="0" width="100%" height="100%">
            <feTurbulence type="fractalNoise" baseFrequency="0.85" numOctaves={3} seed={7} stitchTiles="stitch" />
            {/* Map noise to warm brown specks with sparse alpha. */}
            <feColorMatrix type="matrix" values="0 0 0 0 0.42  0 0 0 0 0.32  0 0 0 0 0.2  0 0 0 -1.6 1.05" />
          </filter>
          <filter id="paper-fibre" x="0" y="0" width="100%" height="100%">
            <feTurbulence type="fractalNoise" baseFrequency="0.006 0.035" numOctaves={4} seed={11} />
            <feColorMatrix type="matrix" values="0 0 0 0 0.55  0 0 0 0 0.43  0 0 0 0 0.27  0 0 0 -2.2 1.25" />
          </filter>
          <radialGradient id="paper-vignette" cx="50%" cy="48%" r="75%">
            <stop offset="55%" stopColor={PALETTE.paperShade} stopOpacity={0} />
            <stop offset="100%" stopColor="#b98f5a" stopOpacity={0.45} />
          </radialGradient>
        </defs>
        <rect width={width} height={height} filter="url(#paper-fibre)" opacity={0.16} />
        <rect width={width} height={height} filter="url(#paper-grain)" opacity={0.3} />
        <rect width={width} height={height} fill="url(#paper-vignette)" />
        <RoughDrawing
          seed={42}
          options={{ stroke: PALETTE.pencil, strokeWidth: 1.6, roughness: 1.6, bowing: 0.6 }}
          build={(g, o) => [g.rectangle(margin, margin, width - margin * 2, height - margin * 2, o)]}
          deps={[width, height]}
        />
      </svg>
    </AbsoluteFill>
  );
};
