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

# Test Strokes (ticket 61): one regular-only Han family whose eight glyphs are
# built of separate stroke contours, as a Song face's are. Every contour runs
# clockwise (TrueType's filled direction). Four of them (口 日 回 中) close each
# stroke on the edge that faces away from the glyph's centre and list the
# strokes counter-clockwise around it, so the points of all contours read as
# ONE polygon have a positive (counter-clockwise) area although every contour's
# own area is negative: the case in which swash 0.2.9's one-polygon winding
# test answers the wrong side. The other four (一 二 三 十) do not.
STROKE = 70


def stroke(pen, x0, y0, x1, y1, closing_edge):
    """A clockwise rectangle whose closing (last -> first) edge is the named one."""
    corners = [(x0, y0), (x0, y1), (x1, y1), (x1, y0)]
    start = {"left": 1, "top": 2, "right": 3, "bottom": 0}[closing_edge]
    corners = corners[start:] + corners[:start]
    pen.moveTo(corners[0])
    for corner in corners[1:]:
        pen.lineTo(corner)
    pen.closePath()


def frame(x0, y0, x1, y1):
    """Four strokes, bottom, right, top, left: counter-clockwise around the centre."""
    t = STROKE
    return [
        (x0, y0, x1, y0 + t, "bottom"),
        (x1 - t, y0, x1, y1, "right"),
        (x0, y1 - t, x1, y1, "top"),
        (x0, y0, x0 + t, y1, "left"),
    ]


t = STROKE
STROKE_GLYPHS = {
    0x4E00: [(100, 320, 800, 320 + t, "bottom")],
    0x4E8C: [(150, 560, 750, 560 + t, "bottom"), (100, 120, 800, 120 + t, "bottom")],
    0x4E09: [
        (150, 600, 750, 600 + t, "bottom"),
        (200, 340, 700, 340 + t, "bottom"),
        (100, 80, 800, 80 + t, "bottom"),
    ],
    0x5341: [(100, 320, 800, 320 + t, "bottom"), (415, 20, 415 + t, 700, "bottom")],
    0x53E3: frame(100, 40, 800, 700),
    0x65E5: frame(150, 20, 750, 720) + [(150 + t, 335, 750 - t, 335 + t, "bottom")],
    0x56DE: frame(100, 20, 800, 720) + frame(300, 220, 600, 520),
    0x4E2D: frame(150, 200, 750, 560) + [(415, 0, 415 + t, 760, "bottom")],
}

builder = FontBuilder(1000, isTTF=True)
names = [".notdef", "space"] + ["uni%04X" % code for code in STROKE_GLYPHS]
builder.setupGlyphOrder(names)
builder.setupCharacterMap(
    {32: "space", **{code: "uni%04X" % code for code in STROKE_GLYPHS}}
)
glyphs = {}
for name in names:
    pen = TTGlyphPen(None)
    if name == ".notdef":
        stroke(pen, 100, 0, 800, 700, "bottom")
    elif name != "space":
        for rect in STROKE_GLYPHS[int(name[3:], 16)]:
            stroke(pen, *rect)
    glyphs[name] = pen.glyph()
builder.setupGlyf(glyphs)
builder.setupHorizontalMetrics({name: (1000, 0) for name in glyphs})
builder.setupHorizontalHeader(ascent=800, descent=-200)
builder.setupNameTable({
    "familyName": "Test Strokes",
    "styleName": "Regular",
    "uniqueFontIdentifier": "Test Strokes400",
    "fullName": "Test Strokes400",
    "psName": "TestStrokes400",
})
builder.setupOS2(
    sTypoAscender=800,
    sTypoDescender=-200,
    usWeightClass=400,
    ulCodePageRange1=1 << 18,
    fsSelection=64,
)
builder.setupPost()
builder.setupMaxp()
builder.font["head"].macStyle = 0
builder.font.recalcTimestamp = False
builder.font["head"].created = builder.font["head"].modified = 2082844800
builder.save(OUTPUT / "Test-Strokes400.ttf")
