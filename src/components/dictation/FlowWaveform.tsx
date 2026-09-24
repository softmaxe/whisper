import { useEffect, useRef, type CSSProperties } from "react";
import { cn } from "../lib/utils";
import { FLOW_BAR_COUNT, resolveFlowBarHeight, resolveFlowBarTarget } from "./waveformMath";

interface FlowWaveformProps {
  /** Returns the current input level (0..~1) or null when no signal source exists. */
  getLevel: () => number | null;
  /** While true the bars follow the live level; false freezes the current shape. */
  active: boolean;
  /** Drops an inactive waveform back to dots, e.g. while the microphone reconnects. */
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

/**
 * The Flow bar's live waveform. Like PillWaveform it writes heights straight
 * to the DOM from a rAF loop, so listening never pays React re-render cost.
 */
export function FlowWaveform({
  getLevel,
  active,
  resting = false,
  className,
  style,
}: FlowWaveformProps) {
  const barRefs = useRef<(HTMLSpanElement | null)[]>([]);

  useEffect(() => {
    // Stopping freezes the last shape so it fades out with the collapsing pill;
    // a lost signal must not keep showing the last syllable, so it rests.
    if (!active) {
      if (resting) {
        for (const bar of barRefs.current) {
          if (bar) bar.style.height = `${resolveFlowBarHeight(0)}px`;
        }
      }
      return;
    }

    // A new session swells from dots, never from the previous session's shape.
    const heights = new Array(FLOW_BAR_COUNT).fill(0);
    let frame = 0;
    let last = 0;
    const paint = (now: number) => {
      // Normalize the easing to elapsed time so 120Hz displays move at the
      // same speed as 60Hz ones.
      const frames = last ? Math.min(4, (now - last) / FRAME_MS) : 1;
      last = now;
      const rms = getLevel() ?? 0;
      for (let i = 0; i < FLOW_BAR_COUNT; i += 1) {
        const target = resolveFlowBarTarget(rms, i, now);
        const rate = target > heights[i] ? RISE : FALL;
        heights[i] += (target - heights[i]) * (1 - Math.pow(1 - rate, frames));
        const bar = barRefs.current[i];
        if (bar) bar.style.height = `${resolveFlowBarHeight(heights[i])}px`;
      }
      frame = requestAnimationFrame(paint);
    };
    frame = requestAnimationFrame(paint);
    return () => cancelAnimationFrame(frame);
  }, [active, resting, getLevel]);

  return (
    <div
      className={cn("voice-flow-waveform flex h-full items-center justify-center", className)}
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
