# make-objstm-pdf.py — writes `test-assets/folio-pdf-objstm-test.pdf`.
#
# The fixture for a PDF that keeps its catalogue, its page tree and its page
# objects inside a **compressed object stream** (PDF 1.5's `/Type /ObjStm`),
# reached through a **cross-reference stream** rather than a `xref` table. That
# is what every pdfTeX-produced document on this machine looks like, and it is
# the shape a byte scan cannot read: nothing that says `/Type /Pages`, `/Count`
# or `/Type /Page` survives compression, so the file's structure is only
# readable to something that inflates it (`crates/bt-app/src/pdf.rs`).
#
# Five pages, so the number cannot be confused with the three of the Skia
# fixture beside it. Every page is an empty `/MediaBox` — the fixture is about
# structure and nothing is drawn.
#
# Run from the repository root:
#
#     python scripts/dev/make-objstm-pdf.py
#
# It is deterministic: the same bytes come out on every machine and every run,
# which is what lets the file be committed and compared.

import io
import os
import zlib

PAGES = 5
# 1 catalogue + 1 page-tree node + PAGES page objects, numbered from 1.
FIRST_PAGE_OBJ = 3
OBJSTM_OBJ = FIRST_PAGE_OBJ + PAGES  # 8
XREF_OBJ = OBJSTM_OBJ + 1  # 9


def packed_objects():
    """The objects that go inside the object stream, as (number, bytes)."""
    kids = " ".join(f"{FIRST_PAGE_OBJ + i} 0 R" for i in range(PAGES))
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, f"<< /Type /Pages /Kids [{kids}] /Count {PAGES} >>".encode("ascii")),
    ]
    for i in range(PAGES):
        objects.append(
            (
                FIRST_PAGE_OBJ + i,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
            )
        )
    return objects


def object_stream():
    """`/Type /ObjStm`: a pair table, then the objects, all Flate-compressed."""
    objects = packed_objects()
    body = io.BytesIO()
    pairs = []
    for number, payload in objects:
        pairs.append(f"{number} {body.tell()}")
        body.write(payload)
        body.write(b" ")
    table = (" ".join(pairs) + "\n").encode("ascii")
    plain = table + body.getvalue()
    packed = zlib.compress(plain, 9)
    head = (
        f"<< /Type /ObjStm /N {len(objects)} /First {len(table)} "
        f"/Length {len(packed)} /Filter /FlateDecode >>"
    ).encode("ascii")
    return head, packed


def main():
    out = io.BytesIO()
    out.write(b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n")

    head, packed = object_stream()
    offsets = {}
    offsets[OBJSTM_OBJ] = out.tell()
    out.write(f"{OBJSTM_OBJ} 0 obj\n".encode("ascii"))
    out.write(head)
    out.write(b"\nstream\n")
    out.write(packed)
    out.write(b"\nendstream\nendobj\n")

    # The cross-reference stream. /W [1 2 1]: one byte of type, two of offset or
    # object-stream number, one of generation or index within that stream.
    offsets[XREF_OBJ] = out.tell()
    rows = [bytes([0, 0, 0, 255])]  # object 0, the head of the free list
    for index, (number, _) in enumerate(packed_objects()):
        assert number == index + 1
        rows.append(bytes([2, OBJSTM_OBJ >> 8, OBJSTM_OBJ & 0xFF, index]))
    for number in (OBJSTM_OBJ, XREF_OBJ):
        offset = offsets[number]
        rows.append(bytes([1, offset >> 8, offset & 0xFF, 0]))
    table = zlib.compress(b"".join(rows), 9)
    out.write(f"{XREF_OBJ} 0 obj\n".encode("ascii"))
    out.write(
        (
            f"<< /Type /XRef /Size {XREF_OBJ + 1} /W [1 2 1] /Root 1 0 R "
            f"/Length {len(table)} /Filter /FlateDecode >>"
        ).encode("ascii")
    )
    out.write(b"\nstream\n")
    out.write(table)
    out.write(b"\nendstream\nendobj\n")
    out.write(f"startxref\n{offsets[XREF_OBJ]}\n%%EOF\n".encode("ascii"))

    here = os.path.dirname(os.path.abspath(__file__))
    target = os.path.join(here, "..", "..", "test-assets", "folio-pdf-objstm-test.pdf")
    with io.open(os.path.normpath(target), "wb") as handle:
        handle.write(out.getvalue())
    print(f"{os.path.normpath(target)}: {out.tell()} bytes, {PAGES} pages")


if __name__ == "__main__":
    main()
