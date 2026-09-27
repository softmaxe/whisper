"""Recording starts: the app's rising two-note cue, C5 then E5 (START_NOTES in dictationCues.js).

Put the cue where the Recording pill opens (see `doubleTapCues` in timeline/helpers.ts).
Params (optional): `gain` (default 0.3).
"""

from __future__ import annotations

import numpy as np

from ._app_cue import render_notes

NOTES = (523.25, 659.25)
# The pill floats a little right of centre, where the Film draws it.
PAN = 0.2


def render(cue: dict, sr: int, rng: np.random.Generator) -> np.ndarray:
    return render_notes(NOTES, cue.get("params", {}), sr, rng)
