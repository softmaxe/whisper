# /// script
# requires-python = ">=3.11"
# dependencies = ["fonttools>=4.50"]
# ///
"""Cut static Anthropic Sans / Serif faces from the variable TTFs.

GPUI picks faces by family + weight and does not drive variation axes, so the
variable fonts are pinned to static instances at the Text optical size
(opsz 16) and renamed to the plain "Anthropic Sans" / "Anthropic Serif"
families.

The Anthropic faces are proprietary. They may only live in this repository
while it is private, and builds that embed them are for personal use.

    uv run scripts/instance-anthropic-fonts.py [variable-ttf-dir]
"""

import sys
from pathlib import Path

from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "assets/fonts"
DEFAULT_SRC = Path.home() / "development/scratch/claude-app-rea/assets/fonts/ttf"

# (source file, family, style, wght, italic)
FACES = [
    ("AnthropicSans-Romans-Variable-25x258.ttf", "Anthropic Sans", "Regular", 400, False),
    ("AnthropicSans-Romans-Variable-25x258.ttf", "Anthropic Sans", "Medium", 500, False),
    ("AnthropicSans-Romans-Variable-25x258.ttf", "Anthropic Sans", "SemiBold", 600, False),
    ("AnthropicSans-Italics-Variable-25x258.ttf", "Anthropic Sans", "Italic", 400, True),
    ("AnthropicSerif-Romans-Variable-25x258.ttf", "Anthropic Serif", "Regular", 400, False),
    ("AnthropicSerif-Romans-Variable-25x258.ttf", "Anthropic Serif", "Medium", 500, False),
    ("AnthropicSerif-Italics-Variable-25x258.ttf", "Anthropic Serif", "Italic", 402, True),
]


def rename(font: TTFont, family: str, style: str, weight: int, italic: bool) -> None:
    ribbi = style in ("Regular", "Italic")
    legacy_family = family if ribbi else f"{family} {style}"
    legacy_style = style if ribbi else "Regular"
    full = family if style == "Regular" else f"{family} {style}"
    ps = f"{family.replace(' ', '')}-{style}"
    name = font["name"]
    name.names = [n for n in name.names if n.nameID not in (1, 2, 3, 4, 6, 16, 17, 21, 22, 25)]
    for nid, value in (
        (1, legacy_family),
        (2, legacy_style),
        (3, f"{ps};instanced"),
        (4, full),
        (6, ps),
        (16, family),
        (17, style),
    ):
        name.setName(value, nid, 3, 1, 0x409)
        name.setName(value, nid, 1, 0, 0)
    os2 = font["OS/2"]
    os2.usWeightClass = round(weight, -2)
    # fsSelection bits: 0 italic, 5 bold, 6 regular. macStyle bit 1 is italic.
    os2.fsSelection &= ~0b1100001
    if italic:
        os2.fsSelection |= 1
    elif style == "Regular":
        os2.fsSelection |= 1 << 6
    font["head"].macStyle = 0b10 if italic else 0
    if "STAT" in font:
        del font["STAT"]


def main() -> None:
    src = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_SRC
    for file, family, style, wght, italic in FACES:
        font = TTFont(src / file)
        static = instancer.instantiateVariableFont(font, {"wght": wght, "opsz": 16})
        rename(static, family, style, wght, italic)
        out = OUT / f"{family.replace(' ', '')}-{style}.ttf"
        static.save(out)
        print(f"{out.relative_to(REPO)}  {out.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
