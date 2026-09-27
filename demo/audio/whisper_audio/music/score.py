"""The score on paper: key, chords, per-Beat arrangement and the bar grid.

Nothing here makes sound. `layers/music.py` turns the bars into notes.

The grid follows the timeline: every Beat starts on a downbeat. Each Beat is
split into whole beats at a tempo as close to 84 BPM as fits the Beat exactly
(so a 22 s Beat plays 31 beats at about 84.5 BPM), grouped into 4/4 bars. A
leftover of 2 or 3 beats becomes a short closing bar; a single leftover beat
lengthens the last bar instead. The Film's last Beat always folds its leftover
into the final bar, so the closing home chord rings as long as possible.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from ..timeline import Timeline

BPM = 84
BEATS_PER_BAR = 4


@dataclass(frozen=True)
class Chord:
    name: str
    bass: int  # MIDI note of the bass string
    voicing: tuple[int, ...]  # guitar voicing, low to high (MIDI)

    @property
    def tones(self) -> tuple[int, ...]:
        """Melody notes for the kalimba: the voicing's top four notes, an octave up."""
        return tuple(n + 12 for n in self.voicing[-4:])


# D major, with guitar-like open voicings.
D = Chord("D", 38, (50, 57, 62, 66, 69))
G = Chord("G", 43, (55, 59, 62, 67, 71))
GMAJ7 = Chord("Gmaj7", 43, (55, 59, 62, 66, 71))
A = Chord("A", 45, (52, 57, 61, 64, 69))
ASUS4 = Chord("Asus4", 45, (52, 57, 62, 64, 69))
BM = Chord("Bm", 47, (54, 59, 62, 66, 71))
FSM = Chord("F#m", 42, (54, 57, 61, 66, 69))
# The closing home chord: D with the high D on top.
HOME = Chord("D", 38, (50, 57, 62, 66, 69, 74))

# A guitar pattern is a list of (beat position within the bar, voicing index, velocity).
# Positions at or past a bar's length are skipped; a 5-beat bar repeats beat 0's notes on beat 4.
GUITAR_PATTERNS: dict[str, list[tuple[float, int, float]]] = {
    # Unhurried quarter-note arpeggio: the Opening's slow typing.
    "sparse": [(0.0, 0, 0.9), (1.0, 2, 0.5), (2.0, 3, 0.6), (3.0, 4, 0.45)],
    # Rolling eighths: the feature Beats.
    "rolling": [
        (0.0, 0, 0.95), (0.5, 2, 0.45), (1.0, 3, 0.6), (1.5, 2, 0.4),
        (2.0, 1, 0.75), (2.5, 3, 0.45), (3.0, 4, 0.6), (3.5, 3, 0.4),
    ],
    # Night: a bass note and one high note per bar, left to ring.
    "night": [(0.0, 0, 0.8), (2.0, 3, 0.45)],
}

# Kalimba figures: (beat position, chord-tone index into Chord.tones, velocity), one list per bar, cycled.
KALIMBA_FIGURES: dict[str, list[list[tuple[float, int, float]]]] = {
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


@dataclass(frozen=True)
class Section:
    """How one Beat is scored. `chords` cycle over the bars; `cadence` chords take the last bars."""

    chords: tuple[Chord, ...]
    cadence: tuple[Chord, ...] = ()
    guitar: str = "sparse"
    kalimba: str | None = None
    bass: bool = False
    drums: bool = False
    pad: bool = False
    paper: float = 0.3  # paper rustles per second


# Arranged by Beat key: sparse opening, drums through the feature Beats, a soft pad at night,
# and the home chord to close.
SECTIONS: dict[str, Section] = {
    "opening": Section((D, GMAJ7), cadence=(ASUS4,), guitar="sparse", kalimba="question", paper=0.8),
    "speak": Section((D, A, BM, G), guitar="rolling", kalimba="tune", bass=True, drums=True),
    "more": Section((G, A, FSM, BM), cadence=(A,), guitar="rolling", kalimba="answer", bass=True, drums=True),
    "review": Section((BM, G, D), cadence=(A,), guitar="night", kalimba="lullaby", bass=True, pad=True, paper=0.15),
    "servers": Section((G, A, BM), cadence=(ASUS4, HOME), guitar="rolling", kalimba="home", bass=True, pad=True),
}
# A Beat added to the timeline without a Section here still gets a quiet bed.
DEFAULT_SECTION = Section((D, G), guitar="sparse", kalimba="lullaby")


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
