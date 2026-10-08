//! **The tests that cannot leave this module's path** — each is named as
//! `tests::<name>` by something outside its own body: a test that runs itself again with
//! `--exact "tests::<name>"`, or a row of `docs/plans/TIMING-BOUND-TESTS.tsv`. Every other
//! test of the crate root is in a file named for what it tests, and their shared fixtures
//! are in [`crate::test_support`].

use super::*;
use crate::test_support::{
    BOTH_NOTIFICATION_ROWS_ON, CARDS_AT_150, CARDS_AT_200, CROSS_DPI, EndSessionHome, LedgerPane,
    POWERSHELL_PROMPT, PtyPresentationHarness, RAIL_FAILED_THEN_PROMPT, RESTORE_CARD_RUNG,
    ResizeGateHarness, RevivedShape, THREE_LINES, TITLE_FRAME, TwoPaneHarness, a_decode,
    a_held_raster, a_local_file, a_local_folder, a_shell, arriving_as, assert_close, at,
    buffer_read_from, buffer_saying, calls_of, card_restore_first, card_restore_fixture,
    card_restore_resize, card_restore_settle, card_restore_widen, cards_column, cell_ink,
    chevron_button, claim_after_a_look, cross_merge, cross_metrics, cross_seats, cross_solve,
    cross_tab, dir_entry, disk_scratch, document_key, engines_settling_to, facts, facts_with,
    files_column, flush_test_wheel, focused_frame, found, found_in, found_in_package,
    frame_row_text, free_fn_body, glance_fixture, grid_of, hyperlink_hit, in_product, item_body,
    latched, launch_plan_on_disk, leaf_saying, ledger_gate, listed, logical_width, markdown_body,
    method_body, mono_caret_block, on_a_screen, on_the_window_thread, one_turn, package_item_body,
    pane_box_of, pane_rects_of, paste_leaf, paste_tab, paste_text_into, paste_text_into_on,
    peek_open, presentation_of, probe_leaf, prose, prose_caret_block, quiet, rail_test_body,
    reader_names, record_as_the_app_does, resolve_focused_pane, rested_bars,
    restored_three_terminals_before_the_window_is_maximized, restored_two_previews_and_a_terminal,
    ringing_tab, row_box, saved_files_and_terminal, saved_tab, scale_task, shells_document, source,
    source_block, split_window, squeezed, squeezed_body, staged_bytes_sent, strip_with_cli_tab,
    tab_holding, tab_with_a_files_column, tab_with_a_preview, text_buffer, the_system_asks,
    the_three_chevrons, wheel_pane_at_top, windows_on_disk, wired,
};
use bt_render::{DARK_CHROME, LIGHT_CHROME};
use bt_source::{ItemQuery, Pattern, Scope, Search, View, needle};
use std::time::Duration;
use winit::keyboard::{Key, NamedKey};

/// Runs the actual production queue in a disposable process. The watchdog
/// kills only this test's child if a regression makes conversion infinite.
///
/// RED GATE: mis-aim the selector — rename this test, or misspell the name the
/// child is given — and the first assertion names the selector and says how
/// many tests it found. It used to be green: a filter that matches nothing
/// makes the harness print `running 0 tests` and exit 0, and the parent reads
/// only the exit status.
#[test]
fn hostile_math_is_refused_and_the_real_decoration_worker_survives() {
    const CHILD: &str = "BT_MATH_ROBUSTNESS_TEST_CHILD";
    /// The name the child is told to run — written once, so that the proof
    /// below and the run itself cannot be about two different tests.
    const SELECTOR: &str = "tests::hostile_math_is_refused_and_the_real_decoration_worker_survives";
    if std::env::var_os(CHILD).is_none() {
        // **A selector that matches nothing is not a pass**
        // (`docs/plans/bt-app-split-prep.md` §6.3, P9). The harness answers a
        // filter that names no test with `running 0 tests` and exit code 0, so
        // a child spawned on a name this file had renamed or misspelled would
        // be a green test that ran nothing at all — the whole of this case
        // lives in the child, and the parent only reads its status. So the
        // harness is asked what the selector names *before* it is run with it,
        // and the answer has to be this one test. `--list` runs nothing, which
        // is why the proof costs a process that exits at once rather than the
        // minute the real run takes.
        let listing = bt_platform::quiet_command(std::env::current_exe().unwrap())
            .args(["--exact", SELECTOR, "--list"])
            .output()
            .expect("the harness can list its own tests");
        let listed = String::from_utf8_lossy(&listing.stdout);
        let named: Vec<&str> = listed
            .lines()
            .filter_map(|line| line.trim_end().strip_suffix(": test"))
            .collect();
        assert_eq!(
            named,
            [SELECTOR],
            "the child selector `{SELECTOR}` names {} test(s) in this binary, and this case is \
             the child's to run. The harness said:\n{listed}",
            named.len()
        );
        let mut child = bt_platform::quiet_command(std::env::current_exe().unwrap())
            .args(["--exact", SELECTOR, "--nocapture"])
            .env(CHILD, "1")
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "worker regression child: {status}");
                return;
            }
            if started.elapsed() > Duration::from_secs(60) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("decoration worker exceeded the process watchdog");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let log = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/math-robustness-panic-test.log");
    std::fs::write(&log, "").unwrap();
    let fatal_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = fatal_calls.clone();
    // Use the production logging/containment hook, with only the final
    // dialog/exit action replaced. Even a failing test opens no UI.
    install_panic_log_hook_at(log.clone(), move |_| {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    let (tasks, requests) = mpsc::channel();
    let (results, completions) = mpsc::channel();
    let worker = bt_platform::spawn_at_priority(
        "bt-test-decoration",
        bt_platform::ThreadPriority::Normal,
        move |ctx| run_decoration_worker(ctx, requests, results, || {}),
    )
    .expect("start decoration worker");
    let leaf = probe_leaf();
    let render = |source: &str, budget| {
        tasks
            .send(MathWorkerRequest::PreviewMath {
                leaf,
                key: Box::new(PreviewMathKey {
                    source: source.to_owned(),
                    mode: MathMode::Display,
                    em_milli_px: 16_000,
                    foreground_rgb: [220, 220, 220],
                }),
            })
            .unwrap();
        let completion = completions
            .recv_timeout(budget)
            .expect("worker must answer within budget");
        let DecorationWorkerCompletion::PreviewMath { result, .. } = completion.completion else {
            panic!("expected formula completion");
        };
        result
    };
    // Warm up fonts/Typst independently of the refusal-time measurement.
    assert!(render("x+1", Duration::from_secs(30)).is_ok());
    let mut exponential = String::new();
    for (name, next) in ('a'..='y').zip('b'..='z') {
        exponential.push_str(&format!(r"\newcommand{{\{name}}}{{\{next}\{next}}}"));
    }
    exponential.push_str(r"\newcommand{\z}{x}\a");
    for (source, expected) in [
        (r"\newcommand{\a}{#}", MathRenderError::ConversionPanic),
        (r"\newcommand{\a}{\a}\a", MathRenderError::MacroCycle),
        (exponential.as_str(), MathRenderError::MacroExpansionLimit),
        // **Typst code in a formula, through the worker the window uses.** The body is
        // deliberately a harmless `1` rather than the loop this refusal exists for: a guard
        // that regressed would draw a "1" and fail this line, where a loop would hang the
        // harness and report nothing. The loops themselves are refused at the conversion
        // boundary, before a compiler is handed anything — `bt_math`'s
        // `a_formula_that_carries_typst_code_is_refused_before_it_is_compiled`.
        (r"x\iftypst #1 \fi", MathRenderError::RawTypstCode),
    ] {
        // Start at Markdown delimiters, then submit the resulting source to
        // the very same PreviewMath branch the UI uses.
        let blocks = preview::parse_markdown(&format!("$${source}$$"));
        let preview::MarkdownBlock::Math { source } = &blocks[0] else {
            panic!("display formula must be detected");
        };
        let seconds = if expected == MathRenderError::ConversionPanic {
            5
        } else {
            1
        };
        assert_eq!(
            render(source, Duration::from_secs(seconds)).unwrap_err(),
            expected
        );
        assert!(render("x+1", Duration::from_secs(10)).is_ok());
    }
    assert!(
        render(
            r"\newcommand{\a}{\b}\newcommand{\b}{\c}\newcommand{\c}{x+1}\a",
            Duration::from_secs(10)
        )
        .is_ok()
    );
    // **The path lane, which is a thread of its own since audit 3 C-2** — a stat that blocks for
    // the redirector's timeout must not stand in front of a formula. Driven here by hand, exactly
    // as the decoration queue above it is.
    let (path_tasks, path_requests) = mpsc::channel();
    let (path_results, path_completions) = mpsc::channel();
    let path_worker =
        std::thread::spawn(move || run_path_verify_worker(path_requests, path_results, || {}));
    path_tasks
        .send(PathWorkerRequest {
            leaf,
            path: log.clone(),
        })
        .unwrap();
    assert!(matches!(
        path_completions
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .completion,
        DecorationWorkerCompletion::VerifiedPath { verdict, .. } if verdict.exists
    ));
    drop(path_tasks);
    path_worker.join().unwrap();
    assert!(std::fs::read_to_string(log).unwrap().contains("unwrap"));
    assert_eq!(fatal_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(!bt_math::render_panic_is_contained());
    drop(tasks);
    worker.join().unwrap();
    assert!(panic::catch_unwind(|| panic!("ordinary panic hook regression probe")).is_err());
    assert_eq!(fatal_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// PIN (user report, 2026-08-25) — **`BT_PROBE_INPUT=` opens no probe and
/// kills nothing.**
///
/// The report was `Folio stopped: read BT_PROBE_INPUT : The system cannot
/// find the path specified. (os error 3)` — a window that never appeared
/// because a variable had been cleared rather than removed. The ruling is
/// this program's own and already written down for `BT_PTY_DUMP`: an emptied
/// variable is off.
///
/// Red gate: read the value into a `PathBuf` without asking whether it names
/// anything, and the empty string becomes a file to open.
#[test]
fn an_emptied_probe_variable_is_off_and_not_a_nameless_file() {
    assert!(
        super::probe_input(None)
            .expect("an unset variable is not a failure")
            .is_none()
    );
    assert!(
        super::probe_input(Some(std::ffi::OsString::new()))
            .expect("an emptied variable is off, not a file this run cannot open")
            .is_none(),
    );
    let named = std::env::temp_dir().join(format!(
        "{}.vt",
        bt_testpath::unique_name("folio-probe-input")
    ));
    std::fs::write(&named, b"\x1b[2J").expect("write a fixture into the scratch directory");
    assert_eq!(
        super::probe_input(Some(named.clone().into_os_string()))
            .expect("a variable that names a readable file is read")
            .as_deref(),
        Some(b"\x1b[2J".as_slice()),
        "and a variable that does name a file still feeds it in"
    );
    let _ = std::fs::remove_file(&named);
}

/// PIN — **the file's three answers and the table's two languages line up,
/// and `System` is the only one that asks the machine.**
///
/// The mapping used to be written out inside `Runtime::new`; it is a free
/// function now because the Language row calls it again on every press, and
/// a second spelling of it is how a row and a settings file come to disagree
/// about what `System` means.
#[test]
fn a_stored_language_resolves_to_the_column_it_names() {
    use bt_persist::LanguageV1;
    assert_eq!(
        super::resolved_language(LanguageV1::English),
        i18n::Lang::English
    );
    assert_eq!(
        super::resolved_language(LanguageV1::Chinese),
        i18n::Lang::Chinese
    );
    assert_eq!(
        super::resolved_language(LanguageV1::System),
        i18n::resolve(i18n::LanguageMode::System, &bt_platform::os_ui_language()),
        "`System` is the machine's answer and nothing else"
    );
}

/// V14 — the graph's six keys, and only those six.
///
/// The negative half is the half worth writing down: every key not in this
/// list has to reach the scroll below it, because a focused preview's
/// `PageDown` is still a page down and its letters are still swallowed by
/// the surface that already swallows them. A translation that claimed one
/// key too many would take a verb away from a surface underneath and there
/// would be nothing on screen to say so.
#[test]
fn a_focused_graph_answers_six_keys_and_leaves_every_other_one_alone() {
    use winit::keyboard::SmolStr;
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::ArrowUp), ModifiersState::empty()),
        Some(git_graph::GraphKey::Up)
    );
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty()),
        Some(git_graph::GraphKey::Down)
    );
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::Home), ModifiersState::empty()),
        Some(git_graph::GraphKey::Home)
    );
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::End), ModifiersState::empty()),
        Some(git_graph::GraphKey::End)
    );
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::Enter), ModifiersState::empty()),
        Some(git_graph::GraphKey::Enter)
    );
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::Escape), ModifiersState::empty()),
        Some(git_graph::GraphKey::Escape)
    );
    for other in [
        Key::Named(NamedKey::PageDown),
        Key::Named(NamedKey::PageUp),
        Key::Named(NamedKey::ArrowLeft),
        Key::Named(NamedKey::ArrowRight),
        Key::Named(NamedKey::Space),
        Key::Named(NamedKey::Tab),
        Key::Character(SmolStr::new("g")),
    ] {
        assert_eq!(
            graph_key_of(&other, ModifiersState::empty()),
            None,
            "{other:?} is not the graph's"
        );
    }
    // And none of the six is a **bare** row of the chord registry — see
    // [`graph_key_of`] for why a seat-local key is not a binding.
    //
    // Bare, and not "the key at all" (2026-08-16): the registry matches its
    // modifiers exactly, so `Ctrl+Shift+↑` walking the command marks and `↑`
    // walking a graph's rows are two different presses and neither can reach
    // the other. What the graph's keys may not survive is a row claiming the
    // *same* press, which is what this asserts.
    //
    // **`Scope::SearchOpen` is exempt, and the ladder is why** (§7.7, W2
    // slice ④). The one row that claims a bare key on this list is
    // `close-search`, and its scope is exactly the state in which §7.1.5's
    // Escape ladder has *already* taken the key: `close_search` answers at
    // its own rung, which stands whole screens above `preview_key` where the
    // graph's rung is. So the row is never the thing that takes an Escape
    // from a graph — with a capsule up, the graph did not have it before
    // this row existed either. What the assertion protects is a row taking
    // one of the six in a state where the ladder would have left it alone,
    // and that is still nobody's.
    for binding in shortcuts::BINDINGS {
        let Some(chord) = &binding.chord else {
            continue;
        };
        if binding.scope == shortcuts::Scope::SearchOpen {
            continue;
        }
        assert!(
            !matches!(
                chord.key,
                shortcuts::ChordKey::Named(
                    NamedKey::ArrowUp
                        | NamedKey::ArrowDown
                        | NamedKey::Home
                        | NamedKey::End
                        | NamedKey::Enter
                        | NamedKey::Escape
                )
            ) || chord.modifiers != ModifiersState::empty(),
            "{:?} claims a key the graph answers bare",
            binding.action
        );
    }
}

/// PIN — an empty family contributes no layer, so the order above is about
/// what is *on screen* and not about eight always-present slots.
///
/// The stack is built every frame with most of it empty; if flattening
/// emitted placeholders, the renderer would be handed — and diff — a list of
/// blank layers that changed every time a family came and went.
#[test]
fn a_family_with_nothing_in_it_adds_nothing_to_the_overlay() {
    assert!(OverlayStack::default().flattened().is_empty());
    let only_float = OverlayStack {
        float: vec![marks::OverlayLayer::default()].into(),
        ..OverlayStack::default()
    };
    assert_eq!(only_float.flattened().len(), 1);
}

// ── the notices (user ruling, 2026-08-16) ──────────────────────────────

/// PIN — **a verb git refused raises exactly one notice, carrying git's own
/// words; a read that failed raises none.**
///
/// This is the whole transient/persistent split, and both halves matter. The
/// first half is what replaced the red banner: the report is made once, at
/// the instant the answer lands, rather than re-derived from a remembered
/// sentence on every frame until the next attempt. The second half is what
/// stops the replacement from being worse than what it replaced — a machine
/// with no git would otherwise raise a card that appears, says so, and takes
/// the only report away again six seconds later.
///
/// Mutation: add `Repo`/`Status` to `git_answer_notice` and the second half
/// goes red; drop the `Checkout` arm and the refusal the user reported —
/// "fatal: 't1-tab-basics' is already used by worktree at …" — is silent.
#[test]
fn a_refused_verb_raises_one_notice_and_a_read_that_failed_raises_none() {
    let root = std::path::PathBuf::from(r"D:\repo");
    let refused = |words: &str| git::GitFault::Refused(words.to_owned());
    let worktree = "fatal: 't1-tab-basics' is already used by worktree at D:/x";

    let checkout = git::GitAnswer::Checkout {
        root: root.clone(),
        target: "t1-tab-basics".to_owned(),
        outcome: Err(refused(worktree)),
    };
    let lock = "fatal: Unable to create '.git/index.lock': File exists.";
    let write = git::GitAnswer::Write {
        root: root.clone(),
        verb: git::GitWriteVerb::Stage,
        paths: vec!["work.rs".to_owned()],
        outcome: Err(refused(lock)),
    };
    assert_eq!(git_answer_notice(&checkout).as_deref(), Some(worktree));
    assert_eq!(git_answer_notice(&write).as_deref(), Some(lock));

    // Every read, and the two verbs that went through: silence.
    for quiet in [
        git::GitAnswer::Repo {
            dir: root.clone(),
            outcome: Err(git::GitFault::GitMissing("no git.exe".to_owned())),
        },
        git::GitAnswer::Status {
            root: root.clone(),
            outcome: Err(refused("fatal: detected dubious ownership")),
        },
        git::GitAnswer::Refs {
            root: root.clone(),
            outcome: Err(git::GitFault::TimedOut),
        },
        git::GitAnswer::Log {
            root: root.clone(),
            skip: 0,
            outcome: Err(refused("fatal: bad object HEAD")),
        },
        git::GitAnswer::Checkout {
            root: root.clone(),
            target: "main".to_owned(),
            outcome: Ok(()),
        },
        git::GitAnswer::Write {
            root,
            verb: git::GitWriteVerb::Stage,
            paths: vec!["work.rs".to_owned()],
            outcome: Ok(()),
        },
    ] {
        assert_eq!(
            git_answer_notice(&quiet),
            None,
            "a persistent fault is not a notice: {quiet:?}"
        );
    }

    // And who is named as speaking, which is the other half of the words:
    // the sentence is git's, so the title has to say so rather than let a
    // paragraph of `fatal:` look like something this window decided.
    assert_eq!(git_panel::git_toast_title(), "Git");
}

// ── one file, one card, from wherever you point at it (2026-08-27) ─────

/// PIN — **a reference printed in the terminal raises the card the files
/// column raises, and a remote address raises nothing.**
///
/// The ruling's first half as a table: *一个文件,不论从哪指向它,都是同一
/// 张卡*. What is being pinned is not really the mapping — it is that the
/// mapping is [`hyperlink_activation`]'s and not a second one: §7.1.5j ①
/// folded every printed shape of a local file into one `file:` link fed to
/// one routing table, and a hover that judged for itself which references
/// are files would drift from the click that opens them. The first symptom
/// of that drift is a card standing over something a press does nothing to.
///
/// MUTATIONS that must turn it red:
/// ① give the directory arm to the glance (`Preview` and `FilesColumn` both
///    to [`ReferenceCard::File`]) — a folder gets a document card that can
///    only refuse it, and the flyout that *is* a folder's card never opens;
/// ② answer a card for [`HyperlinkActivation::Page`] — a remote address
///    raises a card this window cannot fill. It is `Page` and **not**
///    `Browser` that has to stay silent, which is worth knowing: `Browser`
///    is `Ctrl`'s answer and is unreachable at `control: false`, so since
///    2026-08-29 a remote address arrives down the plain half as a page —
///    an arm that *acts*, and still has no card, because both of this
///    window's cards are made of a file on this disk. The arms are spelled
///    out rather than folded into a wildcard so that a future reader has to
///    decide about them rather than inherit a `_`;
/// ③ pass `control: true` — the table's other half is the system's, so
///    every local file stops raising a card the moment `Ctrl` is held down.
#[test]
fn a_reference_in_the_output_raises_the_card_the_files_column_raises() {
    let file = |_: &Path| Some(a_local_file());
    let folder = |_: &Path| Some(a_local_folder());

    // A file, however it was printed: §7.1.5j turns a bare path, a `file:`
    // URI and an OSC 8 target into the same link, so one case covers all
    // three by construction.
    assert_eq!(
        reference_card(
            "file:///C:/Developer/notes.md",
            bt_transcript::paths::PathNamer::ThisWindow,
            &file
        ),
        Some(ReferenceCard::File(PathBuf::from(r"C:\Developer\notes.md")))
    );
    // And every class the card has a body for arrives down that same arm —
    // which is the point: the lane is chosen from the *path*, by the one
    // reader ([`peek_body_kind`]) both hosts go through, and never here.
    for name in ["report.pdf", "page.html", "shot.png", "clip.mp4", "a.bin"] {
        let uri = format!("file:///C:/Developer/{name}");
        assert!(
            matches!(
                reference_card(&uri, bt_transcript::paths::PathNamer::ThisWindow, &file),
                Some(ReferenceCard::File(_))
            ),
            "{name} is a file, and the card decides what to draw of it elsewhere"
        );
    }

    // A folder — including one named like a page, because the directory
    // question is asked before the page question and was settled first.
    assert_eq!(
        reference_card(
            "file:///C:/Developer/src",
            bt_transcript::paths::PathNamer::ThisWindow,
            &folder
        ),
        Some(ReferenceCard::Folder(PathBuf::from(r"C:\Developer\src")))
    );
    assert!(matches!(
        reference_card(
            "file:///C:/Developer/site.html",
            bt_transcript::paths::PathNamer::ThisWindow,
            &folder
        ),
        Some(ReferenceCard::Folder(_))
    ));

    // A share is a file to this door, and the card it raises prints §7.1.3's
    // refusal — the preview's judgement borrowed, exactly as the files
    // column borrows it. The disk is never asked: `is_directory` would stall
    // the loop on a cold server, and the arm above it returns first.
    assert_eq!(
        reference_card(
            "file://server/share/notes.md",
            bt_transcript::paths::PathNamer::ThisWindow,
            &|_| { panic!("a share is answered without touching the network") }
        ),
        Some(ReferenceCard::File(PathBuf::from(
            r"\\server\share\notes.md"
        )))
    );

    // **And nothing at all for what this window has no card of.** A remote
    // address opens on a plain click and still raises nothing: a card is
    // built from a file, and the address names none. A scheme with no arm at
    // all says nothing for the older reason — there is nowhere for it to go.
    for uri in [
        "https://example.com/report.pdf",
        "http://example.com",
        "mailto:someone@example.com",
        "notascheme",
    ] {
        assert_eq!(
            reference_card(uri, bt_transcript::paths::PathNamer::ThisWindow, &file),
            None,
            "{uri} has no destination inside this window, so it has no card"
        );
    }
}

/// PIN — **the card over a reference stands beside the run the underline is
/// lit on, one row's worth, on the row the hand is on.**
///
/// [`bt_viewport::ViewportFrame::hyperlink_cells`] hands over a segment that
/// may cover the tail of one row and the head of the next, and its own note
/// says a consumer "gets a row's worth at a time and must union or choose".
/// This chooses, and the choice is the pointer's row.
///
/// MUTATIONS that must turn it red:
/// ① drop the `cell / columns != row` filter — a wrapped reference is
///    boxed from column 0 to the pane's last column and the card is placed
///    against a rectangle with a hole in it;
/// ② use `last` instead of `last + 1` for the right edge — the card
///    overlaps the reference's final character;
/// ③ take the row's height from the cell height instead of the frame's own
///    interval — a pane with a formula in it places every card below the
///    block against the wrong row.
#[test]
fn a_card_over_a_reference_stands_beside_the_run_the_underline_is_on() {
    const COLUMNS: u32 = 80;
    let origin = [12.0_f32, 40.0];
    let cell_width = 9.0_f32;

    // A run of six cells on row 3, columns 10..=15.
    let run: Vec<u32> = (10..=15).map(|column| 3 * COLUMNS + column).collect();
    let rect = reference_run_rect(&run, COLUMNS, 3, origin, cell_width, 60.0, 78.0)
        .expect("the run is on the row asked about");
    assert_eq!(
        rect,
        [
            origin[0] + 10.0 * cell_width,
            origin[1] + 60.0,
            origin[0] + 16.0 * cell_width,
            origin[1] + 78.0,
        ],
        "the whole run, its last cell included, and the row's own interval"
    );

    // **A wrapped reference is two boxes and this is the one under the
    // hand.** The same segment, asked about each of its rows in turn.
    let wrapped: Vec<u32> = (76..80)
        .map(|column| 3 * COLUMNS + column)
        .chain((0..4).map(|column| 4 * COLUMNS + column))
        .collect();
    let upper = reference_run_rect(&wrapped, COLUMNS, 3, origin, cell_width, 60.0, 78.0)
        .expect("the tail of row three");
    let lower = reference_run_rect(&wrapped, COLUMNS, 4, origin, cell_width, 78.0, 96.0)
        .expect("the head of row four");
    assert_eq!(
        upper,
        [
            origin[0] + 76.0 * cell_width,
            origin[1] + 60.0,
            origin[0] + 80.0 * cell_width,
            origin[1] + 78.0,
        ]
    );
    assert_eq!(
        lower,
        [
            origin[0],
            origin[1] + 78.0,
            origin[0] + 4.0 * cell_width,
            origin[1] + 96.0,
        ]
    );
    assert!(
        upper[0] > lower[2],
        "and they are not one rectangle: unioning them would box the whole pane"
    );

    // A row the run does not reach has no box, which is how a stale cell
    // index answers nothing rather than answering the wrong thing.
    assert_eq!(
        reference_run_rect(&run, COLUMNS, 9, origin, cell_width, 0.0, 18.0),
        None
    );
    assert_eq!(
        reference_run_rect(&[], COLUMNS, 3, origin, cell_width, 60.0, 78.0),
        None
    );
}

/// PIN — **the window a card becomes opens with the card's own top-left,
/// held exactly where the hand is holding it.**
///
/// The ruling's second half: *一张卡,不论它是什么,拖头就变浮窗*, and
/// "拖动跟手" is the clause this is about. A peek float keeps its own
/// frame and so cannot move; a glance card becomes something bigger than it
/// was, so the question is which edge stays — and it is the top-left,
/// because the head is there and the hand is on the head.
///
/// MUTATIONS that must turn it red:
/// ① measure the grab before the clamp — a card near the window's bottom
///    edge promotes to a window that then drifts away from the pointer by
///    however far the clamp moved it, on every move after the first;
/// ② centre the window on the card instead of sharing its corner — the
///    thing being carried slides out from under the finger the moment it is
///    picked up;
/// ③ drop the clamp — a card at the bottom of the window promotes to a
///    window whose foot is off the glass.
#[test]
fn a_card_promoted_to_a_window_keeps_the_corner_the_hand_is_on() {
    const SCALE: f32 = 1.0;
    let viewport = [0.0_f32, 0.0, 1600.0, 900.0];
    // A card of the module's own width, standing clear of every edge.
    let card = [400.0_f32, 200.0, 700.0, 464.0];
    let size = [520.0_f32, 480.0];
    let pointer = [460.0_f32, 212.0];

    let (placed, grab) = file_peek_promotion(card, size, pointer, viewport, SCALE);
    assert_eq!(
        [placed[0], placed[1]],
        [card[0], card[1]],
        "the head does not move, because the hand is on the head"
    );
    assert_eq!(
        [placed[2] - placed[0], placed[3] - placed[1]],
        size,
        "and the window is the size it asked to be"
    );
    assert_eq!(
        grab,
        [60.0, 12.0],
        "the offset inside the frame that opened"
    );
    assert_eq!(
        float::float_dragged_to(placed, pointer, grab, viewport, SCALE),
        placed,
        "so the move that opened it moves it no further"
    );

    // **Near the bottom edge the clamp overrules the corner — once.** The
    // grab is measured after it, so the window is held where it actually
    // stands rather than where it asked to stand.
    let low = [400.0_f32, 700.0, 700.0, 880.0];
    let low_pointer = [460.0_f32, 712.0];
    let (placed, grab) = file_peek_promotion(low, size, low_pointer, viewport, SCALE);
    assert!(
        placed[3] <= viewport[3],
        "a window promoted at the bottom of the glass is still on the glass"
    );
    assert!(
        placed[1] < low[1],
        "which it can only be by having moved up"
    );
    assert_eq!(
        float::float_dragged_to(placed, low_pointer, grab, viewport, SCALE),
        placed,
        "and it does not drift away from the hand on the next move"
    );
    // **And the move after that is the hand's alone.** Carried up into open
    // ground it travels exactly as far as the hand did — not that plus the
    // distance the clamp had already corrected. Asked out here rather than
    // down against the edge, where the drag's own clamp would put a wrong
    // frame back where the right one is and hide the difference entirely.
    let carried = [low_pointer[0] - 40.0, low_pointer[1] - 300.0];
    assert_eq!(
        float::float_dragged_to(placed, carried, grab, viewport, SCALE),
        [
            placed[0] - 40.0,
            placed[1] - 300.0,
            placed[2] - 40.0,
            placed[3] - 300.0,
        ],
        "the window follows the hand and never the clamp's own correction"
    );
}

/// PIN (GitHub issue #3) — **opening the dialog asks the machine for
/// nothing.**
///
/// An outside user reported the window freezing for seconds when the gear is
/// clicked. The cause was one line of this file: `settings_values` asks
/// `settings::family_index` which family is ticked, on the press that opens
/// the dialog, and the list behind it had just been marked stale by the same
/// press — so every open walked DirectWrite's whole system font collection on
/// this thread, opening a font face per family to name its files.
///
/// What this pins is the negative, which is the only half a counter can
/// state and the only half that was ever in doubt: **reading the list a
/// frame draws performs no walk**. That covers the press, the hover, the hit
/// test and the draw, because all four reach the list through exactly these
/// two functions.
///
/// A delta and not a total, because the counter belongs to the thread and
/// the test harness may run another test on it first.
///
/// Red gate: put the enumeration back behind `monospace_families` — the
/// revision-keyed `MonospaceFamilySlot::get` this replaced — and the count
/// moves on the first line. See `settings::MonospaceFamilySlot` for the
/// shape that keeps it still, and the three tests beside it for the halves a
/// counter cannot state.
#[test]
fn opening_the_dialog_asks_the_machine_for_no_fonts() {
    let before = settings::monospace_scans();
    let list = settings::monospace_families();
    // Every road the dialog takes to the list, in the order the press takes
    // them: which row is ticked, how many rows there are, and what each one
    // reads.
    let ticked = settings::family_index(bt_platform::DEFAULT_MONOSPACE_FAMILY);
    let cjk = settings::cjk_families();
    let cjk_ticked = settings::cjk_family_index("");
    let drawn: Vec<&str> = list.iter().map(|family| family.name.as_str()).collect();
    let again = settings::monospace_families();
    assert_eq!(
        settings::monospace_scans(),
        before,
        "the dialog read both family lists {} times and walked no font \
             collection to do it (ticked rows {ticked}/{cjk_ticked}, {} families drawn)",
        drawn.len() + cjk.len() + 5,
        drawn.len(),
    );
    assert_eq!(
        list.as_ptr(),
        again.as_ptr(),
        "and two reads are one list, so a page redrawn on hover cannot be \
             drawn from two"
    );
}

/// PIN (D8(a) of the 2026-09-11 adversarial review) — **the box's advisory
/// asks the folder how it tells names apart, and gets the answer the commit
/// gets.**
///
/// The advisory compared the tree's rows with the typed name by exact bytes
/// while the commit compared with `path.exists()`, which on an ordinary
/// Windows volume folds case. So with `Notes.md` in the folder, `notes.md`
/// drew no red, Enter hit `NewNameRefusal::Taken`, the field stayed open —
/// and sat there looking valid and doing nothing, for ever. Exact matching is
/// *right* on a case-sensitive directory; the defect was that nobody asked
/// the directory which it was.
///
/// Run against a real folder on **this** volume, because that is the only
/// place the question has an answer: a Windows directory can carry the
/// case-sensitivity flag WSL sets, and a pin that hard-coded either answer
/// would be the defect written down as a test.
///
/// RED GATE: compare the rows with `==` and the second assertion goes red
/// wherever `path.exists()` folds case — which is every ordinary volume.
#[test]
fn a_case_different_duplicate_is_refused_in_the_box_on_this_volume() {
    let dir = bt_testpath::temp_path("bt-name-case");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory to ask about");
    std::fs::write(dir.join("Notes.md"), b"n").expect("one entry in it");

    let folds = bt_platform::directory_folds_case(&dir);
    let taken = dir.join("notes.md").exists();
    assert_eq!(
        folds, taken,
        "what the folder says about case is what `path.exists()` — the \
             commit's own question — does with a different spelling"
    );
    assert_eq!(
        files::names_are_one("/notes.md", "/Notes.md", folds),
        taken,
        "so the advisory drawn in the box answers the same way the commit will"
    );
    assert!(files::names_are_one("/notes.md", "/notes.md", folds));
    assert!(!files::names_are_one("/notes.md", "/other.md", folds));
    assert!(
        !files::names_are_one("/notes.md", "/Notes.md", false),
        "and a folder that tells case apart is still told apart"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ── B-RESTORE-PINNED: an unclean exit restores what a clean one does ──

/// A tab in the owner's shape — a files column beside a shell, both standing in
/// `cwd` — pinned or not.
fn saved_split_tab(cwd: &Path, pinned: bool) -> TabV1 {
    let cwd = cwd.to_string_lossy().into_owned();
    TabV1 {
        root: LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
            dir: bt_persist::SplitDirV1::Row,
            ratio: 300_000,
            children: [
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Files(
                    bt_persist::FilesLeafV1 {
                        view: bt_persist::FilesViewV1::Files,
                        root: cwd.clone(),
                        open: Vec::new(),
                        sel: None,
                        width: 240,
                        remotes_open: false,
                    },
                ))),
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                    profile_id: "pwsh".to_owned(),
                    cwd,
                    manual_name: None,
                    card_skip: 0,
                    last_command: String::new(),
                }))),
            ],
        }),
        pinned,
        focused_leaf: "leaf-1".to_owned(),
        preview: None,
    }
}

/// **The launch's whole restore road, from the bytes on disk** — the session
/// written by the real writer into a scratch home, the run's sentinel left
/// standing when `crashed` (the process was killed before its clean-exit path),
/// then the real probe, the real reader, `plan_windows`, `plan_launch` and
/// `revive_plan`. Answers what the probe said, the first window's plan and the
/// shape each opened tab is revived as.
fn launch_from_disk(
    home: &Path,
    tabs: Vec<TabV1>,
    active_tab: u32,
    crashed: bool,
) -> (bt_persist::ExitState, LaunchPlan, Vec<RevivedShape>) {
    let _ = std::fs::remove_dir_all(home);
    std::fs::create_dir_all(home).expect("a scratch home");
    let session_path = home.join("session.json");
    let sentinel_path = home.join("session.lock");
    let document = bt_persist::SessionV1 {
        windows: vec![bt_persist::SessionWindowV1 {
            tabs,
            active_tab,
            ..bt_persist::SessionWindowV1::default()
        }],
        ..bt_persist::SessionV1::default()
    };
    bt_persist::write_session_atomic(&session_path, &document).expect("the session is written");
    if crashed {
        bt_persist::create_sentinel(&sentinel_path).expect("the run's sentinel");
    }
    let exit = bt_persist::probe_sentinel(&sentinel_path).expect("the sentinel is asked about");
    let (plan, shapes) = launch_plan_on_disk(&session_path);
    let _ = std::fs::remove_dir_all(home);
    (exit, plan, shapes)
}

fn restore_home(name: &str) -> PathBuf {
    bt_testpath::temp_path(&format!("bt-restore-pinned-{name}"))
}

/// PIN (B-RESTORE-PINNED) — **after an unclean exit, a pinned tab comes back
/// with its whole saved tree: every leaf, each shell in its own folder.**
///
/// The owner's report (2026-09-27, 0.4.4, after a reboot) was pinned tabs back
/// as one shell each. The launch reads the sentinel only to log it
/// (`SessionStore::open`); nothing on the restore road asks how the last run
/// ended, and a pinned tab is revived by the same `revive_plan` as a Restore, a
/// Recent row and Ctrl+Shift+T. This pins that: two pinned tabs whose roots are
/// `[files | shell]` splits, the sentinel standing, both open with both leaves.
/// Green on BASE as well — the ticket's report says why the loss is not on this
/// road.
///
/// MUTATION: revive a pinned tab from its identity leaf alone (e.g.
/// `Seats::lone_terminal()` for `tab.pinned` in `revive_plan`), and this goes
/// red.
#[test]
fn an_unclean_exit_restores_pinned_tabs_with_their_trees() {
    let home = restore_home("unclean-trees");
    let (exit, plan, shapes) = launch_from_disk(
        &home,
        vec![saved_split_tab(&home, true), saved_split_tab(&home, true)],
        1,
        true,
    );
    assert_eq!(
        exit,
        bt_persist::ExitState::Crashed,
        "the sentinel says the last run did not reach its clean exit"
    );
    assert_eq!(plan.open.len(), 2, "both pinned tabs open");
    assert_eq!(plan.active_open, Some(1), "on the tab that was in front");
    let whole = (
        vec![bt_layout::SeatKind::Files, bt_layout::SeatKind::Terminal],
        vec![Some(home.clone())],
    );
    assert_eq!(
        shapes,
        vec![whole.clone(), whole],
        "each comes back as its files column and its shell, in its folder"
    );
}

/// PIN (B-RESTORE-PINNED) — **whether the restore card is raised is decided by
/// the pins alone, never by how the last run ended; and a window whose tabs are
/// all pinned opens every tree whole.**
///
/// The owner's shape: three pinned `[files | shell]` tabs and one unpinned.
/// After a crash exactly as after a clean exit, the three open and the fourth
/// is the card's question; with all four pinned there is no question on either
/// road and all four trees open. There is no "all pinned → rebuild from the
/// pins" shortcut: `pins.json` is the table of pinned folders and files, and
/// the launch never reads it for tabs.
///
/// MUTATION: make `plan_launch` leave `ask` empty whenever a tab is pinned, or
/// revive pinned tabs from a seed rather than their tree, and this goes red.
#[test]
fn all_pinned_does_not_skip_the_restore_card_or_the_trees() {
    let home = restore_home("all-pinned");
    let owner = |last_pinned: bool| {
        vec![
            saved_split_tab(&home, true),
            saved_split_tab(&home, true),
            saved_split_tab(&home, true),
            saved_split_tab(&home, last_pinned),
        ]
    };
    let whole = (
        vec![bt_layout::SeatKind::Files, bt_layout::SeatKind::Terminal],
        vec![Some(home.clone())],
    );
    for crashed in [true, false] {
        let (_, plan, shapes) = launch_from_disk(&home, owner(false), 0, crashed);
        assert_eq!(
            plan.ask,
            vec![saved_split_tab(&home, false)],
            "the unpinned tab is the card's question (crashed = {crashed})"
        );
        assert_eq!(shapes, vec![whole.clone(); 3], "crashed = {crashed}");

        let (_, plan, shapes) = launch_from_disk(&home, owner(true), 0, crashed);
        assert!(
            plan.ask.is_empty(),
            "nothing unpinned, nothing to ask (crashed = {crashed})"
        );
        assert!(!plan.placeholder, "crashed = {crashed}");
        assert_eq!(shapes, vec![whole.clone(); 4], "crashed = {crashed}");
    }
}

/// PIN (B-RESTORE-PINNED) — **a clean exit's restore is the unclean exit's
/// restore**: the same document, with and without the sentinel, gives the same
/// plan and the same revived trees.
///
/// MUTATION: let the restore road read the sentinel (e.g. `read_session`
/// answering the default document while `session.lock` stands beside the file,
/// or `plan_launch` opening nothing but the pinned tabs' identity leaves after a
/// crash), and this goes red.
#[test]
fn a_clean_exit_restore_is_unchanged() {
    let home = restore_home("clean");
    let tabs = || {
        vec![
            saved_split_tab(&home, true),
            saved_split_tab(&home, false),
            saved_split_tab(&home, false),
        ]
    };
    let clean = launch_from_disk(&home, tabs(), 0, false);
    let unclean = launch_from_disk(&home, tabs(), 0, true);
    assert_eq!(clean.0, bt_persist::ExitState::Normal);
    assert_eq!(unclean.0, bt_persist::ExitState::Crashed);
    assert_eq!(clean.1, unclean.1, "one plan either way");
    assert_eq!(clean.2, unclean.2, "one set of trees either way");
    assert_eq!(clean.1.open, vec![saved_split_tab(&home, true)]);
    assert_eq!(
        clean.1.ask.len(),
        2,
        "the two unpinned tabs are asked about"
    );
}

/// RED (B-ENDSESSION) — **the system's question holds the document, writes it through the quit's
/// road and drops the run's sentinel; nothing that happens after it changes the file.**
///
/// The owner's reboot of 2026-09-27 came back with every pinned tab short of its panes and the
/// run marked unclean: `WM_QUERYENDSESSION` went to `DefWindowProc`, the system ended the shells,
/// each death closed a pane, and the autosave wrote what was left. Here the real platform answer
/// hears the question, the real store writes the held document through the one writer and the
/// one bounded wait, and the "pane closed" recordings that follow — the reap's — are refused.
///
/// MUTATION: in `session_end::hear`, park the question without holding the document — the
/// shrunken layout is what lands, and `session.lock` is still dropped over it.
#[test]
fn a_query_end_session_freezes_writes_and_drops_the_sentinel() {
    on_the_window_thread();
    let home = EndSessionHome::new("query");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    assert!(home.sentinel().is_file(), "the run's sentinel stands");
    let whole = shells_document(&home.0, 2);
    record_as_the_app_does(&mut store, whole.clone());

    assert_eq!(the_system_asks(), Some(1), "the question is answered TRUE");
    // The system ends one shell of each tab; the reap closes their panes and records.
    record_as_the_app_does(&mut store, shells_document(&home.0, 1));
    let settled: Vec<_> = session_end::take()
        .into_iter()
        .map(|end| session_end::settle(end, &mut store, false))
        .collect();
    assert_eq!(settled, vec![session_end::Settled::Saved(Ok(()))]);
    assert_eq!(
        windows_on_disk(&home.session()),
        whole.windows,
        "the layout on the disk is the one before the system's question"
    );
    assert!(
        !home.sentinel().exists(),
        "and the run claims its clean exit, as a quit does"
    );

    // Later "pane closed" edits, and a write forced after them: the file does not move.
    record_as_the_app_does(&mut store, shells_document(&home.0, 1));
    assert!(bt_platform::admission::exiting());
    assert_eq!(store.flush_judged(), Ok(()));
    assert_eq!(windows_on_disk(&home.session()), whole.windows);
}

/// PIN (B-ENDSESSION) — **without the system's question, a shell that exits still closes its
/// pane, and the layout on the disk says so.**
///
/// The behaviour the hold must not touch: a shell that ends for its own reasons (`exit`, a crash)
/// is a change the reader made, recorded and written as before; the run's sentinel stands.
///
/// MUTATION: make `session_end::holds_the_document` answer `true` — the closed pane never
/// reaches the file.
#[test]
fn an_ordinary_shell_exit_without_a_freeze_still_closes_the_pane() {
    on_the_window_thread();
    let home = EndSessionHome::new("ordinary");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    assert!(bt_platform::admission::exiting());
    assert_eq!(store.flush_judged(), Ok(()));
    record_as_the_app_does(&mut store, shells_document(&home.0, 1));
    assert_eq!(store.flush_judged(), Ok(()));

    assert!(!session_end::holds_the_document());
    assert_eq!(
        windows_on_disk(&home.session()),
        shells_document(&home.0, 1).windows,
        "the closed pane is gone from the layout"
    );
    let (_, shapes) = launch_plan_on_disk(&home.session());
    let one = (
        vec![bt_layout::SeatKind::Terminal],
        vec![Some(home.0.clone())],
    );
    assert_eq!(shapes, vec![one.clone(), one]);
    assert!(
        home.sentinel().is_file(),
        "and nothing claimed a clean exit"
    );
}

/// RED (B-ENDSESSION) — **a shutdown taken back lets the document go and puts the sentinel
/// back: the run goes on, and so do its saves.**
///
/// `WM_ENDSESSION` with `FALSE` — another program, or the person at the shutdown screen, stopped
/// it. Without this the rest of the run would never save its layout again, and a crash after it
/// would pass for a clean exit.
///
/// MUTATION: in `session_end::settle`'s `TakenBack` arm, leave the document held, or drop the
/// `rearm_after_the_systems_end` call.
#[test]
fn a_shutdown_taken_back_lets_the_document_go_and_puts_the_sentinel_back() {
    on_the_window_thread();
    let home = EndSessionHome::new("taken-back");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    assert_eq!(the_system_asks(), Some(1));
    assert_eq!(
        bt_platform::session_end::answer(
            bt_platform::session_end::WM_ENDSESSION,
            0,
            &session_end::hear
        ),
        Some(0),
        "a shutdown taken back is processed"
    );
    let settled: Vec<_> = session_end::take()
        .into_iter()
        .map(|end| session_end::settle(end, &mut store, false))
        .collect();
    assert_eq!(
        settled,
        vec![
            session_end::Settled::Saved(Ok(())),
            session_end::Settled::TakenBack
        ]
    );
    assert!(home.sentinel().is_file(), "the run is running again");
    assert!(!session_end::holds_the_document());

    record_as_the_app_does(&mut store, shells_document(&home.0, 1));
    assert!(bt_platform::admission::exiting());
    assert_eq!(store.flush_judged(), Ok(()));
    assert_eq!(
        windows_on_disk(&home.session()),
        shells_document(&home.0, 1).windows,
        "a change after the shutdown was taken back is saved"
    );
}

/// PIN — mock-up 7426-7431: "Launch asks about exactly one thing, and it is
/// not the pinned tabs. **Pinning IS the answer**."
///
/// Red gate: `Runtime::create` used to rebuild *every* persisted tab
/// unconditionally, which is both halves of this wrong at once — it asked
/// nothing, and it restored what the user may well have meant to close.
#[test]
fn launch_opens_what_you_pinned_and_asks_only_about_the_rest() {
    let saved = [
        saved_tab("pwsh", "C:\\a", None, false),
        saved_tab("pwsh", "C:\\b", None, true),
        saved_tab("pwsh", "C:\\c", None, false),
    ];
    let plan = plan_launch(&saved, 0, false);

    assert_eq!(plan.open, vec![saved[1].clone()], "the pinned one, alone");
    assert_eq!(
        plan.ask,
        vec![saved[0].clone(), saved[2].clone()],
        "the question is the tabs you did not pin, in their own order"
    );
    assert!(
        !plan.placeholder,
        "a pinned tab is already a window worth showing"
    );
    assert_eq!(
        plan.active_open, None,
        "the tab you were on was not pinned, so it is not one of these"
    );
}

/// The seat you were in comes back with you — but only if it was pinned.
#[test]
fn the_tab_you_were_on_keeps_its_seat_when_it_is_one_of_the_pinned() {
    let saved = [
        saved_tab("pwsh", "C:\\a", None, false),
        saved_tab("pwsh", "C:\\b", None, true),
        saved_tab("pwsh", "C:\\c", None, true),
    ];
    // index 2 of the saved list is the second *pinned* tab.
    assert_eq!(plan_launch(&saved, 2, false).active_open, Some(1));
    assert_eq!(plan_launch(&saved, 1, false).active_open, Some(0));
}

/// The boundary ruled in this ticket: "Reopen your **other** tabs?" needs
/// other tabs. With one unpinned tab and nothing pinned there is no question
/// — and declining would have handed back a fresh shell in the wrong folder,
/// which is strictly worse than the tab it replaced.
#[test]
fn a_lone_unpinned_tab_is_restored_rather_than_asked_about() {
    let saved = [saved_tab("pwsh", "C:\\only", None, false)];
    let plan = plan_launch(&saved, 0, false);

    assert_eq!(plan.open, saved.to_vec(), "it simply comes back");
    assert!(plan.ask.is_empty(), "nothing to ask");
    assert!(!plan.placeholder, "it is a real tab, not scaffolding");
    assert_eq!(plan.active_open, Some(0));

    // Two unpinned tabs *are* a question, and then a stand-in shell carries
    // the window until it is answered.
    let two = [
        saved_tab("pwsh", "C:\\a", None, false),
        saved_tab("pwsh", "C:\\b", None, false),
    ];
    let plan = plan_launch(&two, 0, false);
    assert!(plan.open.is_empty());
    assert_eq!(plan.ask.len(), 2);
    assert!(
        plan.placeholder,
        "nothing was pinned, so nothing is standing"
    );
}

#[test]
fn a_first_launch_with_nothing_saved_asks_nothing_and_stands_something_up() {
    let plan = plan_launch(&[], 0, false);
    assert!(plan.open.is_empty());
    assert!(plan.ask.is_empty(), "no prompt on a first run");
    assert!(plan.placeholder);
}

/// PIN (§7.2) — **a command line is not a placeholder**, so a Restore
/// accepted afterwards cannot sweep it away.
///
/// The placeholder exists for one situation: nothing was pinned, and the
/// window needed *something* to be a window with. A pane somebody named a
/// folder for is not that; it is the thing they asked for, and
/// `answer_restore` retires the placeholder without asking.
///
/// MUTATION: drop the `&& !cli_wants_pane` from `placeholder` and this
/// fails — and on the real machine, `folio --cwd D:\proj` followed by
/// "Restore" would close the pane in `D:\proj` while the shells it revived
/// came up.
#[test]
fn a_tab_the_command_line_asked_for_is_never_the_launch_placeholder() {
    let two = [
        saved_tab("pwsh", "C:\\a", None, false),
        saved_tab("pwsh", "C:\\b", None, false),
    ];
    assert!(plan_launch(&two, 0, false).placeholder, "the red half");
    let plan = plan_launch(&two, 0, true);
    assert!(!plan.placeholder);
    assert!(plan.open.is_empty(), "nothing was pinned, so nothing opens");
    assert_eq!(plan.ask.len(), 2, "and the question is still asked");

    // The same on a first run with nothing saved at all: there is one tab and
    // it is the one that was asked for.
    let plan = plan_launch(&[], 0, true);
    assert!(!plan.placeholder);
    assert!(plan.ask.is_empty());
}

/// PIN (§7.2) — **a command line turns a lone saved tab back into a
/// question.**
///
/// The one-tab shortcut above holds because declining leaves the user with a
/// fresh shell in the wrong folder, which is strictly worse than the tab it
/// replaced. A launch that was told the folder has already opened one in the
/// right one, so that premise is gone and the question is a real question.
///
/// MUTATION: drop the `&& !cli_wants_pane` from the shortcut and this fails
/// — `folio --cwd D:\proj` would silently revive last night's tab beside the
/// one that was asked for, with no prompt and no way to say no.
#[test]
fn a_command_line_turns_a_lone_saved_tab_back_into_a_question() {
    let saved = [saved_tab("pwsh", "C:\\only", None, false)];
    let plan = plan_launch(&saved, 0, true);
    assert!(plan.open.is_empty(), "nothing comes back unasked");
    assert_eq!(plan.ask, saved.to_vec(), "it is the prompt's question");
    assert_eq!(plan.active_open, None);

    // A *pinned* tab is an answer already given, and a command line does not
    // reopen that question either: it opens alongside.
    let pinned = [saved_tab("pwsh", "C:\\only", None, true)];
    let plan = plan_launch(&pinned, 0, true);
    assert_eq!(plan.open, pinned.to_vec());
    assert!(plan.ask.is_empty());
}

/// The pure functions §7.2 names, put together over the one launch that
/// used to break: a session with an unpinned tab and two pinned ones, and a
/// `--cwd` on the command line.
///
/// Red gate: with [`cli_tab_slot`] answering `0`, the strip is
/// `cli, pinned, pinned` and the active tab is the one at slot 0 — the first
/// of those is what `debug_assert!` in `tab_trailers` fired on.
#[test]
fn a_launch_told_a_folder_opens_it_at_the_head_of_the_unpinned_run() {
    let saved = [
        saved_tab("pwsh", "C:\\a", None, false),
        saved_tab("pwsh", "C:\\b", None, true),
        saved_tab("pwsh", "C:\\c", None, true),
    ];
    let plan = plan_launch(&saved, 1, true);
    assert_eq!(plan.open, vec![saved[1].clone(), saved[2].clone()]);
    assert_eq!(plan.ask, vec![saved[0].clone()], "the rest is still asked");

    let pins = plan.open.iter().map(|tab| tab.pinned).collect::<Vec<_>>();
    let slot = cli_tab_slot(&pins);
    assert_eq!(slot, 2, "both pinned tabs keep the head they were promised");
    assert!(
        seed::pins_are_normalized(&strip_with_cli_tab(&pins, false), |pinned| *pinned),
        "F57: the pinned run leads the strip"
    );
    assert_eq!(
        launch_active_tab(Some(slot), plan.active_open, plan.open.len() + 1),
        2,
        "and it is still the tab you are put in, rather than the pinned tab \
             you were last on"
    );
}

/// A tab's identity is the terminal it holds, wherever that sits in the tree.
#[test]
fn a_seed_is_read_from_the_first_terminal_in_the_tree() {
    let split = LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
        dir: bt_persist::SplitDirV1::Row,
        ratio: 500_000,
        children: [
            Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Files(
                bt_persist::FilesLeafV1 {
                    view: bt_persist::FilesViewV1::Files,
                    root: "C:\\repo".to_owned(),
                    open: Vec::new(),
                    sel: None,
                    width: 240,
                    remotes_open: false,
                },
            ))),
            Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                profile_id: "pwsh".to_owned(),
                cwd: "C:\\repo\\src".to_owned(),
                manual_name: Some("build".to_owned()),
                card_skip: 0,
                last_command: String::new(),
            }))),
        ],
    });
    let leaf = first_term_leaf(&split).expect("a files pane is not the tab's identity");
    assert_eq!(leaf.cwd, "C:\\repo\\src");
    assert_eq!(leaf.manual_name.as_deref(), Some("build"));

    // A files-only tree has no terminal to speak for it.
    assert!(
        first_term_leaf(&LayoutNodeV1::Leaf(LeafNodeV1::Unknown)).is_none(),
        "an unknown leaf is not a terminal"
    );
}

/// §5.4 逐叶降级, "未知 profile→默认": a profile this build does not have
/// costs you the shell choice, never the tab.
#[test]
fn a_seed_naming_a_profile_we_do_not_have_still_comes_back() {
    let tab = saved_tab("wsl-ubuntu", "C:\\a", Some("notes"), true);
    let (seats, seed, leaves, _files, _preview) = revive_plan(&tab);
    assert_eq!(
        leaves
            .get(&seats.identity())
            .map(|leaf| leaf.profile.as_str()),
        Some(profiles::fallback_profile_id()),
        "an id this build cannot place falls to the default profile"
    );
    assert_eq!(
        seed.manual_name.as_deref(),
        Some("notes"),
        "your name stays"
    );
    assert!(seed.pinned, "and so does the promise");
}

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

/// **A hand on the divider outranks a preview's floor** (最小值主权,
/// 2026-08-08: "a minimum is law to the program and advice to the user").
///
/// Real machine, slice 7: with the layout above on screen, dragging the root
/// divider to give the terminal half the window did *nothing*. The edit was
/// never the problem — `Edit::DragDivider` writes the ratio the hand asked
/// for and consults no minimum — but the solve that turns that ratio into
/// rectangles ran under `Lawful`, so the concession chain put every seat
/// straight back on its floor and folded the terminal again. The divider
/// went dead under the hand.
///
/// MUTATION: solve the dragged tree under `Lawful` (which is what
/// `drive_divider_drag` did before this slice) and every assertion below the
/// first goes red — the terminal comes back 24 pixels wide.
#[test]
fn a_divider_drag_takes_the_room_a_preview_floor_was_holding() {
    let (mut seats, metrics, viewport) = restored_two_previews_and_a_terminal();
    let terminal = seats.identity();
    let root = seats
        .split_slots(
            &seats
                .solve(viewport, &metrics, SizePolicy::Lawful)
                .expect("the folded layout solves"),
        )
        .first()
        .expect("the tree has a root split")
        .id;

    assert_eq!(
        seats.drag_divider(
            &metrics,
            root,
            bt_layout::Ratio::clamped_from_ppm(500_000),
            bt_layout::LogicalPx::px(950),
        ),
        Ok(true),
        "the edit has always honoured the hand — it writes the ratio asked for"
    );

    // What law does with that same tree, and why the hand's rectangles must
    // not go through this door: the tree still does not fit, so law pays for
    // the window by **rearranging** it — a pane becomes a §2.6.3 bar. That is
    // not an answer to "make this divider move"; it is the program deciding
    // the user asked for one pane fewer.
    //
    // (Before the 2026-08-13 collapse-order ruling the bar was the
    // *terminal's* and this line read `logical_width(terminal) ==
    // COLLAPSED_EXTENT` — the divider looked dead because the seat the hand
    // was enlarging was the very seat law folded. Law now folds a preview
    // instead, which is a better layout and still a layout nobody asked for.)
    let lawful = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("still solvable");
    assert_eq!(
        seats
            .preview_seats()
            .into_iter()
            .filter(|seat| presentation_of(&lawful, *seat).is_collapsed_along(bt_layout::Axis::Row))
            .count(),
        1,
        "law buys the room by turning a pane into a bar"
    );

    // And what it does now: nothing folds at all, and past the floors the
    // floors give way together.
    let sovereign = seats
        .solve(viewport, &metrics, SizePolicy::Sovereign)
        .expect("sovereign cannot refuse");
    assert!(
        seats
            .preview_seats()
            .into_iter()
            .chain([terminal])
            .all(|seat| presentation_of(&sovereign, seat) == bt_layout::Presentation::Full),
        "under the hand every pane stays a pane — there are no bars on this road"
    );
    assert!(
        logical_width(&sovereign, terminal) > 200,
        "and it is wide enough to be one: {}",
        logical_width(&sovereign, terminal)
    );
    let previews: Vec<i64> = seats
        .preview_seats()
        .into_iter()
        .map(|seat| logical_width(&sovereign, seat))
        .collect();
    assert!(
        previews
            .iter()
            .all(|width| *width < bt_layout::MIN_PREVIEW_W.floor_px()),
        "the 360 floor gave way, which is the whole of the ruling: {previews:?}"
    );
    assert!(
        previews
            .windows(2)
            .all(|pair| (pair[0] - pair[1]).abs() <= 1),
        "and it gave way *in proportion*, not by one pane paying for the other: {previews:?}"
    );
}

/// Sovereignty taken by a divider drag is not handed back by a rectangle
/// that happens to match the program's last claim — the same asymmetry the
/// window drag already relies on, and the reason the layout does not snap
/// back one frame after the button comes up.
#[test]
fn a_layout_the_hand_chose_stays_the_hands_until_the_program_claims_again() {
    let claimed = PhysicalSize::new(1920, 1200);
    let (policy, held) =
        size_authority_for_rectangle(SizePolicy::Sovereign, Some(claimed), claimed);
    assert_eq!(policy, SizePolicy::Sovereign);
    assert_eq!(held, Some(claimed));
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

/// PIN (real-machine bug, 2026-08-10): **a tab whose shell said nothing wears
/// nothing, however many frames it published.**
///
/// The reported window: a pinned three-pane tab, no new output anywhere, and
/// a blue dot on it the instant the user moved to the next tab. The recorded
/// ledger at the moment of the switch was `frames=95 seen=94` — the shell had
/// been silent for the whole session, and the extra frame was a chrome
/// repaint that went out after the turn's last reconciliation and before the
/// switch was paid.
///
/// Two mutations die here. Count publication instead of output and the
/// thirty-six silent frames below become thirty-six units of news; measure
/// unread against `status.published_revision` and [`quiet`]'s absurd value
/// lights the tab immediately.
#[test]
fn frames_are_not_output_and_a_silent_tab_stays_silent() {
    let mut output = 0;
    let mut seen = 0;
    for _ in 0..36 {
        // A blinking cursor, a hovered link, a repainted chrome: the window
        // publishes, the shell says nothing.
        output = output_revision(output, false, false);
        seen = seen_revision(seen, output, true);
    }
    assert_eq!(
        (output, seen),
        (0, 0),
        "a frame is not a sentence: nothing was said and nothing is owed"
    );
    assert!(!facts(output, seen, false).has_unseen_output());
    assert_eq!(
        facts(output, seen, false).claim(),
        StatusClaim::Silent,
        "the tab the user just left had nothing to report"
    );
}

/// PIN (T2 D41): the accessibility preference is read in the right
/// direction.
///
/// Win32 and CSS spell this setting with opposite polarity —
/// `SPI_GETCLIENTAREAANIMATION` is `TRUE` when animation is *wanted*, while
/// `prefers-reduced-motion: reduce` matches when it is *not* — and the
/// inversion is invisible on any machine left at the default. Getting it
/// backwards would force animation on exactly the users who asked for none
/// and strip it from everyone else, and no screenshot review would catch
/// it. So the mapping is a named function with a test rather than a `!` at
/// a call site.
#[test]
fn the_reduced_motion_preference_is_read_in_the_right_direction() {
    assert_eq!(
        Motion::from_client_area_animation(Some(true)),
        Motion::Full,
        "TRUE means the system wants animation"
    );
    assert_eq!(
        Motion::from_client_area_animation(Some(false)),
        Motion::Reduced,
        "FALSE is the accessibility setting turned on"
    );
    assert_eq!(
        Motion::from_client_area_animation(None),
        Motion::Full,
        "a failed read is not a request for less motion"
    );
    // The default a `Motion` takes when nothing has asked is the same one a
    // failed read gets, so the two cannot drift apart.
    assert_eq!(Motion::default(), Motion::Full);
}

/// PIN (T2 D35, the per-leaf half): work in flight suppresses the finished
/// claim of **the shell doing the work**, and of nothing else.
///
/// This is the pin that fails the moment someone re-reads D35 as a tab-wide
/// rule — "any progress anywhere means the tab has not finished". A pane
/// with a download running contributes `Silent` because *it* has not
/// finished; its quiet sibling has finished and gone unread and contributes
/// `Unread`; and the tab wears `Unread`, because a tab must never say less
/// than its panes do (D34). Suppressing tab-wide would swallow the
/// sibling's news under a download it has nothing to do with, and the user
/// would learn about it only by opening the tab to check — which is the one
/// thing the badge exists to save them.
///
/// Red gate: rewrite `fleet_claim` as `loudest_claim` over the fleet with a
/// tab-wide `any(work_in_flight)` gate in front of it and this reads
/// `Silent`.
#[test]
fn a_download_in_one_pane_does_not_silence_its_quiet_siblings_unread() {
    // Leaf A: unseen output *and* a download still running.
    let mut downloading = quiet();
    downloading.progress = Some(ProgressState::Normal(40));
    assert_eq!(
        facts_with(downloading, 12, 3, false).claim(),
        StatusClaim::Silent,
        "per leaf, the download suppresses this pane's own finished claim"
    );
    // Leaf B: quiet, and holding output nobody has read.
    assert_eq!(facts(7, 3, false).claim(), StatusClaim::Unread);

    assert_eq!(
        fleet_claim([facts_with(downloading, 12, 3, false), facts(7, 3, false)]),
        StatusClaim::Unread,
        "the suppression is per leaf; the aggregation is per tab"
    );
    // Order must not matter: `max` is commutative and the pin says so out
    // loud, because a fold that carried a suppression forward would not be.
    assert_eq!(
        fleet_claim([facts(7, 3, false), facts_with(downloading, 12, 3, false)]),
        StatusClaim::Unread
    );
    // And with no quiet sibling there is genuinely nothing finished to
    // report, so the tab is silent — D35 intact where it does apply.
    assert_eq!(
        fleet_claim([facts_with(downloading, 12, 3, false)]),
        StatusClaim::Silent
    );
}

/// PIN (T2 D35): work in flight suppresses every "finished" claim.
///
/// The mock-up's own comment is a user ruling (line 1920): "an active
/// download is still WORK IN FLIGHT: no finished-unread claim until the
/// progress ends". The ring and the breathing icon are already reporting
/// what is happening, and a dot beside them would be a third voice on one
/// fact — and a wrong one, since nothing has finished.
#[test]
fn a_session_still_working_makes_no_finished_claim() {
    let unseen = quiet();
    // Quiet and unseen: the plain unread claim.
    assert_eq!(facts_with(unseen, 9, 4, false).claim(), StatusClaim::Unread);

    // The same session, still running.
    let mut working = unseen;
    working.working = true;
    assert_eq!(
        facts_with(working, 9, 4, false).claim(),
        StatusClaim::Silent
    );

    // The same session, reporting progress — suppressed in every flavour,
    // because every one of them means a run that has not ended.
    for state in [
        ProgressState::Normal(40),
        ProgressState::Indeterminate,
        ProgressState::Paused(Some(40)),
        ProgressState::Error(Some(40)),
    ] {
        let mut in_flight = unseen;
        in_flight.progress = Some(state);
        assert_eq!(
            facts_with(in_flight, 9, 4, false).claim(),
            StatusClaim::Silent,
            "{state:?} is work in flight, not a finished claim"
        );
    }

    // A failure is suppressed by the same rule, for the same reason.
    let mut failing = unseen;
    failing.failure_exit_code = Some(1);
    assert_eq!(
        facts_with(failing, 9, 4, false).claim(),
        StatusClaim::Failed
    );
    failing.progress = Some(ProgressState::Normal(10));
    assert_eq!(
        facts_with(failing, 9, 4, false).claim(),
        StatusClaim::Silent
    );
}

/// PIN (T2): the bell is latched, so it survives what suppresses the rest.
///
/// A bell is a thing that *rang* — a past event, not a state — so a session
/// that is busy again has still rung, and the claim stands until the user
/// looks. This is the one claim the work-in-flight rule does not touch.
#[test]
fn the_bell_outlives_the_work_that_followed_it() {
    let mut ringing = quiet();
    ringing.bell = Some(bt_term::BellSource::Bel);
    ringing.working = true;
    ringing.progress = Some(ProgressState::Indeterminate);
    assert_eq!(facts_with(ringing, 9, 9, false).claim(), StatusClaim::Bell);
    // Even with nothing unread — the bell is not an unread claim.
    assert_eq!(facts_with(ringing, 9, 99, false).claim(), StatusClaim::Bell);
}

/// PIN (T2 J97): unread is "said since you last saw it", and the tab you are
/// looking at is never unread.
///
/// Red gate: without the active-tab clause the tab under the user's eyes
/// wears a dot asking them to look at it, in the window between its shell
/// speaking and the frame that carries those words being presented.
#[test]
fn unread_is_what_was_said_since_the_last_look_and_never_on_the_active_tab() {
    // Behind an inactive tab, new output is unread.
    assert!(facts(7, 3, false).has_unseen_output());
    // Caught up: nothing new.
    assert!(!facts(7, 7, false).has_unseen_output());
    // The active tab is the one being read, whatever its ledger says.
    assert!(!facts(7, 3, true).has_unseen_output());
    assert_eq!(facts(7, 0, true).claim(), StatusClaim::Silent);
    // A failure on the active tab makes no dot either — the same clause
    // covers it, because a failure is a kind of unread.
    let mut failed = quiet();
    failed.failure_exit_code = Some(2);
    assert_eq!(facts_with(failed, 7, 0, true).claim(), StatusClaim::Silent);
    assert_eq!(facts_with(failed, 7, 0, false).claim(), StatusClaim::Failed);
}

/// PIN (T2 D41): with animations off the breath holds one value instead of
/// stopping at whatever opacity it happened to be passing through.
///
/// The mock-up spells the replacement out (line 1927): `.ticon.working {
/// opacity: .6 }`. "Working" still has to be legible when nothing may move,
/// so the answer is a held value, not a still frame and not full opacity.
#[test]
fn reduced_motion_holds_the_breath_at_one_value() {
    for fraction in [0.0_f32, 0.25, 0.5, 0.75, 1.0, 3.7] {
        let held = breathe_opacity(
            Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS).mul_f32(fraction),
            Motion::Reduced,
        );
        assert!((held - WINDOW_TAB_BREATHE_REDUCED_OPACITY).abs() < 1e-6);
    }
    // And it is genuinely quieter than a mark that is not working at all,
    // which is what makes it still say something.
    const { assert!(WINDOW_TAB_BREATHE_REDUCED_OPACITY < 1.0) };
}

/// PIN (`docs/DESIGN.md` §7.1.5b, 2026-07-18; the clock ruled by the owner
/// 2026-09-20) — **the waiting dot breathes with the halo and never goes out.**
///
/// The mock-up wrote `.unreaddot.await { animation: fcpulse .9s infinite }`
/// (`ui-mockup.html:346`) and never defined `@keyframes fcpulse`, so the number
/// in it was never a curve anybody had seen. The owner ruled the missing curve
/// to be the halo's: *two things saying one fact breathe together*, so the dot
/// rides the very same 1.7s breath at the very same phase and differs only in
/// what it does with it — a glow goes out, a badge does not.
///
/// Red gate: give the dot a period or a phase of its own and the first pair of
/// assertions part company; let it ramp from zero like the halo and the floor
/// assertion goes red; answer anything but a flat 1.0 under `Reduced` and the
/// last does — which would be the accessibility setting deleting a claim.
#[test]
fn the_waiting_dot_breathes_with_the_halo_and_never_goes_out() {
    let period = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS);
    let at = |fraction: f32| wait_pulse(period.mul_f32(fraction), Motion::Full);

    // One clock: brightest at the same instant, faintest at the same instant.
    assert!(
        (at(0.5).halo - 1.0).abs() < 1e-6 && (at(0.5).dot - 1.0).abs() < 1e-6,
        "both are full at the one keyframe the mock-up writes: {:?}",
        at(0.5)
    );
    assert!(
        at(0.0).halo.abs() < 1e-6 && (at(0.0).dot - WINDOW_TAB_BREATHE_MIN_OPACITY).abs() < 1e-6,
        "and at the trough the glow is out while the badge is merely faint: {:?}",
        at(0.0)
    );
    assert!(
        (at(0.25).dot - at(0.75).dot).abs() < 1e-6 && (at(2.5).dot - at(0.5).dot).abs() < 1e-6,
        "symmetric about the keyframe, and `infinite`"
    );

    // One sample: the dot is a function of the halo's own number at every
    // phase, which is what stops the two drifting.
    for step in 0..=64 {
        let pulse = at(step as f32 / 64.0);
        assert!(
            (0.0..=1.0).contains(&pulse.halo),
            "the halo stays in gamut at phase {step}/64: {pulse:?}"
        );
        assert!(
            (WINDOW_TAB_BREATHE_MIN_OPACITY..=1.0).contains(&pulse.dot),
            "and the dot never goes out at phase {step}/64: {pulse:?}"
        );
        assert!(
            (pulse.dot
                - (WINDOW_TAB_BREATHE_MIN_OPACITY
                    + (1.0 - WINDOW_TAB_BREATHE_MIN_OPACITY) * pulse.halo))
                .abs()
                < 1e-6,
            "one breath, two faces of it: {pulse:?}"
        );
    }

    // Reduced motion: each channel's own value with the animation stood down,
    // and neither of them is "not waiting".
    for fraction in [0.0_f32, 0.25, 0.5, 0.75, 1.0, 3.7] {
        let pulse = wait_pulse(period.mul_f32(fraction), Motion::Reduced);
        assert_eq!(
            (pulse.halo, pulse.dot),
            (0.0, 1.0),
            "an animation turned off leaves the element as it is written: a \
             keyframe set with no 0% frame leaves no shadow, and `.unreaddot` \
             is an opaque dot"
        );
    }
}

/// PIN (§7.1.5b P1-8) — **`Ctrl+Shift+A` serves the oldest, then walks on,
/// then wraps.**
///
/// The ruling's three clauses against the one function that implements them,
/// with the queue handed over out of order on purpose: the walk is a function
/// of the serials, not of the order anything was collected in.
///
/// Red gate: order by the tuple's first element and the first assertion goes
/// red; drop the pivot and the second does; fall back to `.next()` instead of
/// rotating and the last does.
#[test]
fn the_attention_queue_serves_the_oldest_then_walks_on_and_wraps() {
    let queue = [("c", 9_u64), ("a", 2), ("b", 5)];
    assert_eq!(next_attention_stop::<&str>(&[], None), None);
    assert_eq!(
        next_attention_stop(&queue, None),
        Some("a"),
        "先到先服务 — the oldest serial, whatever order the window walked its \
             tabs in"
    );
    assert_eq!(
        next_attention_stop(&queue, Some(2)),
        Some("b"),
        "已在队列中再按 = 走到下一个"
    );
    assert_eq!(next_attention_stop(&queue, Some(5)), Some("c"));
    assert_eq!(
        next_attention_stop(&queue, Some(9)),
        Some("a"),
        "循环 — past the newest is back to the oldest"
    );
    assert_eq!(
        next_attention_stop(&[("only", 4_u64)], Some(4)),
        Some("only"),
        "a queue of one wraps onto itself rather than answering nothing"
    );
    assert_eq!(
        next_attention_stop(&queue, Some(7)),
        Some("c"),
        "and a pivot that is nobody's serial still names a position in the \
             order — the walk is over serials, not over membership"
    );
}

fn ledger_strong_wait(kind: attention::WaitKind) -> attention::Event {
    attention::Event::StrongWait(attention::WaitSlot::Level(kind))
}

fn ledger_clear_all(reason: attention::ClearReason) -> attention::Event {
    attention::Event::StrongClear {
        selector: attention::ClearSelector::All,
        class: attention::ClearClass::Boundary,
        reason,
        begins_turn: false,
    }
}

/// PIN — **`OSC 1337;RequestAttention=` on the wire becomes an episode accounted to `src=osc`.**
///
/// The two halves of this block meet here and nowhere else: `bt-term` mints a *generation* from the
/// bytes, and the ledger mints an *episode* from the generation. Pinning them separately leaves the
/// join untested, and the join is where a level would be read as an edge — a program restating its
/// request once a second would then mint an episode once a second, and the badge would re-arm
/// forever.
///
/// The withdrawal is the other half of what makes this sequence the one the plan chose over four
/// alternatives: the program can take its own sentence back, and the ledger writes that down as the
/// program's doing rather than as anybody's answer.
#[test]
fn the_bytes_of_a_standing_request_become_one_episode_charged_to_the_osc_lane() {
    fn wrote(session: &mut bt_term::DualPlaneSession, bytes: &[u8]) -> Option<u64> {
        session.feed(bytes).expect("the session accepts bytes");
        session.status().attention_request
    }

    let mut session = wired();
    let mut pane = LedgerPane::new();
    let level = wrote(&mut session, b"\x1b]1337;RequestAttention=yes\x07");
    let rose = pane.ledger.weak_edge(level).expect("a rising edge");
    assert_eq!(
        pane.at(rose),
        ["mint tab=1 seat=SeatId(2) episode=1 src=osc gen=1 grounds=requested prev=-"]
    );
    let level = wrote(&mut session, b"\x1b]1337;RequestAttention=yes\x07");
    assert_eq!(
        pane.ledger.weak_edge(level),
        None,
        "a restatement is one program saying one thing twice"
    );
    assert_eq!(
        pane.away(),
        ["admit tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=requested active=0 focused=0"],
        "a program that wants you is not a program that is blocked on you: no interruption"
    );
    let level = wrote(&mut session, b"\x1b]1337;RequestAttention=no\x07");
    let fell = pane.ledger.weak_edge(level).expect("a falling edge");
    assert_eq!(
        pane.at(fell),
        ["withdraw tab=1 seat=SeatId(2) ticket=0 episode=1 reason=program src=osc"]
    );
    assert_eq!(pane.state(), attention::State::Idle);
}

/// PIN — **the two lanes meet on one account: one pane, two producers, one request.**
///
/// The wire says "this pane wants you" and, six seconds later, a hook says "and it is blocked on
/// your input". Those are **one** request with two pieces of evidence, not two requests: the place
/// in the queue is not re-stamped, no second episode is minted, and the wording rises — and falls
/// again the moment the stronger evidence is withdrawn, because a pane that says "waiting for you"
/// on the strength of a credential that no longer exists is a pane telling you something untrue.
///
/// It ends on the wire because that is the half this slice added: the program takes its own
/// sentence back, the place goes, and the line says the withdrawal came in over `src=osc`. A trace
/// that could not tell the two producers apart would be a trace that could not answer the one
/// question anybody asks it — *did the adapter actually install, or is this the generic path?*
#[test]
fn one_pane_two_producers_and_one_episode_between_them() {
    let mut session = wired();
    let mut pane = LedgerPane::new();
    session
        .feed(b"\x1b]1337;RequestAttention=yes\x07")
        .expect("the session accepts bytes");
    let rose = pane
        .ledger
        .weak_edge(session.status().attention_request)
        .expect("a rising edge");
    assert_eq!(
        pane.at(rose),
        ["mint tab=1 seat=SeatId(2) episode=1 src=osc gen=1 grounds=requested prev=-"]
    );
    assert_eq!(
        pane.at(ledger_strong_wait(attention::WaitKind::Permission)),
        ["upgrade tab=1 seat=SeatId(2) episode=1 grounds=awaiting src=pipe gen=1"],
        "the same request, confirmed by the other producer"
    );
    assert_eq!(
        pane.away(),
        [
            "admit tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=awaiting active=0 focused=0",
            "toast tab=1 seat=SeatId(2) why=awaiting ticket=0 episode=1 reach=flash",
        ]
    );
    assert_eq!(
        pane.at(ledger_clear_all(attention::ClearReason::Hook)),
        [
            "clear tab=1 seat=SeatId(2) episode=1 src=pipe gen=1 reason=hook",
            "downgrade tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=requested src=pipe \
             reason=clear",
        ],
        "the strong layer withdrew; the weak one is still up, so the place stays and the wording \
         falls back"
    );
    session
        .feed(b"\x1b]1337;RequestAttention=no\x07")
        .expect("the session accepts bytes");
    let fell = pane
        .ledger
        .weak_edge(session.status().attention_request)
        .expect("a falling edge");
    assert_eq!(
        pane.at(fell),
        ["withdraw tab=1 seat=SeatId(2) ticket=0 episode=1 reason=program src=osc"]
    );
    assert_eq!(pane.state(), attention::State::Idle);
}

/// PIN — **`once` and `fireworks` reach the ledger as nothing at all.**
///
/// Both are on iTerm2's own list beside `yes` and `no`, which is what makes them worth a pin: the
/// tempting reading is "four values of one sequence, so four values of one state". `once` is a
/// one-shot and takes the bell's path inside the session; `fireworks` is a gesture this terminal
/// does not have. Neither is a level, so neither can produce an edge.
#[test]
fn the_one_shot_and_the_unimplemented_never_reach_the_ledger() {
    for payload in [
        &b"\x1b]1337;RequestAttention=once\x07"[..],
        &b"\x1b]1337;RequestAttention=fireworks\x07"[..],
    ] {
        let mut session = wired();
        let ledger = attention::AttentionLedger::default();
        session.feed(payload).expect("the session accepts bytes");
        assert_eq!(
            ledger.weak_edge(session.status().attention_request),
            None,
            "{payload:?}"
        );
    }
}

/// PIN (census-3, `docs/plans/design/ownership-census-2026-09-25.md` §R6) — **the window's
/// attention pass arms a wait's ten minutes when it arrives, forgets them when its clear arrives,
/// and spends them when they run out.**
///
/// The rule and the clock left this crate for `bt-workbench` (`attention::expiry`), where their own
/// pure tests live. What stayed here is *where the clock is driven*: `deliver_attention` arms and
/// forgets it as a hook's line lands, `settle_attention` spends what has come due on the next
/// pass. Neither half can be seen from the other crate, so this walks both through the product's
/// own functions — a real message naming a real pane's capability, looked up in the shipped
/// Claude Code rows — and reads the answer the loop reads, `attention_ledger_deadline`.
///
/// MUTATION: drop the `arm` arm in `deliver_attention` and the first assertion goes red; drop the
/// `forget` arm and the second does; take the `due` loop out of `settle_attention` and the last
/// pair does.
#[test]
fn a_waits_ten_minutes_are_armed_on_arrival_forgotten_on_its_clear_and_spent_when_they_run_out() {
    use attention::expiry::WAIT_TTL;

    let mut tabs = vec![ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let capability = attention_wire::mint_capability();
    tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .attention_capability
        .clone_from(&capability);
    let installed =
        attention_map::installed_rows(attention_map::ROWS, attention_map::CLAUDE_CODE, |_| true);
    let said = |event: &str| attention_wire::Message {
        capability: capability.clone(),
        family: attention_map::CLAUDE_CODE.to_owned(),
        event: event.to_owned(),
        id: None,
        text: None,
    };
    let mut places = attention::Places::default();
    let start = Instant::now();
    let deliver = |tabs: &mut [TabState], places: &mut attention::Places, event: &str| {
        deliver_attention(
            tabs,
            1,
            on_a_screen(false),
            BOTH_NOTIFICATION_ROWS_ON,
            places,
            &[said(event)],
            &installed,
            start,
            None,
            &mut Vec::new(),
        );
    };
    let settle = |tabs: &mut [TabState], places: &mut attention::Places, now: Instant| {
        settle_attention(
            tabs,
            1,
            on_a_screen(false),
            BOTH_NOTIFICATION_ROWS_ON,
            places,
            now,
            None,
            &mut Vec::new(),
        );
    };

    deliver(&mut tabs, &mut places, "PermissionRequest");
    assert_eq!(
        attention_ledger_deadline(&tabs),
        Some(start + WAIT_TTL),
        "a wait that arrives starts its ten minutes"
    );
    deliver(&mut tabs, &mut places, "Stop");
    assert_eq!(
        attention_ledger_deadline(&tabs),
        None,
        "the clear that ends it takes its clock with it"
    );

    deliver(&mut tabs, &mut places, "PermissionRequest");
    settle(
        &mut tabs,
        &mut places,
        start + WAIT_TTL - Duration::from_secs(1),
    );
    assert_ne!(
        tabs[0].sessions[&seat].attention.state(),
        attention::State::Idle,
        "a second before the ten minutes are up, the pane is still asking"
    );
    settle(&mut tabs, &mut places, start + WAIT_TTL);
    assert_eq!(
        attention_ledger_deadline(&tabs),
        None,
        "a clock that ran out is spent, not repeated"
    );
    assert_eq!(
        tabs[0].sessions[&seat].attention.state(),
        attention::State::Idle,
        "and the wait it stood for is over"
    );
}

/// PIN — **the look that spends a latch is the runtime's own call.**
///
/// [`claim_after_a_look`] reproduces `clear_attention`'s two field writes against a bare
/// [`SessionStatus`], because a `SessionStatus` cannot be injected into a live session. It is
/// therefore the one place in these tests that could drift away from the runtime without
/// anything failing. This is the anchor: a real leaf, a real `BEL`, a real failing exit code,
/// and the runtime's own pass — and the two fields the fixture clears are exactly the two that
/// end up clear.
#[test]
fn the_look_that_spends_a_latch_is_the_runtimes_own_call() {
    let mut tabs = vec![ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let mut next = attention::Places::default();
    tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's one shell")
        .session
        .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07x\x1b]133;C\x07\x1b]133;D;1\x07\x07")
        .expect("the fixture's bytes parse");
    let rang = tabs[0].sessions[&seat].session.status();
    assert!(rang.bell_latched() && rang.failure_exit_code == Some(1));

    one_turn(&mut tabs, 0, true, &mut next);

    let looked = tabs[0].sessions[&seat].session.status();
    assert!(!looked.bell_latched());
    assert_eq!(looked.failure_exit_code, None);
    // The fixture, handed the same two facts, says the same thing.
    assert_eq!(
        claim_after_a_look(latched(), true, true),
        StatusClaim::Silent
    );
    assert_eq!(tabs[0].fleet_claim_for(true), StatusClaim::Silent);
}

/// PIN (T2 D41): the indeterminate arc turns once per its own period, and
/// stands still — rather than vanishing — when animation is off.
#[test]
fn the_indeterminate_arc_turns_once_a_period_and_holds_still_when_asked() {
    let period = Duration::from_millis(WINDOW_TAB_RING_SPIN_PERIOD_MS);
    assert_eq!(
        indeterminate_start_milliturns(Duration::ZERO, Motion::Full),
        0
    );
    assert_eq!(
        indeterminate_start_milliturns(period.mul_f32(0.25), Motion::Full),
        250
    );
    assert_eq!(
        indeterminate_start_milliturns(period.mul_f32(0.5), Motion::Full),
        500
    );
    // A whole turn returns to the start rather than running off the end.
    assert_eq!(indeterminate_start_milliturns(period, Motion::Full), 0);
    assert_eq!(
        indeterminate_start_milliturns(period.mul_f32(7.25), Motion::Full),
        250
    );
    // Stopped, it holds at noon — and it is still an arc. A ring with no
    // arc at all would be reporting 0%, which is a different claim.
    for fraction in [0.0_f32, 0.3, 0.75, 9.1] {
        assert_eq!(
            indeterminate_start_milliturns(period.mul_f32(fraction), Motion::Reduced),
            0
        );
    }
}

/// PIN (T2): every `OSC 9;4` state maps to the arc the mock-up gives it.
///
/// The two states that may arrive *without* a percentage are the reason
/// this takes a `last_sweep`: states 2 and 4 change a run that is already
/// under way, so the reading already on the wire still stands, and keeping
/// it is the protocol's own answer rather than an invented number.
#[test]
fn each_progress_state_paints_its_own_arc() {
    let palette = LIGHT_CHROME;
    let arc = |state, last| ring_arc(state, last, Duration::ZERO, Motion::Full, &palette);

    let normal = arc(ProgressState::Normal(40), None);
    assert_eq!(normal.color, palette.accent);
    assert_eq!(normal.sweep_milliturns, 400);
    assert!(
        !normal.animating,
        "a determinate arc does not move by itself"
    );

    // Percent is a fraction of the whole turn, at both ends of its range.
    assert_eq!(arc(ProgressState::Normal(0), None).sweep_milliturns, 0);
    assert_eq!(arc(ProgressState::Normal(100), None).sweep_milliturns, 1000);
    // And a report beyond 100 is clamped rather than wrapped — an arc that
    // wrapped would report 130% as 30%.
    assert_eq!(arc(ProgressState::Normal(255), None).sweep_milliturns, 1000);

    // Only the arc's colour changes; the ring is not redrawn as something
    // else (mock-up lines 280-281 recolour `.arc` and nothing more).
    let failed = arc(ProgressState::Error(Some(40)), None);
    assert_eq!(failed.color, palette.status_err);
    assert_eq!(failed.sweep_milliturns, 400);
    let paused = arc(ProgressState::Paused(Some(40)), None);
    assert_eq!(paused.color, palette.status_pause);
    assert_eq!(paused.sweep_milliturns, 400);

    // A state change with no percentage keeps the reading already showing.
    assert_eq!(
        arc(ProgressState::Error(None), Some(400)).sweep_milliturns,
        400
    );
    assert_eq!(
        arc(ProgressState::Paused(None), Some(730)).sweep_milliturns,
        730
    );
    // With no reading ever taken, a full ring — so a failure is visible
    // rather than reported as a bare track.
    assert_eq!(arc(ProgressState::Error(None), None).sweep_milliturns, 1000);

    let spinning = arc(ProgressState::Indeterminate, None);
    assert_eq!(spinning.color, palette.accent);
    assert_eq!(spinning.sweep_milliturns, 243, "13 of the mock-up's 53.4");
    assert!(
        spinning.animating,
        "an indeterminate arc owes the next frame"
    );
    // Stopped, it is the same arc and no longer owes a frame.
    let still = ring_arc(
        ProgressState::Indeterminate,
        None,
        Duration::ZERO,
        Motion::Reduced,
        &palette,
    );
    assert_eq!(still.sweep_milliturns, spinning.sweep_milliturns);
    assert!(!still.animating);
}

/// PIN (T2): the arc eases to a new reading instead of snapping to it, and
/// stops owing frames the moment it arrives.
///
/// `.pring .arc { transition: stroke-dashoffset .3s ease }` (line 279).
/// The "stops owing frames" half is what keeps an idle window idle: a tween
/// that never reports itself finished is a 60fps loop that never ends.
#[test]
fn the_arc_eases_to_a_new_reading_and_then_stands_down() {
    let started = Instant::now();
    let tween = SweepTween {
        from: 200,
        to: 700,
        started,
    };
    let duration = Duration::from_millis(WINDOW_TAB_RING_SWEEP_TRANSITION_MS);

    let (at_start, moving) = tween.sample(started);
    assert_eq!(at_start, 200, "it begins where the arc already was");
    assert!(moving);

    let (midway, moving) = tween.sample(started + duration / 2);
    assert!(moving);
    assert!(
        (200..=700).contains(&midway),
        "the tween left its endpoints: {midway}"
    );

    let (arrived, moving) = tween.sample(started + duration);
    assert_eq!(arrived, 700, "it arrives exactly, not nearly");
    assert!(!moving, "an arrived tween owes no further frames");
    let (still_there, moving) = tween.sample(started + duration * 4);
    assert_eq!(still_there, 700);
    assert!(!moving);

    // `ease` leaves quickly and arrives slowly, so by the halfway point it
    // is already past halfway. A linear ramp would sit exactly on 450.
    assert!(
        midway > 450,
        "the arc must use CSS `ease`, which front-loads its travel: {midway}"
    );
}

/// PIN (T2): the two CSS timing functions are solved, not approximated.
///
/// Both are checked against their defining points — the endpoints every
/// curve shares, and the midpoint value that tells them apart. `ease` and
/// `ease-in-out` are symmetric only in the second case, and a solver that
/// silently returned one for the other would pass every endpoint test.
#[test]
fn the_css_timing_curves_are_the_real_beziers() {
    for curve in [EASE, EASE_IN_OUT] {
        assert_eq!(cubic_bezier(0.0, curve), 0.0);
        assert_eq!(cubic_bezier(1.0, curve), 1.0);
        // Out of range in either direction is clamped, not extrapolated.
        assert_eq!(cubic_bezier(-1.0, curve), 0.0);
        assert_eq!(cubic_bezier(2.0, curve), 1.0);
        // Monotonic: time only moves forward, so the curve must too.
        let mut previous = 0.0_f32;
        for step in 0..=200 {
            let value = cubic_bezier(step as f32 / 200.0, curve);
            assert!(value >= previous - 1e-4, "{curve:?} went backwards");
            previous = value;
        }
    }
    // `ease-in-out` is symmetric about its centre and therefore passes
    // through exactly .5 at half time.
    assert!((cubic_bezier(0.5, EASE_IN_OUT) - 0.5).abs() < 1e-3);
    // `ease` is not symmetric: it is already well past half by half time,
    // which is the whole difference between the two and the reason both
    // exist rather than one standing in for the other.
    assert!(cubic_bezier(0.5, EASE) > 0.75);
}

/// PIN (T2 D32/D33): each claim wears the mock-up's own colour, and a
/// silent session draws no dot at all.
///
/// Presence-versus-absence is the point: the mock-up keeps `.unreaddot` in
/// the DOM always and shows it by class (its comment at line 249 records
/// why), but what lands on screen is still nothing when there is nothing to
/// say. A dot drawn in the tab's own colour would be a smudge, not a state.
#[test]
fn each_claim_wears_its_own_colour_and_silence_draws_nothing() {
    for palette in [LIGHT_CHROME, DARK_CHROME] {
        assert_eq!(StatusClaim::Silent.dot(&palette), None);
        let ink = |claim: StatusClaim| claim.dot(&palette).map(|dot| dot.ink);
        assert_eq!(ink(StatusClaim::Unread), Some(palette.accent));
        assert_eq!(ink(StatusClaim::Bell), Some(palette.status_warn));
        assert_eq!(ink(StatusClaim::Failed), Some(palette.status_err));
        // Every speaking claim differs from every other in at least one of the two axes —
        // a taxonomy that collapses is not a taxonomy. Three of the four differ in ink;
        // the fourth pair shares `--warn` and is told apart by its fill (red line 3).
        let drawn = [
            StatusClaim::Unread,
            StatusClaim::Bell,
            StatusClaim::Failed,
            StatusClaim::Awaiting,
        ]
        .map(|claim| claim.dot(&palette).expect("a speaking claim has a dot"));
        for (index, dot) in drawn.iter().enumerate() {
            for other in &drawn[index + 1..] {
                assert_ne!(dot, other, "two claims cannot arrive as the same badge");
            }
        }
    }
}

#[test]
fn theme_mode_resolution_covers_every_os_theme_input() {
    use bt_persist::ThemeModeV1::{Dark, Light, System};
    use winit::window::Theme::{Dark as OsDark, Light as OsLight};

    for (mode, os_theme, expected) in [
        (System, Some(OsDark), Theme::Dark),
        (System, Some(OsLight), Theme::Light),
        (System, None, Theme::Dark),
        (Light, Some(OsDark), Theme::Light),
        (Light, Some(OsLight), Theme::Light),
        (Light, None, Theme::Light),
        (Dark, Some(OsDark), Theme::Dark),
        (Dark, Some(OsLight), Theme::Dark),
        (Dark, None, Theme::Dark),
    ] {
        assert_eq!(resolve_theme_mode(mode, os_theme), expected);
    }
}

#[test]
fn theme_changed_is_ignored_by_explicit_modes_and_resolved_by_system() {
    use bt_persist::ThemeModeV1::{Dark, Light, System};
    use winit::window::Theme::{Dark as OsDark, Light as OsLight};

    assert_eq!(resolved_theme_change(System, OsDark), Some(Theme::Dark));
    assert_eq!(resolved_theme_change(System, OsLight), Some(Theme::Light));
    assert_eq!(resolved_theme_change(Light, OsDark), None);
    assert_eq!(resolved_theme_change(Light, OsLight), None);
    assert_eq!(resolved_theme_change(Dark, OsDark), None);
    assert_eq!(resolved_theme_change(Dark, OsLight), None);
}

/// PIN — §7.1.6c-4f amendment: the acrylic plate follows the scheme in
/// force, and this is the whole of the decision that makes it.
///
/// Red gate: the window used to declare nothing at all, so DWM tinted its
/// plate light whatever the scheme was — measured on a light desktop with
/// Solarized Dark at 30 %, a pane body read `(156,177,183)` instead of the
/// `(99,120,126)` the flag buys. A version of this that keyed on the theme
/// *row* rather than on the painted background would pass the first two
/// cases and fail the last two.
#[test]
fn the_dwm_plate_is_told_which_canvas_is_actually_painted() {
    const SOLARIZED_DARK: [u8; 3] = [0x00, 0x2B, 0x36];
    const FOLIO_LIGHT: [u8; 3] = [0xFA, 0xFA, 0xFA];

    // A window that has never spoken says it either way: DWM's default is
    // DWM's assumption, not this window's statement.
    assert_eq!(dwm_dark_mode_owed(None, SOLARIZED_DARK), Some(true));
    assert_eq!(dwm_dark_mode_owed(None, FOLIO_LIGHT), Some(false));
    // Said once, not said again — the whole point of remembering it.
    assert_eq!(dwm_dark_mode_owed(Some(true), SOLARIZED_DARK), None);
    assert_eq!(dwm_dark_mode_owed(Some(false), FOLIO_LIGHT), None);
    // A canvas that moved is a statement that has to move with it, in both
    // directions — this is the theme switch and the scheme switch alike.
    assert_eq!(dwm_dark_mode_owed(Some(false), SOLARIZED_DARK), Some(true));
    assert_eq!(dwm_dark_mode_owed(Some(true), FOLIO_LIGHT), Some(false));
    // The luma decides, not the row's name: a "dark scheme" file naming a
    // pale background gets the light plate that background asks for, and a
    // "light scheme" naming a near-black one gets the dark plate. This is
    // `scheme_in_force`'s rule, at `background_is_light`'s one threshold.
    assert_eq!(dwm_dark_mode_owed(None, [0xEE, 0xEE, 0xE8]), Some(false));
    assert_eq!(dwm_dark_mode_owed(None, [0x10, 0x10, 0x12]), Some(true));
    // And the threshold itself is borrowed, never restated here.
    for background in [SOLARIZED_DARK, FOLIO_LIGHT, [0x7F, 0x7F, 0x7F]] {
        assert_eq!(
            dwm_dark_mode_owed(None, background),
            Some(!bt_render::background_is_light(background)),
            "the plate and the canvas must take one decision, not two"
        );
    }
}

/// What a program inside a pane is told when it asks, on each canvas.
///
/// The `BT_BG` row is the one that earns this test: the override moves the
/// glass without moving the settings, and a window that answered `OSC 11;?`
/// out of the *scheme* would send a program off to dress for a canvas nobody
/// is looking at. The canvas verdict and the background must both come from
/// the painted colour; only the sixteen and the caret come from the scheme.
#[test]
fn the_colours_a_program_is_told_are_the_ones_the_glass_is_wearing() {
    use bt_render::{FOLIO_DARK, FOLIO_LIGHT};

    let light = terminal_palette(FOLIO_LIGHT, [0xff, 0xff, 0xff], [0x37, 0x35, 0x2f]);
    assert_eq!(light.canvas, TerminalCanvas::Light);
    assert_eq!(light.background, [0xff, 0xff, 0xff]);
    assert_eq!(light.foreground, [0x37, 0x35, 0x2f]);
    assert_eq!(light.ansi, FOLIO_LIGHT.ansi);
    assert_eq!(light.cursor, FOLIO_LIGHT.cursor);

    let dark = terminal_palette(FOLIO_DARK, [0x1b, 0x1b, 0x1b], [0xe1, 0xe1, 0xe1]);
    assert_eq!(dark.canvas, TerminalCanvas::Dark);
    assert_eq!(dark.background, [0x1b, 0x1b, 0x1b]);
    assert_ne!(dark.ansi, light.ansi);

    // A `BT_BG` override: the light scheme is in force by luma, and what the
    // program is told is the overridden canvas, not the scheme's own white.
    let overridden = terminal_palette(FOLIO_LIGHT, [0xf0, 0xe8, 0xd8], [0x37, 0x35, 0x2f]);
    assert_eq!(overridden.canvas, TerminalCanvas::Light);
    assert_eq!(overridden.background, [0xf0, 0xe8, 0xd8]);
}

/// RED GATE (coordinator ruling 2026-08-29, DESIGN §7.46 ②) — **the theme
/// has one store, and it is `settings.json`.**
///
/// Until this gate there were two: `settings.json`'s `theme_mode`, which the
/// Settings page draws and nobody read, and `session.json`'s `theme`, which
/// nobody could see and which every boot actually obeyed. They disagreed by
/// construction — `SettingsV1::default()` is `System` and `SessionV1`'s
/// default was **Dark** — so a machine whose Windows says light opened a dark
/// window from a brand-new profile, and the first pane in it answered
/// `OSC 11` with the dark canvas. That is the shape §7.46 could not rule out
/// as the cause of the report it was written for: a Codex started on that
/// first dark canvas asks once, is told dark, and keeps it.
///
/// The three cases the ruling names, and the fourth that keeps the fix from
/// costing every existing user their choice.
#[test]
fn the_theme_a_window_opens_in_is_the_one_the_settings_file_names() {
    use bt_persist::SettingsV1;
    use bt_render::{FOLIO_DARK, FOLIO_LIGHT};

    /// What a pane is told when the window opened on `theme`.
    ///
    /// The chain this gate is really about — a boot mode, the canvas it
    /// resolves to, and the background the first `OSC 11` carries — spelled
    /// once so each case below reads as the one thing it varies.
    fn told(mode: ThemeModeV1, os: Option<OsTheme>) -> [u8; 3] {
        let scheme = match resolve_theme_mode(mode, os) {
            Theme::Light => FOLIO_LIGHT,
            Theme::Dark => FOLIO_DARK,
        };
        terminal_palette(scheme, scheme.background, scheme.foreground).background
    }

    // A brand-new profile carries no session theme at all. This is the fact
    // the old default contradicted, and asserting it here is what makes the
    // `None` arm below reachable on a real machine.
    assert_eq!(SessionV1::default().theme, None);

    // ① Fresh profile, Windows says light → the window opens light and the
    //    first pane is told the light canvas.
    let fresh = startup_theme_mode(&SettingsV1::default(), &SessionV1::default());
    assert_eq!(fresh, ThemeModeV1::System);
    assert_eq!(told(fresh, Some(OsTheme::Light)), FOLIO_LIGHT.background);

    // ② `theme_mode = Dark` outranks a Windows that says light. A chosen
    //    mode is a choice, not a preference to be overridden by the OS.
    let chosen_dark = SettingsV1 {
        theme_mode: ThemeModeV1::Dark,
        ..SettingsV1::default()
    };
    let dark = startup_theme_mode(&chosen_dark, &SessionV1::default());
    assert_eq!(dark, ThemeModeV1::Dark);
    assert_eq!(told(dark, Some(OsTheme::Light)), FOLIO_DARK.background);

    // ③ And a chosen Light outranks a Windows that says dark, which is the
    //    case the user in the report is actually in.
    let chosen_light = SettingsV1 {
        theme_mode: ThemeModeV1::Light,
        ..SettingsV1::default()
    };
    let light = startup_theme_mode(&chosen_light, &SessionV1::default());
    assert_eq!(told(light, Some(OsTheme::Dark)), FOLIO_LIGHT.background);

    // ④ **The carry-forward, and it fires exactly here.** Every profile
    //    written before this ruling holds the user's real choice in
    //    `session.json` and an untouched `System` in `settings.json`. Reading
    //    settings alone would silently reset all of them, so a session
    //    document that still carries the retired key is believed once — it is
    //    what that user has been looking at — and the boot that believes it
    //    writes it into settings and stops writing the key.
    let carried = startup_theme_mode(
        &SettingsV1::default(),
        &SessionV1 {
            theme: Some(SessionThemeV1::Light),
            ..SessionV1::default()
        },
    );
    assert_eq!(
        carried,
        ThemeModeV1::Light,
        "an old profile's choice lives in session.json and must survive the move"
    );
    assert_eq!(told(carried, Some(OsTheme::Dark)), FOLIO_LIGHT.background);
}

/// RED GATE (same ruling) — **the canvas is decided before the window
/// exists, so there is no first frame in the other one.**
///
/// The resolution used to read `window.theme()`, which cannot be asked until
/// there is a window; everything that happens between `create_window` and
/// `set_theme` therefore happens on whichever canvas the process was born
/// with. `Window::theme()` also answers `None` on a machine that will not
/// say, and `resolve_theme_mode` reads `None` as dark — a silent wrong answer
/// at exactly the moment the first pane is about to be asked what colour it
/// is standing on.
///
/// Mutation: move `set_theme` back below `create_window`, or resolve from the
/// window again.
#[test]
fn the_canvas_is_in_force_before_the_window_is_made() {
    let body = method_body("Runtime", "create");
    let themed = body
        .find("set_theme(resolved_theme)")
        .expect("`Runtime::create` puts a canvas in force");
    let made = body
        .find("create_window(")
        .expect("`Runtime::create` creates the window");
    let schemes = body
        .find("adopt_stored_schemes(")
        .expect("`Runtime::create` adopts the stored scheme pair");
    assert!(
        schemes < themed,
        "the pair is adopted before the theme picks one of it"
    );
    assert!(
        themed < made,
        "the canvas is settled before the window exists, or the first frame is              painted in the canvas the process was born with"
    );
    assert!(
        !body[..made].contains("window.theme()"),
        "the boot resolution cannot ask a window that does not exist yet"
    );
}

/// **Whose size is it** (user ruling 2026-08-08), as the three rules that decide it.
///
/// Windows delivers the same `Resized` event for a hand on the frame, for a window created at
/// a restored size, and for the resize that rides along with a DPI change. Only the first is a
/// user saying "this is my size"; getting that wrong either folds a window somebody dragged
/// narrow or leaves a restored-too-small session showing slivers.
#[test]
fn a_resize_is_the_users_unless_the_program_asked_for_it() {
    let opened = PhysicalSize::new(1600, 900);
    let dragged = PhysicalSize::new(700, 400);

    // ① Startup, exactly as it really arrives. `Runtime::create` leaves a standing claim and
    // the first rectangle answers it — whatever the OS made of the request. Repeat deliveries
    // of that same rectangle change nothing, so a session restored into a window too small for
    // its tree still folds.
    let (mut policy, mut claimed) = (SizePolicy::Lawful, None);
    for _ in 0..3 {
        (policy, claimed) = size_authority_for_rectangle(policy, claimed, opened);
    }
    assert_eq!(
        claimed,
        Some(opened),
        "the pending claim adopted the opening rectangle"
    );
    assert_eq!(
        policy,
        SizePolicy::Lawful,
        "a restored session must still fold; startup is not a gesture"
    );

    // ② A rectangle we did not ask for is a hand on the frame.
    (policy, claimed) = size_authority_for_rectangle(policy, claimed, dragged);
    assert_eq!(policy, SizePolicy::Sovereign);
    assert_eq!(
        claimed,
        Some(opened),
        "the claim is not rewritten by a gesture"
    );

    // ③ Sovereignty is not handed back by coincidence. Dragging back through the opening size
    // is not a change of mind about owning the window.
    let (back, _) = size_authority_for_rectangle(policy, claimed, opened);
    assert_eq!(
        back,
        SizePolicy::Sovereign,
        "only a claim returns the layout to law"
    );

    // ④ ...and the claim is `claim_lawful_layout`, whose whole content is the pair below. A
    // DPI change makes it *before* the new rectangle is known, so the resize Windows sends
    // alongside `WM_DPICHANGED` is adopted rather than read as a drag — no matter what size it
    // turns out to be, and no matter how many times it is announced.
    let (mut policy, mut claimed) = (SizePolicy::Lawful, None);
    let after_dpi = PhysicalSize::new(1050, 590);
    for _ in 0..2 {
        (policy, claimed) = size_authority_for_rectangle(policy, claimed, after_dpi);
    }
    assert_eq!((policy, claimed), (SizePolicy::Lawful, Some(after_dpi)));

    // ⑤ The transient that made this rule necessary: winit announced 826x1271 at startup while
    // the window's own inner size was 800x1200. Judging on the presentation rectangle — the
    // number the solver is actually handed — never sees it.
    let (policy, _) = size_authority_for_rectangle(
        SizePolicy::Lawful,
        Some(PhysicalSize::new(800, 1200)),
        PhysicalSize::new(800, 1200),
    );
    assert_eq!(policy, SizePolicy::Lawful);
}

/// **RED — a seam that changes its mind twenty times is paid for once**
/// (§7.50, user ruling 2026-08-31).
///
/// The reporter's log has fifteen `stage=scale-factor-changed` lines inside
/// one drag, alternating 1.5 and 2. Each of them used to remeasure the
/// terminal font, walk every leaf in every tab, rebuild every grid — and
/// re-state this window's minimum size to the OS, which is a `SetWindowPos`
/// aimed at a window the reader still had hold of.
///
/// Two claims, and the second is the one worth the type: outside a drag
/// nothing is deferred at all, so a scale change on a settled window is
/// answered on the spot exactly as it always was.
///
/// Red gate: return `true` unconditionally from `arrived` and the count goes
/// to twenty; return `true` unconditionally from `due` and the settled-window
/// half fires a payment nobody owes.
#[test]
fn a_seam_that_flips_twenty_times_under_one_hand_is_settled_once() {
    let mut settlement = DpiSettlement::default();

    // Twenty flips with the hand on the frame. None of them buys the
    // expensive half, and asking `due` while the hand is still on does not
    // smuggle it in either. No turn is expected there — Windows pumps its own
    // message loop while the modal move/size loop runs, which delivers events
    // but sends no `AboutToWait` (see `hang_watch`'s
    // `a_thread_that_answers_is_alive_even_when_its_loop_has_stopped_turning`)
    // — and the settlement does not rely on that.
    let mut paid = 0;
    for _ in 0..20 {
        if settlement.arrived(true) {
            paid += 1;
        }
        if settlement.due(true) {
            paid += 1;
        }
    }
    assert_eq!(paid, 0, "nothing is paid for while the window is moving");

    // The hand lets go. One payment, on the first turn after it, and the
    // turn after that is quiet.
    assert!(settlement.due(false), "the deferred change comes due once");
    assert!(!settlement.due(false), "and only once");
    assert_eq!(settlement, DpiSettlement::default());

    // And a window nobody is holding is not deferred at all: two opposite
    // changes, two payments, and no third adjustment invented between them.
    let mut settlement = DpiSettlement::default();
    assert!(settlement.arrived(false), "1.5 is answered on the spot");
    assert!(!settlement.due(false), "and owes nothing afterwards");
    assert!(settlement.arrived(false), "so is 2");
    assert!(!settlement.due(false), "and it owes nothing either");
}

/// **RED — a scale change owes a rectangle, and is paid by the first one to
/// arrive** (T-CARD-ANCHOR-DPI, user report 2026-09-14; §7.1.6b′ ④).
///
/// The report is a focus card scrolled back through a shell's output on a
/// full-screen window carried 4K → 1080p → 4K. It came back showing
/// something else, and the reason is on this road rather than on the card's:
/// winit raises `ScaleFactorChanged` from inside its `WM_DPICHANGED`
/// handler, *before* the `SetWindowPos` that handler ends with, so the whole
/// of `scale_factor_changed` works with the rectangle of the display being
/// left. Cutting a grid out of it counts one display's pixels in the other
/// display's cells — and a grid change is a vendor reflow, which freezes
/// whatever it pushes off the top at the width it pushed it off at, for
/// ever. The panes went 240 → 320 → 160 → 120 → 240 columns, two of those
/// widths belonging to no display, and the card's transcript came back cut
/// differently.
///
/// Red gate: return `true` unconditionally from `may_cut_a_grid` and the
/// announcement buys nothing; return `false` unconditionally from `due` and
/// a maximized window whose own display changed scale — the one case that
/// produces no `Resized` at all — keeps the grid it had for ever.
#[test]
fn a_scale_change_owes_a_rectangle_and_is_paid_by_the_first_one_to_arrive() {
    // A window nobody has moved cuts grids exactly as it always did.
    let mut rectangle = DpiRectangle::default();
    assert!(rectangle.may_cut_a_grid());
    assert!(!rectangle.due(), "nothing is owed before a scale changes");

    // The scale arrives without its rectangle. Nothing is cut until one does.
    rectangle.announced();
    assert!(!rectangle.may_cut_a_grid());
    rectangle.announced();
    assert!(
        !rectangle.may_cut_a_grid(),
        "a seam that changes its mind twice still owes one rectangle"
    );

    // The `Resized` Windows sends alongside the scale change: the debt is
    // paid by the event itself, so the turn after it owes nothing.
    rectangle.arrived();
    assert!(rectangle.may_cut_a_grid());
    assert!(
        !rectangle.due(),
        "the rectangle arrived; nothing is deferred"
    );

    // And the case that sends no `Resized` at all — a maximized window whose
    // own display's scale changed, every physical pixel where it was. One
    // payment, on the first turn, and the turn after it is quiet.
    rectangle.announced();
    assert!(
        rectangle.due(),
        "the grids are owed to the rectangle in hand"
    );
    assert!(rectangle.may_cut_a_grid());
    assert!(!rectangle.due(), "and owed once");
    assert_eq!(rectangle, DpiRectangle::default());

    // A rectangle that arrives when none was owed is not a payment waiting
    // to be spent: it clears nothing and leaves nothing behind.
    rectangle.arrived();
    assert!(!rectangle.due());
    assert!(rectangle.may_cut_a_grid());
}

/// **RED (shape) — the two halves of a DPI change are on either side of the
/// question, and the deferred half has somewhere to be spent** (§7.50).
///
/// Two facts about where a line sits relative to another line, which no
/// value in the program carries — so they are held against the source, the
/// way this file's other structural promises are.
///
/// ① The swapchain and the seat solve happen *before* the drag is asked
/// about, because they are owed on every judgement; the font remeasure and
/// everything downstream of it happen *after*, because they are owed only by
/// a window that has stopped moving. A check that drifted below
/// `apply_scale_factor` would defer nothing at all.
///
/// ② A change written down during a drag is paid for from `Runtime::turn`.
/// There is no event at the end of the OS's modal move/size loop, so a
/// settlement with no turn to be spent on is a window that keeps the font it
/// left the other display with, forever.
#[test]
fn the_deferred_half_of_a_dpi_change_is_asked_about_late_and_spent_on_a_turn() {
    let reconcile = method_body("Runtime", "reconcile_authoritative_dpi");
    let solved = reconcile
        .find("self.resolve_seat_layout(render_physical);")
        .expect("the seat rectangles are re-solved against the new surface");
    let asked = reconcile
        .find(".arrived(self.window.custom_window_frame.in_size_move())")
        .expect("the drag is asked whether the expensive half may run");
    let remeasured = reconcile
        .find("self.apply_scale_factor(snapshot.authoritative_scale)?;")
        .expect("the terminal font is remeasured at the new scale");
    assert!(
        solved < asked && asked < remeasured,
        "the cheap half is owed on every judgement and the expensive half only \
             by a window that has stopped moving"
    );

    assert!(
        method_body("Runtime", "turn").contains("self.settle_deferred_dpi()?;"),
        "a deferred DPI change is spent on the first turn after the hand lets go"
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

/// **RED (shape) — the scale change's own arm says the panel's scroll out
/// loud** (the pin the fix asks for).
///
/// The restatement needs the scale the offsets were measured at, and that
/// number lives in exactly one place for exactly as long as it takes
/// `update_scale_factor` to overwrite it. A reading taken after the
/// remeasure is the new scale twice over and the ratio is 1 — a fix that
/// silently does nothing. So the order is held against the source, the way
/// this file's other structural promises are.
#[test]
fn the_scale_change_arm_restates_the_cards_columns_scroll() {
    let body = method_body("Runtime", "apply_scale_factor");
    let read = body
        .find("let measured_at = self.window.renderer.scale_factor();")
        .expect("the scale the panel's lists were measured at is read");
    let remeasured = body
        .find(".update_scale_factor(&mut self.app.gpu, scale_factor)")
        .expect("the renderer is remeasured at the new scale");
    let restated = body
        .find("self.restate_panel_scroll(measured_at, scale_factor);")
        .expect("the panel's scroll offsets are restated in the new scale's pixels");
    assert!(
        read < remeasured && remeasured < restated,
        "the old scale is read before it is overwritten, and spent after"
    );
}

#[test]
fn tab_state_machine_creates_switches_and_closes_to_the_adjacent_tab() {
    let mut tabs = vec!["first"];
    tabs.push("second");
    let mut active = tabs.len() - 1;
    assert_eq!(
        (tabs.as_slice(), active),
        (["first", "second"].as_slice(), 1)
    );

    active = 0;
    assert_eq!(active, 0, "clicking a tab changes only the active index");
    assert_eq!(
        tab_close_action(tabs.len(), active, 0),
        TabCloseAction::Keep { active_tab: 0 },
        "closing the active left tab activates its right neighbour"
    );
    tabs.remove(0);
    assert_eq!(tabs, ["second"]);
    assert_eq!(
        tab_close_action(tabs.len(), 0, 0),
        TabCloseAction::CloseWindow,
        "the last tab delegates to the existing WM_CLOSE path"
    );
}

/// The jump's flash lands on the row that is **showing** the command, and on
/// nothing at all when that content is not on the glass.
///
/// Read out of the presented frame's own `cell_anchors` rather than from the
/// projection, for the reason [`Runtime::command_flash_layer`] gives: the
/// frame is the picture, and a second derivation of "which row is that
/// content in" is a second thing that can be off by one.
///
/// MUTATION: drop the containment check at the end of [`frame_row_of_anchor`]
/// and the last assertion goes red — an anchor below the last drawable row
/// would light the last one, which is a band drawn over the wrong command.
#[test]
fn the_flash_finds_the_row_a_frame_is_showing_a_command_on() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(4).unwrap());
    let mut projection = session.new_projection(session.layout_key());
    session.feed(b"one\r\ntwo\r\nthree\r\n").unwrap();
    session.refresh_projection(&mut projection);
    let frame = session
        .viewport_frame(&mut projection)
        .expect("a frame for a four-row grid");
    let columns = frame.columns.get() as usize;

    // Every row of the frame answers itself, which is the property the whole
    // band is placed by.
    for row in 0..frame.drawable_rows() {
        let anchor = frame.cell_anchors[row * columns].start.clone();
        assert_eq!(
            frame_row_of_anchor(&frame, &anchor).map(|found| found.top_subpixels),
            Some(frame.row_map[row].top_subpixels),
            "row {row} does not find itself"
        );
    }

    // The alternate screen is an isolated namespace (§3.2), so its
    // coordinates are neither before nor after this frame's — the honest
    // answer is no row rather than a guessed one.
    assert!(
        frame_row_of_anchor(
            &frame,
            &bt_doc::ContentAnchor::Live {
                screen: bt_doc::ScreenId::Alternate,
                point: bt_doc::GridPoint { row: 0, column: 0 },
                bias: bt_doc::Bias::Before,
                generation: bt_doc::GridGeneration(1),
            }
        )
        .is_none(),
        "an alternate-screen anchor lights nothing on a primary frame"
    );

    // And content past the bottom of the frame: a row that has not been drawn
    // is not a row the last one may stand in for.
    let past = bt_doc::ContentAnchor::Live {
        screen: bt_doc::ScreenId::Primary,
        point: bt_doc::GridPoint {
            row: frame.grid_rows.get() + 40,
            column: 0,
        },
        bias: bt_doc::Bias::Before,
        generation: bt_doc::GridGeneration(1),
    };
    assert!(frame_row_of_anchor(&frame, &past).is_none());
}

#[test]
fn input_routes_only_to_the_active_tab_and_background_output_survives_switching() {
    let mut writes = [Vec::new(), Vec::new()];
    let active = 1;
    active_item_mut(&mut writes, active).extend_from_slice(b"whoami\r");
    assert!(writes[0].is_empty());
    assert_eq!(writes[1], b"whoami\r");

    let mut sessions = [
        DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(2).unwrap()),
        DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(2).unwrap()),
    ];
    sessions[0].feed(b"kept in background").unwrap();
    let mut projection = sessions[0].new_projection(sessions[0].layout_key());
    sessions[0].refresh_projection(&mut projection);
    let frame = sessions[0].viewport_frame(&mut projection).unwrap();
    let visible = frame
        .cells
        .iter()
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    assert!(visible.contains("kept in background"));
}

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

/// The gear no longer *is* the theme switch — it opens the surface the
/// switch lives on, and nothing about a caption button decides a colour any
/// more. The theme now comes from a press on a picker item, which
/// `settings::theme_requested` answers and `settings.rs` pins.
///
/// Red gate: the previous version of this test asserted the gear returned
/// the opposite theme. That function is gone, and this one fails the moment
/// something starts deciding a theme from a `ChromeTarget` again.
#[test]
fn the_gear_opens_the_settings_surface_rather_than_deciding_a_theme() {
    let mut panel = settings::SettingsPanel::default();
    let rows = settings::visible_rows(seats::TabLayoutMode::Horizontal);
    panel.toggle(settings::SettingsContent {
        rows: &rows,
        shortcuts: &[],
        profiles: &[],
        scheme_files: &[],
        advanced: settings::AdvancedOpen::default(),
        advanced_reveal: None,
        editor: None,
        values: Box::leak(Box::new(settings::SettingsValues::sample())),
    });
    assert!(panel.is_open(), "the gear's verb is 'open the dialog'");
    assert_eq!(
        settings::theme_requested(settings::SettingsTarget::Close),
        None,
        "nothing but a picker item asks for a theme"
    );
    assert_eq!(
        settings::theme_requested(settings::SettingsTarget::Choice(
            settings::SettingsRow::Theme,
            0
        )),
        Some(ThemeModeV1::Light),
        "the mock-up's picker opens with Light (2500)"
    );
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

/// RED (ticket 14) — **a share handed over meets the same program list a local file meets.**
///
/// The share arm leaves through the files column's door (`Handoff::Open`), which reads
/// `bt_platform::names_a_program` — so `\\server\share\run.cmd` under `Ctrl` is refused in the
/// words a local `.cmd` is refused in, and the reader is told in the same notice. Driven through
/// the real OS hand-off lane: the table's arm, the Runtime's request, the lane's thread, the door.
/// The door refuses a program before it calls the system, so nothing is launched and no network
/// is touched.
///
/// Red on the base: the arm is `Preview` and there is no hand-off to make.
///
/// MUTATION: build the share's request as an address in `unverified_reference_handoff`
/// (`Handoff::Address`, `shell_execute` — the door without the list) and the request below is not
/// `Open`.
#[test]
fn a_share_handed_over_meets_the_same_program_list() {
    let HyperlinkActivation::Share(path) = hyperlink_activation(
        true,
        true,
        "file://server/share/run.cmd",
        bt_transcript::paths::PathNamer::ThisWindow,
        &|_| panic!("a share is never asked about"),
    ) else {
        panic!("Ctrl on a share is the share arm");
    };
    let request = unverified_reference_handoff(&path);
    let bt_platform::Handoff::Open(handed) = &request else {
        panic!("a share must leave through the door that reads the program list: {request:?}");
    };
    assert_eq!(handed, &path);

    let mut lane = handoff_lane::HandoffLane::spawn(|| {}).expect("the lane starts");
    let id = lane.submit(bt_platform::NativeWindow::stand_in(0), request);
    let deadline = Instant::now() + Duration::from_secs(10);
    let answer = loop {
        if let Some(answer) = lane.answers().into_iter().find(|answer| answer.id == id) {
            break answer;
        }
        assert!(Instant::now() < deadline, "the lane answered");
        std::thread::sleep(Duration::from_millis(2));
    };
    let reason = answer.outcome.expect_err("a program on a share is refused");
    assert_eq!(reason, bt_platform::PROGRAM_REFUSED, "the door's own words");
    assert_eq!(
        UNVERIFIED_REFERENCE_REFUSAL.words(&reason).notice,
        Some(files_program_refused_notice()),
        "and the reader is told, as for a local program"
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
/// `preview_reference_row` — the base's answer — and the share rows go red.
#[test]
fn a_document_link_answers_the_same_row_as_a_terminal_reference() {
    let directory = bt_testpath::temp_path("folio-t14-document-links");
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a scratch folder");
    let document = directory.join("README.md");
    let notes = directory.join("notes.md");
    std::fs::write(&document, b"# x").expect("a document");
    std::fs::write(&notes, b"x").expect("a file it links to");
    let verdict = bt_term::verify_path(&notes);
    assert!(verdict.exists && !verdict.directory);
    let ledger = |path: &Path| (path == notes.as_path()).then(|| verdict.clone());

    let notes_uri = bt_transcript::paths::local_path_to_file_uri(&notes);
    for (document_target, terminal_uri) in [
        ("mailto:x@example.com", "mailto:x@example.com".to_owned()),
        (
            r"\\server\share\a.md",
            "file://server/share/a.md".to_owned(),
        ),
        (
            "file://server/share/a.md",
            "file://server/share/a.md".to_owned(),
        ),
        (
            "https://example.test/a",
            "https://example.test/a".to_owned(),
        ),
        ("./notes.md", notes_uri),
    ] {
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

/// RED (39) — **A `file:` address hands the file it names, whatever the file is, never the address.**
///
/// The road is a property of the address, not of page-ness. A `.html` on this disk keeps the path
/// road it has had since 2026-08-23 (`Runtime::open_local_path`, the files column's door), and so
/// does a `file:` address that is not a page — a picture, a program — because that door reads the
/// program list: a `file:///…/x.exe` must never leave as a bare address for the shell to run.
/// Real files, minted by the real `webnav::Mint::file`, read back by the real
/// `webnav::LocalFileUrl`.
///
/// MUTATION: in `preview_page_browser_hand_off`, answer a `web_url()` with
/// `preview_page_hand_off(source).map(PageHandOff::File)` instead of `LocalFileUrl::parse` — the
/// `.png` and the `.exe` hand nothing (or, with the `file:` fork removed too, the address).
#[test]
fn a_file_address_hands_the_file_it_names_whatever_the_file_is() {
    let dir = bt_testpath::temp_path("folio-t39-page-file");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    for (name, bytes) in [
        ("a.html", &b"<h1>a</h1>"[..]),
        ("b.png", &b"PNG"[..]),
        ("x.exe", &b"MZ"[..]),
    ] {
        let file = dir.join(name);
        std::fs::write(&file, bytes).expect("a file on a disk");
        let canonical = std::fs::canonicalize(&file).expect("canonicalise it");
        let url = webnav::Mint::file(&canonical)
            .expect("a local path mints")
            .target()
            .expect("a mint names its URL")
            .to_owned();
        let Some(PageHandOff::File(path)) =
            preview_page_browser_hand_off(&preview::PreviewSource::Web(webnav::switcher_key(&url)))
        else {
            panic!("a file: address takes the file door, never the address: {url:?}");
        };
        assert_eq!(
            std::fs::canonicalize(&path).expect("the handed path names a file on the disk"),
            canonical,
            "the path handed over is the address's own file: {url:?}"
        );
    }
    // A local page shown as a file keeps the page answer, and a seat with no page has nothing.
    let page = std::fs::canonicalize(dir.join("a.html")).expect("the page");
    assert_eq!(
        preview_page_browser_hand_off(&preview::PreviewSource::file(&page)),
        Some(PageHandOff::File(page.clone()))
    );
    assert_eq!(
        preview_page_browser_hand_off(&preview::PreviewSource::file(dir.join("notes.md"))),
        None
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn peek_hover_settles_after_the_delay_and_slides_along_one_span_without_restarting() {
    let start = Instant::now();
    let path = PathBuf::from(r"C:\img\a.png");
    let mut hover = PeekHover::default();

    let subject = PeekSubject::from_path(path.clone());
    let pane = SeatId(1);
    assert!(!hover.observe(
        Some((subject.clone(), pane)),
        PhysicalPosition::new(10.0, 10.0),
        start
    ));
    assert!(
        hover
            .activate_if_due(start + Duration::from_millis(299))
            .is_none()
    );
    // Sliding along the same path span refreshes the anchor but keeps the original clock:
    // the flyout settles where the pointer last was, without ever restarting the delay.
    assert!(!hover.observe(
        Some((subject.clone(), pane)),
        PhysicalPosition::new(30.0, 12.0),
        start + Duration::from_millis(200)
    ));
    let settled = hover
        .activate_if_due(start + Duration::from_millis(300))
        .expect("original deadline must fire");
    assert_eq!(settled.subject, subject);
    assert_eq!(settled.pointer.x, 30.0);
    // While active, staying on the span neither hides nor re-arms.
    assert!(!hover.observe(
        Some((subject.clone(), pane)),
        PhysicalPosition::new(31.0, 12.0),
        start + Duration::from_millis(400)
    ));
    assert!(hover.show_at.is_none());
    // Leaving the span hides the flyout and drops all state.
    assert!(hover.observe(
        None,
        PhysicalPosition::new(31.0, 40.0),
        start + Duration::from_millis(500)
    ));
    assert!(hover.active.is_none());
}

/// PIN — a peek belongs to the pane the pointer was in, and carries it.
///
/// Two panes, and the *same file* printed in both — which is what makes the pin bite, because
/// subject identity alone cannot tell the two hovers apart. Sliding from one pane's copy to the
/// other's has genuinely left the reference the flyout was raised over: the old flyout must go
/// down, a fresh settle clock must run, and what settles must name the pane it settled in, so
/// the box can be sized and placed against that pane instead of against whichever one happens
/// to hold the keyboard.
///
/// Drop the seat from the span's identity and every assertion below reverses.
#[test]
fn one_file_printed_in_two_panes_is_two_hovers_and_each_names_its_own_pane() {
    let start = Instant::now();
    let subject = PeekSubject::from_path(PathBuf::from(r"C:\img\shared.png"));
    let left = SeatId(1);
    let right = SeatId(2);
    let mut hover = PeekHover::default();

    assert!(!hover.observe(
        Some((subject.clone(), left)),
        PhysicalPosition::new(100.0, 200.0),
        start
    ));
    let settled = hover
        .activate_if_due(start + Duration::from_millis(300))
        .expect("the left pane's hover settles");
    assert_eq!(
        settled.seat, left,
        "a settled peek names the pane whose cells the pointer was on",
    );

    // Straight across into the other pane's copy of the same file.
    assert!(
        hover.observe(
            Some((subject.clone(), right)),
            PhysicalPosition::new(900.0, 200.0),
            start + Duration::from_millis(400)
        ),
        "leaving the pane takes the flyout down even though the file is the same",
    );
    assert!(
        hover
            .activate_if_due(start + Duration::from_millis(600))
            .is_none(),
        "the second pane's hover runs its own settle clock",
    );
    let settled = hover
        .activate_if_due(start + Duration::from_millis(700))
        .expect("the right pane's hover settles in its turn");
    assert_eq!(settled.seat, right);
    assert_eq!(
        settled.pointer,
        PhysicalPosition::new(900.0, 200.0),
        "the anchor is the window point the hover settled on, untranslated",
    );

    // And staying inside one pane still slides along one span, as it always did.
    assert!(!hover.observe(
        Some((subject, right)),
        PhysicalPosition::new(920.0, 200.0),
        start + Duration::from_millis(800)
    ));
    assert!(hover.show_at.is_none());
}

#[test]
fn peek_hover_switching_paths_hides_the_old_flyout_and_restarts_the_clock() {
    let start = Instant::now();
    let first = PeekSubject::from_path(PathBuf::from(r"C:\img\a.png"));
    let second = PeekSubject::from_path(PathBuf::from(r"C:\img\b.png"));
    let mut hover = PeekHover::default();
    let pane = SeatId(1);
    hover.observe(
        Some((first, pane)),
        PhysicalPosition::new(10.0, 10.0),
        start,
    );
    assert!(
        hover
            .activate_if_due(start + Duration::from_millis(300))
            .is_some()
    );
    let hidden = hover.observe(
        Some((second.clone(), pane)),
        PhysicalPosition::new(50.0, 10.0),
        start + Duration::from_millis(400),
    );
    assert!(hidden, "switching spans must hide the visible flyout");
    assert!(
        hover
            .activate_if_due(start + Duration::from_millis(600))
            .is_none(),
        "the second span runs a fresh settle clock"
    );
    let settled = hover
        .activate_if_due(start + Duration::from_millis(700))
        .expect("second span settles on its own deadline");
    assert_eq!(settled.subject, second);
}

/// PIN (user repro 2026-08-02, re-seated by the frame-derived ruling 2026-08-04): one hovered
/// cell resolves to one reference, through the frame's own row stride, and where a printed path
/// and a link target cover the same cell the pointer answers with the text it is standing on.
///
/// What the four verbs share is this lookup: the scan produced the list, and hover, click and
/// peek all ask it the same question about the same `GridHit`. The list's own contents — which
/// shapes are in it, and that a `file://` to a `.txt` is in none — are bt-term's to pin, beside
/// the detector that decides it (`underline_coverage_equals_peek_coverage_for_every_shape`).
#[test]
fn one_hit_resolves_to_one_reference_through_the_frames_own_stride() {
    let printed = PathBuf::from(r"D:\from-text.png");
    let linked = PathBuf::from(r"D:\layout-preview.png");
    let references = FrameImageReferences {
        columns: 10,
        references: vec![
            // The scan's order: printed text first, link targets after it.
            bt_term::FrameImageReference {
                path: printed.clone(),
                cells: vec![12, 13, 14],
                verified: true,
            },
            bt_term::FrameImageReference {
                path: linked.clone(),
                cells: vec![10, 11, 12, 13, 14, 15],
                verified: true,
            },
        ],
    };
    let at = |row, column| {
        references
            .at(bt_render::GridHit { row, column })
            .map(|reference| reference.path.clone())
    };
    assert_eq!(
        at(1, 2),
        Some(printed),
        "text under the pointer is what the pointer was put on",
    );
    assert_eq!(
        at(1, 0),
        Some(linked),
        "and the link answers where its label spells no file",
    );
    assert_eq!(at(1, 6), None, "one column past the link is ordinary text");
    assert_eq!(at(0, 2), None, "the row is part of the address");
    assert_eq!(
        FrameImageReferences::default().at(bt_render::GridHit { row: 0, column: 0 }),
        None,
        "a frame with no references answers nothing anywhere",
    );
}

/// PIN (band retirement ruling, 2026-08-03, docs §6.1): the peek's third source is an OSC 1337
/// payload, which names no file, and the pipeline tells the two apart by exactly one property —
/// whether a cache miss has anything to read.
///
/// A named file's identity is its normalized path, so the same file spelled two ways is one
/// hover and one cache entry. A payload's identity is the decoder's content key, and it carries
/// no path at all: the bytes came through the stream and were remembered when the decode landed,
/// so a miss is a hover that arrived early, never a disk read to schedule. That `path: None` is
/// the whole of the difference is what keeps `show_or_request_peek` one function.
#[test]
fn a_stream_payload_is_a_peek_subject_with_nothing_to_read() {
    let by_path = PeekSubject::from_path(PathBuf::from(r"C:\img\a.png"));
    assert_eq!(
        by_path.key,
        normalized_local_image_path_key(std::path::Path::new(r"C:\img\a.png")),
        "a named file is identified the way the decoder identifies it",
    );
    assert!(by_path.path.is_some(), "a named file is readable on a miss");
    assert_eq!(
        PeekSubject::from_path(PathBuf::from(r"C:\IMG\A.PNG")),
        by_path,
        "one file spelled two ways is one hover and one cache entry",
    );

    let payload = PeekSubject::from_content_key("image:sha-abc".to_owned());
    assert_eq!(payload.key, "image:sha-abc");
    assert!(
        payload.path.is_none(),
        "a stream payload has no file behind it, so a cache miss reads nothing",
    );
    assert_ne!(payload, by_path);
}

/// PIN (verification ruling 2026-08-04, the warm peek): the decode a verified reference already
/// paid for is filed under the very key the hover looks up, so the flyout opens from cache and
/// no second read of the same file is ever scheduled.
///
/// `show_or_request_peek` sends a `PeekImage` task on exactly one condition — a `None` entry
/// under `PeekSubject::key`. So "the peek is warm" and "the two keys are the same string" are
/// the same statement, and it is the one asserted here. The stream-payload arm is asserted
/// beside it because both shapes go through this one function and must not converge: a payload
/// has no path to key by.
///
/// RED CHECK: keying a named file's verification decode by `decoded.key` (its content identity)
/// instead of its path leaves the hover's lookup missing, and the first assertion goes red —
/// which is precisely the "decoded twice, cached twice" defect the shared key rules out.
#[test]
fn a_verified_references_decode_is_filed_under_the_key_the_hover_asks_by() {
    let path = PathBuf::from(r"C:\img\Sunset.PNG");
    let decoded = bt_term::DecodedInlineImage {
        occurrence_id: 7,
        key: "image:0123456789abcdef0123456789abcdef".to_owned(),
        rgba: Arc::from(vec![0u8; 4]),
        width_px: 1,
        height_px: 1,
        native_size: None,
        animated: false,
    };

    assert_eq!(
        peek_cache_key_for_decode(
            &bt_term::InlineImageSource::LocalPath(path.clone()),
            &decoded
        ),
        PeekSubject::from_path(path.clone()).key,
        "the verification decode lands exactly where the hover will look for it",
    );
    assert_eq!(
        peek_cache_key_for_decode(
            &bt_term::InlineImageSource::LocalPath(PathBuf::from(r"c:/img/sunset.png")),
            &decoded
        ),
        PeekSubject::from_path(path).key,
        "and one file spelled two ways is still one warm entry",
    );
    assert_eq!(
        peek_cache_key_for_decode(
            &bt_term::InlineImageSource::Osc1337(b"AAAA".to_vec()),
            &decoded
        ),
        PeekSubject::from_content_key(decoded.key.clone()).key,
        "a stream payload has no path, so it stays keyed by content",
    );
}

/// Pin (a) of the peek raster defect: every peek pixel that reaches the renderer is one the
/// flyout draws. The chain the app runs — the renderer's box, the worker's resample, the
/// thumbnail slot, the overlay — is walked end to end here, so a future edit that hands the
/// renderer a native decode again fails on the resident byte count and on the texture key.
#[test]
fn the_peek_overlay_carries_display_sized_pixels_under_a_display_sized_key() {
    // A decode far larger than any flyout: 1024x768 in a 640x480 pane.
    let (native_width_px, native_height_px) = (1024_u32, 768_u32);
    let native_rgba: Arc<[u8]> =
        Arc::from(vec![
            0x40_u8;
            native_width_px as usize * native_height_px as usize * 4
        ]);
    let content_key = "image:0123456789abcdef0123456789abcdef".to_owned();

    let (display_width_px, display_height_px) =
        bt_render::peek_thumbnail_extent(640.0, 480.0, 8.0, 1.0, native_width_px, native_height_px)
            .expect("the pane can host the flyout");
    assert!(
        display_width_px < native_width_px && display_height_px < native_height_px,
        "the 40% cap is what makes the flyout smaller than its decode",
    );

    let target: PeekThumbnailTarget = (content_key.clone(), display_width_px, display_height_px);
    let task = peek_scale_task(
        &target,
        Arc::clone(&native_rgba),
        native_width_px,
        native_height_px,
    );
    let thumbnail = PeekThumbnail::from_scaled(bt_term::scale_inline_image(&task));
    let overlay = thumbnail.overlay(
        bt_render::SeatViewport {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        },
        PhysicalPosition::new(120.0, 90.0),
    );
    assert_eq!(
        (overlay.width_px, overlay.height_px),
        (display_width_px, display_height_px),
    );
    assert_eq!(
        overlay.rgba.len(),
        display_width_px as usize * display_height_px as usize * 4,
        "the resident bytes the renderer uploads are the display box, not the decode",
    );
    assert!(
        overlay.rgba.len() * 16 < native_rgba.len(),
        "the defect uploaded {} bytes where {} suffice",
        native_rgba.len(),
        overlay.rgba.len(),
    );
    assert_eq!(
        overlay.key,
        bt_term::display_texture_key(&content_key, display_width_px, display_height_px),
        "the display size is part of the texture identity, so the shared LRU can never \
             serve a raster sized for another box",
    );
    assert!(
        thumbnail.matches(&target),
        "the slot answers the question the hover asked, so a raster for another box is \
             never presented as this one",
    );
}

#[test]
fn osc_8_display_text_hits_the_real_target_uri() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(24).unwrap(), NonZeroU32::new(2).unwrap());
    session
        .feed(b"\x1b]8;;https://actual.example/login\x1b\\trusted label\x1b]8;;\x1b\\")
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let mut frame = session.viewport_frame(&mut projection).unwrap();

    let hit = frame.hyperlink_at(0, 4).unwrap();
    assert_eq!(hit.uri, "https://actual.example/login");
    assert!(frame.underline_hyperlink(&hit));
    assert!(frame.cells[..13].iter().all(|cell| {
        cell.style
            .flags
            .contains(bt_transcript::CellFlags::UNDERLINE)
    }));
    assert!(
        !frame.cells[13]
            .style
            .flags
            .contains(bt_transcript::CellFlags::UNDERLINE)
    );
}

/// The mutant: the window this fix replaced, which reprojected on every
/// animation tick no matter what the picture on the glass already said.
fn always_recompose(_: PictureOnGlass) -> bool {
    false
}

/// The same six hundred ticks under the mutant, so the assertion above is
/// known to have teeth: the count it asserts is zero was six hundred.
#[test]
fn the_unconditional_reprojection_this_replaced_costs_one_projection_a_tick() {
    let mut harness = PtyPresentationHarness::new(80, 24);
    harness.feed_drain(b"$ sleep 30\r\n");
    harness.present_pending();
    let projections_before = harness.viewport_frames;

    for _ in 0..600 {
        harness.chrome_tick(always_recompose);
    }

    assert_eq!(harness.viewport_frames - projections_before, 600);
}

/// And it is never *slower* than the frame either: a tick offered exactly
/// one frame after the last is due, not one frame and a bit.
#[test]
fn a_tick_offered_exactly_one_frame_later_is_due() {
    let start = Instant::now();
    let frame = pace::DEFAULT_FRAME_INTERVAL;
    assert!(
        strip_animation_tick_is_due(None, start, frame),
        "the first ever"
    );
    assert!(strip_animation_tick_is_due(
        Some(start),
        start + frame,
        frame
    ));
    assert!(!strip_animation_tick_is_due(
        Some(start),
        start + frame - Duration::from_micros(1),
        frame
    ));
}

/// RED — **a tab whose only pane is a preview draws like any other**
/// (§7.10 ④‴, user report on `next21`).
///
/// The three funnels that answer a retained-state change —
/// `present_chrome_change`, `present_peek_overlay`,
/// `represent_on_screen_frame` — re-queue the picture already on the glass
/// and ask for a redraw; on a tab with no shell there is no such picture to
/// re-queue, ever, so all they leave behind is the bare request. Answering
/// that request with "nothing composed, nothing owed" is a preview pane
/// whose wheel moves the document in memory and never puts it on the glass.
///
/// RED GATE: drop the `|| !tab_has_a_shell` and the second assertion goes
/// red — which is exactly the shipped build, measured at ten notches and
/// zero frames.
#[test]
fn a_redraw_asked_for_by_a_tab_with_no_shell_is_always_a_present() {
    assert!(
        !a_bare_redraw_still_owes_a_present(false, true),
        "a tab with a shell and no filed debt owes nothing: everything that \
             changes such a window publishes a frame or files the debt first"
    );
    assert!(
        a_bare_redraw_still_owes_a_present(false, false),
        "a tab with no shell has only one kind of frame, so every redraw it \
             is asked for is that kind — without this its wheel scrolls a \
             document nobody draws"
    );
    assert!(
        a_bare_redraw_still_owes_a_present(true, true),
        "and the debt a chrome animation files is still the debt it always was"
    );
    assert!(a_bare_redraw_still_owes_a_present(true, false));

    // And the half a value cannot hold: that `redraw` really asks this
    // question rather than the narrower one it asked before. A funnel that
    // files no debt and a slot that is empty is the whole of the defect, and
    // it is invisible from any value this function could be handed. The
    // needle is split so that this assertion cannot find itself.
    let redraw = method_body("Runtime", "redraw");
    assert!(
        redraw.contains("a_bare_redraw_still_owes_a_present("),
        "`redraw` drops a bare request again, so a preview alone in a tab \
             stops drawing:\n{redraw}"
    );
}

/// RED — **a page a modal covers is drawn as the frame it last stood on the
/// glass with** (§7.8 ⑩, user report on `next22`, 缺陷 #203: a pane holding a
/// pdf or a web page went blank behind the settings dialog).
///
/// A page is not drawn by this window — it is a DirectComposition visual
/// under the swapchain's — so when a modal takes it off the glass there is
/// nothing of it left anywhere except the photograph `web_thumb` keeps. Four
/// statements carry that photograph to the pane, and **breaking any one of
/// them is the blank pane verbatim**:
///
/// * `sync_web_page` is the one walk that knows all five reasons a page can
///   be off the glass, so it is where the page the modal *alone* took is
///   named;
/// * `keep_what_the_modal_covers` photographs every page on the glass — on
///   the clock, whether or not a card is looking — and asks for the one
///   decode a dialog needs, because a hidden WebView never answers a
///   capture and the ask therefore has to have happened *before*;
/// * `refresh_chrome` puts the frame down the chrome pass's own textured-quad
///   channel, which is under every overlay layer and therefore under the
///   scrim;
/// * `preview_float_layer` does the same for a page a float is carrying, on
///   that window's own layer.
///
/// The value half is the arithmetic that decides which page gets one: the
/// same predicate the presence answer is made of, asked a second time with
/// no modal standing over it.
///
/// RED GATE: drop the `icons.extend(self.page_keepsake_icons())` — which is
/// the build as it shipped — and the third assertion fails; the pane then
/// draws its own ground, which is exactly what the report is a photograph
/// of.
#[test]
fn a_page_a_modal_covers_is_drawn_as_a_kept_frame() {
    // ① Only the modal mints a keepsake. A page hidden for any of the other
    // four reasons has something else to draw, or nobody looking at it.
    assert!(
        !a_page_is_off_the_glass(false, false, true, false, false),
        "a page in the front tab with no card and no source face is on the \
             glass, and that is the one kind a dialog takes away"
    );
    assert!(a_page_is_off_the_glass(true, false, true, false, false));
    for (floated, in_front, carded, sourced) in [
        (false, false, false, false),
        (false, true, true, false),
        (false, true, false, true),
    ] {
        assert!(
            a_page_is_off_the_glass(false, floated, in_front, carded, sourced),
            "this page is off the glass with no dialog open at all, so the \
                 modal is not what took it and it is owed no standing-in frame"
        );
    }

    // ② The walk that names them, and the pass that keeps them.
    let sync = squeezed_body("Runtime", "sync_web_page");
    assert!(
        sync.contains("keepsakes.push(PageKeepsake{"),
        "the one walk that knows all five reasons is where the page a modal \
             alone took is named:\n{sync}"
    );
    assert!(
        sync.contains("self.keep_what_the_modal_covers(keepsakes,now);"),
        "and it hands them on:\n{sync}"
    );
    let keeping = squeezed_body("Runtime", "keep_what_the_modal_covers");
    assert!(
        keeping.contains("self.photograph_pages(demands,now);"),
        "every page on the glass is photographed on the clock — a hidden \
             WebView never answers, so the ask has to be older than the \
             dialog:\n{keeping}"
    );
    assert!(
        keeping.contains("self.window.web_thumbs.frame_job(keepsake.leaf)"),
        "and the dialog asks for the one decode it needs:\n{keeping}"
    );
    assert!(
        keeping.contains("self.window.web_thumbs.drop_frames();"),
        "and lets the pixels go when it comes down:\n{keeping}"
    );

    // ③ The two places a kept frame reaches the glass.
    let chrome = squeezed_body("Runtime", "refresh_chrome_with_overlay");
    assert!(
        chrome.contains("icons.extend(self.page_keepsake_icons());"),
        "a docked pane draws its page's last frame in the chrome pass, \
             under every overlay and therefore under the scrim:\n{chrome}"
    );
    let float = squeezed_body("Runtime", "preview_float_layer");
    assert!(
        float.contains("self.float_page_keepsake_icon(id)"),
        "and a page a float is carrying draws it on that window's own \
             layer, for the reason its picture already does:\n{float}"
    );
}

/// RED ④ — **a retirement asks for one frame, not one per turn** (§7.10 ④‴).
///
/// The freeze beside the blank page. A page whose pane has gone owes the
/// glass one frame, because the hole it was seen through was cut while a
/// frame was composed and the retirement happens after that frame. But
/// `WebSeat::close` is idempotent and the wait for the browser process runs
/// to ten seconds, so the set of orphaned pages stays non-empty for the
/// whole wait — and asking for that frame off the *set* rather than off the
/// *event* is a window that goes round its own loop as fast as it can for
/// ten seconds: the funnel requests a redraw whether or not it found a
/// picture, the redraw is answered at the tail of the same turn, and the
/// next turn asks again.
///
/// RED GATE: ask the question of `!orphaned.is_empty()` — the shipped build
/// — and the last assertion goes red. The three above it are the rule
/// itself: a page already told to go is not retiring again.
#[test]
fn a_retirement_asks_for_one_frame_and_not_one_per_turn() {
    assert!(
        !a_retirement_happens_on_this_turn(std::iter::empty()),
        "a window with nothing orphaned owes no frame at all"
    );
    assert!(
        a_retirement_happens_on_this_turn([false].into_iter()),
        "the turn a page is first found orphaned is the turn it retires on"
    );
    assert!(
        !a_retirement_happens_on_this_turn([true].into_iter()),
        "and every turn after it is a page that has already been told, \
             waiting for a process to exit — nothing on the glass is changing"
    );
    assert!(
        a_retirement_happens_on_this_turn([true, false].into_iter()),
        "a second page going while the first is still leaving owes its own frame"
    );

    let clock = squeezed_body("Runtime", "advance_web_page");
    assert!(
        clock.contains("ifretiring{self.present_chrome_change()?;}"),
        "the frame is owed by the retirement and not by the wait:\n{clock}"
    );
    assert!(
        !clock.contains("if!orphaned.is_empty(){"),
        "asking the wait for it is ten seconds of a window pinned at a core \
             with nothing on the glass changing:\n{clock}"
    );
}

/// RED — **this window's decoded pictures have a ceiling** (review row R1-8,
/// adversarial review 2026-09-08).
///
/// RED EVIDENCE (2026-09-08), before the budget:
///
/// ```text
/// eight thirty-two megabyte pictures into a 201326592 byte cache: it is holding 268436480
/// ```
///
/// `peek_cache` was a `HashMap` with one door in and one narrow door out —
/// [`forget_a_picture`], which is a *file watch* telling the window one named
/// file has changed. Nothing anywhere took an entry out because there were
/// too many of them, so every distinct picture a pointer had rested on since
/// the window opened was still decoded in it.
///
/// The entries here are the real ones: `PeekCacheEntry::Ready` holding real
/// `Arc<[u8]>`s, through the real budget, so what is being asserted is what
/// this window will actually hold.
///
/// MUTATION: build the cache with `u64::MAX` and the first assertion goes red
/// with everything ever inserted still in it.
#[test]
fn the_windows_decoded_pictures_are_bounded_and_the_oldest_goes_first() {
    const PICTURE_BYTES: usize = 32 * 1024 * 1024;
    let mut cache = PeekCache::with_budget(MAX_PEEK_CACHE_BYTES);
    for index in 0..8_u8 {
        cache.insert(
            format!(r"d:\shots\{index}.png"),
            PeekCacheEntry::Ready {
                key: format!("image:{index}"),
                rgba: Arc::from(vec![index; PICTURE_BYTES]),
                width_px: 2048,
                height_px: 4096,
                native_size: None,
            },
        );
    }
    assert!(
        cache.bytes_held() <= MAX_PEEK_CACHE_BYTES,
        "eight thirty-two megabyte pictures into a {} byte cache: it is holding {}",
        MAX_PEEK_CACHE_BYTES,
        cache.bytes_held(),
    );
    assert!(
        cache.get(r"d:\shots\7.png").is_some(),
        "the picture asked for last is the one it kept",
    );
    assert!(
        cache.get(r"d:\shots\0.png").is_none(),
        "and the one nothing has looked at since the window opened is gone",
    );
}

/// **The mutant**: the policy this fix replaces, in which the focused pane's
/// picture decided, alone, whether the window had anything to say.
fn focused_frame_alone(focused_frame_unchanged: bool, _unpainted_pane_output: bool) -> bool {
    focused_frame_unchanged
}

/// **The bug, as a count.** A pane that is not holding the keyboard says
/// something; it must be on the glass at the end of the same turn of the
/// loop, having cost exactly one present — the rhythm the focused pane gets.
///
/// The second turn is the one that matters: by then the focused pane's
/// picture is a byte-for-byte match of the one on the glass, so the
/// unchanged-frame gate — which only ever looks at that pane — says the
/// window has nothing to say while the other half of it has just scrolled.
#[test]
fn a_pane_that_is_not_the_keyboards_reaches_the_glass_on_the_turn_it_speaks() {
    let mut harness = TwoPaneHarness::new(24, 6);
    harness.turn(b"prompt\r\n", b"first\r\n", pty_drain_says_nothing_new);
    assert!(harness.sibling_shows("first"));
    let presents_before = harness.presents;

    harness.turn(b"", b"second\r\n", pty_drain_says_nothing_new);

    assert!(
        harness.sibling_shows("second"),
        "the pane beside the keyboard spoke and is on the glass in the same turn"
    );
    assert_eq!(
        harness.presents,
        presents_before + 1,
        "and it cost one present, not a wait for some unrelated event"
    );
}

/// **The mutation.** The identical script with the focused pane's frame as
/// the only judge — which is what this window shipped — and the sibling
/// stays on a picture it has already outgrown, for as long as nothing else
/// happens to publish. Measured on the real machine as 19 wheel notches out
/// of 20 producing no picture at all.
#[test]
fn the_focused_frame_alone_strands_the_other_pane_on_a_stale_picture() {
    let mut harness = TwoPaneHarness::new(24, 6);
    harness.turn(b"prompt\r\n", b"first\r\n", focused_frame_alone);
    assert!(harness.sibling_shows("first"));
    let presents_before = harness.presents;

    for _ in 0..20 {
        harness.turn(b"", b"second\r\n", focused_frame_alone);
    }

    assert!(
        !harness.sibling_shows("second"),
        "twenty turns of a shell talking, and none of it on the glass"
    );
    assert_eq!(
        harness.presents, presents_before,
        "because not one frame was ever published to draw it in"
    );
}

#[test]
fn a_late_zoom_reprint_keeps_the_last_formula_frame_until_exact_source_reanchors() {
    let start = Instant::now();
    let mut harness = PtyPresentationHarness::new(40, 24);
    harness
        .session
        .feed_at(b"intro\r\n$$x$$\r\nbarrier", start)
        .unwrap();
    assert_eq!(
        harness
            .session
            .advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
        1
    );
    let mut initial_task = harness.session.take_live_worker_task().unwrap();
    let initial_raster =
        render_live_detection_task(&MathEngine::new(), &mut initial_task, foreground_rgb())
            .expect("initial formula rasterizes");
    assert!(
        harness
            .session
            .complete_live_worker_result(initial_task, Ok(initial_raster))
    );
    assert!(harness.publish_pty_frame());
    assert!(harness.present_pending());
    assert_eq!(
        harness.last_presented.as_ref().unwrap().math_blocks.len(),
        1
    );

    // Match reconcile_authoritative_dpi: metrics, grid resize, then the new-DPI layout key.
    let zoom_at = start + Duration::from_millis(210);
    harness
        .session
        .set_cell_height_subpixels(NonZeroI64::new(14 * bt_viewport::SUBPIXELS_PER_PX).unwrap());
    harness
        .session
        .set_cell_width_subpixels(NonZeroI64::new(7 * bt_viewport::SUBPIXELS_PER_PX).unwrap());
    harness
        .session
        .set_ascii_baseline_subpixels(NonZeroI64::new(11 * bt_viewport::SUBPIXELS_PER_PX).unwrap());
    harness
        .session
        .resize_at(
            NonZeroU32::new(52).unwrap(),
            NonZeroU32::new(32).unwrap(),
            zoom_at,
        )
        .unwrap();
    harness.session.mark_pty_resize_requested_at(
        NonZeroU32::new(52).unwrap(),
        NonZeroU32::new(32).unwrap(),
        zoom_at,
    );
    harness.session.set_layout_key(bt_doc::LayoutKey {
        width_cells: NonZeroU32::new(52).unwrap(),
        dpi_milli: NonZeroU32::new(800).unwrap(),
        font_size_subpixels: 16 * 1024,
        font_rev: 1,
        theme_rev: harness.session.layout_key().theme_rev,
        lang_rev: harness.session.layout_key().lang_rev,
        profile_rev: harness.session.layout_key().profile_rev,
        line_wrapping: true,
    });
    let delayed_relayout = harness.session.take_live_worker_task().unwrap();
    assert!(harness.publish_pty_frame());
    assert!(harness.present_pending());
    assert!(
        harness
            .session
            .finish_resize_if_quiescent(zoom_at + Duration::from_millis(300))
            .unwrap()
    );

    let publications_before_gap = harness.publications;
    harness
        .session
        .feed_at(
            b"\x1b[2J\x1b[H\x1b[3Jintro\r\n$",
            zoom_at + Duration::from_millis(310),
        )
        .unwrap();
    assert!(
        !harness.publish_pty_frame(),
        "publish_frame_inner must skip the diagnosed incomplete reprint frame"
    );
    assert!(!harness.present_pending());
    assert_eq!(harness.publications, publications_before_gap);
    assert_eq!(
        harness.last_presented.as_ref().unwrap().math_blocks.len(),
        1,
        "the swapchain remains on the last complete formula frame"
    );

    harness
        .session
        .feed_at(b"$x$$\r\nbarrier", zoom_at + Duration::from_millis(324))
        .unwrap();
    let reanchor_published = harness.publish_pty_frame();
    assert!(
        !harness.projection.presentation_hold(),
        "exact-source re-anchor releases the hold immediately"
    );
    if reanchor_published {
        assert!(harness.present_pending());
    }
    assert_eq!(
        harness.last_presented.as_ref().unwrap().math_blocks.len(),
        1
    );

    // The re-anchored stale frame may be content-identical to last_presented and therefore need
    // no publication. Either way, no incomplete grid frame entered the presentation slot.
    drop(delayed_relayout);
}

#[test]
fn keyboard_mapping_carries_the_layouts_letters_and_preserves_terminal_controls() {
    assert_eq!(
        input::legacy_bytes(
            &Key::Character("hello".into()),
            ModifiersState::empty(),
            false
        ),
        Some(b"hello".to_vec())
    );
    assert_eq!(
        input::legacy_bytes(&Key::Named(NamedKey::Enter), ModifiersState::empty(), false),
        Some(vec![b'\r'])
    );
    assert_eq!(
        input::legacy_bytes(
            &Key::Named(NamedKey::Backspace),
            ModifiersState::empty(),
            false
        ),
        Some(vec![0x7f])
    );
    assert_eq!(
        input::legacy_bytes(&Key::Named(NamedKey::Space), ModifiersState::empty(), false),
        Some(vec![b' '])
    );
    assert_eq!(
        input::legacy_bytes(&Key::Character("c".into()), ModifiersState::CONTROL, false),
        Some(vec![0x03])
    );
    // Every bare `Ctrl+letter` is the shell's and is sent as its control
    // code (2026-08-17: `^X` used to be swallowed here, and with it `^B`,
    // `^L`, `^R` — the whole readline alphabet the shortcut table promised
    // to leave alone).
    assert_eq!(
        input::legacy_bytes(&Key::Character("x".into()), ModifiersState::CONTROL, false),
        Some(vec![0x18])
    );
    // **And a character outside ASCII is bytes like any other** (M1-7, X-3
    // §4 ⑤). This case asserted `None` until 2026-09-12, and what it was
    // really encoding was a guard (`text.is_ascii()`) that was supposed to
    // stop a *composed* character being typed twice. It did not need to: a
    // composition never arrives as a key event on either platform — winit
    // drops an IME's `WM_CHAR` here because no key event stands under it, and
    // on macOS a commit arrives as `Ime::Commit` with no `KeyboardInput` at
    // all (X-3 measured `你好` once, six bytes). What the guard did instead
    // was swallow `ü ä ö ß` on a German layout, on both platforms, whole.
    assert_eq!(
        input::legacy_bytes(&Key::Character("中".into()), ModifiersState::empty(), false),
        Some("中".as_bytes().to_vec())
    );
    assert_eq!(
        input::legacy_bytes(&Key::Character("ü".into()), ModifiersState::empty(), false),
        Some("ü".as_bytes().to_vec())
    );
    assert_eq!(
        input::legacy_bytes(
            &Key::Named(NamedKey::Process),
            ModifiersState::CONTROL,
            false
        ),
        None
    );
}

#[test]
fn bracketed_paste_follows_vendor_decset_and_normalizes_crlf() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(2).unwrap());

    session.feed(b"\x1b[?2004h").unwrap();
    assert_eq!(
        input::paste_bytes("one\r\ntwo\n", session.bracketed_paste_mode()),
        b"\x1b[200~one\rtwo\r\x1b[201~"
    );

    session.feed(b"\x1b[?2004l").unwrap();
    assert_eq!(
        input::paste_bytes("one\r\ntwo\n", session.bracketed_paste_mode()),
        b"one\rtwo\r"
    );
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

#[test]
fn copy_on_select_writes_nonempty_text_and_keeps_the_selection() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"drag me").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let selection = ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 6, Bias::After).unwrap().unwrap(),
    };
    session.set_view_selection(Some(selection));
    let mut clipboard = String::new();

    assert!(write_selection_text(&session, true, |text| {
        clipboard.push_str(text);
        Ok(())
    }));
    assert_eq!(clipboard, "drag me");
    assert_eq!(session.selection_text().as_deref(), Some("drag me"));
}

#[test]
fn copy_on_select_does_not_touch_the_clipboard_for_an_empty_selection() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"click").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let anchor = frame.anchor_at(0, 2, Bias::Before).unwrap().unwrap();
    session.set_view_selection(Some(ViewSelection {
        start: anchor.clone(),
        end: anchor,
    }));
    let mut writes = 0;

    assert!(!write_selection_text(&session, true, |_| {
        writes += 1;
        Ok(())
    }));
    assert_eq!(writes, 0);
}

#[test]
fn unavailable_clipboard_during_copy_on_select_keeps_the_selection() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"retry me").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    session.set_view_selection(Some(ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 7, Bias::After).unwrap().unwrap(),
    }));

    assert!(!write_selection_text(&session, true, |_| {
        Err(anyhow!("injected clipboard owner contention"))
    }));
    assert_eq!(session.selection_text().as_deref(), Some("retry me"));
}

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

#[test]
fn wheel_flush_at_the_top_clamp_publishes_no_frame() {
    let mut pane = wheel_pane_at_top();
    let revision = pane.content_revision;
    assert!(!flush_test_wheel(&mut pane, 1.0));
    assert_eq!(pane.publications, 0);
    assert_eq!(pane.content_revision, revision);
    assert!(!pane.present_pending());
    // The former unconditional wheel publish fails the zero-frame rule.
    assert!(pane.publish_expose_frame());
    assert_eq!(pane.publications, 1);
}

#[test]
fn disconnected_math_dispatch_downgrades_once_and_leaves_the_real_session_usable() {
    let start = Instant::now();
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(40).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed_at(b"$$x$$\x1b[?25l", start).unwrap();
    assert_eq!(
        session.advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
        1
    );
    let (tasks, receiver) = mpsc::channel();
    drop(receiver);
    let (scale_tasks, _scale_receiver) = mpsc::channel();
    let (path_tasks, _path_receiver) = mpsc::channel();
    let mut running = true;
    let mut notice_pending = false;

    assert!(dispatch_pending_math_tasks(
        probe_leaf(),
        &mut session,
        &tasks,
        &scale_tasks,
        &path_tasks,
        &mut running,
        &mut notice_pending,
    ));
    assert!(!running);
    assert!(notice_pending);
    session.feed(b"\r\nterminal-still-running").unwrap();
    assert!(
        session
            .terminal()
            .visible_text()
            .iter()
            .any(|row| row.contains("terminal-still-running"))
    );

    assert_eq!(
        take_math_worker_notice(&mut notice_pending),
        Some(math_worker_stopped_notice())
    );
    assert!(!notice_pending);
    assert_eq!(take_math_worker_notice(&mut notice_pending), None);
    assert!(!dispatch_pending_math_tasks(
        probe_leaf(),
        &mut session,
        &tasks,
        &scale_tasks,
        &path_tasks,
        &mut running,
        &mut notice_pending,
    ));
    assert!(
        !notice_pending,
        "the user-visible downgrade notice is one-shot"
    );
}

/// RED: a divider storm used to put every intermediate Lanczos3 request on the one FIFO.
/// The worker must execute only the newest size for one content/purpose, while preserving a
/// different purpose as an independent question.
#[test]
fn scale_worker_drag_storm_discards_superseded_work_and_completes_the_latest() {
    let (sender, receiver) = mpsc::channel();
    for width in 1..=128 {
        sender
            .send(ScaleWorkerRequest::Preview {
                leaf: probe_leaf(),
                task: scale_task("same-path", width),
            })
            .unwrap();
    }
    sender
        .send(ScaleWorkerRequest::Peek {
            leaf: probe_leaf(),
            task: scale_task("same-path", 17),
        })
        .unwrap();
    drop(sender);

    let mut executed = Vec::new();
    run_scale_worker(receiver, |request| {
        executed.push((request.purpose(), request.task().display_width_px));
    });

    assert_eq!(
        executed,
        vec![(ScalePurpose::Preview, 128), (ScalePurpose::Peek, 17)]
    );
}

/// The whole point of zooming about the pointer: whatever pixel of the
/// picture the hand was on stays under the hand.
#[test]
fn zooming_about_a_point_leaves_that_point_where_it_was() {
    // Read off `image_destination`'s own arithmetic: a picture point `u`
    // from the picture's centre lands at `centre + pan + u * scale`.
    let anchor_at =
        |scale: f32, pan: [f32; 2], u: [f32; 2]| [pan[0] + u[0] * scale, pan[1] + u[1] * scale];
    let (old, new) = (0.4_f32, 1.7_f32);
    let pan = [37.0, -12.0];
    // The pointer, measured from the body's centre.
    let point = [180.0, -95.0];
    let u = [(point[0] - pan[0]) / old, (point[1] - pan[1]) / old];
    assert_eq!(anchor_at(old, pan, u), point);

    let moved = zoom_about(point, old, new, pan);
    let after = anchor_at(new, moved, u);
    assert_close(after[0], point[0], "the anchor did not move sideways");
    assert_close(after[1], point[1], "nor down");

    assert_eq!(
        zoom_about(point, old, old, pan),
        pan,
        "and a zoom that changes nothing moves nothing"
    );
}

/// One seat, named the way a card aim names one.
fn aim_seat(tab: u64, seat: u64) -> LeafId {
    LeafId {
        tab: TabId(tab),
        seat: SeatId(seat),
    }
}

/// A driver that reports travel rather than detents, `y` pixels of it.
fn wheel_pixels(y: f64) -> MouseScrollDelta {
    MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, y))
}

/// **A sixth of a detent, six times, is one row** (user report 2026-08-21:
/// "turning up works, but I have to turn for ages before it moves").
///
/// A high-resolution wheel and a precision touchpad both report one detent
/// as a run of small travels — 20 pixels at a time against Win32's 120 —
/// and each of those on its own rounds to no rows at all. Rounding each
/// event in isolation throws the whole turn away, which is what the report
/// is describing; the fraction is *carried* instead, and the row falls the
/// moment the sixth nudge completes the detent.
#[test]
fn six_sixths_of_a_detent_add_up_to_one_row() {
    let mut aim = None;
    let seat = aim_seat(1, 1);
    let steps: Vec<i32> = (0..6)
        .map(|_| CardAim::spend(&mut aim, seat, wheel_pixels(20.0)))
        .collect();
    assert_eq!(
        steps,
        vec![0, 0, 0, 0, 0, 1],
        "a detent delivered in six pieces moves exactly one row, on the piece that completes it"
    );
}

/// The standard mouse is not made slower by the carry: a detent that arrives
/// whole is one row on arrival, and a merged burst is worth its own count.
#[test]
fn a_whole_detent_is_one_row_the_moment_it_lands() {
    let mut aim = None;
    let seat = aim_seat(1, 1);
    assert_eq!(
        CardAim::spend(&mut aim, seat, MouseScrollDelta::LineDelta(0.0, 1.0)),
        1
    );
    assert_eq!(
        CardAim::spend(&mut aim, seat, MouseScrollDelta::LineDelta(0.0, 1.0)),
        1
    );
    assert_eq!(
        CardAim::spend(&mut aim, seat, MouseScrollDelta::LineDelta(0.0, 3.0)),
        3,
        "a flick merged into one burst is worth every detent in it"
    );
    assert_eq!(
        CardAim::spend(&mut aim, seat, wheel_pixels(120.0)),
        1,
        "and a driver reporting travel says the same thing in its own currency"
    );
}

/// **A hand that changes its mind does not get the fraction it was owed.**
///
/// Half a detent upward is a promise about *up*; spending it on the way down
/// would make the first row down arrive early or late depending on something
/// the reader did before and cannot see.
#[test]
fn turning_the_other_way_forgets_the_fraction_it_was_owed() {
    let mut aim = None;
    let seat = aim_seat(1, 1);
    assert_eq!(CardAim::spend(&mut aim, seat, wheel_pixels(60.0)), 0);
    assert_eq!(
        CardAim::spend(&mut aim, seat, wheel_pixels(-120.0)),
        -1,
        "a whole detent down is one row down, not half of one"
    );
    assert_eq!(CardAim::spend(&mut aim, seat, wheel_pixels(-60.0)), 0);
    assert_eq!(
        CardAim::spend(&mut aim, seat, wheel_pixels(-60.0)),
        -1,
        "and the downward halves add up among themselves"
    );
}

/// **The bare wheel is the column's, whatever else is held down** (user
/// ruling 2026-08-21).
///
/// The list this window puts under the pointer is the thing with content out
/// of sight, so the gesture every other list in the product answers is the
/// one this one answers too. `Shift` is named here on purpose: it is spent
/// twice already on the wheel — the preview body's other axis, and
/// [`wheel_route`]'s "this notch is mine, not the program's" — so a reader
/// holding it over the column gets the list, not a third meaning.
#[test]
fn a_bare_notch_over_the_column_is_the_lists() {
    for held in [
        ModifiersState::empty(),
        ModifiersState::SHIFT,
        ModifiersState::CONTROL,
        ModifiersState::CONTROL.union(ModifiersState::SHIFT),
        ModifiersState::SUPER,
    ] {
        assert_eq!(
            column_notch(held),
            ColumnNotch::List,
            "{held:?} over the card column has to scroll the card column"
        );
    }
}

/// **The fraction belongs to the seat it was turned at**, not to the window.
///
/// Moving the pointer to another seat — or to another card — starts that
/// seat's aim from nothing, because a carry that followed the pointer would
/// move a window the reader never turned the wheel over.
#[test]
fn the_fraction_belongs_to_the_seat_it_was_turned_at() {
    let mut aim = None;
    let half = wheel_pixels(60.0);
    assert_eq!(CardAim::spend(&mut aim, aim_seat(1, 1), half), 0);
    assert_eq!(
        CardAim::spend(&mut aim, aim_seat(1, 2), half),
        0,
        "the seat next door did not inherit the half detent"
    );
    assert_eq!(CardAim::spend(&mut aim, aim_seat(1, 2), half), 1);
    assert_eq!(
        CardAim::spend(&mut aim, aim_seat(2, 1), half),
        0,
        "and neither did the same-numbered seat on another card"
    );
    assert_eq!(CardAim::spend(&mut aim, aim_seat(2, 1), half), 1);
}

/// RED — **a 24 megapixel photograph is resampled on the decode worker, and
/// the window thread never touches it** (owner's ruling 2026-09-12).
///
/// The reduction is a full Lanczos3 pass over the native decode — tenths of
/// a second on a photograph, which is the measurement §7.1.3j (e) was
/// written about. On the thread that answers the keyboard that is a window
/// that has stopped answering it, and the hang watchdog would not even catch
/// it: that watchdog times one turn, and this would be one turn.
///
/// So the pin is structural, because the cost is structural. `bt_term` puts
/// the resample inside the file lane itself rather than owing it back as an
/// errand, and this window builds exactly one picture decoder — inside the
/// decoration worker's closure.
///
/// MUTATION: build a second one anywhere outside that closure, or lift the
/// one there is out of it, and the count or the bounds go red.
#[test]
fn the_reduction_happens_on_the_worker_not_the_window_thread() {
    // The whole of `main.rs`, and not a production half of it. Until 2026-09-18
    // the count was taken over the text above `mod tests`, because a fixture in
    // that module builds a decoder of its own on a thread that is nobody's
    // window — and that module is this file now, so the second decoder is no
    // longer in `main.rs` and there is no half left to cut. Counting the whole
    // of it is the stricter reading: a decoder built anywhere in that file, in
    // its product or in any of the fifty-four test modules it kept, is named
    // here. The needle is still spelled in two pieces, which now costs nothing.
    let decoder = concat!("InlineImage", "Decoder");
    assert_eq!(
        in_product(&found(
            needle!(Pattern::path("InlineImageDecoder::default")),
            View::Identifiers,
        )),
        1,
        "this window builds more than one picture decoder"
    );
    for (what, needle) in [
        (
            "the decoder",
            format!("let mut image_decoder = {decoder}::default();"),
        ),
        (
            "the inline lane",
            "image_decoder.decode(task.clone())".to_owned(),
        ),
        (
            "the picture lane",
            "peek_pixels(&mut image_decoder, &path)".to_owned(),
        ),
    ] {
        assert!(
            free_fn_body("run_decoration_worker").contains(&needle),
            "{what} stands outside the decoration worker's own body"
        );
    }
    // And the pass itself is made where the decode is, rather than handed
    // back to whoever asked: an answer that came back as an errand would be
    // an errand for the thread that asked for it.
    let lane = package_item_body("bt-term", &ItemQuery::function("decode_local_image_bytes"));
    assert!(
        lane.contains("scale_inline_image(&InlineImageScaleTask {"),
        "the reduction is not made where the decode is:\n{lane}"
    );
    assert_eq!(
        found_in_package(
            "bt-term",
            needle!(Pattern::call("decode_local_image_bytes")),
            View::Identifiers,
            Scope::Module("crate::inline_image".to_owned()),
        )
        .len(),
        2,
        "and that lane is declared once and called once — from the file read, \
             which is the worker's own"
    );
}

/// A cap that bites by a hair is a half-second spent making the picture worse
/// (user ruling 2026-08-25; [`PREVIEW_IMAGE_TEXTURE_SLACK`]).
#[test]
fn a_decode_inside_the_slack_keeps_its_own_pixels_rather_than_paying_for_a_pass() {
    let allowance =
        bt_viewport::MATH_TEXTURE_CACHE_BUDGET_BYTES as f64 * PREVIEW_IMAGE_TEXTURE_SHARE;
    let bytes = |image: [u32; 2]| f64::from(image[0]) * f64::from(image[1]) * 4.0;

    // 33.2 MiB against a 32 MiB share: over it, and inside the slack.
    let barely = [3000_u32, 2900];
    assert!(bytes(barely) > allowance, "the share really is crossed");
    assert!(
        bytes(barely) <= allowance * PREVIEW_IMAGE_TEXTURE_SLACK,
        "and crossed by less than the slack"
    );
    assert_eq!(
        image_raster_cap(barely),
        (barely[0], barely[1]),
        "so 100% is the file's own pixels and no pass is run at all"
    );

    // 39.1 MiB: past the slack, so the pass is worth what it costs.
    let past = [3200_u32, 3200];
    assert!(
        bytes(past) > allowance * PREVIEW_IMAGE_TEXTURE_SLACK,
        "this one is past the slack"
    );
    let capped = image_raster_cap(past);
    assert_ne!(capped, (past[0], past[1]), "and is capped");
    assert!(
        f64::from(capped.0) * f64::from(capped.1) * 4.0 <= allowance,
        "down to the share itself and not merely to the slack — once the pass \
             is paid for there is no reason to stop short of it"
    );

    // The slack admits at most 0.6 of the whole budget, which is the number
    // `ByteLru` would refuse a texture over.
    assert!(
        allowance * PREVIEW_IMAGE_TEXTURE_SLACK
            < bt_viewport::MATH_TEXTURE_CACHE_BUDGET_BYTES as f64,
        "nothing the slack admits can be refused outright by the shared cache"
    );
}

/// A scale worker may spend arbitrarily long inside Lanczos3; validation still reaches the
/// independent decoration receiver instead of sitting behind that raster in one FIFO.
#[test]
fn local_path_validation_and_resampling_are_dispatched_to_independent_lanes() {
    let (tasks, task_receiver) = mpsc::channel();
    let (scale_tasks, scale_receiver) = mpsc::channel();
    // The third lane, since audit 3 C-2: a path question is not the decoration queue's.
    let (path_tasks, path_receiver) = mpsc::channel();
    assert!(dispatch_decoration_task(
        probe_leaf(),
        SessionDecorationTask::ScaleInlineImage(scale_task("same-path", 128)),
        &tasks,
        &scale_tasks,
        &path_tasks,
    ));
    assert!(dispatch_decoration_task(
        probe_leaf(),
        SessionDecorationTask::InlineImage(bt_term::InlineImageTask {
            occurrence_id: 7,
            source: bt_term::InlineImageSource::LocalPath(PathBuf::from("same-path.png")),
        }),
        &tasks,
        &scale_tasks,
        &path_tasks,
    ));

    assert!(matches!(
        scale_receiver.try_recv(),
        Ok(ScaleWorkerRequest::InlineImage { .. })
    ));
    assert!(matches!(
        task_receiver.try_recv(),
        Ok(MathWorkerRequest::InlineImage {
            task: bt_term::InlineImageTask {
                occurrence_id: 7,
                ..
            },
            ..
        })
    ));

    // **And a path question goes down a third lane** (audit 3 C-2). It rode the decoration queue
    // until a printed name turned out to be able to name a mapped drive whose server is gone: a
    // `GetFileAttributesW` inside the SMB redirector takes about twenty-one seconds to give up,
    // and twenty-one seconds at the head of *this* queue is every formula and every picture in
    // the window waiting behind one hostile line of output. Nobody waits on the path lane — an
    // unanswered name is simply not a link yet — so it is the one that may be slow.
    assert!(dispatch_decoration_task(
        probe_leaf(),
        SessionDecorationTask::VerifyPath(PathBuf::from(r"Z:\work\notes.md")),
        &tasks,
        &scale_tasks,
        &path_tasks,
    ));
    assert!(matches!(
        path_receiver.try_recv(),
        Ok(PathWorkerRequest { .. })
    ));
    assert!(
        task_receiver.try_recv().is_err(),
        "a path question must not be able to stand in front of a formula"
    );
}

// ── the taskbar's state, asked on its own lane (0.4.5 ticket 62) ──────────
//
// `Runtime` cannot be built without a window, so the turn's taskbar call is run
// through the real lane (`taskbar_lane::TaskbarLane::observe`, the call
// `sample_window_place` makes) and the call sites are held through `bt_source`.

/// RED (62) — **A turn asks the shell nothing about the taskbar.**
///
/// The owner's stall on next93 held the window thread 535 ms with two
/// `taskbar_is_auto_hidden` asks of 99 ms and 92 ms in it: `sample_window_place`
/// put the question to Explorer at every turn's head and again for a delivery
/// between turns. What a turn does about the taskbar is now
/// `TaskbarLane::observe`; this drives it through a thousand turns, a tenth of
/// the refresh interval apart, on this thread, while the lane asks the real
/// shell on its own. The shell is asked — the first answer lands and wakes the
/// loop — and never by the thread that turns. The call sites are the source
/// half: the platform question has one caller in `bt-app`,
/// `taskbar_lane::ask_the_shell`, which is handed only to the product's lane,
/// and `sample_window_place` reads the lane.
///
/// MUTATION: restore the inline call — `bt_platform::taskbar_is_auto_hidden()`
/// in `sample_window_place`, or `ask_the_shell()` in `TaskbarLane::observe` —
/// red.
#[test]
fn a_turn_asks_the_shell_nothing_about_the_taskbar() {
    let (lane, wakes) = taskbar_lane::tests::lane(taskbar_lane::ask_the_shell);
    let start = Instant::now();
    let asked_before = taskbar_lane::shell_asks();
    for turn in 0..1000_u32 {
        let _ = lane.observe(start + taskbar_lane::REFRESH_INTERVAL / 10 * turn, true);
        if turn == 0 {
            wakes
                .recv_timeout(Duration::from_secs(30))
                .expect("the lane's first answer wakes the loop");
        }
    }
    assert!(
        lane.reading().answered(),
        "the shell was asked, on the lane"
    );
    assert_eq!(
        taskbar_lane::shell_asks(),
        asked_before,
        "a thousand turns put the question to the shell on the thread that turns them"
    );

    let asks = source()
        .search(&Search::new(
            needle!(Pattern::call("taskbar_is_auto_hidden")),
            View::Identifiers,
        ))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .in_the_product(source());
    assert_eq!(
        reader_names(&asks),
        vec!["ask_the_shell".to_owned()],
        "{}",
        asks.report(source())
    );
    let handed = source()
        .search(
            &Search::new(
                needle!(Pattern::identifier("ask_the_shell")),
                View::Identifiers,
            )
            .exempting_declarations_of(ItemQuery::function("ask_the_shell")),
        )
        .unwrap_or_else(|failure| panic!("{failure}"))
        .in_the_product(source());
    assert_eq!(
        reader_names(&handed),
        vec!["product_lane".to_owned()],
        "{}",
        handed.report(source())
    );
    let sampled = free_fn_body("sample_window_place");
    assert!(sampled.contains("taskbar_lane::observe("));
}

/// A covered window's delivery about one pane, as the door would have decided it
/// on `taskbar`.
fn delivery_on(taskbar: bt_platform::TaskbarReading) -> AttentionDelivery {
    AttentionDelivery {
        tab: TabId(7),
        seat: SeatId(3),
        reach: notify::desktop_reach(false, covered_window_on(taskbar)),
        why: attention::Why::Awaiting,
        title: "pwsh".to_owned(),
        body: None,
    }
}

/// On a screen, unfocused, with something on top of it — the window a flash is
/// for.
fn covered_window_on(taskbar: bt_platform::TaskbarReading) -> notify::WindowPlace {
    notify::WindowPlace {
        focused: false,
        hidden: false,
        exposed: false,
        taskbar_is_auto_hidden: taskbar.auto_hidden,
    }
}

/// RED (62) — **A notification placed before the first answer lands uses the
/// default and is re-placed when the answer says auto-hidden.**
///
/// Until the taskbar lane's first answer the reading is "not hidden", so a
/// covered window's request flashes the taskbar button. On a desktop whose bar
/// hides itself that flash slides the whole bar out and keeps it out — the very
/// thing the 2026-08-28 ruling took the flash off such a desktop for — so the
/// flash is kept with the number of the reading it was decided on, and the
/// lane's answer re-places it: decided again on a reading that carries the
/// answer, the request goes to the desktop. An answer that says the bar is on
/// screen leaves the flash running, and a pane that has gone is not called to.
/// The answers here are real lane answers; the runtime's half — the lane's wake
/// reaches every window, a flash is recorded where it is raised and forgotten
/// where the window comes to the front — is held through `bt_source`.
///
/// MUTATION: make `TaskbarFlash::contradicted_by` answer `false` (no re-place)
/// — red.
#[test]
fn a_notification_placed_before_the_first_answer_uses_the_default_and_is_re_placed_when_the_answer_says_auto_hidden()
 {
    let (hides, woke) = taskbar_lane::tests::lane(|| true);
    let default = hides.reading();
    assert!(!default.answered());
    let placed = delivery_on(default);
    assert_eq!(
        placed.reach,
        attention::Reach::Flash,
        "the default: the bar is on screen"
    );
    assert_eq!(
        notify::interruption(placed.reach, true),
        notify::Interruption::FlashTheTaskbarButton
    );
    let mut flash = None;
    TaskbarFlash::record(&mut flash, default.generation, vec![placed]);
    let flash = flash.expect("the flash is kept with its reading");
    assert!(!flash.contradicted_by(default));

    hides.request(Instant::now());
    woke.recv_timeout(Duration::from_secs(30))
        .expect("the first answer wakes the loop");
    let answer = hides.reading();
    assert!(answer.answered() && answer.auto_hidden);
    assert!(
        flash.contradicted_by(answer),
        "an answer that says the bar hides itself re-places the flash"
    );
    let standing =
        |tab: TabId, seat: SeatId| (tab == TabId(7) && seat == SeatId(3)).then_some(false);
    let replaced = flash.replaced(covered_window_on(answer), standing);
    assert_eq!(replaced.len(), 1);
    assert_eq!(replaced[0].reach, attention::Reach::Toast);
    assert_eq!(
        notify::interruption(replaced[0].reach, true),
        notify::Interruption::PutItOnTheDesktop,
        "re-placed on the desktop"
    );

    // The answer that confirms the bar is on screen leaves the flash where it is.
    let (shows, woke) = taskbar_lane::tests::lane(|| false);
    let mut flash = None;
    TaskbarFlash::record(&mut flash, 0, vec![delivery_on(shows.reading())]);
    let flash = flash.expect("kept");
    shows.request(Instant::now());
    woke.recv_timeout(Duration::from_secs(30))
        .expect("the first answer wakes the loop");
    assert!(!flash.contradicted_by(shows.reading()));
    // And a pane that has gone meanwhile is called to by nobody.
    assert!(
        flash
            .replaced(covered_window_on(answer), |_, _| None)
            .is_empty()
    );

    // The runtime's half.
    let events =
        item_body(&ItemQuery::method("FolioApp", "user_event").of_trait("ApplicationHandler"));
    let arm = events
        .find("AppEvent::TaskbarAnswered =>")
        .map(|at| &events[at..])
        .expect("the lane's wake has an arm");
    let arm = &arm[..arm[1..].find("AppEvent::").map_or(arm.len(), |at| at + 1)];
    assert!(arm.contains("replace_contradicted_flash()"), "{arm}");
    let raise = method_body("Runtime", "raise_attention");
    assert!(raise.contains("TaskbarFlash::record("));
    assert!(raise.contains("self.window.taskbar_reading.generation"));
    let windows =
        item_body(&ItemQuery::method("FolioApp", "window_event").of_trait("ApplicationHandler"));
    let focused = windows
        .find("WindowEvent::Focused(true) => {")
        .map(|at| &windows[at..])
        .expect("the window's focus has an arm");
    let focused = &focused[..focused[1..]
        .find("WindowEvent::")
        .map_or(focused.len(), |at| at + 1)];
    assert!(
        focused.contains("taskbar_flash = None"),
        "the record ends where the flash does"
    );
}

/// RED (49) — **A title that did not change is never written to the OS again.**
///
/// A shell that sets the title on every prompt (OSC 0/2) makes the drain say
/// "the chrome changed" on every turn, and until ticket 49 every one of those
/// turns called `Window::set_title` with the same string — a message to the
/// taskbar measured waiting up to 2.2 s. A hundred turns a whole frame apart,
/// each wanting the same title: one write.
///
/// MUTATION: write on every offer (drop the equality check in
/// `pace::LatestThrottle::offer`) — a hundred writes.
#[test]
fn a_title_that_did_not_change_is_never_written_to_the_os_again() {
    let start = Instant::now();
    let mut slot = TitleSlot::default();
    let mut writes = Vec::new();
    for turn in 0..100_u32 {
        slot.want("pwsh — ~/src".to_owned());
        if let Some(title) = slot.take_due(TITLE_FRAME, start + TITLE_FRAME * turn) {
            writes.push(title);
        }
    }
    assert_eq!(writes, vec!["pwsh — ~/src".to_owned()]);
    assert_eq!(
        slot.deadline(),
        None,
        "nothing is held, so nothing wakes the loop"
    );
}

/// RED (49) — **A title that changes every turn reaches the OS at most once per
/// frame interval, and the last one always arrives.**
///
/// A program that animates its title changes it faster than any taskbar can
/// show. Ten turns a quarter of a frame apart, each with a new title: the OS
/// hears at most one per frame, and once the held title's deadline comes — the
/// wake the turn folds in — it holds the tenth.
///
/// MUTATION: drop the pending value when the interval has not passed (no
/// trailing write) in `pace::LatestThrottle::offer` — the OS is left holding
/// the ninth.
#[test]
fn a_title_that_changes_every_turn_reaches_the_os_at_most_once_a_frame_and_the_last_one_arrives() {
    let start = Instant::now();
    let quarter = TITLE_FRAME / 4;
    let mut slot = TitleSlot::default();
    let mut os_holds: Option<String> = None;
    let mut writes = 0_u32;
    let mut now = start;
    for turn in 0..10_u32 {
        now = start + quarter * turn;
        slot.want(format!("building {turn}/10"));
        if let Some(title) = slot.take_due(TITLE_FRAME, now) {
            writes += 1;
            os_holds = Some(title);
        }
    }
    // The turns after the last change want nothing new; the one the deadline
    // books writes what was held.
    let due = slot.deadline().expect("the tenth title is held and booked");
    assert!(
        due > now && due <= now + TITLE_FRAME,
        "held for at most one frame"
    );
    assert_eq!(
        slot.take_due(TITLE_FRAME, due - Duration::from_millis(1)),
        None
    );
    if let Some(title) = slot.take_due(TITLE_FRAME, due) {
        writes += 1;
        os_holds = Some(title);
    }
    assert!(
        writes <= 10_u32.div_ceil(4) + 1,
        "{writes} writes for ten titles"
    );
    assert_eq!(os_holds.as_deref(), Some("building 9/10"));
    assert_eq!(slot.deadline(), None);
}

/// RED (49) — **Only one function in bt-app calls Window::set_title.**
///
/// Five roads used to write the title straight to the OS — the drain, a tab
/// switch, a finished rename, a released synchronized update and a window's
/// birth — and a sixth written the same way would bring back the unthrottled
/// write this ticket removed. Read through `bt_source`, product files only.
///
/// **Since A1d the one function reaches it through its owner-thread door**: the winit call is
/// `owner_door::set_title`'s body, and that door is called from `flush_title` alone. The needle
/// reads three: the door's own name where it is declared, its winit call, and `flush_title`'s
/// call of it.
///
/// MUTATION: put a direct `self.window.window.set_title(&self.display_title())`
/// back in `Runtime::activate_tab` — red.
#[test]
fn only_one_function_in_bt_app_calls_window_set_title() {
    let calls =
        found(needle!(Pattern::call("set_title")), View::Identifiers).in_the_product(source());
    let mut readers = reader_names(&calls);
    readers.sort();
    assert_eq!(
        readers,
        vec!["flush_title".to_owned(), "set_title".to_owned()],
        "{}",
        calls.report(source())
    );
    assert_eq!(calls.len(), 3, "{}", calls.report(source()));
    assert_eq!(calls.outside_items(source()), 0);
}

#[test]
fn startup_polls_pty_until_the_first_text_frame_is_presented() {
    assert_eq!(startup_poll_delay(false), Some(STARTUP_PTY_POLL_INTERVAL));
    assert_eq!(startup_poll_delay(true), None);
}

#[test]
fn startup_trace_title_is_human_readable_without_console_output() {
    assert_eq!(startup_scale_title(1.5), "Folio M0-beta · 1.5x");
    assert_eq!(
        startup_trace_title(
            Duration::from_millis(682),
            Duration::from_millis(1089),
            1.25,
        ),
        "Folio M0-beta — bg 682ms · text 1089ms · 1.25x"
    );
}

/// PIN — every surface that says the product's name out loud says the same
/// name, and none of them still says the old one.
///
/// A rename is not one edit, it is seven, spread over five modules and two
/// shell scripts, and six of the seven are sentences that *embed* the name
/// rather than reading it: `&'static str` constants cannot interpolate
/// [`APP_NAME`], because `concat!` takes literals and nothing else. So the
/// thing that makes them one decision is this test rather than the type
/// system, and it is written as both halves on purpose — the positive one
/// (each surface names the product) would pass a sentence that named it
/// twice, once under each brand.
///
/// The old spelling is checked for by name because that is what a half-done
/// rename leaves behind, and because there is no other way to state "and
/// nothing here still says the thing we stopped saying".
///
/// Red gate: rename any one of these and leave the others, in either
/// direction, and this names the surface that moved. It is what the
/// BetterTerminal → Folio rename was carried out under.
#[test]
fn every_surface_that_names_the_product_says_the_same_name() {
    let banner = banner_line("something failed to start");
    let surfaces: [(&str, &str); 7] = [
        ("the window title bar", APP_NAME),
        ("the startup trace title", WINDOW_TITLE),
        (
            "a pane this build cannot draw",
            seats::placeholder_seat_notice(),
        ),
        (
            "the settings dialog's startup row",
            settings::SettingsRow::DefaultProfile
                .literal_description(&settings::SettingsValues::sample())
                .expect("the startup row's sentence is a literal"),
        ),
        ("the restore prompt", restore::sub_text()),
        ("this terminal's own banner", &banner),
        (
            "the TERM_PROGRAM this terminal declares",
            bt_pty::TERM_PROGRAM,
        ),
    ];
    for (surface, text) in surfaces {
        assert!(
            text.contains(APP_NAME),
            "{surface} must name the product ({APP_NAME:?}): {text:?}"
        );
        assert!(
            !text.contains("BetterTerminal"),
            "{surface} still carries the name this product had before \
                 2026-08-13: {text:?}"
        );
    }
    // The storage directory is the eighth surface and is deliberately not
    // touched here: reading it would relocate the directory of whoever is
    // running the tests. `persist::tests` pins its two names against
    // `APP_NAME` without going near `%APPDATA%`.
}

/// PIN — **§7.1.6e (2026-08-20): the turn is not merely hidden where it says
/// something false, it is never started.**
///
/// The angle the `˅` is drawn at is one half of the ruling and `seats` owns
/// it; this is the other half. A tween aimed at 180° under
/// [`profiles::MenuSide::Beside`] would run a clock for 140ms, ask for a
/// repaint on every frame of it, and arrive at a number no surface is going
/// to draw — a whole animation whose only observable effect is the work it
/// costs. So the criterion is applied to the *target*, and it is the same one
/// statement of it the arrow reads: the side decides, and nothing else does.
///
/// `None` — a vertical window whose rail is folded away — is not a third
/// rule. There is no button on screen, so there is no arrow to aim.
#[test]
fn the_chevron_s_turn_is_only_aimed_where_the_picker_hangs_below() {
    assert!(
        chevron_turn_target(true, Some(profiles::MenuSide::Below)),
        "the strip's arrow turns over, and the ruling kept that arm"
    );
    assert!(
        !chevron_turn_target(true, Some(profiles::MenuSide::Beside)),
        "a rail's picker opens to the side, so its arrow has nowhere true to \
             turn — and no clock to run getting there"
    );
    assert!(
        !chevron_turn_target(false, Some(profiles::MenuSide::Below)),
        "a shut menu aims the arrow back down wherever it hangs"
    );
    assert!(
        !chevron_turn_target(true, None),
        "and a window with no new-tab button on screen has no arrow to aim"
    );
}

/// The caption run's four boxes map to four tooltip anchors and nothing else
/// does — a divider is a click target nobody hovers for an explanation.
#[test]
fn only_the_caption_run_carries_a_window_chrome_tooltip() {
    assert_eq!(
        tooltip_anchor_for(seats::ChromeTarget::Settings),
        Some(tooltip::TooltipAnchorId::Settings)
    );
    assert_eq!(
        tooltip_anchor_for(seats::ChromeTarget::CloseWindow),
        Some(tooltip::TooltipAnchorId::CloseWindow)
    );
    assert_eq!(tooltip_anchor_for(seats::ChromeTarget::Tab(0)), None);
    assert_eq!(tooltip_anchor_for(seats::ChromeTarget::TabClose(0)), None);
}

/// N25 — **the scaffold is retired and its chord belongs to nobody.**
///
/// `Ctrl+Alt+Shift+P` opened and closed the preview seat while the block had
/// no verbs that could. Every one of them exists now, so a chord that
/// duplicated them would be a second way into the same state — and it wore
/// Alt, which the shortcut audit rules off limits precisely because AltGr
/// produces `Ctrl+Alt` on the layouts a European user types on.
///
/// Red gate: put the matcher back and `p` under that chord stops reaching
/// the shell. Asserted through the registry rather than against a deleted
/// function, because "no code claims it" is the property, and the registry
/// is the only thing left that could.
#[test]
fn the_retired_preview_chord_reaches_the_shell_like_any_other_key() {
    let key = Key::Character("p".into());
    let table = shortcuts::Shortcuts::defaults();
    let claimed = |modifiers| {
        table.lookup(
            &key,
            &key,
            modifiers,
            shortcuts::Focus {
                preview: false,
                terminal_primary: true,
                terminal: true,
                search_open: false,
                web_page: false,
            },
        )
    };
    let in_preview = |modifiers| {
        table.lookup(
            &key,
            &key,
            modifiers,
            shortcuts::Focus {
                preview: true,
                terminal_primary: false,
                terminal: false,
                search_open: false,
                web_page: false,
            },
        )
    };
    for modifiers in [
        ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::SHIFT,
        // A bare Ctrl+P is DLE and must keep reaching the child.
        ModifiersState::CONTROL,
        ModifiersState::CONTROL | ModifiersState::ALT,
    ] {
        assert!(
            claimed(modifiers).is_none() && in_preview(modifiers).is_none(),
            "{modifiers:?}+P is claimed by nothing, in either focus"
        );
    }
    // And the chord the scaffold deliberately stood aside from belongs to
    // the command palette again (DESIGN.md §7.55): its row left `BINDINGS`
    // for the v0.1 preview because the verb behind it did not exist yet, and
    // returned in v0.2 with the verb in hand. The scaffold wore Alt to keep
    // off this chord; what retired the scaffold is that Alt was never free
    // either, and that reasoning is untouched in either direction.
    assert_eq!(
        claimed(ModifiersState::CONTROL | ModifiersState::SHIFT),
        Some(shortcuts::Action::CommandPalette),
        "Ctrl+Shift+P is the palette's - see shortcuts::Action::CommandPalette"
    );
}

/// **PIN — every popup says which surface it grew out of, and only the two
/// the tab list itself raises can name the sidebar.**
///
/// Two, since 丙2: the `˅`'s profile list hangs off whichever surface carries
/// the `+`, and a tab's context menu hangs off the row a right press landed
/// on — which is the same surface, by the same [`tab_surface`] answer. The
/// other seven are raised by a press somewhere in the panes, and if any of
/// *them* ever answered [`PopupOwner::Tabs`] the rail would be held open by a
/// menu standing in the middle of the stage — the 2026-08-15 flyout report
/// with a different panel in it.
///
/// Both directions matter and the loop below is what keeps them apart: drop
/// `Popup::Tab` back onto the `Stage` line and the sidebar retracts out from
/// under a menu it is still drawing (the 2026-08-25 report); move any of the
/// seven onto the `Tabs` line and the rail is held open by a pane's `⌄`.
///
/// Mutation: either move, and the loop goes red at that popup on all three
/// surfaces.
#[test]
fn only_the_two_menus_the_tab_list_raises_belong_to_a_tab_surface() {
    const OF_THE_TAB_LIST: [Popup; 2] = [Popup::Profile, Popup::Tab];
    for surface in [TabSurface::Strip, TabSurface::Rail, TabSurface::FocusColumn] {
        for popup in OF_THE_TAB_LIST {
            assert_eq!(
                popup_owner(popup, surface),
                PopupOwner::Tabs(surface),
                "{popup:?} is raised on whichever surface carries the tabs"
            );
        }
        for popup in Popup::ALL
            .into_iter()
            .filter(|popup| !OF_THE_TAB_LIST.contains(popup))
        {
            assert_eq!(
                popup_owner(popup, surface),
                PopupOwner::Stage,
                "{popup:?} is raised inside the stage and belongs to no tab \
                     surface"
            );
        }
    }
}

/// RED GATE (user reports and rulings, 2026-08-29 and 2026-08-30) — **a
/// press outside an open dropdown closes it and still lands.**
///
/// The first report: open `General ▸ Language`, click the dialog's own blank
/// background, and the list stays standing — while every other popup in this
/// window goes away on a press outside it. The ruling is one rule for all of
/// them, and the dialog is where it had to be asked separately: it is a modal
/// with a dispatch of its own, so a press inside it never reaches
/// `mouse_input`'s popup arms.
///
/// The second report is what that first fix cost: with a list open, the `×`
/// needed two presses. So the gate no longer ends every press — only the two
/// that land on the dialog's ground (`settings::target_is_ground`: the
/// panel's blank and the scrim). A press on a control takes the list down
/// and then goes on to that control.
///
/// **What is asserted is the position of the gate**, because the position is
/// what makes "the list is already gone when the verb runs" true: the
/// question stands between the hit test and every verb, and its ground arm
/// returns. Judged by `settings::popup_press`, whose own answers are pinned
/// beside it in `settings.rs`
/// (`a_press_outside_an_open_dropdown_closes_it_and_still_lands` there, and
/// `a_press_inside_the_dropdown_still_picks` for the reverse).
///
/// Read off the source for `a_right_press_on_a_tab_raises_its_menu_and_leaves_the_active_tab_alone`'s
/// reason: raising this dialog needs a `Runtime`, and a `Runtime` needs a
/// GPU device, a swapchain and a live window.
///
/// MUTATIONS that must turn it red:
/// ① delete the gate — the first assertion, which is the 08-29 report;
/// ② drop the `DismissAndLand` arm, or make it return like its neighbour —
///    the third and fourth, which are the 08-30 report;
/// ③ let the ground arm fall through instead of returning, so a press on the
///    scrim shuts the dialog under an open list — the fifth;
/// ④ move the gate below `SettingsPanel::press` or below the verb match,
///    where the focus has already moved and the scrim has already answered —
///    the sixth and seventh;
/// ⑤ judge it against a rectangle of its own instead of the hit test's
///    answer — the second, which pins that `hit` is asked first.
#[test]
fn a_press_outside_an_open_dropdown_closes_it_and_still_lands() {
    let router = method_body("Runtime", "settings_mouse_input");

    // ① the gate exists, and it asks the one door both popups leave by.
    let gate = router
        .find("settings::popup_press(self.window.settings.popup_up(), target)")
        .expect(
            "a press outside the dialog's open popup is judged by \
                 `settings::popup_press`, not by a rule written here",
        );
    // ② off the hit test's own answer — the geometry is `settings::hit`'s
    // and this router measures nothing.
    let hit = router
        .find("let target = settings::hit(layout,")
        .expect("the router hit-tests the press once");
    assert!(
        hit < gate,
        "the gate reads the hit test's answer, which is where the popup's \
             own rectangle was already asked"
    );
    // ③ the popup goes away on both of the outside answers.
    let press = router
        .find("self.window.settings.press(target);")
        .expect("the router moves the focus to what was pressed");
    let arm = &router[gate..press];
    let land = arm
        .find("settings::PopupPress::DismissAndLand(popup)")
        .expect("a press outside the list that landed on a control has an arm");
    let ground = arm
        .find("settings::PopupPress::DismissOnly(popup)")
        .expect("a press outside the list that landed on ground has an arm");
    assert_eq!(
        arm.matches("self.window.settings.close_popup(popup)")
            .count(),
        2,
        "both outside answers put the popup away: {arm}"
    );
    assert!(
        land < ground,
        "the arms read in the order the ruling does: land, then the ground \
             that ends the gesture"
    );
    // ④ and the one that landed on a control does NOT end the press — the
    // × that used to need pressing twice (user report 2026-08-30).
    assert!(
        !arm[land..ground].contains("return Ok(());"),
        "a press that landed on a control goes on to that control: {arm}"
    );
    // ⑤ while the one that landed on ground does, which is what leaves the
    // dialog standing under a press on its own scrim.
    assert!(
        arm[ground..].contains("return Ok(());"),
        "a press on the dialog's ground is the whole gesture: {arm}"
    );
    // ⑥ before the focus moves.
    assert!(gate < press, "the gate stands above `SettingsPanel::press`");
    // ⑦ and before every verb, the scrim's close included.
    let verbs = router
        .find("settings::SettingsTarget::Scrim => self.window.settings.close()")
        .expect("the scrim closes the dialog");
    assert!(
        gate < verbs,
        "a press on the scrim while a list is open shuts the list, not the \
             dialog — one layer per gesture, which is what the first Esc does"
    );
}

#[test]
fn startup_metrics_must_match_the_authoritative_win32_scale_factor() {
    assert!(ensure_metrics_match_authoritative_scale(1.5, 1.5).is_ok());
    assert!(ensure_metrics_match_authoritative_scale(1.0, 1.5).is_err());
}

#[test]
fn recorded_swapchain_size_matches_clamped_physical_inner_after_every_reconcile_size() {
    const LIMIT: u32 = 8192;
    for inner_size in [
        PhysicalSize::new(960, 600),
        PhysicalSize::new(1440, 900),
        PhysicalSize::new(1920, 1200),
        PhysicalSize::new(2560, 1440),
    ] {
        assert!(swapchain_size_matches_inner(
            (inner_size.width, inner_size.height),
            inner_size,
            LIMIT,
        ));
    }
    assert!(swapchain_size_matches_inner(
        (534, LIMIT),
        PhysicalSize::new(534, 65_464),
        LIMIT,
    ));
    assert!(!swapchain_size_matches_inner(
        (3840, 2160),
        PhysicalSize::new(1920, 1200),
        LIMIT,
    ));
}

#[test]
fn pty_pixel_size_is_clamped_to_backend_width() {
    let size = pty_size(
        GridSize {
            columns: std::num::NonZeroU16::new(80).unwrap(),
            rows: std::num::NonZeroU16::new(24).unwrap(),
        },
        PhysicalSize::new(100_000, 80_000),
    );
    assert_eq!((size.pixel_width, size.pixel_height), (u16::MAX, u16::MAX));
}

#[test]
fn private_resize_repaint_input_is_exact_and_integration_gated() {
    assert_eq!(
        psreadline_resize_repaint_input(profiles::Integration::PowerShellOptIn, true),
        Some(PSREADLINE_INVOKE_PROMPT_INPUT)
    );
    assert_eq!(
        psreadline_resize_repaint_input(profiles::Integration::PowerShellOptIn, false),
        None,
        "a session without an open OSC 133 input region injects zero bytes"
    );
}

#[test]
fn resize_storm_reanchor_debt_is_replaced_and_paid_once() {
    fn powershell(pending: &mut bool) -> ResizeReanchor<'_> {
        ResizeReanchor {
            pending,
            integration: profiles::Integration::PowerShellOptIn,
        }
    }
    let mut pending = false;
    for _ in 0..3 {
        replace_psreadline_resize_reanchor_debt(powershell(&mut pending), true);
    }
    assert_eq!(
        take_psreadline_resize_reanchor_input(powershell(&mut pending), true),
        Some(PSREADLINE_INVOKE_PROMPT_INPUT),
        "three commits in one open-input transaction coalesce to one chord"
    );
    assert_eq!(
        take_psreadline_resize_reanchor_input(powershell(&mut pending), true),
        None,
        "the repair debt is one shot"
    );

    replace_psreadline_resize_reanchor_debt(powershell(&mut pending), true);
    replace_psreadline_resize_reanchor_debt(powershell(&mut pending), false);
    assert_eq!(
        take_psreadline_resize_reanchor_input(powershell(&mut pending), true),
        None,
        "a later closed-region commit replaces stale open-prompt debt"
    );
    replace_psreadline_resize_reanchor_debt(powershell(&mut pending), true);
    assert_eq!(
        take_psreadline_resize_reanchor_input(powershell(&mut pending), false),
        None,
        "a prompt that closes before quiescence receives no stale chord"
    );
}

/// RED — **a gesture that comes back to the width the child already has still owes a
/// release** (user report 2026-09-17).
///
/// What is queued here is the end of a gesture, not a size the child is owed. It used to be
/// the second thing, so a solve answering `conpty_grid` cancelled the queue outright — and
/// `plan_grid_change` had already reflowed this pane's own actor on that very solve, opening a
/// resize transaction that only a release can settle. No release, no settlement, and the
/// transaction stayed open for the rest of the pane's life.
///
/// Red gate: put the `*pending = None` back in the `else`. The drag below then ends with an
/// empty queue and nothing to close its transaction with.
#[test]
fn a_gesture_that_returns_to_the_childs_own_width_still_queues_its_release() {
    let start = Instant::now();
    let child = grid_of(86, 31);
    let away = grid_of(85, 31);
    let physical = PhysicalSize::new(688, 620);
    let mut pending = None;

    assert!(
        coalesce_pty_resize_on_grid_change(&mut pending, away, child, child, physical, start),
        "the hand leaves the width the child holds, so the child is owed a word"
    );
    let back_at = start + Duration::from_millis(17);
    assert!(
        !coalesce_pty_resize_on_grid_change(&mut pending, child, child, away, physical, back_at),
        "and comes back to it, so it is owed none"
    );
    let due = take_due_pty_resize(&mut pending, back_at + WINDOW_RESIZE_QUIET)
        .expect("the end of the gesture is still owed, and it is what the queue carries");
    assert_eq!(
        due.grid, child,
        "carrying the last grid solved, never an intermediate one"
    );

    // And a solve that moved neither grid is still not a gesture: it schedules nothing, and it
    // does not forget one that is already waiting to be released.
    let mut pending = None;
    assert!(!coalesce_pty_resize_on_grid_change(
        &mut pending,
        child,
        child,
        child,
        physical,
        start
    ));
    assert!(
        take_due_pty_resize(&mut pending, start + WINDOW_RESIZE_QUIET).is_none(),
        "a spurious repeat is not a gesture"
    );
    coalesce_pty_resize_on_grid_change(&mut pending, away, child, child, physical, start);
    coalesce_pty_resize_on_grid_change(&mut pending, away, child, away, physical, start);
    assert!(
        take_due_pty_resize(&mut pending, start + WINDOW_RESIZE_QUIET).is_some(),
        "and a repeat arriving behind a real one must not cancel it"
    );
}

#[test]
fn window_resize_coalescer_keeps_only_the_last_size_and_resets_quiet_deadline() {
    let start = Instant::now();
    let first = GridSize {
        columns: std::num::NonZeroU16::new(80).unwrap(),
        rows: std::num::NonZeroU16::new(24).unwrap(),
    };
    let final_grid = GridSize {
        columns: std::num::NonZeroU16::new(112).unwrap(),
        rows: std::num::NonZeroU16::new(31).unwrap(),
    };
    let mut pending = None;
    coalesce_pty_resize(&mut pending, first, PhysicalSize::new(960, 600), start);
    coalesce_pty_resize(
        &mut pending,
        final_grid,
        PhysicalSize::new(1440, 900),
        start + Duration::from_millis(150),
    );

    assert!(take_due_pty_resize(&mut pending, start + Duration::from_millis(349)).is_none());
    let committed = take_due_pty_resize(&mut pending, start + Duration::from_millis(350)).unwrap();
    assert_eq!(committed.grid, final_grid);
    assert_eq!(committed.physical, PhysicalSize::new(1440, 900));
    assert!(pending.is_none());
}

/// RED-CHECK for the pin above: proves it is not vacuous. The pre-fix `resize()` and
/// `commit_seat_geometry()` called `coalesce_pty_resize` unconditionally — the old
/// spawn-then-resize shape this pin exists to forbid — which schedules a real ConPTY resize
/// even when the grid the PTY was spawned with never moved. Restoring that unconditional call
/// at the two real call sites is exactly what turns the pin above red.
#[test]
fn the_old_unconditional_coalesce_would_have_scheduled_a_resize_for_an_unchanged_grid() {
    let grid = GridSize {
        columns: std::num::NonZeroU16::new(100).unwrap(),
        rows: std::num::NonZeroU16::new(30).unwrap(),
    };
    let mut pending = None;
    let now = Instant::now();
    // The old shape: no `next_grid != current_grid` gate at all.
    coalesce_pty_resize(&mut pending, grid, PhysicalSize::new(1000, 700), now);
    assert!(
        take_due_pty_resize(&mut pending, now + WINDOW_RESIZE_QUIET).is_some(),
        "an unconditional coalesce call schedules a resize even for an unchanged grid"
    );
}

/// The other half of that rule: **letting go is what releases it, not a timer that outlives
/// the gesture.** A hand that comes up after the quiet window has already passed is answered
/// on the very next turn, with no second wait.
///
/// Red gate: return the pending deadline while the hand is down instead of `None` and the
/// wake asked for is one this loop does not need; refuse to release on the turn the hand comes
/// up and the assertion below goes red naming an empty request list.
#[test]
fn letting_go_releases_the_size_on_the_next_turn() {
    let start = Instant::now();
    let mut harness = ResizeGateHarness::new(50, 50);
    harness.hand_down = true;
    harness.window_resized(PhysicalSize::new(960, 1800), false, start);
    // Long past the quiet window, and still nothing, because the hand is still down.
    harness.tick(start + Duration::from_secs(5));
    assert_eq!(harness.requests, Vec::new());
    // The button comes up. No further solve — the rectangle has not moved — and the very next
    // turn pays what is owed.
    harness.hand_down = false;
    harness.tick(start + Duration::from_secs(5) + Duration::from_millis(1));
    assert_eq!(harness.requests, vec![grid_of(16, 50)]);
}

/// A window with no client area is still refused, and by the same gate.
#[test]
fn a_window_with_no_client_area_is_not_a_rectangle_to_solve_for() {
    for empty in [
        PhysicalSize::new(0, 600),
        PhysicalSize::new(960, 0),
        PhysicalSize::new(0, 0),
    ] {
        assert!(!resize_worth_solving(false, empty), "{empty:?}");
    }
    assert!(resize_worth_solving(false, PhysicalSize::new(1, 1)));
}

/// PIN: a PTY deadline handed to `ControlFlow::WaitUntil` must still be in the future.
///
/// A past `WaitUntil` makes winit immediately re-enter `about_to_wait` instead of sleeping, so
/// the loop spins on a deadline it has already served. The pending request's own coalescing
/// deadline is the only one there is, and the instant it comes due it is released and there is
/// nothing left to wait for.
///
/// Red gate: drop the `filter(|deadline| *deadline > now)` in `pty_resize_wake_deadline` and
/// the first assertion names the already-due instant it offered.
#[test]
fn wait_until_is_never_offered_an_already_due_resize_deadline() {
    let start = Instant::now();
    let mut pending = None;
    coalesce_pty_resize(
        &mut pending,
        grid_of(80, 24),
        PhysicalSize::new(800, 600),
        start,
    );
    let due_at = start + WINDOW_RESIZE_QUIET;

    assert_eq!(
        pty_resize_wake_deadline(pending, start),
        Some(due_at),
        "before it is due, the coalescing deadline is exactly what the loop should wait for"
    );
    assert!(
        pty_resize_wake_deadline(pending, due_at).is_none(),
        "ControlFlow::WaitUntil must never receive an already-due PTY deadline"
    );
    let (released, wake_deadline) = service_pending_pty_resize(&mut pending, due_at, false);
    assert_eq!(
        released.map(|released| released.grid),
        Some(grid_of(80, 24)),
        "the same reading that refuses the deadline is the one that released the request"
    );
    assert_eq!(
        wake_deadline, None,
        "nothing is owed, so nothing is awaited"
    );
}

#[test]
fn resize_atomic_present_gate_rejects_the_previous_grid_frame() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
    let mut projection = session.new_projection(session.layout_key());
    let old_frame = session.viewport_frame(&mut projection).unwrap();
    let new_grid = GridSize {
        columns: std::num::NonZeroU16::new(42).unwrap(),
        rows: std::num::NonZeroU16::new(12).unwrap(),
    };
    assert!(!frame_matches_grid(&old_frame, new_grid));

    session
        .resize(NonZeroU32::new(42).unwrap(), NonZeroU32::new(12).unwrap())
        .unwrap();
    session.refresh_projection(&mut projection);
    let new_frame = session.viewport_frame(&mut projection).unwrap();
    assert!(frame_matches_grid(&new_frame, new_grid));
}

#[test]
fn pty_mode_only_update_is_presentation_equivalent_but_text_is_not() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(3).unwrap());
    let mut projection = session.new_projection(session.layout_key());
    let before = session.viewport_frame(&mut projection).unwrap();

    session.feed(b"\x1b[?2004h").unwrap();
    session.refresh_projection(&mut projection);
    let mode_only = session.viewport_frame(&mut projection).unwrap();
    assert!(presentation_equivalent(&before, &mode_only));

    session.feed(b"visible").unwrap();
    session.refresh_projection(&mut projection);
    let text = session.viewport_frame(&mut projection).unwrap();
    assert!(!presentation_equivalent(&mode_only, &text));
}

/// **A frame that says the same thing from somewhere else is not the same
/// frame.**
///
/// The dedupe above exists because a frame drawing the same picture need not
/// be published again, and that holds right up until the picture stops being
/// the only thing the frame says. It is also the answer to every question
/// about where the pointer is: `live_point_at`, the word and line selections,
/// the links and the caret all read `horizontal` off the *published* frame.
/// Drop a frame whose cells happen to match and the reader is left holding an
/// origin they have moved off, and their next click lands on a column they
/// have left.
///
/// The two frames here differ in the axis and in nothing else, on purpose:
/// building a live fixture whose cells genuinely coincide across a move is
/// hard *in this build* — a physical row past its own last column is empty
/// rather than blank (plan §5.1 clause 4), so the live plane usually gives
/// the move away. "Usually" is not "always", and it is not a property this
/// dedupe should be resting on.
///
/// MUTATION: drop `previous.horizontal == next.horizontal` from
/// `presentation_equivalent` and this passes the moved frame off as
/// unchanged.
#[test]
fn a_frame_that_says_the_same_thing_from_a_new_origin_is_not_the_old_frame() {
    use bt_viewport::horizontal::HorizontalProjection;

    let session = DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(2).unwrap());
    let mut projection = session.new_projection(session.layout_key());
    let before = session.viewport_frame(&mut projection).unwrap();
    assert!(
        presentation_equivalent(&before, &before.clone()),
        "a frame is equivalent to itself, or this test proves nothing"
    );

    let mut moved = before.clone();
    moved.horizontal = HorizontalProjection::new(ContentColumn(40), 8, ContentColumn(4));
    assert_eq!(
        before.cells, moved.cells,
        "the two differ in the axis and nowhere else"
    );
    assert!(
        !presentation_equivalent(&before, &moved),
        "same cells, different origin — a different frame, not the same one"
    );
}

#[test]
fn content_before_trailing_bsu_is_published_and_survives_empty_sync_timeout() {
    let mut harness = PtyPresentationHarness::new(20, 2);

    assert!(harness.feed_drain(b"visible-before-bsu\x1b[?2026h"));
    assert!(harness.session.synchronized_update_deadline().is_some());
    assert_eq!(harness.publications, 1);
    assert!(
        frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("visible-before-bsu")
    );

    let (finished, republished) = harness.finish_synchronized_update();
    assert!(finished);
    assert!(
        !republished,
        "the already-pending visible frame is equivalent"
    );
    assert_eq!(harness.publications, 1);
    assert!(
        frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("visible-before-bsu")
    );
}

#[test]
fn completed_sync_update_is_published_before_a_trailing_bsu_in_the_same_drain() {
    let mut harness = PtyPresentationHarness::new(24, 2);

    assert!(harness.feed_drain(b"\x1b[?2026h\x1b[H\x1b[2Kclosed-update\x1b[?2026l\x1b[?2026h"));
    assert!(harness.session.synchronized_update_deadline().is_some());
    assert_eq!(harness.publications, 1);
    assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("closed-update"));
}

#[test]
fn rapid_synchronized_update_chain_never_withholds_a_completed_frame() {
    const UPDATE_COUNT: usize = 81;
    let mut harness = PtyPresentationHarness::new(24, 2);

    for update in 0..UPDATE_COUNT {
        let prefix = if update == 0 { "\x1b[?2026h" } else { "" };
        let bytes = format!("{prefix}\x1b[H\x1b[2Kframe-{update:02}\x1b[?2026l\x1b[?2026h");
        assert!(
            harness.feed_drain(bytes.as_bytes()),
            "completed update {update} was not published"
        );
        assert!(harness.session.synchronized_update_deadline().is_some());
        assert!(
            frame_row_text(harness.pending.pending_frame().unwrap(), 0)
                .contains(&format!("frame-{update:02}"))
        );
    }

    assert_eq!(harness.publications, UPDATE_COUNT);
    assert!(!harness.feed_drain(b"\x1b[?2026l"));
    assert!(harness.session.synchronized_update_deadline().is_none());
    assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("frame-80"));
    assert!(harness.present_pending());
    assert!(harness.pending.pending_frame().is_none());
}

#[test]
fn pending_frame_is_the_a_b_a_equivalence_baseline() {
    let mut harness = PtyPresentationHarness::new(4, 1);

    assert!(harness.feed_drain(b"A"));
    assert!(harness.present_pending());
    assert!(frame_row_text(harness.last_presented.as_ref().unwrap(), 0).contains('A'));

    assert!(harness.feed_drain(b"\rB"));
    assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains('B'));
    assert!(harness.feed_drain(b"\rA"));
    assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains('A'));
    assert_eq!(harness.publications, 3);
}

#[test]
fn forwarded_mouse_hit_stays_bound_to_the_presented_frame_during_an_unpresented_shift() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(6).unwrap());
    session
        .feed(b"\x1b[?1003h\x1b[?1006ha\r\nb\r\nc\r\nheader\r\nx\r\ny")
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let presented = session.viewport_frame(&mut projection).unwrap();
    assert!(frame_row_text(&presented, 3).contains("header"));

    session.feed(b"\r\nexpanded-1\r\nexpanded-2").unwrap();
    session.refresh_projection(&mut projection);
    let unpresented = session.viewport_frame(&mut projection).unwrap();
    assert!(frame_row_text(&unpresented, 1).contains("header"));
    assert!(!frame_row_text(&unpresented, 3).contains("header"));

    let stale_aim = bt_render::GridHit { row: 3, column: 0 };
    let forwarded = live_viewport_mouse_hit(&presented, stale_aim);
    let mut route = None;
    let bytes = route_forwarded_mouse_button(
        &mut route,
        ElementState::Pressed,
        input::MouseProtocolButton::Left,
        forwarded,
        session.terminal_modes(),
        ModifiersState::empty(),
        PressedCellTarget::Ordinary,
        a_shell(),
    )
    .unwrap();

    assert_eq!(bytes, b"\x1b[<0;1;4M");
    assert_eq!(
        forwarded.row, 3,
        "the stale row correctly misses live row 1"
    );
}

#[test]
fn a_tracked_pane_keeps_a_press_on_an_ordinary_cell_and_shift_still_takes_it_back() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1000h\x1b[?1006h").unwrap();
    let hit = bt_render::GridHit { row: 1, column: 2 };
    let mut route = None;
    let forwarded = route_forwarded_mouse_button(
        &mut route,
        ElementState::Pressed,
        input::MouseProtocolButton::Left,
        hit,
        session.terminal_modes(),
        ModifiersState::CONTROL,
        PressedCellTarget::Ordinary,
        a_shell(),
    );
    assert!(forwarded.is_some());
    assert!(matches!(route, Some(MouseRoute::Forward { .. })));

    let mut shifted_route = None;
    assert!(
        route_forwarded_mouse_button(
            &mut shifted_route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
            PressedCellTarget::Ordinary,
            a_shell(),
        )
        .is_none()
    );
    assert!(shifted_route.is_none());
}

/// PIN (ticket #62) — **`Copy` on the menu is copy-on-select's own door: the
/// same bytes, and the selection is still there afterwards.**
///
/// The contrast with `Ctrl+C`'s door is the content. A keystroke that ends a
/// gesture may reasonably clear the highlight; a menu row reached by
/// *pointing at the highlight* may not, because the thing the reader aimed at
/// would vanish as the reward for having used it. Both doors are exercised on
/// one selection here so the difference cannot be read as an accident of two
/// separate fixtures.
///
/// MUTATION: point the `Copy` row at `copy_selection` and the second half of
/// this test goes red on the selection that is no longer standing.
#[test]
fn the_menus_copy_is_copy_on_selects_door_and_leaves_the_selection_standing() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"copy me").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let selection = ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 6, Bias::After).unwrap().unwrap(),
    };
    session.set_view_selection(Some(selection.clone()));
    projection.set_selection(Some(selection));

    // The menu row's door, which is the one copy-on-select spends.
    let mut menu_clipboard = String::new();
    assert!(write_selection_text(&session, true, |text| {
        menu_clipboard.push_str(text);
        Ok(())
    }));
    assert_eq!(menu_clipboard, "copy me");
    assert!(
        session.view_selection().is_some(),
        "the row was reached by pointing at this selection; it stays"
    );
    assert!(projection.selection().is_some());

    // The keyboard's door, on the same selection, for the difference.
    let mut keyboard_clipboard = String::new();
    assert!(copy_selection(&mut session, &mut projection, |text| {
        keyboard_clipboard.push_str(text);
        Ok(())
    }));
    assert_eq!(
        keyboard_clipboard, menu_clipboard,
        "the two doors put the same bytes on the clipboard"
    );
    assert!(session.view_selection().is_none());
}

/// PIN (ticket #62) — **the menu's `Paste` is the keyboard's paste, and
/// there is only one of it.**
///
/// What the row spends is `paste_from_clipboard_into`, which is what
/// `Ctrl+V` spends with the focused seat filled in — so the thing worth
/// pinning is the door itself, exercised here on the four promises every
/// caller inherits: the bytes are bracketed when the shell asked for
/// bracketing, `\r\n` is normalised the way a terminal delivers it, the
/// selection goes, and the view comes back to the bottom. A second paste
/// path would be a second place for the multi-line policy to be decided when
/// it lands (P2-6), which is the whole reason there is one.
///
/// MUTATION: give the menu its own writer and the bracketing promise is the
/// first thing to drift — a `Paste` that skipped `ESC [ 200 ~` hands `bash`
/// a multi-line paste it runs a line at a time.
#[test]
fn the_menus_paste_is_the_keyboards_paste_door_and_leaves_the_view_at_the_bottom() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session
        .feed(b"\x1b[?2004hone\r\ntwo\r\nthree\r\nfour")
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let selection = ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 2, Bias::After).unwrap().unwrap(),
    };
    session.set_view_selection(Some(selection.clone()));
    projection.set_selection(Some(selection));
    projection.scroll_by_subpixels(2 * projection.cell_height_subpixels().get());
    assert!(
        projection.is_scrolled(),
        "the fixture has to be reading history or the return to the bottom proves nothing"
    );

    let mut written = Vec::new();
    paste_text(&mut session, &mut projection, "a\r\nb\n", |chunk| {
        written.extend_from_slice(chunk);
        Ok(())
    })
    .unwrap();

    assert_eq!(
        written, b"\x1b[200~a\rb\r\x1b[201~",
        "bracketed because the shell asked, with the breaks a terminal delivers"
    );
    assert!(session.view_selection().is_none());
    assert!(projection.selection().is_none());
    assert!(
        !projection.is_scrolled(),
        "typing returns the view to the live bottom, and a paste is typing"
    );
}

/// RED (T-RESTART-CWD, 2026-10-04) — **a pane whose shell never reported a
/// folder is started again where it was born** (`docs/M2-restart-shell-contract.md`
/// §1.1: 无上报则该 seat 的初始 cwd).
///
/// `Restart shell` read only the shell's OSC 7 report, so a pane opened by
/// `New terminal in folder…` (or split off into a folder) whose shell has no
/// integration restarted at its profile's default folder. The ladder is
/// [`LeafSession::place_for_a_new_shell`]: the report, else the folder the
/// spawn put the shell down in, except the shell's-own-home mark, which is
/// handed on as nothing so the next spawn asks for the home again.
///
/// MUTATIONS, each observed red: drop the spawn rung (answer `None` after the
/// report) — the first assertion; read the spawn rung before the report — the
/// second; drop the shell's-home exception — the third.
#[test]
fn a_pane_born_in_a_named_folder_starts_its_next_shells_there_whatever_the_profile_says() {
    // RED (coordinator's ruling 2026-10-05) — **a pane opened in a named folder hands that folder
    // on as named**, so Restart shell, Duplicate tab, Duplicate pane and the splits stand there
    // under Home and a fixed folder too; a pane not born that way hands its folder on as carried
    // and keeps the profile's rule. The leaf's half is `LeafSession::seed_place_for_a_new_shell`;
    // `profiles::place_for` weighs it (pinned by
    // `every_road_that_names_a_folder_opens_there_whatever_the_profile_says`).
    //
    // MUTATION, observed red: answer `seed_place_for_a_new_shell` with `Carried` whatever
    // `born_named` says — the named leaf's restart goes to the fixed folder.
    let born_in = PathBuf::from(r"D:\项目\clicked");
    let named = LeafSession {
        spawn_place: Some(born_in.clone()),
        born_named: true,
        ..leaf_saying("no report from this shell")
    };
    let carried = LeafSession {
        spawn_place: Some(born_in.clone()),
        ..leaf_saying("no report from this shell either")
    };
    let fixed = profiles::StartAt::Fixed(PathBuf::from(r"E:\固定"));
    let restarted_in = |leaf: &LeafSession| {
        struct Nowhere;
        impl bt_pty::ShellEnvironment for Nowhere {
            fn var_os(&self, _: &str) -> Option<std::ffi::OsString> {
                None
            }
            fn is_file(&self, _: &Path) -> bool {
                false
            }
        }
        profiles::place_for(
            &fixed,
            &profiles::StartingDir::AccountHome,
            profiles::PathNamespace::Windows,
            restart_seed(&leaf.profile, leaf.seed_place_for_a_new_shell()).cwd,
            &Nowhere,
        )
        .working_directory
    };
    assert_eq!(
        named.seed_place_for_a_new_shell(),
        Some(profiles::SeedPlace::Named(born_in.clone()))
    );
    assert_eq!(restarted_in(&named), Some(born_in.clone()));
    assert_eq!(
        carried.seed_place_for_a_new_shell(),
        Some(profiles::SeedPlace::Carried(born_in))
    );
    assert_eq!(
        restarted_in(&carried),
        Some(PathBuf::from(r"E:\固定")),
        "a pane not born in a named folder keeps its profile's fixed folder"
    );
    // The `+` and a picker row beside a pane born named still carry (the review's unpinned
    // clause). MUTATION, observed red: answer `place_for_a_new_tab_beside` with
    // `seed_place_for_a_new_shell` — the folder arrives named.
    let reported = LeafSession {
        spawn_place: Some(PathBuf::from(r"D:\项目\clicked")),
        born_named: true,
        ..leaf_saying("\u{1b}]7;file://localhost/D:/Developer/elsewhere\u{7}")
    };
    assert_eq!(
        reported.place_for_a_new_tab_beside(),
        Some(profiles::SeedPlace::Carried(PathBuf::from(
            r"D:\Developer\elsewhere"
        ))),
        "a new tab beside a pane born in a named folder carries its folder"
    );
}

#[test]
fn a_pane_that_never_reported_a_folder_is_started_again_where_it_was_born() {
    let born_in = PathBuf::from(r"D:\Projects\chosen-folder");

    let silent = LeafSession {
        spawn_place: Some(born_in.clone()),
        ..leaf_saying("no report from this shell")
    };
    assert_eq!(
        restart_seed(&silent.profile, silent.seed_place_for_a_new_shell()).cwd,
        Some(profiles::SeedPlace::Carried(born_in.clone())),
        "the folder the pane was born in, not the profile's default"
    );

    let reported = LeafSession {
        spawn_place: Some(born_in.clone()),
        ..leaf_saying("\u{1b}]7;file://localhost/D:/Developer/folio-terminal\u{7}")
    };
    assert_eq!(
        reported.place_for_a_new_shell(),
        Some(PathBuf::from(r"D:\Developer\folio-terminal")),
        "a report is the first rung and beats where the shell was born"
    );

    // A WSL pane put down at its shell's home hands its mark on; `profiles::place_for` reads a
    // place equal to the home mark as the shell's home (round 2), pinned in `profiles::tests`.
    let at_home = LeafSession {
        spawn_place: Some(PathBuf::from("~")),
        ..leaf_saying("no report from this shell either")
    };
    assert_eq!(at_home.place_for_a_new_shell(), Some(PathBuf::from("~")));
}

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

#[test]
fn post_drag_wheel_frame_reaches_the_renderer_text_row_slice_boundary() {
    let start = Instant::now();
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(3).unwrap());
    session.feed_at(b"a\r\nb\r\nc", start).unwrap();
    session
        .resize_at(
            NonZeroU32::new(4).unwrap(),
            NonZeroU32::new(2).unwrap(),
            start + Duration::from_millis(10),
        )
        .unwrap();
    session.mark_pty_resize_requested_at(
        NonZeroU32::new(4).unwrap(),
        NonZeroU32::new(2).unwrap(),
        start + Duration::from_millis(210),
    );
    session
        .feed_at(b"\r\nx", start + Duration::from_millis(220))
        .unwrap();
    assert!(
        session
            .finish_resize_if_quiescent(start + Duration::from_millis(420))
            .unwrap()
    );

    let mut projection = session.new_projection(session.layout_key());
    projection.scroll_by_rows(1);
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();
    session.record_published_frame(&frame, start + Duration::from_millis(421));
    let render_rows = bt_render::text_row_cells(&frame)
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(frame.grid_rows.get(), 2);
    assert_eq!(frame.rows.get(), 3);
    assert_eq!(frame.drawable_rows(), 2);
    assert_eq!(render_rows.len(), 3);
    assert!(render_rows.iter().all(|row| row.len() == 4));
}

#[test]
fn panic_log_uses_the_process_temp_directory_without_requiring_stderr() {
    assert_eq!(
        panic_log_path(),
        std::env::temp_dir().join("folio-panic.log"),
        "the file a user is asked to send is named after the product"
    );
}

#[test]
fn the_sentence_a_crash_raises_names_the_file_it_left() {
    let path = std::env::temp_dir().join("folio-panic.log");
    let text = panic_alert_text(&path);
    assert!(
        text.contains(&path.display().to_string()),
        "the reader is told where the file is: {text}"
    );
    assert!(
        text.contains(&version::banner()),
        "and which build left it: {text}"
    );

    // **And it is raised, not filed.** Once the front door has closed this
    // process's `stdout` is `diagnostics.log`, so "did the write succeed"
    // is not the question — the write always succeeds and reaches nobody.
    //
    // MUTATION: return `true` for `Channel::Log` and the box stops
    // appearing in exactly the case it was written for; the crash goes into
    // the log twice and the user still watches the window vanish.
    use diagnostics::Channel;
    assert!(
        !diagnostics::a_screen_is_watching(Some(Channel::Log)),
        "a resident run's stdout is its own log file"
    );
    assert!(
        !diagnostics::a_screen_is_watching(Some(Channel::Nowhere)),
        "and a run with nowhere to write has nowhere to write"
    );
    assert!(
        diagnostics::a_screen_is_watching(Some(Channel::Console)),
        "a run that kept its console kept somebody looking at it"
    );
    assert!(
        diagnostics::a_screen_is_watching(None),
        "and before the front door closes, stdout is still the caller's"
    );
}

#[test]
fn one_cell_terminal_and_zero_pixel_transition_are_defended() {
    let one = std::num::NonZeroU16::new(1).unwrap();
    let grid = GridSize {
        columns: one,
        rows: one,
    };
    let backend = pty_size(grid, PhysicalSize::new(0, 0));
    assert_eq!((backend.columns.get(), backend.rows.get()), (1, 1));
    assert_eq!((backend.pixel_width, backend.pixel_height), (0, 0));

    let mut session =
        DualPlaneSession::new(NonZeroU32::new(1).unwrap(), NonZeroU32::new(1).unwrap());
    session.feed(b"A").unwrap();
    session
        .resize(NonZeroU32::new(1).unwrap(), NonZeroU32::new(1).unwrap())
        .unwrap();
    assert_eq!(session.terminal().visible_text(), ["A"]);
}

#[test]
fn direct_width_fixture_places_legacy_and_2027_closing_bars_on_their_rulers() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
    session
        .feed(include_bytes!("../../../scripts/dev/width-probe-input.vt"))
        .unwrap();

    let closing_bar = |row: u32, start: usize| {
        session
            .terminal()
            .visible_row(row)
            .unwrap()
            .cells
            .iter()
            .enumerate()
            .skip(start)
            .filter(|(_, cell)| cell.text == "|")
            .map(|(column, _)| column)
            .nth(1)
            .unwrap()
    };
    let rows = [2, 4, 6, 8, 10, 12, 14];
    let legacy_widths = [8, 4, 2, 1, 7, 1, 2];
    let mode_2027_widths = [2, 2, 2, 1, 7, 1, 2];
    for ((row, legacy), clustered) in rows.into_iter().zip(legacy_widths).zip(mode_2027_widths) {
        assert_eq!(closing_bar(row, 0), 1 + legacy, "legacy content row {row}");
        assert_eq!(
            closing_bar(row + 1, 0),
            1 + legacy,
            "legacy ruler row {}",
            row + 1
        );
        assert_eq!(
            closing_bar(row, 40),
            41 + clustered,
            "2027 content row {row}"
        );
        assert_eq!(
            closing_bar(row + 1, 40),
            41 + clustered,
            "2027 ruler row {}",
            row + 1
        );
    }
}

#[test]
fn direct_glyph_fixture_preserves_emoji_and_ambiguous_cell_occupancy() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
    session
        .feed(include_bytes!("../../../scripts/dev/glyph-probe-input.vt"))
        .unwrap();

    let bar_columns = |row: u32| {
        session
            .terminal()
            .visible_row(row)
            .unwrap()
            .cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.text == "|")
            .map(|(column, _)| column)
            .collect::<Vec<_>>()
    };
    for (content_row, width) in [(2, 2), (4, 2), (6, 2), (8, 2), (10, 1)] {
        assert_eq!(bar_columns(content_row), [0, 1 + width]);
        assert_eq!(bar_columns(content_row + 1), [0, 1 + width]);
    }
    assert_eq!(bar_columns(13), [0, 2, 4]);
    assert_eq!(bar_columns(14), [0, 2, 4]);
}

#[test]
fn real_powershell_input_reaches_a_viewport_owned_frame() {
    let columns = std::num::NonZeroU16::new(48).unwrap();
    let rows = std::num::NonZeroU16::new(10).unwrap();
    // The default shell, resolved as a pane resolves it, but started through `TestShell`:
    // without the user's `$PROFILE`, and with history refused (and read back) before the line
    // below is typed. Started the ordinary way, this test appended that line to the user's own
    // PSReadLine history on every run (T-TEST-SHELL-HYGIENE).
    let mut pty =
        bt_pty::test_shell::TestShell::spawn_default(PtySize::cells(columns, rows)).unwrap();
    let mut session = DualPlaneSession::with_quotas_and_cell_height(
        nonzero_u32(columns.get()),
        nonzero_u32(rows.get()),
        DEFAULT_STAGING_QUOTA,
        DEFAULT_FROZEN_LINE_QUOTA,
        std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
    );
    // The child here is a real PowerShell starting for real, so *how long* it needs to
    // reach a prompt and echo a command back is a fact about the machine, not about this
    // terminal. A total wall-clock budget therefore made this test a load meter: at rest it
    // finished in six seconds, but with twenty-four spinners on this twenty-four-thread host
    // the 2026-08-20 experiment failed it **16 times out of 16**, always on the same ten-second
    // ceiling, always with the child working normally on the other side of it.
    //
    // What the test wants to know is whether the child has *stopped*, and that question
    // survives a busy host: a starved machine delivers the same bytes, only further apart. So
    // the budget restarts on every byte read, and a separate ceiling catches the one shape
    // silence cannot — a child that talks forever without ever saying this.
    //
    // Enlarging the old ten-second *total* was the option not taken, and the distinction is
    // the point: a total grows with the work the child has left, so no value of it is right on
    // a machine of unknown speed, while a silence budget is one judgement about how long a
    // live process may be denied the CPU before we call it dead. Thirty seconds is that
    // judgement, and it is `bt-pty`'s `PROBE_SILENCE_BUDGET` to the second — same question,
    // same host, and the two probes should not answer it differently. See there for the
    // measurements it was chosen from.
    const SILENCE_BUDGET: Duration = Duration::from_secs(30);
    const CEILING: Duration = Duration::from_secs(180);
    const MARKER: &str = "BT_APP_INPUT_OK";

    let started = Instant::now();
    let mut last_output = Instant::now();
    let mut bytes_read = 0usize;
    let mut command_sent = false;
    let mut output_seen = false;
    while !output_seen {
        let bytes = pty.read_output();
        if !bytes.is_empty() {
            last_output = Instant::now();
            bytes_read += bytes.len();
            session.feed(&bytes).unwrap();
            let replies = session.take_pty_writes();
            for reply in &replies {
                pty.reply(reply).unwrap();
            }
            if !command_sent && !replies.is_empty() {
                pty.write(&ime_commit_bytes("Write-Output ('BT_APP_' + 'INPUT_OK')\r"))
                    .unwrap();
                command_sent = true;
            }
            output_seen = session
                .terminal()
                .visible_text()
                .iter()
                .any(|line| line.contains(MARKER));
            continue;
        }
        let silent_for = last_output.elapsed();
        if silent_for >= SILENCE_BUDGET || started.elapsed() >= CEILING {
            panic!(
                "{MARKER} never reached Term: gave up after {:?}, the last {:?} of it with the \
                     child silent, having read {bytes_read} bytes; the handshake {}; {}. Screen \
                     {:?}",
                started.elapsed(),
                silent_for,
                if command_sent {
                    "completed and the command was sent"
                } else {
                    "never completed, so the command was never sent"
                },
                pty.account(),
                session.terminal().visible_text()
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let rendered_text = frame
        .cells
        .iter()
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    assert!(rendered_text.contains("BT_APP_INPUT_OK"));
    pty.write(b"exit\r").unwrap();
    pty.shutdown().unwrap();
}
// ── T5: the drag's own clocks and rulings ──

/// PIN (§7.1.6b″) — **"in flight" is a fraction of the journey, not a
/// distance, and a hand is always the whole of it.**
///
/// Three claims, and each one is a way the obvious implementation goes
/// wrong:
///
/// * A tab *in the hand* answers `1.0` even when the pointer has carried it
///   back to exactly its own slot. `offset != 0.0` gets this wrong, and the
///   frame it gets wrong is the one where a card you are still holding drops
///   back into the list under its neighbours.
/// * A settling tab answers the *remaining fraction* and not `from`. Two
///   tabs displaced by one slot and by five are one event at two magnitudes;
///   a shadow keyed on the distance is five times heavier for the second.
/// * **Reduced motion answers `0.0`**, because there is no slide: with no
///   animation the tab is simply in its slot, and lifting a thing that is
///   not moving would be inventing a state the reader asked not to see.
///
/// Red gate: derive the answer from `sample().0` and the first assertion
/// goes red; drop the `motion` filter in `FlipTween::remaining` and the last
/// one does.
#[test]
fn a_flight_is_how_much_of_the_journey_is_left_and_a_hand_is_all_of_it() {
    let now = Instant::now();
    let resting = FlipTween::default();

    assert_eq!(
        resting.flight(now, Motion::Full, None),
        0.0,
        "a tab nothing has touched is not in flight"
    );
    assert_eq!(
        resting.flight(now, Motion::Full, Some(0.0)),
        1.0,
        "a tab carried back to its own slot is still in the hand"
    );

    // Two tabs displaced by very different distances, sampled at the same
    // instant of the same tween: one event, one answer.
    let mut near = FlipTween::default();
    let mut far = FlipTween::default();
    near.displace(12.0, now, Motion::Full);
    far.displace(600.0, now, Motion::Full);
    let quarter = now + TAB_FLIP / 4;
    assert!(
        (near.flight(quarter, Motion::Full, None) - far.flight(quarter, Motion::Full, None)).abs()
            < 1e-6,
        "a one-slot swap and a five-slot swap are the same flight"
    );
    assert!(
        near.flight(quarter, Motion::Full, None) < 1.0,
        "and it has already begun to fade"
    );
    assert_eq!(
        near.flight(now + TAB_FLIP, Motion::Full, None),
        0.0,
        "it is over when the tween is"
    );

    let mut reduced = FlipTween::default();
    reduced.displace(120.0, now, Motion::Reduced);
    assert_eq!(
        reduced.flight(quarter, Motion::Reduced, None),
        0.0,
        "there is no slide to be in the middle of when the machine has been \
             asked not to animate"
    );
}

#[test]
fn a_flip_runs_the_displacement_down_to_nothing_on_the_grab_curve() {
    // K117/K118. One motion, the base span, cubic-bezier(.2, 0, 0, 1).
    let now = Instant::now();
    let mut flip = FlipTween::default();
    assert_eq!(flip.sample(now, Motion::Full), (0.0, false));
    flip.displace(-96.0, now, Motion::Full);
    let (start, moving) = flip.sample(now, Motion::Full);
    assert!((start + 96.0).abs() < 1e-3, "it starts where the tab was");
    assert!(moving);
    let (mid, moving) = flip.sample(now + Duration::from_millis(80), Motion::Full);
    assert!(moving);
    assert!(
        mid > -96.0 && mid < 0.0,
        "and travels the whole way in between"
    );
    assert!(
        mid.abs() < 48.0,
        "the curve leaves fast: half the time is well past half the distance"
    );
    // The mock-up writes `.16s` at 6570; the archive answers the base span,
    // because a tab sliding one slot along a row is one interaction (§7.18).
    assert_eq!(TAB_FLIP, bt_render::MOTION_BASE);
    assert!(
        flip.sample(now + TAB_FLIP - Duration::from_millis(1), Motion::Full)
            .1,
        "still moving one millisecond short of the end"
    );
    assert_eq!(
        flip.sample(now + TAB_FLIP, Motion::Full),
        (0.0, false),
        "and at the end the tab is simply in its slot"
    );
}

#[test]
fn the_landing_wash_runs_out_over_its_own_two_hundred_milliseconds() {
    // K121. Only the `from` is a design value, so only how much of it is left
    // is a state.
    let now = Instant::now();
    let mut landing = LandTween::default();
    assert_eq!(landing.sample(now, Motion::Full), (0.0, false));
    landing.start(now, Motion::Full);
    assert_eq!(landing.sample(now, Motion::Full), (1.0, true));
    let mut last = 1.0;
    for step in 1..20 {
        let (left, moving) = landing.sample(now + Duration::from_millis(step * 10), Motion::Full);
        assert!(left <= last, "the wash only ever fades");
        assert!(moving);
        last = left;
    }
    assert_eq!(
        TAB_LAND,
        Duration::from_millis(200),
        "`animation: tab-land .2s` (mock-up 967)"
    );
    assert!(
        landing
            .sample(now + Duration::from_millis(199), Motion::Full)
            .1
    );
    assert_eq!(
        landing.sample(now + Duration::from_millis(200), Motion::Full),
        (0.0, false)
    );
}

/// PIN — U8, R3. The counter-scale, stated as the three things it composes
/// to, in one frame.
///
/// The mock-up writes the animation as `scale(s)` on the pane and
/// `scale(1/s)` on an inner wrapper (6584-6586), and it needs both because
/// in CSS a transform is the only way to move a box that is already laid
/// out: the outer scale is the price of the movement and the inner one buys
/// back the text it stretched. Multiply the pair out and three separate
/// facts fall out, which are the three clauses below —
///
/// 1. the pane's **box** on the first frame is the box it left;
/// 2. the pane's **content origin** on that frame is the corner it left;
/// 3. the pane's **content extent** is already the one the solver just gave
///    it, on that same first frame.
///
/// The third is the one that fails a literal transcription of the CSS. Scale
/// this crate's viewport by `s` and clauses 1 and 2 still pass — the box and
/// the corner are right — while the grid inside it is drawn at the *old*
/// width and has to reflow toward the new one over 200ms, which is a resize
/// per frame handed to ConPTY (R2) and glyphs that visibly squeeze.
#[test]
fn the_first_frame_of_a_split_draws_the_old_box_around_contents_already_at_their_new_size() {
    let now = Instant::now();
    let (seats, before, after, survivor, _) = split_window(true);
    let mut motion = PaneMotion::default();
    motion.begin(
        &pane_rects_of(&before),
        &pane_rects_of(&after),
        now,
        Motion::Full,
    );

    let was = pane_box_of(&before, survivor);
    let is = pane_box_of(&after, survivor);
    assert!(
        (was[0] - is[0]).abs() > 1.0 && (was[2] - was[0]) > (is[2] - is[0]),
        "the survivor has to both move and narrow, or this pin proves \
             nothing: {was:?} -> {is:?}"
    );
    let transform = motion.transform_of(survivor, now, Motion::Full);

    // Clause 1 — the box.
    let box_now = transform.applied_to(is);
    for channel in 0..4 {
        assert!(
            (box_now[channel] - was[channel]).abs() < 1e-3,
            "the first frame is drawn through the box the pane left: \
                 {box_now:?} against {was:?}"
        );
    }

    let body =
        seats::pane_body_viewport(&seats, &after, survivor, 1.0).expect("the survivor has a body");
    let (viewport, _) = animated_pane_viewports(body, is, transform);

    // Clause 2 — the content's corner.
    assert_eq!(
        viewport.x as f32, was[0],
        "the contents are drawn from the corner the pane left, not from the \
             one the solver just gave it ({} against {})",
        viewport.x, is[0]
    );

    // Clause 3 — the content's extent, and the one a scaled viewport fails.
    assert_eq!(
        (viewport.width, viewport.height),
        (body.width, body.height),
        "the contents are already at the size the solver gave them on the \
             very first frame — nothing is ever scaled"
    );
    assert!(
        f32::from(u16::try_from(viewport.width).unwrap_or(u16::MAX)) < was[2] - was[0],
        "and that size really is different from the box's, so clause 3 is \
             not restating clause 1"
    );
}

/// PIN — U8, P177. The veil is the bottom-most thing the overlay carries.
///
/// The dock drawing is already documented as the lowest layer in the stack
/// ([`Runtime::dock_overlay_layers`]), so "under the dock" is "under
/// everything": the menus, the restore prompt, the settings dialog, the
/// layout peek, the tip and the drag ghost are all stacked after it by
/// [`Runtime::refresh_overlay`]. What this fails is the reasonable-looking
/// mistake of appending the veil to the stack that is already built, which
/// would put a pane's fade over an open dialog.
#[test]
fn the_arriving_panes_veil_is_the_bottom_most_overlay_layer() {
    let veil = marks::OverlayLayer {
        quads: vec![bt_render::OverlayQuad {
            rect: [0.0, 0.0, 10.0, 10.0],
            color: bt_render::chrome_palette().seat_body,
            alpha: 0.5,
        }],
        ..marks::OverlayLayer::default()
    };
    let dock = vec![marks::OverlayLayer {
        quads: vec![bt_render::OverlayQuad {
            rect: [20.0, 20.0, 30.0, 30.0],
            color: bt_render::chrome_palette().title_bar,
            alpha: 1.0,
        }],
        ..marks::OverlayLayer::default()
    }];

    let stacked = ground_overlay_layers(vec![veil.clone()], dock.clone().into());
    assert_eq!(
        stacked.layers.first(),
        Some(&veil),
        "the veil paints first and is therefore covered by everything after it"
    );
    assert_eq!(
        &stacked.layers[1..],
        dock.as_slice(),
        "and the dock is over it"
    );
    assert_eq!(
        ground_overlay_layers(Vec::new(), dock.clone().into()),
        marks::Band::from(dock),
        "with nothing arriving the stack is the one that was there before P177"
    );
}

/// PIN — B22. The resizing cards pull in over exactly a hundred milliseconds
/// on CSS `ease`, and draw nothing at all on the frame the grab lands.
///
/// `.pane { transition: margin .1s ease, border-radius .1s ease, box-shadow
/// .1s ease }` (mock-up 1464) serving `.slot.resizing .pane { margin: 5px;
/// border-radius: 8px }` (1465-1470). Three halves of that declaration are
/// load-bearing and each is pinned against the way it gets quietly lost: the
/// **span** (borrow the pane FLIP's 200ms and the cards are still arriving
/// after the seam has been dragged somewhere else), the **curve** (linear is
/// a different gesture — it starts at full speed, which reads as a snap that
/// then decelerates), and the **first frame** (both drawn numbers are floored
/// at one physical pixel, so a card scaled by zero is a hairline of floor
/// around a pane nobody is resizing).
#[test]
fn a_grabbed_dividers_cards_pull_in_over_a_hundred_milliseconds_on_ease() {
    let now = Instant::now();
    let mut cards = RevealTween::over(RESIZING_CARD_TRANSITION);
    cards.retarget(1.0, now, Motion::Full);

    assert_eq!(
        cards.sample(now, Motion::Full).0,
        0.0,
        "on the frame the button goes down the panes are still flush"
    );
    assert_eq!(
        seats::resizing_card_inset(1.0, cards.sample(now, Motion::Full).0),
        None,
        "and nothing is drawn for them — a one-pixel inset is a visible line \
             around a pane that has not moved"
    );

    let span = RESIZING_CARD_TRANSITION.as_millis() as u64;
    for ms in [20_u64, 45, 70] {
        let at = now + Duration::from_millis(ms);
        let (inset, moving) = cards.sample(at, Motion::Full);
        assert!(
            moving,
            "{ms}ms into a {span}ms transition it is still running"
        );
        let want = cubic_bezier(ms as f32 / span as f32, EASE);
        assert!(
            (inset - want).abs() < 1e-6,
            "{ms}ms in the inset is {inset} where CSS `ease` says {want}"
        );
        let (margin, radius) =
            seats::resizing_card_inset(1.0, inset).expect("a running card is a card");
        assert!(
            margin <= bt_render::SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX
                && radius <= bt_render::SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX,
            "and it never overshoots the declaration: {margin}, {radius}"
        );
    }

    // `ease` has a long flat tail, so the last frames of the transition round
    // to the very pixels it lands on — which is why the frame debt is settled
    // on the *drawn* inset. The frames that are genuinely part way in are the
    // early ones, and those are asserted rather than the tail.
    let (margin, radius) = seats::resizing_card_inset(
        1.0,
        cards
            .sample(now + Duration::from_millis(20), Motion::Full)
            .0,
    )
    .expect("a card twenty milliseconds in is a card");
    assert!(
        margin < bt_render::SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX
            && radius < bt_render::SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX,
        "twenty milliseconds in the card has not arrived: {margin}, {radius}"
    );

    let landed = now + RESIZING_CARD_TRANSITION;
    assert_eq!(
        cards.sample(landed, Motion::Full),
        (1.0, false),
        "a hundred milliseconds is the whole of it, and the loop may sleep"
    );
    assert_eq!(
        seats::resizing_card_inset(1.0, cards.sample(landed, Motion::Full).0),
        Some((
            bt_render::SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX,
            bt_render::SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX
        )),
        "landing on exactly the 5px and 8px F63 always drew"
    );
    assert_ne!(
        RESIZING_CARD_TRANSITION, PANE_FLIP,
        "a card inset answering a button press and half the window changing \
             shape cannot be one number"
    );
}

/// PIN — B22. A grab reversed mid-transition turns around from where the
/// cards actually are, not from an end they never reached.
///
/// The fourth user of [`RevealTween`] and the first one that is genuinely
/// interrupted in ordinary use: a hand that grabs a divider, thinks better of
/// it and lets go inside the hundred milliseconds. A CSS transition
/// interrupted mid-flight restarts from the computed value, which is what
/// `retarget` gives — restart from 1.0 instead and the cards jump *in* the
/// rest of the way before running out, which is a flinch rather than a
/// reversal.
#[test]
fn a_card_transition_reversed_mid_flight_turns_around_from_where_it_is() {
    let now = Instant::now();
    let mut cards = RevealTween::over(RESIZING_CARD_TRANSITION);
    cards.retarget(1.0, now, Motion::Full);

    let at = now + Duration::from_millis(40);
    let (reached, _) = cards.sample(at, Motion::Full);
    assert!(
        reached > 0.0 && reached < 1.0,
        "40ms of 100ms is genuinely mid-flight: {reached}"
    );

    cards.retarget(0.0, at, Motion::Full);
    assert!(
        (cards.sample(at, Motion::Full).0 - reached).abs() < 1e-6,
        "the reversal opens on the inset the cards had, {reached}, and not on \
             the 1.0 they were aimed at"
    );
    assert!(
        cards.sample(at + Duration::from_millis(10), Motion::Full).0 < reached,
        "and runs down from there"
    );
    assert_eq!(
        cards.sample(at + RESIZING_CARD_TRANSITION, Motion::Full),
        (0.0, false),
        "reaching flush a hundred milliseconds after the hand opened"
    );
}

/// PIN — B22 under reduced motion: the cards appear and vanish at once, and
/// no frame is owed for either.
///
/// Verified rather than assumed. "There is no transition under Reduced" is a
/// property of [`RevealTween::retarget`] storing no `started`, and the two
/// things that follow from it — the terminal inset on the first sample, and
/// a `false` that lets `strip_animation_work` ask for no deadline at all —
/// are each a separate consumer that could have read the clock for itself.
#[test]
fn reduced_motion_snaps_the_resizing_cards_in_and_out_and_asks_for_no_frames() {
    let now = Instant::now();
    let mut cards = RevealTween::over(RESIZING_CARD_TRANSITION);

    cards.retarget(1.0, now, Motion::Reduced);
    assert_eq!(
        cards.sample(now, Motion::Reduced),
        (1.0, false),
        "the cards are simply there, and nothing is asking to be woken"
    );
    assert_eq!(
        seats::resizing_card_inset(1.0, cards.sample(now, Motion::Reduced).0),
        Some((
            bt_render::SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX,
            bt_render::SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX
        )),
    );

    cards.retarget(0.0, now, Motion::Reduced);
    assert_eq!(
        cards.sample(now, Motion::Reduced),
        (0.0, false),
        "and simply gone"
    );
    assert_eq!(
        seats::resizing_card_inset(1.0, cards.sample(now, Motion::Reduced).0),
        None
    );
}

/// PIN — the profile picker's arrow turns over across 140ms on
/// `cubic-bezier(.2,0,0,1)`, and it is the turn that is drawn rather than
/// its two ends.
///
/// `.chevbtn svg { transition: transform 140ms cubic-bezier(.2,0,0,1) }`
/// (mock-up 415-418). Both halves of that declaration are load-bearing and
/// both are pinned here against the ways they get quietly deleted: cut the
/// duration to nothing and the arrow arrives before the first sample, so
/// there is no midpoint left to read; swap the curve for `linear` and the
/// midpoint lands at half a turn instead of where this curve actually puts
/// it, which — because `.2,0,0,1` front-loads almost everything and then
/// crawls — is nearly nine tenths of the way over.
#[test]
fn the_profile_chevron_turns_over_across_a_hundred_and_forty_milliseconds() {
    assert_eq!(
        CHEVRON_TURN,
        Duration::from_millis(140),
        "`transition: transform 140ms` (mock-up 417)"
    );
    assert_eq!(
        GRAB_EASE,
        [0.2, 0.0, 0.0, 1.0],
        "`cubic-bezier(.2,0,0,1)` (mock-up 417)"
    );

    let now = Instant::now();
    let mut turn = ChevronTurn::default();
    assert_eq!(
        turn.sample(now, Motion::Full),
        (0.0, false),
        "an untouched picker's arrow points down and is not moving"
    );

    turn.retarget(true, now, Motion::Full);
    assert_eq!(turn.sample(now, Motion::Full), (0.0, true));

    // The middle of the transition is a real place, and it is where this
    // curve puts it rather than where a straight line would.
    let (halfway, moving) = turn.sample(now + Duration::from_millis(70), Motion::Full);
    assert!(moving);
    assert!(
        halfway > 0.0 && halfway < 1.0,
        "70ms into a 140ms turn the arrow is partway over, saw {halfway}"
    );
    let eased = cubic_bezier(0.5, GRAB_EASE);
    assert!(
        (halfway - eased).abs() < 1e-3,
        "the turn is drawn on its own curve: expected {eased}, saw {halfway}"
    );
    assert!(
        (halfway - 0.5).abs() > 0.2,
        "halfway in time is not halfway over on this curve — saw {halfway}, \
             which is what `linear` would have given"
    );

    // It only ever goes forwards, and it stops.
    let mut last = 0.0;
    for step in 0..=14 {
        let (at, _) = turn.sample(now + Duration::from_millis(step * 10), Motion::Full);
        assert!(at >= last, "the arrow does not turn back on its way over");
        last = at;
    }
    assert!(
        turn.sample(now + Duration::from_millis(139), Motion::Full)
            .1
    );
    assert_eq!(
        turn.sample(now + Duration::from_millis(140), Motion::Full),
        (1.0, false),
        "at 140ms the arrow has arrived and owes no more frames"
    );
    assert_eq!(
        turn.sample(now + Duration::from_secs(9), Motion::Full),
        (1.0, false)
    );
}

/// PIN — a turn reversed mid-flight carries on from the angle the arrow is
/// actually at.
///
/// This is what a CSS transition does to a property whose target changes
/// while it is running, and it is the whole difference between a control
/// that turns and one that flickers: clicking the picker twice quickly must
/// not snap the arrow to an end it never reached and then run back from
/// there. Red gate: the sample taken at the instant of the reversal is the
/// same number on both sides of it.
#[test]
fn the_chevron_reverses_from_where_the_arrow_actually_is() {
    let now = Instant::now();
    let mut turn = ChevronTurn::default();
    turn.retarget(true, now, Motion::Full);

    let reversed_at = now + Duration::from_millis(40);
    let (mid, _) = turn.sample(reversed_at, Motion::Full);
    assert!(mid > 0.0 && mid < 1.0, "caught mid-turn, saw {mid}");

    turn.retarget(false, reversed_at, Motion::Full);
    let (restarted, moving) = turn.sample(reversed_at, Motion::Full);
    assert!(moving);
    assert!(
        (restarted - mid).abs() < 1e-6,
        "the arrow jumped from {mid} to {restarted} when it was told to come back"
    );

    // And from there it goes the other way, all the way home.
    let mut last = restarted;
    for step in 1..=14 {
        let (at, _) = turn.sample(reversed_at + Duration::from_millis(step * 10), Motion::Full);
        assert!(
            at <= last,
            "the reversed turn went further over instead of coming back"
        );
        last = at;
    }
    assert_eq!(
        turn.sample(reversed_at + CHEVRON_TURN, Motion::Full),
        (0.0, false)
    );

    // Told again what it is already doing, it does not restart: a caller
    // that re-reports the same state must not stretch the transition.
    let mut steady = ChevronTurn::default();
    steady.retarget(true, now, Motion::Full);
    let at = now + Duration::from_millis(100);
    let (before, _) = steady.sample(at, Motion::Full);
    steady.retarget(true, at, Motion::Full);
    assert_eq!(steady.sample(at, Motion::Full).0, before);
    assert_eq!(
        steady.sample(now + CHEVRON_TURN, Motion::Full),
        (1.0, false)
    );
}

/// PIN — `@media (prefers-reduced-motion: reduce) { .chevbtn svg {
/// transition: none } }` (mock-up 420).
///
/// `none` is not a faster transition: there are no intermediate frames at
/// all, the arrow is simply already over, and — the half that actually
/// costs something — nothing asks to be woken up to draw the frames that do
/// not exist.
#[test]
fn reduced_motion_turns_the_chevron_over_with_no_frames_in_between() {
    let now = Instant::now();
    let mut turn = ChevronTurn::default();
    turn.retarget(true, now, Motion::Reduced);
    assert_eq!(
        turn.sample(now, Motion::Reduced),
        (1.0, false),
        "under reduced motion the arrow is over the instant the list is up"
    );
    for step in 0..=14 {
        assert_eq!(
            turn.sample(now + Duration::from_millis(step * 10), Motion::Reduced),
            (1.0, false),
            "and there is never a frame of it on the way"
        );
    }
    turn.retarget(false, now + Duration::from_millis(50), Motion::Reduced);
    assert_eq!(
        turn.sample(now + Duration::from_millis(50), Motion::Reduced),
        (0.0, false)
    );
}

/// PIN — the turn pays the strip's frame debt in the mark's own quantized
/// angles, so it draws every step it has and stops the moment it lands.
///
/// Two failures this stands against, and they pull opposite ways. Compare
/// the raw fraction and every wake-up of the 140ms owes a present, including
/// the long tail where `.2,0,0,1` is crawling through less than a degree and
/// the rasterized arrow is byte-identical. Compare nothing at all and the
/// arrow strands on whatever frame the last present happened to catch —
/// which is the failure `tab_owes_frame` was written for, in its original
/// half-faded-icon form.
#[test]
fn the_chevron_s_frame_debt_is_paid_in_drawn_angles() {
    let now = Instant::now();
    let mut turn = ChevronTurn::default();
    turn.retarget(true, now, Motion::Full);

    // The strip wakes on its own beat; what it draws each time is the mark.
    let mut last_drawn: Option<marks::ChromeMark> = None;
    let mut presents = 0_u32;
    let mut wakes = 0_u32;
    let mut drawn = Vec::new();
    let mut at = now;
    loop {
        let (fraction, moving) = turn.sample(at, Motion::Full);
        let showing = marks::ChromeMark::chevron(fraction);
        if tab_owes_frame(last_drawn, showing) {
            last_drawn = Some(showing);
            presents += 1;
            drawn.push(showing);
        }
        wakes += 1;
        if !moving {
            break;
        }
        at += pace::DEFAULT_FRAME_INTERVAL;
    }

    assert!(
        wakes > presents,
        "every single wake-up presented a frame ({presents} of {wakes}) — the debt \
             is being measured on something finer than the arrow is drawn at"
    );
    assert!(
        presents >= 5,
        "only {presents} frames of the turn were ever drawn — that is a swap \
             wearing an animation's clothes"
    );
    assert!(
        presents <= u32::from(marks::CHEVRON_TURN_STEPS),
        "a single turn asked for {presents} rasters, more than the quantum allows"
    );
    assert_eq!(
        drawn.first().copied(),
        Some(marks::ChromeMark::chevron(0.0)),
        "the turn is drawn from the arrow's resting angle"
    );
    assert_eq!(
        drawn.last().copied(),
        Some(marks::ChromeMark::chevron(1.0)),
        "and the frame it settles on is the terminal one — an arrow left at 175° is \
             the stranded-mid-breath bug wearing a different mark"
    );
    for pair in drawn.windows(2) {
        assert_ne!(pair[0], pair[1], "a present that redrew the same angle");
    }
}

/// Which of them the float is asking for — hover, and the gesture that
/// outlives it.
#[test]
fn the_grip_keeps_its_arrow_for_the_whole_pull() {
    use float::FloatPart;
    assert_eq!(
        float_grasp(None, Some(FloatPart::Grip), true),
        Some(FloatGrasp::Grip),
        "hovering the grip is the whole bug: an arrow was showing where a resize lives"
    );
    assert_eq!(
        float_grasp(None, Some(FloatPart::Head), true),
        Some(FloatGrasp::Head)
    );
    for part in [
        FloatPart::Body,
        FloatPart::Foot,
        FloatPart::Dock,
        FloatPart::Close,
        FloatPart::Row(0),
    ] {
        assert_eq!(
            float_grasp(None, Some(part), true),
            None,
            "the rest of the window is an ordinary arrow"
        );
    }
    // The pull survives the pointer leaving the grip it began on — the
    // corner stops at the 200×150 floor while the hand keeps going.
    assert_eq!(
        float_grasp(Some(FloatDragKind::Resize), Some(FloatPart::Body), true),
        Some(FloatGrasp::Grip)
    );
    assert_eq!(
        float_grasp(Some(FloatDragKind::Move { grab: [0.0, 0.0] }), None, true),
        Some(FloatGrasp::Carrying)
    );
    // **A peek's header is a handle too** (user ruling 2026-08-27, §7.29).
    // The sentence that stood here — "a peek is not a window you were told
    // you had: its header is not a handle" — was overturned on the day the
    // gesture it denied became the product's way of keeping a glance: since
    // 2026-08-12 six pixels on that header promote the peek, and a header
    // that does that while advertising nothing is §7.21's complaint exactly.
    assert_eq!(
        float_grasp(None, Some(FloatPart::Head), false),
        Some(FloatGrasp::Head),
        "the one header that turns a glimpse into a window must look like one"
    );
    // The grip is still the pinned window's alone, and a peek is drawn
    // without one — so this is belt and braces rather than a rule with a
    // surface behind it.
    assert_eq!(float_grasp(None, Some(FloatPart::Grip), false), None);
    // **But the carry a peek's header turns into wears the closed fist**
    // (rule ③, 2026-08-12). The gesture is asked before the hover and before
    // `pinned`, which is what makes this true without a special case: by the
    // time the drag exists the window has been promoted anyway, and even a
    // frame where the two disagreed would be answered by the drag.
    assert_eq!(
        float_grasp(
            Some(FloatDragKind::Move { grab: [60.0, 12.0] }),
            Some(FloatPart::Head),
            false
        ),
        Some(FloatGrasp::Carrying),
        "a peek being carried off is a hand that has closed on something"
    );
}

/// User ruling 2026-08-12, which overturns `M2-tiny-window-priority.md`
/// §3.3: a float may stand on the rail and on the pane heads — the overlay
/// order already draws it over them — and may never stand on the caption,
/// whose buttons and drag band have to be reachable at every moment.
#[test]
fn a_float_may_stand_anywhere_in_the_window_except_the_title_bar() {
    let caption = |scale: f32| (bt_render::WINDOW_TITLE_BAR_LOGICAL_PX * scale).round();
    let rect = float_viewport_rect(2200, 1400, 2.0);
    assert_eq!(
        rect,
        [0.0, caption(2.0), 2200.0, 1400.0],
        "the rail's own column, the pane heads and every edge of the client \
             area are ground a float may cover; only the caption is removed"
    );
    assert_eq!(
        float_viewport_rect(800, 600, 1.0),
        [0.0, 40.0, 800.0, 600.0],
        "and the strip is the title bar's own 40 logical px, not a number of \
             this function's own"
    );
    // The floor under the tiny-window ladder: a client area shorter than its
    // own caption still answers with a rectangle that is the right way up,
    // because every clamp downstream subtracts from these two edges.
    let tiny = float_viewport_rect(120, 20, 2.0);
    assert!(
        tiny[1] < tiny[3],
        "a viewport must never come back inverted: {tiny:?}"
    );
}

/// The same question, asked of the host that is not in a tab.
///
/// A float has no seat layout and no tab, so the two facts left are the
/// master switch and the page the window was left on — and a window keeps
/// its page while the switch is off, exactly as a column does (§7.1.6g ②:
/// the setting decides reachability, the view records the choice).
#[test]
fn a_floating_window_shows_the_page_it_was_torn_off_on() {
    assert!(float_git_page_shown(true, seats::FilesView::Git));
    assert!(
        !float_git_page_shown(false, seats::FilesView::Git),
        "the master switch decides whether the page is reachable at all"
    );
    assert!(
        !float_git_page_shown(true, seats::FilesView::Files),
        "and a window torn off the tree is still on the tree"
    );
}

/// PIN — **the card's bar promises exactly as far as the card's clamp
/// allows** (user ruling, 2026-08-14: the glance scrolls).
///
/// The wheel over the card goes down `scroll_preview_body`, so the offset it
/// writes is clamped by `preview_document_max_scroll` — the one authority
/// every write of a preview offset already goes through. The *bar* is drawn
/// from `preview_document_height` instead, because a box that sizes itself
/// to its content asks the other question. Two numbers, two geometries, and
/// they have to be the same last pixel: a bar that promised further than the
/// clamp allows is a thumb that stops short of its own track, and one that
/// promised less is a document with a tail nothing can reach.
///
/// Asked of the three documents whose two answers are computed apart — the
/// mono body, the patch and the table, each out of its own geometry.
/// Markdown's pair is the same expression written twice in
/// `preview_document_height` and `preview_document_max_scroll`, which is
/// this same guarantee said in a way that cannot drift.
///
/// MUTATIONS that must turn it red:
/// ① drop the outer padding from `PreviewTableGeometry::document_height`
///    (`self.content_height` alone) — the csv's bar is two paddings short of
///    its clamp;
/// ② drop the `padding_y * 2.0` from `preview_mono_geometry`'s
///    `content_height` — text and diff both lose their air;
/// ③ have `file_peek::scroll_bar` measure the overflow against the card's
///    *frame* rather than its body — every case goes red by the head and the
///    foot together.
#[test]
fn the_glance_cards_bar_promises_exactly_as_far_as_its_clamp_allows() {
    let scale = 1.0_f32;
    let advance = 7.0_f32;
    let row = [40.0, 300.0, 240.0, 320.0];
    let window = (1200.0, 800.0);
    let probe = [
        0.0,
        0.0,
        file_peek::body_width(scale),
        file_peek::body_max_height(scale, true),
    ];

    let agree = |what: &str, document: &PreviewDocument, rows_height: f32, columns: usize| {
        let height = preview_document_height(document, probe, scale, advance, rows_height, columns);
        let card = file_peek::PeekContent {
            name: "sample".to_owned(),
            ftype: "text".to_owned(),
            dirty: false,
            meta: Some("3 KB".to_owned()),
            body: file_peek::PeekBody::Document(height),
        };
        let layout = file_peek::layout(
            &card,
            file_peek::PeekAnchor::row(row),
            window,
            60.0,
            24.0,
            scale,
        );
        let clamp = preview_document_max_scroll(
            document,
            layout.body,
            scale,
            advance,
            rows_height,
            columns,
        )[1];
        match preview_body_bar(
            layout.body,
            preview::ScrollAxis::Vertical,
            [0.0, 0.0],
            height,
            scale,
        ) {
            Some(bar) => {
                assert!(
                    clamp > 0.0,
                    "{what}: a bar was drawn over a document with nowhere to go"
                );
                assert!(
                    (bar.overflow - clamp).abs() < 0.5,
                    "{what}: the bar offers {} and the clamp allows {clamp}",
                    bar.overflow
                );
            }
            None => assert_eq!(
                clamp, 0.0,
                "{what}: no bar drawn, so there had better be nowhere to go"
            ),
        }
    };

    // ① Plain text — the mono body, forty lines in a card that holds a dozen.
    let lines: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
    let text_metrics = seats::preview_text_metrics(scale);
    agree(
        "text",
        &PreviewDocument::Text {
            wrap: preview_edit::WrapLayout::unwrapped(&lines),
            lines: lines.clone(),
            highlight: highlight::Highlighting::default(),
        },
        text_metrics.line_height * 40.0,
        0,
    );

    // ② A patch — the same mono geometry with the hunk margins in it.
    let patch = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n context\n-gone\n+here\n".repeat(8);
    let diff_metrics = seats::preview_diff_metrics(scale);
    let margin = (seats::PREVIEW_DIFF_HUNK_MARGIN_LOGICAL_PX * scale).round();
    let mut top = 0.0_f32;
    let diff_rows: Vec<DiffRow> = patch
        .lines()
        .map(|line| {
            let kind = preview::diff_line_kind(line);
            if kind == preview::DiffLineKind::Hunk {
                top += margin;
            }
            let row = DiffRow {
                text: preview::expand_tabs(line),
                kind,
                top,
            };
            top += diff_metrics.line_height;
            row
        })
        .collect();
    let diff_height = diff_rows
        .last()
        .map_or(0.0, |row| row.top + diff_metrics.line_height);
    agree("diff", &PreviewDocument::Diff(diff_rows), diff_height, 40);

    // ③ A csv — the table geometry, whose padding lives somewhere else
    //    entirely and is the one this most easily drifts on.
    let csv = "name,size\n".to_owned() + &"alpha,10\n".repeat(40);
    let rows = preview::csv_rows(&csv);
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let column_cells: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| bt_unicode::text_width(cell))
                .max()
                .unwrap_or(0)
        })
        .collect();
    agree(
        "table",
        &PreviewDocument::Table { rows, column_cells },
        0.0,
        0,
    );

    // ④ And a short file, which wears no bar and has nowhere to go — the
    //    other half of the same agreement.
    agree(
        "short text",
        &PreviewDocument::Text {
            wrap: preview_edit::WrapLayout::unwrapped(&lines[..2]),
            lines: lines[..2].to_vec(),
            highlight: highlight::Highlighting::default(),
        },
        text_metrics.line_height * 2.0,
        0,
    );
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

/// PIN (user ruling, 2026-08-13) — **a wide block is its own scrolling
/// region: it clamps at both ends, it takes the offset, and the prose beside
/// it does not move.**
///
/// The third assertion is the one the ruling is really about. The reported
/// symptom was two defects wearing one shape: a table that could not be
/// scrolled at all (the page's axis had been clamped to zero while the
/// table's width was still in the extent), and — before that — prose sliding
/// out of the pane to reach it.
///
/// MUTATIONS:
/// ① drop the `clamp(0.0, overflow)` in `build_preview_markdown_body` — the
///    first assertion goes red and a table can be pushed past its own end;
/// ② apply the offset to `left` for every block rather than the wide one —
///    the third assertion goes red, which is exactly the page-slide the
///    ruling overturned;
/// ③ stop pushing the block into `blocks` — the second assertion goes red and
///    the table prints over the paragraph beside it, unclipped.
#[test]
fn a_wide_markdown_block_scrolls_inside_itself_and_the_prose_stays_put() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 300.0, 400.0];
    let page = body[2] - body[0] - metrics.padding_x * 2.0;
    let wide = page + 400.0;
    let blocks = vec![
        preview::MarkdownBlock::Paragraph(vec![preview::Span::plain("prose")]),
        preview::MarkdownBlock::Code {
            lang: None,
            text: "a very long line".to_owned(),
        },
    ];
    let layout: preview_viewport::Layout = vec![
        MarkdownBlockLayout::solid(metrics.line_height),
        MarkdownBlockLayout {
            width: wide,
            top: metrics.line_height + metrics.paragraph_gap,
            ..MarkdownBlockLayout::solid(metrics.line_height * 3.0)
        },
    ]
    .into();
    let overflow = wide - page;
    let prose_left = |offsets: &[f32]| {
        let built = markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(offsets),
            (&blocks, &layout),
            &palette,
        );
        built
            .paragraphs
            .iter()
            .find(|paragraph| paragraph.runs.iter().any(|run| run.text == "prose"))
            .expect("the prose is drawn on the page itself")
            .rect[0]
    };
    let fence_left = |offsets: &[f32]| {
        let built = markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(offsets),
            (&blocks, &layout),
            &palette,
        );
        let block = built
            .blocks
            .first()
            .expect("a block wider than its page scrolls inside itself")
            .clone();
        // The fence's own ground, which is the second quad it draws.
        (block.quads[0].rect[0], block.clip)
    };

    // ① Clamped at both ends, in the block's own units.
    let (at_rest, clip) = fence_left(&[0.0, 0.0]);
    let (at_end, _) = fence_left(&[0.0, overflow]);
    let (past_end, _) = fence_left(&[0.0, overflow * 10.0]);
    let (before_start, _) = fence_left(&[0.0, -50.0]);
    assert_eq!(at_rest - at_end, overflow, "the block travels its overflow");
    assert_eq!(past_end, at_end, "and stops at its own end");
    assert_eq!(before_start, at_rest, "and at its own start");

    // ② The region is cropped to the block's rectangle, so what it scrolls
    //    cannot print over the page.
    assert_eq!(clip[0], body[0] + metrics.padding_x);
    assert_eq!(clip[2], clip[0] + page);

    // ②(b) **The indicator belongs to the block it was pushed with.** A page
    //    scrolled past its first wide block still has one region and one
    //    indicator, and the pair must be the same pair — the version that
    //    re-derived the list and zipped it drew every indicator one block out
    //    as soon as anything above had scrolled off the top.
    let tall: preview_viewport::Layout = vec![
        MarkdownBlockLayout::solid(1000.0),
        MarkdownBlockLayout {
            width: wide,
            top: 1000.0,
            ..MarkdownBlockLayout::solid(metrics.line_height * 3.0)
        },
    ]
    .into();
    let built = markdown_body(
        body,
        metrics,
        [0.0, 1000.0],
        rested_bars(&[0.0, overflow]),
        (&blocks, &tall),
        &palette,
    );
    let block = built.blocks.first().expect("the wide block is on screen");
    let thumb = block.quads.last().expect("an indicator has a thumb");
    assert!(
        thumb.rect[0] > block.clip[0],
        "the thumb of a block scrolled to its end sits away from the left edge"
    );

    // ③ **The prose does not move**, at any offset the block can hold.
    assert_eq!(prose_left(&[0.0, 0.0]), prose_left(&[0.0, overflow]));
}

/// RED — **a picture pane that has been answered does not ask again when the
/// decode store lets its pixels go** (adversarial review 2026-09-11, row
/// RB-1; `docs/DESIGN.md` §7.1.3u ③).
///
/// RED EVIDENCE. §7.1.3u taught a markdown *page* that a miss in a bounded
/// cache is not "never asked". The standalone picture pane was the consumer
/// that fix did not touch: `refit_preview_picture` consulted
/// [`PeekCache`] and nothing else, and a miss fell straight through to
/// hiding the picture, asking for the file and filing a `Pending` — even
/// though the pane was standing on a `PeekThumbnail` of its own, made from
/// that very decode, and drawing it. With a visible working set over
/// [`MAX_PEEK_CACHE_BYTES`] each arrival evicts a picture another host is
/// drawing, the refit that arrival triggers finds the miss, asks again, and
/// the cycle sustains itself with no input at all.
///
/// MUTATION: make [`surface_pixels`] read a miss as
/// [`SurfacePixels::Nothing`] again — ignore the `standing` argument — and
/// the errand at the size the pane is already holding becomes
/// [`PictureErrand::Read`], which is the first turn of the loop.
#[test]
fn a_pane_whose_picture_was_evicted_does_not_ask_again() {
    const PIXELS: usize = 3 * 1024 * 1024;
    const BUDGET: u64 = 4 * 1024 * 1024;
    const NATIVE: (u32, u32) = (1024, 768);
    let paths = [
        PathBuf::from(r"D:\shots\a.png"),
        PathBuf::from(r"D:\shots\b.png"),
    ];
    let keys = paths
        .iter()
        .map(|path| bt_term::normalized_local_image_path_key(path))
        .collect::<Vec<_>>();
    let mut peek = PeekCache::with_budget(BUDGET);
    peek.insert(keys[0].clone(), a_decode("content-a", NATIVE, PIXELS));
    // The pane resampled that decode to the box it stands in, and holds the
    // answer: this is what is on the glass.
    let held = a_held_raster("content-a", (500, 375));
    let target: PeekThumbnailTarget = ("content-a".to_owned(), 500, 375);

    // A neighbouring surface's decode lands, and this one's is what the
    // bounded store lets go of to make room.
    peek.insert(keys[1].clone(), a_decode("content-b", NATIVE, PIXELS));
    assert!(
        peek.get(&keys[0]).is_none(),
        "the fixture is a store that cannot hold both decodes at once"
    );

    let pixels = surface_pixels(
        &mut peek,
        Some(held.content_key.as_str()),
        Some(NATIVE),
        None,
        &keys[0],
    );
    assert!(
        matches!(pixels, SurfacePixels::Standing { .. }),
        "the pane was answered once and is still drawing that answer: {pixels:?}"
    );
    assert_eq!(
        picture_errand(&pixels, held.matches(&target)),
        PictureErrand::Nothing,
        "a surface holding the very raster this frame wants has nothing to \
             ask anybody — a miss in a bounded cache is not 'never asked'",
    );

    // And the one case that *is* a question: the pane is made wider, so the
    // raster it holds is not the raster it wants, and the pixels a sharper
    // pass would be made from are not in this window.
    let wider: PeekThumbnailTarget = ("content-a".to_owned(), 700, 525);
    assert_eq!(
        picture_errand(&pixels, held.matches(&wider)),
        PictureErrand::Read,
        "a size it does not hold, with nothing to resample from, is one read"
    );
    peek.insert(keys[0].clone(), PeekCacheEntry::Pending);
    let asked = surface_pixels(
        &mut peek,
        Some(held.content_key.as_str()),
        Some(NATIVE),
        None,
        &keys[0],
    );
    assert_eq!(
        picture_errand(&asked, held.matches(&wider)),
        PictureErrand::Wait,
        "and the store's own `Pending` is what keeps it to one read"
    );
}

/// RED — **more pictures on the glass than the decode store can hold still
/// settles** (adversarial review 2026-09-11, row RB-1).
///
/// RED EVIDENCE. The loop above is only visible at more than one surface: a
/// decode landing for pane A evicts pane B's, the refit that the arrival
/// triggers walks **every** picture host ([`Runtime::refresh_preview_for_layout`]),
/// B finds its miss and asks, and B's answer evicts C's. Four 4000×4000 PNGs
/// are 256 MiB against a 192 MiB store, which is four screenshots opened at
/// once.
///
/// The fixture is that story at the size a test can hold: four surfaces,
/// each on its own file, a store that can carry one decode, and one decode
/// landing per round the way `complete_peek_image` lands them. The claim is
/// the count — **one read per surface, for ever**.
///
/// MUTATION: the same one. Read a miss as "never asked" and the count climbs
/// by one per surface per round, which is the report.
#[test]
fn a_visible_working_set_over_the_cache_settles() {
    const PIXELS: usize = 3 * 1024 * 1024;
    const BUDGET: u64 = 4 * 1024 * 1024;
    const NATIVE: (u32, u32) = (1024, 768);
    const DRAWN: (u32, u32) = (500, 375);
    const ROUNDS: usize = 12;
    let paths: Vec<PathBuf> = (0..4)
        .map(|index| PathBuf::from(format!(r"D:\shots\{index}.png")))
        .collect();
    let keys: Vec<String> = paths
        .iter()
        .map(|path| bt_term::normalized_local_image_path_key(path))
        .collect();
    let content = |index: usize| format!("content-{index}");
    let mut peek = PeekCache::with_budget(BUDGET);
    // What each surface is holding, and what it wants: the box is the same
    // every round, so the size it wants is the same every round.
    let mut held: Vec<Option<PeekThumbnail>> = paths.iter().map(|_| None).collect();
    let mut native: Vec<Option<(u32, u32)>> = vec![None; paths.len()];
    let mut asked = 0_usize;
    let mut inbox: Vec<usize> = Vec::new();

    for _ in 0..ROUNDS {
        if !inbox.is_empty() {
            let index = inbox.remove(0);
            peek.insert(
                keys[index].clone(),
                a_decode(&content(index), NATIVE, PIXELS),
            );
        }
        for index in 0..paths.len() {
            let pixels = surface_pixels(
                &mut peek,
                held[index]
                    .as_ref()
                    .map(|raster| raster.content_key.as_str()),
                native[index],
                None,
                &keys[index],
            );
            // `refit_preview_picture`'s own order: a surface with nothing to
            // draw at all never reaches the errand — it hides the picture,
            // asks, and is done for this frame.
            if let SurfacePixels::Nothing { asked: already } = pixels {
                if !already {
                    asked += 1;
                    inbox.push(index);
                    peek.insert(keys[index].clone(), PeekCacheEntry::Pending);
                }
                continue;
            }
            let target: PeekThumbnailTarget = (content(index), DRAWN.0, DRAWN.1);
            let exact = held[index]
                .as_ref()
                .is_some_and(|raster| raster.matches(&target));
            match picture_errand(&pixels, exact) {
                PictureErrand::Nothing | PictureErrand::Wait => {}
                PictureErrand::Read => {
                    asked += 1;
                    inbox.push(index);
                    peek.insert(keys[index].clone(), PeekCacheEntry::Pending);
                }
                // The scale lane answers, the way it answers on the window:
                // the surface is handed the raster and holds it.
                PictureErrand::Resample => {
                    held[index] = Some(a_held_raster(&content(index), DRAWN));
                    native[index] = Some(NATIVE);
                }
            }
        }
    }
    assert_eq!(
        asked,
        paths.len(),
        "{asked} reads for {} pictures over {ROUNDS} rounds: the store let \
             one go and the surface drawing it took that for never having asked",
        paths.len(),
    );
    for (index, raster) in held.iter().enumerate() {
        assert!(
            raster.is_some(),
            "picture {index} went blank when the store let its pixels go"
        );
    }
}

/// RED — **a page keeps the formulas it was handed when the formula cache
/// lets them go** (adversarial review 2026-09-11, row RB-2; §7.1.3u ③).
///
/// RED EVIDENCE. [`PreviewMathCache`] is bounded in bytes and evicts by last
/// use; `resolve_document_math` had no standing-answer parameter, unlike its
/// twin for pictures, so it asked the engine for every key the cache had no
/// entry for. A page whose distinct rasters are worth more than
/// [`PREVIEW_MATH_CACHE_BUDGET_BYTES`] therefore evicted one it was drawing
/// on every arrival and typeset it again — §7.1.3u's own report, one lane
/// over. It is reachable because the cache is window-wide and the size is
/// part of the key, so each preview zoom step mints a fresh set of every
/// formula on the page.
///
/// MUTATION: drop the standing arm from [`answer_one_formula`]'s `None`
/// branch and the evicted formula is asked for again, which is the first
/// turn of the loop.
#[test]
fn a_page_keeps_its_formula_answers_across_an_eviction() {
    /// Three of these do not fit under the budget; two do.
    const BYTES: usize = 20 * 1024 * 1024;
    let ink = [17_u8, 18, 19];
    let key = |source: &str| PreviewMathKey {
        source: source.to_owned(),
        mode: MathMode::Display,
        em_milli_px: math_em_milli(13.0),
        foreground_rgb: ink,
    };
    let mut cache = PreviewMathCache::default();
    let mut page = DocumentMath::default();
    let sources = ["a", "b", "c"];
    for source in sources {
        let picture = PreviewMathPicture {
            key: format!("preview-math:{source}"),
            rgba: Arc::from(vec![0_u8; BYTES].into_boxed_slice()),
            width_px: 100,
            height_px: 50,
            baseline_px: 10.0,
        };
        cache.land(key(source), PreviewMathArtifact::Ready(picture.clone()));
        // What the page was handed, which is what it is drawing.
        page.insert(&key(source), picture);
    }
    assert_eq!(
        cache.entries.len(),
        2,
        "the fixture is a cache that cannot hold the page: {} bytes resident",
        cache.resident_bytes,
    );
    let gone = sources
        .iter()
        .find(|source| !cache.entries.contains_key(&key(source)))
        .expect("the budget let one of them go");

    let mut needs_typesetting = false;
    let answer = answer_one_formula(&mut cache, &page, &key(gone), &mut needs_typesetting);
    assert!(
        answer.is_some(),
        "the page was told what `{gone}` looks like and is still drawing it"
    );
    assert!(
        !needs_typesetting,
        "but it asked the engine to set `{gone}` again, because the cache \
             no longer holds the pixels it was given"
    );

    // And the one thing a standing answer may not survive: the page being
    // set in another ink, which is a different picture of the same formula.
    let mut in_another_theme = key(gone);
    in_another_theme.foreground_rgb = [200, 200, 200];
    let mut needs_typesetting = false;
    let answer = answer_one_formula(&mut cache, &page, &in_another_theme, &mut needs_typesetting);
    assert!(
        answer.is_none() && needs_typesetting,
        "a formula set in a new ink is a new picture"
    );
}

/// RED — **an eviction is not an invalidation of the documents that still
/// hold the answer** (adversarial review 2026-09-11, row RB-2).
///
/// RED EVIDENCE. `PreviewMathCache::evict_to_budget` bumped `generation`,
/// which is part of [`PageArtKey`] and therefore re-keys every page in the
/// window — a full re-flow each. That was right while the cache was a page's
/// only copy of a formula: the block went back to standing on its source
/// text. It is exactly wrong once a page carries what it was handed, and it
/// is the engine of the loop: land → generation++ → rebuild → miss on the
/// key just evicted → ask → land → evict the next.
///
/// MUTATION: tick the generation in `evict_to_budget` again and the count
/// below is one higher per eviction, which is one whole-document re-flow per
/// eviction for a page that has not changed.
#[test]
fn an_eviction_does_not_invalidate_a_document_that_still_holds_the_answer() {
    const BYTES: usize = 20 * 1024 * 1024;
    let mut cache = PreviewMathCache::default();
    for source in ["a", "b", "c"] {
        cache.land(
            PreviewMathKey {
                source: source.to_owned(),
                mode: MathMode::Display,
                em_milli_px: math_em_milli(13.0),
                foreground_rgb: [0, 0, 0],
            },
            PreviewMathArtifact::Ready(PreviewMathPicture {
                key: format!("preview-math:{source}"),
                rgba: Arc::from(vec![0_u8; BYTES].into_boxed_slice()),
                width_px: 100,
                height_px: 50,
                baseline_px: 10.0,
            }),
        );
    }
    assert_eq!(
        cache.entries.len(),
        2,
        "the fixture is a cache that had to let one go"
    );
    assert_eq!(
        cache.generation, 3,
        "one tick per formula that arrived, and none at all for the \
             eviction: a page still holding the answer has not changed, and \
             re-keying it is a re-shape of every paragraph on it for nothing",
    );
}

/// RED GATE (found on the glass, 2026-08-28) — **a theme flip asks every
/// markdown page to lay out again.**
///
/// Two things on a page do not recolour for free: a formula's raster came
/// out of the engine already inked, and a `<picture>` names one file for a
/// dark page and another for a light one. Both are in [`PageArtKey`]
/// *precisely* so a theme flip is a different layout question — and until
/// this line nothing asked the question. Measured in the real window: the
/// chrome went light and the page kept the dark hero and the dark
/// screenshot, indefinitely.
///
/// It is a source gate rather than a behavioural one because the subject is
/// a `Runtime` method that needs a window, a GPU and a palette; what is
/// load-bearing is that the *call* is in the one function every theme change
/// goes through, beside the sibling cache-clear that was written for the
/// same failure one surface over.
///
/// MUTATION: delete the `refresh_preview_for_layout()` call from
/// `adopt_new_palette` and this goes red.
#[test]
fn a_theme_flip_asks_every_markdown_page_to_lay_out_again() {
    let body = method_body("Runtime", "adopt_new_palette");
    assert!(
        body.contains("self.refresh_preview_for_layout();"),
        "a page whose pictures and formulas are inked by the theme has to be \
             asked again: {body}",
    );
    // And the rail's own clear is still there beside it, because the two are
    // the same sentence about two surfaces.
    assert!(body.contains("cache.clear();"));
}

/// RED (0.4.4 ticket 09) — **every palette change tells this window's web pages their colour
/// scheme, the `Web pages` row goes through that same door, and a page is told at birth.**
///
/// `webhost::color_scheme_tests` holds the rule and the walk over a window's seats; this holds
/// the three places the window reaches them. `adopt_new_palette` is the one function every theme
/// flip, scheme swap and contrast floor goes through in every window (`adopt_application_change`
/// runs it on the others), so a page told from there cannot be left behind by any of them.
///
/// MUTATION: delete the `tell_web_pages_their_color_scheme` call from `adopt_new_palette`, or
/// open a seat without `web_color_scheme_in_force`, and this goes red.
#[test]
fn a_theme_flip_tells_every_web_page_its_colour_scheme() {
    let adopt = method_body("Runtime", "adopt_new_palette");
    assert!(
        adopt.contains("self.tell_web_pages_their_color_scheme();"),
        "a palette change reaches the pages: {adopt}"
    );
    let row = method_body("Runtime", "apply_web_color_scheme");
    assert!(
        row.contains("self.adopt_new_palette()?;"),
        "the row takes the palette's door: {row}"
    );
    let open = method_body("Runtime", "open_web_page_on");
    assert!(
        open.contains("self.web_color_scheme_in_force(),"),
        "a seat is told before its engine is asked for: {open}"
    );
    let tell = method_body("Runtime", "tell_web_pages_their_color_scheme");
    assert!(
        tell.contains("tell_all_seat_its_color_scheme(self.window.web.values_mut(), scheme)"),
        "every seat of the window, every tab: {tell}"
    );
}

/// PIN (same report) — **every formula on a page reaches the engine, from
/// wherever on the page it stands.**
///
/// A formula is a run of inline text, and a run of inline text can be in a
/// heading, a list item, a quoted line or a table cell as easily as in a
/// paragraph — there is one inline parser and it serves all of them. A walk
/// that only looked at paragraphs would leave every other one standing on its
/// source for good, with nobody ever asking for it. A fence is the one place
/// a dollar is never markup, and it is excluded here as well as by the parser.
///
/// MUTATION: drop any arm of `document_formulas` and its member goes missing.
#[test]
fn every_formula_on_the_page_is_asked_for_wherever_it_stands() {
    let blocks = preview::parse_markdown(
        "# The $\\alpha$ chapter\n\
             \n\
             Prose with $x^2$ in it.\n\
             \n\
             - an item with $y^2$\n\
             \n\
             > a quote with $z^2$\n\
             \n\
             | a | b |\n\
             |---|---|\n\
             | $c^2$ | plain |\n\
             \n\
             $$\n\
             \\int_0^1 f\n\
             $$\n\
             \n\
             ```text\n\
             $not^2$\n\
             ```\n",
    );
    let metrics = seats::preview_markdown_metrics(1.0);
    let mut found = document_formulas(&blocks, metrics)
        .into_iter()
        .map(|(source, mode, em_px)| {
            (
                source,
                matches!(mode, MathMode::Display),
                math_em_milli(em_px),
            )
        })
        .collect::<Vec<_>>();
    found.sort();
    let body = math_em_milli(metrics.font_size);
    assert_eq!(
        found,
        vec![
            (
                "\\alpha".to_owned(),
                false,
                math_em_milli(metrics.heading_font(1))
            ),
            ("\\int_0^1 f".to_owned(), true, body),
            ("c^2".to_owned(), false, body),
            ("x^2".to_owned(), false, body),
            ("y^2".to_owned(), false, body),
            ("z^2".to_owned(), false, body),
        ],
        "five inline formulas, one display block, nothing from the fence — and \
             the one in the masthead asked for at the masthead's size",
    );
}

/// **A drag through a table and the paragraph under it bands every piece
/// between them, and only the parts of them it reached.**
///
/// The bands are the shaper's, so what is under test here is the arithmetic
/// that decides *which* bytes of each piece to ask about — the first piece
/// from the offset the drag began at, the last up to where it is now, and
/// every piece in between whole.
///
/// MUTATION: clamp a middle piece to the head's offset instead of its own
/// length and the table's second row loses its tail.
#[test]
fn a_drag_bands_the_first_piece_from_its_offset_and_the_ones_between_whole() {
    let boxes = vec![
        text_box(preview_select::Place::new(0, 0, 0), 5, "alpha"),
        text_box(preview_select::Place::new(0, 1, 0), 4, "beta"),
        text_box(preview_select::Place::new(1, 0, 0), 5, "gamma"),
    ];
    let mut asked: Vec<(String, Range<usize>)> = Vec::new();
    let bands = preview_selection_bands(
        &boxes,
        preview_select::Place::new(0, 0, 2),
        preview_select::Place::new(1, 0, 3),
        &mut |paragraph, range| {
            asked.push((bt_render::preview_paragraph_text(paragraph), range.clone()));
            vec![[range.start as f32, 0.0, range.end as f32, 1.0]]
        },
    );
    assert_eq!(
        asked,
        vec![
            ("alpha".to_owned(), 2..5),
            ("beta".to_owned(), 0..4),
            ("gamma".to_owned(), 0..3),
        ],
        "the head's piece from where the hand went down, the tail's up to \
             where it is, and everything between whole",
    );
    assert_eq!(bands.len(), 3);
}

/// **A formula is one band whatever of it was touched** (the atom rule).
///
/// MUTATION: drop the `atomic` arm of `PreviewTextPiece::shaped_range` and a
/// drag that clips a formula's corner bands two characters of a picture.
#[test]
fn a_formula_is_banded_whole_however_little_of_it_was_touched() {
    let mut formula = text_box(preview_select::Place::new(1, 0, 0), 13, "$$a+b$$");
    formula.piece.atomic = true;
    let boxes = vec![
        text_box(preview_select::Place::new(0, 0, 0), 5, "alpha"),
        formula,
    ];
    let mut asked: Vec<Range<usize>> = Vec::new();
    preview_selection_bands(
        &boxes,
        preview_select::Place::new(0, 0, 5),
        preview_select::Place::new(1, 0, 1),
        &mut |_, range| {
            asked.push(range);
            Vec::new()
        },
    );
    assert_eq!(
        asked,
        vec![0..7],
        "one byte of the formula asked for takes the whole of what is drawn \
             for it, and the piece before it contributed nothing",
    );
}

/// **A picture standing for a formula bands as its own rectangle**, because
/// there is no paragraph under it to ask the shaper about.
#[test]
fn a_rendered_formulas_picture_bands_as_the_box_it_was_drawn_in() {
    let picture = PreviewTextBox {
        piece: PreviewTextPiece {
            at: preview_select::Place::new(0, 0, 0),
            len: 9,
            lead: 0,
            atoms: Vec::new(),
            atomic: true,
        },
        rect: [10.0, 20.0, 130.0, 60.0],
        clip: [0.0, 0.0, 400.0, 400.0],
        paragraph: None,
    };
    let bands = preview_selection_bands(
        &[picture],
        preview_select::Place::new(0, 0, 0),
        preview_select::Place::new(0, 0, 9),
        &mut |_, _| panic!("a picture has no paragraph to shape"),
    );
    assert_eq!(bands, vec![[10.0, 20.0, 130.0, 60.0]]);
}

/// **A band is cropped to the window its piece is seen through**, so a table
/// scrolled sideways inside itself does not paint a highlight over the prose
/// beside it.
#[test]
fn a_band_is_cropped_to_the_window_its_own_block_is_seen_through() {
    let mut cell = text_box(preview_select::Place::new(0, 0, 0), 5, "alpha");
    cell.clip = [100.0, 0.0, 200.0, 50.0];
    let bands = preview_selection_bands(
        &[cell],
        preview_select::Place::new(0, 0, 0),
        preview_select::Place::new(0, 0, 5),
        &mut |_, _| vec![[50.0, 10.0, 150.0, 30.0]],
    );
    assert_eq!(
        bands,
        vec![[100.0, 10.0, 150.0, 30.0]],
        "the half of the band outside the block's window is not drawn",
    );
}

/// **Where a pointer lands when it is not on any letter at all.**
///
/// A drag does not stay inside the column of prose it started in: it goes
/// out into the margin, above the first block and below the last, and every
/// one of those has to be an answer or the selection stops growing at the
/// edge of the text.
///
/// MUTATION: return `None` for a point outside every rectangle and a drag
/// out of the pane freezes.
#[test]
fn a_point_off_the_text_lands_on_the_piece_it_is_nearest_to() {
    let boxes = vec![
        row_box(preview_select::Place::new(0, 0, 0), 5, "alpha", 0.0, 20.0),
        row_box(preview_select::Place::new(1, 0, 0), 4, "beta", 40.0, 60.0),
    ];
    assert_eq!(preview_text_box_at(&boxes, 30.0, 10.0), Some(0), "on it");
    assert_eq!(
        preview_text_box_at(&boxes, 300.0, 10.0),
        Some(0),
        "out in the margin beside it",
    );
    assert_eq!(
        preview_text_box_at(&boxes, 30.0, -50.0),
        Some(0),
        "above the whole page",
    );
    assert_eq!(
        preview_text_box_at(&boxes, 30.0, 500.0),
        Some(1),
        "below the whole page",
    );
    assert_eq!(
        preview_text_box_at(&boxes, 30.0, 31.0),
        Some(1),
        "in the gap between two blocks, on the nearer one",
    );
    assert_eq!(preview_text_box_at(&[], 0.0, 0.0), None, "an empty page");
    // Two cells side by side on one row: the gap between them belongs to the
    // one it is nearer to, not to whichever was laid out first.
    let mut left = row_box(preview_select::Place::new(2, 0, 0), 4, "name", 0.0, 20.0);
    left.rect = [0.0, 0.0, 100.0, 20.0];
    let mut right = row_box(preview_select::Place::new(2, 1, 0), 4, "size", 0.0, 20.0);
    right.rect = [140.0, 0.0, 240.0, 20.0];
    let row = vec![left, right];
    assert_eq!(preview_text_box_at(&row, 110.0, 10.0), Some(0));
    assert_eq!(preview_text_box_at(&row, 135.0, 10.0), Some(1));
    assert_eq!(preview_text_box_at(&row, 400.0, 10.0), Some(1));
}

/// **A document that has been re-read leaves no selection standing** (the
/// file changed on disk; the offsets are about text that is gone) — **and a
/// document re-parsed because the reader typed into it leaves both marks
/// exactly where they were** (§7.1.3q; research open question 15, "the
/// single most dangerous line in T4").
///
/// The two halves are one test because the danger is in the *difference*:
/// the clearing rule was written when the only thing that could replace a
/// parse was the disk, and a live preview re-parses on every keystroke. A
/// rule that could not tell the two apart would either highlight somebody
/// else's sentence or drop the reader's own selection every time they typed
/// beside it.
///
/// MUTATION ①: assign `pane.doc` directly at the refresh and a selection made
/// before a save goes on being drawn over whatever replaced it.
/// MUTATION ②: clear on both arms and a selection cannot survive being typed
/// next to; keep on both and an external save keeps a highlight over text
/// that is gone.
#[test]
fn a_freshly_parsed_document_leaves_no_selection_standing() {
    let marked = || PreviewPane {
        md_select: Some(preview_select::Selection::collapsed(
            preview_select::Place::new(3, 1, 4),
            preview_select::Grain::Character,
        )),
        md_text: vec![text_box(
            preview_select::Place::new(3, 1, 0),
            9,
            "somewhere",
        )],
        caret: preview_edit::EditCaret {
            anchor: 12,
            caret: 20,
            desired_column: None,
            desired_x: None,
        },
        ..PreviewPane::default()
    };
    // "new\n\nlines\n" — two paragraphs and the blank line between them,
    // which belongs to neither of them (§7.1.3o).
    let parsed = || PreviewDocument::Markdown {
        blocks: vec![
            preview::MarkdownBlock::Paragraph(vec![preview::Span::plain("new")]),
            preview::MarkdownBlock::Paragraph(vec![preview::Span::plain("lines")]),
        ],
        ranges: vec![0..4, 5..11],
        maps: Vec::new(),
        source: SourceBlocks::default(),
        intrinsic: Vec::new(),
        layout: preview_viewport::Layout::default(),
        math: DocumentMath::default(),
        pictures: DocumentPictures::default(),
        wrap: Arc::default(),
    };

    let mut disk = marked();
    disk.show_document(parsed(), Reparse::Elsewhere);
    assert_eq!(disk.md_select, None, "the selection went with the document");
    assert!(
        disk.md_text.is_empty(),
        "and so did the boxes it was drawn in"
    );
    assert_eq!(
        (disk.caret.anchor, disk.caret.caret),
        (20, 20),
        "and what the caret had dragged over went with it — but not the \
             caret, which is a byte offset the buffer's own doors heal and a \
             session restores before the file it belongs to has even landed",
    );

    let mut ours = marked();
    ours.show_document(parsed(), Reparse::Ours);
    assert!(
        ours.md_select.is_some(),
        "a keystroke is not a stranger's save: what was marked is still \
             about the words it was about",
    );
    assert_eq!(
        (ours.caret.anchor, ours.caret.caret),
        (12, 20),
        "and the caret keeps both its ends",
    );
    assert!(
        ours.md_text.is_empty(),
        "the boxes go whatever happened: they are where the *last* document \
             was drawn and nothing has drawn this one",
    );
}

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

/// **The caret the painter strikes, the cell the IME hangs from and the
/// column the editor counts are one number** (user report, 2026-09-11).
///
/// The report was a caret standing a gap to the right of the character it
/// was editing on a line of Chinese, and the gap grew with every ideograph
/// before it: the letters were shaped by a fallback face whose advances are
/// its own, while the caret was counted in cells. The letters are placed on
/// the cells now ([`bt_render::PreviewParagraph::cell_advance`]) and this
/// holds the other three readings to the same arithmetic.
///
/// MUTATION: derive the IME's rectangle from anything but
/// [`markdown_source_cell`] and a candidate list stands beside the caret it
/// claims to follow.
#[test]
fn the_caret_the_ime_and_the_column_are_one_arithmetic_on_a_chinese_line() {
    let line = "网页预览需要 WebView2。";
    let source = source_block(0, 0, line);
    let box_of_block = [100.0, 40.0, 500.0, 60.0];
    let palette = bt_render::chrome_palette();
    let paint = |caret: usize| {
        let column = preview_edit::column_of(line, caret);
        let mut quads = Vec::new();
        let mut paragraphs = Vec::new();
        push_markdown_source_block(
            (&mut quads, &mut paragraphs),
            &source,
            Some(&MarkdownCaretPaint {
                seat: MarkdownCaretSeat::Source {
                    block: source.index,
                    line: 0,
                    column,
                },
                lit: true,
                selection: 0..0,
                band: 0..0,
                caret_width: 2.0,
                preedit: None,
            }),
            &highlight::Highlighting::plain(),
            box_of_block,
            [0.0, 0.0, 1000.0, 1000.0],
            &palette,
        );
        let [quad] = quads.as_slice() else {
            panic!("one caret: {quads:#?}");
        };
        (column, quad.rect[0])
    };
    // Every seam of the line, in the file's own bytes: before each cluster
    // and after the last one.
    let mut byte = 0usize;
    for cluster in bt_unicode::graphemes(line) {
        let (column, drawn) = paint(byte);
        assert!(
            (drawn - (box_of_block[0] + 8.0 * column as f32)).abs() < f32::EPSILON,
            "the caret in front of {cluster:?} is struck at {drawn}, not on its \
                 column {column}",
        );
        assert!(
            (markdown_source_cell(&source, box_of_block, 0, column)[0] - drawn).abs()
                < f32::EPSILON,
            "and the cell the IME hangs its candidates from is the same one",
        );
        // The press that would put the caret there agrees as well, which is
        // the third reading of the one grid.
        assert_eq!(
            markdown_source_offset_at(&source, box_of_block, drawn, 44.0),
            byte,
            "a press on the caret's own x names the byte the caret is at",
        );
        byte += cluster.len();
    }
    let (column, _) = paint(line.len());
    assert_eq!(
        column,
        bt_unicode::text_width(line),
        "the last seam is the whole line's width in cells, ideographs counted \
             as the two they draw as",
    );
}

/// **A Chinese line folds where the wrap says it folds** (user report,
/// 2026-09-11).
///
/// Chinese has no spaces, so every character is a break opportunity and a
/// fold lands *between two ideographs* — which is only safe if the rows the
/// painter draws are cut at the columns the fold was computed in, and if the
/// letters then stand on those very cells
/// ([`bt_render::PreviewParagraph::cell_advance`]).
#[test]
fn a_chinese_line_folds_at_the_columns_the_wrap_names() {
    let line = "这一段完全没有空格";
    let source = source_block(0, 0, line);
    // A ten-cell-wide block: five ideographs a row.
    let box_of_block = [100.0, 40.0, 180.0, 100.0];
    let wrap = source.wrap(box_of_block[2] - box_of_block[0]);
    assert_eq!(wrap.rows(), 2, "nine ideographs are eighteen cells");
    assert_eq!(
        wrap.row_span(0),
        Some((0, 0, 10)),
        "the first row is the ten cells that fit",
    );
    assert_eq!(
        wrap.row_span(1),
        Some((0, 10, 19)),
        "and the rest is the second, whose far end runs one column past the \n             line to stand the break in",
    );
    let palette = bt_render::chrome_palette();
    let mut quads = Vec::new();
    let mut paragraphs = Vec::new();
    push_markdown_source_block(
        (&mut quads, &mut paragraphs),
        &source,
        None,
        &highlight::Highlighting::plain(),
        box_of_block,
        [0.0, 0.0, 1000.0, 1000.0],
        &palette,
    );
    let [first, second] = paragraphs.as_slice() else {
        panic!("one paragraph a row: {paragraphs:#?}");
    };
    let text = |paragraph: &bt_render::PreviewParagraph| {
        paragraph
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()
    };
    assert_eq!(
        text(first),
        "这一段完全",
        "five ideographs on the first row"
    );
    assert_eq!(text(second), "没有空格", "and four on the second");
    for paragraph in [first, second] {
        assert_eq!(
            paragraph.cell_advance,
            Some(8.0),
            "each row is drawn on the very cells it was folded in",
        );
    }
    assert!(
        bt_unicode::text_width(&text(first)) <= 10,
        "and no row is wider than the block it folded inside",
    );
}

/// **Enter splits a block and Backspace at a block start merges two**, and
/// neither is a case anybody wrote: the keys move bytes and the parser
/// answers.
///
/// MUTATION: make Backspace refuse to cross a block boundary and the two
/// paragraphs can never be joined again — which is the special case §9.1
/// exists to avoid having.
#[test]
fn enter_and_backspace_split_and_merge_blocks_through_the_reparse() {
    let mut content = String::from("one two\n");
    let mut caret = preview_edit::EditCaret {
        anchor: 3,
        caret: 3,
        desired_column: None,
        desired_x: None,
    };
    let eol = preview_edit::eol_of(&content).to_owned();
    assert!(preview_edit::insert(&mut content, &mut caret, &eol));
    assert!(preview_edit::insert(&mut content, &mut caret, &eol));
    assert_eq!(content, "one\n\n two\n");
    let (_, ranges) = preview::parse_markdown_ranged(&content);
    assert_eq!(ranges.len(), 2, "one paragraph has become two");
    assert_eq!(
        preview_live::caret_seat(&content, &ranges, caret.caret),
        preview_live::CaretSeat::Block(1),
        "and the caret is in the new one, which is therefore the source block",
    );

    let mut content = String::from("one\n\ntwo\n");
    let (_, ranges) = preview::parse_markdown_ranged(&content);
    assert_eq!(ranges.len(), 2);
    let mut caret = preview_edit::EditCaret {
        anchor: 5,
        caret: 5,
        desired_column: None,
        desired_x: None,
    };
    assert!(preview_edit::backspace(&mut content, &mut caret));
    assert_eq!(content, "one\ntwo\n");
    let (_, ranges) = preview::parse_markdown_ranged(&content);
    assert_eq!(ranges.len(), 1, "and two paragraphs have become one");
    assert_eq!(
        preview_live::block_source(&content, &ranges[0]),
        "one\ntwo",
        "the merged block is drawn as both of its lines",
    );
}

/// **A caret's selection is the file's own bytes, marks and all** (T5 ④,
/// research §10 Q3) — the copy semantics, said as an assertion.
///
/// This is where the rendered page's copy parts company with §7.31 ⑥, and
/// the departure is narrow: the *range* is still the run of the document the
/// reader dragged across, and what comes with it is the `#` and the `**`
/// inside that run, because they are the characters the caret was dragged
/// over. It is what a paste of the result puts back.
#[test]
fn a_caret_selection_copies_the_files_own_bytes() {
    let content = "# head\n\nsome **bold** words\n";
    let caret = preview_edit::EditCaret {
        anchor: 0,
        caret: 6,
        desired_column: None,
        desired_x: None,
    };
    assert_eq!(
        caret.selected(content),
        "# head",
        "the hashes are inside the range and come with it",
    );
    let across = preview_edit::EditCaret {
        anchor: 13,
        caret: 21,
        desired_column: None,
        desired_x: None,
    };
    assert_eq!(across.selected(content), "**bold**");
}

/// **A double click takes the word and a triple click the block**, in the
/// caret's own coordinate (T5 ①, §7.31 ⑦).
///
/// The word is the classifier the terminal beside the pane uses, walked over
/// the file's bytes; the block is the run of the file it was parsed from,
/// its trailing break off, because the blank line after a paragraph belongs
/// to no block.
#[test]
fn a_repeated_press_takes_a_word_and_then_the_block() {
    let content = "# head\n\nsome bold words\n";
    let (_, ranges) = preview::parse_markdown_ranged(content);
    assert_eq!(
        (
            preview_select::word_start(content, 14),
            preview_select::word_end(content, 14)
        ),
        (13, 17),
        "the word the pointer is inside, and not the spaces round it",
    );
    let block = ranges[1].clone();
    assert_eq!(block, 8..24);
    assert_eq!(
        block.start + preview_live::block_source(content, &block).len(),
        23,
        "a triple click takes the paragraph and stops before its own break",
    );
}

/// One piece of a rendered document, boxed on a page 400px wide.
fn text_box(at: preview_select::Place, len: usize, text: &str) -> PreviewTextBox {
    row_box(at, len, text, 0.0, 20.0)
}

/// **An inline formula's placeholder is one thing to the shaper and a
/// whole `$x$` to the document**, and the two byte spaces rejoin after it.
///
/// MUTATION: carry the difference forwards with the wrong sign and every
/// word after a formula in the same paragraph selects the word beside it.
#[test]
fn an_inline_formulas_placeholder_stands_for_the_whole_of_its_source() {
    // `see $x^2$ there` — the middle run is drawn as a picture, so the
    // shaper is handed one non-breaking space where the document has six
    // bytes.
    let piece = PreviewTextPiece {
        at: preview_select::Place::new(0, 0, 0),
        len: "see $x^2$ there".len(),
        lead: 0,
        atoms: vec![PreviewTextAtom {
            shaped: (4, 6),
            doc: (4, 9),
        }],
        atomic: false,
    };
    assert_eq!(piece.doc_offset(0), 0, "before it, the two agree");
    assert_eq!(piece.doc_offset(4), 4, "and at its own first byte");
    assert_eq!(piece.doc_offset(6), 9, "past it, the document has run on");
    assert_eq!(
        piece.doc_offset(8),
        11,
        "and every byte after it keeps that distance",
    );
    assert_eq!(
        piece.doc_offset(5),
        4,
        "an offset the shaper cannot return — inside the one placeholder \
             glyph — is the formula's own beginning rather than a byte of it",
    );
    // Backwards: a range that cuts into the formula covers all of it.
    assert_eq!(piece.shaped_range(0..6, 12), 0..6);
    assert_eq!(piece.shaped_range(0..11, 12), 0..8);
    assert_eq!(piece.shaped_range(9..15, 12), 6..12);
}

/// PIN (user report, 2026-08-13) — **a block is built at its own full width
/// and cropped when it is drawn.**
///
/// The report: drag a code fence's thumb to the right and the fence goes
/// blank but for a sliver of glyphs against its left edge. The cause was
/// that a scrolling block's inner frame was **one page wide and slid left**
/// (`right - offset`), so every rectangle inside it was laid out in a
/// window that walked off its own clip. A fence is one paragraph spanning
/// the whole line, so its single box left the clip bodily and
/// `shape_preview_body`'s `crop_to` — correctly — drew nothing of it. A
/// table survived the same offset only because [`push_markdown_table`] lays
/// its cells out from the block's *origin* and never reads the frame's
/// right edge at all: a reprieve its structure happened to grant, not a
/// rule, which is why both are asserted here.
///
/// The frame is now the content's width placed at the offset, and the
/// cropping is where cropping belongs — at the draw, in `crop_to`, which
/// stays exactly as it is. That gate is for inverted and `NaN` boxes; a
/// block scrolled to its own end is neither, and it was never the thing
/// `crop_to` was put there to stop.
///
/// MUTATION: put the crop back at build time — restore `right - offset` as
/// the frame's right edge — and ① and ② both go red.
#[test]
fn a_scrolled_block_keeps_its_whole_width_and_is_cropped_only_when_drawn() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 300.0, 400.0];
    let page = body[2] - body[0] - metrics.padding_x * 2.0;
    let wide = page + 400.0;
    let overflow = wide - page;
    let line = "a fence line far wider than the page it is standing on";
    let cells = |text: &str| {
        vec![
            vec![preview::Span::plain("head")],
            vec![preview::Span::plain(text)],
        ]
    };
    let blocks = [
        preview::MarkdownBlock::Code {
            lang: None,
            text: line.to_owned(),
        },
        preview::MarkdownBlock::Table {
            rows: vec![cells("first"), cells("last")],
            alignments: vec![bt_detect::table::ColumnAlignment::None; 1],
        },
    ];
    let fence_height =
        metrics.code_border * 2.0 + metrics.code_padding_y * 2.0 + metrics.line_height;
    let layout: preview_viewport::Layout = [
        MarkdownBlockLayout {
            width: wide,
            ..MarkdownBlockLayout::solid(fence_height)
        },
        MarkdownBlockLayout {
            width: wide,
            top: fence_height + metrics.paragraph_gap,
            columns: vec![80.0, wide - 80.0],
            ..MarkdownBlockLayout::rows(
                vec![metrics.line_height, metrics.line_height],
                metrics.table_border,
            )
        },
    ]
    .into();
    let render = |offsets: &[f32]| {
        markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(offsets),
            (&blocks, &layout),
            &palette,
        )
    };
    let fence_line = |built: &bt_render::PreviewBody| {
        built.blocks[0]
            .paragraphs
            .iter()
            .find(|paragraph| {
                paragraph
                    .runs
                    .iter()
                    .any(|run| run.text.contains("fence line"))
            })
            .expect("a fence draws its line")
            .clone()
    };
    // Both blocks driven to the far end of their own travel — the gesture
    // the report is about — against the same document at rest.
    let at_rest = render(&[0.0, 0.0]);
    let built = render(&[overflow, overflow]);
    let region = |index: usize| {
        built
            .blocks
            .get(index)
            .unwrap_or_else(|| panic!("block {index} is wide and scrolls inside itself"))
    };

    // ① The fence's line is still there, whole, and covering the window.
    let fence = region(0);
    let drawn = fence_line(&built);
    assert_eq!(
        drawn
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        line,
        "and it is the whole line — the text is never cut to what fits"
    );
    let visible = bt_render::crop_to(drawn.rect, fence.clip)
        .expect("its box still meets the block's own rectangle");
    assert_eq!(
        [visible[0], visible[2]],
        [fence.clip[0], fence.clip[2]],
        "covering the window edge to edge, rather than surviving as a sliver"
    );
    // And what stands at the window's left edge is the far end of the line:
    // the box has travelled its whole overflow, and only translated —
    // nothing about it narrowed on the way.
    let resting = fence_line(&at_rest);
    assert_eq!(
        resting.rect[0] - drawn.rect[0],
        overflow,
        "the line has travelled its whole overflow"
    );
    assert_eq!(
        drawn.rect[2] - drawn.rect[0],
        resting.rect[2] - resting.rect[0],
        "and it is the same box that set out — a translation, not a squeeze"
    );
    assert!(
        drawn.rect[0] < fence.clip[0] && drawn.rect[2] >= fence.clip[2],
        "so the window is looking into the middle of it, with the tail \
             still reaching the far edge"
    );

    // ② The table, at the same offset, by the same rule — it looked right
    //    before only by the accident of being made of small boxes.
    let table = region(1);
    let last = table
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.runs.iter().any(|run| run.text == "last"))
        .expect("the last row's wide cell is still drawn");
    assert!(
        bt_render::crop_to(last.rect, table.clip).is_some(),
        "and it stands inside the window a scroll to the end opened on it"
    );

    // ③ At rest, nothing has been glued open: the window starts at the
    //    line's own beginning.
    assert!(
        resting.rect[0] >= at_rest.blocks[0].clip[0],
        "an unscrolled fence begins inside its own window"
    );
}

/// **The caret's block is as tall as its own source, and the page under it
/// moves by the difference** (§7.1.3q, ticket T4).
///
/// The one rule of the live preview, stated as geometry: a block drawn as
/// the file's own bytes is its folded line count times the source face's
/// line height, the shaper is never asked about it — a monospace row's
/// height is a fact, not a measurement — and every block after it starts
/// exactly that much further down.
///
/// MUTATION ①: drop the `source.filter(...)` arm in `lay_markdown_out` and
/// the block under the caret is laid out as wrapped prose, so the source
/// lines are drawn into a box measured for something else and the blocks
/// below overlap them.
/// MUTATION ②: measure the block's height from `lines.len()` instead of from
/// `wrap(width).rows()` and a source line wider than the column is drawn on
/// rows the layout did not reserve.
#[test]
fn the_carets_block_is_laid_out_as_its_own_source_lines() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let blocks = prose(&["first", "middle", "last"]);
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let calls = std::cell::Cell::new(0usize);
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        calls.set(calls.get() + 1);
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let width = 400.0;
    let rendered = lay_markdown_out(
        &blocks,
        &intrinsic,
        &NO_SOURCE_BLOCKS,
        width,
        metrics,
        art,
        &mut shaper,
    );

    let source = mono_caret_block(1, 6, "one\ntwo\nthree\nfour");
    let asked = calls.get();
    let live = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(source.clone()),
        width,
        metrics,
        art,
        &mut shaper,
    );
    assert_eq!(
        calls.get() - asked,
        blocks.len() - 1,
        "the shaper is asked about every block but the one drawn as source",
    );
    assert_eq!(
        (live[1].height, live[1].rows.len()),
        (80.0, 4),
        "four source lines at the source face's own line height",
    );
    assert_eq!(
        live[0], rendered[0],
        "the block in front of it is untouched",
    );
    assert_eq!(
        live[2].top - rendered[2].top,
        live[1].height - rendered[1].height,
        "and the block under it moves by exactly the difference",
    );

    // **And a source line too wide for the column folds**, on the source
    // face's own terms — the block is taller, and it is taller by whole
    // rows.
    let long = mono_caret_block(1, 6, &"x".repeat(200));
    let folded = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(long.clone()),
        width,
        metrics,
        art,
        &mut shaper,
    );
    let (measure_left, measure_right) =
        preview::markdown_measure_box([0.0, 0.0, width, 400.0], metrics);
    let _ = (measure_left, measure_right);
    assert_eq!(
        folded[1].rows.len(),
        (200.0_f32 / (width / 8.0)).ceil() as usize,
        "one row per column-full of a two-hundred-character line",
    );
}

/// **The prose block draws the file's own bytes at their own offsets**
/// (§7.1.3w) — which is what lets a press, a caret and a band be read
/// straight back as file offsets, with no provenance in between.
///
/// Every line is one paragraph, the paragraphs concatenate to the block's own
/// bytes with the file's own breaks between them, and the offset beside each
/// one is where that line begins **in the file**. Tabs are not expanded and
/// nothing is normalised: this is the file and not a rendering of it.
///
/// MUTATION: expand tabs the way the monospace face does and every offset
/// after the first tab on a line names the wrong byte.
#[test]
fn a_prose_block_draws_the_files_own_bytes_at_their_own_offsets() {
    let file = "intro\n\n- \tone **two**\n- 三 four\n";
    let block = file.find("- \t").expect("the fixture has a list in it");
    let text = "- \tone **two**\n- 三 four";
    let prose = MarkdownProseBlock {
        index: 1,
        range: block..file.len(),
        lines: prose_source_lines(text),
        text: text.to_owned(),
        heading: false,
        font_size: 14.0,
        line_height: 20.0,
    };
    let palette = bt_render::chrome_palette();
    let paragraphs = markdown_prose_paragraphs(
        &prose,
        [10.0, 100.0, 210.0, 140.0],
        &[20.0, 20.0],
        None,
        &palette,
    );
    assert_eq!(paragraphs.len(), 2, "one paragraph per source line");
    for line in &paragraphs {
        let drawn: String = line
            .paragraph
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect();
        assert_eq!(
            &file[line.start..line.start + drawn.len()],
            drawn,
            "the line drawn at {} is the file's own bytes there",
            line.start,
        );
    }
    assert!(
        paragraphs[0].paragraph.runs[0].text.contains('\t'),
        "and a tab is drawn as the character it is, not as the spaces it stands in for",
    );
    // Stacked at the heights the measuring pass wrote down, and both
    // wrapped into the same column.
    assert!((paragraphs[0].paragraph.rect[1] - 100.0).abs() < f32::EPSILON);
    assert!((paragraphs[1].paragraph.rect[1] - 120.0).abs() < f32::EPSILON);
    assert!(
        paragraphs
            .iter()
            .all(|line| (line.paragraph.rect[0] - 10.0).abs() < f32::EPSILON
                && (line.paragraph.rect[2] - 210.0).abs() < f32::EPSILON)
    );
}

/// **Seating a caret in a prose block re-flows the page and does not
/// re-parse it** (§7.1.3q, kept whole by §7.1.3w).
///
/// The caret's block is a *layout* fact: the page is laid out again because
/// one block is now drawn from other bytes, and the parse standing behind it
/// is untouched. What the shaper is asked is one question per line of the
/// prose block — how far that line folds — and nothing at all about the
/// block's rendered spans, which are not on the glass while the caret is in
/// it.
///
/// MUTATION: measure the rendered arm as well and every keystroke pays for a
/// block that is not being drawn.
#[test]
fn seating_a_caret_in_a_prose_block_reflows_and_does_not_reparse() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let blocks = prose(&["first", "middle", "last"]);
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let asked = std::cell::RefCell::new(Vec::<String>::new());
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        asked
            .borrow_mut()
            .push(runs.iter().map(|run| run.text.as_str()).collect::<String>());
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let width = 400.0;
    let rendered = lay_markdown_out(
        &blocks,
        &intrinsic,
        &NO_SOURCE_BLOCKS,
        width,
        metrics,
        art,
        &mut shaper,
    );
    asked.borrow_mut().clear();
    let source = prose_caret_block(1, 6, "**middle**\nsecond line");
    let live = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(source.clone()),
        width,
        metrics,
        art,
        &mut shaper,
    );
    assert_eq!(
        asked.borrow().clone(),
        ["first", "**middle**", "second line", "last"],
        "the shaper is asked about the prose block's own lines and never \
             about the rendering it is standing in for",
    );
    assert_eq!(
        live[1].rows.len(),
        2,
        "the block is as tall as its own two lines",
    );
    assert!(
        live[2].top > rendered[2].top,
        "and the block under it moved by the difference",
    );
    // The parse is untouched: the same blocks, in the same order, with the
    // same spans — the caret changed a layout and nothing else.
    assert_eq!(blocks, prose(&["first", "middle", "last"]));
}

/// **The letters being composed are drawn where they are being typed, in
/// the block's own face** (user report 2026-09-12; adversarial review
/// 2026-09-11 finding A8).
///
/// The report: a paragraph of Chinese with the caret in it showed
/// `我们是天下第一好`, a bare caret, and the candidate list — and nothing at
/// all between the caret and the list, because the page drew the file and
/// the composition lived in `window.preedit` where only the text face and
/// the grid ever looked.
///
/// What closes it is the composition spliced into the very paragraph the
/// shaper is handed, which is what makes the letters land in the block's own
/// face beside the letters they were typed among, with the rest of the
/// sentence pushed along in front of them rather than drawn over.
///
/// MUTATION ①: drop the splice and the paragraph is the file's own bytes
/// again — the first assertion goes red, which is the report.
/// MUTATION ②: splice into `prose.text` instead of into the runs and the
/// last goes red: the composition would be in the block, an Escape would
/// have to un-type it, and the caret's own byte would have moved.
#[test]
fn a_composition_is_drawn_at_the_caret_in_a_prose_block() {
    let text = "我们是天下第一好";
    let prose = MarkdownProseBlock {
        index: 0,
        range: 0..text.len() + 1,
        lines: prose_source_lines(text),
        text: text.to_owned(),
        heading: false,
        font_size: 14.0,
        line_height: 20.0,
    };
    let palette = bt_render::chrome_palette();
    let at = "我们是".len();
    // An input method that pre-edits latin, one that pre-edits Han, and the
    // apostrophe'd reading a Chinese method actually shows mid-word.
    for composing in ["nikan", "你看", "ni'kan"] {
        let preedit = MarkdownPreedit {
            text: composing.to_owned(),
            caret_byte: composing.len(),
        };
        let lines = markdown_prose_paragraphs(
            &prose,
            [10.0, 100.0, 210.0, 120.0],
            &[20.0],
            Some((at, &preedit)),
            &palette,
        );
        let [line] = lines.as_slice() else {
            panic!("one source line: {lines:#?}", lines = lines.len());
        };
        let drawn: String = line
            .paragraph
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect();
        assert_eq!(
            drawn,
            format!("我们是{composing}天下第一好"),
            "the composition stands at the caret, among the letters it is being typed into",
        );
        assert_eq!(line.paragraph.runs.len(), 3, "head, composition, tail");
        assert_eq!(line.paragraph.runs[1].text, composing);
        let face = &line.paragraph.runs[0];
        assert!(
            line.paragraph.runs.iter().all(|run| run.mono == face.mono
                && run.bold == face.bold
                && run.italic == face.italic
                && (run.font_scale - face.font_scale).abs() < f32::EPSILON
                && run.color == face.color),
            "and it is set in the block's own face, not in a fourth one",
        );
        assert_eq!(
            line.splice,
            Some(preview_live::ProseSplice {
                at,
                len: composing.len(),
                caret: composing.len(),
            }),
            "and the measuring pass is told where the letters went in",
        );
        assert_eq!(
            prose.text, text,
            "and the block's own bytes are exactly what they were",
        );
    }

    // **The mixed line, with its marks showing** (§7.1.3w) — a composition
    // typed between the stars of `**预览**` splices there and nowhere else,
    // and the marks either side of it are still the characters they are.
    let marked = "**预览**窗格";
    let mixed = MarkdownProseBlock {
        range: 0..marked.len() + 1,
        lines: prose_source_lines(marked),
        text: marked.to_owned(),
        ..prose.clone()
    };
    let preedit = MarkdownPreedit {
        text: "shi".to_owned(),
        caret_byte: 3,
    };
    let lines = markdown_prose_paragraphs(
        &mixed,
        [10.0, 100.0, 210.0, 120.0],
        &[20.0],
        Some(("**预览**".len(), &preedit)),
        &palette,
    );
    let drawn: String = lines[0]
        .paragraph
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(drawn, "**预览**shi窗格");
}

/// **The candidate list hangs at the composition's own caret** (user report
/// 2026-09-12).
///
/// A list offering to finish `nikan` that stood at the byte the `n` went in
/// front of would sit over the letters it is offering to replace, which is
/// §7.1.3u's complaint one surface along: the box and the bar are one
/// derivation, and while a composition is in flight that derivation is the
/// composition's.
///
/// MUTATION: hang the box off `prose.caret(caret)` while composing and the
/// first assertion goes red — the two x's are a whole pre-edit apart.
#[test]
fn the_candidate_box_sits_at_the_composition_caret_not_the_block_caret() {
    // One row of `我nikan看` as a body face lays it out: sixteen pixels an
    // ideograph, eight a latin letter, and a seam in front of every cluster.
    let seams: Vec<preview_live::ProseSeam> = [
        (0, 0.0),
        (3, 16.0),
        (4, 24.0),
        (5, 32.0),
        (6, 40.0),
        (7, 48.0),
        (8, 56.0),
        (11, 72.0),
    ]
    .into_iter()
    .map(|(offset, x)| preview_live::ProseSeam { offset, x })
    .collect();
    let cut = preview_live::split_prose_row(
        100.0,
        20.0,
        &seams,
        0,
        Some(preview_live::ProseSplice {
            at: 3,
            len: 5,
            caret: 2,
        }),
    );
    let rows = preview_live::ProseRows {
        index: Some(0),
        rows: vec![cut.row.clone()],
        composition: Some(preview_live::ProseComposition {
            rows: cut.composition.into_iter().collect(),
            caret: cut.caret,
        }),
    };
    assert_eq!(
        cut.caret.map(|rect| rect[0]),
        Some(32.0),
        "the box hangs two letters into the composition, where the method put its caret",
    );
    assert_eq!(
        rows.caret(3).map(|rect| rect[0]),
        Some(56.0),
        "while the block's own caret is the byte after the letters, a whole \
             pre-edit away",
    );
    assert_eq!(
        cut.composition,
        Some([16.0, 100.0, 56.0, 120.0]),
        "and the rule under the composition spans exactly the letters being typed",
    );
    // The file's own seams are the file's: what is in the paragraph and not
    // in the file is gone from them, and everything after the composition is
    // back where the file has it.
    assert_eq!(
        cut.row
            .seams
            .iter()
            .map(|seam| (seam.offset, seam.x))
            .collect::<Vec<_>>(),
        vec![(0, 0.0), (3, 56.0), (6, 72.0)],
    );
}

/// **Cancelling a composition goes through one door** (§7.1.5a″).
///
/// Two claims, and the pins are what keep them true: the Win32 notification
/// is named in exactly one place in this workspace, and this window reaches
/// it through exactly one function of its own — which is also the function
/// that clears everything this window was drawing from the composition. A
/// second spelling of either is a candidate list left on the glass with its
/// letters gone, or letters cleared with the list still up.
///
/// MUTATION: clear `window.preedit` at a second site without the platform
/// call and the last assertion goes red.
#[test]
fn cancelling_a_composition_goes_through_one_door() {
    assert_eq!(
        found_in_package(
            "bt-platform",
            needle!(Pattern::identifier("NI_COMPOSITIONSTR")),
            View::Identifiers,
            Scope::Module("crate".to_owned()),
        )
        .len(),
        2,
        "named where it is imported and where it is called, and nowhere else",
    );
    // **`cancel_composition` is declared three times in that crate** — once in
    // each of `windows_impl`, `macos_ime` and `portable_ime` — so the query
    // says which arm it means. The text needle this replaces said the same
    // thing by accident, by spelling the Windows arm's signature.
    assert!(
        package_item_body(
            "bt-platform",
            &ItemQuery::function("cancel_composition").in_module("crate::windows_impl"),
        )
        .contains("NI_COMPOSITIONSTR"),
        "and the place it is called is the door",
    );
    // The needle no longer has to be spelled in two pieces: a name written
    // inside a string literal is one token and not a path, so this line cannot
    // be one of the occurrences it is counting.
    assert_eq!(
        in_product(&found(
            needle!(Pattern::path("bt_platform::cancel_composition")),
            View::Identifiers,
        )),
        1,
        "and it is reached from exactly one place in this window",
    );
    let door = method_body("Runtime", "cancel_composition");
    for (cleared, what) in [
        ("bt_platform::cancel_composition", "the method's own state"),
        ("self.window.preedit = None", "the letters"),
        ("ime_cursor.reset()", "the rectangle the list hung from"),
        (
            "destroy_ime_caret(\"cancel_composition\")",
            "the caret Pinyin follows",
        ),
        ("set_preedit(\"\")", "the field's own copy of them"),
    ] {
        assert!(door.contains(cleared), "the one door lets go of {what}");
    }
    assert!(
        !door.contains("set_ime_allowed"),
        "and it does not re-associate the window's input context to do it",
    );
    // The one place that notices is the tail of the pass, which is where
    // every way of moving the keyboard has already happened.
    assert_eq!(
        in_product(&calls_of("Runtime", "settle_composition_owner")),
        1,
        "one watcher, and no list of the ways a field can go away",
    );
}

/// **One rebuild of a whole document, in three clocks** — the harness both
/// budget tests measure with, so that the two are measuring one thing.
///
/// **What is in the clock and what is not.** The parse is real — and since
/// ticket T7 that is the *mapped* parse, because
/// [`preview::parse_markdown_ranged`] is [`preview::parse_markdown_mapped`]
/// with its maps dropped and the window builds the maps on every parse. The
/// fence highlighting is real (syntect, the half the research expected to
/// dominate), and the layout arithmetic is real; the *shaper* is the stub
/// below, for this legacy arithmetic-only probe. So this is the cost of
/// everything a rebuild re-derives except the proportional shaping.
///
/// The three constructions above the clocks are outside all of them, which is
/// where they belong: a palette and an empty picture map are a test's setup
/// and not a document's cost.
fn rebuild_cost(
    content: &str,
    cache: &mut MarkdownIntrinsicCache,
) -> (
    usize,
    std::time::Duration,
    std::time::Duration,
    std::time::Duration,
) {
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let math = DocumentMath::default();
    let pictures = DocumentPictures::default();
    let art = PageArt {
        math: &math,
        pictures: &pictures,
        theme: bt_render::Theme::Dark,
    };
    let pass = IntrinsicPass {
        metrics,
        math: &math,
        palette: &palette,
        scale_ppm: scale_ppm(1.0),
        math_generation: 0,
    };
    let mut width_of = |runs: &[bt_render::PreviewRun], _: f32, _: f32| {
        runs.iter()
            .map(|run| run.text.chars().count())
            .sum::<usize>() as f32
            * 8.0
    };
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };

    let clock = Instant::now();
    let (blocks, ranges) = preview::parse_markdown_ranged(content);
    let parse = clock.elapsed();
    let clock = Instant::now();
    let intrinsic = measure_markdown_intrinsics(
        &blocks,
        MarkdownSourceBytes {
            content,
            ranges: &ranges,
        },
        pass,
        cache,
        &mut width_of,
    );
    let intrinsics = clock.elapsed();
    let clock = Instant::now();
    let source = MarkdownCaretBlock::Mono(MarkdownSourceBlock {
        index: 0,
        range: ranges[0].clone(),
        text: preview_live::block_source(content, &ranges[0]).to_owned(),
        lines: preview_edit::display_lines(preview_live::block_source(content, &ranges[0])),
        font_size: 14.0,
        line_height: 20.0,
        advance: 8.0,
    });
    let layout = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(source.clone()),
        1000.0,
        metrics,
        art,
        &mut shaper,
    );
    let laid = clock.elapsed();
    assert_eq!(layout.len(), blocks.len());
    (blocks.len(), parse, intrinsics, laid)
}

/// **A page written in Chinese costs what a page written in English costs**
/// (user report, 2026-09-10: opening `README.zh-CN.md` froze the window).
///
/// The report's own hypothesis was that the parse or the provenance mapping
/// walks a document by *byte* where it means *character*, or searches from
/// the start of a block for every piece — either of which is quadratic, and
/// Chinese triples the byte count of the same page. This is the measurement
/// that would say so: the same harness the English budget above uses, over
/// 64 KiB of this repository's Chinese front page and 64 KiB of a page
/// written in both scripts, against the same one-frame budget.
///
/// **The reported document is two documents now** (2026-09-14). The front
/// page was cut down to a summary and its feature sections moved to
/// `docs/features.zh-CN.md`, so the Chinese prose the report was about is
/// mostly in the second file; both are padded to 64 KiB and both are asked.
///
/// **Mixed text is here beside pure Chinese because it is not the same
/// document** to this parser. A run of ideographs never reaches the flanking
/// rule, the link scanner or the code-span scanner at all; `**中文**english`
/// and `` `代码`中文 `` put a marker hard against a three-byte character on
/// both sides, which is where a walk that steps by bytes and a walk that
/// steps by characters first disagree. See
/// [`preview::MIXED_SCRIPT_PAGE`].
///
/// **What it said** (2026-09-10, the machine the report came from, beside
/// the English line above on the same run): 64 KiB of Chinese / 37 897
/// characters / 293 blocks — parse 0.99 ms, intrinsics 4 µs, layout 0.12 ms,
/// **total 1.12 ms**; 64 KiB of both scripts / 42 382 characters / 561
/// blocks — parse 1.27 ms, intrinsics 12 µs, layout 0.12 ms, **total 1.41
/// ms**; against English's 64 KiB / 200 blocks at **1.48 ms**. So a page of
/// Chinese costs *less* than the same weight of English and not more — the
/// parser walks bytes and Chinese spends three of them on a character, so
/// the same 64 KiB is fewer words, fewer spans and fewer delimiter runs. The
/// report's hypothesis is disproved by this line, and the line is here so
/// that it stays disproved.
///
/// The budget is one frame, on the English test's own terms and for its
/// reason: what it catches is a change of *shape* — a walk that became
/// quadratic on multi-byte text — and not a percentage on whatever machine
/// happens to run it. The numbers are printed as well as asserted, because a
/// ratio against the English line above is the reading that matters and a
/// number nobody can read is not a measurement.
///
/// MUTATION: give [`preview::TextOrigin`]'s `run_at` a scan from the start of
/// the file rather than of its own runs, or count a paragraph's characters to
/// find a byte, and this goes red while the English one stays green.
#[test]
fn a_page_written_in_chinese_rebuilds_inside_the_frame_budget() {
    for (name, one) in [
        ("Chinese", preview::CHINESE_PAGE),
        ("Chinese, the long half", preview::CHINESE_FEATURE_PAGE),
        ("Chinese and English", preview::MIXED_SCRIPT_PAGE),
    ] {
        let mut document = String::new();
        while document.len() < 64 * 1024 {
            document.push_str(one);
            document.push_str("\n\n");
        }
        let mut cache = MarkdownIntrinsicCache::default();
        rebuild_cost(&document, &mut cache);
        let mut typed = document.clone();
        // The midpoint of a page written in Chinese may fall inside a
        // character; step back to the boundary before slicing.
        let at = (0..=typed.len() / 2)
            .rev()
            .find(|&i| typed.is_char_boundary(i))
            .unwrap_or(0);
        let at = typed[..at].rfind('\n').map_or(0, |line| line + 1);
        typed.insert(at, 'x');
        let (blocks, parse, intrinsics, laid) = rebuild_cost(&typed, &mut cache);
        let total = parse + intrinsics + laid;
        println!(
            "{name}: {} bytes, {} characters, {blocks} blocks — parse {:?}, \
                 intrinsics {:?}, layout {:?}, total {:?}",
            typed.len(),
            typed.chars().count(),
            parse,
            intrinsics,
            laid,
            total,
        );
        assert!(
            total < std::time::Duration::from_millis(16),
            "{name}: a keystroke in a 64 KiB document has to fit in a frame \
                 whatever script it is written in, and this one took {total:?} \
                 (parse {parse:?}, intrinsics {intrinsics:?}, layout {laid:?})",
        );
    }
}

/// PIN — a composition **opens a space in the line** rather than being
/// painted over it, and the IME's caret stands inside it.
///
/// The terminal's machine writes preedit into grid cells; this surface has
/// none, so the form is copied and the mechanism cannot be. This used to
/// copy it by drawing the row whole and laying the letters on top over an
/// opaque patch of the *pane's* ground — which real-machine capture
/// (2026-08-13) showed fails twice over: the patch hides only the cells the
/// composition itself covers, so every character after the caret stays where
/// it was and the composition and the rest of the word share cells neither
/// can be read in; and on a float, whose body stands on the window's face
/// rather than a pane's, the patch is the wrong colour as well. Splitting the
/// row cures both, because there is then nothing left to hide.
///
/// Asserted at **two geometries** — a docked pane's body and a float's, which
/// is the surface the colour half of the bug only showed on — because a body
/// that is a pure function of its rectangle must not have learned which kind
/// of container it is in.
///
/// MUTATIONS: draw the row unsplit (one run over `from..to`) and the tail's
/// offset assertion goes red; put the ground quad back and the "no pane ink"
/// assertion does; leave the caret at the document's column and the last one
/// does.
#[test]
fn a_composition_in_the_preview_opens_a_space_in_the_line_it_lands_in() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_text_metrics(1.0);
    let lines = vec!["one".to_owned(), "paragraph".to_owned()];
    let advance = 8.0;
    let wrap = preview_edit::WrapLayout::unwrapped(&lines);
    // Mid-word, which is the case that was unreadable: four characters of
    // "paragraph" stand before the caret and five after it.
    let paint = PreviewEditPaint {
        bands: Vec::new(),
        caret: Some((1, 4)),
        caret_width: 1.0,
        preedit: Some(PreviewPreedit {
            row: 1,
            column: 4,
            text: "ni".to_owned(),
            columns: 2,
            caret_columns: 1,
        }),
    };
    // A pane's body and a float's — the same builder, two containers.
    for (surface, body) in [
        ("a docked pane", [0.0, 0.0, 400.0, 200.0]),
        ("a preview float", [1866.0, 145.0, 2266.0, 345.0]),
    ] {
        let geometry = seats::preview_mono_geometry(
            body,
            metrics,
            metrics.line_height * 2.0,
            12,
            advance,
            [0.0, 0.0],
        );
        let built = build_preview_text_body(
            &geometry,
            &lines,
            &wrap,
            &highlight::Highlighting::default(),
            advance,
            Some(&paint),
            &palette,
        );
        let row = geometry.line_rect(1);
        let at = |columns: f32| row[0] + advance * columns;
        let run_at = |text: &str| {
            built
                .paragraphs
                .iter()
                .find(|paragraph| paragraph.runs.iter().any(|run| run.text == text))
                .unwrap_or_else(|| panic!("{surface}: no run drawing {text:?}"))
                .rect[0]
        };

        // The line is two runs with the composition's own width between them.
        assert_eq!(run_at("para"), at(0.0), "{surface}: the head stays put");
        assert_eq!(
            run_at("graph"),
            at(6.0),
            "{surface}: and the tail is pushed right by the whole composition \
                 — four columns of head plus its own two"
        );
        let letters = run_at("ni");
        assert_eq!(
            letters,
            at(4.0),
            "{surface}: the letters land at the caret, in the space just opened"
        );
        assert!(
            letters + advance * 2.0 <= run_at("graph") + 0.001,
            "{surface}: and stop before the tail begins — nothing overlaps"
        );

        // Nothing is masked, so no pane-only ink is painted at all: that is
        // the half of the bug a float showed and a pane hid.
        assert!(
            !built
                .quads
                .iter()
                .any(|quad| quad.color == palette.seat_body),
            "{surface}: a split line needs no ground under the composition"
        );
        let rule = built
            .quads
            .iter()
            .find(|quad| quad.color == palette.preview_body_text)
            .unwrap_or_else(|| panic!("{surface}: underlined, as a preedit is"));
        assert_eq!(
            rule.rect[3], row[3],
            "{surface}: along the bottom of its line"
        );
        let caret = built
            .quads
            .iter()
            .find(|quad| quad.color == palette.preview_caret)
            .unwrap_or_else(|| panic!("{surface}: the caret is drawn"));
        assert_eq!(
            caret.rect[0],
            at(5.0),
            "{surface}: inside the composition, where the IME says it is"
        );
    }
}

/// PIN — **the painter puts the document in the measure's column**, centred
/// when the pane can afford it and flush to the pane when it cannot
/// (§7.1.3i; user report 2026-08-16).
///
/// The geometry itself is `preview::markdown_measure_box`'s and is pinned
/// beside it; what is asserted here is that the *painter* asks — a body that
/// went on computing `body[0] + padding_x` for itself would draw the prose
/// across a maximised window while the layout pass wrapped it at the
/// measure, which
/// is a paragraph that reserves four rows and paints two.
///
/// MUTATION: put `let left = body[0] + metrics.padding_x` back at the top of
/// `build_preview_markdown_body` and the wide case goes red.
#[test]
fn a_pane_wider_than_the_measure_is_painted_into_a_centred_column() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let blocks = preview::parse_markdown("Prose enough to have an edge.\n");
    let layout: preview_viewport::Layout =
        vec![MarkdownBlockLayout::solid(metrics.line_height)].into();
    let column = |body: [f32; 4]| {
        let built = markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(&[]),
            (&blocks, &layout),
            &palette,
        );
        let rect = built.paragraphs[0].rect;
        (rect[0], rect[2])
    };

    let narrow = [0.0, 0.0, 500.0, 600.0];
    assert_eq!(
        column(narrow),
        (metrics.padding_x, 500.0 - metrics.padding_x),
        "a pane under the measure keeps the pane, exactly as before"
    );

    let wide = [0.0, 0.0, 1601.0, 600.0];
    let (left, right) = column(wide);
    assert_eq!(right - left, metrics.measure, "the column stops growing");
    assert_eq!(
        left - wide[0],
        wide[2] - right,
        "and it is centred in the pane"
    );
}

/// PIN — **a fence is set at 85% of the body on its own 1.45 leading**
/// (`pre { font-size: 85%; line-height: 1.45; padding: 16px }`, §7.1.3i).
///
/// The height the layout pass reserved is `code_line_height` a row, so a
/// painter still stepping by the body's `line_height` would draw the last
/// line of a long fence outside its own ground.
///
/// MUTATION: put `metrics.line_height` back on either the paragraph or the
/// step and the rows stop landing on the reservation.
#[test]
fn a_fence_is_set_at_its_own_size_and_stepped_at_its_own_leading() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 600.0, 600.0];
    let blocks = preview::parse_markdown("```\nfn a() {}\nfn b() {}\n```\n");
    let rows = 2.0;
    let layout: preview_viewport::Layout = vec![MarkdownBlockLayout::solid(
        metrics.code_border * 2.0 + metrics.code_padding_y * 2.0 + metrics.code_line_height * rows,
    )]
    .into();
    let built = markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        (&blocks, &layout),
        &palette,
    );
    assert_eq!(built.paragraphs.len(), 2, "one paragraph per fenced line");
    assert!(metrics.code_font < metrics.font_size, "85%, not 100%");
    for paragraph in &built.paragraphs {
        assert_eq!(paragraph.font_size_px, metrics.code_font);
        assert_eq!(paragraph.line_height_px, metrics.code_line_height);
        assert!(!paragraph.wrap, "and it still refuses to reflow");
    }
    assert_eq!(
        built.paragraphs[1].rect[1] - built.paragraphs[0].rect[1],
        metrics.code_line_height,
        "the rows are stepped at the fence's own leading"
    );
    let ground = built.paragraphs[0].rect[1];
    assert_eq!(
        ground,
        body[1] + metrics.padding_y + metrics.code_border + metrics.code_padding_y,
        "and the first one starts inside a 1em pad"
    );
}

/// PIN — a code fence is a box with a ground, and its language rides the
/// top-right corner of that box (mock-up 1202-1211).
///
/// Mutation: drop the `align_right` on the language tag, which parks it over
/// the first line of the code.
#[test]
fn a_code_fence_is_a_box_and_its_language_sits_in_the_corner() {
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 600.0, 400.0];
    let metrics = seats::preview_markdown_metrics(1.0);
    let blocks = preview::parse_markdown("```rust\nlet x = 1;\n```\n");
    let height = metrics.code_border * 2.0 + metrics.code_padding_y * 2.0 + metrics.line_height;
    let built = markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        (
            &blocks,
            &preview_viewport::Layout::from([MarkdownBlockLayout {
                top: 0.0,
                height,
                ..MarkdownBlockLayout::default()
            }]),
        ),
        &palette,
    );
    let border = built
        .quads
        .iter()
        .find(|quad| quad.color == palette.preview_code_border)
        .expect("the fence has a border");
    let ground = built
        .quads
        .iter()
        .find(|quad| quad.color == palette.preview_code_ground)
        .expect("and a ground inside it");
    assert!(
        ground.rect[0] > border.rect[0] && ground.rect[2] < border.rect[2],
        "the ground is inset by the border"
    );
    let lang = built
        .paragraphs
        .iter()
        .find(|p| p.runs[0].color == palette.preview_code_lang)
        .expect("the language tag is drawn");
    assert_eq!(lang.runs[0].text, "RUST", "and it is upper-cased");
    assert!(lang.align_right, "in the corner, not over the code");
    assert_eq!(lang.letter_spacing_em, seats::PREVIEW_MD_LANG_TRACKING_EM);
    assert_eq!(lang.rect[2], border.rect[2] - metrics.lang_inset_right);
    assert!(
        built.paragraphs.iter().any(|p| {
            p.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
                == "let x = 1;"
                && p.runs.iter().all(|run| run.mono)
        }),
        "the code itself is monospace"
    );
}

/// PIN — the caret, the band and the click all agree about which **row** a
/// column of a folded line is on.
///
/// Three readings of one arithmetic that used to be one reading: the caret's
/// drawn position, the selection band under it and the byte a click names.
/// Left disagreeing they produce the classic reflow bug — a caret drawn on
/// the first row of a paragraph while the text it is editing is on the
/// third.
///
/// MUTATION: build the paint with `WrapLayout::unwrapped` and the caret's row
/// goes to zero while its column runs off the pane — the pre-ruling drawing
/// of a wrapped surface.
#[test]
fn a_caret_on_a_folded_line_is_drawn_on_the_row_it_is_really_on() {
    let lines = vec!["aaaa bbbb cccc dddd".to_owned()];
    let wrap = preview_edit::WrapLayout::wrapped(&lines, 10);
    assert_eq!(wrap.rows(), 2);

    // Column 12 is the third character of "cccc", which is on the second row
    // and two cells into it.
    assert_eq!(preview_caret_row(&wrap, 0, 12), (1, 2));
    // And a caret at the head of the line is on the first row at column
    // zero, which is the case a wrap-blind painter also gets right — so it
    // is asserted next to one it does not.
    assert_eq!(preview_caret_row(&wrap, 0, 0), (0, 0));

    // The band for a selection covering columns 8..14 is cut in two, one
    // piece per row, each measured from its own row's left edge.
    let content = "aaaa bbbb cccc dddd";
    let starts = preview_edit::line_starts(content);
    let selection = 8..14;
    let selected = preview_edit::selected_columns(content, &starts, 0, &selection)
        .expect("the selection covers this line");
    assert_eq!(selected, (8, 14));
    let bands = preview_edit_bands(content, &starts, &selection, &wrap, 0..wrap.rows());
    assert_eq!(
        bands,
        vec![(0, 8, 10), (1, 0, 4)],
        "the band turns the corner with the text it is under"
    );

    // And Down from the first row lands on the same column of the second,
    // rather than leaving the line entirely.
    let mut caret = preview_edit::EditCaret {
        anchor: 2,
        caret: 2,
        desired_column: None,
        desired_x: None,
    };
    step_preview_caret_by_row(content, &mut caret, preview_edit::Motion::Down, &wrap, 10)
        .expect("Down is a vertical motion");
    assert_eq!(
        caret.caret, 12,
        "Down walks one drawn row, not one paragraph"
    );
    assert_eq!(caret.desired_column, Some(2));
    // Up returns to where it started.
    step_preview_caret_by_row(content, &mut caret, preview_edit::Motion::Up, &wrap, 10)
        .expect("Up is one too");
    assert_eq!(caret.caret, 2);
    // Home and End are not this function's business: they belong to the
    // logical line, which is the textarea convention the ruling names.
    assert!(
        step_preview_caret_by_row(
            content,
            &mut caret,
            preview_edit::Motion::LineEnd,
            &wrap,
            10
        )
        .is_none()
    );
}

/// The per-keystroke cost of the edit surface, on a file the size of the
/// whole head read.
///
/// **Re-laying the whole document per key is the design**, not a compromise
/// waiting for an incremental one: a 64KB head is the largest body this
/// surface can ever hold (§7.1.3 refuses more), and what a key costs is one
/// splice, one re-split into lines and one walk for the widest line. This
/// pins that the whole of it stays inside a frame's budget, so that the day
/// an incremental relayout is proposed there is a number to argue with.
///
/// The bound is deliberately loose — this is a wall clock on a shared
/// machine, and a test that fails on a busy build server teaches people to
/// ignore it. The number that matters is reported, not asserted.
#[test]
fn a_keystroke_relays_a_full_head_read_inside_a_frame() {
    let line = "    let value = compute(argument, other) + 1; // a line of source\n";
    let mut body = String::with_capacity(preview::PREVIEW_HEAD_BYTES);
    while body.len() < preview::PREVIEW_HEAD_BYTES {
        body.push_str(line);
    }
    body.truncate(preview::PREVIEW_HEAD_BYTES);
    let mut buffer = text_buffer("big.rs", &body);
    let mut caret = preview_edit::EditCaret {
        anchor: body.len() / 2,
        caret: body.len() / 2,
        desired_column: None,
        desired_x: None,
    };

    const KEYS: usize = 60;
    let started = Instant::now();
    for index in 0..KEYS {
        buffer.edit_content(|content| preview_edit::insert(content, &mut caret, "x"));
        // What the painter does with the result, every key: the document is
        // re-derived from the new revision.
        let lines = preview_edit::display_lines(buffer.content.as_deref().unwrap());
        assert!(!lines.is_empty(), "key {index}");
        std::hint::black_box(&lines);
    }
    let per_key = started.elapsed() / KEYS as u32;
    println!(
        "preview quick-edit: {:.3} ms/key over {} bytes",
        per_key.as_secs_f64() * 1000.0,
        body.len()
    );
    assert!(
        per_key < Duration::from_millis(40),
        "a keystroke took {per_key:?}, which is not a frame by any reading"
    );
}

/// **T5/§7.1.4 — a tab is seeded by the pane it is reopened as, and all three
/// shapes now exist.**
///
/// The vault's own sentence read against the three tab shapes: `TabState::seed`
/// answers off the *identity* seat, so a `[files | shell]` tab is still seeded
/// by its shell (that is what identity ordering is for) while a folder tab is
/// seeded by its folder and a file tab by its file.
///
/// The fourth answer is `None`, and it is the one worth writing a test for
/// because nothing else in the product produces it: a tab whose identity pane
/// is a leaf this build cannot read has no profile, no place and no path, so
/// `close_tab` writes no vault row rather than one that reopens as a guess.
///
/// Red gate: key `seed` on "does this tab hold a shell" instead of on the
/// identity pane's kind, and the first assertion turns into `Seed::Files` —
/// a split tab that would reopen as a bare column with its shell forgotten.
#[test]
fn a_tab_is_seeded_by_the_pane_it_would_be_reopened_as() {
    let split = tab_with_a_files_column(1, "D:\\work\\folio");
    assert!(
        matches!(split.seed(), Some(seed::Seed::Term { .. })),
        "a tab holding a shell beside a column is still reopened as the shell"
    );

    let mut source = tab_with_a_files_column(2, "D:\\work\\folio");
    let column = source.seats.files()[0];
    let folder = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        column,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a files column may become a tab of its own");
    assert_eq!(
        folder.seed(),
        Some(seed::Seed::Files {
            root: "D:\\work\\folio".to_owned()
        }),
        "and a folder tab as the place it was standing"
    );

    let (mut source, pane) = tab_with_a_preview(
        3,
        vec![buffer_saying("D:\\work\\notes.md", "notes.md", "hello")],
    );
    let file = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        pane,
        TabId(10),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a preview may become a tab of its own");
    assert_eq!(
        file.seed(),
        Some(seed::Seed::Preview {
            path: "D:\\work\\notes.md".to_owned(),
            source: bt_persist::PreviewSourceV1::File,
        }),
        "and a file tab as the file it was on — the vault's third shape, \
             without which Ctrl+Shift+T would be a door onto an empty store"
    );
}

/// PIN (user ruling 2026-08-25) — **the `…` chip's list runs deepest first,
/// and its rows point where they say.**
///
/// Windows Explorer's own breadcrumb `…` is the reference the ruling handed
/// over: 「由深到浅排,最近的隐藏级在最上,一路到根方向」. The geometry hands
/// over the *fold* order, which is the other way round, so the turn has to
/// happen somewhere and this is where.
///
/// **Asked with three levels**, because a fixture with two is symmetrical
/// under the very mistake this pin exists to catch.
///
/// RED EVIDENCE (2026-08-25): the chip did not build a list at all — it
/// raised the file menu on `folded.last()`, one folder, unnamed.
///
/// MUTATIONS: drop the `.rev()`; take the name from one segment and the
/// folder from another.
#[test]
fn the_folded_levels_read_from_the_deepest_towards_the_root() {
    let path = Path::new(r"D:\Developer\folio-terminal\test-assets\huge.txt");
    // What `preview_rail_geometry` folds: the middle, nearest the root
    // first.
    let levels = folded_levels(path, &[1, 2, 3]);
    assert_eq!(
        levels
            .iter()
            .map(|level| level.name.as_str())
            .collect::<Vec<_>>(),
        vec!["test-assets", "folio-terminal", "Developer"],
    );
    assert_eq!(
        levels
            .iter()
            .map(|level| level.folder.clone())
            .collect::<Vec<_>>(),
        vec![
            PathBuf::from(r"D:\Developer\folio-terminal\test-assets"),
            PathBuf::from(r"D:\Developer\folio-terminal"),
            PathBuf::from(r"D:\Developer"),
        ],
        "every row goes to the place it names"
    );
    assert!(folded_levels(path, &[]).is_empty());
}

/// PIN (user ruling 2026-08-25) — **every control the breadcrumb row grew
/// says what it is, and the two that name a place say the whole place.**
///
/// 「预览头与地址行/面包屑行的新控件全部无 tooltip」 was the report. The two
/// interesting halves are the ones a static string could not have covered:
/// a segment's tip is the **whole path** it stands for, because the word
/// drawn in it is only that path's last piece; and the `…`'s is the run of
/// levels it is hiding, **root first**, because it is a stretch of a path
/// being read left to right — the opposite order from the *menu* behind it,
/// which is a list of destinations.
///
/// RED EVIDENCE (2026-08-25): none of these anchors existed, so hovering any
/// of the five produced nothing at all.
///
/// MUTATIONS: give the flip one wording for both faces; give `⧉` one
/// wording for both rows; turn the fold's tip round.
#[test]
fn the_breadcrumb_rows_controls_each_say_what_they_are() {
    let path = PathBuf::from(r"D:\Developer\folio-terminal\test-assets\huge.txt");
    let segments = crumb_segments(&path);
    let folded = [1, 2, 3];
    let tip = |tip, kind, to_source| {
        preview_rail_tip_text(
            tip,
            kind,
            &segments,
            &folded,
            to_source,
            "Read-only over 8 MB",
        )
    };
    assert_eq!(
        tip(
            seats::PreviewRailTip::Crumb(2),
            seats::PreviewRailKind::Crumbs,
            false
        ),
        r"D:\Developer\folio-terminal",
        "a segment names the whole place, not the word drawn in it"
    );
    assert_eq!(
        tip(
            seats::PreviewRailTip::Fold,
            seats::PreviewRailKind::Crumbs,
            false
        ),
        format!(
            "Developer {sep} folio-terminal {sep} test-assets",
            sep = seats::PREVIEW_CRUMB_SEPARATOR
        ),
        "and the chip names the run it stands for, the way the row reads"
    );
    // `⧉` is one glyph on both rows and two sentences, which is the whole
    // reason it needed a tip.
    assert_ne!(
        tip(
            seats::PreviewRailTip::Copy,
            seats::PreviewRailKind::Crumbs,
            false
        ),
        tip(
            seats::PreviewRailTip::Copy,
            seats::PreviewRailKind::Address,
            false
        ),
    );
    // And `</>` names the destination rather than the state, because the
    // button changes.
    assert_ne!(
        tip(
            seats::PreviewRailTip::Flip,
            seats::PreviewRailKind::Crumbs,
            true
        ),
        tip(
            seats::PreviewRailTip::Flip,
            seats::PreviewRailKind::Crumbs,
            false
        ),
    );
    for text in [
        tip(
            seats::PreviewRailTip::Open,
            seats::PreviewRailKind::Crumbs,
            false,
        ),
        tip(
            seats::PreviewRailTip::Flip,
            seats::PreviewRailKind::Crumbs,
            true,
        ),
        tip(
            seats::PreviewRailTip::Copy,
            seats::PreviewRailKind::Address,
            false,
        ),
    ] {
        assert!(!text.is_empty(), "a control that registers has words");
    }
}

/// RED (review row D1, the premise) — **a float that declines a point still
/// consumes it.**
///
/// This is why the fall-through was a defect rather than a nicety.
/// `float_hit` is total inside the frame: a body its tenant has no answer
/// for comes back `FloatPart::Body`, and anything the named rectangles miss
/// comes back `FloatPart::Head`. There is no `None` for the caller to read
/// as "the pointer went through". So the *router* is where the rule has to
/// live, and since the report of 2026-09-12 it lives there — one place for
/// the press and the hover both.
///
/// Red gate: make `float_hit`'s body arm answer `None` when the tenant
/// declines and the first assertion fails; then `file_row_under`'s rule
/// would be unnecessary — and the window would be transparent to the
/// pointer, which is the bug this window was built not to have.
#[test]
fn a_float_that_declines_a_point_still_consumes_it() {
    let geometry = float::float_geometry(
        [100.0, 100.0, 364.0, 500.0],
        float::FloatMode::Peek,
        1.0,
        30.0,
        float::FloatHeadTools::default(),
    );
    let middle = |rect: [f32; 4]| ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
    let (x, y) = middle(geometry.body);
    assert_eq!(
        float::float_hit(&geometry, x, y, None, |_, _| None),
        Some(float::FloatPart::Body),
        "a tenant with no row under the pointer — a preview's text, the \
             space below a tree's last row — still hands the point to the window"
    );
    assert_eq!(
        float::float_hit(&geometry, x, y, None, |_, _| Some(float::FloatPart::Row(2))),
        Some(float::FloatPart::Row(2)),
        "and a tenant that does have one answers with it"
    );
    assert_eq!(
        float::float_hit(&geometry, geometry.frame[0] - 1.0, y, None, |_, _| None),
        None,
        "outside the frame, and only outside it, the pointer goes past"
    );
    // The one part that is not a tree row and not silence either: whatever
    // the named rectangles leave over is the head, which is what makes a
    // press anywhere inside this window a drag of it.
    let body = method_body("Runtime", "file_row_under");
    assert!(
        body.contains("Some(PointerTarget::Float(id, float::FloatPart::Row(index)))"),
        "so the door that raises a file menu names the one part it can \
             answer for and returns nothing for the rest"
    );
}

/// RED (user report 2026-09-12, during `next60` acceptance) — **a hand
/// resting on a floating window lights no row of the column underneath it.**
///
/// The press half of this is `b1cf054`'s, and it was written at one caller:
/// `file_row_under`. The hover does not pass that caller — `pointer_moved`
/// asks `update_chrome_hover`, which asks `chrome_target_at`, and the docked
/// ladder does not consider floats at all — so a preview window standing
/// over a files column was opaque to the eye and transparent to the hover.
/// The row behind it lit up, and after `peek_strip::PEEK_DELAY` raised its
/// glance card on top of the window that was hiding it.
///
/// Read as text for this family's stated reason: what it guards against is a
/// *second* door onto one question, and a second door that agrees today
/// cannot be driven into disagreeing by any state machine.
///
/// Red gate: put the ladder's body back under the name `chrome_target_at`,
/// or let its float arm answer with the chrome behind the window, and the
/// first two assertions fail by name.
#[test]
fn a_hover_inside_a_floats_body_lights_no_docked_row() {
    let door = method_body("Runtime", "chrome_target_at");
    assert!(
        door.contains("self.pointer_target_at(position)?"),
        "the chrome's door is the router's answer read through, and not a \
             walk of the docked ladder that never heard of a window"
    );
    assert!(
        door.contains("PointerTarget::Float(..) => None,"),
        "and a point a window has claimed is no chrome at all — never the \
             chrome that window is covering"
    );
    let hover = method_body("Runtime", "update_chrome_hover");
    assert!(
        hover.contains("Some(PointerTarget::Float(..)) | None => None,"),
        "so the hover this window paints is read through the same claim"
    );
}

/// RED (the same report, the glance half) — **a hand resting on a floating
/// window arms no peek for the row beneath it.**
///
/// `row_under` is the glance clock's one question, and it consumed the
/// float's claim for a tree row only: every other part of a window fell
/// through to the docked columns, and then to the terminal's own printed
/// references. So a rest on a preview window's text armed the covered row's
/// glance, which matured into a card drawn on top of the window.
///
/// Red gate: drop the declining arm and the ordering assertions fail by
/// name; move it below the docked rows and the first one does.
#[test]
fn a_hover_inside_a_float_arms_no_peek_for_the_row_beneath() {
    let rows = method_body("Runtime", "row_under");
    let own = rows
        .find("Some(PointerTarget::Float(id, float::FloatPart::Row(index)))")
        .expect("a window's own tree row is that window's row");
    let declined = rows
        .find("Some(PointerTarget::Float(..)) => None,")
        .expect("and every other part of that window is no row at all");
    let docked = rows
        .find("Some(PointerTarget::Chrome(")
        .expect("the docked columns answer after the windows");
    assert!(
        own < declined && declined < docked,
        "the window's own row first, then its refusal, and only then the \
             chrome it is standing on"
    );
    let cell = rows
        .find("self.terminal_reference_cell()")
        .expect("a printed reference is the last row this question has");
    assert!(
        declined < cell,
        "a reference printed under a window is not under the pointer either"
    );
    let glancing = method_body("Runtime", "glancing_row_at");
    assert!(
        glancing.contains("self.row_under(position)?"),
        "and the glance's clock is armed from this one answer, so `None` \
             here is both `no row lights` and `the card already up is retired`"
    );
}

/// RED (confirmation review of `6049179a`, P1) — **a press handed to the
/// program is released to it, however the hand comes up over the capsule or
/// the strip, and the route comes off.**
///
/// The in-pane surfaces decide where a gesture *starts*. `6049179a` let them
/// decide where one ends too: the cell root (`pane_hit_context`) refuses a
/// point on the capsule or the strip, `mouse_input` stopped at the failed cell
/// lookup, and the child was given a press and never its release while
/// `MouseRoute::Forward` stayed latched.
///
/// Run: a lone terminal wearing a strip, a forwarded press on a cell, and the
/// release with the pointer on the strip's `×` — a point no cell lookup names,
/// which the owner's clamp folds into the body's first row — comes back as the
/// release bytes and clears the route. Read: the release is ended by
/// `release_owned_gesture` above every surface claim, from the clamped owner
/// cell, and a routed drag's moves are reported the same way.
///
/// Red gate: move the owned release below the cell lookup (where the forwarded
/// release used to be answered) or measure it with `pane_hit_context`, and the
/// ordering or the clamp assertion fails; remove the drag-motion station and
/// the motion assertion fails.
#[test]
fn a_forwarded_press_is_released_to_its_pane_over_the_capsule_and_the_strip() {
    let scale = seats::scale_ppm(CROSS_DPI) as f32 / 1_000_000.0;
    let mut seats = seats::Seats::lone_terminal();
    let seat = seats.terminals()[0];
    seats.set_notices(std::collections::BTreeSet::from([seat]));
    let (layout, _) = cross_solve(&seats);
    let body = seats::pane_body_viewport(&seats, &layout, seat, scale).expect("a placed pane");
    let strip = seats::pane_notice_strip(&seats, &layout, seat, scale).expect("a strip");
    // No band kind is worn by a terminal since T-INTEGRATION-INJECT-1 retired `Offer` and
    // `Added`; the seat model and the router do not tell a terminal's strip from a preview's, so
    // the release's ordering is pinned with a band that still exists.
    let bar = notice::lay_out(
        strip,
        notice::NoticeSay::band(notice::Notice::DiskChanged),
        &[90.0, 90.0],
        scale,
    );
    let close = bar.close.expect("a band has its `×`");
    let (x, y) = (
        f64::from((close[0] + close[2]) / 2.0),
        f64::from((close[1] + close[3]) / 2.0),
    );
    assert!(
        y < f64::from(body.y),
        "the strip's `×` is above the first row of cells, so no cell lookup names it"
    );
    let (clamped_x, clamped_y) = clamp_into_body(body, x, y);
    assert_eq!(
        clamped_y, 0.0,
        "the owner's clamp folds it into the first row"
    );
    assert!(clamped_x > 0.0 && clamped_x < f64::from(body.width));

    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
    session.feed(b"\x1b[?1000h\x1b[?1006h").unwrap();
    let mut route = None;
    let pressed = route_forwarded_mouse_button(
        &mut route,
        ElementState::Pressed,
        input::MouseProtocolButton::Left,
        bt_render::GridHit { row: 3, column: 4 },
        session.terminal_modes(),
        ModifiersState::empty(),
        PressedCellTarget::Ordinary,
        a_shell(),
    );
    assert!(pressed.is_some() && matches!(route, Some(MouseRoute::Forward { .. })));
    let released = route_forwarded_mouse_button(
        &mut route,
        ElementState::Released,
        input::MouseProtocolButton::Left,
        bt_render::GridHit { row: 0, column: 9 },
        session.terminal_modes(),
        ModifiersState::empty(),
        PressedCellTarget::Ordinary,
        a_shell(),
    )
    .expect("the release is owed to the forwarded press");
    assert_eq!(
        released, b"\x1b[<0;10;1m",
        "an SGR release at the clamped cell"
    );
    assert!(route.is_none(), "and the route comes off");

    let input = squeezed_body("Runtime", "mouse_input");
    let owned = input
        .find("ifstate==ElementState::Released&&self.release_owned_gesture(button)?")
        .expect("a release is first offered to the gesture that owns it");
    for later in [
        "self.quit_card_layout()",
        "self.press_in_pane_surface(button,position)?",
        "self.chrome_mouse_input(state,button,position)?",
        "self.pane_frame_hit()",
    ] {
        let at = input
            .find(later)
            .unwrap_or_else(|| panic!("`{later}` is in the router"));
        assert!(owned < at, "the owned release is answered before `{later}`");
    }
    let release = squeezed_body("Runtime", "release_owned_gesture");
    assert!(
        release.contains("Some(MouseRoute::Forward{button:latched,owner,..})=>")
            && release.contains("self.forwarded_gesture_hit(seat)")
            && release.contains("ElementState::Released,"),
        "the forwarded release is sent from the owner's cell"
    );
    assert!(
        release.contains("self.window.mouse_route=None;returnOk(true);"),
        "and a pane with no frame still lets go of the route"
    );
    let hit = squeezed_body("Runtime", "forwarded_gesture_hit");
    assert!(
        hit.contains("self.drag_hit_in_pane(seat)?") && !hit.contains("pane_hit_context"),
        "the owner's cell is clamped into its body, not refused by the surfaces over it"
    );
    let moved = squeezed_body("Runtime", "pointer_moved");
    let routed = moved
        .find("ifmatches!(self.window.mouse_route,Some(MouseRoute::Forward{..})){returnself.forward_owned_drag_motion();}")
        .expect("a routed drag's moves go to the pane that took the press");
    let guard = moved.find("ifhit.is_none(){").expect("the cell guard");
    assert!(
        routed < guard,
        "ahead of the guard that needs a cell under the pointer"
    );
    assert!(
        squeezed_body("Runtime", "forward_owned_drag_motion")
            .contains("self.forwarded_gesture_hit(seat)"),
        "measured the same way the release is"
    );
}

/// RED (confirmation review of `6049179a`, P2) — **the in-pane surfaces yield
/// to every band painted above them, and that list is the paint order.**
///
/// The wheel's in-pane station stood above the palette, so a notch on the
/// palette's list where it overlapped a pill was swallowed by the pill. The
/// rule is not a station moved by hand: the router's in-pane step yields to
/// `OVER_IN_PANE_TOP_FIRST`, and every reader that asks the router — the
/// wheel, the press door, the tip — inherits it. This test reads
/// `OverlayStack::flattened` and requires that list to be exactly the bands it
/// paints above `in_pane`, top first, less the ones that take no pointer — so a
/// band added to the paint above the in-pane surfaces fails here until it is
/// classified, and a reorder of the paint fails here until the list follows.
///
/// Red gate: swap two entries of the list, or drop the yield from the router,
/// and the assertion naming it fails.
#[test]
fn the_in_pane_surfaces_yield_to_every_band_painted_above_them() {
    let paint = squeezed(item_body(&ItemQuery::method("OverlayStack", "flattened")));
    let array = &paint[paint.find("[preview_bars,").expect("the paint array")..];
    let array = &array[1..array.find(']').expect("its end")];
    let bands: Vec<&str> = array.split(',').filter(|band| !band.is_empty()).collect();
    let in_pane = bands
        .iter()
        .position(|band| *band == "in_pane")
        .expect("the in-pane surfaces are painted as one band");
    let above: Vec<&str> = bands[in_pane + 1..]
        .iter()
        .rev()
        .copied()
        .filter(|band| !BANDS_OVER_IN_PANE_THAT_TAKE_NO_POINTER.contains(band))
        .collect();
    let listed: Vec<&str> = OVER_IN_PANE_TOP_FIRST
        .iter()
        .map(|family| family.band())
        .collect();
    assert_eq!(
        listed, above,
        "OVER_IN_PANE_TOP_FIRST is the paint order above the in-pane surfaces, top first"
    );
    let router = squeezed_body("Runtime", "pointer_target_at");
    let step = router
        .find("forsurfaceinIN_PANE_SURFACES_TOP_FIRST")
        .expect("the in-pane step");
    assert!(
        router[step..].contains(
            "ifself.painted_over_in_pane_at(position,&OVER_IN_PANE_TOP_FIRST){break;}returnclaim;"
        ),
        "the router's in-pane claim yields to every band painted over it"
    );
    assert!(
        squeezed_body("Runtime", "notice_at")
            .contains("OVER_IN_PANE_TOP_FIRST.split(|family|*family==OverInPane::Float)"),
        "and a window's own pill to every band painted over the window"
    );
    for reader in [
        "mouse_wheel",
        "press_in_pane_surface",
        "owned_tooltip_anchor_at",
    ] {
        assert!(
            squeezed_body("Runtime", reader).contains("self.in_pane_surface_at(position)"),
            "`{reader}` takes the in-pane claim from the router, so it yields in the paint order"
        );
    }
    let surface = squeezed_body("Runtime", "in_pane_surface_at");
    assert!(
        surface.contains("self.pointer_target_at(position)?"),
        "and the claim is the router's"
    );
}

/// RED (review row D7) — **a menu's rows are the rows its host can carry
/// out.**
///
/// A floating tree got the docked column's whole face — `Rename`, `Delete`
/// and, on a folder, `New file…` and `New folder…` — while
/// `open_files_row_rename`, `open_files_row_new` and `delete_files_row` each
/// answer only `RowHost::Column`. Four rows of a six-row menu did nothing at
/// all, with no field, no card and no explanation. The refusals are right; it
/// was the menu that was not told.
///
/// Red gate: hand `profiles::file_menu` the subject alone and every
/// assertion in the first loop fails by name.
#[test]
fn a_menus_rows_are_the_rows_its_host_can_perform() {
    use profiles::FileMenuRow as Row;
    let on = |host| {
        file_menu_powers(Some(&FileMenuTreeRow {
            host,
            key: "/notes.md".to_owned(),
        }))
    };
    let column = on(RowHost::Column(SeatId(1)));
    let float = on(RowHost::Float(7));
    assert!(column.writes_rows, "a docked column owns the rows it draws");
    assert!(
        !float.writes_rows,
        "a float has no box to measure an editor into and no column to \
             report a refusal on"
    );
    let writes = |row: &Row| {
        matches!(
            row,
            Row::Rename | Row::Delete | Row::NewFile | Row::NewFolder
        )
    };
    for subject in [
        profiles::FileMenuSubject::File,
        profiles::FileMenuSubject::Folder { expanded: false },
        profiles::FileMenuSubject::Folder { expanded: true },
        profiles::FileMenuSubject::Root,
    ] {
        let docked = profiles::file_menu(subject, column).rows;
        let floating = profiles::file_menu(subject, float).rows;
        assert!(
            docked.iter().any(writes),
            "{subject:?} on a column offers verbs that write"
        );
        assert!(
            !floating.iter().any(writes),
            "{subject:?} on a float offers none of them"
        );
        // And nothing *else* is taken away: the two faces differ by exactly
        // the rows the float cannot perform, so a floating tree still opens,
        // folds, starts a shell and hands out its path.
        assert_eq!(
            floating,
            docked
                .iter()
                .copied()
                .filter(|row| !writes(row))
                .collect::<Vec<_>>(),
            "{subject:?}"
        );
    }
    // The gap `Delete` stands in goes with it rather than being left behind
    // as a rule with nothing above it.
    assert_eq!(
        profiles::file_menu(profiles::FileMenuSubject::File, float).lone_separator_after,
        None,
        "no lone row, no lone rule"
    );
    // A face with no tree row behind it has no host to ask about, and its
    // list is the same either way — neither carries a row that writes.
    for subject in [
        profiles::FileMenuSubject::Document,
        profiles::FileMenuSubject::FoldedPath { levels: 3 },
    ] {
        assert_eq!(
            profiles::file_menu(subject, column).rows,
            profiles::file_menu(subject, float).rows,
            "{subject:?}"
        );
    }
    assert!(
        !file_menu_powers(None).writes_rows,
        "and a menu with no row behind it is handed the powerless set"
    );
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

/// RAIL (user ruling 2026-08-24) — **a path reads as its segments, each one
/// naming the place it leads.**
///
/// The two things about this walk a reader can see and a re-parse would get
/// wrong: the drive and its separator are **one** segment (a row that drew
/// both would offer a folder called `\`), and every segment's target is the
/// path built *up to and including it* rather than a prefix guessed from the
/// name.
///
/// MUTATIONS:
/// ① give `RootDir` a segment of its own — the row grows a `\` between the
///    drive and the first folder, and pressing the drive stands the column
///    at `D:` (the process's current directory on that drive, which is not
///    where the reader pointed);
/// ② rebuild each target by joining the names — a path holding `..` or a
///    folder literally called `D:` lands somewhere else, which the last case
///    catches;
/// ③ drop the last segment because it is a file — the row loses the one
///    part the ruling says is bold, and the tail assertion goes red.
#[test]
fn a_path_reads_as_segments_that_each_name_where_they_lead() {
    let walked = crumb_segments(Path::new(r"D:\Developer\Folio\docs\DESIGN.md"));
    let names: Vec<&str> = walked.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        vec!["D:", "Developer", "Folio", "docs", "DESIGN.md"],
        "the drive and its separator are one segment, and the file is the last"
    );
    let targets: Vec<PathBuf> = walked.into_iter().map(|(_, target)| target).collect();
    assert_eq!(
        targets,
        vec![
            PathBuf::from(r"D:\"),
            PathBuf::from(r"D:\Developer"),
            PathBuf::from(r"D:\Developer\Folio"),
            PathBuf::from(r"D:\Developer\Folio\docs"),
            PathBuf::from(r"D:\Developer\Folio\docs\DESIGN.md"),
        ],
        "pressing the drive stands the column at its root, not at the \
             process's directory on that drive"
    );
    // A relative path keeps what it was written with: resolving `..` here
    // would be this walk inventing a place the reader never named.
    let relative = crumb_segments(Path::new(r"..\sibling\notes.md"));
    assert_eq!(
        relative
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["..", "sibling", "notes.md"]
    );
    assert_eq!(
        relative.last().expect("a tail").1,
        PathBuf::from(r"..\sibling\notes.md")
    );
    // A path that begins at a root with no drive still has a top: `\` is the
    // only name that level of the tree has.
    let rooted = crumb_segments(Path::new(r"\srv\share"));
    assert_eq!(
        rooted
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec![std::path::MAIN_SEPARATOR_STR, "srv", "share"]
    );
}

/// RED GATE (§13.40) — **a formula that lands rebuilds whatever was
/// standing on its source text**, the pane as well as the card.
///
/// The Mac's reading sweep opened a page holding `$$\int_0^1 x\,dx$$` and
/// photographed `\int_0^1 x\,dx` printed where the integral belonged, on
/// every run, thirty seconds in and after a press. `BT_PREVIEW_TRACE` says
/// what happened: `math formulas=1 drawn=0 asked=1 worker=1` at 111 ms, the
/// page built at 175 ms, `math answered set=1` at **1684 ms** — and then no
/// `build` line ever again. The picture was set, it landed, and nothing
/// asked the page to be laid out a second time, so
/// `PreviewMathCache::generation` — which exists for exactly that next
/// layout — was read by nobody.
///
/// This is a race and not a platform: `refresh_preview_body` is reached
/// from a resize, a scroll, an edit, an open and a palette change, and a
/// formula arriving is none of them, so the picture only ever reached the
/// glass because the page had not settled yet. The port is what lost the
/// race — a Mac's first typesetting of a session takes about 1.7 s against
/// a startup that settles inside 200 ms — and a slow first formula on any
/// machine is the same defect.
///
/// A **source pin**, because the shape being asserted is a call in a method
/// that needs a window, a device and a worker to run at all, and the three
/// facts worth keeping are all in the text: that the rebuild is asked for,
/// that it is gated on a *picture* rather than on `changed` (a refusal
/// leaves the block drawing exactly what it was drawing), and that it
/// happens before the publish rather than after it.
///
/// MUTATIONS:
/// ① drop the `refresh_preview_body` call — this goes red, and a page whose
///    formula is slow stands on its LaTeX for as long as the reader leaves
///    it alone;
/// ② gate it on `changed` instead — the flag assertion goes red, and every
///    refusal costs a rebuild of every document in the window;
/// ③ move it below the publish — the order assertion goes red, and the
///    frame that goes out is one behind the picture.
#[test]
fn a_formula_that_lands_rebuilds_the_page_that_was_standing_on_its_source() {
    let body = method_body("Runtime", "apply_math_results");
    let rebuild = body
        .find("self.refresh_preview_body();")
        .expect("a landed picture asks the page to be laid out again");
    let publish = body
        .find("self.publish_frame(FrameTrigger {")
        .expect("and the window still owes a frame after it");
    assert!(
        rebuild < publish,
        "the rebuild goes before the publish, so the frame that goes out \
             carries the new body rather than the one after it"
    );
    let guard = body[..rebuild]
        .rfind("if picture_landed {")
        .expect("the rebuild is gated on a picture");
    assert!(
        guard < rebuild && body[guard..rebuild].find("changed").is_none(),
        "gated on a picture rather than on `changed`: a refusal leaves the \
             block drawing exactly what it was drawing and owes no rebuild"
    );
    assert!(
        body.contains("picture_landed |= matches!(artifact, PreviewMathArtifact::Ready(_));"),
        "and the flag is set by a picture, never by a refusal"
    );
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

/// RED — **a picture opened again after its file was replaced is read off
/// the disk, not out of this window's memory** (user report 2026-08-31, the
/// second half).
///
/// The plainest gesture a reader has, and the one that named the second
/// cause: the pane was closed, the file was replaced by a `mv`, the pane was
/// opened again on the very same path — and the old picture came back. Two
/// caches keyed by a path alone were holding it, and only one of them is in
/// this crate. The other is [`bt_term::InlineImageDecoder`]'s, which its own
/// test answers for; this one drives **both**, in the order the app drives
/// them, over a real file that a real `std::fs::rename` really replaces —
/// because each road can regress on its own and a window served by a correct
/// decoder is still wrong if it never asks it.
///
/// MUTATIONS: take [`forget_a_picture`]'s `peek_cache.remove` away and the
/// reopened picture is 4×2 again — the report. Take the decoder's stamp
/// guard away (`bt-term`) and the same line goes red one layer down. Leave
/// the neighbouring file out of the removal — i.e. clear the whole cache —
/// and the last assertion goes red: forgetting one picture is not forgetting
/// every picture.
#[test]
fn a_picture_opened_again_after_a_rename_is_read_off_the_disk() {
    fn png_of(width: u32, height: u32, colour: [u8; 4]) -> Vec<u8> {
        let picture = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba(colour),
        ));
        let mut bytes = std::io::Cursor::new(Vec::new());
        picture
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("a PNG this process wrote");
        bytes.into_inner()
    }

    /// Opening a picture onto a pane, as far as the caches are concerned:
    /// the decode lane is asked, and the answer is remembered under the
    /// file's normalized path.
    fn open(decoder: &mut bt_term::InlineImageDecoder, peek_cache: &mut PeekCache, path: &Path) {
        let decoded = decoder
            .decode(bt_term::InlineImageTask {
                occurrence_id: 0,
                source: bt_term::InlineImageSource::LocalPath(path.to_path_buf()),
            })
            .expect("the picture decodes");
        peek_cache.insert(
            normalized_local_image_path_key(path),
            PeekCacheEntry::Ready {
                key: decoded.key,
                rgba: decoded.rgba,
                width_px: decoded.width_px,
                height_px: decoded.height_px,
                native_size: decoded.native_size,
            },
        );
    }

    fn size_of(peek_cache: &PeekCache, path: &Path) -> Option<(u32, u32)> {
        match peek_cache.get(&normalized_local_image_path_key(path))? {
            PeekCacheEntry::Ready {
                width_px,
                height_px,
                ..
            } => Some((*width_px, *height_px)),
            PeekCacheEntry::Pending | PeekCacheEntry::Failed(_) => None,
        }
    }

    let directory = bt_testpath::temp_path("folio-picture-reopen");
    std::fs::create_dir(&directory).expect("a scratch folder");
    let card = directory.join("card-3.png");
    let neighbour = directory.join("card-4.png");
    std::fs::write(&card, png_of(4, 2, [255, 0, 0, 255])).expect("the first card");
    std::fs::write(&neighbour, png_of(9, 9, [255, 255, 0, 255])).expect("its neighbour");

    let mut decoder = bt_term::InlineImageDecoder::default();
    let mut peek_cache = PeekCache::with_budget(MAX_PEEK_CACHE_BYTES);
    let mut video_facts = BTreeMap::new();
    let mut pictures = MarkdownPictures::default();

    open(&mut decoder, &mut peek_cache, &card);
    open(&mut decoder, &mut peek_cache, &neighbour);
    assert_eq!(size_of(&peek_cache, &card), Some((4, 2)));
    let generation = pictures.generation;

    // The pane is closed. Nothing forgets anything, and nothing should:
    // this is a picture a hover card or a markdown page may still be drawing.
    // Then another shell replaces the file — `mv` over an existing name is a
    // rename, so the bytes at that path are a different file entirely.
    let replacement = directory.join("card-3.new.png");
    std::fs::write(&replacement, png_of(6, 5, [0, 0, 255, 255])).expect("the new card");
    std::fs::rename(&replacement, &card).expect("the replacement lands on the name");

    // And the pane is opened again on the same path.
    forget_a_picture(&mut peek_cache, &mut video_facts, &mut pictures, &card);
    assert!(
        size_of(&peek_cache, &card).is_none(),
        "opening a picture ends this window's right to answer from memory"
    );
    assert!(
        pictures.generation > generation,
        "and every markdown page standing on that file is told to ask again"
    );
    open(&mut decoder, &mut peek_cache, &card);

    assert_eq!(
        size_of(&peek_cache, &card),
        Some((6, 5)),
        "the reader opened the file that is on the disk, so that is the \
             picture and that is the size the meta line states"
    );
    assert_eq!(
        size_of(&peek_cache, &neighbour),
        Some((9, 9)),
        "and the file nobody touched kept its decode: forgetting one picture \
             is not forgetting every picture"
    );

    std::fs::remove_file(&card).expect("the card goes");
    std::fs::remove_file(&neighbour).expect("its neighbour goes");
    std::fs::remove_dir(&directory).expect("and the folder with them");
}

/// **P121/§7.1.3 「若是原 tab 最后一个预览 pane 则整池随行」.**
///
/// A merge takes every seat the source had, so its last preview pane is
/// leaving by construction. The two buffers no pane was showing are the point
/// of the clause: they are the tab's *history*, one of them dirty, and the
/// ruling's own argument is that an orphaned dirty buffer must stay reachable
/// somewhere. Left behind on a tab that is about to stop existing, it is
/// reachable nowhere — and the dirty gate that would have named it goes down
/// with the same tab.
///
/// MUTATION ②: drop the `merge_from` line from `absorb_tab_sessions` and the
/// count goes to 0 while the source keeps all three — the pool stranded on a
/// dissolving tab, which is the shape of the bug.
#[test]
fn the_last_preview_pane_leaving_a_tab_takes_the_whole_pool_with_it() {
    let shown = buffer_saying(r"D:\notes\todo.txt", "todo.txt", "milk\n");
    let history = buffer_saying(r"D:\notes\README.md", "README.md", "# hi\n");
    let mut stranded = buffer_saying(r"D:\notes\draft.txt", "draft.txt", "half a thought");
    stranded.dirty = true;

    let (mut source, _) = tab_with_a_preview(1, vec![shown, history, stranded]);
    let arrived = arriving_as(&source, 90);
    let mut target = cross_tab(2, &["ALPHA"]);
    absorb_tab_sessions(&mut source, &mut target, &arrived);

    assert_eq!(
        target.preview_pool.len(),
        3,
        "the whole pool moved, not just the buffer on screen"
    );
    assert_eq!(
        target.preview_pool.dirty_names(None).collect::<Vec<_>>(),
        vec!["draft.txt"],
        "so the gate that speaks for the unsaved one still has it to name"
    );
    assert_eq!(
        source.preview_pool.len(),
        0,
        "and it moved — a second copy on a dissolving tab is the fork the law forbids"
    );
}

/// PIN (F1b, `plan.md` v4 增补 ②; `codex-final.md` §2 "pane 升格换号的活性
/// 仍缺一半") — **a pane promoted into a tab of its own asks its outstanding
/// questions again, under the name it now has.**
///
/// A pane is addressed by [`LeafId`], which is its tab and its seat. Promote
/// it and both change, so everything already out with a worker is addressed
/// to a leaf that no longer exists — and, crucially, **the tab it left is
/// still open**, because it had a sibling. The v3 draft said the old answers
/// would "naturally fail to match once the source tab died"; the source tab
/// does not die, and that sentence is void.
///
/// Safety is had by construction: the old tab no longer has that seat, so an
/// answer addressed to the old leaf is dropped. This test is the other half,
/// **liveness**. Every ledger that says "already asked" travels with the pane
/// — `DirNode::Pending` in its directory cache, a claimed head read on its
/// buffer, a path already out for verification — and a ledger that survives
/// an answer that never comes is a column that stays on "Loading …" for the
/// rest of the session. So the promotion clears them, and the ordinary
/// per-frame asking asks again under the new leaf: `wanted` is exactly what
/// [`files::tree_view`] puts a key on when the cache has never heard of it.
///
/// MUTATION: drop the `forget_work_in_flight_for_seat` call out of
/// `pane_into_new_tab` and the torn-out column keeps its `Pending` node,
/// `wanted` comes back empty, and nothing ever asks again.
#[test]
fn a_pane_promoted_into_its_own_tab_asks_its_outstanding_questions_again() {
    let mut source = tab_with_a_files_column(1, "D:\\work\\folio");
    let column = source.seats.files()[0];
    // One read is out with the worker, addressed to `LeafId { tab: 1, seat:
    // column }`, and the cache says so.
    source
        .file_trees
        .entry(column)
        .or_default()
        .mark_pending("");
    assert!(
        files::tree_view(&source.files[&column].clone(), &source.file_trees[&column])
            .wanted
            .is_empty(),
        "a directory already asked about is not asked about twice — which is \
             the ledger this test is about"
    );

    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        column,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a files column may become a tab of its own");
    let landed = torn.seats.files()[0];

    assert!(
        !source.files.contains_key(&column),
        "safety: the tab it left no longer has that seat, so the answer to \
             the old question lands nowhere"
    );
    assert!(
        !source.sessions.is_empty(),
        "and it is still open, which is why the old address failing to match \
             is not enough on its own"
    );
    assert_eq!(
        files::tree_view(&torn.files[&landed].clone(), &torn.file_trees[&landed]).wanted,
        vec![String::new()],
        "liveness: the promoted column wants its root read again, under the \
             leaf it now is"
    );
}

/// **T5 — a tab with no shell may not be renamed, and the guard that turns it
/// away was written for this day.**
///
/// `Seed::can_be_named` has answered `false` for a files place since it was
/// written, with its own note recording that "today this never answers
/// `false` for a real tab" and that the case would exist once T5 landed. It
/// has now. A preview-root tab is turned away on the same footing: its
/// identity is a path on disk, and the manual name is a slot on the terminal
/// seed and on nothing else.
#[test]
fn a_sessionless_tab_has_no_name_slot_for_the_editor_to_write_to() {
    assert!(
        seed::Seed::Term {
            profile_id: "pwsh.exe".to_string(),
            cwd: "D:\\".to_string(),
            manual_name: None,
        }
        .can_be_named()
    );
    assert!(
        !seed::Seed::Files {
            root: "D:\\work".to_string()
        }
        .can_be_named(),
        "a place is identified by its root, which is a fact about the disk"
    );
    assert!(
        !seed::Seed::Preview {
            path: "D:\\work\\notes.md".to_string(),
            source: bt_persist::PreviewSourceV1::File,
        }
        .can_be_named(),
        "and so is a file"
    );
}

/// Reflow shrinks the reachable maximum from 136 to 116. The drawing is held
/// to 116 and the stored 130 is not (T-CARD-NO-PASSIVE-CLAMP): a number cut
/// down by a width the pane wore on its way somewhere else is a reader's
/// place spent by nobody.
#[test]
fn card_restore_keeps_stored_skip_across_reflow() {
    let mut leaf = card_restore_fixture();
    card_restore_widen(&mut leaf);
    assert_eq!(card_restore_first(&leaf), "H001");
    assert_eq!(
        leaf.card_skip, 130,
        "the reflow drew the card, not the leaf"
    );
    // And the hand still has no debt to pay off: the notch clamps on the way
    // in, so the first reverse from the visible 116 moves one row.
    aim_card_window(&mut leaf, 4, -1, card_trace::Card::untraced());
    assert_eq!(leaf.card_skip, 115);
    assert_eq!(card_restore_first(&leaf), "H002");
}

#[test]
fn card_restore_extra_upward_detent_stays_at_top() {
    let mut leaf = card_restore_fixture();
    card_restore_widen(&mut leaf);
    let before = card_restore_first(&leaf);
    let mut carry = None;
    let steps = CardAim::spend(
        &mut carry,
        LeafId {
            tab: TabId(1),
            seat: SeatId(1),
        },
        MouseScrollDelta::LineDelta(0.0, 1.0),
    );
    aim_card_window(&mut leaf, 4, steps, card_trace::Card::untraced());
    let after = card_restore_first(&leaf);
    eprintln!("upward projection: {before} -> {after}");
    assert_eq!(after, "H001");
    assert_eq!(after, before);
}

#[test]
fn card_restore_reverse_detent_moves_toward_tail() {
    let mut leaf = card_restore_fixture();
    card_restore_widen(&mut leaf);
    let before = card_restore_first(&leaf);
    let mut carry = None;
    let steps = CardAim::spend(
        &mut carry,
        LeafId {
            tab: TabId(1),
            seat: SeatId(1),
        },
        MouseScrollDelta::LineDelta(0.0, -1.0),
    );
    aim_card_window(&mut leaf, 4, steps, card_trace::Card::untraced());
    let after = card_restore_first(&leaf);
    eprintln!("reverse projection: {before} -> {after}");
    assert_eq!(after, "H002");
    assert_ne!(after, before);
}

/// A deferred resize is persisted as the number the reader chose, and drawn
/// as the number the pane can reach (T-CARD-NO-PASSIVE-CLAMP).
#[test]
fn card_restore_deferred_resize_keeps_numeric_persistence() {
    let mut leaf = card_restore_fixture();
    card_restore_resize(&mut leaf, 40, 40, LeafOnStage::Behind);
    assert_eq!(leaf.card_skip, 130);
    card_restore_settle(&mut leaf);
    assert_eq!(card_restore_first(&leaf), "H001");
    // The session file carries the raw number, saturating at `u32`, and a
    // restart hands back what it carried.
    let saved = u32::try_from(leaf.card_skip).unwrap_or(u32::MAX);
    leaf.card_skip = saved as usize;
    assert_eq!(card_restore_first(&leaf), "H001");
    assert_eq!(leaf.card_skip, 130);
    // The reflow left nothing for the hand to pay off either.
    aim_card_window(&mut leaf, 4, -1, card_trace::Card::untraced());
    assert_eq!(leaf.card_skip, 115);
    assert_eq!(card_restore_first(&leaf), "H002");
}

#[test]
fn card_restore_alternate_screen_excludes_primary_history() {
    let mut leaf = card_restore_fixture();
    leaf.session.feed(b"\x1b[?1049h").unwrap();
    let text = (1..=20)
        .map(|number| format!("A{number:03}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    leaf.session.feed(text.as_bytes()).unwrap();
    // The card sees only alternate rows, never primary history: the stored
    // 130 (a place among the primary's 120 rows) is *drawn* at the alternate
    // screen's own 36, and the top of that screen is the blank the twenty
    // lines scrolled past on their way up from the saved cursor. The number
    // itself stands, because an app that took the screen for a moment is not
    // a reader deciding to read somewhere else (T-CARD-NO-PASSIVE-CLAMP) —
    // the clamp that meets the alternate screen is the next notch's, on the
    // way in.
    assert_eq!(card_restore_first(&leaf), "");
    assert_eq!(leaf.card_skip, 130);
    aim_card_window(&mut leaf, 4, i32::MIN, card_trace::Card::untraced());
    assert_eq!(card_restore_first(&leaf), "A017");
    aim_card_window(&mut leaf, 4, 10, card_trace::Card::untraced());
    assert_eq!(card_restore_first(&leaf), "A007");
    card_restore_resize(&mut leaf, 40, 40, LeafOnStage::Shown);
    assert_eq!(card_restore_first(&leaf), "A007");
    card_restore_settle(&mut leaf);
    assert_eq!(card_restore_first(&leaf), "A007");
    let assembled = focus_thumb::transcript_tail(&leaf.session, 40, 200, 0).0;
    let nonblank = assembled
        .iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(nonblank.len(), 20);
    assert!(
        nonblank
            .iter()
            .all(|line| line.trim_start().starts_with('A')),
        "primary content leaked into the alternate card: {assembled:?}"
    );
}

#[test]
fn card_restore_boundary_discards_overflow_before_reversal() {
    let mut leaf = card_restore_fixture();
    card_restore_widen(&mut leaf);
    aim_card_window(&mut leaf, 4, i32::MAX, card_trace::Card::untraced());
    assert_eq!(card_restore_first(&leaf), "H001");
    aim_card_window(&mut leaf, 4, i32::MAX, card_trace::Card::untraced());
    let mut carry = None;
    let steps = CardAim::spend(
        &mut carry,
        LeafId {
            tab: TabId(1),
            seat: SeatId(1),
        },
        MouseScrollDelta::LineDelta(0.0, -1.0),
    );
    aim_card_window(&mut leaf, 4, steps, card_trace::Card::untraced());
    assert_eq!(card_restore_first(&leaf), "H002");
}

/// RED (69a round 2, E8) — a worker answer lands only in the addressed shell incarnation. The
/// platform fake-tree pin proves WSL/ssh produce `Unknown`; this pin proves that answer is stored
/// as unknown, while an unlisted local image remains known for ordinary E3(b).
#[test]
fn an_addressed_foreground_answer_stores_unknown_and_preserves_an_unlisted_local_name() {
    let mut leaf = leaf_saying("foreground answer");
    let incarnation = leaf.incarnation;
    assert_eq!(
        foreground_program::apply_answer(
            &mut leaf,
            incarnation,
            bt_platform::foreground_program::ForegroundProgram::Unknown,
        ),
        Some((false, false))
    );
    assert_eq!(
        leaf.session.foreground_program(),
        &bt_detect::ForegroundProgram::Unknown
    );

    assert_eq!(
        foreground_program::apply_answer(
            &mut leaf,
            incarnation,
            bt_platform::foreground_program::ForegroundProgram::Known("powershell".to_owned()),
        ),
        Some((true, false))
    );
    assert_eq!(
        leaf.session.foreground_program(),
        &bt_detect::ForegroundProgram::known("powershell")
    );

    assert_eq!(
        foreground_program::apply_answer(
            &mut leaf,
            incarnation + 1,
            bt_platform::foreground_program::ForegroundProgram::Unknown,
        ),
        None
    );
    assert_eq!(
        leaf.session.foreground_program(),
        &bt_detect::ForegroundProgram::known("powershell")
    );
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

/// RED — **the texture a page is cached under names the file, its version,
/// which page it is, and its size**, so the shared GPU cache can never serve
/// one for another.
///
/// MUTATION: drop any one of the four from the key and the matching
/// assertion goes red — which on screen is the wrong document, yesterday's
/// document, **page 1 in every slot of the column** (the one the 2026-08-26
/// ruling added), or a page rastered for another monitor drawn soft on this
/// one.
#[test]
fn one_page_texture_is_one_page_of_one_file_at_one_version_at_one_size() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let then = now + Duration::from_secs(1);
    let key =
        |path: &str, mtime, page, w, h| peek_page_texture_key(Path::new(path), mtime, page, w, h);
    let base = key(r"D:\reports\q3.pdf", Some(now), 0, 124, 160);
    assert_ne!(base, key(r"D:\reports\q4.pdf", Some(now), 0, 124, 160));
    assert_ne!(base, key(r"D:\reports\q3.pdf", Some(then), 0, 124, 160));
    assert_ne!(base, key(r"D:\reports\q3.pdf", Some(now), 0, 248, 320));
    assert_ne!(
        base,
        key(r"D:\reports\q3.pdf", Some(now), 1, 124, 160),
        "two pages of one document are two pictures"
    );
    assert_eq!(base, key(r"D:\reports\q3.pdf", Some(now), 0, 124, 160));
    // A filesystem that will not say when a file was written gets a name of
    // its own rather than one that could collide with a real stamp — such a
    // file is re-drawn on every question anyway.
    assert_ne!(base, key(r"D:\reports\q3.pdf", None, 0, 124, 160));
    assert_ne!(
        key(r"D:\reports\q3.pdf", None, 0, 124, 160),
        key(
            r"D:\reports\q3.pdf",
            Some(SystemTime::UNIX_EPOCH),
            0,
            124,
            160
        )
    );
}

/// RED — **the texture a video's frame is cached under names the file, its
/// version and its size** (user ruling 2026-08-27; §7.23).
///
/// A picture file's texture is named by a hash of its own bytes, and a frame
/// cannot be: the bytes it came from are a container this process never held
/// whole, and re-decoding a hundred megabytes to name the eight it produced
/// would be the read the cache exists to avoid. So it is named the way a
/// PDF's page is — by the file, by when the file was last written, and by
/// the box it came back in.
///
/// **The modification time is the load-bearing field.** The window keeps a
/// video's pixels for as long as its decode cache holds them; a key without
/// a version would go on naming the same texture after the file underneath
/// had been re-recorded, and the GPU cache would serve yesterday's frame for
/// a clip that no longer contains it.
///
/// RED GATE: drop the stamp from [`video_frame_texture_key`] and the second
/// assertion goes red — on screen, a capture overwritten while the window
/// was open goes on showing the frame it used to have.
#[test]
fn one_video_texture_is_one_file_at_one_version_at_one_size() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let then = now + Duration::from_secs(1);
    let key = |path: &str, mtime, w, h| video_frame_texture_key(Path::new(path), mtime, w, h);
    let base = key(r"D:\shots\clip.mp4", Some(now), 1920, 1080);
    assert_ne!(base, key(r"D:\shots\other.mp4", Some(now), 1920, 1080));
    assert_ne!(
        base,
        key(r"D:\shots\clip.mp4", Some(then), 1920, 1080),
        "a capture re-recorded under the window is a different picture"
    );
    assert_ne!(base, key(r"D:\shots\clip.mp4", Some(now), 960, 540));
    assert_eq!(base, key(r"D:\shots\clip.mp4", Some(now), 1920, 1080));
    // A filesystem that will not say gets a name of its own rather than one
    // that could collide with a real stamp.
    assert_ne!(base, key(r"D:\shots\clip.mp4", None, 1920, 1080));
    // And it cannot collide with a page's, which shares the cache.
    assert!(base.starts_with("video-frame:"));
    assert!(
        peek_page_texture_key(Path::new(r"D:\shots\clip.mp4"), Some(now), 0, 1920, 1080) != base
    );
}

/// RED — **the frame under a hover card and the frame in the preview pane
/// come out of one decoder, asked through one door** (user ruling
/// 2026-08-27; §7.23).
///
/// The whole of §7.10 ⑥ said about a *lane* rather than about a name: a
/// glance card and a pane over the same file must show the same picture, and
/// the only way that is true by construction is that both ask
/// [`WindowRuntime::request_peek_pixels`] and neither reads the file's name
/// for itself. The two surfaces are hundreds of lines apart and each has its
/// own cache guard, so a second `MathWorkerRequest::PeekVideoFrame` built at
/// one of them would compile, would work, and would be the place the two
/// drift the day the fork grows a third arm.
///
/// Asserted as text for [`files_locate_door_tests`]' reason exactly: what is
/// being pinned is *which function builds which request*, and no value any
/// assertion can read says that.
///
/// RED GATE: inline the fork back into `refit_preview_picture` — that
/// function starts naming a decoder and this names the function and the
/// lane it named.
#[test]
fn one_door_decides_which_decoder_a_hover_and_a_pane_ask() {
    // Built at run time so that this test's own text is not one of the
    // sites it is counting.
    let lane = |variant: &str| format!("MathWorkerRequest::{variant} {{");
    let door = method_body("Runtime", "request_peek_pixels");
    for variant in ["PeekImage", "PeekVideoFrame"] {
        assert!(
            door.contains(&lane(variant)),
            "the door does not build {variant}:\n{door}"
        );
    }
    // And the two surfaces that show pixels name neither: what they ask for
    // is "this file's pixels", and a second reading of the name at either of
    // them is where the card and the pane come to disagree.
    for surface in ["refit_preview_picture", "file_peek_fitted_pixels"] {
        let text = method_body("Runtime", surface);
        for variant in ["PeekImage", "PeekVideoFrame"] {
            assert!(
                !text.contains(&lane(variant)),
                "`{surface}` chooses a decoder for itself: it names {variant}"
            );
        }
        assert!(
            text.contains("self.request_peek_pixels("),
            "`{surface}` must ask through the one door"
        );
    }
    // The door reads the class through the same predicate the open lane
    // forks on, so a file that *opened* as a video is decoded as one.
    assert!(
        door.contains("preview::path_names_a_video("),
        "the door must ask the class's own predicate:\n{door}"
    );
}

/// RED — **the card keeps the last few pages of the document it is over, and
/// keeps the ones the hand is nearest** (user ruling 2026-08-26;
/// [`PEEK_PAGE_CACHE`]).
///
/// A rastered page is hundreds of kilobytes and a long report has hundreds of
/// pages, so the cache is bounded — and a bound is only useful if what it
/// throws away is what nobody is looking at. The order is written at one
/// place, [`PeekPageSlot::wanted`], which the request lane calls for every
/// page in view on every frame; that is what makes "least recently used"
/// mean "furthest from where the reader stopped" rather than "drawn longest
/// ago", and the two differ exactly when a reader winds back up a document.
///
/// RED GATE ①: let [`PeekPageSlot::keep`] push without evicting and the
/// second block fails — the cache is unbounded, which for a two-hundred-page
/// report is a hover that costs a third of a gigabyte. RED GATE ②: make
/// `wanted` a plain lookup that does not reorder and the last block fails:
/// page 0, which the reader has just scrolled back to, is thrown away while
/// it is the one on screen.
#[test]
fn the_cards_page_cache_keeps_what_the_hand_is_nearest() {
    let raster = |page: u32| PeekPageRaster {
        key: format!("page-{page}"),
        rgba: Arc::from(vec![0_u8; 4].into_boxed_slice()),
        width_px: 1,
        height_px: 1,
    };
    let mut slot = PeekPageSlot {
        path: PathBuf::from(r"D:\reports\long.pdf"),
        fit: (280, 160),
        mtime: None,
        asked: BTreeSet::new(),
        pages: Vec::new(),
    };
    for page in 0..PEEK_PAGE_CACHE as u32 {
        slot.keep(page, raster(page));
    }
    assert_eq!(slot.pages.len(), PEEK_PAGE_CACHE);
    assert!(
        (0..PEEK_PAGE_CACHE as u32).all(|page| slot.page(page).is_some()),
        "everything asked for so far is still here"
    );

    // One more page than the cache holds, with nothing having been re-read:
    // the oldest goes and only the oldest.
    let past = PEEK_PAGE_CACHE as u32;
    slot.keep(past, raster(past));
    assert_eq!(slot.pages.len(), PEEK_PAGE_CACHE, "the bound is a bound");
    assert!(slot.page(0).is_none(), "the page furthest behind is gone");
    assert!(slot.page(past).is_some(), "and the newest one is here");
    assert!(
        slot.page(1).is_some(),
        "and nothing else was thrown away with it"
    );

    // The same page drawn again replaces itself rather than joining the run
    // twice — a second entry would let the stale one be found first for ever.
    slot.keep(past, raster(past));
    assert_eq!(slot.pages.len(), PEEK_PAGE_CACHE);
    assert_eq!(
        slot.pages.iter().filter(|(page, _)| *page == past).count(),
        1
    );

    // **A page the reader has scrolled back to is not the oldest thing here,
    // whenever it was drawn.** This is the whole of what `wanted` records.
    assert!(
        slot.wanted(1),
        "page 1 is in the cache and is being looked at"
    );
    let next = past + 1;
    slot.keep(next, raster(next));
    assert!(
        slot.page(1).is_some(),
        "so the page under the pointer survived the eviction it was next in line for"
    );
    assert!(slot.page(2).is_none(), "and the one behind it left instead");
    assert!(
        !slot.wanted(0),
        "a page that is not held is not made recent by being asked about"
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

/// **A drop that was aimed at no shell types nothing** (review X-1).
///
/// Chrome, a files column, a preview pane, a pane whose shell has gone: all
/// of them answer `None` at the arrival, and `None` is carried rather than
/// re-asked at the flush. The claim is that the batch is still assembled and
/// still spent — the drop is not *lost*, it is delivered to nobody — which
/// is what keeps a second drop from finding the first one still standing.
#[test]
fn a_drop_aimed_at_no_shell_is_still_collected_and_still_spent() {
    let mut standing: Option<DropBatch> = None;
    let at = PhysicalPosition::new(5.0, 5.0);
    DropBatch::collect(&mut standing, "/one".into(), Some(at), None);
    DropBatch::collect(&mut standing, "/two".into(), Some(at), None);
    let batch = standing.take().expect("the drop opened all the same");
    assert_eq!(batch.target, None, "there was nothing under the hand");
    assert_eq!(batch.point, Some(at), "but the window still knows where");
    assert_eq!(batch.paths.len(), 2);
    assert!(
        standing.is_none(),
        "and the batch is spent, not left to rot"
    );
}

/// **K121 as re-ruled, both sides of the line in one test.**
///
/// The wash belongs to a *hand-over* and to nothing else. A row reordered
/// inside its own strip was already put where it is, slot by slot, as it
/// travelled (K122); a pane torn out of a layout arrives in a run you were
/// not looking at, and is the arrival the wash exists to announce. The
/// mock-up spends the class in exactly one place for the same reason —
/// `extractPaneToTab` (3517-3542) adds `.landing`, `releaseGrabbed`
/// (6672-6685) never does.
///
/// Both halves are asserted as *tweens the strip would sample*, not as
/// fields that were set, because `sample` is the only question the paint
/// layer ever asks; and both name `Motion::Full`, so a reduced-motion zero
/// cannot pass the reorder half by accident.
///
/// Red gate: put `landing.start` back into `release_drag`'s `Commit` arm and
/// the reorder half goes red; take it out of `tear_pane_into_tab` and the
/// hand-over half does.
#[test]
fn a_reorder_settles_where_a_hand_over_washes() {
    let now = Instant::now();

    let mut reordered = cross_tab(1, &["ALPHA"]);
    reordered.settle_into_slot(40.0, now, Motion::Full);
    let (offset, sliding) = reordered.flip.sample(now, Motion::Full);
    assert!(
        sliding && offset.abs() > 0.0,
        "the last few pixels of travel are the whole of a reorder's release: \
             offset {offset}, sliding {sliding}"
    );
    assert_eq!(
        reordered.landing.sample(now, Motion::Full),
        (0.0, false),
        "and no wash rides along with them — a reorder announces nothing \
             because nothing arrived"
    );

    let mut source = cross_tab(2, &["ALPHA", "BETA"]);
    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        SeatId(2),
        TabId(9),
        now,
        Motion::Full,
        cross_solve,
    )
    .expect("a two-pane tab can spare one");
    let (wash, washing) = torn.landing.sample(now, Motion::Full);
    assert!(
        washing && wash > 0.0,
        "a pane that became a tab crossed a boundary, and that is exactly \
             what the wash is for: wash {wash}, washing {washing}"
    );
}

// ── the `⌄` ruling (2026-08-16) ─────────────────────────────────────────

/// PIN (**the ruling's own point**) — the tab strip's `⌄` and the pane
/// head's `⌄` are driven by **one call, one policy and one pair of
/// constants**.
///
/// The ruling is "两处 ⌄ 语义完全对齐", and the failure it guards against is
/// not a wrong delay: it is two implementations that agree in the build that
/// wrote them and drift in the one after. [`ChevronGates::observe`] is the
/// only function in this program that starts either clock — it takes both
/// buttons' states and cannot be called for one of them — so "the two agree"
/// is a fact about the type rather than a promise about two call sites.
///
/// Red gate: give either gate its own `observe` at its own call site and
/// this test still passes, but the *shape* it is asserting is gone — so the
/// assertion is written against the pair, driving both through one script
/// and demanding identical deadlines at every step.
#[test]
fn both_chevrons_are_driven_by_one_policy_and_one_pair_of_constants() {
    use profiles::{ChevronAction, ChevronPointer};
    let start = Instant::now();
    let mut gates = ChevronGates::default();

    // A rest on the strip's chevron while the pointer is nowhere near the
    // pane head's: one clock runs and the other does not.
    gates.observe(
        (ChevronPointer::Button, false),
        (ChevronPointer::Away, false),
        (ChevronPointer::Away, false),
        start,
    );
    assert_eq!(
        gates.deadline(),
        Some(start + profiles::CHEVRON_HOVER_OPEN_DELAY)
    );
    assert_eq!(gates.pane.deadline(), None, "an idle chevron owes nothing");
    assert_eq!(
        gates
            .profile
            .due(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        Some(ChevronAction::Open)
    );

    // The mirrored situation gives the mirrored answer at the same instant,
    // which is the whole claim.
    let mut mirrored = ChevronGates::default();
    mirrored.observe(
        (ChevronPointer::Away, false),
        (ChevronPointer::Button, false),
        (ChevronPointer::Away, false),
        start,
    );
    assert_eq!(mirrored.profile, gates.pane);
    assert_eq!(mirrored.pane, gates.profile);
    assert_eq!(mirrored.deadline(), gates.deadline());

    // Both graces run on the same 150, and the earliest deadline is the one
    // the loop is told about.
    let mut leaving = ChevronGates::default();
    leaving.observe(
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, false),
        start,
    );
    assert_eq!(
        leaving.deadline(),
        Some(start + profiles::CHEVRON_LEAVE_GRACE)
    );
    assert_eq!(
        leaving.profile.due(start + profiles::CHEVRON_LEAVE_GRACE),
        Some(ChevronAction::Close)
    );
    assert_eq!(
        leaving.pane.due(start + profiles::CHEVRON_LEAVE_GRACE),
        Some(ChevronAction::Close)
    );

    // And a press — every door that is not a pointer move — stops both.
    leaving.clear();
    assert_eq!(leaving.deadline(), None);

    // **The pin is one bit on the one type, and the three gates answer it
    // alike** (owner ruling 2026-09-23). Every menu pinned, the hand gone from
    // all three: no gate owes anything and the loop is told of no wake-up (A3).
    // Then each menu goes, and each gate is back to the gate it was before any
    // of this — no fourth state, and no gate remembering how its menu opened.
    let mut pinned = ChevronGates::default();
    for popup in [Popup::Profile, Popup::Pane, Popup::File] {
        pinned.gate(popup).expect("a `⌄` governs this menu").pin();
    }
    pinned.observe(
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, true),
        start,
    );
    assert_eq!(pinned.profile, pinned.pane, "one policy for the pin too");
    assert_eq!(pinned.pane, pinned.rail, "and the rail's pill is in it");
    assert_eq!(
        pinned.deadline(),
        None,
        "a pinned menu registers no wake deadline"
    );
    for popup in [Popup::Profile, Popup::Pane, Popup::File] {
        pinned.menu_gone(popup);
    }
    assert_eq!(pinned, ChevronGates::default());
    for popup in [
        Popup::Root,
        Popup::GraphFilter,
        Popup::Preview,
        Popup::GitMenu,
        Popup::TermMenu,
        Popup::Tab,
        Popup::Palette,
    ] {
        assert!(
            pinned.gate(popup).is_none(),
            "{popup:?} is raised by no `⌄` and has no pin"
        );
    }
}

/// PIN (**menu-openers hover, actions click**) — user ruling, 2026-09-10.
///
/// The preview rail's `Open` pill expands
/// [`profiles::FileMenuSubject::Document`] and does nothing else, so by the
/// owner's principle of this day — 「展开菜单的控件 hover 就开,执行动作的控
/// 件必须点」 — it is a menu-opener and it rests open. It had been click-only
/// since it was drawn, for no reason but that it is spelled with a word
/// instead of with a `⌄`: the 2026-08-16 ruling drew its boundary around the
/// *glyph*, and this one redraws it around the *behaviour*.
///
/// So the pill is enrolled in the very clock the two `⌄` already run on
/// rather than given a second one, and that is what is asserted: the rail
/// gate is driven through the same [`ChevronGates::observe`] and answers the
/// same verbs at the same instants as the strip's. A second clock at 250ms
/// would pass a test that only checked the pill; it cannot pass one written
/// as an equality against the chevron beside it.
///
/// Red gate: give the pill its own `Duration` or its own `observe` call and
/// the equalities fail; delete the rail arm and the `Open` answers vanish.
#[test]
fn the_rails_open_pill_rests_open_on_the_chevrons_own_clock() {
    use profiles::{ChevronAction, ChevronPointer};
    let start = Instant::now();

    // A rest on the pill and a rest on the strip's `⌄`, told to the gates in
    // one call: the same deadline, the same verb, the same instant.
    let mut resting = ChevronGates::default();
    resting.observe(
        (ChevronPointer::Button, false),
        (ChevronPointer::Away, false),
        (ChevronPointer::Button, false),
        start,
    );
    assert_eq!(
        resting.rail, resting.profile,
        "one policy: the pill's clock and the chevron's are the same state"
    );
    assert_eq!(
        resting.rail.deadline(),
        Some(start + profiles::CHEVRON_HOVER_OPEN_DELAY)
    );
    assert_eq!(
        resting.rail.due(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        Some(ChevronAction::Open),
        "resting on `Open` for the ruling's quarter second raises the \
             document menu"
    );

    // A hand that left before the rest matured has raised nothing and owes
    // nothing — leaving a shut control clears the clock outright rather than
    // pausing it, so coming back starts the quarter second again from zero.
    let mut left_early = resting;
    left_early.observe(
        (ChevronPointer::Away, false),
        (ChevronPointer::Away, false),
        (ChevronPointer::Away, false),
        start + profiles::CHEVRON_HOVER_OPEN_DELAY - Duration::from_millis(1),
    );
    assert_eq!(left_early.rail.deadline(), None);
    assert_eq!(
        left_early
            .rail
            .due(start + profiles::CHEVRON_HOVER_OPEN_DELAY * 4),
        None,
        "no menu is ever raised by a rest the hand did not finish"
    );

    // The pointer moving into the menu the pill opened keeps it up: on the
    // surface, no clock runs in either direction.
    let mut on_menu = ChevronGates::default();
    on_menu.observe(
        (ChevronPointer::Away, false),
        (ChevronPointer::Away, false),
        (ChevronPointer::Surface, true),
        start,
    );
    assert_eq!(
        on_menu.rail.deadline(),
        None,
        "a hand on the menu is a hand still dealing with the pill"
    );

    // And leaving both of them runs the chevrons' own 150ms grace.
    let mut leaving = ChevronGates::default();
    leaving.observe(
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, true),
        (ChevronPointer::Away, true),
        start,
    );
    assert_eq!(leaving.rail, leaving.profile);
    assert_eq!(
        leaving.rail.due(start + profiles::CHEVRON_LEAVE_GRACE),
        Some(ChevronAction::Close)
    );
}

/// RED (35) — **a second press on a `⌄` pins a peek, and closes only a pinned
/// menu.**
///
/// Ruling 4 of 2026-09-23: 「"再点即收"只在钉住态成立」. Since 2026-09-13 a press
/// on the control a popover hangs from was spent closing it; that is still the
/// rule for a pinned menu, and for every popover no `⌄` governs. For a peek —
/// a menu a rest raised — the same press pins it instead, and the menu stays.
/// Driven through the press rule the router asks
/// ([`press_spends_itself_closing`], then [`press_pins_a_peek`]) for each of
/// the three controls.
///
/// MUTATION: make `press_pins_a_peek` return `verdict` unchanged and the first
/// assertion goes red (the peek is spent closing, as on `main`).
#[test]
fn a_second_click_closes_only_a_pinned_menu() {
    let start = Instant::now();
    for (popup, control) in the_three_chevrons() {
        let mut gates = peek_open(popup, start);
        let first = press_pins_a_peek(
            press_spends_itself_closing(chevron_button(control), Some(control)),
            gates.gate(popup),
        );
        assert_eq!(
            first,
            OwnPress::Pinned,
            "{popup:?}: a press on a peek pins it"
        );
        assert!(!first.dismisses() && first.ends_the_press());
        assert!(gates.gate(popup).is_some_and(|gate| gate.is_pinned()));

        let second = press_pins_a_peek(
            press_spends_itself_closing(chevron_button(control), Some(control)),
            gates.gate(popup),
        );
        assert_eq!(
            second,
            OwnPress::Spent,
            "{popup:?}: a press on a pinned menu closes it"
        );
        assert!(second.dismisses() && second.ends_the_press());
    }
    // A popover no `⌄` governs keeps the 2026-09-13 rule untouched.
    let filter = PopoverTrigger::Chrome(seats::ChromeTarget::FilesRoot(SeatId(1)));
    assert_eq!(
        press_pins_a_peek(
            press_spends_itself_closing(chevron_button(filter), Some(filter)),
            None
        ),
        OwnPress::Spent
    );
}

/// RED (35) — **a click elsewhere closes a pinned menu, and the pin goes with
/// it**, for the pane head's `⌄` and the tab strip's.
///
/// A press that lands on anything but the menu's own button is
/// [`OwnPress::Elsewhere`] whatever the pin says: the dismissal arm puts the
/// menu away through its closer and the press goes on being the press it was.
/// That includes a press on *another* pane head's `⌄`, which is how a pinned
/// menu moves across a split — the toggle opens the new head's menu in place
/// of the old one, and that press pins the new one.
///
/// MUTATION: make `press_pins_a_peek` pin on any verdict (drop the
/// `verdict == OwnPress::Spent` guard) and the first assertion goes red —
/// a click elsewhere would stop dismissing.
#[test]
fn a_click_elsewhere_closes_a_pinned_menu() {
    let start = Instant::now();
    for (popup, control) in the_three_chevrons().into_iter().take(2) {
        let mut gates = peek_open(popup, start);
        gates.gate(popup).expect("governed").pin();
        for elsewhere in [
            None,
            Some(PopoverTrigger::Chrome(seats::ChromeTarget::Settings)),
            Some(PopoverTrigger::Chrome(seats::ChromeTarget::PaneMenu(
                SeatId(9),
            ))),
        ] {
            let verdict = press_pins_a_peek(
                press_spends_itself_closing(chevron_button(control), elsewhere),
                gates.gate(popup),
            );
            assert_eq!(
                verdict,
                OwnPress::Elsewhere,
                "{popup:?} pressed at {elsewhere:?}"
            );
            assert!(verdict.dismisses() && !verdict.ends_the_press());
        }
        // The dismissal.
        gates.menu_gone(popup);
        assert!(gates.gate(popup).is_some_and(|gate| !gate.is_pinned()));
    }
    // And the arms that dismiss do it through the closers that drop the pin.
    let router = method_body("Runtime", "mouse_input");
    assert!(router.contains("self.close_pane_menu()?"));
    assert!(router.contains("self.close_profile_menu()?"));
    assert!(router.contains("self.close_file_menu()?"));
}

/// PIN (**the card column reaches the peek's one predicate, and only it**) —
/// user ruling and report with screenshot, 2026-08-21.
///
/// A layout peek was dropping over the focus column, covering the two cards
/// below the one under the pointer. `peek_strip::eligible` now refuses a
/// window whose cards are unfolded — `a_column_of_unfolded_cards_refuses_
/// every_peek` is that policy driven directly — and what is left to pin here
/// is the *wiring*, which has two halves and can only be read as text:
///
/// * the posture reaches the predicate, from `rail_posture()` — the same
///   join the solver and every geometry are handed — rather than from
///   `window.focus_mode`, so nothing in this window has a second opinion
///   about whether a column is on screen;
/// * and `layout_peek_target_at` adds **no** judgment of its own. It is the
///   arming path; `hide_layout_peek`'s side is the retiring one; and
///   `layout_peek_eligible`'s own doc says why one predicate serves both —
///   "the two asking different questions is exactly how a popup survives the
///   death of its own subject". A peek that settled a frame before focus
///   mode came on would, with a second `if focus` at the arming site only,
///   have nobody left to retire it.
///
/// Red gate: pass `self.window.focus_mode` instead and the first assertion
/// fails by name; add the second author at the call site and the third does.
#[test]
fn the_focus_column_refuses_the_layout_peek_through_one_predicate() {
    let predicate = method_body("Runtime", "layout_peek_eligible");
    assert!(
        predicate.contains("self.rail_posture().draws_focus_rail(),"),
        "the peek's one predicate is told whether this window's cards are \
             unfolded, and told it by the posture every other geometry reads"
    );
    let arming = method_body("Runtime", "layout_peek_target_at");
    assert!(
        arming.contains("self.layout_peek_eligible(tab)"),
        "the arming path asks the one predicate"
    );
    assert!(
        !arming.contains("focus"),
        "and asks nothing else: a second author here is a peek the retiring \
             path can no longer take down"
    );
}

/// PIN (**E61 — one popup at a time**, now including both `⌄` menus).
///
/// The list is the rule: whatever is being raised, every other popup goes.
/// Six openers used to carry six hand-copied runs of `self.x = None` and no
/// two of them agreed — the pane menu left the preview switcher up, the root
/// menu left the file menu up — so what is pinned here is not a set of pairs
/// but the *completeness*: for each popup, `others()` is exactly the rest of
/// the list, and adding one without listing it breaks this test rather than
/// shipping a pair that can be up together.
///
/// Red gate: drop one arm of `ALL` and the count assertion goes red; return
/// a hand-written subset from `others` and the "every other popup" assertion
/// names the one that got away.
#[test]
fn opening_any_popup_closes_every_other_one() {
    assert_eq!(
        Popup::ALL.len(),
        10,
        "nine popups and the command palette, and this list is the rule"
    );
    for keep in Popup::ALL {
        let closed: Vec<Popup> = keep.others().collect();
        assert_eq!(
            closed.len(),
            Popup::ALL.len() - 1,
            "{keep:?} closes every popup but itself"
        );
        assert!(
            !closed.contains(&keep),
            "{keep:?} must not close the popup it is raising — that is what                  makes a toggle possible through the same door as an open"
        );
        for other in Popup::ALL {
            assert_eq!(
                other != keep,
                closed.contains(&other),
                "{keep:?} against {other:?}"
            );
        }
    }
    // The two chevron menus are on the list, which is the ruling's own
    // requirement: a hover-opening surface that could coexist with another
    // popup would be a menu a pointer drops on top of an open one.
    assert!(Popup::ALL.contains(&Popup::Profile));
    assert!(Popup::ALL.contains(&Popup::Pane));
    // And so is the git context menu (v2 (4)) — it is raised by a right
    // press, which is a gesture no other popup answers, so it is exactly the
    // one that could otherwise have come up on top of an open menu.
    assert!(Popup::ALL.contains(&Popup::GitMenu));
    // And the terminal's own menu (ticket #62), which is the one raised
    // *inside a pane* — the surface every other popup on this list is drawn
    // over, and therefore the one that could otherwise have come up
    // underneath an open menu rather than on top of it.
    assert!(Popup::ALL.contains(&Popup::TermMenu));
    // And a tab's own menu (丙2), which is the one raised *on the tab list* —
    // the surface the profile picker's own list hangs beside — and therefore
    // the one that could otherwise have come up next to an open picker
    // rather than instead of it.
    assert!(Popup::ALL.contains(&Popup::Tab));
}

/// PIN — **F57 survives N160①: a pin that arrives by merge still leads the
/// strip.**
///
/// The user's report, as a list. Four tabs, the first one pinned; a pinned
/// tab is dragged into the *last* one's layout, so "pin follows content"
/// (N160①) pins a tab that does not move. What the strip then read was
/// `pinned, unpinned, unpinned, pinned` — the pins at slots 1 and 4 with
/// unpinned tabs between them, which is the screenshot exactly.
///
/// The two clamps cannot catch this and it is worth saying why: both
/// [`strip_insert_slot`] and [`partition_clamped`] rule on where a tab may
/// *land*, and here nothing landed. Only a flag changed, on a tab standing
/// still, so the repair has to be the one thing neither clamp does — put the
/// run back in its partition.
///
/// Order is asserted by identity and not merely by flag: a normalization that
/// got the partition right by shuffling the two unpinned tabs would satisfy
/// `pins_are_normalized` and still be wrong, because `normalize_pins` is
/// documented stable and the tabs nobody touched must not move.
///
/// Red gate: drop the `settle_pin_partition` call at the foot of
/// [`absorb_tab_into_strip`] and the merged tab stays at slot 3 behind two
/// unpinned tabs.
#[test]
fn a_pin_arriving_by_merge_still_leads_the_strip() {
    // A partitioned strip: two pinned tabs lead, two plain ones follow. The
    // second pinned tab is the one about to be dragged away.
    let mut tabs = vec![
        cross_tab(2, &["ALREADY"]),
        cross_tab(1, &["SRCA"]),
        cross_tab(3, &["PLAIN"]),
        cross_tab(5, &["TARGET"]),
    ];
    tabs[0].pinned = true;
    tabs[1].pinned = true;
    assert!(
        seed::pins_are_normalized(&tabs, |tab| tab.pinned),
        "the strip starts partitioned, so nothing below is an inherited mess"
    );

    // The target is the tab on screen — the merge's own precondition (K129)
    // — and it is the last one in the run, which is what leaves the arriving
    // pin behind an unpinned tab.
    let (source_index, mut active_tab) = (1, 3);
    let source_seats = tabs[source_index].seats.clone();
    let arrived = cross_merge(
        &source_seats,
        &mut tabs[active_tab],
        seats::LayoutAim::SeatEdge(SeatId(1), seats::DropEdge::Right),
    );
    let (from, into) = two_tabs_mut(&mut tabs, source_index, active_tab);
    let ejected = absorb_tab_into_layout(from, into, &arrived, None, TabId(9), cross_solve);
    assert!(ejected.is_none(), "an edge landing displaces nothing");
    assert!(
        tabs[active_tab].pinned,
        "N160(1) still holds: the pin followed the content"
    );

    absorb_tab_into_strip(&mut tabs, &mut active_tab, source_index, ejected);

    assert!(
        seed::pins_are_normalized(&tabs, |tab| tab.pinned),
        "F57: the pinned run leads the strip"
    );
    assert_eq!(
        tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![TabId(2), TabId(5), TabId(3)],
        "the newly pinned tab joins the pinned run, and the tab nobody \
             touched keeps its place behind it"
    );
    assert_eq!(
        tabs[active_tab].id,
        TabId(5),
        "the active tab is followed by identity across the reorder, not by index"
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

/// PIN — **P86: letting go where you picked it up is a clean "never mind".**
///
/// The user's report of 2026-07-17 is what this is for: without a home
/// rectangle, retracting a drag lands it on the source's own edge zone and
/// splits a pane anyway, so there is no gesture at all for "actually, no".
/// It is K135's sentence generalised from a seat identity to a rectangle,
/// which is what a payload that is not a pane needs — and it is the seam an
/// inline image drag plugs its `srcRect` into unchanged (P86's second half).
///
/// Mutation: make the bounds exclusive on the far edges — a release on the
/// row's own last pixel column stops being a retraction, which is the one
/// place a hand that has travelled exactly six pixels tends to end up.
#[test]
fn a_payload_let_go_over_its_own_ground_lands_nowhere() {
    let home = Some([100.0, 200.0, 300.0, 220.0]);
    assert!(over_home_ground(home, at(150.0, 210.0)), "inside");
    assert!(over_home_ground(home, at(100.0, 200.0)), "its own corner");
    assert!(over_home_ground(home, at(300.0, 220.0)), "and its far one");
    assert!(!over_home_ground(home, at(301.0, 210.0)), "just outside");
    assert!(
        !over_home_ground(None, at(150.0, 210.0)),
        "a source with no home ground retracts nowhere: a tab goes back to its slot"
    );
}

/// PIN (R31's third invalidation moment) — **the two automatic triggers read
/// only what somebody is looking at, and the command one reads only the
/// repository the command was run in.**
///
/// The showing gate is the same one the first reading keeps
/// ([`columns_wanting_git`]) and it is kept a second time here on purpose: a
/// file changing is not on its own a reason to spend a subprocess. What is
/// new is the asymmetry between the two triggers, and it is the whole reason
/// there are two — a command end carries a folder to compare against, and a
/// window coming back does not, because what happened while it was away
/// happened in another process.
#[test]
fn a_command_and_a_focus_re_read_only_the_repositories_on_screen() {
    let page = SeatId(1);
    let tree = SeatId(2);
    let repo = PathBuf::from(r"D:\repo");
    let other = PathBuf::from(r"D:\other");
    let graph = PathBuf::from(r"D:\repo");
    let surfaces = vec![
        (GitOrigin::Column(page), repo.clone(), true),
        // Same tab, same window, on its Files page: available, not showing.
        (GitOrigin::Column(tree), other.clone(), false),
        (GitOrigin::Graph(graph.clone()), graph.clone(), true),
    ];

    assert_eq!(
        git_surfaces_wanting_reread(false, &surfaces, None),
        Vec::new(),
        "with the master switch off nothing is read, however many pages are up"
    );

    // B: the window came back. Every showing surface, and nothing else.
    assert_eq!(
        git_surfaces_wanting_reread(true, &surfaces, None),
        vec![GitOrigin::Column(page), GitOrigin::Graph(graph.clone())],
        "a column on its tree is not a surface looking at a repository"
    );

    // A: a command ended in a pane standing in the repository the page shows.
    assert_eq!(
        git_surfaces_wanting_reread(
            true,
            &surfaces,
            Some(&[PathBuf::from(r"D:\repo\crates\bt-app")])
        ),
        vec![GitOrigin::Column(page), GitOrigin::Graph(graph)],
        "a subdirectory of the root is inside the root"
    );
    assert_eq!(
        git_surfaces_wanting_reread(true, &surfaces, Some(std::slice::from_ref(&other))),
        Vec::new(),
        "a command that ended in the folder the *hidden* page is rooted at \
             reads nothing: that page is not showing, and the one that is shows \
             another repository"
    );
    assert_eq!(
        git_surfaces_wanting_reread(true, &surfaces, Some(&[PathBuf::from(r"D:\repository")])),
        Vec::new(),
        "and the folder next door whose name merely starts the same way is \
             not inside it"
    );
    assert_eq!(
        git_surfaces_wanting_reread(true, &surfaces, Some(&[])),
        Vec::new(),
        "no shell finished anywhere: nothing to re-read"
    );
}

/// **The highlighting is on the width-free side of the key** (#49).
///
/// A resize changes `body_width_px` and nothing else, and `PreviewParseKey`
/// is what a re-parse is gated on — so a drag cannot re-walk a grammar. An
/// edit changes the revision, which is in the parse key, so it can and must.
/// This is the same argument [`MarkdownBlockIntrinsic`] was created for, made
/// again for the thing that was added to it.
#[test]
fn a_resize_cannot_re_walk_a_grammar_and_an_edit_must() {
    let mut buffer = text_buffer("main.rs", "fn main() {}\n");
    let narrow = document_key(&buffer, false, 400.0, 1.0);
    let wide = document_key(&buffer, false, 1200.0, 1.0);
    assert_ne!(narrow, wide, "the two widths are two documents");
    assert_eq!(
        narrow.parse, wide.parse,
        "but one parse — which is the half the highlighting hangs off"
    );
    buffer.edit_content(|content| {
        content.insert(0, 'p');
        true
    });
    assert_ne!(
        document_key(&buffer, false, 400.0, 1.0).parse,
        narrow.parse,
        "and an edit re-walks it"
    );
}

/// PIN (v2 ④) — **a ref verb git refused is one card carrying git's own
/// sentence**, raised off the answer and nowhere else.
///
/// The named verbs ride the same `Write` answer the four pathspec verbs do,
/// which is the whole reason nothing new had to be taught to the notice
/// path: `git branch -d` on an unmerged branch comes back as a refused
/// write, and the window prints what git said rather than paraphrasing a
/// program that has already explained itself.
///
/// MUTATION: paraphrase the refusal ("could not delete the branch") and this
/// goes red on the exact bytes — which is the only way a reader can search
/// for the sentence they were shown.
#[test]
fn a_ref_verb_git_refused_says_gits_own_words_once() {
    let root = PathBuf::from(r"D:\repo");
    let refusal = "error: the branch 'goner' is not fully merged.";
    let refused = git::GitAnswer::Write {
        root: root.clone(),
        verb: git::GitWriteVerb::DeleteBranch {
            name: "goner".to_owned(),
        },
        paths: Vec::new(),
        outcome: Err(git::GitFault::Refused(refusal.to_owned())),
    };
    assert_eq!(
        git_answer_notice(&refused).as_deref(),
        Some(refusal),
        "one notice, in git's words"
    );
    let went_through = git::GitAnswer::Write {
        root,
        verb: git::GitWriteVerb::CreateBranch {
            name: "feature".to_owned(),
            at: "a1b2c3d".to_owned(),
        },
        paths: Vec::new(),
        outcome: Ok(()),
    };
    assert_eq!(
        git_answer_notice(&went_through),
        None,
        "and a verb that worked says nothing — the page redrawing is the report"
    );
}

/// PIN (v2 ④) — a repo-relative path becomes the path a person would type,
/// which is what the clipboard and Explorer are both handed.
///
/// git speaks forward slashes on every machine; `/select` and a pasted path
/// want the platform's own separator. One function, so the copy and the
/// reveal cannot spell the same file two ways.
#[test]
fn a_repo_relative_path_is_joined_in_the_grammar_the_platform_reads() {
    let root = Path::new(r"D:\repo");
    let full = git_full_path(root, "crates/bt-app/src/main.rs");
    assert!(full.starts_with(root));
    assert!(
        full.ends_with("main.rs"),
        "the file is still the file: {full:?}"
    );
    assert_eq!(
        full.components().count(),
        root.components().count() + 4,
        "and every folder git named is a folder of the path: {full:?}"
    );
}

// ── W2 slice ③: a page is a preview buffer, in every list a file is in ──

/// A pool holding a file and a page, which is the state every claim below is
/// about.
fn a_pool_with_a_file_and_a_page() -> preview::PreviewPool {
    let mut pool = preview::PreviewPool::default();
    pool.insert(preview::PreviewBuffer::new(
        preview::PreviewSource::file(r"C:\work\notes.md"),
        "notes.md".to_owned(),
    ));
    pool.insert(preview::PreviewBuffer::new(
        preview::PreviewSource::Web("http://localhost:5173/app".to_owned()),
        "Folio site".to_owned(),
    ));
    pool
}

/// **A page and a file are the same kind of row** (`docs/DESIGN.md` §7.7 ⑤ —
/// 「同一张列表、同一个索引空间」).
///
/// One list, one index space, one order: the pool's. What differs between the
/// two rows is the two things that genuinely differ — the mark, and which
/// category the pin writes — and the test says both out loud because a build
/// that filed a page under `file` would put it in the section the *root menu*
/// draws from.
///
/// Red gate: `preview_menu_target` answering `PinKind::File` for every source
/// makes the category assertion fail; `preview_row_mark` answering `File` for
/// every row makes the mark assertion fail.
#[test]
fn the_switcher_lists_a_page_in_the_same_table_as_a_file() {
    let rows = switcher_rows(&a_pool_with_a_file_and_a_page(), None, &[]);
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        vec!["notes.md", "Folio site"],
        "the pool's own order, both kinds of row in it"
    );
    assert_eq!(
        rows.iter().filter_map(|row| row.pool).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        rows[0].keep.as_ref().map(|keep| keep.kind),
        Some(bt_persist::PinKind::File)
    );
    assert_eq!(
        rows[1].keep.as_ref().map(|keep| keep.kind),
        Some(bt_persist::PinKind::Url),
        "a page is kept as a page, not as a file with a strange name"
    );
    assert_eq!(
        rows[1].keep.as_ref().map(|keep| keep.target.as_str()),
        Some("http://localhost:5173/app"),
        "and what it keeps is the switcher key, verbatim"
    );
    assert!(!rows[0].is_page() && rows[1].is_page());
    assert_eq!(
        marks::preview_row_mark(rows[1].is_page(), None),
        marks::ChromeMark::Globe { favicon: None }
    );
    assert_eq!(
        marks::preview_row_mark(rows[0].is_page(), None),
        marks::ChromeMark::File,
        "and the file beside it is unchanged"
    );
    // **And the row can name the site it would ask about** (the favicon
    // slice): the address it keeps is the address the store is keyed by, so
    // the row needs nothing this list was not already holding.
    assert_eq!(rows[1].page_url(), Some("http://localhost:5173/app"));
    assert_eq!(rows[0].page_url(), None);
    assert_eq!(
        marks::preview_row_mark(rows[0].is_page(), Some(favicon::FaviconId::for_tests(1))),
        marks::ChromeMark::File,
        "and an icon handed to a file is refused rather than drawn"
    );
}

/// **A kept page that is also open is one row, not two** (user ruling
/// 2026-08-19: 「已钉 URL 再次出现提升同一条目,PINNED 与 MRU 不留双副本」).
///
/// The lift compares the *pair* — category and target — because `pins.json`
/// is one array and a row is identified by both. A kept page nobody has
/// opened is still a row, which is what makes the section worth having on the
/// first frame after a restart, and it is named by its site because the pin
/// file stores a place and never a title.
///
/// Red gate: make `preview_menu_target` answer one category for everything —
/// the open page stops matching its own kept row and is listed twice, once
/// above under its site and once below under its title.
#[test]
fn a_kept_page_that_is_open_is_lifted_rather_than_copied() {
    let kept = vec![
        (
            bt_persist::PinKind::Url,
            "http://localhost:5173/app".to_owned(),
        ),
        (
            bt_persist::PinKind::Url,
            "http://127.0.0.1:8080/report".to_owned(),
        ),
    ];
    let rows = switcher_rows(&a_pool_with_a_file_and_a_page(), None, &kept);
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        vec!["Folio site", "127.0.0.1:8080", "notes.md"],
        "the two kept pages first, then what is left of the pool"
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.keep.as_ref().map(|keep| keep.target.as_str())
                == Some("http://localhost:5173/app"))
            .count(),
        1,
        "the open page appears once, lifted rather than copied"
    );
    assert_eq!(
        rows[0].pool,
        Some(1),
        "and the lifted row still points at the buffer it came from"
    );
    assert_eq!(
        rows[1].pool, None,
        "while the kept page nobody has opened has no buffer behind it"
    );
    assert!(rows[0].pinned && rows[1].pinned && !rows[2].pinned);
}

/// **A local page is kept by the same door that presses it** (§7.7 ⑤;
/// `plan.md` §3「钉不是授权」).
///
/// The bug this is the gate for: the switcher's pin did nothing at all on a
/// `file:///…/demo.html` row. W2 slice ⑤ gave the *press* a second door —
/// [`page_destination`], which takes a `file:` string back to the disk and
/// mints it again — and left the *pin* asking [`switcher_row_destination`],
/// which is `webnav::address_bar` and refuses `file:` from every door and
/// always will. So the two validations the design calls "the same door" were
/// two different doors, and the one that decides what reaches `pins.json`
/// was the one that cannot say yes to a local page. Nothing was drawn,
/// nothing was written, and the only report was an `eprintln!` nobody sees.
///
/// RED GATE: put [`switcher_row_destination`] back in
/// [`switcher_pin_is_allowed`] and the first assertion fails — the row the
/// user pressed is refused, exactly as it was on the real machine.
#[test]
fn a_local_page_is_kept_by_the_same_door_that_presses_it() {
    let dir = bt_testpath::temp_path("folio-switcher-pin");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let page = dir.join("sidebar-focus-demo.html");
    std::fs::write(&page, b"<h1>demo</h1>").expect("a page on a disk");
    let canonical = std::fs::canonicalize(&page).expect("canonicalise it");
    let url = webnav::Mint::file(&canonical)
        .expect("a local path mints")
        .target()
        .expect("a mint names its URL")
        .to_owned();

    assert!(
        switcher_pin_is_allowed(bt_persist::PinKind::Url, &url),
        "the row the user pressed reaches pins.json: {url}"
    );
    assert!(
        page_destination(&url).is_some(),
        "and it is the press door that said so, which is the whole claim"
    );
    // A page reached over the network is unchanged by any of this.
    assert!(switcher_pin_is_allowed(
        bt_persist::PinKind::Url,
        "https://example.com/report#ch3"
    ));

    // The three refusals that survive, each for its own reason. A `file:`
    // row is admitted by being taken back to the disk, so a string the disk
    // does not answer for is still refused — and now visibly.
    let missing = url.replace("sidebar-focus-demo", "never-existed");
    assert!(
        !switcher_pin_is_allowed(bt_persist::PinKind::Url, &missing),
        "a page whose file is not there is not kept: {missing}"
    );
    // A local file that is not a page could never have been a page row, and
    // the pin door asks the same question the lane fork asked to make one.
    let notes = dir.join("notes.md");
    std::fs::write(&notes, b"# notes\n").expect("a document on a disk");
    let notes_url =
        webnav::Mint::file(&std::fs::canonicalize(&notes).expect("canonicalise the document"))
            .expect("a local path mints")
            .target()
            .expect("a mint names its URL")
            .to_owned();
    assert!(
        !switcher_pin_is_allowed(bt_persist::PinKind::Url, &notes_url),
        "a document is not a page, however it is spelled: {notes_url}"
    );
    // And the file going away takes the pin's answer with it, which is the
    // press door's own answer for the same row.
    std::fs::remove_file(&page).expect("take the page away");
    assert!(!switcher_pin_is_allowed(bt_persist::PinKind::Url, &url));
    assert_eq!(page_destination(&url), None, "both doors, one answer");
    let _ = std::fs::remove_dir_all(&dir);
}

/// RED — **a tick whose only news is a decoded picture still reaches the
/// glass** (the freeze of 2026-08-28; §7.44 ③).
///
/// The defect this pins was found by photographing the machine: press play,
/// take your hand off the mouse, and the recording runs for two seconds and
/// then stops dead, while the decoder behind it goes on for as long as you
/// leave it. Two seconds is `VIDEO_BAR_IDLE_REST + VIDEO_BAR_FADE` — the
/// control bar's dwell and fade — because while the bar is up its clock and
/// its scrubber change the *chrome*, and the picture was only ever reaching
/// the glass as a passenger on that.
///
/// Two halves, because the fault had two places to live and mending one
/// without the other mends nothing:
///
/// ① **The rule.** A picture's debt alone is enough, exactly as a chrome
/// change alone is and a pane in flight alone is — and a tick carrying none
/// of the three presents nothing, which is what keeps an idle window at
/// zero.
///
/// ② **The call site asks it.** Read out of the source, because the gate is
/// a `return` inside a method that cannot be called without a window: what
/// there is to assert is that `advance_strip_animation` puts its question
/// through [`tick_owes_a_present`] and hands it the picture's debt.
///
/// RED GATE ①: return `chrome_changed || panes_owe` from
/// `tick_owes_a_present` and the first block fails. RED GATE ②: write the
/// gate back as `!self.refresh_chrome() && !panes_owe` and the second block
/// fails — which is the state the binary that froze was built from.
#[test]
fn a_video_frame_alone_is_enough_to_present() {
    // ① the rule.
    assert!(
        tick_owes_a_present(false, false, true),
        "a decoded picture and nothing else is still a frame the glass is owed"
    );
    assert!(tick_owes_a_present(true, false, false), "the chrome moved");
    assert!(
        tick_owes_a_present(false, true, false),
        "a pane is in flight"
    );
    assert!(
        !tick_owes_a_present(false, false, false),
        "and a tick with no news at all presents nothing — an idle window \
             costs what it costs because of this half"
    );

    // ② the call site asks it.
    let tick = method_body("Runtime", "advance_strip_animation");
    assert!(
        tick.contains("tick_owes_a_present(self.refresh_chrome(), panes_owe, pictures_owe)"),
        "the chrome gate asks the whole question, with the picture's debt in it"
    );
    // **The debt is now written down between two passes** (closure review
    // O4, 2026-09-18): the pictures are serviced above every gate, so what
    // reaches this gate is what the service recorded — including a frame
    // that arrived on a turn the gate refused, which nothing else would ever
    // come back for.
    assert!(
        tick.contains("std::mem::take(&mut self.window.pictures_owe_a_frame)"),
        "and the picture's debt is a name that survives as far as that gate"
    );
    let service = method_body("Runtime", "service_pictures");
    assert!(
        service.contains("if frames_arrived || boxes_moved {")
            && service.contains("self.window.pictures_owe_a_frame = true;"),
        "and the service is what writes it down"
    );
}

/// RED — **one recording is one seat, and a seat is at home on any of the
/// three surfaces** (user ruling 2026-08-28: *「视频在 hover 卡、固定浮窗、侧边
/// 预览 pane 三个表面用同一个引擎与同一张画、同一套手势」*; §7.44 ①).
///
/// The whole of "one model, three surfaces", stated as the two halves it
/// actually decomposes into:
///
/// ① **The model does not know which surface it is on.** The same door —
/// `VideoSeats::open` — is taken for a docked pane, a floating window and
/// the glance card, and what comes back answers the same questions with the
/// same types. There is no per-surface branch to get wrong because there is
/// no per-surface type.
///
/// ② **And they are still three pictures.** Three surfaces over one file
/// are three seats with three engines and three texture names, because they
/// are three rectangles that may be at three playheads. A build that
/// "shared" the seat between surfaces would show one picture in three places
/// and stop one of them stopping all three.
///
/// RED GATE ①: key the map by path instead of by surface and the second
/// `open` returns the first seat — the count is one, the keys are equal, and
/// two of the three surfaces are drawing somebody else's playhead. RED GATE
/// ②: leave the engines to `Drop` at the end of the test instead of
/// `shutdown_all` and `engines_outstanding` never comes back to where it
/// started, which is §7.42 ⑦'s counter noticing a leak this slice could
/// introduce three times over.
#[test]
fn a_video_is_one_seat_on_three_surfaces() {
    use bt_platform::video::engine::engines_outstanding;
    let _ledger = ledger_gate();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/assets/folio-video-test.mp4");
    let before = engines_outstanding();
    let mut seats = video_seat::VideoSeats::default();
    let surfaces = [
        PreviewSurface::Seat(LeafId {
            tab: TabId(1),
            seat: SeatId(2),
        }),
        PreviewSurface::Float(9),
        PreviewSurface::Peek,
    ];
    let now = Instant::now();
    for surface in surfaces {
        seats
            .open(surface, &fixture, now)
            .unwrap_or_else(|error| panic!("{surface:?} opens the fixture: {error:?}"));
    }
    // ① one door, one shape of answer, three surfaces.
    let mut keys = std::collections::BTreeSet::new();
    for surface in surfaces {
        let seat = seats
            .get(surface)
            .unwrap_or_else(|| panic!("{surface:?} holds a seat"));
        assert_eq!(seat.path(), fixture, "{surface:?}");
        // The same questions, answered for every surface alike.
        let _ = seat.state();
        let _ = seat.is_sounding();
        let _ = seat.presence(now, Motion::Full);
        keys.insert(seat.key().to_owned());
    }
    // ② three pictures, not one shared between three boxes.
    assert_eq!(keys.len(), 3, "three surfaces are three textures: {keys:?}");
    assert_eq!(
        engines_settling_to(before + 3),
        before + 3,
        "three surfaces are three decoders"
    );
    // And closing one closes exactly one.
    assert!(seats.close(surfaces[1]));
    assert!(seats.get(surfaces[1]).is_none());
    assert!(
        seats.get(surfaces[0]).is_some(),
        "and leaves the others alone"
    );
    assert_eq!(engines_outstanding(), before + 2);
    seats.shutdown_all();
    assert_eq!(
        engines_outstanding(),
        before,
        "and no engine outlives the surfaces it was opened for"
    );
}

/// RED — **a card dragged into a window carries its engine with it** (user
/// ruling 2026-08-28: *「拖头转浮窗时把引擎带走(不重开,位置不丢)」*; §7.44 ③).
///
/// The one behaviour in this slice that a reader can see and a reviewer
/// cannot infer. A glance card playing a recording, dragged by its head six
/// pixels, becomes a floating window — and the ruling is that what arrives
/// in that window is *the same playback*: not restarted, not re-decoded, not
/// back at zero.
///
/// Four assertions, and each is one way a re-open would give itself away:
///
/// 1. **No engine was started and none was shut down.** §7.42 ⑦'s two
///    counters are process-wide and monotone, so "the same engine" is a
///    thing this test can state in arithmetic rather than by looking at a
///    pointer.
/// 2. **The texture kept its name.** A key derived from the surface would
///    change here, the renderer would release one texture and upload
///    another, and the reader would see one black frame.
/// 3. **The playhead did not go back.** A re-opened engine starts at zero;
///    this one is where it was.
/// 4. **The card is empty afterwards.** A `rehome` that copied rather than
///    moved would leave a decoder running for a card that has gone.
///
/// RED GATE: replace `rehome` in `promote_file_peek` with a `close` and an
/// `open` — which is what the door did before this slice, and is what
/// `open_preview_onto` would do on its own — and assertions 1, 2 and 3 all
/// fail at once.
#[test]
fn a_card_torn_off_carries_its_engine_with_it() {
    use bt_platform::video::engine::{engines_shut_down, engines_started};
    let _ledger = ledger_gate();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/assets/folio-video-test.mp4");
    let mut seats = video_seat::VideoSeats::default();
    let now = Instant::now();
    seats
        .open(PreviewSurface::Peek, &fixture, now)
        .expect("the card opens the fixture");
    // Let the clock get off zero, so that "the playhead did not go back" is
    // a claim with something in it. The wait ends the moment the playhead
    // moves; the ceiling only has to outlast a cold decoder on a loaded
    // shared runner, where the first frame has taken longer than five
    // seconds.
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if seats
            .get(PreviewSurface::Peek)
            .is_some_and(|seat| seat.state().position_secs > 0.0)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let card = seats.get(PreviewSurface::Peek).expect("a seat on the card");
    let key = card.key().to_owned();
    let was = card.state().position_secs;
    assert!(was > 0.0, "the fixture is playing before the tear-off");
    let started = engines_started();
    let stopped = engines_shut_down();

    let float = PreviewSurface::Float(4);
    assert!(seats.rehome(PreviewSurface::Peek, float), "the tear-off");

    // ① nothing was started and nothing was stopped.
    assert_eq!(engines_started(), started, "a second engine was opened");
    assert_eq!(
        engines_shut_down(),
        stopped,
        "the first engine was shut down"
    );
    let window = seats.get(float).expect("the window holds the seat now");
    // ② the texture kept its name.
    assert_eq!(
        window.key(),
        key,
        "the picture was released and re-uploaded"
    );
    assert_eq!(window.path(), fixture);
    // ③ the playhead did not go back to zero.
    assert!(
        window.state().position_secs >= was,
        "the playback restarted: {was} became {}",
        window.state().position_secs
    );
    // ④ and the card is holding nothing.
    assert!(seats.get(PreviewSurface::Peek).is_none());
    seats.shutdown_all();
}

/// PIN (user ruling 2026-08-23) — **a page comes back from a session file as
/// a page, whichever of the two ways it was stored.**
///
/// The restore path is the one door `open_preview_source_on` is not on: a
/// tab's pool is seeded before the window that could host an engine exists,
/// so the turn-around happens where the pages are revived instead. Two
/// spellings reach it and both are the same file — the `Url` row every page
/// has written since W2 slice ⑤, and the `File` row a `.html` dropped on a
/// pane wrote while `.html` was still a document.
///
/// A real file in a real directory, because `canonicalize` is the step under
/// test — the row is a name and never a permission, so what authorises the
/// load is a mint made from the disk this instant.
///
/// RED GATE: drop the `source_opens_as_a_page` arm from [`revived_page_of`]
/// and the `File` half answers `None` — a restored tab comes back showing the
/// "no preview for this file type" card over a page it had rendered before
/// the restart.
#[test]
fn a_restored_page_comes_back_as_a_page_however_it_was_stored() {
    let dir = bt_testpath::temp_path("folio-page-revival");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let page = dir.join("report.html");
    std::fs::write(&page, b"<h1>x</h1>").expect("a page on a disk");
    let canonical = std::fs::canonicalize(&page).expect("canonicalise it");
    let minted = webnav::Mint::file(&canonical).expect("a local path mints");
    let url = minted.target().expect("a mint names its URL").to_owned();

    assert_eq!(
        revived_page_of(&preview::PreviewSource::Web(url.clone())),
        Some((url.clone(), minted.clone())),
        "a page stored as a page comes back as one, exactly as it always did"
    );
    assert_eq!(
        revived_page_of(&preview::PreviewSource::file(&page)),
        Some((url.clone(), minted)),
        "and a page stored as a file comes back as the same page, minted \
             from the same disk"
    );
    // A document is not a page and is revived by the head-read lane, so this
    // door answers nothing for it.
    let notes = dir.join("notes.md");
    std::fs::write(&notes, b"# notes\n").expect("a document on a disk");
    assert_eq!(revived_page_of(&preview::PreviewSource::file(&notes)), None);
    // And a row naming a file that is not there goes nowhere, which is the
    // answer §7.9 ⑤ already gives for a row this window would not navigate to.
    std::fs::remove_file(&page).expect("take the file away");
    assert_eq!(revived_page_of(&preview::PreviewSource::file(&page)), None);
    assert_eq!(revived_page_of(&preview::PreviewSource::Web(url)), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// PIN (user ruling 2026-08-23) — **the glance card says what the row opens
/// as.**
///
/// The defect this closes was visible without running anything: a `.pdf` row
/// drew "No preview — binary or unrecognized type." under a resting pointer
/// and opened a rendered page under a double click. The card and the row are
/// two readings of one gesture, so they read it through one function
/// ([`preview_open_lane`]) and the card can no longer contradict the door.
///
/// **The card no longer stops at saying it** (user ruling 2026-08-25): the
/// page class splits on whether the file's own bytes are readable, and each
/// half shows what it has. `.html` shows its markup — it is text, and it goes
/// down the very lane every text file goes down — while `.pdf` shows the two
/// facts a binary container can still state. The chip and the foot are
/// untouched by both, so the row still says `web` and still says how to open
/// it.
///
/// RED GATE: delete the `Web` arm of [`peek_body_kind`] — `.pdf` falls to the
/// refusal arm (its buffer has no reader) and says "no preview" over a row a
/// double click renders. Point both halves of
/// [`preview::path_page_glance`] at `Source` and a `.pdf` card asks the
/// document pipeline for a body no reader in this window can build.
#[test]
fn the_glance_card_says_what_the_row_opens_as() {
    let kind = |name: &str, refused: bool| {
        let path = PathBuf::from(format!(r"D:\site\{name}"));
        peek_body_kind(preview::preview_ftype(name), Some(&path), refused, false)
    };
    // **A page whose bytes are text shows them.** One lane, the document's,
    // and no branch of its own below this line.
    for name in ["index.html", "index.htm", "INDEX.HTM"] {
        assert_eq!(
            kind(name, false),
            PeekBodyKind::Document,
            "a page made of text shows its source: {name}"
        );
        // And when that read came back refused — a binary body under an
        // `.html` name, a file that went away — the refusal is what the card
        // owes, exactly as for any other document.
        assert_eq!(kind(name, true), PeekBodyKind::Refused, "{name}");
    }
    // **A page made of nothing this window reads states its facts instead**,
    // and states them whatever the buffer behind it holds: there is no read
    // to be refused, because nothing is read.
    for name in ["report.pdf", "REPORT.PDF"] {
        assert_eq!(kind(name, false), PeekBodyKind::Facts, "{name}");
        assert_eq!(kind(name, true), PeekBodyKind::Facts, "{name}");
    }
    // The regression half — every other class draws exactly what it drew.
    assert_eq!(kind("notes.md", false), PeekBodyKind::Document);
    assert_eq!(kind("main.rs", false), PeekBodyKind::Document);
    assert_eq!(kind("notes.md", true), PeekBodyKind::Refused);
    assert_eq!(kind("a.exe", false), PeekBodyKind::Refused);
    assert_eq!(kind("shot.png", false), PeekBodyKind::Picture);
    assert_eq!(kind("index.htmlx", false), PeekBodyKind::Refused);
    assert_eq!(kind("report.html.txt", false), PeekBodyKind::Document);
    // A page on a share is not a page: the mint refuses it, so the card is
    // the refusal the seat would have shown.
    assert_eq!(
        peek_body_kind(
            preview::PreviewFtype::Web,
            Some(Path::new(r"\\server\share\index.html")),
            true,
            false,
        ),
        PeekBodyKind::Refused
    );
    // A composed document has no path, so it is drawn as the document it is
    // however its name is spelled.
    assert_eq!(
        peek_body_kind(preview::PreviewFtype::Web, None, false, false),
        PeekBodyKind::Document
    );
    assert_eq!(
        peek_body_kind(preview::PreviewFtype::Image, None, false, false),
        PeekBodyKind::Refused,
        "and a picture with no file is still a picture nothing can decode"
    );

    // **And the glance really reads with the glance's buffer.** The ladder
    // above says a `.html` row shows a document; what makes a document
    // *arrive* is the buffer the card is armed with, and a card armed with a
    // pane's buffer would ask no disk at all and sit empty for ever — the
    // one failure the ladder cannot see. A fact about this file, so it is
    // read off this file, exactly as the pool's own door is two tests up.
    let arming = method_body("Runtime", "mature_file_peek");
    assert!(
        arming.contains("preview::PreviewBuffer::glancing("),
        "the glance arms itself with a pane's buffer, so a page's source is \
             never read and the card stays empty:\n{arming}"
    );
}

/// PIN (W2 slice 5) - **a stored row naming a local page is taken back to
/// the disk, never trusted as a string.**
///
/// [`page_destination`] is what a switcher row, a `session.json` line and a
/// hand-edited pin all leave by, and its `file:` arm does the same three
/// steps the files column does: decode to a path, canonicalise it against
/// the disk, mint from *that*. So what authorises the load is a mint the
/// host made this instant, and the row contributes a name and nothing more.
///
/// The three refusals below are the shape of that: a path that is not there
/// has nothing to canonicalise, a `..` in the string never reaches the disk
/// at all, and a percent escape this door does not write is a URL somebody
/// else built.
///
/// A real file in a real directory, because `canonicalize` is the step under
/// test and there is no way to ask it about a disk that is not there.
///
/// RED GATE: return `Some((target.to_owned(), Mint::Nothing))` for a `file:`
/// string - the round trip below still passes and every refusal fails, which
/// is the whole difference between naming a file and being allowed to load
/// one.
#[test]
fn a_stored_local_page_is_minted_from_the_disk_and_not_from_the_row() {
    let dir = bt_testpath::temp_path("folio-slice5-page-destination");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let page = dir.join("report.html");
    std::fs::write(&page, b"<h1>x</h1>").expect("a page on a disk");
    let canonical = std::fs::canonicalize(&page).expect("canonicalise it");
    let minted = webnav::Mint::file(&canonical).expect("a local path mints");
    let url = minted
        .target()
        .expect("a file mint names its URL")
        .to_owned();

    let (destination, carried) = page_destination(&url).expect("a row naming a file that is there");
    assert_eq!(destination, url, "the same page, minted again");
    assert_eq!(carried, minted, "and the mint travels with it");

    // The fragment a local report's table of contents uses is the page's own
    // business and survives the trip.
    let with_fragment = format!("{url}#chapter-3");
    assert_eq!(
        page_destination(&with_fragment).map(|(url, _)| url),
        Some(with_fragment),
        "the page answers for its own fragment"
    );

    std::fs::remove_file(&page).expect("take the file away");
    assert_eq!(
        page_destination(&url),
        None,
        "a row naming a file that is not there goes nowhere"
    );
    for hostile in [
        "file:///C:/site/../../Windows/win.ini",
        "file:///C:/site/%2e%2e/secret.html",
        "file://server/share/page.html",
        "file:///",
    ] {
        assert!(
            page_destination(hostile).is_none(),
            "not a string this door minted: {hostile}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The switcher's identity is the recovery machine's field, and there is no
/// second account of it** (`plan.md` §3 与 §4).
///
/// "The URL a session file may record" and "what the switcher calls this
/// seat" are one sentence, so they are one field. This drives the machine
/// through the three things that must *not* move it — a navigation in flight,
/// a failure page, an `about:blank` — and the one that must, and reads the
/// switcher's answer off the machine each time.
///
/// Red gate: let `WebMachine::on_navigation_completed` write its
/// `recoverable_url` whatever `success` said, and the two assertions about a
/// page that never loaded both move.
#[test]
fn the_switchers_identity_is_the_machines_last_committed_url() {
    let mut machine = webhost::WebMachine::new();
    machine.request("http://LocalHost:5173/app?tab=logs#top");
    let generation = machine.generation();
    machine.on_environment(generation, true);
    machine.on_controller(generation, true);
    machine.on_events_installed(generation);
    assert_eq!(
        webnav::switcher_identity(machine.recoverable_url()),
        None,
        "a navigation that has not committed has no identity — a row for a \
             page that never existed is a row the switcher cannot honour"
    );
    machine.on_navigation_completed(generation, "http://localhost:5173/app?tab=logs#top", true);
    assert_eq!(
        webnav::switcher_identity(machine.recoverable_url()),
        Some("http://localhost:5173/app?tab=logs#top".to_owned()),
        "query and fragment participate, and only a default port is dropped"
    );
    machine.request("http://localhost:5173/gone");
    assert_eq!(
        webnav::switcher_identity(machine.recoverable_url()),
        Some("http://localhost:5173/app?tab=logs#top".to_owned()),
        "asking for a page is not being on it"
    );
    machine.on_navigation_completed(generation, "http://localhost:5173/gone", false);
    machine.on_navigation_completed(generation, "about:blank", true);
    assert_eq!(
        webnav::switcher_identity(machine.recoverable_url()),
        Some("http://localhost:5173/app?tab=logs#top".to_owned()),
        "a failure page and a blank page are neither of them where you are"
    );
    // A redirect: what committed is the identity, not what was asked for.
    machine.request("http://localhost:5173/old");
    machine.on_navigation_completed(generation, "http://localhost:5173/new", true);
    assert_eq!(
        webnav::switcher_identity(machine.recoverable_url()),
        Some("http://localhost:5173/new".to_owned()),
        "after a redirect the seat is where it landed"
    );
}

/// PIN (§7.7 ⑨) — **"never been anywhere" is the recovery machine's own
/// state and not a second account kept beside it.**
///
/// The withdrawal has to be able to tell a page the person walked away from
/// apart from a page they typed an address into, and the difference is
/// already a field: `recoverable_url` is written by a successful navigation
/// and the line that writes it names `about:blank` in order to refuse it. So
/// a blank page's identity stays `None` however many times the engine says it
/// arrived, and the first real address ends the door.
///
/// MUTATION: let `on_navigation_completed` write `about:blank` through — the
/// first assertion goes red, and every `Ctrl+Shift+L` becomes unwithdrawable
/// the moment its blank page finishes loading.
#[test]
fn a_blank_page_is_a_page_that_has_never_been_anywhere() {
    let mut machine = webhost::WebMachine::new();
    machine.request(webnav::BLANK_PAGE);
    let generation = machine.generation();
    machine.on_environment(generation, true);
    machine.on_controller(generation, true);
    machine.on_events_installed(generation);
    machine.on_navigation_completed(generation, webnav::BLANK_PAGE, true);
    assert_eq!(
        machine.recoverable_url(),
        None,
        "the host's own scaffolding is not somewhere a person went"
    );
    machine.on_navigation_completed(generation, "http://localhost:5173/app", true);
    assert_eq!(
        machine.recoverable_url(),
        Some("http://localhost:5173/app"),
        "and the first address that is one ends the door"
    );
}

/// **A hole is only ever cut where a floor already stands** (§7.14; user
/// ruling 2026-08-25, from a photograph of a first-opened page showing the
/// desktop).
///
/// The hole and the floor were on two different clocks and only one of them
/// was the pane's. A seat's rectangle exists the moment its pane does, so
/// `sync_web_page` cut the hole on the first frame; the floor was minted by
/// `attach_web_visual`, which runs when WebView2 hands back a controller —
/// hundreds of milliseconds later on a first open, and never at all if the
/// engine fails to arrive. In between, the pane is a rectangle nothing in
/// the composition tree paints, over a `topmost = true` target on a
/// per-pixel-alpha HWND: the desktop, at full size, for as long as it takes.
///
/// **Measured on the machine before the fix** (release, cold profile,
/// isolated `APPDATA`/`LOCALAPPDATA`, a magenta board window behind Folio):
/// twelve presented frames carried the hole with `engine_up=false`, spanning
/// 403 ms, and the camera caught the board filling the whole pane body from
/// 88.5 ms to 514.6 ms — 120 056 sampled pixels, the pane's body exactly.
/// Two runs out of two. After the fix, two runs out of two: not one board
/// pixel inside the frame, peak zero.
///
/// The fix is that the placement now answers the hole — `WebSeat::place`
/// returns whether a floor stands — and this is the sentence that reads that
/// answer. It is pure, so unlike the pane it decides for it can be held
/// here rather than described.
///
/// MUTATIONS:
/// ① make [`super::hole_for`] ignore `floored` — the first assertion goes
///    red, and that is exactly the shipped build the user photographed;
/// ② let it cut a hole for a `Hidden` page — the second goes red, and a page
///    behind a modal or on a background tab becomes a window-shaped window.
#[test]
fn a_hole_is_only_cut_where_a_floor_already_stands() {
    let bounds = super::webhost::WebBounds {
        x: 12,
        y: 34,
        width: 500,
        height: 400,
    };
    let shown = super::webhost::WebPresence::Shown(bounds);

    assert_eq!(
        super::hole_for(shown, false, None),
        None,
        "a page whose floor is not down yet is still given a hole, which is \
             a rectangle of desktop inside this window"
    );
    assert_eq!(
        super::hole_for(shown, true, None).map(|hole| hole.rect),
        Some(bounds.as_rect()),
        "a page standing on its own floor is given no hole, so the page \
             nobody can see is hosted perfectly"
    );
    assert_eq!(
        super::hole_for(super::webhost::WebPresence::Hidden, true, None),
        None,
        "a hidden page is cut a hole anyway"
    );
}

/// Reading a periodic appointment does not move it; only spending it does.
/// This is the startup poll's instance of the deadline-fold rule.
#[test]
fn an_unchanged_periodic_owner_answers_the_same_absolute_instant() {
    let epoch = Instant::now();
    let mut appointment = epoch + STARTUP_PTY_POLL_INTERVAL;
    let first = appointment;
    let second = appointment;
    assert_eq!(first, second, "two reads did not rebase the startup poll");

    advance_periodic_deadline(
        &mut appointment,
        epoch + STARTUP_PTY_POLL_INTERVAL,
        STARTUP_PTY_POLL_INTERVAL,
    );
    assert_eq!(
        appointment,
        epoch + STARTUP_PTY_POLL_INTERVAL * 2,
        "the owner advances exactly when its appointment is spent",
    );
}

/// The named fold preserves both the absolute winner and its evidence label.
/// Supplying the same owner snapshots at two different query instants has no
/// place from which to manufacture a different answer.
#[test]
fn an_unchanged_deadline_fold_answers_the_same_named_instant() {
    let epoch = Instant::now();
    let names = ["later", "winner", "absent"];
    let entries = [Some(epoch + Duration::from_secs(2)), Some(epoch), None];
    let ask = |_now| earliest_named_deadline(names, entries);
    let first = ask(epoch);
    let second = ask(epoch + Duration::from_secs(1));
    assert_eq!(first, Some(("winner", epoch)));
    assert_eq!(second, first);
}

/// Red gates for every query-time renewal that was an entry in the baseline
/// fold. The names make a failure identify all entries sharing that source;
/// the behavioral tests in `pace`, `debounce`, and above pin their replacement.
#[test]
fn deadline_owners_do_not_manufacture_appointments_from_the_query_time() {
    for (owners, retired_form) in [
        ("startup poll", "map(|delay| now + delay)"),
        (
            "strip, tooltip, key hint, Cards hint, toast, command flash, command rails, terminal thumbs, file peek, file-peek close and dwell, float, formula tools, formula toggle, refused frame",
            "next_animation_frame(now)",
        ),
        (
            "strip animation",
            "now + self.window.frame_clock.interval()",
        ),
        ("pane motion fold entry", "pane_motion.deadline("),
    ] {
        assert_eq!(
            in_product(&found(needle!(Pattern::text(retired_form)), View::Raw)),
            0,
            "{owners} still renew from the fold query time via `{retired_form}`",
        );
    }
    let drag = method_body("Runtime", "drag_autoscroll_deadline");
    assert!(
        !drag.contains(".map_or(now") && !drag.contains(".unwrap_or(now"),
        "drag auto-scroll still renews from the fold query time:\n{drag}",
    );
    assert!(
        found_in(
            needle!("Instant::now() + SESSION_DEBOUNCE"),
            View::Raw,
            Scope::Module("crate::persist".to_owned()),
        )
        .is_empty(),
        "session save still renews from the time its deadline is queried",
    );
}

/// The macOS menu memo is decided entirely from inputs. An unchanged turn is
/// rejected here, before plan construction and before any AppKit call.
#[test]
fn unchanged_main_menu_inputs_are_silent_before_appkit() {
    let shortcuts = shortcuts::Shortcuts::defaults();
    let focus = Some(shortcuts::Focus::default());
    let memo = MainMenuInputs::new(&shortcuts, focus);
    assert!(memo.matches(&shortcuts, focus));

    let mut rebound = shortcuts.clone();
    rebound.set("new-tab", None);
    assert!(
        !memo.matches(&rebound, focus),
        "a rebound chord rebuilds the menu"
    );
    assert!(
        !memo.matches(&shortcuts, None),
        "a focus change rebuilds the menu"
    );

    let stale_language = MainMenuInputs {
        language_revision: memo.language_revision.wrapping_sub(1),
        ..memo
    };
    assert!(
        !stale_language.matches(&shortcuts, focus),
        "a language revision rebuilds the menu",
    );
}

/// RED (ticket 12, user ruling 2026-09-20) — **a plain click on the foot finds
/// the file in the files column and selects it, by the locate verb's three
/// arms.**
///
/// The press is decided by [`peek_foot_press`] and landed by [`files_locate`] —
/// the decision [`Runtime::locate_folder_in_files_column`] makes, the verb every
/// "show this in the files column" in this window already means (rule 9: the
/// existing verb, not a second one). Its range rule is the 2026-08-25 ruling
/// 「打开文件不许重根文件树」: a folder inside the column's tree keeps the root and
/// selects the file's row under it; a folder outside it re-roots the column at
/// the folder and selects the file's row there; a tab with no column gets one
/// rooted at the folder, with the file's row selected.
///
/// MUTATION: route the plain click to the reveal door (answer
/// `PeekFootPress::Reveal` for `ClickIntent::Here` in `peek_foot_press`) — the
/// first assertion goes red.
#[test]
fn a_click_on_the_foot_selects_the_file_in_the_files_column() {
    let (folder, file) = glance_fixture("locate");
    for host in [RowHost::Column(SeatId(1)), RowHost::Terminal(SeatId(2))] {
        let Some(PeekFootPress::Locate {
            folder: at,
            file: name,
        }) = peek_foot_press(host, &file, false)
        else {
            panic!("a plain click on {host:?}'s foot must stay in the window and locate");
        };
        assert_eq!(at, folder.join("notes"));
        assert_eq!(name, "plan.md");

        // ① Inside the tree the column already shows: the root stays, and the
        //    file's row under the way down is the one selected.
        let root = folder.display().to_string();
        assert_eq!(
            files_locate(Some((SeatId(7), &root)), &at, Some(&name)),
            FilesLocate::Inside {
                seat: SeatId(7),
                select: "/notes/plan.md".to_owned(),
            }
        );
        // ② Outside it: the column is rooted at the folder, and the file's row
        //    — a child of the new root — is selected.
        let elsewhere = std::env::temp_dir()
            .join("folio-glance-foot-elsewhere")
            .display()
            .to_string();
        assert_eq!(
            files_locate(Some((SeatId(7), &elsewhere)), &at, Some(&name)),
            FilesLocate::Root {
                select: Some("/plan.md".to_owned()),
            }
        );
        // ③ No column at all: one is opened there, the same answer.
        assert_eq!(
            files_locate(None, &at, Some(&name)),
            FilesLocate::Root {
                select: Some("/plan.md".to_owned()),
            }
        );
    }
    // And the folder-only callers are the verb they always were.
    let root = folder.display().to_string();
    assert_eq!(
        files_locate(Some((SeatId(7), &root)), &folder.join("notes"), None),
        FilesLocate::Inside {
            seat: SeatId(7),
            select: "/notes".to_owned(),
        }
    );
    assert_eq!(
        files_locate(None, &folder, None),
        FilesLocate::Root { select: None }
    );
    let _ = std::fs::remove_dir_all(&folder);
}

/// RED (ticket 12, owner ruling 2026-09-23) — **the files column stays where the
/// foot took it when the card goes.**
///
/// The 2026-09-20 note said 「不在当前根下就让文件列临时切过去」, and "temporarily"
/// could be read as "switch back when the card goes". The owner settled it: "The
/// files column does not switch back after the glance-foot click; it is
/// navigation, not a peek." So the press takes the card down and then locates,
/// and nothing on the card's way down touches the column: the card holds no
/// root to restore, and the door that ends its life names no files-column verb.
///
/// MUTATION: have `hide_file_peek` re-root the column at a remembered root (a
/// `reroot_files_column` or `show_folder_in_files_column` call) — the second
/// loop goes red.
#[test]
fn the_column_stays_where_the_foot_took_it_when_the_card_goes() {
    let press = method_body("Runtime", "press_file_peek_foot");
    let down = press
        .find("self.hide_file_peek();")
        .expect("the foot's press takes the card down");
    let locate = press
        .find("self.locate_folder_in_files_column(&folder, Some(&file))")
        .expect("and locates through the existing verb");
    assert!(
        down < locate,
        "the card goes before the column moves, so it is never placed against a row that moved"
    );
    for verb in [
        "reroot_files_column",
        "show_folder_in_files_column",
        "seat_a_files_column",
        ".root =",
    ] {
        assert!(
            !press.contains(verb),
            "the foot moves the column only through the locate verb — found `{verb}`"
        );
    }
    for verb in [
        "reroot_files_column",
        "show_folder_in_files_column",
        "locate_folder_in_files_column",
        ".root =",
    ] {
        assert!(
            !method_body("Runtime", "hide_file_peek").contains(verb),
            "taking the card down moves the files column (`{verb}`): it would switch back"
        );
    }
}

/// RED (ticket 12, user ruling 2026-09-20) — **`Ctrl` (`⌘`) and a click on the
/// foot reveals the file, selected, through the door the card's surface already
/// uses for its own hand-over.**
///
/// A files row, a Git row and a folder card's row reveal through
/// [`Runtime::reveal_in_explorer`], the door a row's own menu takes; a reference
/// a program printed reveals through [`Runtime::reveal_verified`], with the
/// pane's ledger — exactly as the reference itself does under `Ctrl`, so the
/// card does not become a way round audit 3 C-2's rule. The file is revealed,
/// not its folder: Explorer and Finder open the folder with the file selected.
///
/// MUTATION: answer `Reveal` for a terminal host (the files door) — the second
/// assertion goes red; drop the reveal call from `press_file_peek_foot` — the
/// body pin does.
#[test]
fn a_ctrl_click_on_the_foot_reveals_the_file() {
    let (folder, file) = glance_fixture("reveal");
    for host in [
        RowHost::Column(SeatId(1)),
        RowHost::Float(3),
        RowHost::Git(SeatId(4)),
    ] {
        assert_eq!(
            peek_foot_press(host, &file, true),
            Some(PeekFootPress::Reveal(file.clone())),
            "{host:?}"
        );
    }
    assert_eq!(
        peek_foot_press(RowHost::Terminal(SeatId(2)), &file, true),
        Some(PeekFootPress::RevealVerified(SeatId(2), file.clone())),
        "a printed reference is handed over off its ledger"
    );
    let press = method_body("Runtime", "press_file_peek_foot");
    for door in [
        "self.reveal_in_explorer(&path);",
        "let facts = self.verified_target(seat, &path);",
        "self.reveal_verified(&path, facts);",
        "input::pointer_chord_held(self.window.modifiers_held)",
    ] {
        assert!(
            press.contains(door),
            "the foot's press reaches `{door}` rather than a door of its own"
        );
    }
    let _ = std::fs::remove_dir_all(&folder);
}

/// RED (ticket 20) — **the command palette's field and rows are primary
/// text, not half a point above and below it.**
///
/// `UI-SPEC.md` T3/T4: the field used to sit at 13.5 and the rows at 12.5,
/// where every menu item, tree row, tab, button and combo is 13.
/// `palette.rs` has no test module of its own, so this lives here.
///
/// MUTATION: revert `palette::FIELD_FONT_LOGICAL_PX` or
/// `palette::ROW_FONT_LOGICAL_PX` to a literal and this goes red.
#[test]
fn ui_spec_palette_class_a_values_follow_the_rule() {
    assert_eq!(crate::palette::FIELD_FONT_LOGICAL_PX, 13.0, "UI-SPEC.md T3");
    assert_eq!(crate::palette::ROW_FONT_LOGICAL_PX, 13.0, "UI-SPEC.md T4");
}

/// RED (27) — **Palette rows use list corners and the shared icon-to-label gap.**
///
/// UI-SPEC.md R3 and G3 replace the palette's own 7-point corners and
/// 7/9-point gaps. The restore row radius is private, so its rule is pinned
/// numerically without changing its visibility.
///
/// MUTATION: restore palette::DOT_GAP_LOGICAL_PX to 7.0.
/// MUTATION: restore palette::ROW_GAP_LOGICAL_PX to 9.0.
/// MUTATION: restore palette::ROW_RADIUS_LOGICAL_PX to 7.0.
#[test]
fn ui_spec_palette_rest_values_follow_the_rule() {
    assert_eq!(
        crate::palette::DOT_GAP_LOGICAL_PX,
        8.0,
        "UI-SPEC.md G3: the label-to-dot gap is 8"
    );
    assert_eq!(
        crate::palette::ROW_GAP_LOGICAL_PX,
        8.0,
        "UI-SPEC.md G3: the mark-to-label gap is 8"
    );
    assert_eq!(
        crate::palette::ROW_RADIUS_LOGICAL_PX,
        6.0,
        "UI-SPEC.md R3: restore::ROW_RADIUS_LOGICAL_PX is the list-row rule"
    );
}

/// The scales a real display runs at, and three pane widths in logical pixels: a
/// narrow pane (the report's, squeezed beside a graph pane), a medium one and a
/// wide one. Odd numbers, so no width lands on a cell boundary by luck.
const RAIL_SCALES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];
const RAIL_LOGICAL_WIDTHS: [u32; 3] = [331, 797, 1913];

/// RED (ticket 32) — **with a rail, the last text column ends left of the rail's
/// resting band, at every width and scale.**
///
/// The owner's ruling of 2026-09-23 is that decoration never covers text. The
/// grid used to reserve only its symmetric `padding_px`, while the rail stands
/// inboard of the eight-pixel scroll lane — so a line that filled the pane ran
/// under the ticks, and a failed command's rose tick sat on its last letters
/// (ticket 15's `12-crop-rail-over-text.png`). The rail here is the real
/// [`cmdrail::lay_out`] of a real ledger, and the grid is the one function every
/// seat-to-grid site asks. The reserve is the resting band's width
/// ([`cmdrail::Rail::bounds`]), not the hot crest's (owner, 2026-09-23). It is
/// also no wider than it has to be: one more column would cross the band.
///
/// MUTATION: return `metrics.grid_for_pixels(body.width, body.height)`
/// unconditionally from `cmdrail::terminal_grid_for` — red at the narrow width
/// (and at every other: the resting tick always overlapped the old last column).
#[test]
fn a_rail_never_covers_the_last_text_column() {
    let leaf = leaf_saying(RAIL_FAILED_THEN_PROMPT);
    let stack = cmdrail::commands(leaf.session.command_marks());
    assert!(
        stack
            .entries
            .iter()
            .any(|entry| entry.signal == cmdrail::Signal::Fail),
        "the fixture must hold the failed command whose rose tick the report saw"
    );
    let mut fonts = bt_render::preview_measure_font_system();
    for scale in RAIL_SCALES {
        let metrics = bt_render::CellMetrics::measure(&mut fonts, scale).unwrap();
        for logical in RAIL_LOGICAL_WIDTHS {
            let body = rail_test_body(logical, scale);
            let edges = [
                body.x as f32,
                body.y as f32,
                (body.x + body.width) as f32,
                (body.y + body.height) as f32,
            ];
            let rail = cmdrail::lay_out(edges, &stack, scale as f32, None);
            assert!(!rail.ticks.is_empty(), "a ledger with marks draws a rail");
            let grid = cmdrail::terminal_grid_for(&metrics, body, true);
            let columns = f32::from(grid.columns.get());
            let text_right = edges[0] + metrics.padding_px + columns * metrics.cell_width_px;
            assert!(
                text_right <= rail.bounds[0],
                "{logical} logical px at {scale}x: the last column ends at {text_right}, \
                 the rail's resting band starts at {}",
                rail.bounds[0]
            );
            for tick in &rail.ticks {
                assert!(
                    tick.rect[0] >= text_right,
                    "{logical} logical px at {scale}x: a tick starts at {} inside the text",
                    tick.rect[0]
                );
            }
            assert!(
                text_right + metrics.cell_width_px > rail.bounds[0],
                "{logical} logical px at {scale}x: the reserve took a column the band \
                 does not stand on"
            );
        }
    }
}

/// PIN (ticket 32) — **a pane with no rail keeps exactly the grid it always had.**
///
/// `cmd.exe` without its prompt marks, a WSL shell without the init file and a
/// program run bare never send a mark, and the ruling changes nothing for them:
/// the same columns and rows [`bt_render::CellMetrics::grid_for_pixels`] gives the
/// rectangle, at every width and scale the rail test uses.
///
/// MUTATION: reserve the band whatever `has_rail` says — red at every width.
#[test]
fn without_a_rail_the_grid_is_unchanged() {
    let mut fonts = bt_render::preview_measure_font_system();
    for scale in RAIL_SCALES {
        let metrics = bt_render::CellMetrics::measure(&mut fonts, scale).unwrap();
        for logical in RAIL_LOGICAL_WIDTHS {
            let body = rail_test_body(logical, scale);
            assert_eq!(
                cmdrail::terminal_grid_for(&metrics, body, false),
                metrics.grid_for_pixels(body.width, body.height),
                "{logical} logical px at {scale}x"
            );
        }
    }
}

/// RED (ticket 32) — **every seat-to-grid site asks the one function.**
///
/// Four places turn a seat rectangle into a grid — a leaf's birth, the panes on
/// screen, the focused pane and every hidden tab's panes. Each used to call
/// `grid_for_pixels` itself, which is how a rule about the rail could have been
/// honoured at three of them and forgotten at the fourth. Read through
/// `bt_source`, product files only, comments masked.
///
/// MUTATION: put back one raw `renderer.metrics().grid_for_pixels(...)` at any of
/// the four sites — red.
#[test]
fn every_seat_to_grid_site_asks_the_one_function() {
    let calls = found(needle!(Pattern::call("grid_for_pixels")), View::Identifiers)
        .in_the_product(source());
    let owners: Vec<String> = calls
        .owners(source())
        .into_iter()
        .map(|(identity, count)| format!("{}×{count}", identity.name))
        .collect();
    assert_eq!(
        owners,
        vec!["terminal_grid_for×2".to_owned()],
        "{}",
        calls.report(source())
    );
    assert_eq!(calls.outside_items(source()), 0);
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

/// PIN (47) — **a resize present still refuses the frame composed before the resize, and is
/// silent once paid.**
///
/// The gate's own purpose, which reading the grid at validation keeps: the pane is re-solved into
/// a new rectangle, and the frame composed at the old grid is refused while the present is owed.
/// Once a present has landed nothing is owed, and a frame of any grid is admitted.
///
/// MUTATION: make `TabState::admit_resize_present` return `Ok(())` without asking
/// `frame_matches_grid` — red at the first refusal.
#[test]
fn a_resize_present_refuses_the_frame_composed_before_the_resize() {
    let mut fonts = bt_render::preview_measure_font_system();
    let metrics = bt_render::CellMetrics::measure(&mut fonts, 1.0).unwrap();
    let body = rail_test_body(797, 1.0);
    let mut tab = tab_holding(leaf_saying(
        "a line before the resize
",
    ));
    let before = focused_frame(&mut tab);
    tab.admit_resize_present(&before)
        .expect("nothing is owed before a resize");
    resolve_focused_pane(&mut tab, &metrics, body);
    tab.owe_resize_present();
    assert!(!frame_matches_grid(&before, tab.focused().unwrap().grid));
    assert!(
        tab.admit_resize_present(&before).is_err(),
        "the frame composed at the old grid is refused while the resize present is owed"
    );
    let after = focused_frame(&mut tab);
    tab.admit_resize_present(&after)
        .expect("the frame of the new grid is admitted");
    // The first frame that reaches the glass pays the debt (`Runtime::redraw`'s commit arm).
    tab.resize_present_owed = false;
    tab.admit_resize_present(&before)
        .expect("a paid debt gates nothing");
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

/// RED (0.4.4 ticket 02) — **while the card is up, a key other than Enter, Tab, Shift+Tab or Esc
/// reaches nothing and the card stays** (owner's rulings 2026-09-23; `Shift+Tab` joined the keys
/// with the two-button-dialog ruling of the same day).
///
/// The key's meaning is `paste_card_key`; that it reaches nothing is the rung: it stands above
/// every road to a shell in `keyboard_input` and returns whatever the key was.
///
/// MUTATION: map `Key::Character` to `Cancel` in `paste_card_key`, or take the `return Ok(())`
/// out of the card's rung.
#[test]
fn a_key_other_than_enter_tab_or_esc_reaches_nothing_and_leaves_the_card_up() {
    use winit::keyboard::ModifiersState;
    let none = ModifiersState::empty();
    for key in [
        Key::Character("a".into()),
        Key::Character("v".into()),
        Key::Named(NamedKey::Space),
        Key::Named(NamedKey::Backspace),
        Key::Named(NamedKey::ArrowUp),
        Key::Named(NamedKey::F5),
    ] {
        assert_eq!(
            paste_card_key(&key, none, PasteAnswer::RunLineByLine),
            None,
            "{key:?}"
        );
    }
    for (key, modifiers) in [
        (Key::Character("v".into()), ModifiersState::CONTROL),
        (Key::Named(NamedKey::Enter), ModifiersState::SHIFT),
        (Key::Named(NamedKey::Enter), ModifiersState::CONTROL),
        (Key::Named(NamedKey::Tab), ModifiersState::CONTROL),
        (Key::Named(NamedKey::Tab), ModifiersState::ALT),
        (Key::Named(NamedKey::Escape), ModifiersState::ALT),
    ] {
        assert_eq!(
            paste_card_key(&key, modifiers, PasteAnswer::RunLineByLine),
            None,
            "{key:?} {modifiers:?}"
        );
    }
    // The card is still up after them: nothing took the paste.
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    assert_eq!(
        paste_text_into(&mut tab, target, THREE_LINES, true),
        StagedPaste::Held
    );
    assert!(pending_paste_in(&tab).is_some());

    let ladder = squeezed_body("Runtime", "keyboard_input");
    let rung = "ifself.paste_card_seat().is_some(){if!event.repeat&&letSome(focus)=self.paste_card_focus()&&letSome(key)=paste_card_key(&event.logical_key,self.window.modifiers,focus){self.press_paste_card_key(key)?;}returnOk(());}";
    let at = ladder
        .find(rung)
        .unwrap_or_else(|| panic!("the card's rung is not whole"));
    for road in [
        "self.paste_from_clipboard()?;",
        "self.copy_selection()?;",
        "send_user_input(",
    ] {
        if let Some(later) = ladder.find(road) {
            assert!(at < later, "`{road}` is reached before the card's rung");
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

/// RED (57) — **With the restore card up, a press on a pane beneath changes nothing: not the
/// focus, not what the shell is sent, not the tab.**
///
/// Owner's ruling 2026-09-25: the card is a full-window gate, as the paste card is, and owns the
/// pointer too. On BASE its press arm returned only for a press that landed on the card
/// (`restore::hit` answered `Some`), and every other press went on to the chrome router — the tab
/// strip, the pane that takes the focus, the program's mouse report. Now the arm stands in the
/// router where the paste card's does and returns for every press, answering only on its two
/// buttons; the wheel under it is nobody's (one reading, `a_modal_covers_the_window`, which the
/// card is on); and nothing under it lights on a hover.
///
/// The pure half runs the card's own layout and hit test: a press on the window beside the card
/// hits nothing, so it answers nothing. The routing half is pinned on `mouse_input`,
/// `mouse_wheel` and `pointer_moved`, read through `bt_source` — no `Runtime` can be built without
/// a window.
///
/// MUTATION: put back BASE's arm (`&& let Some(target) = restore::hit(..)` in the `if let`, so the
/// arm is taken only on the card) — the arm is no longer whole and its pin goes red.
#[test]
fn with_the_restore_card_up_a_press_on_a_pane_beneath_changes_nothing() {
    let content = restore::RestoreContent {
        rows: Vec::new(),
        sub_lines: vec!["These come back as new shells.".to_owned()],
        decline_text_width: 62.0,
        restore_text_width: 47.0,
    };
    let layout = restore::layout(&content, 1200.0, 800.0, 1.0);
    // The top-left corner is the tab strip and the first pane, never the centred card.
    for (x, y) in [(10.0, 10.0), (40.0, 120.0), (1190.0, 790.0)] {
        assert_eq!(
            restore::hit(&layout, x, y),
            None,
            "({x}, {y}) is beside the card"
        );
        assert_eq!(
            restore::hit(&layout, x, y).and_then(restore::answer),
            None,
            "a press beside the card answers nothing"
        );
    }

    let press = squeezed_body("Runtime", "mouse_input");
    let arm = "iflet(Some(layout),Some(position))=(self.restore_layout(),self.window.pointer_position){ifstate==ElementState::Pressed&&button==MouseButton::Left&&letSome(answer)=restore::hit(&layout,position.x,position.y).and_then(restore::answer){self.answer_restore_prompt(answer)?;}returnOk(());}";
    let at = press
        .find(arm)
        .unwrap_or_else(|| panic!("the restore card's press arm does not swallow every press"));
    let chrome = press
        .find("self.chrome_mouse_input(")
        .expect("the chrome router is still reached from `mouse_input`");
    assert!(
        at < chrome,
        "a press reaches the tab strip, the panes and the programs before the card"
    );

    let covers = squeezed_body("Runtime", "a_modal_covers_the_window");
    assert!(
        covers.contains("||self.restore_card_is_up()"),
        "the card is not on the one reading of a modal:\n{covers}"
    );
    let wheel = squeezed_body("Runtime", "mouse_wheel");
    let nobody = wheel
        .find("ifself.a_modal_covers_the_window(){")
        .unwrap_or_else(|| panic!("a notch under a modal card is not swallowed"));
    let beneath = wheel
        .find("self.scroll_web_page(")
        .expect("the wheel still reaches a page");
    assert!(
        nobody < beneath,
        "a notch under the card scrolls what is beneath"
    );
    assert!(
        squeezed_body("Runtime", "pointer_moved")
            .contains("letfree=!self.a_modal_covers_the_window()&&"),
        "a hover under the card lights what is beneath"
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

// ── ticket 51: the find box reads a bounded slice of history per keystroke ──

/// A shell-less pane whose history holds `lines` generated lines, each narrow enough never to
/// wrap on the fixture's forty columns; `name(index)` writes line `index`. Generated, not
/// recorded (standing rules: fixtures).
fn pane_with_history(lines: usize, name: impl Fn(usize) -> String) -> LeafSession {
    let mut text = String::with_capacity(lines * 24);
    for index in 0..lines {
        text.push_str(&name(index));
        text.push_str("\r\n");
    }
    leaf_saying(&text)
}

/// The history hits of a whole-plane scan, as the set of `(line, start, end)` a hit is known by.
fn whole_answer(
    compiled: &bt_transcript::search::CompiledSearch,
    leaf: &LeafSession,
) -> std::collections::BTreeSet<(bt_viewport::SearchLine, u32, u32)> {
    search::scan_history(compiled, leaf.session.transcript())
        .iter()
        .map(|hit| (hit.line, hit.start, hit.end))
        .collect()
}

/// RED (51) — **A keystroke in the find box runs the pattern over at most one slice of history,
/// however long the history is.**
///
/// The pane holds a hundred thousand generated lines, the default scrollback. Before ticket 51
/// a changed question re-ran the pattern over every one of them on the keystroke's frame, once
/// per character typed (`ARCHITECTURE` §5.3 row 6). Now the keystroke's road
/// ([`SearchRefresh::Asked`]) reads one slice, each turn's ([`SearchRefresh::Walk`]) one more,
/// and a published frame ([`SearchRefresh::Output`]) none — and the walk those slices make ends
/// on the answer a scan from scratch gives. The real leaf, the real transcript, the real
/// [`rescan_leaf_for_search`] that `Runtime::refresh_search` calls.
///
/// MUTATION: make the keystroke road call the unbounded scan (`SearchRefresh::walk_budget`
/// answering `usize::MAX` for `Asked`) — `lines_scanned` reads the whole plane and the first
/// assertion goes red.
#[test]
fn a_keystroke_in_the_find_box_reads_at_most_one_slice_of_history() {
    let leaf = pane_with_history(100_000, |index| format!("worker {index} ok"));
    let frozen = leaf.session.transcript().frozen().len();
    assert!(
        frozen > 4 * search::SEARCH_HISTORY_SLICE,
        "the history is long enough to need a walk: {frozen} lines"
    );
    let compiled = search::engine(search::SearchFlags::default(), "worker 9").unwrap();
    let seat = SeatId(1);

    let asked =
        rescan_leaf_for_search(&compiled, &leaf, seat, 1, None, SearchRefresh::Asked, false);
    assert!(
        asked.lines_scanned <= search::SEARCH_HISTORY_SLICE,
        "the keystroke read {} of {frozen} lines",
        asked.lines_scanned
    );
    assert!(!asked.cache.history.is_complete(), "the rest is owed");
    assert!(!asked.unchanged);

    // A frame published after the keystroke carries the answer and reads no slice of it.
    let published = rescan_leaf_for_search(
        &compiled,
        &leaf,
        seat,
        1,
        Some(&asked.cache),
        SearchRefresh::Output,
        false,
    );
    assert_eq!(published.lines_scanned, 0);
    assert!(
        published.unchanged,
        "nothing moved, so nothing is re-installed"
    );

    // Each turn after it reads one slice, and the walk ends on the whole answer.
    let mut cache = asked.cache;
    let mut turns = 0;
    while cache.walking() {
        let walked = rescan_leaf_for_search(
            &compiled,
            &leaf,
            seat,
            1,
            Some(&cache),
            SearchRefresh::Walk,
            false,
        );
        assert!(walked.lines_scanned <= search::SEARCH_HISTORY_SLICE);
        assert!(!walked.unchanged, "a slice is never mistaken for nothing");
        assert!(!walked.carried_complete);
        cache = walked.cache;
        turns += 1;
    }
    assert_eq!(turns, (frozen - 1) / search::SEARCH_HISTORY_SLICE);
    let found: std::collections::BTreeSet<_> = cache
        .history
        .hits()
        .iter()
        .map(|hit| (hit.line, hit.start, hit.end))
        .collect();
    assert_eq!(found, whole_answer(&compiled, &leaf));
    assert_eq!(cache.history.hits().len(), found.len());
}

/// RED (A4) — **A search walk stops when the turn's allowance is spent and resumes on the next
/// turn, with its progress kept and nothing dropped.**
///
/// A slice is one unit of deferrable work (budget note §R-B): a turn that has run past its 16 ms
/// when the walk asks reads nothing, and the cache keeps the walk's cursor, so the next turn
/// reads the slice this one would have. Every other turn here is spent. The walk ends on the
/// answer a scan from scratch gives, in exactly as many slices as it would have taken without
/// the yields. The real leaf, the real `rescan_leaf_for_search`, the real heartbeat on the test
/// clock — and `advance_search_scan` asks through the heartbeat's verb.
///
/// MUTATION: run the unit in `Heartbeat::deferrable` whatever `unit_may_start` answers and no
/// turn yields.
#[test]
fn the_search_walk_stops_when_the_allowance_is_spent_and_resumes_next_turn() {
    let before_the_heart = Instant::now();
    let leaf = pane_with_history(4 * search::SEARCH_HISTORY_SLICE, |index| {
        format!("worker {index} ok")
    });
    let compiled = search::engine(search::SearchFlags::default(), "worker 1").unwrap();
    let seat = SeatId(1);
    let asked =
        rescan_leaf_for_search(&compiled, &leaf, seat, 1, None, SearchRefresh::Asked, false);
    let heart = hang_watch::Heartbeat::on_test_clock();
    let mut cache = asked.cache;
    let (mut walked, mut yielded) = (0, 0);
    let mut turn_ns = 0;
    while cache.walking() {
        hang_watch::set_test_clock_ns(turn_ns);
        heart.woke();
        heart.allow_turn(std::iter::empty());
        let spent = (walked + yielded) % 2 == 0;
        hang_watch::set_test_clock_ns(turn_ns + if spent { 17_000_000 } else { 1_000_000 });
        let kept = cache.clone();
        match heart.deferrable(|| {
            rescan_leaf_for_search(
                &compiled,
                &leaf,
                seat,
                1,
                Some(&cache),
                SearchRefresh::Walk,
                false,
            )
        }) {
            None => {
                assert!(spent, "a turn with time left reads its slice");
                assert_eq!(cache, kept, "the walk's progress is kept");
                assert_eq!(
                    heart.ms_at(heart.deferred_until(before_the_heart)),
                    (turn_ns + 16_000_000) / 1_000_000,
                    "and asked for again at the turn's boundary"
                );
                yielded += 1;
            }
            Some(walk) => {
                assert!(!spent, "a spent turn reads nothing");
                assert!(walk.lines_scanned <= search::SEARCH_HISTORY_SLICE);
                cache = walk.cache;
                walked += 1;
            }
        }
        heart.park(hang_watch::Park::Indefinite);
        turn_ns += 100_000_000;
    }
    let frozen = leaf.session.transcript().frozen().len();
    assert_eq!(walked, (frozen - 1) / search::SEARCH_HISTORY_SLICE);
    assert_eq!(yielded, walked, "every other turn yielded");
    let found: std::collections::BTreeSet<_> = cache
        .history
        .hits()
        .iter()
        .map(|hit| (hit.line, hit.start, hit.end))
        .collect();
    assert_eq!(found, whole_answer(&compiled, &leaf), "nothing dropped");
    assert!(
        method_body("Runtime", "advance_search_scan")
            .contains("hang_watch::deferrable(|| self.refresh_search(SearchRefresh::Walk))"),
        "the turn's slice is asked for through the allowance"
    );
}

/// RED (51) — **A new question abandons the walk of the old one.**
///
/// A walk for `ab` is under way when `c` is typed. The new question has its own revision, and
/// the old answer is no answer to it: no hit of `ab` that is not a hit of `abc` may appear in
/// anything installed after the keystroke, on the keystroke's own frame or on any turn of the new
/// walk — the cursor of the old walk is never read again. Half the lines hold `ab` alone, so a
/// carried hit would be caught on the first install.
///
/// MUTATION: continue the old cursor under the new revision (drop `cache.revision == revision`
/// from the filter in `rescan_leaf_for_search`) — the `ab` hits the old walk found are carried
/// into the first install, and so is its cursor.
#[test]
fn a_new_question_abandons_the_walk_of_the_old_one() {
    let lines = 3 * search::SEARCH_HISTORY_SLICE;
    let leaf = pane_with_history(lines, |index| {
        if index % 2 == 0 {
            format!("ab {index}")
        } else {
            format!("abc {index}")
        }
    });
    let seat = SeatId(1);
    let ab = search::engine(search::SearchFlags::default(), "ab").unwrap();
    let old = rescan_leaf_for_search(&ab, &leaf, seat, 1, None, SearchRefresh::Asked, false);
    assert!(old.cache.walking(), "the old question is mid-walk");

    let abc = search::engine(search::SearchFlags::default(), "abc").unwrap();
    let allowed = whole_answer(&abc, &leaf);
    let mut rescan = rescan_leaf_for_search(
        &abc,
        &leaf,
        seat,
        2,
        Some(&old.cache),
        SearchRefresh::Asked,
        false,
    );
    assert!(rescan.lines_scanned <= search::SEARCH_HISTORY_SLICE);
    loop {
        for hit in rescan.cache.history.hits() {
            assert!(
                allowed.contains(&(hit.line, hit.start, hit.end)),
                "a hit of the old question was installed under the new one: {hit:?}"
            );
        }
        if !rescan.cache.walking() {
            break;
        }
        rescan = rescan_leaf_for_search(
            &abc,
            &leaf,
            seat,
            2,
            Some(&rescan.cache),
            SearchRefresh::Walk,
            false,
        );
    }
    assert_eq!(rescan.cache.history.hits().len(), allowed.len());
}

/// RED (51) — **A turn continues a search walk in progress under the scan's own name, and books
/// its next turn at once while one is owed; a published frame reads no slice.**
///
/// The wiring the two tests above cannot reach, since `Runtime` is not built without a window:
/// the clock stands in `turn` after the drain (so the lines the shell froze meanwhile are carried,
/// not owed) under `Station::SearchScan`, the wake fold asks for the next turn through
/// `search_walk_deadline`, and `publish_frame_inner`'s refresh is the carrying road.
///
/// MUTATION: delete the `advance_search_scan` clock from `turn` — the walk never gets past the
/// keystroke's slice, and the first assertion goes red.
#[test]
fn a_turn_walks_a_search_in_progress_and_wakes_for_it() {
    let turning = method_body("Runtime", "turn");
    let clock = turning
        .find("self.advance_search_scan()")
        .expect("`turn` advances a search walk");
    let drain = turning.find("self.drain_pty()?;").expect("`turn` drains");
    assert!(
        clock > drain,
        "the walk reads after the drain has frozen what it will"
    );
    let station = turning[..clock]
        .rfind("hang_watch::Station::")
        .map(|at| &turning[at..clock]);
    assert!(
        station.is_some_and(|text| text.starts_with("hang_watch::Station::SearchScan")),
        "the walk's slice is charged to the scan's own name"
    );
    assert!(
        turning.contains("self.search_walk_deadline(now)"),
        "the wake fold books the next slice's turn"
    );
    assert!(
        method_body("Runtime", "publish_frame_inner")
            .contains("self.refresh_search(SearchRefresh::Output)"),
        "a published frame carries the answer and reads no slice"
    );
}

/// RED (46) — **under reduced motion no fade offers the renderer its group
/// path**: every surface a fade would have drawn apart stands at rest, and a
/// band that passes through the arrival register carries no span at all.
///
/// The fade audit's gate (c), the producers' half; the renderer's half
/// (`bt-render` `tests::overlay_groups::a_group_at_rest_never_takes_the_group_path`)
/// holds that a span at rest is drawn straight onto the frame. Asked of the
/// fades' own doors at the first instant of each fade: the arrival register
/// (menus, the palette, the settings dialog, the notice strip, the Cards
/// bubble), the hover fade the tip and the glance card both read, and a notice
/// card's opacity and slide. The tear-out ghost's 0.7 is a standing
/// translucency, not motion, so reduced motion (which is about motion) does not
/// forbid the group path there: it is a group in every motion mode, by the
/// coordinator's ruling of 2026-09-24, and not a hole in this pin.
///
/// MUTATION: sample the curve under `Reduced` in `hover_fade_opacity` (or
/// keep an entry in `Passages::stage` under `Reduced`) and a span below 1 is
/// handed over.
#[test]
fn under_reduced_motion_no_fade_offers_the_group_path() {
    let now = Instant::now();
    let layer = || marks::OverlayLayer {
        quads: vec![bt_render::OverlayQuad {
            rect: [10.0, 10.0, 110.0, 50.0],
            color: [40, 40, 40],
            alpha: 1.0,
        }],
        ..marks::OverlayLayer::default()
    };

    let mut passages = arrival::Passages::<u8>::default();
    let menu = passages.stage(
        1,
        vec![layer()].into(),
        Some(Travel::Down),
        now,
        Motion::Reduced,
        2.0,
    );
    assert!(
        menu.groups.is_empty(),
        "a band arriving under reduced motion is no surface in passage"
    );

    let tip = tooltip::hover_fade_opacity(Duration::ZERO, Motion::Reduced);
    let laid = tooltip::layout(
        "bash",
        [400.0, 10.0, 460.0, 40.0],
        &[40.0],
        (1000.0, 700.0),
        1.0,
        tooltip::TipFace::Chrome,
    )
    .expect("a tip is placed");
    let band = tooltip::build(
        &laid,
        &bt_render::chrome_palette(),
        1.0,
        tip,
        tooltip::TipFace::Chrome,
    );
    assert!(
        band.groups.iter().all(bt_render::OverlayGroup::at_rest),
        "the tip on its first frame under reduced motion: {:?}",
        band.groups
    );

    let mut host = toast::ToastHost::default();
    host.raise(
        toast::ToastKind::Ok,
        toast::ToastAnchor::Window,
        None,
        "done",
        None,
        Motion::Reduced,
        now,
    );
    let laid = toast::place(
        host.toasts(),
        |_| None,
        (1000.0, 700.0),
        1.0,
        &mut |run, _| run.chars().count() as f32 * 8.0,
    );
    let cards = toast::build(
        &laid,
        &host,
        toast::ToastPointer::default(),
        &bt_render::chrome_palette(),
        1.0,
        now,
        Motion::Reduced,
    );
    assert!(!cards.is_empty(), "the card is drawn on its first frame");
    assert!(
        cards.groups.iter().all(bt_render::OverlayGroup::at_rest),
        "a notice card on its first frame under reduced motion: {:?}",
        cards.groups
    );
}

// ── A1d: every owner-thread door takes a token (design note 2026-09-26, revision (e)2 as
//    corrected by (f)1) ──────────────────────────────────────────────────────────────────────

/// **One owner-thread door, asked from a worker and in every phase of the window thread.**
///
/// A thread the thread door started (A1b's real spawner) is refused with the door's name and
/// its work does not run; on a thread that entered as the window thread the door is admitted —
/// and its work runs — in exactly `phases`, and in every other phase it is refused, with that
/// phase named, and its work does not run.
fn a_door_answers_by_role_and_phase<D: bt_platform::admission::Door>(
    phases: &'static [bt_platform::admission::Phase],
) {
    use bt_platform::admission::{Phase, Refused, Role, admitted};
    const WORKER: &str = "bt-test-owner-door";
    let door = D::KEY.name();
    let on_a_worker =
        bt_platform::spawn_at_priority(WORKER, bt_platform::ThreadPriority::BelowNormal, |_ctx| {
            let mut ran = false;
            let answer = admitted::<D, _>(|_token| ran = true);
            (answer, ran)
        })
        .expect("a worker through the thread door")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert_eq!(
        on_a_worker,
        (
            Err(Refused {
                door,
                role: Role::Worker(WORKER),
                phase: None,
            }),
            false
        ),
        "`{door}` is refused on a worker, by its own name, and does nothing there"
    );
    std::thread::spawn(move || {
        assert!(bt_platform::admission::enter_window_thread());
        for phase in [Phase::Starting, Phase::Running, Phase::Exiting] {
            match phase {
                Phase::Starting => {}
                Phase::Running => assert!(bt_platform::admission::loop_running()),
                Phase::Exiting => assert!(bt_platform::admission::exiting()),
            }
            let mut ran = false;
            let answer = admitted::<D, _>(|_token| ran = true);
            if phases.contains(&phase) {
                assert_eq!(
                    (answer, ran),
                    (Ok(()), true),
                    "`{door}` is admitted, and runs, on the window thread in {phase:?}"
                );
            } else {
                assert_eq!(
                    (answer, ran),
                    (
                        Err(Refused {
                            door,
                            role: Role::Window,
                            phase: Some(phase),
                        }),
                        false
                    ),
                    "`{door}` is refused on the window thread in {phase:?}, and does nothing"
                );
            }
        }
    })
    .join()
    .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

/// RED (A1d) — **every owner-thread door is refused on a worker by its own name, and is admitted
/// on the window thread in exactly the phases the design note's door table gives it.**
///
/// The phases are written here as the table (revision (e)2, with (f)1's `Exiting` for
/// `WebController`) writes them, not read back from the types, so a door whose phase set drifts
/// from the table goes red by name. What this adds to A1a's and A1b's probes is the claim per
/// door, through the real thread door, for the doors the product now reaches only inside an
/// admission.
///
/// MUTATION: widen a door's phases in `bt_platform::admission::doors` (`CompositorBirth` to
/// `[Running, Exiting]`), or narrow one (`WebController` back to `[Running]`), and this names it.
#[test]
fn every_owner_door_is_refused_on_a_worker_and_admitted_only_in_its_phases() {
    use bt_platform::admission::Phase::{Exiting, Running, Starting};
    use bt_platform::admission::doors;
    a_door_answers_by_role_and_phase::<doors::FontFamilyLookup>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::PresentFrame>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::CompositorCommit>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::CompositorBirth>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::CompositorWindowSize>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::SurfaceBirth>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::PtyBirth>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::PtyResize>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::PlaceHidden>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::PlaceExposure>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::TitleFlush>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::PaneRetirementWait>(&[Exiting]);
    a_door_answers_by_role_and_phase::<doors::SessionWriteWait>(&[Exiting]);
    a_door_answers_by_role_and_phase::<doors::SessionWriterRetire>(&[Exiting]);
    a_door_answers_by_role_and_phase::<doors::TraceFlush>(&[Exiting]);
    a_door_answers_by_role_and_phase::<doors::UpdateLeave>(&[Exiting]);
    a_door_answers_by_role_and_phase::<doors::LaunchHandOver>(&[Starting]);
    a_door_answers_by_role_and_phase::<doors::WebController>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::WebEnvironment>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::WebRehost>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::ImeCaretArea>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::GpuOpen>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::FocusWindow>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::SetVisible>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::SetCursor>(&[Running]);
    assert_eq!(
        doors::ALL.len(),
        25,
        "a door added to the registry is a door this list has to name"
    );
}

/// RED (A1d, the owner side of M5) — **a boxed closure and a function pointer that reach an
/// owner-thread door from the window thread are admitted and run.**
///
/// The passing control for A1b's worker-refusal arm: the same two indirections, invoked on a
/// thread that entered as the window thread with its loop running, reach the door. A check that
/// read the caller's shape rather than the thread's role would refuse one of them.
///
/// MUTATION: make `admitted` refuse `Role::Window` too and both answers are `Err`.
#[test]
fn an_owner_door_reached_through_a_boxed_closure_or_a_function_pointer_runs_on_the_window_thread() {
    use bt_platform::admission::{Refused, admitted, doors};
    fn through_a_pointer() -> Result<&'static str, Refused> {
        admitted::<doors::SetCursor, _>(|_token| "the pointer's door ran")
    }
    std::thread::spawn(|| {
        assert!(bt_platform::admission::enter_window_thread());
        assert!(bt_platform::admission::loop_running());
        let boxed: Box<dyn Fn() -> Result<&'static str, Refused>> =
            Box::new(|| admitted::<doors::TitleFlush, _>(|_token| "the box's door ran"));
        let pointer: fn() -> Result<&'static str, Refused> = through_a_pointer;
        assert_eq!(boxed(), Ok("the box's door ran"));
        assert_eq!(pointer(), Ok("the pointer's door ran"));
    })
    .join()
    .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

/// RED (A1d, revision (c)8 item 9) — **every owner-thread door's function takes its own door's
/// token, by value, first.**
///
/// Checked by coercion, where the compiler reads the signature: each line below compiles only if
/// the function's first parameter (after `self`) is `WaitToken<'_, doors::<its door>>`. The two
/// generic doors (`present_frame_with_phases`, `launch_wire::hand_over`) are called from a
/// function whose own signature says the same. The session writer's two doors are private to
/// `persist` and are checked in its tests.
///
/// MUTATION: give any door another door's token type (`owner_door::set_title` taking
/// `WaitToken<'_, doors::SetCursor>`), or take the token off it, and this does not compile.
#[test]
#[expect(
    clippy::type_complexity,
    reason = "each coercion spells one door's whole signature, which is what it checks"
)]
fn every_owner_door_takes_its_own_token_by_value() {
    use bt_platform::admission::{WaitToken, doors};
    use bt_platform::{Compositor, NativeWindow, RehostOutcome, RehostSide, WebHost};
    use bt_render::{
        FrameTrigger, GpuContext, PresentOutcome, RenderError, SeatFrame, WindowRenderer,
        WindowTarget,
    };
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    let _: fn(
        WaitToken<'_, doors::PtyBirth>,
        OsString,
        &[OsString],
        bool,
        shell_integration::EnvironmentDerivation,
        &[(OsString, OsString)],
        &[(OsString, OsString)],
        PtySize,
        OutputWake,
        Option<PathBuf>,
    ) -> Result<PtySession, PtyError> = pty_door::spawn_shell;
    let _: fn(WaitToken<'_, doors::PtyResize>, &mut PtySession, PtySize) -> Result<(), PtyError> =
        pty_door::resize;
    let _: fn(WaitToken<'_, doors::PaneRetirementWait>, Duration) -> usize =
        pty_door::wait_for_retirements;
    let _: fn(
        WaitToken<'_, doors::GpuOpen>,
        WindowTarget,
        u32,
        u32,
        f64,
    ) -> Result<(GpuContext, WindowRenderer), RenderError> = gpu_door::open_first_window;
    let _: fn(WaitToken<'_, doors::TitleFlush>, &Window, &str) = owner_door::set_title;
    let _: fn(WaitToken<'_, doors::ImeCaretArea>, &Window, winit::dpi::Position, winit::dpi::Size) =
        owner_door::set_ime_cursor_area;
    let _: fn(WaitToken<'_, doors::FocusWindow>, &Window) = owner_door::focus_window;
    let _: fn(WaitToken<'_, doors::SetVisible>, &Window, bool) = owner_door::set_visible;
    let _: fn(WaitToken<'_, doors::SetCursor>, &Window, winit::window::Cursor) =
        owner_door::set_cursor;
    let _: fn(WaitToken<'_, doors::PlaceHidden>, &Window) -> bool = window_is_hidden;
    let _: fn(WaitToken<'_, doors::PlaceExposure>, &Window) -> bool = window_is_exposed;
    let _: fn(WaitToken<'_, doors::TraceFlush>) = trace_sink::flush;
    let _: fn(WaitToken<'_, doors::UpdateLeave>) -> Option<crate::update_apply::Left> =
        crate::update_handoff::leave_armed;
    let _: fn(&Compositor, WaitToken<'_, doors::CompositorCommit>) -> Result<(), String> =
        Compositor::commit;
    let _: fn(WaitToken<'_, doors::CompositorBirth>, NativeWindow) -> Result<Compositor, String> =
        Compositor::new;
    let _: fn(
        WaitToken<'_, doors::CompositorBirth>,
    ) -> Result<Option<bt_platform::SpareParent>, String> = bt_platform::spare_parent;
    let _: fn(
        &Compositor,
        WaitToken<'_, doors::CompositorWindowSize>,
        u32,
        u32,
    ) -> Result<(), String> = Compositor::set_window_size;
    let _: fn(
        &mut WebHost,
        WaitToken<'_, doors::WebController>,
        NativeWindow,
        u64,
    ) -> Result<(), String> = WebHost::request_controller;
    let _: fn(
        &mut WebHost,
        WaitToken<'_, doors::WebEnvironment>,
        &Path,
        u64,
    ) -> Result<(), String> = WebHost::request_environment;
    let _: fn(
        &mut WebHost,
        WaitToken<'_, doors::WebRehost>,
        &RehostSide<'_>,
        &RehostSide<'_>,
        (i32, i32, u32, u32),
        bool,
    ) -> RehostOutcome = WebHost::rehost;
    let _: fn(
        WaitToken<'_, doors::FontFamilyLookup>,
        &str,
    ) -> Option<bt_platform::MonospaceFamily> = bt_platform::monospace_family_named;
    let _: fn(
        WaitToken<'_, doors::SurfaceBirth>,
        &mut GpuContext,
        WindowTarget,
        u32,
        u32,
        f64,
    ) -> Result<WindowRenderer, RenderError> = WindowRenderer::new;
    fn presents(
        renderer: &mut WindowRenderer,
        token: WaitToken<'_, doors::PresentFrame>,
        gpu: &mut GpuContext,
        seats: &[SeatFrame<'_>],
        trigger: FrameTrigger,
    ) -> Result<PresentOutcome, RenderError> {
        renderer.present_frame_with_phases(token, gpu, seats, trigger, |_| {})
    }
    let _ = presents;
    fn hands_over(
        token: WaitToken<'_, doors::LaunchHandOver>,
        admitted: &update_startup::Admitted,
        directory: &Path,
        argv: &cli::CliRequest,
    ) -> Option<i32> {
        launch_wire::hand_over(token, admitted, directory, argv, |_| {})
    }
    let _ = hands_over;
}

/// RED (U-34, round 2; Codex's review, finding 9) — **an update door's panic
/// unwinds through its road's exit guard under the very hook `fn main`
/// installs for the doors, and the report still reaches the panic log.**
///
/// The product's own hook ends the process from inside the hook, before any
/// unwinding, so no `Drop` — no exit guard — would ever run; the doors install
/// `install_update_door_panic_hook_at` instead, before their line is parsed.
/// The hook is the process's, so the check runs in a copy of this test binary
/// of its own (`BT_UPDATE_DOOR_PANIC_TEST_CHILD` names the panic log there): a
/// guard whose start is recorded, a panic under it on a thread, the thread
/// joined, the start made, the report written, and the process still here to
/// say so.
///
/// MUTATION: in `install_update_door_panic_hook_at`, install the hook with the
/// product's end — `|_| std::process::exit(101)` — as its `fatal`.
#[test]
fn an_update_doors_panic_unwinds_through_its_exit_guard_under_mains_hook() {
    use std::sync::atomic::{AtomicBool, Ordering};
    const CHILD: &str = "BT_UPDATE_DOOR_PANIC_TEST_CHILD";
    const NAME: &str =
        "tests::an_update_doors_panic_unwinds_through_its_exit_guard_under_mains_hook";
    static STARTED: AtomicBool = AtomicBool::new(false);
    struct Recorder;
    impl update_apply::Leave for Recorder {
        fn say(&mut self, _line: &str) {}
        fn opening(&mut self) -> Option<(PathBuf, Vec<std::ffi::OsString>)> {
            Some((PathBuf::from("folio"), Vec::new()))
        }
        fn start(&mut self, _program: &Path, _words: &[std::ffi::OsString]) -> std::io::Result<()> {
            STARTED.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn acknowledged(&mut self) -> bool {
            true
        }
        fn show_here(&mut self, _why: &str) {}
    }
    if let Some(log) = std::env::var_os(CHILD) {
        install_update_door_panic_hook_at(PathBuf::from(&log));
        let joined = bt_platform::spawn_at_priority(
            "bt-u34-door-panic",
            bt_platform::ThreadPriority::BelowNormal,
            |_worker| {
                let _guard = update_apply::ExitGuard::new(Recorder);
                panic!("a fault inside the road (test)");
            },
        )
        .expect("a thread")
        .join();
        assert!(joined.is_err(), "the road panicked");
        assert!(STARTED.load(Ordering::SeqCst), "the guard's start was made");
        let report = std::fs::read_to_string(&log).expect("the panic log");
        assert!(report.contains("a fault inside the road"), "{report}");
        println!("u34: the door panic unwound");
        return;
    }
    let folder = bt_testpath::temp_path("bt-u34-door-panic");
    std::fs::create_dir_all(&folder).unwrap();
    let log = folder.join("folio-panic.log");
    let ran = bt_platform::quiet_command(std::env::current_exe().expect("this test binary"))
        .args(["--exact", NAME, "--test-threads=1", "--nocapture"])
        .env(CHILD, &log)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the child runs");
    let _ = std::fs::remove_dir_all(&folder);
    let said = String::from_utf8_lossy(&ran.stdout);
    assert!(ran.status.success(), "{said}");
    assert!(
        said.contains("u34: the door panic unwound"),
        "the child got past the panic: {said}"
    );
    assert!(said.contains("1 passed"), "{said}");
}
