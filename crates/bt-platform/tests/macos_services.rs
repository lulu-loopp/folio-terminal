//! **Finder's *Services ▸ Open in Folio*, driven end to end** — the claims
//! M4-9 makes that need LaunchServices, a registered bundle and a second
//! process (ticket M4-9, `docs/DESIGN.md` §13.36).
//!
//! # Why a target of its own, and why it is not `macos_app_delegate`
//!
//! Everything that file's header says about the main thread and about winit
//! applies here word for word: the provider is registered on `NSApplication`,
//! which is the main thread's, and the channel it posts into is the one
//! `AppDelegate::install` opens onto a class **winit** registers, so a proof
//! without a winit event loop would be a proof about a class that is not in the
//! process. `harness = false` is what hands this file the process's own `main`.
//!
//! What makes it a second target is that it needs something M3-1's does not: a
//! **second bundle**. X-4 measured that a Service sent from the receiving
//! application's own executable is refused by LaunchServices — *"never opened
//! its Services port before the timeout"*, a self-collision rather than a defect
//! — so the sender has to be a different bundle with an identifier of its own.
//! This one binary is both: with `--send-service` it writes a pasteboard and
//! calls `NSPerformService`, and the launcher puts that same file inside a
//! second `.app` for it to be run out of.
//!
//! # The pasteboard is private, always
//!
//! `+[NSPasteboard pasteboardWithUniqueName]` and **never the general
//! pasteboard**. Finder uses the general one; this file must not, because the
//! machine it runs on synchronises the owner's clipboard between machines, and a
//! test that wrote there would put its fixtures into a person's paste buffer on
//! another computer. A private pasteboard is what `NSPerformService` documents
//! and it is the same delivery: AppKit copies the named board's contents to the
//! provider either way.
//!
//! # What it costs a run that is not asking for it
//!
//! Nothing, and the gate is not an environment variable, for
//! `macos_app_delegate`'s reason exactly: `open` passes none of the shell's
//! environment, so the thing that is already true of the run that wants this is
//! that the binary is **inside a `.app`**. An ordinary `cargo test -p
//! bt-platform` runs it out of `target/debug/deps`, prints one line and exits —
//! on macOS and everywhere else.
//!
//! # What it proves, in order
//!
//! ① the provider AppKit holds answers `openInFolio:userData:error:` — asked of
//!    `NSApp.servicesProvider` rather than of this crate's bookkeeping;
//! ② a **cold** delivery — the Service is the reason this process exists — is
//!    delivered, and **nothing at all crossed before the application said it was
//!    ready**, which is the buffer M3-1 built and M4-9 inherits;
//! ③ a plain folder, warm;
//! ④ a folder whose name carries **a space and a CJK character**, decoded to the
//!    bytes it spells — plan §M4 acceptance ⑤;
//! ⑤ a **multi-selection of three**, one event, in the reader's own order;
//! ⑥ a **file**: the door delivers the file's own path, because the rule that a
//!    file means the folder it is in is the product's and is spent at the
//!    landing (`bt_app`'s `open_one_place_for_a_service`, pinned on a machine
//!    with no AppKit by `a_service_opens_the_folder_and_a_document_opens_itself`).
//!
//! Every one of ③–⑥ is sent by a process this file started, from a bundle this
//! file's launcher built, and no pid it did not start is ever looked at.

#[cfg(target_os = "macos")]
mod mac {
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use bt_platform::{
        AppDelegate, AppDelegateEvent, AppDelegateEventKind, AppDelegateOrigin,
        services_provider_answers_open_in_folio,
    };
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSApplication, NSPasteboard, NSPasteboardWriting, NSPerformService};
    use objc2_foundation::{NSArray, NSString, NSURL};
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{Window, WindowId};

    /// **The name of the row, read out of the bundle this process is running
    /// out of** — `NSMenuItem/default`, which is the string `NSPerformService`
    /// asks LaunchServices for.
    ///
    /// Read from the plist and **not** from the environment, for this file's
    /// whole gating reason: LaunchServices passes none of the shell's
    /// environment to an application it starts, and the cold case is an
    /// application LaunchServices started. It is also why the launcher's
    /// throwaway bundle may name its row whatever it likes without this file
    /// knowing: a run against the shipped bundle asks for *Open in Folio*, and a
    /// run against a probe bundle asks for whatever that bundle declares, which
    /// is the only way the two cannot be confused for one another on a machine
    /// where both are registered.
    fn service_name(bundle: &Path) -> String {
        let fallback = || "Open in Folio".to_owned();
        let Ok(plist) = std::fs::read_to_string(bundle.join("Contents").join("Info.plist")) else {
            return fallback();
        };
        let Some(at) = plist.find("<key>NSMenuItem</key>") else {
            return fallback();
        };
        let rest = &plist[at..];
        let Some(open) = rest.find("<string>").map(|at| at + "<string>".len()) else {
            return fallback();
        };
        match rest[open..].find("</string>") {
            Some(close) => rest[open..open + close].to_owned(),
            None => fallback(),
        }
    }

    /// How long one step may take before it is called failed.
    ///
    /// LaunchServices is not fast and is not a promise — a cold delivery to a
    /// freshly registered bundle has taken seconds on this machine.
    const STEP_DEADLINE: Duration = Duration::from_secs(20);

    /// What the door has said, parked by the sender on AppKit's stack.
    static INBOX: Mutex<Vec<AppDelegateEvent>> = Mutex::new(Vec::new());

    /// **Whether the application has said it is up**, written immediately
    /// *before* `AppDelegate::ready` and read by the sender below.
    ///
    /// Before and not after, and that is the whole of the measurement: an event
    /// the buffer releases *because of* `ready` finds this already true, so a
    /// non-zero count below is a delivery that crossed while the application had
    /// no window and no restored session — the state the buffer exists to stop.
    static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// How many events crossed the sender before [`READY`]. It must be zero.
    static BEFORE_READY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn park(event: AppDelegateEvent) {
        if !READY.load(std::sync::atomic::Ordering::SeqCst) {
            BEFORE_READY.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
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
    fn enclosing_bundle() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let bundle = exe.parent()?.parent()?.parent()?;
        (bundle.extension()? == "app").then(|| bundle.to_path_buf())
    }

    /// The report, appended to and flushed on every line.
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

    // ── the sender half ────────────────────────────────────────────────────

    /// **`--send-service <row> <path>…`** — the pasteboard Finder builds,
    /// handed to `NSPerformService`.
    ///
    /// Run out of a **second** bundle, for the reason this file's header gives.
    /// It writes each path as a `public.file-url` item — which is what
    /// `+[NSURL fileURLWithPath:]` written through `NSPasteboardWriting`
    /// produces — onto a pasteboard with a unique name, and never onto the
    /// general one.
    ///
    /// The row's name is an argument rather than a constant because the sender
    /// is a bundle of its own and has no `NSServices` of its own to read one
    /// out of: the application under test knows which row it declared, and
    /// tells it.
    fn send_service(name: &str, paths: &[String]) {
        let mtm = MainThreadMarker::new().expect("the sender is its own main thread");
        let _app = NSApplication::sharedApplication(mtm);
        let pasteboard = NSPasteboard::pasteboardWithUniqueName();
        pasteboard.clearContents();

        let urls: Vec<Retained<NSURL>> = paths
            .iter()
            .map(|path| NSURL::fileURLWithPath(&NSString::from_str(path)))
            .collect();
        let writable: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = urls
            .iter()
            .map(|url| ProtocolObject::from_retained(url.clone()))
            .collect();
        let wrote = pasteboard.writeObjects(&NSArray::from_retained_slice(&writable));
        println!("SENDER wrote {} url(s): {wrote}", urls.len());
        for path in paths {
            println!("SENDER   {path}");
        }
        let performed = NSPerformService(&NSString::from_str(name), Some(&pasteboard));
        println!("SENDER NSPerformService({name:?}) = {performed}");
        // The board is this process's and dies with it; `releaseGlobally` is
        // not called, because the delivery AppKit makes is asynchronous and a
        // board released here would be one the receiver is still reading.
    }

    // ── the script ─────────────────────────────────────────────────────────

    /// One step of the exercise.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Step {
        /// ① and ②, both about the launch itself.
        TheColdDelivery,
        /// ③.
        APlainFolder,
        /// ④.
        ASpaceAndCjk,
        /// ⑤.
        ThreeAtOnce,
        /// ⑥.
        AFile,
        /// Nothing left; write the tally and go.
        Done,
    }

    impl Step {
        fn next(self) -> Self {
            match self {
                Self::TheColdDelivery => Self::APlainFolder,
                Self::APlainFolder => Self::ASpaceAndCjk,
                Self::ASpaceAndCjk => Self::ThreeAtOnce,
                Self::ThreeAtOnce => Self::AFile,
                Self::AFile | Self::Done => Self::Done,
            }
        }
    }

    struct Probe {
        door: AppDelegate,
        report: Report,
        /// The folder the launcher put the fixtures and the sender bundle in.
        work: PathBuf,
        /// The name of the row this bundle declares, which is what the sender
        /// asks LaunchServices for.
        row: String,
        window: Option<Window>,
        /// Every `OpenPaths` that arrived, with the origin it arrived from.
        deliveries: Vec<(AppDelegateOrigin, Vec<PathBuf>)>,
        /// The one number the buffer is judged on. It must be zero.
        delivered_before_ready: usize,
        ready: bool,
        step: Step,
        entered: bool,
        entered_at: Instant,
        /// How many `Services` deliveries had arrived when the step that is
        /// running began, so that "one more" is a claim about *this* step and
        /// not about the whole run.
        services_before_this_step: usize,
        /// Whether the closing tally has been written. See `about_to_wait`.
        tallied: bool,
        /// Whatever sender this step started, so that it can be read for a
        /// refusal — and so that the only process this file ever looks at is one
        /// it started itself.
        child: Option<Child>,
    }

    impl Probe {
        fn fixtures(&self) -> PathBuf {
            self.work.join("fixtures")
        }

        /// The folder the launcher cold-launched this application with.
        fn cold_folder(&self) -> PathBuf {
            self.fixtures().join("cold folder")
        }

        fn plain_folder(&self) -> PathBuf {
            self.fixtures().join("plain")
        }

        /// Acceptance ⑤'s own name: a space **and** a CJK character.
        fn space_and_cjk(&self) -> PathBuf {
            self.fixtures().join("中文 folder")
        }

        fn three(&self) -> [PathBuf; 3] {
            [
                self.fixtures().join("one"),
                self.space_and_cjk(),
                self.fixtures().join("three three"),
            ]
        }

        fn a_file(&self) -> PathBuf {
            self.space_and_cjk().join("notes 中文.md")
        }

        /// The sender bundle's executable, which the launcher built beside this
        /// bundle.
        fn sender(&self) -> Option<PathBuf> {
            let bundle = self
                .work
                .join("ServiceSender.app")
                .join("Contents")
                .join("MacOS");
            std::fs::read_dir(bundle)
                .ok()?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| path.is_file())
        }

        /// Start the sender on these paths, and keep the handle.
        fn send(&mut self, paths: &[PathBuf]) {
            let Some(sender) = self.sender() else {
                self.report.fail(
                    "the sender bundle is beside this one",
                    "no executable under ServiceSender.app/Contents/MacOS",
                );
                return;
            };
            let mut arguments = vec!["--send-service".to_owned(), self.row.clone()];
            arguments.extend(paths.iter().map(|path| path.display().to_string()));
            self.report.say(&format!(
                "spawn {} {}",
                sender.display(),
                arguments.join(" ")
            ));
            match Command::new(&sender)
                .args(&arguments)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => self.child = Some(child),
                Err(error) => self
                    .report
                    .fail("the sender could be started", &format!("{error}")),
            }
        }

        /// Whatever the sender said, once it has said it.
        ///
        /// **Only when it has already left.** `wait_with_output` blocks, and the
        /// one state where that would matter is a sender that is stuck — which
        /// is exactly the state the deadline branch is reporting. So the handle
        /// is reaped with `try_wait` first and let go of otherwise: the report
        /// says the sender is still running and names nothing to end, which is
        /// this venue's rule about processes written into the one place a test
        /// could break it.
        fn read_the_sender(&mut self) {
            let Some(mut child) = self.child.take() else {
                return;
            };
            match child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    self.report
                        .say(&format!("the sender (pid {}) has not left yet", child.id()));
                    // Kept rather than dropped, so that the next step reaps it
                    // rather than leaving a child nobody ever waits on.
                    self.child = Some(child);
                    return;
                }
                Err(error) => {
                    self.report
                        .say(&format!("the sender was not read: {error}"));
                    return;
                }
            }
            match child.wait_with_output() {
                Ok(output) => {
                    for line in String::from_utf8_lossy(&output.stdout).lines() {
                        self.report.say(&format!("  | {line}"));
                    }
                    for line in String::from_utf8_lossy(&output.stderr).lines() {
                        self.report.say(&format!("  ! {line}"));
                    }
                }
                Err(error) => self
                    .report
                    .say(&format!("the sender was not read: {error}")),
            }
        }

        fn make_the_fixtures(&mut self) {
            let mut folders = vec![self.cold_folder(), self.plain_folder()];
            folders.extend(self.three());
            for folder in folders {
                if let Err(error) = std::fs::create_dir_all(&folder) {
                    self.report.fail(
                        "the fixtures are on the disk",
                        &format!("{}: {error}", folder.display()),
                    );
                }
            }
            let file = self.a_file();
            if let Err(error) = std::fs::write(&file, b"# notes\n") {
                self.report.fail(
                    "the fixtures are on the disk",
                    &format!("{}: {error}", file.display()),
                );
            }
        }

        /// The paths of the last `Services` delivery, and nothing else's.
        fn last_service(&self) -> Option<&Vec<PathBuf>> {
            self.deliveries
                .iter()
                .rev()
                .find(|(origin, _)| *origin == AppDelegateOrigin::Services)
                .map(|(_, paths)| paths)
        }

        fn services_so_far(&self) -> usize {
            self.deliveries
                .iter()
                .filter(|(origin, _)| *origin == AppDelegateOrigin::Services)
                .count()
        }

        /// One delivery arrived: what it was, and whether the buffer held.
        fn record(&mut self, event: AppDelegateEvent) {
            if !self.ready {
                self.delivered_before_ready += 1;
            }
            match event.kind {
                AppDelegateEventKind::OpenPaths(paths) => {
                    self.report.say(&format!(
                        "EVENT {} {:?}",
                        event.origin.selector(),
                        paths
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                    ));
                    self.deliveries.push((event.origin, paths));
                }
                AppDelegateEventKind::Reopen { .. }
                | AppDelegateEventKind::TerminationRequested(_)
                | AppDelegateEventKind::LastWindowClosed
                | AppDelegateEventKind::MenuChosen(_) => {
                    // M3-1's and M3-2's, and this script asks nothing of them.
                    // Named rather than matched by a wildcard so that a new kind
                    // is a compile error here rather than an event this file
                    // silently swallows.
                    self.report.say(&format!(
                        "EVENT {} (not this ticket's)",
                        event.origin.selector()
                    ));
                }
            }
        }

        /// **Whether one more `Services` delivery has arrived since this step
        /// began** — the one question every step is waiting on.
        fn arrived(&self) -> bool {
            self.services_so_far() > self.services_before_this_step
        }

        /// Settle the step that is running, if anything has happened yet.
        ///
        /// It advances the script **only when the step it is on has finished**,
        /// which is the whole of the loop: a step that is still waiting for its
        /// delivery leaves `self.step` alone and is asked again on the next turn.
        fn settle(&mut self) {
            let waited = self.entered_at.elapsed();
            let finished = match self.step {
                Step::TheColdDelivery => {
                    if !self.arrived() {
                        if waited > STEP_DEADLINE {
                            self.report.fail(
                                "a cold Service reaches the application it started",
                                "no delivery before the deadline",
                            );
                            self.step = Step::Done;
                        }
                        return;
                    }
                    let cold = self.cold_folder();
                    match self.last_service() {
                        Some(paths) if paths.as_slice() == [cold.clone()] => self
                            .report
                            .pass("a cold Service reaches the application it started"),
                        other => self.report.fail(
                            "a cold Service reaches the application it started",
                            &format!("{other:?} is not [{}]", cold.display()),
                        ),
                    }
                    let crossed_early = BEFORE_READY.load(std::sync::atomic::Ordering::SeqCst);
                    if crossed_early == 0 {
                        self.report
                            .pass("nothing crossed the door before the application said it was up");
                    } else {
                        self.report.fail(
                            "nothing crossed the door before the application said it was up",
                            &format!("{crossed_early} event(s) did"),
                        );
                    }
                    true
                }
                Step::APlainFolder => {
                    let expected = vec![self.plain_folder()];
                    self.check("a folder, warm", &expected, waited)
                }
                Step::ASpaceAndCjk => {
                    let expected = vec![self.space_and_cjk()];
                    self.check(
                        "a folder with a space and a CJK character",
                        &expected,
                        waited,
                    )
                }
                Step::ThreeAtOnce => {
                    let expected = self.three().to_vec();
                    self.check(
                        "a multi-selection of three, one event, in order",
                        &expected,
                        waited,
                    )
                }
                Step::AFile => {
                    let expected = vec![self.a_file()];
                    self.check(
                        "a file is delivered as itself, for the landing to read as its folder",
                        &expected,
                        waited,
                    )
                }
                Step::Done => false,
            };
            if finished && self.step != Step::Done {
                self.step = self.step.next();
                self.entered = false;
            }
        }

        /// The body of every warm step: wait for one more `Services` delivery,
        /// then say whether it is the one that was sent.
        ///
        /// Answers **whether this step is over** — false while it is still
        /// waiting, which is what keeps the script on it.
        fn check(&mut self, claim: &str, expected: &[PathBuf], waited: Duration) -> bool {
            if !self.arrived() {
                if waited > STEP_DEADLINE {
                    self.read_the_sender();
                    self.report.fail(claim, "no delivery before the deadline");
                    self.step = Step::Done;
                }
                return false;
            }
            self.read_the_sender();
            match self.last_service() {
                Some(paths) if paths.as_slice() == expected => self.report.pass(claim),
                other => self.report.fail(
                    claim,
                    &format!(
                        "{:?} is not {:?}",
                        other.map(|paths| paths
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()),
                        expected
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                    ),
                ),
            }
            true
        }

        /// Start whatever the step that has just begun has to start.
        fn enter(&mut self) {
            self.entered = true;
            self.entered_at = Instant::now();
            self.services_before_this_step = self.services_so_far();
            self.report.say(&format!("STEP {:?}", self.step));
            match self.step {
                Step::TheColdDelivery | Step::Done => {}
                Step::APlainFolder => {
                    let paths = [self.plain_folder()];
                    self.send(&paths);
                }
                Step::ASpaceAndCjk => {
                    let paths = [self.space_and_cjk()];
                    self.send(&paths);
                }
                Step::ThreeAtOnce => {
                    let paths = self.three();
                    self.send(&paths);
                }
                Step::AFile => {
                    let paths = [self.a_file()];
                    self.send(&paths);
                }
            }
        }
    }

    impl ApplicationHandler for Probe {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let attributes = Window::default_attributes()
                .with_title("Folio M4-9 services probe")
                .with_inner_size(winit::dpi::LogicalSize::new(520.0, 320.0));
            match el.create_window(attributes) {
                Ok(window) => self.window = Some(window),
                Err(error) => self.report.fail("a window opens", &format!("{error}")),
            }
            // **And now the door may speak** — the same line `bt-app` runs, in
            // the same place, which is what makes the count a measurement of the
            // product's own buffer rather than of this file's.
            READY.store(true, std::sync::atomic::Ordering::SeqCst);
            self.door.ready();
            self.ready = true;
            self.report.say("ready");
        }

        fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
            if matches!(event, WindowEvent::CloseRequested) {
                self.window = None;
            }
        }

        fn about_to_wait(&mut self, el: &ActiveEventLoop) {
            // **The step is entered before the inbox is drained**, and the
            // order is the whole of `arrived`: what a step waits for is *one
            // more* delivery than there were when it began, so the count it
            // begins with has to be taken before this turn's arrivals are
            // recorded. The first run of this file had it the other way round
            // and the cold step could never finish — the delivery it was
            // waiting for was already in the number it was counting from.
            if !self.entered {
                self.enter();
            }
            for event in take() {
                self.record(event);
            }
            self.settle();
            if self.step == Step::Done && !self.tallied {
                // Once. `exit` asks the loop to end and the loop still turns a
                // few more times before it does, so a tally written here without
                // the flag is a report with the same last line fifty times in it
                // — which is what the first run of this file produced.
                self.tallied = true;
                self.report.say(&format!(
                    "TALLY services={} crossed_before_ready={} drained_before_resumed={} \
                     failures={}",
                    self.services_so_far(),
                    BEFORE_READY.load(std::sync::atomic::Ordering::SeqCst),
                    self.delivered_before_ready,
                    self.report.failures
                ));
                self.report.say("ALL_DONE");
                el.exit();
                return;
            }
            if self.step == Step::Done {
                return;
            }
            el.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(50),
            ));
        }
    }

    pub(super) fn run() {
        let arguments: Vec<String> = std::env::args().skip(1).collect();
        if arguments.first().map(String::as_str) == Some("--send-service") {
            match arguments.get(1) {
                Some(row) => send_service(row, &arguments[2..]),
                None => println!("SENDER --send-service takes the row's name and then the paths"),
            }
            return;
        }

        let Some(bundle) = enclosing_bundle() else {
            println!(
                "macos_services: this binary is not inside a `.app`, so there is no bundle to \
                 declare `NSServices` and nothing to prove; see this file's header"
            );
            return;
        };
        let work = bundle
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let row = service_name(&bundle);
        let mut report = Report::at(&work.join("m4-9-report.log"));
        report.say(&format!(
            "pid={} bundle={} row={row:?}",
            std::process::id(),
            bundle.display()
        ));

        let event_loop = match EventLoop::new() {
            Ok(loop_) => loop_,
            Err(error) => {
                report.fail("the event loop is built", &format!("{error}"));
                report.say("ALL_DONE");
                return;
            }
        };
        let door = match AppDelegate::install(park) {
            Ok(door) => door,
            Err(reason) => {
                report.fail("the application delegate installs", &reason);
                report.say("ALL_DONE");
                return;
            }
        };
        match door.offer_open_in_folio() {
            Ok(()) => report.say("the Services provider was registered"),
            Err(reason) => report.fail("the Services provider is registered", &reason),
        }
        if services_provider_answers_open_in_folio() {
            report.pass("NSApp.servicesProvider answers openInFolio:userData:error:");
        } else {
            report.fail(
                "NSApp.servicesProvider answers openInFolio:userData:error:",
                "AppKit is holding no provider that answers it",
            );
        }

        let mut probe = Probe {
            door,
            report,
            work,
            row,
            window: None,
            deliveries: Vec::new(),
            delivered_before_ready: 0,
            ready: false,
            step: Step::TheColdDelivery,
            entered: false,
            entered_at: Instant::now(),
            child: None,
            services_before_this_step: 0,
            tallied: false,
        };
        probe.make_the_fixtures();
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
    println!("macos_services: nothing to run on this platform");
}
