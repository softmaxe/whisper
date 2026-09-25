import { Easing, interpolate } from "remotion";
import { FPS, SECONDS_PER_BEAT } from "../timeline.ts";

/** Frames spanned by `count` beats. */
export const beats = (count: number) => count * SECONDS_PER_BEAT * FPS;

export const easeOut = Easing.bezier(0.16, 1, 0.3, 1);
export const easeInOut = Easing.bezier(0.65, 0, 0.35, 1);
/** The app's own pill morph curve (VOICE_PILL_GROW_EASING). */
export const pillEase = Easing.bezier(0.2, 0, 0, 1);

/** 0..1 progress of a transition starting at `start` lasting `duration` frames. */
export const ramp = (
  frame: number,
  start: number,
  duration: number,
  easing: (t: number) => number = easeOut
) =>
  interpolate(frame, [start, start + Math.max(1, duration)], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing,
  });

export const lerp = (from: number, to: number, t: number) => from + (to - from) * t;

/** Stable pseudo-random value in 0..1 for an integer seed. */
export const hash = (seed: number) => {
  const x = Math.sin(seed * 127.1 + 311.7) * 43758.5453;
  return x - Math.floor(x);
};

/**
 * A speech-like input level (RMS, 0..~0.15) for the waveform: syllables of
 * varying loudness separated by short dips.
 */
export const speechLevel = (frame: number) => {
  const syllable = Math.floor(frame / 5);
  const phase = (frame % 5) / 5;
  const loudness = 0.05 + hash(syllable) * 0.1;
  const shape = Math.sin(Math.PI * phase);
  const pause = hash(Math.floor(frame / 23) + 99) < 0.15 ? 0.25 : 1;
  return loudness * (0.35 + 0.65 * shape) * pause;
};
