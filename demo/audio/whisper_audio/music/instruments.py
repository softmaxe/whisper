"""Synthesised instruments for the score. Every voice returns a mono clip.

Nothing here knows about the Film's timing; `layers/music.py` places the notes.
Randomness always comes from a caller-supplied generator so rebuilds are identical.
"""

from __future__ import annotations

import numpy as np

from ..dsp import attack_release, exp_decay, midi_hz, seconds


def karplus_strong(
    note: float,
    length: float,
    sr: int,
    rng: np.random.Generator,
    brightness: float = 0.5,
    decay: float = 0.996,
) -> np.ndarray:
    """A plucked string: a noise burst circulating in a delay line with a two-tap averaging low-pass.

    `brightness` (0..1) shapes the initial burst (lower = softer, more like a thumb pluck);
    `decay` is the loop gain per period. Computed one period at a time, so a note costs
    about `length * f` numpy operations rather than a Python loop per sample.
    """
    period = max(2, int(round(sr / midi_hz(note))))
    n = seconds(length, sr)
    burst = rng.uniform(-1.0, 1.0, period)
    burst = brightness * burst + (1.0 - brightness) * np.convolve(burst, [0.5, 0.5], mode="same")
    burst -= np.mean(burst)

    # buf[0] is a leading zero so that buf[i - period - 1] exists for the first loop.
    buf = np.zeros(n + 1)
    buf[1 : 1 + min(period, n)] = burst[: min(period, n)]
    for k in range(period + 1, n + 1, period):
        end = min(k + period, n + 1)
        buf[k:end] = decay * 0.5 * (buf[k - period : end - period] + buf[k - period - 1 : end - period - 1])
    out = buf[1:]
    return out * attack_release(n, seconds(0.002, sr), seconds(0.08, sr))


# Kalimba tine partials: (frequency ratio, amplitude, decay seconds). The upper
# partials are inharmonic and die away fast, which gives the bell-like attack.
_KALIMBA_PARTIALS = ((1.0, 1.0, 1.1), (2.76, 0.32, 0.22), (5.4, 0.14, 0.07))


def kalimba(note: float, length: float, sr: int, rng: np.random.Generator) -> np.ndarray:
    """A kalimba tine: a few decaying sine partials at inharmonic ratios plus a tiny thumb click."""
    n = seconds(length, sr)
    t = np.arange(n) / sr
    f = midi_hz(note)
    out = np.zeros(n)
    for ratio, amp, tau in _KALIMBA_PARTIALS:
        out += amp * np.sin(2 * np.pi * f * ratio * t + rng.uniform(0, 2 * np.pi)) * np.exp(-t / tau)
    click = rng.standard_normal(n) * exp_decay(n, 0.0015 * sr) * 0.12
    return (out + click) * attack_release(n, seconds(0.001, sr), seconds(0.06, sr))
