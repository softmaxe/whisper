"""The red-pen scratch: scratchy, bounded, deterministic and made of separate strokes."""

from __future__ import annotations

import numpy as np

from whisper_audio import SAMPLE_RATE as SR
from whisper_audio.sfx import pen_scratch


def render(params: dict | None = None, seed: int = 0) -> np.ndarray:
    return pen_scratch.render({"type": "pen_scratch", "at": 0, "params": params or {}}, SR, np.random.default_rng(seed))


def test_scratch_is_deterministic_for_a_given_rng():
    assert np.array_equal(render(seed=5), render(seed=5))


def test_scratch_is_short_and_matches_its_declared_length():
    for strokes in (2, 3, 4):
        clip = render({"strokes": strokes})
        assert abs(clip.shape[0] - pen_scratch.length({"strokes": strokes}) * SR) <= strokes + 1
        assert clip.shape[0] < 0.5 * SR


def test_scratch_peaks_at_its_gain_and_ends_in_silence():
    clip = render({"gain": 0.25})
    assert abs(np.max(np.abs(clip)) - 0.25) < 1e-9
    assert abs(clip[-1]) < 1e-6


def test_scratch_is_scratchy_not_tonal():
    clip = render()
    energy = np.abs(np.fft.rfft(clip)) ** 2
    freqs = np.fft.rfftfreq(clip.shape[0], 1 / SR)
    centroid = float(np.sum(freqs * energy) / np.sum(energy))
    assert 1800 < centroid < 8000


def test_scratch_has_a_quiet_gap_between_strokes():
    clip = render({"strokes": 3, "stroke": 0.08})
    stroke = int(0.08 * SR)
    gap = int(pen_scratch.GAP * SR)
    during = np.sqrt(np.mean(clip[stroke // 4 : stroke // 2] ** 2))
    between = np.sqrt(np.mean(clip[stroke + gap // 4 : stroke + 3 * gap // 4] ** 2))
    assert between < 0.2 * during
