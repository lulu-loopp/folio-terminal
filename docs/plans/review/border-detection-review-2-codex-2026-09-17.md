# Border detection: second adversarial review

Reviewed `fix/formulas-behind-a-border` at `1d8104324b8a77843713fb642b5650ebc0de6695`.
Read both complete changes, `37b337e2` and `1d810432`, the updated `border.rs` header,
DESIGN §4.6e, and the first review. References below are to this reviewed HEAD.
Verdict: **do not merge**. The ordinary pane fixes work, but the exemption still
loses previously detected formulas and permits incorrect inline placement.
Keep this out of **0.4.2**; revisit for the next release after those regressions
are fixed and the accepted limitations have permanent regression coverage.

Product code remained unchanged. Only permitted package tests and repository/read
operations ran; no application or service was launched or stopped. Fixtures below
are synthetic, reconstructed from round 1 where its deleted scratch sources were
unavailable. They are not captures of installed lazygit, vim, Claude Code, or tmux.

## B-1 through B-5

| Finding | Status | Closing line / remaining evidence |
| --- | --- | --- |
| B-1: region bytes against whole-screen text | **Partly** | `bt-term/src/session.rs:14259` reads `live_region_text`; `:14198` fixes logical text too. Original offsets now work. `:14271` still reconstructs cells from text plus the region origin; the exempt-wide-row counterexample below is wrong. |
| B-2: raster, source, ground, and hit bounds | **Closed** for the reported geometry | `session.rs:8935` fits the region, `:8960` measures regional source width, `:8992`/`:9213` carry left limits; `bt-viewport/src/lib.rs:3461` carries display limits. `bt-render/src/lib.rs:581` bounds ground, `:449`/`:450` bound source rows, `:15771` bounds ink/scissor, `:330` hides tools that cannot fit. |
| B-3: later pane shortens an earlier band | **Partly** | `bt-viewport/src/lib.rs:4374` closes the original overwrite with `max`, preserving ordinary bands across three regions. `:4398` still adds clipped-top height after merging; order-dependent overexpansion remains. |
| B-4: row-only candidate/record identities | **Open, explicitly accepted limit** | No closing code line. `bt-detect/src/lib.rs:3327` returns the first region; `session.rs:7316` keys installation by start row. DESIGN `docs/DESIGN.md:1004` documents the decision. This review does not make the accepted redesign a new merge condition. |
| B-5: occupancy is not stream independence | **Partly** | `border.rs:300` invokes the unbroken-frame check; `bt-detect/src/lib.rs:1509` and `:3255` veto visible whole-screen fences. Both original regressions close. The edge exemption still cuts through a proven formula, and live fence proof resets its checkpoint at `:3239`. |

Paths abbreviated above are under `crates/`, except DESIGN. B-2 closure is a code
trace plus passing session tests, not a new GPU/pixel or renderer-test claim.
B-4 also retains same-start replacement, whole-row dependency comparisons, and
no explicit region equality in `session.rs:13595`'s completion validation. The
documented same-closer omission is not the entire remaining ownership limitation.

## R2-1 — must-fix: an exempt edge is still sliced through real content

`border.rs:260` permits one non-clear row at either edge, but regions still include
that row. Use 36 rows `log  │ text`, three `plain` rows, and `$$x^2$$` last.
Column 5 qualifies; ordinary detection finds one display formula, regional and live
detection find zero. Put the formula first instead: the same 1-to-0 regression.
Put it at row 36 with the three plain rows after it: the new veto preserves it.
Thus the first review's B-5 counterexample was repaired only away from the edges.
Calling the edge a status line does not establish that its text is disposable.

The exemption also exposes the unfinished B-1 cell mapping. Ten rows, with row 0
`a能 $x^2$` and rows 1..9 `a│ $x^2$`, qualify at column 1. The right region starts
at column 2, but its first retained boundary on row 0 is column 3: `能` spans 1..3
and is omitted by slicing. The slice is ` $x^2$`, with run bytes 1..6.
Detection correctly anchors the formula at column 4. The actual lookup expression
at `session.rs:14271` computes 2 + 1 = **3**, claiming cells 3..8 instead of 4..9.
That clears the preceding space and leaves the final dollar; it is not the original
26-column error, but remains incorrect placement/suppression. Scratch assertions
exercise detection and that lookup arithmetic; this is not a rendered screenshot.

Required: preserve unsliced semantics for exempt crossing content (or decline the
split when it intersects a proven block), and map inline bytes through retained
captured boundaries. A constant region origin is insufficient when slicing drops
a straddling cluster. Pin the actual cleared cells, including joined fragments.

## Frame-policy attacks: two edges, middle status, and a tmux 2×2 layout

On 40 rows `left      │$x^2$`, replace row 0 or 39 with a crossing status line:
column 10 survives. Replace both: no border, although coverage is 38/40 = 95%.
Replace row 20 alone: no border at 97.5%. This implements the ruling exactly;
it excludes full-height two-cap boxes and interior title/status interruptions.
The lazygit-like nested box and Claude-like full-height box from round 1 now fail
the split test. No claim is made that a particular application version always
draws those shapes. A short Claude-like input box remains below the share threshold.

Concrete 2×2 fixture: 40 rows with a vertical rule at column 10; row 19 is
`──────────┼──────────`. Put right-pane display blocks on rows 1..3 and 22..24
(`$$`, `x^2`/`y^2`, `$$`), with left-pane prose. Coverage is 39/40, yet the vertical
rule **does not survive**. The old splitting policy detects two display blocks;
the new policy detects zero. Separate interior `├` and `┤` variants also veto.
Inline examples alone conceal this loss because the unsplit scanner can still
recognize inline math after prose. A bottom status row cannot rescue the junction.

This is a defensible conservative false negative, but an overly broad cost for a
feature described as supporting multiplexer panes: a connected horizontal split
is evidence of a frame too. Under the owner's ruling, document 2×2/nested layouts
as unsupported; do not silently count every junction as a vertical rule, which
would reopen table/box ambiguity. Supporting them needs connected-frame topology
and horizontal pane boundaries, not another occupancy percentage.
Tmux's [manual](https://man.openbsd.org/tmux#pane-border-status) distinguishes pane
border status from the global [status position](https://man.openbsd.org/tmux#status-position).
A single global top status row fits the exemption. An interior pane title only
vetoes if it actually breaks/crosses this column; a title confined to one side with
the vertical glyph intact does not. These are shape conclusions, not live captures.

## Fence attacks: the visible veto works, with a broad cost and a checkpoint gap

The original opening fence, 38 `log  │ $x^2$` rows, closing fence now yields zero
formulas in both text-region and live resolution paths, while retaining the split.
For a fence opening in the left pane and closing in the right, use three backticks
alone on row 0, row 20 `     │````, and ordinary formula rows
elsewhere. The right closer is not a closing fence on the unsliced line: the rule
precedes it. All formulas, including those below row 20, remain vetoed. Replacing
that row with six spaces then backticks also fails to close at that indentation.
Observed plain/regional/live counts are all zero. There is no cross-pane false
positive; independent right-pane output is suppressed through the screen's end.
That is the literal owner's policy, not evidence of genuinely shared pane context.

The live proof uses only grid rows (`border.rs:425`) and a default checkpoint
(`bt-detect/src/lib.rs:3239`). A task whose documented initial checkpoint is already
inside a fence, with 40 grid rows `log  │ $x^2$`, still resolves candidate row 2.
Also tested on primary with a literal opening fence in a preceding history input
and command-output sites: the split resolves; replacing the rule with a space
retains the unsplit fence context and refuses the same candidate.
This preserves the round-1 history/checkpoint gap: the new veto covers fences
visible on the screen, not fences established before it. Carry a valid upstream
fence proof into the veto, or explicitly scope this limitation in the design.

## Three-region height evidence

Using the public viewport projector, 18 px cells, and disjoint column regions:
A occupies rows 0..2 needing 180 px, B rows 1..3 needing 54 px, C rows 2..4
needing 270 px. All six artifact orders, on both primary and alternate screens,
give band heights A=210, B=240, C=270. No band is shortened; shared neighbouring
text moves with the common rows, as the owner accepted. This closes the original
96-of-180-pixel failure mechanism, including its generalization to three regions.

Clipped-top variant: all three occupy rows 0..2 with one clipped-top row, requiring
144/72/216 px. Across the six orders the tallest band's clip becomes
252, 252, 234, 252, 270, 270 px instead of a consistent 216 px maximum.
`lib.rs:4398` adds each hidden top extent onto an already merged row. Fold that
extent into each artifact's own first-row requirement *before* taking `max`.
This is extra height/order dependence, not clipping. Same-start B-4 prevents the
ordinary session map from installing this exact triple today; the public projector
still fails the general rule. Treat it as a projection gap, not a proven live flicker.

## Round-1 fixture rerun ledger

| Reconstructed fixture | Round 2 versus round 1 |
| --- | --- |
| 25 blanks + `│Inline: $x^2$` | Fixed: detector anchor and lookup both 34; bytes still 8..13 locally. |
| `能🙂│前 $x^2$` | Fixed: anchor and lookup both 8; no invalid whole-row UTF-8 slice. |
| Interior crossing formula | Fixed: no split, one display formula instead of zero. |
| Visible whole-screen fence | Fixed: zero regional/live formulas instead of 38. |
| lazygit-like four junction rows | Changed: no split; the log-only shape remains harmless. |
| vim-like two crossing status rows | Changed: no split; all 38 inline formulas still detected. |
| ASCII pager / full-height Unicode columns | Unchanged: no split / split; 40 inline formulas each. |
| Two-cap banner / full-height Claude-like box | Changed: no split; 18 / 38 inline formulas still detected unsplit. |
| htop-like eight meter rows / short Claude-like box | Unchanged: no split. |
| Two formulas per row, `$x^2$     │$y^2$`, ten rows | Unchanged B-4: 20 regional blocks, ten live tasks, left wins. |
| Nine `a│ $x^2$` rows, final `能 $x^2$` | Still splits; wide cluster omitted at the exempt edge. New shifted-wide variant fails placement above. |
| `│`, `││`, `a│`, `│x│` tails | Unchanged: an empty unbounded trailing region still exists (B-7). |
| Ordinary no-border mixed math/fence/indent/CJK/emoji | Exact flat-versus-region block equality on 12 lines. Original pinned-main checkpoint differential was not repeated. |

## Per-frame cost after measuring runs (B-6 remains open)

Paired local microbenchmark: old `947e31b1` border module versus HEAD, 300×100,
prebuilt cell boundaries, `black_box`, existing optimized test profile, serial
test execution. Median of five batches; 300 calls/batch for plain/one split,
ten for dense cases. Measures `live_screen_regions`, not full detection or pixels;
the comparison includes both commits, not just the isolated run-measuring change.

| Shape | Old ms/call | New ms/call |
| --- | ---: | ---: |
| Plain | 0.114 | 0.232 |
| One split | 0.253 | 0.496 |
| All rules | 0.579 | 1.241 |
| Alternating `a│`, 150 nonempty regions | 4.253 | 4.679 |

Run measurement adds work even without a border. It avoids rescanning graphemes
for every candidate's clear-cell check, but `covered` (`border.rs:186`) linearly
searches runs. For R rows, C cells, K candidates, and G nonblank runs per row,
geometry is O(RC), with rule tally/search factors; gap checks can add O(KRG).
Slicing still scans boundaries per region, O(regions × R × C), and a split now
also clones the unsliced screen. Fence scanning and per-block linear ID lookups
add resolver work not included in the table. There is still no shared snapshot cache.
Arming, resolution, and completion validation repeat the work (`session.rs:13688`,
`:3758`, `:13651`); stability gating means this is not every rendered animation frame.
At 60 one-split calls/s this helper alone costs about 30 ms CPU/s; at 1,000,
496 ms CPU/s. Cache by complete capture identity and slice regions in one row walk.

## Validation and merge conditions

Passed `cargo test -p bt-detect -j 4`: 162 unit, seven framed, one strand test;
17 temporary tests also passed, including copied old-border tests and assertions
that pin the failures above. Passed `cargo test -p bt-viewport -j 4`: 138 unit,
three existing integration tests, and two scratch tests covering the permutations.
Passed permitted bt-term `--lib` filters: `herdr_pane_rows_typeset_behind_their_sidebar`,
`a_formula_in_a_split_is_fitted_to_its_pane_and_drawn_inside_it`, and
`alternate_short_block_keeps_its_source_band_and_does_not_move_input` (one each).
No renderer tests or heavier build ran. All scratch sources were removed afterwards.

Before merge, fix R2-1's formula loss and captured-cell lookup and promote those
regressions to permanent tests. B-4 remains accepted; clipped-top aggregation,
upstream fence context, unsupported junction layouts, and roughly doubled common
helper cost still argue for the next release, not 0.4.2. B-9 coverage is improved,
but permanent three-region, edge-wide, topology-change, and geometry/hit tests remain.
Documentation also needs cleanup: `border.rs:78` still says junctions are absorbed
by the unused tenth; the header and DESIGN's `log  │ $$x^2$$` counterexample draws
the rule and does not cross it. The actual regression fixture uses bare `$$x^2$$`.
