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
