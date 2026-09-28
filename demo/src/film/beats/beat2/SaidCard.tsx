import type { Token } from "../../../copy.ts";
import { useCopy } from "../../../lib/copy-context.tsx";
import { ramp } from "../../anim.ts";
import type { Box } from "../../chrome/AppWindow.tsx";
import { HAND_FONT } from "../../fonts.ts";
import { PenMark } from "../../pen/RedPen.tsx";
import { RoughDrawing, useWobbleSeed } from "../../rough/RoughDrawing.tsx";
import { PALETTE } from "../../theme.ts";

/** A punctuation mark the cleanup adds after the spoken token at index `after`. */
export interface Insertion {
  after: number;
  mark: string;
}

export interface SaidCardProps {
  box: Box;
  tokens: readonly Token[];
  /** How many tokens have been heard so far. */
  shown: number;
  /** Strike progress (0..1) of each filler, in spoken order. */
  strikes: readonly number[];
  insertions: readonly Insertion[];
  /** Writing progress (0..1) of each insertion. */
  inserted: readonly number[];
  /** 0..1 sketch-in of the card. */
  progress: number;
  seed: number;
}

/**
 * The "You said" card: a cream sheet pinned below Mail that shows the raw
 * words as they are heard, fillers and all, in the clear app typeface. The
 * red pen crosses out the fillers and adds punctuation with a proofreader's
 * caret, without moving the words.
 */
export const SaidCard: React.FC<SaidCardProps> = ({ box, tokens, shown, strikes, insertions, inserted, progress, seed }) => {
  const copy = useCopy();
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  let filler = -1;
  return (
    <div style={{ position: "absolute", left: box.x, top: box.y, width: box.w, height: box.h }}>
      <svg width={box.w + 12} height={box.h + 12} style={{ position: "absolute", inset: 0, overflow: "visible" }}>
        <rect x={3} y={3} width={box.w - 6} height={box.h - 6} fill={PALETTE.cream} opacity={Math.min(1, progress * 1.6)} />
        <RoughDrawing
          seed={boil}
          progress={progress}
          options={{ stroke: PALETTE.graphite, strokeWidth: 2.6, roughness: 1.3, bowing: 1 }}
          deps={[box.w, box.h]}
          build={(g, o) => [
            g.rectangle(0, 0, box.w, box.h, o),
            // A strip of tape holding the card to the paper.
            g.rectangle(box.w / 2 - 70, -16, 140, 32, { ...o, stroke: PALETTE.pencil, strokeWidth: 1.6, fill: PALETTE.paperShade, fillStyle: "solid" }),
          ]}
        />
      </svg>
      <div
        style={{
          position: "absolute",
          inset: 0,
          padding: "26px 40px",
          boxSizing: "border-box",
          fontFamily: copy.font,
          color: PALETTE.ink,
          opacity: ramp(progress, 0.4, 1),
        }}
      >
        <div style={{ fontFamily: `"${HAND_FONT}", serif`, fontSize: 30, color: PALETTE.pencil, marginBottom: 6 }}>{copy.saidLabel}</div>
        <div style={{ fontSize: 38, lineHeight: 2.05 }}>
          {tokens.map((tok, i) => {
            if (tok.filler) filler += 1;
            if (i >= shown) return null;
            const insertion = insertions.findIndex((ins) => ins.after === i);
            const word = (
              <span style={{ position: "relative", display: "inline-block", color: tok.filler ? PALETTE.pencil : PALETTE.ink }}>
                {tok.text}
                {insertion >= 0 && <Caret mark={insertions[insertion].mark} progress={inserted[insertion] ?? 0} />}
              </span>
            );
            return (
              <span key={i}>
                {i > 0 && copy.gap}
                {tok.filler ? (
                  <PenMark kind="strike" passes={2} pad={4} progress={strikes[filler] ?? 0} seed={seed + 40 + filler}>
                    {word}
                  </PenMark>
                ) : (
                  word
                )}
              </span>
            );
          })}
        </div>
      </div>
    </div>
  );
};

/** A red proofreader's caret under the gap after a word, with the added mark written above it. */
const Caret: React.FC<{ mark: string; progress: number }> = ({ mark, progress }) => {
  if (progress <= 0) return null;
  const caret = Math.min(1, progress / 0.45);
  const write = ramp(progress, 0.35, 1);
  return (
    <span style={{ position: "absolute", left: "100%", top: 0, bottom: 0, width: 0, pointerEvents: "none" }}>
      <svg width={1} height={1} style={{ position: "absolute", left: 0, bottom: 10, overflow: "visible" }}>
        <path
          d="M -11 10 L 1 -12 L 12 9"
          fill="none"
          stroke={PALETTE.redPen}
          strokeWidth={4}
          strokeLinecap="round"
          strokeLinejoin="round"
          pathLength={1}
          strokeDasharray="1 1"
          strokeDashoffset={1 - caret}
        />
      </svg>
      <span
        style={{
          position: "absolute",
          left: -14,
          top: -44,
          fontFamily: `"${HAND_FONT}", serif`,
          fontSize: 76,
          lineHeight: 1,
          color: PALETTE.redPen,
          clipPath: `inset(0 ${(1 - write) * 100}% 0 0)`,
        }}
      >
        {mark}
      </span>
    </span>
  );
};
