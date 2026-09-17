# Live inline-site review — 2026-09-17

Reviewed `66145615f164e0207d37ec7ea5d71605ebb1c1da`, parent `d1db4abd`.
References are at 66145615; `session.rs` means `crates/bt-term/src/session.rs`. Product files were read only.

**Verdict: hold.** A new, reproduced sequence typesets prompt text as inline math.
K1 remains sensitive to CR/LF boundaries. K2 passes the changed-body check; positive tests do not meet the bar.

## New regressions

### R1 — P1: identical prompt writes retain retired output provenance

`session.rs:6779` skips equal fingerprints before `record_live_inline_sites`; the
detector unconditionally consumes the retained site at `:5847`. Equality proves
cell content, not ownership. Reproduction uses 60 columns, eight rows, staging
and frozen quotas of one, and ordinary `feed_at` calls, with no map manipulation:

1. Feed `OSC C`, then `a\r\nb\r\nc\r\nenergy $E = mc^2$ here\r\n`.
2. Feed six `tailN\r\n` lines and `OSC D`. The formula moves to row zero;
   its output region is naturally evicted, which the test explicitly verifies.
3. Feed `CUP 1;1`, `OSC A`, the identical energy line, then `OSC B`.
4. Settle and complete through the real math engine: the viewport contains an
   inline raster over the **prompt**. Main leaves that prompt unrendered.

The empty B..cursor input region excludes the preceding prompt; the authoritative
integration also disables the cursor fallback (`session.rs:5785`).
Fix: track actual write ownership independently of the fingerprint optimization;
prompt/input writes must revoke eligibility even when every cell is identical.

### R2 — P2: CR at a feed/quantum boundary leaves output ineligible

`session.rs:5025` uses the cursor as the open output frontier. After printing
`energy $E = mc^2$ here\r`, the cursor is at column zero, before the line's end.
The changed row is therefore recorded Ineligible (`:6851`). A following `\n` and
D change no source bytes, so `:6779` prevents correcting that answer.
A width-15 A/B/C/D fixture fails when split immediately after that CR;
every split passes on main. Padding with ignored NULs also places CR exactly at
`PARSE_QUANTUM - 1` in a single feed, reproducing the lost site at width 60.
Fix: capture where writes occurred, not merely where the cursor stands when a
budget expires; keep the damage/stability cache separate from that provenance.

## Other attacks: unsafe cases already reproducible on main

These are not attributed as newly introduced regressions, but still violate the bar:

- Reprinting an identical prompt without eviction, after ED2/home, or after RIS
  produces a real inline raster. A changed, shorter prompt also remains eligible
  through the old closed spatial region (`session.rs:5013`, `:9748`, `:9757`).
- Two identical hard lines, upper prompt and lower output, with the cursor on the
  upper line: shrinking height from two to one assigns output eligibility to the
  surviving prompt. The new bottom-text carry (`session.rs:6899`) independently
  accepts this false identity. The vendor can remove bottom lines
  (`vendor/alacritty_terminal/src/grid/resize.rs:90`, `:100`); main's region
  rematching also fails this case. Require actual line identity or fail closed.
- With identical prompt/output/prompt rows, IL, DL, RI, CSI S/T and partial-margin
  S/T leave the moved prompt at an output site. Grid movement only clears cursor
  memory at `session.rs:9745`; equal fingerprints cannot recover ownership.
  A top-anchored DECSTBM region ending above fixed bottom rows is also unsafe:
  `:10651` drains the entire record vector. In a six-row fixture with margins 1..3,
  fixed prompt row 4 inherits fixed output row 5's site. Carry exact moved ranges.

## Boundary, logical-line, screen, and K2 checks

`crates/bt-term/src/adapter.rs:768` pauses at each marker before later bytes.
`session.rs:2985` applies the marker **before** observing damage; D has already
closed the region. Closed endpoints remain usable (`:4647`, `:5013`), so D alone
is not retirement. Other tested quantum positions, including D first in the next
quantum and a split inside formula text, retain output eligibility.
Output without newline followed by prompt/input becomes Ineligible for the whole
logical line (`:6980`); unclaimed text is not upgraded by a later C without writes.
Reflow joins WRAPLINE continuations, not hard lines. Snapshot/re-establishment is
synchronous; shell redraw bytes are handled by later feeds, where R1 applies.
Alternate enter/leave resets records (`:9788`, `:9802`); the primary prompt stays
Ineligible in the scratch test. This resets, rather than saves, primary site memory.

K2: changed body bytes call `invalidate_live_row` (`session.rs:6805`, `:6999`),
which removes every covering decoration even with an unchanged end row; the
scratch body-edit test passes. Late resolved work checks the whole borrowed band
(`:13824`); reprint projection rejects internal source mismatches (`:13618`).
The unresolved end-row check at `:7460` does not remove these independent guards.

## Test evidence

Each new test passed with `cargo test -p bt-term --lib <filter> -j 4`:

- `a_live_inline_run_keeps_its_site_after_the_prompt_returns`
- `a_wrapped_output_line_keeps_its_formulas_across_a_width_change`
- `an_unresolved_body_row_completion_does_not_retire_the_block_that_covers_it`

Transplanted unchanged onto the actual main session/doc source, all three fail:
accepted completions 0 vs 2; reflowed site Ineligible vs CommandOutput; remaining
decorations 0 vs 1. These are useful, non-tautological retirement/K2 fixtures,
but the K1 tests clear the region map explicitly, not merely close it with D.
None attacks laundering; existing `session.rs:30071` and `:30680` cover only ordinary separation/reflow.
The eleven initial scratch attacks yield 2 passes/9 failures here versus 4/7 on
main; the two new failures are R1 after eviction and R2's feed split. Additional
quantum checks isolate R2 and verify ordinary mixed/unclaimed-line behavior.
`cargo test -p bt-detect -j 4`: 150 unit tests and one integration test passed;
Zero doc tests; no broader suite. All scratch source/tests were deleted.
