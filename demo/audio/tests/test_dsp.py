"""Shared DSP helpers used by every sound effect."""

from __future__ import annotations

import numpy as np

from whisper_audio.dsp import fade, normalise

SR = 48_000


def test_normalise_scales_the_peak_to_the_given_level():
    x = np.array([0.0, -0.2, 0.1])
    assert np.isclose(np.max(np.abs(normalise(x, 0.5))), 0.5)
    assert np.array_equal(normalise(np.zeros(4), 0.5), np.zeros(4))


def test_fade_starts_and_ends_in_silence():
    x = fade(np.ones(SR // 10), SR, attack=0.002, release=0.01)
    assert x[0] == 0.0 and x[-1] == 0.0
    assert np.all(x[SR // 100 : SR // 20] == 1.0)
