import { ramp } from "../../anim.ts";
import { AppWindow } from "../../chrome/AppWindow.tsx";
import { useCopy } from "../../../lib/copy-context.tsx";
import { revealText } from "../../../lib/text.ts";
import { PenMark } from "../../pen/RedPen.tsx";
import { PALETTE } from "../../theme.ts";
import { NOTE_WINDOW } from "./PinnedNote.tsx";

/** Pencilled message bubbles and the snippet's link colour. */
const THEIRS = "#efe5d0";
const MINE = "#dce6f7";
export const LINK_BLUE = "#2f5fc4";

export interface SnippetNoteProps {
  t: number;
  /** Window sketch-in 0..1. */
  progress: number;
  /** Film time the pill opens (a caret blinks in the composer from then). */
  listenAt: number;
  /** Film time the expanded text lands in the composer. */
  pasteAt: number;
  /** Film time the red pen starts underlining the expanded link. */
  underlineAt: number;
}

/**
 * The Snippet note's Messages window: a chat with Priya ending in her
 * question, and the composer where the reply lands with the trigger phrase
 * already expanded into the full link (blue, then underlined in red pen).
 */
export const SnippetNote: React.FC<SnippetNoteProps> = ({ t, progress, listenAt, pasteAt, underlineAt }) => {
  const copy = useCopy();
  const { snippet } = copy;
  const reveal = ramp(t, pasteAt, pasteAt + 0.4);
  const before = revealText(snippet.before, Math.min(1, reveal * 2));
  const expansion = revealText(snippet.expansion, Math.max(0, reveal * 2 - 1));
  const glow = ramp(t, pasteAt + 0.2, pasteAt + 0.5);
  const caret = t >= listenAt && (t >= pasteAt || Math.floor(t * 2.4) % 2 === 0);
  const messages = [...snippet.earlier, { mine: false, text: snippet.incoming }];

  return (
    <AppWindow box={NOTE_WINDOW} title={snippet.app} progress={progress} seed={331} contentStyle={{ padding: 0 }}>
      <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
        <div
          style={{
            padding: "12px 32px",
            borderBottom: `2px solid ${PALETTE.paperShade}`,
            color: PALETTE.pencil,
            fontSize: 26,
            fontWeight: 600,
          }}
        >
          {snippet.contact}
        </div>
        <div style={{ flex: 1, padding: "18px 32px", display: "flex", flexDirection: "column", justifyContent: "flex-end", gap: 14 }}>
          {messages.map((message, i) => (
            <div
              key={i}
              style={{
                alignSelf: message.mine ? "flex-end" : "flex-start",
                maxWidth: 760,
                padding: "12px 22px",
                borderRadius: 26,
                border: `2px solid ${PALETTE.pencil}`,
                background: message.mine ? MINE : THEIRS,
                fontSize: 28,
              }}
            >
              {message.text}
            </div>
          ))}
        </div>
        <div style={{ padding: "16px 28px 22px", borderTop: `2px solid ${PALETTE.paperShade}` }}>
          <div
            style={{
              padding: "12px 24px",
              borderRadius: 32,
              border: `2.5px solid ${PALETTE.pencil}`,
              background: PALETTE.whitePaper,
              fontSize: 32,
              minHeight: 46,
              whiteSpace: "nowrap",
            }}
          >
            {before}
            {expansion && (
              <PenMark kind="underline" progress={ramp(t, underlineAt, underlineAt + 0.35)} seed={351} pad={8}>
                <span
                  style={{
                    color: LINK_BLUE,
                    background: `rgba(91,134,245,${0.18 * glow})`,
                    borderRadius: 6,
                    padding: "0 4px",
                  }}
                >
                  {expansion}
                </span>
              </PenMark>
            )}
            <span
              style={{
                display: "inline-block",
                width: 3,
                height: 36,
                marginLeft: 3,
                verticalAlign: "text-bottom",
                background: PALETTE.ink,
                opacity: caret ? 1 : 0,
              }}
            />
          </div>
        </div>
      </div>
    </AppWindow>
  );
};
