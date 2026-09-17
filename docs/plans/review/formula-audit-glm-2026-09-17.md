# Formula pipeline audit — third pass (geometry) — 2026-09-17

STATUS: COMPLETE

Worktree math-audit-glm, detached at d1db4abd. Read-only audit of the LaTeX
pipeline across bt-detect / bt-term / bt-viewport / bt-math / bt-app / bt-render,
focused on areas 2 (resize and re-wrap), 3 (the frozen/live seam) and 6 (layout
accounting and caches). One finding (F1); the other areas are sound as noted.
K1-K6 (being fixed elsewhere) are excluded and not restated.

## F1 — a boundary-split block blanks its frozen source rows without painting over them

- ID: F1
- Severity: high
- Area: 3 (the frozen/live seam)
- Files: crates/bt-term/src/session.rs:7312-7313, crates/bt-term/src/session.rs:10992-11001,
  crates/bt-viewport/src/lib.rs:2981, 2988-2994, 3375-3382, 3383-3411, 3484-3496, 4234

Sequence:

1. A display `$$` block taller than the room above it freezes its opener rows
   into history while the closer is still on the live grid.
   `apply_live_worker_completion` installs the live record with
   `frozen_prefix: frozen_prefix_ids(&task.span)` and `staging_prefix: Vec::new()`
   (session.rs:7312-7313) — the staging prefix is empty on this path no matter
   what the staging plane holds.
2. In the viewport's bridge loop the prefix is validated (contiguous history
   tail, no rendered artifact on it) and filed:
   `frozen_prefix_geometry.insert(live_math.occurrence_id, abs_top);`
   (lib.rs:2981) — inserted *before* the staging check.
3. The staging check then runs:
   `if staged_rows.iter().map(|row| row.id).ne(live_math.staging_prefix.iter().copied())`
   → `continue` (lib.rs:2988-2994). Because `staging_prefix` is empty, equality
   requires the staging plane to be completely empty. Any staged row — an
   unrelated in-progress logical line, or the block's own rows in transit to
   history — leaves the occurrence with **no** `bridge_geometry` entry while its
   `frozen_prefix_geometry` entry stands.
4. The placement loop's else-branch still fires for that occurrence:
   `if let Some(&abs_top) = frozen_prefix_geometry.get(&live_math.occurrence_id)`
   → `bridge_prefix_blank.push((abs_top, history_rows));` (lib.rs:3375-3382),
   but the placement it builds covers only the live closer band —
   `band_height` from `live_row_prefix[block_last + 1] - live_row_prefix[block_first]`,
   `frozen_rows = 0`, `(top, band_height, content_offset, 0)` (lib.rs:3383-3411).
5. The blanking loop then suppresses every cell in `[abs_top, history_rows)`:
   `for (abs_top, abs_end) in bridge_prefix_blank.drain(..) { … suppress_math_source_cell }`
   (lib.rs:3484-3496), and `sync_live_math_artifacts` skips row-height expansion
   for the same record (`if !artifact.frozen_prefix.is_empty() { continue; }`,
   lib.rs:4234), so the band is not grown to cover the raster either.
6. `retire_stale_bridge_prefixes` only clears the prefix when it stops being the
   history tail (session.rs:10992-11001). Here it still is the tail, so nothing
   retires the record; the state lasts as long as the staging plane is non-empty.

What the user sees: the upper rows of the formula — its frozen source prefix —
go blank with nothing drawn over them, while the closer band at the bottom shows
a vertically centered, band-clipped slice of the whole-formula raster. During
agent streaming this reads as the formula tearing itself apart: blank gap, then
a rendered bottom fragment. When the staged rows finalize, staging empties, the
clean bridge returns and the block re-renders whole.

Distinct from K3: K3 is the *clean-bridge* branch sizing the combined band by
source rows and clipping it (the `bridge_geometry.get` arm, lib.rs:3336-3373).
F1 is the *no-bridge* arm: rows are blanked that no placement covers at all.
The branch's own comment states the intent — "keep that line visible while
still swallowing the occurrence's proven frozen source prefix" (lib.rs:3377-3380)
— but the swallow leaves nothing standing over the prefix.

Reachability: the code path is unconditional once "bridged record displayed +
staging non-empty" holds — that conjunction is asserted by the viewport's own
comments (lib.rs:2984-2987, 3377-3380 name the unrelated-staged-line case
explicitly, and the staging-equality gate exists only because staging can hold
rows that are not the occurrence's). What I could not prove from code alone is
the *duration* of the non-empty-staging window in a live session (it depends on
freeze-candidate finalization latency in bt-transcript, which I did not finish
tracing); the geometry defect itself is proven. Marked PLAUSIBLE only in
duration, not in existence.

Smallest correct general fix: honor the bridge loop's own stated rule —
"anything else is not a clean boundary split and is left to render as source"
(lib.rs:2946-2947) — in the placement loop too. In the live-placement else-arm,
when the occurrence owns rows outside the live band (`frozen_prefix_geometry`
has its id, or `staging_prefix` is non-empty) and `bridge_geometry` has no entry
for it, emit no rendered placement and push nothing to `bridge_prefix_blank`:
the occurrence renders as source until the split is clean again. One condition,
no heuristics; it covers both the unrelated-staged-line case and the block's
own rows transiting staging with an empty `staging_prefix`.

Pinning test: in the bt-viewport bridge tests (beside the existing
boundary-split tests): a live math artifact whose `frozen_prefix` matches the
history tail, with one unrelated staged row between history and the live band;
project a frame and assert (a) no `MathBlockPlacement` with
`display: Rendered` exists for that occurrence, and (b) the prefix history
rows' cells are not suppressed (their text survives). Name in repo style:
`an_unclean_boundary_split_renders_the_whole_occurrence_as_source`.

## Areas examined and sound

1. Scheduling vs completion (both planes). Live completions are gated on
   screen + grid_generation + detection_revision plus `live_task_is_current`
   (session.rs:13545), which re-derives the dependency rows and re-resolves the
   span, so scroll-in-flight, resize-between-schedule-and-completion
   (`invalidate_layout` session.rs:11005; `worker_task_is_current` 7488) and
   text-overwritten-under-flight all drop the stale result; placements are
   recomputed from current state every frame. Results route by task id to the
   owning session (bt-app apply_math_results, main.rs:82492) so duplicates and
   out-of-order arrivals replace rather than stack. Sound. (Worker-process
   death and respawn were not traced; no defect claimed.)
2. Resize and re-wrap. Display width shrink is fitted, not clipped:
   `MathBand::fit_scale_milli` (session.rs:14421) / `math_fit_scale_milli`
   (14461) shrink toward the band with a 500-milli readable floor (14448) and
   leave the remainder to the horizontal offset; the factor is recomputed from
   the current pane width every frame, so growing back restores the scale with
   no stale state, and inline math owns no band by design (14468). Width/DPI/
   font/theme changes re-key rasters end to end via LayoutKey (bt-doc
   versions.rs) and MathRenderKey (bt-math lib.rs:194-200); resize retires and
   re-arms after quiescence (`finish_resize_if_quiescent` session.rs:3473), and
   the stale-artifact path shows the old raster scaled during relayout. Sound.
3. The frozen/live seam (beyond F1). Selection and copy read source rows —
   history + staging + live (`copyable_rows` session.rs:7787-7827,
   `selection_text` 7916-7946) — never presented cells, so the rendered face
   cannot strip the clipboard and copy across the seam stays ordered. Show-
   source is symmetric: `sync_live_projection_artifacts` filters `show_source`
   records out of the viewport's artifact list (session.rs:8385), so the source
   face leaves both prefix and band as text with nothing blanked. Scrolling
   while bridged recomputes bridge geometry per frame, with exact prefix sums
   in window and uniform-cell extrapolation above it (lib.rs:3346-3362).
   Sound, F1 aside. (Block finishing freeze while displayed is K1/K2.)
4. Alternate screen and repaint. Repaint protection closes with reprojection
   (`finish_alternate_repaint` session.rs:5958, `finish_primary_repaint` 6237);
   decorations are keyed by screen, and CSI 2J/3J/reset map to distinct adapter
   facts (`drain_transcript_events`, bt-term adapter.rs:1359-1383). Sound.
5. Eligibility gates. Detection reads SGR-free logical text (runs built from
   plain source, bt-detect lib.rs:700-714; `dollar_census` 850 is a byte scan);
   the across-row join relaxes no gate — lone unescaped opener, first-dollar
   closer within 48 cells (lib.rs:731, 771-835), same eligible site, and the
   joined source faces every one-row gate again. Columns are measured wide-
   aware in the bounded closing walk. Absent/partial shell integration and
   never-closed C regions decline arming, leaving text as text — conservative
   by design. Sound.
6. Layout accounting and caches. Scroll anchoring re-derives the window from a
   content anchor every frame (lib.rs:2812-2846), so a band growing or
   shrinking above the viewport moves the anchor's absolute position instead
   of the view, and a vanished anchor falls back through the displacement
   path (2830-2844); `move_artifact_bands` (4080-4119) updates both height
   trees in place or bails to a full rebuild. The measured-layout cache keys
   on span + source generation + detection revision + layout + artifact height
   (lib.rs:4557-4569), so width, zoom, theme and DPI staleness cannot survive.
   `rows_above` (2928-2934) and `scroll_offset_rows` (2274-2282) divide the
   same subpixel extent by the same uniform cell height the scrollbar extent
   uses, so badge and thumb agree by construction. Sound.
7. Resource and failure paths. Rasterization refuses width > 131072, height >
   16384 and bytes > MAX_RASTER_BYTES = 64 MiB (bt-math lib.rs:843, 861, 99),
   and every MathRenderError leaves the source standing (failure_reason gates
   suppression, session.rs:8988/9080) — an untypesettable formula never goes
   blank. The renderer re-attempts texture upload each frame on a cache miss
   (bt-render lib.rs:10102-10227), and the LRU refuses only an artifact larger
   than the whole budget, which the rasterizer's equal cap already prevents.
   The macro engine's budget proof bounds expansion before it runs
   (bt-math macro_budget.rs:33, 101-273). Sound.

## Ranked summary

1. F1 (high, area 3) — an unclean boundary split blanks a bridged formula's
   frozen prefix while rendering only a clipped slice over the closer band;
   fix: render the occurrence as source whenever its prefix exists but the
   bridge geometry does not (lib.rs:3375-3382).

STATUS COMPLETE
