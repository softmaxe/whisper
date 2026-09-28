"""The Upload file drop: short, bounded, deterministic, one soft pat per file."""

from __future__ import annotations

import numpy as np

from whisper_audio import SAMPLE_RATE as SR
from whisper_audio.sfx import file_drop


def render(params: dict | None = None, seed: int = 0) -> np.ndarray:
    return file_drop.render({"type": "file_drop", "at": 0, "params": params or {}}, SR, np.random.default_rng(seed))


def test_drop_is_deterministic_for_a_given_rng():
    assert np.array_equal(render(seed=3), render(seed=3))


def test_drop_matches_its_declared_length_and_stays_short():
    for files in (1, 3, 5):
        clip = render({"files": files})
        assert abs(clip.shape[0] - file_drop.length({"files": files}) * SR) <= files + 1
        assert clip.shape[0] < 0.6 * SR


def test_drop_peaks_at_its_gain_and_ends_in_silence():
    clip = render({"gain": 0.3})
    assert abs(np.max(np.abs(clip)) - 0.3) < 1e-9
    assert abs(clip[-1]) < 1e-6


def test_drop_is_a_soft_low_pat_not_a_click():
    clip = render()
    energy = np.abs(np.fft.rfft(clip)) ** 2
    freqs = np.fft.rfftfreq(clip.shape[0], 1 / SR)
    centroid = float(np.sum(freqs * energy) / np.sum(energy))
    assert 80 < centroid < 1500


def test_each_file_lands_as_its_own_pat():
    spacing = 0.07
    clip = render({"files": 3, "spacing": spacing})
    step = int(spacing * SR)
    onset = int(0.003 * SR)
    # Just before each later pat the sound has decayed; just after, it jumps back up.
    for i in (1, 2):
        before = np.max(np.abs(clip[i * step - onset * 3 : i * step]))
        after = np.max(np.abs(clip[i * step : i * step + onset * 4]))
        assert after > 1.3 * before
