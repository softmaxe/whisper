"""CLI: python -m whisper_audio <timeline.json> <out.wav>"""

from __future__ import annotations

import argparse
from pathlib import Path

from . import SAMPLE_RATE
from .mix import render_mix, write_wav
from .music.bank import SampleBank
from .timeline import load_timeline


def main(argv: list[str] | None = None, bank: SampleBank | None = None) -> None:
    """`bank` overrides the build's default sample bank (tests pass a stand-in)."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("timeline", type=Path)
    parser.add_argument("out", type=Path)
    args = parser.parse_args(argv)

    timeline = load_timeline(args.timeline)
    mix = render_mix(timeline, SAMPLE_RATE, bank)
    write_wav(args.out, mix, SAMPLE_RATE)
    print(f"wrote {args.out} ({mix.shape[0] / SAMPLE_RATE:.2f}s, {mix.shape[1]} ch)")


if __name__ == "__main__":
    main()
