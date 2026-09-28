"""Shared DSP helpers used by every sound effect."""

from __future__ import annotations

import numpy as np

from whisper_audio.dsp import fade, normalise, tape_saturate

SR = 48_000


def test_normalise_scales_the_peak_to_the_given_level():
    x = np.array([0.0, -0.2, 0.1])
    assert np.isclose(np.max(np.abs(normalise(x, 0.5))), 0.5)
    assert np.array_equal(normalise(np.zeros(4), 0.5), np.zeros(4))


def test_fade_starts_and_ends_in_silence():
    x = fade(np.ones(SR // 10), SR, attack=0.002, release=0.01)
    assert x[0] == 0.0 and x[-1] == 0.0
    assert np.all(x[SR // 100 : SR // 20] == 1.0)


def test_tape_saturation_leaves_quiet_sound_alone_and_rounds_off_peaks():
    quiet = 0.001 * np.sin(np.linspace(0, 20, 200))
    assert np.allclose(tape_saturate(quiet), quiet, atol=1e-5)
    loud = np.array([-2.0, 2.0])
    assert np.all(np.abs(tape_saturate(loud)) < np.abs(loud))
