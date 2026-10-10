//! **The tests that cannot leave this module's path** — each is named as
//! `tests::<name>` by something outside its own body: a test that runs itself again with
//! `--exact "tests::<name>"`, or a row of `docs/plans/TIMING-BOUND-TESTS.tsv`. Every other
//! test of the crate root is in a file named for what it tests, and their shared fixtures
//! are in [`crate::test_support`].

use super::*;
use crate::test_support::{
    free_fn_body, item_body, ledger_gate, method_body, probe_leaf, reader_names, source,
    text_buffer,
};
use bt_source::{ItemQuery, Pattern, Search, View, needle};
use std::time::Duration;

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
///
/// Windows only: a share on another machine is a spelling only Windows' grammar has
/// (`bt_transcript::paths::is_a_share_on_another_machine`). On macOS a share is a mount point with
/// a local name, so it leaves through the local file's arm and meets the program list there.
#[cfg(windows)]
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

    let (wake, wakes) = crate::lane::wake_channel();
    let mut lane = handoff_lane::HandoffLane::spawn(wake).expect("the lane starts");
    let id = lane.submit(bt_platform::NativeWindow::stand_in(0), request);
    let answer = loop {
        if let Some(answer) = lane.answers().into_iter().find(|answer| answer.id == id) {
            break answer;
        }
        crate::lane::wait_for_a_wake(&wakes, "the share's answer");
    };
    let reason = answer.outcome.expect_err("a program on a share is refused");
    assert_eq!(reason, bt_platform::PROGRAM_REFUSED, "the door's own words");
    assert_eq!(
        UNVERIFIED_REFERENCE_REFUSAL.words(&reason).notice,
        Some(files_program_refused_notice()),
        "and the reader is told, as for a local program"
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

#[test]
fn real_powershell_input_reaches_a_viewport_owned_frame() {
    let columns = std::num::NonZeroU16::new(48).unwrap();
    let rows = std::num::NonZeroU16::new(10).unwrap();
    // PowerShell's readiness handshake is Windows-specific. Unix `/bin/sh` uses a controlled
    // `PS1` prompt as its readiness marker, then receives a POSIX `printf` line. Both shells go
    // through `TestShell`, which keeps profiles, history and home in a per-test scratch directory.
    #[cfg(windows)]
    let mut pty =
        bt_pty::test_shell::TestShell::spawn_default(PtySize::cells(columns, rows)).unwrap();
    #[cfg(unix)]
    let mut pty = bt_pty::test_shell::TestShell::spawn(
        bt_pty::PtyCommand::new("/bin/sh")
            .arg("-i")
            .env("PS1", "BT_APP_READY> "),
        PtySize::cells(columns, rows),
    )
    .unwrap();
    let mut session = DualPlaneSession::with_quotas_and_cell_height(
        nonzero_u32(columns.get()),
        nonzero_u32(rows.get()),
        DEFAULT_STAGING_QUOTA,
        DEFAULT_FROZEN_LINE_QUOTA,
        std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
    );
    // The child is a real shell, so startup and input echo take machine time, not terminal
    // time. The old Windows PowerShell version made a total wall-clock budget a load meter: at
    // rest it finished in six seconds, but with twenty-four spinners on this twenty-four-thread
    // host the 2026-08-20 experiment failed it **16 times out of 16**, always on the same
    // ten-second ceiling, always with the child working normally on the other side of it.
    //
    // What the test wants to know is whether the child has *stopped*, and that question
    // survives a busy host: a starved machine delivers the same bytes, only further apart. So
    // the budget restarts on every byte read, and a separate ceiling catches the one shape
    // silence cannot — a child that talks forever without ever saying this.
    //
    // Enlarging the old ten-second *total* was the option not taken: a total grows with the
    // work the child has left, while a silence budget asks how long a live process may be denied
    // the CPU before we call it dead. Thirty seconds is that judgement, and it is `bt-pty`'s
    // `PROBE_SILENCE_BUDGET` to the second — same question, same host, and the two probes should
    // not answer it differently. See there for the measurements it was chosen from.
    const SILENCE_BUDGET: Duration = Duration::from_secs(30);
    const CEILING: Duration = Duration::from_secs(180);
    const MARKER: &str = "BT_APP_INPUT_OK";
    #[cfg(windows)]
    const COMMAND: &str = "Write-Output ('BT_APP_' + 'INPUT_OK')\r";
    #[cfg(unix)]
    const COMMAND: &str = "printf '%s%s\\n' 'BT_APP_' 'INPUT_OK'\r";
    #[cfg(unix)]
    const READY_MARKER: &str = "BT_APP_READY>";

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
            #[cfg(windows)]
            let ready = !replies.is_empty();
            #[cfg(unix)]
            let ready = session
                .terminal()
                .visible_text()
                .iter()
                .any(|line| line.contains(READY_MARKER));
            if !command_sent && ready {
                pty.write(&ime_commit_bytes(COMMAND)).unwrap();
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
                     child silent, having read {bytes_read} bytes; input {}; {}. Screen \
                     {:?}",
                started.elapsed(),
                silent_for,
                if command_sent {
                    "the command was sent"
                } else {
                    "the shell readiness signal never arrived, so the command was not sent"
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
    // Put the playhead off zero, so that "the playhead did not go back" is a
    // claim with something in it — by the engine's own seek and not by waiting
    // for playback to move it (M-SWEEP-048). A seek lands on every machine; a
    // Mac's `AVPlayer` only advances while the main run loop drains the main
    // queue, which no libtest thread can do, its own run loop pumped or not.
    // Both waits are on the engine thread's publications, within the lane
    // suite's patience.
    let card = seats
        .get_mut(PreviewSurface::Peek)
        .expect("a seat on the card");
    let loaded =
        card.state_reaching(|state| state.duration_secs.is_some() || state.error.is_some());
    assert_eq!(loaded.error, None, "the fixture loads");
    card.seek_to(0.2, now);
    let target = 0.2 * loaded.duration_secs.expect("a length");
    let was = card
        .state_reaching(|state| state.position_secs >= target - 0.05)
        .position_secs;
    assert!(
        was >= target - 0.05,
        "the playhead reached the seek before the tear-off: {was} for {target}"
    );
    let key = card.key().to_owned();
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

#[test]
fn maximize_intent_keeps_desired_state_ahead_of_stale_answers() {
    let mut intent = WindowMaximizeIntent::default();
    assert_eq!(intent.observe(Some(false)), None);
    assert_eq!(intent.toggle(), WindowMaximizeAction::Request(true));
    assert_eq!(intent.posture_state(), None);
    assert_eq!(intent.observe(Some(false)), None);
    assert_eq!(intent.posture_state(), None);
    assert_eq!(intent.toggle(), WindowMaximizeAction::Request(false));
}

#[test]
fn maximize_intent_preserves_rapid_absolute_toggle_order() {
    let mut intent = WindowMaximizeIntent::default();
    assert_eq!(intent.observe(Some(false)), None);
    assert_eq!(intent.toggle(), WindowMaximizeAction::Request(true));
    assert_eq!(intent.toggle(), WindowMaximizeAction::Request(false));
    assert_eq!(intent.posture_state(), Some(false));
}

#[test]
fn maximize_intent_waits_for_unknown_state_and_cancels_even_toggles() {
    let mut intent = WindowMaximizeIntent::default();
    assert_eq!(intent.toggle(), WindowMaximizeAction::WaitForObservation);
    assert_eq!(intent.toggle(), WindowMaximizeAction::WaitForObservation);
    assert_eq!(intent.observe(Some(false)), None);
    assert_eq!(intent.posture_state(), Some(false));

    assert_eq!(intent.toggle(), WindowMaximizeAction::Request(true));
    let mut unknown = WindowMaximizeIntent::default();
    assert_eq!(unknown.toggle(), WindowMaximizeAction::WaitForObservation);
    assert_eq!(
        unknown.observe(Some(true)),
        Some(WindowMaximizeAction::Request(false))
    );
    assert_eq!(unknown.posture_state(), None);
    assert_eq!(unknown.observe(Some(false)), None);
    assert_eq!(unknown.posture_state(), Some(false));
}

#[test]
fn an_initial_maximize_request_stays_desired_until_observed() {
    let mut intent = WindowMaximizeIntent::default();
    intent.request_initial(true);
    assert_eq!(intent.toggle(), WindowMaximizeAction::Request(false));
    assert_eq!(intent.posture_state(), None);
    assert_eq!(intent.observe(Some(true)), None);
    assert_eq!(intent.posture_state(), None);
}
