import { useCurrentFrame } from "remotion";
import { easeOut, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { graphemes, revealText } from "../lib/text.ts";
import { MONO } from "../theme.ts";
import { TrafficLights, Window } from "./Desktop.tsx";

const Caret = ({ color = "#1d1d1f", height = 30 }: { color?: string; height?: number }) => {
  const frame = useCurrentFrame();
  return Math.floor(frame / 15) % 2 === 0 ? (
    <span
      style={{
        display: "inline-block",
        width: 2.5,
        height,
        background: color,
        verticalAlign: -6,
        marginLeft: 1,
      }}
    />
  ) : null;
};

function Avatar({ name, color, size = 44 }: { name: string; color: string; size?: number }) {
  return (
    <span
      style={{
        width: size,
        height: size,
        borderRadius: size / 3.2,
        background: color,
        color: "#fff",
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        fontWeight: 700,
        fontSize: size * 0.45,
        flexShrink: 0,
      }}
    >
      {graphemes(name)[0]}
    </span>
  );
}

interface ChatProps {
  pasteAt: number;
  /** Selection of the misheard word starts; the correction is typed; message sent. */
  selectAt: number;
  fixAt: number;
  sendAt: number;
}

/** A team chat with the composer where the dictation lands and is corrected. */
export function Chat({ pasteAt, selectAt, fixAt, sendAt }: ChatProps) {
  const frame = useCurrentFrame();
  const { chat } = useCopy();
  const [head, tail] = chat.heard.split(chat.wrong);
  const pasted = frame >= pasteAt;
  const pasteReveal = ramp(frame, pasteAt, 6, (t) => t);
  const selected = frame >= selectAt && frame < fixAt;
  const selection = ramp(frame, selectAt, 6);
  const fixed = revealText(
    chat.right,
    ramp(frame, fixAt, 8, (t) => t)
  );
  const word = frame < fixAt ? chat.wrong : fixed;
  const sent = ramp(frame, sendAt, 10, easeOut);
  const composerText =
    frame < fixAt ? revealText(chat.heard, pasteReveal) : `${head}${word}${tail}`;
  const finalText = `${head}${chat.right}${tail}`;

  return (
    <Window x={200} y={70} width={1200} height={740} bare>
      <div style={{ display: "flex", height: "100%" }}>
        <div style={{ width: 280, background: "#1f3b4d", color: "#d8e6ef", padding: "18px 0" }}>
          <div style={{ padding: "0 20px 22px" }}>
            <TrafficLights />
          </div>
          <div style={{ padding: "0 22px 16px", fontSize: 22, fontWeight: 700, color: "#fff" }}>
            {chat.app}
          </div>
          {chat.channels.map((channel) => (
            <div
              key={channel}
              style={{
                padding: "10px 22px",
                fontSize: 20,
                background: channel === chat.channel ? "#2e5a74" : undefined,
                color: channel === chat.channel ? "#fff" : undefined,
                fontWeight: channel === chat.channel ? 600 : 400,
              }}
            >
              # {channel}
            </div>
          ))}
        </div>
        <div style={{ flex: 1, display: "flex", flexDirection: "column", background: "#fff" }}>
          <div
            style={{
              padding: "22px 30px",
              fontSize: 24,
              fontWeight: 700,
              borderBottom: "1px solid #eee",
            }}
          >
            # {chat.channel}
          </div>
          <div
            style={{
              flex: 1,
              padding: "26px 30px",
              display: "flex",
              flexDirection: "column",
              gap: 24,
              justifyContent: "flex-end",
            }}
          >
            {chat.earlier.map((message, i) => (
              <div key={message.time} style={{ display: "flex", gap: 16 }}>
                <Avatar name={message.name} color={i === 0 ? "#8a6fd1" : "#e76f51"} />
                <div>
                  <div style={{ fontWeight: 700, fontSize: 20 }}>
                    {message.name}{" "}
                    <span style={{ color: "#999", fontWeight: 400, fontSize: 16 }}>
                      {message.time}
                    </span>
                  </div>
                  <div style={{ fontSize: 23, marginTop: 4 }}>{message.text}</div>
                </div>
              </div>
            ))}
            <div style={{ display: "flex", gap: 16 }}>
              <Avatar name={chat.teammate} color="#e76f51" />
              <div>
                <div style={{ fontWeight: 700, fontSize: 20 }}>
                  {chat.teammate}{" "}
                  <span style={{ color: "#999", fontWeight: 400, fontSize: 16 }}>9:28</span>
                </div>
                <div style={{ fontSize: 23, marginTop: 4 }}>{chat.question}</div>
              </div>
            </div>
            {sent > 0 && (
              <div
                style={{
                  display: "flex",
                  gap: 16,
                  opacity: sent,
                  transform: `translateY(${(1 - sent) * 30}px)`,
                }}
              >
                <Avatar name={chat.you} color="#2f8f83" />
                <div>
                  <div style={{ fontWeight: 700, fontSize: 20 }}>
                    {chat.you}{" "}
                    <span style={{ color: "#999", fontWeight: 400, fontSize: 16 }}>9:30</span>
                  </div>
                  <div style={{ fontSize: 23, marginTop: 4 }}>{finalText}</div>
                </div>
              </div>
            )}
          </div>
          <div
            style={{
              margin: "0 30px 26px",
              padding: "18px 22px",
              border: "2px solid #d6d9de",
              borderRadius: 14,
              fontSize: 24,
              minHeight: 34,
            }}
          >
            {sent > 0 ? (
              <span style={{ color: "#aaa" }}>{chat.placeholder}</span>
            ) : !pasted ? (
              <span style={{ color: "#aaa" }}>
                <Caret />
                {chat.placeholder}
              </span>
            ) : frame < selectAt ? (
              <>
                {composerText}
                <Caret />
              </>
            ) : (
              <>
                {head}
                <span
                  style={{
                    background: selected ? `rgba(91,134,245,${0.3 * selection})` : undefined,
                    borderRadius: 4,
                    boxShadow:
                      frame >= fixAt
                        ? `inset 0 -3px 0 rgba(52,211,153,${1 - ramp(frame, fixAt + 30, 20)})`
                        : undefined,
                  }}
                >
                  {word}
                </span>
                {frame >= fixAt && fixed.length < chat.right.length && <Caret />}
                {tail}
              </>
            )}
          </div>
        </div>
      </div>
    </Window>
  );
}

/** Whisper's success toast, as shown when a correction is learned. */
export function LearnedToast({ at, until }: { at: number; until: number }) {
  const frame = useCurrentFrame();
  const { chat, lang } = useCopy();
  const enter = ramp(frame, at, 10);
  const exit = ramp(frame, until, 8);
  if (enter <= 0 || exit >= 1) return null;
  return (
    <div
      style={{
        position: "absolute",
        right: 40,
        top: 60,
        display: "flex",
        alignItems: "center",
        gap: 14,
        padding: "16px 20px",
        borderRadius: 14,
        background: "#18191c",
        border: "1px solid rgba(52,211,153,0.35)",
        color: "#ececec",
        fontSize: 20,
        boxShadow: "0 20px 40px rgba(0,0,0,0.35)",
        opacity: enter * (1 - exit),
        transform: `translateX(${(1 - enter) * 60}px)`,
      }}
    >
      <span
        style={{
          width: 28,
          height: 28,
          borderRadius: 14,
          background: "#34d399",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          color: "#0b2b20",
          fontWeight: 800,
        }}
      >
        ✓
      </span>
      {chat.toast}
      <span
        style={{
          marginLeft: 6,
          fontSize: 15,
          padding: "5px 12px",
          borderRadius: 6,
          background: "rgba(52,211,153,0.15)",
          border: "1px solid rgba(52,211,153,0.25)",
          color: "#bdf5de",
        }}
      >
        {lang === "en" ? "Undo" : "撤销"}
      </span>
    </div>
  );
}

/** A messaging app thread where a snippet expands inside the composer. */
export function Messages({ pasteAt }: { pasteAt: number }) {
  const frame = useCurrentFrame();
  const { snippet } = useCopy();
  const reveal = ramp(frame, pasteAt, 10, (t) => t);
  const before = revealText(snippet.before, Math.min(1, reveal * 2));
  const expansion = revealText(snippet.expansion, Math.max(0, reveal * 2 - 1));
  const glow = ramp(frame, pasteAt + 6, 8) * (1 - ramp(frame, pasteAt + 60, 20));
  return (
    <Window x={330} y={110} width={940} height={680} title={snippet.contact}>
      <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "#fff" }}>
        <div
          style={{
            flex: 1,
            padding: 30,
            display: "flex",
            flexDirection: "column",
            justifyContent: "flex-end",
            gap: 14,
          }}
        >
          {[...snippet.earlier, { mine: false, text: snippet.incoming }].map((message, i) => (
            <div
              key={i}
              style={{
                alignSelf: message.mine ? "flex-end" : "flex-start",
                maxWidth: 560,
                padding: "14px 20px",
                borderRadius: 24,
                background: message.mine ? "#0a84ff" : "#ececf0",
                color: message.mine ? "#fff" : undefined,
                fontSize: 24,
              }}
            >
              {message.text}
            </div>
          ))}
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 14,
            padding: "18px 24px",
            borderTop: "1px solid #eee",
          }}
        >
          <div
            style={{
              flex: 1,
              padding: "14px 22px",
              borderRadius: 28,
              border: "2px solid #dcdce2",
              fontSize: 24,
              minHeight: 34,
            }}
          >
            {before}
            <span
              style={{
                color: expansion ? "#2563eb" : undefined,
                background: `rgba(91,134,245,${0.16 * glow})`,
                borderRadius: 6,
                padding: expansion ? "2px 4px" : 0,
              }}
            >
              {expansion}
            </span>
            <Caret />
          </div>
          <span
            style={{
              width: 52,
              height: 52,
              borderRadius: 26,
              background: "#34c759",
              color: "#fff",
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 26,
            }}
          >
            ↑
          </span>
        </div>
      </div>
    </Window>
  );
}

/** The snippet trigger turning into its saved text, floating above the composer. */
export function SnippetChip({ at, until }: { at: number; until: number }) {
  const frame = useCurrentFrame();
  const { snippet, lang } = useCopy();
  const enter = ramp(frame, at, 12);
  const exit = ramp(frame, until, 8);
  if (enter <= 0 || exit >= 1) return null;
  return (
    <div
      style={{
        position: "absolute",
        left: 420,
        top: 190,
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "14px 22px",
        borderRadius: 16,
        background: "#18191c",
        color: "#ececec",
        fontSize: 22,
        boxShadow: "0 20px 40px rgba(0,0,0,0.3)",
        opacity: enter * (1 - exit),
        transform: `translateY(${(1 - enter) * 20}px)`,
      }}
    >
      <span style={{ color: "#9a9ba1", fontSize: 17 }}>{lang === "en" ? "Snippet" : "片段"}</span>
      <span style={{ padding: "4px 12px", borderRadius: 8, background: "#2a2c31" }}>
        “{snippet.trigger}”
      </span>
      <span style={{ color: "#5b86f5" }}>→</span>
      <span style={{ fontFamily: MONO }}>{snippet.expansion}</span>
    </div>
  );
}

const KEYWORDS = /\b(export|async|function|const|await|return)\b/g;

function CodeLine({ text }: { text: string }) {
  if (text.trimStart().startsWith("//")) return <span style={{ color: "#7fbf7f" }}>{text}</span>;
  const parts: { text: string; keyword: boolean }[] = [];
  let last = 0;
  for (const match of text.matchAll(KEYWORDS)) {
    parts.push({ text: text.slice(last, match.index), keyword: false });
    parts.push({ text: match[0], keyword: true });
    last = match.index! + match[0].length;
  }
  parts.push({ text: text.slice(last), keyword: false });
  return (
    <>
      {parts.map((part, i) => (
        <span
          key={i}
          style={{
            color: part.keyword ? "#c792ea" : part.text.includes('"') ? "#f0c674" : "#d4d4d8",
          }}
        >
          {part.text}
        </span>
      ))}
    </>
  );
}

/** A dark code editor; the dictated comment lands on the blank line. */
export function Editor({ pasteAt }: { pasteAt: number }) {
  const frame = useCurrentFrame();
  const { hold } = useCopy();
  const typed = revealText(
    hold.comment,
    ramp(frame, pasteAt, 8, (t) => t)
  );
  const glow = ramp(frame, pasteAt, 6) * (1 - ramp(frame, pasteAt + 30, 20));
  const blank = hold.code.indexOf("");
  return (
    <Window x={220} y={70} width={1160} height={740} title={hold.file} dark>
      <div style={{ display: "flex", height: "100%", background: "#1e1f24" }}>
        <div
          style={{
            width: 230,
            background: "#18191d",
            padding: "20px 0",
            fontSize: 18,
            color: "#9a9ba1",
          }}
        >
          {["src", "  api.ts", `  ${hold.file}`, "  retry.ts", "test", "package.json"].map(
            (name) => (
              <div
                key={name}
                style={{
                  padding: "7px 20px",
                  whiteSpace: "pre",
                  background: name.trim() === hold.file ? "#2a2c33" : undefined,
                  color: name.trim() === hold.file ? "#fff" : undefined,
                }}
              >
                {name}
              </div>
            )
          )}
        </div>
        <div
          style={{ flex: 1, padding: "26px 0", fontFamily: MONO, fontSize: 23, lineHeight: 1.75 }}
        >
          {hold.code.map((line, i) => (
            <div
              key={i}
              style={{
                display: "flex",
                background: i === blank ? `rgba(91,134,245,${0.06 + 0.14 * glow})` : undefined,
              }}
            >
              <span style={{ width: 70, textAlign: "right", paddingRight: 24, color: "#55565d" }}>
                {i + 1}
              </span>
              <span style={{ whiteSpace: "pre" }}>
                {i === blank ? (
                  <>
                    <CodeLine text={typed} />
                    <Caret color="#9fb8ff" height={28} />
                  </>
                ) : (
                  <CodeLine text={line} />
                )}
              </span>
            </div>
          ))}
        </div>
      </div>
    </Window>
  );
}
