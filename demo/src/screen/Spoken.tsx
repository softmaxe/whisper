import type { ReactNode } from "react";
import { useCurrentFrame } from "remotion";
import type { Token } from "../copy.ts";
import { ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { MONO } from "../theme.ts";
import { SCREEN_W } from "./Desktop.tsx";

interface SpokenProps {
  tokens: Token[];
  /** Local frames: words start and finish arriving, fillers get struck, bubble leaves. */
  from: number;
  to: number;
  strikeAt?: number;
  hideAt: number;
  /** Bottom edge in screen points. */
  bottom?: number;
  width?: number;
}

/**
 * What the microphone heard, word by word, floating above the Recording pill.
 * Fillers show dimmed and are struck through when cleanup runs.
 */
export function Spoken({
  tokens,
  from,
  to,
  strikeAt,
  hideAt,
  bottom = 200,
  width = 820,
}: SpokenProps) {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const enter = ramp(frame, from - 4, 8);
  const exit = ramp(frame, hideAt, 8);
  if (enter <= 0 || exit >= 1) return null;
  const count = Math.floor(ramp(frame, from, to - from, (t) => t) * tokens.length + 0.0001);
  const strike = strikeAt === undefined ? 0 : ramp(frame, strikeAt, 10);
  return (
    <div
      style={{
        position: "absolute",
        left: (SCREEN_W - width) / 2,
        bottom,
        width,
        padding: "18px 24px",
        borderRadius: 20,
        background: "rgba(16,17,20,0.9)",
        color: "#fff",
        fontFamily: copy.font,
        boxShadow: "0 18px 40px rgba(0,0,0,0.3)",
        opacity: enter * (1 - exit),
        transform: `translateY(${(1 - enter) * 16}px)`,
      }}
    >
      <div
        style={{
          fontSize: 17,
          fontWeight: 600,
          color: "#9fb8ff",
          marginBottom: 8,
          letterSpacing: 0.3,
        }}
      >
        {copy.saidLabel}
      </div>
      <div style={{ fontFamily: MONO, fontSize: 25, lineHeight: 1.5, minHeight: 38 }}>
        {tokens.slice(0, count).map((token, i) => (
          <span key={i}>
            {i > 0 && copy.gap}
            <span
              style={{
                color: token.filler ? "#8b8c92" : "#fff",
                textDecoration: token.filler && strike > 0 ? "line-through" : undefined,
                textDecorationColor: "#ff6b6b",
                textDecorationThickness: 3,
                opacity: token.filler ? 1 - strike * 0.55 : 1,
              }}
            >
              {token.text}
            </span>
          </span>
        ))}
        {count < tokens.length && frame >= from && (
          <span
            style={{
              display: "inline-block",
              width: 3,
              height: 26,
              background: "#9fb8ff",
              marginLeft: 4,
              verticalAlign: -4,
            }}
          />
        )}
      </div>
    </div>
  );
}

/** Floating layer that holds the Recording pill at the bottom center of the screen. */
export function PillDock({ children, bottom = 116 }: { children: ReactNode; bottom?: number }) {
  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        right: 0,
        bottom,
        display: "flex",
        justifyContent: "center",
        pointerEvents: "none",
      }}
    >
      {children}
    </div>
  );
}
