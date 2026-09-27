"""The red pen on paper: a few scratchy felt-tip strokes (a circle, a strike-through, an arrow).

Each stroke is a burst of band-passed noise (the nib dragging on paper) with
an uneven drag, separated by tiny gaps where the pen turns. Adapted from the
Pelican Test Film's pen_scribble.

Put one cue where each red-pen mark starts drawing (the moment its `progress`
leaves 0). Params (all optional): `strokes` (default 3), `stroke` (seconds per
stroke, default 0.09), `pitch` (brightness multiplier, default 1.0),
`gain` (default 0.4).
"""

from __future__ import annotations

import numpy as np

from ..dsp import fade, fft_filter, normalise, seconds

GAP = 0.018


def length(params: dict) -> float:
    """Clip length in seconds for these params."""
    strokes = max(1, int(params.get("strokes", 3)))
    stroke = float(params.get("stroke", 0.09))
    return strokes * stroke + (strokes - 1) * GAP


def render(cue: dict, sr: int, rng: np.random.Generator) -> np.ndarray:
    params = cue.get("params", {})
    strokes = max(1, int(params.get("strokes", 3)))
    stroke = float(params.get("stroke", 0.09))
    pitch = float(params.get("pitch", 1.0))
    gain = float(params.get("gain", 0.4))

    stroke_len = seconds(stroke, sr)
    gap_len = seconds(GAP, sr)
    out = np.zeros(strokes * stroke_len + (strokes - 1) * gap_len)
    u = np.linspace(0.0, 1.0, stroke_len)
    for i in range(strokes):
        # Alternate strokes differ a little in brightness, like a hand moving back and forth.
        tilt = 1.0 + 0.18 * (i % 2)
        grain = fft_filter(rng.standard_normal(stroke_len), sr, lowpass=6500 * pitch * tilt, highpass=1800 * pitch)
        # Fast attack as the nib bites, then an uneven drag that eases off at the turn.
        env = np.minimum(u / 0.12, 1.0) * (1.0 - u) ** 0.6
        env *= 1.0 + 0.35 * np.sin(2 * np.pi * (7 + 2 * i) * u + rng.uniform(0, 2 * np.pi))
        start = i * (stroke_len + gap_len)
        out[start : start + stroke_len] += grain * env * (0.85 + 0.15 * rng.random())

    return normalise(fade(out, sr, attack=0.002, release=0.006), gain)
