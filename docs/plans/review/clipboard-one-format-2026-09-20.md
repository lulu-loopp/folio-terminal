# Pasting a picture reads one shape, not every shape (2026-09-20)

Brief `scratchpad/codex-night/81-clipboard-one-format.md`; branch
`fix/clipboard-reads-one-picture` off main `95502c1b`.

## What was there

`WindowsClipboard::picture` ran `IsClipboardFormatAvailable` +
`GetClipboardData` + `GlobalLock` + `to_vec` for all three picture formats and
answered with the whole list; `macos_clipboard_payload.rs` did the same over
`public.png` and `public.tiff` — H3 holds. All of it on the window thread, and
`GetClipboardData` renders delayed formats, so each is a synchronous call into
the source application. H1 holds: `bt_app::clipboard_picture::png_bytes` takes
the first entry that decodes, so the second and third copies were never looked
at.

## What it is now

One ordered preference list per platform in `bt-platform/src/clipboard.rs` —
`WINDOWS_PICTURE_ORDER = [Png, DibV5, Dib]`, `MACOS_PICTURE_ORDER = [Png, Tiff]`
— walked by `first_offered_picture`, which asks a `PictureSource` whether a
shape is offered and, if so, to render it, and **returns on the first that
answers**. `PNG` first: lossless, alpha, an order of magnitude smaller, and
already the encoding this paste writes out. `CF_DIBV5` before `CF_DIB` because
its header can carry alpha. `Tiff` is macOS' fallback, not an alternative — an
AppKit copy offers it alone. `ClipboardPort::picture` keeps its list-shaped
answer (the worker consumes a list; `bt-app` is untouched); the list holds one.

## Budget (A5), by construction

4K screenshot, 3840x2160, offered as all three Windows shapes. Before: 3 copies,
~33.2 MB + ~33.2 MB + PNG ≈ 66 MB + PNG, 3 `GetClipboardData`. After: 1 copy,
the PNG alone, 1 `GetClipboardData`. A DIB of that screen is 3840·2160·4 =
33,177,600 bytes plus header; a 4K TIFF on macOS is about the same.

## Evidence

`cargo test -p bt-platform --lib -j 4 clipboard` — 18 tests, all pass;
`cargo clippy -p bt-platform -j 4 --all-targets -- -D warnings` clean. Three new
tests in `clipboard::tests` over a `Board` double that counts availability
questions and renders separately — no real clipboard is touched:

- A1 `the_best_shape_on_offer_is_the_only_one_rendered`. Red with only the
  product change reverted to main's eager collect: `left: [Png, DibV5, Dib] /
  right: [Png]` at `assert_eq!(board.rendered, [PictureEncoding::Png])`.
- A2 `a_shape_that_is_missing_or_will_not_render_falls_to_the_next` — five
  boards: first absent, first unrenderable, two unrenderable in a row, only
  `CF_DIB`, nothing renderable at all (still `Absent`, today's answer).
- A3 `text_and_files_win_without_a_single_picture_being_rendered`, through
  `read_payload`: zero questions, zero bytes. Plus
  `the_preference_lists_are_each_platforms_own_shapes_best_first`.

Reverting only the product change turned A1, A2 and A3 red (rule 4).

## Not verified

macOS (A4) is edited for compile-cleanliness only; no toolchain here. `bt-app`
was not built. One behaviour to name: a source that renders a `PNG` its own
decoder cannot read now ends the paste with that rung's reason instead of
falling to the `CF_DIB` behind it — the brief calls this a separate question.
`png_bytes`'s fallback loop is untouched, and a shape that is advertised and
then declines to render is still handled, before the copy, by the walk.

## Follow-up (2026-09-20, second commit)

The residual named above was the coordinator's blocking finding under B1, and it
is now closed at read time rather than by giving the waste back.

`first_offered_picture` accepts a shape only if `shape_is_intact` agrees it is
the shape it claims to be — headers only, no inflate, no pixel touched:

- **PNG** — the 8-byte signature; `IHDR` present as the first chunk with its
  spec-fixed length of 13; width and height non-zero and `<= MAX_PICTURE_SIDE`
  (16,384, the number `bt_app::clipboard_picture::MAX_SIDE` refuses at); bit
  depth in {1,2,4,8,16}, colour type in {0,2,3,4,6}, compression and filter 0,
  interlace `<= 1`; then chunk headers walked to the first `IDAT`, each chunk's
  declared extent required to lie inside the copy, at most
  `PNG_CHUNKS_BEFORE_PIXELS` = 64 of them. **Bound: 8 + 25 + 64x8 = 545 bytes
  examined, whatever the picture's size.**
- **DIBV5/DIB** — info header size in {12,40,52,56,64,108,124} and present;
  width not negative, both sides non-zero and `<= MAX_PICTURE_SIDE`; one plane;
  bit count in {1,2,4,8,16,24,32}; a compression Windows defines; and, for the
  uncompressed forms, header + bit-field masks + palette + `stride x height`
  must fit the global. **Bound: 124 bytes plus arithmetic.**
- **TIFF** — `II`/`MM`, the answer 42, and a first directory at offset `>= 8`
  with its entry count inside the copy. **Bound: 8 bytes.**

A shape that fails is treated exactly like declined/unlockable: the walk moves
on. A board where *nothing* is intact hands the worker the best shape that
rendered rather than `Absent`, so the paste still ends in the worker's sentence
about why instead of in silence — that costs the old three reads, on the one
board where the old cost was the only route to an answer.

**Residual.** A PNG that is structurally whole but fails deep in inflate still
ends the paste; today it would have fallen back to the `CF_DIB`. The class has
narrowed from "anything the PNG decoder dislikes" (wrong signature, truncation,
a garbage header, bad dimensions — what the real offenders produce) to "valid
container, corrupt compressed stream", and a source that gets the container
right and the deflate wrong is one nobody has reported. Acceptable, and recorded
here rather than in the code alone. The one sound way to close it without eager
copying is to re-open the clipboard from the paste only if
`GetClipboardSequenceNumber()` still matches the value taken at read time, which
proves the board has not changed; the coordinator ruled re-opening out of bounds,
and it would cost a second window-thread trip, so it is not built.

Known second copy: `MAX_PICTURE_SIDE` here and `MAX_SIDE` in
`bt_app::clipboard_picture`, like the existing `MAX_PICTURE_BYTES` /
`MAX_ENCODED_BYTES` pair. `bt-platform` is the right owner (the dependency runs
one way), but making `bt-app` consume it cannot be compiled under this brief.

Evidence: 22 tests pass. Dropping only the `shape_is_intact` call from the walk
turns `a_png_that_no_decoder_could_read_loses_to_the_bitmap_beside_it`,
`a_broken_pasteboard_png_falls_to_the_tiff` and
`a_board_where_nothing_is_whole_still_gives_the_worker_something_to_refuse` red.
A1 stays red on the eager mutation, with the same `[Png, DibV5, Dib] / [Png]`.
`a_header_is_read_for_shape_and_never_for_pixels` pins every rule above against
the smallest break of a whole fixture.
