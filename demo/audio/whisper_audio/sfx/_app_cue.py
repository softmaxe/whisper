"""The app's own dictation cues (src/utils/dictationCues.js), shared by rec_start and rec_stop.

The app plays two short sine notes: each 90 ms long, 25 ms apart, with a
15 ms linear attack and an exponential fall to near silence. The Film uses the
same notes and envelope, adding only a faint octave partial so the tone
carries over the music.
"""

from __future__ import annotations

import numpy as np

from ..dsp import fade, normalise, seconds

NOTE_SECONDS = 0.09
GAP_SECONDS = 0.025
ATTACK_SECONDS = 0.015
_MIN_GAIN = 0.0001 / 0.2  # the app's MIN_GAIN relative to its MAX_GAIN
_OCTAVE = 0.12


def length(notes: tuple[float, ...]) -> float:
    """Clip length in seconds for a cue of `notes`."""
    return len(notes) * NOTE_SECONDS + (len(notes) - 1) * GAP_SECONDS


def _note(hz: float, sr: int, rng: np.random.Generator) -> np.ndarray:
    n = seconds(NOTE_SECONDS, sr)
    t = np.arange(n) / sr
    attack = seconds(ATTACK_SECONDS, sr)
    env = np.empty(n)
    env[:attack] = np.linspace(_MIN_GAIN, 1.0, attack, endpoint=False)
    # exponentialRampToValueAtTime(MIN_GAIN, stop): geometric fall from 1 to MIN over the rest.
    env[attack:] = _MIN_GAIN ** np.linspace(0.0, 1.0, n - attack)
    phase = rng.uniform(0, 2 * np.pi)
    tone = np.sin(2 * np.pi * hz * t + phase) + _OCTAVE * np.sin(4 * np.pi * hz * t + 2 * phase)
    return tone * env


def render_notes(notes: tuple[float, ...], params: dict, sr: int, rng: np.random.Generator) -> np.ndarray:
    gain = float(params.get("gain", 0.3))
    step = seconds(NOTE_SECONDS + GAP_SECONDS, sr)
    out = np.zeros(seconds(length(notes), sr) + 1)
    for i, hz in enumerate(notes):
        note = _note(hz, sr, rng)
        out[i * step : i * step + note.shape[0]] += note
    return normalise(fade(out, sr, release=0.004), gain)
