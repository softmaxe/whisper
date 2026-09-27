"""The synthesiser against the real timeline (exported fresh from the TypeScript source).

Run with `npm run test:audio` (or `uv run --project audio pytest audio/tests`).
"""

from __future__ import annotations

import subprocess
import sys
import wave
from pathlib import Path

import numpy as np
import pytest

from whisper_audio import SAMPLE_RATE as SR
from whisper_audio.mix import render_layers
from whisper_audio.timeline import all_cues, load_timeline

ROOT = Path(__file__).resolve().parents[2]


def db(x: float) -> float:
    return 20 * np.log10(max(x, 1e-12))


def rms(x: np.ndarray) -> float:
    return float(np.sqrt(np.mean(x**2))) if x.size else 0.0


@pytest.fixture(scope="module")
def timeline_json(tmp_path_factory) -> Path:
    out = tmp_path_factory.mktemp("timeline") / "timeline.json"
    subprocess.run(["node", "scripts/export-timeline.ts", str(out)], cwd=ROOT, check=True, capture_output=True)
    return out


@pytest.fixture(scope="module")
def timeline(timeline_json):
    return load_timeline(timeline_json)


@pytest.fixture(scope="module")
def wav(timeline_json, tmp_path_factory) -> np.ndarray:
    """The CLI's output, read back as float stereo samples."""
    out = tmp_path_factory.mktemp("audio") / "audio.wav"
    subprocess.run([sys.executable, "-m", "whisper_audio", str(timeline_json), str(out)], check=True, capture_output=True)
    with wave.open(str(out), "rb") as w:
        assert (w.getnchannels(), w.getsampwidth(), w.getframerate()) == (2, 2, SR)
        pcm = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2")
    return pcm.reshape(-1, 2) / 32768.0


def test_the_wav_lasts_exactly_the_film(timeline, wav):
    assert wav.shape[0] == round(timeline["durationSeconds"] * SR)


def test_every_cue_type_has_a_synthesiser_that_sounds_at_the_cue(timeline):
    sfx = render_layers(timeline, SR)["sfx"]
    cues = all_cues(timeline)
    assert cues, "the timeline has sound cues"
    for cue in cues:
        start = round(cue["at"] * SR)
        window = sfx[start : start + SR // 10]
        before = sfx[max(0, start - SR // 200) : start]
        assert db(np.max(np.abs(window))) > -30, f"{cue['type']}@{cue['at']} is silent"
        # Nothing sounds early: the clip starts at the cue, not before it.
        assert before.size == 0 or np.max(np.abs(before)) < np.max(np.abs(window))


def test_sound_is_present_in_the_mix_at_each_cue(timeline, wav):
    for cue in all_cues(timeline):
        start = round(cue["at"] * SR)
        assert db(np.max(np.abs(wav[start : start + SR // 10]))) > -30, f"{cue['type']}@{cue['at']}"


def test_music_plays_under_the_film_and_fades_out(timeline, wav):
    assert db(rms(wav[SR : 2 * SR])) > -45, "music audible near the start"
    assert db(rms(wav[-SR:])) < -40, "the last second has faded"
    assert np.max(np.abs(wav)) < 0.95, "no clipping"


def test_the_synthesiser_is_deterministic(timeline):
    a = render_layers(timeline, SR)
    b = render_layers(timeline, SR)
    for name in a:
        assert np.array_equal(a[name], b[name]), name
