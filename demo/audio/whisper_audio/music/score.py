"""The score on paper: key, chords, per-Beat arrangement and the bar grid.

Nothing here makes sound. `layers/music.py` turns the bars into notes.

The grid follows the timeline: every Beat starts on a downbeat. Each Beat is
split into whole beats at a tempo as close to 76 BPM as fits the Beat exactly
(so a 22 s Beat plays 28 beats at about 76.4 BPM), grouped into 4/4 bars. A
leftover of 2 or 3 beats becomes a short closing bar; a single leftover beat
lengthens the last bar instead. The Film's last Beat always folds its leftover
into the final bar, and that bar lasts at least HOME_BEATS beats (borrowing
from the bar before, which becomes a short pickup), so the closing home chord
rings as long as possible.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from ..timeline import Timeline

BPM = 76
BEATS_PER_BAR = 4
HOME_BEATS = 6


@dataclass(frozen=True)
class Chord:
    name: str
    bass: int  # MIDI root for the bass
    voicing: tuple[int, ...]  # accompaniment voicing, low to high (MIDI)

    @property
    def tones(self) -> tuple[int, ...]:
        """Melody notes: the voicing's top four notes, an octave up."""
        return tuple(n + 12 for n in self.voicing[-4:])


# C major in soft maj7/add9 colours: an open low note under a close upper voicing.
CMAJ7 = Chord("Cmaj7", 36, (48, 55, 59, 64, 67))
DM9 = Chord("Dm9", 38, (50, 57, 60, 64, 65))
EM7 = Chord("Em7", 40, (52, 59, 62, 67, 71))
FMAJ7 = Chord("Fmaj7", 41, (53, 57, 60, 64, 67))
G6SUS = Chord("G6sus", 43, (55, 60, 62, 64, 67))
AM9 = Chord("Am9", 45, (45, 55, 59, 60, 64))
# The closing home chord: Cadd9 (no seventh), with the high C on top.
HOME = Chord("Cadd9", 36, (48, 55, 62, 64, 67, 72))

# An accompaniment pattern is a list of (beat position within the bar, voicing index, velocity).
# Positions at or past a bar's length are skipped; a 5-beat bar repeats beat 0's notes on beat 4.
CHORD_PATTERNS: dict[str, list[tuple[float, int, float]]] = {
    # Unhurried quarter-note arpeggio: the Opening's slow typing.
    "sparse": [(0.0, 0, 0.85), (1.0, 2, 0.45), (2.0, 3, 0.55), (3.0, 4, 0.4)],
    # Rolling eighths: the feature Beats.
    "rolling": [
        (0.0, 0, 0.85), (0.5, 2, 0.4), (1.0, 3, 0.55), (1.5, 2, 0.35),
        (2.0, 1, 0.65), (2.5, 3, 0.4), (3.0, 4, 0.55), (3.5, 3, 0.35),
    ],
    # Night: a low note and one high note per bar, left to ring.
    "night": [(0.0, 0, 0.75), (2.0, 3, 0.4)],
}

# Melody figures: (beat position, chord-tone index into Chord.tones, velocity), one list per bar, cycled.
MELODY_FIGURES: dict[str, list[list[tuple[float, int, float]]]] = {
    "question": [
        [(2.0, 1, 0.7), (3.0, 2, 0.6)],
        [(2.0, 3, 0.65), (3.0, 2, 0.55)],
    ],
    "tune": [
        [(1.0, 1, 0.7), (1.5, 2, 0.55), (2.0, 3, 0.65), (3.0, 2, 0.5)],
        [(0.5, 1, 0.55), (1.0, 0, 0.6), (2.0, 1, 0.6)],
        [(1.0, 1, 0.7), (1.5, 2, 0.55), (2.0, 3, 0.65), (3.0, 3, 0.55)],
        [(1.0, 3, 0.6), (2.0, 2, 0.55), (3.0, 1, 0.5)],
    ],
    "answer": [
        [(0.5, 3, 0.55), (2.0, 2, 0.5)],
        [(0.5, 2, 0.55), (2.0, 3, 0.5), (3.0, 1, 0.4)],
    ],
    "lullaby": [
        [(0.0, 2, 0.5), (2.0, 1, 0.4)],
        [(0.0, 3, 0.45), (2.0, 2, 0.4)],
    ],
    "home": [
        [(1.0, 1, 0.55), (2.0, 2, 0.55), (3.0, 3, 0.5)],
        [(1.0, 3, 0.5), (2.0, 2, 0.5)],
    ],
}

# Pad thickness: a thin warm floor by day, a fuller and louder pad at night.
DAY_PAD = 0.4
NIGHT_PAD = 1.0


@dataclass(frozen=True)
class Section:
    """How one Beat is scored. `chords` cycle over the bars; `cadence` chords take the last bars."""

    chords: tuple[Chord, ...]
    cadence: tuple[Chord, ...] = ()
    comp: str = "sparse"  # accompaniment pattern, a key of CHORD_PATTERNS
    melody: str | None = None  # melody figures, a key of MELODY_FIGURES
    bass: bool = False
    drums: bool = False
    pad: float = DAY_PAD
    paper: float = 0.3  # paper rustles per second

    @property
    def night(self) -> bool:
        return self.pad >= NIGHT_PAD


# Arranged by Beat key: sparse opening, drums through the feature Beats, the pad under
# everything and thicker at night (which lasts until the Film's end), and the home chord to close.
SECTIONS: dict[str, Section] = {
    "opening": Section((CMAJ7, FMAJ7), cadence=(G6SUS,), comp="sparse", melody="question", paper=0.8),
    "speak": Section((CMAJ7, AM9, FMAJ7, G6SUS), comp="rolling", melody="tune", bass=True, drums=True),
    "more": Section(
        (FMAJ7, EM7, AM9, DM9), cadence=(G6SUS,), comp="rolling", melody="answer", bass=True, drums=True
    ),
    "review": Section(
        (AM9, FMAJ7, CMAJ7), cadence=(G6SUS,), comp="night", melody="lullaby", bass=True, pad=NIGHT_PAD, paper=0.15
    ),
    "servers": Section(
        (FMAJ7, EM7, AM9), cadence=(G6SUS, HOME), comp="rolling", melody="home", bass=True, pad=NIGHT_PAD
    ),
}
# A Beat added to the timeline without a Section here still gets a quiet bed.
DEFAULT_SECTION = Section((CMAJ7, FMAJ7), comp="sparse", melody="lullaby")


@dataclass(frozen=True)
class Bar:
    beat_key: str
    index: int  # bar number within its Beat
    start: float  # seconds
    beats: int
    beat_len: float  # seconds per beat in this Beat
    chord: Chord
    section: Section = field(repr=False)
    last_in_film: bool = False

    @property
    def end(self) -> float:
        return self.start + self.beats * self.beat_len

    def at(self, position: float) -> float:
        """Film time of a beat position within this bar."""
        return self.start + position * self.beat_len


def section_for(key: str) -> Section:
    return SECTIONS.get(key, DEFAULT_SECTION)


def bar_lengths(total_beats: int, last_in_film: bool) -> list[int]:
    full, rest = divmod(total_beats, BEATS_PER_BAR)
    if full == 0:
        return [total_beats]
    bars = [BEATS_PER_BAR] * full
    if rest == 1 or (rest and last_in_film):
        bars[-1] += rest
    elif rest:
        bars.append(rest)
    if last_in_film and len(bars) > 1 and bars[-1] < HOME_BEATS:
        borrow = min(HOME_BEATS - bars[-1], bars[-2] - 2)
        bars[-2] -= borrow
        bars[-1] += borrow
    return bars


def chords_for(section: Section, count: int) -> list[Chord]:
    cadence = list(section.cadence[-count:]) if section.cadence else []
    body = count - len(cadence)
    return [section.chords[i % len(section.chords)] for i in range(body)] + cadence


def bars(timeline: Timeline) -> list[Bar]:
    """Every bar of the Film in order, downbeats aligned to each Beat's start."""
    out: list[Bar] = []
    beats = timeline["beats"]
    for k, beat in enumerate(beats):
        start, end = float(beat["start"]), float(beat["end"])
        length = end - start
        if length <= 0:
            continue
        total = max(1, round(length * BPM / 60.0))
        beat_len = length / total
        last_beat = k == len(beats) - 1
        lengths = bar_lengths(total, last_beat)
        section = section_for(beat["key"])
        chords = chords_for(section, len(lengths))
        at = start
        for i, (count, chord) in enumerate(zip(lengths, chords)):
            out.append(Bar(beat["key"], i, at, count, beat_len, chord, section, last_beat and i == len(lengths) - 1))
            at += count * beat_len
    return out


def chord_at(timeline: Timeline, t: float) -> Chord:
    """The chord sounding at Film time `t`."""
    grid = bars(timeline)
    for bar in grid:
        if bar.start <= t < bar.end:
            return bar.chord
    return grid[-1].chord
