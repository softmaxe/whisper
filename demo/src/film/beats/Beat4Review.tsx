import { AbsoluteFill } from "remotion";
import { beat4 as beat } from "../../../timeline/beats/beat4-review.ts";
import { keystrokeTimes } from "../../../timeline/helpers.ts";
import { ramp, useBeatTime } from "../anim.ts";
import { AppWindow, type Box } from "../chrome/AppWindow.tsx";
import { Clawd, CLAWD_POSES, clawdPose } from "../characters/Clawd.tsx";
import { useLang } from "../lang.tsx";
import { RoughDrawing, useWobbleSeed } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";
import { HistoryPage, InsightsPage, Sidebar } from "./beat4/WhisperPages.tsx";

/* Composition (canvas px). Captions are written below y = 850. */
const GROUND_Y = 832;
const CLAWD_X = 320;
const CLAWD_SCALE = 1.05;
const WINDOW: Box = { x: 620, y: 62, w: 1220, h: 750 };
const SIDEBAR = 230;
/** The pencilled moon and stars in the night sky, top left, clear of the window. */
const MOON = { x: 250, y: 190, r: 62 };
const STARS: readonly (readonly [number, number, number])[] = [
  [420, 130, 16],
  [110, 320, 12],
  [470, 300, 11],
  [150, 110, 10],
];

/**
 * Beat 4 · The day in review. Night falls on the paper (the wash) and a
 * pencilled moon and stars are drawn in. The Whisper main window is sketched
 * in on History; Clawd types a word into search, entries that don't match fade
 * back and the matches are swept with a highlighter and underlined in red pen.
 * The window switches to Insights: the values are written in by hand, the
 * activity bars are drawn, and today's bar is circled while Clawd looks proud.
 */
export const Beat4Review: React.FC = () => {
  const { frame, fps, t } = useBeatTime(beat);
  const lang = useLang();
  const m = beat.moments;
  const [typing] = beat.typing;
  const keys = keystrokeTimes(typing);

  const windowP = ramp(t, m.sketchIn, m.sketchIn + 1.1);
  const clawdP = ramp(t, m.sketchIn + 0.1, m.sketchIn + 1.0);
  const skyP = ramp(t, m.nightfall + 0.4, m.nightfall + 1.8);
  const beatOpacity = 1 - ramp(t, m.beatOut, beat.end);

  // History cross-fades to Insights; the nav shading follows.
  const toInsights = ramp(t, m.insights, m.insights + 0.4);
  const historyOpacity = 1 - toInsights;
  const insightsOpacity = ramp(t, m.insights + 0.2, m.insights + 0.55);

  // Typing: the last keystroke dips an arm.
  const typed = keys.filter((k) => k <= t).length;
  const lastKey = typed > 0 ? keys[typed - 1] : -Infinity;
  const strike = Math.exp(-(t - lastKey) / 0.07);

  return (
    <AbsoluteFill style={{ opacity: beatOpacity }}>
      <svg width={1920} height={1080} style={{ position: "absolute", inset: 0 }}>
        <NightSky progress={skyP} />
      </svg>
      <AppWindow
        box={WINDOW}
        title="Whisper"
        progress={windowP}
        seed={44}
        sidebarWidth={SIDEBAR}
        sidebar={<Sidebar active={toInsights < 0.5 ? "home" : "insights"} activeAt={1} />}
        contentStyle={{ padding: "22px 32px" }}
      >
        <div style={{ position: "absolute", inset: "22px 32px", opacity: historyOpacity }}>
          {historyOpacity > 0 && <HistoryPage t={t} />}
        </div>
        <div style={{ position: "absolute", inset: "22px 32px", opacity: insightsOpacity }}>
          {insightsOpacity > 0 && <InsightsPage t={t} lang={lang} />}
        </div>
      </AppWindow>
      <svg width={1920} height={1080} style={{ position: "absolute", inset: 0 }}>
        <Clawd x={CLAWD_X} y={GROUND_Y} scale={CLAWD_SCALE} seed={24} draw={clawdP} {...clawdAt()} />
      </svg>
    </AbsoluteFill>
  );

  /** Clawd's pose: watching, typing the query, pointing at the matches, watching Insights, proud of today. */
  function clawdAt() {
    if (t < typing.start - 0.3) return { ...clawdPose("idle", frame, fps), look: [1, -0.3] as const };
    if (t < typing.end + 0.25) {
      const right = typed % 2 === 0;
      const base = CLAWD_POSES.typing;
      return {
        ...clawdPose("typing", frame, fps),
        leftArm: base.leftArm - (right ? 0 : 14 * strike),
        rightArm: base.rightArm - (right ? 14 * strike : 0),
      };
    }
    if (t < m.highlight - 0.1) return { ...clawdPose("idle", frame, fps), look: [1, -0.4] as const };
    if (t < m.insights) return clawdPose("point", frame, fps);
    if (t < m.today + 0.15) return { ...clawdPose("idle", frame, fps), look: [1, -0.35] as const };
    return clawdPose("proud", frame, fps);
  }
};

/** A pencilled crescent moon with a pale hatched fill, and a few stars, drawn in as night falls. */
const NightSky: React.FC<{ progress: number }> = ({ progress }) => {
  const seed = useWobbleSeed(470, 4, progress < 1);
  if (progress <= 0) return null;
  const { x, y, r } = MOON;
  // Outer arc of the full moon, inner arc bitten out by an offset circle.
  const crescent = `M ${x + r * 0.35} ${y - r * 0.94} A ${r} ${r} 0 1 0 ${x + r * 0.35} ${y + r * 0.94} A ${r * 0.78} ${r * 0.78} 0 1 1 ${x + r * 0.35} ${y - r * 0.94} Z`;
  return (
    <RoughDrawing
      seed={seed}
      progress={progress}
      options={{ stroke: PALETTE.graphite, strokeWidth: 3, roughness: 1.1, bowing: 0.8 }}
      build={(g, o) => [
        g.path(crescent, { ...o, fill: "#f3e3a6", fillStyle: "hachure", hachureGap: 6, hachureAngle: -35, fillWeight: 2 }),
        ...STARS.flatMap(([sx, sy, s]) => [
          g.line(sx - s, sy, sx + s, sy, { ...o, strokeWidth: 2.4 }),
          g.line(sx, sy - s, sx, sy + s, { ...o, strokeWidth: 2.4 }),
          g.line(sx - s * 0.6, sy - s * 0.6, sx + s * 0.6, sy + s * 0.6, { ...o, strokeWidth: 1.6 }),
          g.line(sx - s * 0.6, sy + s * 0.6, sx + s * 0.6, sy - s * 0.6, { ...o, strokeWidth: 1.6 }),
        ]),
      ]}
    />
  );
};
