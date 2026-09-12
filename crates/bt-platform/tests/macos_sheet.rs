//! **A real `NSOpenPanel` sheet, on a real window, cancelled through the
//! panel's own Cancel** — the one claim M2-3 makes that needs a window server
//! (ticket M2-3, `docs/DESIGN.md` §13.16).
//!
//! # Why this is a target of its own rather than two `#[test]`s
//!
//! Everything here is AppKit and AppKit is **the main thread's**. libtest does
//! not give a case the main thread: measured on this workspace's toolchain
//! (1.94.1, aarch64-apple-darwin, 2026-09-12), a case run with
//! `--test-threads=1` still executes on a thread libtest spawned, and
//! `MainThreadMarker::new()` there is `None` — so a `#[test]`, however it is
//! gated, can construct an `NSWindow` on no thread at all. `harness = false`
//! hands this file the process's own `main`, which *is* the main thread, and
//! the whole of the proof follows from that.
//!
//! It is `tests/` rather than `src/bin/` because it is a test and is run by
//! `cargo test -p bt-platform` like every other; the two `BT_` names below are
//! outside the environment document's walk for the same reason `bt-replay`'s
//! are, and `docs/BT-ENVIRONMENT.md` §4 says so where a reader looks.
//!
//! # What it costs a run that is not asking for it
//!
//! Nothing. `main` returns before it touches AppKit unless **`BT_MAC_GUI`** is
//! set, so an ordinary `cargo test -p bt-platform` on the Mac runs this binary,
//! prints one line and exits — no window, no sheet, no application. The variable
//! is consent rather than configuration: these cases put windows on somebody's
//! desk.
//!
//! **`BT_MAC_GUI_SHOT`**, when it names a directory, additionally writes each
//! sheet's window number into it and holds that sheet up for two and a half
//! seconds, so that the session which started this probe can photograph it.
//! The photographer is outside this process on purpose — see
//! [`mac::stand_for_a_photograph`].
//!
//! # How it is run
//!
//! An ssh session cannot reach the window server (X-1 measured that too), so on
//! the Mac mini this binary is put inside a throwaway `.app` and started with
//! `open`, which asks launchd to run it in the logged-in session. The launcher
//! that does it is `~/folio-port/launchers/m2_3_gui.sh`.
//!
//! # What it proves, in order
//!
//! ① the panel is a **sheet on the window** — `NSWindow.attachedSheet` is the
//!    panel, which is what distinguishes it from an application-modal panel that
//!    would have blocked this thread instead of returning to it;
//! ② a second `request` while it is up is **coalesced**, not stacked;
//! ③ nothing is collectable while the sheet is still standing;
//! ④ `cancel:` — the panel's own action, so no key is posted and no
//!    Accessibility grant is needed — lands as `Ok(None)`, once.

#[cfg(target_os = "macos")]
mod mac {
    use std::ptr::NonNull;

    use bt_platform::{FolderPicker, ImagePicker, NativeWindow, ShellPickKind};
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSOpenPanel, NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{
        NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize, ns_string,
    };

    /// The owner's consent, and the main thread this file is written for.
    fn asked_for(name: &str) -> Option<MainThreadMarker> {
        if std::env::var_os("BT_MAC_GUI").is_none() {
            println!("{name}: skipped — set BT_MAC_GUI to open real windows");
            return None;
        }
        let mtm = MainThreadMarker::new().expect(
            "this target is `harness = false` precisely so that it owns the main thread, and it \
             is not on it",
        );
        // A window is ordered front and a sheet is begun on `NSApp`, and nothing
        // here calls `NSApplicationMain`, so the application is brought up by
        // hand — once per process, because `finishLaunching` is not idempotent.
        static LAUNCHED: std::sync::Once = std::sync::Once::new();
        LAUNCHED.call_once(|| NSApplication::sharedApplication(mtm).finishLaunching());
        Some(mtm)
    }

    /// A small real window, on screen, for a sheet to be attached to.
    fn a_window_to_sheet_onto(mtm: MainThreadMarker) -> Retained<NSWindow> {
        let frame = NSRect::new(NSPoint::new(120.0, 120.0), NSSize::new(640.0, 400.0));
        // SAFETY: `NSWindow`'s designated initializer, on the main thread, with
        // a style mask and a backing store the class documents.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(ns_string!("Folio M2-3 sheet probe"));
        window.makeKeyAndOrderFront(None);
        window
    }

    /// The handle `bt-app` would have handed the backend: winit's is the content
    /// view, and so is this.
    fn handle_of(window: &NSWindow) -> NativeWindow {
        let view = window
            .contentView()
            .expect("a titled window has a content view");
        NativeWindow::from_appkit(NonNull::from(&*view).cast())
    }

    /// Turn the main run loop until `done`, or give up after four seconds.
    ///
    /// A sheet does not appear synchronously and a completion handler does not
    /// run synchronously; both are the run loop's, and a check made straight
    /// after `request` would be a check about a moment that has not happened.
    fn turn_until(mut done: impl FnMut() -> bool) -> bool {
        let loops = NSRunLoop::currentRunLoop();
        for _ in 0..400 {
            if done() {
                return true;
            }
            autoreleasepool(|_| {
                let until = NSDate::dateWithTimeIntervalSinceNow(0.01);
                // SAFETY: the mode is AppKit's own default-mode constant and the
                // date is a live object; running the loop on the thread that owns
                // it is what this function is for.
                unsafe { loops.runMode_beforeDate(NSDefaultRunLoopMode, &until) };
            });
        }
        done()
    }

    /// Turn the run loop until `poll` answers, and hand that answer back.
    fn collect<T>(mut poll: impl FnMut() -> Option<T>) -> Option<T> {
        let mut answer = None;
        turn_until(|| {
            answer = poll();
            answer.is_some()
        });
        answer
    }

    /// The sheet the window is wearing.
    fn the_attached_sheet(host: &NSWindow) -> Retained<NSWindow> {
        host.attachedSheet()
            .expect("the panel is a sheet on this window")
    }

    /// Say where the sheet is and hold it there long enough to be photographed,
    /// when the run asked for a picture.
    ///
    /// **The photographer is outside this process**, and that is not a detour.
    /// `screencapture -l<window>` and `CGWindowListCreateImage` are both gated
    /// on the Screen Recording grant, and a throwaway bundle assembled by a
    /// launcher is a new application to TCC every time — measured: the call from
    /// in here answers `could not create image from window`. The ssh session
    /// that builds and starts this probe **does** hold that grant, so what this
    /// function does is hand it the one thing it cannot work out for itself, the
    /// sheet's window number, and then keep turning the run loop while the
    /// picture is taken. Turning it rather than sleeping matters: a sheet is
    /// only drawn by a loop that is running, and a `sleep` here would photograph
    /// a window that had not been asked to paint.
    fn stand_for_a_photograph(sheet: &NSWindow, name: &str) {
        let Some(into) = std::env::var_os("BT_MAC_GUI_SHOT") else {
            return;
        };
        let mut path = std::path::PathBuf::from(into);
        path.push(format!("{name}.window"));
        let number = sheet.windowNumber();
        let wrote = std::fs::write(
            &path,
            format!(
                "{number}
"
            ),
        );
        println!(
            "  standing as window {number} for {} -> {wrote:?}",
            path.display()
        );
        let until = std::time::Instant::now() + std::time::Duration::from_millis(2500);
        turn_until(|| std::time::Instant::now() >= until);
    }

    /// Dismiss the sheet through the panel's own Cancel action.
    fn cancel_the_attached_sheet(host: &NSWindow) {
        let panel = the_attached_sheet(host)
            .downcast::<NSOpenPanel>()
            .expect("the attached sheet is the open panel this ticket put there");
        // SAFETY: `sender` is allowed to be nil, which is what an action sent
        // programmatically passes.
        unsafe { panel.cancel(None) };
    }

    /// The folder chooser: the four claims in the file header, in order.
    fn a_folder_sheet_is_attached_to_the_window_and_cancels_to_nothing() {
        let name = "a_folder_sheet_is_attached_to_the_window_and_cancels_to_nothing";
        let Some(mtm) = asked_for(name) else {
            return;
        };
        let host = a_window_to_sheet_onto(mtm);
        let picker = FolderPicker::new(handle_of(&host)).expect("the folder chooser builds");
        assert_eq!(picker.request(None), Ok(true), "the sheet goes up");
        assert!(
            turn_until(|| host.attachedSheet().is_some()),
            "① the panel is a sheet on this window"
        );
        stand_for_a_photograph(&the_attached_sheet(&host), "folder-sheet");
        assert_eq!(
            picker.request(None),
            Ok(false),
            "② a second press while it is up is one sheet, not two"
        );
        assert!(
            picker.take_result().is_none(),
            "③ there is no answer while the sheet is still standing"
        );
        cancel_the_attached_sheet(&host);
        assert_eq!(
            collect(|| picker.take_result()),
            Some(Ok(None)),
            "④ a cancelled folder sheet is nothing"
        );
        assert!(
            picker.take_result().is_none(),
            "and the answer is handed over exactly once"
        );
        host.close();
        println!("{name}: ok");
    }

    /// The picture chooser: the same, and the only difference is what the panel
    /// would have let a reader choose.
    fn a_picture_sheet_is_attached_to_the_window_and_cancels_to_nothing() {
        let name = "a_picture_sheet_is_attached_to_the_window_and_cancels_to_nothing";
        let Some(mtm) = asked_for(name) else {
            return;
        };
        let host = a_window_to_sheet_onto(mtm);
        let picker = ImagePicker::new(handle_of(&host)).expect("the picture chooser builds");
        assert_eq!(
            picker.request(ShellPickKind::Image, None),
            Ok(true),
            "the sheet goes up"
        );
        assert!(
            turn_until(|| host.attachedSheet().is_some()),
            "① the panel is a sheet on this window"
        );
        stand_for_a_photograph(&the_attached_sheet(&host), "picture-sheet");
        assert_eq!(
            picker.request(ShellPickKind::Image, None),
            Ok(false),
            "② a second press while it is up is one sheet, not two"
        );
        cancel_the_attached_sheet(&host);
        assert_eq!(
            collect(|| picker.take_result()),
            Some(Ok(None)),
            "④ a cancelled picture sheet is nothing"
        );
        host.close();
        println!("{name}: ok");
    }

    pub fn run() {
        a_folder_sheet_is_attached_to_the_window_and_cancels_to_nothing();
        a_picture_sheet_is_attached_to_the_window_and_cancels_to_nothing();
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macos_sheet: nothing to run on this platform");
}
