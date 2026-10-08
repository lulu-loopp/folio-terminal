//! **The crate root: clipboard and paste.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    THREE_LINES, a_shell, cross_tab, found, free_fn_body, hyperlink_hit, paste_leaf, paste_tab,
    paste_text_into, source, squeezed, squeezed_body,
};
use bt_source::{Pattern, View, needle};

fn local_selection_route(mode: SelectionDragMode) -> MouseRoute {
    let hit = hyperlink_hit("https://example.test");
    MouseRoute::Local(Box::new(SelectionDrag {
        mode,
        owner: a_shell(),
        origin_row: 1,
        origin_column: 2,
        origin: ViewSelection {
            start: hit.start,
            end: hit.end,
        },
        hyperlink: None,
        hyperlink_control: false,
        local_image_activation: LocalImageActivation::None,
    }))
}

#[test]
fn selection_release_copy_policy_covers_drag_word_and_line_but_not_click_or_forwarding() {
    for mode in [
        SelectionDragMode::Linear,
        SelectionDragMode::Word,
        SelectionDragMode::Line,
    ] {
        let route = local_selection_route(mode);
        assert!(should_copy_on_select_release(Some(&route), false, true));
    }

    let click = local_selection_route(SelectionDragMode::Linear);
    assert!(!should_copy_on_select_release(Some(&click), true, true));
    let forwarded = MouseRoute::Forward {
        button: input::MouseProtocolButton::Left,
        sgr: true,
        owner: a_shell(),
    };
    assert!(!should_copy_on_select_release(
        Some(&forwarded),
        false,
        true
    ));
    assert!(!should_copy_on_select_release(None, false, true));
}

/// RED (gesture audit 2026-08-26, 丙4) — **`Copy on select` is a switch, and
/// turning it off stops the write.**
///
/// This gesture is the odd one of the audit's seven: the reader can do it
/// and does, every time they drag across a line. What is invisible is the
/// *result* — the clipboard they had is gone and nothing said so. Windows
/// Terminal ships `copyOnSelect` off, so it is not a habit arriving with
/// the reader; a toast per drag would be noise; so the row on the Terminal
/// page is what names the behaviour, and this is the assertion that the
/// name is attached to something.
///
/// MUTATION: drop the flag from the conjunction and the second assertion
/// goes red — a switch that changes nothing is a worse answer than no
/// switch, because it says the reader was heard.
#[test]
fn copy_on_select_is_the_readers_answer_and_off_means_off() {
    let route = local_selection_route(SelectionDragMode::Linear);
    assert!(should_copy_on_select_release(Some(&route), false, true));
    assert!(!should_copy_on_select_release(Some(&route), false, false));
    // Off does not turn a single click into a copy either — the two
    // conditions are independent and both still have to hold.
    assert!(!should_copy_on_select_release(Some(&route), true, false));
}

#[test]
fn unavailable_clipboard_copy_keeps_selection_and_allows_a_retry() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"retry me").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let selection = ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 7, Bias::After).unwrap().unwrap(),
    };
    session.set_view_selection(Some(selection.clone()));
    projection.set_selection(Some(selection));

    let copied = copy_selection(&mut session, &mut projection, |_| {
        Err(anyhow!("injected clipboard owner contention"))
    });
    assert!(
        !copied,
        "clipboard contention must not escape as a fatal error"
    );
    assert_eq!(session.selection_text().as_deref(), Some("retry me"));
    assert!(projection.selection().is_some());

    let mut clipboard = String::new();
    let copied = copy_selection(&mut session, &mut projection, |text| {
        clipboard.push_str(text);
        Ok(())
    });
    assert!(copied);
    assert_eq!(clipboard, "retry me");
    assert!(session.view_selection().is_none());
    assert!(projection.selection().is_none());
}

#[test]
fn ctrl_c_keeps_its_existing_empty_text_write_and_clear_semantics() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"   ").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let selection = ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 2, Bias::After).unwrap().unwrap(),
    };
    session.set_view_selection(Some(selection.clone()));
    projection.set_selection(Some(selection));
    let mut writes = Vec::new();

    assert!(copy_selection(&mut session, &mut projection, |text| {
        writes.push(text.to_owned());
        Ok(())
    }));
    assert_eq!(writes, [""]);
    assert!(session.view_selection().is_none());
    assert!(projection.selection().is_none());
}

/// **A drop keeps the point it opened with, however many files follow**
/// (release review 0.4.2 X-10).
///
/// The defect this closes, stated as a sequence: the files of one drop
/// arrive one event at a time, and the point was read at the *end* of the
/// run — at the turn boundary, where the paste happens. On a window with
/// work to do that is late enough for the hand to have left the pane it
/// dropped on, and the file went somewhere it was never let go of. The point
/// is now read as the first file arrives, and the batch is what carries it.
///
/// Three claims: the first file's point is the batch's; every later file of
/// the same drop is added to it without disturbing that point, **even when a
/// later point is offered**, which is what makes this a property of the type
/// rather than of one caller's discipline; and a drop that follows a spent
/// one opens afresh.
///
/// **And the same three of the address beside it** (review X-1). The shell a
/// drop is aimed at is the other fact that belongs to the arrival, and it
/// goes stale in the same way and worse: the point moves with the hand,
/// where the tab on top and the program in a seat can both have been
/// replaced by the turn that spends the batch. One type carries both, so
/// there is one answer to "when was this decided".
///
/// MUTATION: let the `Some` arm overwrite `point` or `target` and the second
/// block goes red — that arm is exactly what a flush-time reading would be,
/// arriving through the door the fix closed.
#[test]
fn a_drop_keeps_the_point_and_the_shell_it_opened_with() {
    let opened_at = PhysicalPosition::new(37.0, 41.0);
    let later = PhysicalPosition::new(900.0, 12.0);
    let aimed_at = PasteTarget {
        tab: TabId(3),
        seat: SeatId(2),
        incarnation: 11,
    };
    let elsewhere = PasteTarget {
        tab: TabId(4),
        seat: SeatId(1),
        incarnation: 12,
    };
    let mut standing: Option<DropBatch> = None;

    DropBatch::collect(
        &mut standing,
        "/first".into(),
        Some(opened_at),
        Some(aimed_at),
    );
    let batch = standing.as_ref().expect("the first file opens the drop");
    assert_eq!(batch.point, Some(opened_at));
    assert_eq!(batch.target, Some(aimed_at));
    assert_eq!(batch.paths, [PathBuf::from("/first")]);

    // The second and third files of the same drop. The point offered with
    // them is where the hand has since travelled to, and the address is the
    // shell that is under it now. Both are refused.
    DropBatch::collect(
        &mut standing,
        "/second file".into(),
        Some(later),
        Some(elsewhere),
    );
    DropBatch::collect(&mut standing, "/third".into(), None, None);
    let batch = standing.as_ref().expect("the drop is still standing");
    assert_eq!(
        batch.point,
        Some(opened_at),
        "a point offered after the drop opened has replaced the one it \
             opened with, which is the flush-time reading X-10 named"
    );
    assert_eq!(
        batch.target,
        Some(aimed_at),
        "and an address offered after it opened has replaced the shell the \
             hand was actually over, which is X-1 one door along"
    );
    assert_eq!(
        batch.paths,
        [
            PathBuf::from("/first"),
            PathBuf::from("/second file"),
            PathBuf::from("/third"),
        ],
        "one drop, three events, one batch, in the order winit delivered them"
    );

    // Spent, and then a second drop somewhere else entirely.
    let spent = standing.take().expect("the flush takes the whole batch");
    assert_eq!(spent.paths.len(), 3);
    assert!(
        standing.is_none(),
        "nothing is left behind to be pasted twice"
    );
    DropBatch::collect(
        &mut standing,
        "/fourth".into(),
        Some(later),
        Some(elsewhere),
    );
    let next = standing.expect("the next drop opens");
    assert_eq!(
        next.point,
        Some(later),
        "a new drop reads the cursor again; the point belongs to the drop \
             and not to the window"
    );
    assert_eq!(
        next.target,
        Some(elsewhere),
        "and names the shell afresh, for the same reason"
    );
}

/// RED (0.4.4 ticket 02) — **a second multi-line paste replaces the pending one**, wherever in
/// the tab it was aimed.
///
/// MUTATION: drop the loop that clears every leaf's `pending_paste` in `stage_paste` — two panes
/// hold a paste and the first is still found.
#[test]
fn a_second_multi_line_paste_replaces_the_pending_one() {
    let mut tab = cross_tab(1, &["a", "b"]);
    let seats: Vec<SeatId> = tab.sessions.keys().copied().collect();
    for seat in &seats {
        tab.sessions
            .get_mut(seat)
            .unwrap()
            .paste_recipient
            .encoder
            .grammar = shell_literal::ShellGrammar::Cmd;
    }
    let aim = |tab: &TabState, seat: SeatId| PasteTarget {
        tab: tab.id,
        seat,
        incarnation: tab.sessions[&seat].incarnation,
    };
    let first = aim(&tab, seats[0]);
    let second = aim(&tab, seats[1]);
    assert_eq!(
        paste_text_into(&mut tab, first, "a\nb", true),
        StagedPaste::Held
    );
    assert_eq!(
        paste_text_into(&mut tab, second, "c\nd\ne", true),
        StagedPaste::Held
    );
    let held: Vec<SeatId> = tab
        .sessions
        .iter()
        .filter(|(_, leaf)| leaf.pending_paste.is_some())
        .map(|(seat, _)| *seat)
        .collect();
    assert_eq!(held, vec![seats[1]], "one question, the newest");
    let (_, pending) = pending_paste_in(&tab).unwrap();
    assert_eq!((pending.text.as_str(), pending.lines), ("c\nd\ne", 3));
}

/// RED (0.4.4 tickets 02 and 03) — **the question has one address**: `deliver_paste` asks it
/// through `stage_paste`, and `paste_road` is the one rule, read by nothing else.
///
/// A PowerShell pane whose integration never spoke has no prompt the shell opened in order, so it
/// is a program without bracketed paste and is asked.
///
/// MUTATION: drop `stage_paste(` from `deliver_paste`.
#[test]
fn the_paste_question_has_one_address() {
    let deliver = squeezed_body("Runtime", "deliver_paste");
    assert!(deliver.contains("matchstage_paste(&mutself.window.tabs[active],"));
    assert!(squeezed(free_fn_body("stage_paste")).contains("paste_road(&text,PasteFacts::of("));
    for (name, count) in [("stage_paste(", 2), ("paste_road(", 2)] {
        let items = found(needle!(Pattern::text(name)), View::CodeKeepingLiterals)
            .in_the_product(source())
            .len();
        assert_eq!(items, count, "`{name}` is asked from a second place");
    }
    // No OSC 133 — the integration never spoke — is a program without bracketed paste.
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::PowerShell, b""));
    assert_eq!(
        paste_text_into(&mut tab, target, THREE_LINES, true),
        StagedPaste::Held
    );
}
