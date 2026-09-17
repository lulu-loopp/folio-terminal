STATUS COMPLETE

# Formula pipeline: lead adversarial audit

Revision: **d1db4abdfa0de13930feb72d0d05c25e8f108f51**. All source citations below refer to this revision.
Static source audit only: no cargo, builds, tests, application launches, process termination, or commits.
Only this report was written. Tests below are proposed regression pins, not executed results.
K1-K6 are excluded. Seven new defects are established by the cited control/data paths.

## F1 — Inline composites with different geometry alias the same GPU texture

**Severity: high. Areas: 1, 6.**

Evidence:
- `crates/bt-detect/src/lib.rs:1240`: inline `render_source` is only the run sources joined with `.join("; ")` (line 1244).
- `crates/bt-term/src/session.rs:11776`: raster positions use `UnicodeWidthStr::width(before)`; line 11801 computes `x` from the resulting column. Thus intervening prose affects pixels.
- `crates/bt-term/src/session.rs:11993` and `crates/bt-term/src/session.rs:12016`: both planes call `shared_math_artifact_key` with `&task.span.render_source`, layout and detection revision.
- `crates/bt-term/src/session.rs:12043`: the hash contains kind, mode, source, layout and detection only; neither run offsets nor intervening text enter it.
- `crates/bt-term/src/session.rs:14308`: a complete composite keeps the original key (`row.crop_px = None`).
- `crates/bt-render/src/lib.rs:10122`: upload occurs only when `gpu.math_textures.get(key).is_none()`; line 10136 takes the cached tiles without comparing their dimensions or bytes.

Sequence: in eligible output, print `$x$ and $y$` and `$x$ and then $y$` on separate, unwrapped lines at the same layout. Both tasks succeed. Their composite keys are identical (`x; y`), but their second runs have different x coordinates. Display both, in either completion/visibility order.

Visible consequence: whichever texture is admitted first supplies both pictures. On the other line, `y` appears at the first line's offset, overlapping intervening prose or leaving the intended formula position empty. The placement has already cleared its actual run cells (`crates/bt-term/src/session.rs:9037`: `cell.text.clear()`). Scrolling/eviction can change which picture wins. This affects live and frozen occurrences, independently of K1.

Smallest general fix: make the artifact key identify the complete raster recipe, including inline run identities, relative cell positions and fitting geometry (or the immutable resulting raster). Sharing a render source must never imply sharing a composite texture.

Pin: render both lines in both admission orders, live and frozen, and compare each placement's uploaded pixels/geometry with its own raster. Also overwrite one line's prose while retaining its formulas; the new composite must not hit the old texture.

## F2 — Resize restoration uses the old physical band length on the new wrapping

**Severity: high. Area: 2.**

Evidence:
- `crates/bt-term/src/session.rs:12356`: occurrence identity captures `band_end_row - band_start_row + 1`; line 12386 stores `band_rows`.
- `crates/bt-term/src/session.rs:3227`: resize invalidates live decorations into preservation; line 3261 calls `restore_offscreen_decorations` before publication of the new layout.
- `crates/bt-term/src/session.rs:6609`: restoration finds the complete source in the new grid using `exact_live_source_match`.
- `crates/bt-term/src/session.rs:6628`: its new band end nevertheless uses `record.identity.band_rows.saturating_sub(1)`.
- `crates/bt-term/src/session.rs:6641`: `record.start = start; record.end = end`, but lines 6643-6644 install that independently calculated old-length band.
- `crates/bt-viewport/src/lib.rs:3457`: every cell in that band is passed to `suppress_math_source_cell` (line 3460).

Sequence: display one long, soft-wrapped `$$...$$` logical line on the primary grid, followed by ordinary text. Widen the window enough that the formula occupies fewer physical rows. Its unique complete source is restored, while its old band length still extends below the new closing delimiter. Keep the worker completion pending; a drag naturally exposes these intermediate frames.

Visible consequence: the preserved formula temporarily swallows ordinary rows following it. Conversely, narrowing can make `end.row` exceed `band_end_row`; the viewport rejects the placement (`crates/bt-viewport/src/lib.rs:3304`: `live_math.band_end_row < live_math.end.row`) and shows source until a fresh result. The latter completion recomputes the band (`crates/bt-term/src/session.rs:12546`), but cannot correct frames already published during resize.

Smallest general fix: rederive physical ownership and its row proof from the newly matched source segments whenever wrapping changes. Preserve occurrence identity and raster separately from placement; never carry a previous layout's physical band count into a new row map.

Pin: hold completions, widen a two-row display source to one row with a sentinel text row immediately below, then shrink it back. At every intermediate frame, the sentinel remains visible and the owned band ends at the current source endpoint. Repeat A-B-A resizes and deliver old results last.

## F3 — A bridge disappears when its visible part is entirely frozen

**Severity: medium. Area: 3.**

Evidence:
- `crates/bt-viewport/src/lib.rs:2948`: bridge geometry is calculated for the complete frozen/staging/live occurrence; line 2999 stores its absolute top.
- `crates/bt-viewport/src/lib.rs:3262`: the entire live-artifact placement loop is inside `if window_end > live_base && window_start < live_base + expected_rows`.
- `crates/bt-viewport/src/lib.rs:3298`: only inside that condition are `live_math_artifacts` visited; line 3372 queues suppression of the bridge prefix.
- `crates/bt-viewport/src/lib.rs:3484`: prefix suppression is also inside the same live-visible branch.

Sequence: a multiline display block has a long frozen prefix and a short live suffix. Scroll upward until the viewport plus overscan contains a middle slice of the prefix but no live row. Choose enough source lines that the raster's centered ink is in that slice; no raster-height overflow is needed.

Visible consequence: the complete bridge is culled, although its frozen portion and ink intersect the viewport. Raw `$$`/body source reappears. Scrolling down until even the live edge enters the presentation window restores the picture. This is visibility/ownership culling, not K3's height calculation.

Smallest general fix: emit and cull a bridge against its complete cross-plane presentation interval. Suppress its visible owned fragments independently of whether a live grid row is visible.

Pin: construct a bridge longer in source rows than the viewport; scroll through frozen-only, seam and live views without changing source or raster. Assert the intersecting raster remains placed and its source remains suppressed in every view.

## F4 — Source-face state is lost for a block first detected across the seam

**Severity: medium. Areas: 1, 3.**

Evidence:
- `crates/bt-term/src/session.rs:7287`: a completed bridge directly records `frozen_prefix_ids(&task.span)`.
- `crates/bt-term/src/session.rs:8127`: the user's toggle writes `record.show_source = !record.show_source`.
- `crates/bt-term/src/session.rs:10561`: handoff creation refuses `!record.frozen_prefix.is_empty()`; the comment at 10558 explicitly excludes a block first detected as a bridge.
- `crates/bt-term/src/session.rs:10666`: finalization creates/schedules the ordinary frozen record and then tries the handoff.
- `crates/bt-term/src/session.rs:10679`: no pending handoff means immediate return. `crates/bt-detect/src/lib.rs:376` initializes the frozen face to `show_source: false`.
- `crates/bt-term/src/session.rs:10757`: the transfer of `pending.show_source` exists only on the successful handoff path.

Sequence: print a block whose opener has frozen before its first render completes, so its first successful live record is a bridge. Toggle that occurrence to Show source. Print enough ordinary output to freeze its remaining rows, then view it in history after frozen rendering completes.

Visible consequence: that same occurrence turns itself back to typeset. The same path also discards reuse of its already available raster and requires a fresh frozen render. An all-live-origin occurrence has a handoff; this reachable lifecycle entry point does not.

Smallest general fix: establish durable occurrence ownership for bridges when they are first proven, including existing transcript IDs and the remaining live/staging lineage. Transfer raster and interaction state through finalization on that ownership, irrespective of which plane first detected it.

Pin: first render only after the opener freezes, toggle to source, finish the freeze, and assert source face and artifact identity survive. Print an identical second block and assert it still starts typeset.

## F5 — Selection wash omits the frozen/staging half of a bridge

**Severity: medium. Area: 3.**

Evidence:
- `crates/bt-viewport/src/lib.rs:3415`: the bridge placement receives a `MathBlockAnchor::Live`; line 3441 separately stores `frozen_prefix_rows`.
- `crates/bt-term/src/session.rs:9274`: rows attributed to a live placement are selected solely through `mapped.live_grid_row` (line 9280). Frozen/staging rows have no such row number.
- `crates/bt-term/src/session.rs:9310`: only selection spans whose rows are in that set reach `placement.selection_spans`.
- `crates/bt-render/src/lib.rs:833`: an empty placement selection produces no picture wash; lines 862-863 restrict display wash to the supplied row intervals.
- `crates/bt-term/src/session.rs:7810` and `crates/bt-term/src/session.rs:7934`: copy still walks history, staging and live source and appends overlapping cells.

Sequence: with a bridge visibly crossing the seam, drag a selection across its frozen-prefix ink, or from history through the bridge into live text, and copy.

Visible consequence: the prefix source is included in the clipboard, but the corresponding formula ink never receives the selection wash. Only the live part highlights over the picture; the ordinary background selection is insufficient to mark the ink. This is independent of bridge height.

Smallest general fix: derive selected presentation fragments from the complete occurrence ownership used for placement and copying, including exact frozen and staging prefixes. Use that same set to transfer spans out of the ordinary background band.

Pin: select only the frozen prefix, only staging, and then the whole bridge. Verify copied source and over-picture wash agree in all three cases, including after another row freezes.

## F6 — Live queue eviction permanently strands already-marked candidates

**Severity: medium. Areas: 1, 7.**

Evidence:
- `crates/bt-term/src/scheduling.rs:12`: `WORKER_QUEUE_CAP` is 64.
- `crates/bt-term/src/session.rs:3690`: an equal `candidate_signature` skips scheduling; line 3693 sets that signature before enqueue.
- `crates/bt-term/src/session.rs:3749`: the complete stable batch is enqueued in one pass.
- `crates/bt-term/src/session.rs:6864`: when full, the live queue simply executes `self.live_tasks.pop_front()`; it neither re-arms nor records the discarded task.
- `crates/bt-term/src/scheduling.rs:145`: the frozen queue, in contrast, records `retry_on_idle` when full.

Sequence: in a sufficiently tall pane (for example 70 terminal rows), print 65 eligible inline-formula lines as one batch and leave them unchanged. This is ordinary dense report output at a small font. All 65 stable candidates are marked scheduled before the application can drain the queue.

Visible consequence: the first discarded line stays as `$...$` indefinitely while the retained 64 render. Advancing idle time cannot recover it because its signature is unchanged. A later content/layout/context change is needed. There is no worker failure in this sequence.

Smallest general fix: make queue admission and candidate-attempt state transactional. Keep rejected/evicted candidates in a fair bounded retry ledger, or retain a scan frontier until every candidate has been serviced; resetting signatures without fairness can just evict the same work again.

Pin: schedule 65 and 130 unchanged candidates, drain/complete all admitted work, and run ordinary idle scheduling until quiescent. Every eligible occurrence must eventually complete, with a bounded queue and no extra terminal output.

## F7 — A partially visible frozen inline run is relocated to its continuation

**Severity: medium. Areas: 2, 6.**

Evidence:
- `crates/bt-term/src/session.rs:14015`: `frozen_fragment_cells` searches only the published frame's cells.
- `crates/bt-term/src/session.rs:14029`: it accepts any offset in `[start, end)`.
- `crates/bt-term/src/session.rs:14035`: the first matching visible cell becomes `origin`; there is no requirement that its offset equal the run's start.
- `crates/bt-term/src/session.rs:9006`: frozen inline placement uses that lookup; line 9037 clears its returned cells and lines 9052-9054 place the raster at that origin.

Sequence: freeze an eligible inline formula which soft-wraps across two physical rows, at a width where its raster fits the first row's remaining cells. Scroll by one row so its opening `$` and intended picture origin are above the viewport, while the continuation is visible.

Visible consequence: the entire formula is redrawn at the first visible continuation cell. Its picture moves down a row as the reader scrolls, instead of leaving the viewport with its true origin; continuation source cells are cleared under this false placement. This also occurs after narrowing history enough to create the wrap.

Smallest general fix: derive the run's true projected origin from its source anchor and full line layout, independently of the visible frame slice. Clip that placement to the viewport; do not substitute the first visible source fragment for its origin.

Pin: put a short-rendering but longer-source run such as `$x^2$` across a wrap with enough room for its raster on the opening row; freeze it, move the viewport one row at a time past its opening, and compare absolute raster origins. The origin must stay attached to the opening anchor; no full formula may restart on a continuation.

## Coverage and state audit

### 1. Scheduling and completion on both planes

Live tasks capture screen, grid generation, detection revision, layout, three cell metrics, options, initial parser context, input rows/sites, and resolved geometry (`crates/bt-detect/src/lib.rs:311`: `LiveDetectionTask`). Completion checks screen/generation/detection/layout (`crates/bt-term/src/session.rs:7216`) and reruns source detection (`crates/bt-term/src/session.rs:13601`: `resolve_live_detection_task`). Changed text on a reused row is rejected even if it contains the old formula (`crates/bt-term/src/session.rs:13573`: `snapshot.text != current.text`). Same-byte repaint revisions alone do not reject it.

A top-row scroll clears queued live tasks and advances generation (`crates/bt-term/src/session.rs:10402`: `live_tasks.clear(); ...grid_generation.0 += 1`); retained records are reprojected and candidates rescan against moved inputs (lines 10424, 10439, 3688). An in-flight old result cannot apply to the shifted row. A first result arriving after its row freezes is rejected on the live plane; finalization independently schedules the frozen plane (`crates/bt-term/src/session.rs:10666`: `schedule_detection(id)`). Ready all-live records use the explicit handoff at line 10749; F4 covers the missing bridge-born transfer.

Frozen tasks carry candidate/transcript IDs, versions, options, metrics, parser checkpoint and immutable inputs (`crates/bt-detect/src/lib.rs:178`). The candidate must be `Frozen + Pending` at identical versions (`crates/bt-term/src/session.rs:7492`), and block input text must still match (line 7401). Accepted handoffs set `Ready` (line 10747), excluding their old candidate completions. Stranded pending attempts are recorded and re-armed at quiescence (lines 7511, 7533). No additional cross-plane double application or out-of-order wrong-source installation was proved. F1 is the separate identical-key raster collision; F6 is admission/liveness failure.

### 2. Resize, wrapping, width and scale

The adapter forks canonical terminal/parser state before the resize transaction (`crates/bt-term/src/adapter.rs:995`: `self.term.fork`; line 1010 replays `parser_tail` into a sink). Live generations advance (`crates/bt-term/src/session.rs:3202`), rows get a new stability clock (`crates/bt-term/src/session.rs:3239`), layout changes retain stale rasters and clear candidate signatures (`crates/bt-term/src/session.rs:11022`, `crates/bt-term/src/session.rs:11081`). Old-layout completions are gated at `crates/bt-term/src/session.rs:7219`. Frozen eligibility survives re-arm through the stored `HistoryEntry.inline_site` (`crates/bt-doc/src/document.rs:38`; `crates/bt-term/src/session.rs:11096`). F2 is a placement failure despite those version gates; F7 is the history visibility/re-wrap failure.

Inline workers read logical text (`crates/bt-term/src/session.rs:11643`) and decline individual runs wider than their starting-row budget (lines 11783, 11796: `continue`). Display math fits down to a readable floor and leaves further overflow for horizontal scrolling (lines 14476-14479, 8268). Growing the width recalculates the fit. No separate width-overflow defect is asserted here.

### 3. The seam

F3-F5 cover scrolling, selection/copy and source-face/final-freeze behavior beyond K3. Exact prefix identity is checked (`crates/bt-viewport/src/lib.rs:2965`: history tail equality; line 2991: staging-ID equality). Deleted prefixes are retired (`crates/bt-term/src/session.rs:10881`: `retire_stale_bridge_prefixes`), so this review does not claim an orphan bridge survives ED3. Ordinary copy rejoins source rows, not raster text (`crates/bt-term/src/session.rs:7934`).

### 4. Alternate screen, repaint, clear and reset

No additional proven defect: per-turn settlement remaps before release (`crates/bt-term/src/session.rs:3028`: `finish_alternate_repaint`; line 3032: `finish_primary_repaint`); real row changes invalidate outside those windows (line 6794), while equal fingerprints retain stability (line 6769).
Screen switching clears live work and restoration advances generation (`crates/bt-term/src/session.rs:9554`, `crates/bt-term/src/session.rs:9578`); clear-history/staging uses deletion (`crates/bt-term/src/session.rs:9524`), and reset/invalidation retires staging and semantic ownership (`crates/bt-term/src/session.rs:9529`, `crates/bt-term/src/session.rs:9548`). No code-proven new stale-formula-over-changed-text sequence was found; preservation is not treated as proof that arbitrary producer repaints are equivalent.

### 5. Eligibility, streaming, text coordinates

No additional proven gate defect: missing authoritative integration refuses primary inline sites (`crates/bt-term/src/session.rs:4956`: `return false`); open C uses the cursor frontier (line 5005), A closes missing-D output (line 4096), repeated C closes the old region (line 4195). Thus completed rows can render during an ongoing command; background output outside C-D lacks inline proof.
Long/code-like lines are refused (`crates/bt-detect/src/lib.rs:648`), and capture keeps text separate from SGR/style (`crates/bt-term/src/cell_capture.rs:163`) with byte-to-cell boundaries (`crates/bt-term/src/session.rs:13751`). CJK/wide/tab and styled-source coordinate paths were inspected; no additional concrete misplacement is claimed. Unsupported glyphs return source through `crates/bt-math/src/lib.rs:334` (`MissingCjkGlyph`/`MissingGlyph`). K1 remains excluded.

### 6. Layout accounting and caches

F1 and F7 are the proven cache/placement issues. Measured layout includes source generation, span, detection, full layout and artifact height (`crates/bt-viewport/src/lib.rs:4557`); height-only updates move both row and pixel trees (lines 4110-4112). Anchored views resolve their content anchor before recomputing its y (lines 2814-2823), so an earlier band's growth is not itself evidence of a jump. The relief-ceiling problem is K4, not a new finding.

Row-cache identity includes cell contents, metrics, font and theme (`crates/bt-render/src/lib.rs:2946`). Math render keys include DPI, point size, foreground and mode (`crates/bt-math/src/lib.rs:195`); physical inline em enters layout (`crates/bt-term/src/session.rs:2060`). Theme changes clear stale ink (line 11028). No additional proven zoom, monitor-scale, scrollbar or badge discrepancy was found beyond the cited placement failures and K4.

### 7. Budgets, textures and worker failures

F6 is a proved budget-induced liveness loss. Ordinary GPU eviction re-uploads from the placement's retained RGBA (`crates/bt-render/src/lib.rs:10122`) and draw batches retain `Arc` tile references (line 10136), so eviction later in the batch does not invalidate earlier draws. CPU single-raster and GPU cache budgets both equal 64 MiB (`crates/bt-math/src/lib.rs:99`; `crates/bt-viewport/src/lib.rs:352`). A textureless refusal only logs and skips (renderer lines 10138-10141); no reachable valid formula exceeding that shared budget was established here.

Conversion panic is caught (`crates/bt-math/src/lib.rs:86`: `catch_unwind`); validation limits source bytes and nesting (lines 606, 626). Failed live results either remove the record or install `artifact: None` with `stale_artifact: None` (`crates/bt-term/src/session.rs:7246`, `crates/bt-term/src/session.rs:7322`); failed frozen results select `DecorationIntent::Plain` (`crates/bt-term/src/session.rs:7445`). These paths retain source rather than blanking it.

The worker is a serial receive/render loop (`crates/bt-app/src/main.rs:1690`); no per-task deadline or restart exists there. Dispatch failure disables the feature (line 10441). Outside conversion, the panic hook invokes the fatal path (lines 119953-119957; exit at 119930). **Reachability limit:** no accepted formula causing a non-conversion panic or hang was established, so neither is promoted to a defect finding. The audit does not claim containment or recovery that this code lacks.

## Ranked summary

1. F1 — high — Inline composite texture aliasing draws another occurrence's geometry; fix complete raster identity.
2. F2 — high — Resize preservation keeps old row ownership and hides following text; rebuild placement from new source segments.
3. F6 — medium — Live queue overflow leaves eligible rows permanently unscheduled; add fair retry/admission ownership.
4. F3 — medium — Frozen-only views of a bridge lose its picture; cull the complete occurrence interval.
5. F4 — medium — Bridge-born occurrences forget Show source when fully frozen; carry durable occurrence state across both planes.
6. F7 — medium — Frozen inline formulas restart at a visible continuation; project the actual source origin.
7. F5 — medium — Bridge prefix copies without matching picture selection; include all owned plane fragments in the wash.

STATUS COMPLETE
