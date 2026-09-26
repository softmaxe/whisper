// The beat grid shared by the picture and the score. Scenes and their cues are
// counted in beats, so every cut, key press, and paste lands on the music. To
// cut the picture to a different track, change BPM and MUSIC_OFFSET_SECONDS.

export const FPS = 30;
export const WIDTH = 1920;
export const HEIGHT = 1080;

export const BPM = 76;
export const BEATS_PER_BAR = 4;
/** Where beat 0 falls in the audio track; the lead-in before it is skipped. */
export const MUSIC_OFFSET_SECONDS = 0;
export const SECONDS_PER_BEAT = 60 / BPM;

/**
 * Scenes in playback order. Cues are beats from the scene start; `tap`
 * marks the first of a Double tap on the Dictation hotkey.
 */
export const SCENES = [
  { id: "intro", beats: 6, cues: { key: 0.5, tap: 2, pill: 3, title: 3.5 } },
  {
    id: "morning",
    beats: 14,
    cues: { push: 1.5, tap: 4, speak: 5, stop: 8.5, paste: 9.5, cleanup: 10.5 },
  },
  {
    id: "chat",
    beats: 10,
    cues: { push: 1, tap: 2, speak: 2.5, stop: 4.5, paste: 5, fix: 6, learn: 8 },
  },
  { id: "snippet", beats: 8, cues: { push: 1, tap: 2, speak: 2.5, stop: 4, paste: 4.5 } },
  { id: "hold", beats: 6, cues: { press: 0.5, speak: 1, release: 3.5, paste: 4 } },
  { id: "upload", beats: 8, cues: { push: 1, drop: 2, progress: 3, done: 6 } },
  {
    id: "night",
    beats: 12,
    cues: { push: 1, search: 2.5, copy: 5, insights: 6.5, count: 7 },
  },
  { id: "settings", beats: 4, cues: { asr: 0.5, cleanup: 1.5, tagline: 2 } },
  { id: "outro", beats: 8, cues: { pull: 0, logo: 1.5, install: 3, link: 5.5 } },
] as const;

export type Scene = (typeof SCENES)[number];
export type SceneId = Scene["id"];
export type CueName<Id extends SceneId> = keyof Extract<Scene, { id: Id }>["cues"] & string;

export const TOTAL_BEATS = SCENES.reduce((sum, scene) => sum + scene.beats, 0);

export const beatToSeconds = (beat: number) => MUSIC_OFFSET_SECONDS + beat * SECONDS_PER_BEAT;
export const beatToFrame = (beat: number) => Math.round(beat * SECONDS_PER_BEAT * FPS);
export const TOTAL_FRAMES = beatToFrame(TOTAL_BEATS);

const sceneById = (id: SceneId) => {
  const scene = SCENES.find((candidate) => candidate.id === id);
  if (!scene) throw new Error(`Unknown scene ${id}`);
  return scene;
};

export const sceneStartBeat = (id: SceneId) => {
  let beat = 0;
  for (const scene of SCENES) {
    if (scene.id === id) return beat;
    beat += scene.beats;
  }
  throw new Error(`Unknown scene ${id}`);
};

export const sceneFrames = (id: SceneId) => {
  const start = sceneStartBeat(id);
  const from = beatToFrame(start);
  return { from, durationInFrames: beatToFrame(start + sceneById(id).beats) - from };
};

/** Absolute beat of a scene cue. */
export const cueBeat = <Id extends SceneId>(id: Id, cue: CueName<Id>) =>
  sceneStartBeat(id) + (sceneById(id).cues as Record<string, number>)[cue];

/** Frame of a scene cue relative to the scene start, for use inside a Sequence. */
export const cueFrame = <Id extends SceneId>(id: Id, cue: CueName<Id>) =>
  beatToFrame(cueBeat(id, cue)) - sceneFrames(id).from;

/** A scene's cue frames by name, for use inside that scene. */
export const sceneCues =
  <Id extends SceneId>(id: Id) =>
  (cue: CueName<Id>) =>
    cueFrame(id, cue);
