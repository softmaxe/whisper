"""CLI: python -m whisper_audio <timeline.json> <out.wav>"""

from __future__ import annotations

import argparse
from pathlib import Path

from . import SAMPLE_RATE
from .mix import render_mix, write_wav
from .timeline import load_timeline


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("timeline", type=Path)
    parser.add_argument("out", type=Path)
    args = parser.parse_args()

    timeline = load_timeline(args.timeline)
    mix = render_mix(timeline, SAMPLE_RATE)
    write_wav(args.out, mix, SAMPLE_RATE)
    print(f"wrote {args.out} ({mix.shape[0] / SAMPLE_RATE:.2f}s, {mix.shape[1]} ch)")


if __name__ == "__main__":
    main()
