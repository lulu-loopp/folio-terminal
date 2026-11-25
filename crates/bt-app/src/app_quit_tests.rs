//! **The crate root: the quit.** Tests of items `main.rs` owns about the quit
//! transaction's surfaces — the cancelled quit's card (T-QUIT-DOCUMENTS) — written in the
//! crate root's scope (`use super::*`).

use super::*;

/// RED (T-QUIT-DOCUMENTS, revision (d)2) — **the cancelled quit's card names the files and
/// carries one choice.**
///
/// One Error card on the window: a plain statement — `settings.json could not be saved.`,
/// several files on one card, at most three named and then how many more — and one verb,
/// *Leave without this change*. The full list with each reason is `diagnostics.log`'s.
///
/// MUTATION: raise the card with no choice (`None` for the action in
/// `raise_cancelled_quit_card_on`) — the card offers nothing to press.
#[test]
fn the_cancelled_quits_card_names_the_files_and_carries_one_choice() {
    use i18n::Lang;
    let now = Instant::now();
    let cases: [(&[&'static str], &str); 4] = [
        (&["settings.json"], "settings.json could not be saved."),
        (
            &["settings.json", "profiles.json"],
            "settings.json and profiles.json could not be saved.",
        ),
        (
            &["settings.json", "keybindings.json", "profiles.json"],
            "settings.json, keybindings.json and profiles.json could not be saved.",
        ),
        (
            &[
                "settings.json",
                "keybindings.json",
                "profiles.json",
                "pins.json",
            ],
            "settings.json, keybindings.json, profiles.json and 1 more could not be saved.",
        ),
    ];
    for (files, sentence) in cases {
        let cancelled = quit::Cancelled::of(files);
        assert_eq!(
            i18n::quit_documents_not_saved_in(Lang::English, &cancelled.files()),
            sentence
        );
        let mut toasts = toast::ToastHost::default();
        let card = raise_cancelled_quit_card_on(&mut toasts, &cancelled, Motion::Full, now);
        let [raised] = toasts.toasts() else {
            panic!("one card: {:?}", toasts.toasts());
        };
        assert_eq!(raised.id(), card);
        assert_eq!(raised.kind(), toast::ToastKind::Error);
        assert_eq!(raised.body(), cancelled.sentence(), "the statement");
        assert_eq!(
            raised.action(),
            Some(i18n::Text::QuitLeaveWithoutChange.text()),
            "and its one choice"
        );
        assert_eq!(
            cancelled.lines().len(),
            files.len(),
            "every file, with why, goes to diagnostics.log"
        );
    }
    assert_eq!(
        i18n::Text::QuitLeaveWithoutChange.in_lang(Lang::English),
        "Leave without this change"
    );
}

/// A headless application whose quit has come to its session write: the documents and the
/// recovery phases walked with nothing to do (the recovery folder is this test's own), the
/// picture taken, and a session document recorded and not yet written — into a store at `store`
/// whose save budget is the suite's patience, by a quit whose one budget is the suite's patience
/// too (T-QUIT-FLUSH-BUDGET-PER-QUIT: the write is judged on the writer's answer, and a loaded
/// machine gives the same answer later rather than a timeout) — with the documents phase begun
/// at the instant handed back beside it, read once the window is built (T-TIMING-BOUND-STRAYS:
/// building a headless window is the slow part of this fixture, and the quit's one budget is
/// counted from this instant, so a fixture that read its clock before the build spent the budget
/// on it).
/// The store's folder is the third thing handed back.
fn a_quit_at_its_write(
    tag: &str,
    store: impl FnOnce(&std::path::Path) -> std::path::PathBuf,
) -> (
    crate::headless::HeadlessApplication,
    Instant,
    std::path::PathBuf,
) {
    let folio = crate::headless::HeadlessFolio::with_tabs(
        tag,
        vec![crate::test_support::ringing_tab(1, 1)],
        0,
    );
    let root = folio.scratch.0.clone();
    let mut application = crate::headless::HeadlessApplication::of(folio);
    let app = application.folio.app.as_mut().expect("the application");
    let place = store(&root);
    let mut store =
        persist::SessionStore::at(place.join("session.json"), place.join("session.lock"));
    store.set_save_budget(crate::lane::PATIENCE);
    app.storage_on(store);
    app.pane_retirement_deadline = crate::lane::PATIENCE;
    let begun = Instant::now();
    app.session_store
        .record(crate::test_support::shells_document(&root, 1), begun);
    let mut quit = quit::Quit::begin(Vec::new());
    quit.set_budget(crate::lane::PATIENCE);
    let mut none: [&mut dyn crate::document_request::QuitDocument; 0] = [];
    quit.documents_turn(&mut none, begun);
    quit.recovery_turn(&mut none, &root.join("recovered"), SystemTime::now(), begun);
    assert_eq!(
        quit.photographed(),
        quit::QuitStep::Write,
        "the fixture's own precondition: the quit is at its write"
    );
    app.quit = Some(quit);
    (application, begun, place)
}

/// Whether the window carries the card that says the session was not written.
fn says_not_written(application: &mut crate::headless::HeadlessApplication) -> bool {
    let words = i18n::Text::QuitSessionNotWritten.text();
    application
        .runtime()
        .window
        .toasts
        .toasts()
        .iter()
        .any(|toast| toast.body() == words)
}

/// RED (方案 ③) — **the quit judges its final write, and the verdict is what decides**: a
/// write the disk refused is said on every window and keeps them open, while a write that
/// landed — or a save that ran out of the quit's one budget — leaves, with nothing said.
///
/// MUTATION: swallow the store's answer in `FolioApp::settle_quit`'s `Write` arm
/// (`let landed: Result<(), persist::SaveRefusal> = Ok(());` over the flush's own) — the refused
/// write goes on to hide the windows over a session file it never wrote; read every refusal as
/// one that may proceed — the same; read the clock again in the `Write` arm (`let now =
/// Instant::now();`) instead of judging the write on the turn's own instant — the turn past the
/// budget writes the session anyway (T-TIMING-BOUND-STRAYS); read `persist::SESSION_SAVE_BUDGET`
/// in `Quit::session_write_until` instead of the quit's own budget — the turn past the product's
/// three seconds, still inside this quit's budget, hands nothing over
/// (T-QUIT-FLUSH-BUDGET-PER-QUIT).
#[test]
fn the_final_write_is_judged_and_the_verdict_is_what_decides() {
    crate::test_support::on_the_window_thread();

    // Refused: the store's folder is not there to be written into.
    let (mut refused, begun, _) = a_quit_at_its_write("headless-quit-refused", |root| {
        root.join("这里没有 not-here")
    });
    refused
        .folio
        .settle_quit(&refused.event_loop, begun)
        .expect("the turn is answered");
    assert!(
        says_not_written(&mut refused),
        "a refused write is said on the window"
    );
    let quit = refused.folio.app.as_ref().unwrap().quit.as_ref();
    assert!(
        quit.is_none_or(|quit| !quit.is_retiring()),
        "and the windows stay"
    );

    // Landed: the store's folder is there.
    let (mut landed, begun, _) =
        a_quit_at_its_write("headless-quit-landed", |root| root.to_path_buf());
    landed
        .folio
        .settle_quit(&landed.event_loop, begun)
        .expect("the turn is answered");
    assert!(
        !says_not_written(&mut landed),
        "a landed write says nothing"
    );
    assert!(
        landed
            .folio
            .app
            .as_ref()
            .unwrap()
            .quit
            .as_ref()
            .is_some_and(|quit| quit.document_is_written()),
        "and the quit goes on its way out"
    );

    // Inside the quit's own budget: the turn comes later than the product's three seconds, and
    // the write is judged on what this quit has left, not on the constant.
    let (mut own, begun, folder) =
        a_quit_at_its_write("headless-quit-own-budget", |root| root.to_path_buf());
    own.folio
        .settle_quit(
            &own.event_loop,
            begun + persist::SESSION_SAVE_BUDGET + std::time::Duration::from_secs(1),
        )
        .expect("the turn is answered");
    assert!(
        folder.join("session.json").exists(),
        "a quit with its own budget left hands its write over"
    );
    assert!(
        !says_not_written(&mut own),
        "and it landed: nothing is said"
    );
    assert!(
        own.folio
            .app
            .as_ref()
            .unwrap()
            .quit
            .as_ref()
            .is_some_and(|quit| quit.document_is_written()),
        "and the quit goes on its way out"
    );

    // Out of budget: the turn comes longer after the documents phase began than the quit's one
    // budget — on the turn's own clock, which is the one the write is judged on.
    let (mut spent, begun, folder) =
        a_quit_at_its_write("headless-quit-spent", |root| root.to_path_buf());
    let budget = spent
        .folio
        .app
        .as_ref()
        .and_then(|app| app.quit.as_ref())
        .map(quit::Quit::budget)
        .expect("the quit");
    spent
        .folio
        .settle_quit(
            &spent.event_loop,
            begun + budget + std::time::Duration::from_secs(1),
        )
        .expect("the turn is answered");
    assert!(
        !folder.join("session.json").exists(),
        "a quit with none of its budget left hands nothing over"
    );
    assert!(
        !says_not_written(&mut spent),
        "a save that ran out of its budget is not read as one that was refused"
    );
    assert!(
        spent
            .folio
            .app
            .as_ref()
            .unwrap()
            .quit
            .as_ref()
            .is_some_and(|quit| quit.document_is_written()),
        "and it leaves anyway"
    );
}

/// An update's quit walked to its way out with the session landed (`Handoff::Owed`), on a
/// headless application whose ledger is then emptied.
fn owed_handoff(tag: &str, txn: crate::update_txn::TxnId) -> crate::headless::HeadlessApplication {
    let folio = crate::headless::HeadlessFolio::with_tabs(
        tag,
        vec![crate::test_support::ringing_tab(1, 1)],
        0,
    );
    let mut application = crate::headless::HeadlessApplication::of(folio);
    let now = Instant::now();
    let mut quit = quit::Quit::begin_for(Vec::new(), quit::Reason::UpdateRestart { txn });
    assert_eq!(quit.documents(&[], now), quit::QuitStep::Photograph);
    assert_eq!(quit.photographed(), quit::QuitStep::Write);
    assert_eq!(
        quit.written(quit::WriteVerdict::Landed),
        quit::QuitStep::Retire
    );
    let _ = quit.retired(now);
    assert_eq!(quit.pages(true, now), quit::QuitStep::Exit);
    assert_eq!(quit.handoff(), quit::Handoff::Owed, "the session landed");
    application
        .folio
        .app
        .as_mut()
        .expect("the application")
        .quit = Some(quit);
    let _ = application.event_loop.ledger.take();
    application
}

/// A transaction staged in `root` as Prepare leaves it: the journal durably at `Prepared`, the
/// transaction lock held.
fn staged_in(root: &Path, txn: crate::update_txn::TxnId) -> crate::update_handoff::Staged {
    use crate::update_txn::{Event, Home, Inventories, Journal, Layout};
    let home = Home::at(root.join(".folio-update"));
    std::fs::create_dir_all(home.transaction(txn).join("rescue"))
        .expect("the transaction's folder");
    let rescue = home.transaction(txn).join("rescue").join("folio.exe");
    let journal = Journal::allocate(
        txn,
        rescue.to_string_lossy().into_owned(),
        Layout::Members(Inventories {
            old_shipped: Vec::new(),
            old_present: Vec::new(),
            new: Vec::new(),
        }),
    )
    .advance(&Event::Prepared)
    .expect("Allocated → Prepared");
    bt_platform::install_txn::durable_write(&home.journal(), &journal.encode())
        .expect("the journal is written durably");
    let lock =
        bt_platform::install_txn::try_hold(&home.lock(), bt_platform::install_txn::Hold::Exclusive)
            .expect("the lock file opens")
            .expect("nobody else holds the lock");
    crate::update_handoff::Staged {
        home,
        journal,
        lock,
    }
}

/// RED (T-RESTART-REGISTRATION; review F5) — **an update's hand-over takes the restart
/// registration back on its first step, whether or not the job then goes**: from there on, who
/// opens Folio is the election's, and a reboot before this process has left must not start a
/// third contender.
///
/// The update's quit walked to its way out with the session landed (`Handoff::Owed`), then the
/// loop's own `FolioApp::hand_the_update_over`, twice:
/// - **sent**: a transaction staged in a scratch home and a storage lane only the test reads, so
///   the hand-over job is queued (never run: no journal is advanced, no applier started) — the
///   registration was taken back, and the job is on the lane;
/// - **refused**: nothing staged, so the hand-over is refused and said — the registration was
///   taken back all the same.
///
/// Both effects are this one call's, and the order between the two inside it is not a thing
/// either ledger can see: the withdraw is the arm's first statement, ahead of the job's build.
/// What the two cases hold is that it is asked on both of the arm's roads.
///
/// MUTATION: drop the `B::withdraw_restart` call in `FolioApp::hand_the_update_over` — both
/// cases leave a registration standing; move it into the `Err(why)` arm — the sent case does;
/// move it into the `Ok(answer)` arm — the refused case does.
#[test]
fn the_update_hand_over_takes_the_restart_registration_back() {
    crate::test_support::on_the_window_thread();
    let txn = crate::update_txn::TxnId::new([0x5a; 16]);
    let now = Instant::now();

    let root = bt_testpath::temp_path("bt-restart-交接 handoff");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("the scratch folder");
    let mut application = owed_handoff("handoff-restart-sent", txn);
    let (lane, jobs) = persist::StorageLane::scripted();
    {
        let app = application.folio.app.as_mut().expect("the application");
        app.storage_on(persist::SessionStore::at_on(
            root.join("session.json"),
            root.join("session.lock"),
            lane,
        ));
        app.update_job.stage_for_a_test(staged_in(&root, txn));
    }
    let leaves = application
        .folio
        .hand_the_update_over(&application.event_loop, now);
    let effects = application.event_loop.ledger.take();
    assert!(
        effects.contains(&crate::headless::WindowEffect::RestartWithdrawn),
        "the registration is taken back: {effects:?}"
    );
    assert!(
        !leaves,
        "the job was sent, and the way out waits for its answer"
    );
    assert!(
        matches!(jobs.next(), Some(persist::ScriptedJob::Handoff(_))),
        "the hand-over job is on the lane"
    );
    drop(application);
    let _ = std::fs::remove_dir_all(&root);

    let mut application = owed_handoff("handoff-restart-refused", txn);
    let leaves = application
        .folio
        .hand_the_update_over(&application.event_loop, now);
    let effects = application.event_loop.ledger.take();
    assert!(
        effects.contains(&crate::headless::WindowEffect::RestartWithdrawn),
        "the registration is taken back: {effects:?}"
    );
    assert!(
        leaves,
        "an update with nothing staged is not handed over, and the quit leaves"
    );
}

/// RED (T-STORAGE-LANE-ORDER-BY-CONSTRUCTION, plan row 104, review F1) — **the update
/// hand-over's answer is heard once, and the way out reads the answer heard**: the storage
/// lane's one drain takes the hand-over with its end and lets the request go, so a later drain
/// on the same turn has nothing of it to hear.
///
/// The update's quit at its way out sends the hand-over on a lane the test serves; the test
/// answers it `Started` with an applier and lets the job go, as the worker drops it after its
/// end. Then the turn the worker's wake brings: the wake's drain (`take_document_answers`), and
/// the way out's own drain before it looks (`answer_storage`, the Exit arm's first step). What
/// the way out finds kept is the `Started` answer with that applier, and the request is no longer
/// held. No timing decides it: the test is the worker, and every answer is in before the drains.
///
/// MUTATION: keep the request in `App::handoff_answer` after `App::handoff_answer_below` heard
/// its end — the second drain hears its road closed as the worker gone, `Ended::Gone`, over the
/// `Started` the first kept, and the way out says the storage worker stopped with no applier.
#[test]
fn the_update_hand_overs_answer_is_heard_once_and_the_way_out_reads_it() {
    crate::test_support::on_the_window_thread();
    let txn = crate::update_txn::TxnId::new([0x5b; 16]);
    let now = Instant::now();

    let root = bt_testpath::temp_path("bt-handoff-heard-once 交接一次");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("the scratch folder");
    let mut application = owed_handoff("handoff-heard-once", txn);
    let (lane, jobs) = persist::StorageLane::scripted();
    {
        let app = application.folio.app.as_mut().expect("the application");
        app.storage_on(persist::SessionStore::at_on(
            root.join("session.json"),
            root.join("session.lock"),
            lane,
        ));
        app.update_job.stage_for_a_test(staged_in(&root, txn));
    }
    assert!(
        !application
            .folio
            .hand_the_update_over(&application.event_loop, now),
        "the job was sent, and the way out waits for its answer"
    );
    let Some(persist::ScriptedJob::Handoff(answer)) = jobs.next() else {
        panic!("the hand-over job is on the lane");
    };
    let applier = bt_platform::install_flip::Running {
        pid: 4242,
        started: 7,
    };
    answer
        .send(crate::update_handoff::HandedOff::Started { applier })
        .expect("the way out waits for the answer");
    drop(answer);

    application
        .folio
        .take_document_answers()
        .expect("the wake's drain");
    let _ = application.folio.answer_storage();

    let app = application.folio.app.as_ref().expect("the application");
    assert!(
        matches!(
            &app.handoff_heard,
            Some(persist::Ended::Answered(crate::update_handoff::HandedOff::Started {
                applier: heard
            })) if *heard == applier
        ),
        "the way out finds the Started answer the first drain heard: {:?}",
        app.handoff_heard
    );
    assert!(
        app.handoff_answer.is_none(),
        "the request was let go with the end it was heard at"
    );
    drop(application);
    let _ = std::fs::remove_dir_all(&root);
}

// ── The quit asks while something runs (0.4.10 plan row 87, part 87e; T-TRAY-87E-QUIT-ASKS) ──

/// A command started in an integrated shell: prompt, input, `OSC 133` `C`, and no `D`.
const A_COMMAND_STARTS: &[u8] = b"\x1b]133;A\x07PS> \x1b]133;B\x07cargo build\r\n\x1b]133;C\x07";

/// The line the card says for one command running.
const ONE_COMMAND_LINE: &str = "A command is running; quitting ends it.";

/// A headless application of one window and one tab of one shell, the shell landed live
/// (`test_support::land_a_live_shell`) and then shaped by `shape` — fed bytes, its agent's turns
/// heard, its life ended.
fn a_run_whose_shell(
    tag: &str,
    shape: impl FnOnce(&mut ShellSession),
) -> crate::headless::HeadlessApplication {
    let mut tab = crate::test_support::ringing_tab(1, 1);
    let seat = tab.seats.terminals()[0];
    let shell = &mut tab
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .session;
    crate::test_support::land_a_live_shell(shell);
    shape(shell);
    crate::headless::HeadlessApplication::of(crate::headless::HeadlessFolio::with_tabs(
        tag,
        vec![tab],
        0,
    ))
}

/// A run whose one shell has a command running.
fn a_run_with_a_command(tag: &str) -> crate::headless::HeadlessApplication {
    a_run_whose_shell(tag, |shell| fed(shell, A_COMMAND_STARTS))
}

/// The shell fed `bytes`, as the drain feeds it.
fn fed(shell: &mut ShellSession, bytes: &[u8]) {
    shell.session.feed(bytes).expect("the bytes parse");
}

/// An agent typed into the shell, its turn begun (`Some(true)`) or ended (`Some(false)`) as its
/// hooks said, or never heard (`None`).
fn an_agent_typed(shell: &mut ShellSession, turn: Option<bool>) {
    fed(shell, A_COMMAND_STARTS);
    let steps: &[attention_map::TurnStep] = match turn {
        None => &[],
        Some(true) => &[attention_map::TurnStep::Began],
        Some(false) => &[
            attention_map::TurnStep::Began,
            attention_map::TurnStep::Ended,
        ],
    };
    for (seq, step) in (1_u64..).zip(steps) {
        shell.turn_account.hear_turn_step(
            *step,
            attention_map::CLAUDE_CODE,
            agent_resume::Child::Shell(shell.session.command_marks()),
            seq,
        );
    }
}

/// A person's quit asked for `reason` through the one door, and the loop's begin of it: the step
/// the quit owes.
fn asked(
    application: &mut crate::headless::HeadlessApplication,
    reason: quit::Reason,
) -> quit::QuitStep {
    let app = application.folio.app.as_mut().expect("the application");
    app.quit_reason = reason;
    app.ask_to_quit();
    application
        .folio
        .begin_quit_if_asked()
        .expect("the quit begins");
    step(application)
}

/// What the quit owes now.
fn step(application: &mut crate::headless::HeadlessApplication) -> quit::QuitStep {
    application
        .folio
        .app
        .as_ref()
        .and_then(|app| app.quit.as_ref())
        .map(quit::Quit::step)
        .expect("a quit is under way")
}

/// The card on the window, as it is drawn: its question and its lines.
fn card(
    application: &mut crate::headless::HeadlessApplication,
) -> Option<(&'static str, Vec<String>)> {
    application
        .runtime()
        .quit_card_layout()
        .map(|layout| restore::quit_card_text(&layout))
}

/// RED (T10a; R8) — **every person's door asks once while a command runs**: the chord, the menu
/// bar's Quit, the tray's Quit and the Settings card's Uninstall (each `Reason::Asked` through
/// the one door, as `every_door_onto_the_quit_records_the_same_debt` pins), an update's Restart,
/// and AppKit's quit when no system reason is given. One card, which says what runs; its Quit
/// goes on to the documents and nothing asks again. With nothing running — a command that ended,
/// or a shell printing without shell integration — nothing asks.
///
/// PLANT 87e-P33: ask only on names in `Quit::begin_asking` — no card stands.
#[test]
fn every_persons_door_asks_once_while_a_command_runs() {
    let txn = crate::update_txn::TxnId::new([0x0a; 16]);
    for (door, reason, title) in [
        ("asked", Some(quit::Reason::Asked), "Quit Folio?"),
        (
            "restart",
            Some(quit::Reason::UpdateRestart { txn }),
            "Restart Folio?",
        ),
        ("dock", None, "Quit Folio?"),
    ] {
        let mut application = a_run_with_a_command(&format!("87e-door-{door}"));
        let step_now = match reason {
            Some(reason) => asked(&mut application, reason),
            None => {
                application
                    .folio
                    .begin_the_systems_quit_for(bt_platform::QuitReason::NoneGiven);
                application
                    .folio
                    .begin_quit_if_asked()
                    .expect("the quit begins");
                step(&mut application)
            }
        };
        assert_eq!(step_now, quit::QuitStep::Ask, "{door}: the quit asks");
        assert_eq!(
            card(&mut application),
            Some((title, vec![ONE_COMMAND_LINE.to_owned()])),
            "{door}: one card, saying what runs"
        );
        application
            .runtime()
            .answer_quit_card(quit::QuitAnswer::Quit)
            .expect("the answer is spent");
        assert_eq!(
            step(&mut application),
            quit::QuitStep::Documents,
            "{door}: Quit goes on"
        );
        assert_eq!(
            card(&mut application),
            None,
            "{door}: and asks nothing again"
        );
    }

    for (idle, bytes) in [
        ("ended", [A_COMMAND_STARTS, b"\x1b]133;D;0\x07"].concat()),
        (
            "unintegrated",
            b"PS> cargo build\r\nCompiling bt-app\r\n".to_vec(),
        ),
    ] {
        let mut application =
            a_run_whose_shell(&format!("87e-door-{idle}"), |shell| fed(shell, &bytes));
        assert_eq!(
            asked(&mut application, quit::Reason::Asked),
            quit::QuitStep::Documents,
            "{idle}: nothing runs, and nothing asks"
        );
    }
}

/// RED (T10b; R2) — **an agent mid-turn asks, and an agent waiting at its prompt does not**,
/// though the command it was typed as is still running in its shell's eyes; an agent whose hooks
/// were never heard is read by its shell, as any pane is.
///
/// PLANT 87e-P32: read the shell's `working` for a bound agent's pane — the idle agent asks.
#[test]
fn an_agents_turn_asks_and_an_idle_agent_does_not() {
    let mut working = a_run_whose_shell("87e-agent-working", |shell| {
        an_agent_typed(shell, Some(true));
    });
    assert_eq!(
        asked(&mut working, quit::Reason::Asked),
        quit::QuitStep::Ask
    );
    assert_eq!(
        card(&mut working),
        Some((
            "Quit Folio?",
            vec!["An agent is working; quitting ends it.".to_owned()]
        ))
    );

    let mut waiting = a_run_whose_shell("87e-agent-waiting", |shell| {
        an_agent_typed(shell, Some(false));
    });
    assert_eq!(
        asked(&mut waiting, quit::Reason::Asked),
        quit::QuitStep::Documents,
        "an agent between turns is not working"
    );

    let mut unheard = a_run_whose_shell("87e-agent-unheard", |shell| {
        an_agent_typed(shell, None);
    });
    assert_eq!(
        asked(&mut unheard, quit::Reason::Asked),
        quit::QuitStep::Ask
    );
    assert_eq!(
        card(&mut unheard).map(|(_, lines)| lines),
        Some(vec![ONE_COMMAND_LINE.to_owned()]),
        "unbound, the pane is its shell's: a command running"
    );
}

/// RED (T10c; item 19) — **a command holding the full screen is running work**: `vim`, `less`, an
/// `ssh` end on quit as surely as a build. The tab's breath does not count it, and the quit does.
///
/// PLANT 87e-P17: read the pane by its breath (`session_is_breathing`) — no card.
#[test]
fn a_full_screen_command_is_running_work() {
    let mut application = a_run_whose_shell("87e-full-screen", |shell| {
        fed(shell, &[A_COMMAND_STARTS, b"\x1b[?1049h"].concat());
    });
    assert!(
        !application.runtime().window.tabs[0].fleet_working(),
        "the fixture's own precondition: the tab does not breathe"
    );
    assert_eq!(
        asked(&mut application, quit::Reason::Asked),
        quit::QuitStep::Ask
    );
    assert_eq!(
        card(&mut application).map(|(_, lines)| lines),
        Some(vec![ONE_COMMAND_LINE.to_owned()])
    );
}

/// RED (T10i; the review's P2-1) — **a pane whose shell has ended runs nothing**: a shell the
/// system ended, kept with its face (`Phase::Kept`, plan row 191), and a split pane whose shell
/// exited (`Phase::Exited`) are not running work, though the `C` they last saw stands and nothing
/// will ever write its `D`; nor is a kept pane's agent whose last heard turn had begun.
///
/// PLANT 87e-P41: drop the live-shell guard from `running_work::pane_work` — the ended shells ask.
#[test]
fn a_pane_whose_shell_has_ended_runs_nothing() {
    let kept = |shell: &mut ShellSession| {
        let _ = shell.life.release_dead_shell();
        shell.life.keep_birth_death();
        assert!(
            matches!(shell.life.phase, crate::Phase::Kept),
            "the fixture's own precondition: the pane is kept with its face"
        );
    };
    let mut kept_command = a_run_whose_shell("87e-ended-kept", |shell| {
        fed(shell, A_COMMAND_STARTS);
        kept(shell);
    });
    let mut exited_command = a_run_whose_shell("87e-ended-exited", |shell| {
        fed(shell, A_COMMAND_STARTS);
        assert!(
            shell.life.exited(std::time::Duration::from_secs(60)),
            "the fixture's own precondition: the shell exits"
        );
    });
    let mut kept_agent = a_run_whose_shell("87e-ended-agent", |shell| {
        an_agent_typed(shell, Some(true));
        kept(shell);
    });
    for (case, application) in [
        ("kept", &mut kept_command),
        ("exited", &mut exited_command),
        ("kept agent", &mut kept_agent),
    ] {
        assert!(
            application.runtime().window.tabs[0]
                .sessions
                .values()
                .all(|TerminalPane { session: leaf, .. }| leaf.session.status().working),
            "{case}: the fixture's own precondition: the C stands"
        );
        assert_eq!(
            asked(application, quit::Reason::Asked),
            quit::QuitStep::Documents,
            "{case}: nothing runs, and nothing asks"
        );
    }
}

/// RED (T10d; item 14, R5) — **unsaved names and running work are one card**: a dirty preview
/// buffer beside a running command, through the one door — the unsaved card's question and its
/// three buttons, the names, and the running-work line last.
///
/// PLANT 87e-P34: leave the running-work line off a card that names files — the last line goes.
#[test]
fn unsaved_names_and_running_work_are_one_card() {
    let (mut tab, _) = crate::test_support::tab_with_a_preview(
        1,
        vec![crate::test_support::edited_buffer(
            "/work/notes.md",
            "notes.md",
            "hello",
            " and unsaved",
        )],
    );
    let seat = tab.seats.terminals()[0];
    let shell = &mut tab
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .session;
    crate::test_support::land_a_live_shell(shell);
    fed(shell, A_COMMAND_STARTS);
    let mut application = crate::headless::HeadlessApplication::of(
        crate::headless::HeadlessFolio::with_tabs("87e-one-card", vec![tab], 0),
    );
    assert_eq!(
        asked(&mut application, quit::Reason::Asked),
        quit::QuitStep::Ask
    );
    let names = application
        .folio
        .app
        .as_ref()
        .and_then(|app| app.quit.as_ref())
        .map(|quit| quit.names().to_vec())
        .expect("a quit is under way");
    assert!(
        matches!(names.as_slice(), [name] if name.starts_with("notes.md")),
        "the dirty buffer is named: {names:?}"
    );
    let mut lines = restore::unsaved_lines(&names);
    lines.push(ONE_COMMAND_LINE.to_owned());
    assert_eq!(
        card(&mut application),
        Some((i18n::Text::QuitTitle.text(), lines))
    );
    let layout = application
        .runtime()
        .quit_card_layout()
        .expect("the card stands");
    let (save, _, _) = restore::quit_button_rects(&layout);
    assert!(save.is_some(), "Save all is on it");
}

/// RED (T10e; R4) — **Enter and Esc both cancel the running-work card**: the focused answer is
/// Cancel on it as on every question about losing something, and the windows stay. Each key's
/// answer is spent as the keyboard spends it (`Runtime::answer_quit_card`), and the loop's turn
/// then settles the quit (`FolioApp::settle_quit`): no quit is left, the card is down, and the
/// window is still there.
///
/// PLANT 87e-P39: make `QUIT_FOCUSED_ANSWER` the card's Quit — Enter leaves.
#[test]
fn enter_and_esc_cancel_the_running_work_card() {
    for (key, answer) in [
        ("enter", quit::QUIT_FOCUSED_ANSWER),
        ("esc", quit::QuitAnswer::Cancel),
    ] {
        let mut application = a_run_with_a_command(&format!("87e-keys-{key}"));
        assert_eq!(
            asked(&mut application, quit::Reason::Asked),
            quit::QuitStep::Ask
        );
        application
            .runtime()
            .answer_quit_card(answer)
            .expect("the answer is spent");
        assert_eq!(
            step(&mut application),
            quit::QuitStep::Abandon,
            "{key}: the quit is cancelled"
        );
        application
            .folio
            .settle_quit(&application.event_loop, Instant::now())
            .expect("the loop's turn settles the quit");
        assert!(
            application
                .folio
                .app
                .as_ref()
                .is_some_and(|app| app.quit.is_none()),
            "{key}: no quit is left"
        );
        assert_eq!(
            application.folio.windows.len(),
            1,
            "{key}: the window stays"
        );
        assert_eq!(card(&mut application), None, "{key}: and the card is down");
    }
}

/// RED (T18; R1, R9, the coordinator's ruling on the review's P2-3) — **AppKit's quit asks only
/// for a person's reason, and the system's end is never held by the running-work card nor starts
/// anything on the way out**: the Dock's Quit (no reason) asks; a log-out, a restart, a shut-down
/// and an unknown reason do not; a shut-down that finds a `Cmd+Q`'s running-work card standing
/// answers it Quit; a quit a person asked for that the loop had not yet begun is begun and
/// answered at once; a log-out that finds an update's *Restart Folio?* card standing gives the
/// update up — the job back at `Verified`, its Ready card for the next start — and the quit
/// leaves; one that finds an Uninstall's card standing disarms it and the quit leaves; and the
/// unsaved card stands for a shut-down as it always has.
///
/// PLANT 87e-P20: count `QuitReason::Other` as a person's — the unknown reason asks.
/// PLANT 87e-P42: drop the begin-and-answer block from `FolioApp::begin_the_systems_quit_for` —
/// the standing, pending, restart and uninstall cases stay asking.
/// PLANT 87e-P43: empty `FolioApp::start_nothing_at_the_systems_end` — the update is not given
/// up, and the Uninstall stays armed.
#[test]
fn the_systems_quit_asks_only_for_a_persons_reason() {
    use bt_platform::QuitReason;
    for (reason, asks) in [
        (QuitReason::NoneGiven, true),
        (QuitReason::LogOut, false),
        (QuitReason::Restart, false),
        (QuitReason::ShutDown, false),
        (QuitReason::Other(0x7175_6974), false),
    ] {
        let mut application = a_run_with_a_command(&format!("87e-system-{reason:?}"));
        application.folio.begin_the_systems_quit_for(reason);
        application
            .folio
            .begin_quit_if_asked()
            .expect("the quit begins");
        assert_eq!(
            step(&mut application) == quit::QuitStep::Ask,
            asks,
            "{reason:?}"
        );
    }

    let mut standing = a_run_with_a_command("87e-system-standing");
    assert_eq!(
        asked(&mut standing, quit::Reason::Asked),
        quit::QuitStep::Ask
    );
    standing
        .folio
        .begin_the_systems_quit_for(QuitReason::ShutDown);
    assert_eq!(
        step(&mut standing),
        quit::QuitStep::Documents,
        "the shut-down answers the standing card"
    );
    assert_eq!(card(&mut standing), None, "and takes it down");

    let mut pending = a_run_with_a_command("87e-system-pending");
    pending
        .folio
        .app
        .as_mut()
        .expect("the application")
        .ask_to_quit();
    pending.folio.begin_the_systems_quit_for(QuitReason::LogOut);
    assert_eq!(
        step(&mut pending),
        quit::QuitStep::Documents,
        "the person's pending quit is begun and answered"
    );

    let mut restart = a_run_with_a_command("87e-system-restart");
    let offer = update_job::Offer::mint(
        crate::update_txn::TxnId::new([0x0b; 16]),
        "v0.4.11",
        bt_platform::HostPlatform::Windows,
    )
    .expect("an offer");
    {
        let app = restart.folio.app.as_mut().expect("the application");
        app.update_job = update_job::Job::verified_for_test(offer);
        app.restart_for_update().expect("the Ready card's Restart");
    }
    restart
        .folio
        .begin_quit_if_asked()
        .expect("the quit begins");
    assert_eq!(
        card(&mut restart).map(|(title, _)| title),
        Some("Restart Folio?"),
        "the update's card stands"
    );
    restart.folio.begin_the_systems_quit_for(QuitReason::LogOut);
    assert_eq!(
        step(&mut restart),
        quit::QuitStep::Documents,
        "the log-out's quit leaves"
    );
    let job = &restart
        .folio
        .app
        .as_ref()
        .expect("the application")
        .update_job;
    assert!(
        matches!(job.state(), update_job::State::Verified(_)),
        "the update is given up, and stays ready for the next start"
    );
    assert_eq!(
        job.quit_abandoned(),
        Some(update_job::Abandon::SystemEndingTheSession)
    );

    let mut uninstall = a_run_with_a_command("87e-system-uninstall");
    uninstall
        .folio
        .app
        .as_mut()
        .expect("the application")
        .uninstall_on_quit(false);
    uninstall
        .folio
        .begin_quit_if_asked()
        .expect("the quit begins");
    assert_eq!(step(&mut uninstall), quit::QuitStep::Ask);
    uninstall
        .folio
        .begin_the_systems_quit_for(QuitReason::ShutDown);
    assert_eq!(
        step(&mut uninstall),
        quit::QuitStep::Documents,
        "the shut-down's quit leaves"
    );
    assert!(
        !crate::uninstall::armed_for_a_test(),
        "and starts no uninstaller"
    );

    let mut unsaved = a_run_with_a_command("87e-system-unsaved");
    let work = unsaved
        .folio
        .every_windows_running_work()
        .expect("the windows are read");
    unsaved.folio.app.as_mut().expect("the application").quit = Some(quit::Quit::begin_asking(
        vec!["notes.md".to_owned()],
        work,
        quit::Reason::Asked,
    ));
    unsaved
        .folio
        .begin_the_systems_quit_for(QuitReason::ShutDown);
    assert_eq!(
        step(&mut unsaved),
        quit::QuitStep::Ask,
        "the unsaved card stands"
    );
}
