"""Mixes the layers in `whisper_audio.layers` into the final stereo track.

Each layer module exposes

    GAIN: float                      # optional, default 1.0
    def render(timeline, sr, ...) -> np.ndarray  # shape (n_samples, 2)

and is listed in `render_layers` below. The music layer also takes the sample
bank (`music/bank.py`) that plays its melodic stems.
"""

from __future__ import annotations

import wave
from pathlib import Path

import numpy as np

from .layers import music, sfx
from .music.bank import SampleBank, default_bank
from .timeline import Timeline, film_samples


def render_layers(timeline: Timeline, sr: int, bank: SampleBank | None = None) -> dict[str, np.ndarray]:
    """Every layer's output at its GAIN, keyed by layer name ("music", "sfx").

    `bank` plays the music's melodic stems; without one, the build's `default_bank()` is used.
    """
    bank = default_bank() if bank is None else bank
    n = film_samples(timeline, sr)
    layers = {
        "music": (music, lambda: music.render(timeline, sr, bank)),
        "sfx": (sfx, lambda: sfx.render(timeline, sr)),
    }
    rendered = {}
    for name, (layer, render) in layers.items():
        out = np.asarray(render(), dtype=np.float64)
        if out.shape != (n, 2):
            raise ValueError(f"layer {name} returned {out.shape}, expected {(n, 2)}")
        rendered[name] = getattr(layer, "GAIN", 1.0) * out
    return rendered


def render_mix(timeline: Timeline, sr: int, bank: SampleBank | None = None) -> np.ndarray:
    n = film_samples(timeline, sr)
    return master(sum(render_layers(timeline, sr, bank).values(), np.zeros((n, 2))))


def master(mix: np.ndarray, ceiling: float = 0.89) -> np.ndarray:
    """Soft-clip peaks and normalise down (never up) to the ceiling."""
    mix = np.tanh(mix)
    peak = float(np.max(np.abs(mix))) if mix.size else 0.0
    if peak > ceiling:
        mix *= ceiling / peak
    return mix


def write_wav(path: Path, mix: np.ndarray, sr: int) -> None:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    pcm = (np.clip(mix, -1.0, 1.0) * 32767.0).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(pcm.tobytes())
