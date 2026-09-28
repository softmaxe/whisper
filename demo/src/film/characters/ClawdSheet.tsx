import { AbsoluteFill, useCurrentFrame, useVideoConfig } from "remotion";
import { Paper } from "../components/Paper.tsx";
import { HAND_FONT } from "../fonts.ts";
import { RoughDrawing, useWobbleSeed } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";
import { Clawd, CLAWD_POSE_NAMES, clawdPose, type ClawdPoseName } from "./Clawd.tsx";

/** Frames in the ClawdSheet composition (the build's still frame is in scripts/paths.ts). */
export const CLAWD_SHEET_FRAMES = 90;

const LABELS: Record<ClawdPoseName, string> = {
  idle: "idle",
  typing: "typing",
  scratch: "scratching its head",
  proud: "proud",
  wave: "waving",
  point: "pointing",
};

/**
 * Stress-test washes: stronger than the Film's watercolour wash is likely to
 * get, so a Clawd that reads here reads in every Beat. Clawd is drawn over the
 * wash, as in the Film (Paper → Wash → Beat visuals).
 */
const WASHES = [
  { label: "day wash", colour: PALETTE.washDay, opacity: 0.55, pose: "wave" as const },
  { label: "night wash", colour: PALETTE.washNight, opacity: 0.75, pose: "proud" as const },
];

const PANEL = { x: 1390, w: 470, h: 400, ys: [130, 590] };

/**
 * Model sheet for Clawd: every pose, animated with its stroke wobble, plus
 * Clawd over a day and a night wash. Not part of the Film; registered as the
 * "ClawdSheet" composition and exported by the build as
 * out/frames/clawd-sheet.png.
 */
export const ClawdSheet: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const frameSeed = useWobbleSeed(900);
  return (
    <AbsoluteFill>
      <Paper />
      {WASHES.map((w, i) => (
        <div
          key={w.label}
          style={{
            position: "absolute",
            left: PANEL.x,
            top: PANEL.ys[i],
            width: PANEL.w,
            height: PANEL.h,
            background: w.colour,
            opacity: w.opacity,
            mixBlendMode: "multiply",
          }}
        />
      ))}
      <svg width={1920} height={1080} style={{ position: "absolute", inset: 0 }}>
        <text x={90} y={110} fontFamily={`"${HAND_FONT}", serif`} fontSize={64} fill={PALETTE.ink}>
          Clawd
        </text>
        <text x={300} y={110} fontFamily={`"${HAND_FONT}", serif`} fontSize={34} fill={PALETTE.pencil}>
          Whisper's mascot · model sheet
        </text>

        {CLAWD_POSE_NAMES.map((name, i) => {
          const cx = 250 + (i % 3) * 400;
          const cy = 480 + Math.floor(i / 3) * 440;
          return (
            <g key={name}>
              <Clawd x={cx} y={cy} seed={11 + i} {...clawdPose(name, frame, fps)} />
              <text x={cx} y={cy + 66} textAnchor="middle" fontFamily={`"${HAND_FONT}", serif`} fontSize={36} fill={PALETTE.pencil}>
                {LABELS[name]}
              </text>
            </g>
          );
        })}

        {WASHES.map((w, i) => {
          const top = PANEL.ys[i];
          return (
            <g key={w.label}>
              <RoughDrawing
                seed={frameSeed + i}
                options={{ stroke: PALETTE.graphite, strokeWidth: 2.5, roughness: 1.2 }}
                build={(g, o) => [g.rectangle(PANEL.x, top, PANEL.w, PANEL.h, o)]}
              />
              <Clawd x={PANEL.x + PANEL.w / 2} y={top + PANEL.h - 70} seed={31 + i} {...clawdPose(w.pose, frame, fps)} />
              <text x={PANEL.x + 16} y={top - 14} fontFamily={`"${HAND_FONT}", serif`} fontSize={32} fill={PALETTE.pencil}>
                {w.label}
              </text>
            </g>
          );
        })}
      </svg>
    </AbsoluteFill>
  );
};
