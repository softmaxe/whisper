"""Sound-effect layer: renders every timeline sound cue.

Each cue `type` maps to `whisper_audio/sfx/<type>.py`, which exposes

    def render(cue: dict, sr: int, rng: np.random.Generator) -> np.ndarray  # mono
    PAN: float  # optional, -1 (left) .. 1 (right)

Add a new effect by adding a module; this file does not need to change.
"""

from __future__ import annotations

import hashlib
import importlib

import numpy as np

from .. import sfx
from ..dsp import place, seconds
from ..timeline import Timeline, all_cues, film_samples

GAIN = 1.0


def load_effect(cue_type: str):
    try:
        return importlib.import_module(f"{sfx.__name__}.{cue_type}")
    except ModuleNotFoundError as err:
        raise ValueError(f"no synthesiser for sound cue type {cue_type!r} (add whisper_audio/sfx/{cue_type}.py)") from err


def cue_seed(cue: dict) -> int:
    """A stable RNG seed from the cue's type and time.

    Unlike its position in the cue list, this does not change when cues are
    added or removed elsewhere in the Film, so every other cue keeps its sound.
    """
    key = f"{cue['type']}@{float(cue['at']):.6f}".encode()
    return int.from_bytes(hashlib.sha256(key).digest()[:8], "little")


def render(timeline: Timeline, sr: int) -> np.ndarray:
    out = np.zeros((film_samples(timeline, sr), 2))
    for cue in all_cues(timeline):
        effect = load_effect(cue["type"])
        rng = np.random.default_rng(cue_seed(cue))  # deterministic per cue
        clip = np.asarray(effect.render(cue, sr, rng), dtype=np.float64)
        pan = float(cue.get("params", {}).get("pan", getattr(effect, "PAN", 0.0)))
        place(out, clip, seconds(cue["at"], sr), pan)
    return out
