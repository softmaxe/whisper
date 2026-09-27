"""The music bed (minimal tracer version): a quiet plucked-string arpeggio on the
home chord in every bar at 84 BPM, a kalimba note every other bar, a short
fade-in and a fade to silence with the picture's closing fade.

Reads only the Film length from the timeline. The full score (arranged by
Beat, read by key from the timeline) replaces this module.
"""

from __future__ import annotations

import numpy as np

from ..dsp import place, seconds
from ..music.instruments import kalimba, karplus_strong
from ..timeline import Timeline, film_samples

GAIN = 0.5

BPM = 84
BEAT = 60.0 / BPM
BAR = 4 * BEAT
# D major: the home chord as a rising arpeggio, one note per beat (MIDI notes).
ARPEGGIO = (50, 57, 62, 66)
KALIMBA_NOTE = 74
FADE_IN = 0.25
FADE_OUT = 4.0
# No new note starts this close to the end, so the fade ends in silence.
LAST_NOTE_MARGIN = 1.5


def fade_envelope(n: int, sr: int) -> np.ndarray:
    """0.25 s fade-in, then a squared raised-cosine fade over the last FADE_OUT seconds, reaching 0 at the end."""
    t = np.arange(n) / sr
    duration = n / sr
    env = np.clip(t / FADE_IN, 0.0, 1.0)
    remaining = np.clip((duration - t) / FADE_OUT, 0.0, 1.0)
    return env * (0.5 * (1.0 - np.cos(np.pi * remaining))) ** 2


def render(timeline: Timeline, sr: int) -> np.ndarray:
    n = film_samples(timeline, sr)
    duration = n / sr
    rng = np.random.default_rng(84)
    out = np.zeros((n, 2))

    bar = 0
    while bar * BAR < duration - LAST_NOTE_MARGIN:
        start = bar * BAR
        for i, note in enumerate(ARPEGGIO):
            at = start + i * BEAT
            if at >= duration - LAST_NOTE_MARGIN:
                break
            velocity = 0.9 if i == 0 else 0.6
            clip = velocity * karplus_strong(note, 2.4, sr, rng, brightness=0.45)
            place(out, clip, seconds(at, sr), pan=-0.3 + 0.2 * i)
        if bar % 2 == 1:
            place(out, 0.35 * kalimba(KALIMBA_NOTE, 1.6, sr, rng), seconds(start + 2 * BEAT, sr), pan=0.35)
        bar += 1

    peak = float(np.max(np.abs(out))) or 1.0
    return (out / peak) * fade_envelope(n, sr)[:, None]
