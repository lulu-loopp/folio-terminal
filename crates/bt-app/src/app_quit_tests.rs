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
    app.session_store =
        persist::SessionStore::at(place.join("session.json"), place.join("session.lock"));
    app.session_store.set_save_budget(crate::lane::PATIENCE);
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
