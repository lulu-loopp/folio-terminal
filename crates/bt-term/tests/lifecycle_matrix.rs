use std::num::{NonZeroU32, NonZeroUsize};

use bt_doc::{DecorationIntent, DecorationLifecycle};
use bt_term::DualPlaneSession;
use bt_transcript::{CellFlags, TerminalColor};

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
fn g1_resize_shrink_captures_exactly_nonblank_rows_removed_from_the_top() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(4));
    session.feed(b"r1\r\nr2\r\nr3\r\nr4").unwrap();
    session.resize(nz32(6), nz32(2)).unwrap();
    assert_eq!(history_text(&session), vec!["r1", "r2"]);
    assert_eq!(session.terminal().visible_text(), vec!["r3", "r4"]);
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
fn g1_resize_jitter_does_not_duplicate_captured_rows() {
    let mut session = DualPlaneSession::new(nz32(8), nz32(4));
    session.feed(b"r1\r\nr2\r\nr3\r\nr4").unwrap();
    session.resize(nz32(7), nz32(2)).unwrap();
    session.resize(nz32(9), nz32(5)).unwrap();
    session.resize(nz32(6), nz32(2)).unwrap();
    let text = history_text(&session);
    assert_eq!(text.iter().filter(|line| **line == "r1").count(), 1);
    assert_eq!(text.iter().filter(|line| **line == "r2").count(), 1);
}

#[test]
fn g1_width_resize_forces_a_cross_boundary_logical_line_split() {
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
