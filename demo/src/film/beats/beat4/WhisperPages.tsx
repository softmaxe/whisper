/**
 * The Whisper main window's pages for Beat 4, drawn as HTML inside an
 * AppWindow: the sidebar nav, History (today's Dictations with search) and
 * Insights (usage values and the activity chart). Labels and sample data come
 * from the copy module, so both cuts show the app's own words.
 */
import { interpolate } from "remotion";
import { BARS_DRAW_SECONDS, beat4 as beat } from "../../../../timeline/beats/beat4-review.ts";
import { keystrokeTimes } from "../../../../timeline/helpers.ts";
import { clamp, ramp } from "../../anim.ts";
import { useCopy } from "../../../lib/copy-context.tsx";
import { PenCircle, PenMark, PenNote } from "../../pen/RedPen.tsx";
import { RoughDrawing, useWobbleSeed } from "../../rough/RoughDrawing.tsx";
import { PALETTE } from "../../theme.ts";

const m = beat.moments;

/** Highlighter yellow for the search match. */
const MARKER = "rgba(244, 196, 64, 0.78)";
/** Pencil blue for the focused search field. */
const FOCUS = "#5b7fbf";
/** Relative heights of the activity bars, oldest first; the last is today. */
export const ACTIVITY = [0.34, 0.52, 0.28, 0.6, 0.72, 0.18, 0.14, 0.46, 0.64, 0.5, 0.82, 0.3, 0.22, 0.95] as const;

type NavKey = "home" | "insights" | "upload" | "dictionary" | "settings";

/** The main window's sidebar nav, with the current page shaded. */
export const Sidebar: React.FC<{ active: NavKey; activeAt: number }> = ({ active, activeAt }) => {
  const { whisper } = useCopy();
  const keys: NavKey[] = ["home", "insights", "upload", "dictionary", "settings"];
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6, padding: "18px 10px" }}>
      {keys.map((key) => (
        <div
          key={key}
          style={{
            padding: "10px 16px",
            borderRadius: 10,
            fontSize: 24,
            color: key === active ? PALETTE.ink : PALETTE.pencil,
            fontWeight: key === active ? 600 : 400,
            background: key === active ? `rgba(230, 214, 180, ${0.9 * activeAt})` : undefined,
          }}
        >
          {whisper.nav[key]}
        </div>
      ))}
    </div>
  );
};

/** History: the search field, today's Dictations, and the highlighted match. */
export const HistoryPage: React.FC<{ t: number }> = ({ t }) => {
  const { whisper } = useCopy();
  const [typing] = beat.typing;
  const keys = keystrokeTimes(typing);
  const typed = keys.filter((k) => k <= t).length;
  const queryChars = [...whisper.query];
  const query = queryChars.slice(0, Math.round((typed / keys.length) * queryChars.length)).join("");
  const focus = ramp(t, m.searchOpen, m.searchOpen + 0.25);
  const caret = t >= m.searchOpen && t < m.filter + 0.5 && Math.floor(t * 2.6) % 2 === 0;
  const dim = interpolate(t, [m.filter, m.filter + 0.4], [1, 0.28], clamp);
  const markerP = ramp(t, m.highlight, m.highlight + 0.35);
  const underlineP = ramp(t, m.highlight + 0.25, m.highlight + 0.7);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 14,
          height: 58,
          padding: "0 18px",
          borderRadius: 14,
          border: `2.5px solid ${focus > 0.5 ? FOCUS : PALETTE.paperShade}`,
          boxShadow: `0 0 0 ${4 * focus}px rgba(91, 127, 191, 0.18)`,
          fontSize: 28,
        }}
      >
        <svg width={26} height={26} viewBox="0 0 24 24" fill="none" stroke={PALETTE.pencil} strokeWidth={2.4} strokeLinecap="round">
          <circle cx={10.5} cy={10.5} r={6.5} />
          <path d="M20 20l-4.6-4.6" />
        </svg>
        <span style={{ whiteSpace: "pre" }}>{query}</span>
        <span style={{ width: 3, height: 32, marginLeft: -12, background: FOCUS, opacity: caret ? 1 : 0 }} />
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 16, fontSize: 20, color: PALETTE.pencil, marginTop: 4 }}>
        <span style={{ fontWeight: 600 }}>{whisper.today}</span>
        <span style={{ flex: 1, height: 2, background: PALETTE.paperShade }} />
        <span>{whisper.showDiscarded}</span>
      </div>
      {whisper.history.map((entry, i) => {
        const match = entry.text.includes(whisper.query);
        return (
          <div
            key={entry.time}
            style={{
              display: "flex",
              gap: 28,
              padding: "10px 14px",
              borderRadius: 12,
              fontSize: 25,
              lineHeight: 1.45,
              opacity: match ? 1 : dim,
              background: match ? `rgba(251, 246, 234, ${1 - dim})` : undefined,
            }}
          >
            <span style={{ color: PALETTE.pencil, fontSize: 21, width: 64, paddingTop: 3, fontVariantNumeric: "tabular-nums" }}>
              {entry.time}
            </span>
            <span style={{ flex: 1 }}>
              {match ? highlight(entry.text, whisper.query, markerP, underlineP, 700 + i * 10) : entry.text}
            </span>
          </div>
        );
      })}
    </div>
  );
};

/** Wraps every occurrence of `query` in a highlighter sweep with a red-pen underline. */
function highlight(text: string, query: string, markerP: number, underlineP: number, seed: number) {
  const parts = text.split(query);
  return parts.map((part, i) => (
    <span key={i}>
      {part}
      {i < parts.length - 1 && (
        <PenMark kind="underline" progress={underlineP} seed={seed + i} pad={8}>
          <span
            style={{
              padding: "0 3px",
              borderRadius: 4,
              background: `linear-gradient(${MARKER}, ${MARKER}) no-repeat 0 65% / ${markerP * 100}% 78%`,
            }}
          >
            {query}
          </span>
        </PenMark>
      )}
    </span>
  ));
}

/** Insights: the title, four usage values written by hand, and the hand-drawn activity bars. */
export const InsightsPage: React.FC<{ t: number; lang: string }> = ({ t, lang }) => {
  const { whisper } = useCopy();
  const { insights } = whisper;
  const v = insights.values;
  const cards = [
    { label: insights.words, value: v.words.toLocaleString(lang), note: insights.allTime },
    { label: insights.streak, value: insights.days(v.streak), note: "" },
    { label: insights.wpm, value: v.wpm.toLocaleString(lang), note: "" },
    { label: insights.dictations, value: v.dictations.toLocaleString(lang), note: "" },
  ];
  const card: React.CSSProperties = {
    borderRadius: 16,
    border: `2px solid ${PALETTE.paperShade}`,
    background: PALETTE.cream,
    padding: "16px 20px",
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
        <span style={{ fontSize: 32, fontWeight: 700 }}>{insights.title}</span>
        <span style={{ fontSize: 18, padding: "4px 12px", borderRadius: 12, background: PALETTE.cream, color: PALETTE.pencil }}>
          {insights.onDevice}
        </span>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(4, minmax(0, 1fr))", gap: 16 }}>
        {cards.map((c, i) => (
          <div key={c.label} style={{ ...card, height: 132 }}>
            <div style={{ fontSize: 20, color: PALETTE.pencil, whiteSpace: "nowrap" }}>{c.label}</div>
            <svg width={170} height={78} style={{ overflow: "visible" }}>
              <PenNote
                x={2}
                y={62}
                text={c.value}
                progress={ramp(t, m.values[i], m.values[i] + 0.45)}
                fontSize={48}
                rotate={-1.5}
                color={PALETTE.graphite}
              />
            </svg>
          </div>
        ))}
      </div>
      <div style={{ ...card, padding: "16px 20px 10px" }}>
        <div style={{ fontSize: 20, color: PALETTE.pencil }}>{insights.activity}</div>
        <ActivityBars t={t} today={whisper.today} />
      </div>
    </div>
  );
};

const CHART = { w: 860, h: 250, base: 214, left: 20, gap: 22 };

/** The activity bars, drawn one after another in pencil hatching; today's bar is circled in red pen. */
const ActivityBars: React.FC<{ t: number; today: string }> = ({ t, today }) => {
  const drawP = ramp(t, m.bars, m.bars + BARS_DRAW_SECONDS);
  const seed = useWobbleSeed(90, 4, drawP < 1);
  const barW = (CHART.w - CHART.left * 2 - CHART.gap * (ACTIVITY.length - 1)) / ACTIVITY.length;
  const maxH = CHART.base - 44;
  const lastX = CHART.left + (ACTIVITY.length - 1) * (barW + CHART.gap);
  const lastH = ACTIVITY.at(-1)! * maxH;
  const circleP = ramp(t, m.today, m.today + 0.45);
  const noteP = ramp(t, m.today + 0.35, m.today + 0.75);

  return (
    <svg width={CHART.w} height={CHART.h} style={{ overflow: "visible", display: "block" }}>
      <RoughDrawing
        seed={seed}
        progress={drawP}
        options={{ stroke: PALETTE.graphite, strokeWidth: 2.4, roughness: 1.2, bowing: 0.8 }}
        deps={[barW]}
        build={(g, o) => [
          g.line(4, CHART.base, CHART.w - 4, CHART.base, o),
          ...ACTIVITY.map((h, i) =>
            g.rectangle(CHART.left + i * (barW + CHART.gap), CHART.base - h * maxH, barW, h * maxH, {
              ...o,
              fill: i === ACTIVITY.length - 1 ? PALETTE.redPen : PALETTE.pencil,
              fillStyle: "hachure",
              hachureGap: 6,
              hachureAngle: -41,
              fillWeight: 1.6,
            }),
          ),
        ]}
      />
      <PenCircle
        cx={lastX + barW / 2}
        cy={CHART.base - lastH / 2}
        rx={barW * 0.95 + 8}
        ry={lastH / 2 + 18}
        progress={circleP}
        seed={95}
      />
      <PenNote x={lastX - 34} y={CHART.base - lastH + 8} text={today} progress={noteP} fontSize={40} align="end" />
    </svg>
  );
};
