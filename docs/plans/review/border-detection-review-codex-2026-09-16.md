# Adversarial review: formulas behind a multiplexer border

Reviewed committed HEAD `947e31b1`, implementation `f473c264`, against merge-base
`580c8ede`; read `git log main..HEAD` and `git diff main...HEAD` first.
Verdict: **do not merge until B-1 through B-5 are addressed**.
All file:line references below identify that committed HEAD, unless labelled WIP.

The already-dirty working tree kept changing: WIP adds width/right-edge bounds,
source widths, and an inline lookup fix for B-1. No follow-up commit was present.
This review did not edit product files or start/stop applications or services;
only repository operations and permitted tests were executed.

Read the supplied herdr `REPORT.md` and `VERIFY-HIDDEN.md`. Their byte evidence
supports a 25-cell sidebar plus one rule (content starts at zero-based column 26),
compact content at column 4, and hidden content at column 0. The other application
fixtures below are explicit synthetic screen shapes, not new application captures.

## B-1 — blocker — inline formulas use region bytes against whole-screen text

`crates/bt-detect/src/lib.rs:3340`; `crates/bt-term/src/session.rs:14134`,
`:14152`, and the inline placement call near `:9110`.

`apply_live_detected_block` correctly makes cell segments and anchors screen-based,
but leaves inline-run byte ranges region-local and `task.inputs` whole-screen.
The record retains those original inputs. `live_inline_run_cells` passes the byte
ranges to `live_fragment_cells`, which indexes the original text and infers columns
from its prefix. That path does not use the proved screen-cell segments.

Reproducer, 40 identical rows: `25 spaces + "│Inline: $x^2$"`.
The scratch test proves anchor column 34, run bytes 8..13, and original input bytes
8..13 equal five sidebar spaces. The placement lookup therefore draws at column 8
and clears sidebar columns 8..13; the real formula remains at 34. This is a direct
trace of the renderer's lookup, not a GPU screenshot. The existing herdr test only
checks records and anchors, so it passes despite the error.

Wide reproducer: ten rows `能🙂│前 $x^2$`. The correct anchor is column 8;
run bytes 4..9 index the middle of the emoji in the original row, so the lookup
returns `None` and the inline picture disappears.

Smallest correct fix: use the region's captured byte-to-screen-cell boundaries for
each inline run and joined head, retaining the whole snapshot separately for
validation. Merely adding `region.column_start` to the current lookup is insufficient:
the substring is already wrong, and Unicode-width inference bypasses wide_spacers.
Add frame assertions for actual cleared cells and placement columns in both cases.

## B-2 — must-fix — committed rendering and hit geometry escape the region

`crates/bt-term/src/session.rs:2087`, `:8934`;
`crates/bt-render/src/lib.rs:555`, `:279`, `:317`, `:417`.

HEAD fits live artifacts against the entire terminal width. Its renderer clips to
the entire terminal seat. A 600-pixel raster in a 49-column left pane with 9-pixel
cells fits the 900-pixel terminal but crosses the rule at pixel 441. Region-scoped
source suppression does not prevent that raster from covering the neighbouring pane.
Source mode measures whole frame rows before adding the region origin, likewise
making the source band and its marks cover unrelated content.

The WIP adds `math_band_for(region)`, `right_limit_columns`, and region-local source
width measurement. These address the right-bound problem, but are not in HEAD.
The inspected WIP still uses terminal padding as `pane_left` for ground bounds,
and `MathBandFace.rows_left/rows_right` describe the full screen. A right-pane source
face starting immediately after a rule extends its left ground inset over that rule.
For narrow bands, `math_tool_boxes_px` can return buttons wider than the block;
hit testing accepts those button rectangles before testing the block rectangle.

Smallest correct fix: carry both region edges through the shared geometry used by
raster scissors, source-face rows, ground, marks, and hit tests. Clamp or hide tools
when they cannot fit. Bound fitting and horizontal scrolling to the same rectangle;
a fit-to-width change alone is insufficient at the readable scaling floor.

## B-3 — must-fix — overlapping pane bands overwrite shared vertical geometry

`crates/bt-viewport/src/lib.rs:4240`, `:4333`.

The alternate-screen `max(artifact_height, source_band_height)` rule remains, but
there is one `per_row_height` vector for the entire terminal, and every accepted
artifact assigns `*height = *distributed`. Region columns do not participate.

Concrete geometry: 18-pixel cells; a left display block on rows 0..2 needs 180 pixels
(60 per row); a right block on rows 1..3 needs only its 54-pixel source band
(18 per row). Their starts and closers differ, so both can survive row-keyed storage.
The second assignment changes rows 1 and 2 back to 18: the left block receives
96 pixels instead of 180. Its placement clip is that shared prefix interval.
Even with one tall block, unrelated sidebar/right-pane text follows the same expanded
row map. This is source-derived arithmetic; no new viewport executable was run.

Smallest fix for clipping while keeping shared-row presentation: combine overlapping
height requirements with `max`, never last-writer assignment. That still moves the
neighbour. True per-region expansion requires region-specific row projection, source
suppression, and hit mapping. A contained safe interim is to keep a framed block as
source when it needs expansion the region cannot independently provide. Existing
plain alternate-screen expand-only behavior must remain unchanged.

## B-4 — must-fix — region-aware overlap tests sit on row-only identities

`crates/bt-detect/src/lib.rs:3267`; `crates/bt-term/src/session.rs:1412`,
`:6849`, `:7203`, `:13529`.

Scratch input: ten rows `$x^2$     │$y^2$`. Detection finds both regions, but resolving
candidate row 0 returns only left `x^2`. Same-row closers are permanently omitted on
the right, not delayed until another task. Distinct closers do not solve everything:
blocks sharing a start row still overwrite each other at `insert(task.start.row, ...)`.
Repaint preservation rejects any occupied row even for disjoint regions. Unresolved
completion and refused-table retirement also evict by row without region ownership.

Two agents running the same demonstration can collide deterministically; two inline
outputs on the same row collide on every such row. Independent agents need not be
synchronized: their visible blocks only need overlapping closing row numbers. No
measured real-world frequency is claimed. For scale, independent uniform sets of
five closers each over 40 rows have 25/40 expected colliding row pairs; actual shell
layouts are not uniform. The primary effect is missing formulas; overwrites and
whole-row damage/preservation can cause source/raster flicker. Row keys alone do not
prove a raster jumps panes; B-1 independently proves wrong-pane drawing.

Smallest complete fix: candidates/tasks/signatures keyed by `(closing row, region)`,
records by `(starting row, region)`, and region-aware overlap, retirement, state
remembering, repaint occupancy, and completion validation. Check region equality
in `live_task_is_current` too. Compare dependency slices within the region so output
in the other pane does not continually invalidate an unchanged formula.

## B-5 — must-fix — 90% occupancy is not proof of independent text streams

`crates/bt-detect/src/border.rs:168`, `:251`, `:301`;
`crates/bt-detect/src/lib.rs:3161`.

Two passing scratch assertions demonstrate regressions in the detector itself:

- 36 rows `log  │ text`, followed by `$$x^2$$`, then three `plain` rows: column 5
  qualifies at exactly 90%. Unsplit detection finds one display formula; region
  detection finds none because it cuts through the formula on the exceptional row.
- An opening triple-backtick text fence, 38 rows `log  │ $x^2$`, then a closing
  triple-backtick fence: column 5 qualifies at 95%. Whole-screen detection
  correctly finds zero formulas inside the fence; region detection finds 38.
  The fence is confined to the left slice while the right starts with neutral context.

Application-shaped fixture results (all columns zero-based; harmless means the
split decision itself, independently of B-1/B-2):

| Fixture | Rule coverage and split | Consequence |
| --- | --- | --- |
| lazygit-like nested boxes, 40 rows, four junction rows | 36/40 at columns 0, 6, 12: yes | Log-only fixture harmless. A fifth junction row gives 35/40 and no split. Box presence alone does not prove independent parser context. |
| vim `:vsplit` shape, 38 content rows plus two status rows | 38/40 at column 10: yes | Independent columns are reasonable; all 38 right inline formulas survive detection. Live placement is wrong under B-1. |
| Full-height raw Markdown in a pager, ASCII pipes | 0 eligible strokes: no | Unchanged. A pager that renders Unicode borders instead has the next case's ambiguity. |
| Unicode `column -t`-style rows `left │ $x^2$` | 40/40 at column 5: yes | All 40 formulas survive detection; B-1 makes the live placement harmful. Fenced variant above is a false positive even before rendering. |
| Box-drawn banner: two cap rows and 18 body rows | 18/20 at columns 0, 11: yes | Interior formulas survive detection. At 17/19, no split. Merely two horizontal caps do not protect a tall box. |
| htop-like screen: eight meter rows, 32 process rows | 8/40: no | Harmless; no invented pane boundary. This is a model, not a claim about every htop layout. |
| Claude input box: two caps, one body row, 37 other rows | 1/40: no | Harmless. For an h-line input, verticals occupy h rows, not the caps. |
| Claude-like input occupying all 40 rows | 38/40: yes | Interior formula detection survives; B-1 affects placement. An editable full-height input must not become eligible merely because it is boxed. |

There is no evidence for choosing 95% or 100% as a universal fix. At 95% the fenced
case still fails; full-height column output reaches 100%, while raising the threshold
loses genuine panes with status/junction rows. Keep ASCII pipes excluded. Minimum
safe policy: veto inferred cuts that intersect a whole-screen proven formula/table
or break a known code-fence context, including history checkpoints. Prefer trusted
pane metadata/explicit opt-in where the pixels are indistinguishable. A neutral
checkpoint and discarded history are not justified solely by a frequent glyph.

## B-6 — should-fix — repeated border work is uncached and slicing multiplies it

`crates/bt-detect/src/border.rs:168`, `:301`;
`crates/bt-term/src/session.rs:3671`, `:5778`, `:13544`.

The finder walks every captured boundary: O(rows * columns) on ordinary text;
rule sorting and BTreeMap tallies add logarithmic factors in rule-dense input.
Region construction scans/copies each row's boundaries again for each region:
O(regions * rows * columns), potentially quadratic in columns for many borders.
There is no border/region memoization by grid generation. Candidate arming, batched
resolution, and per-completion validation call it again. Candidate signatures avoid
some task creation only after the arming pass; stability gating means this is not
literally one full scan per PTY byte or every rendered animation frame.

Scratch microbenchmark, 300 columns x 100 rows, prebuilt authoritative boundaries,
`black_box`, existing test profile opt-level=1, no live PTY/GPU: `live_screen_regions`
took 0.114 ms/call plain, 0.330 ms with one split (300 iterations each), 0.574 ms
with all-rule rows, and 5.037 ms with alternating `a│` (150 nonempty regions;
10 iterations each). This measures finder plus slicing, not the full detector.
One split costs about 20 ms CPU/second at 60 invocations/second, 330 ms at 1,000;
repeat validation and pathological region counts make it material under output load.
These are single-run local measurements, not production throughput guarantees.

Cheapest correct cache: share one immutable region snapshot across arming, resolution,
and completion checks while the captured grid is unchanged. Key by screen, dimensions,
row revisions/content epoch and capture/site metadata; grid generation alone is unsafe
unless guaranteed to advance on every relevant write. Invalidate on content changes,
resize, and screen switch. Slice all sorted regions in one row walk if many survive.

## B-7 — note — cell arithmetic mostly holds, with an empty-tail exception

`crates/bt-detect/src/border.rs:199`, `:251`, `:329`;
`crates/bt-term/src/session.rs:13753`.

Scratch CJK/emoji input proves the border at cell 4 and the region at 5; its math
anchor is cell 8, not a character or byte offset. Capture skips `wide_spacer` cells
while extending the preceding cluster's end boundary; the finder requires width 1.
On nine `a│ $x^2$` rows plus one `能 $x^2$`, inferred border 1 falls inside the
wide glyph on the exceptional row: that glyph belongs to neither slice. This avoids
half-cluster anchors but disproves the comment that straddling is impossible.
Do not suppress half a cluster when rendering an inferred region on such a row.

Adjacent/first-column rules produce no interior zero-width region. However, `│`,
`││`, and `a│` retain an empty unbounded tail starting beyond the final cell.
`│x│` retains the valid one-cell middle plus that empty tail. No panic or false
formula was observed. Smallest cleanup: give live region construction the captured
screen width, discard empty trailing regions, and skip zero-width layout rather
than treating it as one cell. Inline rendering remains subject to B-1.

## B-8 — note — the no-border fallback preserves main's detector behavior

`crates/bt-detect/src/lib.rs:3161`; `crates/bt-detect/src/border.rs:301`.

Read proof: no border returns `None`; the caller reuses the original Arc and initial
context, then the original logical-line, captured-column, clipped-prefix, scanner,
and occurrence-mapping sequence. Whole-region text is not trimmed or rebased.

Scratch differential proof: compiled a test-only copy of main's detector, ledger,
and table modules alongside the branch. It matched `580c8ede`/`cc538d03` exactly
apart from test-module removal and the ledger module import. Across 12 candidate
rows containing inline/display math, fences, indentation, trailing spaces, CJK and
emoji, exact Debug bytes matched for blocks, spans, parser checkpoints after every
line, and live results; start/end anchors, bands and refused rows matched directly.
The `main` ref advanced during review; this compares the pinned baseline,
not later unrelated main changes or all possible screens.

## B-9 — should-fix — committed tests stop short of the failing presentation paths

`crates/bt-detect/tests/framed_screen.rs:1` contains exactly these six fixtures:
`the_herdr_sidebar_no_longer_hides_the_pane`,
`the_herdr_compact_sidebar_no_longer_hides_the_pane`,
`a_plain_screen_detects_exactly_what_it_always_did`,
`a_tmux_split_typesets_the_right_pane_and_nothing_in_the_left`,
`prose_left_of_the_rule_still_splits_the_screen`, and
`a_table_drawn_inside_a_tui_never_splits_the_screen` (five table rows in forty).

Missing: every lazygit/vim/full-height pager/Unicode column/banner/htop/Claude-box
shape above, fenced and exceptional-row regressions, wide/emoji live placements,
two active panes with colliding starts/closers, overlapping tall bands, narrow-region
marks/hits, and region changes during outstanding worker completion. The six detector
fixtures do not validate pixels, cleared cells, toolbar geometry, or repaint survival.

Validation: `cargo test -p bt-detect -j 4` passed 160 unit tests, six framed fixtures,
one existing strand test, and the scratch evidence test. Permitted bt-term filters
`herdr_pane_rows_typeset_behind_their_sidebar` and
`alternate_short_block_keeps_its_source_band_and_does_not_move_input` each passed.
WIP filter `a_formula_in_a_split_is_fitted_to_its_pane_and_drawn_inside_it` passed on
retry after a transient incomplete-import compile failure. The updated WIP herdr
test, now checking inline placement cells, also passed. Neither fix was committed.
No renderer tests were run. Promote the scratch regressions and add frame/geometry
assertions with the fixes. Scratch sources and generated results were removed.
