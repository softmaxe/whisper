"""Read-only view of the shared timeline JSON."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

Timeline = dict[str, Any]


def load_timeline(path: Path) -> Timeline:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def all_cues(timeline: Timeline) -> list[dict[str, Any]]:
    cues = [cue for beat in timeline["beats"] for cue in beat["cues"]]
    return sorted(cues, key=lambda c: c["at"])


def beat(timeline: Timeline, key: str) -> dict[str, Any]:
    for b in timeline["beats"]:
        if b["key"] == key:
            return b
    raise KeyError(key)


def film_samples(timeline: Timeline, sr: int) -> int:
    """Length of the Film in samples; every layer renders exactly this many."""
    return int(round(timeline["durationSeconds"] * sr))
