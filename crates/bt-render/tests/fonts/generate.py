"""Recreate the rectangle-only fonts used by cjk_picker_regressions.

Requires fontTools only when regenerating; Rust tests use the checked-in bytes.
"""
from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

OUTPUT = Path(__file__).parent
FACES = [
    ("Test Escape", 700, True),
    ("Test CJK", 400, True),
    ("Test CJK", 700, True),
    ("Test Other", 400, True),
    ("Test Sans", 400, False),
]

for family, weight, han in FACES:
    builder = FontBuilder(1000, isTTF=True)
    builder.setupGlyphOrder([".notdef", "space", "latin", "han"])
    charmap = {32: "space", 65: "latin"}
    if han:
        charmap.update({0x4F60: "han", 0x8FD9: "han"})
    builder.setupCharacterMap(charmap)
    glyphs = {}
    for name in [".notdef", "space", "latin", "han"]:
        pen = TTGlyphPen(None)
        if name != "space":
            pen.moveTo((100, 0))
            pen.lineTo((800, 0))
            pen.lineTo((800, 700))
            pen.lineTo((100, 700))
            pen.closePath()
        glyphs[name] = pen.glyph()
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics({name: (1000, 0) for name in glyphs})
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupNameTable({
        "familyName": family,
        "styleName": "Bold" if weight == 700 else "Regular",
        "uniqueFontIdentifier": family + str(weight),
        "fullName": family + str(weight),
        "psName": family.replace(" ", "") + str(weight),
    })
    builder.setupOS2(
        sTypoAscender=800,
        sTypoDescender=-200,
        usWeightClass=weight,
        ulCodePageRange1=(1 << 18) if han else 1,
        fsSelection=32 if weight == 700 else 64,
    )
    builder.setupPost()
    builder.setupMaxp()
    builder.font["head"].macStyle = 1 if weight == 700 else 0
    builder.font.recalcTimestamp = False
    builder.font["head"].created = builder.font["head"].modified = 2082844800
    builder.save(OUTPUT / (family.replace(" ", "-") + str(weight) + ".ttf"))
