/**
 * The Recording pill's waveform, replayed with the app's own math
 * (src/components/dictation/waveformMath.ts): the same bar count, per-bar
 * targets, mic warm-up sweep, thinking wave and rise/fall easing as
 * FlowWaveform, advanced one video frame at a time.
 *
 * Pure and deterministic: every value is a function of the frame, so frames
 * can render in parallel. Frames are Film frames (any consistent counter works).
 */
import {
  FLOW_BAR_COUNT,
  resolveFlowBarHeight,
  resolveFlowBarTarget,
  resolveFlowSweepOpacity,
  resolveFlowWaveOpacity,
  resolveFlowWaveTarget,
} from "../../../../src/components/dictation/waveformMath.ts";
import { hash01 } from "../../../timeline/helpers.ts";

export { FLOW_BAR_COUNT as PILL_BAR_COUNT };

/** Footprints from VOICE_PILL_FOOTPRINT (src/helpers/voicePillPresentation.js), in app px. */
export const PILL_SLIVER = { width: 38, height: 10 } as const;
export const PILL_LISTENING = { width: 84, height: 30 } as const;
/** Bar geometry of FlowWaveform, in app px. */
export const PILL_BAR_WIDTH = 2.5;
export const PILL_BAR_GAP = 3;
/** LISTENING_ENTRANCE_TIMING.expansionMs: the sliver grows into the pill. */
export const PILL_GROW_SECONDS = 0.3;
/** The sweep shown before the microphone is ready. */
export const PILL_WARMUP_SECONDS = 0.2;
/** FlowWaveform's easing toward each target, per 60 fps frame. */
const RISE = 0.45;
const FALL = 0.16;

/** When the pill listens, stops listening (thinking) and closes, in frames. */
export interface PillTiming {
  listenAt: number;
  /** Speech ends and the thinking wave runs until `doneAt`. Default: never. */
  stopAt?: number;
  /** The text lands and the pill closes. Default: never. */
  doneAt?: number;
}

export type PillPhase = "hidden" | "warming" | "listening" | "thinking" | "done";

/**
 * A speech-like microphone level (RMS, 0..~0.15) at a 30 fps frame: syllables
 * of varying loudness separated by short dips and the odd pause.
 */
export function speechLevel(frame: number): number {
  const syllable = Math.floor(frame / 5);
  const phase = (frame % 5) / 5;
  const loudness = 0.05 + hash01(syllable) * 0.1;
  const shape = Math.sin(Math.PI * phase);
  const pause = hash01(Math.floor(frame / 23) + 99) < 0.15 ? 0.25 : 1;
  return loudness * (0.35 + 0.65 * shape) * pause;
}

export function pillPhase(frame: number, timing: PillTiming, fps: number): PillPhase {
  const { listenAt, stopAt = Infinity, doneAt = Infinity } = timing;
  if (frame < listenAt) return "hidden";
  if (frame >= doneAt) return "done";
  if (frame >= stopAt) return "thinking";
  return frame < listenAt + Math.round(PILL_WARMUP_SECONDS * fps) ? "warming" : "listening";
}

/**
 * Lane values (0..1 of the bar lane) at `frame`, replayed from `listenAt` with
 * FlowWaveform's easing: zero during warm-up, the app's bar targets for
 * `level(f)` while listening, the thinking wave after `stopAt`.
 */
export function pillLanes(
  frame: number,
  timing: PillTiming,
  fps: number,
  level: (frame: number) => number = speechLevel,
): number[] {
  const { listenAt, stopAt = Infinity, doneAt = Infinity } = timing;
  const lanes = new Array<number>(FLOW_BAR_COUNT).fill(0);
  const warmup = Math.round(PILL_WARMUP_SECONDS * fps);
  const last = Math.min(frame, doneAt);
  for (let f = listenAt; f <= last; f += 1) {
    const now = (f * 1000) / fps;
    const thinking = f >= stopAt;
    const live = !thinking && f >= listenAt + warmup;
    for (let i = 0; i < FLOW_BAR_COUNT; i += 1) {
      const target = thinking ? resolveFlowWaveTarget(i, now) : live ? resolveFlowBarTarget(level(f), i, now) : 0;
      const rate = target > lanes[i] ? RISE : FALL;
      lanes[i] += (target - lanes[i]) * (1 - Math.pow(1 - rate, 60 / fps));
    }
  }
  return lanes;
}

/** One bar as the app draws it: height in app px (3..18) and opacity. */
export interface PillBar {
  height: number;
  opacity: number;
}

/** The pill's bars at `frame`, in app px; empty while hidden. */
export function pillBars(
  frame: number,
  timing: PillTiming,
  fps: number,
  level: (frame: number) => number = speechLevel,
): PillBar[] {
  const phase = pillPhase(frame, timing, fps);
  if (phase === "hidden") return [];
  const now = (frame * 1000) / fps;
  return pillLanes(frame, timing, fps, level).map((lane, i) => ({
    height: resolveFlowBarHeight(lane),
    opacity:
      phase === "warming" ? resolveFlowSweepOpacity(i, now) : phase === "thinking" ? resolveFlowWaveOpacity(i, now) : 1,
  }));
}
