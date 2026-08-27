use std::{
    num::{NonZeroU32, NonZeroUsize},
    time::{Duration, Instant},
};

use bt_detect::resolve_detection_task;
use bt_doc::{ContentAnchor, DecorationIntent, DecorationLifecycle};
use bt_math::MathRaster;
use bt_term::DualPlaneSession;
use bt_transcript::{CellFlags, TerminalColor};
use bt_viewport::{FrameViewportOrigin, ViewportFrame};
use proptest::prelude::*;

fn nz32(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn nz_size(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}

fn history_text(session: &DualPlaneSession) -> Vec<&str> {
    session
        .document()
        .entries()
        .values()
        .map(|entry| entry.line.text.as_str())
        .collect()
}

fn staged_text(session: &DualPlaneSession) -> Vec<String> {
    session
        .transcript()
        .staged_rows()
        .map(|staged| {
            staged
                .row
                .cells
                .iter()
                .filter(|cell| !cell.wide_spacer)
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn repaint_rows<T: AsRef<str>>(rows: &[T]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (row, text) in rows.iter().enumerate() {
        bytes.extend_from_slice(format!("\x1b[{};1H\x1b[2K{}", row + 1, text.as_ref()).as_bytes());
    }
    bytes
}

fn finish_resize_transaction(session: &mut DualPlaneSession, request_at: Instant) {
    let (columns, rows) = session.terminal().dimensions();
    session.mark_pty_resize_requested_at(columns, rows, request_at);
    assert!(
        session
            .finish_resize_if_quiescent(request_at + Duration::from_millis(200))
            .unwrap()
    );
}

fn complete_next_math(session: &mut DualPlaneSession, height_px: u32) {
    let mut task = session
        .take_worker_task()
        .expect("pending math worker task");
    assert!(resolve_detection_task(&mut task));
    assert!(session.complete_worker_result(
        task,
        Ok(MathRaster {
            rgba: vec![0; 4 * height_px as usize],
            width_px: 1,
            height_px,
            content_height_px: height_px,
            ascent_px: height_px as f32,
            descent_px: 0.0,
            baseline_px: height_px as f32,
            render_time: Duration::from_millis(1),
            inline_runs: Vec::new(),
        })
    ));
}

fn logical_content(session: &DualPlaneSession) -> Vec<String> {
    let mut logical = history_text(session)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let staged = session
        .transcript()
        .staged_rows()
        .map(|row| row.row.clone());
    let (_, live_rows) = session.terminal().dimensions();
    let live = (0..live_rows.get()).filter_map(|row| session.terminal().visible_row(row));
    let mut current = String::new();
    for row in staged.chain(live) {
        current.extend(
            row.cells
                .iter()
                .filter(|cell| !cell.wide_spacer)
                .map(|cell| cell.text.as_str()),
        );
        if !row.continues {
            let trimmed = current.trim_end();
            if !trimmed.is_empty() {
                logical.push(trimmed.to_owned());
            }
            current.clear();
        }
    }
    let trimmed = current.trim_end();
    if !trimmed.is_empty() {
        logical.push(trimmed.to_owned());
    }
    logical
}

fn frame_row_text(frame: &ViewportFrame, row: usize) -> String {
    let columns = frame.columns.get() as usize;
    frame.cells[row * columns..(row + 1) * columns]
        .iter()
        .map(|cell| cell.text.as_str())
        .collect()
}

fn captured_row_text(row: &bt_transcript::CapturedRow) -> String {
    row.cells.iter().map(|cell| cell.text.as_str()).collect()
}

fn assert_frame_rows_come_from_real_sources(session: &DualPlaneSession, frame: &ViewportFrame) {
    let columns = frame.columns.get() as usize;
    for row in 0..frame.drawable_rows() {
        let source = &frame.cell_anchors[row * columns].start;
        let expected = match source {
            ContentAnchor::History { id, offset, .. } => {
                assert_eq!(offset.0, 0, "fixture history rows never wrap");
                session.document().entries()[id].line.text.clone()
            }
            ContentAnchor::Staging { id, offset, .. } => {
                assert_eq!(offset.0, 0, "fixture staging rows never wrap");
                captured_row_text(
                    &session
                        .transcript()
                        .staged_rows()
                        .find(|staged| staged.id == *id)
                        .expect("visible staging anchor has a source row")
                        .row,
                )
            }
            ContentAnchor::Live { point, .. } => captured_row_text(
                &session
                    .terminal()
                    .visible_row(point.row)
                    .expect("visible live anchor has a source row"),
            ),
        };
        assert_eq!(
            frame_row_text(frame, row),
            expected,
            "composed row {row} must be the exact source row, including intentional erasure"
        );
    }
}

fn collapse_blocks(block_count: usize, rows: usize) -> Vec<Vec<u8>> {
    (0..block_count)
        .map(|block| {
            let first = block * rows / block_count;
            let last = (block + 1) * rows / block_count;
            let mut bytes = b"\x1b[?2026h".to_vec();
            for row in first..last {
                bytes.extend_from_slice(format!("\x1b[{};1H\x1b[2K", row + 1).as_bytes());
                if row % 3 != 1 {
                    bytes.extend_from_slice(format!("collapsed-{row:02}").as_bytes());
                }
            }
            bytes.extend_from_slice(b"\x1b[?2026l");
            bytes
        })
        .collect()
}

#[test]
fn m1_8f_collapse_lifecycle_matrix_has_no_projection_holes_or_anchor_drift() {
    const COLUMNS: u32 = 20;
    const ROWS: u32 = 8;
    const ESU_LEN: usize = b"\x1b[?2026l".len();
    let mut scenarios = 0;
    let mut published_frames = 0;

    for scrolled_out in [4_usize, 12] {
        for block_count in [1_usize, 4] {
            for anchored in [false, true] {
                for timeout_commit in [false, true] {
                    scenarios += 1;
                    let seed = (0..scrolled_out + ROWS as usize)
                        .map(|row| format!("seed-{row:02}"))
                        .collect::<Vec<_>>()
                        .join("\r\n");
                    let blocks = collapse_blocks(block_count, ROWS as usize);

                    let mut session = DualPlaneSession::new(nz32(COLUMNS), nz32(ROWS));
                    session.feed(seed.as_bytes()).unwrap();
                    let mut projection = session.new_projection(session.layout_key());
                    let bottom = session.viewport_frame(&mut projection).unwrap();
                    assert_frame_rows_come_from_real_sources(&session, &bottom);
                    if anchored {
                        projection.scroll_by_rows(3);
                    }
                    session.refresh_projection(&mut projection);
                    let initial = session.viewport_frame(&mut projection).unwrap();
                    assert_frame_rows_come_from_real_sources(&session, &initial);
                    let pinned_top = frame_row_text(&initial, 0);
                    if anchored {
                        assert!(matches!(
                            initial.viewport_origin,
                            FrameViewportOrigin::Anchored(_)
                        ));
                    }

                    for (index, block) in blocks.iter().enumerate() {
                        if timeout_commit && index == block_count / 2 {
                            session.feed(&block[..block.len() - ESU_LEN]).unwrap();
                            assert!(session.synchronized_update_deadline().is_some());
                            assert!(session.finish_synchronized_update(Instant::now()).unwrap());
                        } else {
                            session.feed(block).unwrap();
                        }
                        session.refresh_projection(&mut projection);
                        let frame = session.viewport_frame(&mut projection).unwrap();
                        frame.validate_shape().unwrap();
                        assert_frame_rows_come_from_real_sources(&session, &frame);
                        if anchored {
                            assert!(matches!(
                                frame.viewport_origin,
                                FrameViewportOrigin::Anchored(_)
                            ));
                            assert_eq!(frame_row_text(&frame, 0), pinned_top);
                        }
                        published_frames += 1;
                    }

                    let split_final = session.viewport_frame(&mut projection).unwrap();
                    let mut direct = DualPlaneSession::new(nz32(COLUMNS), nz32(ROWS));
                    direct.feed(seed.as_bytes()).unwrap();
                    let mut direct_projection = direct.new_projection(direct.layout_key());
                    let _ = direct.viewport_frame(&mut direct_projection).unwrap();
                    if anchored {
                        direct_projection.scroll_by_rows(3);
                    }
                    direct
                        .feed(&blocks.iter().flatten().copied().collect::<Vec<_>>())
                        .unwrap();
                    direct.refresh_projection(&mut direct_projection);
                    let direct_final = direct.viewport_frame(&mut direct_projection).unwrap();

                    assert_eq!(
                        session.terminal().visible_text(),
                        direct.terminal().visible_text()
                    );
                    assert_eq!(split_final.cells, direct_final.cells);
                    assert_eq!(
                        split_final.scroll_offset_rows,
                        direct_final.scroll_offset_rows
                    );
                    assert_eq!(
                        frame_row_text(&split_final, 0),
                        frame_row_text(&direct_final, 0)
                    );
                }
            }
        }
    }

    assert_eq!(scenarios, 16);
    assert_eq!(published_frames, 40);
}

#[test]
fn m1_9a_math_ready_and_output_publications_preserve_a_wheel_anchor_inside_a_math_block() {
    let mut session = DualPlaneSession::new(nz32(16), nz32(2));
    session.feed(b"$$x^2$$\r\nnext\r\ntail").unwrap();
    complete_next_math(&mut session, 64);

    let mut projection = session.new_projection(session.layout_key());
    let bottom = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(bottom.scroll_offset_rows, 0);

    // The rendered block is four visual rows high. One wheel notch targets its last row, which
    // must become a semantic anchor with an in-block local offset instead of falling back to Bottom.
    projection.scroll_by_rows(1);
    session.refresh_projection(&mut projection);
    let scrolled = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(scrolled.scroll_offset_rows, 1);
    assert!(matches!(
        scrolled.viewport_origin,
        FrameViewportOrigin::Anchored(_)
    ));
    let anchor = projection.scroll_anchor().unwrap().clone();
    assert!(anchor.local_offset > 0);
    let math_top = scrolled.math_blocks[0].top_subpixels;

    // Ordinary PTY output may publish while the worker is pending, but it must not impersonate
    // keyboard input or reset the user's viewport-follow state.
    session.feed(b"\r\n$$y^2$$\r\nbelow\r\ntail-2").unwrap();
    session.refresh_projection(&mut projection);
    let output_publish = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(projection.scroll_anchor(), Some(&anchor));
    assert_eq!(output_publish.math_blocks[0].top_subpixels, math_top);

    // Completing the newly frozen formula changes the height tree below the anchor. Its publish
    // must reproject from the semantic source/local offset rather than selecting Bottom again.
    complete_next_math(&mut session, 72);
    session.refresh_projection(&mut projection);
    let math_ready_publish = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(projection.scroll_anchor(), Some(&anchor));
    assert_eq!(math_ready_publish.math_blocks[0].top_subpixels, math_top);
    assert!(matches!(
        math_ready_publish.viewport_origin,
        FrameViewportOrigin::Anchored(_)
    ));
}

#[test]
fn g1_scroll_out_tail_rewrite_with_inline_prose_stays_plain() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(2));
    session.feed(b"$$abcdef$$Z\r\n").unwrap();
    assert!(session.document().entries().is_empty());
    assert_eq!(session.transcript().staging_len(), 1);

    session.feed(b"\x1b[1;5H!\x1b[2;1H\r\n").unwrap();
    let (id, entry) = session.document().entries().first_key_value().unwrap();
    let id = *id;
    assert_eq!(entry.line.text, "$$abcdef$$Z !");
    assert_eq!(entry.decoration, DecorationIntent::Plain);
    assert_eq!(
        session.decoration(id).unwrap().decoration,
        DecorationLifecycle::Pending
    );
    session.run_workers();
    assert_eq!(
        session.decoration(id).unwrap().decoration,
        DecorationLifecycle::Suppressed
    );
    assert_eq!(
        session.document().entries().get(&id).unwrap().decoration,
        DecorationIntent::Plain
    );
}

#[test]
fn g1_resize_grow_makes_the_entire_new_grid_addressable() {
    let mut session = DualPlaneSession::new(nz32(4), nz32(2));
    session.resize(nz32(6), nz32(4)).unwrap();
    session.feed(b"\x1b[4;6HZ").unwrap();
    let visible = session.terminal().visible_text();
    assert_eq!(visible.len(), 4);
    assert_eq!(visible[3], "     Z");
}

#[test]
fn reused_projection_refreshes_layout_before_framing_a_width_resize() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(2));
    let mut projection = session.new_projection(session.layout_key());
    let initial = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(
        (
            initial.columns.get(),
            initial.grid_rows.get(),
            initial.rows.get(),
            initial.cells.len()
        ),
        (8, 2, 3, 24)
    );

    session.resize(nz32(20), nz32(2)).unwrap();
    session.refresh_projection(&mut projection);
    let resized = session.viewport_frame(&mut projection).unwrap();

    assert_eq!(projection.layout_key(), session.layout_key());
    assert_eq!(
        (
            resized.columns.get(),
            resized.grid_rows.get(),
            resized.rows.get(),
            resized.cells.len()
        ),
        (20, 2, 3, 60)
    );
}

#[test]
fn g1_resize_staging_exposes_nonblank_shrink_rows_until_grow_restores_them() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(4));
    session.feed(b"r1\r\nr2\r\nr3\r\nr4").unwrap();
    session.resize(nz32(8), nz32(2)).unwrap();
    assert!(history_text(&session).is_empty());
    assert_eq!(staged_text(&session), ["r1", "r2"]);
    assert_eq!(session.terminal().resize_transaction_history_size(), 2);
    assert_eq!(session.terminal().visible_text(), vec!["r3", "r4"]);

    session.resize(nz32(8), nz32(4)).unwrap();
    assert!(history_text(&session).is_empty());
    assert_eq!(session.transcript().staging_len(), 0);
    assert_eq!(
        session.terminal().visible_text(),
        vec!["r1", "r2", "r3", "r4"]
    );
}

#[test]
fn g1_vendor_tail_harvests_once_at_transaction_finish() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz32(8), nz32(4));
    session.feed_at(b"r1\r\nr2\r\nr3\r\nr4", start).unwrap();
    session
        .resize_at(nz32(8), nz32(2), start + Duration::from_millis(10))
        .unwrap();
    assert_eq!(session.transcript().staging_len(), 2);
    assert_eq!(session.terminal().resize_transaction_history_size(), 2);

    session
        .feed_at(b"\r\nnew", start + Duration::from_millis(20))
        .unwrap();
    assert!(history_text(&session).is_empty());
    finish_resize_transaction(&mut session, start + Duration::from_millis(210));
    assert_eq!(history_text(&session), vec!["r1", "r2", "r3"]);
    assert_eq!(session.transcript().staging_len(), 0);

    session.resize(nz32(8), nz32(4)).unwrap();
    assert_eq!(session.terminal().visible_text(), vec!["r4", "new", "", ""]);
}

#[test]
fn g1_width_reflow_keeps_the_displaced_banner_reachable_through_history() {
    const WARNING: &str = "Did not find path entry D:\\App\\Base\\anaconda3\\bin";
    const PROMPT: &str = "(base) PS D:\\Developer\\folio-terminal> ";

    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz32(80), nz32(2));
    session
        .feed_at(format!("{WARNING}\r\n{PROMPT}").as_bytes(), start)
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let _ = session.viewport_frame(&mut projection).unwrap();

    session
        .resize_at(nz32(47), nz32(2), start + Duration::from_millis(10))
        .unwrap();

    assert_eq!(logical_content(&session), [WARNING, PROMPT.trim_end()]);
    let staged_banner_head = staged_text(&session).join("");
    assert!(
        !staged_banner_head.is_empty() && WARNING.starts_with(&staged_banner_head),
        "the row displaced above the live grid must be owned by transcript staging: {staged_banner_head:?}"
    );

    session.refresh_projection(&mut projection);
    let _ = session.viewport_frame(&mut projection).unwrap();
    projection.scroll_to_top();
    session.refresh_projection(&mut projection);
    let narrow_review = session.viewport_frame(&mut projection).unwrap();
    let narrow_review_text = (0..narrow_review.drawable_rows())
        .map(|row| frame_row_text(&narrow_review, row))
        .collect::<String>();
    assert!(
        narrow_review_text.contains("Did not find path entry"),
        "scrolling to the ceiling must reveal the displaced banner head: {narrow_review_text:?}"
    );

    finish_resize_transaction(&mut session, start + Duration::from_millis(210));
    assert_eq!(logical_content(&session), [WARNING, PROMPT.trim_end()]);

    session
        .resize_at(nz32(80), nz32(2), start + Duration::from_millis(1_000))
        .unwrap();
    finish_resize_transaction(&mut session, start + Duration::from_millis(1_200));
    assert_eq!(logical_content(&session), [WARNING, PROMPT.trim_end()]);
}

#[test]
fn g1_sparse_width_reflow_grows_down_into_blank_rows() {
    const LINE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvw";
    const PROMPT: &str = "BTP> ";

    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz32(104), nz32(8));
    session
        .feed_at(format!("{LINE}\r\n{PROMPT}").as_bytes(), start)
        .unwrap();
    assert_eq!(
        session.terminal().visible_text(),
        [LINE, PROMPT.trim_end(), "", "", "", "", "", ""]
    );

    session
        .resize_at(nz32(46), nz32(8), start + Duration::from_millis(10))
        .unwrap();

    assert_eq!(session.transcript().staging_len(), 0);
    assert_eq!(session.terminal().resize_transaction_history_size(), 0);
    assert_eq!(
        session.terminal().visible_text(),
        [
            &LINE[..46],
            &LINE[46..],
            PROMPT.trim_end(),
            "",
            "",
            "",
            "",
            "",
        ]
    );
    assert_eq!(
        session.terminal().cursor(),
        bt_term::TerminalCursor {
            row: 2,
            column: PROMPT.len() as u32,
            visible: true,
        }
    );
}

#[test]
fn g1_full_width_reflow_stages_only_the_rows_that_cannot_fit() {
    const WARNING: &str = "Did not find path entry D:\\App\\Base\\anaconda3\\bin";
    const PROMPT: &str = "(base) PS D:\\Developer\\folio-terminal> ";

    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz32(80), nz32(4));
    session
        .feed_at(format!("{WARNING}\r\n{PROMPT}").as_bytes(), start)
        .unwrap();
    assert_eq!(
        session.terminal().visible_text(),
        [WARNING, PROMPT.trim_end(), "", ""]
    );

    session
        .resize_at(nz32(16), nz32(4), start + Duration::from_millis(10))
        .unwrap();
    assert_eq!(logical_content(&session), [WARNING, PROMPT.trim_end()]);
    assert!(session.transcript().staging_len() >= 3);

    let mut projection = session.new_projection(session.layout_key());
    let _ = session.viewport_frame(&mut projection).unwrap();
    projection.scroll_to_top();
    session.refresh_projection(&mut projection);
    let review = session.viewport_frame(&mut projection).unwrap();
    assert!(
        (0..review.drawable_rows())
            .map(|row| frame_row_text(&review, row))
            .collect::<String>()
            .contains("Did not find path entry")
    );

    session
        .resize_at(nz32(80), nz32(4), start + Duration::from_millis(20))
        .unwrap();
    assert_eq!(session.transcript().staging_len(), 0);
    assert_eq!(
        session.terminal().visible_text(),
        [WARNING, PROMPT.trim_end(), "", ""]
    );
}

/// Regression recording distilled from `anchor-glide-verify.vt`: PowerShell starts wide, recalls a
/// wrapping line at 49 columns, the pane narrows far enough for the sparse grid to grow downward,
/// and then widens to 118 columns before PSReadLine repaints the recalled input.  Every byte below
/// is emitted by the child in the recording; keyboard input and terminal replies are deliberately
/// absent from a PTY-output capture.
#[test]
fn recalled_input_keeps_its_prompt_head_and_never_welds_to_the_banner_after_widen() {
    const WARNING: &str = "Did not find path entry D:\\App\\Base\\anaconda3\\bin";
    const PROMPT: &str = "(base) PS D:\\Developer\\folio-terminal\\dist> ";
    const RECALLED: &str =
        "echo \"[Image: source: C:\\Windows\\Web\\Wallpaper\\Windows\\img0.jpg]\"";
    const REPAINTED: &str = "Write-Output ('BT_APP_' + 'INPUT_OK')";
    const CAPTURED_CORRUPT_REPAINT: &[u8] = b"\x1b[?25l\x1b[1;65H\x1b[0m\x1b[93mWrite-Output\x1b[0m\x1b[39;49m \x1b[0m\x1b[37m(\x1b[0m\x1b[36m'BT_APP_'\x1b[0m\x1b[39;49m \x1b[0m\x1b[90m+\x1b[0m\x1b[39;49m \x1b[0m\x1b[36m'INPUT_OK'\x1b[0m\x1b[37m)\x1b[39;49m                  \x1b[2;1H           \x1b[0m\x1b[1;102H\x1b[?25h";
    const REPAIRED_REPAINT: &[u8] = b"\x1b[?25l\x1b[2;45H\x1b[0m\x1b[93mWrite-Output\x1b[0m\x1b[39;49m \x1b[0m\x1b[37m(\x1b[0m\x1b[36m'BT_APP_'\x1b[0m\x1b[39;49m \x1b[0m\x1b[90m+\x1b[0m\x1b[39;49m \x1b[0m\x1b[36m'INPUT_OK'\x1b[0m\x1b[37m)\x1b[39;49m                            \x1b[0m\x1b[2;82H\x1b[?25h";

    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz32(120), nz32(39));
    session
        .feed_at(format!("{WARNING}\r\n{PROMPT}").as_bytes(), start)
        .unwrap();

    let mut at = start;
    for (columns, cursor) in [
        (29, b"\x1b[4;16H".as_slice()),
        (27, b"\x1b[4;18H"),
        (28, b"\x1b[4;17H"),
        (55, b""),
        (27, b"\x1b[4;18H"),
        (28, b""),
        (27, b"\x1b[4;18H"),
        (42, b"\x1b[4;3H"),
        (49, b"\x1b[2;45H"),
    ] {
        at += Duration::from_millis(10);
        session.resize_at(nz32(columns), nz32(39), at).unwrap();
        session.mark_pty_resize_requested_at(nz32(columns), nz32(39), at);
        if !cursor.is_empty() {
            session.feed_at(cursor, at).unwrap();
        }
    }
    session
        .feed_at(
            format!("\x1b[?25l\x1b[2;45H{RECALLED}\x1b[4;12H\x1b[?25h").as_bytes(),
            at,
        )
        .unwrap();

    for (columns, cursor) in [
        (27, b"\x1b[7;2H".as_slice()),
        (49, b""),
        (27, b"\x1b[7;2H"),
        (44, b""),
        (56, b"\x1b[3;54H"),
        (118, b""),
    ] {
        at += Duration::from_millis(10);
        session.resize_at(nz32(columns), nz32(39), at).unwrap();
        session.mark_pty_resize_requested_at(nz32(columns), nz32(39), at);
        if !cursor.is_empty() {
            session.feed_at(cursor, at).unwrap();
        }
    }
    let before_repaint = session.terminal().visible_text();
    let expected_input_line = format!("{PROMPT}{RECALLED}");
    assert_eq!(
        &before_repaint[..2],
        [WARNING, expected_input_line.as_str()],
        "the local grow/rejoin must restore both hard-line heads before the child repaints"
    );

    // Red check: the captured fallback bytes alone are sufficient to produce the field report.
    // They are kept closed over the test instead of read from the mutable diagnostic recording.
    let mut captured = DualPlaneSession::new(nz32(118), nz32(3));
    captured
        .feed_at(format!("{WARNING}\r\n{PROMPT}{RECALLED}").as_bytes(), start)
        .unwrap();
    captured.feed_at(CAPTURED_CORRUPT_REPAINT, start).unwrap();
    let captured_rows = captured.terminal().visible_text();
    assert!(captured_rows[0].contains("Write-Output"));
    assert!(captured_rows[1].starts_with("           "));
    assert!(captured_rows[1][11..].starts_with(&PROMPT[11..]));

    session.feed_at(REPAIRED_REPAINT, at).unwrap();

    let rows = session.terminal().visible_text();
    assert_eq!(rows[0], WARNING, "the banner must remain its own hard line");
    assert!(
        rows[1].starts_with(PROMPT),
        "the prompt head must survive the narrow/widen rejoin: {:?}",
        &rows[..3]
    );
    assert!(
        !rows[0].contains("Write-Output"),
        "the recalled input must never weld to the banner: {:?}",
        &rows[..3]
    );
    assert_eq!(rows[1], format!("{PROMPT}{REPAINTED}"));
}

/// The heap, counted per thread, underneath every test in this file.
///
/// `resize_drag_200_frames_stays_within_the_sparse_and_full_budget` used to spend its budget in
/// milliseconds, and could not. `cargo test -p bt-term` runs this file's 42 tests across 24 cores;
/// the sparse drag arm asks the allocator for ~630 KiB per frame; the heap and scheduler
/// contention that follows moved a summed 200-frame wall clock between 64 ms and 129 ms with not
/// one line of this crate changing, so the pin was red in 8 of 20 suite runs on an *idle* machine
/// and green in 20 of 20 when run alone. What did not move is how much those 200 frames ask for:
/// the same bytes in the same number of allocations in every run measured — alone, inside the
/// suite, and against a saturating 19-job `cargo build`. That is where the budget is spent now.
/// The distributions are in `docs/M1.8-resize-visual-stability.md`.
///
/// Thread-local on purpose: the other 41 tests of this binary are running on their own threads
/// while the drag runs, and must not land in its total.
///
/// **Two pins spend their budget here now.**
/// `g1_primary_tui_explicit_scroll_repaint_is_fast_and_never_becomes_transcript` was the same
/// story with the same ending, told a year apart: see its own comment for the numbers.
struct HeapCounter;

thread_local! {
    static HEAP_BYTES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static HEAP_ALLOCATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[expect(
    unsafe_code,
    reason = "a global allocator has no safe form. Every method here forwards to \
              `std::alloc::System` with its arguments unchanged and adds nothing but two \
              thread-local counter bumps, so the safety contract is exactly System's."
)]
unsafe impl std::alloc::GlobalAlloc for HeapCounter {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        charge(layout.size() as u64);
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        charge(layout.size() as u64);
        unsafe { std::alloc::System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        // A grow is charged for the growth only; a shrink returns memory and is charged nothing.
        // Forwarding to System's own `realloc` matters for more than speed: routing it through
        // `alloc` + copy + `dealloc` instead would change what every `Vec` in this binary does.
        charge(new_size.saturating_sub(layout.size()) as u64);
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
}

fn charge(bytes: u64) {
    HEAP_BYTES.with(|counter| counter.set(counter.get().wrapping_add(bytes)));
    HEAP_ALLOCATIONS.with(|counter| counter.set(counter.get().wrapping_add(1)));
}

#[global_allocator]
static HEAP_COUNTER: HeapCounter = HeapCounter;

/// The frames in one measured drag. A real divider drag delivers a width change per pointer event
/// and never waits for the child between two of them, which is what this reproduces.
const DRAG_FRAMES: usize = 200;

/// One screen being dragged: even frames narrow it to `narrow`, odd frames widen it back to
/// `wide`, and each frame's wall time and heap draw are kept.
struct DragArm {
    session: DualPlaneSession,
    rows: NonZeroU32,
    narrow: u32,
    wide: u32,
    frame_nanos: Vec<u64>,
    heap_bytes: u64,
    heap_allocations: u64,
}

impl DragArm {
    fn new(session: DualPlaneSession, rows: NonZeroU32, narrow: u32, wide: u32) -> Self {
        Self {
            session,
            rows,
            narrow,
            wide,
            // Reserved before the first frame: a `push` that grew mid-drag would charge the drag
            // for this test's own bookkeeping.
            frame_nanos: Vec::with_capacity(DRAG_FRAMES),
            heap_bytes: 0,
            heap_allocations: 0,
        }
    }

    fn frame(&mut self, frame: usize) {
        let columns = if frame.is_multiple_of(2) {
            self.narrow
        } else {
            self.wide
        };
        let bytes_before = HEAP_BYTES.with(std::cell::Cell::get);
        let allocations_before = HEAP_ALLOCATIONS.with(std::cell::Cell::get);
        let started = Instant::now();
        self.session.resize(nz32(columns), self.rows).unwrap();
        std::hint::black_box(self.session.transcript().staging_len());
        let elapsed = started.elapsed();
        self.heap_bytes += HEAP_BYTES.with(std::cell::Cell::get) - bytes_before;
        self.heap_allocations += HEAP_ALLOCATIONS.with(std::cell::Cell::get) - allocations_before;
        self.frame_nanos.push(elapsed.as_nanos() as u64);
    }

    /// The median of the 100 shrink frames.
    ///
    /// The median and not the sum, because a sum of 200 samples collects every preemption the OS
    /// hands out over the whole drag and reports it as if the code had done the work. The shrink
    /// half alone and not all 200, because the two halves are two different workloads — a sparse
    /// shrink reflows and reconciles, the widen back is nearly free at ~40 us — so a median over
    /// the mixture sits on the seam between the modes and wanders between them.
    fn median_shrink_nanos(&self) -> u64 {
        let mut shrinks = self
            .frame_nanos
            .iter()
            .step_by(2)
            .copied()
            .collect::<Vec<_>>();
        shrinks.sort_unstable();
        shrinks[shrinks.len() / 2]
    }

    fn report(&self, name: &str) {
        let mut sorted = self.frame_nanos.clone();
        sorted.sort_unstable();
        eprintln!(
            "BT_RESIZE_BENCH {name} frames={DRAG_FRAMES} sum_us={} shrink_p50_ns={} \
             frame_p50_ns={} frame_p90_ns={} frame_max_ns={} heap_bytes={} heap_allocations={}",
            self.frame_nanos.iter().sum::<u64>() / 1_000,
            self.median_shrink_nanos(),
            sorted[sorted.len() / 2],
            sorted[sorted.len() * 9 / 10],
            sorted[sorted.len() - 1],
            self.heap_bytes,
            self.heap_allocations,
        );
    }
}

/// How many times dearer a sparse shrink frame is than the full shrink frame that ran microseconds
/// after it, at the median of the 100 pairs.
///
/// Paired, and not two medians divided: the arms are interleaved frame by frame precisely so that
/// each of these 100 samples is two measurements taken within microseconds of each other, under
/// whatever the machine happened to be doing right then. Measured as two separate blocks seconds
/// apart the same ratio ranged 3.62-7.74, because the sparse arm draws 3.4x the heap of the full
/// one and therefore pays more for a burst of allocator contention that arrives during only one of
/// the two blocks. Paired it ranges 2.62-3.90 over the same three load conditions, while the
/// absolute median it is built from still moves by 1.9x.
fn median_paired_shrink_shape(sparse: &DragArm, full: &DragArm) -> f64 {
    let mut shapes = (0..DRAG_FRAMES)
        .step_by(2)
        .map(|frame| sparse.frame_nanos[frame] as f64 / full.frame_nanos[frame] as f64)
        .collect::<Vec<_>>();
    shapes.sort_by(f64::total_cmp);
    shapes[shapes.len() / 2]
}

/// A resize drag is 200 width changes with no PTY round trip in between, on the two screens whose
/// resize paths differ: `sparse` has a blank tail below the cursor and takes the grow-down branch
/// added in `6133230`, `full` is a screen with no blank tail and keeps the older history/staging
/// path. The pin exists because that commit put new work on every sparse drag frame — a blank-tail
/// scan and a second `reconcile_resize_transaction_to_viewport` — and because the same commit
/// removed per-frame work from the drag path that must not come back: a `Vec<Row<Cell>>` clone of
/// vendor history on every frame, a whole-screen `String` rebuilt to re-anchor semantic input
/// regions that were not there, and a staging retain over rows nothing had staged.
///
/// It is spent in three currencies, in order of how much they can be trusted.
///
/// **Heap volume** is exact. The 200 frames ask the allocator for the same bytes in the same
/// number of allocations on every run, under every load, so this arm is the one that actually
/// pins the algorithm: every regression named above is a regression in how much this path
/// allocates, and none of them can hide inside a 12% band.
///
/// **Shape** is each sparse shrink frame against the full shrink frame that ran microseconds
/// after it. A clock cancels out of a ratio, and pairing the samples cancels the load as well.
///
/// **Ceiling** is the only absolute number left, and it is not a budget — it is the line past
/// which no machine plausibly running this suite could still be called slow rather than broken.
/// It sits 3x above the worst median this machine produced while 19 `rustc` processes fought it.
///
/// The distributions this is derived from, and what each arm was proved to catch, are in
/// `docs/M1.8-resize-visual-stability.md`.
#[test]
fn resize_drag_200_frames_stays_within_the_sparse_and_full_budget() {
    // Measured 2026-08-19 over 90 runs in four load conditions, identical in every one of them:
    // sparse 128_942_382 B / 107_432 allocations, full 38_212_398 B / 34_129 allocations. Held to
    // a per-frame figure with ~12% of slack, which is room for the same algorithm to be written
    // differently and not room for a screenful of rows to be cloned once a frame.
    const SPARSE_FRAME_HEAP_BYTES: u64 = 720 * 1024;
    const SPARSE_FRAME_HEAP_ALLOCATIONS: u64 = 600;
    const FULL_FRAME_HEAP_BYTES: u64 = 216 * 1024;
    const FULL_FRAME_HEAP_ALLOCATIONS: u64 = 192;
    // See `median_paired_shrink_shape` for why this is a paired median rather than a quotient of
    // two medians. Observed 2.62-3.90 over 28 runs on an idle machine, inside the 42-test suite,
    // and against a saturating cold workspace build; 6 leaves half again as much room as the worst
    // of those and, against a typical 3.5, still reports a sparse shrink frame that has grown by
    // 70%. Measured sensitivity: a 1.9x slowdown on the sparse branch alone is red 3 of 3.
    const SHRINK_SHAPE: f64 = 6.0;
    // Worst medians seen under a saturating 19-job build: sparse 1006 us, full 211 us.
    const SPARSE_SHRINK_CEILING: Duration = Duration::from_millis(3);
    const FULL_SHRINK_CEILING: Duration = Duration::from_millis(1);

    let mut sparse = DualPlaneSession::new(nz32(104), nz32(26));
    sparse
        .feed(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvw\r\nBTP> ")
        .unwrap();
    let mut sparse = DragArm::new(sparse, nz32(26), 46, 104);

    let mut full = DualPlaneSession::new(nz32(80), nz32(24));
    let full_input = (0..24)
        .map(|row| format!("F{row:02}-{}", "X".repeat(75)))
        .collect::<Vec<_>>()
        .join("\r\n");
    full.feed(full_input.as_bytes()).unwrap();
    let mut full = DragArm::new(full, nz32(24), 20, 80);

    for frame in 0..DRAG_FRAMES {
        sparse.frame(frame);
        full.frame(frame);
    }

    sparse.report("sparse");
    full.report("full");

    let frames = DRAG_FRAMES as u64;
    assert!(
        sparse.heap_bytes <= SPARSE_FRAME_HEAP_BYTES * frames,
        "sparse drag asked for {} B per frame, budget {SPARSE_FRAME_HEAP_BYTES} B",
        sparse.heap_bytes / frames,
    );
    assert!(
        sparse.heap_allocations <= SPARSE_FRAME_HEAP_ALLOCATIONS * frames,
        "sparse drag made {} allocations per frame, budget {SPARSE_FRAME_HEAP_ALLOCATIONS}",
        sparse.heap_allocations / frames,
    );
    assert!(
        full.heap_bytes <= FULL_FRAME_HEAP_BYTES * frames,
        "full drag asked for {} B per frame, budget {FULL_FRAME_HEAP_BYTES} B",
        full.heap_bytes / frames,
    );
    assert!(
        full.heap_allocations <= FULL_FRAME_HEAP_ALLOCATIONS * frames,
        "full drag made {} allocations per frame, budget {FULL_FRAME_HEAP_ALLOCATIONS}",
        full.heap_allocations / frames,
    );

    let shape = median_paired_shrink_shape(&sparse, &full);
    eprintln!("BT_RESIZE_BENCH shape shrink_paired_p50={shape:.2}");
    assert!(
        shape <= SHRINK_SHAPE,
        "the sparse shrink frame is {shape:.2}x the full shrink frame beside it, budget \
         {SHRINK_SHAPE}x",
    );

    let sparse_shrink = Duration::from_nanos(sparse.median_shrink_nanos());
    let full_shrink = Duration::from_nanos(full.median_shrink_nanos());
    assert!(
        sparse_shrink <= SPARSE_SHRINK_CEILING,
        "the median sparse shrink frame is {sparse_shrink:?}, ceiling {SPARSE_SHRINK_CEILING:?}",
    );
    assert!(
        full_shrink <= FULL_SHRINK_CEILING,
        "the median full shrink frame is {full_shrink:?}, ceiling {FULL_SHRINK_CEILING:?}",
    );
}

#[test]
fn g1_resize_shrink_discards_blank_rows_below_the_cursor() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(4));
    session.feed(b"top\r\ncursor").unwrap();

    session.resize(nz32(8), nz32(2)).unwrap();

    assert!(session.document().entries().is_empty());
    assert_eq!(session.terminal().visible_text(), vec!["top", "cursor"]);
}

#[test]
fn g1_width_reflow_never_rewrites_frozen_source() {
    let mut session = DualPlaneSession::new(nz32(12), nz32(2));
    session.feed(b"abcdefgh\r\nnext\r\ntail").unwrap();
    let before = session.document().entries().clone();
    session.resize(nz32(4), nz32(2)).unwrap();
    session.resize(nz32(20), nz32(3)).unwrap();
    assert_eq!(session.document().entries(), &before);
}

#[test]
fn g1_no_output_resize_jitter_does_not_duplicate_captured_rows() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(4));
    session.feed(b"r1\r\nr2\r\nr3\r\nr4").unwrap();
    session.resize(nz32(7), nz32(2)).unwrap();
    session.resize(nz32(9), nz32(5)).unwrap();
    session.resize(nz32(6), nz32(2)).unwrap();
    session.resize(nz32(8), nz32(4)).unwrap();
    assert!(history_text(&session).is_empty());
    assert_eq!(session.transcript().staging_len(), 0);
    assert_eq!(
        session.terminal().visible_text(),
        vec!["r1", "r2", "r3", "r4"]
    );
}

#[test]
fn g1_no_output_shrink_grow_storm_harvests_no_history_and_keeps_bottom_following() {
    const PROMPT: &str = "(base) PS D:\\Developer\\folio-terminal>";
    let mut session = DualPlaneSession::new(nz32(64), nz32(8));
    session
        .feed(
            format!(
                "Did not find path entry D:\\App\\Base\\anaconda3\\bin\r\n\
                 alpha\r\nbeta\r\ngamma\r\ndelta\r\nepsilon\r\n{PROMPT}"
            )
            .as_bytes(),
        )
        .unwrap();
    assert!(history_text(&session).is_empty());
    let initial = session.terminal().visible_text();
    let mut projection = session.new_projection(session.layout_key());

    let storm = [(64, 8), (10, 2), (36, 3), (8, 7), (12, 2), (64, 8)];
    for _ in 0..6 {
        for (columns, rows) in storm {
            session.resize(nz32(columns), nz32(rows)).unwrap();
            session.refresh_projection(&mut projection);
            let frame = session.viewport_frame(&mut projection).unwrap();
            assert_eq!(
                frame.cells.len(),
                columns as usize * (rows as usize + bt_viewport::FRAME_OVERSCAN_ROWS as usize)
            );
            assert_eq!(projection.scroll_offset_rows(), 0);
            assert_eq!(frame.status_text, None);
            assert!(history_text(&session).is_empty());
        }
    }

    assert_eq!(session.transcript().staging_len(), 0);
    assert_eq!(session.terminal().visible_text(), initial);
    assert!(
        session
            .terminal()
            .visible_text()
            .iter()
            .all(|line| line.trim() != "Terminal>")
    );
    assert!(
        session
            .terminal()
            .visible_text()
            .iter()
            .any(|line| line == PROMPT)
    );
    assert!(
        session
            .terminal()
            .visible_text()
            .iter()
            .any(|line| line == "Did not find path entry D:\\App\\Base\\anaconda3\\bin")
    );
}

#[test]
fn m1_8_six_line_resize_can_never_manufacture_five_lines_below() {
    let start = Instant::now();
    let content = ["one", "two", "three", "four", "five", "Terminal>"];
    let mut session = DualPlaneSession::new(nz32(40), nz32(6));
    session
        .feed_at(content.join("\r\n").as_bytes(), start)
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let initial = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(initial.status_text, None);

    session
        .resize_at(nz32(18), nz32(2), start + Duration::from_millis(10))
        .unwrap();
    session.refresh_projection(&mut projection);
    let narrow = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(projection.scroll_offset_rows(), 0);
    assert_eq!(narrow.status_text, None);
    assert!(matches!(
        projection.scroll_state(),
        bt_viewport::ViewportScrollState::Bottom
    ));

    finish_resize_transaction(&mut session, start + Duration::from_millis(210));
    session.refresh_projection(&mut projection);
    let harvested = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(projection.scroll_offset_rows(), 0);
    assert_eq!(harvested.status_text, None);

    session.resize(nz32(40), nz32(6)).unwrap();
    session.refresh_projection(&mut projection);
    let wide = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(projection.scroll_offset_rows(), 0);
    assert_eq!(wide.status_text, None);
    assert!(matches!(
        projection.scroll_state(),
        bt_viewport::ViewportScrollState::Bottom
    ));
}

fn replay_r2_extreme_shrink_grow_and_recall(start: Instant) -> Vec<bt_term::ResizeTraceEvent> {
    const WARNING: &str = "Did not find path entry D:\\App\\Base\\anaconda3\\bin";
    const PROMPT: &str = "(base) PS D:\\Developer\\folio-terminal> ";
    const RECALL: &str = "Write-Output ('BT_APP_' + 'INPUT_OK')";
    let mut session = DualPlaneSession::new(nz32(104), nz32(26));
    session
        .feed_at(format!("{WARNING}\r\n{PROMPT}").as_bytes(), start)
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());

    let trace_sizes = [
        (95, 24),
        (62, 16),
        (48, 12),
        (38, 10),
        (28, 8),
        (25, 8),
        (23, 7),
        (20, 7),
        (18, 6),
        (16, 6),
        (16, 5),
        (16, 6),
        (19, 7),
        (23, 8),
        (26, 8),
        (29, 9),
        (36, 11),
        (41, 12),
        (46, 14),
        (49, 14),
        (52, 15),
        (56, 16),
        (61, 17),
        (66, 17),
        (71, 18),
        (77, 20),
        (85, 21),
        (78, 19),
        (66, 17),
        (60, 16),
        (52, 14),
        (25, 9),
        (13, 6),
        (12, 6),
        (15, 7),
        (33, 10),
        (51, 13),
        (60, 15),
        (61, 15),
        (47, 12),
        (23, 7),
        (11, 3),
        (11, 4),
        (11, 6),
        (22, 8),
        (30, 9),
        (34, 10),
        (37, 10),
        (38, 10),
        (38, 11),
        (38, 10),
        (26, 8),
        (12, 6),
        (11, 5),
        (13, 6),
        (35, 10),
        (46, 12),
        (49, 12),
        (43, 10),
        (39, 10),
        (38, 10),
        (37, 10),
        (35, 9),
        (34, 9),
        (31, 9),
        (30, 9),
    ];
    for (index, (columns, rows)) in trace_sizes.into_iter().enumerate() {
        session
            .resize_at(
                nz32(columns),
                nz32(rows),
                start + Duration::from_millis(index as u64 + 10),
            )
            .unwrap();
    }
    assert_eq!(
        session.terminal().resize_transaction_history_size(),
        0,
        "the final 30x9 viewport can hold both logical lines, so the local branch already grows down"
    );

    session.mark_pty_resize_requested_at(nz32(30), nz32(9), start + Duration::from_millis(220));
    assert_eq!(session.terminal().resize_transaction_history_size(), 0);
    assert_eq!(logical_content(&session), [WARNING, PROMPT.trim_end()]);
    assert!(
        session
            .finish_resize_if_quiescent(start + Duration::from_millis(420))
            .unwrap()
    );
    assert!(session.document().entries().is_empty());
    assert_eq!(session.transcript().staging_len(), 0);

    session.refresh_projection(&mut projection);
    let settled = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(settled.status_text, None);
    assert_eq!(settled.scroll_offset_rows, 0);
    session.record_published_frame(&settled, start + Duration::from_millis(420));

    // A CSI A keyboard round-trip returns the viewport to Bottom before PowerShell emits the
    // recalled command. Model those two observable actor-side effects deterministically.
    projection.scroll_to_bottom();
    session
        .feed_at(RECALL.as_bytes(), start + Duration::from_millis(430))
        .unwrap();
    session.refresh_projection(&mut projection);
    let recalled = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(recalled.status_text, None);
    assert_eq!(recalled.scroll_offset_rows, 0);
    assert!(recalled.cursor.visible);
    assert_eq!(
        logical_content(&session),
        [WARNING.to_owned(), format!("{PROMPT}{RECALL}")]
    );
    session.record_published_frame(&recalled, start + Duration::from_millis(430));

    session.resize_trace().to_vec()
}

#[test]
fn m1_8_r2_extreme_shrink_grow_and_csi_a_replay_has_no_frozen_live_seam() {
    let start = Instant::now();
    let first = replay_r2_extreme_shrink_grow_and_recall(start);
    let second = replay_r2_extreme_shrink_grow_and_recall(start + Duration::from_secs(10));
    assert_eq!(first, second);
    assert!(first.iter().any(|event| matches!(
        event.kind,
        bt_term::ResizeTraceKind::VendorReconcile {
            history_before: 0,
            history_after: 0,
            cursor_row: 3,
            cursor_column: 9,
            cursor_visible: true,
        }
    )));
    assert!(first.iter().any(|event| matches!(
        &event.kind,
        bt_term::ResizeTraceKind::Harvest { widths, .. } if widths.is_empty()
    )));
    assert!(first.iter().all(|event| match &event.kind {
        bt_term::ResizeTraceKind::FramePublished {
            scroll_offset_rows,
            anchored,
            ..
        } => *scroll_offset_rows == 0 && !anchored,
        _ => true,
    }));
}

#[test]
fn g3_vendor_wrapline_rejoins_rows_inside_one_harvest_batch() {
    let start = Instant::now();
    let line_a = "A-0123456789AB";
    let line_b = "B-complete";
    let mut session = DualPlaneSession::new(nz32(20), nz32(2));
    session
        .feed_at(format!("{line_a}\r\n{line_b}").as_bytes(), start)
        .unwrap();
    assert_eq!(logical_content(&session), [line_a, line_b]);

    session
        .resize_at(nz32(10), nz32(2), start + Duration::from_millis(12))
        .unwrap();
    assert_eq!(session.terminal().resize_transaction_history_size(), 1);

    // Model a stale ConPTY repaint: row 1 is rewritten before a bottom-edge linefeed scrolls it
    // out, then the final cursor-addressed repaint restores the expected live screen.
    let line_a_tail = &line_a[10..];
    session.mark_pty_resize_requested_at(nz32(10), nz32(2), start + Duration::from_millis(212));
    session
        .feed_at(
            format!(
                "\x1b[1;1H\x1b[2K{line_b}\x1b[2;1H\r\n\
                 \x1b[1;1H\x1b[2K{line_a_tail}\x1b[2;1H\x1b[2K{line_b}"
            )
            .as_bytes(),
            start + Duration::from_millis(226),
        )
        .unwrap();
    assert!(
        session
            .finish_resize_if_quiescent(start + Duration::from_millis(426))
            .unwrap()
    );

    let actual = logical_content(&session);
    // M1.8 makes a narrower claim than the old per-row split policy: within this single vendor
    // harvest, WRAPLINE is the authoritative causal continuation. The batch boundary below remains
    // the no-weld boundary for later transactions.
    assert_eq!(actual, ["A-01234567B-complete", "89AB", line_b]);
    assert!(
        actual
            .iter()
            .all(|line| !(line.contains("89AB") && line.contains(line_b)))
    );
}

#[test]
fn g3_active_narrow_harvest_widen_returns_every_wrapline_to_vendor() {
    let start = Instant::now();
    let logical = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcd";
    let mut session = DualPlaneSession::new(nz32(42), nz32(1));
    session.feed_at(logical.as_bytes(), start).unwrap();

    session
        .resize_at(nz32(14), nz32(1), start + Duration::from_millis(10))
        .unwrap();
    assert_eq!(session.terminal().resize_transaction_history_size(), 2);
    finish_resize_transaction(&mut session, start + Duration::from_millis(210));

    assert!(session.document().entries().is_empty());
    assert_eq!(staged_text(&session), [&logical[..14], &logical[14..28]]);
    assert_eq!(session.terminal().resize_staging_candidate_rows(), 2);
    assert!(session.resize_trace().iter().any(|event| matches!(
        &event.kind,
        bt_term::ResizeTraceKind::Harvest {
            widths,
            continues,
            ..
        } if widths == &[14, 14] && continues == &[true, true]
    )));

    session.resize(nz32(42), nz32(2)).unwrap();
    assert!(session.document().entries().is_empty());
    assert_eq!(session.transcript().staging_len(), 0);
    assert_eq!(session.terminal().visible_text(), [logical, ""]);
    assert_eq!(
        session.terminal().cursor(),
        bt_term::TerminalCursor {
            row: 0,
            column: logical.len() as u32,
            visible: true,
        }
    );
    assert!(session.resize_trace().iter().any(|event| matches!(
        event.kind,
        bt_term::ResizeTraceKind::VendorRestore { rows: 2 }
    )));

    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    let first_row = frame.cells[..42]
        .iter()
        .filter(|cell| !cell.wide_spacer)
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    assert!(first_row.starts_with(logical));
    assert!(frame.cursor.visible);
}

#[test]
fn s9_separate_resize_transactions_return_the_active_prompt_to_vendor_reflow() {
    let prompt = "(base) PS D:\\Developer\\folio-terminal> ";
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz32(40), nz32(4));
    session.feed_at(prompt.as_bytes(), start).unwrap();
    let mut projection = session.new_projection(session.layout_key());

    session
        .resize_at(nz32(10), nz32(3), start + Duration::from_millis(10))
        .unwrap();
    finish_resize_transaction(&mut session, start + Duration::from_millis(210));

    assert!(session.document().entries().is_empty());
    assert_eq!(session.transcript().staging_len(), 1);
    assert_eq!(session.terminal().resize_staging_candidate_rows(), 1);
    session.refresh_projection(&mut projection);
    let tiny_settled = session.viewport_frame(&mut projection).unwrap();
    tiny_settled.validate_shape().unwrap();
    assert!(tiny_settled.cursor.visible);
    session.record_published_frame(&tiny_settled, start + Duration::from_millis(410));

    // The pause above closed the first transaction. Growing is deliberately a second gesture.
    session
        .resize_at(nz32(40), nz32(4), start + Duration::from_millis(1_000))
        .unwrap();
    assert_eq!(session.transcript().staging_len(), 0);
    assert_eq!(session.terminal().resize_transaction_history_size(), 0);
    assert_eq!(session.terminal().visible_text()[0], prompt.trim_end());

    session.refresh_projection(&mut projection);
    let grown = session.viewport_frame(&mut projection).unwrap();
    grown.validate_shape().unwrap();
    assert_eq!(
        (grown.cursor.row, grown.cursor.column),
        (0, prompt.len() as u32)
    );
    assert!(grown.cursor.visible);
    session.record_published_frame(&grown, start + Duration::from_millis(1_000));

    finish_resize_transaction(&mut session, start + Duration::from_millis(1_200));
    session.feed(b"echo").unwrap();
    let echoed = format!("{prompt}echo");
    assert_eq!(
        logical_content(&session).last().map(String::as_str),
        Some(echoed.as_str())
    );
    assert!(session.resize_trace().iter().any(|event| matches!(
        event.kind,
        bt_term::ResizeTraceKind::VendorRestore { rows: 1 }
    )));
    assert!(session.resize_trace().iter().all(|event| match event.kind {
        bt_term::ResizeTraceKind::VendorReconcile { cursor_visible, .. }
        | bt_term::ResizeTraceKind::FramePublished { cursor_visible, .. } => cursor_visible,
        _ => true,
    }));
}

#[test]
fn narrowing_an_idle_wrapped_prompt_keeps_the_cursor_after_its_trailing_space() {
    const STARTUP: &[u8] = b"Did not find path entry D:\\App\\Base\\anaconda3\\bin\r\n\
\x1b[0m\x1b[0m(base) \x1b[0m\x1b[0m\
\x1b]7;file:///D:/Developer/folio-terminal/dist\x07\
\x1b]133;A\x07PS D:\\Developer\\folio-terminal\\dist> \x1b]133;B\x07";

    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz32(104), nz32(39));
    session.feed_at(STARTUP, start).unwrap();
    let _ = session.take_pty_writes();

    let resized_at = start + Duration::from_millis(10);
    session.resize_at(nz32(36), nz32(39), resized_at).unwrap();
    session.mark_pty_resize_requested_at(nz32(36), nz32(39), resized_at);
    session
        .feed_at(b"\x1b[6n", resized_at + Duration::from_millis(200))
        .unwrap();

    assert_eq!(
        session.take_pty_writes(),
        vec![b"\x1b[4;9R".to_vec()],
        "the CPR must name the insertion cell after `l\\dist> `"
    );
    assert_eq!(
        session.terminal().cursor(),
        bt_term::TerminalCursor {
            row: 3,
            column: 8,
            visible: true,
        }
    );
}

#[test]
fn repeated_idle_prompt_cpr_resize_dance_keeps_the_cursor_after_the_prompt() {
    const STARTUP: &[u8] = b"Did not find path entry D:\\App\\Base\\anaconda3\\bin\r\n\
\x1b[0m\x1b[0m(base) \x1b[0m\x1b[0m\
\x1b]7;file:///D:/Developer/folio-terminal/dist\x07\
\x1b]133;A\x07PS D:\\Developer\\folio-terminal\\dist> \x1b]133;B\x07";
    const RESIZE_BURSTS: &[&[u32]] = &[
        &[27],
        &[41, 44],
        &[39],
        &[40],
        &[41, 42],
        &[48],
        &[52],
        &[51],
        &[50],
        &[48],
        &[49],
        &[33, 48],
        &[49],
        &[48],
        &[47, 46],
        &[44, 41],
        &[44],
        &[43],
        &[42],
        &[43],
        &[42],
        &[43],
        &[42],
        &[78],
        &[36],
        &[109, 38],
        &[40],
        &[36],
    ];

    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz32(104), nz32(39));
    session.feed_at(STARTUP, start).unwrap();
    let _ = session.take_pty_writes();

    for (index, burst) in RESIZE_BURSTS.iter().enumerate() {
        let burst_at = start + Duration::from_secs(index as u64 + 1);
        for (offset, columns) in burst.iter().enumerate() {
            let at = burst_at + Duration::from_millis(offset as u64 * 10);
            session.resize_at(nz32(*columns), nz32(39), at).unwrap();
            session.mark_pty_resize_requested_at(nz32(*columns), nz32(39), at);
        }

        let columns = *burst.last().unwrap();
        let warning_rows = 49_u32.div_ceil(columns);
        let (expected_row, expected_column) = if 44 % columns == 0 {
            (warning_rows + 44 / columns - 1, columns - 1)
        } else {
            (warning_rows + 44 / columns, 44 % columns)
        };
        assert_eq!(
            session.terminal().cursor(),
            bt_term::TerminalCursor {
                row: expected_row,
                column: expected_column,
                visible: true,
            },
            "cursor first drifted after resize burst {index} ending at {columns} columns"
        );

        let cpr_at = burst_at + Duration::from_millis(200);
        session.feed_at(b"\x1b[6n", cpr_at).unwrap();
        let mut replies = session.take_pty_writes();
        assert_eq!(replies.len(), 1);
        let mut cup = replies.pop().unwrap();
        assert_eq!(cup.pop(), Some(b'R'));
        cup.push(b'H');
        session.feed_at(&cup, cpr_at).unwrap();
        assert!(
            session
                .finish_resize_if_quiescent(cpr_at + Duration::from_millis(200))
                .unwrap()
        );
    }

    assert_eq!(
        session.terminal().cursor(),
        bt_term::TerminalCursor {
            row: 3,
            column: 8,
            visible: true,
        },
        "at 36 columns the insertion cell follows the eight-cell `l\\dist> ` tail"
    );
}

#[test]
fn g1_modal_pixel_resize_timing_preserves_content_and_rectangular_scroll_frames() {
    let expected = [
        "resize-line-00",
        "resize-line-01",
        "resize-line-02",
        "resize-line-03",
        "resize-line-04",
        "resize-line-05",
        "(base) PS BT>",
    ]
    .map(str::to_owned);
    let start = Instant::now();
    let mut now = start;
    let mut session = DualPlaneSession::new(nz32(54), nz32(7));
    session
        .feed_at(expected.join("\r\n").as_bytes(), now)
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());

    for step in 0..180_u32 {
        now += Duration::from_millis(u64::from(12 + step % 15));
        let phase = step % 72;
        let columns = if phase < 36 {
            54 - phase
        } else {
            18 + phase - 36
        };
        let row_phase = step % 8;
        let rows = if row_phase < 4 {
            7 - row_phase
        } else {
            3 + row_phase - 4
        };
        session.resize_at(nz32(columns), nz32(rows), now).unwrap();

        session.refresh_projection(&mut projection);
        let frame = session.viewport_frame(&mut projection).unwrap();
        frame.validate_shape().unwrap();
        session.record_published_frame(&frame, now);
        assert_eq!(projection.scroll_offset_rows(), 0);
        assert_eq!(frame.status_text, None);
    }

    now += Duration::from_millis(10);
    session.resize_at(nz32(54), nz32(7), now).unwrap();
    let request_at = now + Duration::from_millis(200);
    session.mark_pty_resize_requested_at(nz32(54), nz32(7), request_at);
    let mut final_repaint = format!("\x1b[{};1H\r\n", expected.len()).into_bytes();
    final_repaint.extend_from_slice(&repaint_rows(&expected));
    session
        .feed_at(&final_repaint, request_at + Duration::from_millis(10))
        .unwrap();
    assert!(
        session
            .finish_resize_if_quiescent(request_at + Duration::from_millis(210))
            .unwrap()
    );

    let actual = logical_content(&session);
    assert!(actual.ends_with(&expected));
    assert!(actual.len() <= expected.len() + 1);
    assert!(actual.iter().all(|line| expected.contains(line)));
    session.refresh_projection(&mut projection);
    let bottom = session.viewport_frame(&mut projection).unwrap();
    bottom.validate_shape().unwrap();
    session.record_published_frame(&bottom, request_at + Duration::from_millis(211));
    projection.scroll_by_rows(3);
    session.refresh_projection(&mut projection);
    let scrolled = session.viewport_frame(&mut projection).unwrap();
    scrolled.validate_shape().unwrap();
    session.record_published_frame(&scrolled, request_at + Duration::from_millis(220));
    let _ = scrolled.word_selection(0, 0).unwrap();
    assert_eq!(projection.scroll_offset_rows(), 1);
    assert!(
        session
            .resize_trace()
            .iter()
            .any(|event| matches!(event.kind, bt_term::ResizeTraceKind::PtyChunkArrived { .. }))
    );
    assert!(
        session
            .resize_trace()
            .iter()
            .any(|event| matches!(event.kind, bt_term::ResizeTraceKind::Harvest { .. }))
    );
}

fn replay_resize_trace(start: Instant) -> Vec<bt_term::ResizeTraceEvent> {
    let mut session = DualPlaneSession::new(nz32(8), nz32(3));
    session.feed_at(b"a\r\nb\r\nc", start).unwrap();
    session
        .resize_at(nz32(4), nz32(2), start + Duration::from_millis(10))
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let live = session.viewport_frame(&mut projection).unwrap();
    live.validate_shape().unwrap();
    session.record_published_frame(&live, start + Duration::from_millis(10));

    session.mark_pty_resize_requested_at(nz32(4), nz32(2), start + Duration::from_millis(210));
    session
        .feed_at(b"\r\nx\x1b[A", start + Duration::from_millis(220))
        .unwrap();
    assert!(
        session
            .finish_resize_if_quiescent(start + Duration::from_millis(420))
            .unwrap()
    );

    session.refresh_projection(&mut projection);
    let settled = session.viewport_frame(&mut projection).unwrap();
    settled.validate_shape().unwrap();
    session.record_published_frame(&settled, start + Duration::from_millis(420));
    projection.scroll_by_rows(1);
    session.refresh_projection(&mut projection);
    let scrolled = session.viewport_frame(&mut projection).unwrap();
    scrolled.validate_shape().unwrap();
    assert_eq!(scrolled.scroll_offset_rows, 1);
    assert_eq!(scrolled.cursor.row, 1);
    assert!(scrolled.cursor.visible);
    session.record_published_frame(&scrolled, start + Duration::from_millis(421));
    session.resize_trace().to_vec()
}

#[test]
fn g1_resize_trace_replay_is_deterministic_through_post_drag_wheel_frame() {
    let start = Instant::now();
    let first = replay_resize_trace(start);
    let second = replay_resize_trace(start + Duration::from_secs(10));

    assert_eq!(first, second);
    assert!(
        first
            .iter()
            .enumerate()
            .all(|(index, event)| { event.ordinal == index as u64 })
    );
    assert!(
        first
            .iter()
            .any(|event| matches!(event.kind, bt_term::ResizeTraceKind::AdapterRows { .. }))
    );
    assert!(matches!(
        first.last().map(|event| &event.kind),
        Some(bt_term::ResizeTraceKind::FramePublished {
            grid_rows: 2,
            rows: 3,
            cells: 12,
            anchors: 12,
            layout_columns: 4,
            scroll_offset_rows: 1,
            anchored: true,
            cursor_row: 1,
            cursor_visible: true,
            ..
        })
    ));
    assert!(first.iter().all(|event| match &event.kind {
        bt_term::ResizeTraceKind::FramePublished {
            columns,
            layout_columns,
            ..
        } => columns == layout_columns,
        _ => true,
    }));
}

#[test]
fn m1_8_prompt_echo_cursor_and_composition_share_one_post_harvest_width() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz32(40), nz32(4));
    session
        .feed_at(b"one\r\ntwo\r\nthree\r\nfour", start)
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let _ = session.viewport_frame(&mut projection).unwrap();

    session
        .resize_at(nz32(20), nz32(2), start + Duration::from_millis(10))
        .unwrap();
    finish_resize_transaction(&mut session, start + Duration::from_millis(210));
    assert!(!session.document().entries().is_empty());

    session.resize(nz32(40), nz32(4)).unwrap();
    let echo = "Terminal> cargo test";
    session
        .feed(format!("\x1b[1;1H\x1b[2K{echo}").as_bytes())
        .unwrap();
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    frame.validate_shape().unwrap();

    let first_row = frame.cells[..40]
        .iter()
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    assert!(first_row.starts_with(echo));
    assert_eq!(frame.cursor.row, 0);
    assert_eq!(frame.cursor.column, echo.len() as u32);
    let cursor_index = frame.cursor.column as usize;
    assert!(matches!(
        &frame.cell_anchors[cursor_index].start,
        bt_doc::ContentAnchor::Live {
            point: bt_doc::GridPoint { row: 0, column },
            ..
        } if *column == echo.len() as u32
    ));
    assert_eq!(frame.layout_key.width_cells, frame.columns);
    assert_eq!(frame.scroll_offset_rows, 0);
    assert!(matches!(
        frame.viewport_origin,
        bt_viewport::FrameViewportOrigin::Bottom
    ));
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 384,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn g1_arbitrary_resize_sequences_preserve_the_logical_content_set(
        sizes in prop::collection::vec((6_u32..72, 2_u32..12), 8..80),
    ) {
        let expected = vec![
            "Did not find path entry D:\\App\\Base\\anaconda3\\bin".to_owned(),
            "same-prefix-but-a-hard-line".to_owned(),
            "same-prefix-but-a-second-hard-line".to_owned(),
            "spaces inside this logical line stay intact".to_owned(),
            "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ".to_owned(),
            "(base) PS D:\\Developer\\folio-terminal>".to_owned(),
        ];
        let mut session = DualPlaneSession::new(nz32(72), nz32(12));
        session.feed(expected.join("\r\n").as_bytes()).unwrap();
        prop_assert_eq!(logical_content(&session), expected.clone());

        for (columns, rows) in sizes {
            session.resize(nz32(columns), nz32(rows)).unwrap();
            prop_assert!(history_text(&session).is_empty());
        }

        session.resize(nz32(72), nz32(12)).unwrap();
        finish_resize_transaction(&mut session, Instant::now());
        prop_assert_eq!(logical_content(&session), expected.clone());
        prop_assert!(history_text(&session).is_empty());
        prop_assert_eq!(session.transcript().staging_len(), 0);
    }
}

#[test]
fn g1_human_paced_resize_redraw_cycles_allow_only_clean_bounded_growth() {
    // The coordinator's pixel recipe mapped through a 10x20 test cell: (1400,900), (260,180),
    // (1100,300), (240,650), (1400,900).
    let mut expected = (0..45)
        .map(|row| format!("row-{row:02}"))
        .collect::<Vec<_>>();
    expected[0] = "Did not find path".into();
    expected[44] = "BetterTerminal>".into();
    let start = Instant::now();
    let mut now = start;
    let mut session = DualPlaneSession::new(nz32(140), nz32(45));
    session
        .feed_at(expected.join("\r\n").as_bytes(), now)
        .unwrap();

    let recipe = [
        (140, 45, 30),
        (26, 9, 90),
        (110, 15, 45),
        (24, 32, 80),
        (140, 45, 130),
    ];
    for gesture in 0..4 {
        for (columns, rows, pause_ms) in recipe {
            now += Duration::from_millis(pause_ms);
            session.resize_at(nz32(columns), nz32(rows), now).unwrap();
        }

        let request_at = now + Duration::from_millis(200);
        session.mark_pty_resize_requested_at(nz32(140), nz32(45), request_at);
        let mut redraw = format!("\x1b[{};1H\r\n", expected.len()).into_bytes();
        redraw.extend_from_slice(&repaint_rows(&expected));
        session
            .feed_at(&redraw, request_at + Duration::from_millis(20))
            .unwrap();
        now = request_at + Duration::from_millis(220);
        assert!(session.finish_resize_if_quiescent(now).unwrap());

        let actual = logical_content(&session);
        assert!(actual.ends_with(&expected));
        assert!(
            actual.len() <= expected.len() + gesture + 1,
            "unexpected resize growth: {actual:?}"
        );
        assert!(actual.iter().all(|line| expected.contains(line)));
        assert_eq!(session.terminal().visible_text(), expected);
    }
}

#[test]
fn g1_resize_silence_delays_but_never_drops_continuous_true_output() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz32(8), nz32(3));
    session.feed_at(b"a\r\nb\r\nc", start).unwrap();
    session
        .resize_at(nz32(8), nz32(2), start + Duration::from_millis(10))
        .unwrap();

    for (index, text) in ["n1", "n2", "n3", "n4"].into_iter().enumerate() {
        let at = start + Duration::from_millis(100 + index as u64 * 100);
        session
            .feed_at(format!("\r\n{text}").as_bytes(), at)
            .unwrap();
        if index == 1 {
            session.mark_pty_resize_requested_at(
                nz32(8),
                nz32(2),
                start + Duration::from_millis(210),
            );
        }
        assert!(!session.finish_resize_if_quiescent(at).unwrap());
    }

    assert!(
        session
            .finish_resize_if_quiescent(start + Duration::from_millis(600))
            .unwrap()
    );
    assert_eq!(history_text(&session), vec!["a", "b", "c", "n1", "n2"]);
    assert_eq!(session.terminal().visible_text(), vec!["n3", "n4"]);
}

#[test]
fn g1_transaction_begin_wrap_splits_preexisting_normal_staging() {
    let mut session = DualPlaneSession::new(nz32(4), nz32(2));
    session.feed(b"abcde\r\n").unwrap();
    assert_eq!(session.transcript().staging_len(), 1);
    session.resize(nz32(5), nz32(2)).unwrap();
    let entry = session.document().entries().first_key_value().unwrap().1;
    assert_eq!(entry.line.text, "abcd");
    assert!(entry.line.wrap_split);
}

#[test]
fn g1_staging_quota_forces_a_split_instead_of_growing_without_bound() {
    let mut session = DualPlaneSession::with_quotas(nz32(4), nz32(2), nz_size(1), nz_size(32));
    session.feed(b"abcdefghijklmnop").unwrap();
    assert!(session.transcript().staging_len() <= 1);
    assert!(
        session
            .document()
            .entries()
            .values()
            .any(|entry| entry.line.wrap_split)
    );
}

#[test]
fn g1_harvested_resize_rows_never_move_back_from_frozen_history() {
    let start = Instant::now();
    let mut session = DualPlaneSession::with_quotas(nz32(8), nz32(4), nz_size(1), nz_size(32));
    session.feed(b"r1\r\nr2\r\nr3\r\nr4").unwrap();
    session.resize_at(nz32(8), nz32(2), start).unwrap();
    finish_resize_transaction(&mut session, start + Duration::from_millis(200));
    assert_eq!(history_text(&session), vec!["r1", "r2"]);

    session.resize(nz32(8), nz32(4)).unwrap();
    assert_eq!(history_text(&session), vec!["r1", "r2"]);
    assert_eq!(session.transcript().staging_len(), 0);
    assert_eq!(session.terminal().visible_text(), vec!["r3", "r4", "", ""]);
}

#[test]
fn g1_ed3_deletes_history_and_records_tombstones() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(2));
    session.feed(b"one\r\ntwo\r\ntail").unwrap();
    let removed = session
        .document()
        .entries()
        .keys()
        .copied()
        .collect::<Vec<_>>();
    assert!(!removed.is_empty());
    session.feed(b"\x1b[3J").unwrap();
    assert!(session.document().entries().is_empty());
    assert_eq!(session.document().tombstones(), removed);
}

#[test]
fn g1_frozen_quota_evicts_through_the_document_pipeline() {
    let mut session = DualPlaneSession::with_frozen_quota(nz32(8), nz32(2), nz_size(2));
    session
        .feed(b"one\r\ntwo\r\nthree\r\nfour\r\ntail\r\nend")
        .unwrap();
    assert_eq!(session.document().entries().len(), 2);
    assert_eq!(session.document().tombstones().len(), 2);
    assert_eq!(history_text(&session), vec!["three", "four"]);
}

#[test]
fn g1_alternate_screen_never_enters_primary_history() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(3));
    session.feed(b"keep\r\na\r\nb").unwrap();
    let before = session.document().entries().clone();
    session
        .feed(b"\x1b[?1049h1\r\n2\r\n3\r\n4\r\n5\x1b[?1049l")
        .unwrap();
    assert_eq!(session.document().entries(), &before);
}

#[test]
fn g1_vendor_resize_tail_reflows_with_the_primary_while_it_is_parked() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(4));
    session.feed(b"r1\r\nr2\r\nr3\r\nr4").unwrap();
    session.resize(nz32(8), nz32(2)).unwrap();
    assert_eq!(session.transcript().staging_len(), 2);
    assert_eq!(session.terminal().resize_transaction_history_size(), 2);

    session.feed(b"\x1b[?1049h").unwrap();
    session.resize(nz32(4), nz32(3)).unwrap();
    session.feed(b"\x1b[?1049l").unwrap();
    session.resize(nz32(8), nz32(4)).unwrap();
    finish_resize_transaction(&mut session, Instant::now());

    assert!(history_text(&session).is_empty());
    assert_eq!(session.transcript().staging_len(), 0);
    assert_eq!(
        session.terminal().visible_text(),
        vec!["r1", "r2", "r3", "r4"]
    );
}

#[test]
fn g1_alternate_screen_parks_detection_and_restores_fresh_work() {
    let mut session = DualPlaneSession::new(nz32(16), nz32(2));
    session.feed(b"$$x$$\r\nnext\r\ntail").unwrap();
    let id = *session.document().entries().first_key_value().unwrap().0;
    let in_flight = session.take_worker_task().unwrap();

    session.feed(b"\x1b[?1049hpaint").unwrap();
    assert!(!session.complete_worker_task(in_flight));
    assert_eq!(session.pending_tasks(), 0);

    session.feed(b"\x1b[?1049l").unwrap();
    assert_eq!(session.pending_tasks(), 1);
    session.run_workers();
    assert_eq!(
        session.decoration(id).unwrap().decoration,
        DecorationLifecycle::Ready
    );
}

#[test]
fn g1_local_scroll_region_never_enters_history() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(4));
    session.feed(b"keep\r\na\r\nb\r\nc").unwrap();
    let before = session.document().entries().clone();
    session.feed(b"\x1b[2;3r\x1b[3;1Hlocal\nlocal\n").unwrap();
    assert_eq!(session.document().entries(), &before);
}

/// The cycles in one measured burst of explicit TUI repaint.
const EXPLICIT_SCROLL_REPAINT_CYCLES: usize = 512;

/// One arm of the repaint pin: `EXPLICIT_SCROLL_REPAINT_CYCLES` explicit scroll/repaints on a
/// screen of the given shape. Returns what one cycle drew from the heap, in allocations and in
/// bytes, and proves along the way that none of it reached the transcript.
///
/// The escape sequences are built before the meter starts. A `format!` per cycle is this test's
/// own bookkeeping, and charging the session for it would be measuring the fixture.
fn explicit_scroll_repaint_arm(columns: u32, rows: u32) -> (u64, u64) {
    let mut session = DualPlaneSession::new(nz32(columns), nz32(rows));
    let initial = (0..rows)
        .map(|row| format!("frame-row-{row:02}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    session.feed(initial.as_bytes()).unwrap();
    assert!(history_text(&session).is_empty());

    let repaints = (0..EXPLICIT_SCROLL_REPAINT_CYCLES)
        // CSI S is an explicit full-screen manipulation used to collapse/repaint an upward TUI.
        // It is not a linefeed carrying new process output into canonical history.
        .map(|cycle| format!("\x1b[S\x1b[{rows};1H\x1b[2Kframe-{cycle:03}").into_bytes())
        .collect::<Vec<_>>();

    let bytes_before = HEAP_BYTES.with(std::cell::Cell::get);
    let allocations_before = HEAP_ALLOCATIONS.with(std::cell::Cell::get);
    let started = Instant::now();
    for repaint in &repaints {
        session.feed(repaint).unwrap();
    }
    let elapsed = started.elapsed();
    let cycles = EXPLICIT_SCROLL_REPAINT_CYCLES as u64;
    let heap_bytes = (HEAP_BYTES.with(std::cell::Cell::get) - bytes_before) / cycles;
    let heap_allocations =
        (HEAP_ALLOCATIONS.with(std::cell::Cell::get) - allocations_before) / cycles;
    // The wall clock is still printed, because it is what a human reads when they want to know
    // whether this got slower. It is no longer what the test concludes from.
    eprintln!(
        "G1_TUI_REPAINT {columns}x{rows} cycles={EXPLICIT_SCROLL_REPAINT_CYCLES} \
         elapsed={elapsed:?} per_cycle_allocations={heap_allocations} per_cycle_bytes={heap_bytes}"
    );

    assert!(
        history_text(&session).is_empty(),
        "explicit TUI repaint polluted {} frozen rows on a {columns}x{rows} screen",
        history_text(&session).len()
    );
    assert_eq!(session.transcript().staging_len(), 0);

    // The exemption is specific to explicit screen manipulation; a bottom-edge linefeed remains
    // genuine process output and must still enter the normal transcript path.
    session
        .feed(format!("\x1b[{rows};1H\r\nreal-output").as_bytes())
        .unwrap();
    assert_eq!(history_text(&session).len(), 1);

    (heap_allocations, heap_bytes)
}

/// PIN - **an explicit TUI scroll/repaint costs the row that scrolled off, never the screen it
/// left, and never becomes transcript.**
///
/// **This used to be a 25 ms wall clock over the whole burst, and could not stay one.** One cycle
/// costs about two microseconds; the gate stood at ~23x the measured total and called the
/// difference "full-suite scheduling headroom". It was not headroom. Ten identical runs of the
/// old test against a full `cargo test --workspace` on the other cores, with not one line of this
/// crate changing, summed to 6.5, 13.7, 18.5, 30.7, 33.7, 35.1, 38.1, 47.2, 48.7 and 62.0 ms -
/// a ninefold spread and seven reds - against 19.5 ms on the same idle machine that had set the
/// gate at 25. The number was reporting the scheduler.
///
/// What the M1.9e regression was is a thing that can be *counted*: work proportional to the whole
/// screen on every cycle, where the screen manipulation only ever moves one row off the top. So
/// that is what is pinned, in the currency
/// `resize_drag_200_frames_stays_within_the_sparse_and_full_budget` already banked one screen up
/// and for the same reason - the heap draw does not move when the machine does.
///
/// **The sharp half is the second arm.** Doubling the rows and changing nothing else must change
/// nothing: a repaint that rebuilt a screenful of anything per cycle would grow with the screen,
/// and a per-row rebuild cannot cover forty more rows without asking for at least forty more
/// allocations. Measured 2026-08-26, identical in every run - alone, inside the 42-test suite,
/// and against a full workspace test run: 16 allocations per cycle at both heights, 25_144 B per
/// cycle at 40 rows and 25_344 B at 80. Doubling the *columns* instead does double the bytes
/// (49_344 B at 240 columns, still 16 allocations), which is the one row that really did scroll
/// off, and is exactly the shape this pin wants to see.
///
/// The absolute budgets are the other half, for a constant-factor blow-up that would keep the
/// shape: ~12% of slack over the measured figures, which is room for the same work written
/// differently and not room for a second structure per cycle.
#[test]
fn g1_primary_tui_explicit_scroll_repaint_is_fast_and_never_becomes_transcript() {
    /// Measured 16; a per-row rebuild of a 40-row screen is at least 40 more.
    const CYCLE_HEAP_ALLOCATIONS: u64 = 18;
    /// Measured 25_144 B on the 120-column arm.
    const CYCLE_HEAP_BYTES: u64 = 28 * 1024;
    /// Measured 5 B per extra row: the one scrolled row's own bookkeeping growing with the grid
    /// it is indexed in. A rebuilt row is a `String` and a boundary table - two orders of
    /// magnitude more than this.
    const BYTES_PER_EXTRA_ROW: u64 = 16;
    /// The rows the second arm adds to the first.
    const EXTRA_ROWS: u64 = 40;

    let (short_allocations, short_bytes) = explicit_scroll_repaint_arm(120, 40);
    let (tall_allocations, tall_bytes) = explicit_scroll_repaint_arm(120, 80);

    assert!(
        short_allocations <= CYCLE_HEAP_ALLOCATIONS,
        "one explicit TUI scroll/repaint cycle made {short_allocations} allocations, budget \
         {CYCLE_HEAP_ALLOCATIONS}"
    );
    assert!(
        short_bytes <= CYCLE_HEAP_BYTES,
        "one explicit TUI scroll/repaint cycle asked for {short_bytes} B, budget \
         {CYCLE_HEAP_BYTES} B"
    );
    assert_eq!(
        tall_allocations, short_allocations,
        "twice the rows must be the same repaint: a cycle costs the row that scrolled off, not \
         the screen it left"
    );
    assert!(
        tall_bytes <= short_bytes + BYTES_PER_EXTRA_ROW * EXTRA_ROWS,
        "twice the rows took {tall_bytes} B a cycle against {short_bytes} B, which is more than \
         {BYTES_PER_EXTRA_ROW} B a row for {EXTRA_ROWS} rows that did not move"
    );
}

#[test]
fn g1_ris_and_deccolm_invalidate_candidates_but_keep_frozen_history() {
    for reset in [b"\x1bc".as_slice(), b"\x1b[?3h".as_slice()] {
        let mut session = DualPlaneSession::new(nz32(4), nz32(2));
        session.feed(b"old\r\nabcde\r\n").unwrap();
        let before = session.document().entries().clone();
        assert!(session.transcript().staging_len() > 0);
        session.feed(reset).unwrap();
        assert_eq!(session.document().entries(), &before);
        assert_eq!(session.transcript().staging_len(), 0);
    }
}

#[test]
fn g1_unterminated_last_line_remains_live() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(3));
    session.feed(b"last line").unwrap();
    assert!(session.document().entries().is_empty());
}

#[test]
fn g1_style_color_and_osc8_metadata_survive_the_real_capture_pipeline() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(2));
    session
        .feed(
            "\x1b[1;31m\x1b]8;;https://example.test\x1b\\界\x1b]8;;\x1b\\\x1b[0m\r\nplain\r\ntail"
                .as_bytes(),
        )
        .unwrap();
    let line = session
        .document()
        .entries()
        .values()
        .find(|entry| entry.line.text == "界")
        .unwrap();
    let span = &line.line.styles[0];
    assert!(span.style.flags.contains(CellFlags::BOLD));
    assert_eq!(span.style.foreground, TerminalColor::Named(1));
    assert_eq!(
        span.hyperlink.as_ref().map(|link| link.uri.as_str()),
        Some("https://example.test")
    );
}
