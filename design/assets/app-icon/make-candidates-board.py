#!/usr/bin/env python3
"""Draw the contact sheet the five icon candidates are chosen from.

    python design/assets/app-icon/make-candidates-board.py

Writes `candidates-2026-08-28.png` beside itself: one column per candidate, and
in each column the drawing at 256, 48, 32 and 16 pixels, twice — once on the
grey a light Windows taskbar paints and once on the near-black a dark one does.

Every pixel on the board is a real rasterisation. The small sizes are rendered
from the vector at that size (`make-ico.py`'s `rasterise`, the same call the
`.ico` is built with), never shrunk from the 256; and the magnified strip is a
nearest-neighbour blow-up of those very pixels, so what it shows is the icon's
own 16px grid rather than a smooth restatement of the drawing.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

from PIL import IcoImagePlugin, Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent

#: `make-ico.py` is not importable by name — the hyphen is not an identifier —
#: and it is still the right place for the renderer: the board must be made of
#: the same pixels the icon will be.
_spec = importlib.util.spec_from_file_location("make_ico", HERE / "make-ico.py")
make_ico = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(make_ico)

#: One line each, in the order the README argues them.
CANDIDATES = (
    ("A", "The page and the prompt", "A sheet with its corner turned down, and\none chevron printed on it."),
    ("B", "The page and the integral", "A clean sheet carrying the one glyph no\nother terminal would put on its icon."),
    ("C", "The command mark", "The tile is the pane: the rule that marks\na command block, and its two lines."),
    ("D", "The F and the cursor", "A monogram at prompt weight, with the\nblock cursor standing in its counter."),
    ("E", "The fold is the prompt", "One sheet folded once, seen along the\ncrease — a folio and a chevron at once."),
)

SIZES = (256, 48, 32, 16)
SMALL = (48, 32, 16)
MAGNIFY = 3

#: What Windows paints behind a taskbar icon, light theme and dark.
LIGHT_GROUND = (243, 243, 243)
DARK_GROUND = (32, 32, 32)

#: The hero's own tokens (`assets/readme/hero-light.svg`).
PAPER = (250, 250, 249)
INK = (55, 53, 47)
QUIET = (139, 137, 133)
INK_ON_DARK = (232, 231, 229)
QUIET_ON_DARK = (144, 142, 138)
RULE = (226, 225, 223)

COLUMN = 320
GAP = 24
MARGIN = 40


def font(*names_then_size) -> ImageFont.FreeTypeFont:
    """The first of `names` that this machine has, at the trailing size.

    Windows 11 ships Segoe UI as a variable family and no longer installs the
    semibold face as its own file, so every weight here names its fallback.
    """
    *names, size = names_then_size
    for name in names:
        path = Path(rf"C:\Windows\Fonts\{name}")
        if path.is_file():
            return ImageFont.truetype(str(path), size)
    raise SystemExit(f"none of {names} is installed")


SEMIBOLD = ("segoeuisb.ttf", "segoeuib.ttf")
TITLE = font(*SEMIBOLD, 30)
SUBTITLE = font("segoeui.ttf", 16)
LETTER = font("segoeuib.ttf", 26)
NAME = font(*SEMIBOLD, 17)
BODY = font("segoeui.ttf", 14)
TINY = font("segoeui.ttf", 12)
LABEL = font(*SEMIBOLD, 11)


def card(rendered: dict[int, Image.Image], ground, ink, quiet) -> Image.Image:
    """One background's worth of one candidate: the 256, the true sizes, the 3×."""
    height = 20 + 256 + 22 + 16 + 8 + 48 + 4 + 16 + 20 + 16 + 8 + 48 * MAGNIFY + 20
    out = Image.new("RGBA", (COLUMN, height), (*ground, 255))
    pen = ImageDraw.Draw(out)
    y = 20
    out.alpha_composite(rendered[256], ((COLUMN - 256) // 2, y))
    y += 256 + 22

    pen.text((20, y), "TRUE SIZE", font=LABEL, fill=quiet)
    y += 16 + 8
    # Bottom-aligned on one line, the way a taskbar would line them up.
    x = 26
    for size in SMALL:
        out.alpha_composite(rendered[size], (x, y + 48 - size))
        pen.text((x + size / 2, y + 52), f"{size}", font=TINY, fill=quiet, anchor="ma")
        x += size + 46
    y += 48 + 4 + 16 + 20

    pen.text((20, y), f"{MAGNIFY}× — THE SAME PIXELS", font=LABEL, fill=quiet)
    y += 16 + 8
    x = 10
    for size in SMALL:
        blown = rendered[size].resize((size * MAGNIFY, size * MAGNIFY), Image.NEAREST)
        out.alpha_composite(blown, (x, y + (48 - size) * MAGNIFY))
        x += size * MAGNIFY + 6
    return out


def placeholder(sizes) -> dict[int, Image.Image]:
    """Today's `folio.ico`, entry by entry — not one entry resampled.

    It is on the board because the choice is a replacement, and a replacement is
    judged against what it replaces at the size that is hardest to win.
    """
    # The file stays open until every frame has been read: `IcoFile` seeks in the
    # handle it was given rather than taking a copy of the bytes.
    with open(HERE / "folio.ico", "rb") as handle:
        ico = IcoImagePlugin.IcoFile(handle)
        return {size: ico.getimage((size, size)).convert("RGBA") for size in sizes}


def reference(rendered: dict[int, Image.Image], ground, quiet, width: int) -> Image.Image:
    """The placeholder on one ground: 128 beside its three small entries."""
    out = Image.new("RGBA", (width, 168), (*ground, 255))
    pen = ImageDraw.Draw(out)
    out.alpha_composite(rendered[128], (20, 20))
    x = 190
    for size in SMALL:
        out.alpha_composite(rendered[size], (x, 20 + 128 - size))
        pen.text((x + size / 2, 154), f"{size}", font=TINY, fill=quiet, anchor="ma")
        x += size + 54
    return out


def main() -> None:
    browser = make_ico.find_browser(None)
    columns = []
    for letter, _name, _line in CANDIDATES:
        svg = HERE / "candidates" / f"{letter.lower()}.svg"
        columns.append(make_ico.rasterise(svg, SIZES, browser))

    sample = card(columns[0], LIGHT_GROUND, INK, QUIET)
    body_top = 150
    width = MARGIN * 2 + COLUMN * 5 + GAP * 4
    height = body_top + 96 + sample.height * 2 + GAP + 56 + 168 + MARGIN + 34
    board = Image.new("RGB", (width, height), PAPER)
    pen = ImageDraw.Draw(board)

    pen.text((MARGIN, 44), "Folio — application icon candidates", font=TITLE, fill=INK)
    pen.text(
        (MARGIN, 86),
        "2026-08-28. Five directions, one column each. Every size is rendered from its own vector at that size, "
        "then shown on the grey a light Windows taskbar paints and the near-black a dark one paints.",
        font=SUBTITLE,
        fill=QUIET,
    )
    pen.line((MARGIN, 130, width - MARGIN, 130), fill=RULE, width=1)

    for index, (letter, name, line) in enumerate(CANDIDATES):
        x = MARGIN + index * (COLUMN + GAP)
        y = body_top + 8
        pen.text((x, y), letter, font=LETTER, fill=INK)
        pen.text((x + 34, y + 6), name, font=NAME, fill=INK)
        pen.multiline_text((x, y + 38), line, font=BODY, fill=QUIET, spacing=6)
        y = body_top + 96
        board.paste(card(columns[index], LIGHT_GROUND, INK, QUIET), (x, y))
        y += sample.height + GAP
        board.paste(card(columns[index], DARK_GROUND, INK_ON_DARK, QUIET_ON_DARK), (x, y))

    y = body_top + 96 + sample.height * 2 + GAP + 30
    pen.text(
        (MARGIN, y),
        "Today's placeholder — design/assets/app-icon/folio.ico, the sheet folded once that "
        "release engineering drew so folio.exe,0 had something to point at.",
        font=BODY,
        fill=QUIET,
    )
    y += 30
    strip = (width - MARGIN * 2 - GAP) // 2
    shown = placeholder((128, *SMALL))
    board.paste(reference(shown, LIGHT_GROUND, QUIET, strip), (MARGIN, y))
    board.paste(
        reference(shown, DARK_GROUND, QUIET_ON_DARK, strip), (MARGIN + strip + GAP, y)
    )

    pen.text(
        (MARGIN, height - MARGIN - 6),
        "Rendered by design/assets/app-icon/make-candidates-board.py — headless Chromium per size, "
        "nearest-neighbour magnification only. Sources: design/assets/app-icon/candidates/{a..e}.svg",
        font=TINY,
        fill=QUIET,
    )

    target = HERE / "candidates-2026-08-28.png"
    board.save(target)
    print(f"{target} — {board.width}×{board.height}")


if __name__ == "__main__":
    main()
