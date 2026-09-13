//! **A real application delegate, on a real winit event loop, inside a real
//! `.app`** — the claims M3-1 makes that need LaunchServices and a window
//! server (ticket M3-1, `docs/DESIGN.md` §13.21).
//!
//! # Why this is a target of its own, and why it is not `macos_sheet`
//!
//! Two reasons, and the second is this ticket's own.
//!
//! **The main thread.** Everything here is AppKit and AppKit is the main
//! thread's; libtest does not give a case that thread — M2-3 measured it on this
//! workspace's toolchain, where a `#[test]` run with `--test-threads=1` still
//! executes on a thread libtest spawned and `MainThreadMarker::new()` is `None`.
//! `harness = false` hands this file the process's own `main`.
//!
//! **winit.** The door under test adds four selectors to
//! `WinitApplicationDelegate`, a private class **winit** registers when its
//! event loop is built. A proof that did not build a winit event loop would be a
//! proof about a class that is not in the process, and a proof that did not keep
//! turning that loop would not be able to say the thing X-4 found to be the only
//! FAIL-shaped hazard here: that the answer to a deferred termination has to come
//! from winit's own handler. So this target has winit as a dev-dependency, which
//! is the one this crate has.
//!
//! # What it costs a run that is not asking for it
//!
//! Nothing, and the gate is not an environment variable. `open` launches through
//! LaunchServices, which passes **none** of the shell's environment, so a
//! variable could not reach the process that matters. The gate is instead the
//! thing that is already true of the run that wants this: the binary is **inside
//! a `.app` bundle**. An ordinary `cargo test -p bt-platform` runs it out of
//! `target/debug/deps`, where there is no bundle, so it prints one line and
//! exits — on macOS and everywhere else.
//!
//! # How it is run
//!
//! An ssh session cannot reach the window server (X-1 measured that), so on the
//! Mac mini the binary is copied into a throwaway ad-hoc-signed `.app` and
//! started with `open`, which asks launchd to run it in the logged-in session.
//! The launcher creates the bundle and one fixture — `fixtures/cold launch.md`,
//! beside the bundle — and starts the app **with that file as its argument**, so
//! that the very first thing the delegate is asked is the cold
//! `application:openURLs:` the buffer exists for. Everything after that the test
//! drives itself, by spawning `open` and `osascript` at its own bundle and at
//! nothing else.
//!
//! The report is written beside the bundle, at `m3-1-report.log`, because a
//! process started by `open` inherits no working directory either.
//!
//! # What it proves, in order
//!
//! ① the delegate AppKit holds answers all four selectors, and it is still
//!    winit's own object — `respondsToSelector:` asked of `NSApp.delegate`;
//! ② a **cold** `application:openURLs:` is delivered, and **nothing at all was
//!    delivered before the application said it was ready**, which is the buffer;
//! ③ a second `open -a` of the running app is a **reopen** — `hasVisibleWindows`
//!    YES — and starts **no second executable**, which is the second-Finder-launch
//!    cell and therefore also "no second Dock icon";
//! ④ with every window closed, the application **stays**: AppKit is answered NO
//!    and the process is still there;
//! ⑤ a reopen with no window reports `hasVisibleWindows` NO and the application
//!    opens one;
//! ⑥ `open -a <app> <folder> <file>` delivers both paths, decoded — a space and
//!    CJK in the names — through `path_from_file_url`;
//! ⑦ a quit request answered `NSTerminateCancel` **from winit's handler** leaves
//!    the application alive and its loop turning;
//! ⑧ a quit request answered `NSTerminateNow` ends it, and the answer given from
//!    that same handler unwinds AppKit's deferred-termination loop rather than
//!    hanging in it.
//!
//! **The Dock click itself is NOT-CHECKABLE by an agent** and this file does not
//! try: X-4 measured that it needs `System Events`, and therefore Accessibility
//! *and* Automation, and that `osascript` from ssh answered `-1712` behind a TCC
//! prompt nobody could reach. A Dock click delivers exactly the reopen ③ and ⑤
//! are about, which is why it is covered rather than missing.

#[cfg(target_os = "macos")]
mod mac {
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use bt_platform::{
        AppDelegate, AppDelegateEvent, AppDelegateEventKind, TerminationAnswer,
        TerminationDecision, delegate_answers_the_four_selectors,
    };
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{Window, WindowId};

    /// How long one step may take before it is called failed.
    ///
    /// LaunchServices is not fast and is not a promise: a cold `open` of a
    /// freshly registered bundle has taken seconds on this machine. Twenty is
    /// long enough that a pass is a pass and short enough that the whole script
    /// is bounded.
    const STEP_DEADLINE: Duration = Duration::from_secs(20);

    /// How long the application is watched after a cancelled quit before that
    /// cancellation is believed.
    const STILL_ALIVE: Duration = Duration::from_millis(1500);

    /// What the delegate has said, parked by the sender on AppKit's stack.
    ///
    /// The same shape `bt-app` uses and for the same reason: the sender runs
    /// inside the delegate method, and the only two things it may do are push
    /// and wake.
    static INBOX: Mutex<Vec<AppDelegateEvent>> = Mutex::new(Vec::new());

    fn park(event: AppDelegateEvent) {
        INBOX
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }

    fn take() -> Vec<AppDelegateEvent> {
        std::mem::take(&mut *INBOX.lock().unwrap_or_else(PoisonError::into_inner))
    }

    // ── where this process is, and where it writes ─────────────────────────

    /// The `.app` this binary is inside, or `None` when it is not inside one.
    ///
    /// `…/Something.app/Contents/MacOS/<binary>` — three levels, and the
    /// extension is what makes it a bundle rather than a folder that happens to
    /// be three deep.
    fn enclosing_bundle() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let bundle = exe.parent()?.parent()?.parent()?;
        (bundle.extension()? == "app").then(|| bundle.to_path_buf())
    }

    /// The identifier in the bundle's own `Info.plist`.
    ///
    /// Read out of the plist rather than taken from a launcher, so that the
    /// `osascript` below can only ever name the bundle this process is actually
    /// running out of. A crude search and not a parser, for
    /// `macos_dialog_backend_tests`' reason: the file was written by the
    /// launcher two minutes ago and a parser here would be a second thing to be
    /// wrong.
    fn bundle_identifier(bundle: &Path) -> Option<String> {
        let plist = std::fs::read_to_string(bundle.join("Contents").join("Info.plist")).ok()?;
        let at = plist.find("<key>CFBundleIdentifier</key>")?;
        let rest = &plist[at..];
        let open = rest.find("<string>")? + "<string>".len();
        let close = rest[open..].find("</string>")?;
        Some(rest[open..open + close].to_owned())
    }

    /// The report, appended to and flushed on every line.
    ///
    /// Flushed because the last thing this process does is hand AppKit a
    /// `NSTerminateNow`, and what happens after that is `exit()` inside
    /// `-[NSApplication terminate:]` — a buffer is not something this program
    /// gets to flush afterwards.
    struct Report {
        file: std::fs::File,
        started: Instant,
        failures: usize,
    }

    impl Report {
        fn at(path: &Path) -> Self {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("the report is beside the bundle, which this process can write to");
            Self {
                file,
                started: Instant::now(),
                failures: 0,
            }
        }

        fn say(&mut self, line: &str) {
            let at = self.started.elapsed().as_millis();
            let _ = writeln!(self.file, "[{at:>7}ms] {line}");
            let _ = self.file.flush();
            println!("[{at:>7}ms] {line}");
        }

        fn pass(&mut self, claim: &str) {
            self.say(&format!("PASS {claim}"));
        }

        fn fail(&mut self, claim: &str, because: &str) {
            self.failures += 1;
            self.say(&format!("FAIL {claim}: {because}"));
        }
    }

    // ── the script ─────────────────────────────────────────────────────────

    /// One step of the exercise: what it does, and what has to have happened by
    /// the time its deadline runs out.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Step {
        /// ① and ②, both about the launch itself.
        TheLaunch,
        /// ③, first half.
        ReopenWithWindows,
        /// ③, second half.
        OneProcessOnly,
        /// ④.
        CloseEveryWindow,
        /// ⑤.
        ReopenWithNone,
        /// ⑥.
        OpenTwoPaths,
        /// ③ again, stated as the acceptance line does.
        ASecondFinderLaunch,
        /// ⑦.
        QuitCancelled,
        /// ⑦, the waiting half.
        StillAlive,
        /// ⑧.
        QuitAccepted,
        /// Nothing left; write the tally and go.
        Done,
    }

    impl Step {
        fn next(self) -> Self {
            match self {
                Self::TheLaunch => Self::ReopenWithWindows,
                Self::ReopenWithWindows => Self::OneProcessOnly,
                Self::OneProcessOnly => Self::CloseEveryWindow,
                Self::CloseEveryWindow => Self::ReopenWithNone,
                Self::ReopenWithNone => Self::OpenTwoPaths,
                Self::OpenTwoPaths => Self::ASecondFinderLaunch,
                Self::ASecondFinderLaunch => Self::QuitCancelled,
                Self::QuitCancelled => Self::StillAlive,
                Self::StillAlive => Self::QuitAccepted,
                Self::QuitAccepted | Self::Done => Self::Done,
            }
        }
    }

    /// Everything the delegate has said so far, in the shape the steps ask
    /// questions of.
    #[derive(Default)]
    struct Tally {
        reopens: Vec<bool>,
        paths: Vec<Vec<PathBuf>>,
        last_window_closed: usize,
        terminations: usize,
    }

    struct Probe {
        door: AppDelegate,
        report: Report,
        bundle: PathBuf,
        bundle_id: String,
        work: PathBuf,
        windows: Vec<Window>,
        tally: Tally,
        /// The one number the buffer is judged on: how many events crossed
        /// before `AppDelegate::ready` was called. It must be zero.
        delivered_before_ready: usize,
        ready: bool,
        termination: Option<TerminationAnswer>,
        step: Step,
        entered: bool,
        entered_at: Instant,
        /// Whatever `open` or `osascript` this step started, so that it can be
        /// read for a refusal — and so that the only processes this file ever
        /// looks at are ones it started itself.
        child: Option<Child>,
        turns: usize,
        turns_at_cancel: usize,
    }

    impl Probe {
        fn fixtures(&self) -> PathBuf {
            self.work.join("fixtures")
        }

        fn cold_file(&self) -> PathBuf {
            self.fixtures().join("cold launch.md")
        }

        /// A folder and a document, both with a space and CJK in the name.
        fn warm_paths(&self) -> [PathBuf; 2] {
            [
                self.fixtures().join("中文 folder"),
                self.fixtures().join("notes 中文.md"),
            ]
        }

        fn make_the_warm_fixtures(&mut self) {
            let [folder, file] = self.warm_paths();
            if let Err(error) = std::fs::create_dir_all(&folder) {
                self.report
                    .fail("the fixtures are on the disk", &format!("{error}"));
            }
            if let Err(error) = std::fs::write(&file, b"# notes\n") {
                self.report
                    .fail("the fixtures are on the disk", &format!("{error}"));
            }
        }

        fn open_a_window(&mut self, el: &ActiveEventLoop) {
            let attributes = Window::default_attributes()
                .with_title("Folio M3-1 delegate probe")
                .with_inner_size(winit::dpi::LogicalSize::new(520.0, 320.0));
            match el.create_window(attributes) {
                Ok(window) => self.windows.push(window),
                Err(error) => self.report.fail("a window opens", &format!("{error}")),
            }
        }

        /// Start something, and keep the handle. Only ever `open` or `osascript`
        /// aimed at this process's own bundle.
        fn start(&mut self, program: &str, arguments: &[&str]) {
            self.report
                .say(&format!("spawn {program} {}", arguments.join(" ")));
            match Command::new(program)
                .args(arguments)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => self.child = Some(child),
                Err(error) => self.report.fail(
                    "the exercise could be started",
                    &format!("{program}: {error}"),
                ),
            }
        }

        /// How many processes are running this binary.
        ///
        /// `ps -axo comm=` and a file-name comparison rather than `pgrep -x`:
        /// measured on this machine, `pgrep -x <name>` answers **nothing** for a
        /// process LaunchServices started out of a bundle, which would have made
        /// this claim pass by never finding anything at all — the worst shape a
        /// check can have. `ps` prints each process's executable path and the
        /// name inside the bundle is this process's own, so the comparison is
        /// exact and depends on nothing any tool calls a "process name".
        ///
        /// It reads a list and acts on nothing: no pid here is ever signalled.
        fn copies_running(&mut self) -> usize {
            let Some(name) = std::env::current_exe().ok().and_then(|exe| {
                exe.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            }) else {
                return 0;
            };
            let listed = match Command::new("/bin/ps").args(["-axo", "comm="]).output() {
                Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
                Err(error) => {
                    self.report.say(&format!("ps failed: {error}"));
                    return 0;
                }
            };
            let running = listed
                .lines()
                .filter(|line| {
                    Path::new(line.trim())
                        .file_name()
                        .is_some_and(|file| file == name.as_str())
                })
                .count();
            self.report
                .say(&format!("ps: {running} process(es) named {name}"));
            running
        }

        /// Route one delegate event to a named action, and tally it.
        fn record(&mut self, event: AppDelegateEvent, el: &ActiveEventLoop) {
            if !self.ready {
                self.delivered_before_ready += 1;
            }
            match event.kind {
                AppDelegateEventKind::Reopen {
                    had_visible_windows,
                } => {
                    self.report.say(&format!(
                        "EVENT {} hasVisibleWindows={had_visible_windows} windows={}",
                        event.origin.selector(),
                        self.windows.len()
                    ));
                    self.tally.reopens.push(had_visible_windows);
                    if self.windows.is_empty() {
                        self.open_a_window(el);
                    }
                }
                AppDelegateEventKind::OpenPaths(paths) => {
                    self.report.say(&format!(
                        "EVENT {} {:?}",
                        event.origin.selector(),
                        paths
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                    ));
                    self.tally.paths.push(paths);
                }
                AppDelegateEventKind::TerminationRequested(answer) => {
                    self.report
                        .say(&format!("EVENT {}", event.origin.selector()));
                    self.tally.terminations += 1;
                    self.termination = Some(answer);
                }
                AppDelegateEventKind::LastWindowClosed => {
                    self.report
                        .say(&format!("EVENT {}", event.origin.selector()));
                    self.tally.last_window_closed += 1;
                }
                // M3-2's, and this probe builds no menu bar: the arm is here so
                // that the kind's arrival is recorded rather than swallowed by a
                // wildcard, which is what would have hidden it from this script.
                AppDelegateEventKind::MenuChosen(choice) => {
                    self.report
                        .say(&format!("EVENT {} {choice:?}", event.origin.selector()));
                }
            }
        }

        /// What this step sets in motion, run once.
        fn enter(&mut self, el: &ActiveEventLoop) {
            match self.step {
                Step::TheLaunch => {
                    self.make_the_warm_fixtures();
                    self.open_a_window(el);
                    if delegate_answers_the_four_selectors() {
                        self.report
                            .pass("① NSApp.delegate answers all four selectors");
                    } else {
                        self.report.fail(
                            "① NSApp.delegate answers all four selectors",
                            "the delegate AppKit holds is missing at least one of them",
                        );
                    }
                }
                Step::ReopenWithWindows | Step::ReopenWithNone | Step::ASecondFinderLaunch => {
                    let bundle = self.bundle.display().to_string();
                    self.start("/usr/bin/open", &["-a", &bundle]);
                }
                Step::OneProcessOnly => {}
                Step::CloseEveryWindow => {
                    self.windows.clear();
                }
                Step::OpenTwoPaths => {
                    let bundle = self.bundle.display().to_string();
                    let [folder, file] = self.warm_paths();
                    let folder = folder.display().to_string();
                    let file = file.display().to_string();
                    self.start("/usr/bin/open", &["-a", &bundle, &folder, &file]);
                }
                Step::QuitCancelled | Step::QuitAccepted => {
                    let script = format!("tell application id \"{}\" to quit", self.bundle_id);
                    self.start("/usr/bin/osascript", &["-e", &script]);
                }
                Step::StillAlive => {
                    self.turns_at_cancel = self.turns;
                }
                Step::Done => {}
            }
        }

        /// Whether this step's claim has been met yet.
        fn satisfied(&mut self) -> bool {
            match self.step {
                Step::TheLaunch => {
                    let Some(first) = self.tally.paths.first() else {
                        return false;
                    };
                    let cold = self.cold_file();
                    if first.contains(&cold) {
                        self.report.pass("② the cold application:openURLs: arrived");
                    } else {
                        self.report.fail(
                            "② the cold application:openURLs: arrived",
                            &format!("{first:?} does not name {}", cold.display()),
                        );
                    }
                    if self.delivered_before_ready == 0 {
                        self.report.pass(
                            "② and nothing crossed the channel before the application was ready",
                        );
                    } else {
                        self.report.fail(
                            "② and nothing crossed the channel before the application was ready",
                            &format!("{} did", self.delivered_before_ready),
                        );
                    }
                    true
                }
                Step::ReopenWithWindows => {
                    if self.tally.reopens.is_empty() {
                        return false;
                    }
                    if self.tally.reopens[0] {
                        self.report
                            .pass("③ a second `open -a` is a reopen, with hasVisibleWindows YES");
                    } else {
                        self.report.fail(
                            "③ a second `open -a` is a reopen, with hasVisibleWindows YES",
                            "AppKit said NO while a window was on the screen",
                        );
                    }
                    true
                }
                Step::OneProcessOnly => {
                    let copies = self.copies_running();
                    if copies == 1 {
                        self.report.pass(
                            "③ and it started no second executable, so there is no second Dock icon",
                        );
                    } else {
                        self.report.fail(
                            "③ and it started no second executable",
                            &format!("{copies} copies of this binary are running"),
                        );
                    }
                    true
                }
                Step::CloseEveryWindow => {
                    if self.tally.last_window_closed == 0 {
                        return false;
                    }
                    self.report.pass(
                        "④ the last window closed, AppKit was answered NO and the application \
                         is still here",
                    );
                    true
                }
                Step::ReopenWithNone => {
                    if self.tally.reopens.len() < 2 {
                        return false;
                    }
                    if self.tally.reopens[1] {
                        self.report.fail(
                            "⑤ a reopen with no window reports hasVisibleWindows NO",
                            "AppKit said YES with no window open",
                        );
                    } else {
                        self.report
                            .pass("⑤ a reopen with no window reports hasVisibleWindows NO");
                    }
                    if self.windows.len() == 1 {
                        self.report.pass("⑤ and the application opened one");
                    } else {
                        self.report.fail(
                            "⑤ and the application opened one",
                            &format!("{} windows are open", self.windows.len()),
                        );
                    }
                    true
                }
                Step::OpenTwoPaths => {
                    if self.tally.paths.len() < 2 {
                        return false;
                    }
                    let wanted = self.warm_paths();
                    let delivered = &self.tally.paths[1];
                    if wanted.iter().all(|path| delivered.contains(path))
                        && delivered.len() == wanted.len()
                    {
                        self.report.pass(
                            "⑥ a folder and a document, both with a space and CJK in the name, \
                             arrive decoded",
                        );
                    } else {
                        self.report.fail(
                            "⑥ a folder and a document with a space and CJK arrive decoded",
                            &format!("{delivered:?} is not {wanted:?}"),
                        );
                    }
                    true
                }
                Step::ASecondFinderLaunch => {
                    if self.tally.reopens.len() < 3 {
                        return false;
                    }
                    let copies = self.copies_running();
                    if copies == 1 {
                        self.report.pass(
                            "③ with Folio running, launching it again opens in the running \
                             process and starts no second one",
                        );
                    } else {
                        self.report.fail(
                            "③ with Folio running, launching it again starts no second one",
                            &format!("{copies} copies are running"),
                        );
                    }
                    true
                }
                Step::QuitCancelled => {
                    let Some(answer) = self.termination.take() else {
                        return false;
                    };
                    match answer.answer(TerminationDecision::Cancel) {
                        Ok(()) => self.report.pass(
                            "⑦ a quit request was answered NSTerminateCancel from winit's handler",
                        ),
                        Err(reason) => self.report.fail(
                            "⑦ a quit request was answered NSTerminateCancel from winit's handler",
                            &reason,
                        ),
                    }
                    if answer.answer(TerminationDecision::Now).is_err() {
                        self.report
                            .pass("⑦ and a second answer to the same request is refused");
                    } else {
                        self.report.fail(
                            "⑦ and a second answer to the same request is refused",
                            "it was accepted",
                        );
                    }
                    true
                }
                Step::StillAlive => {
                    if self.entered_at.elapsed() < STILL_ALIVE {
                        return false;
                    }
                    let turned = self.turns - self.turns_at_cancel;
                    if turned > 1 {
                        self.report.pass(&format!(
                            "⑦ the application lived through the cancelled quit and its loop \
                             turned {turned} more times"
                        ));
                    } else {
                        self.report.fail(
                            "⑦ the application's loop kept turning after the cancelled quit",
                            "it turned at most once, which is a loop AppKit is still holding",
                        );
                    }
                    true
                }
                Step::QuitAccepted => {
                    let Some(answer) = self.termination.take() else {
                        return false;
                    };
                    self.report.say(&format!(
                        "TALLY reopens={:?} deliveries={} last-window-closed={} terminations={}",
                        self.tally.reopens,
                        self.tally.paths.len(),
                        self.tally.last_window_closed,
                        self.tally.terminations
                    ));
                    self.report
                        .say(&format!("RESULT {} failed", self.report.failures));
                    self.report.say("ALL_DONE");
                    // **Everything is written before this line**, because
                    // answering `NSTerminateNow` lets `-[NSApplication
                    // terminate:]` go on to `exit()` and nothing after it is
                    // this program's to run.
                    match answer.answer(TerminationDecision::Now) {
                        Ok(()) => self.report.pass(
                            "⑧ a quit request was answered NSTerminateNow from winit's handler",
                        ),
                        Err(reason) => self.report.fail(
                            "⑧ a quit request was answered NSTerminateNow from winit's handler",
                            &reason,
                        ),
                    }
                    true
                }
                Step::Done => true,
            }
        }

        /// What to say when a step ran out of time.
        fn timed_out(&mut self) {
            let step = self.step;
            let mut because = format!("{step:?} did not happen within {STEP_DEADLINE:?}");
            if let Some(child) = self.child.as_mut()
                && let Ok(Some(status)) = child.try_wait()
            {
                let mut said = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    use std::io::Read as _;
                    let _ = stderr.read_to_string(&mut said);
                }
                because.push_str(&format!(
                    "; the process it started exited {status} saying {said:?}"
                ));
            }
            self.report.fail(&format!("{step:?}"), &because);
        }
    }

    impl ApplicationHandler for Probe {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.ready {
                return;
            }
            self.report.say("WINIT resumed");
            // **The door opens for business here and not in `main`**, which is
            // claim ②: everything AppKit said before this line is still in the
            // buffer, and `delivered_before_ready` is the number that says so.
            self.door.ready();
            self.ready = true;
            let _ = el;
        }

        fn window_event(&mut self, _el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            if matches!(event, WindowEvent::CloseRequested) {
                self.windows.retain(|window| window.id() != id);
            }
        }

        fn user_event(&mut self, _el: &ActiveEventLoop, (): ()) {}

        fn about_to_wait(&mut self, el: &ActiveEventLoop) {
            self.turns += 1;
            for event in take() {
                self.record(event, el);
            }
            if self.step == Step::Done {
                // Only reached when the accepted quit did not end the process,
                // which is itself the failure and is already recorded.
                el.exit();
                return;
            }
            if !self.entered {
                self.report.say(&format!("STEP {:?}", self.step));
                self.entered_at = Instant::now();
                self.entered = true;
                self.enter(el);
            }
            if self.satisfied() {
                self.step = self.step.next();
                self.entered = false;
                self.child = None;
            } else if self.entered_at.elapsed() >= STEP_DEADLINE {
                self.timed_out();
                self.step = self.step.next();
                self.entered = false;
                self.child = None;
            }
            el.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(120),
            ));
        }

        fn exiting(&mut self, _el: &ActiveEventLoop) {
            self.report.say(&format!(
                "WINIT exiting after {} turns, {} failed",
                self.turns, self.report.failures
            ));
            self.report.say("ALL_DONE");
        }
    }

    pub fn run() {
        let Some(bundle) = enclosing_bundle() else {
            println!(
                "macos_app_delegate: skipped — this binary is not inside a .app, and \
                 LaunchServices is the whole exercise"
            );
            return;
        };
        let work = bundle
            .parent()
            .expect("a bundle is inside a directory")
            .to_path_buf();
        let mut report = Report::at(&work.join("m3-1-report.log"));
        report.say(&format!(
            "pid={} bundle={}",
            std::process::id(),
            bundle.display()
        ));
        let Some(bundle_id) = bundle_identifier(&bundle) else {
            report.fail(
                "the bundle names itself",
                "there is no CFBundleIdentifier in Info.plist, so nothing can be told to quit",
            );
            return;
        };
        report.say(&format!("bundle identifier = {bundle_id}"));

        let event_loop = EventLoop::<()>::with_user_event()
            .build()
            .expect("an event loop");
        // **After the loop and before `run_app`** — the order is the whole
        // finding: `EventLoop::new` is what registers the class the door adds
        // its selectors to.
        let proxy = event_loop.create_proxy();
        let door = match AppDelegate::install(move |event| {
            park(event);
            let _ = proxy.send_event(());
        }) {
            Ok(door) => door,
            Err(reason) => {
                report.fail("the application delegate installs", &reason);
                report.say("ALL_DONE");
                return;
            }
        };
        report.pass("the application delegate installed onto winit's own class");

        let mut probe = Probe {
            door,
            report,
            bundle,
            bundle_id,
            work,
            windows: Vec::new(),
            tally: Tally::default(),
            delivered_before_ready: 0,
            ready: false,
            termination: None,
            step: Step::TheLaunch,
            entered: false,
            entered_at: Instant::now(),
            child: None,
            turns: 0,
            turns_at_cancel: 0,
        };
        match event_loop.run_app(&mut probe) {
            Ok(()) => probe.report.say("run_app returned Ok"),
            Err(error) => probe
                .report
                .fail("the event loop ran to the end", &format!("{error}")),
        }
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macos_app_delegate: nothing to run on this platform");
}
