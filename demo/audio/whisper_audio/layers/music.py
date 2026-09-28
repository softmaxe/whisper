"""Music layer: the Film's score, played from the bar grid in `music/score.py`.

A warm C major score at about 76 BPM: accompaniment chords, a melody above
them, a round sine bass, light brushes, a soft pad and paper rustle, arranged
by Beat and read from the timeline. The chords and melody stems play their notes
through a sample bank (`music/bank.py`); the other stems are synthesised here:

    opening   a sparse arpeggio over a thin pad, a questioning melody, paper rustle
    speak     rolling chords; the bass and light brushes join on the downbeat
    more      the band carries on under a lighter melodic answer
    review    the brushes drop out; night: the pad thickens under ringing chords
              and a lullaby
    servers   the chords roll again over the night pad and resolve on Cadd9,
              which rings through a fade to silence at the end of the Film

The whole music bus then goes through a soft room and a lo-fi finish: tape-style
saturation and a gentle low-pass. The sound effects are a separate layer and stay dry.

The music reads only Beat keys and windows and the Film length from the
timeline. It ignores sound cues, so the sound-effect layer can grow freely.
"""

from __future__ import annotations

import numpy as np

from ..dsp import fft_filter, place, seconds, tape_saturate
from ..music import instruments as inst
from ..music.bank import CHORDS, MELODY, Note, SampleBank, check_bank
from ..music.score import CHORD_PATTERNS, MELODY_FIGURES, Bar, bars
from ..timeline import Timeline, film_samples

GAIN = 1.4
SEED = 84

# Level of each stem in the music bus, tuned against the sampled instruments so the bed sits
# at least 10 dB under the sound cues. The Rhodes melody leads; the piano sits under it.
# The round bass sustains where a plucked one decays, so its gain is small for the same loudness.
STEM_GAINS = {CHORDS: 0.135, "bass": 0.062, MELODY: 0.115, "drums": 0.16, "pad": 0.08, "paper": 0.07}
# How much of each stem goes to the shared room reverb.
ROOM_SENDS = {CHORDS: 0.3, "bass": 0.05, MELODY: 0.45, "drums": 0.2, "pad": 0.5, "paper": 0.0}
ROOM_LEVEL = 0.4
# A soft room: a slightly longer, darker tail than a bright live room.
ROOM = {"decay": 1.6, "predelay": 0.024, "tone": 2400.0}
# The lo-fi finish on the music bus: tape drive, then a gentle low-pass (and a DC-blocking high-pass).
TAPE_DRIVE = 1.5
BUS_LOWPASS = 5000.0
BUS_HIGHPASS = 30.0
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


def render_chords(grid: list[Bar], n: int, sr: int, rng: np.random.Generator, bank: SampleBank) -> np.ndarray:
    out = stereo(n)
    play = bank[CHORDS]
    for bar in grid:
        voicing = bar.chord.voicing
        if bar.last_in_film:
            # The home chord, rolled slowly from the bottom and left to ring through the fade.
            ring = bar.end - bar.start + 0.5
            for j, note in enumerate(voicing):
                clip = play(Note(note, 0.7 - 0.05 * j, ring, let_ring=True), sr, rng)
                place(out, clip, seconds(bar.at(0.0) + 0.06 * j, sr), pan=-0.3 + 0.1 * j)
            continue
        # Held a little past the next note, like a pianist's half pedal.
        ring = 1.6 if bar.section.comp == "rolling" else 3.2
        for pos, voice, vel in positions(CHORD_PATTERNS[bar.section.comp], bar):
            note = voicing[min(voice, len(voicing) - 1)]
            clip = play(Note(note, vel * rng.uniform(0.85, 1.0), ring), sr, rng)
            place(out, clip, played(bar, pos, sr, rng), pan=-0.3 + 0.1 * voice)
    # A darker top end for the soft, felt-like piano.
    return filtered(out, sr, lowpass=3000, highpass=60)


def render_bass(grid: list[Bar], n: int, sr: int, rng: np.random.Generator) -> np.ndarray:
    out = stereo(n)
    for bar in grid:
        if not bar.section.bass:
            continue
        root = bar.chord.bass
        # Root on the downbeat, the fifth on beat 3 in full bars (just the root at night).
        hits = [(0.0, root, 1.0)]
        if bar.beats >= 4 and not bar.section.night and not bar.last_in_film:
            hits.append((2.0, root + 7, 0.7))
        for i, (pos, note, vel) in enumerate(hits):
            # Each note is held until the next one, with a short overlap so the line stays legato.
            until = hits[i + 1][0] if i + 1 < len(hits) else bar.beats
            length = (until - pos) * bar.beat_len + (0.5 if bar.last_in_film else 0.12)
            clip = vel * inst.round_bass(note, length, sr, decay=2.2 if bar.last_in_film else 1.6)
            place(out, clip, played(bar, pos, sr, rng, 0.004))
    return filtered(out, sr, lowpass=650, highpass=35)


def render_melody(grid: list[Bar], n: int, sr: int, rng: np.random.Generator, bank: SampleBank) -> np.ndarray:
    out = stereo(n)
    play = bank[MELODY]
    for bar in grid:
        name = bar.section.melody
        if name is None:
            continue
        tones = bar.chord.tones
        if bar.last_in_film:
            ring = bar.end - bar.at(1.0) + 0.5
            place(out, play(Note(tones[-1], 0.6, ring, let_ring=True), sr, rng), seconds(bar.at(1.0), sr), pan=0.3)
            continue
        figures = MELODY_FIGURES[name]
        for pos, tone, vel in positions(figures[bar.index % len(figures)], bar):
            clip = play(Note(tones[tone % len(tones)], vel, 2.2), sr, rng)
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
                place(out, 0.7 * kick, played(bar, beat, sr, rng, 0.003))
            elif beat % 4 == 2:
                place(out, 0.3 * kick, played(bar, beat, sr, rng, 0.003))
            else:
                tap = taps[rng.integers(len(taps))]
                place(out, 0.45 * rng.uniform(0.85, 1.0) * tap, played(bar, beat, sr, rng, 0.005), pan=0.1)
            tick = ticks[rng.integers(len(ticks))]
            swing = 0.58  # lightly swung off-beat, in beats
            place(out, 0.14 * rng.uniform(0.7, 1.0) * tick, played(bar, beat + swing, sr, rng, 0.004), pan=0.35)
            if beat % 2 == 0:
                length = min(2, bar.beats - beat) * bar.beat_len
                pan = -0.2 if (beat // 2) % 2 == 0 else 0.2
                place(out, 0.08 * inst.brush_sweep(length, sr, rng), seconds(bar.at(beat), sr), pan=pan)
    return out


def render_pad(grid: list[Bar], n: int, sr: int, rng: np.random.Generator) -> np.ndarray:
    """A soft pad under the whole Film: a thin three-note floor by day, fuller and louder at night."""
    out = stereo(n)
    for bar in grid:
        thickness = bar.section.pad
        if thickness <= 0:
            continue
        notes = [bar.chord.bass + 12, *bar.chord.voicing[1:3]]
        if bar.section.night:
            # Night adds the upper voices, which carry the chord's colour (its 7th, 9th or 6th).
            notes += list(bar.chord.voicing[3:5])
        # Each chord overlaps the next a little so the pad never dips at a bar line.
        length = bar.beats * bar.beat_len + (0.5 if bar.last_in_film else 0.8)
        chunk = thickness * inst.pad(notes, length, sr, rng)
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


def stems(timeline: Timeline, sr: int, bank: SampleBank) -> dict[str, np.ndarray]:
    """Each stem, dry, at its mix level and before the fade. Shape (film_samples, 2).

    The melodic stems (CHORDS and MELODY) play their notes through `bank`; the rest are synthesised here.
    """
    check_bank(bank)
    n = film_samples(timeline, sr)
    grid = bars(timeline)
    rng = np.random.default_rng(SEED)
    renderers = {
        CHORDS: lambda: render_chords(grid, n, sr, rng, bank),
        "bass": lambda: render_bass(grid, n, sr, rng),
        MELODY: lambda: render_melody(grid, n, sr, rng, bank),
        "drums": lambda: render_drums(grid, n, sr, rng),
        "pad": lambda: render_pad(grid, n, sr, rng),
        "paper": lambda: render_paper(grid, n, sr, rng),
    }
    return {name: STEM_GAINS[name] * fn() for name, fn in renderers.items()}


def fade_envelope(n: int, sr: int) -> np.ndarray:
    """0.25 s fade-in, then a squared raised-cosine fade over the last FADE_OUT seconds, reaching 0 at the end."""
    t = np.arange(n) / sr
    duration = n / sr
    env = np.clip(t / FADE_IN, 0.0, 1.0)
    remaining = np.clip((duration - t) / FADE_OUT, 0.0, 1.0)
    return env * (0.5 * (1.0 - np.cos(np.pi * remaining))) ** 2


def lofi(bed: np.ndarray, sr: int) -> np.ndarray:
    """The music bus's warm finish: tape-style saturation, then a gentle low-pass."""
    return filtered(tape_saturate(bed, TAPE_DRIVE), sr, lowpass=BUS_LOWPASS, highpass=BUS_HIGHPASS)


def render(timeline: Timeline, sr: int, bank: SampleBank) -> np.ndarray:
    n = film_samples(timeline, sr)
    dry = stems(timeline, sr, bank)
    send = sum((ROOM_SENDS[name] * x for name, x in dry.items()), stereo(n))
    wet = inst.room(send, sr, np.random.default_rng(SEED + 1), **ROOM)
    bed = sum(dry.values(), stereo(n)) + ROOM_LEVEL * wet
    return lofi(bed, sr) * fade_envelope(n, sr)[:, None]
