"""The sound-effect layer: every cue's sound depends only on the cue itself."""

from __future__ import annotations

import numpy as np

from whisper_audio.layers import sfx

SR = 48_000


def timeline(cues: list[dict]) -> dict:
    return {"durationSeconds": 2, "beats": [{"key": "b", "cues": cues}]}


def test_cue_seed_depends_on_type_and_time_only():
    click = {"type": "key_click", "at": 1.0}
    assert sfx.cue_seed(click) == sfx.cue_seed({"type": "key_click", "at": 1.0, "params": {"gain": 0.3}})
    assert sfx.cue_seed(click) != sfx.cue_seed({"type": "key_click", "at": 1.5})
    assert sfx.cue_seed(click) != sfx.cue_seed({"type": "tick", "at": 1.0})


def test_adding_an_earlier_cue_does_not_change_later_cues():
    later = {"type": "key_click", "at": 1.0}
    alone = sfx.render(timeline([later]), SR)
    with_earlier = sfx.render(timeline([{"type": "key_click", "at": 0.1}, later]), SR)
    tail = slice(int(0.9 * SR), None)
    assert np.array_equal(alone[tail], with_earlier[tail])
