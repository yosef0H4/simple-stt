"""Subset the OFL Source Sans 3 variable font for Settings.

Usage: python settings-font.py SourceSans3.ttf ../public/settings-sans.woff2
FontTools and Brotli are generation-only dependencies.
"""

import sys

from fontTools import subset
from fontTools.ttLib import TTFont


def main(source, destination):
    options = subset.Options()
    options.name_IDs = [0, 1, 2, 4, 6, 13, 14, 16, 17, 25]
    options.layout_features = ["kern", "liga"]
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=(
        *range(0x20, 0x7F), 0xA0, 0xB0, 0xB7, 0x2013, 0x2014,
        *range(0x2018, 0x201E), 0x2022, 0x2026,
    ))

    with TTFont(source) as font:
        subsetter.subset(font)
        renamed = {
            1: "Simple STT Paper Sans",
            4: "Simple STT Paper Sans",
            6: "SimpleSTTPaperSans-Regular",
            16: "Simple STT Paper Sans",
            25: "SimpleSTTPaperSans",
        }
        for record in font["name"].names:
            if record.nameID in renamed:
                record.string = renamed[record.nameID]
        font.flavor = "woff2"
        font.save(destination)


if __name__ == "__main__":
    main(*sys.argv[1:])
