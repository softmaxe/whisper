"""The score: its length, its arrangement by Beat, its ending, and its place in the mix.

The arrangement checks run on the real timeline and on a copy with different
Beat windows, so they prove the score follows the timeline instead of
hard-coding today's times.
"""

from __future__ import annotations

import copy
import math

import numpy as np
import pytest

from whisper_audio import SAMPLE_RATE as SR
from whisper_audio.layers import music
from whisper_audio.dsp import seconds
from whisper_audio.mix import master, render_layers
from whisper_audio.music.bank import CHORDS, MELODY, ROLES, Note
from whisper_audio.music.score import BPM, bars
from whisper_audio.timeline import all_cues, beat


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
def layers(timeline, bank):
    return render_layers(timeline, SR, bank)


@pytest.fixture(scope="module")
def stems(any_timeline, bank):
    return any_timeline, music.stems(any_timeline, SR, bank)


def section(stem: np.ndarray, start: float, end: float) -> np.ndarray:
    return stem[max(0, round(start * SR)) : max(0, round(end * SR))]


def test_the_score_lasts_exactly_the_film(any_timeline, bank):
    duration = any_timeline["durationSeconds"]
    assert music.render(any_timeline, SR, bank).shape == (round(duration * SR), 2)
    grid = bars(any_timeline)
    assert grid[0].start == 0.0
    assert math.isclose(grid[-1].end, duration)
    for a, b in zip(grid, grid[1:]):
        assert math.isclose(a.end, b.start), "bars are contiguous"


def test_every_beat_starts_on_a_downbeat_near_the_target_tempo(any_timeline):
    grid = bars(any_timeline)
    starts = [bar.start for bar in grid]
    for b in any_timeline["beats"]:
        assert any(math.isclose(b["start"], s) for s in starts), f"{b['key']} starts mid-bar"
    for bar in grid:
        assert abs(60.0 / bar.beat_len - BPM) <= 4, f"{bar.beat_key} bar {bar.index} is off tempo"
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


# Night falls in the review Beat and lasts until the paper clears at the end of the Film.
NIGHT = ("review", "servers")


def test_the_pad_plays_throughout_and_is_louder_at_night(stems):
    timeline, parts = stems
    pad = parts["pad"]
    for t in np.arange(1.0, timeline["durationSeconds"] - 1, 1.0):
        assert db(rms(section(pad, t, t + 1))) > -50, f"pad missing at {t:.1f}s"
    for day in (b for b in timeline["beats"] if b["key"] not in NIGHT):
        for key in NIGHT:
            night = beat(timeline, key)
            # Skip each Beat's first bar, where the swell from the Beat before still overlaps.
            quiet = db(rms(section(pad, day["start"] + 1.5, day["end"])))
            loud = db(rms(section(pad, night["start"] + 1.5, night["end"])))
            assert loud > quiet + 3, f"the pad in {key} is not louder than in {day['key']}"


def test_the_opening_is_sparser_than_the_feature_beats(stems):
    timeline, parts = stems
    bed = sum(parts.values())
    opening, speak = beat(timeline, "opening"), beat(timeline, "speak")
    assert db(rms(section(bed, 1, opening["end"]))) < db(rms(section(bed, speak["start"], speak["end"]))) - 2


PITCH_CLASSES = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")


def power_spectrum(x: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """The frequencies (Hz) and power of a stereo signal's Hann-windowed mono sum."""
    mono = x.mean(axis=1) * np.hanning(x.shape[0])
    return np.fft.rfftfreq(mono.shape[0], 1.0 / SR), np.abs(np.fft.rfft(mono)) ** 2


def pitch_class_levels(x: np.ndarray) -> dict[str, float]:
    """Spectral energy (dB) per pitch class, from about C2 to C7."""
    freqs, power = power_spectrum(x)
    keep = (freqs > 60) & (freqs < 2100)
    classes = np.round(12 * np.log2(freqs[keep] / 440.0) + 69).astype(int) % 12
    energy = np.bincount(classes, weights=power[keep], minlength=12)
    return {name: 10 * np.log10(e + 1e-30) for name, e in zip(PITCH_CLASSES, energy)}


def test_the_score_resolves_on_cadd9(stems):
    """The pitched parts end on C, D, E and G (Cadd9: no seventh), ringing through the fade."""
    timeline, parts = stems
    end = timeline["durationSeconds"]
    last = bars(timeline)[-1]
    assert end - last.start >= music.FADE_OUT - 0.5, "the home chord rings through the fade"
    window = (end - 3.5, end - 1.0)
    levels = pitch_class_levels(section(parts[CHORDS] + parts[MELODY], *window))
    home = [levels[name] for name in ("C", "D", "E", "G")]
    assert min(home) > max(levels.values()) - 20, f"a Cadd9 tone is missing: {levels}"
    for name, level in levels.items():
        if name not in ("C", "D", "E", "G"):
            assert level < min(home) - 6, f"{name} sounds in the home chord"
    bass = pitch_class_levels(section(parts["bass"], *window))
    assert max(bass, key=bass.get) == "C", "the bass lands on the root"


def test_music_ignores_sound_cues(timeline, bank):
    """The score and the sound effects are independent layers: adding cues never changes the music."""
    without_cues = reshaped(timeline, {b["key"]: (b["start"], b["end"]) for b in timeline["beats"]})
    assert np.array_equal(music.render(without_cues, SR, bank), music.render(timeline, SR, bank))


def test_the_chords_and_melody_play_through_the_sample_bank(timeline, bank):
    """Every chord and melody note comes from the bank: a silent bank silences those stems and nothing else."""
    heard = {role: [] for role in ROLES}

    def recording(role):
        def render(note: Note, sr: int, rng: np.random.Generator) -> np.ndarray:
            heard[role].append(note)
            return np.zeros(seconds(note.duration, sr))

        return render

    parts = music.stems(timeline, SR, {role: recording(role) for role in ROLES})
    for role in ROLES:
        assert heard[role], f"no {role} notes reached the bank"
        assert np.max(np.abs(parts[role])) == 0, f"{role} sounds without the bank"
        assert any(note.let_ring for note in heard[role]), f"{role} has no closing note left to ring"
    assert db(rms(parts["bass"])) > -60, "the synthesised stems still play"


def test_a_bank_must_cover_every_role(timeline, bank):
    with pytest.raises(ValueError, match=MELODY):
        render_layers(timeline, SR, {CHORDS: bank[CHORDS]})


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


def high_share(x: np.ndarray, above: float) -> float:
    """How much of the signal's energy lies above `above` Hz, in dB."""
    freqs, power = power_spectrum(x)
    return 10 * np.log10(power[freqs > above].sum() / power.sum())


def test_only_the_music_gets_the_warm_low_pass(layers):
    """The music bus is darkened like worn tape; the sound effects stay dry and keep their bright edges."""
    assert high_share(layers["music"], 8000) < -38
    assert high_share(layers["sfx"], 8000) > -20


def test_every_cue_stands_out_over_the_music(timeline, layers):
    for cue in all_cues(timeline):
        at = round(cue["at"] * SR)
        cue_peak = np.max(np.abs(layers["sfx"][at : at + SR // 5]))
        bed = rms(layers["music"][max(at - SR // 2, 0) : at + SR // 2])
        assert db(cue_peak) - db(bed) > 10, f"{cue['type']} at {cue['at']}s is buried"
