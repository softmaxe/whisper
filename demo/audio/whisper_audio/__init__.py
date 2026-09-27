"""Synthesised audio for the Whisper demo Film.

The synthesiser reads the shared timeline (exported to JSON by
`scripts/export-timeline.ts`) and never hard-codes a time of its own. No
samples are downloaded: every sound is generated with numpy.
"""

SAMPLE_RATE = 48_000
