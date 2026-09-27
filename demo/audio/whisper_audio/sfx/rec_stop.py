"""Recording stops: the app's falling two-note cue, D5 then A4 (STOP_NOTES in dictationCues.js).

Put the cue where speech ends and the Recording pill starts thinking (its `stopAt`).
Params (optional): `gain` (default 0.3).
"""

from __future__ import annotations

import numpy as np

from ._app_cue import render_notes

NOTES = (587.33, 440.0)
PAN = 0.2


def render(cue: dict, sr: int, rng: np.random.Generator) -> np.ndarray:
    return render_notes(NOTES, cue.get("params", {}), sr, rng)
