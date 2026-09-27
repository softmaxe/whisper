import { AbsoluteFill, useCurrentFrame, useVideoConfig } from "remotion";
import { Paper } from "../components/Paper.tsx";
import { HAND_FONT } from "../fonts.ts";
import { RoughDrawing, useWobbleSeed } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";
import { Whis, WHIS_POSE_NAMES, whisPose, type WhisPoseName } from "./Whis.tsx";

/** Frames in the WhisSheet composition (the build's still frame is in scripts/paths.ts). */
export const WHIS_SHEET_FRAMES = 90;

const LABELS: Record<WhisPoseName, string> = {
  idle: "idle",
  typing: "typing",
  scratch: "scratching its head",
  proud: "proud",
  wave: "waving",
  point: "pointing",
};

/**
 * Stress-test washes: stronger than the Film's watercolour wash is likely to
 * get, so a Whis that reads here reads in every Beat. Whis is drawn over the
 * wash, as in the Film (Paper → Wash → Beat visuals).
 */
const WASHES = [
  { label: "day wash", colour: PALETTE.washDay, opacity: 0.55, pose: "wave" as const },
  { label: "night wash", colour: PALETTE.washNight, opacity: 0.75, pose: "proud" as const },
];

const PANEL = { x: 1390, w: 470, h: 400, ys: [130, 590] };

/**
 * Model sheet for Whis: every pose, animated with its stroke wobble, plus
 * Whis over a day and a night wash. Not part of the Film; registered as the
 * "WhisSheet" composition and exported by the build as
 * out/frames/whis-sheet.png.
 */
export const WhisSheet: React.FC = () => {
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
          Whis
        </text>
        <text x={250} y={110} fontFamily={`"${HAND_FONT}", serif`} fontSize={34} fill={PALETTE.pencil}>
          Whisper's mascot · model sheet
        </text>

        {WHIS_POSE_NAMES.map((name, i) => {
          const cx = 250 + (i % 3) * 400;
          const cy = 480 + Math.floor(i / 3) * 440;
          return (
            <g key={name}>
              <Whis x={cx} y={cy} seed={11 + i} {...whisPose(name, frame, fps)} />
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
              <Whis x={PANEL.x + PANEL.w / 2} y={top + PANEL.h - 70} seed={31 + i} {...whisPose(w.pose, frame, fps)} />
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
