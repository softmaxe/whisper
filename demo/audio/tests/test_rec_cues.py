"""Recording start and stop cues: the app's two notes, in order, short and clean."""

from __future__ import annotations

import numpy as np
import pytest

from whisper_audio import SAMPLE_RATE as SR
from whisper_audio.sfx import _app_cue, rec_start, rec_stop


def render(module, params: dict | None = None, seed: int = 0) -> np.ndarray:
    return module.render({"type": module.__name__.rsplit(".", 1)[-1], "at": 0, "params": params or {}}, SR, np.random.default_rng(seed))


def dominant_hz(x: np.ndarray) -> float:
    spectrum = np.abs(np.fft.rfft(x * np.hanning(x.shape[0]), n=1 << 16))
    return float(np.fft.rfftfreq(1 << 16, 1 / SR)[np.argmax(spectrum)])


@pytest.mark.parametrize("module", [rec_start, rec_stop])
def test_cue_is_deterministic_short_and_ends_in_silence(module):
    clip = render(module, seed=3)
    assert np.array_equal(clip, render(module, seed=3))
    assert abs(clip.shape[0] / SR - _app_cue.length(module.NOTES)) < 0.002
    assert abs(np.max(np.abs(clip)) - 0.3) < 1e-9
    assert abs(clip[-1]) < 1e-3


@pytest.mark.parametrize(("module", "notes"), [(rec_start, (523.25, 659.25)), (rec_stop, (587.33, 440.0))])
def test_cue_plays_the_apps_notes_in_order(module, notes):
    clip = render(module)
    step = round((_app_cue.NOTE_SECONDS + _app_cue.GAP_SECONDS) * SR)
    note_len = round(_app_cue.NOTE_SECONDS * SR)
    for i, hz in enumerate(notes):
        assert abs(dominant_hz(clip[i * step : i * step + note_len]) - hz) < 6, f"note {i + 1}"


def test_start_rises_and_stop_falls():
    assert rec_start.NOTES[1] > rec_start.NOTES[0]
    assert rec_stop.NOTES[1] < rec_stop.NOTES[0]
