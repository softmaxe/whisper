"""A red-pen tick: a short down-stroke, a longer flick up, and a small bright "ding" of approval.

The strokes are band-passed noise like the pen scratch (a nib on paper); the
ding is a soft two-partial bell on G6 that rings out as the tick is finished.

Put the cue where the tick starts drawing (the moment its `progress` leaves 0).
Params (all optional): `gain` (default 0.42), `ding` (bell level relative to
the strokes, default 0.8; 0 = the pen alone).
"""

from __future__ import annotations

import numpy as np

from ..dsp import exp_decay, fade, fft_filter, normalise, seconds

DOWN = 0.045
TURN = 0.012
UP = 0.085
RING = 0.3
BELL_HZ = 1567.98  # G6, in the score's D major


def length() -> float:
    """Clip length in seconds."""
    return DOWN + TURN + UP + RING


def _stroke(n: int, sr: int, rng: np.random.Generator, bright: float, fall: float) -> np.ndarray:
    u = np.linspace(0.0, 1.0, n)
    grain = fft_filter(rng.standard_normal(n), sr, lowpass=6000 * bright, highpass=1900 * bright)
    return grain * np.minimum(u / 0.15, 1.0) * (1.0 - u) ** fall


def render(cue: dict, sr: int, rng: np.random.Generator) -> np.ndarray:
    params = cue.get("params", {})
    gain = float(params.get("gain", 0.42))
    ding = float(params.get("ding", 0.8))

    out = np.zeros(seconds(length(), sr))
    down_n = seconds(DOWN, sr)
    up_at = down_n + seconds(TURN, sr)
    up_n = seconds(UP, sr)
    out[:down_n] += 0.8 * _stroke(down_n, sr, rng, 0.9, 0.8)
    # The flick up speeds up and brightens as the pen lifts off.
    out[up_at : up_at + up_n] += _stroke(up_n, sr, rng, 1.2, 0.35)

    ring_at = up_at + up_n // 2
    ring_n = out.shape[0] - ring_at
    t = np.arange(ring_n) / sr
    bell = np.sin(2 * np.pi * BELL_HZ * t) + 0.3 * np.sin(2 * np.pi * BELL_HZ * 2.76 * t) * exp_decay(ring_n, 0.03 * sr)
    out[ring_at:] += ding * 0.55 * bell * exp_decay(ring_n, 0.09 * sr) * np.minimum(t / 0.003, 1.0)

    return normalise(fade(out, sr, attack=0.002, release=0.02), gain)
