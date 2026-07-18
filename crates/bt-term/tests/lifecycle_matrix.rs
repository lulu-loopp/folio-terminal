use std::{
    num::{NonZeroU32, NonZeroUsize},
    time::{Duration, Instant},
};

use bt_doc::{DecorationIntent, DecorationLifecycle};
use bt_term::DualPlaneSession;
use bt_transcript::{CellFlags, TerminalColor};
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

#[test]
fn g1_scroll_out_stages_finalizes_decorates_and_observes_tail_rewrite() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(2));
    session.feed(b"$$abcdef$$Z\r\n").unwrap();
    assert!(session.document().entries().is_empty());
    assert_eq!(session.transcript().staging_len(), 1);

    session.feed(b"\x1b[1;5H!\x1b[2;1H\r\n").unwrap();
    let (id, entry) = session.document().entries().first_key_value().unwrap();
    let id = *id;
    assert_eq!(entry.line.text, "$$abcdef$$Z !");
    assert!(matches!(entry.decoration, DecorationIntent::Math { .. }));
    assert_eq!(
        session.decoration(id).unwrap().decoration,
        DecorationLifecycle::Pending
    );

    session.run_workers();
    assert_eq!(
        session.decoration(id).unwrap().decoration,
        DecorationLifecycle::Ready
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
    assert_eq!((initial.columns.get(), initial.cells.len()), (8, 16));

    session.resize(nz32(20), nz32(2)).unwrap();
    session.refresh_projection(&mut projection);
    let resized = session.viewport_frame(&mut projection).unwrap();

    assert_eq!(projection.layout_key(), session.layout_key());
    assert_eq!((resized.columns.get(), resized.cells.len()), (20, 40));
}

#[test]
fn g1_vendor_tail_owns_nonblank_shrink_rows_until_grow_restores_them() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(4));
    session.feed(b"r1\r\nr2\r\nr3\r\nr4").unwrap();
    session.resize(nz32(8), nz32(2)).unwrap();
    assert!(history_text(&session).is_empty());
    assert!(staged_text(&session).is_empty());
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
    assert_eq!(session.transcript().staging_len(), 0);
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
    const PROMPT: &str = "(base) PS D:\\Developer\\BetterTerminal>";
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
            assert_eq!(frame.cells.len(), columns as usize * rows as usize);
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
    const PROMPT: &str = "(base) PS D:\\Developer\\BetterTerminal> ";
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
    assert_eq!(session.terminal().resize_transaction_history_size(), 2);

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
            history_before: 2,
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
    let prompt = "(base) PS D:\\Developer\\BetterTerminal> ";
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
            cells: 8,
            anchors: 8,
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
            "(base) PS D:\\Developer\\BetterTerminal>".to_owned(),
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
    assert_eq!(session.transcript().staging_len(), 0);
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

#[test]
fn g1_primary_tui_explicit_scroll_repaint_is_fast_and_never_becomes_transcript() {
    const CYCLES: usize = 512;
    const BUDGET: Duration = Duration::from_millis(250);

    let mut session = DualPlaneSession::new(nz32(120), nz32(40));
    let initial = (0..40)
        .map(|row| format!("frame-row-{row:02}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    session.feed(initial.as_bytes()).unwrap();
    assert!(history_text(&session).is_empty());

    let started = Instant::now();
    for cycle in 0..CYCLES {
        // CSI S is an explicit full-screen manipulation used to collapse/repaint an upward TUI.
        // It is not a linefeed carrying new process output into canonical history.
        session
            .feed(format!("\x1b[S\x1b[40;1H\x1b[2Kframe-{cycle:03}").as_bytes())
            .unwrap();
    }
    let elapsed = started.elapsed();

    assert!(
        elapsed <= BUDGET,
        "{CYCLES} explicit TUI scroll/repaint cycles took {elapsed:?}, budget {BUDGET:?}"
    );
    assert!(
        history_text(&session).is_empty(),
        "explicit TUI repaint polluted {} frozen rows in {elapsed:?}",
        history_text(&session).len()
    );
    assert_eq!(session.transcript().staging_len(), 0);

    // The exemption is specific to explicit screen manipulation; a bottom-edge linefeed remains
    // genuine process output and must still enter the normal transcript path.
    session.feed(b"\x1b[40;1H\r\nreal-output").unwrap();
    assert_eq!(history_text(&session).len(), 1);
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
    assert_eq!(span.hyperlink.as_deref(), Some("https://example.test"));
}
