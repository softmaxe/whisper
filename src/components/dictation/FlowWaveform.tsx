import { useEffect, useRef, type CSSProperties } from "react";
import { cn } from "../lib/utils";
import {
  FLOW_BAR_COUNT,
  resolveFlowBarHeight,
  resolveFlowBarTarget,
  resolveFlowSweepOpacity,
  resolveFlowWaveOpacity,
  resolveFlowWaveTarget,
} from "./waveformMath";

/**
 * What the bars are doing: following the live level, sweeping a light across
 * resting dots while the microphone warms up, or rippling a travelling wave
 * while the transcript is being made.
 */
export type FlowMotion = "live" | "sweep" | "wave";

interface FlowWaveformProps {
  /** Returns the current input level (0..~1) or null when no signal source exists. */
  getLevel: () => number | null;
  /** Null freezes the current shape. */
  motion: FlowMotion | null;
  /** Drops a frozen waveform back to plain dots, e.g. while the microphone reconnects. */
  resting?: boolean;
  className?: string;
  style?: CSSProperties;
}

const BAR_WIDTH_PX = 2.5;
const BAR_GAP_PX = 3;
// Per-60fps-frame easing toward the target: bars jump up with a syllable and
// fall back more slowly, so the shape breathes instead of flickering.
const RISE = 0.45;
const FALL = 0.16;
const FRAME_MS = 1000 / 60;

// Per motion: each bar's target lane (0..1) and opacity at a moment.
const MOTION_BARS: Record<
  FlowMotion,
  {
    target: (rms: number, index: number, now: number) => number;
    opacity: (index: number, now: number) => number;
  }
> = {
  live: { target: resolveFlowBarTarget, opacity: () => 1 },
  sweep: { target: () => 0, opacity: resolveFlowSweepOpacity },
  wave: {
    target: (_rms, index, now) => resolveFlowWaveTarget(index, now),
    opacity: resolveFlowWaveOpacity,
  },
};

/**
 * The Flow bar's waveform. Like PillWaveform it writes heights straight to the
 * DOM from a rAF loop, so listening never pays React re-render cost.
 */
export function FlowWaveform({
  getLevel,
  motion,
  resting = false,
  className,
  style,
}: FlowWaveformProps) {
  const barRefs = useRef<(HTMLSpanElement | null)[]>([]);
  // Lanes persist across motions so stopping eases the live shape into the
  // thinking wave instead of snapping it to dots first.
  const lanesRef = useRef<number[]>(new Array(FLOW_BAR_COUNT).fill(0));

  useEffect(() => {
    const lanes = lanesRef.current;
    const paintBar = (index: number, opacity: number) => {
      const bar = barRefs.current[index];
      if (!bar) return;
      bar.style.height = `${resolveFlowBarHeight(lanes[index])}px`;
      bar.style.opacity = String(opacity);
    };

    // Stopping freezes the last shape so it fades out with the collapsing pill;
    // a lost signal must not keep showing the last syllable, so it rests.
    if (!motion) {
      if (resting) {
        lanes.fill(0);
        for (let i = 0; i < FLOW_BAR_COUNT; i += 1) paintBar(i, 1);
      }
      return;
    }

    // A new session swells from dots, never from the previous session's shape.
    if (motion === "live") lanes.fill(0);
    const bars = MOTION_BARS[motion];
    let frame = 0;
    let last = 0;
    const paint = (now: number) => {
      // Normalize the easing to elapsed time so 120Hz displays move at the
      // same speed as 60Hz ones.
      const frames = last ? Math.min(4, (now - last) / FRAME_MS) : 1;
      last = now;
      const rms = motion === "live" ? (getLevel() ?? 0) : 0;
      for (let i = 0; i < FLOW_BAR_COUNT; i += 1) {
        const target = bars.target(rms, i, now);
        const rate = target > lanes[i] ? RISE : FALL;
        lanes[i] += (target - lanes[i]) * (1 - Math.pow(1 - rate, frames));
        paintBar(i, bars.opacity(i, now));
      }
      frame = requestAnimationFrame(paint);
    };
    frame = requestAnimationFrame(paint);
    return () => cancelAnimationFrame(frame);
  }, [motion, resting, getLevel]);

  return (
    <div
      className={cn("voice-flow-waveform flex h-full items-center justify-center", className)}
      data-motion={motion ?? undefined}
      style={{ gap: BAR_GAP_PX, ...style }}
      aria-hidden="true"
    >
      {Array.from({ length: FLOW_BAR_COUNT }, (_, i) => (
        <span
          key={i}
          ref={(el) => {
            barRefs.current[i] = el;
          }}
          className="rounded-full bg-current voice-flow-bar"
          style={{ width: BAR_WIDTH_PX, height: resolveFlowBarHeight(0) }}
        />
      ))}
    </div>
  );
}
