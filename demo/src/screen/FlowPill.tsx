import { interpolate, useCurrentFrame } from "remotion";
import {
  FLOW_BAR_COUNT,
  resolveFlowBarHeight,
  resolveFlowBarTarget,
  resolveFlowSweepOpacity,
  resolveFlowWaveOpacity,
  resolveFlowWaveTarget,
} from "../../../src/components/dictation/waveformMath";
import { pillEase, speechLevel } from "../lib/anim.ts";
import { APP } from "../theme.ts";

// Footprints from VOICE_PILL_FOOTPRINT (src/helpers/voicePillPresentation.js).
const SLIVER = { width: 38, height: 10 };
const LISTENING = { width: 84, height: 30 };
const GROW_FRAMES = 9; // 300ms expansion
const WARMUP_FRAMES = 6; // the sweep before the microphone is ready
const RISE = 0.45;
const FALL = 0.16;
const BAR_WIDTH = 2.5;
const BAR_GAP = 3;
const MS_PER_FRAME = 1000 / 30;

interface FlowPillProps {
  /** Local frames: listening starts, speech stops (thinking), text lands (rest). */
  listenAt: number;
  stopAt: number;
  doneAt: number;
  /** Visual magnification so the pill reads at video size. */
  scale?: number;
  /** The Hold mode press glows the rim while the key is down. */
  held?: boolean;
}

/** Lanes after easing toward each frame's target, replayed from the start. */
function lanesAt(frame: number, listenAt: number, stopAt: number) {
  const lanes = new Array<number>(FLOW_BAR_COUNT).fill(0);
  for (let f = listenAt; f <= frame; f += 1) {
    const now = f * MS_PER_FRAME;
    const live = f >= listenAt + WARMUP_FRAMES && f < stopAt;
    const thinking = f >= stopAt;
    for (let i = 0; i < FLOW_BAR_COUNT; i += 1) {
      const target = thinking
        ? resolveFlowWaveTarget(i, now)
        : live
          ? resolveFlowBarTarget(speechLevel(f), i, now)
          : 0;
      const rate = target > lanes[i] ? RISE : FALL;
      // Two 60fps easing steps per 30fps frame.
      lanes[i] += (target - lanes[i]) * (1 - Math.pow(1 - rate, 2));
    }
  }
  return lanes;
}

/** The floating Flow bar: a black capsule whose bars follow the voice. */
export function FlowPill({ listenAt, stopAt, doneAt, scale = 2.2, held = false }: FlowPillProps) {
  const frame = useCurrentFrame();
  const grow = interpolate(frame, [listenAt, listenAt + GROW_FRAMES], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: pillEase,
  });
  const shrink = interpolate(frame, [doneAt, doneAt + GROW_FRAMES], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: pillEase,
  });
  const open = grow * (1 - shrink);
  const width = SLIVER.width + (LISTENING.width - SLIVER.width) * open;
  const height = SLIVER.height + (LISTENING.height - SLIVER.height) * open;
  const lanes = frame >= listenAt ? lanesAt(Math.min(frame, doneAt), listenAt, stopAt) : null;
  const now = frame * MS_PER_FRAME;
  const warming = frame >= listenAt && frame < listenAt + WARMUP_FRAMES;
  const thinking = frame >= stopAt && frame < doneAt;

  return (
    <div
      style={{
        width,
        height,
        borderRadius: height,
        background: APP.flowInk,
        border: `1px solid ${held ? "rgba(255,255,255,0.42)" : APP.flowRim}`,
        boxShadow: held
          ? "0 0 0 3px rgba(91,134,245,0.35), 0 2px 6px rgba(0,0,0,0.28)"
          : "0 2px 6px rgba(0,0,0,0.28)",
        transform: `scale(${scale})`,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: BAR_GAP,
        color: "#fff",
        overflow: "hidden",
      }}
    >
      {open > 0.4 &&
        lanes &&
        lanes.map((lane, i) => (
          <span
            key={i}
            style={{
              width: BAR_WIDTH,
              height: resolveFlowBarHeight(lane),
              borderRadius: 2,
              background: "currentColor",
              opacity:
                ((open - 0.4) / 0.6) *
                (warming
                  ? resolveFlowSweepOpacity(i, now)
                  : thinking
                    ? resolveFlowWaveOpacity(i, now)
                    : 1),
              flexShrink: 0,
            }}
          />
        ))}
    </div>
  );
}
