"""The sample bank: how the score's melodic parts turn a note into sound.

`layers/music.py` decides what each part plays and when; a bank decides what
that note sounds like. Each part has a role, not an instrument name, so
changing an instrument means changing the bank, not the score:

    CHORDS   the accompaniment (arpeggios and the closing chord)
    MELODY   the tune above it

A bank maps every role to a `NoteRenderer`, which turns one `Note` into a mono
clip. `sampled_bank()` plays the recorded instruments in the sample cache
(`samples.py`, `sfz.py`); the build uses it whenever `render_layers` is given
no bank. Tests pass their own bank to `render_layers`, so the audio suite never
needs the cache or the network.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass, replace
from typing import Protocol

import numpy as np

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


def softly(play: NoteRenderer, touch: float) -> NoteRenderer:
    """`play`, always struck with a velocity of at most `touch` but scaled back to the note's own level.

    A sampled instrument picks its velocity layer from the note's velocity, so this keeps a
    piano on its soft, dark layer (a felt-like touch) while the score still sets how loud each note is.
    """

    def render(note: Note, sr: int, rng: np.random.Generator) -> np.ndarray:
        struck = min(note.velocity, touch)
        if struck <= 0:
            return play(note, sr, rng)
        return note.velocity / struck * play(replace(note, velocity=struck), sr, rng)

    return render


# The upright piano's soft layer covers velocities up to 80 of 127.
PIANO_TOUCH = 0.6


def sampled_bank() -> SampleBank:
    """The recorded instruments in `samples.toml`, read from the sample cache: each library plays its role
    (upright piano for the chords, played softly for a felt-like tone; Rhodes for the melody).

    Only verifies the cache; the build's fetch step downloads it.
    """
    from ..samples import load_manifest
    from .sfz import load_library

    libraries = load_manifest()
    seen: set[str] = set()
    for library in libraries:  # checked before any archive is read
        if library.role not in ROLES:
            raise ValueError(f"sample library '{library.id}' has role '{library.role}', not one of {', '.join(ROLES)}")
        if library.role in seen:
            raise ValueError(f"sample library '{library.id}' plays '{library.role}', which another library already plays")
        seen.add(library.role)
    bank = {library.role: load_library(library) for library in libraries}
    if CHORDS in bank:
        bank[CHORDS] = softly(bank[CHORDS], PIANO_TOUCH)
    return check_bank(bank)
