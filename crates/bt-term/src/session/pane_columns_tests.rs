//! **Formulas inside a multiplexer pane, through the real terminal** (ticket 69a, T-PANE-COLUMNS;
//! `docs/plans/design/pane-columns-2026-09-29.md` §7.2 and §7.4).
//!
//! Every screen here reaches the session as bytes, the way a multiplexer repaints the host screen:
//! each row addressed, the frame cells written, then the pane's own text. The cell boundaries the
//! detector reads are therefore the captured ones, never inferred.
//!
//! The §7.4 tests belong to ticket 69b (T-PANE-IDENTITY: stability, candidates, records and
//! completion keyed by `(row, pane)`). They are written here in full and carry
//! `#[ignore = "69b"]` until 69b lands; `scripts/ci/ignored-tests.txt` lists them.

use super::tests::{complete_detected_live_tasks, nz, synthetic_raster};
use super::*;

/// An alternate-screen repaint of these rows, each addressed and cleared to its end.
fn repaint(rows: &[String]) -> Vec<u8> {
    let mut bytes = b"\x1b[?1049h".to_vec();
    for (index, text) in rows.iter().enumerate() {
        bytes.extend_from_slice(format!("\x1b[{};1H{text}\x1b[K", index + 1).as_bytes());
    }
    bytes
}

/// Rewrite one row in place, as a multiplexer does when one pane's line changes.
fn rewrite_row(row: u32, text: &str) -> Vec<u8> {
    format!("\x1b[{};1H{text}\x1b[K", row + 1).into_bytes()
}

fn rect(top: u32, bottom: u32, left: u32, right: u32) -> PaneRect {
    PaneRect {
        top,
        bottom,
        left,
        right,
    }
}

/// Terminal metrics an inline formula can be placed in (the same as the inline tests' own): a
/// 24 px line box, a 19 px baseline, 12 px cells and a 20 px pane em.
fn seat_inline_metrics(session: &mut DualPlaneSession) {
    session.set_font_size_subpixels(NonZeroI64::new(20 * SUBPIXELS_PER_PX).unwrap());
    session.set_cell_height_subpixels(NonZeroI64::new(24 * SUBPIXELS_PER_PX).unwrap());
    session.set_cell_width_subpixels(NonZeroI64::new(12 * SUBPIXELS_PER_PX).unwrap());
    session.set_ascii_baseline_subpixels(NonZeroI64::new(19 * SUBPIXELS_PER_PX).unwrap());
}

/// Resolve and rasterize every queued live task with the real engine, as the worker does, and
/// count the proven blocks whose completion was accepted.
fn complete_for_real(session: &mut DualPlaneSession) -> usize {
    let engine = MathEngine::new();
    let mut completed = 0;
    while let Some(mut task) = session.take_live_worker_task() {
        let result = render_live_detection_task(&engine, &mut task, [220, 220, 220]);
        let proven = task.resolved;
        if session.complete_live_worker_result(task, result) && proven {
            completed += 1;
        }
    }
    completed
}

/// **What the other pane prints**: an ordinary sentence. A multiplexer's neighbour is another
/// program's output, and a sentence is what keeps the unsplit reading from pairing one pane's `$$`
/// with the other's — the scan's prose guard refuses a body with a sentence in it — which is what
/// real output does. A neighbour of bare words would let the unsplit screen prove a block across the
/// rule, and R5 would then (rightly) refuse the split: the note's stated price.
fn prose(row: usize) -> String {
    format!("a pane printed this sentence on row {row}")
}

/// A scan that closes a block, taken off the queue and resolved; everything queued before it is
/// completed as the worker would.
fn take_a_proven_task(session: &mut DualPlaneSession) -> LiveDetectionTask {
    loop {
        let mut task = session
            .take_live_worker_task()
            .expect("a scan that proves a block");
        if resolve_live_detection_task(&mut task) {
            return task;
        }
        session.complete_live_worker_result(task, Err(MathRenderError::NotDetected));
    }
}

fn frame_of(session: &DualPlaneSession) -> ViewportFrame {
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    session.viewport_frame(&mut projection).unwrap()
}

/// The text of one frame row between two columns, blanks as spaces.
fn frame_text(frame: &ViewportFrame, live_row: u32, columns: std::ops::Range<usize>) -> String {
    let width = frame.columns.get() as usize;
    let row = frame
        .row_map
        .iter()
        .position(|mapped| mapped.live_grid_row == Some(live_row))
        .expect("the row is on the frame");
    frame.cells[row * width..(row + 1) * width][columns]
        .iter()
        .map(|cell| {
            if cell.text.is_empty() {
                " "
            } else {
                cell.text.as_str()
            }
        })
        .collect()
}

/// The thirteen lines of the pane: one inline formula and two display blocks.
const PANE: [&str; 13] = [
    "Inline: $e^{i\\pi}+1=0$ stays inline.",
    "",
    "$$",
    "\\frac{1}{2}",
    "$$",
    "",
    "$$",
    "\\begin{pmatrix}",
    "a & b \\\\",
    "c & d \\\\",
    "e & f",
    "\\end{pmatrix}",
    "$$",
];

/// The screen `herdr` 0.8.2 repaints, as its 100×40 capture decodes (`herdr_client.bin`): a
/// twenty-five-cell sidebar, a `│` in column 25 on every row, the pane from column 26. The sidebar
/// text and the command lines are synthetic, with the capture's geometry.
fn herdr_screen() -> Vec<String> {
    let sidebar = |row: usize| match row {
        0 => " spaces",
        2 => " · math",
        19 => " new               ● menu",
        20 => "─────────────────────────",
        21 => " agents           grouped",
        39 => "                        «",
        _ => "",
    };
    (0..40)
        .map(|row| {
            let pane = match row {
                0 => "   1     +".to_owned(),
                1 => "user@host ~ % sh -c 'cat notes/math.md; sleep 30'".to_owned(),
                3..=15 => PANE[row - 3].to_owned(),
                16 => "sh -c 'cat notes/math.md; sleep 45'".to_owned(),
                _ => String::new(),
            };
            let side = sidebar(row);
            let padding = 25usize.saturating_sub(bt_unicode::text_width(side));
            format!("{side}{}\u{2502}{pane}", " ".repeat(padding))
        })
        .collect()
}

/// RED (69a) — **formulas typeset inside `herdr`, behind its sidebar.**
///
/// Read as one line, every pane row is `<25 cells>│<text>`: indented code whose trimmed form opens
/// on `│`, so all three formulas stayed source. The frame is the rule at column 25, the pane is
/// `[26, 100)`, and the pane's lines prove the `PANE`'s three formulas in its own columns: every
/// record stands in that pane, the display pictures are drawn from its first column and stop
/// nowhere short of the grid's edge, and the inline run's **cleared cells** are the pane's columns
/// 34 onwards — not columns 8 onwards, which is where the run's bytes would land if they were
/// counted from the start of the screen row.
///
/// MUTATION: return the unframed frame from `ScreenFrame::measure`, or count the inline run's
/// columns with `UnicodeWidthStr` over the pane's text in `live_fragment_cells`.
#[test]
fn herdr_pane_rows_typeset_behind_their_sidebar() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(100), nz(40));
    seat_inline_metrics(&mut session);
    session.feed_at(&repaint(&herdr_screen()), start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    assert_eq!(complete_for_real(&mut session), 3);
    let pane = rect(0, 40, 26, 100);
    assert_eq!(session.live_decorations.len(), 3);
    assert!(
        session
            .live_decorations
            .values()
            .all(|record| record.pane == pane && record.start.column >= 26),
        "every block stands in the pane: {:?}",
        session
            .live_decorations
            .values()
            .map(|record| (record.pane, record.start))
            .collect::<Vec<_>>()
    );
    let frame = frame_of(&session);
    let cell = session.cell_width_subpixels.get();
    let display = frame
        .math_blocks
        .iter()
        .filter(|block| {
            block.artifact.mode == MathMode::Display && block.display == MathBlockDisplay::Rendered
        })
        .collect::<Vec<_>>();
    assert_eq!(display.len(), 2);
    for block in display {
        assert_eq!(block.left_limit_columns, Some(26));
        assert_eq!(block.right_limit_columns, None);
        assert!(block.left_subpixels >= 26 * cell);
    }
    let inline = frame
        .math_blocks
        .iter()
        .find(|block| block.artifact.mode == MathMode::Inline)
        .expect("the inline picture");
    assert_eq!(inline.left_subpixels, 34 * cell);
    // `Inline: ` stays, the run's cells from column 34 are cleared, the sidebar is untouched.
    assert_eq!(frame_text(&frame, 3, 26..34), "Inline: ");
    assert_eq!(frame_text(&frame, 3, 34..48).trim(), "");
    assert_eq!(frame_text(&frame, 3, 25..26), "\u{2502}");
    assert_eq!(frame_text(&frame, 2, 0..8), " · math ");
}

/// The column of the rule a vertical split stands on, and so the width of the pane left of it.
const SPLIT_RULE_COLUMN: u32 = 49;

/// A `tmux` vertical split: the `PANE` left of the rule, an ordinary shell session right of it.
fn split_screen() -> Vec<String> {
    const NEIGHBOUR: [&str; 13] = [
        "the right pane is an ordinary shell",
        "$ ls -l",
        "total 24",
        "-rw-r--r--  1 user  staff   120 notes.md",
        "-rw-r--r--  1 user  staff  2048 report.md",
        "$ echo \"costs $5 and $10\"",
        "costs $5 and $10",
        "$ git status",
        "On branch main",
        "nothing to commit, working tree clean",
        "$ uname -a",
        "Darwin 25.0.0 arm64",
        "$",
    ];
    (0..40)
        .map(|row| {
            let math = PANE.get(row).copied().unwrap_or("");
            let neighbour = NEIGHBOUR.get(row).copied().unwrap_or("$");
            format!(
                "{math:<width$}\u{2502}{neighbour}",
                width = SPLIT_RULE_COLUMN as usize
            )
        })
        .collect()
}

/// RED (69a) — **a formula in one pane of a split is fitted to that pane and drawn inside it**
/// (R10). The band it is fitted to is the pane's forty-nine columns, not the screen's hundred, so a
/// raster the whole screen would take at full size is shrunk to the pane by the rule that has always
/// fitted an over-wide block; and the placement carries the rule's column as its right limit, where
/// the raster, its ground and its scissor stop.
///
/// MUTATION: fit a live block to `self.math_band()` in `sync_live_projection_artifacts`.
#[test]
fn a_formula_in_a_split_is_fitted_to_its_pane_and_drawn_inside_it() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(100), nz(40));
    session.feed_at(&repaint(&split_screen()), start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    // Wider than the pane's forty-nine columns of nine pixels, narrower than the screen's hundred.
    const NATURAL_WIDTH_PX: u32 = 600;
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(NATURAL_WIDTH_PX, 18)),
        3
    );
    let pane = rect(0, 40, 0, SPLIT_RULE_COLUMN);
    let display = session
        .live_decorations
        .values()
        .find(|record| record.span.mode == MathMode::Display)
        .expect("a display block");
    assert!(
        session
            .live_decorations
            .values()
            .all(|record| record.pane == pane)
    );
    let band = session.math_band_for(display);
    assert_eq!(band.pane_width_px, SPLIT_RULE_COLUMN * 9);
    let fitted = projected_live_artifact(
        display,
        session.layout_key(),
        band,
        session.math_vertical_padding_subpixels(),
        session.cell_height_subpixels.get(),
        session.live_block_box_limit_subpixels(display.screen),
    )
    .expect("a fitted artifact");
    let unfitted = projected_live_artifact(
        display,
        session.layout_key(),
        session.math_band(),
        session.math_vertical_padding_subpixels(),
        session.cell_height_subpixels.get(),
        session.live_block_box_limit_subpixels(display.screen),
    )
    .expect("an artifact");
    // At the screen's width this raster is not over-wide at all.
    assert_eq!(unfitted.render_scale_milli, 1000);
    let presented = u64::from(fitted.width_px) * u64::from(fitted.render_scale_milli) / 1000;
    let available = u64::from(math_block_available_width_px(
        band.pane_width_px,
        MathMode::Display,
        band.display_left_inset_subpixels,
    ));
    assert!(
        presented <= available,
        "{presented}px drawn in a {available}px pane"
    );
    let frame = frame_of(&session);
    let live = frame
        .math_blocks
        .iter()
        .filter(|block| matches!(block.anchor, MathBlockAnchor::Live { .. }))
        .collect::<Vec<_>>();
    assert!(!live.is_empty());
    for block in live {
        assert_eq!(block.right_limit_columns, Some(SPLIT_RULE_COLUMN));
        assert_eq!(block.left_limit_columns, None);
    }
    // The pane right of the rule keeps its text on the rows a band stands on.
    assert_eq!(frame_text(&frame, 3, 50..58), "-rw-r--r");
}

/// RED (69a) — **a framed pane of wide characters places its formula in the grid's cells** (R7).
/// The sidebar is CJK and so is the pane's prose, so every column on this screen is a cell and not a
/// character. Detection anchors the occurrence through the pane's captured boundaries and placement
/// looks its cells up the same way: the two name one column, and no cell of the sidebar is claimed.
///
/// MUTATION: count the run's columns from the pane text's own width in `live_fragment_cells`.
#[test]
fn a_framed_pane_of_wide_characters_places_its_formula_in_the_grid_cells() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(60), nz(20));
    seat_inline_metrics(&mut session);
    let rows = (0..20)
        .map(|row| {
            let pane = if row == 3 {
                "前置 $e^{i\\pi}+1=0$ 之后"
            } else {
                "普通输出"
            };
            format!("目录条目\u{2502}{pane}")
        })
        .collect::<Vec<_>>();
    session.feed_at(&repaint(&rows), start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    assert_eq!(complete_for_real(&mut session), 1);
    let record = session
        .live_decorations
        .values()
        .find(|record| record.span.mode == MathMode::Inline)
        .expect("the inline formula");
    assert_eq!(record.pane, rect(0, 20, 9, 60));
    // `前置 ` is two wide clusters and a space: five cells after the pane's first column.
    assert_eq!(record.start.column, 14);
    let frame = frame_of(&session);
    let run = record.span.inline_runs.first().expect("its run");
    let (_, left, cells) = live_inline_run_cells(
        &frame,
        record.pane_inputs().expect("the pane's rows"),
        record.start.row,
        run,
    )
    .expect("the run's cells");
    assert_eq!(left, record.start.column);
    assert!(cells.iter().all(|index| index % 60 >= 14));
}

/// RED (69a) — **a pane narrower than a block's two marks keeps the block's source** (R11). The
/// screen is eight columns wide with rules at columns 2 and 5, and `$$ / x / $$` stands in the
/// two-column pane between them. The block is proven there, but a pane of eighteen pixels cannot
/// hold the forty-six the source and copy marks take, so nothing is presented: no picture, no
/// cleared cell, nothing scissored — the rows read exactly as the program printed them.
///
/// MUTATION: return `false` from `live_pane_narrower_than_marks`.
#[test]
fn a_two_column_region_keeps_source_when_the_formula_cannot_fit() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(8), nz(10));
    let rows = (0..10)
        .map(|row| {
            let middle = match row {
                0 | 2 => "$$",
                1 => "x",
                _ => "",
            };
            format!("ab\u{2502}{middle:<2}\u{2502}ef")
        })
        .collect::<Vec<_>>();
    session.feed_at(&repaint(&rows), start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
        1,
        "the block is proven in its pane"
    );
    let record = session
        .live_decorations
        .values()
        .next()
        .expect("the record");
    assert_eq!(record.pane, rect(0, 10, 3, 5));
    assert!(session.live_pane_narrower_than_marks(record));
    let frame = frame_of(&session);
    assert!(
        frame
            .math_blocks
            .iter()
            .all(|block| !matches!(block.anchor, MathBlockAnchor::Live { .. })),
        "nothing of the block is presented: {:?}",
        frame.math_blocks
    );
    assert_eq!(frame_text(&frame, 0, 3..5), "$$");
    assert_eq!(frame_text(&frame, 1, 3..5), "x ");
    assert_eq!(frame_text(&frame, 2, 3..5), "$$");
}

/// RED (69a) — **the oracle's ledger owns a framed pane's formulas** (note §4 step 7). On the herdr
/// screen the held-unbacked report is exactly what it is for the same pane with no frame around it:
/// no display formula is reported, because the pane's own ledger owns it. The branch never touched
/// the oracle, and a whole-screen ledger over `<25 cells>│$$` rows owns nothing.
///
/// MUTATION: build the ledger from the whole capture (`pane_ownership_ledger(capture.inputs(), …)`)
/// instead of per pane.
#[test]
fn the_oracles_ledger_owns_a_framed_panes_formulas() {
    let held = |rows: &[String], columns: u32| {
        let start = Instant::now();
        let mut session = DualPlaneSession::new(nz(columns), nz(40));
        seat_inline_metrics(&mut session);
        session.feed_at(&repaint(rows), start).unwrap();
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
        assert_eq!(complete_for_real(&mut session), 3);
        session
            .held_unbacked_records()
            .into_iter()
            .map(|record| record.original_source)
            .collect::<Vec<_>>()
    };
    let framed = held(&herdr_screen(), 100);
    let unframed_rows = herdr_screen()
        .iter()
        .map(|row| {
            row.split_once('\u{2502}')
                .map_or(String::new(), |(_, pane)| pane.to_owned())
        })
        .collect::<Vec<_>>();
    let unframed = held(&unframed_rows, 74);
    assert_eq!(framed, unframed);
    assert!(
        framed.iter().all(|source| !source.contains("$$")),
        "a display formula the pane proves is reported unbacked: {framed:?}"
    );
}

/// RED (69a) — **the oracle does not back a stale region from another region's equal source** (Codex
/// finding 5). The same `$$ / x / $$` stands in the left pane on rows 1–3 and in the right pane on
/// rows 6–8. The right occurrence is erased while a hold keeps its record and artifact; the right
/// pane's own ledger no longer owns that source, and the record is reported — although the left
/// pane still owns an equal one, which a union of the panes' ledgers would have counted.
///
/// MUTATION: answer `held_unbacked_records` from any pane's ledger (a union).
#[test]
fn the_oracle_does_not_back_a_stale_region_from_another_regions_equal_source() {
    let block = |row: usize| match row {
        0 | 2 => "$$",
        1 => "x",
        _ => "",
    };
    let rows = (0..20_usize)
        .map(|row| {
            let left = match row.checked_sub(1).map_or("", block) {
                "" => prose(row),
                text => text.to_owned(),
            };
            let right = match row.checked_sub(6).map_or("", block) {
                "" => prose(row),
                text => text.to_owned(),
            };
            format!("{left:<60}\u{2502}{right}")
        })
        .collect::<Vec<_>>();
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(120), nz(20));
    session.feed_at(&repaint(&rows), start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
        2
    );
    let right = session
        .live_decorations
        .values()
        .find(|record| record.pane.left == 61)
        .cloned()
        .expect("the right pane's record");
    assert!(session.held_unbacked_records().is_empty());

    // Erase the right occurrence; a hold keeps its record standing with its artifact.
    for row in 6..=8u32 {
        session
            .feed_at(
                &rewrite_row(
                    row,
                    &format!("{:<60}\u{2502}{}", prose(row as usize), prose(row as usize)),
                ),
                start + LIVE_MATH_STABLE_INTERVAL,
            )
            .unwrap();
    }
    session
        .live_decorations
        .insert(right.start.row, right.clone());
    let reported = session.held_unbacked_records();
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0].band_start_row, right.band_start_row);
}

/// RED (69a) — **a block in one pane does not displace a block in the other pane on the same rows.**
/// The left pane's block stands on rows 5–7 and the right pane's on rows 6–8. Installing one used to
/// retire every record whose rows it overlapped, so the second to land took the first down and its
/// candidate stayed spent: one formula of the two stayed source. A record is displaced only by a
/// block in its own columns.
///
/// MUTATION: drop the pane-columns clause from the displacement in `apply_live_worker_completion`.
#[test]
fn a_block_in_one_pane_does_not_displace_the_other_panes_block_on_its_rows() {
    let rows = (0..20_usize)
        .map(|row| {
            let left = match row {
                5 | 7 => "$$".to_owned(),
                6 => "p^2".to_owned(),
                _ => prose(row),
            };
            let right = match row {
                6 | 8 => "$$".to_owned(),
                7 => "q^2".to_owned(),
                _ => prose(row),
            };
            format!("{left:<60}\u{2502}{right}")
        })
        .collect::<Vec<_>>();
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(120), nz(20));
    session.feed_at(&repaint(&rows), start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
        2
    );
    let mut panes = session
        .live_decorations
        .values()
        .map(|record| record.pane)
        .collect::<Vec<_>>();
    panes.sort();
    assert_eq!(panes, vec![rect(0, 20, 0, 60), rect(0, 20, 61, 120)]);
}

// ---- §7.4: ticket 69b (T-PANE-IDENTITY) -------------------------------------------------------

/// A 40×100 split at column 50 with a display block in the left pane on rows 10–12, and a spinner
/// in the right pane on row 11 at column 80.
fn spinner_screen(spinner: char) -> Vec<String> {
    (0..40)
        .map(|row| {
            let left = match row {
                10 | 12 => "$$".to_owned(),
                11 => "x^2".to_owned(),
                _ => prose(row),
            };
            let right = if row == 11 {
                format!("{:<29}{spinner}", "working")
            } else {
                prose(row)
            };
            format!("{left:<50}\u{2502}{right}")
        })
        .collect()
}

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

/// RED (69b) — **a formula in one pane settles while the other pane writes** (Codex finding 6). A
/// spinner in the right pane changes row 11 every 50 ms; the left block on rows 10–12 still arms
/// and lands, and is not re-armed while the spinner runs.
///
/// 69a keys stability by the whole screen row, so the spinner keeps row 11 from ever settling and
/// the left formula never arms: the note's stated cost of shipping 69a alone.
///
/// MUTATION: compare whole-row fingerprints in the pane math tier.
#[test]
#[ignore = "69b"]
fn a_formula_in_one_pane_settles_while_the_other_pane_writes() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(100), nz(40));
    session
        .feed_at(&repaint(&spinner_screen('|')), start)
        .unwrap();
    let mut landed = 0;
    for tick in 1..=40u32 {
        let at = start + Duration::from_millis(50) * tick;
        let spinner = SPINNER[tick as usize % SPINNER.len()];
        session
            .feed_at(&rewrite_row(11, &spinner_screen(spinner)[11]), at)
            .unwrap();
        session.advance_live_stability(at);
        landed += complete_detected_live_tasks(&mut session, synthetic_raster(40, 40));
    }
    assert_eq!(landed, 1, "the left block lands once and is not re-armed");
    assert!(
        session
            .live_decorations
            .values()
            .any(|record| record.pane == rect(0, 40, 0, 50))
    );
}

/// RED (69b) — **a spinner in one pane advances the row but not the other pane's math.** Row 11's
/// global revision advances with every spinner frame, as the printed-path watermark needs; the left
/// pane's math revision does not, so its already-landed block stays standing and is not re-armed.
///
/// 69a has one clock per row: the spinner's damage invalidates the left block's record.
///
/// MUTATION: advance the pane math clock on any damaged row.
#[test]
#[ignore = "69b"]
fn a_spinner_in_one_pane_advances_the_row_but_not_the_other_panes_math() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(100), nz(40));
    session
        .feed_at(&repaint(&spinner_screen('|')), start)
        .unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
        1
    );
    let occurrence = session
        .live_decorations
        .values()
        .next()
        .map(|record| record.identity.occurrence_id)
        .expect("the landed block");
    let revision_before = session.live_rows[11].revision;
    for tick in 1..=10u32 {
        let at = start + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(50) * tick;
        let spinner = SPINNER[tick as usize % SPINNER.len()];
        session
            .feed_at(&rewrite_row(11, &spinner_screen(spinner)[11]), at)
            .unwrap();
        session.advance_live_stability(at);
        assert_eq!(
            complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
            0,
            "nothing re-armed at tick {tick}"
        );
    }
    assert!(session.live_rows[11].revision > revision_before);
    assert!(
        session
            .live_decorations
            .values()
            .any(|record| record.identity.occurrence_id == occurrence),
        "the left block kept its record"
    );
}

/// A 40×100 split at column 50 with a display block in each pane; `horizontal` adds a
/// junction-anchored horizontal split at row 20 inside the right pane only.
fn two_pane_screen(horizontal: bool) -> Vec<String> {
    (0..40_usize)
        .map(|row| {
            let block = |first: usize| match row.checked_sub(first) {
                Some(0 | 2) => "$$",
                Some(1) => "y^2",
                _ => "",
            };
            let left = match block(5) {
                "" => prose(row),
                text => text.to_owned(),
            };
            if horizontal && row == 20 {
                return format!("{left:<50}\u{251c}{}", "\u{2500}".repeat(49));
            }
            let right = match block(25) {
                "" => prose(row),
                text => text.to_owned(),
            };
            format!("{left:<50}\u{2502}{right}")
        })
        .collect()
}

/// RED (69a, written for 69b) — **a frame change invalidates every pane-keyed state** (Codex's
/// check of (b), blocker 2; coordinator's ruling 2026-09-29). Capture A is a split at column 50;
/// capture B adds a junction-anchored horizontal split at row 20 inside the right pane only. On B
/// every record is dropped — the left pane's too, although its rectangle did not move — and both
/// panes' formulas re-arm and land again, under new occurrences.
///
/// 69a builds the frame identity and wires this invalidation (`observe_frame`), so the test runs
/// here; 69b adds the pane math tier it also clears.
///
/// MUTATION: compare frames by the left pane's rectangle only, or skip `invalidate_every_pane`.
#[test]
fn a_frame_change_invalidates_every_pane_keyed_state() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(100), nz(40));
    session
        .feed_at(&repaint(&two_pane_screen(false)), start)
        .unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
        2
    );
    let before = session
        .live_decorations
        .values()
        .map(|record| (record.pane, record.identity.occurrence_id))
        .collect::<Vec<_>>();
    let left = rect(0, 40, 0, 50);
    assert!(before.iter().any(|(pane, _)| *pane == left));

    let changed = start + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(10);
    session
        .feed_at(&rewrite_row(20, &two_pane_screen(true)[20]), changed)
        .unwrap();
    session.advance_live_stability(changed + LIVE_MATH_STABLE_INTERVAL);
    // Every record of the old frame is gone, the unchanged left pane's included.
    assert!(
        session.live_decorations.values().all(|record| before
            .iter()
            .all(|(_, id)| *id != record.identity.occurrence_id)),
        "a record minted against the old frame survived"
    );
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
        2,
        "both panes' formulas re-arm and land again"
    );
    let after = session
        .live_decorations
        .values()
        .map(|record| record.pane)
        .collect::<Vec<_>>();
    assert!(after.contains(&left));
    assert!(after.contains(&rect(21, 40, 51, 100)));
}

/// RED (69a, written for 69b) — **a fence opened on the status row suppresses every pane, and its
/// closing releases them** (Codex's check of (b), blocker 3). Row 0 is an excluded status row; rows
/// 1–39 are split at column 50 with a settled formula in each pane. Repainting only row 0 to three
/// backticks leaves the frame's rectangles alone but changes the screen-owned fence state: every
/// record retires and no pane re-arms. Repainting it back re-arms both, and their formulas land
/// again. A task resolved before the opening is refused at completion.
///
/// MUTATION: leave the screen fence state out of `frames_agree`.
#[test]
fn a_fence_opened_on_the_status_row_suppresses_every_pane_and_its_closing_releases_them() {
    let screen = |status: &str| {
        let mut rows = vec![status.to_owned()];
        rows.extend(two_pane_screen(false).into_iter().skip(1));
        rows
    };
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(100), nz(40));
    session
        .feed_at(&repaint(&screen("status ready")), start)
        .unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    let in_flight = take_a_proven_task(&mut session);
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
        1
    );

    let opened = start + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(10);
    session.feed_at(&rewrite_row(0, "```"), opened).unwrap();
    session.advance_live_stability(opened + LIVE_MATH_STABLE_INTERVAL);
    assert!(session.live_decorations.is_empty(), "every record retires");
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
        0,
        "no pane re-arms inside the screen's fence"
    );
    assert!(
        !session.complete_live_worker_result(in_flight, Ok(synthetic_raster(40, 40))),
        "a task resolved before the opening is refused"
    );

    let closed = opened + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(10);
    session
        .feed_at(&rewrite_row(0, "status ready"), closed)
        .unwrap();
    session.advance_live_stability(closed + LIVE_MATH_STABLE_INTERVAL);
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
        2,
        "both panes re-arm and land again"
    );
}

/// Two panes that each hold a display block ending on row 12 (`starts` = the rows they open on).
///
/// The left pane writes `\[ … \]` and the right `$$ … $$`: two bare `$$` on one screen row read,
/// unsplit, as one complete display `$$ … │$$` across the rule, which R5 rightly refuses to cut
/// (finding F-2 of the T-PANE-COLUMNS report); these tests are about the panes' identity, not that.
fn same_row_screen(left_start: usize, right_start: usize) -> Vec<String> {
    (0..40)
        .map(|row| {
            let block = |first: usize, last: usize, open: &'static str, close: &'static str| {
                if row == first {
                    open
                } else if row == last {
                    close
                } else if (first..last).contains(&row) {
                    "z^2"
                } else {
                    ""
                }
            };
            let left = match block(left_start, 12, "\\[", "\\]") {
                "" => prose(row),
                text => text.to_owned(),
            };
            let right = match block(right_start, 12, "$$", "$$") {
                "" => prose(row),
                text => text.to_owned(),
            };
            format!("{left:<50}\u{2502}{right}")
        })
        .collect()
}

/// RED (69b) — **two panes closing on the same row both typeset** (the branch's B-4). 69a looks the
/// candidate row up pane by pane and fills the task from the first pane whose block closes there, so
/// one of the two formulas never gets a task.
///
/// MUTATION: key candidates by row alone.
#[test]
#[ignore = "69b"]
fn two_panes_closing_on_the_same_row_both_typeset() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(100), nz(40));
    session
        .feed_at(&repaint(&same_row_screen(8, 10)), start)
        .unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    assert_eq!(
        complete_detected_live_tasks(&mut session, synthetic_raster(40, 40)),
        2
    );
    let mut panes = session
        .live_decorations
        .values()
        .map(|record| record.pane)
        .collect::<Vec<_>>();
    panes.sort();
    assert_eq!(panes, vec![rect(0, 40, 0, 50), rect(0, 40, 51, 100)]);
}

/// RED (69b) — **two panes starting on the same row both keep their records** (the branch's B-4).
/// `live_decorations` is keyed by the start row in 69a, so the second record installed on row 10
/// replaces the first.
///
/// MUTATION: key `live_decorations` by the start row alone.
#[test]
#[ignore = "69b"]
fn two_panes_starting_on_the_same_row_both_keep_their_records() {
    let rows = (0..40)
        .map(|row| {
            // `\[ … \]` left and `$$ … $$` right, for the reason `same_row_screen` gives.
            let left = match row {
                10 => "\\[".to_owned(),
                12 => "\\]".to_owned(),
                11 => "a^2".to_owned(),
                _ => prose(row),
            };
            let right = match row {
                10 | 14 => "$$".to_owned(),
                11..=13 => "b^2".to_owned(),
                _ => prose(row),
            };
            format!("{left:<50}\u{2502}{right}")
        })
        .collect::<Vec<_>>();
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(100), nz(40));
    session.feed_at(&repaint(&rows), start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_detected_live_tasks(&mut session, synthetic_raster(40, 40));
    let mut panes = session
        .live_decorations
        .values()
        .map(|record| record.pane)
        .collect::<Vec<_>>();
    panes.sort();
    assert_eq!(panes, vec![rect(0, 40, 0, 50), rect(0, 40, 51, 100)]);
}

/// RED (69b) — **completion ignores the other pane's half of the row.** A task for the left pane's
/// block is in flight when the right pane rewrites its half of one of the block's rows;
/// `live_task_is_current` compares only the pane's slice, so the completion is accepted.
///
/// 69a compares the dependency rows whole and refuses it.
///
/// MUTATION: compare the whole row in `live_task_is_current`.
#[test]
#[ignore = "69b"]
fn completion_ignores_the_other_panes_half_of_the_row() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(100), nz(40));
    session
        .feed_at(&repaint(&spinner_screen('|')), start)
        .unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    let task = take_a_proven_task(&mut session);
    session
        .feed_at(
            &rewrite_row(11, &spinner_screen('/')[11]),
            start + LIVE_MATH_STABLE_INTERVAL,
        )
        .unwrap();
    assert!(session.complete_live_worker_result(task, Ok(synthetic_raster(40, 40))));
}
