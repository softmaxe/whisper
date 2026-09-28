import { ramp } from "../../anim.ts";
import { AppWindow } from "../../chrome/AppWindow.tsx";
import { useCopy } from "../../../lib/copy-context.tsx";
import { revealText } from "../../../lib/text.ts";
import { PenTick } from "../../pen/RedPen.tsx";
import { PALETTE } from "../../theme.ts";
import { NOTE_WINDOW } from "./PinnedNote.tsx";

/** Code editor text: monospace, with CJK falling back to the system face. */
const MONO = '"SF Mono", Menlo, Monaco, "PingFang SC", monospace';
const CODE_SIZE = 27;
export const CODE_LINE = 46;
/** Muted pencil syntax colours on paper. */
const KEYWORD = "#8a4fa8";
const STRING = "#a2672a";
const COMMENT = "#4f8a4a";
const HIGHLIGHT = "rgba(91,134,245,";
const SIDEBAR = 250;
const KEYWORDS = /\b(export|async|function|const|await|return)\b/g;

export interface HoldNoteProps {
  t: number;
  /** Window sketch-in 0..1. */
  progress: number;
  /** While fn is held (listen..release) the blank line waits, highlighted, with a caret. */
  listenAt: number;
  /** Film time the comment lands on the blank line. */
  pasteAt: number;
  /** Film time the red pen ticks it. */
  tickAt: number;
}

/**
 * The Hold mode note's code editor: `upload.ts` with a blank line where the
 * dictated comment lands on release, then a red-pen tick after it.
 */
export const HoldNote: React.FC<HoldNoteProps> = ({ t, progress, listenAt, pasteAt, tickAt }) => {
  const { hold } = useCopy();
  const blank = hold.code.indexOf("");
  const typed = revealText(hold.comment, ramp(t, pasteAt, pasteAt + 0.35));
  const waiting = ramp(t, listenAt, listenAt + 0.2);
  const glow = t < pasteAt ? 0.1 * waiting : 0.1 + 0.14 * (1 - ramp(t, pasteAt + 0.4, pasteAt + 1.2));
  const caret = t >= listenAt && t < pasteAt + 0.35 && Math.floor(t * 2.4) % 2 === 0;
  const files = ["src", "  api.ts", `  ${hold.file}`, "  retry.ts", "test", "package.json"];

  return (
    <AppWindow
      box={NOTE_WINDOW}
      title={hold.file}
      progress={progress}
      seed={361}
      sidebarWidth={SIDEBAR}
      sidebar={
        <div style={{ paddingTop: 14, fontFamily: MONO, fontSize: 22 }}>
          {files.map((name) => {
            const active = name.trim() === hold.file;
            return (
              <div
                key={name}
                style={{
                  padding: "6px 18px",
                  whiteSpace: "pre",
                  color: active ? PALETTE.ink : PALETTE.pencil,
                  fontWeight: active ? 600 : 400,
                  background: active ? PALETTE.paperShade : undefined,
                }}
              >
                {name}
              </div>
            );
          })}
        </div>
      }
      contentStyle={{ padding: "24px 0", fontFamily: MONO, fontSize: CODE_SIZE, lineHeight: `${CODE_LINE}px` }}
    >
      {hold.code.map((line, i) => (
        <div key={i} style={{ display: "flex", background: i === blank ? `${HIGHLIGHT}${glow})` : undefined }}>
          <span style={{ width: 64, textAlign: "right", paddingRight: 26, color: PALETTE.pencil, opacity: 0.7 }}>{i + 1}</span>
          <span style={{ whiteSpace: "pre" }}>
            {i === blank ? (
              <>
                <CodeLine text={typed} />
                <span
                  style={{
                    display: "inline-block",
                    width: 3,
                    height: 32,
                    marginLeft: 2,
                    verticalAlign: "middle",
                    background: "#3d63c9",
                    opacity: caret ? 1 : 0,
                  }}
                />
                {t >= tickAt && (
                  <span style={{ position: "relative", display: "inline-block", width: 80, height: CODE_LINE, verticalAlign: "top" }}>
                    <svg width={80} height={CODE_LINE} style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }}>
                      <PenTick x={48} y={CODE_LINE / 2} size={46} progress={ramp(t, tickAt, tickAt + 0.3)} seed={371} />
                    </svg>
                  </span>
                )}
              </>
            ) : (
              <CodeLine text={line} />
            )}
          </span>
        </div>
      ))}
    </AppWindow>
  );
};

/** One line of code with muted syntax colours (keywords, strings, comments). */
const CodeLine: React.FC<{ text: string }> = ({ text }) => {
  if (text.trimStart().startsWith("//")) return <span style={{ color: COMMENT }}>{text}</span>;
  const parts: { text: string; keyword: boolean }[] = [];
  let last = 0;
  for (const match of text.matchAll(KEYWORDS)) {
    parts.push({ text: text.slice(last, match.index), keyword: false });
    parts.push({ text: match[0], keyword: true });
    last = match.index + match[0].length;
  }
  parts.push({ text: text.slice(last), keyword: false });
  return (
    <>
      {parts.map((part, i) => (
        <span key={i} style={{ color: part.keyword ? KEYWORD : part.text.includes('"') ? STRING : PALETTE.ink }}>
          {part.text}
        </span>
      ))}
    </>
  );
};
