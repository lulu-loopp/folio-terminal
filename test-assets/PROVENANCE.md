# Where the files in `test-assets/` came from

28 tracked files: 10 here and 18 under `preview-samples/`. Each is one of three
things — **own** (written here), **upstream** (someone else's, under someone
else's licence), or **generated** (produced from something else in this
directory).

**None of these is upstream.** Nothing in this directory was copied from another
project, another codebase, or a public dataset. They are fixtures, and a fixture
whose provenance is "we found it somewhere" is a fixture that cannot be shipped.

Several are compiled into the test binaries with `include_bytes!` /
`include_str!` (`crates/bt-app/src/pdf.rs`, `preview.rs`, `main.rs`), so they are
not merely files in a folder: they are part of what the test suite asserts on.

## The ten at the top

| File | | |
|---|---|---|
| `folio-pdf-test.html` | **own** | The source document for the PDF beside it. Its own HTML comment says so and gives the command to remake it. |
| `folio-pdf-test.pdf` | **generated** | Printed from `folio-pdf-test.html` with headless Edge (`msedge --headless=new --print-to-pdf`). The file's own metadata agrees: `/Producer (Skia/PDF m151)`. It replaced an earlier fixture in 2026-08 whose relative asset references made the preview fail with `ERR_FILE_NOT_FOUND` — the story is in `docs/DESIGN.md`. |
| `latex-render-check.md` | **own** | The formula-rendering acceptance corpus. Its contents are standard mathematical notation — the quadratic formula, an integral, a matrix — which is not anyone's copyrightable expression. |
| `md-formula-check.md` | **own** | The markdown-preview formula corpus, positive and negative cases. |

### The five recordings

All **generated**, and generated from nothing: each is two solid colours from
`ffmpeg`'s own `lavfi` synthetic source, concatenated. There is no third-party
footage here and there is nothing to licence. Every one is 160×120 and 3.0
seconds (the `.mp4` is 5.0), with the first fifth of a second black so that a
test asserting "the first frame is dark and a frame later in is not" has
something to bite on.

The five exist because there are five **containers**, and §7.44 ⑥ built the
playable matrix by opening a file in each rather than by asking `CanPlayType` —
a function §7.42 ⑧ caught under-reporting. Deleting one of these deletes the
evidence for a row of that table.

| File | Bytes | Codec / container | Colour | Made by |
|---|---|---|---|---|
| `folio-video-test.mp4` | 2982 | H.264 / MPEG-4 | orange `0xE07A2F` | §7.23 ⑨'s command, 2026-08-27 |
| `folio-video-test.mov` | 2315 | H.264 / QuickTime | blue `0x2F7AE0` | §7.23 (i)'s command, 2026-08-27 |
| `folio-video-test.mkv` | 2457 | H.264 / Matroska | green `0x2FE07A` | below, 2026-08-28 |
| `folio-video-test.avi` | 10986 | MPEG-4 part 2 (XVID) / AVI | pink `0xE02F7A` | below, 2026-08-28 |
| `folio-video-test.wmv` | 3937 | WMV2 / ASF | violet `0x7A2FE0` | below, 2026-08-28 |

The three from 2026-08-28, made with this repository's machine's own ffmpeg
8.1.1, one line each with `<C>` and the encoder flags from the table above:

```
ffmpeg -f lavfi -i color=c=black:s=160x120:r=5:d=0.2 \
       -f lavfi -i color=c=<C>:s=160x120:r=5:d=2.8 \
       -filter_complex "[0:v][1:v]concat=n=2:v=1:a=0[v]" -map "[v]" \
       <encoder> folio-video-test.<ext>
```

| ext | `<encoder>` |
|---|---|
| `mkv` | `-c:v libx264 -preset veryslow -crf 28 -g 1 -pix_fmt yuv420p` |
| `avi` | `-c:v mpeg4 -vtag XVID -q:v 8 -pix_fmt yuv420p` |
| `wmv` | `-c:v wmv2 -b:v 60k -pix_fmt yuv420p` |

### The animation

| File | | |
|---|---|---|
| `folio-anim-test.gif` | **generated** | 423 bytes, 64×64, four frames of one solid colour each — red `0xE04B2F`, green `0x2FE04B`, blue `0x2F4BE0`, yellow `0xE0D22F` — with **deliberately unequal delays of 100, 200, 300 and 400 ms**. Written by this repository's own `image` crate (`GifEncoder`, `Delay::from_saturating_duration`) and read back through the same crate's `GifDecoder` to confirm the four delays survive the round trip. |

The delays are unequal on purpose and that is the whole fixture: a build that
advanced an animation at a fixed rate — one frame per redraw, or a constant
100 ms — passes every test a uniform GIF could state and fails
`a_gif_advances_by_its_own_frame_delays` on the second frame. The four colours
are distinct so that "which frame is standing" is one pixel read.

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

Every one of the 28 files was read. If a file is added to this directory, it
belongs in this table before it belongs in a commit — and if it ever comes from
somewhere else, it belongs in `THIRD-PARTY-NOTICES.md` too.
