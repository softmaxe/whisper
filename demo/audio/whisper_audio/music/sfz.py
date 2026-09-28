"""A minimal SFZ player: just enough of the format to play a multisampled instrument note by note.

It reads each region's `sample`, `pitch_keycenter`, key range (`lokey`/`hikey`
or `key`), velocity range (`lovel`/`hivel`) and `ampeg_release`, with
`<global>`/`<master>`/`<group>` inheritance and `default_path`. Everything
else (loops, crossfades, filters, LFOs, controllers) is ignored.

Only regions struck on a note-on play: those with a `trigger` other than
`attack` or `first` (release noises, legato transitions) are dropped. Round
robins are played deterministically: only the first of a `seq_position`
cycle and the `lorand`/`hirand` region that starts at 0 are kept.

A note plays the region whose key and velocity ranges hold it (or, outside
every range, the region with the nearest key centre at that velocity),
repitched by resampling, cut to the note's duration with a release fade, and
scaled by the note's velocity. Each sample is normalised to a peak of 1, so a
velocity layer changes the timbre while the velocity alone sets the level.
"""

from __future__ import annotations

import io
import posixpath
import re
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

import numpy as np
import soundfile

from ..dsp import normalise, seconds
from ..samples import CACHE_DIR, Library, read_members
from .bank import Note

_NOTE_NAMES = {"c": 0, "d": 2, "e": 4, "f": 5, "g": 7, "a": 9, "b": 11}
_TOKEN = re.compile(r"<(\w+)>|([A-Za-z_]\w*)=")
_LEVELS = ("global", "master", "group", "region")
# Triggers that sound on a plain note-on; `release`, `release_key` and `legato` regions never do here.
_NOTE_ON_TRIGGERS = ("attack", "first")


def midi_key(value: str) -> int:
    """An SFZ key: a MIDI number, or a note name such as `c4` (60), `F#1` or `Bb3`."""
    value = value.strip()
    if re.fullmatch(r"-?\d+", value):
        return int(value)
    match = re.fullmatch(r"([a-gA-G])([#b]?)(-?\d+)", value)
    if not match:
        raise ValueError(f"not an SFZ key: {value!r}")
    letter, accidental, octave = match.groups()
    shift = {"#": 1, "b": -1, "": 0}[accidental]
    return 12 * (int(octave) + 1) + _NOTE_NAMES[letter.lower()] + shift


def parse_regions(text: str) -> list[dict[str, str]]:
    """Every `<region>`'s opcodes, merged with those it inherits from its global, master and group."""
    text = re.sub(r"//[^\n]*", "", text)
    scopes: dict[str, dict[str, str]] = {level: {} for level in _LEVELS}
    control: dict[str, str] = {}
    regions: list[dict[str, str]] = []
    current: dict[str, str] | None = None

    def close_region():
        if current is not None and scopes["region"] is current:
            merged = {**control, **scopes["global"], **scopes["master"], **scopes["group"], **current}
            regions.append(merged)

    tokens = list(_TOKEN.finditer(text))
    for i, token in enumerate(tokens):
        header, opcode = token.groups()
        if header:
            close_region()
            if header in _LEVELS:
                # A header starts a fresh scope at its level and clears every level below it.
                for level in _LEVELS[_LEVELS.index(header) :]:
                    scopes[level] = {}
                current = scopes[header]
            elif header == "control":
                current = control
            else:  # <curve>, <effect>, ...: not used here
                current = {}
            continue
        end = tokens[i + 1].start() if i + 1 < len(tokens) else len(text)
        if current is not None:
            current[opcode] = text[token.end() : end].strip()
    close_region()
    return regions


@dataclass(frozen=True)
class Region:
    sample: str  # path inside the library, relative to the archive root
    keycenter: int
    lokey: int
    hikey: int
    lovel: int
    hivel: int
    release: float  # seconds


def plays_on_note_on(op: Mapping[str, str]) -> bool:
    """Whether the region sounds on a note-on here: an attack region, and the first of any round robin."""
    return (
        op.get("trigger", "attack") in _NOTE_ON_TRIGGERS
        and int(op.get("seq_position", "1")) == 1
        and float(op.get("lorand", "0")) <= 0.0
    )


def regions_from_sfz(text: str, sfz_path: str) -> list[Region]:
    """The instrument's note-on regions, with sample paths resolved against the SFZ file's folder."""
    base = posixpath.dirname(sfz_path)
    out = []
    for op in parse_regions(text):
        if "sample" not in op or not plays_on_note_on(op):
            continue
        sample = posixpath.normpath(posixpath.join(base, op.get("default_path", ""), op["sample"].replace("\\", "/")))
        key = op.get("key")
        lokey = midi_key(op.get("lokey", key or "0"))
        hikey = midi_key(op.get("hikey", key or "127"))
        keycenter = midi_key(op.get("pitch_keycenter", key or "60"))
        out.append(
            Region(
                sample=sample,
                keycenter=keycenter,
                lokey=lokey,
                hikey=hikey,
                lovel=int(op.get("lovel", "1")),
                hivel=int(op.get("hivel", "127")),
                release=float(op.get("ampeg_release", "0.05")),
            )
        )
    if not out:
        raise ValueError(f"{sfz_path} has no regions with samples")
    return out


def decode(data: bytes) -> tuple[np.ndarray, int]:
    """A mono float32 clip (channels averaged) normalised to a peak of 1, and its sample rate."""
    audio, sr = soundfile.read(io.BytesIO(data), dtype="float32", always_2d=True)
    return normalise(audio.mean(axis=1)).astype(np.float32), sr


class SfzInstrument:
    """A `NoteRenderer` (see `bank.py`) that plays an SFZ instrument's samples."""

    def __init__(self, regions: list[Region], samples: Mapping[str, tuple[np.ndarray, int]]):
        self.regions = regions
        self.samples = samples

    def region_for(self, pitch: float, velocity: int) -> Region:
        key = int(round(pitch))
        layer = [r for r in self.regions if r.lovel <= velocity <= r.hivel] or self.regions
        inside = [r for r in layer if r.lokey <= key <= r.hikey]
        return min(inside or layer, key=lambda r: abs(r.keycenter - pitch))

    def __call__(self, note: Note, sr: int, rng: np.random.Generator) -> np.ndarray:
        n = seconds(note.duration, sr)
        velocity = int(np.clip(round(note.velocity * 127), 1, 127))
        region = self.region_for(note.pitch, velocity)
        data, sample_sr = self.samples[region.sample]
        # Source samples advanced per output sample: the pitch shift and the sample-rate change together.
        step = 2.0 ** ((note.pitch - region.keycenter) / 12.0) * sample_sr / sr
        clip = np.interp(np.arange(n) * step, np.arange(data.shape[0]), data, right=0.0)
        # The note is cut to its duration with a release fade; a closing note rings out over a longer one.
        release = min(n // 2, seconds(max(region.release, 0.4 * note.duration) if note.let_ring else region.release, sr))
        if release:
            clip[n - release :] *= 0.5 * (1.0 + np.cos(np.linspace(0.0, np.pi, release)))
        return note.velocity * clip


def load_instrument(files: Mapping[str, bytes], sfz_path: str) -> SfzInstrument:
    """An instrument from its SFZ file and samples, given as file contents keyed by archive path."""
    regions = regions_from_sfz(files[sfz_path].decode("utf-8", errors="replace"), sfz_path)
    samples = {path: decode(files[path]) for path in {r.sample for r in regions}}
    return SfzInstrument(regions, samples)


def load_library(library: Library, cache_dir: Path = CACHE_DIR) -> SfzInstrument:
    """The library's SFZ instrument, read from its verified archive in the sample cache."""
    sfz = read_members(library, [library.sfz], cache_dir)
    regions = regions_from_sfz(sfz[library.sfz].decode("utf-8", errors="replace"), library.sfz)
    files = {**sfz, **read_members(library, {r.sample for r in regions}, cache_dir)}
    try:
        return load_instrument(files, library.sfz)
    except (RuntimeError, ValueError) as err:  # soundfile raises LibsndfileError, a RuntimeError
        raise library.error(f"could not load {library.sfz}: {err}") from err
