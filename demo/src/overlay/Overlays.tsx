import type { ReactNode } from "react";
import { useCurrentFrame } from "remotion";
import { easeOut, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { INK } from "../theme.ts";

/** Frames a single key press stays down. */
const PRESS_FRAMES = 4;

/** 0..1 depth of the key at `frame` for presses at `taps` (held until `release`). */
export function keyDepth(frame: number, taps: number[], release?: number) {
  if (release !== undefined && taps.length > 0 && frame >= taps[0]) {
    return frame < release ? 1 : 1 - ramp(frame, release, 3);
  }
  let depth = 0;
  for (const tap of taps) {
    if (frame >= tap && frame < tap + PRESS_FRAMES + 3) {
      depth = Math.max(
        depth,
        frame < tap + PRESS_FRAMES ? 1 : 1 - (frame - tap - PRESS_FRAMES) / 3
      );
    }
  }
  return depth;
}

function Globe({ size, color }: { size: number; color: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke={color}
      strokeWidth="1.7"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18M12 3c2.6 2.6 3.6 5.6 3.6 9s-1 6.4-3.6 9c-2.6-2.6-3.6-5.6-3.6-9s1-6.4 3.6-9z" />
    </svg>
  );
}

/** The fn/Globe key, pressing on each tap with a ripple. */
export function KeyCap({
  taps,
  release,
  size = 1,
}: {
  taps: number[];
  release?: number;
  size?: number;
}) {
  const frame = useCurrentFrame();
  const depth = keyDepth(frame, taps, release);
  const w = 118 * size;
  return (
    <div style={{ position: "relative", width: w, height: w * 0.86 }}>
      {taps.map((tap) => {
        const t = ramp(frame, tap, 14);
        return t > 0 && t < 1 ? (
          <div
            key={tap}
            style={{
              position: "absolute",
              inset: -24 * t * size,
              borderRadius: 26 * size,
              border: `${3 * size}px solid rgba(91,134,245,${0.8 * (1 - t)})`,
            }}
          />
        ) : null;
      })}
      <div
        style={{
          position: "absolute",
          inset: 0,
          borderRadius: 20 * size,
          background: "#26272c",
          boxShadow: `0 ${(8 - depth * 6) * size}px 0 #0d0e10, 0 ${(14 - depth * 8) * size}px ${22 * size}px rgba(0,0,0,0.35)`,
          transform: `translateY(${depth * 6 * size}px)`,
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          padding: `${14 * size}px ${16 * size}px`,
          color: depth > 0.5 ? "#9fb8ff" : "#f2f2f2",
          fontFamily: "-apple-system, sans-serif",
        }}
      >
        <span style={{ alignSelf: "flex-end", fontSize: 26 * size, fontWeight: 500 }}>fn</span>
        <Globe size={30 * size} color={depth > 0.5 ? "#9fb8ff" : "#f2f2f2"} />
      </div>
    </div>
  );
}

interface CaptionProps {
  text: string;
  start: number;
  end: number;
  children?: ReactNode;
  kicker?: string;
}

/** A feature caption card in the lower left, optionally led by the key. */
export function Caption({ text, start, end, children, kicker }: CaptionProps) {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const enter = ramp(frame, start, 12);
  const exit = ramp(frame, end - 8, 8);
  const shown = enter * (1 - exit);
  if (shown <= 0) return null;
  return (
    <div
      style={{
        position: "absolute",
        left: 64,
        bottom: 60,
        display: "flex",
        alignItems: "center",
        gap: 26,
        padding: children ? "20px 34px 20px 22px" : "22px 34px",
        borderRadius: 30,
        background: "rgba(255,255,255,0.94)",
        boxShadow: "0 20px 50px rgba(20,16,40,0.25)",
        color: INK,
        fontFamily: copy.font,
        opacity: shown,
        transform: `translateY(${(1 - enter) * 30}px) scale(${0.96 + 0.04 * enter})`,
        transformOrigin: "left bottom",
      }}
    >
      {children}
      <div>
        {kicker && (
          <div style={{ fontSize: 22, fontWeight: 600, color: "#5b86f5", marginBottom: 4 }}>
            {kicker}
          </div>
        )}
        <div
          style={{
            maxWidth: children ? 600 : 760,
            fontSize: 34,
            lineHeight: 1.2,
            fontWeight: 700,
            letterSpacing: copy.lang === "en" ? -0.4 : 1,
          }}
        >
          {text}
        </div>
      </div>
    </div>
  );
}

/** The time of day, in the upper left of each establishing shot. */
export function ClockChip({
  text,
  start,
  end,
  night,
}: {
  text: string;
  start: number;
  end: number;
  night?: boolean;
}) {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const enter = ramp(frame, start, 12, easeOut);
  const exit = ramp(frame, end - 8, 8);
  const shown = enter * (1 - exit);
  if (shown <= 0) return null;
  return (
    <div
      style={{
        position: "absolute",
        left: 64,
        top: 56,
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "14px 28px 14px 18px",
        borderRadius: 40,
        background: night ? "rgba(20,22,52,0.85)" : "rgba(255,255,255,0.9)",
        color: night ? "#fff" : INK,
        fontFamily: copy.font,
        fontSize: 36,
        fontWeight: 700,
        fontVariantNumeric: "tabular-nums",
        boxShadow: "0 14px 40px rgba(20,16,40,0.2)",
        opacity: shown,
        transform: `translateY(${(1 - enter) * -20}px)`,
      }}
    >
      <svg width="40" height="40" viewBox="0 0 40 40">
        {night ? (
          <path d="M26 6a15 15 0 1 0 8 26A13 13 0 0 1 26 6z" fill="#ffd98a" />
        ) : (
          <g fill="#ffb347">
            <circle cx="20" cy="20" r="8" />
            {Array.from({ length: 8 }, (_, i) => (
              <rect
                key={i}
                x="18.5"
                y="2"
                width="3"
                height="7"
                rx="1.5"
                transform={`rotate(${i * 45} 20 20)`}
              />
            ))}
          </g>
        )}
      </svg>
      {text}
    </div>
  );
}
