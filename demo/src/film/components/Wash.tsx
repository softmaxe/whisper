import { AbsoluteFill, useCurrentFrame, useVideoConfig } from "remotion";
import { WASH_LAYERS, washAt } from "./washKeys.ts";

/**
 * Translucent watercolour wash over the paper, showing the time of day: dawn
 * peach, pale day, dusk and a blue night, then clearing to blank paper for the
 * ending (keyframes in washKeys.ts, by Film time).
 *
 * The pigment is uneven like a real wash: soft blotches from a static
 * low-frequency noise, fine granulation, a heavier sky along the top and
 * pigment pooled at the edges. The texture is fixed across frames (fixed
 * seeds); only the colour and strength move. It sits under the Beat content,
 * so windows, Whis, the red pen and the Captions are drawn on top of it.
 */
export const Wash: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps, width, height } = useVideoConfig();
  const { color, opacity } = washAt(frame / fps);
  if (opacity <= 0.001) return null;

  return (
    <AbsoluteFill style={{ mixBlendMode: "multiply", opacity }}>
      <svg width={width} height={height} style={{ position: "absolute", inset: 0 }}>
        <defs>
          {/* Soft blotches: alpha 0.45–1 from a static low-frequency noise, coloured with the wash. */}
          <filter id="wash-blotch" x="0" y="0" width="100%" height="100%">
            <feTurbulence type="fractalNoise" baseFrequency="0.0028 0.0045" numOctaves={4} seed={9} result="noise" />
            <feColorMatrix in="noise" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  1.5 0 0 0 0" result="alpha" />
            <feComponentTransfer in="alpha" result="blotch">
              <feFuncA type="linear" slope={1.1} intercept={-0.1} />
            </feComponentTransfer>
            <feFlood floodColor={color} result="pigment" />
            <feComposite in="pigment" in2="blotch" operator="in" />
          </filter>
          {/* Granulation: pigment settling into the paper's tooth. */}
          <filter id="wash-grain" x="0" y="0" width="100%" height="100%">
            <feTurbulence type="fractalNoise" baseFrequency="0.6" numOctaves={2} seed={23} result="noise" />
            <feColorMatrix in="noise" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  2.2 0 0 0 -0.9" result="alpha" />
            <feFlood floodColor={color} result="pigment" />
            <feComposite in="pigment" in2="alpha" operator="in" />
          </filter>
          <linearGradient id="wash-sky" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={color} stopOpacity={WASH_LAYERS.skyTop} />
            <stop offset="45%" stopColor={color} stopOpacity={WASH_LAYERS.skyTop * 0.35} />
            <stop offset="100%" stopColor={color} stopOpacity={0} />
          </linearGradient>
          <radialGradient id="wash-pool" cx="50%" cy="52%" r="72%">
            <stop offset="62%" stopColor={color} stopOpacity={0} />
            <stop offset="100%" stopColor={color} stopOpacity={WASH_LAYERS.poolEdge} />
          </radialGradient>
        </defs>
        <rect width={width} height={height} filter="url(#wash-blotch)" />
        <rect width={width} height={height} filter="url(#wash-grain)" opacity={WASH_LAYERS.grain} />
        <rect width={width} height={height} fill="url(#wash-sky)" />
        <rect width={width} height={height} fill="url(#wash-pool)" />
      </svg>
    </AbsoluteFill>
  );
};
