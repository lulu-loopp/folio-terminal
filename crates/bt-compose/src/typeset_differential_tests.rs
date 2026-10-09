//! **Old against new, while both exist** (design T-COMPOSE-CRATE §6.1: temporary migration
//! evidence, deleted with the old path). Every math task a corpus of screens files is typeset by
//! `bt_term`'s functions and by this crate's; the answers, the tasks they leave behind and the
//! frames drawn from them must be the same.

use std::num::{NonZeroI64, NonZeroU32};

use bt_doc::{
    SUBPIXELS_PER_PX,
    math::{MathRaster, MathRenderError},
};
use bt_math::MathEngine;
use bt_term::{DualPlaneSession, LIVE_MATH_STABLE_INTERVAL, SessionMathTask};
use bt_viewport::ViewportProjection;
use web_time::Instant;

const INK: [u8; 3] = [0xd8, 0xdc, 0xe8];

/// Screens whose formulas are typeset: display and inline, command output and the alternate
/// screen, mixed script, a fraction that must be fitted to its row, a run too wide for its
/// cells beside one that fits, a source that does not compile, and a proven table.
const SCREENS: &[&str] = &[
    "\u{516c}\u{5f0f}\r\n$$\\int_0^1 x^2\\,dx = \\frac{1}{3}$$\r\n",
    "\x1b]133;A\x07> \x1b]133;B\x07show\x1b]133;C\x07\r\n\u{80fd}\u{91cf} $E = mc^2$ \u{6d4b}\u{8bd5}\r\nratio $\\dfrac{a}{b}$ here\r\n\x1b]133;D;0\x07\x1b]133;A\x07> \x1b]133;B\x07",
    "\x1b[?1049h energy $E = mc^2$ here and $\\sum_{n=1}^{\\infty} \\frac{1}{n^2}$\r\n",
    "\x1b]133;A\x07> \x1b]133;B\x07show\x1b]133;C\x07\r\nw $\\left(\\sum_{i=1}^{100} x_i y_i z_i w_i\\right)^{2}$ x $y$\r\n\x1b]133;D;0\x07\x1b]133;A\x07> \x1b]133;B\x07",
    "$$\\frac{a}{b} + \\undefinedmacro{x}$$\r\n",
    "$$\r\n\\begin{pmatrix} 1 & 2 \\\\ 3 & 4 \\end{pmatrix}\r\n$$\r\n",
    "| \u{540d} | value |\r\n|---|---|\r\n| a | 1 |\r\n| b | 2 |\r\n",
];

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn subpixels(px: i64) -> NonZeroI64 {
    NonZeroI64::new(px * SUBPIXELS_PER_PX).unwrap()
}

fn pane_of() -> (DualPlaneSession, ViewportProjection) {
    bt_term::install_test_host_names();
    let mut session = DualPlaneSession::new(nz(60), nz(10));
    session.set_font_size_subpixels(subpixels(20));
    session.set_cell_height_subpixels(subpixels(24));
    session.set_cell_width_subpixels(subpixels(12));
    session.set_ascii_baseline_subpixels(subpixels(19));
    let view = session.new_projection(session.layout_key());
    (session, view)
}

/// Everything of an answer but the time it took, which no two runs share.
fn without_time(
    result: &Result<MathRaster, MathRenderError>,
) -> Result<MathRaster, MathRenderError> {
    result.clone().map(|mut raster| {
        raster.render_time = std::time::Duration::ZERO;
        raster
    })
}

/// The task as the typesetter leaves it: the band it settled and whether it was resolved.
fn left_behind(task: &SessionMathTask) -> String {
    match task {
        SessionMathTask::Frozen(task) => format!("{} {:?}", task.resolved, task.span.render_source),
        SessionMathTask::Live(task) => format!(
            "{} {} {} {:?}",
            task.resolved, task.band_start_row, task.band_end_row, task.span.render_source
        ),
    }
}

/// Answer every math task of two identical sessions, the old way in one and the new way in the
/// other, and say how many tasks there were.
fn answer_both(
    engine: &MathEngine,
    old: &mut DualPlaneSession,
    new: &mut DualPlaneSession,
) -> usize {
    let mut answered = 0;
    while let Some(mut old_task) = old.take_math_worker_task() {
        let mut new_task = new
            .take_math_worker_task()
            .expect("the same work was filed");
        let old_result = match &mut old_task {
            SessionMathTask::Frozen(task) => bt_term::render_detection_task(engine, task, INK),
            SessionMathTask::Live(task) => bt_term::render_live_detection_task(engine, task, INK),
        };
        let new_result = crate::typeset(engine, &mut new_task, INK);
        assert_eq!(without_time(&old_result), without_time(&new_result));
        assert_eq!(left_behind(&old_task), left_behind(&new_task));
        match (old_task, new_task) {
            (SessionMathTask::Frozen(old_task), SessionMathTask::Frozen(new_task)) => {
                old.complete_worker_result(old_task, old_result);
                new.complete_worker_result(new_task, new_result);
            }
            (SessionMathTask::Live(old_task), SessionMathTask::Live(new_task)) => {
                old.complete_live_worker_result(old_task, old_result);
                new.complete_live_worker_result(new_task, new_result);
            }
            _ => panic!("the two sessions filed different kinds of work"),
        }
        answered += 1;
    }
    assert!(new.take_math_worker_task().is_none());
    answered
}

/// RED — **the moved typesetting answers exactly as `bt-term`'s did**, task by task and frame by
/// frame, over every screen of the corpus, live and then scrolled into history.
///
/// MUTATION: in this crate's `terminal_math_render_key`, key display math at 11 pt instead of
/// 12 — every display answer differs and the first assertion goes red.
#[test]
fn the_moved_typesetting_answers_as_bt_term_did() {
    let engine = MathEngine::new();
    let mut tasks = 0;
    let mut frames = 0;
    let mut pictures = 0;
    for screen in SCREENS {
        let start = Instant::now();
        let (mut old, mut old_view) = pane_of();
        let (mut new, mut new_view) = pane_of();
        for (session, view) in [(&mut old, &mut old_view), (&mut new, &mut new_view)] {
            session.feed_at(screen.as_bytes(), start).unwrap();
            session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
            session.refresh_projection(view);
            let frame = session.viewport_frame(view).unwrap();
            session.schedule_visible_artifacts(&frame);
        }
        tasks += answer_both(&engine, &mut old, &mut new);
        old.refresh_projection(&mut old_view);
        new.refresh_projection(&mut new_view);
        let old_frame = old.viewport_frame(&mut old_view).unwrap();
        let new_frame = new.viewport_frame(&mut new_view).unwrap();
        pictures += new_frame.math_blocks.len();
        assert!(old_frame == new_frame, "{screen:?}: the live frames differ");
        frames += 1;
        // Into history: enough lines to scroll the screen away, then the top of it again.
        let later = start + LIVE_MATH_STABLE_INTERVAL * 2;
        for (session, view) in [(&mut old, &mut old_view), (&mut new, &mut new_view)] {
            session
                .feed_at("\r\n\u{586b}\u{5145}".repeat(12).as_bytes(), later)
                .unwrap();
            view.scroll_to_top();
            session.refresh_projection(view);
            let frame = session.viewport_frame(view).unwrap();
            session.schedule_visible_artifacts(&frame);
        }
        tasks += answer_both(&engine, &mut old, &mut new);
        for at_bottom in [false, true] {
            if at_bottom {
                old_view.scroll_to_bottom();
                new_view.scroll_to_bottom();
            }
            old.refresh_projection(&mut old_view);
            new.refresh_projection(&mut new_view);
            let old_frame = old.viewport_frame(&mut old_view).unwrap();
            let new_frame = new.viewport_frame(&mut new_view).unwrap();
            assert!(old_frame == new_frame, "{screen:?}: the frames differ");
            frames += 1;
        }
    }
    assert!(tasks >= SCREENS.len(), "the corpus filed work: {tasks}");
    assert!(pictures > 0, "and some of it became pictures");
    println!(
        "CC6B_DIFFERENTIAL screens={} tasks={tasks} frames={frames} pictures={pictures} answers=byte-equal",
        SCREENS.len()
    );
}
