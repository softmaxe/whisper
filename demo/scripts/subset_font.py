# /// script
# requires-python = ">=3.11"
# dependencies = ["fonttools>=4.50", "brotli>=1.1"]
# ///
"""Regenerate the bundled LXGW WenKai web-font subset.

The upstream TTF is ~25 MB, so the repository ships a WOFF2 subset covering
ASCII, common CJK punctuation and the full GB2312 hanzi set (6763 characters).
That keeps Caption edits working without re-running this script in the common
case. The subset is used solely as a web font inside the headless renderer,
which the LXGW WenKai OFL additional permission allows under the original name.

Usage:
    uv run scripts/subset_font.py path/to/LXGWWenKai-Regular.ttf

Download the source TTF from https://github.com/lxgw/LxgwWenKai/releases
(e.g. `gh release download -R lxgw/LxgwWenKai -p LXGWWenKai-Regular.ttf`).
"""

from __future__ import annotations

import sys
from pathlib import Path

from fontTools import subset

OUT = Path(__file__).resolve().parent.parent / "public/fonts/LXGWWenKai-Regular.subset.woff2"
# Every character the subset can draw; tests check Captions against it.
CHARSET = OUT.with_name("LXGWWenKai-Regular.subset.charset.txt")


def gb2312_chars() -> str:
    chars = []
    for hi in range(0xB0, 0xF8):
        for lo in range(0xA1, 0xFF):
            try:
                chars.append(bytes([hi, lo]).decode("gb2312"))
            except UnicodeDecodeError:
                pass
    return "".join(chars)


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    src = Path(sys.argv[1])
    ascii_chars = "".join(chr(c) for c in range(0x20, 0x7F))
    punctuation = "，。、；：？！“”‘’（）《》〈〉【】「」『』—…·～　％＋－＝／"
    text = ascii_chars + punctuation + gb2312_chars()

    options = subset.Options()
    options.flavor = "woff2"
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.name_languages = ["*"]
    options.notdef_outline = True

    font = subset.load_font(str(src), options)
    subsetter = subset.Subsetter(options)
    subsetter.populate(text=text)
    subsetter.subset(font)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    subset.save_font(font, str(OUT), options)
    covered = sorted(chr(cp) for cp in font.getBestCmap())
    CHARSET.write_text("".join(covered) + "\n", encoding="utf-8")
    print(f"wrote {OUT} ({OUT.stat().st_size / 1e6:.2f} MB, {len(set(text))} chars requested)")


if __name__ == "__main__":
    main()
