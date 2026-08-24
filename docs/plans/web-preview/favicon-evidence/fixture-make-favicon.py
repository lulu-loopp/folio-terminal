"""Write the acceptance fixture's favicon: 32x32, unmistakable, no dependencies.

Solid magenta with a white ring and a black centre dot. Three flat colours and
hard edges, so that a screenshot can be checked by naming a pixel rather than by
looking: nothing this window draws is magenta, so a magenta pixel where the globe
used to be *is* the assertion.
"""

import struct
import zlib

SIZE = 32
MAGENTA = (255, 0, 255, 255)
WHITE = (255, 255, 255, 255)
BLACK = (0, 0, 0, 255)


def pixel(x: int, y: int):
    cx = cy = (SIZE - 1) / 2
    d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
    if d < 4:
        return BLACK
    if d < 9:
        return WHITE
    return MAGENTA


rows = bytearray()
for y in range(SIZE):
    rows.append(0)  # filter: none
    for x in range(SIZE):
        rows.extend(pixel(x, y))


def chunk(tag: bytes, body: bytes) -> bytes:
    return (
        struct.pack(">I", len(body))
        + tag
        + body
        + struct.pack(">I", zlib.crc32(tag + body) & 0xFFFFFFFF)
    )


png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(bytes(rows), 9))
png += chunk(b"IEND", b"")

with open("favicon.png", "wb") as out:
    out.write(png)
print(f"favicon.png {len(png)} bytes")
