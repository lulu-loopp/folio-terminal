#!/usr/bin/env python3
"""Draw the three PNGs `packaging/msix/AppxManifest.xml` names.

A package manifest has to point at square logos, and Folio already has a mark:
`make-folio-ico.py` states it as geometry in units of the square, so the three
sizes a manifest asks for are that same drawing resolved three more times rather
than three crops of a bitmap. That is the whole content of this file — it owns
no geometry, no colour and no size but the three the manifest names.

Nothing outside the standard library, and nothing outside this directory. Run it
from anywhere:

    python assets/app-icon/make-msix-logos.py

and it rewrites the three files under `packaging/msix/images/`. They are checked
in, because the packaging script must not need a Python to build a release.
"""

from __future__ import annotations

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

# The one drawing, imported rather than copied: a second statement of the mark is
# a second mark, and it would be the one that went stale. Loaded by path because
# the file it lives in is spelled with hyphens and cannot be an import name.
import importlib.util

_spec = importlib.util.spec_from_file_location("folio_icon", HERE / "make-folio-ico.py")
folio = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(folio)

# The three the manifest names, and no fourth. `Square44x44Logo` is what a task
# bar and an app list draw, `Square150x150Logo` is the medium tile, and the
# `Logo` in `<Properties>` is the 50-pixel one the deployment stack reads.
LOGOS = {
    "Square44x44Logo.png": 44,
    "Square150x150Logo.png": 150,
    "StoreLogo.png": 50,
}


def main() -> None:
    out = HERE.parents[1] / "packaging" / "msix" / "images"
    out.mkdir(parents=True, exist_ok=True)
    for name, size in LOGOS.items():
        (out / name).write_bytes(folio.png(size, folio.render(size)))
        print(f"{name} — {size}x{size}")


if __name__ == "__main__":
    main()
