"""Synthesised audio for the Whisper demo Film.

The synthesiser reads the shared timeline (exported to JSON by
`scripts/export-timeline.ts`): every Beat window, sound cue and the Film's
length come from there, so retiming the Film re-times the audio. Only sound
design durations are fixed here, such as note and effect lengths and the
music's closing fade (`layers/music.py` FADE_OUT), which is measured back from
the Film's end. The chords and melody play recorded instruments from the
sample cache (`samples.py`, listed in `samples.toml`); every other sound is
generated with numpy.
"""

SAMPLE_RATE = 48_000
