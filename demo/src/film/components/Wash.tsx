import { AbsoluteFill } from "remotion";
import { PALETTE } from "../theme.ts";

/**
 * Translucent watercolour wash over the paper, showing the time of day.
 * Placeholder: a constant light dawn tint. The day-to-night keyframes (by Film
 * time, following BEAT_BOUNDS) replace this in the Beat 4 ticket.
 */
export const Wash: React.FC = () => (
  <AbsoluteFill
    style={{
      background: `radial-gradient(ellipse at 30% 20%, ${PALETTE.washDawn} 0%, transparent 70%)`,
      opacity: 0.22,
      mixBlendMode: "multiply",
    }}
  />
);
