STATUS COMPLETE

# Formula (LaTeX) pipeline — independent second-pass audit

Scope: asynchronous boundaries and state machines of the formula typesetting pipeline
(Folio, Rust + winit + wgpu, Windows + macOS). Read-only static audit at commit `d1db4abd`
in worktree `math-audit-ds`. Method: source reading only — no cargo, no build, no test, no
program launched, no process ended, no tracked file modified except this report. Every claim
is cited as `crates/…/file.rs:LINE` with the decisive line quoted. Areas 1, 4, 5 were read
deepest; areas 2, 3, 6, 7 were fanned out to independent subagents and re-verified here where
a defect was claimed. Known issues K1–K6 are not re-reported.

## Findings

### F1 — live worker queue overflow permanently strands the topmost formulas as source
Severity: **HIGH**. Area: 1. File: `crates/bt-term/src/session.rs:6856-6868` (+ `3690-3693`).

Decisive lines, `enqueue_live_task`:
```rust
if self.live_tasks.len() == WORKER_QUEUE_CAP {   // 6864, WORKER_QUEUE_CAP = 64
    self.live_tasks.pop_front();                 // 6865 — the task is DROPPED, not retried
}
self.live_tasks.push_back(task);
```
Arming is gated on a per-row signature that is set before enqueue and never cleared on drop:
```rust
if state.candidate_signature == Some(signature) { continue; }  // 3690
state.candidate_signature = Some(signature);                   // 3693
```
`schedule_live_artifacts` (3667) arms every candidate of a single stability pass in one loop
(3749-3751), so one pass that arms more than 64 rows drops the oldest 64+ tasks with their
signatures left set. The frozen scheduler keeps the same overflow rows alive instead of
dropping them: `enqueue` inserts the id into `retry_on_idle` and returns `EnqueueOutcome::RetryOnIdle`
(`crates/bt-term/src/scheduling.rs:137-153`); the live queue has no equivalent. The only
signature-clearing sweep on the live plane is a *layout* change
(`session.rs:11080-11081`, "Clearing content signatures still provides a retry path if a
queued task is dropped: resize changed layout").

Sequence: a single PTY feed writes more than 64 formula-bearing rows; they all share one
`last_damage_at`, so one `advance_live_stability` (3633) arms all of them at once; `enqueue_live_task`
drops the topmost of the burst; the dropped rows keep `candidate_signature = Some`, so the
next pass's `== Some(signature)` check (3690) skips them forever.

What the user sees: the topmost formulas of a dense burst stay as raw `$$…$$` source on the
live grid indefinitely — until the row is re-damaged by new output, scrolled into history
(where the independent frozen pipeline re-detects it), or the pane is resized/zoomed/themed.

Smallest correct general fix: give the live queue the same overflow semantics as the frozen
one — when an entry is dropped, clear that row's `candidate_signature` (or keep a live
`retry_on_idle` set), so a later stability pass re-arms the dropped rows. Do not special-case
the count; the invariant to enforce is "a row is skipped only while a task for it is actually
queued or in flight".

Test that pins it: build a live grid with ≥80 rows, feed 80 single-line `$$x$$` blocks in one
PTY write, advance past `LIVE_MATH_STABLE_INTERVAL`, and assert all 80 rows produced a
decoration record (today the top 16 strand at source).

### F3 — right-click "Copy LaTeX" reads the keyboard's pane, not the pane you clicked
Severity: **MEDIUM**. Area: 3. File: `crates/bt-app/src/main.rs:86923-86932`.

The anchor is captured under the pointer: `crates/bt-app/src/main.rs:93581-93583` stores
`self.window.pending_math_context_anchor = Some(math_hit.anchor.clone())` where `math_hit`
is resolved by `seats::pane_at(x, y)` (no focus gate), and the context menu's only item is
"Copy LaTeX" (`crates/bt-platform/src/lib.rs:6216`). But the copy reads the *focused* leaf:
```rust
let Some(source) = self
    .focused()                                      // 86928
    .and_then(|leaf| leaf.session.math_source(anchor))
else { return; };
```
A right press never moves the keyboard, because the focus move lives inside the left-only
route: `if button != MouseButton::Left { return Ok(false); }` (`main.rs:91614-91615`) returns
before the focus call. The other anchor verbs do it correctly and search the answering seat —
`math_toggle_faces` (`main.rs:86607-86615`) and `math_toggle_heights` (`86627-86632`) both
`self.sessions.iter().find_map(|(seat, leaf)| leaf.session.math_…(anchor))`.

Sequence: two panes, keyboard focus in the left one; hover a formula in the right pane (hover
marks light, it is pointer-based), right-click it, choose Copy LaTeX; `math_source` is asked of
the left pane's session, where the anchor names nothing (Live anchors match on
`screen && start && end && generation`, `session.rs:8119-8124`).

What the user sees: the clipboard is unchanged and no acknowledgement tick appears — or, when
the left pane coincidentally holds a record of the same anchor shape, the *wrong* formula is
copied. Left-clicking the copy mark is unaffected because that press focuses the pane first.

Smallest correct general fix: `copy_math_latex` must find the session that answers for the
anchor exactly as the toggle verbs do — `self.sessions.iter().find_map(|(_, leaf)|
leaf.session.math_source(anchor))` — instead of `self.focused()`.

Test: two panes, a rendered formula in the right pane, focus in the left; right-click the
formula → Copy LaTeX; assert the clipboard holds that formula's source.

### F2 — a refused boundary-split bridge blanks the formula's frozen source (PLAUSIBLE)
Severity: **MEDIUM** (PLAUSIBLE). Area: 3. File: `crates/bt-viewport/src/lib.rs:2960-3411`.

`frozen_prefix_geometry` is inserted *before* the staging gate (`lib.rs:2981`, in the
`else` at 2960-2982), and the gate refuses to form a bridge whenever the staging plane is not
exactly the occurrence's staging prefix (`lib.rs:2988-2993`, `continue`). The non-bridge arm
then still blanks the frozen prefix — `bridge_prefix_blank.push((abs_top, history_rows))`
(`lib.rs:3375-3382`, drained at 3484-3496) — while the placement it emits is sized from the
live band alone: `top` from the live band start (`3405-3410`), `frozen_rows = 0` (`3411`),
and `frozen_prefix_rows == 0` pins the block's top to the band start (`3545-3546`).

Sequence: a `$$` block whose closer sits on the live grid while its opener/body rows are
frozen in history, and the staging plane holds an unrelated in-progress logical line — a state
the code's own comment attests is expected ("Staging may hold an unrelated in-progress logical
line", `lib.rs:2984-2987`). The bridge is refused, yet the frozen opener/body rows are blanked
and only the on-grid slice is drawn.

What the user sees: a blank gap where the top of the formula was, over a clipped half-formula
image, for as long as the closer stays on the live grid (indefinite on an idle session); in
the both-prefixes-non-empty case the occurrence's own staging rows reappear above its picture
because `abs_end` stops at `history_rows` (`lib.rs:3484-3486`).

PLAUSIBLE: reachability needs a live capture of a bridged block whose staging plane holds an
unrelated line; the covering test exercises only the clean case where `staged_rows ==
staging_prefix` (`lib.rs:9248-9344`, fixture `9258-9287`). Distinct from K3, which is the
*formed* bridge's sizing (`:3363`/`:4231`); this is the branch that refuses to form one.

Smallest correct general fix: blank frozen-prefix rows only when a bridge was actually formed
— move the `bridge_prefix_blank.push((abs_top, history_rows))` from the non-bridge arm into
the bridge arm (3372) — so a refused bridge leaves the occurrence entirely at source.

Test: a grid with a boundary-split block, an unrelated line in staging, and a live closer;
render the frame and assert the frozen prefix rows still show their source (not blanked).

### F4 — a worker fault outside the MiTeX conversion takes the whole process (PLAUSIBLE)
Severity: **LOW** (PLAUSIBLE). Area: 7. File: `crates/bt-math/src/lib.rs:86,341,348,379`.

Only the MiTeX conversion is contained: `std::panic::catch_unwind(...)` wraps
`mitex::convert_math` (`lib.rs:86`); the Typst compile (`self.engine.compile_with_input`, 379)
and `typst_svg::svg` / `rasterize_svg` (341/348) run outside it. The process panic hook
escalates any non-contained panic to process exit
(`crates/bt-app/src/main.rs:119953` → `leave_process(PANIC_EXIT_CODE)`). `validate_source`
does not bound recursion: it counts only `b'{'` to depth 256 (`lib.rs:621-633`), so
brace-free deep recursion (e.g. `\sqrt\sqrt\sqrt…`, thousands of levels inside the 8 KiB
source budget, `lib.rs:95`) is unbounded, and a stack overflow is not caught by `catch_unwind`.

Sequence (if reached): one formula that panics or overflows outside `catch_unwind` → the panic
hook exits the process → the window hides and `folio.exe` dies, taking every shell in every
pane, not just the formula.

PLAUSIBLE: no input is proven to panic or overflow; the missing evidence is a concrete
panicking or deep-recursing source. When it is reachable the cost is process-wide, so the
containment boundary is the correct thing to widen regardless of this finding's confidence.

Smallest correct general fix: extend `catch_unwind` to the whole render (`compile_with_input`
+ svg + rasterize), returning `MathRenderError::Aborted` on unwind, so a single formula can
fail to source while the process lives.

Test: a source that makes Typst/rasterize panic (or a test-only panic injection) must produce
a failed record at source, not a process exit.

### F5 — repaint-protection window misses a repaint that straddles a feed turn or writes home-before-2J (PLAUSIBLE)
Severity: **LOW** (PLAUSIBLE). Area: 4. File: `crates/bt-term/src/session.rs:3026-3035,13900-13926`.

`settle_feed_turn` closes the repaint window at the end of every drain turn, so a repaint
whose bytes span two turns loses suppression for its later turns. And
`contains_clear_home_snapshot_boundary` accepts only `?2026h`, `2J`-then-home, or an early
home with ≥3 `\x1b[K`; a chunk written `\x1b[H\x1b[2J…` (home first, no `\x1b[K`) is not
recognised as a clear-home boundary. The suppression itself is otherwise sound where it
applies (fingerprint at `session.rs:6769-6771`, window cleared on every live-screen switch at
6727-6743).

Sequence (if reached): a full-screen TUI repaint that straddles reads, or that clears home
first then `2J`, is not shielded, so a formula re-printed byte-identically could fall back to
source for a window (the fingerprint still saves the byte-identical reprint).

PLAUSIBLE: no real TUI is proven to straddle reads or to write home-before-`2J` without
`\x1b[K`; the missing evidence is such a capture.

Smallest correct general fix: carry the repaint window across feed turns until the snapshot is
settled or a genuine change arrives, and add `home-then-2J` to the recognised clear-home
boundary shapes.

Test: feed a two-chunk clear-home repaint whose boundary is home-then-`2J` and assert the
reprinted formulas keep their rendered records.

### F6 — a hung worker request freezes every decoration lane forever (PLAUSIBLE, lowest)
Severity: **LOW** (PLAUSIBLE). Area: 7. File: `crates/bt-app/src/main.rs:1690`.

The worker is one serial `while let Ok(work) = task_rx.recv()` loop serving math, inline
images, peeks and video animation, with no per-request timeout and no restart path. One
request that never returns freezes every lane for the life of the run: math stops arriving
(blocks stay at source), and so do video frames and peeks.

PLAUSIBLE: needs a real request that hangs; no concrete input demonstrated. This is the
weakest finding in the report and is recorded for completeness, not as a demonstrated defect.

## Sound areas (verified, no defect)

Area 1 routing/keying is otherwise sound: an answer is re-validated against the session's own
state, not matched by a render key — `worker_task_is_current` (session.rs:7488, Frozen +
Pending + `record.versions == task.versions`) and `live_task_is_current` (13545, re-resolves
the span and compares `start/end/span`). Out-of-order answers, two identical formulas in
flight, and a row overwritten with the same formula cannot cross or mis-apply; answers are
claimed by `Tab`/`Window` id (main.rs:14725-14745), and worker death is a one-way downgrade to
source (main.rs:110746 → 10382).

Area 2 (resize/re-wrap) is sound: teardown is synchronous with the geometry change
(`invalidate_all_live_decorations`, session.rs:3227) and re-arming is gated on
`ResizeEpoch::decorations_allowed()` (scheduling.rs:80-82), so a drag cannot thrash; a late
completion is rejected by the layout gate (session.rs:7210-7226). Zoom/theme/dpi/font/language/
profile/wrap all sit in `LayoutKey` by `Eq`+`Hash` (versions.rs:110-158), and the GPU key
hashes that key upstream, so no stale measurement is handed to a new layout.

Area 5 (eligibility gates) is sound: authority comes only from `B` and `C`
(session.rs:4169/4199, sole reader 3903); the site is recorded once at freeze (10794), defaults
to `Ineligible` (document.rs:274), and is re-read not re-derived. A-without-B, C-without-D,
nested/repeated D, missing D after Ctrl+C (recovered by A at 4096), and background-job output
after the prompt all behave as designed. Byte indexing throughout keeps CJK/wide characters
from shifting a `$` slice, and SGR rides beside the text, never inside it. The one live/frozen
divergence found (trailing styled whitespace) is provably inert for math because the detector
trims the line tail before the only test that cares.

Area 6 (layout accounting) is sound: extent, rows-above and the scrollbar thumb read one
triple (`bt-viewport/src/lib.rs:2402-2407, 2928-2934`; `bt-app/src/termscroll.rs:1385-1392`),
so they cannot disagree; a band growing above the viewport does not move a parked anchor
(lib.rs:4641-4711) and a shrinking band re-clamps the overflow offset (2720-2722).

Area 7 (resources) is sound for reachable input: an evicted texture is re-uploaded from the
bytes the frame carries (`bt-render/src/lib.rs:10122-10132`) and draws hold their tiles by
`Arc` (1991-1994); the refusal path is unreachable for a formula because
`MAX_RASTER_BYTES` (64 MiB) equals the LRU budget; and an unrenderable formula keeps its
source — never blank (session.rs:7110-7123, 8308-8327).

## Ranked summary

1. F1 HIGH — a >64-row formula burst strands the topmost formulas at source forever (`enqueue_live_task` drops, signature never cleared).
2. F3 MEDIUM — right-click Copy LaTeX reads the focused pane, not the pane under the pointer.
3. F2 MEDIUM (PLAUSIBLE) — a refused boundary-split bridge blanks the frozen prefix it refused to draw.
4. F4 LOW (PLAUSIBLE) — a non-MiTeX worker panic or overflow kills the whole process.
5. F5 LOW (PLAUSIBLE) — repaint window misses repaints that straddle turns or write home-before-2J.
6. F6 LOW (PLAUSIBLE) — a hung worker request freezes every decoration lane.

## Cross-verification of two findings from the lead audit

### X1 — inline composite texture-key aliasing — CONFIRMED (HIGH)

Verdict: **CONFIRMED**, HIGH, reachable in ordinary use. The fix direction is correct; the smallest
general fix is stated below.

The composite is one raster per logical line. `render_task_math` blits every run into a single
`rgba` buffer at `x = (UnicodeWidthStr::width(before) - base_column) * cell_width_px`
(`session.rs:11776`, `:11801`, composition `:11831-11847`), so the run offsets — and the whole
image width — are a function of the prose between runs. The key carries none of it: `render_source`
is only the run sources joined `"; "` (`bt-detect/src/lib.rs:1240-1244`), and
`shared_math_artifact_key` hashes exactly `(kind, mode, render_source, layout, detection)`
(`session.rs:12036-12050`), reached from both `artifact_from_raster` (`11993-11999`) and
`artifact_from_live_raster` (`12016-12022`). A whole (unfolded) composite keeps that key
byte-for-byte (`14308-14309`); only the folded per-row crop appends `#x{x0}` (`14347`). The
uploader is key-only with no size or byte comparison:
`if gpu.math_textures.get(key).is_none() && let Some(texture) = gpu.upload_math_texture(...)`
(`bt-render/src/lib.rs:10122-10132`), and the draw then walks the cached tile, not the placement's
own rgba.

So two eligible inline lines in the same pane — same layout, same `detection_revision` (which
changes only on the inline-math toggle, `session.rs:2171-2178`, not per detection) — with the same
run sources in order but different-width prose, e.g. `a = $x$, b = $y$` then `a = $x$, and b = $y$`,
produce the same key. The first line populates the cache; the second skips the upload and draws the
first line's composite, so its later runs land at the first line's offsets. Log lines or scripted
output that repeat a formula pair with varying prose reach this in ordinary use.

Fix: the inline composite key must be a function of the inter-run geometry, which is determined by
the full logical line. Smallest general fix: key inline artifacts by the full logical line text (or
by the per-run cell columns / `x_px` offsets already present in `raster.inline_runs`) in addition to
`render_source`. Display blocks are unaffected (one run, no prose dependence). Test: two lines with
the same run sources and different-width prose at one layout must produce different artifact keys and
each render at its own offsets.

### X2 — resize restoration keeps the stale band length — CONFIRMED (transient, MEDIUM)

Verdict: **CONFIRMED** as a real defect, MEDIUM rather than HIGH: the mechanism is exactly as
claimed, but the window is transient and self-healing — frames with the stale band are published only
until the re-detection triggered by the resize's own stability re-seed completes.

Mechanics verified. Identity captures `band_rows = band_end_row - band_start_row + 1` and both
offsets (`session.rs:12356-12361`). On primary resize `invalidate_all_live_decorations` parks
records off-band (`3227`) and `restore_offscreen_decorations` re-anchors each by exact source
(`3261` → `6608-6609`); it recomputes `logical_band_end = (start.row - source_start_offset) +
(band_rows - 1)` (`6622-6628`) and installs `record.start/end` from the match while `band_end_row`
keeps the old length (`6641-6644`). The viewport then suppresses every cell in
`band_start_row..=band_end_row` (`bt-viewport/src/lib.rs:3455-3462`) and rejects
`band_end_row < end.row` as source (`lib.rs:3301-3307`, `:3304`). Widening a soft-wrapped `$$…$$`
line shrinks its physical extent, so `band_end_row` now reaches past `end.row` and blanks the
ordinary rows below; narrowing makes `end.row > band_end_row`, which is caught and shows source.

Frames are published with the stale band: the record is re-inserted into `live_decorations` inside
`resize_at` (`6696`) and every subsequent frame projects it via `sync_projection_state`
(`8287` → `8399-8405`). The correction is the same resize's stability re-seed — `live_rows` is
rebuilt with `last_damage_at = Some(observed_at)` (`3237-3243`) — so a fresh detection lands one
`LIVE_MATH_STABLE_INTERVAL` (200 ms) after the *last* resize event. During a continuous drag that
spans the whole drag plus 200 ms; on a single click-resize it is a handful of frames. It is visible
(rows blanked/misplaced while dragging with a wrapped formula on screen) but self-healing — it does
not persist.

This contradicts the earlier "area 2 sound" verdict only in scope: that verdict covered
teardown/re-arm thrash and the layout gate on late completion (both still hold); it did not cover
the preserved-record band recomputation, which is a real gap.

Fix: re-derive the band from the matched extent using the stored offsets, not the stored length —
`band_start_row = start.row - source_start_offset`, `band_end_row = end.row + (band_rows - 1 -
source_end_offset)` (equivalently `end.row` in the common display case where the band equals the
source extent). The identity already stores both offsets (`12360-12361`), so this is the smallest
correct general fix. Test: display a `$$` block that soft-wraps, widen the pane, and assert the
preserved record's `band_end_row` equals the matched `end.row` (no cell below the formula is
suppressed) before any fresh detection runs.

STATUS COMPLETE
