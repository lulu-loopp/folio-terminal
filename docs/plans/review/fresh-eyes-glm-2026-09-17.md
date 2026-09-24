# Fresh-eyes review: live math decoration subsystem at 74baff25

Reviewer: GLM 5.3, independent, design-level (not a diff review).
Scope: `crates/bt-term/src/session.rs`, `crates/bt-detect/src/lib.rs`,
`crates/bt-term/src/adapter.rs`. Read-only.

## 1. Record lifecycle census and bar-(A) verdicts

Creation: `apply_live_worker_completion` session.rs:7736; `restore_offscreen_decorations`
:7027; `finish_alternate_repaint` :6316; `finish_primary_repaint` :6633;
`preserve_live_after_top_scroll` :10903. Demotion/move: `invalidate_live_row`
:7166 (from `observe_live_damage` :7130); `retain_offscreen_record` :6801;
`retain_live_decorations_offscreen` :6819; `invalidate_layout` :11485
(artifact→stale); off-band cap eviction :6810. Destruction: `invalidate_all_live_
decorations` :7201 (callers: :3049 parse error, :3352 resize_at, :3527 ESU error,
:7060 screen switch, :8032 redetect, :10752 rebase_vendor_owned_rows, :10839
apply_removed_rows, :9993/:10007 park/restore); worker-completion removals
:7671/:7677/:7682/:7686/:7718; `reconcile_decorations_against_semantic_input`
:5954; `retire_stale_bridge_prefixes` :11447; `retire_offscreen_records_replaced_
by_frozen` :6758. Face-only mutators (toggle/hover/scroll) :8549-:8569.

Bar-(A) violations still open, beyond what the branch fixed:

* **D1 (defect, high).** `apply_removed_rows` :10835-10839: any `RowsRemoved`
  that is not `NormalScroll`+`FullScreen` — i.e. every DL (`CSI M`,
  `ScrollOutCause::DeleteLines`, vendor term/mod.rs:1357) and every
  partial-scope scroll — calls `invalidate_all_live_decorations()`. On the
  alternate screen with no window open that clears `live_decorations` AND
  `offscreen_decorations` (:7212-7215): a total, unpreserved wipe of every
  formula on screen, including rows the scroll never touched. Recovery is full
  re-detection ≥ `LIVE_MATH_STABLE_INTERVAL` (200 ms) + worker + raster later.
  Trigger: vim/neovim scrolling via `csr` region + DL on the alternate screen;
  a `CSI r`-narrowed region scrolling on primary. Broken bar: (A).
* **D2 (defect, medium-high).** Primary idle has no preservation at all:
  `offscreen_preservation_active` :6662 is alternate-only outside resize/reprint
  epochs, so `invalidate_live_row` :7184-7190 drops rendered records outright.
  Any window-less primary repaint/scroll that moves a formula's rows (IL, RI,
  `CSI S`/`CSI T`, TUI diff redraw) flashes; only the exact-source scroll
  (`LF` at bottom) is protected via `preserve_live_after_top_scroll`.
* **D3 (design decision, documented at :11372-11379, still a bar-(A) breach).**
  Park/restore (`?1049h`/`l`, :9993/:10007) destroys every live record and
  resets `live_rows`; the restored primary grid re-detects only after a fresh
  200 ms stability window. Leaving a full-screen program flashes every formula
  on the primary screen for ~200 ms + detection latency.
* **D4 (fragility, medium).** A record that enters `offscreen_decorations` and
  fails exact-source re-anchor stays there forever (no TTL, no quiescence gate;
  eviction only at 128 cap or screen switch). Its source can be on screen and
  unchanged yet non-unique (duplicate formula), leaving it unpainted — bar (A)
  by the letter — and it costs a full-grid context build + prefix walk +
  substring scan + detector re-run per record **per feed turn**
  (`restore_offscreen_decorations` :6922 runs unconditionally from
  `settle_feed_turn` :3151). This is also the subsystem's main bar-(D) leak:
  under Claude Code's repaint-per-keystroke with one dormant record the cost
  is paid every keystroke.
* **D5 (defect, medium).** Mid-window destructions are un-done at window
  close. `reconcile_decorations_against_semantic_input` :5954,
  `retire_stale_bridge_prefixes` :11447, and worker refusals :7671/:7718
  plain-`remove()` records while an open `alternate_repaint_snapshot` /
  `primary_repaint_snapshot` still holds their clones; `finish_*_repaint`
  projects the clone back (the occurrence-id/generation guards :6518-:6546
  cover only the top-scroll reproof). A record the detector just refused, or
  whose rows the shell just claimed as input, can be resurrected over text it
  must not cover — bar (B) (policy side) hazard, needing unchanged rows and a
  succeeding projection, hence medium.

## 2. Repaints that do not match the boundary heuristic

`contains_clear_home_snapshot_boundary` :14591 requires `?2026h` anywhere,
`2J`+home in the same chunk, or home in the first ~10 bytes + ≥3 `EL`.

* Claude Code scroll (2026): window — fixed by the branch.
* **vim/neovim DL scroll: worst case — D1 total wipe** (no window, and the
  removal path bypasses off-band). Additionally `alternate_detection_context`
  is advanced only in the NormalScroll+FullScreen arm (:9915-9923), so a DL
  scroll that removes fence-bearing rows leaves a stale prefix state: formulas
  can be refused (stuck at source) or mis-contexted until a boundary repaint
  resets it (:3013). Bar (A)/(B) tail risk.
* vim RI/IL/`CSI S`/`CSI T`: vendor emits only `GridScrolled` + full damage;
  no `RowsRemoved`, so records survive via fingerprint-skip for unchanged rows
  plus off-band + exact-source restore at settle — **safe on alternate,
  destructive on primary (D2)**.
* less: bottom LF scroll is the preserved exact-source path; refresh repaints
  carry `2J`+home — covered.
* htop/lazygit/tmux/ratatui diff redraws (CUP + text + `EL` per changed row,
  no home): no window, but unchanged rows pass the fingerprint equality
  (:7100) and changed-row records go off-band + restore on alternate. Survives
  by row damage + exact re-anchor, not by the window — acceptable on alternate,
  flashes on primary (D2).
* `?1049h` re-entry: park wipes (D3).
* Split-read fragility: the heuristic is evaluated per feed chunk; a repaint
  whose `2J` and `H` land in different PTY reads, or whose home is preceded by
  >~10 bytes of SGR, opens no window. On alternate the damage fallback absorbs
  it; on primary it does not (D2).

## 3. Grid mutations vs. reconciliation points

`feed_at` chunk loop (damage observed per chunk, settle closes windows +
restores) ✓; `finish_synchronized_update` :3533 (damage observed with
suppression still armed) ✓; `resize_at` (damage discarded :3347; records
reconciled via snapshot/invalidate/restore + `rebase_open_repaint_windows`
:3451) ✓; `settle_resize_transaction` canonical install :3647 (records
reconciled via pre-install snapshots :3621-:3632) ✓; park/restore reconciled by
wholesale destruction (by design) ✓; RIS and `clear_screen_keeping_cursor_row`
flow through feed damage ✓. One gap, fragility not defect: the reconcile
install moves rows without any `take_damage`, so `live_rows` fingerprints and
`live_content_revision` go stale; correctness is rescued only because the
candidate context signature (:13148) hashes every structural row, so reflowed
rows change the signature and re-ask. Nothing asserts or documents that this
is the load-bearing coupling.

## 4. State census and unasserted invariants

I count ~15 distinct hold-ish states: alternate window (snapshot + mirror
flag), primary window (flag + snapshot + dirty bit — three fields),
`primary_resize_preservation_active`, `primary_repaint_active`,
`offscreen_preservation_active`, off-band queue ("dormant"/"off-band"/"held"/
"preserved" — four names, one VecDeque), stale-pending (artifact XOR stale),
`show_source` face, clipped top/bottom rows, frozen/staging prefix,
`pending_live_handoffs`, `primary_reprint_hold_occurrences` + history floor,
`alternate_content_end_row`, `primary_parked`, row-level `LiveRowStability`
(5 fields), `RecordProjection::Visible`/`Dormant`, resize epoch + canonical
fork.

Overlaps: `alternate_repaint_in_progress` duplicates
`alternate_repaint_snapshot.is_some()` and they are kept in step by hand at
six call sites (:2972, :3148, :3167, :3345/:3370, :3496, :3541) — the exact
two-structures-one-fact smell the code's own comment (:6205-6210) criticizes;
`observe_live_damage` keys suppression on the snapshot (:7119/:7122) while
off-band retention keys on the flag (:6662-6665), so a disagreement produces
retained-but-unpainted records. `primary_repaint_in_progress=true` with
`snapshot=None` is a legitimate armed state, muddying "window open".

Invariants relied on but asserted nowhere (no debug_assert, no test found):
record lives in exactly one of `live_decorations`/`offscreen_decorations`
(:2989); flag⟺snapshot agreement; `artifact` XOR `stale_artifact`;
`band_start_row <= start.row <= end.row <= band_end_row`; off-band is never
painted; a record in an open snapshot is still authoritative unless carried or
reproven (broken by D5).

## 5. XTVERSION feed segmenting vs. ordinary output

No semantic change for ordinary output found: both cuts are gated on
`xtversion_replies_owed > 0` (adapter.rs:995-996, :1042), a query-free feed is
one segment (:971-975), bells keep their old ordering (:1069-1085), and the
canonical resize fork sees identical segments (:1071-1075). Costs/fragilities:
the per-byte `observe_parser_boundary_byte` pass runs on every feed (the
comment says it predates the branch — it is nonetheless now the hot path);
`advance_parsers_through_any_overflow` hard-couples to `VENDOR_SYNC_BUFFER_SIZE`
arithmetic and the vendor's give-up rule (:1050-1055) — if the vendored
constant or rule drifts, `trips_at == 0` silently degrades to the old
wrong-order behaviour rather than failing loudly; the reply produced by the
single tripping byte is asserted to be "one of the block's own" by reasoning
alone (:1034-1037), with no test I could find for that interleaving.

## Summary

The branch's own fixes are coherent, but the *fallback ladder* outside the
window is asymmetric by design — alternate always preserves off-band, primary
preserves only inside resize/reprint epochs — and the `RowsRemoved`
classification (`NormalScroll`+`FullScreen` or nothing) sits above that split:
DL and partial-scope scrolls take the "nothing" arm and wipe both screens
(D1/D2). Park/restore (D3) and the unbounded per-feed off-band retry (D4) are
the remaining bar-(A)/(D) exposures; mid-window resurrection (D5) is the
remaining bar-(B) exposure. Nothing found in the XTVERSION segmenting that
touches ordinary output.

STATUS: COMPLETE
