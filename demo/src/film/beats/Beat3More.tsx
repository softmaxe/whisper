import { AbsoluteFill, Easing, interpolate } from "remotion";
import { BEAT3_TEXT, beat3 as beat, FILE_TRANSCRIBE_SECONDS } from "../../../timeline/beats/beat3-more.ts";
import { clamp, ramp, useBeatTime } from "../anim.ts";
import { emWidth } from "../components/captionLayout.ts";
import { FnKey, keyDepth } from "../chrome/FnKey.tsx";
import { RecordingPill } from "../chrome/RecordingPill.tsx";
import { isBlinking, Whis, whisPose } from "../characters/Whis.tsx";
import { useLang } from "../lang.tsx";
import { useCopy } from "../../lib/copy-context.tsx";
import { PenArrow, PenCircle, PenNote } from "../pen/RedPen.tsx";
import { speechLevel } from "../pill/waveform.ts";
import { RoughDrawing, useWobbleSeed } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";
import { HoldNote } from "./beat3/HoldNote.tsx";
import { NOTE, NOTE_WINDOW, PinnedNote, THUMB_SLOTS } from "./beat3/PinnedNote.tsx";
import { SnippetNote } from "./beat3/SnippetNote.tsx";
import { dropZoneCentre, FileIcon, UploadNote } from "./beat3/UploadNote.tsx";

/* Composition (canvas px). Notes on the right, Whis and the fn key lower left; Captions below y = 850. */
const GROUND_Y = 832;
const WHIS = { x: 192, scale: 0.85 };
const FN = { x: 338, y: 730, size: 90 };
/** Whis's speech bubble in the Snippet note. */
const BUBBLE = { cx: 250, cy: 522, fontSize: 40 };
/** The pill floats over the bottom of the note's app window, over its content area. */
const pillAt = (sidebar: number) => ({
  cx: NOTE.x + NOTE_WINDOW.x + sidebar + (NOTE_WINDOW.w - sidebar) / 2,
  cy: NOTE.y + NOTE_WINDOW.y + NOTE_WINDOW.h - 82,
  scale: 2.8,
});
const moveEase = Easing.bezier(0.45, 0, 0.2, 1);

/**
 * Beat 3 · More ways to use it. Three notes are pinned up in turn, each
 * shrinking to a thumbnail at the top left when done:
 * 1. Snippet: Whis Double taps fn and says the trigger phrase (in a speech
 *    bubble); the reply lands in Messages with the full link, and the red pen
 *    circles the phrase and arrows it to the underlined link.
 * 2. Hold mode: Whis holds fn down (the pill gets its blue ring) and dictates;
 *    on release the comment lands on the code editor's blank line, ticked.
 * 3. Upload: three audio files are dragged onto Whisper's Upload page, their
 *    pencilled progress bars fill one after another, each ticked, and the
 *    completed status is circled.
 */
export const Beat3More: React.FC = () => {
  const { frame, fps, t } = useBeatTime(beat);
  const lang = useLang();
  const copy = useCopy();
  const m = beat.moments;
  const beatOpacity = 1 - ramp(t, m.beatOut, beat.end);

  const noteP = (at: number) => ramp(t, at, at + 0.8);
  const windowP = (at: number) => ramp(t, at + 0.25, at + 1.05);
  const moveP = (at: number) => ramp(t, at, at + 0.55);

  // The fn key serves the Snippet and Hold mode notes, then leaves.
  const fnP = ramp(t, m.snippetIn, m.snippetIn + 0.6);
  const fnOpacity = 1 - ramp(t, m.holdOut, m.holdOut + 0.4);
  const fnDepth = t < m.holdIn ? keyDepth(t, m.taps) : keyDepth(t, [m.holdPress], m.holdRelease);

  // Canvas-space marks belonging to a note fade as the note shrinks away.
  const snippetMarks = 1 - ramp(t, m.snippetOut, m.snippetOut + 0.25);
  const holdMarks = 1 - ramp(t, m.holdOut, m.holdOut + 0.25);

  // Snippet: the bubble, the circled trigger and the arrow to the expanded link.
  const said = BEAT3_TEXT.said[lang];
  const saidW = emWidth(said) * BUBBLE.fontSize;
  const bubbleP = ramp(t, m.said - 0.25, m.said + 0.1);
  const saidP = ramp(t, m.said, m.said + 0.8);
  const arrowTip = snippetArrowTip(copy.snippet.before);

  const snippetPill = pillAt(0);
  const holdPill = pillAt(250);

  return (
    <AbsoluteFill style={{ opacity: beatOpacity }}>
      {/* Later notes are pinned over earlier thumbnails' old place, so draw in story order. */}
      <PinnedNote
        progress={noteP(m.snippetIn)}
        move={moveP(m.snippetOut)}
        slot={THUMB_SLOTS[0]}
        title={BEAT3_TEXT.snippetTitle[lang]}
        seed={301}
      >
        <SnippetNote
          t={t}
          progress={windowP(m.snippetIn)}
          listenAt={m.listen}
          pasteAt={m.snippetPaste}
          underlineAt={m.expandArrow + 0.35}
        />
      </PinnedNote>
      <PinnedNote progress={noteP(m.holdIn)} move={moveP(m.holdOut)} slot={THUMB_SLOTS[1]} title={BEAT3_TEXT.holdTitle[lang]} seed={311}>
        <HoldNote t={t} progress={windowP(m.holdIn)} listenAt={m.holdListen} pasteAt={m.holdPaste} tickAt={m.holdTick} />
      </PinnedNote>
      <PinnedNote progress={noteP(m.uploadIn)} title={BEAT3_TEXT.uploadTitle[lang]} seed={321}>
        <UploadNote
          t={t}
          progress={windowP(m.uploadIn)}
          hoverAt={m.drop - 0.25}
          dropAt={m.drop}
          doneAt={m.filesDone}
          transcribe={FILE_TRANSCRIBE_SECONDS}
          completeAt={m.uploadComplete}
        />
      </PinnedNote>

      <svg width={1920} height={1080} style={{ position: "absolute", inset: 0 }}>
        <RecordingPill {...snippetPill} t={t} listenAt={m.listen} stopAt={m.snippetStop} doneAt={m.snippetPaste} seed={341} />
        <RecordingPill
          {...holdPill}
          t={t}
          listenAt={m.holdListen}
          stopAt={m.holdRelease}
          doneAt={m.holdPaste}
          held={t < m.holdRelease}
          seed={345}
        />

        <g opacity={fnOpacity}>
          <FnKey x={FN.x} y={FN.y} size={FN.size} depth={fnDepth} progress={fnP} seed={307} />
        </g>
        <Whis x={WHIS.x} y={GROUND_Y} scale={WHIS.scale} seed={23} draw={ramp(t, beat.start, beat.start + 0.8)} {...whisAt(t)} />

        <g opacity={snippetMarks}>
          <SpeechBubble cx={BUBBLE.cx} cy={BUBBLE.cy} width={saidW + 70} progress={bubbleP} seed={333} />
          <PenNote
            x={BUBBLE.cx}
            y={BUBBLE.cy + BUBBLE.fontSize * 0.35}
            text={said}
            progress={saidP}
            fontSize={BUBBLE.fontSize}
            align="middle"
            rotate={0}
            color={PALETTE.ink}
          />
          <PenCircle
            cx={BUBBLE.cx}
            cy={BUBBLE.cy}
            rx={saidW / 2 + 22}
            ry={BUBBLE.fontSize * 0.85}
            progress={ramp(t, m.triggerCircle, m.triggerCircle + 0.4)}
            seed={335}
          />
          <PenArrow
            from={[BUBBLE.cx + saidW / 2 + 30, BUBBLE.cy - 10]}
            to={arrowTip}
            bend={60}
            progress={ramp(t, m.expandArrow, m.expandArrow + 0.4)}
            seed={337}
          />
        </g>

        <g opacity={holdMarks}>
          <PenNote
            x={WHIS.x + 60}
            y={600}
            text={BEAT3_TEXT.holdNote[lang]}
            progress={ramp(t, m.holdNote, m.holdNote + 0.5)}
            fontSize={40}
            align="middle"
          />
          <PenNote
            x={WHIS.x + 90}
            y={540}
            text={BEAT3_TEXT.releaseNote[lang]}
            progress={ramp(t, m.releaseNote, m.releaseNote + 0.35)}
            fontSize={44}
            align="middle"
            rotate={-5}
          />
        </g>

        <DraggedFiles t={t} from={m.filesIn} to={m.drop} />
        {t >= m.listen && t < m.snippetStop && <SpeechLines x={WHIS.x + 140} y={GROUND_Y - 150} t={t} />}
        {t >= m.holdListen && t < m.holdRelease && <SpeechLines x={WHIS.x + 140} y={GROUND_Y - 150} t={t} />}
      </svg>
    </AbsoluteFill>
  );

  /** Whis's performance: tapping and speaking, holding the key, pointing at the files, proud of each result. */
  function whisAt(t: number) {
    const idle = whisPose("idle", frame, fps);
    const speaking = () => {
      const level = speechLevel(frame + Math.round(beat.start * fps));
      return {
        eyes: isBlinking(frame, fps) ? ("closed" as const) : ("open" as const),
        look: [1, -0.4] as const,
        squash: 1 + level * 0.35,
        leftArm: -14 + 40 * level,
      };
    };
    // Snippet
    if (t < m.taps[0] - 0.35) return { ...idle, look: [1, -0.2] as const };
    if (t < m.listen) return { ...idle, rightArm: -30 - 24 * fnDepth, leftArm: -24, look: [0.7, 0.7] as const, rotate: 2 };
    if (t < m.snippetStop) return { ...idle, ...speaking(), rightArm: 8 };
    if (t < m.expandArrow) return { ...idle, look: [1, -0.3] as const };
    if (t < m.holdIn + 0.4) return whisPose("proud", frame, fps);
    // Hold mode: the right arm presses fn down for as long as the key is held.
    if (t < m.holdPress - 0.3) return { ...idle, look: [0.7, 0.7] as const };
    if (t < m.holdListen) return { ...idle, rightArm: -30 - 24 * fnDepth, leftArm: -24, look: [0.7, 0.7] as const, rotate: 2 };
    if (t < m.holdRelease) return { ...idle, ...speaking(), rightArm: -30 - 24 * fnDepth, rotate: 2 };
    if (t < m.holdTick) return { ...idle, rightArm: -30 - 24 * fnDepth, look: [1, -0.3] as const };
    if (t < m.uploadIn + 0.4) return whisPose("proud", frame, fps);
    // Upload: points the files onto the drop zone, watches the bars, proud when done.
    if (t < m.filesIn - 0.3) return { ...idle, look: [1, -0.3] as const };
    if (t < m.drop + 0.4) return whisPose("point", frame, fps);
    if (t < m.uploadComplete) return { ...idle, look: [1, -0.4] as const };
    return whisPose("proud", frame, fps);
  }
};

/**
 * Where the Snippet arrow points: just above the start of the expanded link
 * in the composer, estimated from the width of the text before it.
 */
function snippetArrowTip(before: string): [number, number] {
  // Composer text: 32 px system face, after the window, content and composer padding.
  const textLeft = NOTE.x + NOTE_WINDOW.x + 4 + 28 + 24;
  const beforeW = emWidth(before) * 32;
  const composerTop = NOTE.y + NOTE_WINDOW.y + NOTE_WINDOW.h - 4 - 22 - 76;
  return [textLeft + beforeW + 60, composerTop - 6];
}

/** A pencilled speech bubble with a tail down towards Whis. */
const SpeechBubble: React.FC<{ cx: number; cy: number; width: number; progress: number; seed: number }> = ({
  cx,
  cy,
  width,
  progress,
  seed,
}) => {
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  const h = 96;
  const w = Math.round(width);
  return (
    <g opacity={Math.min(1, progress * 2)}>
      <ellipse cx={cx} cy={cy} rx={w / 2} ry={h / 2} fill={PALETTE.whitePaper} />
      <RoughDrawing
        seed={boil}
        progress={progress}
        options={{ stroke: PALETTE.graphite, strokeWidth: 3, roughness: 1, bowing: 0.8 }}
        deps={[cx, cy, w]}
        build={(g, o) => [
          g.ellipse(cx, cy, w, h, o),
          g.linearPath(
            [
              [cx - 40, cy + h / 2 - 6],
              [cx - 36, cy + h / 2 + 56],
              [cx + 6, cy + h / 2 - 2],
            ],
            o,
          ),
        ]}
      />
    </g>
  );
};

/** The three audio files, dragged in from the right edge onto the drop zone, where they vanish. */
const DraggedFiles: React.FC<{ t: number; from: number; to: number }> = ({ t, from, to }) => {
  const copy = useCopy();
  const travel = interpolate(t, [from, to], [0, 1], { ...clamp, easing: Easing.bezier(0.2, 0, 0.1, 1) });
  const vanish = ramp(t, to, to + 0.15);
  if (t < from || vanish >= 1) return null;
  const [zx, zy] = dropZoneCentre();
  return (
    <g opacity={1 - vanish}>
      {copy.whisper.upload.files.map((name, i) => {
        const x0 = 1980 + i * 60;
        const y0 = 360 + i * 150;
        const x = x0 + (zx - 50 + (i - 1) * 80 - x0) * travel;
        const y = y0 + (zy - 60 + (i - 1) * 16 - y0) * travel;
        return (
          <g key={name} transform={`translate(${x} ${y}) rotate(${(i - 1) * 9} 45 55)`}>
            <FileIcon name={name} seed={421 + i} />
          </g>
        );
      })}
    </g>
  );
};

/** Little pencil sound strokes beside Whis's head while it speaks. */
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

