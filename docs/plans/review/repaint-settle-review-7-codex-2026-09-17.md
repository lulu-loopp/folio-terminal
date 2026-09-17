# Repaint settle: final review 7

Verdict: **MERGE**. No demonstrated finding meets release bar (1), (2), or (3).
Scope: `git show` of 0f49d743, 3a20a69e, c425e400; reviewed tree c425e400. Nothing below 2b42eab1 reopened.
Bar: introduced/worsened (1); ordinary-use mismatched picture publication (2); alternate-screen TUI scrolling with typeset `$$` broken (3).

## Scoped audit
- Round-6 clipped-closer rows: CLOSED for closed and open fences, immediately and after stability; permanent oracle fixture passes. Immediate-parent comparison reproduces the closed-fence failure.
- Clip/phantom detector decisions now reach the restore door through the complete inputs and the detector's own boundary/clip/resync calculation; no extent re-run or prefix fence pre-gate remains.
- Ownership requires matching rows, original/render source, delimiter, mode, and kind. No remaining disagreement with that scan was demonstrated.
- Memo: screen, content revision, grid generation, detection revision, layout, options, history count/tail, and both parser checkpoints are explicit key fields; queued ids and resident bands are explicit restore fields.
- Cursor position/suppression and selection are not inputs to either restore ownership or this scan; stability is deliberately all-true. Their omission cannot stale this answer.
- Shell authority is implied for eligible primary text: output provenance cannot precede authority, which persists on primary; alternate authority resets with the screen. Provenance changes are fingerprinted; alternate output/content sites have identical detection eligibility.
- `inline_math_bands` is in detection options. Wrap flags, cell boundaries, and fixed-tail text follow content revision; dimensions/reflow follow generation/layout.
- Clip evidence is derived from keyed inputs/checkpoints. `alternate_content_end_row` affects projection, not this scan; borrowed-band clearance is checked against current inputs outside the cached answer.
- Frozen tail source/site/checkpoints are established on ingestion; append/eviction changes the tail key. Queued source identity stays fixed; stale/relayout changes follow layout/generation, and prefix cleanup is not a match dependency.
- No constructed stale-answer publication or missed same-read return from the memo. The identical-repaint and returning-source fixtures pass.
- Retirement strikes both snapshot lists and the queue. Remaining removals are damage invalidation, lifecycle clear/transfer, queue capacity, close-time deduplication, resize-hold expiry, and off-band frozen-successor handoff; no new resident-verdict bypass found.
- Same-id re-proof is not tombstoned: a later resident survives carried-first alternate close; primary carries it because the struck snapshot no longer lists that id. Worker replacement itself allocates a fresh occurrence id.
- No new stability/cursor candidate gate delays `$$` restoration. Bracketed/unbracketed scrolling, clipped blocks, return, replacement, and retirement fixtures pass.

## Recorded for 0.4.3
- R1 (none): a soft-wrapped inline record can return only after the stability interval. Temporary residency probe: alternate 100x12, source `"$" + "x+" repeated 49 times + "x$"`; draw at CUP 2;1, prove, repaint away at 300 ms, clear/home and return at CUP 4;1 at 600 ms, cursor at 9;1. Synthetic raster residency is 6,400 B initially, 0 immediately on return, 6,400 after stability/completion. Same result with c425e400 and its parent's session code; the grid changes on both draws, so this is not an identical-answer memo skip, and no retirement verdict occurs. No mismatched publication or headline `$$` failure demonstrated; stability-interval behavior is non-blocking under the supplied bar. Probe removed after comparison.

## Validation and measured budgets
All four permitted commands used `-j 4`: repaint_flash_oracle 30/30; lifecycle_matrix 46/46; lib detection 11/11; lib repaint 13/13.
Additional temporary probe runs are described above; the immediate-parent oracle also fails the expected round-6 fence fixture. Reviewed source/test files restored exactly; only this report is committed.
Per-cycle allocations / bytes, measured from lifecycle_matrix output:
- G1_TUI_REPAINT 120x40: 16 / 21,304.
- G1_TUI_REPAINT 120x80: 16 / 21,504.
- G1_CARRIED_REPAINT 120x16, off_band=false: 644 / 566,946.
- G1_CARRIED_REPAINT 120x16, off_band=true: 1,011 / 759,475.
- G1_IDENTICAL_REPAINT 120x16: 636 / 398,730.
- G1_RESTORED_REPAINT 100x40, blocks=1: 2,974 / 1,989,859.
- G1_RESTORED_REPAINT 100x40, blocks=8: 3,467 / 2,199,925.
All seven match the claims. No heavier cargo command, application launch, or process termination performed.
