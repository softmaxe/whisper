const PITCH_CLASSES: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };

/** MIDI number of a sample-file note name such as "C4", "A#2", or Salamander's "Ds1". */
export function noteNameToMidi(name: string): number | null {
  const match = /^([A-G])(#|s)?(-?\d)$/.exec(name);
  if (!match) return null;
  const [, letter, sharp, octave] = match;
  return PITCH_CLASSES[letter] + (sharp ? 1 : 0) + (Number(octave) + 1) * 12;
}

/**
 * The sample whose root is closest to `midi`. On a tie the higher root wins,
 * since pitching a sample down sounds more natural than stretching it up.
 */
export function nearestSample<T extends { midi: number }>(samples: T[], midi: number): T {
  let best = samples[0];
  for (const sample of samples) {
    const distance = Math.abs(sample.midi - midi);
    const bestDistance = Math.abs(best.midi - midi);
    if (distance < bestDistance || (distance === bestDistance && sample.midi > best.midi)) {
      best = sample;
    }
  }
  return best;
}
