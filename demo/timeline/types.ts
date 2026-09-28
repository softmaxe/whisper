/**
 * Shared Film timeline types. The timeline is pure data: no React, no Node
 * APIs and no functions, so the Remotion compositions, the build scripts and
 * the tests can import it, and `scripts/export-timeline.ts` can write it to
 * JSON for the Python audio synthesiser.
 *
 * All times are in seconds from the start of the Film. Intervals are
 * half-open: [start, end).
 */

export type Seconds = number;

/** The two cuts. They share every time and differ only in on-screen text. */
export type Lang = "en" | "zh-CN";

/** Where a Caption is written on the paper (1920x1080 canvas pixels). */
export interface CaptionPlacement {
  /** Horizontal anchor point in px; meaning depends on `align`. */
  x: number;
  /** Top of the text block in px. */
  y: number;
  /** Maximum line width in px; longer Captions wrap. */
  width: number;
  align: "left" | "center" | "right";
  /** Font size in px. */
  fontSize: number;
  /** Optional tilt in degrees, for a hand-placed feel. */
  rotate?: number;
}

export interface Caption {
  /** Stable id, unique across the Film (e.g. "b2-clean"). */
  id: string;
  /** The Caption in each cut's language. */
  text: Record<Lang, string>;
  start: Seconds;
  end: Seconds;
  placement: CaptionPlacement;
  /** "title" is written in the red pen; default is narration in ink. */
  variant?: "title" | "narration";
}

/**
 * A sound cue. `type` names a synthesiser module in
 * `audio/whisper_audio/sfx/<type>.py` (so it must be snake_case); a test
 * enforces that it exists. `params` is passed to that module untouched.
 */
export interface SoundCue {
  type: string;
  at: Seconds;
  params?: Record<string, number | string | boolean>;
}

/**
 * Keys typed one at a time. `keys` is the keystroke pattern (a space gets the
 * space-bar sound and a slightly longer gap); it is shared by both cuts, so
 * the picture may show different text per language and reveal it by the
 * keystroke times from `keystrokeTimes()` in helpers.ts.
 */
export interface Typing {
  keys: string;
  /** First keystroke. */
  start: Seconds;
  /** Last keystroke. */
  end: Seconds;
}

/** Beat keys. The audio reads Beat windows by these keys, so they are fixed. */
export type BeatKey = "opening" | "speak" | "more" | "review" | "servers";

/**
 * Named moments (absolute seconds) that a Beat's visuals animate against: a
 * single time or an ordered series. Declared in story order; a test checks it.
 */
export type Moments = Record<string, Seconds | readonly Seconds[]>;

/** A PNG review frame the build extracts from each cut. */
export interface ReviewFrame {
  /** File-name-safe name, unique within the Beat (e.g. "fn-circled"). */
  name: string;
  at: Seconds;
}

/**
 * One Beat of the Film. `M` is the Beat's own moments type, so visuals read
 * `beat1.moments.pill` with a checked key; each Beat module exports its Beat
 * with that precise type.
 */
export interface Beat<M extends Moments = Moments> {
  /** 1-based Beat number, as used in the spec. */
  index: 1 | 2 | 3 | 4 | 5;
  key: BeatKey;
  title: string;
  start: Seconds;
  end: Seconds;
  captions: Caption[];
  cues: SoundCue[];
  /** Keys typed on screen during this Beat, if any. */
  typing?: Typing[];
  /** Named moments that the Beat's visuals animate against. */
  moments: M;
  /** Story frames to extract for review, besides midpoints and Caption ends. */
  reviewFrames?: ReviewFrame[];
}

export interface Film {
  durationSeconds: Seconds;
  fps: number;
  width: number;
  height: number;
  /** Length of the closing fade to blank paper. */
  fadeOutSeconds: Seconds;
  beats: Beat[];
}
