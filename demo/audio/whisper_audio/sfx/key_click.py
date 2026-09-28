"""One keyboard keystroke: a soft plastic "thock" with a bright click on top.

Params (all optional): `key` ("char" default, "space" = deeper and longer,
"enter" = firmer, with a release click), `gain` (default 0.32).
Every cue gets its own rng, so no two keystrokes sound exactly alike.
"""

from __future__ import annotations

import numpy as np

from ..dsp import exp_decay, fade, normalise, seconds

# The keyboard sits a little left of centre, under Clawd's hands.
PAN = -0.25

_VOICES = {
    # key: (thock Hz, thock decay s, click brightness 0..1, length s, gain scale)
    "char": (240.0, 0.012, 0.8, 0.07, 1.0),
    "space": (150.0, 0.022, 0.55, 0.11, 1.15),
    "enter": (190.0, 0.018, 0.7, 0.16, 1.35),
}


def _one_pole_lowpass(x: np.ndarray, alpha: float) -> np.ndarray:
    y = np.empty_like(x)
    acc = 0.0
    for i, v in enumerate(x):
        acc += alpha * (v - acc)
        y[i] = acc
    return y


def _strike(sr: int, rng: np.random.Generator, thock_hz: float, decay: float, bright: float, n: int) -> np.ndarray:
    t = np.arange(n) / sr
    hz = thock_hz * rng.uniform(0.9, 1.12)
    thock = np.sin(2 * np.pi * hz * t) * exp_decay(n, decay * sr)

    noise = rng.standard_normal(n)
    # Band-limit the noise: remove the lows (plastic, not a thud) and the harshest highs.
    band = _one_pole_lowpass(noise, 0.35 + 0.4 * bright) - _one_pole_lowpass(noise, 0.04)
    click = band * exp_decay(n, 0.0035 * sr) * (0.9 + 0.3 * rng.random())
    return 0.55 * thock + 0.9 * click


def render(cue: dict, sr: int, rng: np.random.Generator) -> np.ndarray:
    params = cue.get("params", {})
    key = str(params.get("key", "char"))
    thock_hz, decay, bright, length, scale = _VOICES.get(key, _VOICES["char"])
    gain = float(params.get("gain", 0.32)) * scale * rng.uniform(0.8, 1.05)

    n = seconds(length, sr)
    out = _strike(sr, rng, thock_hz, decay, bright, n)
    if key == "enter":
        # Key bottoms out, then springs back with a quieter release click.
        rel_at = seconds(0.075, sr)
        out[rel_at:] += 0.45 * _strike(sr, rng, thock_hz * 1.3, decay * 0.6, bright, n - rel_at)

    return normalise(fade(out, sr, attack=0.0008), gain)
