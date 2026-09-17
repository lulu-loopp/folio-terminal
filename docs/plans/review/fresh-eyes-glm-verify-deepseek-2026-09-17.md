STATUS: COMPLETE
# Verification of fresh-eyes-glm-2026-09-17 (D1–D5, S2–S4) at 74baff25
Independent read-only verification of the GLM 5.3 design review. Each claim is CONFIRMED /
PARTLY / REFUTED with the code path at 74baff25, a concrete trigger, what the user sees, how
common it is, pre-existing-vs-introduced (vs main e0f682b8), and the smallest general fix.
Baseline (symbol census vs e0f682b8 + `git show e0f682b8` diff): `apply_removed_rows`,
`preserve_full_screen_scroll`, `invalidate_all_live_decorations`, `offscreen_preservation_active`,
`invalidate_live_row`, `restore_offscreen_decorations`, `settle_feed_turn`, `finish_alternate_repaint`,
`finish_primary_repaint`, `alternate_detection_context` (+ its advance in the NormalScroll+FullScreen
arm and the `contains_clear_home_snapshot_boundary` reset), `alternate_repaint_snapshot/_in_progress`,
`primary_repaint_in_progress` all exist on main with the same gating. The ONLY production logic this
branch introduced is `carried = std::mem::take(...)` + occurrence-id dedup in `finish_*_repaint` (:6211).
Therefore D1–D4, S2, S3 are PRE-EXISTING; S4 and D5's merge path are the branch-touched spots.
## D1 — CONFIRMED (pre-existing), high
Path: `apply_removed_rows` session.rs:10838–10840 `if !preserve_full_screen_scroll { invalidate_all_live_decorations() }`
→ :7212–7215 clears `live_decorations` AND `offscreen_decorations` (off-band kept only when
alternate && `alternate_repaint_in_progress`). Gate :9910–9932: preserve only for `cause==NormalScroll &&
scope==FullScreen && count!=0`.
Triggers: (a) DL `CSI M` → `ScrollOutCause::DeleteLines` (vendor term/mod.rs:1357–1359; adapter.rs:1849), never
preserved; (b) region-LF whose region top is NOT row 0 → `Partial` (term/mod.rs:1349–1353: `scope = if origin
== Line(0) { FullScreen } else { Partial }`). adapter.rs:1976–2009 proves both. CORRECTION to the review:
"every `CSI r` user / status-line TUI" overstates — a bottom status line is `CSI 1;N-1r`, top-anchored
(origin==Line(0)) → FullScreen → PRESERVED. Only a top header above a scrolling region (`CSI 2;Nr`) hits Partial.
DL is the reliable common trigger, not region-LF.
Sees: every formula on screen (rows the scroll never touched included) → LaTeX source for ≥200 ms
(`LIVE_MATH_STABLE_INTERVAL` :64) + worker + raster. Common: vim/nvim `dd`/`o`/`p` on alt (LaTeX in vim plausible).
Fix: in the `!preserve` arm, invalidate only the removed rows and drain survivors off-band (fingerprint
re-anchor), keeping the Resize-cause wipe. Blast radius: the wipe arm is shared by parse-error/screen-switch/
redetect — the change must branch on cause, not replace the arm wholesale.

## D2 — CONFIRMED (pre-existing), medium-high
Path: `offscreen_preservation_active` :6662–6666 = alternate || resize || reprint; primary idle → false →
`invalidate_live_row` :7184–7190 counts the record invalidated and DROPS it (no off-band retain).
Trigger: any window-less primary repaint that rewrites a formula's row — IL `CSI L`, RI, `CSI S`/`CSI T`
(ExplicitScreen → `GridScrolled` + full damage, no RowsRemoved; adapter.rs:2012–2034), or a TUI diff redraw
(CUP+EL). Only exact-source bottom LF is protected (`preserve_live_after_top_scroll`).
Sees: ≥200 ms source flash on primary. Common: htop/lazygit/tmux/less-refresh on the primary screen.
Fix: extend off-band retention to primary idle (drain + re-anchor), or targeted row invalidation instead of drop.
Blast radius: primary is the default screen; always-preserve off-band changes memory profile — pair with D4's TTL.

## D3 — CONFIRMED (pre-existing, documented design)
Path: `ParkPrimary` :9991–10004 and `RestorePrimary` :10005–10024 both call `invalidate_all_live_decorations()`
and reset every `live_rows` entry to default. Leaving a full-screen program re-detects after a fresh 200 ms window.
Sees: ~200 ms + detection source flash when leaving vim/htop. Common: every alternate-screen exit.
Fix: none cheap — park changes screen identity, so a preserve requires parking records keyed off-grid (design work).
Not a blocker for the DEC-2026 scroll headline (different interaction).

## D4 — CONFIRMED (pre-existing), medium; one correction
`restore_offscreen_decorations` :6922 is called from `settle_feed_turn` :3151 every feed turn; its only early-out
is `offscreen_decorations.is_empty()` :6923–6925 (no content-revision gate). A record failing `exact_live_source_match`
:6938, the fence prefilter :6944, `detector_owns_live_match` :6951, or `rebase_identity_onto_match` :6967 is re-queued
(`remaining`) and retried EVERY turn. Per-turn cost = full `live_detection_context()` :6926 + prefix walk :6928 +
substring + detector re-run. Bounds: 128 cap (`retain_offscreen_record` :6810), screen switch :7213–7215, frozen
retirement :6758 — no TTL/quiescence. CORRECTION: "runs unconditionally" is true only while the queue is non-empty.
74baff25 adds an allocation-budget TEST, not a runtime TTL.
Sees: wasted work only (bar-D), plus a narrow bar-A — a non-unique/duplicate formula whose unchanged source is on
screen can never re-anchor and stays source. Common trigger: one dormant record under Claude Code's repaint-per-keystroke.
Fix: TTL/quiescence (drop or back off a record that fails re-anchor N times, or skip when `live_content_revision`
unchanged). Blast radius: restore path only; must not drop records whose source merely moved (frozen retirement covers that).

## D5 — CONFIRMED mechanism (pre-existing, UNCHANGED by branch), medium
Mid-window removals mutate only `live_decorations`, leaving the snapshot clone: `reconcile_decorations_against_semantic_input`
:5954 (plain `remove`), `retire_stale_bridge_prefixes` :11447, worker refusals :7671–7672/:7677/:7682/:7686/:7718–7719.
At close `finish_alternate_repaint` :6211–6228 / `finish_primary_repaint` :6547–6555 project the clone back. The
occurrence-id/generation guard :6518–6546 covers only the top-scroll reproof, not these removals. QUALIFIER: projection
is `project_live_record` with a per-row fingerprint proof (`projected_exact_source_row_support` :13812–13832, boundary-only
mismatch), so a resurrected record CANNOT cover CHANGED text — it re-covers only UNCHANGED text removed for a policy
reason (a shell input-claim on an untyped prompt, or a refusal), i.e. a raster over the command line. "Carried first,
dedup by occurrence_id" is UNCHANGED here (a removed record is not in `carried`, so it is not deduped); it only fixes
the separate proven-during-window loss. Pre-existing (main's `finish_alternate_repaint` also projected snapshot clones).
Sees: a formula raster briefly over the prompt (bar-B). Common: primary Codex reprint + input-claim; alt = worker refusal (narrow).
Fix: keep a retired/refused occurrence-id tombstone during the window and drop its snapshot clone at close.
Blast radius: small (finish_*_repaint only).

## Section 2 — CONFIRMED mechanism (pre-existing), tail risk
`advance_detection_context` runs only in the NormalScroll+FullScreen arm :9915–9923; DL/Partial scrolls neither advance
nor reset `alternate_detection_context`; the only reset is `contains_clear_home_snapshot_boundary` :3013–3015. A DL that
deletes a fence row leaves stale prefix state until the next boundary. Sees: a formula refused/mis-contexted (bar-A/B tail).
Common: needs a fence + DL coincidence. Pre-existing (main has the same arm and reset). Fix: subsumed by D1's fix
(context must track the cause that is changing it).

## Section 3 — CONFIRMED as fragility (reviewer's own "not defect")
`settle_resize_transaction` install :3647–3648 replaces the grid with no `take_damage`; re-detection of reflowed rows
relies on `grid_generation` bump :3650 + `live_detection_context_signature` :13148–13159 hashing every structural row,
with nothing at the install site asserting/documenting that coupling. No user-visible bug found. Pre-existing structure
(89b9e23f only adds the re-seat, not the coupling). Fix: a comment + debug_assert tying the generation bump to the
signature-driven re-arm. Blast radius: documentation only.

## Section 4 — CONFIRMED as code-fact/fragility; "disagreement produces retained-but-unpainted" NOT demonstrated
`alternate_repaint_in_progress` duplicates `alternate_repaint_snapshot.is_some()` and is re-derived by hand at :2972,
:3148, :3167, :3345, :3370 (+ the ESU/sync sites :3496/:3541). Different predicates key on flag vs snapshot:
`observe_live_damage` alternate=flag :7119, primary=snapshot :7122; `offscreen_preservation_active`=flag :6680;
`invalidate_all_live_decorations`=flag :7213. For alternate the flag cannot disagree (every snapshot mutation re-derives
it immediately). For primary the flag/snapshot/dirty trio is intentionally separate (flag re-arms per feed :2984, snapshot
spans the window :2979–2980), and the armed-state behavior (retain off-band) is documented intent. So the concrete
"retained-but-unpainted" failure is unreachable in the code I traced; the two-structures-one-fact smell (echoed by the
code's own :6205–6210) is real. Fragility, not a defect. Fix: derive suppression from the snapshot everywhere, or collapse
the flag into `snapshot.is_some()` on alternate. Blast radius: small; behavior-preserving.

## Section 5 — CONFIRMED (reviewer's "no semantic change for ordinary output")
Spot-check agrees: both cuts gated on `xtversion_replies_owed > 0` (adapter.rs:995–996, :1042). No claim to refute.

## Ranked list (stability release: "formulas no longer flash while scrolling in Claude Code, alt screen, DEC 2026")

- BLOCK: none. No claim is introduced or worsened by this branch; the headline fix and the machinery I judged CORRECT
  in rounds 4–5 stand. D1–D4 and S2–S3 predate e0f682b8; D5's merge path is unchanged by "carried-first" (which only
  fixes a real proven-during-window loss). Claude Code's DEC-2026 repaint is a synchronized-update + home + EL redraw,
  NOT a DL scroll, so the headline case is outside D1's blast.
- SCHEDULE AFTER, in order:
  1. D1 (high, pre-existing; common DL trigger in vim/nvim; bar-A flash). First follow-up, but its fix branches on cause
     inside a shared wipe arm — its own change, not a rush into this release.
  2. D5 (medium, pre-existing bar-B; fix is a cheap tombstone set — low-risk enough to fold in if convenient).
  3. D2 (medium-high, pre-existing; primary-idle off-band retention — changes default-screen memory profile, so land it
     after D4's TTL).
  4. D4 (medium; TTL/quiescence for the off-band queue — enables D2 safely).
  5. D3 (design; park/restore wholesale destruction needs a decision, not a patch).
  6. S2 (tail risk; resolved as a by-product of D1's fix).
  7. S3, S4 (fragilities; comments/assertions — safe to defer).
STATUS: COMPLETE
