//! **`attention_map`, as the application drives it.** Tests whose first assertion is about
//! `attention_map`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{PtyPresentationHarness, frame_row_text};
use std::time::Duration;

/// The jump's flash lands on the row that is **showing** the command, and on
/// nothing at all when that content is not on the glass.
///
/// Read out of the presented frame's own `cell_anchors` rather than from the
/// projection, for the reason [`Runtime::command_flash_layer`] gives: the
/// frame is the picture, and a second derivation of "which row is that
/// content in" is a second thing that can be off by one.
///
/// MUTATION: drop the containment check at the end of [`frame_row_of_anchor`]
/// and the last assertion goes red — an anchor below the last drawable row
/// would light the last one, which is a band drawn over the wrong command.
#[test]
fn the_flash_finds_the_row_a_frame_is_showing_a_command_on() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(4).unwrap());
    let mut projection = session.new_projection(session.layout_key());
    session.feed(b"one\r\ntwo\r\nthree\r\n").unwrap();
    session.refresh_projection(&mut projection);
    let frame = session
        .viewport_frame(&mut projection)
        .expect("a frame for a four-row grid");
    let columns = frame.columns.get() as usize;

    // Every row of the frame answers itself, which is the property the whole
    // band is placed by.
    for row in 0..frame.drawable_rows() {
        let anchor = frame.cell_anchors[row * columns].start.clone();
        assert_eq!(
            frame_row_of_anchor(&frame, &anchor).map(|found| found.top_subpixels),
            Some(frame.row_map[row].top_subpixels),
            "row {row} does not find itself"
        );
    }

    // The alternate screen is an isolated namespace (§3.2), so its
    // coordinates are neither before nor after this frame's — the honest
    // answer is no row rather than a guessed one.
    assert!(
        frame_row_of_anchor(
            &frame,
            &bt_doc::ContentAnchor::Live {
                screen: bt_doc::ScreenId::Alternate,
                point: bt_doc::GridPoint { row: 0, column: 0 },
                bias: bt_doc::Bias::Before,
                generation: bt_doc::GridGeneration(1),
            }
        )
        .is_none(),
        "an alternate-screen anchor lights nothing on a primary frame"
    );

    // And content past the bottom of the frame: a row that has not been drawn
    // is not a row the last one may stand in for.
    let past = bt_doc::ContentAnchor::Live {
        screen: bt_doc::ScreenId::Primary,
        point: bt_doc::GridPoint {
            row: frame.grid_rows.get() + 40,
            column: 0,
        },
        bias: bt_doc::Bias::Before,
        generation: bt_doc::GridGeneration(1),
    };
    assert!(frame_row_of_anchor(&frame, &past).is_none());
}

/// PIN (user repro 2026-08-02, re-seated by the frame-derived ruling 2026-08-04): one hovered
/// cell resolves to one reference, through the frame's own row stride, and where a printed path
/// and a link target cover the same cell the pointer answers with the text it is standing on.
///
/// What the four verbs share is this lookup: the scan produced the list, and hover, click and
/// peek all ask it the same question about the same `GridHit`. The list's own contents — which
/// shapes are in it, and that a `file://` to a `.txt` is in none — are bt-term's to pin, beside
/// the detector that decides it (`underline_coverage_equals_peek_coverage_for_every_shape`).
#[test]
fn one_hit_resolves_to_one_reference_through_the_frames_own_stride() {
    let printed = PathBuf::from(r"D:\from-text.png");
    let linked = PathBuf::from(r"D:\layout-preview.png");
    let references = FrameImageReferences {
        columns: 10,
        references: vec![
            // The scan's order: printed text first, link targets after it.
            bt_term::FrameImageReference {
                path: printed.clone(),
                cells: vec![12, 13, 14],
                verified: true,
            },
            bt_term::FrameImageReference {
                path: linked.clone(),
                cells: vec![10, 11, 12, 13, 14, 15],
                verified: true,
            },
        ],
    };
    let at = |row, column| {
        references
            .at(bt_render::GridHit { row, column })
            .map(|reference| reference.path.clone())
    };
    assert_eq!(
        at(1, 2),
        Some(printed),
        "text under the pointer is what the pointer was put on",
    );
    assert_eq!(
        at(1, 0),
        Some(linked),
        "and the link answers where its label spells no file",
    );
    assert_eq!(at(1, 6), None, "one column past the link is ordinary text");
    assert_eq!(at(0, 2), None, "the row is part of the address");
    assert_eq!(
        FrameImageReferences::default().at(bt_render::GridHit { row: 0, column: 0 }),
        None,
        "a frame with no references answers nothing anywhere",
    );
}

#[test]
fn content_before_trailing_bsu_is_published_and_survives_empty_sync_timeout() {
    let mut harness = PtyPresentationHarness::new(20, 2);

    assert!(harness.feed_drain(b"visible-before-bsu\x1b[?2026h"));
    assert!(harness.session.synchronized_update_deadline().is_some());
    assert_eq!(harness.publications, 1);
    assert!(
        frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("visible-before-bsu")
    );

    let (finished, republished) = harness.finish_synchronized_update();
    assert!(finished);
    assert!(
        !republished,
        "the already-pending visible frame is equivalent"
    );
    assert_eq!(harness.publications, 1);
    assert!(
        frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("visible-before-bsu")
    );
}

#[test]
fn completed_sync_update_is_published_before_a_trailing_bsu_in_the_same_drain() {
    let mut harness = PtyPresentationHarness::new(24, 2);

    assert!(harness.feed_drain(b"\x1b[?2026h\x1b[H\x1b[2Kclosed-update\x1b[?2026l\x1b[?2026h"));
    assert!(harness.session.synchronized_update_deadline().is_some());
    assert_eq!(harness.publications, 1);
    assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("closed-update"));
}

#[test]
fn rapid_synchronized_update_chain_never_withholds_a_completed_frame() {
    const UPDATE_COUNT: usize = 81;
    let mut harness = PtyPresentationHarness::new(24, 2);

    for update in 0..UPDATE_COUNT {
        let prefix = if update == 0 { "\x1b[?2026h" } else { "" };
        let bytes = format!("{prefix}\x1b[H\x1b[2Kframe-{update:02}\x1b[?2026l\x1b[?2026h");
        assert!(
            harness.feed_drain(bytes.as_bytes()),
            "completed update {update} was not published"
        );
        assert!(harness.session.synchronized_update_deadline().is_some());
        assert!(
            frame_row_text(harness.pending.pending_frame().unwrap(), 0)
                .contains(&format!("frame-{update:02}"))
        );
    }

    assert_eq!(harness.publications, UPDATE_COUNT);
    assert!(!harness.feed_drain(b"\x1b[?2026l"));
    assert!(harness.session.synchronized_update_deadline().is_none());
    assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("frame-80"));
    assert!(harness.present_pending());
    assert!(harness.pending.pending_frame().is_none());
}

#[test]
fn pending_frame_is_the_a_b_a_equivalence_baseline() {
    let mut harness = PtyPresentationHarness::new(4, 1);

    assert!(harness.feed_drain(b"A"));
    assert!(harness.present_pending());
    assert!(frame_row_text(harness.last_presented.as_ref().unwrap(), 0).contains('A'));

    assert!(harness.feed_drain(b"\rB"));
    assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains('B'));
    assert!(harness.feed_drain(b"\rA"));
    assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains('A'));
    assert_eq!(harness.publications, 3);
}

#[test]
fn post_drag_wheel_frame_reaches_the_renderer_text_row_slice_boundary() {
    let start = Instant::now();
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(3).unwrap());
    session.feed_at(b"a\r\nb\r\nc", start).unwrap();
    session
        .resize_at(
            NonZeroU32::new(4).unwrap(),
            NonZeroU32::new(2).unwrap(),
            start + Duration::from_millis(10),
        )
        .unwrap();
    session.mark_pty_resize_requested_at(
        NonZeroU32::new(4).unwrap(),
        NonZeroU32::new(2).unwrap(),
        start + Duration::from_millis(210),
    );
    session
        .feed_at(b"\r\nx", start + Duration::from_millis(220))
        .unwrap();
    assert!(
        session
            .finish_resize_if_quiescent(start + Duration::from_millis(420))
            .unwrap()
    );

    let mut projection = session.new_projection(session.layout_key());
    projection.scroll_by_rows(1);
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    session.record_published_frame(&frame, start + Duration::from_millis(421));
    let render_rows = bt_render::text_row_cells(&frame)
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(frame.grid_rows.get(), 2);
    assert_eq!(frame.rows.get(), 3);
    assert_eq!(frame.drawable_rows(), 2);
    assert_eq!(render_rows.len(), 3);
    assert!(render_rows.iter().all(|row| row.len() == 4));
}

#[test]
fn direct_width_fixture_places_legacy_and_2027_closing_bars_on_their_rulers() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
    session
        .feed(include_bytes!("../../../scripts/dev/width-probe-input.vt"))
        .unwrap();

    let closing_bar = |row: u32, start: usize| {
        session
            .terminal()
            .visible_row(row)
            .unwrap()
            .cells
            .iter()
            .enumerate()
            .skip(start)
            .filter(|(_, cell)| cell.text == "|")
            .map(|(column, _)| column)
            .nth(1)
            .unwrap()
    };
    let rows = [2, 4, 6, 8, 10, 12, 14];
    let legacy_widths = [8, 4, 2, 1, 7, 1, 2];
    let mode_2027_widths = [2, 2, 2, 1, 7, 1, 2];
    for ((row, legacy), clustered) in rows.into_iter().zip(legacy_widths).zip(mode_2027_widths) {
        assert_eq!(closing_bar(row, 0), 1 + legacy, "legacy content row {row}");
        assert_eq!(
            closing_bar(row + 1, 0),
            1 + legacy,
            "legacy ruler row {}",
            row + 1
        );
        assert_eq!(
            closing_bar(row, 40),
            41 + clustered,
            "2027 content row {row}"
        );
        assert_eq!(
            closing_bar(row + 1, 40),
            41 + clustered,
            "2027 ruler row {}",
            row + 1
        );
    }
}

#[test]
fn direct_glyph_fixture_preserves_emoji_and_ambiguous_cell_occupancy() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
    session
        .feed(include_bytes!("../../../scripts/dev/glyph-probe-input.vt"))
        .unwrap();

    let bar_columns = |row: u32| {
        session
            .terminal()
            .visible_row(row)
            .unwrap()
            .cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.text == "|")
            .map(|(column, _)| column)
            .collect::<Vec<_>>()
    };
    for (content_row, width) in [(2, 2), (4, 2), (6, 2), (8, 2), (10, 1)] {
        assert_eq!(bar_columns(content_row), [0, 1 + width]);
        assert_eq!(bar_columns(content_row + 1), [0, 1 + width]);
    }
    assert_eq!(bar_columns(13), [0, 2, 4]);
    assert_eq!(bar_columns(14), [0, 2, 4]);
}

// ── W2 slice ③: a page is a preview buffer, in every list a file is in ──

/// A pool holding a file and a page, which is the state every claim below is
/// about.
fn a_pool_with_a_file_and_a_page() -> preview::PreviewPool {
    let mut pool = preview::PreviewPool::default();
    pool.insert(preview::PreviewBuffer::new(
        preview::PreviewSource::file(r"C:\work\notes.md"),
        "notes.md".to_owned(),
    ));
    pool.insert(preview::PreviewBuffer::new(
        preview::PreviewSource::Web("http://localhost:5173/app".to_owned()),
        "Folio site".to_owned(),
    ));
    pool
}

/// **A page and a file are the same kind of row** (`docs/DESIGN.md` §7.7 ⑤ —
/// 「同一张列表、同一个索引空间」).
///
/// One list, one index space, one order: the pool's. What differs between the
/// two rows is the two things that genuinely differ — the mark, and which
/// category the pin writes — and the test says both out loud because a build
/// that filed a page under `file` would put it in the section the *root menu*
/// draws from.
///
/// Red gate: `preview_menu_target` answering `PinKind::File` for every source
/// makes the category assertion fail; `preview_row_mark` answering `File` for
/// every row makes the mark assertion fail.
#[test]
fn the_switcher_lists_a_page_in_the_same_table_as_a_file() {
    let rows = switcher_rows(&a_pool_with_a_file_and_a_page(), None, &[]);
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        vec!["notes.md", "Folio site"],
        "the pool's own order, both kinds of row in it"
    );
    assert_eq!(
        rows.iter().filter_map(|row| row.pool).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        rows[0].keep.as_ref().map(|keep| keep.kind),
        Some(bt_persist::PinKind::File)
    );
    assert_eq!(
        rows[1].keep.as_ref().map(|keep| keep.kind),
        Some(bt_persist::PinKind::Url),
        "a page is kept as a page, not as a file with a strange name"
    );
    assert_eq!(
        rows[1].keep.as_ref().map(|keep| keep.target.as_str()),
        Some("http://localhost:5173/app"),
        "and what it keeps is the switcher key, verbatim"
    );
    assert!(!rows[0].is_page() && rows[1].is_page());
    assert_eq!(
        marks::preview_row_mark(rows[1].is_page(), None),
        marks::ChromeMark::Globe { favicon: None }
    );
    assert_eq!(
        marks::preview_row_mark(rows[0].is_page(), None),
        marks::ChromeMark::File,
        "and the file beside it is unchanged"
    );
    // **And the row can name the site it would ask about** (the favicon
    // slice): the address it keeps is the address the store is keyed by, so
    // the row needs nothing this list was not already holding.
    assert_eq!(rows[1].page_url(), Some("http://localhost:5173/app"));
    assert_eq!(rows[0].page_url(), None);
    assert_eq!(
        marks::preview_row_mark(rows[0].is_page(), Some(favicon::FaviconId::for_tests(1))),
        marks::ChromeMark::File,
        "and an icon handed to a file is refused rather than drawn"
    );
}

/// **A kept page that is also open is one row, not two** (user ruling
/// 2026-08-19: 「已钉 URL 再次出现提升同一条目,PINNED 与 MRU 不留双副本」).
///
/// The lift compares the *pair* — category and target — because `pins.json`
/// is one array and a row is identified by both. A kept page nobody has
/// opened is still a row, which is what makes the section worth having on the
/// first frame after a restart, and it is named by its site because the pin
/// file stores a place and never a title.
///
/// Red gate: make `preview_menu_target` answer one category for everything —
/// the open page stops matching its own kept row and is listed twice, once
/// above under its site and once below under its title.
#[test]
fn a_kept_page_that_is_open_is_lifted_rather_than_copied() {
    let kept = vec![
        (
            bt_persist::PinKind::Url,
            "http://localhost:5173/app".to_owned(),
        ),
        (
            bt_persist::PinKind::Url,
            "http://127.0.0.1:8080/report".to_owned(),
        ),
    ];
    let rows = switcher_rows(&a_pool_with_a_file_and_a_page(), None, &kept);
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        vec!["Folio site", "127.0.0.1:8080", "notes.md"],
        "the two kept pages first, then what is left of the pool"
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.keep.as_ref().map(|keep| keep.target.as_str())
                == Some("http://localhost:5173/app"))
            .count(),
        1,
        "the open page appears once, lifted rather than copied"
    );
    assert_eq!(
        rows[0].pool,
        Some(1),
        "and the lifted row still points at the buffer it came from"
    );
    assert_eq!(
        rows[1].pool, None,
        "while the kept page nobody has opened has no buffer behind it"
    );
    assert!(rows[0].pinned && rows[1].pinned && !rows[2].pinned);
}
