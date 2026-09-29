# Formulas inside a multiplexer pane: detection reads pane columns, not screen columns — design note, 2026-09-29

0.4.7 ticket 69, phase 1 (docs only). Base: main `6abd3f5e`. Codex reviews this note; the owner rules on §10; the code tickets are then written from it.

The owner's ruling of 2026-09-29: ticket 69 ships in 0.4.7, and the never-merged branch `fix/formulas-behind-a-border` (nine commits of 2026-09-16/17, ending at `0fa4af14`) is **the specification and the evidence, not a rebase target**. The branch is kept as a patch (`fix_formulas-behind-a-border.patch`, with its commit list, in the coordinator's stale-branch folder; triage in `stale-branches-2026-09-26.md` §2). Its three Codex reviews travelled inside the patch and never reached main; §6 carries their findings and rulings. Below, "the branch" means that patch at `0fa4af14`, and "hunk X" names the file the hunk changes. Facts about main come from grep on `6abd3f5e`, never from line numbers.

---

## 0. The decisions, in one screen

1. **The defect is still on main.** Inside `herdr`, or in a `tmux` vertical split, every pane row reaches the host grid as `<frame cells>│<pane text>`. The detector reads that as one line. `<25 blanks>│$$` is indented code, and its trimmed form opens on `│`, not `$$`, so nothing in the pane is ever typeset. `bt-detect` on main has `lib.rs`, `ledger.rs` and `table.rs` and no border or region API; nothing in `docs/DESIGN.md`, `docs/RULES.md` or `CHANGELOG.md` mentions herdr or tmux panes.
2. **The region model is the branch's final model, with one rule made general** (§2). Border columns are found from the captured grid. Regions are the column spans between them. Every gate of the detector runs unchanged over a region's own line. The branch's final veto against cutting a formula (`math_spans_on_line`) asks one line at a time, so a three-row display block whose rows the frame does not run through can still be cut in half and typeset wrong. This note found that by reading the patch (§6.4). The rule is therefore restated as a veto from **the unsplit screen's own proof**: a column is not a border if a block proven on the unsplit screen would lose rows to the cut.
3. **Owner: `bt-detect`.** The frame is a pure function of one live capture, and `bt-detect` measures it. The door is one new type, `bt_detect::LiveCapture`: the captured inputs plus the parser checkpoint and options they are scanned under, and the frame, measured once on first ask and dropped with the capture. It replaces the branch's thread-local memo, which Codex's third review found outlived its frame. `bt-term` stays the only reader of the grid (`DualPlaneSession::live_detection_context`, which becomes `live_capture`). A proven occurrence carries its region as an attribute, the way it carries `start` and `end`. `bt-viewport` and `bt-render` only receive column limits (§3).
4. **What changes downstream is what makes a block the pane's**: the band width it is fitted to, the source-face width, the column where an inline run's cells come from, the fold width of its logical line, its left and right drawing limits, and its marks. Two bands on the same rows combine their heights with `max`, and a band's hidden-top height is added to its own first row *before* combining. All of this is the branch's behaviour after Codex's reviews B-1…B-3 were closed (§6).
5. **The branch's accepted limit still stands, and it is bigger than the branch said.** Live identity is keyed by row. When two panes close a block on the same row, the left one wins. Beyond that, a row's stability, its re-arm on damage and the completion check all hash or compare the **whole screen row**. So a formula in one pane does not settle while the other pane keeps writing on the same rows. The expected case is two agents side by side; another is herdr's sidebar redrawing a status glyph. That is ticket **69b** (§9).
6. **The cut is two tickets:** **69a T-PANE-COLUMNS** (L: detection plus presentation, which cannot ship apart) and **69b T-PANE-IDENTITY** (M–L: live identity and stability keyed by (row, region); an ownership change, so this note is its design note too). 69a alone is strictly better than today: a pane's formulas typeset once its rows are quiet. With only 69a, same-row collisions and a busy neighbour cost what today costs. §10 asks whether 69b is also 0.4.7.

---

## 1. The defect, and the fixtures that prove it

A multiplexer does not pass its panes' bytes through. It repaints the host screen itself, and every pane row carries the frame. The shapes come from the branch's byte captures (`herdr` 0.8.2, `herdr_client.bin` and `herdr_compact.bin`, 100×40, `TERM=xterm-256color`; described in the header of the branch's `tests/framed_screen.rs` hunk and in its first review, which read the capture's REPORT):

| fixture | row shape | pane text starts at |
|---|---|---|
| herdr sidebar | 25 blanks, `│` (U+2502), pane text | column 26 |
| herdr compact sidebar | 3 blanks, `│`, pane text | column 4 |
| tmux vertical split | left pane padded to 50 cells, `│`, right pane | column 51 |
| plain screen (regression guard) | the pane text itself | column 0 |

The pane content in all of them is the branch's **13-line `PANE` fixture**: one inline formula (`Inline: $e^{i\pi}+1=0$ stays inline.`), a `$$ \frac{1}{2} $$` block and a `$$ \begin{pmatrix}…\end{pmatrix} $$` block. It is synthetic text with the captures' geometry, so the standing fixture rule (no bytes from the owner's recordings) is met. The plain-screen guard is the same 13 lines unframed. It must detect exactly what the unsplit detector detects.

Why the fix is recognising the frame, not relaxing a gate: both refusals are right about the line they are shown. That line is `<25 blanks>│$$`, which is indented code with no opening delimiter. What nobody ever printed is that line. The fix shows each gate the line the program did print.

---

## 2. The region model

Terms: a **capture** is one `Arc<[LiveDetectionInput]>`: an optional bounded tail of frozen history, then every grid row with its text and its **captured cell boundaries** (from the terminal's cells, never inferred from Unicode width). A **grid row** is a `LiveDetectionSource::Grid` input. Columns are cells.

**R1. What nominates a column.** One of the glyphs whose whole form is a vertical stroke (`│ ┃ ┆ ┇ ┊ ┋ ╎ ╏ ║`: the branch's `vertical_rule`, hunk `bt-detect/src/border.rs`) stands in column *c* on at least **nine tenths of all grid rows**, and on at least **three** of them. The denominator is every grid row of the screen, never a subset. That is what keeps a table drawn inside a TUI from cutting anything: its rule reaches a handful of rows, while a pane rule runs the full height by construction. The glyph must be one cell wide in the captured boundaries.

**R2. Carrying the line is not drawing the rule.** On a row that does not draw the nominating glyph, any glyph of U+2500–U+257F with a vertical stroke keeps the line alive (`┼ ├ ┤ ┬ ┴`, corners, arcs, doubles: the branch's `continues_vertical`). So the middle row of a tmux 2×2 layout keeps the vertical border. A row of plain `─` breaks it: that is a horizontal pane boundary, which this model does not handle (§5).

**R3. The frame is unbroken** (owner's rulings 2026-09-16 and 2026-09-17). On every grid row that does not carry the line at *c*, column *c* must be **clear**: the cell is blank, and the row's text does not cross it (no text immediately left *and* immediately right of it). **One** row may break this, and only the topmost or the bottommost: a status line. It qualifies only if it carries **no delimiter the math grammar recognises** (`$`, `\[ \] \( \)`, `\begin{…}`/`\end{…}` of a math environment: the branch's `line_carries_math_delimiter`). This is the grammar's question, not a character test, because `\[x^2\]` has no dollar (ruling 2026-09-17, Codex R3-1). §10 Q2 asks whether R5 should replace the delimiter test.

**R4. ASCII `|` never nominates.** A screen whose rows all carry `|` in one column is far more often `mysql`, `column -t` or a long GFM table than a frame, and nothing local tells the two apart. A missed split costs what today costs. A wrong one re-cuts a table that `bt_detect::table` owns (its `box_drawing_characters_never_trigger` test already keeps box glyphs out of GFM tables, so the two owners do not overlap).

**R5. A formula the screen proves outranks a split the screen only infers — as a veto from the unsplit proof** (this note's generalisation of the 2026-09-17 ruling "a formula's own blanks are never a cut"). Run the detector once over the **unsplit** capture: this is today's scan, byte for byte. A candidate column *c* is refused if some block that scan proves both

- (a) stands on at least one grid row that does **not** carry the line at *c*, and
- (b) has a cell extent (the smallest `cell_start` to the largest `cell_end` across its live `cell_segments`, in screen columns) that covers *c*.

This covers everything the branch's per-line `math_spans_on_line` covered (`$$x   +y$$` on a middle row; the table cell a formula merged across a missing inner rule). It also covers the case per-line spans miss: a block spread over several rows, some of them blank at *c* (§6.4). It asks the grammar and the site rules exactly as the product does, so nothing narrower than the grammar can call a formula "absent", which was Codex's R3-1 lesson. Clause (a) is what keeps a proven frame authoritative. A block whose every row carries the line has been *read across* the rule by the unsplit scan (`$x   │+y$`, two panes' text with a dollar on each side), and under a proven frame it is two panes' text. The split stands, and that one pairing is deliberately not protected (§5).

**R6. Regions.** The border columns cut the grid into regions: column spans `[start, end)` between consecutive borders and the screen edges. A border column belongs to no region. A span with no columns is dropped: two adjacent rules, a rule in column 0, or **a rule in the last column**, whose empty trailing region the branch kept (Codex B-7). `end` is always known, from the grid rows' `captured_columns`, so there is no open-ended region. A screen with no border has exactly **one region, the whole width**. Its scan is the scan that always ran: the same inputs, the same checkpoint, the same history prefix, the same `clipped_tail`.

**R7. Region text.** A region's line for a row is the row's bytes whose clusters lie **wholly** inside the region's columns. A cluster straddling an edge belongs to neither side, and its cell boundaries stay **screen columns**. So every coordinate a region proves (cells, anchors, inline runs, the joined head) is already in screen columns, and nobody ever adds a region origin to a width measured over a slice. That addition was Codex B-1 and R2-1: one cell short when a wide cluster straddles the region's first column. A row that ends its logical line drops trailing blanks, and a continuing row keeps them (the §4.6a rule, unchanged). A row keeps its `continues` flag, so one logical line of the screen is one logical line of each region, at the same index.

**R8. Each region is scanned on its own** from a **neutral** checkpoint, with the frozen history prefix dropped. A frame proves that a program is repainting the host screen, so no scrollback line runs into a pane. Every gate (indented code, clean `$$`/`\[`/environment delimiters, the prose and completeness rules, inline sites, `clipped_tail`, table refusal) runs unchanged over region-local lines.

**R9. Fences** (owner's rulings 2026-09-16 and 2026-09-17). A fence is proven once, on the **unsplit** lines. Its **opening row** decides who owns it:
- opened on a row the frame does **not** run through (the row does not carry the line in *every* border column): **the screen's fence**, which suppresses every region's blocks on the rows it covers, its own two lines included;
- opened on a row the frame **does** run through: **that pane's fence**. Its own region's scan refuses it as it always has, and the other panes are left alone;
- already open before the first grid row (the caller's checkpoint, advanced through the frozen tail): the screen's, suppressing from the top. This closes Codex's round-2 "upstream fence" gap. The branch's `grid_initial_context` is the shape.

**R10. What a region is to presentation.** A block proven in region *r* owns *r*'s columns: it is fitted to *r*'s width, drawn and scissored between *r*'s edges, its source face measured and drawn inside them, its inline line folded at *r*'s width. A block too narrow for both marks shows neither. The live rows remain **one vertical stack** (one top per row across the whole width). Overlapping height requirements combine with `max` (owner's ruling 2026-09-16, Codex B-3), and a band's hidden-top extent is added to its own first-row requirement before the combine (owner's ruling 2026-09-17), so the result is the same in every order. On an unframed screen, a row that only one band claims is sized exactly as before.

In five sentences: border columns are found from the captured grid, as columns where a vertical box-drawing glyph stands on nine tenths of all rows and every other row is clear, except one math-free status line at an edge. A column is also refused wherever the unsplit screen proves a formula that the cut would take rows from, so a formula's own blanks and a merged cell are never a cut. A frame of unbroken, unproven-across columns cuts the screen into column spans, and each is scanned alone from a neutral checkpoint with every existing gate unchanged. A fence opened on a row the frame does not run through, or open before the screen began, belongs to the screen and suppresses every pane; a fence opened inside a pane is that pane's alone. A table's `│` inside a TUI never reaches nine tenths of the rows, ASCII `|` never nominates, and a screen with no frame is one region whose scan is byte-for-byte today's.

---

## 3. Who owns what, and the door

| fact | owner | written by | read by |
|---|---|---|---|
| **the live capture** (grid rows' text, boundaries, sites, the history tail, the checkpoint before it, detection options) | `bt-term::session::DualPlaneSession` (the only reader of the grid) | `DualPlaneSession::live_detection_context` + `live_initial_detection_context`, **merged into one `live_capture()`** returning a `bt_detect::LiveCapture` | arming, resolution, completion checks, the ownership ledger and isolation gap, presentation lookups |
| **the frame of a capture** (border columns, regions, which rows the frame runs through, and each region's sliced inputs) | `bt-detect` (new module `frame`, the branch's `border.rs` redone) | `LiveCapture::frame()`: measured once, on first ask, into a `OnceLock` inside the capture; never written again | every live entry point of `bt-detect`; `bt-term`'s arming walk |
| **the region a block was proven in** | the proven occurrence: `LiveDetectionTask::region`, then `LiveDecorationRecord::region` | `bt_detect::apply_live_detected_block` (like `start`/`end`) | `bt-term` presentation (below); in 69b, the identity keys |
| **the band width, source width, fold width, drawing limits of a live block** | `bt-term` (derivation), `bt-viewport` (`MathBlockPlacement`, `ProjectedLiveMathArtifact`), `bt-render` (pixels) | derived from `record.region` each projection | the renderer's scissor, ground, source rows, marks, hit test |
| **the live rows' heights** | `bt-viewport::ViewportProjection` (the per-row height map in the live projection) | combining rule changes from last-writer-assigns to `max`, hidden-top folded first | unchanged readers |

**The door.** `bt_detect::LiveCapture` is the one type every live entry point of the detector accepts:

```rust
// bt-detect — the shape, not the spelling
#[derive(Clone)]
pub struct LiveCapture(Arc<CaptureInner>);
struct CaptureInner {
    inputs: Arc<[LiveDetectionInput]>,
    initial_context: DetectionContext, // checkpoint before inputs[0], as today's task carries it
    options: DetectionOptions,
    frame: OnceLock<ScreenFrame>,       // measured on first ask, by whichever thread asks
}
impl LiveCapture {
    pub fn new(inputs: Vec<LiveDetectionInput>, initial_context: DetectionContext,
               options: DetectionOptions) -> Self;
    pub fn inputs(&self) -> &Arc<[LiveDetectionInput]>;
    pub fn frame(&self) -> &ScreenFrame;
}
pub struct ScreenFrame { borders: Vec<u32>, regions: Vec<Region>, framed_rows: Vec<bool> }
pub struct Region { pub columns: ColumnSpan, inputs: Arc<[LiveDetectionInput]> }
pub struct ColumnSpan { pub start: u32, pub end: u32 } // always bounded (R6)
```

`LiveDetectionTask`'s `inputs`, `initial_context` and `options` become one `capture: LiveCapture`, and it gains `region: ColumnSpan` (the whole width until resolved). `resolve_live_detection_tasks`' "same snapshot?" test becomes one comparison of captures. The frame therefore lives exactly as long as the last task, record or caller holding that capture. That answers the third review's cache-lifetime finding: the branch's `REMEMBERED_SPLIT` thread-local outlived the caller's last `Arc` and could be kept per idle thread. It also answers the cost finding (B-6), because one capture is shared by arming on the window thread and resolution on the math worker, so it is measured once and not once per thread. The checkpoint and the options belong inside the capture because R5's unsplit proof depends on them. A frame cached on the inputs alone would take whichever checkpoint the first caller happened to pass.

**Why `bt-detect` and not `bt-term`.** The frame is a statement about text and its cells: which column a rule stands in, which rows it runs through, which blocks the grammar proves. R5 and R9 need the scanner itself. `bt-term` never re-reads the grid for it. It hands over the one capture it already builds, and the frame is derived from that capture's text and boundaries. There is **no second reader of the grid**: every product caller of `live_detection_context` today is followed by `live_initial_detection_context` over the same inputs, and the two merge into `live_capture()`. `bt-viewport` and `bt-render` never learn what a border is. They receive column limits, as they already receive rows.

**Rejected alternatives.** (i) Recomputing the frame on every call (the branch's first shape): Codex measured 0.25–0.5 ms per one-split call before R5 adds a scan, repeated by arming, resolution and completion (B-6). (ii) A per-thread memo keyed by `Arc::ptr_eq` (the branch's final shape): its lifetime is wrong, and it misses across threads. (iii) A frame keyed by grid generation: Codex B-6 showed a generation is not guaranteed to advance on every relevant write. (iv) A text-only API (`detect_math_blocks_with_sites_in_regions`, `find_border_columns`, `region_text`, with boundaries inferred by `bt_unicode`): the branch exported it, but no product code called it. It is a second slicing path that tests would pass through while the product never did, so it is **not** carried. Tests build `LiveCapture`s (§7).

---

## 4. How it reaches today's detection, stage by stage

Every stage below reads the one capture. On a screen with no frame, each stage is today's code over one whole-width region.

1. **Capture** (`DualPlaneSession::live_capture`, window thread). The same rows, text and boundaries `live_detection_context` builds today, plus the checkpoint. Nothing is measured yet.
2. **Arming** (`live_candidate_rows`, window thread, inside `schedule_live_artifacts`). Today it walks the whole inputs once. It becomes one walk per region over `region.inputs`, each from the region's context (neutral, or today's for the whole-width region). A row is armed if any region arms it. The row stability passed in is still per screen row in 69a (see 69b). The frame is measured here on first ask. Its cheap part runs on every capture: tallying vertical glyphs per column, with a fast exit when no grid row contains a U+2500–U+257F character at all. R3's clearance and R5's unsplit scan run only for a column that passed R1, which on an ordinary screen is none.
3. **Resolution** (`resolve_live_detection_task(s)`, window thread today; the worker path receives the same capture). One scan per region. The candidate row is looked up region by region, and the first region whose block closes on it fills the task (69a). `refused_table_rows` is the union across regions. `apply_live_detected_block` takes the region's logical lines and inputs and writes `task.region`.
4. **Completion** (`live_task_is_current`). It re-resolves against the current capture, as today, and additionally requires the same region. In 69a the dependency rows are still compared as whole rows. 69b compares the region's slice.
5. **Presentation** (`bt-term`). From the record's region: `math_band` becomes `math_band_for(region)` (the pane width it is fitted to and scrolls within); `frame_rows_width_cells` becomes region-bounded; `InlineGridGeometry::pane_columns` becomes the region's width; `live_logical_line_rows`, `live_snapshot_logical_line_text`, `live_fragment_cells`, `live_inline_run_cells` and `live_joined_head_cells` read the region's text and take cells from the **captured boundaries** (the branch's `live_region_cell_column`); `MathBlockPlacement` gains `left_limit_columns` and `right_limit_columns`.
6. **Projection** (`bt-viewport`): the `max` combine and the fold-first rule of R10. **Drawing** (`bt-render`): `math_block_left_edge_px` and `math_block_right_px` bound the ink, the scissor, the ground and the source rows. `math_tool_boxes_px` returns nothing for a block narrower than the pair, so the hit test cannot answer for a mark lying over the other pane.
7. **The oracle's gates.** `live_detection_ownership_ledger` and `live_detection_isolation_gap` (used by `bt-repaint-oracle` and by `held_unbacked_records`) take the capture and report per region, unioned. **The branch never touched them.** On a framed screen the oracle's ledger would therefore have been an unsplit scan, which owns none of the pane's blocks, and it would have reported every typeset pane formula as `HeldUnbacked`.

What does **not** change: the frozen plane. Multiplexers repaint the alternate screen and leave no scrollback, and a framed primary screen that scrolls into history is read there as whole lines, exactly as today. Also unchanged: every gate of the scanner, `bt_detect::table`, and `DetectionOptions`.

---

## 5. Failure modes

| screen | R1 share | outcome | why this is right |
|---|---|---|---|
| herdr sidebar / compact sidebar | 100 % | split; the pane's 1 inline + 2 display typeset in pane columns | the defect |
| tmux vertical split, prose left, math right | 100 % | split; right typesets, left proves nothing new | the defect |
| a box-drawn table inside a TUI (5 of 40 rows) | 12.5 % | no split | R1 denominator |
| a table drawn with `│` on **every** row, junction separators | ≥ 90 % | split into its cells; each cell's formula proven in its own region | a cut along a drawn rule runs through no text. R5 keeps any formula merged across a *missing* inner rule |
| a full-height box (an agent's input box, a banner with two caps) | ≥ 90 % on the sides | split at the box sides | cuts only its border; R5 refuses if a proven formula would lose rows |
| `column -t` output with a `│` separator, one crossing row | ≥ 90 % | no split | R3: crossing text is not a frame |
| prose that contains `│` on a few rows | < 90 % | no split | R1 |
| ASCII `\|` columns (mysql, pagers, GFM tables) | — | never a border | R4 |
| a status line at the bottom (`[0] 0:bash* 12:00`) | — | exempt; split | R3 |
| a status line with a price (`cost $5`) | — | not exempt; no split (today's behaviour) | R3 as ruled 2026-09-17; §10 Q2 |
| tmux 2×2 (`─────┼─────` middle row) | vertical survives | split into left and right; each half-height pane shares its region with the pane above or below it | R2. The regions are columns, and a fence or block in the top-right pane is read with the bottom-right pane as one column. A fence opened in one of them already suppresses the rest of that column, so nothing false is typeset |
| a horizontal-only split or a plain `─` row across the rule | broken | no split | horizontal pane boundaries are not modelled: needs frame topology, not a share (Codex round 2). **Out of scope; stated** |
| `$x   │+y$` written across an intact rule | — | split; not one formula | R5 (a): under a proven frame it is two panes' text, and a formula must never straddle a pane border. This is a deliberate loss relative to today, and only on screens whose every row draws the rule |
| a wide cluster straddling a region's first column | — | belongs to neither side; cells come from captured boundaries | R7 (Codex R2-1) |
| a rule in the last column (`a│`, `│x│`) | — | no empty trailing region | R6 (Codex B-7) |

---

## 6. What the branch got wrong, and what the reviews ruled

Three Codex reviews (2026-09-16 at `947e31b1`; 2026-09-17 at `1d810432`; 2026-09-17 at `22c7aa1f`), all "do not merge". The final commit `0fa4af14` answered the third review and was never reviewed. The rulings the branch cites are the owner's of 2026-09-16 and 2026-09-17, quoted in its `border.rs` header and its DESIGN §4.6e (neither reached main).

### 6.1 Findings and dispositions

| finding | the branch's last state | ruling / disposition here |
|---|---|---|
| **B-1** inline runs used region bytes against whole-screen text; a straddling cluster made the origin arithmetic one cell short (R2-1) | closed in `22c7aa1f` (captured-boundary lookup, `live_region_cell_column`; hunk `bt-term/src/session.rs`) | carried as R7: cells only from captured boundaries, never origin plus width |
| **B-2** raster, source, ground and hit bounds escaped the region | closed (region band, `left_limit_columns`/`right_limit_columns`, tools hidden when they cannot fit; hunks `bt-viewport`, `bt-render`) | carried as R10 / §4 steps 5–6 |
| **B-3** a later pane's band overwrote an earlier band's rows; clipped-top added after merging was order-dependent | closed (`max`, fold-first; owner's rulings 2026-09-16, 2026-09-17) | carried; the six-order test carried |
| **B-4** candidates, tasks and records keyed by row only; completion compared whole rows | **open, accepted limit** (DESIGN §4.6e "known limit") | **69b** (§9), widened in §6.3 |
| **B-5** 90 % occupancy is not proof: a crossing row cut a formula; a whole-screen fence did not reach the other region | closed for the examples by the unbroken-frame rule, the fence-ownership rule and the upstream checkpoint | carried as R3 and R9. Its formula-loss class is closed generally by R5 |
| **B-6** repeated border work, no shared cache | "partly": a thread-local memo | replaced by the capture-owned frame (§3) |
| **B-7** empty unbounded trailing region | open, "benign" | fixed by R6 (bounded spans) |
| **B-8** no-border equivalence | closed | carried as the plain-screen guard, made stronger (§7) |
| **B-9** tests stopped short of presentation (cleared cells, geometry, marks, hit tests, three regions, topology change) | partly | §7 lists what each seam must test |
| **R2** junction layouts and upstream fence | closed (`continues_vertical`, `grid_initial_context`) | carried (R2, R9) |
| **R2-1** the edge exemption sliced a formula on an edge row | closed for `$` rows | carried (R3), and made general by R5 |
| **R3-1** "no dollar does not mean no formula" (`\[x^2\]` at an edge) | closed by `line_carries_math_delimiter` (owner's ruling 2026-09-17) | carried in R3 (§10 Q2 asks whether R5 may replace it) |
| **R3-2** a cut through a formula's own blanks (`$$x   +y$$`, a merged table cell) | closed per line by `math_spans_on_line` | **not closed in general** (§6.4); R5 replaces it |
| **R3** cache lifetime: the memo outlives the frame | open | gone with the memo (§3) |
| **R3** the table test compared counts, not identities | open | tests compare the multiset of `render_source` (§7) |

### 6.2 Where the rulings came from, and which are carried unchanged

Carried unchanged: the nine-tenths share over all rows and at least three rows; ASCII `|` excluded; the unbroken frame with one edge exemption; junctions continue the line and plain horizontals break it; a fence's owner is decided by its opening row, and an upstream fence is the screen's; one row stack with `max` and fold-first; a region-local line for every gate, with no gate moved. The branch's per-line span veto is carried in *intent* ("a formula's own blanks are never a cut") and replaced in *mechanism* by R5.

### 6.3 A limit the branch understated: the neighbouring pane's writes

`DualPlaneSession::observe_live_damage` sets each damaged row's `revision`, `last_damage_at` and `candidate_signature` from `TerminalAdapter::visible_row_fingerprint(row)`, which hashes the **whole row**. It then calls `rearm_live_bands_containing(row)`. `live_detection_context_signature` hashes every structural input on the screen. `advance_live_stability` arms a row only after `LIVE_MATH_STABLE_INTERVAL` (200 ms) of quiet on that row. `live_task_is_current` compares whole row texts. So while the pane on the other side of the rule writes on the same screen rows (an agent streaming, a sidebar spinner), a formula in this pane is never armed. If it was already rendered, it is re-armed on every write. Codex's B-4 named the last of these ("compare dependency slices within the region"). The first three were not reviewed. None of this is a regression: today such a formula is never typeset at all. But it is the everyday case for the feature, and it is 69b's job.

### 6.4 A gap in the branch's final veto, found by reading

`frame_is_unbroken` asks `RowGeometry::clear_at` of each row that does not carry the line. `clear_at` refuses a column inside one of the row's **own** proven spans (`math_spans_on_line`, which proves only a display block that opens and closes on the line, and inline runs). Take 37 rows of `log  │ text` with three rows in the middle: `$$`, `x   +   y`, `$$`. Column 5 is blank on all three. On the middle row, `+` stands at column 4 and column 6 is blank, so nothing crosses. `math_spans_on_line("x   +   y")` is empty (no `$`, no `\`). 37 of 40 rows is 92.5 %. So column 5 is a border. The left region then reads `$$`, `x   +`, `$$`, a display block that typesets **the wrong formula**, and the unsplit screen's block `x   +   y` is lost. This is static evidence (read from the patch hunk `bt-detect/src/border.rs`, not executed). R5 refuses the column, because the unsplit scan proves the block and the block stands on rows the line does not run through. `a_display_block_across_unframed_rows_keeps_the_screen_whole` (§7) is the new test.

### 6.5 A reader the branch missed

`held_unbacked_records`, `live_detection_ownership_ledger` and `live_detection_isolation_gap` were left as unsplit scans (§4 step 7). They are oracle and test instrumentation, not product behaviour. But the repaint oracle is a gate the owner relies on, and it would have flagged every framed pane formula.

---

## 7. Tests to carry, rewritten against `LiveCapture`

Every test takes the repo shape (`/// RED (69a|69b) — **claim**`, why, `/// MUTATION:`). A test-only helper `framed_capture(rows: &[&str]) -> LiveCapture` builds grid inputs with boundaries from `bt_unicode` cluster widths. That is a fixture, not a product path. **At least one test per seam feeds bytes through the real terminal** (the `bt-term` tests below), where the boundaries are the captured ones. Result sets are compared as multisets of `(region, render_source, start, end)`, never as counts.

**`crates/bt-detect/tests/framed_screen.rs` (69a)**, carried from the branch with these rewrites:

| test | asserts, against the chosen API |
|---|---|
| `the_herdr_sidebar_no_longer_hides_the_pane` | regions `[0,25)`, `[26,100)`; the right region's blocks = the 13-line `PANE`'s three (1 inline, 2 display); the inline anchor is column 34 |
| `the_herdr_compact_sidebar_no_longer_hides_the_pane` | regions `[0,3)`, `[4,…)`; the same three blocks, anchored from column 4 |
| `a_plain_screen_detects_exactly_what_it_always_did` | one whole-width region; resolved tasks equal, field by field, the tasks today's unsplit resolution produces over the same capture (the branch compared only block lists) |
| `a_tmux_split_typesets_the_right_pane_and_nothing_in_the_left` | regions `[0,50)`, `[51,…)`; left proves nothing (its `$5 and $10` prose stays prose) |
| `prose_left_of_the_rule_still_splits_the_screen` | as the branch |
| `a_fence_the_screen_proves_suppresses_every_region` | 38 rows of `log  │ $x^2$` between bare fences: split stands, zero blocks |
| `a_fence_one_pane_prints_leaves_the_other_pane_alone` | fence rows opened inside the left pane: right proves all 40, left none |
| `a_full_screen_table_splits_into_its_cells_without_losing_their_math` | the multiset of the regions' `render_source` equals the unsplit screen's (37) |
| `a_dollar_free_formula_at_an_edge_keeps_the_screen_whole` | `\[x^2\]` at row 0 and at row 39, `\begin{pmatrix}` at 39: no border; the one block is kept |
| `a_cut_never_runs_through_a_formulas_own_blanks` | `$$x   +y$$` on row 20: no border at 5; block kept |
| `a_table_is_not_cut_through_the_cell_a_formula_merged` | borders `[0, 15]`, not 5; block kept |
| `a_table_drawn_inside_a_tui_never_splits_the_screen` | 5 table rows in 40: one whole region |

New in 69a: `a_display_block_across_unframed_rows_keeps_the_screen_whole` (§6.4); `a_fence_open_above_the_screen_vetoes_every_region` (checkpoint inside a fence, and a literal opener in the primary history tail); `a_two_by_two_layout_keeps_its_vertical_rule_through_the_junction`; `a_rule_in_the_last_column_leaves_no_empty_region`; `a_cluster_straddling_the_rule_belongs_to_neither_side`; `a_formula_written_across_an_intact_rule_is_two_panes_text` (pins the deliberate loss in §5); `a_capture_measures_its_frame_once` (two tasks sharing one capture: the frame is measured once, observed through a test-only counter on `ScreenFrame` construction, not a timer). The branch's `border.rs` unit tests (`nine_tenths_is_enough_and_less_is_not`, `one_crossing_row_at_an_edge_is_a_status_line`, `a_junction_keeps_the_vertical_border_and_a_plain_horizontal_breaks_it`, `ascii_pipes_never_cut_a_screen`, `wide_glyphs_are_counted_in_cells`, `junctions_are_not_rules`, `a_rule_in_column_zero_leaves_one_region`, `a_region_keeps_its_own_indentation`) move into the new module's tests.

**`crates/bt-term/src/session.rs` tests (69a, real producer: bytes fed to the session):** `herdr_pane_rows_typeset_behind_their_sidebar` (the whole 40-row herdr repaint as bytes; three live decorations, every one in the pane's columns; the inline run's **cleared cells** are columns 34.., not 8..); `a_formula_in_a_split_is_fitted_to_its_pane_and_drawn_inside_it` (band width = the region's 49 or 50 columns; placement limits; a wide formula shrinks toward the pane, never across the rule); `a_framed_pane_of_wide_characters_places_its_formula_in_the_grid_cells`; new `the_oracles_ledger_owns_a_framed_panes_formulas` (`held_unbacked_records` empty on the herdr capture).

**`crates/bt-viewport` (69a):** `two_panes_sharing_rows_keep_the_taller_band_whole`, `three_panes_with_hidden_tops_agree_in_every_order` (both screens, all six orders). **`crates/bt-render` (69a):** a block narrower than the pair has no marks, and a hit test at the would-be mark rectangle over the neighbouring pane answers nothing.

**69b:** `a_formula_in_one_pane_settles_while_the_other_pane_writes` (the right pane rewrites its half of rows 0..12 every 50 ms; the left block arms and lands; a landed left block is not re-armed); `two_panes_closing_on_the_same_row_both_typeset`; `two_panes_starting_on_the_same_row_both_keep_their_records`; `completion_ignores_the_other_panes_half_of_the_row`.

---

## 8. Architecture impact

### 69a T-PANE-COLUMNS
- **(a) facts touched.** New: *the frame of a live capture*, owner `bt_detect::LiveCapture::frame` (one writer, the `OnceLock` initialiser; §4.4 row written in the same commit). Changed shape: `bt_detect::LiveDetectionTask` (`inputs`/`initial_context`/`options` → `capture`; `+ region`); `DualPlaneSession::live_decorations` records gain `region` (written at install from the task, never changed); `bt_viewport::MathBlockPlacement` and `ProjectedLiveMathArtifact` gain column limits; the live projection's per-row height map changes its combining rule (last-writer → `max`, hidden top folded first).
- **(b) doors.** None new. No file read, child process, OS hand-off, thread or PTY write. The only grid reader stays `DualPlaneSession::live_detection_context` (renamed `live_capture`). The frame is computed on whichever lane first asks: the window thread's arming today, the same lane that already runs `resolve_live_detection_tasks` inside `schedule_live_artifacts`. No new vocabulary call, so the window-thread bare-site inventory is untouched. The report states the measured cost of `frame()` per capture for plain, one-split and dense shapes at 300×100, as Codex measured the branch.
- **(c) structural debt.** None added, none repaid. `bt-term → bt-math` (D-15) is untouched. The thread-local memo that would have been hidden per-thread state is not introduced.
- **(c′) new sources.** A live block's band width, fold width and drawing limits gain a second input: the frame, beside the pane's width. Readers that assumed "band = pane width": `DualPlaneSession::math_band`, `math_pane_width_px`, `frame_rows_width_cells`, `InlineGridGeometry::pane_columns`, `bt_render`'s `math_block_geometry_px` / `math_horizontal_bounds` / `math_block_ground_bounds` / `math_band_face_for` / `math_tool_boxes_px`, and the hit test that reads the tool boxes first. The per-row height map gains a second writer per row wherever two regions' bands share rows. Readers of the whole-screen scan that assumed it *is* the product's scan: `live_detection_ownership_ledger`, `live_detection_isolation_gap`, `held_unbacked_records`, `bt-repaint-oracle`.
- **(d) ownership change.** No.

### 69b T-PANE-IDENTITY
- **(a)** `DualPlaneSession::live_decorations` keyed by `(start row, region)`; candidate state (`LiveRowStability::candidate_signature`, `revision`, `last_damage_at`, `settled_revision`, `content_fingerprint`) held per `(row, region)` when the last capture had a frame; `live_task_is_current` compares the region's slice; repaint occupancy and retirement by region.
- **(c′)** damage now bumps only the regions whose columns a write touched. That needs the last capture's frame at damage time, a new reader of the frame on the damage path: it is held as the session's *current frame* (written only by the capture, read by `observe_live_damage`). A frame change counts as full damage.
- **(d) ownership change: yes**, per screen row → per (row, region). This note is its design note; it is not dispatched before Codex's review.

---

## 9. Size and the ticket cut

The branch was +3,500/−156 (border.rs 1,109 including ~480 lines of tests; lib.rs +570; session.rs +915; viewport +470; render +270; tests 377; docs ~650 including the three reviews). Main has moved under every touched file since (119 changes, per the triage). It is redone, not rebased.

| ticket | scope | size | order |
|---|---|---|---|
| **69a T-PANE-COLUMNS** | `bt-detect` frame module + `LiveCapture` + per-region scans (resolve, batch resolve, ledger, isolation gap) + R5/R9 vetoes; `bt-term` capture, per-region arming, record region, the presentation derivations of §4.5; `bt-viewport` limits and combine; `bt-render` limits and marks; tests of §7; DESIGN entry, RULES §7/§21 lines, CHANGELOG | **L** — ~2,400–3,000 lines incl. tests; the `LiveCapture` migration alone touches the ~9 product callers of `live_detection_context`, 7 product `LiveDetectionTask` constructions and ~30 test sites | first |
| **69b T-PANE-IDENTITY** | region-keyed decorations, candidates and stability; region-scoped damage; completion by region slice; the four tests | **M–L** — ~1,000–1,500 lines; `live_decorations` has ~80 uses and `candidate_row` ~60 in product `session.rs` | after 69a merges |

Why not one ticket: 69a is complete and safe alone. It never regresses an unframed screen, and on a framed one it only adds typesetting. 69b is an ownership change with its own review gate. Why not split 69a again: detection without presentation would typeset a pane's formula at full-screen width, over the neighbouring pane (Codex B-2). That is worse than today, so the two halves ship together.

Lanes: `bt-detect`, `bt-viewport` and `bt-render` filters locally; `bt-term --lib` filters for the session tests; DGX wincheck for check/clippy; CI is the gate. No bt-app change is expected beyond compile fallout of the renamed accessor. A macOS check is required if any `bt-render` accessor a `cfg(target_os = "macos")` path reads is renamed.

---

## 10. Questions for the owner

**Q1. Is 69b also 0.4.7?** With 69a alone, a pane's formulas typeset once the rows they stand on are quiet on *both* sides of the rule. When two panes close a block on the same row, the left one wins. Both are today's cost, not a regression. With 69b, a formula settles on its own pane's quiet. My recommendation: **yes, directly after 69a**. The motivating case is two agents side by side, and there the neighbour is rarely quiet.

**Q2. May R5 replace the delimiter test on the status-line exemption (ruling 2026-09-17)?** Today's ruling: an edge row carrying any math delimiter spends no exemption, so a status line reading `cost $5` or `$HOME` keeps the whole screen unsplit and the pane's formulas stay source. R5 already refuses a cut through any formula the screen proves, on any row, the edge included. The delimiter test then protects only *unproven* delimiters. Such a half-formula can only stand at an edge while it is being printed, and once it gains a second row it is on a middle row, which may never be exempt. My recommendation: **replace it**. The ruling's stated reason ("a formula the screen can prove outranks a split the screen only infers") is exactly R5, and a shell prompt or a price in a status bar should not switch the feature off. Keeping the ruling as it stands is also coherent. It costs only those screens.

---

## Sources

- The branch: `fix_formulas-behind-a-border.patch` (hunks `crates/bt-detect/src/border.rs`, `crates/bt-detect/src/lib.rs`, `crates/bt-detect/tests/framed_screen.rs`, `crates/bt-term/src/session.rs`, `crates/bt-viewport/src/lib.rs`, `crates/bt-render/src/lib.rs`, `docs/DESIGN.md` §4.6e) and its commit list; its three reviews `border-detection-review-codex-2026-09-16.md`, `border-detection-review-2-codex-2026-09-17.md`, `border-detection-review-3-codex-2026-09-17.md` (carried inside the patch; not on main).
- The triage `stale-branches-2026-09-26.md` §2; ticket 69 (`tickets-046/69-formulas-inside-a-multiplexer-pane.md`).
- Main at `6abd3f5e`, by grep: `bt_detect::{LiveDetectionInput, LiveDetectionTask, resolve_live_detection_task, resolve_live_detection_tasks, apply_live_detected_block, clipped_tail, live_detection_isolation_gap, live_detection_ownership_ledger, complete_display_on_line, is_math_environment}`; `bt_detect::table`'s `box_drawing_characters_never_trigger`; `bt_term::session::{DualPlaneSession::live_detection_context, live_initial_detection_context, schedule_live_artifacts, advance_live_stability, observe_live_damage, rearm_live_bands_containing, math_band, math_pane_width_px, held_unbacked_records, LiveDecorationRecord, live_candidate_rows, live_task_is_current, live_detection_context_signature, frame_rows_width_cells, live_logical_line_rows, live_snapshot_logical_line_text, live_inline_run_cells, live_fragment_cells, live_joined_head_cells, InlineGridGeometry, LIVE_MATH_STABLE_INTERVAL}`; `TerminalAdapter::visible_row_fingerprint`; `bt_viewport::{MathBlockPlacement, ProjectedLiveMathArtifact}` and the per-row height map in the live projection; `docs/ARCHITECTURE.md` §3.1 (`bt-detect ← bt-term`), §4.4; `docs/RULES.md` §7 and §21 (`not yet folded`).
