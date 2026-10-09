//! **`preview_text`, as the application drives it.** Tests whose first assertion is about
//! `preview_text`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    RAIL_FAILED_THEN_PROMPT, focused_frame, leaf_saying, rail_test_body, resolve_focused_pane,
    squeezed_body, tab_holding,
};
use std::time::Duration;

/// PIN (user ruling 2026-08-25; `docs/DESIGN.md` §7.10 ⑥) — **a glance card's
/// page is drawn once per version of the file, and a pointer coming back is
/// answered by a `stat`.**
///
/// The window keeps one rastered page and does not throw it away when the
/// card comes down, because a parse and a rasterisation are worth more than
/// the third of a megabyte they produce. What makes that safe is this
/// function and nothing else: the file is on a disk somebody else is also
/// writing to, so every card asks again, and the *worker* — never the thread
/// that draws, where a network `stat` can block for seconds — decides whether
/// the answer is new pixels or the word `Unchanged`.
///
/// RED GATE ①: drop the `known == mtime` arm and the second assertion comes
/// back `Drawn`, which is the whole cache doing nothing.
/// RED GATE ②: compare only `known.is_some()` — or compare the two without
/// requiring `known` to be `Some` — and the last case says a file that has
/// been rewritten is unchanged, which is a card showing yesterday's report.
#[test]
fn a_hovered_page_is_drawn_once_per_version_of_its_file() {
    let dir = bt_testpath::temp_path("bt-peek-page");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join("report.pdf");
    std::fs::write(
        &path,
        include_bytes!("../../../tests/assets/folio-pdf-test.pdf"),
    )
    .expect("the fixture is copied where it can be re-stamped");
    let fit = (280_u32, 160_u32);

    let PeekPageOutcome::Drawn { mtime, raster } = raster_peek_page(&path, 0, fit, None) else {
        panic!("a window holding no pixels is answered with pixels");
    };
    let first = raster.expect("and a real PDF draws");
    let mtime = mtime.expect("carrying the time the file said it was written");
    assert!(first.width <= fit.0 && first.height <= fit.1);

    // The same file at the same stamp: what the window is holding is still
    // the file's, and nothing is parsed, rendered or sent.
    assert!(
        matches!(
            raster_peek_page(&path, 0, fit, Some(mtime)),
            PeekPageOutcome::Unchanged
        ),
        "a re-hover costs one metadata call"
    );

    // Written since — the same bytes under a later stamp, which is exactly
    // what an editor that rewrote the report leaves behind.
    let later = mtime + Duration::from_secs(10);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("the scratch file opens")
        .set_modified(later)
        .expect("and takes a later stamp");
    let PeekPageOutcome::Drawn {
        mtime: seen,
        raster,
    } = raster_peek_page(&path, 0, fit, Some(mtime))
    else {
        panic!("a file written since the pixels were drawn is drawn again");
    };
    assert_eq!(seen, Some(later), "and the new stamp comes home with it");
    assert_eq!(
        raster.map(|raster| (raster.width, raster.height)),
        Some((first.width, first.height))
    );

    // A file that is not there at all is `Drawn` with nothing in it rather
    // than `Unchanged`: the card must lose the page it was showing, not keep
    // the previous file's.
    std::fs::remove_file(&path).expect("the scratch file goes");
    let PeekPageOutcome::Drawn { mtime, raster } = raster_peek_page(&path, 0, fit, Some(later))
    else {
        panic!("a file that has gone is not 'unchanged'");
    };
    assert_eq!(mtime, None);
    assert_eq!(raster, None);
    std::fs::remove_dir_all(&dir).ok();
}

/// RED (47) — **A resize present owed to a leaf accepts the frame the rail's arrival
/// reprojected.**
///
/// The owner's Mac stopped within two seconds of every launch with `Folio stopped: resize
/// presentation requires the newly projected grid: expected 109x16, got 107x16`. The restore
/// resized the window while it was not yet on screen, so the resize present was owed and nothing
/// could pay it; zsh's first prompt mark then arrived, the pane took the command rail's resting
/// reserve out of its grid (ticket 32) and composed a frame at the narrower grid — the right
/// frame — and the gate, which had recorded the grid at the moment of the resize, refused it. The
/// gate's claim is that no frame composed before a resize reaches the glass after it, which is a
/// claim about the grid the pane has *now*; so the gate reads it at validation.
///
/// MUTATION: make `TabState::owe_resize_present` record the focused leaf's grid and
/// `TabState::admit_resize_present` compare the frame against that record instead of the leaf's
/// grid — red at the reprojected frame. Or drop the `publish_frame` after the rail's re-solve in
/// `Runtime::drain_pty` — red on the pin at the end.
#[test]
fn a_resize_present_owed_to_a_leaf_accepts_the_frame_the_rails_arrival_reprojected() {
    let mut fonts = bt_render::preview_measure_font_system();
    let metrics = bt_render::CellMetrics::measure(&mut fonts, 2.0).unwrap();
    let body = rail_test_body(1105, 2.0);
    let mut tab = tab_holding(leaf_saying(""));
    // The restore resizes the window: the pane is re-solved and the present is owed.
    resolve_focused_pane(&mut tab, &metrics, body);
    tab.owe_resize_present();
    let unreserved = tab.focused().unwrap().grid;
    assert_eq!(unreserved, metrics.grid_for_pixels(body.width, body.height));
    let resized = focused_frame(&mut tab);
    tab.admit_resize_present(&resized)
        .expect("the frame the resize composed carries the resized grid");

    // Before any present lands, the shell's first mark arrives and the pane makes room for the
    // rail through the solve every geometry change takes.
    let leaf = tab.focused_mut().unwrap();
    leaf.session
        .feed(RAIL_FAILED_THEN_PROMPT.as_bytes())
        .expect("feed the shell's first prompt");
    assert!(
        leaf.hear_first_mark(),
        "the first mark gives the pane its rail"
    );
    resolve_focused_pane(&mut tab, &metrics, body);
    let reserved = tab.focused().unwrap().grid;
    assert!(
        reserved.columns < unreserved.columns,
        "the fixture's width really loses columns to the rail's reserve"
    );
    assert!(
        tab.resize_present_owed,
        "nothing has paid the resize present yet"
    );
    let reprojected = focused_frame(&mut tab);
    assert!(frame_matches_grid(&reprojected, reserved));
    tab.admit_resize_present(&reprojected)
        .expect("the frame the rail's arrival reprojected is the one the pane has now");

    // A frame of a grid the pane no longer has is still refused, in the log's own sentence.
    let refused = tab
        .admit_resize_present(&resized)
        .expect_err("the frame composed before the reserve carries a grid the pane has left");
    assert_eq!(
        refused.to_string(),
        format!(
            "resize presentation requires the newly projected grid: expected {}x{}, got {}x{}",
            reserved.columns, reserved.rows, unreserved.columns, unreserved.rows
        )
    );

    // And the drain composes that frame on the turn the reserve is taken, as every other road
    // that re-solves the panes does, so the frame a redraw takes from the slot is never the one
    // composed before it.
    assert!(
        squeezed_body("Runtime", "drain_pty").contains(concat!(
            "self.resize_leaves_to_layout(now,\"reservethecommandrail'sroom\")?;",
            "ifself.focused().map(|leaf|leaf.grid)!=focused_grid{self.publish_frame(",
        )),
        "the rail's re-solve publishes a frame of the grid the pane has now"
    );
}
