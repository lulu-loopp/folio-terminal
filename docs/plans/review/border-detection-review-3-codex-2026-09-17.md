# Border detection: third adversarial review

Reviewed `fix/formulas-behind-a-border` at `22c7aa1f15ee2fdb799ee119c8aa2c87f7f67457`.
Read both earlier reviews and the complete changes in this commit.
Verdict: **do not merge; not safe for the 0.4.3 release yet**. The reported fixes
work on their original examples, but two small variations still lose proven math.
Only this review is committed. Product code stayed unchanged; temporary integration
tests were removed. No application, service, PTY, or GPU process was started/stopped.
References below are to the reviewed HEAD; abbreviated crate paths begin at `crates/`.

## B-1 through B-6 and round-2 residuals

| Finding | Status | Closing line / residual evidence |
| --- | --- | --- |
| B-1: inline bytes and cell origin | **Closed** for the reported defect | `bt-term/src/session.rs:14269` and `:14270` use `bt-detect/src/border.rs:679`'s captured-boundary lookup. Sidebar anchor 34, CJK/emoji anchor 8, and explicit straddling slice origin 4 agree. Joined fragments share `live_fragment_cells` (`session.rs:14243`). |
| B-2: geometry escapes the region | **Closed** for the reported defect | `session.rs:8935`, `:8960`, `:8992`, `:9213`; `bt-viewport/src/lib.rs:3461`; `bt-render/src/lib.rs:449`, `:581`, `:15771` carry both bounds; `:330` hides tools that cannot fit. Session fitting/placement tests pass; no new GPU or renderer-test claim. |
| B-3: shared-height overwrite / clipped-top addition | **Closed** under the accepted shared-row policy | `bt-viewport/src/lib.rs:4381` computes each hidden extent; `:4397` folds it before `:4402`'s maximum. Six clipped-top orders give 216 px; ordinary triples give 210/240/270 px in every order on both screens. Neighbouring text still shares the row stack. |
| B-4: row-only ownership | **Open, accepted limit** | `bt-detect/src/lib.rs:3391` returns the first region; `session.rs:7316` installs by start row. Ten two-formula rows still give 20 regional blocks but ten live tasks. Same-start replacement, whole-row comparison (`:13623`), and no explicit region equality (`:13655`) also remain; not a new merge condition. |
| B-5: occupancy is not stream independence | **Partly** | Dollar edge veto at `border.rs:358`; junction continuation at `:355`; fence ownership at `bt-detect/src/lib.rs:1549`; upstream context at `:3262`/`:3324`. Original examples close, but R3-1/R3-2 below preserve the formula-loss class. |
| B-6: repeated border work | **Partly** | `border.rs:579` memoizes the last capture per thread, including no-split answers. Same-Arc reuse closes repeated geometry/slicing there. `:627` still slices every row per region; fresh captures and other threads miss. This is not a cache shared across session and worker threads. |
| R2-1: exempt-edge content / wide lookup | **Partly** | Original dollar examples and the shifted-wide lookup are fixed; dollar-free delimiters still fail (R3-1). The old wide-dollar edge now declines splitting entirely. |
| R2: junction layouts / fence checkpoint | **Closed** for the examples | `border.rs:283` keeps junctions; two right-pane displays survive `┼`, `├`, and `┤`. `grid_initial_context` preserves both a supplied open fence and a literal primary-history opener. Plain horizontal interruptions remain unsupported. |
| B-7: empty tail | **Open, benign observed** | `border.rs:438` still appends an unbounded region after the last rule, including `│`, `││`, `a│`, and `│x│`; no new panic or false formula observed. |
| B-8: no-border equivalence | **Closed** for the exercised path | `bt-detect/src/lib.rs:3246` retains original inputs/context. Twelve mixed math/fence/indent/CJK/emoji lines match flat detection exactly. The old pinned-main checkpoint differential was not repeated. |
| B-9: coverage / documentation | **Partly** | Permanent edge-dollar, junction, boundary-map, pane-fence, 37-block table, wide session, and six-order clipped-top tests pass. Topology-change completion and dedicated narrow hit/geometry coverage remain absent. `border.rs:45` now gives the correct bare crossing example; `:495` still calls straddling impossible although `:945` tests it. |

## R3-1 — must-fix: no dollar does not mean no formula

`bt-detect/src/border.rs:295`, `:358`. Start with forty `log  │ text` rows
and replace row 0 or row 39 with `\[x^2\]` (one backslash at each delimiter).
The scanner supports this display syntax. Whole-screen detection finds **one**
block; column 5 nevertheless survives, and regional/live detection find **zero**.
The row crosses the proposed cut, but contains no `$`, so it spends the exemption.
Both edge positions were exercised through the public detector and live resolver.
The dollar regression is repaired; the stated formula-preservation property is not.

Minimum merge condition: protect every supported proven formula delimiter/source
span from an exempt cut, not just dollar-bearing rows; promote these two cases.
Do not infer formula absence from a character test narrower than the math grammar.

The requested price attack is a separate conservative cost: replacing a crossing
`status cost 5` bottom row with `status cost $5` vetoes the split. A right-pane
three-row display then changes from one detected block to zero because whole-screen
left-pane prose obscures its delimiters. This is acceptable as an explicit preference
for native source over a destructive split, but a legitimate status line can contain
a price, shell variable, or dollar symbol. “A status line never carries math” is not
evidence that a dollar distinguishes status from content. The veto applies only
when the row needs the exemption; rule-bearing or clear rows bypass it at `:355`.

## R3-2 — must-fix: a blank cut can run through a formula

`bt-detect/src/border.rs:268` tests only the cut cell and its immediate neighbours;
`:355` accepts that local clearance before asking about dollars or row position.
Use forty `log  │ text` rows, replacing row 20 with `$$x   +y$$` (three spaces).
Column 5 is blank, column 4 is blank, and column 6 is `+`: `clear_at` returns true.
The rule has 39/40 coverage and survives. Flat detection finds **one** display;
regional and live detection find **zero**. No edge exemption is involved.

A table-shaped reproduction makes the consequence concrete: forty rows
`│ a  │ text    │`, replacing row 20 with `│$x   +y$      │`.
Outer rules remain at columns 0 and 15; the omitted inner rule is column 5.
The formula occupies a merged cell across that missing rule. Borders remain
`[0, 5, 15]`, and flat/regional/live counts are again **1/0/0**.
Thus a table drawing `│` on only some rows can still be cut through valid content.
Less than 90% same-glyph coverage refuses the candidate; sufficient coverage plus
locally clear gaps accepts it. A crossing non-clear interior gap vetoes instead.

Minimum merge condition: decline cuts intersecting a whole-screen proven formula
span, including whitespace within the span; add both reproductions permanently.
This restores the original B-5 safety condition without redesigning B-4 ownership.

For a genuinely independent table cell bounded by an intact rule, its own formula
cannot span two cells by definition. That semantic assumption is not enforced by
the scanner: forty `left │ text` rows with row 20 `$x   │+y$` still produce flat/
regional/live counts **1/0/0**. The unsplit scanner accepts the rule inside math.
This is a parser ambiguity, not proof that independent cells actually share math.
The committed full-screen table fixture passes with **37** blocks, but compares
counts (`bt-detect/tests/framed_screen.rs:276`), not block identities, and covers
ordinary cell-local inline formulas only. “Harmless” at `border.rs:52` is too broad.

## Fence opening row and cache lifetime

The opening-row rule behaves as specified. With bare triple backticks on row 0,
ordinary `log  │ $x^2$` rows, and `     │` followed by triple backticks on row 20,
flat, regional, and live detection all return **zero**, including below row 20.
The apparent pane closer is not a whole-screen closing fence: the rule precedes it.
Six spaces before the closer also fail to close the whole-screen fence. The global
veto consequently lasts to screen end unless an unsliced valid closer arrives.
This conservatively suppresses independent output; it does not leak fenced math.
Pane-local opening/closing rows leave the other pane's forty formulas intact.
Initial checkpoint and primary-history fence probes now both refuse live candidate 2.

The memo owns one strong capture Arc and an optional split per thread
(`border.rs:559`, `:573`), using `Arc::ptr_eq` at `:582`. Holding the Arc prevents
address reuse; no generation-only key or content hash is trusted. Scratch checks
prove repeated calls share region Arcs, equal-but-distinct captures recompute, and
replacement releases the old capture even while its returned split is retained.
Dropping the caller's last capture Arc does **not** release a cached capture.
It survives until that thread caches another capture (`:589`) or its TLS is dropped
at thread exit; “exactly the lifetime of a frame” (`:572`) is inaccurate.
There is no growth proportional to capture count: one entry replaces another.
Retained bytes still scale with capture size, history, regions, and thread count;
the split owns an unsliced grid copy and region data. Idle threads retain their last
entry, and no byte budget exists. Returned splits/tasks can have additional owners.
`session.rs:5796` builds a fresh Arc per capture, so identical recaptures miss too.
The supplied 0.39→0.0013 / 5.85→0.51 ms figures were not rebenchmarked; they measure
same-capture helper reuse, not end-to-end or cross-thread throughput.

## Fixture rerun and validation ledger

All committed round-1/round-2 fixtures pass on this HEAD. Deleted historical scratch
sources were reconstructed from the reviews; old assertions pinning bugs cannot
literally remain passing after fixes. Expected outcomes were updated explicitly:

| Reconstructed family | Result on this HEAD |
| --- | --- |
| Sidebar / CJK+emoji / shifted-wide / bare dollar crossing at top, middle, bottom | Correct anchors; crossing-dollar screens retain one formula without splitting. |
| lazygit-like four junctions / 2×2 | Rules survive; 2×2 retains both displays. |
| vim two crossing status rows / one middle interruption | No split; existing inline counts retained. Either single edge without dollars still splits. |
| ASCII pager / full-height Unicode columns | No split / split; forty inline blocks each. |
| Two-cap banner / full-height Claude-like box | Now split; 18/38 inline blocks retained. A 17-of-19 body still misses 90%. |
| htop eight meter rows / short one-row box shape | No split. |
| Visible fences / upstream fences / two active panes / empty tails | Fence refusals fixed/preserved; accepted row collision and empty-tail limits reproduced. |
| Ordinary triple / hidden-top triple / no-border mixed text | All six height orders agree; flat and regional blocks match for no-border text. |

Passed `cargo test -p bt-detect -j 4`: 166 unit, nine framed, one strand, plus
13 temporary tests. Some temporary assertions deliberately prove the failures above;
their passing is evidence of the defect, not a claim of correct product behaviour.
Passed `cargo test -p bt-viewport -j 4`: 139 unit, three existing integration,
plus one temporary test covering ordinary triples in six orders on both screens.
Passed `cargo test -p bt-term --lib <filter> -j 4`, one test for each filter:
`herdr_pane_rows_typeset_behind_their_sidebar`,
`a_formula_in_a_split_is_fitted_to_its_pane_and_drawn_inside_it`,
`alternate_short_block_keeps_its_source_band_and_does_not_move_input`, and
`a_framed_pane_of_wide_characters_places_its_formula_in_the_grid_cells`.
The read-only machine-path gate passes: 1,254 tracked files scanned, no violations.
No renderer suite, heavyweight build, release packaging, or live app capture ran.
Synthetic reproductions establish detector/geometry behaviour, not application pixels.

For 0.4.3, close R3-1/R3-2 and pin them before reconsidering merge. The accepted
B-4 limitation is not promoted into an additional release blocker by this review.
