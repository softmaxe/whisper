import { useId } from "react";
import { ramp } from "../anim.ts";
import { HAND_FONT } from "../fonts.ts";
import { RoughDrawing, useWobbleSeed } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";

/**
 * Whis, Whisper's mascot, drawn in the Film's paper style: a blocky black
 * body with the app icon's white waveform W on its front, two small white
 * eyes, stubby side arms and four short legs, with a pencil outline, waxy
 * crayon fill and light hatching.
 *
 * Renders an SVG <g>, so place it inside an <svg> (usually a full-canvas
 * 1920x1080 one):
 *
 *   const frame = useCurrentFrame();
 *   <svg width={1920} height={1080}>
 *     <Whis x={400} y={770} {...whisPose("wave", frame, fps)} draw={p} />
 *   </svg>
 *
 * (x, y) is the ground point between the feet. At scale 1 Whis is 296px wide
 * including arms (220px body) and 200px tall. Motion is driven by props, so
 * animate by passing per-frame values. The pencil strokes "boil" (re-seed
 * every 4 frames) unless `wobble` is false; the wobble is held still while
 * `draw` < 1 so the stroke-by-stroke reveal doesn't jump.
 */
export interface WhisProps {
  /** Ground point between the feet, in the parent SVG's coordinates. */
  x: number;
  y: number;
  /** 1 = 220px-wide body. */
  scale?: number;
  /** Tilt in degrees around the feet (positive = clockwise). */
  rotate?: number;
  /** Mirror horizontally: swaps which arm is which on screen (e.g. point left). */
  flip?: boolean;
  /**
   * Arm angles in degrees for the arm on the viewer's left / right (before
   * `flip`). 0 = straight out sideways, positive = raised, negative = lowered.
   * Useful range is about -70..80. See WHIS_POSES and whisPose().
   */
  leftArm?: number;
  rightArm?: number;
  /** Eye shape: open (white blocks), closed (blink line) or happy (^ ^). */
  eyes?: WhisEyes;
  /** Gaze offset of the eyes, each in -1..1 ([1, 0] looks to the viewer's right, before `flip`). */
  look?: readonly [number, number];
  /** Vertical squash & stretch around the feet: 1 = normal, 0.85 = squashed, 1.08 = stretched. Width compensates. */
  squash?: number;
  /** Draw legs. Set false when Whis sits behind something that hides them (e.g. a desk). */
  legs?: boolean;
  /** Extra mark beside the head: a pencilled "?" (puzzled) or sparkle strokes (proud). */
  mark?: WhisMark;
  /** Stick a pointing finger out of the right arm's tip (the "point" pose). */
  finger?: boolean;
  /** Hand-drawn reveal, 0..1: outline first, then the black fill, then eyes and the W. */
  draw?: number;
  /** Base wobble seed. Keep it fixed per on-screen Whis; change it for a differently sketched copy. */
  seed?: number;
  /** Re-seed the pencil strokes every few frames (default true). */
  wobble?: boolean;
  /** Overall opacity. */
  opacity?: number;
}

export type WhisEyes = "open" | "closed" | "happy";
export type WhisMark = "none" | "question" | "sparkles";

/** Static arm presets; spread into <Whis> and override as needed. whisPose() animates them. */
export const WHIS_POSES = {
  /** Idle: arms relaxed, slightly down. */
  idle: { leftArm: -18, rightArm: -18 },
  /** Both arms reaching down and forward to a keyboard (alternate them to type). */
  typing: { leftArm: -40, rightArm: -40 },
  /** Right arm up by the side of the head, scratching it (wiggle ~±10). */
  scratch: { leftArm: -20, rightArm: 70 },
  /** Arms pressed down at the sides, chest out (pair with happy eyes and a slight stretch). */
  proud: { leftArm: -62, rightArm: -62 },
  /** Right arm raised to wave (swing ~±18). */
  wave: { leftArm: -18, rightArm: 54 },
  /** Right arm held out, slightly raised, pointing at something on the viewer's right (flip to point left). */
  point: { leftArm: -26, rightArm: 22 },
} as const satisfies Record<string, { leftArm: number; rightArm: number }>;

export type WhisPoseName = keyof typeof WHIS_POSES;

/** Every pose, in storyboard order. */
export const WHIS_POSE_NAMES = Object.keys(WHIS_POSES) as WhisPoseName[];

/**
 * Deterministic blink: true for 4 frames every ~`periodSeconds` (offset per
 * Whis so two copies don't blink in unison).
 */
export function isBlinking(frame: number, fps: number, periodSeconds = 2.8, offsetSeconds = 0.9): boolean {
  const period = Math.round(periodSeconds * fps);
  const f = (frame + Math.round(offsetSeconds * fps)) % period;
  return f >= period - 4;
}

type PoseProps = Required<Pick<WhisProps, "leftArm" | "rightArm" | "eyes" | "look" | "squash" | "rotate" | "mark" | "finger">>;

/**
 * A pose animated at `frame` (any frame counter; Beat-local is fine): arm
 * swings, blinks, gaze and marks. Spread into <Whis> and override as needed:
 *   <Whis x={..} y={..} {...whisPose("typing", frame, fps)} />
 */
export function whisPose(name: WhisPoseName, frame: number, fps = 30): PoseProps {
  const base = WHIS_POSES[name];
  const s = frame / fps; // seconds
  const blink = isBlinking(frame, fps) ? "closed" : "open";
  const still = { ...base, eyes: blink, look: [0, 0], squash: 1, rotate: 0, mark: "none", finger: false } as const;
  switch (name) {
    case "idle":
      return { ...still, squash: 1 + 0.012 * Math.sin(s * Math.PI * 1.2) };
    case "typing": {
      // Arms hammer alternately at ~5 strokes/s; eyes down on the keys.
      const tap = Math.sin(s * Math.PI * 5);
      return {
        ...still,
        leftArm: base.leftArm + 9 * tap,
        rightArm: base.rightArm - 9 * tap,
        look: [0.35, 0.8],
        rotate: 3,
        squash: 1 - 0.015 * Math.abs(tap),
      };
    }
    case "scratch":
      return {
        ...still,
        rightArm: base.rightArm + 10 * Math.sin(s * Math.PI * 4.5),
        look: [-0.5, -0.8],
        rotate: -3,
        mark: "question",
      };
    case "proud":
      return { ...still, eyes: "happy", squash: 1.06 + 0.01 * Math.sin(s * Math.PI * 2), mark: "sparkles" };
    case "wave":
      return { ...still, rightArm: base.rightArm + 18 * Math.sin(s * Math.PI * 2.4), look: [0.1, -0.2] };
    case "point":
      return {
        ...still,
        rightArm: base.rightArm + 3 * Math.sin(s * Math.PI * 1.5),
        look: [1, -0.1],
        rotate: 2,
        finger: true,
      };
  }
}

/* Geometry at scale 1, origin = ground point between the feet, y up = negative. */
const BODY_W = 220;
const BODY_H = 170;
const LEG_H = 30;
const LEG_W = 22;
const LEG_CENTRES = [-76, -42, 42, 76];
const BODY_TOP = -(LEG_H + BODY_H);
const BODY_BOTTOM = -LEG_H + 2; // legs tuck slightly under the body
const ARM_LEN = 38;
const ARM_H = 30;
/** Arm pivot height (centre of the shoulder), in the lower half of the body. */
const SHOULDER_Y = BODY_TOP + 100;
const EYE_W = 16;
const EYE_H = 26;
const EYE_X = 46;
const EYE_Y = BODY_TOP + 18;
/** Light paper rim around every block, so the black body stays separate from a dark (night) wash. */
const RIM = 4;

/**
 * The app icon's waveform W: fifteen bars whose tops trace the monogram.
 * Heights relative to the tallest bar, measured from resources' icon.
 */
const W_BARS = [1, 0.76, 0.52, 0.29, 0.22, 0.37, 0.52, 0.67, 0.52, 0.37, 0.22, 0.29, 0.52, 0.76, 0.97];
const W_SPAN = 140;
const W_BOTTOM = BODY_BOTTOM - 16;
const W_HEIGHT = 94;

const outline = (seed: number) => ({
  stroke: PALETTE.graphite,
  strokeWidth: 3.2,
  roughness: 1.1,
  bowing: 0.8,
  seed,
});

/** Light pencil hatching over the black crayon: gives the body its hand-coloured texture. */
const hatch = {
  stroke: "none",
  fill: PALETTE.whisWhite,
  fillStyle: "hachure",
  hachureAngle: -52,
  hachureGap: 9,
  fillWeight: 1.6,
  roughness: 1.6,
} as const;

export const Whis: React.FC<WhisProps> = ({
  x,
  y,
  scale = 1,
  rotate = 0,
  flip = false,
  leftArm = WHIS_POSES.idle.leftArm,
  rightArm = WHIS_POSES.idle.rightArm,
  eyes = "open",
  look = [0, 0],
  squash = 1,
  legs = true,
  mark = "none",
  finger = false,
  draw = 1,
  seed: baseSeed = 7,
  wobble = true,
  opacity = 1,
}) => {
  const crayonId = `whis-crayon-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const seed = useWobbleSeed(baseSeed, 4, !wobble || draw < 1);
  const outlineP = ramp(draw, 0, 0.6);
  const fillP = ramp(draw, 0.45, 0.85);
  const eyesP = ramp(draw, 0.55, 0.8);
  const wP = ramp(draw, 0.7, 1);
  const stretch = 1 + (1 - squash) * 0.5;
  const sx = scale * (flip ? -1 : 1) * stretch;
  const sy = scale * squash;

  // Blocks in local coordinates; arms carry their rotation.
  const armBlock = (side: -1 | 1, angle: number) => {
    // The arm starts inside the body so the shoulder never shows a gap when rotated.
    const x0 = side < 0 ? -BODY_W / 2 - ARM_LEN : BODY_W / 2 - 10;
    const pivotX = side * (BODY_W / 2 - 4);
    // Screen-space rotation: raising the left arm is clockwise, the right arm counter-clockwise.
    const deg = side < 0 ? angle : -angle;
    return { x: x0, y: SHOULDER_Y - ARM_H / 2, w: ARM_LEN + 10, h: ARM_H, transform: `rotate(${deg} ${pivotX} ${SHOULDER_Y})` };
  };
  const legBlocks = legs ? LEG_CENTRES.map((cx) => ({ x: cx - LEG_W / 2, y: BODY_BOTTOM - 4, w: LEG_W, h: LEG_H + 2 })) : [];
  const right = armBlock(1, rightArm);
  const arms = [armBlock(-1, leftArm), right];
  if (finger) {
    // A stubby pointing finger off the upper edge of the right arm's tip.
    arms.push({ x: right.x + right.w - 3, y: right.y + 2, w: 24, h: 12, transform: right.transform });
  }
  const rimOpacity = outlineP;

  return (
    <g transform={`translate(${x} ${y}) rotate(${rotate})`} opacity={opacity}>
      <g transform={`scale(${sx} ${sy})`}>
        <defs>
          {/* Waxy crayon: punch grain holes into the fill and roughen its edges. */}
          <filter id={crayonId} x="-15%" y="-15%" width="130%" height="130%">
            <feTurbulence type="fractalNoise" baseFrequency="0.9 0.35" numOctaves={2} seed={baseSeed} result="grain" />
            <feColorMatrix
              in="grain"
              type="matrix"
              values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 -2.4 1.9"
              result="mask"
            />
            <feComposite in="SourceGraphic" in2="mask" operator="in" result="waxy" />
            <feTurbulence type="fractalNoise" baseFrequency="0.045" numOctaves={2} seed={baseSeed + 1} result="warp" />
            <feDisplacementMap in="waxy" in2="warp" scale={6} xChannelSelector="R" yChannelSelector="G" />
          </filter>
        </defs>

        {/* Paper rims first, so no block's rim cuts across another block. */}
        <g opacity={rimOpacity} fill={PALETTE.paper}>
          {legBlocks.map((b, i) => (
            <rect key={i} x={b.x - RIM} y={b.y} width={b.w + 2 * RIM} height={b.h + RIM} rx={RIM} />
          ))}
          {arms.map((b, i) => (
            <rect key={i} x={b.x - RIM} y={b.y - RIM} width={b.w + 2 * RIM} height={b.h + 2 * RIM} rx={RIM} transform={b.transform} />
          ))}
          <rect x={-BODY_W / 2 - RIM} y={BODY_TOP - RIM} width={BODY_W + 2 * RIM} height={BODY_H + 2 * RIM} rx={RIM} />
        </g>

        {legBlocks.map((b, i) => (
          <g key={i}>
            <CrayonBlock {...b} p={fillP} seed={seed + 20 + i} filter={crayonId} />
            <RoughDrawing
              seed={seed + 20 + i}
              options={outline(seed + 20 + i)}
              progress={outlineP}
              build={(g, o) => [
                g.linearPath(
                  [
                    [b.x, BODY_BOTTOM],
                    [b.x, 0],
                    [b.x + b.w, 0],
                    [b.x + b.w, BODY_BOTTOM],
                  ],
                  o,
                ),
              ]}
            />
          </g>
        ))}

        {arms.map((b, i) => (
          <g key={i} transform={b.transform}>
            <CrayonBlock {...b} p={fillP} seed={seed + 30 + i} filter={crayonId} />
            <RoughDrawing
              seed={seed + 30 + i}
              options={outline(seed + 30 + i)}
              progress={outlineP}
              build={(g, o) => [g.rectangle(b.x, b.y, b.w, b.h, o)]}
            />
          </g>
        ))}

        {/* Opaque underlay so arms and legs tucked behind the body never show through the waxy fill. */}
        <rect x={-BODY_W / 2 + 2} y={BODY_TOP + 2} width={BODY_W - 4} height={BODY_H - 4} fill={PALETTE.paper} opacity={outlineP} />
        <CrayonBlock x={-BODY_W / 2} y={BODY_TOP} w={BODY_W} h={BODY_H} p={fillP} seed={seed} filter={crayonId} />
        <RoughDrawing
          seed={seed}
          options={outline(seed)}
          progress={outlineP}
          build={(g, o) => [g.rectangle(-BODY_W / 2, BODY_TOP, BODY_W, BODY_H, o)]}
        />

        <WaveformW progress={wP} seed={seed + 50} />

        <g opacity={eyesP} transform={`translate(${look[0] * 12} ${look[1] * 6})`}>
          {[-1, 1].map((side) => (
            <Eye key={side} cx={side * EYE_X} top={EYE_Y} shape={eyes} seed={seed + 40 + side} />
          ))}
        </g>
      </g>

      {/* Marks sit outside the mirrored group so the "?" never reads backwards. */}
      <g transform={`scale(${scale})`}>
        <Mark mark={mark} side={flip ? -1 : 1} draw={eyesP} seed={seed + 60} />
      </g>
    </g>
  );
};

/** Black crayon fill for one block: a waxy flat layer plus light diagonal hatching. */
const CrayonBlock: React.FC<{ x: number; y: number; w: number; h: number; p: number; seed: number; filter: string }> = ({
  x,
  y,
  w,
  h,
  p,
  seed,
  filter,
}) => {
  if (p <= 0) return null;
  return (
    <g opacity={p}>
      {/* A dense charcoal base, then waxy black crayon whose grain lets the base show through. */}
      <rect x={x + 1} y={y + 1} width={w - 2} height={h - 2} fill={PALETTE.whisBlack} opacity={0.82} />
      <rect x={x + 2} y={y + 2} width={w - 4} height={h - 4} fill={PALETTE.whisBlack} filter={`url(#${filter})`} />
      <g opacity={0.16}>
        <RoughDrawing seed={seed + 100} options={hatch} build={(g, o) => [g.rectangle(x + 3, y + 3, w - 6, h - 6, o)]} />
      </g>
    </g>
  );
};

/** The app icon's white waveform W, pencilled bar by bar across the front of the body. */
const WaveformW: React.FC<{ progress: number; seed: number }> = ({ progress, seed }) => {
  if (progress <= 0) return null;
  const step = W_SPAN / (W_BARS.length - 1);
  return (
    <RoughDrawing
      seed={seed}
      options={{ stroke: PALETTE.whisWhite, strokeWidth: 6.5, roughness: 0.55, bowing: 0.4, disableMultiStroke: true }}
      progress={progress}
      build={(g, o) =>
        W_BARS.map((h, i) => {
          const bx = -W_SPAN / 2 + i * step;
          return g.line(bx, W_BOTTOM, bx, W_BOTTOM - h * W_HEIGHT, o);
        })
      }
    />
  );
};

const Eye: React.FC<{ cx: number; top: number; shape: WhisEyes; seed: number }> = ({ cx, top, shape, seed }) => {
  const ink = { stroke: PALETTE.whisWhite, strokeWidth: 5, roughness: 0.8, seed };
  if (shape === "closed") {
    return (
      <RoughDrawing seed={seed} options={ink} build={(g, o) => [g.line(cx - EYE_W / 2 - 3, top + EYE_H * 0.62, cx + EYE_W / 2 + 3, top + EYE_H * 0.62, o)]} />
    );
  }
  if (shape === "happy") {
    // ^ ^ : an upturned chevron roughly the width of an open eye.
    return (
      <RoughDrawing
        seed={seed}
        options={ink}
        build={(g, o) => [
          g.linearPath(
            [
              [cx - EYE_W / 2 - 5, top + EYE_H * 0.7],
              [cx, top + EYE_H * 0.3],
              [cx + EYE_W / 2 + 5, top + EYE_H * 0.7],
            ],
            o,
          ),
        ]}
      />
    );
  }
  return (
    <RoughDrawing
      seed={seed}
      options={{ ...ink, strokeWidth: 2.5, fill: PALETTE.whisWhite, fillStyle: "solid" }}
      build={(g, o) => [g.rectangle(cx - EYE_W / 2, top, EYE_W, EYE_H, o)]}
    />
  );
};

/** Sparkle rays around the body, in Whis's local (unflipped) coordinates. */
const SPARKLES: [number, number, number][] = [
  [-150, -190, 150],
  [-170, -125, 190],
  [150, -190, 30],
  [170, -125, -10],
];

const Mark: React.FC<{ mark: WhisMark; side: -1 | 1; draw: number; seed: number }> = ({ mark, side, draw, seed }) => {
  if (mark === "none" || draw <= 0) return null;
  if (mark === "question") {
    // Pencilled "?" above the scratching arm, rocking gently with the wobble seed.
    const qx = side * 150;
    const qy = BODY_TOP - 10;
    const rock = ((seed % 7) - 3) * 1.5;
    return (
      <text
        x={qx}
        y={qy}
        fontFamily={`"${HAND_FONT}", serif`}
        fontSize={84}
        fill={PALETTE.graphite}
        stroke={PALETTE.paper}
        strokeWidth={6}
        paintOrder="stroke"
        opacity={draw}
        textAnchor="middle"
        transform={`rotate(${side * 12 + rock} ${qx} ${qy - 30})`}
      >
        ?
      </text>
    );
  }
  return (
    <RoughDrawing
      seed={seed}
      options={{ stroke: PALETTE.redPen, strokeWidth: 4.5, roughness: 0.8 }}
      progress={draw}
      build={(g, o) =>
        SPARKLES.map(([dx, dy, deg]) => {
          const r = (deg * Math.PI) / 180;
          const [ux, uy] = [Math.cos(r), -Math.sin(r)];
          return g.line(dx, dy, dx + ux * 34, dy + uy * 34, o);
        })
      }
    />
  );
};
