"""Music layer: the Film's score, synthesised from the bar grid in `music/score.py`.

Warm plucked strings (Karplus-Strong guitar and bass), kalimba, brushed drums,
a soft night pad and paper rustle, arranged by Beat and read from the timeline:

    opening   sparse guitar arpeggio, a questioning kalimba figure, paper rustle
    speak     rolling guitar, plucked bass and brushed drums join on the downbeat
    more      the band carries on under a lighter kalimba answer
    review    drums drop out; night: a soft pad, ringing plucks, a kalimba lullaby
    servers   the guitar rolls again over the pad and resolves on the home chord,
              which rings through a fade to silence at the end of the Film

The music reads only Beat keys and windows and the Film length from the
timeline. It ignores sound cues, so the sound-effect layer can grow freely.
"""

from __future__ import annotations

import numpy as np

from ..dsp import fft_filter, place, seconds
from ..music import instruments as inst
from ..music.score import GUITAR_PATTERNS, KALIMBA_FIGURES, Bar, bars
from ..timeline import Timeline, film_samples

GAIN = 1.4
SEED = 84

# Level of each stem in the music bus, tuned so the bed sits about 12 dB under the sound cues.
STEM_GAINS = {"guitar": 0.3, "bass": 0.34, "kalimba": 0.1, "drums": 0.2, "pad": 0.09, "paper": 0.07}
# How much of each stem goes to the shared room reverb.
ROOM_SENDS = {"guitar": 0.35, "bass": 0.1, "kalimba": 0.5, "drums": 0.25, "pad": 0.4, "paper": 0.0}
ROOM_LEVEL = 0.45
FADE_IN = 0.25
# The fade to silence at the end: a squared raised cosine over the last FADE_OUT seconds
# (about -33 dB one second before the end, where the picture starts fading to paper).
FADE_OUT = 4.0


def stereo(n: int) -> np.ndarray:
    return np.zeros((n, 2))


def filtered(x: np.ndarray, sr: int, **cutoffs: float) -> np.ndarray:
    """`fft_filter` on a whole stem, zero-padded so ringing at the Film's end can't wrap round to its start."""
    pad = np.zeros((sr, x.shape[1]))
    return fft_filter(np.concatenate([x, pad]), sr, **cutoffs)[: x.shape[0]]


def played(bar: Bar, position: float, sr: int, rng: np.random.Generator, spread: float = 0.006) -> int:
    """Sample index of a note at `position` in `bar`, loosened by a few ms like a human player.

    Never earlier than the bar's downbeat, so nothing leaks before a Beat starts.
    """
    return seconds(max(bar.at(position) + float(rng.normal(0.0, spread)), bar.start), sr)


def positions(pattern: list, bar: Bar) -> list:
    """The pattern's events that fit the bar; a bar longer than 4 beats repeats the first beat's events."""
    events = [e for e in pattern if e[0] < bar.beats]
    for extra in range(4, bar.beats):
        events += [(extra + p - int(p), *rest) for p, *rest in pattern if int(p) == extra - 4]
    return events


def render_guitar(grid: list[Bar], n: int, sr: int, rng: np.random.Generator) -> np.ndarray:
    out = stereo(n)
    for bar in grid:
        voicing = bar.chord.voicing
        if bar.last_in_film:
            # The home chord, strummed slowly and left to ring through the fade.
            for j, note in enumerate(voicing):
                clip = (0.85 - 0.05 * j) * inst.karplus_strong(note, 5.0, sr, rng, brightness=0.4, decay=0.998)
                place(out, clip, seconds(bar.at(0.0) + 0.045 * j, sr), pan=-0.35 + 0.12 * j)
            continue
        ring = 1.2 if bar.section.guitar == "rolling" else 2.8
        for pos, voice, vel in positions(GUITAR_PATTERNS[bar.section.guitar], bar):
            note = voicing[min(voice, len(voicing) - 1)]
            clip = vel * rng.uniform(0.85, 1.0) * inst.karplus_strong(note, ring, sr, rng, brightness=0.35)
            place(out, clip, played(bar, pos, sr, rng), pan=-0.3 + 0.1 * voice)
    return filtered(out, sr, lowpass=3600, highpass=70)


def render_bass(grid: list[Bar], n: int, sr: int, rng: np.random.Generator) -> np.ndarray:
    out = stereo(n)
    for bar in grid:
        if not bar.section.bass:
            continue
        root = bar.chord.bass
        # Root on the downbeat, the fifth on beat 3 in full bars (just the root at night).
        hits = [(0.0, root, 1.0)]
        if bar.beats >= 4 and not bar.section.pad and not bar.last_in_film:
            hits.append((2.0, root + 7, 0.7))
        for pos, note, vel in hits:
            length = min(bar.beats - pos, 2.0) * bar.beat_len + 0.6
            if bar.last_in_film:
                length = 5.0
            clip = vel * inst.karplus_strong(note, length, sr, rng, brightness=0.2, decay=0.999)
            place(out, clip, played(bar, pos, sr, rng, 0.004))
    return filtered(out, sr, lowpass=650, highpass=35)


def render_kalimba(grid: list[Bar], n: int, sr: int, rng: np.random.Generator) -> np.ndarray:
    out = stereo(n)
    for bar in grid:
        name = bar.section.kalimba
        if name is None:
            continue
        tones = bar.chord.tones
        if bar.last_in_film:
            place(out, 0.6 * inst.kalimba(tones[-1], 5.0, sr, rng, ring=2.5), seconds(bar.at(1.0), sr), pan=0.3)
            continue
        figures = KALIMBA_FIGURES[name]
        for pos, tone, vel in positions(figures[bar.index % len(figures)], bar):
            clip = vel * inst.kalimba(tones[tone % len(tones)], 1.8, sr, rng)
            place(out, clip, played(bar, pos, sr, rng, 0.008), pan=0.25 + 0.05 * tone)
    return out


def render_drums(grid: list[Bar], n: int, sr: int, rng: np.random.Generator) -> np.ndarray:
    """Brushed kit: felt kick on 1 (and softly on 3), brush slaps on 2 and 4, tip ticks on the off-beats,
    and a circling brush sweep under every two beats."""
    out = stereo(n)
    kick = inst.soft_kick(sr)
    taps = [inst.brush_tap(sr, rng) for _ in range(3)]
    ticks = [inst.brush_tick(sr, rng) for _ in range(4)]
    for bar in grid:
        if not bar.section.drums:
            continue
        for beat in range(bar.beats):
            if beat % 4 == 0:
                place(out, 0.9 * kick, played(bar, beat, sr, rng, 0.003))
            elif beat % 4 == 2:
                place(out, 0.45 * kick, played(bar, beat, sr, rng, 0.003))
            else:
                tap = taps[rng.integers(len(taps))]
                place(out, 0.7 * rng.uniform(0.85, 1.0) * tap, played(bar, beat, sr, rng, 0.005), pan=0.1)
            tick = ticks[rng.integers(len(ticks))]
            swing = 0.58  # lightly swung off-beat, in beats
            place(out, 0.25 * rng.uniform(0.7, 1.0) * tick, played(bar, beat + swing, sr, rng, 0.004), pan=0.35)
            if beat % 2 == 0:
                length = min(2, bar.beats - beat) * bar.beat_len
                pan = -0.2 if (beat // 2) % 2 == 0 else 0.2
                place(out, 0.12 * inst.brush_sweep(length, sr, rng), seconds(bar.at(beat), sr), pan=pan)
    return out


def render_pad(grid: list[Bar], n: int, sr: int, rng: np.random.Generator) -> np.ndarray:
    out = stereo(n)
    for bar in grid:
        if not bar.section.pad:
            continue
        notes = [bar.chord.bass + 12, *bar.chord.voicing[1:4]]
        # Each chord overlaps the next a little so the pad never dips at a bar line.
        length = bar.beats * bar.beat_len + (5.0 if bar.last_in_film else 0.8)
        chunk = inst.pad(notes, length, sr, rng)
        start = seconds(bar.start, sr)
        end = min(n, start + chunk.shape[0])
        out[start:end] += chunk[: end - start]
    return filtered(out, sr, lowpass=1800, highpass=90)


def render_paper(grid: list[Bar], n: int, sr: int, rng: np.random.Generator) -> np.ndarray:
    """Paper rustle under each Beat, at that Beat's rate."""
    out = stereo(n)
    for bar in grid:
        start, end = seconds(bar.start, sr), min(n, seconds(bar.end, sr))
        if end > start and bar.section.paper > 0:
            out[start:end] += inst.paper_rustle(end - start, sr, rng, rate=bar.section.paper)
    return out


def stems(timeline: Timeline, sr: int) -> dict[str, np.ndarray]:
    """Each stem, dry, at its mix level and before the fade. Shape (film_samples, 2)."""
    n = film_samples(timeline, sr)
    grid = bars(timeline)
    rng = np.random.default_rng(SEED)
    renderers = {
        "guitar": render_guitar,
        "bass": render_bass,
        "kalimba": render_kalimba,
        "drums": render_drums,
        "pad": render_pad,
        "paper": render_paper,
    }
    return {name: STEM_GAINS[name] * fn(grid, n, sr, rng) for name, fn in renderers.items()}


def fade_envelope(n: int, sr: int) -> np.ndarray:
    """0.25 s fade-in, then a squared raised-cosine fade over the last FADE_OUT seconds, reaching 0 at the end."""
    t = np.arange(n) / sr
    duration = n / sr
    env = np.clip(t / FADE_IN, 0.0, 1.0)
    remaining = np.clip((duration - t) / FADE_OUT, 0.0, 1.0)
    return env * (0.5 * (1.0 - np.cos(np.pi * remaining))) ** 2


def render(timeline: Timeline, sr: int) -> np.ndarray:
    n = film_samples(timeline, sr)
    dry = stems(timeline, sr)
    send = sum((ROOM_SENDS[name] * x for name, x in dry.items()), stereo(n))
    wet = inst.room(send, sr, np.random.default_rng(SEED + 1))
    bed = sum(dry.values(), stereo(n)) + ROOM_LEVEL * wet
    return bed * fade_envelope(n, sr)[:, None]
