"""Files dropped on an app: a few soft paper pats landing in quick succession.

Each file is a short low "pat" (a damped thump, like a sheet landing on a
desk) with a brief rustle of paper on top; the pats fall a little apart, in
the order the files land. Used for the batch Upload drop in Beat 3.

Params (all optional): `files` (number of pats, default 3), `spacing`
(seconds between pats, default 0.07), `gain` (default 0.45).
"""

from __future__ import annotations

import numpy as np

from ..dsp import exp_decay, fade, fft_filter, normalise, seconds

# The drop zone is right of centre, where the Whisper window sits.
PAN = 0.2

PAT = 0.16


def length(params: dict) -> float:
    """Clip length in seconds for these params."""
    files = max(1, int(params.get("files", 3)))
    return (files - 1) * float(params.get("spacing", 0.07)) + PAT


def _pat(sr: int, rng: np.random.Generator, n: int) -> np.ndarray:
    t = np.arange(n) / sr
    # A damped low thump that drops in pitch as the sheet settles.
    hz = rng.uniform(110.0, 140.0)
    sweep = hz * (1.0 + 0.6 * np.exp(-t / 0.015))
    thump = np.sin(2 * np.pi * np.cumsum(sweep) / sr) * exp_decay(n, 0.03 * sr)
    # Paper brushing the surface: band-passed noise, quick in and out.
    rustle = fft_filter(rng.standard_normal(n), sr, lowpass=5200, highpass=900)
    rustle *= np.minimum(t / 0.004, 1.0) * exp_decay(n, 0.035 * sr)
    return 0.9 * thump + 0.35 * rustle


def render(cue: dict, sr: int, rng: np.random.Generator) -> np.ndarray:
    params = cue.get("params", {})
    files = max(1, int(params.get("files", 3)))
    spacing = float(params.get("spacing", 0.07))
    gain = float(params.get("gain", 0.45))

    n_pat = seconds(PAT, sr)
    step = seconds(spacing, sr)
    out = np.zeros((files - 1) * step + n_pat)
    for i in range(files):
        # Later files land a touch softer, on top of the first.
        out[i * step : i * step + n_pat] += _pat(sr, rng, n_pat) * (1.0 - 0.15 * i) * rng.uniform(0.9, 1.05)

    return normalise(fade(out, sr, attack=0.001, release=0.02), gain)
