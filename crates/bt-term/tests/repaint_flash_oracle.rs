use std::{num::NonZeroU32, time::Duration};

use bt_math::{MathRaster, MathRenderError};
use bt_term::{
    DualPlaneSession, FormulaFlashOracle, FormulaFrameState, LIVE_MATH_STABLE_INTERVAL,
    SessionMathTask,
};

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn synthetic_raster(width_px: u32, height_px: u32) -> MathRaster {
    MathRaster {
        rgba: vec![0xff; width_px as usize * height_px as usize * 4],
        width_px,
        height_px,
        content_height_px: height_px,
        ascent_px: height_px as f32 - 4.0,
        descent_px: 4.0,
        baseline_px: height_px as f32 - 4.0,
        render_time: Duration::from_millis(1),
        inline_runs: Vec::new(),
    }
}

fn complete_live_math(session: &mut DualPlaneSession) {
    while let Some(task) = session.take_math_worker_task() {
        let SessionMathTask::Live(mut task) = task else {
            panic!("alternate-screen fixture unexpectedly scheduled frozen math");
        };
        if bt_detect::resolve_live_detection_task(&mut task) {
            assert!(session.complete_live_worker_result(task, Ok(synthetic_raster(40, 40))));
        } else {
            assert!(session.complete_live_worker_result(task, Err(MathRenderError::NotDetected)));
        }
    }
}

fn observe_frame(
    session: &mut DualPlaneSession,
    projection: &mut bt_viewport::ViewportProjection,
    oracle: &mut FormulaFlashOracle,
) -> FormulaFrameState {
    session.refresh_projection(projection);
    let frame = session.viewport_frame(projection).unwrap();
    oracle.observe(&frame).state
}

fn synchronized_repaint(rows: &[&str]) -> Vec<u8> {
    let mut out = b"\x1b[?2026h\x1b[?25l\x1b[H".to_vec();
    for (row, line) in rows.iter().enumerate() {
        if row != 0 {
            out.extend_from_slice(format!("\x1b[{};1H", row + 1).as_bytes());
        }
        out.extend_from_slice(b"\x1b[K");
        out.extend_from_slice(line.as_bytes());
    }
    out.extend_from_slice(b"\x1b[?25h\x1b[?2026l");
    out
}

/// The same screen rewrite [`synchronized_repaint`] makes, without the DEC 2026 wrapper around it:
/// every cell lands as its bytes are parsed instead of waiting for a terminator.
fn unsynchronized_repaint(rows: &[&str]) -> Vec<u8> {
    let mut out = b"\x1b[?25l\x1b[H".to_vec();
    for (row, line) in rows.iter().enumerate() {
        if row != 0 {
            out.extend_from_slice(format!("\x1b[{};1H", row + 1).as_bytes());
        }
        out.extend_from_slice(b"\x1b[K");
        out.extend_from_slice(line.as_bytes());
    }
    out.extend_from_slice(b"\x1b[?25h");
    out
}

fn frame_row_text(frame: &bt_viewport::ViewportFrame, row: usize) -> String {
    let columns = frame.columns.get() as usize;
    frame.cells[row * columns..(row + 1) * columns]
        .iter()
        .map(|cell| cell.text.as_str())
        .collect()
}

#[test]
fn interaction_repaint_never_reexposes_ready_formula_source() {
    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(40), nz(12));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();

    session
        .feed_at(b"\x1b[?1049h$$x$$\r\n\r\nspin-0\r\ntail", start)
        .unwrap();
    session.refresh_projection(&mut projection);
    let initial_source = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(
        oracle.observe(&initial_source).state,
        FormulaFrameState::Source
    );

    assert_eq!(
        session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL),
        1,
        "fixture did not schedule its formula: {:?}",
        session.terminal().visible_text()
    );
    complete_live_math(&mut session);
    projection = session.new_projection(session.layout_key());
    let ready = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(oracle.observe(&ready).state, FormulaFrameState::Rendered);
    let ready_detection_count = session.live_detection_count();

    // Claude Code-style pointer repaint: every visible row is rewritten in place in one PTY drain.
    session
        .feed_at(
            b"\x1b[H\x1b[2K$$x$$\x1b[2;1H\x1b[2K\x1b[3;1H\x1b[2Kspin-0\x1b[4;1H\x1b[2Ktail",
            start + Duration::from_millis(210),
        )
        .unwrap();
    session.refresh_projection(&mut projection);
    let clicked = session.viewport_frame(&mut projection).unwrap();
    oracle.observe(&clicked);

    // A second pointer/double-click repaint arrives as a later PTY drain.
    session
        .feed_at(
            b"\x1b[H\x1b[2K$$x$$\x1b[2;1H\x1b[2K\x1b[3;1H\x1b[2Kspin-0\x1b[4;1H\x1b[2Ktail",
            start + Duration::from_millis(230),
        )
        .unwrap();
    session.refresh_projection(&mut projection);
    let double_clicked = session.viewport_frame(&mut projection).unwrap();
    oracle.observe(&double_clicked);

    session.advance_live_stability(start + Duration::from_millis(430));
    complete_live_math(&mut session);
    observe_frame(&mut session, &mut projection, &mut oracle);

    assert!(
        !oracle.flash_detected(),
        "a formula that was already Ready flashed back to source; sequence={:?}, flashed={:?}",
        oracle.frames(),
        oracle.flashed_sources()
    );
    assert_eq!(session.live_detection_count(), ready_detection_count);
}

#[test]
fn captured_clear_home_scroll_repaint_never_reexposes_ready_formula_source() {
    const REPAINTS: &[(u64, &[u8])] = &[
        (
            0,
            b"\x1b[?1049h\x1b[2J\x1b[Hfiller top 0\r\nfiller top 1\r\nfiller top 2\r\nfiller top 3\r\nfiller top 4\r\nfiller top 5\r\nfiller top 6\r\nfiller top 7\r\n$$\r\n\\nabla \\cdot \\mathbf{E} = \\frac{\\rho}{\\varepsilon_0}\r\n$$\r\nfiller bottom 0\r\nfiller bottom 1\r\nfiller bottom 2\r\nfiller bottom 3\r\nfiller bottom 4\r\nfiller bottom 5\r\nprompt> ",
        ),
        (
            400_000,
            b"\x1b[2J\x1b[Hfiller top 0\r\nfiller top 1\r\nfiller top 2\r\nfiller top 3\r\nfiller top 4\r\nfiller top 5\r\n$$\r\n\\nabla \\cdot \\mathbf{E} = \\frac{\\rho}{\\varepsilon_0}\r\n$$\r\nfiller bottom 0\r\nfiller bottom 1\r\nfiller bottom 2\r\nfiller bottom 3\r\nfiller bottom 4\r\nfiller bottom 5\r\nprompt> ",
        ),
        (
            800_000,
            b"\x1b[2J\x1b[Hfiller top 0\r\nfiller top 1\r\nfiller top 2\r\nfiller top 3\r\n$$\r\n\\nabla \\cdot \\mathbf{E} = \\frac{\\rho}{\\varepsilon_0}\r\n$$\r\nfiller bottom 0\r\nfiller bottom 1\r\nfiller bottom 2\r\nfiller bottom 3\r\nfiller bottom 4\r\nfiller bottom 5\r\nprompt> ",
        ),
        (
            1_200_000,
            b"\x1b[2J\x1b[Hfiller top 0\r\nfiller top 1\r\n$$\r\n\\nabla \\cdot \\mathbf{E} = \\frac{\\rho}{\\varepsilon_0}\r\n$$\r\nfiller bottom 0\r\nfiller bottom 1\r\nfiller bottom 2\r\nfiller bottom 3\r\nfiller bottom 4\r\nfiller bottom 5\r\nprompt> ",
        ),
    ];

    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(80), nz(24));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();
    let mut ready_detection_count = None;

    for (elapsed_us, repaint) in REPAINTS {
        let elapsed = Duration::from_micros(*elapsed_us);
        session.advance_live_stability(start + elapsed);
        complete_live_math(&mut session);
        if *elapsed_us != 0 {
            assert_eq!(
                observe_frame(&mut session, &mut projection, &mut oracle),
                FormulaFrameState::Rendered,
                "the previously stable formula was not rendered before repaint at {elapsed_us} us"
            );
            ready_detection_count.get_or_insert(session.live_detection_count());
        }
        session.feed_at(repaint, start + elapsed).unwrap();
        observe_frame(&mut session, &mut projection, &mut oracle);
    }

    session.advance_live_stability(
        start + Duration::from_micros(1_200_000) + LIVE_MATH_STABLE_INTERVAL,
    );
    complete_live_math(&mut session);
    observe_frame(&mut session, &mut projection, &mut oracle);

    // Mutation: restoring eager band invalidation in `observe_live_damage` makes the first shifted
    // clear/home repaint expose the exact source again and turns this assertion red.
    assert!(
        !oracle.flash_detected(),
        "captured scroll repaint flashed a Ready formula; sequence={:?}, flashed={:?}",
        oracle.frames(),
        oracle.flashed_sources()
    );
    assert_eq!(
        Some(session.live_detection_count()),
        ready_detection_count,
        "preserve/translate must not schedule another detector pass"
    );
}

/// Claude Code's real repaint is a DEC 2026 synchronized update that homes and rewrites each line
/// with erase-to-EOL (`\x1b[K`), never `\x1b[2J`. Keying the repaint boundary only on `2J` (the
/// original M1.9o) missed every real repaint, so the formula flashed on the actual terminal while
/// every unit test passed. This fixture repaints the way Claude Code does and must stay flash-free.
/// Mutation: narrowing `contains_clear_home_snapshot_boundary` back to `2J`-only turns it red.
#[test]
fn synchronized_update_repaint_never_reexposes_ready_formula_source() {
    fn sync_repaint(top_filler: usize) -> Vec<u8> {
        let mut rows: Vec<String> = Vec::new();
        for i in 0..top_filler {
            rows.push(format!("filler top {i}"));
        }
        rows.push("$$".to_owned());
        rows.push(r"\nabla \cdot \mathbf{E} = \frac{\rho}{\varepsilon_0}".to_owned());
        rows.push("$$".to_owned());
        rows.push("prompt> ".to_owned());
        let mut out = Vec::new();
        out.extend_from_slice(b"\x1b[?2026h\x1b[?25l\x1b[H");
        for (r, line) in rows.iter().enumerate() {
            if r > 0 {
                out.extend_from_slice(format!("\x1b[{};1H", r + 1).as_bytes());
            }
            out.extend_from_slice(b"\x1b[K");
            out.extend_from_slice(line.as_bytes());
        }
        out.extend_from_slice(b"\x1b[?25h\x1b[?2026l");
        out
    }

    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(80), nz(20));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();

    // Enter alt screen + SGR mouse (Claude Code sets these), then the initial synchronized paint.
    let mut first = b"\x1b[?1049h\x1b[?1006h".to_vec();
    first.extend_from_slice(&sync_repaint(8));
    session.feed_at(&first, start).unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );
    let ready_detection_count = session.live_detection_count();

    // Scroll-review: the formula shifts up 2 rows per synchronized repaint.
    for (i, top_filler) in [6usize, 4, 2].into_iter().enumerate() {
        let elapsed = Duration::from_micros(400_000 * (i as u64 + 1));
        session
            .feed_at(&sync_repaint(top_filler), start + elapsed)
            .unwrap();
        observe_frame(&mut session, &mut projection, &mut oracle);
    }

    session.advance_live_stability(start + Duration::from_micros(1_400_000));
    complete_live_math(&mut session);
    observe_frame(&mut session, &mut projection, &mut oracle);

    // The user-visible invariant: a Ready formula never flashes back to source across the real
    // Claude Code repaint. (A 2026 repaint that shifts the block may fall to the bounded reconcile
    // path rather than an exact translate, so the detector count is not asserted here - that is a
    // synchronous same-frame path with no source frame, covered by the flash assertion.)
    let _ = ready_detection_count;
    assert!(
        !oracle.flash_detected(),
        "synchronized-update repaint flashed a Ready formula; sequence={:?}, flashed={:?}",
        oracle.frames(),
        oracle.flashed_sources()
    );
}

#[test]
fn multiline_formula_crossing_internal_pane_bottom_keeps_identity_and_raster() {
    const BEFORE: &[&str] = &[
        "content alpha",
        "content beta",
        "$$",
        r"\begin{aligned}",
        r"a &= b + c \\",
        r"d &= e + f \\",
        r"\end{aligned}",
        "$$",
        "",
        "────────────────────────",
        "prompt> ",
        "status: ready",
    ];
    const CROSSES_PANE_BOTTOM: &[&str] = &[
        "new content 0",
        "new content 1",
        "new content 2",
        "content alpha",
        "content beta",
        "$$",
        r"\begin{aligned}",
        r"a &= b + c \\",
        "",
        "────────────────────────",
        "prompt> ",
        "status: ready",
    ];

    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(48), nz(12));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();

    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&synchronized_repaint(BEFORE));
    session.feed_at(&first, start).unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );
    let detections = session.live_detection_count();

    // The upper content pane moves by +3 while blank row 8 and the fixed chrome at rows 9..11 stay
    // put. The proven band now intersects the content pane only at rows 5..7; its tail is occluded,
    // not mapped onto the separator/prompt. Mutation: using one whole-band delta compares those
    // tail rows with chrome and makes this assertion turn red.
    session
        .feed_at(
            &synchronized_repaint(CROSSES_PANE_BOTTOM),
            start + Duration::from_millis(400),
        )
        .unwrap();
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );
    let observation = oracle.frames().last().unwrap();
    assert_eq!(observation.occluded_sources.len(), 1);

    // A later in-place repaint keeps the previously proven pane boundary. The Jump chip mutates
    // only the pane's last row, so that row becomes occluded without granting a frame-wide waiver.
    let mut boundary_overlay = CROSSES_PANE_BOTTOM.to_vec();
    boundary_overlay[7] = r"a &= b + c \\        Jump to bottom (ctrl+End)";
    session
        .feed_at(
            &synchronized_repaint(&boundary_overlay),
            start + Duration::from_millis(420),
        )
        .unwrap();
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );
    session.refresh_projection(&mut projection);
    let boundary_frame = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(
        frame_row_text(&boundary_frame, 7).trim(),
        "Jump to bottom (ctrl+End)",
        "an occluded row clears exactly this occurrence's proven source prefix while the \
         application's Jump chip keeps its own cells (text and highlight style)"
    );
    assert_eq!(
        frame_row_text(&boundary_frame, 9).trim(),
        "────────────────────────",
        "fixed chrome which does not match the proven source must never be cleared"
    );
    assert!(!oracle.flash_detected(), "sequence={:?}", oracle.frames());
    assert_eq!(
        session.live_detection_count(),
        detections,
        "preserving a proven block must not schedule first detection again"
    );
}

#[test]
fn repaint_with_opener_above_row_zero_keeps_visible_formula_suffix_rendered() {
    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(40), nz(6));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();

    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&synchronized_repaint(&[
        "top", "$$", "x + y", "$$", "tail", "prompt> ",
    ]));
    session.feed_at(&first, start).unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );
    let detections = session.live_detection_count();

    // The opener and the old top row are now above row zero. The body/closer suffix is exact,
    // so this is preservation of an already-proven occurrence, not an ambiguous first detection.
    session
        .feed_at(
            &synchronized_repaint(&["x + y", "$$", "tail", "prompt> ", "", ""]),
            start + Duration::from_millis(400),
        )
        .unwrap();
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );
    assert!(!oracle.flash_detected(), "sequence={:?}", oracle.frames());
    assert_eq!(session.live_detection_count(), detections);
}

#[test]
fn repaint_with_closer_below_last_row_keeps_visible_formula_prefix_rendered() {
    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(40), nz(8));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();

    let complete = [
        "$$",
        r"\begin{cases}",
        r"x + y &= 1 \\",
        r"x - y &= 0",
        r"\end{cases}",
        "$$",
        "tail",
        "prompt> ",
    ];
    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&synchronized_repaint(&complete));
    session.feed_at(&first, start).unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );
    let detections = session.live_detection_count();

    // The same proven block moves down: its closer is now one row below the grid, while the exact
    // opener/body/end prefix remains visible and must keep the original band/raster identity.
    session
        .feed_at(
            &synchronized_repaint(&[
                "header 0",
                "header 1",
                "header 2",
                "$$",
                r"\begin{cases}",
                r"x + y &= 1 \\",
                r"x - y &= 0",
                r"\end{cases}",
            ]),
            start + Duration::from_millis(400),
        )
        .unwrap();
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );
    assert!(!oracle.flash_detected(), "sequence={:?}", oracle.frames());
    assert_eq!(session.live_detection_count(), detections);
}

#[test]
fn rendered_formula_stays_rendered_across_grid_resize_and_fresh_raster_swap() {
    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(48), nz(8));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();

    session
        .feed_at(b"\x1b[?1049h$$x^2 + y^2 = z^2$$\r\nprompt> ", start)
        .unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );

    session
        .resize_at(nz(40), nz(10), start + Duration::from_millis(250))
        .unwrap();
    projection = session.new_projection(session.layout_key());
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered,
        "the stale raster must bridge the resize without exposing source"
    );
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );
    assert!(!oracle.flash_detected(), "sequence={:?}", oracle.frames());
}

/// **A formula proven while a repaint window is open must survive the window's close.**
///
/// The window opens at the repaint's boundary and can stay open for several reads — for the whole of
/// a DEC 2026 block, or for a repaint the operating system split across reads — and detection keeps
/// running inside it. The snapshot taken when the window opened knows nothing about a block proven
/// after it, so rebuilding the live decorations from that snapshot alone threw exactly those away:
/// the picture vanished at the close and only came back once the detector and the rasteriser had
/// done the whole job again, which on a real screen is several frames of LaTeX. On the owner's
/// recording of 2026-09-17 that happened at every one of the 117 repaints.
///
/// The off-band record is not decoration: a window only opens for a pane that has something to
/// preserve, and in the recording that something was six formulas which had long since scrolled away
/// (`resident=0 dormant=6` at the read that lost the picture). Without one, no window opens and this
/// fixture would prove nothing.
///
/// Mutation: rebuilding `live_decorations` from `snapshot.decorations` alone, without the records
/// carried from the moment the window closes, turns the last assertions red.
#[test]
fn a_formula_proven_inside_an_open_repaint_window_survives_its_close() {
    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(48), nz(10));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();

    // One formula, proven, and then scrolled off the screen — it stays off-band, which is what gives
    // a later repaint a window to open at all.
    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&synchronized_repaint(&[
        "header",
        "$$",
        r"\oint \mathbf{B} \cdot d\ell = \mu_0 I",
        "$$",
        "tail",
        "prompt> ",
    ]));
    session.feed_at(&first, start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );
    let gone = start + Duration::from_millis(300);
    session
        .feed_at(
            &synchronized_repaint(&["gone 0", "gone 1", "gone 2", "gone 3", "gone 4", "prompt> "]),
            gone,
        )
        .unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);

    // A new screen's text arrives, and a repaint begins before anything on it has been proven: the
    // window opens on the off-band record and its snapshot holds no live decoration at all.
    let at = start + Duration::from_millis(400);
    session
        .feed_at(
            &synchronized_repaint(&[
                "header",
                "$$",
                r"\nabla \cdot \mathbf{E} = \frac{\rho}{\varepsilon_0}",
                "$$",
                "tail",
                "prompt> ",
            ]),
            at,
        )
        .unwrap();
    session.feed_at(b"\x1b[?2026h\x1b[?25l\x1b[H", at).unwrap();

    // The block is proven and rastered while that window is open.
    session.advance_live_stability(at + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered,
        "the fixture never rendered its formula inside the window"
    );
    let detections = session.live_detection_count();

    // The repaint finishes. Its window closes, and what it was holding must not take the formula
    // that was proven under it down with it.
    session
        .feed_at(
            b"\x1b[?25h\x1b[?2026l",
            at + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(2),
        )
        .unwrap();
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered,
        "the repaint window's close threw away a formula proven while it was open"
    );
    assert_eq!(
        session.live_detection_count(),
        detections,
        "the formula must survive, not be detected all over again"
    );
    assert!(!oracle.flash_detected(), "sequence={:?}", oracle.frames());
}

/// The screen the mixed-coordinate fixtures below repaint away from: two byte-identical three-row
/// blocks at rows 1 and 5, neither of them proven yet, with three unique rows between and under
/// them for a mapping to anchor on.
const TWO_BLOCKS_BEFORE: &[&str] = &[
    "keep zero",
    "$$",
    "x^2",
    "$$",
    "keep one",
    "$$",
    "x^2",
    "$$",
    "keep two",
    "prompt> ",
];

/// The same screen four rows further down: the blocks now open at rows 5 and 9, and every unique
/// row moved by the same +4, so the mapping a close computes from it is exact and unambiguous.
const TWO_BLOCKS_AFTER: &[&str] = &[
    "head alpha",
    "head beta",
    "head gamma",
    "head delta",
    "keep zero",
    "$$",
    "x^2",
    "$$",
    "keep one",
    "$$",
    "x^2",
    "$$",
    "keep two",
    "prompt> ",
];

/// Prove one formula, then repaint it away, so the pane holds an off-band record and every later
/// repaint has something to open a window for. Returns nothing: what it leaves behind is the queue.
fn seed_one_off_band_record(
    session: &mut DualPlaneSession,
    projection: &mut bt_viewport::ViewportProjection,
    oracle: &mut FormulaFlashOracle,
    start: std::time::Instant,
    after: &[&str],
) {
    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&synchronized_repaint(&[
        "seed head",
        "$$",
        r"\oint \mathbf{B} \cdot d\ell = \mu_0 I",
        "$$",
        "seed tail",
        "prompt> ",
    ]));
    session.feed_at(&first, start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(session);
    assert_eq!(
        observe_frame(session, projection, oracle),
        FormulaFrameState::Rendered,
        "the seed formula never rendered"
    );
    session
        .feed_at(
            &synchronized_repaint(after),
            start + Duration::from_millis(300),
        )
        .unwrap();
    observe_frame(session, projection, oracle);
}

/// **A record proven on the grid a repaint already committed is seated where it says it is.**
///
/// A read that ends one DEC 2026 block and begins another leaves a synchronized-update deadline
/// standing at the end of the drain, so the repaint window that was open over the first block was
/// held open over the second — with the snapshot it took before the first block's cells, while
/// those cells were already on the glass. Detection between reads then proves blocks in the *new*
/// grid's coordinates, and the close pushed them through the old grid's delta a second time.
///
/// Two byte-identical blocks make the double shift visible rather than merely wrong: the +4 seats
/// the first carried record on the second's rows, where its source matches byte for byte, and the
/// second falls off the grid entirely. Neither the identity fallback (four `$$` rows tie its
/// origin vote) nor the bounded re-detection (two render-equivalent tasks, no unique match) can
/// recover it, so one of the two pictures is lost and its LaTeX is exposed.
///
/// Mutation: keeping the window open whenever any block is still buffering, rather than only while
/// the grid it snapshotted is still the grid on the glass, renders one block instead of two.
#[test]
fn a_block_proven_after_a_commit_reopened_the_window_keeps_its_own_coordinates() {
    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(48), nz(14));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();
    seed_one_off_band_record(
        &mut session,
        &mut projection,
        &mut oracle,
        start,
        TWO_BLOCKS_BEFORE,
    );

    // One read: the repaint's cells land unsynchronized, and the producer opens its next frame's
    // block in the same breath. The window has nothing left to preserve the old grid for.
    let at = start + Duration::from_millis(400);
    let mut commit_and_reopen = unsynchronized_repaint(TWO_BLOCKS_AFTER);
    commit_and_reopen.extend_from_slice(b"\x1b[?2026h\x1b[H");
    session.feed_at(&commit_and_reopen, at).unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);

    // Both blocks are proven and rastered while that second block is still buffering.
    session.advance_live_stability(at + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered,
        "the fixture never rendered its two blocks"
    );
    let detections = session.live_detection_count();

    // The second block ends without touching a cell. Nothing may move.
    session
        .feed_at(
            b"\x1b[?2026l",
            at + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(2),
        )
        .unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);
    session.refresh_projection(&mut projection);
    let settled = session.viewport_frame(&mut projection).unwrap();
    for row in [5usize, 6, 7, 9, 10, 11] {
        assert!(
            frame_row_text(&settled, row).trim().is_empty(),
            "row {row} still shows its LaTeX after the close: {:?}",
            (5..=11)
                .map(|row| frame_row_text(&settled, row))
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(
        session.live_detection_count(),
        detections,
        "both blocks must survive the close, not be detected all over again"
    );
    assert!(!oracle.flash_detected(), "sequence={:?}", oracle.frames());
}

/// The control the fixture above is measured against: **one block, opened once and never
/// interrupted, still carries its formulas across a real scroll.**
///
/// Here the cells are withheld for the whole life of the window, so every record the close carries
/// was proven against the grid the snapshot describes and the old grid's delta is exactly the right
/// thing to push them through. `CSI 4 S` moves both blocks up four rows inside the block, and both
/// pictures come out the other side.
#[test]
fn an_atomic_scroll_inside_one_block_carries_both_formulas_through_its_close() {
    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(48), nz(14));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();
    seed_one_off_band_record(
        &mut session,
        &mut projection,
        &mut oracle,
        start,
        TWO_BLOCKS_AFTER,
    );

    let at = start + Duration::from_millis(400);
    session.feed_at(b"\x1b[?2026h\x1b[?25l\x1b[H", at).unwrap();
    session.advance_live_stability(at + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered,
        "the fixture never rendered its two blocks inside the window"
    );
    let detections = session.live_detection_count();

    session
        .feed_at(
            b"\x1b[4S\x1b[?25h\x1b[?2026l",
            at + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(2),
        )
        .unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);
    session.refresh_projection(&mut projection);
    let settled = session.viewport_frame(&mut projection).unwrap();
    for row in [1usize, 2, 3, 5, 6, 7] {
        assert!(
            frame_row_text(&settled, row).trim().is_empty(),
            "row {row} shows its LaTeX after an atomic scroll: {:?}",
            (1..=7)
                .map(|row| frame_row_text(&settled, row))
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(
        session.live_detection_count(),
        detections,
        "an atomic scroll must not schedule detection again"
    );
    assert!(!oracle.flash_detected(), "sequence={:?}", oracle.frames());
}

/// **A resize ends the grid a window snapshotted, exactly as a commit does.**
///
/// The window here is opened by a block that goes on buffering across the resize, so the read that
/// closes it arrives after the reflow. `resize_at` takes its own snapshot, reflows, and reprojects
/// every record onto the new grid itself — correctly: both blocks land at rows 1 and 5. But the
/// window that was already open was left holding the snapshot of a grid the reflow had just
/// replaced, and its close then ran that old grid's delta over records already in the new grid's
/// coordinates. The two blocks are byte-identical, so the second shift seats the first on the
/// second's rows and the second is lost: two pictures become one, on a screen whose text did not
/// change between the resize and the close.
///
/// Mutation: leaving `alternate_repaint_snapshot` alone across the reflow, instead of rebasing it
/// onto the grid the resize settled, renders one block instead of two.
#[test]
fn a_resize_under_an_open_window_does_not_project_its_records_twice() {
    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(48), nz(14));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();
    seed_one_off_band_record(
        &mut session,
        &mut projection,
        &mut oracle,
        start,
        TWO_BLOCKS_AFTER,
    );
    let at = start + Duration::from_millis(400) + LIVE_MATH_STABLE_INTERVAL;
    session.advance_live_stability(at);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered,
        "the fixture never rendered its two blocks"
    );
    let detections = session.live_detection_count();

    // The producer opens its next frame's block; everything in it is withheld.
    session.feed_at(b"\x1b[?2026h\x1b[?25l\x1b[H", at).unwrap();

    // The reader drags the window shorter while that block is still buffering. The cursor is on the
    // last row, so the four rows that go come off the top and both blocks move up by four.
    session
        .resize_at(nz(48), nz(10), at + Duration::from_millis(20))
        .unwrap();
    projection = session.new_projection(session.layout_key());
    complete_live_math(&mut session);
    observe_frame(&mut session, &mut projection, &mut oracle);

    // The block ends without touching a cell. Nothing may move.
    session
        .feed_at(b"\x1b[?25h\x1b[?2026l", at + Duration::from_millis(40))
        .unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);
    session.refresh_projection(&mut projection);
    let settled = session.viewport_frame(&mut projection).unwrap();
    for row in [1usize, 2, 3, 5, 6, 7] {
        assert!(
            frame_row_text(&settled, row).trim().is_empty(),
            "row {row} shows its LaTeX after a resize under an open window: {:?}",
            (0..8)
                .map(|row| frame_row_text(&settled, row))
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(
        session.live_detection_count(),
        detections,
        "both blocks must survive the reflow, not be detected all over again"
    );
}

/// Both blocks are pictures, wherever the reflow has put them, and no row of the frame is left
/// showing their LaTeX. Said this way rather than by row number because a shrink the reader drags
/// back returns the rows the shrink scrolled away, and where the blocks end up is the vendor's
/// answer, not this fixture's.
fn assert_two_blocks_are_pictures(session: &mut DualPlaneSession, why: &str) {
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    let observation = bt_term::observe_formula_frame(&frame);
    assert_eq!(
        observation.rendered_sources.len(),
        2,
        "{why}: {} of 2 pictures; frame={:?}",
        observation.rendered_sources.len(),
        (0..frame.drawable_rows())
            .map(|row| frame_row_text(&frame, row))
            .collect::<Vec<_>>()
    );
    assert!(
        observation.source_rows.is_empty(),
        "{why}: a block's source is exposed: {:?}",
        observation.source_rows
    );
}

/// **The console host's reconcile installs a new grid, and the frame published on it carries the
/// pictures.**
///
/// A resize transaction reflows twice: once when the reader's gesture lands (`resize_at`) and once
/// when the host hands back the grid it decided on (`reconcile_resize_transaction_to_viewport`). The
/// second reflow projected the primary screen's records onto the installed grid and left the
/// alternate screen's where they were — so on the alternate screen the pictures went at the
/// reconcile and only came back at the next terminator, and every frame published in between showed
/// LaTeX. Rebasing the open window is not enough by itself and this is the fixture that says why:
/// a rebase re-takes the *snapshot*, and the records still have to be re-seated against the grid
/// that was just installed, before it is taken.
///
/// Both endings: a shrink, and a shrink the reader immediately drags back — the second replaces the
/// canonical grid a second time, and the pictures are on the rows the first reflow left them on.
#[test]
fn the_reconcile_that_installs_a_grid_reseats_the_pictures_on_it() {
    for grow_back in [false, true] {
        let start = std::time::Instant::now();
        let mut session = DualPlaneSession::new(nz(48), nz(14));
        let mut projection = session.new_projection(session.layout_key());
        let mut oracle = FormulaFlashOracle::default();
        seed_one_off_band_record(
            &mut session,
            &mut projection,
            &mut oracle,
            start,
            TWO_BLOCKS_AFTER,
        );
        let at = start + Duration::from_millis(400) + LIVE_MATH_STABLE_INTERVAL;
        session.advance_live_stability(at);
        complete_live_math(&mut session);
        assert_two_blocks_are_pictures(&mut session, "the fixture never rendered its two blocks");

        // The producer opens its next frame's block; everything in it is withheld.
        session.feed_at(b"\x1b[?2026h\x1b[?25l\x1b[H", at).unwrap();

        // The reader's gesture, which moves both blocks up by four.
        session
            .resize_at(nz(48), nz(10), at + Duration::from_millis(20))
            .unwrap();
        complete_live_math(&mut session);
        if grow_back {
            session
                .resize_at(nz(48), nz(14), at + Duration::from_millis(30))
                .unwrap();
            complete_live_math(&mut session);
        }
        let settled_rows = if grow_back { 14 } else { 10 };

        // The host hands back the grid it decided on. The frame published on it — before any
        // terminator arrives — has to carry the pictures.
        session.mark_pty_resize_requested_at(
            nz(48),
            nz(settled_rows),
            at + Duration::from_millis(40),
        );
        complete_live_math(&mut session);
        assert_two_blocks_are_pictures(
            &mut session,
            &format!(
                "the frame published on the installed grid shows LaTeX (grow_back={grow_back})"
            ),
        );

        // And the block ends without touching a cell.
        session
            .feed_at(b"\x1b[?25h\x1b[?2026l", at + Duration::from_millis(60))
            .unwrap();
        complete_live_math(&mut session);
        assert_two_blocks_are_pictures(
            &mut session,
            &format!("the terminator lost a picture (grow_back={grow_back})"),
        );
    }
}

/// **No raster is ever held over text it does not match, not even for the one read between a
/// commit and the next block's terminator.**
///
/// The owner's hard bar. A proven block is rendered inside an open window; one read then carries
/// the cursor move, a new body, the terminator that writes all three to the glass, and the next
/// frame's block start. The cells that landed say `y^2`; suppression skipped the invalidation those
/// cells would otherwise have caused, and settlement — seeing the *next* block's deadline — skipped
/// the projection that would have judged the record against them, so the frame went on showing the
/// `x^2` picture over a row that no longer says `x^2`.
///
/// Mutation: deciding the window is still open because a block is buffering, rather than because
/// the grid has not moved under it, leaves `held_unbacked_records` reporting that exact raster.
#[test]
fn a_body_rewritten_at_a_commit_never_keeps_the_picture_of_what_it_replaced() {
    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(48), nz(8));
    let mut projection = session.new_projection(session.layout_key());
    let mut oracle = FormulaFlashOracle::default();

    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&synchronized_repaint(&[
        "header", "$$", "x^2", "$$", "tail", "prompt> ",
    ]));
    session.feed_at(&first, start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        observe_frame(&mut session, &mut projection, &mut oracle),
        FormulaFrameState::Rendered
    );

    // The window opens over the proven block, and the block that opens it withholds its cells.
    let at = start + Duration::from_millis(300);
    session.feed_at(b"\x1b[?2026h\x1b[?25l\x1b[H", at).unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);

    // One read: the new body, the terminator that puts it on the glass, and the next block.
    session
        .feed_at(
            b"\x1b[3;1H\x1b[Ky^2\x1b[?25h\x1b[?2026l\x1b[?2026h\x1b[?25l\x1b[H",
            at + Duration::from_millis(20),
        )
        .unwrap();
    observe_frame(&mut session, &mut projection, &mut oracle);

    assert!(
        session.held_unbacked_records().is_empty(),
        "a raster survived the body it was made from: {:?}",
        session.held_unbacked_records()
    );
    session.refresh_projection(&mut projection);
    let committed = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(
        frame_row_text(&committed, 2).trim(),
        "y^2",
        "the committed body must be the text the frame carries"
    );
}

/// The screen both in-place body fixtures start from: `$$` / `x^2` / `$$` at rows 1-3 with a unique
/// row above and below, proven and rendered, on whichever screen the caller asks for.
fn session_with_one_proven_block(alternate: bool, start: std::time::Instant) -> DualPlaneSession {
    // Tall enough that a display block's box clears the primary screen's visible-text floor; the
    // alternate screen has no such rule, and the fixture wants the same screen on both sides.
    let mut session = DualPlaneSession::new(nz(48), nz(16));
    let mut first = Vec::new();
    if alternate {
        first.extend_from_slice(b"\x1b[?1049h");
        first.extend_from_slice(&synchronized_repaint(&[
            "header", "$$", "x^2", "$$", "tail", "prompt> ",
        ]));
    } else {
        first.extend_from_slice(b"header\r\n$$\r\nx^2\r\n$$\r\ntail\r\nprompt> ");
    }
    session.feed_at(&first, start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    session
}

fn assert_band_is_a_picture(session: &mut DualPlaneSession, why: &str) {
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    for row in 1usize..=3 {
        assert!(
            frame_row_text(&frame, row).trim().is_empty(),
            "{why}: row {row} shows its LaTeX; frame={:?}",
            (0..6)
                .map(|row| frame_row_text(&frame, row))
                .collect::<Vec<_>>()
        );
    }
}

/// **A block whose body is rewritten where it stands is typeset again.**
///
/// The producer writes one row — `CUP`, erase-to-EOL, a new body — and leaves the two `$$` rows
/// holding the bytes they already had. The old picture must go, because it is a picture of text
/// that is no longer there; and the formula that replaced it must be proven and drawn, because from
/// the reader's side nothing happened except that a formula changed.
///
/// Two things stood between the second half and the reader. Detection arms on a block's *opener*,
/// and `observe_live_damage` clears the candidate signature only of the rows that were written — so
/// the opener went on saying "a task for this row is already out" about an answer that had been
/// thrown away, and `invalidate_live_row` re-armed nothing when it threw it. And the signature the
/// opener carries hashes the delimiter rows, not the body, so even a re-armed opener would have
/// recognised the changed block as the one it had already answered.
#[test]
fn a_formula_body_rewritten_in_place_is_typeset_again_on_the_alternate_screen() {
    let start = std::time::Instant::now();
    let mut session = session_with_one_proven_block(true, start);
    assert_band_is_a_picture(&mut session, "the fixture never rendered its block");

    let rewritten = start + Duration::from_millis(300);
    session
        .feed_at(b"\x1b[3;1H\x1b[Ky^2\x1b[6;9H", rewritten)
        .unwrap();
    assert!(
        session.held_unbacked_records().is_empty(),
        "the picture of the body that was replaced is still being painted: {:?}",
        session.held_unbacked_records()
    );

    session.advance_live_stability(rewritten + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_band_is_a_picture(&mut session, "the replacement formula was never typeset");
    assert!(session.held_unbacked_records().is_empty());
}

/// The primary-screen half of the fixture above: the same one-row rewrite of a block standing in
/// the live grid under a transcript, where a removed record is dropped outright rather than held
/// off-band.
#[test]
fn a_formula_body_rewritten_in_place_is_typeset_again_on_the_primary_screen() {
    let start = std::time::Instant::now();
    let mut session = session_with_one_proven_block(false, start);
    assert_band_is_a_picture(&mut session, "the fixture never rendered its block");

    let rewritten = start + Duration::from_millis(300);
    session
        .feed_at(b"\x1b[3;1H\x1b[Ky^2\x1b[6;9H", rewritten)
        .unwrap();
    assert!(
        session.held_unbacked_records().is_empty(),
        "the picture of the body that was replaced is still being painted: {:?}",
        session.held_unbacked_records()
    );

    session.advance_live_stability(rewritten + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_band_is_a_picture(&mut session, "the replacement formula was never typeset");
    assert!(session.held_unbacked_records().is_empty());
}

/// The same rewrite arriving as a repaint, which is the other door into the same defect: the window
/// suppresses the record's teardown, so `invalidate_live_row` never sees the changed row at all, and
/// the close drops the record without re-arming anything. The replacement must still be typeset once
/// the screen goes quiet.
#[test]
fn a_formula_body_rewritten_by_a_repaint_is_typeset_again() {
    for alternate in [true, false] {
        let start = std::time::Instant::now();
        let mut session = session_with_one_proven_block(alternate, start);
        assert_band_is_a_picture(&mut session, "the fixture never rendered its block");

        let rewritten = start + Duration::from_millis(300);
        session
            .feed_at(
                &synchronized_repaint(&["header", "$$", "y^2", "$$", "tail", "prompt> "]),
                rewritten,
            )
            .unwrap();
        assert!(
            session.held_unbacked_records().is_empty(),
            "the picture of the body the repaint replaced is still being painted: {:?}",
            session.held_unbacked_records()
        );

        session.advance_live_stability(rewritten + LIVE_MATH_STABLE_INTERVAL);
        complete_live_math(&mut session);
        assert_band_is_a_picture(
            &mut session,
            "the formula the repaint wrote was never typeset",
        );
    }
}

/// The two screens the bottom-edge fixtures repaint between: one where the block that scrolled away
/// is nowhere, and one where it has scrolled back in on the last content row — the row the
/// application also draws its own "jump to bottom" chip on, below a fixed separator and prompt.
fn scrolled_away_screen() -> Vec<&'static str> {
    vec![
        "gone zero",
        "gone one",
        "gone two",
        "gone three",
        "",
        "────────────────",
        "prompt> ",
    ]
}

fn bottom_edge_screen(chip: bool) -> Vec<&'static str> {
    vec![
        "filler a",
        "filler b",
        "filler c",
        "11. section",
        if chip {
            r"$$e^{i\pi} + 1 = 0$$                 Jump to bottom (click)"
        } else {
            r"$$e^{i\pi} + 1 = 0$$"
        },
        "",
        "────────────────",
        "prompt> ",
    ]
}

/// Prove one block, then repaint it off the screen so its record is waiting off-band, and return the
/// session with the detector count taken at the moment it went.
fn session_with_one_off_band_block(start: std::time::Instant) -> DualPlaneSession {
    let mut session = DualPlaneSession::new(nz(100), nz(12));
    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&synchronized_repaint(&[
        "header",
        r"$$e^{i\pi} + 1 = 0$$",
        "tail",
        "",
        "────────────────",
        "prompt> ",
    ]));
    session.feed_at(&first, start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    assert!(
        frame_row_text(&frame, 1).trim().is_empty(),
        "the fixture never rendered its block"
    );
    session
        .feed_at(
            &synchronized_repaint(&scrolled_away_screen()),
            start + Duration::from_millis(300),
        )
        .unwrap();
    session
}

fn rendered_sources(session: &mut DualPlaneSession) -> Vec<String> {
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    let mut sources = bt_term::observe_formula_frame(&frame).rendered_sources;
    sources.sort();
    sources
}

/// **A picture is never put on a row the detector would not read as that block.**
///
/// The owner's own scrolling session, 2026-wrapped: a block scrolls back into view on the last
/// content row, and that row is the one the application draws its "jump to bottom" chip on. The
/// chip sits after the closing `$$`, so the detector does not read the row as a display block at
/// all — but the off-band re-anchor looked for the proven source as a *substring* of the grid, found
/// it, and seated the record there. The frame published a picture the detector disowns; the very
/// next pass over a byte-identical grid took it away again; and when the block finally scrolled up
/// one more row, onto a line of its own, it was proven and drawn for real. Read by read, that is one
/// picture appearing, vanishing and returning while the text underneath it never moved — which is
/// what the owner sees, and it is the last two flicker events of his recording.
///
/// Mutation: letting the re-anchor match a display block anywhere inside a row, rather than only
/// where the row is that block and nothing else, puts the raster back on the chip row and turns both
/// assertions red.
#[test]
fn a_block_sharing_its_row_with_the_application_is_not_re_anchored_onto_it() {
    let start = std::time::Instant::now();
    let mut session = session_with_one_off_band_block(start);

    let back = start + Duration::from_millis(600);
    session
        .feed_at(&synchronized_repaint(&bottom_edge_screen(true)), back)
        .unwrap();
    assert!(
        session.held_unbacked_records().is_empty(),
        "a raster was seated on a row the detector does not read as its block: {:?}",
        session.held_unbacked_records()
    );
    let published = rendered_sources(&mut session);

    // The same grid, one detection pass later. What a frame shows must not depend on which pass a
    // reader happens to be looking at.
    session.advance_live_stability(back + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_eq!(
        rendered_sources(&mut session),
        published,
        "the frame changed its mind about an unchanged grid"
    );
}

/// Prove one block of the caller's own source, then repaint it away so its record is waiting
/// off-band. The generalised form of `session_with_one_off_band_block`.
fn session_with_one_off_band_source(source: &str, start: std::time::Instant) -> DualPlaneSession {
    let mut session = DualPlaneSession::new(nz(100), nz(12));
    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&synchronized_repaint(&[
        "header",
        source,
        "tail",
        "",
        "────────────────",
        "prompt> ",
    ]));
    session.feed_at(&first, start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    assert!(
        frame_row_text(&frame, 1).trim().is_empty(),
        "the fixture never rendered {source:?}: {:?}",
        (0..6)
            .map(|row| frame_row_text(&frame, row))
            .collect::<Vec<_>>()
    );
    session
        .feed_at(
            &synchronized_repaint(&scrolled_away_screen()),
            start + Duration::from_millis(300),
        )
        .unwrap();
    session
}

/// Scroll the block back in on `line`, and say whether the frame typeset it there.
fn block_is_typeset_after_scrolling_back_onto(source: &str, line: &str) -> bool {
    let start = std::time::Instant::now();
    let mut session = session_with_one_off_band_source(source, start);
    let back = start + Duration::from_millis(600);
    session
        .feed_at(
            &synchronized_repaint(&[
                "filler a",
                "filler b",
                "filler c",
                "11. section",
                line,
                "",
                "────────────────",
                "prompt> ",
            ]),
            back,
        )
        .unwrap();
    assert!(
        session.held_unbacked_records().is_empty(),
        "a raster was seated on a row the detector does not read as its block: {:?}",
        session.held_unbacked_records()
    );
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    frame_row_text(&frame, 4).trim().is_empty()
}

/// **One rule, one owner: the re-anchor asks the detector, it does not carry its own copy of the
/// detector's rules.**
///
/// A whitespace test — "the match must cover the row apart from the space around it" — is a second
/// copy of "the detector owns a display block only when its line is that block", and a copy drifts.
/// These four are where it had already drifted. The detector skips a list marker and a heading
/// before the delimiter (`delimiter_start`, and Codex's own reflow prints `# $$…$$`), and holds
/// trailing prose punctuation out of a single-line environment (`complete_display_on_line`) — three
/// lines it owns that a whitespace test refuses, each of them a formula going back to LaTeX for a
/// repaint when it scrolls in. And it refuses a line indented four columns
/// (`commonmark_indented_code`) — a line a whitespace test happily accepts, which is the original
/// defect surviving inside indented code.
///
/// So the re-anchor runs the detector over the rows the match found and keeps the record only if it
/// gets this block back, at these rows, from this source. Nothing to keep in step.
#[test]
fn the_re_anchor_keeps_every_line_the_detector_reads_as_a_block() {
    const FORMULA: &str = r"$$e^{i\pi} + 1 = 0$$";
    const ENVIRONMENT: &str = r"\begin{pmatrix} a & b \end{pmatrix}";

    assert!(
        block_is_typeset_after_scrolling_back_onto(FORMULA, &format!("• {FORMULA}")),
        "a list item is a line the detector reads as a block"
    );
    assert!(
        block_is_typeset_after_scrolling_back_onto(FORMULA, &format!("# {FORMULA}")),
        "a heading is a line the detector reads as a block"
    );
    assert!(
        block_is_typeset_after_scrolling_back_onto(ENVIRONMENT, &format!("{ENVIRONMENT},")),
        "the detector holds trailing prose punctuation out of a single-line environment"
    );
}

/// **And the context the detector is given is the context the detector would have been in.**
///
/// Asking the detector about the rows a match found only answers honestly if it is started from the
/// parser state those rows really sit in, and that state comes from a walker that reads the lines
/// above them. The walker was a second copy of the scanner's structural rules and it had drifted: it
/// did not know that an unfinished `\begin{align}` is abandoned when a display block appears, so it
/// went on swallowing every line below as environment body and never saw the code fence three lines
/// down. Asked about a line inside that fence, it said there was no fence — and a block that the
/// detector reading the whole screen disowns was typeset anyway, then taken off again by the next
/// pass over an unchanged grid.
///
/// Mutation: dropping the swallow-radius bound from the shared structural step puts the picture back
/// on the fenced row, with `held_unbacked_records` naming it.
#[test]
fn a_block_inside_a_fence_below_an_unfinished_environment_is_not_re_anchored() {
    // Row 1 is what ends the unfinished environment's swallow, and there are two ways it can: a
    // display block of its own, and a complete environment on one line. The scanner has always read
    // both; the walker learned them one review apart, and each time the line below went unseen.
    for ender in ["$$z^2$$", r"\begin{align}x=y\end{align}"] {
        // And the fence below may be closed or still open when the screen is read.
        for closer in ["```", "more code"] {
            let start = std::time::Instant::now();
            let mut session = session_with_one_off_band_source(r"$$x^2$$", start);

            let back = start + Duration::from_millis(600);
            session
                .feed_at(
                    &synchronized_repaint(&[
                        r"\begin{align}",
                        ender,
                        "```",
                        "code",
                        "$$x^2$$",
                        closer,
                        "tail",
                        "prompt> ",
                    ]),
                    back,
                )
                .unwrap();
            assert!(
                session.held_unbacked_records().is_empty(),
                "a raster was seated inside a code fence ({ender:?}, {closer:?}): {:?}",
                session.held_unbacked_records()
            );
            let mut projection = session.new_projection(session.layout_key());
            session.refresh_projection(&mut projection);
            let frame = session.viewport_frame(&mut projection).unwrap();
            assert_eq!(
                frame_row_text(&frame, 4).trim(),
                "$$x^2$$",
                "the fenced row must keep its text ({ender:?}, {closer:?}): {:?}",
                (0..8)
                    .map(|row| frame_row_text(&frame, row))
                    .collect::<Vec<_>>()
            );

            // And it stays text once the screen settles: the picture is not put back a pass later.
            session.advance_live_stability(back + LIVE_MATH_STABLE_INTERVAL);
            complete_live_math(&mut session);
            session.refresh_projection(&mut projection);
            let settled = session.viewport_frame(&mut projection).unwrap();
            assert_eq!(frame_row_text(&settled, 4).trim(), "$$x^2$$");
            assert!(session.held_unbacked_records().is_empty());
        }
    }
}

/// The other side of the same rule: a line the detector refuses must not be re-anchored onto either,
/// and four columns of indentation is CommonMark's own way of saying "this is code, not prose".
#[test]
fn the_re_anchor_refuses_every_line_the_detector_reads_as_code() {
    const FORMULA: &str = r"$$e^{i\pi} + 1 = 0$$";

    assert!(
        !block_is_typeset_after_scrolling_back_onto(FORMULA, &format!("    {FORMULA}")),
        "four columns of indentation is an indented code block, not a formula"
    );
    assert!(
        !block_is_typeset_after_scrolling_back_onto(FORMULA, &format!("\t{FORMULA}")),
        "a tab is four columns of indentation"
    );
}

/// **And the detector's answer is the one the whole scan gives, not the one a line-at-a-time walk
/// of the same rows arrives at.**
///
/// The scan reads the whole line list, and two of its decisions need that: a `$$` is consumed as a
/// *clipped closer* when the opener it pairs with is above the window, and a phantom opener is
/// abandoned when the blocks below re-synchronise without it. A walker that sees one line at a time
/// can make neither, and it must stay that way — reading ahead is not its job and would cost it the
/// property that makes it cheap for the job it does have.
///
/// These rows are where the two part, and they part in the direction that matters. Row 1's `$$` has
/// no opener above it: the scan takes it as a clipped closer, moves on, meets the fence and disowns
/// row 4 as code. The walker opens `Dollars` at row 1 instead and swallows the fence whole, so it
/// hands the door a checkpoint that says "no fence here" — and the door seated a raster over
/// `$$x^2$$` inside a code block, took it off on the pass after, and put it back on the pass after
/// that.
///
/// Both fence variants, because a fence that is still open when the screen is read is the same
/// question as one that has closed.
#[test]
fn a_block_inside_a_fence_below_a_clipped_closer_is_not_re_anchored() {
    for closer in ["```", "more code"] {
        let start = std::time::Instant::now();
        let mut session = session_with_one_off_band_source(r"$$x^2$$", start);

        let back = start + Duration::from_millis(600);
        session
            .feed_at(
                &synchronized_repaint(&[
                    "x=y", "$$", "```", "code", "$$x^2$$", closer, "tail", "prompt> ",
                ]),
                back,
            )
            .unwrap();
        assert!(
            session.held_unbacked_records().is_empty(),
            "a raster was seated inside a code fence below a clipped closer ({closer:?}): {:?}",
            session.held_unbacked_records()
        );
        let mut projection = session.new_projection(session.layout_key());
        session.refresh_projection(&mut projection);
        let frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            frame_row_text(&frame, 4).trim(),
            "$$x^2$$",
            "the fenced row must keep its text ({closer:?}): {:?}",
            (0..8)
                .map(|row| frame_row_text(&frame, row))
                .collect::<Vec<_>>()
        );

        // And it stays text once the screen settles: the picture is not put back a pass later.
        session.advance_live_stability(back + LIVE_MATH_STABLE_INTERVAL);
        complete_live_math(&mut session);
        session.refresh_projection(&mut projection);
        let settled = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(frame_row_text(&settled, 4).trim(), "$$x^2$$");
        assert!(session.held_unbacked_records().is_empty());
    }
}

/// The control: the same block, the same re-anchor, on a row it has to itself. It must come back
/// without being detected again — tightening the re-anchor above must not cost the preservation it
/// exists for.
#[test]
fn a_block_scrolling_back_onto_a_row_of_its_own_is_re_anchored_without_re_detection() {
    let start = std::time::Instant::now();
    let mut session = session_with_one_off_band_block(start);
    let detections = session.live_detection_count();

    let back = start + Duration::from_millis(600);
    session
        .feed_at(&synchronized_repaint(&bottom_edge_screen(false)), back)
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    assert!(
        frame_row_text(&frame, 4).trim().is_empty(),
        "the off-band block was not re-anchored onto the row it has to itself: {:?}",
        (0..8)
            .map(|row| frame_row_text(&frame, row))
            .collect::<Vec<_>>()
    );
    assert!(session.held_unbacked_records().is_empty());
    assert_eq!(
        session.live_detection_count(),
        detections,
        "an exact re-anchor must not schedule detection again"
    );
}

/// The same edit, made while the block's *first* scan is still out.
///
/// A record's band is what says which rows an answer was read from, and before the first completion
/// lands there is no record — only a scan in flight, which carries the same knowledge in its own
/// dependency rows. The scan comes back describing a body that has since been replaced and is
/// refused, correctly; but the refusal only counted itself, and the opener's signature still claimed
/// an answer was out for that row. Nothing asked again, and the formula stayed at source for as long
/// as the screen did.
#[test]
fn a_body_edited_while_its_first_scan_is_in_flight_is_still_typeset() {
    for alternate in [true, false] {
        let start = std::time::Instant::now();
        let mut session = DualPlaneSession::new(nz(48), nz(16));
        let mut first = Vec::new();
        if alternate {
            first.extend_from_slice(b"\x1b[?1049h");
            first.extend_from_slice(&synchronized_repaint(&[
                "header", "$$", "x^2", "$$", "tail", "prompt> ",
            ]));
        } else {
            first.extend_from_slice(b"header\r\n$$\r\nx^2\r\n$$\r\ntail\r\nprompt> ");
        }
        session.feed_at(&first, start).unwrap();
        assert!(
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL) > 0,
            "the fixture scheduled no scan (alternate={alternate})"
        );

        // The work goes out to the renderer and stays there.
        let mut in_flight = Vec::new();
        while let Some(task) = session.take_math_worker_task() {
            let SessionMathTask::Live(mut task) = task else {
                panic!("the fixture unexpectedly scheduled frozen math");
            };
            let resolved = bt_detect::resolve_live_detection_task(&mut task);
            in_flight.push((resolved, task));
        }
        assert!(
            in_flight.iter().any(|(resolved, _)| *resolved),
            "the fixture never resolved its block (alternate={alternate})"
        );

        // While it is out, the producer replaces the body and puts the cursor back on its prompt.
        let rewritten = start + Duration::from_millis(300);
        session
            .feed_at(b"\x1b[3;1H\x1b[Ky^2\x1b[6;9H", rewritten)
            .unwrap();

        for (resolved, task) in in_flight {
            let accepted = if resolved {
                session.complete_live_worker_result(task, Ok(synthetic_raster(40, 40)))
            } else {
                session.complete_live_worker_result(task, Err(MathRenderError::NotDetected))
            };
            assert!(
                !(resolved && accepted),
                "a raster made from the body that was replaced was accepted (alternate={alternate})"
            );
        }

        session.advance_live_stability(rewritten + LIVE_MATH_STABLE_INTERVAL);
        complete_live_math(&mut session);
        assert_band_is_a_picture(
            &mut session,
            &format!("the replacement was never typeset (alternate={alternate})"),
        );
        assert!(session.held_unbacked_records().is_empty());
    }
}

/// **The stability interval belongs to the band, not to the row detection was armed on.**
///
/// An agent streams a formula's body in, a few characters at a time, while the two `$$` rows around
/// it sit still. Those rows have been quiet for a long time, so they read as stable; the scan they
/// arm goes out, comes back describing a body that has moved on, and is refused as `SourceChanged`.
/// Re-arming on that refusal is what makes the replacement get typeset at all — but re-arming on the
/// opener's own clock re-arms it while the thing it depends on is still moving, and the answer is
/// refused again, every frame, for as long as the stream lasts.
///
/// So a re-arm carries the newest damage in the band onto every row of it: the rows share one clock,
/// because they are one answer. The recovery is the same recovery it always was — the stream stops,
/// the band goes quiet for its interval, and the block is read once and typeset.
#[test]
fn a_body_streaming_into_a_block_does_not_schedule_a_scan_a_frame() {
    const FRAMES: u64 = 60;
    const FRAME: Duration = Duration::from_millis(16);
    /// **The clock's own ceiling, not the frame rate's.** A body that never settles cannot be read,
    /// and the stability interval's answer to that is to look again once it has been quiet for one;
    /// a stream that outlasts the interval turns that into one look per interval. Both `$$` rows of
    /// a block can arm a scan, so the rule's ceiling over this stream is
    /// `2 × (1 + 960 ms / 200 ms)` = 12, and 8 is what it actually costs — against 48 before, which
    /// was one a frame and would have been 480 at ten times the frame rate for the same second of
    /// output.
    const SCANS_DURING_THE_STREAM: u64 = 2
        * (1 + (FRAMES * FRAME.as_millis() as u64)
            .div_ceil(LIVE_MATH_STABLE_INTERVAL.as_millis() as u64));

    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(48), nz(16));
    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&synchronized_repaint(&[
        "header", "$$", "x", "$$", "tail", "prompt> ",
    ]));
    session.feed_at(&first, start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    let settled = session.live_detection_count();

    // Each frame: the body grows by a character, last frame's scan comes back describing the body
    // it was given, and whatever this frame armed goes out to the renderer.
    let mut in_flight: Vec<(bool, bt_detect::LiveDetectionTask)> = Vec::new();
    for frame in 0..FRAMES {
        let now = start + LIVE_MATH_STABLE_INTERVAL + FRAME * frame as u32;
        // A body that changes every frame without ever growing past the pane: an agent revising
        // what it is writing. One that only appended would wrap and stop being a three-row block,
        // which is a different fixture.
        session
            .feed_at(
                format!("\x1b[3;1H\x1b[Kx^{{{}}}\x1b[6;9H", frame % 9 + 1).as_bytes(),
                now,
            )
            .unwrap();
        session.advance_live_stability(now);
        let mut armed = Vec::new();
        while let Some(task) = session.take_math_worker_task() {
            let SessionMathTask::Live(mut task) = task else {
                panic!("the fixture unexpectedly scheduled frozen math");
            };
            let resolved = bt_detect::resolve_live_detection_task(&mut task);
            armed.push((resolved, task));
        }
        for (resolved, task) in std::mem::replace(&mut in_flight, armed) {
            if resolved {
                let _ = session.complete_live_worker_result(task, Ok(synthetic_raster(40, 40)));
            } else {
                let _ =
                    session.complete_live_worker_result(task, Err(MathRenderError::NotDetected));
            }
        }
    }
    let during = session.live_detection_count() - settled;
    eprintln!("G1_STREAMING_BODY frames={FRAMES} scans={during} budget={SCANS_DURING_THE_STREAM}");
    assert!(
        during <= SCANS_DURING_THE_STREAM,
        "a streaming body scheduled {during} scans over {FRAMES} frames, budget \
         {SCANS_DURING_THE_STREAM}"
    );
    // Said a second way, because it is the half a reader would recognise: the work does not follow
    // the frame rate. Drawing the same second of output twice as often must not cost twice as much.
    assert!(
        during * 4 < FRAMES,
        "a streaming body scheduled {during} scans over {FRAMES} frames — that is a scan a frame \
         in all but name"
    );

    // The stream stops. The band goes quiet for its interval, and the block is read and typeset.
    let quiet = start + LIVE_MATH_STABLE_INTERVAL + FRAME * FRAMES as u32;
    session.advance_live_stability(quiet + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_band_is_a_picture(&mut session, "the settled body was never typeset");
}

/// The control the two fixtures above are measured against: **a row rewritten with the bytes it
/// already had did not change, whoever is looking at it.**
///
/// Every row of the proven band is written again, byte for byte. The picture stays, and nothing is
/// detected a second time — the re-arming that the changed body needs must key on the content of a
/// row and not on the fact that someone wrote to it, or a full-screen program that repaints at
/// sixty frames a second would re-detect its whole screen sixty times a second.
#[test]
fn a_band_rewritten_with_the_bytes_it_already_had_keeps_its_picture() {
    for alternate in [true, false] {
        let start = std::time::Instant::now();
        let mut session = session_with_one_proven_block(alternate, start);
        assert_band_is_a_picture(&mut session, "the fixture never rendered its block");
        let detections = session.live_detection_count();

        let rewritten = start + Duration::from_millis(300);
        session
            .feed_at(
                b"\x1b[2;1H\x1b[K$$\x1b[3;1H\x1b[Kx^2\x1b[4;1H\x1b[K$$\x1b[6;9H",
                rewritten,
            )
            .unwrap();
        session.advance_live_stability(rewritten + LIVE_MATH_STABLE_INTERVAL);
        complete_live_math(&mut session);
        assert_band_is_a_picture(&mut session, "an unchanged band lost its picture");
        assert_eq!(
            session.live_detection_count(),
            detections,
            "an unchanged band was detected again (alternate={alternate})"
        );
    }
}

/// The screen the two mid-window retirement fixtures stand on: one display block at rows 1-3, a
/// unique row above it and a row below that a repaint can rewrite without touching the block.
const RETIRED_RECORD_SCREEN: &[&str] = &["header", "$$", "x^2", "$$", "tail", "spin-0", "prompt> "];

/// **A record retired while a repaint window is open does not come back when the window closes.**
///
/// The window's snapshot is a clone of the resident records, and its close projects that clone onto
/// the settled grid. So a removal that reaches only `live_decorations` is undone a few reads later,
/// and it is undone in exactly the case that matters: these removals are verdicts about rows whose
/// *text* did not change, so projection's exact-row proof passes and the raster goes back where it
/// was just ruled off.
///
/// Here the shell says so. One drain, two reads: a reprint opens the window, and the prompt cycle
/// that follows declares the rows the block stands on to be its command line — which §6.1.1 says no
/// picture may be injected into. The record is retired for that reason, and the close used to hand
/// it straight back, putting a formula's raster over the line the user types on.
#[test]
fn a_record_the_shell_claimed_as_its_command_line_does_not_survive_the_window() {
    let start = std::time::Instant::now();
    let mut session = DualPlaneSession::new(nz(48), nz(16));
    session
        .feed_at(RETIRED_RECORD_SCREEN.join("\r\n").as_bytes(), start)
        .unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert_band_is_a_picture(&mut session, "the fixture never rendered its block");

    let at = start + Duration::from_millis(300);
    session.begin_feed_turn();
    // The producer reprints its transcript: every row is written again, only the spinner changes,
    // and the block's own rows come back byte for byte. That opens the primary reprint window.
    let mut reprinted = RETIRED_RECORD_SCREEN.to_vec();
    reprinted[5] = "spin-1";
    session
        .feed_at(&unsynchronized_repaint(&reprinted), at)
        .unwrap();
    // The shell then reports a prompt cycle whose input line spans the block's rows.
    session
        .feed_at(
            b"\x1b[2;1H\x1b]133;A\x07\x1b]133;B\x07\x1b[5;1H\x1b]133;C\x07",
            at,
        )
        .unwrap();
    session.end_feed_turn();

    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(
        (1usize..=3)
            .map(|row| frame_row_text(&frame, row).trim().to_owned())
            .collect::<Vec<_>>(),
        vec!["$$".to_owned(), "x^2".to_owned(), "$$".to_owned()],
        "the window's close gave the command line its picture back: {:?}",
        (0..7)
            .map(|row| frame_row_text(&frame, row))
            .collect::<Vec<_>>()
    );
}

/// Prove one single-line block on the alternate screen, under a unique row a later read can turn
/// into a code fence without writing one byte of the block's own row.
fn alternate_session_with_one_proven_line(start: std::time::Instant) -> DualPlaneSession {
    let mut session = DualPlaneSession::new(nz(48), nz(12));
    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&synchronized_repaint(&[
        "header", r"$$x^2$$", "", "tail", "prompt> ",
    ]));
    session.feed_at(&first, start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    assert!(
        frame_row_text(&frame, 1).trim().is_empty(),
        "the fixture never rendered its block: {:?}",
        (0..5)
            .map(|row| frame_row_text(&frame, row))
            .collect::<Vec<_>>()
    );
    session
}

/// The other door into the same defect, on the other screen: **a block the detector has just
/// refused does not come back when the repaint window closes either.**
///
/// A code fence opens above the block. The block's own row is never written, so nothing invalidates
/// its record — but everything under the fence is code now, and the scan that the changed context
/// arms says so. That refusal retires the record while an alternate repaint window is open over it,
/// and the close used to project the window's own clone straight back: a raster over a row the
/// detector disowns, which `held_unbacked_records` is the name for.
#[test]
fn a_block_the_detector_refused_mid_window_does_not_survive_the_window() {
    let start = std::time::Instant::now();
    let mut session = alternate_session_with_one_proven_line(start);

    let fenced = start + Duration::from_millis(300);
    session.feed_at(b"\x1b[1;1H\x1b[K```", fenced).unwrap();

    // The repaint window opens over the record, and the scan lands while it is open.
    let opened = fenced + Duration::from_millis(10);
    session
        .feed_at(b"\x1b[?2026h\x1b[?25l\x1b[H", opened)
        .unwrap();
    session.advance_live_stability(opened + LIVE_MATH_STABLE_INTERVAL);
    complete_live_math(&mut session);
    assert!(
        session.held_unbacked_records().is_empty(),
        "the refused block was still holding its raster inside the window: {:?}",
        session.held_unbacked_records()
    );

    // The repaint ends. What the window was holding must not undo the refusal.
    session
        .feed_at(
            b"\x1b[?25h\x1b[?2026l",
            opened + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(2),
        )
        .unwrap();
    assert!(
        session.held_unbacked_records().is_empty(),
        "the window's close put the refused block's raster back: {:?}",
        session.held_unbacked_records()
    );
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    assert_eq!(
        frame_row_text(&frame, 1).trim(),
        r"$$x^2$$",
        "the fenced row must keep its text: {:?}",
        (0..5)
            .map(|row| frame_row_text(&frame, row))
            .collect::<Vec<_>>()
    );
}

/// The other half of the same rule, and the half that says what a retirement may *not* cost: **a
/// block retired and then proven again inside one window keeps the newer picture.**
///
/// Two scans of one row are in flight at once here, which the queue permits and the code's own
/// comment in `complete_live_worker_result` describes: the first went out while a fence stood above
/// the block, the fence went away, a second went out, and the loser arrives last. The loser's
/// refusal retires the record; the winner proves it again inside the same window. A retirement
/// written as a tombstone on the occurrence would take the winner down with the loser at the close.
#[test]
fn a_block_refused_and_proven_again_inside_one_window_keeps_the_newer_picture() {
    let start = std::time::Instant::now();
    let mut session = alternate_session_with_one_proven_line(start);

    let fenced = start + Duration::from_millis(300);
    session.feed_at(b"\x1b[1;1H\x1b[K```", fenced).unwrap();
    session.advance_live_stability(fenced + LIVE_MATH_STABLE_INTERVAL);
    let Some(SessionMathTask::Live(mut under_the_fence)) = session.take_math_worker_task() else {
        panic!("the fence never armed the block's row");
    };

    // The fence goes away again before that scan comes back, and a second scan goes out.
    let cleared = fenced + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(5);
    session.feed_at(b"\x1b[1;1H\x1b[Kheader", cleared).unwrap();
    session.advance_live_stability(cleared + LIVE_MATH_STABLE_INTERVAL);

    // The window opens, and both scans land inside it.
    let opened = cleared + LIVE_MATH_STABLE_INTERVAL + Duration::from_millis(5);
    session
        .feed_at(b"\x1b[?2026h\x1b[?25l\x1b[H", opened)
        .unwrap();
    assert!(
        !bt_detect::resolve_live_detection_task(&mut under_the_fence),
        "the scan taken under the fence was expected to refuse the block"
    );
    session.complete_live_worker_result(under_the_fence, Err(MathRenderError::NotDetected));
    complete_live_math(&mut session);

    session
        .feed_at(b"\x1b[?25h\x1b[?2026l", opened + Duration::from_millis(2))
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    assert!(
        frame_row_text(&frame, 1).trim().is_empty(),
        "the block proven again inside the window lost its picture at the close: {:?}",
        (0..5)
            .map(|row| frame_row_text(&frame, row))
            .collect::<Vec<_>>()
    );
    assert!(session.held_unbacked_records().is_empty());
}

/// The reads the two off-band gate fixtures spend on a screen that does not change: a program
/// redrawing the screen it is already showing, which is what a full-screen agent does on every
/// keystroke.
const IDENTICAL_REPAINTS: u64 = 16;

/// **Whether a record can be re-anchored on a grid is a question about that grid, so it is asked
/// once per grid and not once per read.**
///
/// The off-band re-anchor runs from every feed turn and its only early-out was an empty queue, so a
/// record whose proven source is nowhere on the screen was re-asked on every read: the whole grid
/// read into a fresh detection context, a parser walk over it, a substring search and the
/// detector's own verdict, all to reach the same answer as the read before. Under a program that
/// repaints its screen per keystroke that is paid per keystroke, and the owner's recording sat with
/// six such records at once.
///
/// Sixteen repaints of the screen that is already on the glass. Every one of them writes every row,
/// and not one of them changes a cell, so the question is the same sixteen times over.
#[test]
fn an_unplaceable_off_band_record_is_asked_about_once_per_screen_and_not_once_per_read() {
    let start = std::time::Instant::now();
    let mut session = session_with_one_off_band_block(start);
    let settled = start + Duration::from_millis(400);
    session
        .feed_at(&synchronized_repaint(&scrolled_away_screen()), settled)
        .unwrap();
    let passes = session.offscreen_restore_pass_count();
    assert!(
        passes > 0,
        "the fixture never ran the re-anchor at all, so it measures nothing"
    );

    for repaint in 0..IDENTICAL_REPAINTS {
        session
            .feed_at(
                &synchronized_repaint(&scrolled_away_screen()),
                settled + Duration::from_millis(10 + repaint),
            )
            .unwrap();
    }
    assert_eq!(
        session.offscreen_restore_pass_count(),
        passes,
        "{IDENTICAL_REPAINTS} repaints of an unchanged screen re-asked a question whose answer \
         cannot have changed"
    );
}

/// The other side of that rule, and the one that says what remembering the answer may not cost:
/// **the record comes back in the very read that brings its source back.**
///
/// This is why the gate is the question and not a retry count or a clock: a record whose source
/// scrolls back into view has to be found then and there, however many reads it spent waiting. The
/// sixteen repaints above leave the memo standing, and the read after them — the one that puts the
/// block back on a row of its own — must typeset it in the frame it publishes, with no pass of
/// latency and nothing detected again.
#[test]
fn an_off_band_record_comes_back_in_the_read_that_brings_its_source_back() {
    let start = std::time::Instant::now();
    let mut session = session_with_one_off_band_block(start);
    let settled = start + Duration::from_millis(400);
    for repaint in 0..IDENTICAL_REPAINTS {
        session
            .feed_at(
                &synchronized_repaint(&scrolled_away_screen()),
                settled + Duration::from_millis(repaint),
            )
            .unwrap();
    }
    let detections = session.live_detection_count();

    let back = settled + Duration::from_millis(100);
    session
        .feed_at(&synchronized_repaint(&bottom_edge_screen(false)), back)
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    assert!(
        frame_row_text(&frame, 4).trim().is_empty(),
        "the block was not re-anchored in the read that brought its source back: {:?}",
        (0..8)
            .map(|row| frame_row_text(&frame, row))
            .collect::<Vec<_>>()
    );
    assert!(session.held_unbacked_records().is_empty());
    assert_eq!(
        session.live_detection_count(),
        detections,
        "an exact re-anchor must not schedule detection again"
    );
}

/// **A shell marker changes what every row on the screen is eligible to be, and writes no cell.**
///
/// Each row's site is read off its own cells, and the per-cell claim is in the row fingerprint, so
/// a cell whose provenance changed moves the content revision like any other change. But whether
/// this session listens to a shell on this screen at all is asked once for the whole grid, and that
/// answer flips when an OSC 133 marker is heard — a few bytes that touch nothing. Every row's site
/// moves with it while no cell, no generation, no checkpoint and no transcript line does, so a memo
/// that does not hold that answer can speak for a grid whose rows have just changed what they are.
///
/// Counted rather than looked at, because counting is what the memo decides: the marker's read must
/// ask the off-band question again, and the read after it — which changes nothing at all — must
/// not.
#[test]
fn a_shell_marker_alone_re_asks_the_off_band_question() {
    let start = std::time::Instant::now();
    let mut session = session_with_one_off_band_block(start);
    let settled = start + Duration::from_millis(400);
    for repaint in 0..2 {
        session
            .feed_at(
                &synchronized_repaint(&scrolled_away_screen()),
                settled + Duration::from_millis(repaint),
            )
            .unwrap();
    }
    let passes = session.offscreen_restore_pass_count();

    // A prompt cycle begins. Not one cell is written.
    session
        .feed_at(
            b"\x1b]133;A\x07\x1b]133;B\x07",
            settled + Duration::from_millis(10),
        )
        .unwrap();
    assert_eq!(
        session.offscreen_restore_pass_count(),
        passes + 1,
        "the rows changed what they are eligible to be and the off-band question was not re-asked"
    );

    // And the read after it, which changes nothing at all, still skips.
    session
        .feed_at(
            &synchronized_repaint(&scrolled_away_screen()),
            settled + Duration::from_millis(20),
        )
        .unwrap();
    assert_eq!(
        session.offscreen_restore_pass_count(),
        passes + 1,
        "an unchanged screen re-asked a question whose answer cannot have changed"
    );
}
