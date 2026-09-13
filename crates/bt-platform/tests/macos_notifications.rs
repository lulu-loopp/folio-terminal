//! **A real notification, posted into the real notification centre and read
//! back out of it, inside a real `.app`** — the claims M4-6 makes that no
//! `#[test]` can hold (ticket M4-6, `docs/DESIGN.md` §13.30).
//!
//! # Why this is a target of its own
//!
//! `macos_sheet.rs`'s two reasons and one of its own.
//!
//! **The main thread.** The delegate is set from it and the Dock tile is
//! written from it, and libtest does not hand a case that thread — M2-3
//! measured it on this workspace's toolchain. `harness = false` gives this file
//! the process's own `main`.
//!
//! **The bundle.** `+[UNUserNotificationCenter currentNotificationCenter]`
//! raises an Objective-C exception in a process with no bundle identifier, so
//! everything this file is about is unreachable from `target/debug/deps`. The
//! gate is therefore not an environment variable — it is the thing that is
//! already true of the run that wants this: the binary is inside a `.app`. An
//! ordinary `cargo test -p bt-platform` prints one line and exits, on macOS and
//! everywhere else, exactly as `macos_app_delegate.rs` does. (That same
//! ordinary run is where the *unbundled* half is proved: the `#[test]` in
//! `macos_notify.rs` asks for a `Notifier` out of `deps` and reads the refusal.)
//!
//! # The authorization prompt is not this file's to raise
//!
//! **A notification-authorization prompt is a system prompt on the owner's
//! screen**, and an agent may not put one there. So the settings are read
//! first: if the status is `notDetermined` this file prints `SKIPPED` and does
//! not ask. `Notifier::new` *does* ask — that is the product's own behaviour,
//! on the first notification a reader's own session raises — and this file
//! therefore builds one only when the answer already exists.
//!
//! # What it proves, in order
//!
//! ① this process has a bundle identifier, which is the thing the refusal path
//!    is about;
//! ② the Dock tile carries the reading `dock_badge_label` computes — `40%`, then
//!    `…` for a bar with no number, then nothing at all — and a dropped
//!    `Taskbar` takes its own badge down. **No authorization of any kind is
//!    involved in this half**, so it runs even when ③ is skipped;
//! ③ with authorization already granted, `Notifier::new` sets a delegate on the
//!    centre, `show` posts a request the centre accepts, and the notification
//!    comes back out of `getDeliveredNotifications` carrying **the launch string
//!    in `userInfo` under `folio.launch`** — which is the whole of what a click
//!    would later decode;
//! ④ what it posted is then removed again, so the machine is left as it was.
//!
//! The first line of the report is this process's own pid, so that a run which
//! hangs can be ended on a number the launcher recorded rather than on a name.
//!
//! **The click itself is NOT-CHECKABLE by an agent** and this file does not try:
//! clicking a banner needs `System Events`, and therefore Accessibility *and*
//! Automation, which X-4 measured as a TCC prompt nobody in an ssh session can
//! reach. §13.30 writes the human procedure down instead.

#[cfg(target_os = "macos")]
mod mac {
    use std::io::Write as _;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};

    use block2::RcBlock;
    use bt_platform::{NativeWindow, Taskbar, TaskbarProgress, TaskbarProgressState};
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSArray, NSBundle, NSDate, NSRunLoop, NSString, NSTimeInterval};
    use objc2_user_notifications::{
        UNAuthorizationStatus, UNNotification, UNNotificationSettings, UNUserNotificationCenter,
    };

    /// The key the product writes the route under. **Spelled here rather than
    /// read from the crate**, because that is what makes this a check: the
    /// constant lives in `macos_notify.rs` and is private, and a rename that
    /// forgot the reader on the other side would leave this line failing.
    const LAUNCH_KEY: &str = "folio.launch";

    /// The route this probe posts and expects to read back.
    const LAUNCH: &str = "w=4661&t=2&s=3";

    /// How long any one asynchronous answer is waited for.
    const DEADLINE: NSTimeInterval = 10.0;

    /// One turn of the main run loop, so that whatever queue an answer is
    /// delivered on gets to deliver it.
    const SLICE: NSTimeInterval = 0.05;

    // ── where this process is, and where it writes ─────────────────────────

    /// The `.app` this binary is inside, or `None` when it is not inside one.
    fn enclosing_bundle() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let bundle = exe.parent()?.parent()?.parent()?;
        (bundle.extension()? == "app").then(|| bundle.to_path_buf())
    }

    /// The report, which is where the findings really go: a process started by
    /// `open` has no terminal to print to.
    struct Report {
        into: Option<std::fs::File>,
        failures: usize,
    }

    impl Report {
        fn say(&mut self, line: &str) {
            println!("{line}");
            if let Some(file) = self.into.as_mut() {
                let _ = writeln!(file, "{line}");
                let _ = file.flush();
            }
        }

        fn check(&mut self, name: &str, held: bool, detail: &str) {
            if held {
                self.say(&format!("PASS {name}: {detail}"));
            } else {
                self.failures += 1;
                self.say(&format!("FAIL {name}: {detail}"));
            }
        }
    }

    /// Turn the run loop until `answered` says so, or until the deadline.
    ///
    /// A pumped loop and not a blocking wait, because the framework does not
    /// promise which queue a completion handler arrives on — and a main thread
    /// parked on a condition variable is a main queue that never drains.
    fn wait_for(answered: &AtomicBool) -> bool {
        let until = std::time::Instant::now() + std::time::Duration::from_secs_f64(DEADLINE);
        while std::time::Instant::now() < until {
            if answered.load(Ordering::SeqCst) {
                return true;
            }
            let slice = NSDate::dateWithTimeIntervalSinceNow(SLICE);
            NSRunLoop::currentRunLoop().runUntilDate(&slice);
        }
        answered.load(Ordering::SeqCst)
    }

    // ── the Dock tile ──────────────────────────────────────────────────────

    /// What the Dock is showing, as a Rust string.
    fn badge(mtm: MainThreadMarker) -> Option<String> {
        NSApplication::sharedApplication(mtm)
            .dockTile()
            .badgeLabel()
            .map(|label| label.to_string())
    }

    fn normal(percent: u64) -> TaskbarProgress {
        TaskbarProgress {
            state: TaskbarProgressState::Normal,
            value: Some((percent, 100)),
        }
    }

    fn the_dock_tile_carries_the_reading(report: &mut Report, mtm: MainThreadMarker) {
        let tile = match Taskbar::new(NativeWindow::stand_in(0)) {
            Ok(tile) => tile,
            Err(why) => {
                report.check("dock-tile", false, &format!("Taskbar::new refused: {why}"));
                return;
            }
        };
        let _ = tile.set_progress(normal(40));
        report.check(
            "dock-tile-percent",
            badge(mtm).as_deref() == Some("40%"),
            &format!("badgeLabel is {:?}", badge(mtm)),
        );
        let _ = tile.set_progress(TaskbarProgress {
            state: TaskbarProgressState::Indeterminate,
            value: None,
        });
        report.check(
            "dock-tile-no-number",
            badge(mtm).as_deref() == Some("…"),
            &format!("badgeLabel is {:?}", badge(mtm)),
        );
        let _ = tile.set_progress(normal(90));
        drop(tile);
        report.check(
            "dock-tile-cleared-on-drop",
            badge(mtm).is_none(),
            &format!("badgeLabel is {:?}", badge(mtm)),
        );
    }

    // ── the notification ───────────────────────────────────────────────────

    /// The authorization status, asked without asking the reader anything.
    fn authorization(center: &UNUserNotificationCenter) -> Option<UNAuthorizationStatus> {
        let answered = Arc::new(AtomicBool::new(false));
        let status = Arc::new(AtomicIsize::new(0));
        let (told, recorded) = (Arc::clone(&answered), Arc::clone(&status));
        let block = RcBlock::new(
            move |settings: core::ptr::NonNull<UNNotificationSettings>| {
                // SAFETY: the framework hands this block a live settings object for
                // the length of the call.
                let settings = unsafe { settings.as_ref() };
                recorded.store(settings.authorizationStatus().0, Ordering::SeqCst);
                told.store(true, Ordering::SeqCst);
            },
        );
        center.getNotificationSettingsWithCompletionHandler(&block);
        wait_for(&answered).then(|| UNAuthorizationStatus(status.load(Ordering::SeqCst)))
    }

    /// Every notification of this application's that the centre is still
    /// holding.
    fn delivered(center: &UNUserNotificationCenter) -> Vec<Retained<UNNotification>> {
        let answered = Arc::new(AtomicBool::new(false));
        // An `Arc` over a type that is neither `Send` nor `Sync`, on purpose: Apple
        // documents that this completion handler may run on a background queue,
        // so the vector is shared across threads under the `Mutex` and read only
        // after `answered` (SeqCst) says the handler has returned. `Rc` would
        // state the opposite of what the framework does.
        #[allow(clippy::arc_with_non_send_sync)]
        let held: Arc<Mutex<Vec<Retained<UNNotification>>>> = Arc::new(Mutex::new(Vec::new()));
        let (told, into) = (Arc::clone(&answered), Arc::clone(&held));
        let block = RcBlock::new(
            move |notifications: core::ptr::NonNull<NSArray<UNNotification>>| {
                // SAFETY: the framework hands this block a live array for the
                // length of the call; every element is retained on the way out.
                let notifications = unsafe { notifications.as_ref() };
                let mut into = into.lock().unwrap_or_else(PoisonError::into_inner);
                into.extend(notifications.iter());
                told.store(true, Ordering::SeqCst);
            },
        );
        center.getDeliveredNotificationsWithCompletionHandler(&block);
        if !wait_for(&answered) {
            return Vec::new();
        }
        std::mem::take(&mut *held.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The route one delivered notification carries, read the way a click would
    /// read it.
    fn route_of(notification: &UNNotification) -> Option<String> {
        let info = notification.request().content().userInfo();
        let key = NSString::from_str(LAUNCH_KEY);
        let value = info.objectForKey(key.as_ref())?;
        value.downcast_ref::<NSString>().map(NSString::to_string)
    }

    fn a_notification_goes_out_and_comes_back(report: &mut Report) {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let Some(status) = authorization(&center) else {
            report.say("SKIPPED notification: the centre did not answer about authorization");
            return;
        };
        report.say(&format!("authorization status = {}", status.0));
        if status == UNAuthorizationStatus::NotDetermined {
            report.say(
                "SKIPPED notification: authorization is notDetermined, and asking would put a \
                 system prompt on the owner's screen. A bundle that has never asked is not \
                 listed in System Settings either, so the one way past this is a person: open \
                 this probe app yourself once and answer the prompt, after which every later \
                 run of this case is unattended",
            );
            return;
        }
        if status == UNAuthorizationStatus::Denied {
            report.say(
                "SKIPPED notification: this bundle's notifications are denied, which is the \
                 refusal `show` reports and not something this probe can post through",
            );
            return;
        }
        let before = delivered(&center).len();
        let mut notifier = match bt_platform::Notifier::new(Box::new(|| {})) {
            Ok(notifier) => notifier,
            Err(why) => {
                report.check("notifier", false, &format!("Notifier::new refused: {why}"));
                return;
            }
        };
        report.check(
            "notifier-delegate",
            center.delegate().is_some(),
            "a delegate is set on the current notification centre",
        );
        match notifier.show("Folio M4-6 probe", "the body line", LAUNCH) {
            Ok(()) => report.say("show: accepted"),
            Err(why) => {
                report.check("notifier-show", false, &format!("show refused: {why}"));
                return;
            }
        }
        // The request is accepted asynchronously; a delivered notification is a
        // moment or two after that.
        let settled = AtomicBool::new(false);
        let _ = wait_for(&settled);
        let after = delivered(&center);
        let ours: Vec<_> = after
            .iter()
            .filter(|notification| route_of(notification).as_deref() == Some(LAUNCH))
            .collect();
        report.check(
            "notification-delivered",
            !ours.is_empty(),
            &format!(
                "{} delivered notification(s) carry {LAUNCH_KEY}={LAUNCH}; the centre held {before} \
                 before and {} after",
                ours.len(),
                after.len()
            ),
        );
        for notification in &ours {
            let identifier = notification.request().identifier();
            report.check(
                "notification-identifier",
                identifier.to_string().starts_with("folio.notification."),
                &format!("identifier is {identifier}"),
            );
        }
        // ④ Leave the machine as it was found.
        let identifiers: Vec<Retained<NSString>> = ours
            .iter()
            .map(|notification| notification.request().identifier())
            .collect();
        let borrowed: Vec<&NSString> = identifiers.iter().map(|held| &**held).collect();
        center.removeDeliveredNotificationsWithIdentifiers(&NSArray::from_slice(&borrowed));
        let settled = AtomicBool::new(false);
        let _ = wait_for(&settled);
        report.check(
            "notification-removed",
            delivered(&center)
                .iter()
                .all(|notification| route_of(notification).as_deref() != Some(LAUNCH)),
            "the probe's own notification is no longer in the centre",
        );
        drop(notifier);
    }

    pub fn run() {
        let Some(bundle) = enclosing_bundle() else {
            println!("macos_notifications: not inside a .app, so there is no notification centre");
            return;
        };
        let into = bundle
            .parent()
            .map(|beside| beside.join("m4-6-report.log"))
            .and_then(|path| std::fs::File::create(path).ok());
        let mut report = Report { into, failures: 0 };
        // First line of the report, so that a run which hangs can be ended by
        // the launcher on **this** pid and on nothing that matches a name.
        report.say(&format!("pid {}", std::process::id()));
        let mtm = MainThreadMarker::new().expect(
            "this target is `harness = false` precisely so that it owns the main thread, and it \
             is not on it",
        );
        NSApplication::sharedApplication(mtm).finishLaunching();
        let identifier = NSBundle::mainBundle()
            .bundleIdentifier()
            .map(|identifier| identifier.to_string());
        report.check(
            "bundle-identifier",
            identifier.is_some(),
            &format!("CFBundleIdentifier is {identifier:?}"),
        );
        the_dock_tile_carries_the_reading(&mut report, mtm);
        a_notification_goes_out_and_comes_back(&mut report);
        let failures = report.failures;
        report.say(&format!("m4-6: {failures} failure(s)"));
        report.say("ALL_DONE");
        if failures > 0 {
            std::process::exit(1);
        }
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macos_notifications: nothing to run on this platform");
}
