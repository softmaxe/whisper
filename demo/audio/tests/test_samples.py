"""The sample cache and the SFZ player, on a tiny library built here: no network, no real cache."""

from __future__ import annotations

import dataclasses
import hashlib
import io
import tarfile
from pathlib import Path

import numpy as np
import pytest
import soundfile

from whisper_audio.dsp import midi_hz
from whisper_audio.music.bank import Note, softly
from whisper_audio.music.sfz import load_instrument, load_library
from whisper_audio.samples import Library, SampleCacheError, fetch, verified_archive

SR = 48_000
SFZ = """// a two-sample test instrument
<control> default_path=samples/
<global> ampeg_release=0.2
<group> hivel=127
<region> sample=a3.wav lokey=c3 hikey=b3 pitch_keycenter=a3
<region> sample=a4.wav lokey=c4 hikey=b4 pitch_keycenter=69
"""


def wav_bytes(key: int, sr: int = 44_100) -> bytes:
    t = np.arange(2 * sr) / sr
    buf = io.BytesIO()
    soundfile.write(buf, 0.5 * np.sin(2 * np.pi * midi_hz(key) * t), sr, format="WAV")
    return buf.getvalue()


@pytest.fixture
def source(tmp_path) -> Path:
    """A tarball holding the test instrument, standing in for a library's download."""
    path = tmp_path / "source" / "test-lib-1.0.tar.gz"
    path.parent.mkdir()
    files = {"test-lib/test.sfz": SFZ.encode(), "test-lib/samples/a3.wav": wav_bytes(57), "test-lib/samples/a4.wav": wav_bytes(69)}
    with tarfile.open(path, "w:gz") as archive:
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    return path


@pytest.fixture
def library(source) -> Library:
    return Library(
        id="test-lib",
        name="Test library",
        role="melody",
        version="1.0",
        url=source.as_uri(),
        sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        sfz="test-lib/test.sfz",
    )


def test_fetch_downloads_and_verifies_then_reuses_the_cache(tmp_path, library, source):
    cache = tmp_path / "cache"
    fetch([library], cache, log=lambda _: None)
    assert verified_archive(library, cache).read_bytes() == source.read_bytes()

    source.unlink()  # a primed cache needs no download
    fetch([library], cache, log=lambda _: None)
    assert load_library(library, cache) is not None


def test_a_corrupted_cached_archive_fails_naming_the_library(tmp_path, library):
    cache = tmp_path / "cache"
    fetch([library], cache, log=lambda _: None)
    with open(library.archive(cache), "r+b") as f:
        f.seek(100)
        f.write(b"corrupt")
    for check in (lambda: fetch([library], cache, log=lambda _: None), lambda: load_library(library, cache)):
        with pytest.raises(SampleCacheError, match="'test-lib'.*does not match the manifest"):
            check()


def test_a_mismatched_or_missing_download_fails_naming_the_library(tmp_path, library, source):
    cache = tmp_path / "cache"
    wrong = dataclasses.replace(library, sha256="0" * 64)
    with pytest.raises(SampleCacheError, match="'test-lib'.*does not match the manifest"):
        fetch([wrong], cache, log=lambda _: None)
    assert not wrong.archive(cache).exists()

    missing = dataclasses.replace(library, url=(source.parent / "gone.tar.gz").as_uri())
    with pytest.raises(SampleCacheError, match="'test-lib'.*download .* failed"):
        fetch([missing], cache, log=lambda _: None)
    with pytest.raises(SampleCacheError, match="'test-lib'.*not in the sample cache"):
        load_library(library, cache)


def dominant_hz(x: np.ndarray, sr: int) -> float:
    spectrum = np.abs(np.fft.rfft(x * np.hanning(x.size)))
    return float(np.fft.rfftfreq(x.size, 1 / sr)[np.argmax(spectrum)])


@pytest.mark.parametrize("pitch", [55, 60, 64, 72])
def test_a_note_plays_the_nearest_sample_at_its_pitch_and_length(tmp_path, library, pitch):
    cache = tmp_path / "cache"
    fetch([library], cache, log=lambda _: None)
    instrument = load_library(library, cache)
    clip = instrument(Note(pitch, 0.6, 1.0), SR, np.random.default_rng(0))

    assert clip.shape == (SR,)
    assert np.max(np.abs(clip)) == pytest.approx(0.6, rel=0.01)
    assert dominant_hz(clip[: SR // 2], SR) == pytest.approx(midi_hz(pitch), rel=0.01)
    assert np.max(np.abs(clip[-10:])) < 1e-3  # released to silence at the note's end


# Every region maps A4 to a sample of a different pitch, so the pitch heard tells which region a note
# struck. Only the plain attack region that comes first in its round robin should ever play.
ROUND_ROBIN_SFZ = """
<region> sample=release.wav key=69 trigger=release
<region> sample=legato.wav key=69 trigger=legato
<region> sample=second.wav key=69 seq_length=2 seq_position=2
<region> sample=upper.wav key=69 lorand=0.5 hirand=1
<region> sample=attack.wav key=69 seq_length=2 seq_position=1 lorand=0 hirand=0.5
"""


def test_a_note_strikes_its_attack_region_and_always_the_first_round_robin():
    keys = {"release.wav": 81, "legato.wav": 76, "second.wav": 72, "upper.wav": 64, "attack.wav": 57}
    files = {"inst.sfz": ROUND_ROBIN_SFZ.encode(), **{name: wav_bytes(key) for name, key in keys.items()}}
    instrument = load_instrument(files, "inst.sfz")

    clips = [instrument(Note(69, 0.6, 1.0), SR, np.random.default_rng(seed)) for seed in range(3)]
    for clip in clips:
        assert dominant_hz(clip[: SR // 2], SR) == pytest.approx(midi_hz(57), rel=0.01)
        assert np.array_equal(clip, clips[0])  # the same take every time


def test_a_soft_touch_strikes_softly_but_keeps_the_notes_level():
    struck = []

    def play(note: Note, sr: int, rng: np.random.Generator) -> np.ndarray:
        struck.append(note.velocity)
        return note.velocity * np.ones(sr // 10)

    soft = softly(play, 0.6)
    rng = np.random.default_rng(0)
    assert np.allclose(soft(Note(60, 0.9, 0.1), SR, rng), 0.9)
    assert np.allclose(soft(Note(60, 0.4, 0.1), SR, rng), 0.4)
    assert struck == [0.6, 0.4]
