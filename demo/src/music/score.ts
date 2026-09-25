// A neo-classical cue in D-flat major at the timeline's tempo: solo piano
// that opens into strings as the day fills up, with glockenspiel sparkles on
// the moments text lands. Pure data, rendered by scripts/render-music.ts.

import {
  BEATS_PER_BAR,
  TOTAL_BEATS,
  cueBeat,
  sceneStartBeat,
  type CueName,
  type SceneId,
} from "../timeline.ts";

export type Instrument = "piano" | "cello" | "viola" | "violins" | "pizz" | "harp" | "glock";

export interface Note {
  instrument: Instrument;
  midi: number;
  /** Absolute beat on the timeline grid. */
  beat: number;
  beats: number;
  /** 0..1 */
  velocity: number;
}

/** Sounding MIDI range each sampled instrument can play without stretching too far. */
export const INSTRUMENT_RANGES: Record<Instrument, [number, number]> = {
  piano: [21, 108],
  cello: [36, 77],
  viola: [48, 81],
  violins: [55, 84],
  pizz: [55, 84],
  harp: [36, 96],
  glock: [67, 100],
};

/** Cues that ring a glockenspiel sparkle: text landing, a word learned, the logo. */
export const SPARKLE_CUES = [
  ["intro", "pill"],
  ["morning", "paste"],
  ["chat", "paste"],
  ["chat", "learn"],
  ["snippet", "paste"],
  ["hold", "paste"],
  ["upload", "done"],
  ["night", "copy"],
  ["outro", "logo"],
] as const satisfies readonly (readonly [SceneId, string])[];

// Pitch classes, D-flat = 1.
const DB = 1;
const EB = 3;
const F = 5;
const GB = 6;
const AB = 8;
const BB = 10;
const C = 0;

interface Chord {
  bass: number;
  /** Colour tones for the right hand, add9 and maj7 included. */
  tones: number[];
}

const CHORDS = {
  I: { bass: DB, tones: [DB, EB, F, AB] },
  "I/3": { bass: F, tones: [DB, EB, F, AB] },
  "V/3": { bass: C, tones: [AB, BB, C, EB] },
  V: { bass: AB, tones: [AB, BB, C, EB] },
  Vsus: { bass: AB, tones: [AB, BB, DB, EB] },
  vi7: { bass: BB, tones: [BB, DB, F, AB] },
  IVmaj7: { bass: GB, tones: [GB, BB, DB, F] },
  iii7: { bass: F, tones: [F, AB, C, EB] },
  ii7: { bass: EB, tones: [EB, GB, BB, DB] },
} satisfies Record<string, Chord>;

type ChordName = keyof typeof CHORDS;

/** One entry per bar; a pair splits the bar into halves. */
const PROGRESSION: (ChordName | [ChordName, ChordName])[] = [
  "I",
  ["I", "V/3"],
  "vi7",
  "IVmaj7",
  "I",
  "V/3",
  "vi7",
  "IVmaj7",
  "ii7",
  "V",
  "iii7",
  "vi7",
  "IVmaj7",
  "I/3",
  "IVmaj7",
  "V",
  ["ii7", "Vsus"],
  "I",
  "I",
];

interface ChordSpan {
  chord: Chord;
  beat: number;
  beats: number;
}

const chordSpans = (): ChordSpan[] =>
  PROGRESSION.flatMap((entry, bar) => {
    const beat = bar * BEATS_PER_BAR;
    if (Array.isArray(entry)) {
      const half = BEATS_PER_BAR / 2;
      return [
        { chord: CHORDS[entry[0]], beat, beats: half },
        { chord: CHORDS[entry[1]], beat: beat + half, beats: half },
      ];
    }
    return [{ chord: CHORDS[entry], beat, beats: BEATS_PER_BAR }];
  });

/** Lowest MIDI note of a pitch class at or above `floor`. */
const above = (pitchClass: number, floor: number) =>
  floor + ((pitchClass - (floor % 12) + 12) % 12);

/** Chord tones stacked upward from `floor`, `count` notes long. */
const voicing = (tones: number[], floor: number, count: number) => {
  const sorted = [...tones].map((pc) => above(pc, floor)).sort((a, b) => a - b);
  const notes: number[] = [];
  for (let i = 0; notes.length < count; i += 1) {
    notes.push(sorted[i % sorted.length] + 12 * Math.floor(i / sorted.length));
  }
  return notes;
};

// Arpeggio contour over eight quavers: rises, turns, and settles.
const ARPEGGIO = [0, 2, 1, 3, 4, 3, 2, 1];

export function composeScore(): Note[] {
  const notes: Note[] = [];
  const add = (note: Note) => {
    const end = Math.min(note.beat + note.beats, TOTAL_BEATS - 0.01);
    if (note.beat < TOTAL_BEATS) notes.push({ ...note, beats: end - note.beat });
  };

  const chat = sceneStartBeat("chat");
  const snippet = sceneStartBeat("snippet");
  const night = sceneStartBeat("night");
  const settings = sceneStartBeat("settings");
  const outro = sceneStartBeat("outro");

  for (const span of chordSpans()) {
    const { chord, beat, beats } = span;
    const finale = beat >= outro;
    const inSettings = beat >= settings && !finale;
    // The day brightens: soft at dawn, fullest at night, gentle for the close.
    const lift = inSettings ? 0.55 : Math.min(1, 0.42 + (beat / night) * 0.58);

    // Piano left hand: an open octave held through the chord.
    const bass = above(chord.bass, 36);
    add({ instrument: "piano", midi: bass, beat, beats, velocity: 0.34 * lift });
    if (beat >= 8) add({ instrument: "piano", midi: bass + 12, beat, beats, velocity: 0.3 * lift });

    // Piano right hand: crotchets for the opening bars, quavers once the day starts.
    const rh = voicing(chord.tones, 56, 5);
    if (finale) {
      if (beat === outro) {
        rh.forEach((midi, i) =>
          add({ instrument: "piano", midi, beat: beat + i * 0.12, beats: 8, velocity: 0.42 })
        );
      }
    } else if (inSettings) {
      [0, 1, 2, 3].forEach((i) =>
        add({
          instrument: "piano",
          midi: rh[ARPEGGIO[i * 2]] + 12,
          beat: beat + i * 0.5,
          beats: 1.5,
          velocity: 0.3,
        })
      );
    } else {
      const step = beat < 8 ? 1 : 0.5;
      for (let i = 0; i * step < beats; i += 1) {
        add({
          instrument: "piano",
          midi: rh[ARPEGGIO[i % ARPEGGIO.length]],
          beat: beat + i * step,
          beats: step * 2.5,
          velocity: (i % 4 === 0 ? 0.42 : 0.32) * lift,
        });
      }
    }

    // Strings enter with the working day.
    if (beat >= chat && !finale) {
      add({
        instrument: "cello",
        midi: above(chord.bass, 36),
        beat,
        beats,
        velocity: inSettings ? 0.3 : 0.38,
      });
      if (!inSettings) {
        const [third, fifth] = voicing(chord.tones, 58, 3).slice(1);
        add({ instrument: "viola", midi: third, beat, beats, velocity: 0.34 });
        if (beat >= snippet) add({ instrument: "viola", midi: fifth, beat, beats, velocity: 0.28 });
      }
    }

    // Off-beat pizzicato gives the middle of the day its pulse.
    if (beat >= snippet && beat < settings) {
      const pizz = voicing(chord.tones, 62, 4);
      for (let i = 0; i < beats; i += 1) {
        add({
          instrument: "pizz",
          midi: pizz[i % pizz.length],
          beat: beat + i + 0.5,
          beats: 0.4,
          velocity: 0.4,
        });
      }
    }
  }

  // Night: the violins take the tune.
  const melody: [number, number, number][] = [
    [F + 72, 0, 2],
    [AB + 72, 2, 2],
    [BB + 72, 4, 3],
    [AB + 72, 7, 1],
    [EB + 72, 8, 2],
    [C + 72, 10, 2],
  ];
  for (const [midi, offset, beats] of melody) {
    add({ instrument: "violins", midi, beat: night + offset, beats, velocity: 0.62 });
    add({ instrument: "violins", midi: midi - 12, beat: night + offset, beats, velocity: 0.42 });
  }

  // Finale: the strings swell on the tonic and fall away, leaving the piano alone.
  for (const midi of [DB + 36, DB + 48])
    add({ instrument: "cello", midi, beat: outro, beats: 3, velocity: 0.4 });
  add({ instrument: "viola", midi: F + 60, beat: outro, beats: 2.5, velocity: 0.3 });
  add({ instrument: "violins", midi: AB + 72, beat: outro, beats: 2.5, velocity: 0.34 });
  for (const [midi, offset] of [
    [AB + 72, 3],
    [F + 72, 3.5],
    [EB + 72, 4],
    [DB + 72, 5],
  ] as const) {
    add({ instrument: "piano", midi, beat: outro + offset, beats: 3, velocity: 0.36 });
  }

  // Harp glissandi lift the title and the logo.
  const pentatonic = [DB, EB, F, AB, BB];
  const glissando = (beat: number) => {
    const run = voicing(pentatonic, 61, 12);
    run.forEach((midi, i) =>
      add({
        instrument: "harp",
        midi,
        beat: beat + i * (0.9 / run.length),
        beats: 2,
        velocity: 0.35 + i * 0.02,
      })
    );
  };
  glissando(cueBeat("intro", "title") - 0.9);
  glissando(cueBeat("outro", "logo") - 0.9);

  // Sparkles: a quick rising chord tone pair on each landing.
  for (const [scene, cue] of SPARKLE_CUES) {
    const beat = cueBeat(scene, cue as CueName<typeof scene>);
    const chord = chordAt(beat);
    const [first, second] = voicing(chord.tones, 84, 2);
    add({ instrument: "glock", midi: first, beat, beats: 1.5, velocity: 0.34 });
    add({ instrument: "glock", midi: second, beat: beat + 0.25, beats: 1.5, velocity: 0.28 });
  }

  return notes.sort((a, b) => a.beat - b.beat || a.midi - b.midi);
}

const chordAt = (beat: number) => {
  const span = chordSpans().find(
    (candidate) => beat >= candidate.beat && beat < candidate.beat + candidate.beats
  );
  return (span ?? chordSpans()[0]).chord;
};
