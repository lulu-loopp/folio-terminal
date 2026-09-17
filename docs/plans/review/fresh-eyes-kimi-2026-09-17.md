# Fresh-eyes review: live math decorations at 2b42eab1

Whole-subsystem read (no diff review). Bars: (A) on-screen unchanged source renders every frame;
(B) no picture over mismatched text; (C) changed source re-detected; (D) ordinary output pays nothing.
Line refs are `crates/bt-term/src/session.rs` unless noted.

## 1. Record lifecycle census and bar-A audit

Create: `apply_live_worker_completion` (7736), `restore_offscreen_decorations` (7027),
`finish_alternate_repaint` (6316), `finish_primary_repaint` (6633), `preserve_live_after_top_scroll`
(10892). Move/demote: `retain_offscreen_record` (6801, live→off-band), `invalidate_live_row` (7166),
artifact→stale demotion at 11502/11541 (`invalidate_layout`), 6988 (restore), 6583 (finish_primary),
`RecordProjection::Dormant` (clipped rows), `pending_live_handoffs` (live→frozen carry, 11114).
Destroy: screen switch (7060), `ParkPrimary`/`RestorePrimary` (9993/10007), `invalidate_all` (7201),
off-band cap eviction (6811), quiescence retain-filter (3737), `retire_offscreen_records_replaced_by_frozen`
(6758), `retire_stale_bridge_prefixes` (11447), semantic-input reconcile (5954), replace-by-newer (7718),
error paths (3049, 3527), `redetect` (8032), theme drop (11508).

## Defects demonstrable from the code

**D1 — alt-screen park/restore destroys records whose source comes back unchanged. (Bar A, high.)**
`apply_events`: `ParkPrimary` (9991-10004) and `RestorePrimary` (10005-10025) both call
`invalidate_all_live_decorations()` and reset every `live_rows` entry; no census of the parked screen
is kept (nothing like `snapshot_alternate_repaint` exists for the parked primary grid). The vendored
grid under the alternate screen is byte-identical when `?1049l` returns to it. Trigger: a typeset
block on screen, run `vim`/`less`/`htop`/Claude Code, quit. Result: the block shows LaTeX source for
LIVE_MATH_STABLE_INTERVAL (200 ms, :64) + scan + raster on every full-screen exit. Recovery works
(vendor `mark_fully_damaged` on restore re-arms all rows), so it is a flash, not a stall — but it is
a systematic bar-A violation the branch never touched.

**D2 — off-band cap eviction drops live-source records, and can collapse preservation. (Bar A, medium.)**
`retain_offscreen_record` (6810-6812) `pop_front`s the oldest record at MAX_OFFSCREEN_RECORDS (128, :69)
regardless of whether its source is on screen. Worse: evicting the last stale-pending record flips
`has_pending_resize_relayout` (6712) false, which turns `offscreen_preservation_active` off mid-gesture,
so the *next* damage takes the `else` at 6807 and silently discards live-artifact records too.
Trigger: >128 distinct occurrences churned off-band in one repaint storm (alternate retains the whole
queue across repaints, so accumulation is realistic on a fast-scrolling TUI full of small blocks).

**D3 — duplicate identical blocks can never be re-anchored off-band. (Bar A transient, high.)**
`exact_live_source_match` (12985) requires the source to be *unique* on the grid (13024-13028: a second
match → None). Two byte-identical formulas on screen, then any non-window invalidation (scroll-region
DL/SU on alternate without a boundary chunk): both records drain off-band, both fail uniqueness, both
sit at source until re-detection. The fingerprint reprojection only runs inside repaint windows; the
plain `restore_offscreen_decorations` door has no disambiguation (e.g. by proximity to old coordinates).

**D4 — theme change shows source for unchanged blocks. (Bar A, low-medium, deliberate.)**
`invalidate_layout(theme_changed=true)` drops both `artifact` and `stale_artifact` (11508-11510,
11547-11549): source renders until the relayout lands. Justified in comments (mixed-theme frame), but
it is a bar-A exception the owner’s bars do not grant; geometry-only changes are handled (stale raster
bridges), theme is not.

**D5 — any SessionError mid-feed wipes everything. (Bar A, low.)**
`feed_at` (3040-3049) and `finish_synchronized_update` (3520-3527) clear all records + off-band queue on
error, source unchanged. Rare, but indiscriminate.

**D6 — ordinary output pays a full-grid text rebuild per published frame. (Bar D, medium-high.)**
Every published frame calls `schedule_visible_artifacts` (bt-app/src/main.rs:66616) →
`schedule_live_artifacts` (8125) → `live_detection_context` (3910, 5990-6053), which clones every visible
row’s text into fresh Strings (`captured_row_logical_text_and_boundaries`, 14415) plus a history tail —
with zero math anywhere on screen. "Pays nothing" holds per output byte, not per frame.

## 2. Programs that repaint without the boundary heuristic

`contains_clear_home_snapshot_boundary` (14591) matches only `?2026h`, `2J`+later-home, or home-within-8-bytes
+ ≥3 `EL`. Misses, and what happens (all reasoned from code):

- **vim/neovim scroll regions, less, tmux pane scroll** (`CSI r` + `CSI S/T`/IL/DL, row diffs): no window.
  Vendor reports Full damage; the fingerprint gate (7100) spares unchanged rows, changed rows invalidate
  (7130) → alternate retains off-band (7184, 6662) → `restore_offscreen_decorations` at settle (3151)
  re-anchors by exact source + `detector_owns_live_match` (6855). Survives — except D3 duplicates.
- **htop** (home + rewrite rows with EL): matches arm 3 only if home and ≥3 ELs land in *one PTY read*;
  a frame that OS-splits, or redraws <3 EL’d rows, misses and falls to the per-row path above.
- **lazygit/tcell-style diffed frames, CUP+text+EL-only TUIs**: no boundary ever; per-row path; unchanged
  formula rows keep fingerprints and are never touched (good); primary-screen instances (rare — most are
  alternate) drop to source and re-detect, since preservation is off on primary outside resize/reprint.
- **Codex CLI**: covered — this is the branch’s target (primary reprint window).
- **`?1049h` re-entry**: D1.
- Residual of the original defect: a non-2026 repaint split across two reads closes its window at the
  first settle (`settled = deadline.is_none()`, 3130-3146), reprojects against a half-painted grid, and
  publishes the inter-drain frame with straddled records off-band → source flash. Only `?2026h` spans reads.

## 3. Grid mutation vs. reconciliation before publication

All mutations reconcile before a frame: `feed_at` per segment (3027-3029) + `settle_feed_turn` (3110);
`finish_synchronized_update` (3515-3542); `resize_at` (damage deliberately discarded at 3347, reconciled
by snapshot/finish/restore/rebase 3340-3427); `settle_resize_transaction` (no damage; 3665-3705);
park/restore/RIS/clear via `apply_events`; `CSI r/S/T`/IL/DL via vendor Full damage + fingerprint gate.
No publication-without-reconciliation path found. The load-bearing assumption — vendor damage is complete
and the u64 fingerprint (cell_capture.rs:22) never collides across different content — is asserted nowhere;
a collision or a missed `mark_fully_damaged` breaks bar B silently.

## 4. State-space complexity

≥10 coordinated states per occurrence: resident / off-band(dormant) / two snapshot censuses /
stale-artifact-pending / live→frozen handoff / presentation-hold ledger / frozen+staging prefixes /
clipped rows / per-row candidate_signature / show_source. Smells:
- Two truths for "window open": `primary_repaint_in_progress` (flag, 2984) vs
  `primary_repaint_snapshot.is_some()`; suppression keys on the snapshot (7122), preservation on the flag
  (6679→6665). Flag true + snapshot None is reachable (2992-2994, no decorations at open). Same duality on
  alternate (2972, 3345, 3370).
- "A record lives in exactly one of live/offscreen/snapshot" (comment 2989) has no debug_assert or test;
  close paths de-dup by occurrence_id (6212-6223, 6530-6531) as a workaround.
- `bt-detect/src/lib.rs:1952` and `:2424`: `expect()` on scanner invariants held by convention; a worker
  panic, not a compile error, if they drift.
- `live_content_revision` is bumped only in `observe_live_damage` (7111); any future mutation path that
  forgets damage silently outlives open windows.

## 5. adapter.rs XTVERSION segmentation

No defect for ordinary output. `advance_terminal_bytes` (adapter.rs:986-1010) cuts only at a completed
`CSI > q` or a block-ending ESU with a reply owed; with `xtversion_replies_owed == 0` it is one
`processor.advance` over the whole slice — the pre-change chunking. Splits are exact partitions (no
drop/reorder/duplication). Fragility only: `advance_parsers_through_any_overflow` (1041) hard-couples to
the vendored vte `SYNC_BUFFER_SIZE - 1` give-up rule; a vendor change misplaces replies silently.

Areas checked with nothing found: IL/DL do not reach `invalidate_all` (rows classify `Ignore`,
lifecycle.rs:97-103; apply_removed_rows’ invalidate at 10839 is only resize-cause removals outside an
epoch); live→frozen handoff and bridge-prefix retirement are sound as designed; focus/`Behind` panes
re-sync projection before publish (bt-app main.rs:103119-103129).

STATUS: COMPLETE
