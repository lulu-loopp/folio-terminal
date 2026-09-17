STATUS IN PROGRESS

# Formula r2/r3 verification — DeepSeek, 2026-09-17

Scope: only the two newest commits, `951f7788` (live worker queue fairness) and
`95d5bad6` (delayed math verbs), read at `95d5bad6`. Read-only; nothing built or run.

## 1. `951f7788` — live worker queue fairness (crates/bt-term/src/session.rs)

Mechanism. `schedule_live_artifacts` builds `new_tasks` in ascending `candidate_row`
order, then rotates admission with one cursor (`live_admission_row: u32`, field at
session.rs:1308, init `0` at :1740):

```rust
let split = new_tasks.partition_point(|task| task.candidate_row < self.live_admission_row);
new_tasks.rotate_left(split);
...
first_refused.get_or_insert(candidate_row);
...
if let Some(row) = first_refused { self.live_admission_row = row; }
```
(session.rs:3776-3794). The rotation is the circular order starting at the cursor; the
first refused row becomes the next cursor, so a full queue admits the next 64 rows of the
circle each pass.

(a) Skip invariant. Held. `candidate_signature` is set only at arming (session.rs:3719)
and cleared on refusal (:3790), suppression (:3770), damage (:6726), detector revision
(:7586) and resize relayout (:11082). The commit changes no signature write. The one
"signature set, no queued/in-flight task" state is the settled row (successful or
`NotDetected` completion leaves the signature set on purpose — the row is already
processed), which is correct, not a violation. The stale-result path (`apply_live_worker_completion`
returns `Some(Refusal::…)`, signature kept) is pre-existing and orthogonal to this commit.

(b) Starvation, both directions. None. With step 64 and N rows, the cursor advances by 64
mod N and the wrap window always covers row 0 within N/gcd(64,N) passes (the 64-element
wrap region contains every residue class mod gcd(64,N)); the tail rows `>= cursor` are
served first each pass. Re-armed rows can never "overtake" waiting rows, and a changing
head is re-admitted once per sweep.

(c) Cursor across renumbering. It is NOT reset or re-based in `preserve_live_after_top_scroll`
(session.rs:10408), resize (:3263 rebuilds `live_rows`), the alternate/primary switch
(:9549/:9563 reset `live_rows`), clear-screen (`invalidate_all_live_decorations`), or
`rebase_vendor_owned_rows` (:10286). A stale cursor can point past the grid, but this is
benign: `partition_point`/`rotate_left` are bounds-safe (an out-of-range cursor yields a
no-op rotation), the rotation never drops a candidate so no row is skipped permanently,
and the cursor is overwritten by the next refusal. At worst one pass of the pre-fix order.
Not a defect.

(d) Per-pass allocation. None added. `partition_point` (O(log n)) and `rotate_left` (in-place)
allocate nothing; `first_refused: Option<u32>` is inline. The only allocation in the path,
`let mut new_tasks = Vec::new()` (session.rs:3709), is pre-existing.

(e) Tests. `live_queue_services_65_candidates_during_repaint` / `…130…` (session.rs:15590/:15595)
do exercise the bug: repainting row 0 changes the context signature, and because
`live_detection_signature(context_signature, row)` (session.rs:12679) hashes the whole
context, every row re-arms each pass. Without the rotation the ascending scan re-admits
rows 0–63 forever and the tail starves, so `serviced.len() == count` fails. The "partly-full"
arm (one task queued before the burst) is asserted directly (`live_tasks.len() == 1`).

Verdict: CORRECT.

## 2. `95d5bad6` — delayed math verbs (crates/bt-app/src/main.rs, formula_tools.rs)

Mechanism. Delayed state now carries `PasteTarget { tab, seat, incarnation }`
(main.rs:790) instead of a bare `SeatId`, validated at answer time by the existing
`live_paste_target` (main.rs:98682) → `paste_target_is_live` (main.rs:759), which requires
the tab to still be on top, the seat present, and the incarnation equal.

(a) Every delayed road covered.
- Menu copy: `pending_math_context_anchor: Option<(PasteTarget, MathBlockAnchor)>`
  (main.rs:12925); `apply_math_context_menu_result` (main.rs:87100) → `copy_math_latex`
  (main.rs:87074), which returns before `set_clipboard_text` on a stale target.
- Toggle measurement: `math_toggle_faces` (:86718) and `math_toggle_heights` (:86737)
  both `let index = self.live_paste_target(target)?;`.
- Toggle mutation: `press_math_toggle` (:86749) and `switch_math_source_now` (:86817) use
  `self.window.tabs[index].sessions.get_mut(&seat)`.
- Toggle presentation: `present_math_toggle` (:86845), `settle_math_toggle` (:86892),
  `advance_math_toggle_if_due` (:86952), `formula_toggle_layers` (:87000) all resolve via
  `live_paste_target` and index the target's own tab.
- `switch_math_source_now`: validates (main.rs:86822).
No delayed road resolves by bare seat or through the `Deref` to the active tab
(`Runtime → TabState`, main.rs:16821). Remaining `self.sessions.get(&seat)` sites
(main.rs:85063, :85160, :85310, :87336) are synchronous hover/frame/selection paths, not
delayed verbs.

(b) Stale target. No clipboard write and no tick — `copy_math_latex` returns before
`bt_platform::set_clipboard_text` (main.rs:87078-87088), so `math_copied` is never set.
Pending field cleared, not left to fire: `pending_math_context_anchor.take()` runs
unconditionally in `apply_math_context_menu_result` (:87104); `settle_math_toggle` takes
`window.math_toggle` before the `live_paste_target` check (:86893, :86900). The only
"presentation not released" branch is reached when the owner session is already gone
(shell restart replaced the `LeafSession`), so there is nothing left to leak. Ordering is
correct: `leave_hovered_math` runs before the tab/pane/focus mutation (`activate_tab`
main.rs:39384-39385; `close_pane` :54439-54458; `focus_seat` :54950).

(c) Immediate left-click copy. `copy_math_latex(target, …)` where `target =
self.paste_target(math_seat)` (main.rs:93699) is built from the pointer's pane; within the
same tab `live_paste_target` succeeds, so the unfocused-pane copy still works.

(d) Tests. `delayed_math_menu_validates_tab_seat_and_incarnation` (main.rs:106602) drives
the real decision function `paste_target_is_live` across all four cases (tab mismatch, no
standing shell, wrong incarnation, match) — a real decision test, plus source-text pins.
`delayed_math_toggle_validates_its_original_owner` (main.rs:106642) only pins source text
(`self.live_paste_target(target)` in each function body, no `self.sessions.iter()` in the
overlay); it would not catch a lookup error in `live_paste_target` itself. That lookup is
not unit-tested, but the rule it applies is the one genuinely tested by the menu test, and
the docstring (main.rs:752) states the split deliberately. Mild, acceptable asymmetry.

Verdict: CORRECT.

## Gaps / smallest general fixes

- Commit 1: `live_admission_row` is not re-based on renumbering. Optional, not required —
  provably no starvation/panic. Smallest fix if desired: reset `live_admission_row = 0`
  beside the existing `candidate_signature = None` sweeps (session.rs:7586, :11082, :10408).
- Commit 2: no production gap. Smallest hardening if desired: a unit test of
  `live_paste_target`'s lookup (tab index, seat miss, incarnation read) against a real
  `WindowState`, rather than source-text pins alone.

STATUS COMPLETE
