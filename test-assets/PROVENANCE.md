# Where the files in `test-assets/` came from

22 tracked files: 4 here and 18 under `preview-samples/`. Each is one of three
things — **own** (written here), **upstream** (someone else's, under someone
else's licence), or **generated** (produced from something else in this
directory).

**None of these is upstream.** Nothing in this directory was copied from another
project, another codebase, or a public dataset. They are fixtures, and a fixture
whose provenance is "we found it somewhere" is a fixture that cannot be shipped.

Several are compiled into the test binaries with `include_bytes!` /
`include_str!` (`crates/bt-app/src/pdf.rs`, `preview.rs`, `main.rs`), so they are
not merely files in a folder: they are part of what the test suite asserts on.

## The four at the top

| File | | |
|---|---|---|
| `folio-pdf-test.html` | **own** | The source document for the PDF beside it. Its own HTML comment says so and gives the command to remake it. |
| `folio-pdf-test.pdf` | **generated** | Printed from `folio-pdf-test.html` with headless Edge (`msedge --headless=new --print-to-pdf`). The file's own metadata agrees: `/Producer (Skia/PDF m151)`. It replaced an earlier fixture in 2026-08 whose relative asset references made the preview fail with `ERR_FILE_NOT_FOUND` — the story is in `docs/DESIGN.md`. |
| `latex-render-check.md` | **own** | The formula-rendering acceptance corpus. Its contents are standard mathematical notation — the quadratic formula, an integral, a matrix — which is not anyone's copyrightable expression. |
| `md-formula-check.md` | **own** | The markdown-preview formula corpus, positive and negative cases. |

## `preview-samples/` — 18 synthetic fixtures

All **own**, all built to make one preview-pane behaviour observable. Each was
written for its case; none is a real-world file.

| File | What it is for |
|---|---|
| `.dotfile` | A name with no extension and a leading dot |
| `binary.bin` | `BIN\0` repeated — the "this is not text" path |
| `empty.txt` | Zero bytes |
| `huge.txt` | `line NNNN: the quick brown fox…` × 2999 — the long-document path |
| `long-lines.txt` | An unbreakable 190-character token, and a 400-character line |
| `sample.csv` | A small well-formed table |
| `sample.dat` | `00 01 02 03` repeated — an unknown extension over binary content |
| `sample.diff` | A unified diff, for the diff colouring |
| `sample.html` | A minimal document, for the HTML path |
| `sample.json` / `sample.toml` / `sample.yaml` | One minimal document each, for the structured-text paths |
| `sample.md` | A minimal markdown document |
| `sample.py` / `sample.rs` | Toy snippets — a few lines each, written here, not lifted from this project's own source or anyone else's |
| `stress.md` | Deliberately pathological markdown, with in-file comments saying which case each block is |
| `wide.csv` | A 12-column table, for horizontal overflow |
| `中文文件名.md` | A non-ASCII filename, for the path and label handling |

## Nothing here is undetermined

Every one of the 22 files was read. If a file is added to this directory, it
belongs in this table before it belongs in a commit — and if it ever comes from
somewhere else, it belongs in `THIRD-PARTY-NOTICES.md` too.
