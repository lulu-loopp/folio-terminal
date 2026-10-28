//! **The system ending the session: a shutdown, a restart or a sign-out** (B-ENDSESSION, 0.4.6).
//!
//! # What Windows says, and when
//!
//! Before a session ends Windows *sends* every top-level window `WM_QUERYENDSESSION`, and once
//! every application has agreed it sends each of them `WM_ENDSESSION` — with `wParam` `TRUE` when
//! the session really is ending, and `FALSE` when somebody (another program, or the person at the
//! shutdown screen) stopped it. After an application returns from `WM_ENDSESSION(TRUE)` its
//! process may be ended at any moment. winit 0.30 hands both messages to `DefWindowProc`, so
//! before this module Folio never heard of a shutdown at all: its run ended with `session.lock`
//! still standing, and while the system took the shells down one by one the ordinary "this
//! shell has exited" road closed their panes and the autosave wrote the shrinking layout
//! (`docs/M2-persistence-schema-v1.md` §5.5; the owner's reboot of 2026-09-27).
//!
//! # The shape: one subclass per window, and the loop does the work
//!
//! The same hook as every other platform message this program reads —
//! [`crate::SystemSettingsWatch`]'s shape, one comctl32 subclass per window — and the same
//! discipline: the procedure runs inside somebody else's `SendMessage`, on the window's own
//! thread, while the event loop may be anywhere, so it does only what cannot wait for the loop's
//! next turn and hands the rest over:
//!
//! * `WM_QUERYENDSESSION` — the line the shutdown screen shows for this window is put up
//!   (`ShutdownBlockReasonCreate`), `hear` is told [`SessionEnd::Asked`], and the answer is
//!   `TRUE`: this program never stands in the way of a shutdown. What `hear` does with it is the
//!   caller's; in `bt-app` it holds the session document where it stands and wakes the loop,
//!   which writes it.
//! * `WM_ENDSESSION(FALSE)` — the line comes down and `hear` is told
//!   [`SessionEnd::TakenBack`]: the session goes on, and so does the application.
//! * `WM_ENDSESSION(TRUE)` — nothing: everything owed was handed over at the question, and the
//!   system ends the process.
//! * `WM_ENDSESSION(TRUE)` with `ENDSESSION_CLOSEAPP` in `lParam` — `hear` is told
//!   [`SessionEnd::Closed`]. This is the Restart Manager closing the application (an installer
//!   or the system's servicing needs a file it holds): **the session does not end**, so nobody
//!   ends the process, and an application that stays is one the installer reports as in use — and
//!   one a restart it registered for starts a second copy beside (T-RESTART-REGISTRATION).
//!
//! [`SessionEndWatch::saved`] takes the line down once the loop has written the layout.
//!
//! # Coming back after the system's restart
//!
//! [`register_restart`] is the other half of what an application tells Windows about its end:
//! `RegisterApplicationRestart`, so that after an update's reboot, a sign-in with "Restart apps"
//! on, or the Restart Manager's close, Windows starts it again as it starts Notepad, Edge and
//! Windows Terminal. The line is empty — Windows puts the executable's own path in front of it —
//! because a plain start already reopens the saved layout, and a word an older build does not
//! know would be refused by the build a rollback left. [`RESTART_FLAGS`] is which of the system's
//! four occasions Folio is started again on (the owner's ruling, 2026-10-27).
//!
//! # macOS and the rest
//!
//! There a sign-out, a restart and a shutdown ask the application to terminate —
//! `applicationShouldTerminate:`, which `bt-app` already answers through its own quit
//! transaction (M3-1) — so the watch is constructible and hears nothing.

/// `WM_QUERYENDSESSION`, as `WinUser.h` numbers it. Written out rather than imported so that
/// [`heard`] — the part that can be wrong without a window — is the same function on every
/// platform; the Windows test holds it against the `windows` crate's constant.
pub const WM_QUERYENDSESSION: u32 = 0x0011;

/// `WM_ENDSESSION`, as `WinUser.h` numbers it (see [`WM_QUERYENDSESSION`]).
pub const WM_ENDSESSION: u32 = 0x0016;

/// `ENDSESSION_CLOSEAPP`, the bit of a session-end message's `lParam` that says the Restart
/// Manager is closing this application and the session goes on (see [`WM_QUERYENDSESSION`]).
pub const ENDSESSION_CLOSEAPP: isize = 0x0000_0001;

/// `RESTART_NO_CRASH`: no restart after the process crashed.
pub const RESTART_NO_CRASH: u32 = 0x1;
/// `RESTART_NO_HANG`: no restart after Windows Error Reporting ended it as hung.
pub const RESTART_NO_HANG: u32 = 0x2;
/// `RESTART_NO_PATCH`: no restart after the Restart Manager closed it for an installation.
pub const RESTART_NO_PATCH: u32 = 0x4;
/// `RESTART_NO_REBOOT`: no restart after the system restarted for an update.
pub const RESTART_NO_REBOOT: u32 = 0x8;

/// **When Windows starts Folio again**: after an update's reboot, a sign-in with "Restart apps"
/// on, and the Restart Manager's close — **never after a crash or a hang** (the owner's ruling
/// put on 2026-10-27; recommended and in force until it says otherwise). A layout that crashes a
/// minute after its restore would be restored into the same crash on every restart, which Windows
/// Error Reporting's sixty seconds do not stop, and a hang's restart ends a process in the middle
/// of a write; the next start the person makes reopens the layout anyway.
pub const RESTART_FLAGS: u32 = RESTART_NO_CRASH | RESTART_NO_HANG;

/// **What the system said about the session ending.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionEnd {
    /// `WM_QUERYENDSESSION`: the session is about to end, and the question is answered yes.
    Asked,
    /// `WM_ENDSESSION` with `wParam` `FALSE`: it is not ending after all.
    TakenBack,
    /// `WM_ENDSESSION` with `wParam` `TRUE` and [`ENDSESSION_CLOSEAPP`]: the Restart Manager is
    /// closing this application, the session goes on, and the application leaves by itself.
    Closed,
}

/// **Which session-end message this is, if it is one.** Pure; `WM_ENDSESSION(TRUE)` without
/// [`ENDSESSION_CLOSEAPP`] is `None`, because by then there is nothing left to hear: the system
/// ends the process.
#[must_use]
pub fn heard(message: u32, wparam: usize, lparam: isize) -> Option<SessionEnd> {
    match message {
        WM_QUERYENDSESSION => Some(SessionEnd::Asked),
        WM_ENDSESSION if wparam == 0 => Some(SessionEnd::TakenBack),
        WM_ENDSESSION if lparam & ENDSESSION_CLOSEAPP != 0 => Some(SessionEnd::Closed),
        _ => None,
    }
}

/// **What the window procedure answers a session-end message, once `hear` has been told.**
///
/// `TRUE` (1) to the question whatever `hear` does — a layout that could not be written in time
/// is not a reason to keep somebody's machine from shutting down, and the last completed save is
/// still on the disk — and 0 to `WM_ENDSESSION`, which is what an application that processed it
/// returns. `None` for every other message: the procedure forwards it untouched.
pub fn answer(
    message: u32,
    wparam: usize,
    lparam: isize,
    hear: &dyn Fn(SessionEnd),
) -> Option<isize> {
    let end = heard(message, wparam, lparam)?;
    hear(end);
    Some(match end {
        SessionEnd::Asked => 1,
        SessionEnd::TakenBack | SessionEnd::Closed => 0,
    })
}

/// **Ask Windows to start this program again after the system's restart** —
/// `RegisterApplicationRestart` with no arguments and `flags` ([`RESTART_FLAGS`] in the product).
/// Once per process; nothing off Windows, where the system's end is AppKit's.
///
/// # Errors
/// The system refused the registration, with its error.
pub fn register_restart(flags: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Recovery::{
            REGISTER_APPLICATION_RESTART_FLAGS, RegisterApplicationRestart,
        };
        // SAFETY: no pointer is passed: a null line is a restart with no arguments.
        unsafe {
            RegisterApplicationRestart(
                windows::core::PCWSTR::null(),
                REGISTER_APPLICATION_RESTART_FLAGS(flags),
            )
        }
        .map_err(|error| format!("RegisterApplicationRestart refused: {error}"))
    }
    #[cfg(not(windows))]
    {
        let _ = flags;
        Ok(())
    }
}

/// **Take the registration back** — `UnregisterApplicationRestart`: this process is handing
/// itself over (an update's applier), and the system's restart is no longer its to ask for.
///
/// # Errors
/// The system refused, with its error.
pub fn withdraw_restart() -> Result<(), String> {
    #[cfg(windows)]
    {
        // SAFETY: no arguments; it acts on this process's own registration.
        unsafe { windows::Win32::System::Recovery::UnregisterApplicationRestart() }
            .map_err(|error| format!("UnregisterApplicationRestart refused: {error}"))
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

#[cfg(windows)]
pub use windows_session_end::SessionEndWatch;

#[cfg(windows)]
mod windows_session_end {
    use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::core::HSTRING;

    use super::{SessionEnd, answer, heard};
    use crate::NativeWindow;

    /// "BTSE". Distinct from every other subclass id in this crate.
    const SESSION_END_SUBCLASS_ID: usize = 0x4254_5345;

    /// **One window's ear for the session ending.** Dropping it removes the subclass.
    pub struct SessionEndWatch {
        hwnd: HWND,
        /// Held for as long as the subclass is installed, because the subclass reaches it through
        /// the reference data and nothing may move it.
        hook: Box<SessionEndHook>,
    }

    /// What the subclass calls: who to tell, and the line the shutdown screen shows meanwhile.
    pub(super) struct SessionEndHook {
        pub(super) hear: Box<dyn Fn(SessionEnd)>,
        pub(super) reason: Box<dyn Fn() -> String>,
    }

    impl SessionEndWatch {
        /// Subclass `window`. `hear` is told what the system said, on the window's own thread
        /// and inside the system's `SendMessage`, so it may only park the news and wake the loop;
        /// `reason` is read when the question arrives, so the line is in the language in force
        /// then.
        ///
        /// # Errors
        /// `SetWindowSubclass` refused, with the system's error number.
        pub fn install(
            window: NativeWindow,
            hear: Box<dyn Fn(SessionEnd)>,
            reason: Box<dyn Fn() -> String>,
        ) -> Result<Self, String> {
            let hwnd = window.as_hwnd();
            let hook = Box::new(SessionEndHook { hear, reason });
            let reference_data = (&*hook as *const SessionEndHook) as usize;
            // SAFETY: called on the window's own thread with a live HWND, and the box above
            // outlives the subclass — `Drop` removes it first.
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(session_end_subclass),
                    SESSION_END_SUBCLASS_ID,
                    reference_data,
                )
            };
            if !installed.as_bool() {
                return Err(format!(
                    "SetWindowSubclass(session end) failed: {}",
                    // SAFETY: reads this thread's last error; no preconditions.
                    unsafe { GetLastError().0 }
                ));
            }
            Ok(Self { hwnd, hook })
        }

        /// **The layout is written, or will not be: take the shutdown screen's line down.** A
        /// window with no line up answers an error, which is nothing to report.
        pub fn saved(&self) {
            // SAFETY: the HWND is this watch's own window, alive while the watch is.
            let _ = unsafe { ShutdownBlockReasonDestroy(self.hwnd) };
        }
    }

    impl Drop for SessionEndWatch {
        fn drop(&mut self) {
            // SAFETY: dropped on the thread that installed it; the subclass goes before the box
            // it reads through.
            unsafe {
                let _ = RemoveWindowSubclass(
                    self.hwnd,
                    Some(session_end_subclass),
                    SESSION_END_SUBCLASS_ID,
                );
            }
            let _ = &self.hook;
        }
    }

    /// The subclass procedure. Forwards every message, so the rest of the chain (winit's own
    /// procedure among them) still sees it, and replaces the answer only for the two it hears.
    pub(super) unsafe extern "system" fn session_end_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        let hook = reference_data as *const SessionEndHook;
        // SAFETY: forwarding untouched messages is the required subclass contract.
        let forwarded = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        if hook.is_null() {
            return forwarded;
        }
        // SAFETY: the owning `SessionEndWatch` holds this box for the whole installed interval
        // and removes the subclass before freeing it, on this same thread.
        let hook = unsafe { &*hook };
        match heard(message, wparam.0, lparam.0) {
            // The line goes up before the loop is woken: the write it names is the loop's.
            Some(SessionEnd::Asked) => {
                let reason = HSTRING::from((hook.reason)());
                // SAFETY: `hwnd` is the window this procedure was called for; the string lives
                // across the call. A refusal only means no line is shown.
                let _ = unsafe { ShutdownBlockReasonCreate(hwnd, &reason) };
            }
            Some(SessionEnd::TakenBack) => {
                // SAFETY: as above; a window with no line up answers an error, which is nothing.
                let _ = unsafe { ShutdownBlockReasonDestroy(hwnd) };
            }
            // The line came down when the layout was written, and the process is leaving.
            Some(SessionEnd::Closed) | None => {}
        }
        match answer(message, wparam.0, lparam.0, &*hook.hear) {
            Some(answered) => LRESULT(answered),
            None => forwarded,
        }
    }

    #[cfg(test)]
    mod tests {
        use std::cell::RefCell;
        use std::rc::Rc;

        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging;

        use super::super::{
            ENDSESSION_CLOSEAPP, RESTART_FLAGS, RESTART_NO_CRASH, RESTART_NO_HANG, RESTART_NO_PATCH,
            RESTART_NO_REBOOT, SessionEnd, WM_ENDSESSION, WM_QUERYENDSESSION, register_restart,
            withdraw_restart,
        };
        use super::{SESSION_END_SUBCLASS_ID, SessionEndHook, session_end_subclass};

        /// PIN — **the numbers are the ones Windows gives the two messages, the close's bit and
        /// the four restart flags.**
        #[test]
        fn the_numbers_are_the_ones_windows_gives_them() {
            use windows::Win32::System::Recovery;
            assert_eq!(WM_QUERYENDSESSION, WindowsAndMessaging::WM_QUERYENDSESSION);
            assert_eq!(WM_ENDSESSION, WindowsAndMessaging::WM_ENDSESSION);
            assert_eq!(
                ENDSESSION_CLOSEAPP.cast_unsigned(),
                WindowsAndMessaging::ENDSESSION_CLOSEAPP as usize
            );
            assert_eq!(RESTART_NO_CRASH, Recovery::RESTART_NO_CRASH.0);
            assert_eq!(RESTART_NO_HANG, Recovery::RESTART_NO_HANG.0);
            assert_eq!(RESTART_NO_PATCH, Recovery::RESTART_NO_PATCH.0);
            assert_eq!(RESTART_NO_REBOOT, Recovery::RESTART_NO_REBOOT.0);
        }

        /// RED (B-ENDSESSION; T-RESTART-REGISTRATION) — **the window procedure answers the
        /// system's question yes, and the news reaches the caller on the way; the Restart
        /// Manager's close is heard, the session really ending is not.**
        ///
        /// The real subclass procedure, called in-process with the message the system would
        /// send (no window: nothing here reaches another process or the machine's session).
        /// Before B-ENDSESSION the message went to `DefWindowProc` and nobody heard it; before
        /// T-RESTART-REGISTRATION the close's `lParam` was never read.
        ///
        /// MUTATION: answer `answer`'s `Asked` arm with 0, or drop the `hear(end)` call; pass
        /// `0` for `lParam` to `heard` in the subclass — the close is not heard.
        #[test]
        fn the_question_is_answered_yes_and_heard() {
            let heard = Rc::new(RefCell::new(Vec::new()));
            let into = Rc::clone(&heard);
            let hook = SessionEndHook {
                hear: Box::new(move |end| into.borrow_mut().push(end)),
                reason: Box::new(|| "Saving your layout".to_owned()),
            };
            let reference = (&hook as *const SessionEndHook) as usize;
            let ask = |message: u32, wparam: usize, lparam: isize| {
                // SAFETY: the hook outlives the call; the null window makes every system call
                // inside refuse, which the procedure ignores.
                unsafe {
                    session_end_subclass(
                        HWND::default(),
                        message,
                        WPARAM(wparam),
                        LPARAM(lparam),
                        SESSION_END_SUBCLASS_ID,
                        reference,
                    )
                }
                .0
            };
            assert_eq!(
                ask(WM_QUERYENDSESSION, 0, 0),
                1,
                "the question is answered TRUE"
            );
            assert_eq!(
                ask(WM_ENDSESSION, 0, 0),
                0,
                "a shutdown taken back is processed"
            );
            let _ = ask(WM_ENDSESSION, 1, 0);
            assert_eq!(
                *heard.borrow(),
                vec![SessionEnd::Asked, SessionEnd::TakenBack],
                "the session really ending says nothing new"
            );
            assert_eq!(
                ask(WM_ENDSESSION, 1, ENDSESSION_CLOSEAPP),
                0,
                "the Restart Manager's close is processed"
            );
            assert_eq!(
                heard.borrow().last(),
                Some(&SessionEnd::Closed),
                "and heard"
            );
        }

        /// This process's restart registration as the system reads it back — its flags and its
        /// line — or `None` when there is none.
        fn registration() -> Option<(u32, String)> {
            use windows::Win32::System::Recovery::GetApplicationRestartSettings;
            use windows::Win32::System::Threading::GetCurrentProcess;
            let mut line = [0u16; 1024];
            let mut size = u32::try_from(line.len()).expect("the buffer's length");
            let mut flags = 0u32;
            // SAFETY: the buffer is `size` characters long and outlives the call; the
            // pseudo-handle is this process's own.
            unsafe {
                GetApplicationRestartSettings(
                    GetCurrentProcess(),
                    Some(windows::core::PWSTR(line.as_mut_ptr())),
                    &raw mut size,
                    Some(&raw mut flags),
                )
            }
            .ok()?;
            let length = line.iter().position(|&unit| unit == 0).unwrap_or(line.len());
            Some((flags, String::from_utf16_lossy(&line[..length])))
        }

        /// RED (T-RESTART-REGISTRATION) — **the registration the product makes is the one the
        /// system reads back, and taking it back leaves none.**
        ///
        /// The real `RegisterApplicationRestart` on this test process (which exits normally, and
        /// is therefore never restarted), read back with `GetApplicationRestartSettings`: the
        /// ruled flags — no restart after a crash or a hang — and an empty line, the plain start
        /// Windows puts the executable's own path in front of.
        ///
        /// MUTATION: take `RESTART_NO_CRASH` out of `RESTART_FLAGS` — the system reads back 0x2;
        /// make `withdraw_restart` do nothing — the registration is still read back.
        #[test]
        fn the_registration_is_the_one_the_system_reads_back_and_is_taken_back() {
            register_restart(RESTART_FLAGS).expect("the system registers the restart");
            assert_eq!(
                registration(),
                Some((RESTART_NO_CRASH | RESTART_NO_HANG, String::new())),
                "no restart after a crash or a hang, and a plain start"
            );
            withdraw_restart().expect("the system takes it back");
            assert_eq!(registration(), None, "nothing is left registered");
        }
    }
}

/// **Off Windows, an ear that hears nothing** — see the module's header: the system's end arrives
/// as `applicationShouldTerminate:` on macOS, and the caller already answers that.
#[cfg(not(windows))]
pub struct SessionEndWatch {
    _nothing: (),
}

#[cfg(not(windows))]
impl SessionEndWatch {
    /// Constructs, subscribes to nothing, never calls `hear`.
    ///
    /// # Errors
    /// None; the signature is the Windows arm's.
    pub fn install(
        window: crate::NativeWindow,
        hear: Box<dyn Fn(SessionEnd)>,
        reason: Box<dyn Fn() -> String>,
    ) -> Result<Self, String> {
        let _ = (window, hear, reason);
        Ok(Self { _nothing: () })
    }

    /// Nothing is up to take down.
    pub fn saved(&self) {}
}

#[cfg(test)]
mod tests {
    use super::{
        ENDSESSION_CLOSEAPP, RESTART_FLAGS, RESTART_NO_CRASH, RESTART_NO_HANG, RESTART_NO_PATCH,
        RESTART_NO_REBOOT, SessionEnd, WM_ENDSESSION, WM_QUERYENDSESSION, answer, heard,
    };

    /// `ENDSESSION_CRITICAL | ENDSESSION_LOGOFF`, the other two bits a session-end `lParam`
    /// carries.
    const CRITICAL_AND_LOGOFF: isize = 0xC000_0000;

    /// RED (B-ENDSESSION; T-RESTART-REGISTRATION) — **the question, a shutdown taken back and
    /// the Restart Manager's close are heard; the session really ending, and everything else, is
    /// not.**
    ///
    /// MUTATION: hear `WM_ENDSESSION` whatever its `wParam`; ignore `lParam` in `heard` — the
    /// close is not heard.
    #[test]
    fn the_question_a_shutdown_taken_back_and_the_close_are_heard() {
        assert_eq!(heard(WM_QUERYENDSESSION, 0, 0), Some(SessionEnd::Asked));
        assert_eq!(
            heard(WM_QUERYENDSESSION, 0, ENDSESSION_CLOSEAPP),
            Some(SessionEnd::Asked),
            "the close asks first, as a shutdown does"
        );
        assert_eq!(heard(WM_ENDSESSION, 0, 0), Some(SessionEnd::TakenBack));
        assert_eq!(heard(WM_ENDSESSION, 1, 0), None);
        assert_eq!(heard(WM_ENDSESSION, 1, CRITICAL_AND_LOGOFF), None);
        assert_eq!(
            heard(WM_ENDSESSION, 1, ENDSESSION_CLOSEAPP),
            Some(SessionEnd::Closed)
        );
        assert_eq!(
            heard(WM_ENDSESSION, 1, ENDSESSION_CLOSEAPP | CRITICAL_AND_LOGOFF),
            Some(SessionEnd::Closed)
        );
        assert_eq!(
            heard(0x0010, 0, ENDSESSION_CLOSEAPP),
            None,
            "WM_CLOSE is not the session ending"
        );
        let told = std::cell::Cell::new(0);
        assert_eq!(answer(0x0010, 0, 0, &|_| told.set(told.get() + 1)), None);
        assert_eq!(told.get(), 0, "a message that is not heard tells nobody");
        assert_eq!(
            answer(WM_ENDSESSION, 1, ENDSESSION_CLOSEAPP, &|_| told.set(
                told.get() + 1
            )),
            Some(0),
            "the close is processed"
        );
        assert_eq!(told.get(), 1, "and told");
    }

    /// RED (T-RESTART-REGISTRATION; the owner's ruling) — **Windows starts Folio again after an
    /// update's reboot and the Restart Manager's close, and never after a crash or a hang.**
    ///
    /// MUTATION: register with `RESTART_NO_HANG` alone (a crash restart without the ruling), or
    /// add `RESTART_NO_REBOOT` (no restart after the update's reboot this ticket is for).
    #[test]
    fn the_restart_is_refused_after_a_crash_or_a_hang_and_only_then() {
        assert_eq!(
            RESTART_FLAGS & RESTART_NO_CRASH,
            RESTART_NO_CRASH,
            "no restart after a crash"
        );
        assert_eq!(
            RESTART_FLAGS & RESTART_NO_HANG,
            RESTART_NO_HANG,
            "no restart after a hang"
        );
        assert_eq!(
            RESTART_FLAGS & (RESTART_NO_REBOOT | RESTART_NO_PATCH),
            0,
            "a restart after the update's reboot and the installer's close"
        );
    }
}
