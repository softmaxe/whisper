import { Easing, interpolate, useVideoConfig } from "remotion";
import type { Seconds } from "../../../timeline/types.ts";
import { clamp } from "../anim.ts";
import {
  PILL_BAR_GAP,
  PILL_BAR_WIDTH,
  PILL_GROW_BEZIER,
  PILL_GROW_SECONDS,
  PILL_LISTENING,
  PILL_SLIVER,
  pillBars,
  speechLevel,
} from "../pill/waveform.ts";
import { RoughDrawing, useWobbleSeed } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";

/** The app's pill morph curve (VOICE_PILL_GROW_EASING). */
const pillEase = Easing.bezier(...PILL_GROW_BEZIER);
/** Hold mode's rim glow, pencilled in the app's accent blue. */
const HELD_BLUE = "#5b86f5";

export interface RecordingPillProps {
  /** Centre of the pill in the parent SVG's coordinates. */
  cx: number;
  cy: number;
  /** Current Film time (e.g. `t` from useBeatTime). */
  t: Seconds;
  /** Film time the pill opens and starts listening (e.g. a `listen` moment from doubleTapMoments). */
  listenAt: Seconds;
  /** Film time speech stops: the bars turn into the app's thinking wave. Default: keeps listening. */
  stopAt?: Seconds;
  /** Film time the text lands: the pill shrinks back to a sliver and goes. Default: stays open. */
  doneAt?: Seconds;
  /** Magnification of the app's 84x30 px pill (default 2.6, about 220x78 on the canvas). */
  scale?: number;
  /** Hold mode: the key is held down, so the rim gets a blue pencil ring. */
  held?: boolean;
  /**
   * Microphone level (RMS, 0..~0.15) at a Film frame. Defaults to a
   * speech-like level; pass `() => 0` for silence.
   */
  level?: (frame: number) => number;
  /** Base wobble seed for the pencil outline. */
  seed?: number;
}

/**
 * The floating Recording pill, hand-drawn: a black crayon capsule with a
 * wobbly pencil rim whose white bars replay the app's own waveform math
 * (see ../pill/waveform.ts), so it moves exactly like the real pill: sliver →
 * grow, warm-up sweep, live bars, thinking wave, shrink.
 *
 * Renders an SVG <g>; place it inside a full-canvas <svg>:
 *
 *   const { t } = useBeatTime(beat);
 *   <RecordingPill cx={1300} cy={540} t={t} listenAt={m.listen} stopAt={m.stop} doneAt={m.paste} />
 */
export const RecordingPill: React.FC<RecordingPillProps> = ({
  cx,
  cy,
  t,
  listenAt,
  stopAt = Infinity,
  doneAt = Infinity,
  scale = 2.6,
  held = false,
  level = speechLevel,
  seed = 61,
}) => {
  const { fps } = useVideoConfig();
  const wobble = useWobbleSeed(seed);
  if (t < listenAt) return null;
  const frame = Math.round(t * fps);
  const timing = {
    listenAt: Math.round(listenAt * fps),
    stopAt: Number.isFinite(stopAt) ? Math.round(stopAt * fps) : Infinity,
    doneAt: Number.isFinite(doneAt) ? Math.round(doneAt * fps) : Infinity,
  };
  const grow = interpolate(t, [listenAt, listenAt + PILL_GROW_SECONDS], [0, 1], { ...clamp, easing: pillEase });
  const shrink = t < doneAt ? 0 : interpolate(t, [doneAt, doneAt + PILL_GROW_SECONDS], [0, 1], { ...clamp, easing: pillEase });
  const open = grow * (1 - shrink);
  if (t >= doneAt && shrink >= 1) return null;

  const w = (PILL_SLIVER.width + (PILL_LISTENING.width - PILL_SLIVER.width) * open) * scale;
  const h = (PILL_SLIVER.height + (PILL_LISTENING.height - PILL_SLIVER.height) * open) * scale;
  const x = cx - w / 2;
  const y = cy - h / 2;
  const bars = open > 0.4 ? pillBars(frame, timing, fps, level) : [];
  const barsOpacity = (open - 0.4) / 0.6;
  const barW = PILL_BAR_WIDTH * scale;
  const pitch = (PILL_BAR_WIDTH + PILL_BAR_GAP) * scale;
  const firstBarX = cx - ((bars.length - 1) * pitch) / 2;
  // Rough geometry is rounded so it only regenerates when the size really changes.
  const [rw, rh] = [Math.round(w), Math.round(h)];

  return (
    <g>
      {/* A soft pencil shadow under the pill. */}
      <ellipse cx={cx} cy={cy + h / 2 + 10 * (scale / 2.6)} rx={w * 0.42} ry={5 * (scale / 2.6)} fill={PALETTE.pencil} opacity={0.18 * open} />
      <path d={capsule(x, y, w, h)} fill={PALETTE.clawdBlack} />
      <RoughDrawing
        seed={seed + wobble}
        options={{ stroke: PALETTE.graphite, strokeWidth: 3, roughness: 0.9, bowing: 0.6 }}
        deps={[rw, rh, cx, cy]}
        build={(g, o) => [g.path(capsule(cx - rw / 2, cy - rh / 2, rw, rh), o)]}
      />
      {held && (
        <RoughDrawing
          seed={seed + 7 + wobble}
          options={{ stroke: HELD_BLUE, strokeWidth: 4, roughness: 1.1, bowing: 0.8 }}
          deps={[rw, rh, cx, cy]}
          build={(g, o) => [g.path(capsule(cx - rw / 2 - 12, cy - rh / 2 - 12, rw + 24, rh + 24), o)]}
        />
      )}
      {bars.map((bar, i) => {
        const bh = bar.height * scale;
        return (
          <rect
            key={i}
            x={firstBarX + i * pitch - barW / 2}
            y={cy - bh / 2}
            width={barW}
            height={bh}
            rx={barW / 2}
            fill={PALETTE.clawdWhite}
            opacity={barsOpacity * bar.opacity}
          />
        );
      })}
    </g>
  );
};

/** A stadium (capsule) path: a rectangle with fully rounded ends. */
function capsule(x: number, y: number, w: number, h: number): string {
  const r = Math.min(h, w) / 2;
  return `M ${x + r} ${y} H ${x + w - r} A ${r} ${r} 0 0 1 ${x + w - r} ${y + h} H ${x + r} A ${r} ${r} 0 0 1 ${x + r} ${y} Z`;
}
