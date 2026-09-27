"""The score: its length, its arrangement by Beat, its ending, and its place in the mix.

The arrangement checks run on the real timeline and on a copy with different
Beat windows, so they prove the score follows the timeline instead of
hard-coding today's times.
"""

from __future__ import annotations

import copy
import math
from pathlib import Path

import numpy as np
import pytest

from whisper_audio import SAMPLE_RATE as SR
from whisper_audio.layers import music
from whisper_audio.mix import master, render_layers
from whisper_audio.music.score import BPM, HOME, bars, chord_at
from whisper_audio.timeline import all_cues, beat

PACKAGE = Path(music.__file__).resolve().parents[2]


def db(x: float) -> float:
    return 20 * np.log10(max(float(x), 1e-12))


def rms(x: np.ndarray) -> float:
    return float(np.sqrt(np.mean(x**2))) if x.size else 0.0


def reshaped(timeline: dict, windows: dict[str, tuple[float, float]]) -> dict:
    """A copy of the timeline with new Beat windows (and no cues: the music must not need them)."""
    out = copy.deepcopy(timeline)
    for b in out["beats"]:
        b["start"], b["end"] = windows[b["key"]]
        b["cues"] = []
    out["durationSeconds"] = max(end for _, end in windows.values())
    return out


SHIFTED = {
    "opening": (0.0, 12.0),
    "speak": (12.0, 27.5),
    "more": (27.5, 45.0),
    "review": (45.0, 61.0),
    "servers": (61.0, 80.0),
}


@pytest.fixture(scope="module", params=["real", "shifted"])
def any_timeline(request, timeline):
    return timeline if request.param == "real" else reshaped(timeline, SHIFTED)


@pytest.fixture(scope="module")
def layers(timeline):
    return render_layers(timeline, SR)


@pytest.fixture(scope="module")
def stems(any_timeline):
    return any_timeline, music.stems(any_timeline, SR)


def section(stem: np.ndarray, start: float, end: float) -> np.ndarray:
    return stem[max(0, round(start * SR)) : max(0, round(end * SR))]


def test_the_score_lasts_exactly_the_film(any_timeline):
    duration = any_timeline["durationSeconds"]
    assert music.render(any_timeline, SR).shape == (round(duration * SR), 2)
    grid = bars(any_timeline)
    assert grid[0].start == 0.0
    assert math.isclose(grid[-1].end, duration)
    for a, b in zip(grid, grid[1:]):
        assert math.isclose(a.end, b.start), "bars are contiguous"


def test_every_beat_starts_on_a_downbeat_near_84_bpm(any_timeline):
    grid = bars(any_timeline)
    starts = [bar.start for bar in grid]
    for b in any_timeline["beats"]:
        assert any(math.isclose(b["start"], s) for s in starts), f"{b['key']} starts mid-bar"
    for bar in grid:
        assert abs(60.0 / bar.beat_len - BPM) <= 6, f"{bar.beat_key} bar {bar.index} is off tempo"
        assert 2 <= bar.beats <= 7


def test_drums_play_through_the_feature_beats_only(stems):
    timeline, parts = stems
    drums = parts["drums"]
    speak, more = beat(timeline, "speak"), beat(timeline, "more")
    assert np.max(np.abs(section(drums, 0, speak["start"]))) == 0, "no drums in the Opening"
    assert np.max(np.abs(section(drums, more["end"] + 0.8, timeline["durationSeconds"]))) == 0, "drums stop after"
    for b in (speak, more):
        for t in np.arange(b["start"], b["end"] - 1, 1.0):
            assert db(rms(section(drums, t, t + 1))) > -50, f"drums missing at {t:.1f}s"


def test_the_pad_comes_in_at_night(stems):
    timeline, parts = stems
    pad = parts["pad"]
    review = beat(timeline, "review")
    # Only the filter's pre-ring of a slow swell may reach back past the Beat start.
    assert db(np.max(np.abs(section(pad, 0, review["start"] - 0.05)))) < -90
    for t in np.arange(review["start"] + 1.5, timeline["durationSeconds"] - music.FADE_OUT, 1.0):
        assert db(rms(section(pad, t, t + 1))) > -50, f"pad missing at {t:.1f}s"


def test_the_opening_is_sparser_than_the_feature_beats(stems):
    timeline, parts = stems
    bed = sum(parts.values())
    opening, speak = beat(timeline, "opening"), beat(timeline, "speak")
    assert db(rms(section(bed, 1, opening["end"]))) < db(rms(section(bed, speak["start"], speak["end"]))) - 2


def test_the_score_resolves_on_the_home_chord(any_timeline):
    end = any_timeline["durationSeconds"]
    assert chord_at(any_timeline, end - 0.01) == HOME
    last = bars(any_timeline)[-1]
    assert last.last_in_film and last.chord == HOME
    assert end - last.start >= music.FADE_OUT - 0.5, "the home chord rings through the fade"


def test_music_ignores_sound_cues(timeline):
    """The score and the sound effects are independent layers: adding cues never changes the music."""
    without_cues = reshaped(timeline, {b["key"]: (b["start"], b["end"]) for b in timeline["beats"]})
    assert np.array_equal(music.render(without_cues, SR), music.render(timeline, SR))


def test_music_plays_until_the_fade(timeline, layers):
    fade_start = timeline["durationSeconds"] - music.FADE_OUT
    for second in range(int(fade_start)):
        window = layers["music"][second * SR : (second + 1) * SR]
        assert db(rms(window)) > -40, f"music too quiet at {second}s"


def test_music_fades_out_with_the_picture(timeline, layers):
    """Still sounding (quietly) as the picture starts fading to paper, silent at the very end."""
    picture_fade = layers["music"][-round(timeline["fadeOutSeconds"] * SR) :]
    first_half = picture_fade[: picture_fade.shape[0] // 2]
    assert -75 < db(rms(first_half)) < -35
    assert db(np.max(np.abs(layers["music"][-SR // 100 :]))) < -90


def test_final_mix_is_quiet_in_the_last_second(layers):
    assert db(rms(master(sum(layers.values()))[-SR:])) < -40


def test_layers_sum_below_full_scale(layers):
    """No clipping: the summed layers stay in tanh's gentle region before mastering."""
    total = sum(layers.values())
    assert np.max(np.abs(total)) < 0.8
    assert np.max(np.abs(master(total))) <= 0.89


def test_every_cue_stands_out_over_the_music(timeline, layers):
    for cue in all_cues(timeline):
        at = round(cue["at"] * SR)
        cue_peak = np.max(np.abs(layers["sfx"][at : at + SR // 5]))
        bed = rms(layers["music"][max(at - SR // 2, 0) : at + SR // 2])
        assert db(cue_peak) - db(bed) > 10, f"{cue['type']} at {cue['at']}s is buried"


def test_no_recorded_samples_are_used():
    audio_files = [
        p
        for p in PACKAGE.rglob("*")
        if p.suffix.lower() in {".wav", ".mp3", ".flac", ".ogg", ".aif", ".aiff", ".m4a", ".sf2"}
        and ".venv" not in p.parts
    ]
    assert audio_files == []
    for source in (PACKAGE / "whisper_audio").rglob("*.py"):
        text = source.read_text(encoding="utf-8")
        assert "http" not in text and "urllib" not in text, f"{source.name} fetches something"
