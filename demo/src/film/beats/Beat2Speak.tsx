import { AbsoluteFill, interpolate } from "remotion";
import { beat1 } from "../../../timeline/beats/beat1-opening.ts";
import { BEAT2_TEXT, beat2 as beat } from "../../../timeline/beats/beat2-speak.ts";
import type { Lang } from "../../../timeline/types.ts";
import { useCopy } from "../../lib/copy-context.tsx";
import { clamp, ramp, useBeatTime } from "../anim.ts";
import { AppWindow, type Box } from "../chrome/AppWindow.tsx";
import { FnKey, keyDepth } from "../chrome/FnKey.tsx";
import { RecordingPill } from "../chrome/RecordingPill.tsx";
import { isBlinking, Clawd, clawdPose } from "../characters/Clawd.tsx";
import { HAND_FONT } from "../fonts.ts";
import { useLang } from "../lang.tsx";
import { PenArrow, PenMark, PenNote } from "../pen/RedPen.tsx";
import { speechLevel } from "../pill/waveform.ts";
import { PALETTE } from "../theme.ts";
import { DictionaryNotebook } from "./beat2/DictionaryNotebook.tsx";
import { type Insertion, SaidCard } from "./beat2/SaidCard.tsx";

/* Composition (canvas px). Captions are written below y = 850. */
const GROUND_Y = 832;
const CLAWD_X = 340;
const CLAWD_SCALE = 1.1;
/** Mail stays where Beat 1 drew it, so the Dictation carries straight on. */
const MAIL: Box = { x: 800, y: 64, w: 1000, h: 420 };
const MAIL_PILL = { cx: MAIL.x + MAIL.w / 2, cy: MAIL.y + MAIL.h - 78, scale: 3 };
const CARD: Box = { x: 620, y: 548, w: 1180, h: 270 };
const PASTE_NOTE = { x: 150, y: 300 };
const CHAT: Box = { x: 80, y: 44, w: 1110, h: 540 };
const CHAT_SIDEBAR = 230;
const CHAT_PILL = { cx: 880, cy: 700, scale: 2.6 };
const FN_KEY = { x: 548, y: 716, size: 84 };
const NOTEBOOK: Box = { x: 1290, y: 96, w: 540, h: 500 };
/** A highlighter swipe over freshly pasted text, fading as `a` goes to 0. */
const highlighter = (a: number) => `rgba(244, 211, 94, ${0.55 * a})`;

/**
 * Where the cleanup adds punctuation, as indices into `copy.mail.spoken`:
 * a full stop after the first clause, a comma, and a closing full stop.
 */
const INSERTIONS: Record<Lang, Insertion[]> = {
  en: [
    { after: 5, mark: "." },
    { after: 10, mark: "," },
    { after: 16, mark: "." },
  ],
  "zh-CN": [
    { after: 3, mark: "。" },
    { after: 7, mark: "，" },
    { after: 12, mark: "。" },
  ],
};

/**
 * Beat 2 · Speak and it's written. Mail: the Dictation begun in Beat 1 goes
 * on; the heard words fill the "You said" card, fillers and all. The pill
 * stops, the red pen strikes the fillers and adds punctuation, and the
 * cleaned text lands in the reply by Automatic paste. Team chat: Clawd
 * Double taps fn and dictates a reply; the product name comes out misheard,
 * so the red pen circles it and writes Supabase, and the Dictionary notebook
 * records the correction with a tick.
 */
export const Beat2Speak: React.FC = () => {
  const { frame, fps, t } = useBeatTime(beat);
  const lang = useLang();
  const copy = useCopy();
  const m = beat.moments;

  const beatOpacity = ramp(t, m.sketchIn, m.sketchIn + 0.3) * (1 - ramp(t, m.beatOut, beat.end));
  const mailOpacity = 1 - ramp(t, m.mailOut, m.mailOut + 0.45);
  const inMail = t < m.mailOut + 0.45;

  // Mail: words heard so far, the red pen's strikes and carets, the paste.
  const tokens = copy.mail.spoken;
  const heard = ramp(t, m.wordsIn, m.wordsEnd);
  const shown = t < m.wordsIn ? 0 : Math.max(1, Math.round(heard * tokens.length));
  const strikes = m.strikes.map((at) => ramp(t, at, at + 0.35));
  const inserted = m.punctuation.map((at) => ramp(t, at, at + 0.4));
  const pasted = ramp(t, m.paste, m.paste + 0.2);

  // Team chat.
  const chatP = ramp(t, m.chatIn, m.chatIn + 1.1);
  const chatPasted = ramp(t, m.chatPaste, m.chatPaste + 0.2);
  const fnDepth = keyDepth(t, m.chatTaps);

  return (
    <AbsoluteFill style={{ opacity: beatOpacity }}>
      {inMail && (
        <AbsoluteFill style={{ opacity: mailOpacity }}>
          <AppWindow box={MAIL} title={copy.mail.app} progress={1} seed={31}>
            <MailReply pasted={pasted} highlight={1 - ramp(t, m.paste + 0.6, m.paste + 2.2)} caret={Math.floor(t * 2.4) % 2 === 0} />
          </AppWindow>
          <SaidCard
            box={CARD}
            tokens={tokens}
            shown={shown}
            strikes={strikes}
            insertions={INSERTIONS[lang]}
            inserted={inserted}
            progress={1 - ramp(t, m.paste + 0.3, m.paste + 1.2) * 0.35}
            seed={701}
          />
          <svg width={1920} height={1080} style={{ position: "absolute", inset: 0 }}>
            <RecordingPill
              cx={MAIL_PILL.cx}
              cy={MAIL_PILL.cy}
              scale={MAIL_PILL.scale}
              t={t}
              listenAt={beat1.moments.listen}
              stopAt={m.stop}
              doneAt={m.paste}
              seed={61}
            />
            <PenNote x={PASTE_NOTE.x} y={PASTE_NOTE.y} text={BEAT2_TEXT.pasted[lang]} progress={ramp(t, m.pasteNote, m.pasteNote + 0.55)} fontSize={50} />
            <PenArrow
              from={[PASTE_NOTE.x + 250, PASTE_NOTE.y + 22]}
              to={[MAIL.x + 22, MAIL.y + 300]}
              bend={-40}
              progress={ramp(t, m.pasteNote + 0.45, m.pasteNote + 0.8)}
              seed={711}
            />
          </svg>
        </AbsoluteFill>
      )}
      {t >= m.chatIn && (
        <>
          <AppWindow
            box={CHAT}
            title={copy.chat.app}
            progress={chatP}
            seed={33}
            sidebarWidth={CHAT_SIDEBAR}
            sidebar={<ChatSidebar />}
            contentStyle={{ display: "flex", flexDirection: "column", padding: "14px 30px 16px", fontSize: 27 }}
          >
            <ChatThread
              pasted={chatPasted}
              circle={ramp(t, m.circle, m.circle + 0.45)}
              fix={ramp(t, m.fix, m.fix + 0.6)}
              toast={ramp(t, m.tick + 0.2, m.tick + 0.5)}
            />
          </AppWindow>
          <svg width={1920} height={1080} style={{ position: "absolute", inset: 0 }}>
            <FnKey x={FN_KEY.x} y={FN_KEY.y} size={FN_KEY.size} depth={fnDepth} progress={ramp(t, m.chatIn + 0.3, m.chatIn + 0.9)} seed={721} />
            <RecordingPill
              cx={CHAT_PILL.cx}
              cy={CHAT_PILL.cy}
              scale={CHAT_PILL.scale}
              t={t}
              listenAt={m.chatListen}
              stopAt={m.chatStop}
              doneAt={m.chatPaste}
              seed={63}
            />
            <DictionaryNotebook
              box={NOTEBOOK}
              title={copy.whisper.nav.dictionary}
              wrong={copy.chat.wrong}
              right={copy.chat.right}
              progress={ramp(t, m.notebookIn, m.notebookIn + 0.8)}
              entry={ramp(t, m.entry, m.entry + 1.2)}
              tick={ramp(t, m.tick, m.tick + 0.35)}
              seed={731}
            />
          </svg>
        </>
      )}
      <svg width={1920} height={1080} style={{ position: "absolute", inset: 0 }}>
        <Clawd x={CLAWD_X} y={GROUND_Y} scale={CLAWD_SCALE} seed={21} {...clawdAt(t)} />
        {speaking(t) && <SpeechLines x={CLAWD_X + 150} y={GROUND_Y - 190} t={t} />}
      </svg>
    </AbsoluteFill>
  );

  function speaking(t: number) {
    return t < m.stop || (t >= m.chatListen && t < m.chatStop);
  }

  /** Clawd's pose at time t: dictating, watching the pen, pleased with the paste, tapping fn, puzzled, proud. */
  function clawdAt(t: number) {
    const idle = clawdPose("idle", frame, fps);
    if (speaking(t)) {
      const level = speechLevel(frame + Math.round(beat.start * fps));
      return {
        ...idle,
        eyes: isBlinking(frame, fps) ? ("closed" as const) : ("open" as const),
        look: [1, -0.45] as const,
        squash: 1 + level * 0.35,
        leftArm: -14,
        rightArm: 8 + 60 * level,
      };
    }
    if (t < m.paste) return { ...idle, look: [1, 0.5] as const };
    if (t < m.paste + 1.4) return clawdPose("proud", frame, fps);
    if (t < m.chatIn) return { ...idle, look: [1, -0.6] as const };
    if (t < m.chatListen) {
      // Reaches for the fn key and Double taps it.
      const reach = ramp(t, m.chatIn + 0.4, m.chatTaps[0] - 0.2);
      return { ...idle, rightArm: -30 * reach - 22 * fnDepth, look: [0.8, 0.6] as const, rotate: 2 * reach };
    }
    if (t < m.circle) return { ...idle, look: [1, -0.7] as const };
    if (t < m.fix + 0.5) return clawdPose("scratch", frame, fps);
    if (t < m.tick) return { ...idle, look: [1, -0.4] as const };
    return clawdPose("proud", frame, fps);
  }
};

/** The Mail reply: header fields, the greeting, and the cleaned text once pasted. */
const MailReply: React.FC<{ pasted: number; highlight: number; caret: boolean }> = ({ pasted, highlight, caret }) => {
  const copy = useCopy();
  const field: React.CSSProperties = {
    display: "flex",
    gap: 14,
    padding: "8px 0",
    borderBottom: `2px solid ${PALETTE.paperShade}`,
  };
  const label: React.CSSProperties = { color: PALETTE.pencil };
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <div style={field}>
        <span style={label}>{copy.mail.to}</span>
        <span>{copy.mail.toName}</span>
      </div>
      <div style={field}>
        <span style={label}>{copy.mail.subject}</span>
        <span>{copy.mail.subjectText}</span>
      </div>
      <div style={{ paddingTop: 16, fontSize: 34, lineHeight: 1.35 }}>
        <div>{copy.mail.greeting}</div>
        <div style={{ opacity: pasted, transform: `translateY(${(1 - pasted) * 10}px)` }}>
          {pasted > 0 && (
            <span style={{ background: highlighter(highlight), boxDecorationBreak: "clone" }}>{copy.mail.cleaned}</span>
          )}
          <span
            style={{
              display: "inline-block",
              width: 3,
              height: 38,
              marginLeft: 2,
              verticalAlign: "text-bottom",
              background: PALETTE.ink,
              opacity: caret ? 1 : 0,
            }}
          />
        </div>
      </div>
    </div>
  );
};

/** The channel list in the team chat's sidebar, with the current channel picked out. */
const ChatSidebar: React.FC = () => {
  const copy = useCopy();
  return (
    <div style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 12 }}>
      {copy.chat.channels.map((name) => {
        const current = name === copy.chat.channel;
        return (
          <div
            key={name}
            style={{
              padding: "4px 10px",
              borderRadius: 8,
              fontSize: 22,
              whiteSpace: "nowrap",
              color: current ? PALETTE.ink : PALETTE.pencil,
              fontWeight: current ? 600 : 400,
              background: current ? PALETTE.paperShade : "transparent",
            }}
          >
            # {name}
          </div>
        );
      })}
    </div>
  );
};

interface ChatThreadProps {
  /** 0..1 the dictated reply landing in the message box. */
  pasted: number;
  /** 0..1 the red-pen circle round the misheard word. */
  circle: number;
  /** 0..1 the correction written above it. */
  fix: number;
  /** 0..1 the app's "added to your dictionary" toast. */
  toast: number;
}

/** The channel: earlier messages, the teammate's question, and the message box with the pasted reply. */
const ChatThread: React.FC<ChatThreadProps> = ({ pasted, circle, fix, toast }) => {
  const copy = useCopy();
  const { chat } = copy;
  const messages = [...chat.earlier, { name: chat.teammate, time: "9:26", text: chat.question }];
  const [head, tail] = splitOnce(chat.heard, chat.wrong);
  return (
    <>
      <div style={{ fontWeight: 600, paddingBottom: 8, borderBottom: `2px solid ${PALETTE.paperShade}` }}># {chat.channel}</div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8, paddingTop: 10, lineHeight: 1.3 }}>
        {messages.map((msg, i) => (
          <div key={i}>
            <div>
              <span style={{ fontWeight: 600 }}>{msg.name}</span>
              <span style={{ color: PALETTE.pencil, fontSize: 21, marginLeft: 12 }}>{msg.time}</span>
            </div>
            <div style={{ fontSize: 26 }}>{msg.text}</div>
          </div>
        ))}
      </div>
      <div style={{ flex: 1, minHeight: 64 }} />
      {/* The app's toast, top right over the channel header. */}
      <div
        style={{
          position: "absolute",
          top: 10,
          right: 24,
          padding: "6px 18px",
          borderRadius: 999,
          border: `2px solid ${PALETTE.pencil}`,
          background: PALETTE.cream,
          fontSize: 22,
          opacity: toast,
          transform: `translateY(${(1 - toast) * 12}px)`,
        }}
      >
        ✓ {chat.toast}
      </div>
      <div style={{ border: `2px solid ${PALETTE.pencil}`, borderRadius: 12, padding: "10px 16px", whiteSpace: "nowrap" }}>
        {pasted > 0 ? (
          <span style={{ opacity: pasted, background: highlighter(1 - circle) }}>
            {head}
            <span style={{ position: "relative", display: "inline-block" }}>
              <PenMark kind="circle" progress={circle} seed={741} pad={12} clear>
                {chat.wrong}
              </PenMark>
              <span
                style={{
                  position: "absolute",
                  left: "50%",
                  bottom: "100%",
                  marginBottom: 12,
                  transform: "translateX(-50%) rotate(-3deg)",
                  fontFamily: `"${HAND_FONT}", serif`,
                  fontSize: 44,
                  lineHeight: 1,
                  color: PALETTE.redPen,
                  whiteSpace: "nowrap",
                  clipPath: `inset(-10px ${(1 - fix) * 100}% -10px -10px)`,
                  opacity: fix > 0 ? 1 : 0,
                }}
              >
                {chat.right}
              </span>
            </span>
            {tail}
          </span>
        ) : (
          <span style={{ color: PALETTE.pencil }}>{chat.placeholder}</span>
        )}
      </div>
    </>
  );
};

function splitOnce(text: string, word: string): [string, string] {
  const at = text.indexOf(word);
  return at < 0 ? [text, ""] : [text.slice(0, at), text.slice(at + word.length)];
}

/** Little pencil sound strokes beside Clawd's head while it speaks. */
const SpeechLines: React.FC<{ x: number; y: number; t: number }> = ({ x, y, t }) => {
  const pulse = interpolate(Math.sin(t * Math.PI * 3), [-1, 1], [0.35, 1], clamp);
  const lines = [
    [0, -26, 22, -40],
    [6, 0, 34, 0],
    [0, 26, 22, 40],
  ];
  return (
    <g stroke={PALETTE.graphite} strokeWidth={4} strokeLinecap="round" opacity={pulse}>
      {lines.map(([x1, y1, x2, y2], i) => (
        <line key={i} x1={x + x1} y1={y + y1} x2={x + x2} y2={y + y2} />
      ))}
    </g>
  );
};
