#!/usr/bin/env python3
"""Draw `folio.ico` — the application icon `folio.exe` carries in its resources.

The mark is the product's own word taken literally: a sheet folded once, which
is what a folio is. Two half-pages of the same paper, one turned a shade away
from the light, and the fold between them; at 48 and above the fold gets its
page-number dot. There is no accent colour anywhere in it, which is the standing
ruling from the wordmark study (`design/assets/wordmark-r2/DECISION.md`): the
one borrowed-looking stroke in every option was the blue.

Why this is a script and not a hand-drawn file: the icon has to exist at nine
sizes, and at 16px the difference between a legible fold and a smear is which
pixel column the line lands in. A generator states the geometry once, in units
of the square, and every size is that geometry resolved — so the 16 and the 256
are the same drawing rather than two drawings that resemble each other.

Depends on nothing outside the standard library (`zlib` for the two PNG
entries). Run it from anywhere:

    python design/assets/app-icon/make-folio-ico.py

and it rewrites `folio.ico` beside itself. The `.ico` is checked in; this file
is how it can be checked.
"""

from __future__ import annotations

import struct
import zlib
from pathlib import Path

# ── the drawing, in units of the square ──────────────────────────────────────

GROUND = (0x20, 0x20, 0x27, 0xFF)  # graphite, the colour of a dark title bar
PAGE_LEFT = (0xF4, 0xF1, 0xEA, 0xFF)  # paper facing the light
PAGE_RIGHT = (0xDD, 0xD7, 0xC9, 0xFF)  # the half the fold turns away
FOLD = (0xB6, 0xAD, 0x9B, 0xFF)  # the crease itself
DOT = (0x9A, 0x91, 0x80, 0xFF)  # the page number

GROUND_INSET = 0.02
GROUND_RADIUS = 0.22
SHEET = (0.185, 0.245, 0.815, 0.755)  # x0, y0, x1, y1
SHEET_RADIUS = 0.035
DOT_CENTRE = (0.706, 0.678)
DOT_RADIUS = 0.023
#: Below this the dot is one grey pixel in the middle of the page, which reads
#: as dirt rather than as a folio number.
DOT_FROM_SIZE = 48

#: Nine sizes and not four. Windows picks the nearest one *up* and scales down,
#: so the sizes it actually asks for — 16 in a list, 20 in the tray at 125%, 24
#: in a menu, 32 on the desktop, 40 at 250%, 48 in tiles, 256 in the preview —
#: each get a drawing solved for their own pixel grid instead of a resample of
#: somebody else's.
SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)
#: Above this the entry is stored as PNG. That is the format Explorer expects
#: for the large entries, and it is the difference between a 350KB icon and a
#: 20KB one; below it the classic DIB is what every consumer reads without
#: question.
PNG_FROM_SIZE = 128

#: The grid each pixel is sampled on. Sixteen samples is enough that a fold line
#: landing between two columns comes out as two half-lit columns rather than as
#: a line that jumps a pixel between sizes.
SUPERSAMPLE = 4


def rounded_rect_covers(x: float, y: float, box, radius: float) -> bool:
    """Whether the point is inside a rectangle with rounded corners."""
    x0, y0, x1, y1 = box
    if not (x0 <= x <= x1 and y0 <= y <= y1):
        return False
    # Only the corner quadrants can exclude a point that is inside the box.
    cx = min(max(x, x0 + radius), x1 - radius)
    cy = min(max(y, y0 + radius), y1 - radius)
    dx = x - cx
    dy = y - cy
    return dx * dx + dy * dy <= radius * radius


def sample(x: float, y: float, size: int):
    """The colour of the drawing at one point, topmost layer first."""
    fold_half_width = max(0.006, 0.5 / size)
    if size >= DOT_FROM_SIZE:
        dx = x - DOT_CENTRE[0]
        dy = y - DOT_CENTRE[1]
        if dx * dx + dy * dy <= DOT_RADIUS * DOT_RADIUS:
            return DOT
    if rounded_rect_covers(x, y, SHEET, SHEET_RADIUS):
        if abs(x - 0.5) <= fold_half_width:
            return FOLD
        return PAGE_RIGHT if x > 0.5 else PAGE_LEFT
    ground = (GROUND_INSET, GROUND_INSET, 1.0 - GROUND_INSET, 1.0 - GROUND_INSET)
    if rounded_rect_covers(x, y, ground, GROUND_RADIUS):
        return GROUND
    return (0, 0, 0, 0)


def render(size: int) -> bytes:
    """The drawing at one size, as straight (non-premultiplied) RGBA rows."""
    out = bytearray(size * size * 4)
    step = 1.0 / (size * SUPERSAMPLE)
    samples = SUPERSAMPLE * SUPERSAMPLE
    for row in range(size):
        for column in range(size):
            r = g = b = a = 0
            for sy in range(SUPERSAMPLE):
                y = (row * SUPERSAMPLE + sy + 0.5) * step
                for sx in range(SUPERSAMPLE):
                    x = (column * SUPERSAMPLE + sx + 0.5) * step
                    sr, sg, sb, sa = sample(x, y, size)
                    # Premultiplied, so that a half-covered edge against
                    # transparency does not drag the colour towards black.
                    r += sr * sa
                    g += sg * sa
                    b += sb * sa
                    a += sa
            index = (row * size + column) * 4
            if a == 0:
                continue
            out[index + 0] = round(r / a)
            out[index + 1] = round(g / a)
            out[index + 2] = round(b / a)
            out[index + 3] = round(a / samples)
    return bytes(out)


def png(size: int, rgba: bytes) -> bytes:
    """`rgba` as a PNG, which is the format the large icon entries take."""

    def chunk(kind: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
        )

    raw = bytearray()
    stride = size * 4
    for row in range(size):
        raw.append(0)  # filter: none, which is what a flat-colour drawing wants
        raw += rgba[row * stride : (row + 1) * stride]
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def dib(size: int, rgba: bytes) -> bytes:
    """`rgba` as an icon DIB: a doubled-height header, BGRA bottom-up, AND mask.

    The mask is all zeros and still mandatory. A 32-bit icon's transparency is
    its alpha channel, but the format's height field counts both bitmaps and a
    consumer that reads `biHeight / 2` rows and then finds no mask has been
    handed a truncated file.
    """
    header = struct.pack(
        "<IiiHHIIiiII",
        40,  # biSize
        size,  # biWidth
        size * 2,  # biHeight: colour bitmap and mask, stacked
        1,  # biPlanes
        32,  # biBitCount
        0,  # biCompression: BI_RGB
        0,  # biSizeImage
        0,  # biXPelsPerMeter
        0,  # biYPelsPerMeter
        0,  # biClrUsed
        0,  # biClrImportant
    )
    colour = bytearray()
    for row in reversed(range(size)):  # bottom-up
        for column in range(size):
            index = (row * size + column) * 4
            r, g, b, a = rgba[index : index + 4]
            colour += bytes((b, g, r, a))
    mask_stride = ((size + 31) // 32) * 4
    mask = bytes(mask_stride * size)
    return header + bytes(colour) + mask


def ico(entries) -> bytes:
    """An ICO file out of `(size, image_bytes, is_png)` triples."""
    count = len(entries)
    out = bytearray(struct.pack("<HHH", 0, 1, count))
    offset = 6 + 16 * count
    directory = bytearray()
    body = bytearray()
    for size, image, _is_png in entries:
        directory += struct.pack(
            "<BBBBHHII",
            0 if size >= 256 else size,  # 256 is spelled 0 in one byte
            0 if size >= 256 else size,
            0,  # bColorCount: 0 for anything deeper than 8bpp
            0,  # bReserved
            1,  # wPlanes
            32,  # wBitCount
            len(image),
            offset,
        )
        body += image
        offset += len(image)
    out += directory
    out += body
    return bytes(out)


def main() -> None:
    entries = []
    for size in SIZES:
        rgba = render(size)
        if size >= PNG_FROM_SIZE:
            entries.append((size, png(size, rgba), True))
        else:
            entries.append((size, dib(size, rgba), False))
    target = Path(__file__).resolve().parent / "folio.ico"
    with open(target, "wb") as handle:
        handle.write(ico(entries))
    print(f"{target} — {len(entries)} entries, {target.stat().st_size} bytes")


if __name__ == "__main__":
    main()
