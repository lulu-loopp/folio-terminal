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
