"""Automatic paste: the cleaned text lands in the Target app.

A quick paper swish (the text sliding into place) that ends in a soft, round
"pop" as it settles, with a faint bright tap on top so it reads over the music.

Put the cue where the pasted text appears (the pill's `doneAt`).
Params (all optional): `gain` (default 0.42), `pitch` (multiplier on the pop, default 1.0).
"""

from __future__ import annotations

import numpy as np

from ..dsp import exp_decay, fade, fft_filter, normalise, seconds

# App windows sit right of centre, where Whis's text lands.
PAN = 0.2

SWISH = 0.07
LENGTH = 0.22


def render(cue: dict, sr: int, rng: np.random.Generator) -> np.ndarray:
    params = cue.get("params", {})
    gain = float(params.get("gain", 0.42))
    pitch = float(params.get("pitch", 1.0))

    n = seconds(LENGTH, sr)
    out = np.zeros(n)

    # The swish: band-passed noise swelling towards the moment the text lands.
    swish_n = seconds(SWISH, sr)
    u = np.linspace(0.0, 1.0, swish_n)
    swish = fft_filter(rng.standard_normal(swish_n), sr, lowpass=5200, highpass=1400)
    out[:swish_n] += 0.35 * swish * u**1.6

    # The landing: a round, slightly falling pop and a small bright tap.
    land = swish_n
    pop_n = n - land
    t = np.arange(pop_n) / sr
    hz = 330.0 * pitch * rng.uniform(0.97, 1.03)
    sweep = hz * (1.0 + 0.35 * np.exp(-t / 0.012))
    pop = np.sin(2 * np.pi * np.cumsum(sweep) / sr) * exp_decay(pop_n, 0.035 * sr)
    tap = fft_filter(rng.standard_normal(pop_n), sr, lowpass=7000, highpass=2500) * exp_decay(pop_n, 0.004 * sr)
    out[land:] += pop + 0.45 * tap

    return normalise(fade(out, sr, attack=0.002, release=0.01), gain)
