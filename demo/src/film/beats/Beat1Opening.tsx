import { AbsoluteFill, interpolate } from "remotion";
import { BEAT1_TEXT, beat1 as beat } from "../../../timeline/beats/beat1-opening.ts";
import { keystrokeTimes } from "../../../timeline/helpers.ts";
import { clamp, ramp, useBeatTime } from "../anim.ts";
import { AppWindow, type Box } from "../chrome/AppWindow.tsx";
import { keyDepth } from "../chrome/FnKey.tsx";
import { Keyboard, keyboardLayout } from "../chrome/Keyboard.tsx";
import { RecordingPill } from "../chrome/RecordingPill.tsx";
import { isBlinking, Clawd, CLAWD_POSES, clawdPose } from "../characters/Clawd.tsx";
import { useLang } from "../lang.tsx";
import { useCopy } from "../../lib/copy-context.tsx";
import { PenArrow, PenCircle, PenNote } from "../pen/RedPen.tsx";
import { speechLevel } from "../pill/waveform.ts";
import { PALETTE } from "../theme.ts";

/* Composition (canvas px). Captions are written below y = 850. */
const GROUND_Y = 832;
const CLAWD_X = 340;
const CLAWD_SCALE = 1.1;
const WINDOW: Box = { x: 800, y: 64, w: 1000, h: 420 };
const KEYBOARD: Box = { x: 560, y: 560, w: 880, h: 262 };
/** The pill floats over the bottom of the app, as the real one floats over the screen. */
const PILL = { cx: WINDOW.x + WINDOW.w / 2, cy: WINDOW.y + WINDOW.h - 78, scale: 3 };
/** Where the red-pen label is written: above and left of the keyboard, clear of Clawd. */
const NOTE = { x: 110, y: 470 };

/**
 * Beat 1 · Opening. The Mail window, keyboard and Clawd are sketched in; Clawd
 * pecks out the reply's greeting one keystroke at a time (in step with the
 * key-click cues) and gives up, scratching its head. The fn/Globe key is
 * circled in red pen and labelled with an arrow; Clawd Double taps it and the
 * Recording pill opens, its bars replaying the app's waveform math while
 * Clawd speaks.
 */
export const Beat1Opening: React.FC = () => {
  const { frame, fps, t } = useBeatTime(beat);
  const lang = useLang();
  const copy = useCopy();
  const m = beat.moments;
  const [typing] = beat.typing;
  const keys = keystrokeTimes(typing);

  // Sketch-in, staggered.
  const windowP = ramp(t, m.sketchIn, m.sketchIn + 1.1);
  const keyboardP = ramp(t, m.sketchIn + 0.2, m.sketchIn + 1.0);
  const clawdP = ramp(t, m.sketchIn + 0.1, m.sketchIn + 1.0);
  const beatOpacity = 1 - ramp(t, m.beatOut, beat.end);

  // Typing: the text follows the keystrokes; each strike dips an arm and shades a key.
  const typed = keys.filter((k) => k <= t).length;
  const lastKey = typed > 0 ? keys[typed - 1] : -Infinity;
  const strike = Math.exp(-(t - lastKey) / 0.07);
  const text = BEAT1_TEXT.typed[lang];
  const shownChars = [...text].slice(0, Math.round((typed / keys.length) * [...text].length)).join("");
  const isTyping = t >= typing.start - 0.3 && t < m.giveUp;

  // The Double tap on fn: the key sinks, Clawd's right arm dips with it.
  const fnDepth = keyDepth(t, m.taps);
  const listening = t >= m.listen;

  // Red pen: circle the fn key, then write the label and draw the arrow to it.
  const { fn } = keyboardLayout(KEYBOARD);
  const fnCx = fn.x + fn.w / 2;
  const fnCy = fn.y + fn.h / 2 + 2;
  const circleP = ramp(t, m.fnCircle, m.fnCircle + 0.45);
  const noteP = ramp(t, m.fnNote, m.fnNote + 0.55);
  const arrowP = ramp(t, m.fnNote + 0.45, m.fnNote + 0.8);

  return (
    <AbsoluteFill style={{ opacity: beatOpacity }}>
      <AppWindow box={WINDOW} title={copy.mail.app} progress={windowP} seed={31}>
        <MailDraft typed={shownChars} caret={t < m.listen && Math.floor(t * 2.4) % 2 === 0} />
      </AppWindow>
      <svg width={1920} height={1080} style={{ position: "absolute", inset: 0 }}>
        <Keyboard
          box={KEYBOARD}
          progress={keyboardP}
          pressedKey={typed * 7}
          press={isTyping ? strike : 0}
          fnDepth={fnDepth}
          seed={81}
        />
        <Clawd x={CLAWD_X} y={GROUND_Y} scale={CLAWD_SCALE} seed={21} draw={clawdP} {...clawdAt(t, frame, fps, typed, strike)} />
        <RecordingPill cx={PILL.cx} cy={PILL.cy} scale={PILL.scale} t={t} listenAt={m.listen} seed={61} />
        <PenCircle cx={fnCx} cy={fnCy} rx={fn.w * 0.95} ry={fn.h * 0.95} progress={circleP} seed={401} />
        <PenNote x={NOTE.x} y={NOTE.y} text={BEAT1_TEXT.fnNote[lang]} progress={noteP} fontSize={50} />
        <PenArrow
          from={[NOTE.x + 330, NOTE.y + 26]}
          to={[fnCx - fn.w * 0.75, fnCy - fn.h * 0.95]}
          bend={40}
          progress={arrowP}
          seed={402}
        />
        {listening && <SpeechLines x={CLAWD_X + 150} y={GROUND_Y - 190} t={t} />}
      </svg>
    </AbsoluteFill>
  );

  /** Clawd's pose at time t: typing, giving up, watching the pen, tapping fn, speaking. */
  function clawdAt(t: number, frame: number, fps: number, typed: number, strike: number) {
    if (t < typing.start - 0.3) return clawdPose("idle", frame, fps);
    if (t < m.giveUp) {
      // Hunt and peck: the arm that struck the last key dips, eyes on the keys.
      const right = typed % 2 === 0;
      const base = CLAWD_POSES.typing;
      return {
        ...clawdPose("typing", frame, fps),
        leftArm: base.leftArm - (right ? 0 : 14 * strike),
        rightArm: base.rightArm - (right ? 14 * strike : 0),
        look: [0.6, 0.8] as const,
      };
    }
    if (t < m.fnCircle + 0.2) return clawdPose("scratch", frame, fps);
    if (t < m.listen) {
      // Watches the pen, then taps fn: the right arm dips with the key.
      const idle = clawdPose("idle", frame, fps);
      return { ...idle, rightArm: -30 - 22 * fnDepth, leftArm: -24, look: [0.7, 0.7] as const, rotate: 2 };
    }
    // Speaking to the pill: looks up at it, and bobs with the voice.
    const level = speechLevel(frame + Math.round(beat.start * fps));
    return {
      ...clawdPose("idle", frame, fps),
      eyes: isBlinking(frame, fps) ? ("closed" as const) : ("open" as const),
      look: [1, -0.45] as const,
      squash: 1 + level * 0.35,
      leftArm: -14,
      rightArm: 8 + 60 * level,
    };
  }
};

/** A Mail reply being written: the header fields and the typed greeting with a caret. */
const MailDraft: React.FC<{ typed: string; caret: boolean }> = ({ typed, caret }) => {
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
      <div style={{ paddingTop: 22, fontSize: 38, whiteSpace: "pre" }}>
        {typed}
        <span
          style={{
            display: "inline-block",
            width: 3,
            height: 42,
            marginLeft: 2,
            verticalAlign: "text-bottom",
            background: PALETTE.ink,
            opacity: caret ? 1 : 0,
          }}
        />
      </div>
    </div>
  );
};

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
