"""Shared fixtures: the real timeline, exported fresh from the TypeScript source once per test run,
and a stand-in sample bank so the audio tests never depend on the build's instruments."""

from __future__ import annotations

import subprocess
from pathlib import Path

import numpy as np
import pytest

from whisper_audio.dsp import attack_release, midi_hz, seconds
from whisper_audio.music.bank import CHORDS, MELODY, Note, SampleBank
from whisper_audio.timeline import load_timeline

ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture(scope="session")
def timeline_json(tmp_path_factory) -> Path:
    out = tmp_path_factory.mktemp("timeline") / "timeline.json"
    subprocess.run(["node", "scripts/export-timeline.ts", str(out)], cwd=ROOT, check=True, capture_output=True)
    return out


@pytest.fixture(scope="session")
def timeline(timeline_json):
    return load_timeline(timeline_json)


def standin_voice(level: float, tau: float):
    """A plain decaying tone with two overtones: deterministic, and draws nothing from `rng`.

    `level` and `tau` (decay seconds) put each role at about the loudness of the build's voice,
    so the mix checks still mean something.
    """

    def render(note: Note, sr: int, rng: np.random.Generator) -> np.ndarray:
        n = seconds(note.duration, sr)
        t = np.arange(n) / sr
        f = midi_hz(note.pitch)
        tone = sum(amp * np.sin(2 * np.pi * f * k * t) for k, amp in ((1, 1.0), (2, 0.3), (3, 0.1)))
        decay = np.exp(-t / (tau * (2.5 if note.let_ring else 1.0)))
        return level * note.velocity * tone * decay * attack_release(n, seconds(0.003, sr), seconds(0.05, sr))

    return render


def standin_bank() -> SampleBank:
    return {CHORDS: standin_voice(0.67, 0.4), MELODY: standin_voice(0.9, 0.8)}


@pytest.fixture(scope="session")
def bank() -> SampleBank:
    return standin_bank()
