"""Mixes every layer in `whisper_audio.layers` into the final stereo track.

A layer is any module in `whisper_audio/layers/` exposing

    GAIN: float                      # optional, default 1.0
    def render(timeline, sr) -> np.ndarray  # shape (n_samples, 2)

Layers are discovered automatically, so adding music or ambience means adding
a new module rather than editing this file.
"""

from __future__ import annotations

import importlib
import pkgutil
import wave
from pathlib import Path

import numpy as np

from . import layers
from .timeline import Timeline, film_samples


def discover_layers():
    for info in sorted(pkgutil.iter_modules(layers.__path__), key=lambda m: m.name):
        if not info.name.startswith("_"):
            yield importlib.import_module(f"{layers.__name__}.{info.name}")


def render_layers(timeline: Timeline, sr: int) -> dict[str, np.ndarray]:
    """Every layer's output at its GAIN, keyed by layer name (e.g. "music", "sfx")."""
    n = film_samples(timeline, sr)
    rendered = {}
    for layer in discover_layers():
        out = np.asarray(layer.render(timeline, sr), dtype=np.float64)
        if out.shape != (n, 2):
            raise ValueError(f"layer {layer.__name__} returned {out.shape}, expected {(n, 2)}")
        rendered[layer.__name__.rsplit(".", 1)[-1]] = getattr(layer, "GAIN", 1.0) * out
    return rendered


def render_mix(timeline: Timeline, sr: int) -> np.ndarray:
    n = film_samples(timeline, sr)
    return master(sum(render_layers(timeline, sr).values(), np.zeros((n, 2))))


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
