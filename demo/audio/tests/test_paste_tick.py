"""Paste and tick cues: short, deterministic, ending in silence, with the shape their names promise."""

from __future__ import annotations

import numpy as np
import pytest

from whisper_audio import SAMPLE_RATE as SR
from whisper_audio.sfx import paste, tick


def render(module, params: dict | None = None, seed: int = 0) -> np.ndarray:
    return module.render({"type": module.__name__.rsplit(".", 1)[-1], "at": 0, "params": params or {}}, SR, np.random.default_rng(seed))


def rms(x: np.ndarray) -> float:
    return float(np.sqrt(np.mean(x**2)))


@pytest.mark.parametrize("module", [paste, tick])
def test_cue_is_deterministic_short_and_ends_in_silence(module):
    clip = render(module, seed=4)
    assert np.array_equal(clip, render(module, seed=4))
    assert clip.shape[0] < 0.5 * SR
    assert abs(np.max(np.abs(clip)) - 0.42) < 1e-9
    assert abs(clip[-1]) < 1e-4


@pytest.mark.parametrize("module", [paste, tick])
def test_cue_is_loud_within_the_first_fifth_of_a_second(module):
    # The music test compares each cue's peak in its first 0.2 s with the bed.
    assert np.max(np.abs(render(module)[: SR // 5])) > 0.3


def test_paste_swells_into_its_landing():
    clip = render(paste)
    land = round(paste.SWISH * SR)
    assert rms(clip[: land // 3]) < rms(clip[land : land + land // 2])


def test_tick_ends_with_a_ring_on_its_bell_note():
    clip = render(tick)
    tail = clip[-round(0.2 * SR) :]
    spectrum = np.abs(np.fft.rfft(tail * np.hanning(tail.shape[0]), n=1 << 16))
    assert abs(np.fft.rfftfreq(1 << 16, 1 / SR)[np.argmax(spectrum)] - tick.BELL_HZ) < 10


def test_tick_without_the_ding_is_just_the_pen():
    clip = render(tick, {"ding": 0})
    assert rms(clip[-round(0.15 * SR) :]) < 1e-3
