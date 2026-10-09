//! **`restore`, as the application drives it.** Tests whose first assertion is about
//! `restore`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    RESTORE_CARD_RUNG, buffer_read_from, disk_scratch, item_body, method_body, squeezed,
    squeezed_body, tab_with_a_preview,
};
use bt_source::ItemQuery;

/// PIN (ticket #62) — **`Clear scrollback…` asks by count, and asks nothing
/// when there is nothing to lose.**
///
/// The gate names what goes, which for every other request on its list is a
/// file or a ref and for this one is a number: a transcript has no name, and
/// the part of it that makes the row dangerous is precisely the part that has
/// scrolled out of sight. `raise_dirty_gate` reads an empty name list as
/// "nothing to ask about", so an empty scrollback is cleared without a
/// dialog — the same shortcut a clean preview pool already takes.
///
/// MUTATION: give the empty case a name and every `Clear scrollback…` on a
/// fresh pane raises a modal about deleting nothing.
#[test]
fn the_clear_scrollback_gate_counts_what_it_deletes_and_asks_nothing_for_none() {
    let request = restore::GateRequest::ClearScrollback(bt_layout::SeatId(0));
    assert_eq!(request.title(), "Clear scrollback?");
    assert_eq!(
        request.answer_text(),
        "Clear",
        "the button carries the row's own verb"
    );
    assert_eq!(
        request.message(&[lines_phrase(1_284)]),
        "1284 lines of past output is deleted. Search over it will find nothing.",
        "the sentence leads with what is being lost, and keeps the mock-up's own warning"
    );
    assert!(request.message(&[lines_phrase(1)]).starts_with("1 line "));

    assert_eq!(lines_phrase(1), "1 line");
    assert_eq!(lines_phrase(2), "2 lines");
    assert_eq!(lines_phrase(0), "0 lines");

    // What the empty case looks like where it is actually decided: a pane
    // whose history and staging are both empty offers the gate no name, and
    // `raise_dirty_gate` lets the verb through unasked.
    let session = DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    assert_eq!(session.scrollback_line_count(), 0);
}

// ── ticket 58: a busy dirty gate is never "nothing to ask" ──────────────────

/// A real file under a scratch folder, read into a buffer the way the window
/// reads one, then typed into through the keyboard's own door — a dirty
/// buffer with the reader's words in it and the file's old words on the disk.
fn a_file_being_edited(tag: &str) -> (PathBuf, preview::PreviewBuffer) {
    let dir = disk_scratch(&format!("gate58-{tag}"));
    let path = dir.join("notes.md");
    std::fs::write(&path, "one\n").expect("write the file");
    let mut buffer = buffer_read_from(&path);
    let mut caret = preview_edit::EditCaret::default();
    let body = buffer.content.clone().expect("the head landed");
    caret.place(&body, 4, false);
    assert!(
        buffer.edit_by_caret(&mut caret, |content, caret| {
            preview_edit::insert(content, caret, "typed by hand")
        }),
        "the keystroke landed"
    );
    assert!(buffer.dirty, "there is unsaved work");
    (path, buffer)
}

/// Whether the buffer for `path` in this tab still holds what was typed.
fn still_holds_the_edit(tab: &TabState, path: &Path) -> bool {
    tab.preview_pool
        .get(&preview::PreviewSource::file(path))
        .is_some_and(|buffer| {
            buffer.dirty
                && buffer
                    .content
                    .as_deref()
                    .is_some_and(|body| body.contains("typed by hand"))
        })
}

/// The window's own event handler, the door every OS close (Alt+F4, the
/// taskbar's Close window, `performClose:`) comes through, squeezed.
fn window_event_squeezed() -> String {
    squeezed(item_body(
        &ItemQuery::method("FolioApp", "window_event").of_trait("ApplicationHandler"),
    ))
}

/// RED (58) — **A window close requested while the unsaved-changes gate is up
/// neither closes the window nor loses the buffer.**
///
/// The census's traced sequence (R1): a tab holding an edited file is asked to
/// close, the gate goes up holding `CloseTab`, and before anybody answers the
/// OS asks the window to close. The `CloseRequested` arm put `Shut` to the
/// gate, the gate — already open — answered `Ok(false)`, the word for "nothing
/// to ask", the arm set `shutting`, and `FolioApp::close` let the window go;
/// the reaped `WindowRuntime` took the pool with it, and the session file
/// keeps paths, not bytes. Now the gate answers `Busy`, which never proceeds,
/// and it keeps the request it was asking about.
///
/// The decision runs for real here (`raise_dirty_gate_over`, the whole of
/// `Runtime::raise_dirty_gate` but the repaint) on a real tab over a real file
/// typed into through the keyboard's door. `FolioApp` cannot be built without
/// an event loop, so the arm the OS event reaches is read through `bt_source`:
/// it sets `shutting` from `proceeds()` and from nothing else, and the one
/// `self.close(window_id)` in the handler stands behind `if shutting`. The
/// retirement fork — the ending run's `App::finish` or the non-final window's
/// `vault_this_window` — is decided inside `FolioApp::close` (`ending`), after
/// that line, so neither road is reachable from a busy gate.
///
/// MUTATION: in `DirtyGate::verdict`, answer an open gate with `NothingToAsk`
/// (BASE's "already open → Ok(false)") — the first assertion goes red.
#[test]
fn a_window_close_requested_while_the_gate_is_up_neither_closes_the_window_nor_loses_the_buffer() {
    let (path, buffer) = a_file_being_edited("close-under-gate");
    let (tab, _) = tab_with_a_preview(1, vec![buffer]);
    let tabs = vec![tab];
    let mut gate = restore::DirtyGate::default();

    assert_eq!(
        raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::CloseTab(0)),
        restore::GateRaise::Raised,
        "closing the tab asks first"
    );
    let os_close = raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::Shut);
    assert_eq!(
        os_close,
        restore::GateRaise::Busy,
        "a close requested while the question is up is not nothing to ask"
    );
    assert!(!os_close.proceeds(), "so the window does not shut");
    assert_eq!(
        gate.request(),
        Some(&restore::GateRequest::CloseTab(0)),
        "and the question on the screen is still the one the reader was asked"
    );
    assert!(
        still_holds_the_edit(&tabs[0], &path),
        "the buffer still holds what was typed"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read the file back"),
        "one\n",
        "and nothing was written behind the reader's back"
    );

    let event = window_event_squeezed();
    assert!(
        event.contains(
            "WindowEvent::CloseRequested=>{runtime.raise_dirty_gate(restore::GateRequest::Shut).map(|raised|shutting=raised.proceeds())}"
        ),
        "the OS close shuts only when the gate had nothing to ask:\n{event}"
    );
    assert_eq!(
        event.matches("self.close(window_id)").count(),
        1,
        "the handler has one road to the close"
    );
    assert!(
        event.contains(
            "letresult=ifshutting{result.and(hang_watch::during(hang_watch::Station::EventShut,||{self.close(window_id)}))"
        ),
        "and it stands behind `shutting`:\n{event}"
    );
    let close = squeezed(method_body("FolioApp", "close"));
    assert!(
        close.contains("letending=a_run_ends_with_its_last_visible_window(")
            && close.contains("runtime.close_window(ending)"),
        "the final and the non-final retirement fork inside the close, behind the gate:\n{close}"
    );
    let raise = squeezed(method_body("Runtime", "raise_dirty_gate"));
    assert!(
        raise.contains("crate::raise_dirty_gate_over("),
        "the window's gate decides through the function this test runs:\n{raise}"
    );
    let _ = std::fs::remove_dir_all(path.parent().expect("the scratch folder"));
}

/// RED (58) — **Two OS close requests in a row ask once and close nothing
/// until answered.**
///
/// The census's shorter reproduction: the first Alt+F4 raises `Shut`, and the
/// second found the gate open and was read as "nothing to ask". Now the second
/// is `Busy`: one question, the same question, and the window stands. Once it
/// is answered (the answer takes the request off the gate first), a close is
/// asked again from the top.
///
/// MUTATION: in `DirtyGate::verdict`, answer an open gate with `NothingToAsk` —
/// the second assertion goes red.
#[test]
fn two_os_close_requests_in_a_row_ask_once_and_close_nothing_until_answered() {
    let (path, buffer) = a_file_being_edited("two-closes");
    let (tab, _) = tab_with_a_preview(1, vec![buffer]);
    let tabs = vec![tab];
    let mut gate = restore::DirtyGate::default();

    let first = raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::Shut);
    let second = raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::Shut);
    assert_eq!(first, restore::GateRaise::Raised, "the first close asks");
    assert_eq!(
        second,
        restore::GateRaise::Busy,
        "the second asks nothing new"
    );
    assert!(
        !first.proceeds() && !second.proceeds(),
        "and neither closes the window"
    );
    assert_eq!(gate.request(), Some(&restore::GateRequest::Shut));
    assert!(still_holds_the_edit(&tabs[0], &path));

    assert_eq!(
        gate.take(),
        Some(restore::GateRequest::Shut),
        "Cancel: the answer takes the one question it was"
    );
    assert_eq!(
        raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::Shut),
        restore::GateRaise::Raised,
        "and a close after Cancel is asked again, because the edit is still there"
    );
    let _ = std::fs::remove_dir_all(path.parent().expect("the scratch folder"));
}

/// RED (58) — **Cancel keeps the buffer; Save and Discard replay the accepted
/// request.**
///
/// The answer road is unchanged by this ticket, and this is why it did not
/// need BASE's "already open means proceed": `Runtime::answer_dirty_gate`
/// takes the request off the gate *before* it re-runs anything, so the re-run
/// meets a free gate and a pool the answer has already dealt with. Run here
/// with the busy case in front of it — a `Shut` dropped on a `CloseTab`
/// question — for each answer: Cancel leaves the edit and the tab, and the
/// dropped close is not replayed; Discard empties the tab's pool (the
/// `CloseTab` arm's `clear`) and the replayed close is nothing to ask; Save
/// writes through the pool's own `save_dirty` (`quit_save`'s door, the shut's
/// `Save`) and the replayed shut is nothing to ask, with the typed words on
/// the disk.
///
/// MUTATION: treat `Busy` as proceeding (`proceeds` answering
/// `self != Self::Raised`) — the Cancel block's first assertion goes red; move
/// `take()` after the re-runs in `answer_dirty_gate` — the order pin goes red.
#[test]
fn cancel_keeps_the_buffer_and_save_and_discard_replay_the_accepted_request() {
    // Cancel.
    let (path, buffer) = a_file_being_edited("cancel");
    let (tab, _) = tab_with_a_preview(1, vec![buffer]);
    let tabs = vec![tab];
    let mut gate = restore::DirtyGate::default();
    let _ = raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::CloseTab(0));
    let dropped = raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::Shut);
    assert!(
        !dropped.proceeds(),
        "the OS close under the question is dropped"
    );
    assert_eq!(gate.take(), Some(restore::GateRequest::CloseTab(0)));
    assert!(
        still_holds_the_edit(&tabs[0], &path),
        "Cancel: nothing happens, and nothing is lost"
    );
    assert!(
        !gate.is_open(),
        "and the dropped close was not queued behind it"
    );
    let _ = std::fs::remove_dir_all(path.parent().expect("the scratch folder"));

    // Discard, replaying `CloseTab`.
    let (path, buffer) = a_file_being_edited("discard");
    let (tab, _) = tab_with_a_preview(1, vec![buffer]);
    let mut tabs = vec![tab];
    let mut gate = restore::DirtyGate::default();
    let _ = raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::CloseTab(0));
    let _ = raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::Shut);
    let accepted = gate.take().expect("the answer takes its request");
    assert_eq!(accepted, restore::GateRequest::CloseTab(0));
    tabs[0].preview_pool.clear();
    assert_eq!(
        raise_dirty_gate_over(&mut gate, &tabs, 0, accepted),
        restore::GateRaise::NothingToAsk,
        "Discard: the replayed close meets a free gate and nothing at risk, so it closes"
    );
    let _ = std::fs::remove_dir_all(path.parent().expect("the scratch folder"));

    // Save, replaying `Shut`.
    let (path, buffer) = a_file_being_edited("save");
    let (tab, _) = tab_with_a_preview(1, vec![buffer]);
    let mut tabs = vec![tab];
    let mut gate = restore::DirtyGate::default();
    let _ = raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::Shut);
    assert!(
        restore::GateRequest::Shut.offers_save(),
        "the shut is the question Save answers"
    );
    let accepted = gate.take().expect("the answer takes its request");
    assert_eq!(
        tabs[0].preview_pool.save_dirty(),
        vec![("notes.md".to_owned(), preview::SaveOutcome::Saved)],
        "every dirty buffer written back"
    );
    assert_eq!(
        raise_dirty_gate_over(&mut gate, &tabs, 0, accepted),
        restore::GateRaise::NothingToAsk,
        "Save: the replayed shut has nothing left to ask"
    );
    assert!(
        std::fs::read_to_string(&path)
            .expect("read the file back")
            .contains("typed by hand"),
        "and what was typed is on the disk"
    );
    let _ = std::fs::remove_dir_all(path.parent().expect("the scratch folder"));

    // The answer takes the request before any re-run.
    let answer = squeezed(method_body("Runtime", "answer_dirty_gate"));
    let taken = answer
        .find("self.window.dirty_gate.take()")
        .expect("the answer takes its request");
    for rerun in [
        "self.quit_save()",
        "self.close_pane(seat)",
        "self.close_tab(index)",
        "self.window.window_close_requested=true",
        "self.issue_git_write(",
        "self.checkout_at(",
        "self.clear_pane_scrollback(seat)",
    ] {
        let at = answer
            .find(rerun)
            .unwrap_or_else(|| panic!("the answer still re-runs `{rerun}`"));
        assert!(
            taken < at,
            "`{rerun}` runs before the gate is free:\n{answer}"
        );
    }
}

/// RED (58) — **Nothing to ask still closes at once.**
///
/// The other half of the three-way answer, so that the fix cannot be a gate
/// that refuses everything: a window whose buffers are all clean — a file
/// opened and read, not typed into — shuts on the first OS close, a clean tab
/// closes on its first press, and the gate never opens.
///
/// MUTATION: in `DirtyGate::verdict`, answer an empty list with `Busy` — the
/// first assertion goes red.
#[test]
fn nothing_to_ask_still_closes_at_once() {
    let dir = disk_scratch("gate58-clean");
    let path = dir.join("notes.md");
    std::fs::write(&path, "one\n").expect("write the file");
    let (tab, _) = tab_with_a_preview(1, vec![buffer_read_from(&path)]);
    let tabs = vec![tab];
    let mut gate = restore::DirtyGate::default();

    let shut = raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::Shut);
    assert_eq!(shut, restore::GateRaise::NothingToAsk);
    assert!(shut.proceeds(), "a clean window shuts on the first close");
    assert!(
        raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::CloseTab(0)).proceeds(),
        "and a clean tab closes on the first press"
    );
    assert!(!gate.is_open(), "without a question ever going up");
    let _ = std::fs::remove_dir_all(&dir);
}

/// RED (58) — **A tab whose shell has exited beside an unsaved preview is not
/// asked about again after Cancel, turn after turn.**
///
/// Coordinator ruling 2026-09-25. `Runtime::reap_exited_tabs` runs at the foot
/// of every turn and used to hand every ended tab to `close_tab`, which puts
/// the question: after Cancel the shell is still gone, so the next turn put it
/// again, and Cancel could never make it stay down. Now the cleanup asks the
/// gate's question without putting it (`exited_tabs_the_loop_may_close`) and
/// leaves a tab whose close would ask where it is: the reader closes it, or
/// saves, when they choose.
///
/// Each turn here is what the reap does: the filter, then, for each tab it
/// lets through, `close_tab`'s first step (`raise_dirty_gate_over` over
/// `CloseTab`). The wiring is read through `bt_source`.
///
/// MUTATION: let the cleanup raise the gate again — return `exited` unfiltered
/// from `exited_tabs_the_loop_may_close` — and the second turn's assertion
/// goes red.
#[test]
fn a_tab_whose_shell_has_exited_beside_an_unsaved_preview_is_not_asked_about_again_after_cancel() {
    let (path, buffer) = a_file_being_edited("exited-shell");
    let (tab, _) = tab_with_a_preview(1, vec![buffer]);
    let tabs = vec![tab];
    let mut gate = restore::DirtyGate::default();

    // The reader closes the ended tab and answers Cancel.
    assert_eq!(
        raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::CloseTab(0)),
        restore::GateRaise::Raised
    );
    assert_eq!(gate.take(), Some(restore::GateRequest::CloseTab(0)));

    for turn in 1..=5 {
        let closing = exited_tabs_the_loop_may_close(&gate, &tabs, 0, vec![0]);
        for index in closing {
            let _ =
                raise_dirty_gate_over(&mut gate, &tabs, 0, restore::GateRequest::CloseTab(index));
        }
        assert!(
            !gate.is_open(),
            "turn {turn}: the loop put the question the reader just cancelled"
        );
        assert!(
            still_holds_the_edit(&tabs[0], &path),
            "turn {turn}: and the buffer stays"
        );
    }

    // A clean ended tab is still taken away by the loop at once.
    let dir = disk_scratch("gate58-exited-clean");
    let clean = dir.join("notes.md");
    std::fs::write(
        &clean, "one
",
    )
    .expect("write the file");
    let (clean_tab, _) = tab_with_a_preview(2, vec![buffer_read_from(&clean)]);
    let both = vec![tabs.into_iter().next().expect("the dirty tab"), clean_tab];
    assert_eq!(
        exited_tabs_the_loop_may_close(&gate, &both, 0, vec![0, 1]),
        vec![1],
        "only the tab with nothing to ask is closed by the loop"
    );

    let reap = squeezed(method_body("Runtime", "reap_exited_tabs"));
    let filtered = reap
        .find("letexited=crate::exited_tabs_the_loop_may_close(")
        .expect("the cleanup filters the ended tabs");
    let closes = reap
        .find("self.close_tab(index)?")
        .expect("and closes what is left");
    assert!(
        filtered < closes,
        "the filter stands before the close:
{reap}"
    );
    let _ = std::fs::remove_dir_all(path.parent().expect("the scratch folder"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// MUTATION: make the fence scanner require a *bare* ```` ``` ```` to close
/// (`lines[index] == "```"`) and the largest-block assertion goes red, which
/// is the swallowing hypothesis in its real form.
#[test]
fn the_design_document_parses_into_blocks_that_reserve_nothing_extra() {
    let source = include_str!("../../../docs/DESIGN.md");
    let blocks = preview::parse_markdown(source);
    let source_lines = source.lines().count();
    assert!(
        source_lines > 100,
        "the fixture is the real document, not a stub"
    );

    // ① No fence swallows the file. The largest one in this document is a
    //    short shell transcript; a tenth of the file is a generous bound
    //    that a runaway fence blows through by an order of magnitude.
    let biggest_fence = blocks
        .iter()
        .filter_map(|block| match block {
            preview::MarkdownBlock::Code { text, .. } => Some(text.lines().count()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    assert!(
        biggest_fence * 10 < source_lines,
        "a fence holding {biggest_fence} of {source_lines} lines is a fence \
             that never closed"
    );

    // ② The document really does exercise the blocks the prototype could not
    //    draw, or this fixture proves nothing about them.
    let count = |kind: fn(&preview::MarkdownBlock) -> bool| {
        blocks.iter().filter(|block| kind(block)).count()
    };
    assert!(
        count(|b| matches!(b, preview::MarkdownBlock::Table { .. })) > 0,
        "DESIGN.md has tables, and they must arrive as tables"
    );
    assert!(
        count(|b| matches!(b, preview::MarkdownBlock::Quote(_))) > 0,
        "and quotes"
    );
    assert!(
        count(|b| matches!(
            b,
            preview::MarkdownBlock::List {
                ordered: Some(_),
                ..
            }
        )) > 0,
        "and numbered lists"
    );

    // ③ Every table has a heading row and at least one body row — a table
    //    of one row is a separator that was read as content.
    for block in &blocks {
        if let preview::MarkdownBlock::Table { rows, .. } = block {
            assert!(
                rows.len() >= 2,
                "a table needs its heading and something under it: {rows:?}"
            );
            assert!(
                rows.iter().all(|row| !row.is_empty()),
                "and no row of no cells"
            );
        }
    }
}

/// RED (57) — **The card's own keys still work, and after it closes the shell receives keys
/// again.**
///
/// `Enter` answers the card with the button it opened focused, and the answer closes the prompt
/// before anything else; the rung and the drawing read one predicate, `RestorePrompt::is_asking`,
/// so once the prompt is closed the rung is not taken and the next key falls to the encoder, the
/// way it did before the card was up. The ladder asks about the card in exactly one place, so no
/// second, older check can keep a key from the shell after the answer.
///
/// MUTATION: make `RestorePrompt::is_asking` ignore `open` — a prompt that was never opened, or
/// was closed by the answer, still "asks", and the first assertion goes red (or take
/// `self.window.restore_prompt.close();` out of `answer_restore_prompt` — the answer's assertion
/// goes red).
#[test]
fn the_restore_cards_own_keys_still_work_and_after_it_closes_the_shell_has_the_keys_again() {
    let mut prompt = restore::RestorePrompt::default();
    assert!(!prompt.is_asking(2), "no card before a launch asks");
    prompt.open();
    assert!(prompt.is_asking(2), "up, about two tabs");
    assert!(
        !prompt.is_asking(0),
        "a card about no tab is neither drawn nor holds the keyboard"
    );
    assert!(prompt.close(), "the answer puts it away");
    assert!(
        !prompt.is_asking(2),
        "and the keyboard is the shell's again"
    );

    let answer = squeezed_body("Runtime", "answer_restore_prompt");
    assert!(
        answer.starts_with("{self.window.restore_prompt.close();"),
        "the answer does not close the card first:\n{answer}"
    );
    assert_eq!(restore::FOCUSED_ANSWER, restore::RestoreAnswer::Restore);
    assert!(
        RESTORE_CARD_RUNG.contains(
            "Key::Named(NamedKey::Enter)=>{self.answer_restore_prompt(restore::FOCUSED_ANSWER)?;}"
        ),
        "Enter no longer answers the card"
    );
    let up = squeezed_body("Runtime", "restore_card_is_up");
    assert!(
        up.contains("self.window.restore_prompt.is_asking(self.app.restore_question.len())"),
        "{up}"
    );
    assert!(
        squeezed_body("Runtime", "restore_layout")
            .contains("if!self.restore_card_is_up(){returnNone;}"),
        "the card is drawn on a reading of its own"
    );
    let ladder = squeezed_body("Runtime", "keyboard_input");
    assert_eq!(
        ladder.matches("restore_card_is_up").count(),
        1,
        "the ladder asks about the card in one place"
    );
    assert!(
        !ladder.contains("restore_prompt.is_open()"),
        "an older check on the card is still in the ladder"
    );
}

/// RED (57) — **Esc under the restore card closes it, nothing beneath receives the key, and the
/// session's restorable set is unchanged.**
///
/// Coordinator's decision 2026-09-25 on the owner's full-window gate: "Enter restores, Esc declines
/// for now", so a keyboard-only reader has both answers. Esc takes the card's **unanswered** road,
/// not "No thanks": the key closes the prompt and records nothing, so the question's tabs are still
/// in the window's `pending_restore` (and the application's question), and `window_snapshot` folds
/// them back into `lastSession` for the next launch to ask about (§7.1.4). "No thanks" is an answer
/// (`answer_restore_prompt` → `answer_restore(false)` puts them in Recent), and the Esc arm reaches
/// none of that.
///
/// MUTATION: turn `RestorePrompt::consumes_escape` back to `false` (current head) — Esc is
/// swallowed and the card stays up, and the first assertion goes red.
#[test]
fn esc_under_the_restore_card_closes_it_unanswered_and_reaches_nothing_beneath() {
    let mut prompt = restore::RestorePrompt::default();
    prompt.open();
    assert!(
        prompt.consumes_escape(),
        "Esc does not put the card away: a keyboard can only answer Restore"
    );
    // What the rung's Esc arm does to the prompt, on the real type.
    assert!(prompt.close());
    assert!(
        !prompt.is_asking(3),
        "the card is down and the keyboard is the shell's"
    );

    // The arm, inside the rung that returns for every key.
    let esc = "Key::Named(NamedKey::Escape)ifself.window.restore_prompt.consumes_escape()=>{self.window.restore_prompt.close();ifself.refresh_chrome(){self.present_chrome_change()?;}}";
    assert!(
        RESTORE_CARD_RUNG.contains(esc),
        "the Esc arm is not the prompt's close"
    );
    for answer in [
        "answer_restore_prompt(restore::RestoreAnswer::NoThanks",
        "pending_restore",
        "restore_question",
        "answer_restore(",
    ] {
        assert!(
            !esc.contains(answer),
            "the Esc arm answers the card or spends its tabs: `{answer}`"
        );
    }
    let ladder = squeezed_body("Runtime", "keyboard_input");
    let rung = ladder
        .find(RESTORE_CARD_RUNG)
        .unwrap_or_else(|| panic!("the restore card's rung is not whole"));
    for beneath in [
        "self.dismiss_web_sheet()?",
        "self.dismiss_top_float()?",
        "self.close_search()?",
        "self.send_user_input(",
    ] {
        let at = ladder
            .find(beneath)
            .unwrap_or_else(|| panic!("`{beneath}` is no longer in `keyboard_input`"));
        assert!(rung < at, "Esc reaches `{beneath}` before the card");
    }

    // The unanswered tabs go back to the file: the snapshot carries the window's pending list,
    // and only an answer (the loop's `settle_restore_answer`) clears the application's question.
    assert!(
        squeezed_body("Runtime", "window_snapshot")
            .contains(".extend(self.window.pending_restore.iter().map(|tab|TabV1{pinned:false,"),
        "an unanswered question no longer folds back into the session"
    );
    assert!(
        squeezed_body("Runtime", "answer_restore_prompt")
            .contains("self.app.pending_restore_answer=Some("),
        "the answer is recorded somewhere other than the button's road"
    );
}

// ── D-4, 0.4.8 G7: a stop that cannot ask keeps what it would lose ──────────

/// RED (D-4, 0.4.8 G7; ledger #28) — **a dirty preview buffer and a controlled failure: the file
/// carries the edit.** The failure road keeps each window's unsaved edits through
/// `keep_unsaved_edits_over` before it closes the window, and a file that can still be written
/// takes the edit through the quit's own judged write — `PreviewBuffer::save`, the conflict check
/// and the atomic write — so it is no longer dirty, and the diagnostics line names the file.
/// A second tab's buffer is kept as well: every tab, not the active one.
///
/// MUTATION: in `PreviewPool::keep_dirty`, skip the write (`let outcome =
/// SaveOutcome::Failed(String::new());` in place of `buffer.save()`) — the file still says
/// `one`.
#[test]
fn a_controlled_failure_writes_a_dirty_preview_back_to_its_file() {
    let (path, buffer) = a_file_being_edited("g7-saved");
    let (second, other) = a_file_being_edited("g7-saved-第二");
    let (tab, _) = tab_with_a_preview(1, vec![buffer]);
    let (other_tab, _) = tab_with_a_preview(2, vec![other]);
    let mut tabs = vec![tab, other_tab];
    let recovery = disk_scratch("g7-recovered-unused").join(preview::RECOVERED_FOLDER);

    let kept = keep_unsaved_edits_over(&mut tabs, &recovery, SystemTime::now());

    for (at, file) in [&path, &second].into_iter().enumerate() {
        assert!(
            std::fs::read_to_string(file)
                .expect("read the file back")
                .contains("typed by hand"),
            "the file carries the edit"
        );
        assert_eq!(
            kept[at],
            preview::Kept::Saved {
                name: "notes.md".to_owned(),
                file: Some(file.clone()),
            }
        );
        assert!(
            kept[at].line().contains(&file.display().to_string()),
            "{}",
            kept[at].line()
        );
        assert!(!still_holds_the_edit(&tabs[at], file), "and it is clean");
    }
    assert!(!recovery.exists(), "nothing needed a copy");
    for file in [path, second] {
        let _ = std::fs::remove_dir_all(file.parent().expect("the scratch folder"));
    }
}

/// RED (D-4, 0.4.8 G7; ledger #28) — **an unwritable target: the edit is copied into the
/// recovery folder, never over the file, and the line says where.** The file refuses the write
/// (read-only on Windows; on Unix its folder is), so the quit would keep the buffer and stay; a
/// stopping process cannot, and copies the body — in the file's own encoding — into the
/// recovery folder, under the instant and the file's name. A file that changed on disk since it
/// was read (the conflict) is the same: the copy is made and the other writer's bytes stay.
///
/// MUTATION: in `PreviewPool::keep_dirty`, drop the copy (answer `Kept::Lost` without calling
/// `recovery_copy`) — no copy, and the line says the edits are lost.
#[test]
fn an_unwritable_target_keeps_the_edit_in_a_recovery_copy_and_says_where() {
    let (path, buffer) = a_file_being_edited("g7-refused");
    let (moved, other) = a_file_being_edited("g7-conflict-改");
    let (tab, _) = tab_with_a_preview(1, vec![buffer]);
    let (other_tab, _) = tab_with_a_preview(2, vec![other]);
    let mut tabs = vec![tab, other_tab];
    let recovery = disk_scratch("g7-recovered-数据").join(preview::RECOVERED_FOLDER);
    // The file read-only refuses the write on Windows; its folder read-only refuses it on Unix,
    // where a rename replaces a read-only file. Both are set everywhere, and each is put back.
    let folder = path.parent().expect("the scratch folder").to_path_buf();
    let kept_file = std::fs::metadata(&path).expect("the file").permissions();
    let kept_folder = std::fs::metadata(&folder)
        .expect("the folder")
        .permissions();
    let refuse = |refused: bool| {
        for (place, kept) in [(&path, &kept_file), (&folder, &kept_folder)] {
            let mut permissions = kept.clone();
            if refused {
                permissions.set_readonly(true);
            }
            std::fs::set_permissions(place, permissions).expect("set the permissions");
        }
    };
    refuse(true);
    std::fs::write(&moved, "another writer — 别人\n").expect("the other writer");
    crate::test_support::move_the_disk_forward(&moved);

    let kept = keep_unsaved_edits_over(&mut tabs, &recovery, SystemTime::UNIX_EPOCH);
    refuse(false);

    assert_eq!(
        std::fs::read_to_string(&path).expect("read the file back"),
        "one\n",
        "the refused file is as it was"
    );
    assert_eq!(
        std::fs::read_to_string(&moved).expect("read the file back"),
        "another writer — 别人\n",
        "and the other writer's bytes are not written over"
    );
    let mut copies = Vec::new();
    for (at, file) in [&path, &moved].into_iter().enumerate() {
        let preview::Kept::Copied {
            name,
            file: Some(from),
            copy,
            ..
        } = &kept[at]
        else {
            panic!("{:?}", kept[at]);
        };
        assert_eq!((name.as_str(), from), ("notes.md", file));
        assert_eq!(copy.parent(), Some(recovery.as_path()));
        assert!(
            std::fs::read_to_string(copy)
                .expect("the copy")
                .contains("typed by hand"),
            "the copy holds the edit"
        );
        let line = kept[at].line();
        assert!(
            line.contains(&file.display().to_string())
                && line.contains(&copy.display().to_string()),
            "{line}"
        );
        copies.push(copy.clone());
    }
    assert_eq!(
        copies
            .iter()
            .map(|copy| copy
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned())
            .collect::<Vec<_>>(),
        [
            "1970-01-01T000000Z notes.md",
            "1970-01-01T000000Z (1) notes.md"
        ],
        "two copies of one name from one instant are two files"
    );
    for file in [path, moved] {
        let _ = std::fs::remove_dir_all(file.parent().expect("the scratch folder"));
    }
    let _ = std::fs::remove_dir_all(recovery.parent().expect("the scratch folder"));
}

/// **The controlled failure road is one road** (D-4, 0.4.8 G7): a structural guard over the
/// product (source-reading by design: its subject is where the code ends a run).
///
/// Three tables, each the whole of what the product does:
///
/// * [`FAIL_SITES`] — every call of `FolioApp::fail`, by the item it stands in and how many: the
///   twelve sites, each a controlled failure;
/// * [`ROAD`] — every call of `FolioApp::stop_every_window`, the road that keeps every window's
///   unsaved edits and then closes every window with `ending`: `fail`, and `exiting` (a loop
///   stopped by something that is not a window closing);
/// * [`CLOSES`] — every call of `Runtime::close_window` outside the road, with why it does not
///   need the road. A thirteenth site that closed windows for a stop of its own, past the
///   preservation, is a call this table does not have.
///
/// A call standing in no function (a `const`, a `static`) is refused: it has no row.
mod failure_road {
    use std::collections::BTreeMap;
    use std::path::Path;

    use bt_source::{
        DiskScope, Index, ItemIdentity, ItemQuery, Pattern, Search, TargetId, TargetKind,
        TargetRoot, Universe, Vendor, View, needle, report,
    };

    /// One item that calls, how many times, and why.
    pub(super) struct Row {
        pub(super) item: &'static str,
        pub(super) count: usize,
        pub(super) why: &'static str,
    }

    const fn row(item: &'static str, count: usize, why: &'static str) -> Row {
        Row { item, count, why }
    }

    /// The twelve sites of a controlled failure.
    pub(super) const FAIL_SITES: &[Row] = &[
        row(
            "crate::FolioApp::about_to_wait_inner",
            6,
            "the retirement's turn (2), the settle chain, the drag broker, the reap, a window's turn",
        ),
        row(
            "crate::FolioApp::resumed",
            2,
            "a window's first duties after its launch, and a launch that failed",
        ),
        row(
            "crate::FolioApp::user_event",
            1,
            "an application event's handler",
        ),
        row(
            "crate::FolioApp::window_event",
            3,
            "the wheel's flush, the drop's flush, the handler's result",
        ),
    ];

    /// Every caller of the road.
    pub(super) const ROAD: &[Row] = &[
        row("crate::FolioApp::fail", 1, "a controlled failure"),
        row(
            "crate::FolioApp::exiting",
            1,
            "a loop stopped by something that is not a window closing",
        ),
    ];

    /// Every closing of a window, and why it needs no road of its own.
    pub(super) const CLOSES: &[Row] = &[
        row(
            "crate::FolioApp::stop_every_window",
            1,
            "the road itself, after every window kept its unsaved edits",
        ),
        row(
            "crate::FolioApp::close",
            1,
            "the ordinary close, behind the window's dirty gate",
        ),
        row(
            "crate::FolioApp::retire_the_summon_with_the_run",
            1,
            "the summoned terminal, closed with the run's last ordinary window",
        ),
        row(
            "crate::FolioApp::transfer_tab",
            1,
            "a window emptied by a tab's move, not the process: its last tab went to another",
        ),
        row(
            "crate::FolioApp::settle_tear_out",
            1,
            "a torn-out window nothing arrived in, not the process: the tab is still where it was",
        ),
    ];

    /// `crate::module::Type::item`, as the tables name it.
    fn key(identity: &ItemIdentity) -> String {
        match &identity.type_owner {
            Some(owner) => format!("{}::{owner}::{}", identity.module_path, identity.name),
            None => format!("{}::{}", identity.module_path, identity.name),
        }
    }

    /// Every product call of `name(`, by the item it stands in, its own declaration (`owner`'s)
    /// excepted; a call that stands in no function is a failure of its own.
    fn calls(
        index: &Index,
        owner: &str,
        name: &str,
        failures: &mut Vec<String>,
    ) -> BTreeMap<String, usize> {
        let search = Search::new(needle!(Pattern::call(name)), View::Identifiers)
            .exempting_declarations_of(ItemQuery::method(owner, name));
        let found = index
            .search(&search)
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(index);
        if found.outside_items(index) > 0 {
            failures.push(format!(
                "a call of `{name}` stands in no function, so no row can hold it: {}",
                found.report(index)
            ));
        }
        let mut calls = BTreeMap::new();
        for (identity, count) in found.owners(index) {
            *calls.entry(key(&identity)).or_insert(0) += count;
        }
        calls
    }

    /// The differences between `observed` and `table`, each naming `what`.
    fn compare(
        what: &str,
        observed: &BTreeMap<String, usize>,
        table: &[Row],
        failures: &mut Vec<String>,
    ) {
        for (item, count) in observed {
            match table.iter().find(|row| row.item == item) {
                Some(row) if row.count == *count => {}
                Some(row) => failures.push(format!(
                    "{item} calls {what} {count} time(s) in the code and {} in its row ({})",
                    row.count, row.why
                )),
                None => failures.push(format!(
                    "{item} calls {what} {count} time(s) and has no row: a site past the one road"
                )),
            }
        }
        for row in table {
            if !observed.contains_key(row.item) {
                failures.push(format!(
                    "the row for {} ({}) names a call of {what} the code no longer has",
                    row.item, row.why
                ));
            }
        }
    }

    /// **The guard**: every difference between `index` and the three tables.
    pub(super) fn judge(
        index: &Index,
        fail_sites: &[Row],
        road: &[Row],
        closes: &[Row],
    ) -> Vec<String> {
        let mut failures = Vec::new();
        let fails: BTreeMap<String, usize> = calls(index, "FolioApp", "fail", &mut failures)
            .into_iter()
            .filter(|(item, _)| item.starts_with("crate::FolioApp::"))
            .collect();
        compare("`FolioApp::fail`", &fails, fail_sites, &mut failures);
        let roads = calls(index, "FolioApp", "stop_every_window", &mut failures);
        compare("the road", &roads, road, &mut failures);
        let closes_seen = calls(index, "Runtime", "close_window", &mut failures);
        compare(
            "`Runtime::close_window`",
            &closes_seen,
            closes,
            &mut failures,
        );
        failures
    }

    /// RED (D-4, 0.4.8 G7) — **the twelve `fail` sites all enter through the one road, and no
    /// other site closes a window past it.**
    ///
    /// MUTATION (planted below, and on the product): add a function to `impl FolioApp` that
    /// closes every window with `ending` itself — `self.for_each_window(|runtime|
    /// runtime.close_window(true))` — and the guard names it with no row; call
    /// `close_window(true)` in `fail` again in place of the road and it names `fail`.
    #[test]
    fn every_controlled_failure_enters_through_the_one_road() {
        let failures = judge(Index::of_package("bt-app"), FAIL_SITES, ROAD, CLOSES);
        assert!(
            failures.is_empty(),
            "the failure road and its tables differ:\n  {}",
            failures.join("\n  ")
        );
        assert_eq!(
            FAIL_SITES.iter().map(|row| row.count).sum::<usize>(),
            12,
            "the twelve sites"
        );
    }

    /// A crate with a road, its sites, and a thirteenth site that closes past it.
    const PLANTED: &str = r#"
pub struct Runtime;
impl Runtime {
    pub fn close_window(&mut self, _ending: bool) {}
    pub fn keep_unsaved_edits(&mut self) {}
}
pub struct FolioApp {
    runtime: Runtime,
}
impl FolioApp {
    fn fail(&mut self) {
        self.stop_every_window();
    }
    fn stop_every_window(&mut self) {
        self.runtime.keep_unsaved_edits();
        self.runtime.close_window(true);
    }
    fn turn(&mut self) {
        self.fail();
        self.fail();
    }
    fn past_the_road(&mut self) {
        self.runtime.close_window(true);
    }
}
pub struct Archive;
impl Archive {
    fn read(&self) {
        let fail = |_: u8| ();
        fail(0);
    }
}
"#;

    fn planted_index() -> Index {
        let directory = bt_testpath::temp_path("g7-failure-road");
        std::fs::create_dir_all(&directory).expect("a scratch folder");
        let root = directory.join("lib.rs");
        std::fs::write(&root, PLANTED).expect("the planted crate is written");
        let universe = Universe::declare(
            "the planted failure road",
            vec![TargetRoot {
                id: TargetId {
                    package: "planted".to_owned(),
                    kind: TargetKind::Library,
                    name: "planted".to_owned(),
                },
                file: root,
            }],
            vec![DiskScope::under(&directory)],
            Vendor::Excluded,
        )
        .expect("the planted crate is where it was written");
        let index =
            Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
        let _ = std::fs::remove_dir_all(Path::new(&directory));
        index
    }

    /// RED (D-4, 0.4.8 G7) — **the guard names a thirteenth site that closes past the road, and
    /// a `fail` site the table does not count; and passes the same crate whole.** Another type's
    /// `fail` is no site.
    ///
    /// MUTATION: make `compare` skip items with no row — the planted `past_the_road` passes.
    #[test]
    fn the_guard_names_a_site_past_the_road_and_passes_the_crate_whole() {
        let index = planted_index();
        let fail_sites = [row("crate::FolioApp::turn", 2, "planted")];
        let road = [row("crate::FolioApp::fail", 1, "planted")];
        let whole = [
            row("crate::FolioApp::stop_every_window", 1, "the road"),
            row("crate::FolioApp::past_the_road", 1, "planted"),
        ];
        assert_eq!(
            judge(&index, &fail_sites, &road, &whole),
            Vec::<String>::new()
        );

        let failures = judge(&index, &fail_sites, &road, &whole[..1]);
        assert!(
            failures.len() == 1
                && failures[0].contains("crate::FolioApp::past_the_road")
                && failures[0].contains("has no row"),
            "{failures:#?}"
        );
        let counted = [row("crate::FolioApp::turn", 1, "planted")];
        let failures = judge(&index, &counted, &road, &whole);
        assert!(
            failures.len() == 1 && failures[0].contains("2 time(s) in the code and 1"),
            "{failures:#?}"
        );
    }
}
