# Formulas inside a multiplexer pane: detection reads the pane's own rectangle, not screen lines — design note, 2026-09-29

0.4.7 ticket 69, phase 1 (docs only). Base: main `6abd3f5e`. **Revision (b), 2026-09-29**, folds in Codex's review of revision (a) at `aca459a1` (`69-review-codex-2026-09-29.md`, verdict "not yet", eleven findings) and the owner's rulings of the same day (§10). **Revision (c), 2026-09-29**, folds in Codex's check of (b) at `6375a616` (`69-review-codex-2026-09-29-b.md`, three blockers) and the coordinator's ruling on frame changes. What changed is listed at the end; the sections below are the current text.

The owner's ruling of 2026-09-29: ticket 69 ships in 0.4.7, and the never-merged branch `fix/formulas-behind-a-border` (nine commits of 2026-09-16/17, ending at `0fa4af14`) is **the specification and the evidence, not a rebase target**. The branch is kept as a patch (`fix_formulas-behind-a-border.patch`, with its commit list, in the coordinator's stale-branch folder; triage in `stale-branches-2026-09-26.md` §2). Its three Codex reviews travelled inside the patch and never reached main. Below, "the branch" means that patch at `0fa4af14`, and "hunk X" names the file the hunk changes. Facts about main come from grep on `6abd3f5e`, never from line numbers.

---

## 0. The decisions, in one screen

1. **The defect is still on main.** Inside `herdr`, or in a `tmux` split, every pane row reaches the host grid as `<frame cells>│<pane text>`. The detector reads that as one line. `<25 blanks>│$$` is indented code, and its trimmed form opens on `│`, not `$$`, so nothing in the pane is ever typeset. `bt-detect` on main has `lib.rs`, `ledger.rs` and `table.rs` and no border or region API. Nothing in `docs/DESIGN.md`, `docs/RULES.md` or `CHANGELOG.md` mentions herdr or tmux panes.
2. **A pane is a rectangle, not a column span** (Codex finding 1). The frame is read from the captured grid as a **guillotine layout tree** (§2.2): the screen, minus at most one edge status row, is cut by full vertical rules and by anchored horizontal rules, recursively. The leaves are the panes. Each pane is scanned alone from a neutral checkpoint, and every existing gate of the detector runs unchanged over the pane's own lines. A fence opened in the top-right pane of a 2×2 layout does not reach the bottom-right pane.
3. **A frame is told from a table** (finding 2; the ticket's invariant stands). A cut must carry a vertical stroke on **every** row of its rectangle, and at least one side of it must be *clipped* (§2.3): text that starts or ends at the cell next to the rule. That is what a program clipping a pane produces, and what a table, box art or a padded listing never produces. A leaf that holds only digits and blanks marks a numbered listing (a side-by-side diff), and it refuses the whole frame. A horizontal rule cuts only where a vertical rule already proven a frame anchors it, so a rectangle whose every rule is padded is a table and is cut in **neither** direction. A full-height Unicode table, box-drawing art and a side-by-side diff therefore **never** split the screen.
4. **A formula the unsplit screen proves is never cut** (R5, a veto only). Before any cut is accepted, the unsplit capture is scanned exactly as today. A cut through the cells of any block that scan proves is refused. This holds on every row, rule rows included, so the branch's "deliberate loss" of `$x │+y$` is gone. The owner's rule of 2026-09-17 on the status row is **kept** (§2.4). Codex's counter-example shows that R5 cannot replace it: a sliced status row can manufacture a clean `$$` that the unsplit screen never proved.
5. **Owner: `bt-detect`.** The frame is a pure function of one live capture, and `bt-detect` derives it once. The door is one new type, `bt_detect::LiveCapture`: the inputs, the parser checkpoint and options they are scanned under, and the frame, measured once on first ask (§3). `bt-term` stays the only reader of the grid. **This is an ownership split** (finding 9): a live occurrence's extent, fit, fold width, drawing limits and hit area move from "the pane's full width" to "the rectangle it was proven in". It needs this reviewed note, rows in ARCHITECTURE §4.4, and nothing more (§8).
6. **One release deliverable, two implementation tickets** (owner, 2026-09-29; finding 10). **69a T-PANE-COLUMNS** (the frame, detection, presentation, the oracle) and then **69b T-PANE-IDENTITY** (math identity, stability, damage and completion keyed by pane). **Neither is released alone.** Both land on main before the 0.4.7 tag. The motivating case is two agents side by side, and with 69a alone that case fails: whole-row damage from a busy neighbour keeps a pane's formulas from ever arming.

---

## 1. The defect, and the fixtures that prove it

A multiplexer does not pass its panes' bytes through. It repaints the host screen itself, and every pane row carries the frame. The shapes come from the branch's byte captures (`herdr` 0.8.2, `herdr_client.bin` and `herdr_compact.bin`, 100×40, `TERM=xterm-256color`; described in the header of the branch's `tests/framed_screen.rs` hunk and in its first review):

| fixture | row shape | pane text starts at |
|---|---|---|
| herdr sidebar | 25 blanks, `│` (U+2502), pane text | column 26 |
| herdr compact sidebar | 3 blanks, `│`, pane text | column 4 |
| tmux vertical split | left pane padded to 50 cells, `│`, right pane | column 51 |
| plain screen (regression guard) | the pane text itself | column 0 |

The pane content in all of them is the branch's **13-line `PANE` fixture**: one inline formula, a `$$ \frac{1}{2} $$` block and a `$$ \begin{pmatrix}…\end{pmatrix} $$` block. It is synthetic text with the captures' geometry, so the fixture rule is met.

Why the fix is recognising the frame, not relaxing a gate: both refusals are right about the line they are shown. What nobody ever printed is that line. The fix shows each gate the line the program did print.

---

## 2. The model

Terms: a **capture** is one `Arc<[LiveDetectionInput]>`: an optional bounded tail of frozen history, then every grid row with its text and its **captured cell boundaries** (from the terminal's cells, never inferred from Unicode width). Columns are cells. A cell carries a **vertical stroke** if its glyph is one of U+2500–U+257F with a vertical stroke (`│ ┃ ┆ ┇ ┊ ┋ ╎ ╏ ║`, the junctions `┼ ├ ┤ ┬ ┴`, the corners, arcs and doubles: the branch's `vertical_rule` plus `continues_vertical`). It carries a **horizontal stroke** if its glyph has a horizontal one (`─ ━ ═`, the dashes, and the same junctions and corners). Only one-cell glyphs count. **ASCII `|` and `-` are never strokes** (R4 below).

### 2.1 The outer rectangle

The root rectangle is every grid row, minus **at most one** edge row. The topmost or the bottommost may be excluded as a **status row**, and only when both of these hold:
- without it, a cut exists that fails with it (the row does not carry the stroke the cut needs), and
- it carries **no delimiter the math grammar recognises**: `$`, `\[ \] \( \)`, or `\begin{…}`/`\end{…}` of a math environment (the branch's `line_carries_math_delimiter`; owner's ruling 2026-09-17, kept on 2026-09-29, §2.4).

An excluded status row belongs to **no pane** and no pane scans it. It is read by exactly one thing: the **screen-fence pass** (R9), which advances the fence state across it and never looks for math there. In 69b that state is the screen tier's `screen_fence_state` (§8.2).

### 2.2 The layout tree (guillotine cuts)

`split(R)` for a rectangle `R = rows [top, bottom) × columns [left, right)`:

1. **Vertical cuts.** Column `c`, with `left ≤ c < right`, is a vertical cut of `R` when all of these hold:
   - **(V1)** every row of `R` carries a vertical stroke at `c`, `R` has at least three rows, and the plain vertical rule (not a junction) stands at `c` on at least three of them;
   - **(V2)** the rule is a frame and not a table (§2.3);
   - **(R5)** no block proven on the unsplit screen crosses it (§2.4).

   If any vertical cuts exist, cut `R` at all of them into column strips. A cut column belongs to no strip, and a strip of zero width is dropped (two adjacent rules; a rule in the first or last column, Codex B-7). Then recurse on each strip.
2. **Otherwise, horizontal cuts.** Row `r`, with `top ≤ r < bottom`, is a horizontal cut of `R` when all of these hold:
   - **(H1)** every column of `R` carries a horizontal stroke at `r`;
   - **(H2)** the row is **anchored by a frame**: a cell of that row inside `R`, or the cell just outside `R` at either end of it, is a junction whose vertical stroke continues into an adjacent band, and in that band the junction's column is a **proven frame rule**. That means it passes V1 and V2 (§2.3) over the band's rows: the rows between this row and the next full horizontal-stroke row, or the edge of `R`. A horizontal rule never cuts on its own evidence. It cuts only where a vertical rule already shown to be a multiplexer's, and not a table's, meets it;
   - **(R5)** no block proven on the unsplit screen crosses it.

   If any exist, cut `R` into row bands; a cut row belongs to no band, and an empty band is dropped. Then recurse.
3. **Otherwise `R` is a leaf: a pane.**

**A padded rectangle is a table, in both directions** (Codex check of (b), blocker 1). If every vertical-stroke column of `R` that satisfies V1 fails V2, then `R` is a table or box art. H2 then has no proven frame rule to anchor on, so `R` is cut in neither direction and stays one leaf. That leaf is the whole screen when `R` is the root. Take the 40×11 padded table: row 0 `┌────┬────┐`, row 20 `├────┼────┤`, row 39 `└────┴────┘`, and `│ a  │ b  │` elsewhere.
- The inner rule at column 5 is padded on both sides, so it fails V2.
- The outer rules at columns 0 and 10 have no columns on their outer side. That is not a blank gutter (V2b requires one), and on their inner side they are padded. So they fail V2 too.
- Rows 0, 20 and 39 carry full horizontal strokes, but their junctions and corners stand on unproven columns, so none of them is anchored.

The whole table is one pane (`a_padded_full_height_table_is_never_cut_in_either_direction`).

**Why guillotine.** tmux's layout is a tree of vertical and horizontal splits, and so are herdr's sidebar-plus-pane, vim's `:vsplit`/`:split` and zellij's tiled layout. A full-width `─` row with no junction (a Markdown horizontal rule an agent renders, or a separator under a heading) is not anchored and never cuts. So a screen with only a horizontal split is read whole, as today; that cost is accepted and tested (§5).

**Three cases.**
- *Aligned 2×2* (Codex finding 1's screen: rule at column 10 on 40 rows, row 19 `──────────┼──────────`). Column 10 carries a stroke on every row (the `┼` has one), so the root is cut into two strips. Each strip has row 19 as a horizontal cut, anchored by the `┼` just outside it. The four leaves are the four panes. A fence opened in the top-right pane is scanned only there.
- *Non-aligned* (rule at column 10 on rows 0–18, row 19 `──────────┴──────────`, rows 20–39 one full-width pane). Column 10 is not a stroke on rows 20–39, so there is no vertical cut at the root. Row 19 is a horizontal cut anchored by `┴`. The top band is then cut at column 10. That gives three panes, and the top-right formula is recovered.
- *Both kinds of cut exist in `R`.* This happens only where they cross at a `┼`. Cutting vertically first gives the same leaves as cutting horizontally first, so the order of step 1 before step 2 carries no preference.

**R4. ASCII is never a stroke.** A screen whose rows all carry `|` in one column is far more often `mysql`, `column -t` or a long GFM table than a frame, and nothing local tells them apart. `bt_detect::table` owns GFM tables, and its `box_drawing_characters_never_trigger` test keeps box glyphs out of them, so the two owners do not overlap.

### 2.3 What a frame has that a table lacks (V2)

The one fact that separates them is what put the rule there. A **multiplexer clips**: it cuts each pane's text at the pane's edge and draws the rule beside it. So pane text starts at the cell right of the rule (a prompt, an output line, the herdr pane), or ends at the cell left of it (a line that filled its pane). A **table, box art or listing pads**: every cell is written `│ text │`, with a space on each side of every rule, on every row. That is the rendering rule of CommonMark table renderers, of psql's unicode style, of glow, and of Claude Code's rendered tables.

**(V2a) Clipped.** A side of the rule is **clipped** when, among the rows of `R` on which that side's pane (from the rule to the next rule or edge of `R`) holds any text, a majority have text in the cell next to the rule. The rule passes V2a when either side is clipped.

**(V2b) A blank gutter is harmless.** The rule also passes when one side is blank on every row of `R` **and that side has at least one column** (herdr's sidebar gutter, an empty pane). A rule in the first or last column of the screen has no columns on its outer side. That is not a gutter, so such a rule must be clipped to be a frame. No text can cross a rule with nothing on one side of it, so the cut cannot take anything apart.

**(V2c) A numbered listing is not a layout.** After the tree is built, if any leaf holds only decimal digits and blanks on every row (and is not entirely blank), the **whole frame** is refused. That leaf is a line-number gutter, so the screen is a side-by-side diff (`delta --side-by-side`: `  12 │code…│  12 │code…`) or a numbered listing, not panes. Other diff tools are already excluded: `diff -y` draws ASCII, and `vimdiff` has two status rows (below).

**Truth table.**

| screen | V1 | V2 | frame? |
|---|---|---|---|
| herdr / herdr compact | ✓ | right side clipped (pane text at the rule); left blank | yes |
| tmux split, shell output in either pane | ✓ | right side clipped (lines start at the pane's column 0) | yes |
| tmux 2×2 / nested | ✓ per rectangle | as above | yes |
| a Unicode table, full height (`│ a │ b │`, `┌┬┐ ├┼┤ └┴┘`) | ✓ | padded both sides on every row; its horizontal rules have no proven anchor (H2) | **no**, in either direction |
| box-drawing art or a boxed banner, padded | ✓ | padded | **no** |
| side-by-side diff (delta) | ✓ | clipped, but a digits-only leaf | **no** (V2c) |
| vim `:vsplit` | fails §2.1: two non-rule rows (window status + command line) | — | **no** (accepted, §5) |

**The residual ambiguity, stated honestly.** An *unpadded* full-height Unicode table, or box art whose text touches its rules on most rows, passes V2 and is cut into its cells. What this costs is bounded:
- R5 keeps every formula the unsplit screen proved across a rule;
- each cell's formulas are proved in the cell exactly as they were proved in the whole screen;
- what changes is that a cell can now prove a block the unsplit screen could not (a cell whose own lines are `$$`, `x`, `$$`).

A pane that indents every line (a pane showing only indented code, `man` output with its margin) is not recognised if its neighbour is not clipped either. That screen keeps today's behaviour.

### 2.4 Two vetoes, and why the status-row rule stays

**R5, a veto from the unsplit proof.** Run the detector once over the unsplit capture: this is today's scan, byte for byte. A candidate cut, vertical at column `c` or horizontal at row `r`, is refused if some block that scan proves has a cell rectangle that the cut passes through. The cell rectangle runs from its first to its last live row, and from the smallest `cell_start` to the largest `cell_end` of its live `cell_segments`, in screen columns.

This closes the branch's multi-row gap (§6.4), `$$x   +y$$` on a middle row, and the table cell a formula merged across a missing inner rule. It asks the grammar and the site rules exactly as the product does, so nothing narrower than the grammar can call a formula "absent" (Codex R3-1). Unlike revision (a), it applies **on rule rows too**. `$x       │+y$`, proven by the unsplit scanner, refuses the cut, and no formula the screen proves is ever lost by a split.

The price: in a true tmux split, a line on which the unsplit reading happens to pair a `$` in one pane with a `$` in the other refuses the frame for that capture. That is today's behaviour for that screen, and it lasts only while that row is on screen.

**The status-row delimiter rule stays** (owner's ruling 2026-09-17, confirmed 2026-09-29 after Codex's finding 3). Revision (a) proposed replacing it with R5, and that would be unsafe. Take a 40×20 screen with a candidate rule at column 8 on rows 1–39. Row 0, the one status row, reads `status!!!$$`, so column 8 holds the last `!`. The pane to the right of the rule has `x^2` on row 1 and a clean `$$` on row 2. The unsplit row 0 is `status!!!$$`, so today's scanner proves nothing and R5 has nothing to veto. Slice row 0 at the rule, and the right pane reads `$$ / x^2 / $$`: a display block the unsplit screen never proved, manufactured by the slice. The delimiter rule refuses the exemption, because the row carries `$`. Revision (a)'s other argument, that an incomplete edge formula "gains a second row" and so moves to a middle row, was also wrong: its opener stays on row 0.

In this revision no pane scans an excluded status row (§2.1); only the screen-fence pass reads it, and never for math. That also prevents the manufacture. The rule is kept anyway, as ruled. **R5 is a veto only, never a weakening of another guard.**

### 2.5 Panes, their text, their scans

**R7. Pane text.** A pane's line for one of its rows is that row's bytes whose clusters lie **wholly** inside the pane's columns. A cluster straddling an edge belongs to neither side, and its cell boundaries stay **screen columns**. So every coordinate a pane proves (cells, anchors, inline runs, the joined head) is already in screen columns, and nobody ever adds a pane origin to a width measured over a slice (Codex B-1, R2-1). A row that ends its logical line drops trailing blanks, and a continuing row keeps them (§4.6a, unchanged).

**R8. Each pane is scanned alone** from a **neutral** checkpoint, with the frozen history prefix dropped, over only its own rows and columns. A frame proves that a program repaints the host screen, so no scrollback line runs into a pane, and a delimiter in one pane never runs into another. Every gate runs unchanged over pane-local lines.

**R9. Fences** (owner's rulings 2026-09-16 and 2026-09-17, restated for rectangles):
- a fence opened **inside a pane** is that pane's own fence. That pane's scan refuses what it covers, as it always has, and no other pane is affected. That includes the pane below a horizontal cut, because a horizontal cut restarts the scan.
- a fence opened **on a row no pane owns** (the excluded status row) is **the screen's**, and it suppresses every pane's blocks on the rows it covers. It is found by the screen-fence pass, which walks the unsplit rows for fence state only. Closing it on a row no pane owns releases the panes. In 69b, a change in that state invalidates every pane's math (§8.2, screen tier).
- a fence already **open before the first grid row** (the caller's checkpoint, advanced through the frozen tail: the branch's `grid_initial_context`) is the screen's, and it suppresses from the top (Codex round 2).

**R10. What a pane is to presentation.**
- A block proven in pane `P` owns `P`'s columns. It is fitted to `P`'s width, drawn and scissored between `P`'s left and right edges, its source face is measured and drawn inside them, and its inline line is folded at `P`'s width.
- The live rows remain **one vertical stack**: one top per row across the whole width. Overlapping height requirements combine with `max` (owner's ruling 2026-09-16, Codex B-3), and a band's hidden-top extent is added to its own first-row requirement before that combine (owner's ruling 2026-09-17), so the result does not depend on order.
- On an unframed screen, a row only one band claims is sized exactly as before.

**R11. The minimum presentable pane** (finding 8). A block is presented, meaning its raster is installed and its source cells cleared, only if its pane is at least as wide as the block's two marks (the source/copy pair `bt_render::math_tool_boxes_px` lays out: two buttons and the gap between them, at the pane's scale).
- In a narrower pane the record stays **source**: no raster, no cleared cells, nothing scissored. It goes through the viewport's existing source fallback with a new reason, `pane-narrower-than-marks`.
- Above that width, today's fitting runs unchanged inside the pane: shrink toward the band, stop at the readable floor, then horizontal offset.
- A presented block therefore always has room for the marks that let a person return to source or copy it.

**R12. A rule that moved mid-capture is not a rule** (finding 7, review B-9). V1 requires the stroke on **every** row of the rectangle. Suppose an application repaints its layout row by row (no Folio resize epoch), so the capture holds rows 0–35 with the rule at column 25 and rows 36–39 with it at column 30. Neither column is a cut, and the screen is read whole for that capture: today's behaviour. The next capture, taken after the repaint settles, finds the new layout. The one edge row §2.1 may exclude is never sliced, so a single stale row cannot be cut at a stale column either. A frame that differs from the previous capture's is a **frame change**, and 69b treats it as a full math invalidation (§8.2).

### 2.6 In five sentences

The captured grid is cut into pane rectangles by a guillotine tree: full-height vertical rules and horizontal rules anchored on a vertical rule already proven a frame, at most one math-free edge status row set aside. A rule is a cut only if it carries its stroke on every row of its rectangle, the text beside it is clipped at it rather than padded away from it, and no formula the unsplit screen proves passes through it; a numbered gutter refuses the whole frame. Each pane is scanned alone from a neutral checkpoint over its own rows and columns with every existing gate unchanged, its cells taken from the captured boundaries. A fence opened inside a pane is that pane's; one opened on the status row or before the screen began is the screen's and suppresses every pane. A full-height Unicode table, box art, a side-by-side diff, a table inside a TUI and a screen with no frame are never split, and a screen with no frame is one pane whose scan is byte-for-byte today's.

---

## 3. Who owns what, and the door

| fact | owner | written by | read by | lifetime |
|---|---|---|---|---|
| **the live capture** (grid rows' text, boundaries, sites, the history tail, the checkpoint before it, detection options) | `bt-term::session::DualPlaneSession` (the only reader of the grid) | `DualPlaneSession::live_detection_context` and `live_initial_detection_context`, **merged into one `live_capture()`** returning a `bt_detect::LiveCapture` | arming, resolution, completion, the oracle's ledger and isolation gap, presentation lookups | while any task, record or caller holds it |
| **the frame of a capture** (the pane rectangles, the excluded status row, each pane's sliced inputs, and the unsplit proof R5 consulted) | `bt-detect` (new module `frame`, the branch's `border.rs` redone) | `LiveCapture::frame()`: an `OnceLock<Arc<ScreenFrame>>` initialised by the first caller on whatever lane asks (the window thread's arming today), never written again | every live entry point of `bt-detect`; `bt-term`'s arming; in 69b, the session's current frame | exactly the capture's, plus any `Arc<ScreenFrame>` the session keeps as its current frame (69b) |
| **the pane a block was proven in** | the proven occurrence: `LiveDetectionTask::pane`, then `LiveDecorationRecord::pane` | `bt_detect::apply_live_detected_block` (like `start`/`end`) | presentation (below); in 69b, the identity keys | the record's |
| **a live block's extent, fit width, source width, fold width, drawing limits and hit area** | `bt-term` (derivation), `bt-viewport` (`MathBlockPlacement`, `ProjectedLiveMathArtifact`), `bt-render` (pixels, marks, hit test) | derived from `record.pane` at each projection | the renderer | each frame |
| **the live rows' heights** | `bt-viewport::ViewportProjection` (the per-row height map in the live projection) | the combine changes from last writer assigns to `max`, with the hidden top folded first | unchanged | each projection |

**The door.** `bt_detect::LiveCapture` is the one type every live entry point of the detector accepts:

```rust
// bt-detect — the shape, not the spelling
#[derive(Clone)]
pub struct LiveCapture(Arc<CaptureInner>);
struct CaptureInner {
    inputs: Arc<[LiveDetectionInput]>,
    initial_context: DetectionContext, // checkpoint before inputs[0], as today's task carries it
    options: DetectionOptions,
    frame: OnceLock<Arc<ScreenFrame>>,  // measured on first ask, by whichever lane asks
}
impl LiveCapture {
    pub fn new(inputs: Vec<LiveDetectionInput>, initial_context: DetectionContext,
               options: DetectionOptions) -> Self;
    pub fn inputs(&self) -> &Arc<[LiveDetectionInput]>;
    pub fn frame(&self) -> &Arc<ScreenFrame>;
}
pub struct ScreenFrame { panes: Vec<Pane>, status_row: Option<u32> } // one whole-screen pane when unframed
pub struct Pane { pub rect: PaneRect, inputs: Arc<[LiveDetectionInput]> }
pub struct PaneRect { pub rows: Range<u32>, pub columns: Range<u32> } // always bounded
```

`LiveDetectionTask`'s `inputs`, `initial_context` and `options` become one `capture: LiveCapture`, and the task gains `pane: PaneRect` (the whole screen until resolved). `LiveDecorationRecord`'s `inputs` and `initial_context` are likewise replaced by the `LiveCapture` it was proven from, plus its `pane` (finding 9's storage question). Records proven from one capture share one `Arc`, and presentation reads the pane's text through `capture.frame()`.

**Why the capture owns it.** The frame lives exactly as long as the last holder of the capture. That answers the third review's finding on cache lifetime: the branch's `REMEMBERED_SPLIT` thread-local outlived the caller's last `Arc` and could be kept by each idle thread. It also answers the cost finding (B-6), because one capture shared by arming on the window thread and resolution on a worker is measured once. The checkpoint and the options sit inside the capture because R5's unsplit proof depends on them. A frame cached on the inputs alone would take whichever checkpoint the first caller happened to pass.

**Why `bt-detect` and not `bt-term`.** The frame is a statement about text and its cells: which cells carry a stroke, which side is clipped, which blocks the grammar proves. R5 and R9 need the scanner itself. `bt-term → bt-detect` is an existing edge (ARCHITECTURE §3.1). `bt-term` never re-reads the grid for the frame: it hands over the one capture it already builds. Sharing that immutable snapshot with a worker adds no second reader.

Every product construction of scan inputs goes through `live_capture()`, with one exception: the repaint reconciliation builds bare `LiveDetectionInput`s from captured rows as **row-identity probes** (`ProvenLiveRow::exactly_matches`), never as scan inputs. Those stay as they are. (Revision (a) claimed every product call was already paired with the checkpoint, which was too strong; Codex finding 9.) `bt-viewport` and `bt-render` never learn what a frame is. They receive column limits, as they already receive rows.

**Rejected alternatives.**
- (i) Recomputing the frame on every call: Codex measured 0.25–0.5 ms per one-split call before R5 added a scan, and arming, resolution and completion each repeat it (B-6).
- (ii) A per-thread memo keyed by `Arc::ptr_eq`: its lifetime is wrong, and it misses across threads.
- (iii) A frame keyed by grid generation: a generation is not guaranteed to advance on every relevant write (B-6).
- (iv) The branch's text-only API (`detect_math_blocks_with_sites_in_regions`, `find_border_columns`, `region_text`, with widths inferred by `bt_unicode`): no product code called it, so it was a second slicing path that tests exercised while the product never did. It is **not** carried.

---

## 4. How it reaches today's detection, stage by stage

Every stage reads the one capture. On a screen with no frame, each stage is today's code over one whole-screen pane.

1. **Capture** (`DualPlaneSession::live_capture`, window thread). The same rows, text and boundaries `live_detection_context` builds today, plus the checkpoint and options.
2. **Arming** (`live_candidate_rows`, window thread, inside `schedule_live_artifacts`). One walk per pane over `pane.inputs`, each from the pane's context: neutral for a framed pane, today's for the whole-screen pane. A row is armed if any pane arms it.
   - The frame is measured here on first ask.
   - Its cheap part runs on every capture: a fast exit when no grid row contains a U+2500–U+257F glyph, then a per-column stroke tally.
   - V2, R5 (one extra unsplit scan) and the recursion run only when some column passes V1 over the root rectangle, which on an ordinary screen none does.
   - In 69a, row stability is still per screen row; 69b changes that.
3. **Resolution** (`resolve_live_detection_task(s)`). One scan per pane. The candidate row is looked up pane by pane, and the first pane whose block closes on it fills the task (69a; 69b keys candidates by pane). `refused_table_rows` is the union across panes. `apply_live_detected_block` takes the pane's logical lines and inputs and writes `task.pane`.
4. **Completion** (`live_task_is_current`). It re-resolves against the current capture and additionally requires the same pane rectangle. In 69a the dependency rows are compared as whole rows; 69b compares the pane's slice.
5. **Presentation** (`bt-term`), all from `record.pane`:
   - `math_band` becomes `math_band_for(pane)`;
   - `frame_rows_width_cells` is bounded by the pane;
   - `InlineGridGeometry::pane_columns` becomes the pane's width;
   - `live_logical_line_rows`, `live_snapshot_logical_line_text`, `live_fragment_cells`, `live_inline_run_cells` and `live_joined_head_cells` read the pane's text and take their cells from the **captured boundaries** (the branch's `live_region_cell_column`);
   - `MathBlockPlacement` gains `left_limit_columns` and `right_limit_columns`;
   - R11 is applied before a raster is installed.
6. **Projection** (`bt-viewport`): R10's `max` combine with the hidden top folded first. **Drawing** (`bt-render`): `math_block_left_edge_px` and `math_block_right_px` bound the ink, the scissor, the ground and the source rows. R11 guarantees that `math_tool_boxes_px` always has room, so a mark can never lie over another pane.
7. **The oracle keeps pane identity** (finding 5). `live_detection_ownership_ledger` returns **one ledger per pane**, keyed by `PaneRect`, never a union of source sets. `held_unbacked_records` asks the ledger of **the record's own pane**, by `(pane, original_source)`. A stale right-pane record whose identical source is still owned by the left pane is therefore reported, not backed.
   - `OwnershipLedger::owns_source` keeps its meaning inside one pane.
   - `live_detection_isolation_gap` is computed per pane. On a framed screen every pane is scanned without a history prefix, so its gap is zero by construction, and the unframed value is today's.
   - `bt-repaint-oracle` prints the per-pane ledgers with their rectangles.
   - The branch never touched these readers. On a framed screen its oracle would have reported every typeset pane formula as unbacked.

What does **not** change: the frozen plane. Multiplexers repaint the alternate screen and leave no scrollback, and a framed primary screen that scrolls into history is read there as whole lines, exactly as today. Also unchanged: every gate of the scanner, `bt_detect::table`, and `DetectionOptions`.

---

## 5. Outcomes, including the accepted conservative failures

| screen | outcome | why |
|---|---|---|
| herdr sidebar / compact | split; the pane's 1 inline + 2 display typeset in pane columns | the defect |
| tmux vertical split | split; the right pane typesets; the left proves nothing new | the defect |
| tmux 2×2 aligned; nested non-aligned | four / three panes; a fence in the top-right does not reach the bottom-right | §2.2 |
| tmux with one bottom status row, no `$` or `\` in it | split, status row excluded | §2.1 |
| **tmux status row showing `$HOME`, `cost $5`, or an agent's `$0.42` counter** | **no split; the panes' formulas stay source: today's behaviour. Accepted conservative failure** | §2.4 ruling |
| **vim `:vsplit`** (rule on rows 0–37, window status on row 38, command line on row 39) | **no split: two non-rule rows, and only one edge row may be excluded. Accepted conservative failure** | §2.1 |
| a horizontal-only split (a plain `─` row, no junction) | **no split; a fence above the boundary still suppresses the pane below, and a block there can be refused: today's behaviour. Accepted** | §2.2 H2 |
| a full-height Unicode table / box art / side-by-side diff | no split | §2.3 |
| a boxed table inside a TUI on a few rows | no split | V1 (every row of the rectangle) |
| a rule that moved during a layout repaint | no split for that capture | R12 |
| `$x       │+y$` proven by the unsplit screen | the cut is refused | R5 |
| a two-cell pane | its blocks stay source | R11 |
| a wide cluster straddling a pane edge | belongs to neither side; cells from captured boundaries | R7 |

---

## 6. What the branch got wrong, and what the reviews ruled

Four Codex reviews: the branch's three (2026-09-16 at `947e31b1`; 2026-09-17 at `1d810432`; 2026-09-17 at `22c7aa1f`) and this note's (2026-09-29 at `aca459a1`). All said "not yet" or "do not merge".

### 6.1 The branch's findings

| finding | the branch's last state | disposition |
|---|---|---|
| **B-1** inline runs used pane bytes against whole-screen text; the origin arithmetic was a cell short at a straddling cluster (R2-1) | closed (`live_region_cell_column`) | R7 |
| **B-2** raster, source, ground and hit bounds escaped the region | closed (region band, left/right limits, marks hidden) | R10, R11 |
| **B-3** a later band overwrote an earlier band's rows; hidden top added after merging | closed (`max`, fold first) | R10; six-order test |
| **B-4** candidates, tasks and records keyed by row only; completion compared whole rows | open, "accepted limit" | **69b**, release-gating |
| **B-5** 90 % occupancy is not proof | unbroken frame + fence ownership + upstream checkpoint | replaced by V1 (every row of the rectangle), R5 and R9 |
| **B-6** repeated border work | thread-local memo | the capture owns the frame (§3) |
| **B-7** empty trailing region | open | panes are bounded rectangles |
| **B-8** no-border equivalence | closed | the plain-screen guard, compared field by field |
| **B-9** presentation, topology change and hit coverage untested | partly | R11, R12 and §7 |
| **R2** junction layouts, upstream fence | closed for the examples | §2.2 (rectangles, not column spans), R9 |
| **R3-1** no dollar does not mean no formula | the delimiter test on the edge row | kept (§2.4) |
| **R3-2** a cut through a formula's blanks | per-line spans | R5 (the unsplit proof) |
| **R3** cache lifetime; counts not identities | open | §3; §7 compares exact blocks, anchors and cells |

### 6.2 This note's review (2026-09-29): findings and where each is answered

| # | finding | answered in |
|---|---|---|
| 1 | column spans are not panes; a 2×2 fence leaks down the column; a non-aligned nest is never recovered | §2.2 (guillotine rectangles), R9 |
| 2 | the ticket's full-screen-table invariant was reversed | §2.3 (V1, V2, the gutter rule); the invariant stands |
| 3 | replacing the status-row delimiter rule with R5 manufactures an opener | §2.4: the rule kept (owner, 2026-09-29) |
| 4 | vim `:vsplit` and tmux `$HOME` status rows | §5, stated and tested as accepted failures |
| 5 | a unioned oracle ledger backs a stale pane's record | §4 step 7: ledgers keyed by pane |
| 6 | 69b's damage design against `TerminalDamage = Full \| Rows` | §8.2: two-tier state and slice fingerprints |
| 7 | a stale rule column mid-capture | R12 |
| 8 | panes narrower than the readable floor | R11 |
| 9 | 69a is an ownership split; lifetime; storage | §3, §8.1 |
| 10 | 69a must not ship alone | §0.6, §9 |
| 11 | tests pinning each finding | §7, in full |

### 6.3 Found in revision (a), confirmed by Codex

- **The branch's per-line veto can cut a multi-row block.** Take 37 rows of `log  │ text` and three rows `$$`, `x   +   y`, `$$`. Column 5 is blank on all three of them, and on the middle one nothing crosses it (`+` stands at column 4). `math_spans_on_line("x   +   y")` is empty, and 37 of 40 rows is 92.5 %. So the branch cuts at column 5, and the left region typesets `x   +`: the wrong formula. Here V1 refuses that column, because the stroke is missing on three rows, and so does R5.
- **The branch left the oracle's ledger unsplit** (§4 step 7).
- **Whole-row stability starves a pane beside a busy neighbour** (§8.2).

---

## 7. Tests

Every test takes the repo shape: `/// RED (69a|69b) — **claim**`, a paragraph on why, and a `/// MUTATION:` line.
- A test-only helper `framed_capture(rows: &[&str]) -> LiveCapture` builds grid inputs with boundaries from `bt_unicode` cluster widths. It is a fixture, not a product path.
- **At least one test per seam feeds bytes through the real terminal** (the `bt-term` tests), where the boundaries are the captured ones.
- Results are asserted exactly: panes as `PaneRect`s, blocks as `(pane, render_source, start, end)`, inline anchors and cleared cells as screen columns. Never counts alone.

### 7.1 `crates/bt-detect/tests/framed_screen.rs` (69a)

The twelve ticket names, rewritten:

| test | asserts |
|---|---|
| `the_herdr_sidebar_no_longer_hides_the_pane` | panes `rows 0..13 × [0,25)` and `× [26,100)`; the right pane's blocks are the `PANE`'s three (1 inline, 2 display); the inline anchor is column 34 |
| `the_herdr_compact_sidebar_no_longer_hides_the_pane` | panes `[0,3)`, `[4,…)`; the same three, anchored from column 4 |
| `a_plain_screen_detects_exactly_what_it_always_did` | one whole-screen pane; resolved tasks equal, field by field, today's unsplit resolution over the same capture |
| `a_tmux_split_typesets_the_right_pane_and_nothing_in_the_left` | panes `[0,50)`, `[51,…)`; the left pane's `$5 and $10` prose stays prose |
| `prose_left_of_the_rule_still_splits_the_screen` | as the branch |
| `a_fence_the_screen_proves_suppresses_every_region` | a fence open before the first row, over 40 rows of `log  │ $x^2$` (and a fence opened on an excluded status row): the frame stands, zero blocks |
| `a_fence_one_pane_prints_leaves_the_other_pane_alone` | fence rows opened inside the left pane: the right pane proves all of its rows, the left none |
| `a_dollar_free_formula_at_an_edge_keeps_the_screen_whole` | `\[x^2\]` at row 0 and at row 39, and `\begin{pmatrix}` at row 39: no frame; the one block kept |
| `a_cut_never_runs_through_a_formulas_own_blanks` | `$$x   +y$$` on row 20: no cut at column 5; block kept |
| `a_table_is_not_cut_through_the_cell_a_formula_merged` | no inner cut at column 5; the formula is kept with its unsplit anchor and cells |
| `a_table_drawn_inside_a_tui_never_splits_the_screen` | 5 table rows in 40: one whole-screen pane |
| **`a_full_height_unicode_table_is_not_a_pane_frame`** (replaces the branch's `a_full_screen_table_splits_into_its_cells_without_losing_their_math`) | 40 padded rows `│ name  │ $x^2$    │` with `┌┬┐ ├┼┤ └┴┘`: one whole-screen pane; the blocks, anchors and cells equal the unsplit scan's exactly |

From Codex's finding 11, in full:

| test | asserts |
|---|---|
| `a_fence_in_the_top_right_does_not_suppress_the_bottom_right` | the aligned 2×2 of finding 1: four panes; a top-right fence opener with no closer; the bottom-right `$$ / x^2 / $$` at rows 22–24 is proven, anchored at column 11 |
| `a_non_aligned_nested_split_recovers_the_top_right_pane` | rule on rows 0–18, row 19 `──────────┴──────────`, rows 20–39 full width: three panes; the top-right formula proven |
| `a_padded_full_height_table_is_never_cut_in_either_direction` | Codex's 40×11 table (row 0 `┌────┬────┐`, row 20 `├────┼────┤`, row 39 `└────┴────┘`, `│ a  │ b  │` elsewhere, no math): one whole-screen pane. There is no vertical cut (every rule padded, no outer gutter), and no horizontal cut (no anchor on a proven frame rule) |
| `full_height_box_art_is_not_a_pane_frame` | a padded box diagram with `│` on every row: one pane |
| `a_side_by_side_diff_is_not_a_pane_frame` | delta-shaped rows `  12 │$$…│  12 │$$…`: one pane (the gutter rule); the diffed `$$` lines stay source |
| `a_status_slice_cannot_manufacture_a_clean_display_opener` | finding 3's 40×20 screen: row 0 `status!!!$$` is not excluded, there is no frame, and there are zero blocks |
| `a_vim_vsplit_with_two_status_rows_keeps_todays_behaviour` | rule on rows 0–37, window status on row 38, command line on row 39: one pane; blocks equal the unsplit scan's |
| `a_tmux_status_row_with_a_dollar_keeps_todays_behaviour` | bottom row `[0] 0:bash* $HOME 12:00`: one pane; the right pane's `PANE` blocks are not proven (today's cost, pinned) |
| `a_plain_horizontal_split_keeps_todays_fence_and_formula_cost` | a full-width `─` row with no junction, a fence above it, a block below: one pane; the block is refused as today |
| `a_rule_column_that_changes_mid_capture_is_not_sliced_at_the_stale_column` | finding 7's screen (rule at column 25 on rows 0–35, at column 30 on rows 36–39): one pane; a variant with only row 39 moved and delimiter-free excludes it and never slices it |
| `an_exempt_edge_wide_cluster_maps_through_the_captured_boundaries` | a status row with a wide cluster over the rule column: excluded. A framed row where `能` straddles a pane's first column: the inline run's anchor and cleared cells come from the captured boundaries (column 4, not 3); asserted on the session's cleared cells in `bt-term` |
| `a_formula_written_across_an_intact_rule_refuses_the_cut` | `$x       │+y$` proven on the unsplit screen: no cut; the block kept |
| `a_display_block_across_unframed_rows_keeps_the_screen_whole` | §6.3's screen |
| `a_rule_in_the_last_column_leaves_no_empty_pane`, `a_cluster_straddling_the_rule_belongs_to_neither_side` | R6 of revision (a), carried |
| `a_capture_measures_its_frame_once` | two tasks sharing one capture: one `ScreenFrame` constructed (a test-only construction counter, not a timer) |

The branch's unit tests move into the new module and are re-stated for rectangles: `one_crossing_row_at_an_edge_is_a_status_line` (now: excluded, not sliced), `a_junction_keeps_the_vertical_border_and_a_plain_horizontal_breaks_it`, `ascii_pipes_never_cut_a_screen`, `wide_glyphs_are_counted_in_cells`, `junctions_are_not_rules`, `a_rule_in_column_zero_leaves_one_region`, `a_region_keeps_its_own_indentation`. `nine_tenths_is_enough_and_less_is_not` is retired: the share is replaced by V1's every-row rule (§10, recorded).

### 7.2 `bt-term` (69a; real producer: bytes fed to the session)

- `herdr_pane_rows_typeset_behind_their_sidebar`: the 40-row herdr repaint as bytes; three live decorations, all in the pane's columns; the inline run's **cleared cells** are columns 34.., not 8...
- `a_formula_in_a_split_is_fitted_to_its_pane_and_drawn_inside_it`: the band width is the pane's; the placement limits; a wide formula shrinks toward the pane and never crosses the rule.
- `a_framed_pane_of_wide_characters_places_its_formula_in_the_grid_cells`.
- `a_two_column_region_keeps_source_when_the_formula_cannot_fit`: a width-8 screen with rules at columns 2 and 5 and `$$ / x / $$` in `[3,5)`; the record stays source, with no cleared cells and no raster.
- `the_oracles_ledger_owns_a_framed_panes_formulas`: `held_unbacked_records` is empty on the herdr capture.
- `the_oracle_does_not_back_a_stale_region_from_another_regions_equal_source`: identical `$$ / x / $$` in the left pane (rows 1–3) and the right pane (rows 6–8). Erase the right occurrence while a repaint hold keeps its artifact, and the right record is reported as unbacked.

### 7.3 `bt-viewport` and `bt-render` (69a)

- `two_panes_sharing_rows_keep_the_taller_band_whole`.
- `three_panes_with_hidden_tops_agree_in_every_order` (both screens, all six orders).
- A block whose pane holds its marks draws them inside the pane, and a hit test in the neighbouring pane answers nothing.

### 7.4 69b

| test | asserts |
|---|---|
| `a_formula_in_one_pane_settles_while_the_other_pane_writes` | finding 6's screen: 40×100 split at column 50; a left block on rows 10–12; a spinner changes column 80 of row 11 every 50 ms. The left block arms and lands, and is not re-armed |
| `a_spinner_in_one_pane_advances_the_row_but_not_the_other_panes_math` | the same screen: row 11's global `revision` advances, the path watermark sees a changed row (`revision != path_pass_revision`), and the left pane's math revision does not advance |
| `a_frame_change_invalidates_every_pane_keyed_state` | capture A: 40×100, rule at column 50; capture B: the same rule plus a junction-anchored horizontal split at row 20 inside the right pane only. On B, every pane-keyed candidate, signature, slice clock, task, hold and record is dropped, **the left pane's included although its rectangle is unchanged**; both panes' formulas re-arm and land again |
| `a_fence_opened_on_the_status_row_suppresses_every_pane_and_its_closing_releases_them` | 40×100: row 0 an excluded status row `status ready`, rows 1–39 split at column 50, settled formulas in both panes. Repaint only row 0 to three backticks: the frame is unchanged, `screen_fence_state` changes, every record retires, and no pane re-arms. Repaint row 0 back to `status ready`: both panes re-arm and their formulas land again. A task resolved before the opening is refused at completion |
| `two_panes_closing_on_the_same_row_both_typeset`; `two_panes_starting_on_the_same_row_both_keep_their_records` | the B-4 collisions |
| `completion_ignores_the_other_panes_half_of_the_row` | `live_task_is_current` compares only the pane's slice |

---

## 8. Architecture impact

### 8.1 69a T-PANE-COLUMNS

- **(a) facts touched.**
  - *New:* **the frame of a live capture**, owner `bt_detect::LiveCapture` (a write-once derived fact, `OnceLock<Arc<ScreenFrame>>`, one initialiser, living as long as the capture's holders). An ARCHITECTURE §4.4 row is written in the same commit.
  - *Changed shape:* `bt_detect::LiveDetectionTask` (`inputs`, `initial_context` and `options` become `capture`, plus `pane`); `DualPlaneSession::live_decorations` records (`inputs` and `initial_context` become `capture`, plus `pane`, written at install, never changed); `bt_viewport::MathBlockPlacement` and `ProjectedLiveMathArtifact` gain column limits; the per-row height combine (last writer → `max`, hidden top folded first); `OwnershipLedger` per pane.
- **(b) doors.** None new: no file read, child process, OS hand-off, thread or PTY write. The only grid reader stays `DualPlaneSession::live_capture`. The frame is computed on whichever lane first asks, which is the window thread's arming inside `schedule_live_artifacts`, the lane that already runs `resolve_live_detection_tasks`. No new vocabulary call, so the window-thread bare-site inventory is untouched. The report gives the measured cost of `frame()` per capture for plain, one-split, 2×2 and dense shapes at 300×100.
- **(c) structural debt.** None added and none repaid; D-15 is untouched. The per-thread memo is not introduced.
- **(c′) new sources.** A live block's extent, fit, fold width, limits and hit area gain a second input: the frame, beside the pane's width. The readers that assumed "band = pane width": `DualPlaneSession::math_band`, `math_pane_width_px`, `frame_rows_width_cells`, `InlineGridGeometry::pane_columns`, and `bt_render`'s `math_block_geometry_px`, `math_horizontal_bounds`, `math_block_ground_bounds`, `math_band_face_for` and `math_tool_boxes_px`, plus the hit test that reads the tool boxes first. The per-row height map gains a second writer per row wherever two panes' bands share rows. The readers that assumed the whole-screen scan *is* the product's scan: `live_detection_ownership_ledger`, `live_detection_isolation_gap`, `held_unbacked_records` and `bt-repaint-oracle`.
- **(d) ownership change: yes** (Codex finding 9, the brief template's clause "splits it — e.g. per-window → per-pane", `docs/templates/brief.md`). A live occurrence's horizontal extent, fit width, source width, fold width, drawing limits and hit area move from the terminal pane to the rectangle it was proven in. The owner stays `DualPlaneSession` (the record), with `bt_detect::LiveCapture` as the derivation's owner. What the rule requires: this design note reviewed by Codex before dispatch (this revision is sent back for a blockers-only check), the §4.4 rows in the implementation commit, and the report restating this section as built. `LiveDecorationRecord` is a `bt-term` type, so the bt-app ownership census is not affected.

### 8.2 69b T-PANE-IDENTITY

The damage fact today is `TerminalDamage = Full | Rows(Vec<u32>)`, deliberately without column bounds (its doc comment in `adapter.rs`). So "which pane did a write touch" is not something damage can report. It can only be learned by **fingerprinting each pane's slice of every damaged row** against the previous capture's frame. The state becomes three tiers:

| tier | facts | stay or move |
|---|---|---|
| **global row tier** (unchanged owner: `DualPlaneSession::live_rows`, `LiveRowStability`) | `revision`, `content_fingerprint`, `last_damage_at`, `settled_revision`, `path_pass_revision` | **stay**. `revision` remains the freshness authority for printed paths and image placeholders (`absorb_printed_path_probes` compares it with `path_pass_revision`) and is still copied into `LiveDetectionSource::Grid { revision }`. Image arming keeps reading `settled_revision` |
| **pane math tier** (new: a map keyed by `(row, PaneRect)` on `DualPlaneSession`, present only while the current frame has more than one pane) | a slice fingerprint (over the captured cells in the pane's columns), `math_revision`, `last_math_damage_at`, `settled_math_revision`, `candidate_signature` | **new**, except `candidate_signature`, which **moves** here from `LiveRowStability` (it is math-only) |
| **screen tier** (new: one value on `DualPlaneSession`, beside `current_frame`) | `screen_fence_state`: whether a **screen-owned** fence is open at each pane row, derived from the incoming checkpoint (the fence state before the first grid row, carried through the frozen tail) and from the screen-fence pass over the rows no pane owns (the excluded status row), together with the identity of the frame it was computed for | **new**. It is the only math dependency that no pane's slice can see |

- **Damage.** For each damaged row (`Rows`, or every row on `Full`), the row is captured once, as today's fingerprint already does through `TerminalAdapter::visible_row_fingerprint`'s capture cache. The global tier updates as today. Then each pane of the **current frame** that covers the row compares its slice fingerprint: only a pane whose slice changed advances its math clock, clears its candidate signature and re-arms its bands (`rearm_live_bands_containing` becomes pane-scoped).
- **Unframed screens.** The current frame is one whole-screen pane, so the math tier is the row tier and behaviour is today's.
- **The current frame.** The session keeps `current_frame: Arc<ScreenFrame>`: the `Arc` from the capture that `schedule_live_artifacts` last scheduled with. It is replaced at the next scheduling and compared **by value** (the pane rectangles), never by pointer. This reconciles "the frame dies with the capture" with the damage path. The session holds its own `Arc` to the frame, not the capture, so the capture's inputs are released when their holders go.
- **Any frame change clears every pane's math tier** (coordinator's ruling 2026-09-29, Codex check of (b), blocker 2). When a new capture's set of rectangles differs in any way from the current frame's, every pane-keyed fact is dropped: candidates, candidate signatures, slice fingerprints, math clocks, tasks in flight (refused at completion by the pane-equality check), repaint holds and records. Every record retires to source. The rectangles are rebuilt from the new frame, and the formulas re-arm and re-typeset from there. **There is no retention clause.** A pane whose rectangle is byte-for-byte unchanged is cleared too: when capture B adds a horizontal split inside the right pane of capture A's 40×100 split at column 50, the left pane's formulas are cleared and re-typeset as well. The cost is accepted: a frame change is a resize or relayout of the multiplexer, which is rare, and a re-typeset takes at most a few hundred milliseconds (one stability interval plus the math worker's round trip). The rule is one sentence, and no path can keep a record whose pane identity was minted against a different frame. A write that moves a rule changes the old panes' slice fingerprints, so those bump, and the next capture sees the frame change.
- **A screen-fence change clears every pane's math tier the same way** (Codex check of (b), blocker 3). `screen_fence_state` is recomputed whenever the revision of any row no pane owns advances (the global row tier sees it: the excluded row is damaged like any other), and whenever the incoming checkpoint changes. If it differs from the stored value, the effect is exactly a frame change: every pane-keyed fact is dropped and every pane re-arms against the new screen fence.
  - *Opening:* the status row is repainted to three backticks, which still carry no math delimiter, so the row stays excluded and no pane slice changes. The screen tier sees the new fence, every pane's records retire, and nothing re-arms inside the fence.
  - *Closing:* the row is repainted back. The screen tier changes again, and the panes re-arm.
  - Completion also compares the task's `screen_fence_state` with the current one, so a task resolved before the change is refused.
- **Identity.** `live_decorations` is keyed by `(start row, PaneRect)`. Candidates and the per-pane `live_detection_context_signature` are keyed by `(closing row, PaneRect)`. Repaint occupancy, refused-table retirement and completion's dependency comparison work within the pane (the pane's slice text).
- **(d) ownership change: yes**: per screen row → per (row, pane) for math identity and math stability; the global row facts are unchanged. This note is its design note.

---

## 9. The deliverable and its two tickets

The branch was +3,500/−156. Main has moved under every file it touched since (119 changes, per the triage). It is redone, not rebased.

| ticket | scope | size | order |
|---|---|---|---|
| **69a T-PANE-COLUMNS** | `bt-detect`: the frame module (guillotine tree, V1/V2/R5, status row, gutter rule), `LiveCapture`, per-pane scans (resolve, batch resolve, per-pane ledger, isolation gap). `bt-term`: capture, per-pane arming, record `pane` and capture, the presentation derivations of §4.5, R11, the oracle. `bt-viewport`: limits and the combine. `bt-render`: limits. §7.1–§7.3; DESIGN entry, RULES §7/§21 lines, ARCHITECTURE §4.4 rows | **L+** (the frame tree and the oracle add to revision (a)'s estimate; above 3,000 lines including tests) | first |
| **69b T-PANE-IDENTITY** | the two-tier state, slice fingerprints, current frame, frame-change invalidation, pane-keyed decorations, candidates, occupancy, retirement and completion; §7.4 | **L** (`live_decorations` has ~80 uses and `candidate_row` ~60 in product `session.rs`) | directly after 69a |

**One release deliverable.** 69a merges to main when its gates and CI are green. The 0.4.7 tag waits for 69b as well, and the CHANGELOG line is written once, by 69b, when the feature is complete. 69a on its own regresses nothing: an unframed screen is byte-for-byte today's, and a framed one only gains typesetting. But its failure in the motivating case (Codex finding 10) means it is not the completion of ticket 69.

Why detection and presentation are not split further: detection without presentation would typeset a pane's formula across the other pane (B-2), which is worse than today.

Lanes: `bt-detect`, `bt-viewport` and `bt-render` filters locally; `bt-term --lib` filters for the session tests; DGX wincheck for check and clippy; CI is the gate. A macOS check applies if a `bt-render` accessor that a `cfg(target_os = "macos")` path reads is renamed.

---

## 10. Owner rulings, and what this revision decides

**Ruled 2026-09-29.**
- **Q1 = yes.** 69b ships in 0.4.7; the two tickets are one release deliverable.
- **Q2 = keep the 2026-09-17 status-row delimiter rule** (reversed after Codex's counter-example, §2.4). R5 is a veto only.

**Decided here, for the owner to see** (none is left open):
- **The nine-tenths share (ruling 2026-09-16) is replaced by V1's every-row rule within a rectangle.** The rows the tenth existed to absorb (the status line, junction rows, horizontal pane boundaries) are now each modelled: exclusion, strokes and cuts. So the rule is strictly more conservative, and it is what refuses a rule that moved mid-capture (R12).
- **The branch's "deliberate loss" of a formula written across an intact rule is withdrawn.** R5 now refuses that cut too.
- **Accepted conservative failures** (§5): vim `:vsplit`, a status row with a delimiter, a plain horizontal split, and a pane every line of which is indented beside an unclipped neighbour.

**Ruled 2026-09-29 (coordinator), revision (c):** any frame change, and any change of the screen-owned fence state, clears every pane's math tier; nothing is retained across it (§8.2).

No question remains open for the owner.

---

## Revision (b), 2026-09-29 — what changed after Codex's review and the owner's rulings

1. Panes are guillotine rectangles (vertical cuts, junction-anchored horizontal cuts), not column spans; the scan restarts in each; fences are pane-owned by rectangle (finding 1).
2. The ticket's invariant stands: V2 (clipped versus padded) and the gutter rule tell a frame from a table, box art or a side-by-side diff. `a_full_screen_table_splits_…` is replaced by `a_full_height_unicode_table_is_not_a_pane_frame`, plus box-art and diff tests; the residual ambiguity is stated (finding 2).
3. The status-row delimiter rule is kept, with Codex's `status!!!$$` counter-example as the reason; the excluded row is not scanned (finding 3; owner Q2).
4. vim `:vsplit` and tmux `$HOME` are stated as accepted conservative failures, with tests (finding 4).
5. The oracle keeps pane identity: per-pane ledgers, `held_unbacked_records` asks the record's own pane, and there is a stale-region test (finding 5).
6. 69b's damage design follows today's `TerminalDamage`: two-tier state, slice fingerprints, a session-held current frame compared by value, and a frame change as full invalidation; which facts move and which stay (finding 6).
7. R12: a rule that moved mid-capture is not a cut, with tests (finding 7).
8. R11: the minimum presentable pane; a narrower pane keeps source, and nothing is scissored (finding 8).
9. 69a is recorded as an ownership split; the lifetime of the `OnceLock<Arc<ScreenFrame>>`; the record holds the capture; the repaint-probe exception to "one constructor" (finding 9).
10. One release deliverable, two tickets, sizes raised; finding 11's tests in full (findings 10, 11; owner Q1).
11. R5 now applies on rule rows too, withdrawing the branch's deliberate loss; the nine-tenths share is replaced by V1 (§10).

## Revision (c), 2026-09-29 — what changed after Codex's check of (b)

1. Horizontal cuts need a proven frame: H2 anchors only on a junction whose column passes V1 and V2 in the adjacent band. A rectangle whose every rule is padded is a table and is cut in neither direction. V2b's gutter must have at least one column, so a rule on the screen's edge must be clipped. Test `a_padded_full_height_table_is_never_cut_in_either_direction` (blocker 1).
2. Any frame change clears every pane's math tier: tasks, holds and records retire, rectangles are rebuilt, and the retention clause is removed. `a_frame_change_invalidates_every_pane_keyed_state` pins the unchanged-rectangle case, and the cost is stated as accepted (blocker 2; coordinator's ruling).
3. A screen tier: `screen_fence_state`, from the incoming checkpoint and the screen-fence pass over the rows no pane owns, tied to the frame identity. It is recomputed when such a row's revision advances or the checkpoint changes; a change clears every pane's math tier like a frame change, and completion compares it. The wording of §2.1/§2.4/R9 is reconciled: no pane scans the excluded row, and only the screen-fence pass reads it, for fence state. Test `a_fence_opened_on_the_status_row_suppresses_every_pane_and_its_closing_releases_them` (blocker 3).


## Revision (d), 2026-09-29 — what the implementation (69a, T-PANE-COLUMNS) was forced to state

Only forced deviations and clarifications; none moves a rule of §2–§4.

1. **The cheap tally's gate** (§4 step 2). The note says V2, R5 and the recursion run only when some column passes V1 over the root rectangle. Read literally that forbids the non-aligned case §2.2 requires: its rule stands on rows 0–18 only, so no column passes V1 over the root, and the cut that recovers the top-right pane (row 19, anchored by `┴`) is never tried. The gate is therefore V1's floor, asked of every column: **some column carries the plain rule on at least three rows**. That is necessary for any cut anywhere (a vertical cut over its own rectangle, a horizontal one over the band its anchor stands in) and still false on every ordinary screen (`ScreenFrame::measure`).
2. **What V2 counts as text.** A box-drawing stroke is frame, not text: a padded box whose top and bottom rows are `─` would otherwise have "text in the cell next to the rule" on exactly the rows that are rules, and a full-height box with one text row reads as clipped. V2a and V2b count only cells that are not one-cell strokes (`Cell::Text`).
3. **What H2's junction is.** A cell of the row *joining* it: inside `R` any cell with a vertical stroke (H1 already gives it the horizontal one); just outside `R`, a cell whose stroke reaches into `R` — `├` left of it, `┤` right of it. A plain `│` beside a `─` row joins nothing (herdr's sidebar separator `─────` beside its rule).
4. **Two fixtures of §7.1 are not frames under §2.3 as written.** `a_fence_the_screen_proves_suppresses_every_region` and `a_fence_one_pane_prints_leaves_the_other_pane_alone` name forty rows of `log  │ $x^2$`. Both sides of that rule are padded (the pane text starts one blank after it), so V2 refuses it and there is no frame for the fence to act across. The tests use `log  │$x^2$` — the pane clipped at the rule, which is what a multiplexer draws — and a positive control (without the fence the right pane proves all forty formulas). `a_dollar_free_formula_at_an_edge_keeps_the_screen_whole` likewise uses clipped rows, and adds the control that a delimiter-free status row in the same place *is* set aside.
5. **`a_capture_measures_its_frame_once`** is a unit test in `bt_detect::frame`, because its construction counter is test-only (`#[cfg(test)]`) and an integration test does not see it; `framed_screen.rs` carries the pointer-equality twin `a_capture_shares_its_frame_between_the_tasks_that_hold_it`.
6. **The frame change and the screen-fence change are wired in 69a** (the ticket: "invalidation wired, the 69b tier stubbed"). `DualPlaneSession::observe_frame` compares each scheduled capture's frame with the session's current frame by value and clears every pane's math on any difference; completion refuses a scan read against another frame. So two §7.4 tests pass under 69a and run: `a_frame_change_invalidates_every_pane_keyed_state` and `a_fence_opened_on_the_status_row_suppresses_every_pane_and_its_closing_releases_them`. The other five stay 69b's and are `#[ignore = "69b"]`.
7. **R5 asks the block's cells, segment by segment** (§2.4 says "a cell rectangle … from the smallest `cell_start` to the largest `cell_end` of its live `cell_segments`"). An inline occurrence is one per line and groups every `$…$` run of it, so that rectangle, over a row holding a formula in each pane, covers the rule between them and refuses the split although no cell of either formula is on it — every tmux split with math on both sides would read whole. A cut is refused when a proven block has a cell on the rule's column on a row of the rectangle (vertical), or a cell on the rule's row in the rectangle's columns (horizontal). Every example of §2.4 and §6.3 is still refused: `$x       │+y$` is one run over the rule, and a display block's segments cover each of its lines whole (`a_formula_in_each_pane_on_one_row_does_not_refuse_the_cut` pins the difference).

**Open for the review (F-1).** §0.3 says a full-height Unicode table never splits the screen. That holds for a table as wide as its screen, and the tests pin it there. A full-height table **narrower** than the screen has blank columns right of its outer rule, and V2b reads them as a gutter: the outer rule is cut, and the table's own horizontal rules, anchored on that proven rule, cut the strip into bands. Every formula the whole screen proves is still proven at the same cells (a cut along a table's own rules takes nothing apart), so nothing typesets that did not, and nothing stops typesetting — but the screen is framed, which §0.3 says it never is. V2b cannot tell "an empty pane beside a padded one" from "the margin beside a padded table" from one rule's two sides; closing this needs a rule the note does not have (for example: a V2b cut whose other side is padded on every row is refused unless that side is itself anchored by a clipped rule). Pinned as it stands by `a_full_height_unicode_table_is_not_a_pane_frame`'s second half.

**Open for the review (F-2): R5's price is paid more often than §2.4 suggests.** The unsplit scan reads a `$$` that opens a line with more text after it as an opener, and closes it at a later line that ends in `$$`, when no line between is a sentence the prose guard refuses. So in a split whose left pane prints a bare `$$` row and whose right pane prints `$$` on the same row, the unsplit row `$$ … │$$` is one complete display across the rule; and a left `$$` row followed, a few rows down, by a right-pane row ending in `$$`, with only short lines between, is one block across the rule too. Either is "proven" on the unsplit screen, R5 refuses the cut, and the capture reads whole — today's behaviour, as ruled. In the motivating case (two agents printing display math side by side) it happens whenever two panes' `$$` rows meet on one screen row, or when the rows between are terse. The implementation follows §2.4; the bt-term fixtures avoid the shape and say why (`prose`, `same_row_screen`). Whether R5 should ask the unsplit scan only about blocks whose own delimiters stand on one side of the rule is the review's question.

---

## Sources

- The branch: `fix_formulas-behind-a-border.patch` (hunks `crates/bt-detect/src/border.rs`, `crates/bt-detect/src/lib.rs`, `crates/bt-detect/tests/framed_screen.rs`, `crates/bt-term/src/session.rs`, `crates/bt-viewport/src/lib.rs`, `crates/bt-render/src/lib.rs`, `docs/DESIGN.md` §4.6e) and its commit list; its three reviews `border-detection-review-codex-2026-09-16.md`, `border-detection-review-2-codex-2026-09-17.md` and `border-detection-review-3-codex-2026-09-17.md` (carried inside the patch; not on main); this note's review `69-review-codex-2026-09-29.md` (coordinator's trace folder).
- The triage `stale-branches-2026-09-26.md` §2; ticket 69.
- Main at `6abd3f5e`, by grep:
  - `bt_detect`: `LiveDetectionInput`, `LiveDetectionTask`, `resolve_live_detection_task`, `resolve_live_detection_tasks`, `apply_live_detected_block`, `clipped_tail`, `live_detection_isolation_gap`, `live_detection_ownership_ledger`, `OwnershipLedger::owns_source`, `complete_display_on_line`, `is_math_environment`; `bt_detect::table`'s `box_drawing_characters_never_trigger`;
  - `bt_term::adapter`: `TerminalDamage` and its doc comment, `TerminalAdapter::visible_row_fingerprint`;
  - `bt_term::session`: `DualPlaneSession::live_detection_context`, `live_initial_detection_context`, `schedule_live_artifacts`, `schedule_visible_artifacts`, `advance_live_stability`, `observe_live_damage`, `rearm_live_bands_containing`, `absorb_printed_path_probes`, `math_band`, `math_pane_width_px`, `held_unbacked_records`, `LiveRowStability` (`revision`, `path_pass_revision`, `candidate_signature`), `LiveDecorationRecord`, `ProvenLiveRow::exactly_matches`, `live_candidate_rows`, `live_task_is_current`, `live_detection_context_signature`, `frame_rows_width_cells`, `live_logical_line_rows`, `live_snapshot_logical_line_text`, `live_inline_run_cells`, `live_fragment_cells`, `live_joined_head_cells`, `InlineGridGeometry`, `LIVE_MATH_STABLE_INTERVAL`;
  - `bt_viewport`: `MathBlockPlacement`, `ProjectedLiveMathArtifact`, the per-row height map and its `source-fallback` reasons; `bt_render::math_tool_boxes_px`;
  - `docs/ARCHITECTURE.md` §3.1 and §4.4; `docs/RULES.md` §7 and §21; `docs/templates/brief.md` (the ownership-change clause).
