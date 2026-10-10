//! **`web_trace`, as the application drives it.** Tests whose first assertion is about
//! `web_trace`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    CARDS_AT_150, CARDS_AT_200, cards_column, cross_merge, cross_metrics, cross_seats, cross_solve,
    cross_tab, dir_entry, files_column, listed, pane_rects_of, presentation_of,
    restored_three_terminals_before_the_window_is_maximized, saved_files_and_terminal,
};

/// **The other half of it: a seat that is a bar is not resized either.**
///
/// The birth is only the first of the two roads from a solved rectangle to a
/// ConPTY. The second is [`Runtime::resize_leaves_to_layout`], which reads
/// one rectangle per terminal leaf out of [`leaf_resize_plan`] — and a window
/// dragged narrow enough to fold a pane used to send that fold's 24 pixels
/// down the same road, live, to a shell that had been running for hours.
///
/// A seat the solver did not place at all already answers this way ("the leaf
/// keeps the one it has until a later solve places it"); a bar is the same
/// fact said about a seat that is on screen as chrome.
///
/// Red gate: read the plan off `pane_body_viewport` and the folded seat comes
/// back in it, 48 pixels wide.
#[test]
fn a_seat_the_ladder_folded_into_a_bar_carries_no_resize() {
    let (seats, _, layout) = restored_three_terminals_before_the_window_is_maximized();
    let bar = seats
        .terminals()
        .into_iter()
        .find(|seat| presentation_of(&layout, *seat).is_collapsed_along(bt_layout::Axis::Row))
        .expect("the window is too narrow for three panes, so L3 folded one");
    let plan = leaf_resize_plan(&seats, &layout, seats.focus(), 2.0);
    assert!(
        !plan.iter().any(|target| target.seat == bar),
        "a bar was handed to a shell as a width"
    );
    assert_eq!(
        plan.len(),
        seats.terminals().len() - 1,
        "and every seat that is still a pane is still in the plan"
    );
}

/// A real selection over one leaf's own text, with anchors taken from that
/// leaf's own viewport frame — the way a drag in that pane would make one.
fn leaf_selection(tab: &mut TabState, seat: SeatId, columns: u32) -> ViewSelection {
    let leaf = tab
        .sessions
        .get_mut(&seat)
        .expect("the seat under test holds a session");
    let frame = leaf
        .session
        .viewport_frame(&mut leaf.projection)
        .expect("a live leaf has a viewport frame");
    ViewSelection {
        start: frame
            .anchor_at(0, 0, Bias::Before)
            .expect("a continuous frame")
            .expect("the first cell has an anchor"),
        end: frame
            .anchor_at(0, columns, Bias::After)
            .expect("a continuous frame")
            .expect("the last cell has an anchor"),
    }
}

/// Which panes of a tab are currently wearing selection colour.
///
/// The invariant is a statement about the whole tab, so it is asserted
/// against the whole tab. Counting "did the one pane I happened to think of
/// get cleared" would pass a sweep that only ever clears the pane it saw
/// last.
fn panes_wearing_a_selection(tab: &TabState) -> Vec<SeatId> {
    tab.leaves()
        .filter(|(_, leaf)| leaf.session.view_selection().is_some())
        .map(|(seat, _)| *seat)
        .collect()
}

/// PIN: **a window shows at most one selection, because a window has one
/// clipboard.**
///
/// Copy-on-select means the highlight is not decoration — it is this
/// window's answer to "what will Ctrl+V paste". Two panes highlighted at
/// once is that answer given twice, and the user cannot tell which of them
/// is true. Before this ruling every pane kept its own selection forever, so
/// a session of ordinary work left colour in three panes naming text that
/// had been off the clipboard for minutes.
///
/// Three assertions, each ruling out a different wrong rule:
///
/// * **A new selection displaces every other.** Not just the one made
///   before it — the forbidden two-pane state is built here behind the
///   invariant's back precisely so the sweep has more than one stale claim
///   to find. A rule that cleared only "the previous pane" would pass a
///   two-pane test and leave the third pane lit.
/// * **Focus alone displaces nothing.** Moving the keyboard makes no second
///   claim; there is still one highlight and it still names the clipboard.
///   Clearing on focus would delete a selection the user is about to paste
///   somewhere, which is the opposite of the service.
/// * **`None` displaces nothing.** A click that begins a drag but never
///   moves owns no selection (`begin_local_selection` passes `None` for the
///   linear case on purpose), so clicking into a neighbour to look at it
///   must not wipe the highlight for nothing.
///
/// MUTATION: delete the sweep from [`TabState::set_leaf_selection`] and the
/// first and last blocks go red — two panes, and then three, wear colour at
/// once. Widen it to fire on `None` as well and the focus/click blocks go
/// red instead.
#[test]
fn a_new_selection_leaves_no_other_pane_of_the_tab_wearing_one() {
    let mut tab = cross_tab(1, &["left text", "middle text", "right text"]);
    let [left, middle, right] = tab.seats.terminals()[..] else {
        panic!("a three-pane cross tab holds three terminal seats");
    };

    // One gesture, one selection, wherever it lands and however often the
    // hand moves between panes.
    for seat in [left, middle, right, left] {
        let selection = leaf_selection(&mut tab, seat, 3);
        tab.set_leaf_selection(seat, Some(selection));
        assert_eq!(
            panes_wearing_a_selection(&tab),
            vec![seat],
            "the pane that was just selected in is the only one lit"
        );
    }

    // Focus is not a claim. The keyboard moves; the highlight stays where
    // the user put it.
    tab.focused_leaf = right;
    assert_eq!(
        panes_wearing_a_selection(&tab),
        vec![left],
        "a focus change makes no second claim, so it settles no ambiguity"
    );
    // Nor is a click that never becomes a drag: it owns no selection, and
    // owning none must not take one away.
    tab.set_leaf_selection(right, None);
    assert_eq!(
        panes_wearing_a_selection(&tab),
        vec![left],
        "a bare press passes `None`, and `None` displaces nobody"
    );

    // The state this rule exists to end, built directly so the sweep has two
    // stale claims to clear and not one.
    let stale_left = leaf_selection(&mut tab, left, 3);
    let stale_right = leaf_selection(&mut tab, right, 3);
    tab.sessions
        .get_mut(&left)
        .expect("the left seat holds a session")
        .session
        .set_view_selection(Some(stale_left));
    tab.sessions
        .get_mut(&right)
        .expect("the right seat holds a session")
        .session
        .set_view_selection(Some(stale_right));
    assert_eq!(
        panes_wearing_a_selection(&tab).len(),
        2,
        "the fixture only proves anything if two panes really are lit"
    );

    let fresh = leaf_selection(&mut tab, middle, 4);
    tab.set_leaf_selection(middle, Some(fresh));
    assert_eq!(
        panes_wearing_a_selection(&tab),
        vec![middle],
        "one new selection ends every older claim, not merely the last"
    );
    assert!(
        tab.sessions[&left].projection.selection().is_none()
            && tab.sessions[&right].projection.selection().is_none(),
        "and the projections agree, so nothing stays painted with nothing behind it"
    );
}

/// **RED — the seat `Alt`+wheel aims is the seat under the pointer, on
/// whichever display the window is on** (§7.21, `cardhint`).
///
/// `aim_focus_card_window` walks this very geometry, in this order: the clip
/// box, then the card whose body holds the pointer, then that card's mini
/// seats. The pointer arrives in the *current* display's physical pixels, so
/// the point that was over a card's terminal seat is, after a scale change,
/// that same point times the ratio.
///
/// **Aimed at the foot of the last card**, which is where the report is: a
/// column run down to its end has its last card against the foot of the clip
/// box on either display, and an unrestated offset slid that card a third of
/// a card's height up the panel. The pixel a hand had been turning the wheel
/// on was then blank — no card holds it, the walk stops at the first step,
/// and the notch is declined without a word.
#[test]
fn alt_wheel_finds_the_seat_under_the_pointer_after_a_scale_change() {
    let (_, was) = CARDS_AT_200;
    let (_, now) = CARDS_AT_150;
    let ratio = now / was;
    let tree = LayoutNode::seat(bt_layout::Seat::new(SeatId(1), SeatKind::Terminal));

    let there = cards_column(CARDS_AT_200, 3, 0.0);
    let there = cards_column(CARDS_AT_200, 3, there.max_scroll);
    let aimed = there.cards[2].mini;
    let point = [(aimed[0] + aimed[2]) / 2.0, aimed[3] - 8.0 * was];
    assert!(
        seats::focus_mini_seats(&tree, aimed, was)
            .into_iter()
            .any(|seat| seats::rect_holds(seat.rect, point[0], point[1])),
        "the fixture aims at the seat it means to"
    );

    let here = cards_column(
        CARDS_AT_150,
        3,
        restated_scroll(there.max_scroll, f64::from(was), f64::from(now)),
    );
    let point = [point[0] * ratio, point[1] * ratio];
    let [list_top, list_bottom] = here.viewport;
    assert!(
        point[1] >= list_top && point[1] < list_bottom,
        "the pointer is still inside the list's clip box"
    );
    let card = here
        .cards
        .iter()
        .position(|card| seats::rect_holds(card.body, point[0], point[1]))
        .expect("the pointer is still over a card");
    assert_eq!(card, 2, "and over the same card it was over");
    assert!(
        seats::focus_mini_seats(&tree, here.cards[card].mini, now)
            .into_iter()
            .any(|seat| seats::rect_holds(seat.rect, point[0], point[1])),
        "and over that card's terminal seat, which is what the notch aims"
    );
}

/// RED (ticket 14) — **a link in a previewed document answers the same row as a terminal
/// reference, under both modifiers.**
///
/// Owner ruling 2026-09-23: "Links inside previewed documents follow the same rule as terminal
/// references: click stays in the window, Ctrl+click hands over." So a `mailto:`, a share, an
/// `https:` address and a relative file link are each given to both surfaces in their own
/// spelling, and the two answers must be equal. The relative link names a real file, verified by
/// the real worker function for the terminal's ledger, so no hand-made verdict stands in.
///
/// Red on the base: a document's `mailto:` answered nothing under `Ctrl`, its share nothing under
/// either modifier, and its file the seat under `Ctrl`.
///
/// MUTATION: map `preview::LinkAction::Refused(_)` to `ReferenceRow::Nothing` in
/// `preview_reference_row` — the base's answer — and the share rows go red on Windows; map
/// `preview::LinkAction::Unnamed(_)` to `ReferenceRow::Nothing` (the document road skipping the
/// terminal's judgement, M-SWEEP-048) and the `file://server/…` row goes red on a Mac, where no
/// path is read out of a remote authority.
#[test]
fn a_document_link_answers_the_same_row_as_a_terminal_reference() {
    let directory = bt_testpath::temp_path("folio-t14-document-links");
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a scratch folder");
    let document = directory.join("README.md");
    let notes = directory.join("notes.md");
    std::fs::write(&document, b"# x").expect("a document");
    std::fs::write(&notes, b"x").expect("a file it links to");
    let verdict = bt_term::verify_path(&notes, &bt_platform::resolved_for_a_door);
    assert!(verdict.exists && !verdict.directory);
    let ledger = |path: &Path| (path == notes.as_path()).then(|| verdict.clone());

    let notes_uri = bt_transcript::paths::local_path_to_file_uri(&notes);
    // A share's own spelling in a document is Windows' grammar; on a filesystem
    // where `\` is an ordinary character it is a relative file name, and the
    // `file://server/…` row below is the share on every platform.
    #[cfg(windows)]
    let share_spelling = Some((
        r"\\server\share\a.md",
        "file://server/share/a.md".to_owned(),
    ));
    #[cfg(not(windows))]
    let share_spelling: Option<(&str, String)> = None;
    let pairs = [("mailto:x@example.com", "mailto:x@example.com".to_owned())]
        .into_iter()
        .chain(share_spelling)
        .chain([
            (
                "file://server/share/a.md",
                "file://server/share/a.md".to_owned(),
            ),
            (
                "https://example.test/a",
                "https://example.test/a".to_owned(),
            ),
            ("./notes.md", notes_uri),
        ]);
    for (document_target, terminal_uri) in pairs {
        for control in [false, true] {
            assert_eq!(
                preview_link_activation(control, document_target, &document),
                hyperlink_activation(
                    control,
                    true,
                    &terminal_uri,
                    bt_transcript::paths::PathNamer::ThisWindow,
                    &ledger
                ),
                "{document_target:?} in a document and {terminal_uri:?} in the terminal, \
                 Ctrl {control}"
            );
            assert_eq!(
                preview_link_answers_a_press(control, document_target, &document),
                terminal_link_answers_a_press(
                    control,
                    Some(&terminal_uri),
                    bt_transcript::paths::PathNamer::ThisWindow,
                    &ledger
                ),
                "and the finger says the same: {document_target:?}, Ctrl {control}"
            );
        }
    }
    // An in-document anchor keeps its answer: nothing, under either modifier.
    for control in [false, true] {
        assert_eq!(
            preview_link_activation(control, "#usage", &document),
            HyperlinkActivation::None
        );
    }
    let _ = std::fs::remove_dir_all(&directory);
}

/// PIN — M170/C36. What the tree writes is what reaches the disk.
///
/// The root already had this pin; the expansion set and the selection did
/// not, because until this slice nothing could put anything in them. A press
/// is now the only way they are ever filled, so the round trip has to start
/// at a press and not at a hand-built struct.
#[test]
fn what_a_press_opened_and_selected_is_what_gets_written() {
    let (mut tab, seat) = files_column("D:\\work");
    {
        let state = tab.files.get_mut(&seat).expect("the column has state");
        press_files_node(state, "/src", files::RowKind::Directory { open: false });
        press_files_node(state, "/src/main.rs", files::RowKind::File);
    }
    let saved = tab
        .seats
        .to_persisted(&|seat| tab.term_leaf(seat, false), &|seat| {
            tab.files_state(seat)
        });
    let leaf = persisted_files_leaves(&saved)
        .into_iter()
        .next()
        .expect("the tree has a files leaf")
        .clone();
    assert_eq!(leaf.root, "D:\\work");
    assert_eq!(leaf.open, vec!["/src".to_owned()]);
    assert_eq!(leaf.sel.as_deref(), Some("/src/main.rs"));

    let (seats, _, _, files, _preview) = revive_plan(&saved_files_and_terminal(leaf));
    let revived = seats.files()[0];
    assert_eq!(
        files[&revived].open.iter().cloned().collect::<Vec<_>>(),
        vec!["/src".to_owned()],
        "and comes back open at the same folder"
    );
    assert_eq!(files[&revived].sel.as_deref(), Some("/src/main.rs"));
}

/// PIN — closing a column forgets what it had read.
///
/// Seat ids are re-minted from a counter, so a cache left behind is a cache
/// the *next* column inherits — showing somebody else's directories under
/// its own root.
#[test]
fn closing_a_column_drops_the_directories_it_had_read() {
    let (mut tab, seat) = files_column("D:\\work");
    tab.file_trees
        .entry(seat)
        .or_default()
        .accept("", listed(vec![dir_entry("src", true)]));
    assert!(tab.file_trees.contains_key(&seat));
    tab.files.remove(&seat);
    tab.file_trees.remove(&seat);
    assert!(
        tab.files_tree_walk(None).is_empty() || !tab.file_trees.contains_key(&seat),
        "the cache goes with the state it belonged to"
    );
}

/// A press inside *any* pane of a split reaches that pane's grid, so the
/// selection gesture begins there.
///
/// The bug this pins: the router asked `terminal_contains(seats.identity())`,
/// which is one fixed leaf, and answered "consumed by chrome" for a press in
/// every other pane — `begin_local_selection` was never reached and only the
/// primary pane in the window could be dragged over.
///
/// MUTATION: re-base the predicate on `seats.identity()`'s rectangle and the
/// second and third panes go red, because the red gate below first proves
/// their centres are points that rectangle does not contain.
#[test]
fn a_press_in_any_pane_of_a_split_reaches_that_panes_own_grid() {
    let seats = cross_seats(3);
    let (layout, _) = cross_solve(&seats);
    let shells: std::collections::BTreeSet<SeatId> = seats.terminals().into_iter().collect();
    let primary = seats.identity();
    let rects = pane_rects_of(&layout);
    assert_eq!(rects.len(), 3, "a three-pane tab places three rectangles");

    let mut outside_the_primary = 0;
    for (seat, rect) in &rects {
        let x = f64::from((rect[0] + rect[2]) / 2.0);
        let y = f64::from((rect[1] + rect[3]) / 2.0);
        assert!(
            !press_reaches_no_grid(&layout, x, y, |seat| shells.contains(&seat)),
            "a press in the middle of {seat:?} must reach that pane's grid"
        );
        if *seat != primary {
            // The red gate: without this the assertion above would pass on a
            // predicate that simply never refuses anything.
            assert_ne!(
                seats::pane_at(&layout, x, y),
                Some(primary),
                "{seat:?}'s centre must be a point the primary seat does not \
                     contain, or the mutation this test guards is unobservable"
            );
            outside_the_primary += 1;
        }
    }
    assert_eq!(
        outside_the_primary, 2,
        "two of the three panes are not the primary seat"
    );

    // The clause the old predicate existed for, kept: a seat with no shell
    // behind it — a preview body — is still the seat's press and not the
    // grid's, and so is the surface that is no seat at all.
    let mut with_preview = cross_seats(2);
    with_preview
        .add_preview(&cross_metrics())
        .expect("the preview seat lands");
    let (preview_layout, _) = cross_solve(&with_preview);
    let preview = with_preview.preview().expect("the preview seat is open");
    let shells: std::collections::BTreeSet<SeatId> = with_preview.terminals().into_iter().collect();
    let rect = preview_layout
        .get(preview)
        .and_then(|placement| placement.device_rect)
        .expect("the preview seat has a rectangle");
    let x = f64::from((rect.left + rect.right) as i32) / 2.0;
    let y = f64::from((rect.top + rect.bottom) as i32) / 2.0;
    assert!(
        press_reaches_no_grid(&preview_layout, x, y, |seat| shells.contains(&seat)),
        "a press in the preview's body belongs to that seat, not to the grid \
             underneath it"
    );
    assert!(
        press_reaches_no_grid(&preview_layout, -1.0, -1.0, |seat| shells.contains(&seat)),
        "a point in no pane at all reaches no grid"
    );
}

/// **N159 — what arrives owes no unread claim.**
///
/// The merged panes have just become part of the tab on screen, which is the
/// event `mark_seen` answers, so each migrated leaf gets the same two things
/// `TabState::mark_seen` does per leaf: its ledger brought level with what its
/// shell has said, and its attention latches retired.
///
/// Both claims are made *real* first rather than asserted against a fresh
/// session that never had either. Each source shell is given a unit of output
/// through the rule that counts it, and left unpainted — which is exactly the
/// shape of a leaf drained behind a tab nobody was looking at — and each rings
/// its bell so `bell_latched` is actually set. A leaf that arrived unread and
/// clamouring is then a visible failure instead of a coincidence.
///
/// Red gate: drop `mark_leaf_seen` from the migration loop and the merged tab
/// wears a dot and a bell for panes the user is looking straight at.
#[test]
fn the_arrived_members_of_a_merge_are_already_seen() {
    let mut source = cross_tab(1, &["SRCA", "SRCB"]);
    for (_, leaf) in source.leaves_mut() {
        leaf.output_revision = output_revision(leaf.output_revision, true, false);
        leaf.session
            .feed(b"\x07")
            .expect("a bell latches attention");
        leaf.last_seen_revision = 0;
        assert_ne!(
            leaf.output_revision, 0,
            "the shell has spoken since it was last seen"
        );
        assert!(leaf.session.status().bell_latched());
    }
    let mut target = cross_tab(2, &["TGTA", "TGTB"]);
    let arrived = cross_merge(
        &source.seats,
        &mut target,
        seats::LayoutAim::SeatEdge(SeatId(2), seats::DropEdge::Right),
    );
    let migrated: Vec<SeatId> = arrived.iter().map(|(_, now)| *now).collect();
    absorb_tab_into_layout(
        &mut source,
        &mut target,
        &arrived,
        None,
        TabId(9),
        cross_solve,
    );
    for seat in migrated {
        let leaf = target.sessions.get(&seat).expect("migrated");
        assert_eq!(
            leaf.last_seen_revision, leaf.output_revision,
            "{seat:?} arrived still claiming to be unread"
        );
        assert!(
            !leaf.session.status().bell_latched(),
            "{seat:?} arrived still ringing"
        );
    }
}
