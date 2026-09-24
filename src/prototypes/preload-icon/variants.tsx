// PROTOTYPE (throwaway): four pill identities for the non-listening phases
// (idle, mic warm-up, thinking), each built around the black Flow bar.
import { useRef, type CSSProperties } from "react";
import {
  FLOW_BAR_COUNT as N,
  FLOW_BAR_MIN_PX as MIN,
  resolveFlowBarHeight,
  resolveFlowBarTarget,
} from "../../components/dictation/waveformMath";

export type Phase = "idle" | "hover" | "preparing" | "listening" | "thinking";

export interface VariantProps {
  phase: Phase;
  rms: number;
  now: number;
  /** Slow-motion factor: 1 = real speed, 4 = quarter speed. */
  k: number;
}

// Same chrome and bar geometry as the shipped Flow bar (dictation-panel.css,
// FlowWaveform.tsx).
const INK = "#0c0c0e";
const RIM = "rgb(255 255 255 / 0.16)";
const SHADOW = "0 2px 6px rgb(0 0 0 / 0.28)";
const BAR_W = 2.5;
const PITCH = 5.5; // bar + 3px gap
const EASE = "cubic-bezier(0.2, 0, 0, 1)";
const BARS = Array.from({ length: N }, (_, i) => i);
const rowX = (i: number) => (i - (N - 1) / 2) * PITCH;

/** Live Flow bar heights, eased exactly like FlowWaveform. */
function useLiveBars(active: boolean, rms: number, now: number) {
  const heights = useRef(new Array(N).fill(0));
  const last = useRef(0);
  const frames = last.current ? Math.min(4, (now - last.current) / (1000 / 60)) : 1;
  last.current = now;
  for (let i = 0; i < N; i += 1) {
    const target = active ? resolveFlowBarTarget(rms, i, now) : 0;
    const rate = target > heights.current[i] ? 0.45 : 0.16;
    heights.current[i] += (target - heights.current[i]) * (1 - Math.pow(1 - rate, frames));
  }
  return heights.current.map(resolveFlowBarHeight);
}

const shell = (w: number, h: number, k: number, extra?: CSSProperties): CSSProperties => ({
  position: "relative",
  width: w,
  height: h,
  borderRadius: 9999,
  background: INK,
  border: `1px solid ${RIM}`,
  boxShadow: SHADOW,
  color: "#fff",
  overflow: "hidden",
  transition: `width ${300 * k}ms ${EASE}, height ${300 * k}ms ${EASE}, opacity ${220 * k}ms ease-out, background-color ${220 * k}ms ease-out`,
  ...extra,
});

const bar = (h: number, opacity = 1, extra?: CSSProperties): CSSProperties => ({
  width: BAR_W,
  height: h,
  borderRadius: 9999,
  background: "currentColor",
  opacity,
  flexShrink: 0,
  ...extra,
});

/* ------------------------------------------------------------------ A ---
 * Flow dock: never a circle. Idle is a slim black sliver (Wispr Flow's
 * resting dock); warm-up grows straight into the bar with a light sweeping
 * across the dots; thinking keeps the bar and ripples a travelling wave.
 */
function FlowDock({ phase, rms, now, k }: VariantProps) {
  const live = useLiveBars(phase === "listening", rms, now);
  const t = now / k;
  const size =
    phase === "idle" ? { w: 38, h: 10 } : phase === "hover" ? { w: 64, h: 22 } : { w: 84, h: 30 };
  const head = ((t / 70) % (N + 6)) - 3;
  return (
    <div style={shell(size.w, size.h, k, { opacity: phase === "idle" ? 0.9 : 1 })}>
      <div
        className="absolute inset-0 flex items-center justify-center"
        style={{
          gap: PITCH - BAR_W,
          opacity: phase === "idle" ? 0 : 1,
          transition: `opacity ${(phase === "idle" ? 120 : 200) * k}ms ease-out ${phase === "idle" ? 0 : 120 * k}ms`,
        }}
      >
        {BARS.map((i) => {
          let h = MIN;
          let o = 1;
          if (phase === "listening") h = live[i];
          else if (phase === "hover") o = 0.55;
          else if (phase === "preparing") {
            const d = i - head;
            o = 0.28 + 0.72 * Math.exp(-(d * d) / 2.2);
          } else if (phase === "thinking") {
            const w = 0.5 + 0.5 * Math.sin(t * 0.009 - i * 0.62);
            h = MIN + 6 * w;
            o = 0.45 + 0.55 * w;
          }
          return <span key={i} style={bar(h, o)} />;
        })}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ B ---
 * Black coin: keeps today's 40×40 identity circle and Signal glow, but
 * inverts it to the Flow bar's ink and swaps the ring logo for the four
 * middle Flow bars. Listening widens the coin and reveals the outer six.
 */
const GLYPH = [7, 13, 13, 7];
function BlackCoin({ phase, rms, now, k }: VariantProps) {
  const live = useLiveBars(phase === "listening", rms, now);
  const t = now / k;
  const open = phase === "listening";
  return (
    <span className="voice-pill-glow-anchor">
      <span
        aria-hidden="true"
        className="processing-signal-glow"
        data-active={phase === "thinking" ? "true" : undefined}
      >
        <span className="processing-signal-ring" />
      </span>
      <div
        style={shell(open ? 84 : 40, open ? 30 : 40, k, {
          background: phase === "hover" ? "#1c1c20" : INK,
        })}
      >
        <div
          className="absolute inset-0 flex items-center justify-center"
          style={{ gap: PITCH - BAR_W }}
        >
          {BARS.map((i) => {
            const g = i - 3; // glyph slot, 0..3 for the middle four
            const inGlyph = g >= 0 && g < GLYPH.length;
            let h = MIN;
            let o = open ? 1 : 0;
            if (open) h = live[i];
            else if (inGlyph) {
              o = 1;
              const base = GLYPH[g];
              if (phase === "idle") h = base;
              else if (phase === "hover") h = base + 2;
              else if (phase === "preparing") {
                h =
                  MIN + (base + 3 - MIN) * (0.25 + 0.75 * Math.abs(Math.sin(t * 0.007 - g * 0.8)));
              } else if (phase === "thinking") {
                h =
                  MIN + (base - MIN) * (0.35 + 0.65 * (0.5 + 0.5 * Math.sin(t * 0.008 - g * 1.1)));
                o = 0.9;
              }
            }
            return (
              <span
                key={i}
                style={bar(h, o, {
                  transition: open
                    ? `opacity ${200 * k}ms ease-out ${150 * k}ms`
                    : `opacity ${120 * k}ms ease-out`,
                })}
              />
            );
          })}
        </div>
      </div>
    </span>
  );
}

/* ------------------------------------------------------------------ C ---
 * Ring unfold: the same ten elements are always on screen. At rest they sit
 * as a dotted ring inside a black coin; warm-up and thinking spin it like an
 * activity indicator; listening squashes the ring flat into the Flow row.
 */
const RING_R = 9;
// Ring slots at 18° + 36°·s. Sorted by x they form five top/bottom pairs, so
// row slot i takes the pair's top (even) or bottom (odd) dot: flattening the
// ring vertically lands every dot on its own row slot.
const RING = BARS.map((i) => {
  const column = Math.floor(i / 2); // 0..4, left → right
  const deg = [162, 126, 90, 54, 18][column];
  const phi = ((i % 2 === 0 ? -deg : deg) * Math.PI) / 180;
  const angle = (Math.atan2(Math.sin(phi), Math.cos(phi)) * 180) / Math.PI;
  return {
    x: RING_R * Math.cos(phi),
    y: RING_R * Math.sin(phi),
    order: Math.round(((angle + 360 + 90) % 360) / 36) % N, // clockwise from 12 o'clock
  };
});
function RingUnfold({ phase, rms, now, k }: VariantProps) {
  const live = useLiveBars(phase === "listening", rms, now);
  const t = now / k;
  const open = phase === "listening";
  const spinning = phase === "preparing" || phase === "thinking";
  const head = (t / (phase === "thinking" ? 85 : 60)) % N;
  return (
    <div style={shell(open ? 84 : 40, open ? 30 : 40, k)}>
      {BARS.map((i) => {
        const x = open ? rowX(i) : RING[i].x;
        const y = open ? 0 : RING[i].y;
        let o = 1;
        if (spinning) {
          const behind = (head - RING[i].order + N) % N;
          o = 0.22 + 0.78 * Math.pow(1 - behind / N, 2.2);
        } else if (phase === "idle") o = 0.92;
        const size = open ? BAR_W : 3;
        return (
          <span
            key={i}
            style={{
              position: "absolute",
              left: "50%",
              top: "50%",
              width: size,
              height: open ? live[i] : 3,
              borderRadius: 9999,
              background: "currentColor",
              opacity: o,
              transform: `translate(${x}px, ${y}px) translate(-50%, -50%)`,
              transition: `transform ${300 * k}ms ${EASE}, width ${300 * k}ms ${EASE}`,
            }}
          />
        );
      })}
    </div>
  );
}

/* ------------------------------------------------------------------ D ---
 * Flatline trace: a small black capsule whose ten bars are fused into one
 * flat line at rest. Warm-up pulls the line apart into dots while a white
 * stroke runs the capsule's outline; thinking keeps the capsule and dots
 * still and lets the outline trace carry the progress.
 */
function FlatlineTrace({ phase, rms, now, k }: VariantProps) {
  const live = useLiveBars(phase === "listening", rms, now);
  const t = now / k;
  const compact = phase === "idle" || phase === "hover";
  const w = compact ? 46 : 84;
  const h = compact ? 22 : 30;
  const tracing = phase === "preparing" || phase === "thinking";
  const gap = compact ? 0 : PITCH - BAR_W;
  const lineW = compact ? 2.4 : BAR_W;
  return (
    <div style={shell(w, h, k)}>
      <svg
        className="pointer-events-none absolute inset-0"
        width={84}
        height={30}
        style={{
          opacity: tracing ? 1 : 0,
          transition: `opacity ${200 * k}ms ease-out ${tracing ? 220 * k : 0}ms`,
        }}
      >
        <rect
          x={0.5}
          y={0.5}
          width={82 - 1}
          height={28 - 1}
          rx={13.5}
          fill="none"
          stroke="#fff"
          strokeWidth={1.5}
          strokeLinecap="round"
          pathLength={100}
          strokeDasharray={phase === "thinking" ? "22 78" : "10 90"}
          strokeDashoffset={-((t / (phase === "thinking" ? 14 : 9)) % 100)}
          style={{ transition: `stroke-dasharray ${300 * k}ms ease-out` }}
        />
      </svg>
      <div
        className="absolute inset-0 flex items-center justify-center"
        style={{ gap, transition: `gap ${300 * k}ms ${EASE}` }}
      >
        {BARS.map((i) => {
          let bh = compact ? 2 : MIN;
          let o = phase === "idle" ? 0.7 : 1;
          if (phase === "listening") bh = live[i];
          if (phase === "thinking") o = 0.5;
          return (
            <span
              key={i}
              style={bar(bh, o, {
                width: lineW,
                borderRadius: compact ? 0 : 9999,
                ...(compact && i === 0 && { borderRadius: "9999px 0 0 9999px" }),
                ...(compact && i === N - 1 && { borderRadius: "0 9999px 9999px 0" }),
                transition: `width ${300 * k}ms ${EASE}, opacity ${200 * k}ms ease-out`,
              })}
            />
          );
        })}
      </div>
    </div>
  );
}

export const VARIANTS = [
  { key: "A", name: "Flow dock", Component: FlowDock },
  { key: "B", name: "Black coin", Component: BlackCoin },
  { key: "C", name: "Ring unfold", Component: RingUnfold },
  { key: "D", name: "Flatline trace", Component: FlatlineTrace },
];
