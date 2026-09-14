//! **A real Carbon hot key, claimed and then pressed, inside a real `.app`** —
//! the claims M4-8 makes that no Windows runner can check (ticket M4-8,
//! `docs/DESIGN.md` §13.51).
//!
//! # Why this is a target of its own
//!
//! **The main thread.** `RegisterEventHotKey` against the *application* event
//! target is a statement about the main run loop, and the handler that answers
//! it is dispatched on the main thread. libtest does not give a case that
//! thread — a `#[test]` run with `--test-threads=1` still executes on a thread
//! libtest spawned — so `harness = false` hands this file the process's own
//! `main`.
//!
//! **A running application.** A hot key is delivered by the system's own hot key
//! manager into this application's event dispatch, so a proof with no run loop
//! would be a proof that nothing was listening. `EventLoop::run_app` is that
//! loop, and it is winit's because it is the product's.
//!
//! And one of its own: an ssh session cannot reach the window server, so the
//! binary is copied into a throwaway ad-hoc-signed `.app` with an identifier of
//! its own and an isolated `HOME`, and started with `open`. Outside a bundle
//! this file prints one line and exits, so an ordinary `cargo test -p
//! bt-platform` costs nothing — the gate is the bundle rather than an
//! environment variable, because `open` passes none of the shell's environment
//! through.
//!
//! # What it proves, in order
//!
//! ① **the translation**, held against the numbers the registration is then
//!    actually made with — the pure half runs on every host too, and is asserted
//!    here so that the two halves cannot drift apart on the one machine that has
//!    both;
//! ② **the chord is really claimed**: `bt_platform::hotkey::register` answers
//!    `Ok` on this Mac, for `` ⌃` ``, which is what the shipped macOS default
//!    is;
//! ③ **a second claim on the same chord is refused as `AlreadyRegistered`** —
//!    the fault the Settings page's one line of dim text reads, measured rather
//!    than assumed, and `eventHotKeyExistsErr` proved to be what this OS answers;
//! ④ **the delivery road, end to end, with no keyboard in it**: a
//!    `kEventHotKeyPressed` sent to the application's own event target — which
//!    is where the system's hot key manager puts one — reaches the handler this
//!    crate installed, **once**; and the two events its gate must refuse, a
//!    foreign four-character signature and an id this process holds no claim
//!    under, reach nothing. That is `summon_should_act`'s rule measured on the
//!    machine instead of argued on paper;
//! ⑤ **the press, on the machine**: one `CGEventPost(kCGHIDEventTap, …)` of that
//!    chord and nothing else. Whether that can be driven at all from an agent's
//!    session is itself measured — `AXIsProcessTrusted()`, the read that never
//!    prompts — because `CGEventPost` of a *keyboard* event is one of the calls
//!    macOS gates behind the Accessibility grant and it answers nothing at all
//!    when it is dropped;
//! ⑥ **dropping the claim gives the key back**: the same chord registers again
//!    afterwards, which it could not if `GlobalHotkey::drop` had not called
//!    `UnregisterEventHotKey`.
//!
//! **This process posts exactly one chord, to its own registered key, and only
//! after ② said the key is ours.** The owner is a person who uses this machine.
//! Nothing is posted if the claim was refused — the key would then belong to
//! whatever program took it, and one stray `` ⌃` `` in their editor is a cost
//! this proof has no right to spend.

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use bt_platform::hotkey::{
        GlobalHotkey, Hotkey, SummonKey, carbon_key_code, carbon_registration_bits, register,
        summons_wake,
    };
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::WindowId;

    /// The id this proof claims under. **Not** `bt_app::quake::SUMMON_HOTKEY_ID`
    /// by accident: it is the same number the product uses, because what is
    /// being proved is the road the product takes, and no other Folio is running
    /// inside this bundle.
    const SUMMON_HOTKEY_ID: i32 = 1;

    /// How many times the handler has fired since the process started.
    ///
    /// An atomic and not a channel: the handler runs on the main thread inside
    /// Carbon's dispatch, the loop reads it on the same thread one turn later,
    /// and a counter is what "fired **once**" is asked of.
    static FIRED: AtomicUsize = AtomicUsize::new(0);

    // ── CoreGraphics, declared by hand ─────────────────────────────────────
    //
    // `objc2-core-graphics` is in this crate's manifest but its `CGEvent`
    // feature is not, and turning it on would add generated bindings to the
    // **product** for the sake of a test. These five are declared here instead,
    // which is what `bt_platform::macos_watch` does with CoreServices and for
    // the same reason: the file that needs a framework nobody has bound is the
    // file that declares it.

    type CGEventRef = *mut c_void;
    type CGEventSourceRef = *mut c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceCreate(state: i32) -> CGEventSourceRef;
        fn CGEventCreateKeyboardEvent(
            source: CGEventSourceRef,
            key_code: u16,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: u64);
        fn CGEventPost(tap: u32, event: CGEventRef);
        /// **Never prompts.** `AXIsProcessTrustedWithOptions` is the one that
        /// can, and it is the one this file must not call: a TCC prompt on the
        /// owner's desk is the cost this whole ticket was designed to avoid.
        /// This is the silent read, and it is what turns "the press did not
        /// arrive" into a diagnosis instead of a shrug.
        fn AXIsProcessTrusted() -> u8;
    }

    // ── the second half of Carbon: making an event rather than waiting for one
    //
    // `SendEventToEventTarget` puts an event into the application's own
    // dispatch, which is where the system's hot key manager puts one when a
    // reader presses the chord. It reaches the handler this crate installed by
    // the same road and through the same gate — so it proves everything about
    // the delivery *except* that macOS routes a physical press here, which is
    // the platform's guarantee rather than this code's.

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn CreateEvent(
            allocator: *const c_void,
            class_id: u32,
            kind: u32,
            when: f64,
            attributes: u32,
            event: *mut EventRef,
        ) -> i32;
        fn SetEventParameter(
            event: EventRef,
            name: u32,
            parameter_type: u32,
            size: usize,
            data: *const c_void,
        ) -> i32;
        fn SendEventToEventTarget(event: EventRef, target: EventTargetRef) -> i32;
        fn ReleaseEvent(event: EventRef);
        fn GetApplicationEventTarget() -> EventTargetRef;
    }

    type EventRef = *mut c_void;
    type EventTargetRef = *mut c_void;

    /// `EventHotKeyID`, as Carbon lays it out — the same two fields the handler
    /// in `bt_platform::hotkey` reads back out of an event.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct EventHotKeyID {
        signature: u32,
        id: u32,
    }

    // **Borrowed from the product and not copied out of it**
    // (T-MAC-SUMMON-DIAG), which is the whole reason this proof missed a
    // release's worth of a wrong number.
    //
    // These four were declared here as their own literals. One of them —
    // `kEventParamDirectObject` — was `'obj '` on both sides, and a synthetic
    // event written with a wrong name and read with the same wrong name
    // round-trips perfectly; only the system's own press, which this file could
    // not make without the Accessibility grant, carries the real `'----'`. A
    // test that writes its own copy of the number under test is not a proof of
    // that number, so it no longer has one: what `send_a_hot_key_event` sets is
    // what `bt_platform::hotkey`'s handler gets, by construction, and what makes
    // either of them right is the pin in `hotkey.rs`'s own test module that
    // reads the characters as well as the numbers.
    use bt_platform::hotkey::{
        K_EVENT_CLASS_KEYBOARD, K_EVENT_HOT_KEY_PRESSED, K_EVENT_PARAM_DIRECT_OBJECT,
        TYPE_EVENT_HOT_KEY_ID,
    };

    const K_EVENT_ATTRIBUTE_NONE: u32 = 0;
    const NO_ERR: i32 = 0;

    /// **Send the application the event the system sends it when the chord is
    /// pressed**, and say whether Carbon accepted it.
    ///
    /// The signature and the id are the caller's, so that the two the handler
    /// refuses can be sent as well as the one it answers.
    fn send_a_hot_key_event(signature: &[u8; 4], id: u32) -> bool {
        let mut event: EventRef = ptr::null_mut();
        // SAFETY: a constructor with a null allocator (the default), two
        // four-character codes by value, and an out-parameter that is a local.
        let made = unsafe {
            CreateEvent(
                ptr::null(),
                K_EVENT_CLASS_KEYBOARD,
                K_EVENT_HOT_KEY_PRESSED,
                0.0,
                K_EVENT_ATTRIBUTE_NONE,
                &raw mut event,
            )
        };
        if made != NO_ERR || event.is_null() {
            return false;
        }
        let named = EventHotKeyID {
            signature: u32::from_be_bytes(*signature),
            id,
        };
        // SAFETY: the event is the live one just created; the data pointer is a
        // local of exactly the size declared.
        let set = unsafe {
            SetEventParameter(
                event,
                K_EVENT_PARAM_DIRECT_OBJECT,
                TYPE_EVENT_HOT_KEY_ID,
                size_of::<EventHotKeyID>(),
                (&raw const named).cast::<c_void>(),
            )
        };
        if set != NO_ERR {
            // SAFETY: the event this function created.
            unsafe { ReleaseEvent(event) };
            return false;
        }
        // SAFETY: a live event and the application's own target, which outlives
        // the call. The handler runs synchronously inside this, on this thread.
        let sent = unsafe { SendEventToEventTarget(event, GetApplicationEventTarget()) };
        // SAFETY: the event this function created; the send does not take it.
        unsafe { ReleaseEvent(event) };
        // `eventNotHandledErr` is an answer, not a failure: it is what Carbon
        // says when nothing claimed the event, which is exactly the case the two
        // refusals below are about.
        sent == NO_ERR || sent == -9874
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(reference: *mut c_void);
    }

    /// `kCGEventSourceStateHIDSystemState` — the source a real keyboard's events
    /// come from, which is the one the hot key manager is watching.
    const K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: i32 = 1;
    /// `kCGHIDEventTap` — the point a physical key enters the system at, which
    /// is **above** the hot key manager. Posting to the session tap instead
    /// would put the event below it and is the placement macOS gates behind
    /// Accessibility.
    const K_CG_HID_EVENT_TAP: u32 = 0;
    /// `kCGEventFlagMaskControl`.
    const K_CG_EVENT_FLAG_MASK_CONTROL: u64 = 0x0004_0000;
    /// `kVK_Control`, posted as a key of its own so that the chord arrives the
    /// way a hand makes it — modifier down, key, key up, modifier up — rather
    /// than as one event wearing a flag nothing saw pressed.
    const K_VK_CONTROL: u16 = 0x3b;

    /// **Press this chord on the machine, once.**
    ///
    /// Four events and not one, which is what a finger produces: `⌃` down, the
    /// key down and up with the Control flag on them, `⌃` up. A single flagged
    /// key-down is enough for most listeners and is *not* reliably enough for
    /// the hot key manager, which tracks the modifier state it has been told
    /// about.
    ///
    /// Every event is released; `CGEventCreateKeyboardEvent` is a `Create`, so
    /// the caller owns what it answers.
    fn press(key_code: u16) -> bool {
        // SAFETY: a constructor taking one integer; null is the documented
        // answer when the source cannot be made, and every call below accepts a
        // null source as "no source".
        let source = unsafe { CGEventSourceCreate(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE) };
        if source.is_null() {
            return false;
        }
        let mut made = Vec::new();
        for (code, down, flags) in [
            (K_VK_CONTROL, true, K_CG_EVENT_FLAG_MASK_CONTROL),
            (key_code, true, K_CG_EVENT_FLAG_MASK_CONTROL),
            (key_code, false, K_CG_EVENT_FLAG_MASK_CONTROL),
            (K_VK_CONTROL, false, 0),
        ] {
            // SAFETY: the source is the live one made above; the key code is a
            // `u16` by value.
            let event = unsafe { CGEventCreateKeyboardEvent(source, code, down) };
            if event.is_null() {
                for event in made {
                    // SAFETY: each is a live `CGEventRef` this function created.
                    unsafe { CFRelease(event) };
                }
                // SAFETY: the source this function created.
                unsafe { CFRelease(source) };
                return false;
            }
            // SAFETY: a live event this function created; the flags are a
            // bitmask by value.
            unsafe { CGEventSetFlags(event, flags) };
            made.push(event);
        }
        for event in made {
            // SAFETY: a live event this function created, posted to a tap named
            // by a constant.
            unsafe { CGEventPost(K_CG_HID_EVENT_TAP, event) };
            // SAFETY: posting does not take ownership; this is the release that
            // balances `Create`.
            unsafe { CFRelease(event) };
        }
        // SAFETY: the source this function created.
        unsafe { CFRelease(source) };
        true
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

        fn claim(&mut self, claim: &str, held: bool, because: &str) {
            if held {
                self.pass(claim);
            } else {
                self.fail(claim, because);
            }
        }
    }

    /// **The shipped macOS default, built here rather than read out of
    /// `bt-app`.**
    ///
    /// This crate cannot see `bt_app`, and the pin holding `` ⌃` `` to the
    /// shortcut table is `bt_app::quake`'s own
    /// `every_platform_ships_the_summon_on_a_key_its_own_door_can_claim`, which
    /// runs on a Windows workstation. What is proved *here* is that this chord,
    /// whatever wrote it down, is a chord the machine accepts and answers.
    fn the_shipped_default() -> Hotkey {
        Hotkey {
            ctrl: true,
            alt: false,
            shift: false,
            win: false,
            virtual_key: carbon_key_code(SummonKey::Character('`'))
                .expect("the backtick has a position on every Mac keyboard"),
        }
    }

    /// The loop this proof runs its script on.
    ///
    /// Everything happens on the first turn except the wait for the press, which
    /// is what the deadline is for: a hot key arrives through the system, so
    /// "did it arrive" is a question with a *time* in it and the only honest
    /// answer to a missing one is "not within this long".
    struct Probe {
        report: Report,
        claimed: Option<GlobalHotkey>,
        posted_at: Option<Instant>,
        /// What the counter read the moment before the press was posted.
        ///
        /// A baseline and not a zero, because ④ above fires the handler three
        /// times on purpose before ⑤ posts anything, and "the press arrived"
        /// means *this* many more, not "more than none".
        fired_before_the_post: usize,
        started: bool,
        done: bool,
    }

    /// How long the press is waited for.
    ///
    /// Half a second, which is five hundred times the path's own latency and
    /// short enough that a run on a machine somebody is using is over before
    /// they notice it. A press that has not arrived in that long has not been
    /// delivered.
    const PRESS_DEADLINE: Duration = Duration::from_millis(500);

    impl ApplicationHandler<()> for Probe {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.started {
                return;
            }
            self.started = true;
            self.script(event_loop);
        }

        fn user_event(&mut self, _event_loop: &ActiveEventLoop, (): ()) {}

        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if self.done {
                return;
            }
            let Some(posted_at) = self.posted_at else {
                return;
            };
            let fired = FIRED.load(Ordering::SeqCst);
            // **Not the first sighting** — the deadline is run to the end even
            // when the handler has already fired, because ④'s second half is
            // that it fired *once*, and a press that produced two would look
            // exactly like a press that produced one if the loop left at the
            // first.
            if posted_at.elapsed() < PRESS_DEADLINE {
                event_loop.set_control_flow(ControlFlow::WaitUntil(posted_at + PRESS_DEADLINE));
                return;
            }
            self.done = true;
            let arrived = fired - self.fired_before_the_post;
            self.report.say(&format!(
                "MEASURED the handler fired {arrived} time(s) within {}ms of the post",
                PRESS_DEADLINE.as_millis()
            ));
            // **This half is the platform's, and whether it can be driven at all
            // from here is a thing to measure rather than assume.** `CGEventPost`
            // of a *keyboard* event is one of the calls macOS gates behind the
            // Accessibility grant; a process without it posts into a void, and
            // there is no error to read because the call answers nothing. So the
            // trust bit is read — silently, with the call that never prompts —
            // and it decides which of two sentences this is.
            let trusted = unsafe { AXIsProcessTrusted() } != 0;
            self.report
                .say(&format!("MEASURED AXIsProcessTrusted() = {trusted}"));
            if arrived >= 1 {
                self.report
                    .pass("a CGEvent press of the claimed chord reaches the Carbon handler");
                self.report.claim(
                    "and one press is one summon",
                    arrived == 1,
                    "the handler fired more than once for a single press",
                );
            } else if trusted {
                self.report.fail(
                    "a CGEvent press of the claimed chord reaches the Carbon handler",
                    "this process holds the Accessibility grant, so the post was \
                     delivered and the hot key did not answer it",
                );
            } else {
                self.report.say(
                    "NOT-CHECKABLE-BY-AN-AGENT a synthesised press: CGEventPost of a \
                     keyboard event needs the Accessibility grant on this macOS, this \
                     process does not hold it, and asking for one is the prompt this \
                     ticket's whole mechanism was chosen to avoid. The delivery road \
                     itself is proved above by the events sent to the application's own \
                     target. What a human should do: press ⌃` with another application \
                     frontmost, and Folio's summoned terminal should come down.",
                );
            }
            self.finish(event_loop);
        }
    }

    impl Probe {
        fn script(&mut self, event_loop: &ActiveEventLoop) {
            let chord = the_shipped_default();

            // ① the translation, held against what is about to be asked for.
            let Some((modifiers, key_code)) = carbon_registration_bits(chord) else {
                self.report
                    .fail("the shipped default translates", "it is not a chord at all");
                self.finish(event_loop);
                return;
            };
            self.report.say(&format!(
                "MEASURED carbon_registration_bits(⌃`) = modifiers {modifiers:#06x}, \
                 key code {key_code:#04x}"
            ));
            self.report.claim(
                "the shipped default is controlKey over kVK_ANSI_Grave",
                modifiers == 0x1000 && key_code == 0x32,
                "the numbers are not the ones §13.51 ③ names",
            );

            // ② the claim.
            let first = match register(SUMMON_HOTKEY_ID, chord) {
                Ok(claim) => {
                    self.report
                        .pass("RegisterEventHotKey claimed ⌃` on this Mac");
                    claim
                }
                Err(fault) => {
                    self.report.fail(
                        "RegisterEventHotKey claimed ⌃` on this Mac",
                        &format!("{fault:?}"),
                    );
                    // **Nothing is posted.** The key belongs to whoever took it,
                    // and this process has no right to press it in their window.
                    self.report
                        .say("NOT POSTED: the chord was not ours, so no event was sent");
                    self.finish(event_loop);
                    return;
                }
            };

            // ③ the second claim, which is the sentence the Settings page reads.
            let second = register(SUMMON_HOTKEY_ID + 1, chord);
            self.report.say(&format!(
                "MEASURED a second RegisterEventHotKey for the same chord: {second:?}"
            ));
            self.report.claim(
                "a chord already claimed is refused as AlreadyRegistered",
                matches!(&second, Err(fault) if fault.is_already_registered()),
                "this OS answers a rival claim with something else, so the Settings \
                 page's one line would say the wrong thing",
            );
            drop(second);

            // ④ **the delivery road, end to end, with no keyboard in it.**
            //
            // `SendEventToEventTarget` puts a `kEventHotKeyPressed` into the
            // application's own dispatch, which is where the system's hot key
            // manager puts one. It reaches the handler `bt_platform::hotkey`
            // installed by the same road and through the same gate, so what is
            // proved here is every line of this ticket's own delivery code. What
            // is *not* proved here is that macOS routes a physical press to this
            // application, which is the platform's guarantee and is ⑤'s
            // business.
            //
            // **And the two events the gate must refuse**, which is
            // `summon_should_act`'s rule measured rather than argued: a foreign
            // signature is somebody else's registration — every framework that
            // wants a shortcut has one — and an id this process holds no claim
            // under is the state a cleared row or a refused registration leaves
            // behind.
            let before = FIRED.load(Ordering::SeqCst);
            let sent = send_a_hot_key_event(b"folo", SUMMON_HOTKEY_ID as u32);
            let ours = FIRED.load(Ordering::SeqCst) - before;
            self.report.say(&format!(
                "MEASURED SendEventToEventTarget(ours) accepted={sent}, handler fired {ours} time(s)"
            ));
            self.report.claim(
                "the event the system sends on a press reaches the summon",
                sent && ours == 1,
                "the handler did not answer an event carrying this process's own claim",
            );

            let before = FIRED.load(Ordering::SeqCst);
            let sent = send_a_hot_key_event(b"xxxx", SUMMON_HOTKEY_ID as u32);
            let strangers = FIRED.load(Ordering::SeqCst) - before;
            self.report.say(&format!(
                "MEASURED SendEventToEventTarget(foreign signature) accepted={sent}, \
                 handler fired {strangers} time(s)"
            ));
            self.report.claim(
                "another registration's hot key is not this window's summon",
                strangers == 0,
                "the handler answered an event carrying somebody else's signature",
            );

            let before = FIRED.load(Ordering::SeqCst);
            let sent = send_a_hot_key_event(b"folo", 0x7e55);
            let unclaimed = FIRED.load(Ordering::SeqCst) - before;
            self.report.say(&format!(
                "MEASURED SendEventToEventTarget(an id we hold no claim under) \
                 accepted={sent}, handler fired {unclaimed} time(s)"
            ));
            self.report.claim(
                "a summon nobody registered is not acted on",
                unclaimed == 0,
                "the handler answered an id this process holds no claim under",
            );

            // ⑤ the press, on the machine.
            //
            // The key code comes back from the translation as the `u32`
            // `RegisterEventHotKey` takes; `CGEventCreateKeyboardEvent` wants the
            // `u16` every key code actually is, and the narrowing is asserted
            // rather than truncated — a key code that did not fit would be a
            // different key.
            self.claimed = Some(first);
            let Ok(key_code) = u16::try_from(key_code) else {
                self.report.fail(
                    "the key code fits the event constructor",
                    "a virtual key code wider than 16 bits is not a key",
                );
                self.finish(event_loop);
                return;
            };
            self.fired_before_the_post = FIRED.load(Ordering::SeqCst);
            if press(key_code) {
                self.posted_at = Some(Instant::now());
                self.report
                    .say("POSTED one ⌃` chord to kCGHIDEventTap — four events, one press");
                event_loop
                    .set_control_flow(ControlFlow::WaitUntil(Instant::now() + PRESS_DEADLINE));
            } else {
                self.report.fail(
                    "the press was posted",
                    "CoreGraphics would not make the events",
                );
                self.finish(event_loop);
            }
        }

        /// ⑥, and the end.
        fn finish(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(claim) = self.claimed.take() {
                drop(claim);
                match register(SUMMON_HOTKEY_ID, the_shipped_default()) {
                    Ok(again) => {
                        self.report
                            .pass("dropping the claim gave the chord back — it registers again");
                        drop(again);
                    }
                    Err(fault) => self.report.fail(
                        "dropping the claim gave the chord back",
                        &format!("the second registration was refused: {fault:?}"),
                    ),
                }
            }
            let failures = self.report.failures;
            self.report.say(&format!("FAILURES {failures}"));
            self.report.say("ALL_DONE");
            event_loop.exit();
        }
    }

    pub fn run() {
        let Some(bundle) = enclosing_bundle() else {
            println!(
                "macos_hotkey: skipped — this binary is not inside a .app, and a real \
                 application's event dispatch is the whole exercise"
            );
            return;
        };
        let work = bundle
            .parent()
            .expect("a bundle is inside a directory")
            .to_path_buf();
        let mut report = Report::at(&work.join("m4-8-hotkey-report.log"));
        report.say(&format!(
            "pid={} bundle={}",
            std::process::id(),
            bundle.display()
        ));

        // **Said before the loop is built, which is before anything can be
        // claimed** — the product's own order (`bt_app::main`). A press that
        // arrived in the window between the two would have nowhere to go.
        summons_wake(|| {
            FIRED.fetch_add(1, Ordering::SeqCst);
        });

        let event_loop = EventLoop::<()>::with_user_event()
            .build()
            .expect("an event loop");
        let mut probe = Probe {
            report,
            claimed: None,
            posted_at: None,
            fired_before_the_post: 0,
            started: false,
            done: false,
        };
        event_loop.set_control_flow(ControlFlow::Wait);
        match event_loop.run_app(&mut probe) {
            Ok(()) => probe.report.say("run_app returned Ok"),
            Err(error) => probe
                .report
                .fail("the event loop ran to the end", &format!("{error}")),
        }
        // A refusal that reached no claim leaves the script unrun; say so rather
        // than letting a silent report read as a pass.
        if !probe.started {
            probe
                .report
                .say("FAIL the loop never resumed, so nothing was proved");
            probe.report.say("ALL_DONE");
        }
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macos_hotkey: nothing to run on this platform");
}
