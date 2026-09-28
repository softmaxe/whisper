"""The sample bank: how the score's melodic parts turn a note into sound.

`layers/music.py` decides what each part plays and when; a bank decides what
that note sounds like. Each part has a role, not an instrument name, so
changing an instrument means changing the bank, not the score:

    CHORDS   the accompaniment (arpeggios and the closing chord)
    MELODY   the tune above it

A bank maps every role to a `NoteRenderer`, which turns one `Note` into a mono
clip. `sampled_bank()` plays the recorded instruments in the sample cache
(`samples.py`, `sfz.py`) and is the build's `default_bank()`; `synth_bank()`
wraps the numpy instruments in `instruments.py`. Tests pass their own bank to
`render_layers`, so the audio suite never needs the cache or the network.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Protocol

import numpy as np

from . import instruments as inst

CHORDS = "chords"
MELODY = "melody"
ROLES = (CHORDS, MELODY)


@dataclass(frozen=True)
class Note:
    pitch: float  # MIDI note number
    velocity: float  # linear gain, about 0..1; renderers scale the clip by it
    duration: float  # seconds; the clip's length, release included
    let_ring: bool = False  # a closing note, left to ring out longer and softer


class NoteRenderer(Protocol):
    def __call__(self, note: Note, sr: int, rng: np.random.Generator) -> np.ndarray:
        """A mono clip of the note, `seconds(note.duration, sr)` samples long, already scaled by velocity.

        Draw any randomness from `rng` so rebuilds are identical.
        """
        ...


SampleBank = Mapping[str, NoteRenderer]


def check_bank(bank: SampleBank) -> SampleBank:
    missing = [role for role in ROLES if role not in bank]
    if missing:
        raise ValueError(f"sample bank has no renderer for {', '.join(missing)}")
    return bank


def plucked_guitar(note: Note, sr: int, rng: np.random.Generator) -> np.ndarray:
    """Karplus-Strong guitar: a soft thumb pluck, or a brighter, longer-ringing strum on a closing chord."""
    if note.let_ring:
        return note.velocity * inst.karplus_strong(note.pitch, note.duration, sr, rng, brightness=0.4, decay=0.998)
    return note.velocity * inst.karplus_strong(note.pitch, note.duration, sr, rng, brightness=0.35)


def kalimba(note: Note, sr: int, rng: np.random.Generator) -> np.ndarray:
    """Kalimba tine; a closing note hangs on 2.5 times longer."""
    return note.velocity * inst.kalimba(note.pitch, note.duration, sr, rng, ring=2.5 if note.let_ring else 1.0)


def synth_bank() -> SampleBank:
    """The synthesised instruments: plucked guitar for the chords, kalimba for the melody."""
    return {CHORDS: plucked_guitar, MELODY: kalimba}


def sampled_bank() -> SampleBank:
    """The recorded instruments in `samples.toml`, read from the sample cache: each library plays its role
    (upright piano for the chords, Rhodes for the melody).

    Only verifies the cache; the build's fetch step downloads it.
    """
    from ..samples import load_manifest
    from .sfz import load_library

    return check_bank({library.role: load_library(library) for library in load_manifest()})


def default_bank() -> SampleBank:
    """The bank the build uses when `render_layers` is given none."""
    return sampled_bank()
