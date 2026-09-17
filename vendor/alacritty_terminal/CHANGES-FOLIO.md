# Folio's changes to `alacritty_terminal`

This directory is a vendored copy of **`alacritty_terminal` 0.26.0**, as
published to crates.io, with changes by the Folio contributors. Upstream is
<https://github.com/alacritty/alacritty>, and the crate is licensed under the
Apache License, Version 2.0 — the copy of that licence that came with it is
`LICENSE-APACHE` in this directory, unchanged.

Section 4(b) of that licence requires every modified file to carry a prominent
notice saying it was changed. Every one of them does: a four-to-nine line block
at the very top of the file, opening with `MODIFIED BY THE FOLIO CONTRIBUTORS`
and pointing back here. This file is the index those notices point at.

Upstream ships **no `NOTICE` file** with this crate — the published archive
contains `LICENSE-APACHE` and nothing else of that kind — so there is no
attribution notice to propagate under §4(d). That is an absence upstream chose,
not a missing file on our side.

## How to reproduce this list

The comparison is against the crates.io archive of the same version, which any
machine that has built this tree already has unpacked at
`~/.cargo/registry/src/index.crates.io-*/alacritty_terminal-0.26.0/`:

```powershell
./scripts/check-vendor-notices.ps1
```

That script is the red gate. It walks every file in this directory, compares it
byte-for-byte with the upstream copy, and fails if any file differs without a
`MODIFIED BY THE FOLIO CONTRIBUTORS` notice — or if a file carries the notice
without differing.

## The 23 files that differ

211 upstream files were compared. None was deleted, 23 differ, and one file was
added: this one.

Twenty of the twenty-two `.rs` files differ **only in formatting**. That is not
a judgement call: running `rustfmt --edition 2024` (this repository's
`rustfmt.toml` is stock rustfmt pinned to the 2024 edition) over the *upstream*
file produces the vendored file byte for byte. Upstream formats with its own
`rustfmt.toml`; the workspace formats with this one.

| File | What changed |
|---|---|
| `Cargo.toml` | A path dependency on this repository's `bt-unicode` crate, which `src/term/mod.rs` needs for grapheme segmentation. |
| `src/grid/mod.rs` | **Code.** Two new methods on `Grid<T>`: `take_history`, which drains the whole native scrollback oldest-first and resets `max_scroll_limit`, and `restore_history`, which puts a previously taken tail back. They exist so a resize transaction can move history out of the grid and back without going through reflow. Everything else in the file is formatting. |
| `src/term/mod.rs` | **Code.** The bulk of Folio's divergence — see the section below. |
| `src/event.rs` | Formatting only. |
| `src/event_loop.rs` | Formatting only. |
| `src/grid/resize.rs` | Formatting only. |
| `src/grid/row.rs` | Formatting only. |
| `src/grid/storage.rs` | Formatting only. |
| `src/grid/tests.rs` | Formatting only. |
| `src/index.rs` | Formatting only. |
| `src/selection.rs` | Formatting only. |
| `src/sync.rs` | Formatting only. |
| `src/term/cell.rs` | **Code.** A ceiling on the zerowidth marks one cell stores (`push_zerowidth`), from `bt_unicode::MAX_GRAPHEME_CLUSTER_CHARS`, and one added flag, `Flags::COMMAND_OUTPUT_WRITE`, in the one bit of the `u16` upstream leaves free. Otherwise formatting only. |
| `src/term/search.rs` | Formatting only. |
| `src/thread.rs` | Formatting only. |
| `src/tty/mod.rs` | Formatting only. |
| `src/tty/unix.rs` | Formatting only. |
| `src/tty/windows/blocking.rs` | Formatting only. |
| `src/tty/windows/child.rs` | Formatting only. |
| `src/tty/windows/conpty.rs` | Formatting only. |
| `src/tty/windows/mod.rs` | Formatting only. |
| `src/vi_mode.rs` | Formatting only. |
| `tests/ref.rs` | Formatting only. |

## What changed in `src/term/mod.rs`

- **A transcript hook.** `Term::set_transcript_hook` installs a callback, and a
  new public vocabulary — `TranscriptEvent`, `ScrollOutCause`,
  `ScrollRegionScope`, `TranscriptScreen`, `RemovedRow` — tells an external
  scrollback owner when rows leave the screen, when the grid scrolls, when the
  screen or the history is cleared, when DECCOLM fires, when the primary screen
  is parked and restored, and on RIS. Folio keeps its own scrollback document
  outside the emulator, and this is how it hears about the emulator's.
- **A resize transaction.** `begin_resize_transaction` /
  `finish_resize_transaction` and their staging helpers coalesce a run of local
  resizes into the single resize the PTY is told about, while keeping the
  scrollback bounded. `Term` gained `Clone` and a `fork` that drops the hook and
  the pending input-write set, which gives the transaction an unresized
  canonical branch to reconcile against.
- **A CPR pending-wrap fix.** `device_status` (`CSI 6 n`) now records the
  position and wrap flag it reported, and a `CUP` that immediately follows
  consumes it, so a line editor's CPR-then-CUP echo no longer clears
  `input_needs_wrap` and lose a logical column across reflow. Any other cursor
  motion in between cancels the pairing.
- **Grapheme clustering and emoji width.** Private mode 2027 is implemented:
  when set, printable input is segmented into UAX #29 grapheme clusters through
  `bt_unicode::{cluster_width, extends_grapheme_cluster}` and written as one
  cell run. With the mode off, the legacy single-codepoint path is kept, except
  that a `U+FE0F` variation selector now widens the preceding
  emoji-presentation base to two cells, which is what `wcwidth`-plus-emoji and
  `string-width` conventions expect. `U+FE0E` deliberately does not narrow.
- **A ceiling on one cluster.** `extend_grapheme` stops storing past
  `bt_unicode::MAX_GRAPHEME_CLUSTER_CHARS` code points, and `Cell::push_zerowidth`
  refuses the same. Every mark added to a cluster re-copies and re-measures the
  whole of it on the window thread, and the cell is cloned into every captured
  row, so an unbounded cluster is a child choosing how much work a repaint does.
  Past the ceiling the run stays one cluster and the cursor does not move; the
  extra marks are simply not kept.
- **Private mode 2031.** The dark/light theme-change notification subscription
  that kitty, foot, contour and WezTerm all speak is accepted and reported
  rather than falling into the unknown-mode branch.
- **An origin-mode cursor fix.** `goto` is the one entry point whose line is
  measured from the top of the scroll region, so it is the only one that adds the
  region's offset; the relative moves (`CUU`, `CUD`, `CNL`, `CPL`, and `CHA`,
  which keeps the line it is on) go through a new `Term::goto_absolute` instead.
  They used to pass their already-absolute line into `goto`, which added the
  offset to it a second time — under origin mode with a region that does not
  start at the top, a column move walked the cursor down the screen. Upstream
  0.26.0 and master are both still written that way, so there is nothing to port:
  this is Folio's own fix, and it is the smallest one that keeps every call
  site's clamping byte for byte.
- **UTF-8 mouse reporting refused.** `DECSET 1005` sets no mode and is reported
  as not set. It used to be recorded and answered for, while the coordinates this
  terminal actually writes are the ordinary ones — so a program that asked, was
  told yes, and then framed every click past column 95 as something else.
  Upstream has the same shape; refusing the mode is the honest half of the two
  answers, and the one that costs no encoder nobody can decode.
- **Input-write tracking.** `take_input_writes` drains the set of rows that
  received printable input since the last drain — distinct from render damage,
  which is about what must be repainted.
- **Write provenance.** `set_write_provenance` says whether the bytes fed from
  here on are a shell command's output — once for each screen, because a screen
  swap happens mid-stream and is itself a change of answer: `swap_alt` and a
  reset exchange the two along with the grids they belong to, and every print
  reads the one belonging to the screen that is showing. Every cell whose text
  the terminal *replaces* from then on carries that answer as
  `Flags::COMMAND_OUTPUT_WRITE`
  (see that flag's own documentation). It is there because the answer is a fact
  about a *write* and the cell is the thing that moves: a scroll, a scroll
  region, `IL`/`DL`, `RI`, `CSI S`/`T`, a resize reflow that re-cuts a row and an
  eviction into a scrollback all carry it without anybody having to keep a
  parallel record in step. The terminal never reads it back.

  One rule governs all of it: **a cell's claim is the conjunction over every
  piece of text in the cell** — it holds when a command's output put *all* of
  that text there. The rest follow from it. A write that replaces the whole of a
  cell's text sets or clears the claim from the current provenance. A write that
  does *not* replace the text leaves it alone — which is `put_tab` over an
  occupied cell, whose whole job is to leave that cell's character where it was.
  A zero-width mark appended under a non-output provenance clears the claim on
  the cell it lands on, because that cell's text is now partly the appender's;
  appended under an output provenance it neither grants a claim nor removes one.
  `Cell::reset` takes the default flags, so an erase clears it too.

  Two writes keep somebody else's text while putting text of their own into the
  same cell, and both intersect rather than stamp. `rewrite_grapheme_width`
  re-cuts a cluster whose width changed, and at the right margin it relocates
  the whole cluster to the next row through the printing path — so it carries
  the claim the cluster had (`Term::carry_claim`) instead of letting
  `write_at_cursor` date the base character by the mark that widened it. And
  `put_tab` over a blank cell replaces the base character while leaving that
  cell's zero-width marks in place, so where such marks survive it intersects
  with what the cell could claim before.

  The field starts empty and the flag is the *claim* rather than its denial, so
  a caller that never speaks — upstream's own test suite — writes exactly the
  cells upstream writes, and that silence reads as claimed by nobody rather than
  as claimed by a command. Text that reaches a cell by a road nobody stamped
  (`DECALN`, a reset, a cell a shift creates) therefore claims nothing, which is
  the direction a terminal with this many roads into a cell has to fail in.
- **`primary_grid` is public.** The accessor itself is Folio's and already
  existed for the resize transaction; only its visibility changed. `resize`
  reflows the inactive grid as well as the active one, so a resize taken while a
  full-screen program is showing re-cuts the primary screen's logical lines
  behind it, and Folio — which keeps its scrollback and its anchors outside the
  emulator — has to be able to read that screen at the one moment it is not the
  one on display.
- **Tests** for all of the above, added alongside upstream's.

Upstream's own test suite still runs against this copy: `vendor/alacritty_terminal`
is a workspace member precisely so that it does, and CI asserts that it stays
one.
