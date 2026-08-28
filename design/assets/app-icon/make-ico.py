#!/usr/bin/env python3
"""Turn one SVG into a Windows `.ico` carrying all nine sizes.

    python design/assets/app-icon/make-ico.py candidates/e.svg candidates/e.ico

Why the sizes are *rendered* and not *resampled*: Windows asks for a different
pixel grid in every place it draws an application icon — 16 in a list, 20 in the
tray at 125%, 24 in a menu, 32 on the desktop, 40 at 250%, 48 in Alt-Tab, 256 in
the preview — and picks the nearest entry *up*, scaling down whatever it finds.
An icon built by shrinking one 256px bitmap therefore reaches 16px having been
resampled twice. Here each entry is the vector solved on its own grid, so the 16
and the 256 are the same drawing rather than two drawings that resemble each
other. That is the same rule `make-folio-ico.py` states in code, kept for a
drawing that arrives as a file instead of as geometry.

The renderer is a headless Chrome or Edge, because it is the one SVG rasteriser
that is already on every machine this project is built on; `resvg`, ImageMagick
and `cairosvg` are all absent here. Pass `--browser` to name another one. The
`.ico` container is assembled by hand — Pillow's ICO writer resamples from a
single image, which is exactly the thing this script exists to avoid — and
Pillow is used only to read the PNGs the browser wrote.

Depends on Pillow and on a Chromium-family browser.
"""

from __future__ import annotations

import argparse
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

from PIL import Image

#: The sizes a Windows application icon is actually asked for. See the module
#: docstring; this list is `make-folio-ico.py`'s, unchanged.
SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)

#: At and above this size the entry is stored as a PNG — the format Explorer
#: expects for the large entries, and the difference between a 350KB icon and a
#: 20KB one. Below it the classic DIB is what every consumer reads without
#: question.
PNG_FROM_SIZE = 128

#: Where a Chromium-family browser lives on a stock Windows machine, most
#: preferred first. Overridable with `--browser`.
BROWSERS = (
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
)


def find_browser(named: str | None) -> str:
    if named:
        return named
    for candidate in BROWSERS:
        if Path(candidate).is_file():
            return candidate
    raise SystemExit(
        "no Chromium-family browser found; pass --browser with the path to one"
    )


def rasterise(svg: Path, sizes, browser: str) -> dict[int, Image.Image]:
    """Render `svg` once per size, each on its own pixel grid.

    One browser launch per size. A page holding all the sizes at once would be
    one launch, and would also let a rounding of the page layout move a drawing
    half a pixel off the grid it is being solved for — which at 16px is the
    whole question.
    """
    out: dict[int, Image.Image] = {}
    with tempfile.TemporaryDirectory(prefix="folio-ico-") as work:
        work = Path(work)
        # Copied beside the wrapper so the `img` src is a bare filename: a
        # `file:` URL with a Windows drive letter and spaces in it is the one
        # part of this that browsers disagree about.
        drawing = work / "drawing.svg"
        drawing.write_bytes(svg.read_bytes())
        for size in sizes:
            page = work / f"page-{size}.html"
            page.write_text(
                '<html><body style="margin:0;padding:0;overflow:hidden">'
                f'<img src="drawing.svg" width="{size}" height="{size}">'
                "</body></html>",
                encoding="utf-8",
            )
            shot = work / f"shot-{size}.png"
            subprocess.run(
                [
                    browser,
                    "--headless=new",
                    "--disable-gpu",
                    "--hide-scrollbars",
                    "--force-device-scale-factor=1",
                    # Transparent, so the icon keeps the alpha the drawing has
                    # rather than the white a screenshot defaults to.
                    "--default-background-color=00000000",
                    f"--screenshot={shot}",
                    f"--window-size={size},{size}",
                    str(page),
                ],
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            image = Image.open(shot).convert("RGBA")
            if image.size != (size, size):
                raise SystemExit(f"{browser} returned {image.size} for {size}px")
            out[size] = image.copy()
    return out


def png(image: Image.Image) -> bytes:
    """`image` as a PNG, which is the format the large icon entries take."""

    def chunk(kind: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
        )

    size = image.width
    rgba = image.tobytes()
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


def dib(image: Image.Image) -> bytes:
    """`image` as an icon DIB: a doubled-height header, BGRA bottom-up, AND mask.

    The mask is all zeros and still mandatory. A 32-bit icon's transparency is
    its alpha channel, but the format's height field counts both bitmaps, and a
    consumer that reads `biHeight / 2` rows and then finds no mask has been
    handed a truncated file.
    """
    size = image.width
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
    rgba = image.tobytes()
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
    """An ICO file out of `(size, image_bytes)` pairs."""
    count = len(entries)
    out = bytearray(struct.pack("<HHH", 0, 1, count))
    offset = 6 + 16 * count
    directory = bytearray()
    body = bytearray()
    for size, image in entries:
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


def build(svg: Path, target: Path, browser: str) -> None:
    rendered = rasterise(svg, SIZES, browser)
    entries = [
        (size, png(rendered[size]) if size >= PNG_FROM_SIZE else dib(rendered[size]))
        for size in SIZES
    ]
    target.write_bytes(ico(entries))
    print(f"{target} — {len(entries)} entries, {target.stat().st_size} bytes")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("svg", type=Path, help="the drawing")
    parser.add_argument(
        "ico",
        type=Path,
        nargs="?",
        help="where to write it (default: the SVG's name with an .ico suffix)",
    )
    parser.add_argument("--browser", help="path to a Chromium-family browser")
    arguments = parser.parse_args(argv)

    svg = arguments.svg.resolve()
    if not svg.is_file():
        raise SystemExit(f"{svg} is not a file")
    target = (arguments.ico or svg.with_suffix(".ico")).resolve()
    build(svg, target, find_browser(arguments.browser))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
