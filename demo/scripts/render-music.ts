// Renders src/music/score.ts to public/music.wav with sampled instruments:
// Salamander Grand Piano (CC-BY 3.0) and VSCO 2 Community Edition strings,
// harp, and glockenspiel (CC0). Samples are cached in .cache/samples; ffmpeg
// adds the hall reverb, loudness, and the closing fade.
//
//   node scripts/render-music.ts

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { composeScore, type Instrument, type Note } from "../src/music/score.ts";
import { nearestSample, noteNameToMidi } from "../src/music/pitch.ts";
import { TOTAL_BEATS, beatToSeconds } from "../src/timeline.ts";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CACHE = join(ROOT, ".cache", "samples");
const MIX = join(ROOT, ".cache", "mix");
const OUTPUT = join(ROOT, "public", "music.wav");
const RATE = 48000;
const DURATION = beatToSeconds(TOTAL_BEATS) - beatToSeconds(0);

const SALAMANDER = "https://tonejs.github.io/audio/salamander/";
const VSCO = "https://raw.githubusercontent.com/sgossner/VSCO-2-CE/master/";
const VSCO_TREE = "https://api.github.com/repos/sgossner/VSCO-2-CE/git/trees/master?recursive=1";

interface SampleFile {
  url: string;
  midi: number;
}

interface Voice {
  files: () => Promise<SampleFile[]>;
  gain: number;
  /** -1 left .. 1 right */
  pan: number;
  /** Share of the signal sent to the hall. */
  reverb: number;
  attack: number;
  /** Seconds to fade after the note ends; null lets the sample ring out. */
  release: number | null;
}

let vscoTree: string[] | null = null;

async function vscoFiles(pattern: RegExp, octaveShift: number): Promise<SampleFile[]> {
  if (!vscoTree) {
    const cached = join(CACHE, "vsco-tree.json");
    if (!existsSync(cached)) {
      const response = await fetch(VSCO_TREE);
      if (!response.ok) throw new Error(`VSCO listing failed: ${response.status}`);
      writeFileSync(cached, await response.text());
    }
    const tree = JSON.parse(readFileSync(cached, "utf8")) as { tree: { path: string }[] };
    vscoTree = tree.tree.map((entry) => entry.path);
  }
  const seen = new Set<number>();
  const files: SampleFile[] = [];
  for (const path of vscoTree) {
    const match = pattern.exec(path);
    const midi = match ? noteNameToMidi(match[1]) : null;
    if (midi === null || seen.has(midi)) continue;
    seen.add(midi);
    const url = VSCO + path.split("/").map(encodeURIComponent).join("/");
    files.push({ url, midi: midi + octaveShift });
  }
  if (files.length === 0) throw new Error(`No VSCO samples match ${pattern}`);
  return files;
}

async function salamanderFiles(): Promise<SampleFile[]> {
  const files: SampleFile[] = [];
  for (let octave = 0; octave <= 7; octave += 1) {
    for (const name of ["C", "Ds", "Fs", "A"]) {
      const note = `${name}${octave}`;
      const midi = noteNameToMidi(note)!;
      if (midi >= 21) files.push({ url: `${SALAMANDER}${note}.mp3`, midi });
    }
  }
  files.push({ url: `${SALAMANDER}C8.mp3`, midi: 108 });
  return files;
}

// VSCO names its string sections an octave below sounding pitch (measured:
// "susvib_C3" sounds C4); the harp and glockenspiel are named as they sound.
const VOICES: Record<Instrument, Voice> = {
  piano: { files: salamanderFiles, gain: 0.9, pan: 0, reverb: 0.32, attack: 0.002, release: 0.9 },
  cello: {
    files: () => vscoFiles(/Cello Section\/susvib\/susvib_([A-G]#?\d)_v1_1\.wav$/, 12),
    gain: 0.55,
    pan: -0.3,
    reverb: 0.5,
    attack: 0.18,
    release: 0.9,
  },
  viola: {
    files: () => vscoFiles(/Viola Section\/susvib\/ViolaEns_susvib_([A-G]#?\d)_v1_1\.wav$/, 12),
    gain: 0.42,
    pan: 0.25,
    reverb: 0.55,
    attack: 0.2,
    release: 0.9,
  },
  violins: {
    files: () => vscoFiles(/Violin Section\/susVib\/VlnEns_susVib_([A-G]#?\d)_v1\.wav$/, 12),
    gain: 0.5,
    pan: -0.12,
    reverb: 0.55,
    attack: 0.22,
    release: 1.1,
  },
  pizz: {
    files: () => vscoFiles(/Violin Section\/Pizz\/VlnEns_Pizz_([A-G]#?\d)_v1_rr1\.wav$/, 12),
    gain: 0.5,
    pan: 0.35,
    reverb: 0.4,
    attack: 0.001,
    release: null,
  },
  harp: {
    files: () => vscoFiles(/Strings\/Harp\/KSHarp_([A-G]#?\d)_(?:mf|f|mp)\.wav$/, 0),
    gain: 0.45,
    pan: 0.3,
    reverb: 0.55,
    attack: 0.001,
    release: null,
  },
  glock: {
    files: () => vscoFiles(/Percussion\/Glock\/glock_medium_([A-G]#?\d)\.wav$/, 0),
    gain: 0.22,
    pan: -0.25,
    reverb: 0.6,
    attack: 0.001,
    release: null,
  },
};

interface Sample {
  midi: number;
  left: Float32Array;
  right: Float32Array;
}

async function download(url: string) {
  const name = decodeURIComponent(new URL(url).pathname).replace(/[^\w.#-]+/g, "_");
  const file = join(CACHE, name);
  if (!existsSync(file)) {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Download failed ${response.status}: ${url}`);
    writeFileSync(file, Buffer.from(await response.arrayBuffer()));
  }
  return file;
}

function decode(file: string) {
  const args = ["-v", "error", "-i", file, "-ac", "2", "-ar", String(RATE), "-f", "f32le", "-"];
  const result = spawnSync("ffmpeg", args, { maxBuffer: 1 << 30 });
  if (result.status !== 0) throw new Error(`ffmpeg could not decode ${file}: ${result.stderr}`);
  const bytes = new Uint8Array(result.stdout as Buffer);
  const interleaved = new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 4);
  const frames = interleaved.length / 2;
  const left = new Float32Array(frames);
  const right = new Float32Array(frames);
  for (let i = 0; i < frames; i += 1) {
    left[i] = interleaved[i * 2];
    right[i] = interleaved[i * 2 + 1];
  }
  return { left, right };
}

async function loadVoice(instrument: Instrument): Promise<Sample[]> {
  const samples: Sample[] = [];
  for (const { url, midi } of await VOICES[instrument].files()) {
    samples.push({ midi, ...decode(await download(url)) });
  }
  process.stdout.write(`  ${instrument}: ${samples.length} samples\n`);
  return samples;
}

// Deterministic humanising so every render is identical.
let seed = 7;
const random = () => {
  seed = (seed * 16807) % 2147483647;
  return seed / 2147483647;
};

type Stereo = [Float32Array, Float32Array];

function renderNote(note: Note, sample: Sample, voice: Voice, dry: Stereo, send: Stereo) {
  const ratio = Math.pow(2, (note.midi - sample.midi) / 12);
  // Loosen the piano's quavers a touch so they breathe like a player's.
  const jitter = note.instrument === "piano" && note.beats < 2 ? (random() - 0.5) * 0.014 : 0;
  const start = Math.round((beatToSeconds(note.beat) - beatToSeconds(0) + jitter) * RATE);
  const heldSeconds = beatToSeconds(note.beat + note.beats) - beatToSeconds(note.beat);
  const sampleSeconds = sample.left.length / ratio / RATE;
  const { release } = voice;
  const seconds = release === null ? sampleSeconds : Math.min(sampleSeconds, heldSeconds + release);
  const frames = Math.floor(seconds * RATE);
  const velocity = Math.pow(note.velocity, 1.4) * (0.94 + random() * 0.12) * voice.gain;
  const panLeft = Math.min(1, 1 - voice.pan);
  const panRight = Math.min(1, 1 + voice.pan);
  const attackFrames = Math.max(1, voice.attack * RATE);
  const releaseStart = release === null ? Infinity : heldSeconds * RATE;
  const releaseFrames = release === null ? 1 : release * RATE;
  const tailFrames = 0.08 * RATE;

  for (let i = 0; i < frames; i += 1) {
    const out = start + i;
    if (out < 0) continue;
    if (out >= dry[0].length) break;
    const position = i * ratio;
    const index = Math.floor(position);
    if (index + 1 >= sample.left.length) break;
    const fraction = position - index;
    let envelope = Math.min(1, i / attackFrames, (frames - i) / tailFrames);
    if (i > releaseStart) envelope *= Math.max(0, 1 - (i - releaseStart) / releaseFrames);
    const gain = envelope * velocity;
    const left = sample.left[index] + (sample.left[index + 1] - sample.left[index]) * fraction;
    const right = sample.right[index] + (sample.right[index + 1] - sample.right[index]) * fraction;
    dry[0][out] += left * gain * panLeft;
    dry[1][out] += right * gain * panRight;
    send[0][out] += left * gain * panLeft * voice.reverb;
    send[1][out] += right * gain * panRight * voice.reverb;
  }
}

/** A stereo hall impulse: decorrelated noise with a darkening exponential decay. */
function hallImpulse(seconds: number, rt60: number): Stereo {
  const frames = Math.floor(seconds * RATE);
  const channels: Stereo = [new Float32Array(frames), new Float32Array(frames)];
  const predelay = Math.floor(0.022 * RATE);
  for (const channel of channels) {
    let low = 0;
    for (let i = predelay; i < frames; i += 1) {
      const t = (i - predelay) / RATE;
      // The low-pass closes as the tail ages, so high frequencies die first.
      const cutoff = 0.55 * Math.exp(-t * 1.6) + 0.06;
      low += cutoff * (random() * 2 - 1 - low);
      channel[i] = low * Math.pow(10, (-3 * t) / rt60);
    }
  }
  return channels;
}

function writeWav(file: string, channels: Stereo) {
  const frames = channels[0].length;
  const data = Buffer.alloc(frames * 8);
  for (let i = 0; i < frames; i += 1) {
    data.writeFloatLE(channels[0][i], i * 8);
    data.writeFloatLE(channels[1][i], i * 8 + 4);
  }
  const header = Buffer.alloc(44);
  header.write("RIFF", 0);
  header.writeUInt32LE(36 + data.length, 4);
  header.write("WAVE", 8);
  header.write("fmt ", 12);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(3, 20); // IEEE float
  header.writeUInt16LE(2, 22);
  header.writeUInt32LE(RATE, 24);
  header.writeUInt32LE(RATE * 8, 28);
  header.writeUInt16LE(8, 32);
  header.writeUInt16LE(32, 34);
  header.write("data", 36);
  header.writeUInt32LE(data.length, 40);
  writeFileSync(file, Buffer.concat([header, data]));
}

function ffmpeg(args: string[]) {
  const result = spawnSync("ffmpeg", ["-y", "-v", "error", ...args], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(`ffmpeg failed: ${result.stderr}`);
}

async function main() {
  mkdirSync(CACHE, { recursive: true });
  mkdirSync(MIX, { recursive: true });
  mkdirSync(dirname(OUTPUT), { recursive: true });

  const score = composeScore();
  process.stdout.write("Loading samples\n");
  const library = new Map<Instrument, Sample[]>();
  for (const instrument of new Set(score.map((note) => note.instrument))) {
    library.set(instrument, await loadVoice(instrument));
  }

  const frames = Math.ceil((DURATION + 0.5) * RATE);
  const dry: Stereo = [new Float32Array(frames), new Float32Array(frames)];
  const send: Stereo = [new Float32Array(frames), new Float32Array(frames)];
  for (const note of score) {
    const sample = nearestSample(library.get(note.instrument)!, note.midi);
    renderNote(note, sample, VOICES[note.instrument], dry, send);
  }

  writeWav(join(MIX, "dry.wav"), dry);
  writeWav(join(MIX, "send.wav"), send);
  writeWav(join(MIX, "hall.wav"), hallImpulse(3.4, 2.6));

  // Mix, then normalise in two passes so the arc from solo piano to full
  // strings keeps its dynamics instead of being levelled out.
  const premaster = join(MIX, "premaster.wav");
  const fadeStart = (DURATION - 1.6).toFixed(3);
  ffmpeg([
    ...["-i", join(MIX, "dry.wav"), "-i", join(MIX, "send.wav"), "-i", join(MIX, "hall.wav")],
    "-filter_complex",
    "[1:a][2:a]afir=dry=0:wet=1:gtype=none,volume=0.35[verb];" +
      "[0:a][verb]amix=inputs=2:normalize=0,highpass=f=40,bass=g=-3:f=140:w=0.8," +
      `afade=t=in:d=0.05,afade=t=out:st=${fadeStart}:d=1.6`,
    ...["-t", DURATION.toFixed(3), "-c:a", "pcm_f32le", premaster],
  ]);
  const target = "I=-16:TP=-1.5:LRA=18";
  const measure = spawnSync(
    "ffmpeg",
    [
      "-hide_banner",
      "-i",
      premaster,
      "-af",
      `loudnorm=${target}:print_format=json`,
      "-f",
      "null",
      "-",
    ],
    { encoding: "utf8" }
  );
  if (measure.status !== 0) throw new Error(`ffmpeg loudness pass failed: ${measure.stderr}`);
  const measured = measure.stderr;
  const stats = JSON.parse(
    measured.slice(measured.lastIndexOf("{"), measured.lastIndexOf("}") + 1)
  );
  const linear =
    `loudnorm=${target}:linear=true:measured_I=${stats.input_i}:measured_TP=${stats.input_tp}` +
    `:measured_LRA=${stats.input_lra}:measured_thresh=${stats.input_thresh}:offset=${stats.target_offset}`;
  ffmpeg(["-i", premaster, "-af", `${linear},aresample=${RATE}`, "-c:a", "pcm_s16le", OUTPUT]);
  process.stdout.write(`Wrote ${OUTPUT} (${DURATION.toFixed(2)} s)\n`);
}

await main();
