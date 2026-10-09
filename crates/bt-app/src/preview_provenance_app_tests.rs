//! **`preview_provenance`, as the application drives it.** Tests whose first assertion is about
//! `preview_provenance`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{cross_tab, grid_of, source_block};
use std::time::Duration;

// ── T5: the caret and the keys on the rendered face (§7.1.3t) ───────────

/// **A press inside the source block lands on the byte it pointed at**
/// (T5 ①, §7.1.3t) — the painter's arithmetic read backwards, through the
/// same fold.
///
/// The source block pushes no [`PreviewTextSite`]s, so this is the *only*
/// reading that can answer a press inside it: [`preview_text_box_at`] would
/// hand back the nearest piece it does have boxes for, which is the
/// paragraph above or below, and clicking into the block you are editing
/// would put the caret in its neighbour.
///
/// MUTATION: divide by the page's line height instead of the block's own and
/// every press below the first row of a block lands a row or two out — the
/// two faces are set in different sizes and only one of them drew this.
#[test]
fn a_press_inside_the_source_block_names_the_byte_it_pointed_at() {
    let content = "# head\n\none\ntwo\n";
    let source = source_block(1, 8, "one\ntwo");
    assert_eq!(
        preview_live::block_source(content, &source.range),
        source.text,
        "the fixture is the block the document would have cut",
    );
    let box_of_block = [100.0, 40.0, 500.0, 80.0];
    let at = |x: f32, y: f32| markdown_source_offset_at(&source, box_of_block, x, y);
    assert_eq!(at(100.0, 44.0), 8, "the block's first byte");
    assert_eq!(at(116.0, 44.0), 10, "two columns into its first line");
    assert_eq!(
        at(490.0, 44.0),
        11,
        "past the end of a short line is the end of that line",
    );
    assert_eq!(at(100.0, 65.0), 12, "the second row is the second line");
    assert_eq!(
        at(100.0, 4000.0),
        12,
        "and below the block is its last row, because the press has already \
             been judged to be this block's",
    );
    assert_eq!(at(100.0, 0.0), 8, "as above it is its first");
}

/// PIN — U12. **Every pane's decoration work is collected, not just the keyboard's.**
///
/// A leaf queues its own worker tasks as its own bytes arrive — that half was always per pane.
/// Collecting them was not: dispatch went through the tab's `Deref`, asked the focused leaf for
/// its queue, and left every other pane's work sitting where it was written. The symptom was
/// not a slow pane but a silent one. A file reference wears its resting dotted underline only
/// once the worker has *verified* the file, so an unfocused pane's references stayed bare
/// forever — until a hover opened the same file by the peek's road and the dots appeared, which
/// is what made the affordance look like something hovering granted rather than something every
/// pane is owed.
///
/// Two shells, both naming a file, only one holding the keyboard. Dispatch the tab and read the
/// wire: both seats must be addressed. Route it through `tab.session` again and the unfocused
/// seat never appears.
#[test]
fn every_pane_of_a_tab_hands_its_decoration_work_to_the_worker() {
    let directory = bt_testpath::temp_path("bt-leaf-dispatch-pin");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("shot.png");
    std::fs::write(&path, [0u8; 16]).unwrap();

    let mut tab = cross_tab(1, &["one", "two"]);
    let seats = tab.seats.terminals();
    assert_eq!(seats.len(), 2, "the fixture is a split tab");
    let started = Instant::now();
    for (_, leaf) in tab.leaves_mut() {
        leaf.session.set_math_layout_options(MathLayoutOptions {
            detect_image_paths: true,
            ..MathLayoutOptions::default()
        });
        // Wide enough that the absolute path is one unwrapped line for the detector — and
        // carried through the real transaction, because a resize left open withholds every
        // decoration for as long as it stays open (`decorations_allowed`).
        let local_grid = leaf.grid;
        let integration = leaf.integration;
        commit_leaf_resize(
            &mut leaf.session,
            None,
            ResizeReanchor {
                pending: &mut leaf.pending_psreadline_resize_reanchor,
                integration,
            },
            ReleaseGrids {
                local: local_grid,
                conpty: leaf.conpty_grid,
                next: grid_of(200, 8),
            },
            PhysicalSize::new(1600, 200),
            started,
        )
        .unwrap();
        leaf.grid = grid_of(200, 8);
        leaf.conpty_grid = grid_of(200, 8);
        let settled = leaf
            .session
            .resize_finish_deadline()
            .expect("the committed resize arms its own quiescence");
        leaf.session.finish_resize_if_quiescent(settled).unwrap();
        leaf.session
            .feed_at(
                format!("[Image: source: \"{}\"]\r\nprompt", path.display()).as_bytes(),
                settled,
            )
            .unwrap();
        leaf.session
            .advance_live_stability(settled + Duration::from_secs(1));
    }

    let (math, requests) = mpsc::channel();
    let (scale, _scale_requests) = mpsc::channel();
    let (path, _path_requests) = mpsc::channel();
    let (foreground, _foreground_requests) = mpsc::channel();
    let senders = DecorationSenders {
        math,
        scale,
        path,
        foreground,
    };
    let mut running = true;
    let mut notice_pending = false;
    assert!(
        !dispatch_tab_decoration_tasks(
            WindowId::from(1_u64),
            &mut tab,
            &senders,
            Instant::now(),
            &mut running,
            &mut notice_pending,
        ),
        "a live worker is not downgraded by an ordinary dispatch"
    );

    let addressed = requests
        .try_iter()
        .map(|request| match request {
            MathWorkerRequest::Math { leaf, .. }
            | MathWorkerRequest::InlineImage { leaf, .. }
            | MathWorkerRequest::PeekImage { leaf, .. }
            | MathWorkerRequest::PeekVideoFrame { leaf, .. }
            | MathWorkerRequest::PeekAnimation { leaf, .. }
            | MathWorkerRequest::AnimationFill { leaf, .. }
            | MathWorkerRequest::PeekPage { leaf, .. }
            | MathWorkerRequest::PreviewMath { leaf, .. } => leaf,
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        addressed,
        seats
            .iter()
            .map(|seat| ShellAddress {
                window: WindowId::from(1_u64),
                leaf: LeafId {
                    tab: tab.id,
                    seat: *seat
                },
            })
            .collect::<std::collections::BTreeSet<_>>(),
        "every pane's file reference reaches the worker under its own seat — and under its own \
             window, because the answer comes home on a channel every window can see"
    );
}
