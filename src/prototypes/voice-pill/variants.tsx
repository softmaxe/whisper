// PROTOTYPE (throwaway): Wispr Flow-style recording pill variants.
// Each variant renders the whole pill for one phase; phase changes drive the morph.
import { useRef, type CSSProperties, type ReactNode } from "react";

export type PillPhase = "idle" | "listening" | "processing";

export interface VariantProps {
  phase: PillPhase;
  /** Raw input RMS (0..~0.15 for speech). */
  rms: number;
  /** rAF timestamp, ms. */
  now: number;
}

// Same curve as waveformMath.toBarLevel so every variant reacts like the real pill.
export const toBarLevel = (rms: number) => Math.min(1, Math.pow(Math.max(0, rms) * 8, 0.75));

const EASE = "cubic-bezier(0.2, 0, 0, 1)";
const MORPH = `width 320ms ${EASE}, height 320ms ${EASE}, border-radius 320ms ${EASE}`;

const INK = "#0c0c0e";
const RIM = "rgba(255,255,255,0.16)";
const SHADOW = "0 6px 18px rgba(0,0,0,0.35), 0 1px 2px rgba(0,0,0,0.4)";

function Capsule({
  width,
  height,
  children,
  style,
}: {
  width: number;
  height: number;
  children?: ReactNode;
  style?: CSSProperties;
}) {
  return (
    <div
      style={{
        width,
        height,
        borderRadius: height / 2,
        background: INK,
        boxShadow: `inset 0 0 0 1px ${RIM}, ${SHADOW}`,
        transition: MORPH,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        overflow: "hidden",
        position: "relative",
        ...style,
      }}
    >
      {children}
    </div>
  );
}

/** Center-weighted live bars: every bar follows the current level, shaped by a bell envelope. */
function useLiveBars(count: number, envelope: number[], rms: number, now: number, active: boolean) {
  const heights = useRef<number[]>(new Array(count).fill(0));
  const level = active ? toBarLevel(rms) : 0;
  heights.current = heights.current.map((h, i) => {
    const wobble = 0.5 + 0.5 * Math.sin(now * 0.012 * (1 + i * 0.37) + i * 1.7);
    const target = level * envelope[i] * (0.55 + 0.45 * wobble);
    return h + (target - h) * (target > h ? 0.45 : 0.16);
  });
  return heights.current;
}

function Bars({
  values,
  width,
  gap,
  min,
  max,
  opacity = 1,
}: {
  values: number[];
  width: number;
  gap: number;
  min: number;
  max: number;
  opacity?: number;
}) {
  return (
    <div
      style={{ display: "flex", alignItems: "center", gap, opacity, transition: "opacity 200ms" }}
    >
      {values.map((v, i) => (
        <span
          key={i}
          style={{
            width,
            height: min + v * (max - min),
            borderRadius: width,
            background: "#fff",
          }}
        />
      ))}
    </div>
  );
}

function DotsLoader({
  count,
  size,
  gap,
  now,
}: {
  count: number;
  size: number;
  gap: number;
  now: number;
}) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap }}>
      {Array.from({ length: count }, (_, i) => {
        const wave = 0.5 + 0.5 * Math.sin(now * 0.008 - i * 0.9);
        return (
          <span
            key={i}
            style={{
              width: size,
              height: size,
              borderRadius: size,
              background: "#fff",
              opacity: 0.3 + 0.7 * wave,
              transform: `translateY(${-wave * 1.5}px)`,
            }}
          />
        );
      })}
    </div>
  );
}

const fade = (visible: boolean, delay = 0): CSSProperties => ({
  position: "absolute",
  inset: 0,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  opacity: visible ? 1 : 0,
  transition: `opacity 180ms ease-out ${visible ? delay : 0}ms`,
});

// ── A · Flow bar ────────────────────────────────────────────────────────────
// Wispr Flow's signature: a thin idle sliver that swells into a black pill of
// symmetric white bars. No icon — the waveform is the whole identity.
const A_ENVELOPE = [0.35, 0.55, 0.75, 0.92, 1, 1, 0.92, 0.75, 0.55, 0.35];
export function FlowBar({ phase, rms, now }: VariantProps) {
  const bars = useLiveBars(10, A_ENVELOPE, rms, now, phase === "listening");
  const size =
    phase === "idle"
      ? { w: 44, h: 10 }
      : phase === "listening"
        ? { w: 84, h: 30 }
        : { w: 60, h: 30 };
  return (
    <Capsule width={size.w} height={size.h}>
      <div style={fade(phase === "listening", 120)}>
        <Bars values={bars} width={2.5} gap={3} min={3} max={18} />
      </div>
      <div style={fade(phase === "processing", 120)}>
        <DotsLoader count={3} size={4} gap={4} now={now} />
      </div>
    </Capsule>
  );
}

// ── B · Hands-free ──────────────────────────────────────────────────────────
// Wispr Flow's locked (hands-free) bar: cancel on the left, live bars in the
// middle, a red stop square on the right. Controls live inside the pill.
const B_ENVELOPE = [0.45, 0.7, 0.9, 1, 1, 1, 0.9, 0.7, 0.45];
function RoundButton({ bg, children }: { bg: string; children: ReactNode }) {
  return (
    <span
      style={{
        width: 24,
        height: 24,
        borderRadius: 12,
        background: bg,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
      }}
    >
      {children}
    </span>
  );
}
export function HandsFree({ phase, rms, now }: VariantProps) {
  const bars = useLiveBars(9, B_ENVELOPE, rms, now, phase === "listening");
  const listening = phase === "listening";
  const size =
    phase === "idle" ? { w: 44, h: 10 } : listening ? { w: 128, h: 32 } : { w: 60, h: 32 };
  return (
    <Capsule width={size.w} height={size.h}>
      <div style={{ ...fade(listening, 120), justifyContent: "space-between", padding: "0 4px" }}>
        <RoundButton bg="rgba(255,255,255,0.14)">
          <svg width="10" height="10" viewBox="0 0 10 10">
            <path
              d="M2 2l6 6M8 2L2 8"
              stroke="rgba(255,255,255,0.85)"
              strokeWidth="1.6"
              strokeLinecap="round"
            />
          </svg>
        </RoundButton>
        <Bars values={bars} width={2.5} gap={3} min={3} max={18} />
        <RoundButton bg="#ff3b30">
          <span style={{ width: 8, height: 8, borderRadius: 2, background: "#fff" }} />
        </RoundButton>
      </div>
      <div style={fade(phase === "processing", 120)}>
        <DotsLoader count={3} size={4} gap={4} now={now} />
      </div>
    </Capsule>
  );
}

// ── C · Timeline ────────────────────────────────────────────────────────────
// A scrolling history of what you just said: silence is a row of dots, speech
// lifts them into mirrored bars that slide left and fade out at the edge.
const C_COUNT = 18;
const C_SAMPLE_MS = 70;
export function Timeline({ phase, rms, now }: VariantProps) {
  const samples = useRef<number[]>(new Array(C_COUNT).fill(0));
  const lastSample = useRef(0);
  const listening = phase === "listening";
  if (listening && now - lastSample.current >= C_SAMPLE_MS) {
    lastSample.current = now;
    samples.current = [...samples.current.slice(1), toBarLevel(rms)];
  }
  if (phase === "idle") samples.current = new Array(C_COUNT).fill(0);
  const size = phase === "idle" ? { w: 44, h: 10 } : { w: 108, h: 28 };
  const recPulse = 0.55 + 0.45 * Math.sin(now * 0.006);
  const sweep = ((now * 0.12) % 160) - 30;
  return (
    <Capsule width={size.w} height={size.h} style={{ justifyContent: "flex-start" }}>
      <div
        style={{
          ...fade(phase !== "idle", 120),
          justifyContent: "flex-start",
          gap: 8,
          paddingLeft: 10,
        }}
      >
        <span
          style={{
            width: 6,
            height: 6,
            borderRadius: 3,
            flexShrink: 0,
            background: listening ? "#ff453a" : "rgba(255,255,255,0.5)",
            opacity: listening ? recPulse : 1,
            transition: "background 200ms",
          }}
        />
        <div
          style={{
            WebkitMaskImage:
              phase === "processing"
                ? `linear-gradient(90deg, rgba(0,0,0,0.25) ${sweep}%, #000 ${sweep + 18}%, rgba(0,0,0,0.25) ${sweep + 36}%)`
                : "linear-gradient(90deg, transparent 0%, #000 35%)",
          }}
        >
          <Bars values={samples.current} width={2.5} gap={2} min={2.5} max={18} />
        </div>
      </div>
    </Capsule>
  );
}

// ── D · In place ────────────────────────────────────────────────────────────
// No morph at all: the idle pill already holds five dots; speaking stretches
// them into bars without the capsule ever changing size.
const D_ENVELOPE = [0.55, 0.85, 1, 0.85, 0.55];
export function InPlace({ phase, rms, now }: VariantProps) {
  const bars = useLiveBars(5, D_ENVELOPE, rms, now, phase === "listening");
  return (
    <Capsule width={46} height={24}>
      {phase === "processing" ? (
        <DotsLoader count={5} size={3} gap={3} now={now} />
      ) : (
        <Bars
          values={bars}
          width={3}
          gap={3}
          min={3}
          max={14}
          opacity={phase === "idle" ? 0.45 : 1}
        />
      )}
    </Capsule>
  );
}

export const PILL_VARIANTS = [
  { key: "A", name: "Flow bar", Component: FlowBar },
  { key: "B", name: "Hands-free controls", Component: HandsFree },
  { key: "C", name: "Scrolling timeline", Component: Timeline },
  { key: "D", name: "In place, no morph", Component: InPlace },
] as const;
