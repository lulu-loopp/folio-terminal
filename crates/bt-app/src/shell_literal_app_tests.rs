//! **`shell_literal`, as the application drives it.** Tests whose first assertion is about
//! `shell_literal`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    POWERSHELL_PROMPT, THREE_LINES, method_body, paste_leaf, paste_tab, paste_text_into,
    paste_text_into_on, squeezed_body, staged_bytes_sent,
};
use winit::keyboard::{Key, NamedKey};

#[test]
fn unavailable_clipboard_paste_keeps_state_and_allows_a_retry() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"selected").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let selection = ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 7, Bias::After).unwrap().unwrap(),
    };
    session.set_view_selection(Some(selection.clone()));
    projection.set_selection(Some(selection));
    let mut pty_writes = Vec::new();

    let recipient = shell_literal::Recipient {
        encoder: shell_literal::Encoder {
            grammar: shell_literal::ShellGrammar::Posix,
            named_cmd: false,
            delayed_expansion: false,
            powershell_doubled_quotes: &[],
        },
        namespace: bt_transcript::paths::PrintedPathNamespace::Windows,
        spelling: None,
        wsl_distribution: None,
    };
    let unavailable = prepare_clipboard_paste(Err("injected contention".into()), &recipient, false);
    assert!(unavailable.text.is_none());
    assert!(unavailable.notice.is_some());
    assert!(pty_writes.is_empty());
    assert!(session.view_selection().is_some());
    assert!(projection.selection().is_some());
    let retry = prepare_clipboard_paste(
        Ok(bt_platform::ClipboardPayload::Text("paste me".into())),
        &recipient,
        false,
    );
    paste_text(
        &mut session,
        &mut projection,
        retry.text.as_deref().unwrap(),
        |bytes| {
            pty_writes.extend_from_slice(bytes);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(pty_writes, b"paste me");
    assert!(session.view_selection().is_none());
    assert!(projection.selection().is_none());
}

/// PIN — K144. The exact characters an `Insert path into terminal` press
/// puts into the shell.
///
/// Three separate red gates, and each has its own way of going wrong:
/// dropping the quotes turns one argument with a space in it into two; the
/// leading space is what stops the path being welded to a half-typed
/// command; and the trailing one is what lets the next argument be typed
/// without reaching for the space bar first.
#[test]
fn an_inserted_path_is_always_quoted_and_spaced_on_both_sides() {
    let recipient = shell_literal::Recipient {
        encoder: shell_literal::Encoder {
            grammar: shell_literal::ShellGrammar::PowerShell,
            named_cmd: false,
            delayed_expansion: false,
            powershell_doubled_quotes: &[],
        },
        namespace: bt_transcript::paths::PrintedPathNamespace::Windows,
        spelling: None,
        wsl_distribution: None,
    };
    for path in [
        r"C:\work\notes.md",
        r"C:\Program Files\thing.exe",
        r"C:\$RECYCLE.BIN",
    ] {
        for leading in [false, true] {
            let insertion = shell_literal::paths_text(&[path.into()], &recipient, leading);
            assert_eq!(
                insertion.text,
                format!("{}'{path}' ", if leading { " " } else { "" })
            );
        }
    }
}

/// The bytes the one writer puts on the child's input for `text`.
fn paste_bytes_sent(tab: &mut TabState, seat: SeatId, text: &str) -> Vec<u8> {
    let leaf = tab.sessions.get_mut(&seat).expect("the seat has a shell");
    let mut sent = Vec::new();
    paste_text(&mut leaf.session, &mut leaf.projection, text, |bytes| {
        sent.extend_from_slice(bytes);
        Ok(())
    })
    .expect("a capture cannot fail");
    sent
}

/// RED (0.4.4 ticket 02) — **a program that asked for bracketed paste is never asked about a
/// paste.**
///
/// `?2004` means the block arrives as one lump and nothing runs, so there is nothing to ask: the
/// paste is sent at once, wrapped, exactly as before this ticket.
///
/// MUTATION: drop `|| facts.bracketed` from `paste_road` — the bracketed pane is held.
#[test]
fn a_bracketed_pane_is_never_asked() {
    let (mut tab, target) = paste_tab(paste_leaf(
        shell_literal::ShellGrammar::Posix,
        b"\x1b[?2004h",
    ));
    let staged = paste_text_into(&mut tab, target, THREE_LINES, true);
    assert_eq!(staged, StagedPaste::Send(THREE_LINES.to_owned()));
    assert!(pending_paste_in(&tab).is_none(), "no card");
    assert_eq!(
        paste_bytes_sent(&mut tab, target.seat, THREE_LINES),
        b"\x1b[200~dir\recho one\rver\x1b[201~",
        "today's bracketed bytes"
    );
}

/// RED (0.4.4 ticket 02) — **a single line is never asked about, including one that carries its
/// own trailing newline.**
///
/// MUTATION: raise the card on `lines >= 1` in `paste_road` — every paste into cmd is held.
#[test]
fn a_single_line_paste_is_never_asked() {
    for text in ["ver", "ver\r\n", "ver\n"] {
        let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
        assert_eq!(
            paste_text_into(&mut tab, target, text, true),
            StagedPaste::Send(text.to_owned()),
            "{text:?}"
        );
        assert!(pending_paste_in(&tab).is_none());
    }
}

/// RED (0.4.4 ticket 02) — **a dropped path is never asked about.**
///
/// Runs the real drop producer on a real file: `prepare_dropped_paste` spells the path and marks
/// the payload as Folio's own, and a payload that is not the clipboard's text never reaches the
/// question — even if it could somehow have two lines, which
/// `input::tests::a_path_insertion_can_never_be_multi_line` says it cannot.
///
/// MUTATION: drop `!facts.clipboard_text ||` from `paste_road` — the second assertion goes red.
#[test]
fn a_dropped_path_is_never_asked() {
    let dir = bt_testpath::temp_path("bt-t02-drop");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("notes.txt");
    std::fs::write(&file, b"x").unwrap();
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    let recipient = tab.sessions[&target.seat].paste_recipient.clone();
    let prepared = prepare_dropped_paste(vec![file.clone(), file], &recipient, false);
    assert!(!prepared.clipboard_text, "a drop is Folio's own spelling");
    let text = prepared.text.expect("the path is spelled");
    assert_eq!(
        stage_paste(
            &mut tab,
            target,
            text.clone(),
            prepared.clipboard_text,
            true,
            bt_platform::HostPlatform::Windows,
            "drop"
        ),
        StagedPaste::Send(text)
    );
    // And the arm, not the count, is what exempts it.
    assert_eq!(
        stage_paste(
            &mut tab,
            target,
            THREE_LINES.to_owned(),
            false,
            true,
            bt_platform::HostPlatform::Windows,
            "drop"
        ),
        StagedPaste::Send(THREE_LINES.to_owned())
    );
    assert!(pending_paste_in(&tab).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

/// RED (0.4.4 ticket 02) — **a multi-line paste into cmd sends nothing until it is answered, and
/// the card says how many lines and into what.**
///
/// cmd reads raw input, so every `\r` is a command (the design note's fact 1, and 01's control
/// arm: two commands ran before any Enter). The paste is held on the leaf; nothing is returned
/// for the writer, which is the whole of "no byte before an answer".
///
/// MUTATION: return `StagedPaste::Send(text)` from the `Ask` arm of `stage_paste` — the first
/// assertion goes red.
#[test]
fn a_multi_line_paste_into_cmd_sends_nothing_until_answered() {
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    assert_eq!(
        paste_text_into(&mut tab, target, THREE_LINES, true),
        StagedPaste::Held
    );
    let (seat, pending) = pending_paste_in(&tab).expect("the card is up");
    assert_eq!(seat, target.seat);
    assert_eq!(pending.lines, 3);
    assert_eq!(pending.target, target);
    assert_eq!(
        pending.text, THREE_LINES,
        "held exactly as the clipboard gave it"
    );
    // The card's line, from the profile the pane shows.
    let shell = profile_banner_name(&tab.sessions[&seat].profile);
    let title = i18n::paste_card_title(pending.lines, &shell);
    assert!(
        title.starts_with("3 ") && title.ends_with(&format!("→ {shell}")),
        "{title}"
    );
}

/// RED (0.4.4 ticket 02) — **`Enter` runs the paste line by line, with exactly today's bytes.**
///
/// "Run line by line" is not a new road: it is the paste the reader would have had without the
/// card, byte for byte, through the same writer.
///
/// MUTATION: send `input::join_lines` of the text for `RunLineByLine` in `paste_answer_text`.
#[test]
fn enter_runs_it_line_by_line_with_todays_bytes() {
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    // Today's bytes: the same paste with the question turned off.
    let StagedPaste::Send(today) = paste_text_into(&mut tab, target, THREE_LINES, false) else {
        panic!("the setting off sends at once");
    };
    let today = paste_bytes_sent(&mut tab, target.seat, &today);
    assert_eq!(
        paste_text_into(&mut tab, target, THREE_LINES, true),
        StagedPaste::Held
    );
    let focus = pending_paste_in(&tab).expect("the card is up").1.focus();
    let answer = paste_card_key(
        &Key::Named(NamedKey::Enter),
        winit::keyboard::ModifiersState::empty(),
        focus,
    );
    assert_eq!(
        answer,
        Some(PasteCardKey::Answer(PasteAnswer::RunLineByLine))
    );
    let pending = take_pending_paste(&mut tab).expect("the answer takes it");
    let text = paste_answer_text(&pending, PasteAnswer::RunLineByLine).expect("it sends");
    assert_eq!(paste_bytes_sent(&mut tab, target.seat, &text), today);
    assert_eq!(today, b"dir\recho one\rver");
    assert!(pending_paste_in(&tab).is_none(), "the card is gone");
}

/// RED (0.4.4 ticket 02) — **`Join into one line` sends one line and no Enter.**
///
/// The word is reached as the owner's ruling of 2026-09-23 has it: `Tab` moves the focus to it
/// and `Enter` activates it.
///
/// MUTATION: make `PasteAnswer::Join` send `pending.text` in `paste_answer_text` — the bytes carry
/// two `\r` and two commands run.
#[test]
fn join_sends_one_line_and_no_enter() {
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    assert_eq!(
        paste_text_into(&mut tab, target, THREE_LINES, true),
        StagedPaste::Held
    );
    let none = winit::keyboard::ModifiersState::empty();
    let answer = paste_card_keys(
        &mut tab,
        &[Key::Named(NamedKey::Tab), Key::Named(NamedKey::Enter)],
        none,
    );
    assert_eq!(answer, Some(PasteAnswer::Join));
    let pending = take_pending_paste(&mut tab).expect("the answer takes it");
    let text = paste_answer_text(&pending, PasteAnswer::Join).expect("it sends");
    let sent = paste_bytes_sent(&mut tab, target.seat, &text);
    assert_eq!(sent, b"dir echo one ver");
    assert!(!sent.contains(&b'\r'), "no Enter: nothing runs");
}

/// RED (45) — **a block wrapped with the shell's continuation mark joins on `Enter`, and the join
/// takes the marks off.**
///
/// Owner, 2026-09-23: a block copied as one command wrapped across lines — every line but the last
/// ending with cmd's `^` — run line by line runs each fragment as its own command. The card
/// now defaults to `Join` for such a block, and the joined line has no `^` in it: once the line
/// is one line the mark is no longer part of the command. Through the real road: the clipboard's
/// text staged into a cmd pane, the key's answer, and the bytes the writer would send.
///
/// MUTATION: ignore the marks (`let continued = None;` in `stage_paste`) — the default stays
/// `Run line by line` and the first assertion goes red.
#[test]
fn a_block_wrapped_with_carets_joins_on_enter_and_loses_its_carets() {
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    let wrapped = "dir ^\r\n  /b ^  \r\n  /s\r\n";
    assert_eq!(
        paste_text_into(&mut tab, target, wrapped, true),
        StagedPaste::Held
    );
    let (_, pending) = pending_paste_in(&tab).expect("the card is up");
    assert_eq!(pending.default_answer(), PasteAnswer::Join);
    assert_eq!(
        pending.focus(),
        PasteAnswer::Join,
        "the focus opens on the default"
    );
    let answer = paste_card_keys(
        &mut tab,
        &[Key::Named(NamedKey::Enter)],
        winit::keyboard::ModifiersState::empty(),
    );
    assert_eq!(answer, Some(PasteAnswer::Join), "Enter joins it");
    let pending = take_pending_paste(&mut tab).expect("the answer takes it");
    let text = paste_answer_text(&pending, PasteAnswer::Join).expect("it sends");
    assert_eq!(text, "dir /b /s");
    let sent = paste_bytes_sent(&mut tab, target.seat, &text);
    assert!(!sent.contains(&b'^'), "the marks came off: {sent:?}");
    assert!(!sent.contains(&b'\r'), "no Enter: nothing runs");
}

/// **Keys pressed on the card in order**, through the window's own step ([`paste_card_key`] then
/// [`paste_card_step`] on the pending paste), and the answer the last of them gave, if any.
fn paste_card_keys(
    tab: &mut TabState,
    keys: &[Key],
    modifiers: winit::keyboard::ModifiersState,
) -> Option<PasteAnswer> {
    let mut answer = None;
    for key in keys {
        let pending = tab
            .sessions
            .values_mut()
            .find_map(|leaf| leaf.pending_paste.as_mut())
            .expect("the card is up");
        answer = paste_card_key(key, modifiers, pending.focus())
            .and_then(|key| paste_card_step(pending, key));
    }
    answer
}

/// RED (45b) — **the paste card is a standard two-button dialog: `Tab` moves the focus, `Enter`
/// activates the focused word, `Esc` cancels.**
///
/// Owner's ruling 2026-09-23, superseding the 2026-09-22 "`Tab` = Join": `Tab` and `Shift+Tab`
/// move the focus between the two words, wrapping, and the focus opens on the default the
/// continuation-mark rule chose. So on a block of commands, `Tab` then `Enter` joins; `Tab` twice
/// is back on the default and `Enter` runs it line by line; and `Esc` cancels wherever the focus
/// is.
///
/// MUTATION: make `Tab` answer `Join` again in `paste_card_key` (the superseded ruling) — `Tab`
/// then `Enter` is spent on the `Tab`, and the first assertion goes red.
#[test]
fn tab_moves_the_paste_cards_focus_and_enter_activates_it() {
    let none = winit::keyboard::ModifiersState::empty();
    let tab_key = Key::Named(NamedKey::Tab);
    let enter = Key::Named(NamedKey::Enter);
    let held = || {
        let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
        assert_eq!(
            paste_text_into(&mut tab, target, THREE_LINES, true),
            StagedPaste::Held
        );
        tab
    };

    let mut once = held();
    assert_eq!(
        paste_card_keys(&mut once, &[tab_key.clone(), enter.clone()], none),
        Some(PasteAnswer::Join),
        "Tab then Enter on a line-by-line default joins"
    );

    let mut twice = held();
    assert_eq!(
        paste_card_keys(&mut twice, &[tab_key.clone(), tab_key.clone()], none),
        None,
        "moving the focus answers nothing and the card stays"
    );
    assert_eq!(
        pending_paste_in(&twice).expect("still up").1.focus(),
        PasteAnswer::RunLineByLine,
        "Tab twice is back on the default"
    );
    assert_eq!(
        paste_card_keys(&mut twice, std::slice::from_ref(&enter), none),
        Some(PasteAnswer::RunLineByLine)
    );

    let mut back = held();
    assert_eq!(
        paste_card_keys(
            &mut back,
            std::slice::from_ref(&tab_key),
            winit::keyboard::ModifiersState::SHIFT
        ),
        None
    );
    assert_eq!(
        pending_paste_in(&back).expect("still up").1.focus(),
        PasteAnswer::Join,
        "Shift+Tab moves the other way, which with two words is the other word"
    );

    let mut moved = held();
    assert_eq!(
        paste_card_keys(
            &mut moved,
            &[tab_key.clone(), Key::Named(NamedKey::Escape)],
            none
        ),
        Some(PasteAnswer::Cancel),
        "Esc cancels wherever the focus is"
    );

    // A wrapped block opens with the focus on Join, and one Tab reaches Run line by line.
    let (mut wrapped, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    assert_eq!(
        paste_text_into(&mut wrapped, target, "dir ^\r\n/b", true),
        StagedPaste::Held
    );
    assert_eq!(
        paste_card_keys(&mut wrapped, &[tab_key, enter], none),
        Some(PasteAnswer::RunLineByLine)
    );
}

/// RED (0.4.4 ticket 02) — **`Esc` sends no bytes and leaves the clipboard alone.**
///
/// The answer takes the paste off its leaf and hands the writer nothing; and the method that
/// spends it never names the clipboard, so there is no road by which a cancel could write it.
///
/// MUTATION: return `Some(pending.text.clone())` for `Cancel` in `paste_answer_text`.
#[test]
fn cancel_sends_no_bytes_and_leaves_the_clipboard() {
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    assert_eq!(
        paste_text_into(&mut tab, target, THREE_LINES, true),
        StagedPaste::Held
    );
    let answer = paste_card_key(
        &Key::Named(NamedKey::Escape),
        winit::keyboard::ModifiersState::empty(),
        PasteAnswer::RunLineByLine,
    );
    assert_eq!(answer, Some(PasteCardKey::Answer(PasteAnswer::Cancel)));
    let pending = take_pending_paste(&mut tab).expect("the answer takes it");
    assert_eq!(paste_answer_text(&pending, PasteAnswer::Cancel), None);
    assert!(pending_paste_in(&tab).is_none(), "the card is gone");
    let spend = method_body("Runtime", "answer_paste_card");
    assert!(
        !spend.contains("clipboard"),
        "the answer reaches the clipboard:\n{spend}"
    );
    assert!(spend.contains("take_pending_paste(&mut self.window.tabs[active])"));
}

/// RED (0.4.4 ticket 02) — **a restarted shell takes its pending paste with it.**
///
/// `Runtime::restart_shell` puts a new `LeafSession` in the seat; the pending paste lived on the
/// old one, so the card has nothing left to project and the old address names nothing.
///
/// MUTATION: keep the pending paste on the window instead of the leaf — it outlives the shell.
#[test]
fn a_restarted_shell_cancels_the_pending_paste() {
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    assert_eq!(
        paste_text_into(&mut tab, target, THREE_LINES, true),
        StagedPaste::Held
    );
    // What `restart_shell` does to the seat.
    tab.sessions.insert(
        target.seat,
        paste_leaf(shell_literal::ShellGrammar::Cmd, b""),
    );
    assert!(
        pending_paste_in(&tab).is_none(),
        "the card is gone with the shell"
    );
    let standing = tab.sessions.get(&target.seat).map(|leaf| leaf.incarnation);
    assert!(!paste_target_is_live(tab.id, standing, target));
}

/// RED (0.4.4 ticket 02) — **an answer spent after the tab stopped being on top sends nothing.**
///
/// The answer lands on a later turn, so the writer re-asks `live_paste_target` before a byte
/// leaves (review X-1). The rule is `paste_target_is_live`; the wiring — the answer leaves only
/// through `send_paste`, and `send_paste` asks first — is pinned on the bodies.
///
/// MUTATION: remove the `live_paste_target` guard at the top of `Runtime::send_paste`.
#[test]
fn an_answer_spent_after_the_tab_moved_sends_nothing() {
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    assert_eq!(
        paste_text_into(&mut tab, target, THREE_LINES, true),
        StagedPaste::Held
    );
    let pending = take_pending_paste(&mut tab).unwrap();
    let standing = tab
        .sessions
        .get(&pending.target.seat)
        .map(|leaf| leaf.incarnation);
    assert!(
        paste_target_is_live(tab.id, standing, pending.target),
        "control"
    );
    assert!(
        !paste_target_is_live(TabId(tab.id.0 + 1), standing, pending.target),
        "another tab is on top: nothing may be written"
    );
    let spend = squeezed_body("Runtime", "answer_paste_card");
    assert!(
        spend.contains("self.send_paste(pending.target,PasteBody::Text(&text),pending.context)?")
    );
    let send = squeezed_body("Runtime", "send_paste");
    let guard = send
        .find("letSome(active)=self.live_paste_target(target)else{returnOk(false);};")
        .expect("the writer re-asks the address");
    assert!(guard < send.find("paste_body(").expect("and then writes"));
}

/// RED (0.4.4 ticket 02) — **with the setting off, a multi-line paste is sent exactly as today.**
///
/// MUTATION: ignore `facts.ask` in `paste_road` — the paste is held with the row off.
#[test]
fn turning_the_setting_off_sends_as_today() {
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    let StagedPaste::Send(text) = paste_text_into(&mut tab, target, THREE_LINES, false) else {
        panic!("the row is off: nothing is held");
    };
    assert!(pending_paste_in(&tab).is_none());
    assert_eq!(
        paste_bytes_sent(&mut tab, target.seat, &text),
        input::paste_bytes(THREE_LINES, false),
        "today's bytes"
    );
    // And the persisted default is on, as the owner ruled.
    assert!(bt_persist::SettingsV1::default().multiline_paste_ask);
    assert!(settings::SettingsValues::sample().multiline_paste_ask);
}

/// RED (0.4.4 ticket 03) — **the input-line road goes to a PowerShell prompt the shell opened in
/// order, on Windows, and to no other pane.**
///
/// The mirror of `the_resize_anchor_chord_goes_to_a_powershell_pane_and_to_no_other_shell`: the
/// byte is Ctrl+V, which PSReadLine answers by pasting the clipboard onto its input line, which
/// `cmd.exe` types as `^V` (the spike's control arm), and which a GNU readline or a PSReadLine on
/// a Unix pty does not bind to a paste. So both gate facts are asked — the paste grammar and the
/// parser-owned prompt — and the host: a Git Bash prompt, a cmd pane, a command still running
/// inside PowerShell, a `B` no `A` opened, and a PowerShell on macOS all get no road bytes.
///
/// MUTATION: drop `&& facts.through_conpty` from `paste_road` (the macOS case goes red), or
/// answer `AsTyped` for `InputLine` in `stage_paste` (the first assertion goes red).
#[test]
fn the_input_line_road_goes_to_a_powershell_pane_and_to_no_other_shell() {
    let (mut tab, target) = paste_tab(paste_leaf(
        shell_literal::ShellGrammar::PowerShell,
        POWERSHELL_PROMPT,
    ));
    let staged = paste_text_into(&mut tab, target, THREE_LINES, true);
    assert_eq!(
        staged,
        StagedPaste::InputLine(std::borrow::Cow::Borrowed(b"\x16"))
    );
    assert_eq!(
        staged_bytes_sent(&mut tab, target.seat, &staged).as_deref(),
        Some(&b"\x16"[..]),
        "the only byte written is Ctrl+V"
    );
    assert!(pending_paste_in(&tab).is_none());

    let windows = bt_platform::HostPlatform::Windows;
    let cases: [(
        &str,
        shell_literal::ShellGrammar,
        &[u8],
        bt_platform::HostPlatform,
    ); 5] = [
        (
            "a Git Bash prompt",
            shell_literal::ShellGrammar::Posix,
            POWERSHELL_PROMPT,
            windows,
        ),
        (
            "cmd",
            shell_literal::ShellGrammar::Cmd,
            b"\x1b]133;A\x07C:\\>\x1b]133;B\x07",
            windows,
        ),
        (
            "a program running inside PowerShell",
            shell_literal::ShellGrammar::PowerShell,
            b"\x1b]133;A\x07PS C:\\> \x1b]133;B\x07python\r\n\x1b]133;C\x07>>> ",
            windows,
        ),
        (
            "a prompt no A opened",
            shell_literal::ShellGrammar::PowerShell,
            b"PS C:\\> \x1b]133;B\x07",
            windows,
        ),
        (
            "PowerShell on macOS",
            shell_literal::ShellGrammar::PowerShell,
            POWERSHELL_PROMPT,
            bt_platform::HostPlatform::MacOs,
        ),
    ];
    for (name, grammar, printed, host) in cases {
        for ask in [true, false] {
            let (mut tab, target) = paste_tab(paste_leaf(grammar, printed));
            let staged = paste_text_into_on(&mut tab, target, THREE_LINES, ask, host);
            assert!(
                !matches!(staged, StagedPaste::InputLine(_)),
                "{name}: took the input-line road"
            );
            match staged_bytes_sent(&mut tab, target.seat, &staged) {
                Some(bytes) => assert_eq!(
                    bytes,
                    input::paste_bytes(THREE_LINES, false),
                    "{name}: today's bytes"
                ),
                None => assert!(ask, "{name}: held with the setting off"),
            }
        }
    }
}

/// RED (0.4.4 ticket 03) — **a PowerShell prompt is never shown the card, whatever the setting.**
///
/// Owner's ruling 2 (2026-09-22): "PowerShell asks nothing". The setting governs the card only,
/// so turning it off changes nothing on this road — the same byte either way.
///
/// MUTATION: move the `InputLine` answer below the `facts.ask` question in `paste_road`.
#[test]
fn a_powershell_pane_is_never_shown_the_card_when_the_road_is_open() {
    for ask in [true, false] {
        let (mut tab, target) = paste_tab(paste_leaf(
            shell_literal::ShellGrammar::PowerShell,
            POWERSHELL_PROMPT,
        ));
        let staged = paste_text_into(&mut tab, target, THREE_LINES, ask);
        assert_ne!(staged, StagedPaste::Held, "ask = {ask}");
        assert!(pending_paste_in(&tab).is_none(), "ask = {ask}");
        assert_eq!(
            staged_bytes_sent(&mut tab, target.seat, &staged).as_deref(),
            Some(&b"\x16"[..]),
            "ask = {ask}"
        );
    }
}

/// RED (0.4.4 ticket 03) — **a paste Folio spelled itself never takes the clipboard road.**
///
/// On that road the shell re-reads the clipboard, so what lands would be the file list the
/// clipboard holds rather than the quoted paths Folio made of it. Runs the real producer on real
/// files: `prepare_clipboard_paste` over a `Files` payload into a PowerShell prompt that is open,
/// and the one writer's bytes are the spelled paths — no `0x16` among them.
///
/// MUTATION: drop `!facts.clipboard_text ||` from `paste_road` — a path list with two lines
/// would reach the road (the second assertion goes red).
#[test]
fn a_transformed_paste_never_takes_the_clipboard_road() {
    let dir = bt_testpath::temp_path("bt-t03-files");
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("one.txt");
    let second = dir.join("two words.txt");
    std::fs::write(&first, b"x").unwrap();
    std::fs::write(&second, b"x").unwrap();
    let (mut tab, target) = paste_tab(paste_leaf(
        shell_literal::ShellGrammar::PowerShell,
        POWERSHELL_PROMPT,
    ));
    let recipient = tab.sessions[&target.seat].paste_recipient.clone();
    let prepared = prepare_clipboard_paste(
        Ok(bt_platform::ClipboardPayload::Files(vec![first, second])),
        &recipient,
        false,
    );
    assert!(!prepared.clipboard_text);
    let text = prepared.text.expect("the paths are spelled");
    let staged = stage_paste(
        &mut tab,
        target,
        text.clone(),
        prepared.clipboard_text,
        true,
        bt_platform::HostPlatform::Windows,
        "files",
    );
    assert_eq!(staged, StagedPaste::Send(text.clone()));
    let sent = staged_bytes_sent(&mut tab, target.seat, &staged).unwrap();
    assert!(!sent.contains(&0x16), "{sent:?}");
    assert_eq!(sent, input::paste_bytes(&text, false));
    // And the arm, not the count, is what keeps it off: even text with two lines that Folio
    // marked as its own never reaches the road.
    assert_eq!(
        stage_paste(
            &mut tab,
            target,
            THREE_LINES.to_owned(),
            false,
            true,
            bt_platform::HostPlatform::Windows,
            "files",
        ),
        StagedPaste::Send(THREE_LINES.to_owned())
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// RED (0.4.4 ticket 03) — **a block Folio has to clean still lands on the input line, as
/// Folio's own bytes.**
///
/// The clipboard road would skip `sanitize_paste`: PSReadLine inserts a control character as it
/// is and deletes a lone `\r`, gluing two lines. So that text goes as the cleaned bytes with
/// each break a Shift+Enter record (the spike's C2) — still unexecuted, still one write.
///
/// MUTATION: answer `PSREADLINE_PASTE_INPUT` unconditionally in `powershell_input_line`.
#[test]
fn a_paste_folio_has_to_clean_lands_on_the_input_line_as_its_own_bytes() {
    for text in ["'a'\x07\r\n'b'", "'a'\r'b'\r'c'"] {
        let (mut tab, target) = paste_tab(paste_leaf(
            shell_literal::ShellGrammar::PowerShell,
            POWERSHELL_PROMPT,
        ));
        let staged = paste_text_into(&mut tab, target, text, true);
        let sent = staged_bytes_sent(&mut tab, target.seat, &staged).unwrap();
        assert_eq!(sent, input::input_line_bytes(text), "{text:?}");
        assert!(!sent.contains(&0x16) && !sent.contains(&b'\r'), "{sent:?}");
    }
}
