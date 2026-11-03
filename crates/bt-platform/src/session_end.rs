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
//! * `WM_ENDSESSION(TRUE)` — the last word before the system may end the process, so the one
//!   message whose procedure holds its answer back (plan row 201): `ending` is called, and it
//!   returns only once the question's write has its answer and `diagnostics.log` has been
//!   fenced. The write itself stays the loop's: the question's turn blocks this thread until it
//!   lands, so in the usual order it has landed before this message can even be dispatched.
//!   When this message overtook the wake the question posted, `ending` runs the messages already
//!   queued on this thread ([`drain_what_is_queued`]) — the wake among them — so that turn, and
//!   its one bounded wait, happen before the answer. Nothing here waits.
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
//! four occasions Folio is started again on (pending the owner's ruling; the coordinator's
//! interim choice, as recommended, 2026-10-27).
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
/// on, and the Restart Manager's close — **never after a crash or a hang** (pending the owner's
/// ruling; the coordinator's interim choice, as recommended, 2026-10-27). A layout that crashes a
/// minute after its restore would be restored into the same crash on every restart, which Windows
/// Error Reporting's sixty seconds do not stop, and a hang's restart ends a process in the middle
/// of a write; the next start the person makes reopens the layout anyway.
pub const RESTART_FLAGS: u32 = RESTART_NO_CRASH | RESTART_NO_HANG;

/// `ENDSESSION_CRITICAL`, the bit of a session-end message's `lParam` that says the session is
/// ending whatever any application answers.
pub const ENDSESSION_CRITICAL: isize = 0x4000_0000;
/// `ENDSESSION_LOGOFF`, the bit of a session-end message's `lParam` that says the person is
/// signing out (a shutdown and a restart sign out first).
pub const ENDSESSION_LOGOFF: isize = 0x8000_0000;

/// **The exit code of a process the system ended with the session**: `DBG_TERMINATE_PROCESS`
/// (0x40010004), which a console process — a pane's shell — is ended with when the session it
/// runs in ends (T-SHUTDOWN-KILLS-SHELLS-BEFORE-THE-LAYOUT-IS-SAVED, plan row 191).
///
/// **It arrives before the question does.** The owner's three shutdowns of 2026-11-02, on 0.4.8:
/// every pane's shell ended with this code, one turn after another, while Folio's own windows had
/// not yet been sent `WM_QUERYENDSESSION` — so the hold the question takes (B-ENDSESSION) came
/// after every shell had gone, and each of those exits had already closed its tab. A shell that
/// ends with this code did not end itself: it is the system's end of the session reaching the
/// pane before it reached the window, and `bt-app` keeps the pane rather than retiring it.
///
/// Off Windows no process ends with it (an exit status there is eight bits, and the system's end
/// arrives as `applicationShouldTerminate:` before any shell is ended), so the rule it carries is
/// inert there.
pub const SYSTEM_ENDED_EXIT_CODE: u32 = 0x4001_0004;

/// **One line for `diagnostics.log` saying a session-end message arrived** — which one, its
/// `wParam`, and the bits of its `lParam` named — or `None` for any other message.
///
/// Written for every session-end message, `WM_ENDSESSION(TRUE)` included, which nothing else
/// hears: it is the last thing the system tells this process before it may end it, and a
/// diagnosis that has to infer whether it came from silence is the diagnosis plan row 191 had to
/// make. Pure, so its shape is a thing a test can state.
#[must_use]
pub fn arrival_line(message: u32, wparam: usize, lparam: isize) -> Option<String> {
    let name = match message {
        WM_QUERYENDSESSION => "WM_QUERYENDSESSION",
        WM_ENDSESSION if wparam == 0 => "WM_ENDSESSION(FALSE)",
        WM_ENDSESSION => "WM_ENDSESSION(TRUE)",
        _ => return None,
    };
    let named: Vec<&str> = [
        (ENDSESSION_CLOSEAPP, "CLOSEAPP"),
        (ENDSESSION_CRITICAL, "CRITICAL"),
        (ENDSESSION_LOGOFF, "LOGOFF"),
    ]
    .into_iter()
    .filter(|(bit, _)| lparam & bit != 0)
    .map(|(_, word)| word)
    .collect();
    let flags = if named.is_empty() {
        "none".to_owned()
    } else {
        named.join("|")
    };
    Some(format!(
        "BT_SESSION_END the system sent {name}, lParam {:#010x} ({flags})",
        lparam.cast_unsigned()
    ))
}

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

/// **Whether this is `WM_ENDSESSION(TRUE)` with the session really ending** — not the Restart
/// Manager's close ([`ENDSESSION_CLOSEAPP`]), after which nobody ends the process. The one
/// message after which this process may be ended at any moment, and so the one the procedure
/// holds its answer back for (plan row 201). Pure.
#[must_use]
pub fn the_session_really_ends(message: u32, wparam: usize, lparam: isize) -> bool {
    message == WM_ENDSESSION && wparam != 0 && lparam & ENDSESSION_CLOSEAPP == 0
}

/// **What running the messages already queued came to** ([`drain_what_is_queued`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Drained {
    /// The caller's question was answered: before the first message, or by one of them.
    Answered,
    /// Every message queued before the drain began has run, and the question is still open — or
    /// the queue could not be marked, and nothing was run.
    NothingLeft,
}

/// **Run queued messages, one at a time, until `answered` says yes or `next` has nothing left
/// to run** — the drain's whole rule, without a window: `next` runs one message and says whether
/// it did. Never waits; the bound is `next`'s (the messages queued before the drain began).
pub fn drain_with(answered: &dyn Fn() -> bool, next: &mut dyn FnMut() -> bool) -> Drained {
    loop {
        if answered() {
            return Drained::Answered;
        }
        if !next() {
            return Drained::NothingLeft;
        }
    }
}

/// **Run the posted messages already queued on this thread, until `answered` says yes**
/// (plan row 201) — [`drain_with`] over this thread's queue.
///
/// The bound is a mark: a thread message of this module's own, posted at the start, behind
/// everything already posted. The drain stops when it reaches the mark, so it runs only what
/// was queued before it began, however fast new posts arrive; a mark left behind by an early
/// answer is a windowless thread message the loop's own pump discards. Only posted messages are
/// taken (`PM_QS_POSTMESSAGE`: no input, no paint); a `WM_QUIT` met on the way is posted again
/// and ends the drain, as every modal loop owes its caller. Never waits.
///
/// This is the system's own move/size loop's shape: the messages are dispatched from inside a
/// window procedure, and an event loop that is not inside its handler at that moment (winit's,
/// when this is called from the procedure of a message its pump dispatched) delivers each to the
/// handler as it comes. Off Windows there is no such queue: nothing runs.
#[must_use]
pub fn drain_what_is_queued(answered: &dyn Fn() -> bool) -> Drained {
    #[cfg(windows)]
    {
        windows_session_end::drain_what_is_queued(answered)
    }
    #[cfg(not(windows))]
    {
        drain_with(answered, &mut || false)
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

/// **The shutdown level Folio asks for: the top of the application range** (plan row 191,
/// adopted on the clean-VM reboot of 2026-11-03). Windows ends a session's processes in
/// descending level order, and every process starts at the default, `0x280` — Folio and the
/// console processes it hosts alike. At one level the order between them is the system's, and on
/// the owner's machine the shells were ended first. At `0x300` Folio is ended before any
/// process at the default level; on the VM's reboot Folio was ended first, its shells exited
/// with code 0 after it, and the layout on disk was unchanged. The range above `0x3FF` is the
/// system's own.
pub const SHUTDOWN_LEVEL: u32 = 0x300;

/// **Ask Windows to end this process before the processes at the default shutdown level** —
/// `SetProcessShutdownParameters(level, 0)` ([`SHUTDOWN_LEVEL`] in the product), so that the end
/// of the session reaches Folio's windows before it reaches the shells in its panes. Once per
/// process; nothing off Windows, where the system's end is AppKit's.
///
/// # Errors
/// The system refused the level, with its error.
pub fn ask_to_be_ended_first(level: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::SetProcessShutdownParameters;
        // SAFETY: two integers; it sets this process's own place in the shutdown order.
        unsafe { SetProcessShutdownParameters(level, 0) }
            .map_err(|error| format!("SetProcessShutdownParameters refused: {error}"))
    }
    #[cfg(not(windows))]
    {
        let _ = level;
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
    use std::rc::Rc;

    use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_QS_POSTMESSAGE, PM_REMOVE, PeekMessageW, PostQuitMessage,
        PostThreadMessageW, RegisterWindowMessageW, TranslateMessage, WM_QUIT,
    };
    use windows::core::{HSTRING, w};

    use super::{
        Drained, SessionEnd, answer, arrival_line, drain_with, heard, the_session_really_ends,
    };
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

    /// What the subclass calls: who to tell, the line the shutdown screen shows meanwhile, and
    /// what holds the answer to `WM_ENDSESSION(TRUE)` back (plan row 201).
    pub(super) struct SessionEndHook {
        pub(super) hear: Box<dyn Fn(SessionEnd)>,
        pub(super) reason: Box<dyn Fn() -> String>,
        /// Shared rather than boxed: it runs the thread's queued messages, and one of them may
        /// drop this window's watch — and this hook with it — while it runs. The procedure calls
        /// its own count of it and reads nothing of the hook after.
        pub(super) ending: Rc<dyn Fn()>,
    }

    impl SessionEndWatch {
        /// Subclass `window`. `hear` is told what the system said, on the window's own thread
        /// and inside the system's `SendMessage`, so it may only park the news and wake the loop;
        /// `reason` is read when the question arrives, so the line is in the language in force
        /// then; `ending` is called at `WM_ENDSESSION(TRUE)` with the session really ending, and
        /// the answer waits for it (plan row 201).
        ///
        /// # Errors
        /// `SetWindowSubclass` refused, with the system's error number.
        pub fn install(
            window: NativeWindow,
            hear: Box<dyn Fn(SessionEnd)>,
            reason: Box<dyn Fn() -> String>,
            ending: Rc<dyn Fn()>,
        ) -> Result<Self, String> {
            let hwnd = window.as_hwnd();
            let hook = Box::new(SessionEndHook {
                hear,
                reason,
                ending,
            });
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
        // Said before anything is done about it, so the line stands where it arrived among the
        // shells' exit lines (plan row 191): one queue, in order, written by the log's own thread.
        if let Some(line) = arrival_line(message, wparam.0, lparam.0) {
            crate::stderr_road::say(line);
        }
        // **The last word, held until the question's write has its answer** (plan row 201). The
        // hook's own count is taken first: what `ending` runs may drop this window's watch.
        if the_session_really_ends(message, wparam.0, lparam.0) {
            let ending = Rc::clone(&hook.ending);
            ending();
            return forwarded;
        }
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

    /// [`super::drain_what_is_queued`] on Windows: the mark posted, then this thread's posted
    /// messages run in order until the mark, an empty queue, a `WM_QUIT` or the answer.
    pub(super) fn drain_what_is_queued(answered: &dyn Fn() -> bool) -> Drained {
        // SAFETY: a static, NUL-terminated name; the call only registers it.
        let mark = unsafe { RegisterWindowMessageW(w!("Folio.SessionEnd.DrainMark")) };
        if mark == 0 {
            return drain_with(answered, &mut || false);
        }
        // SAFETY: a thread message to this thread, carrying no pointer.
        let marked = unsafe { PostThreadMessageW(GetCurrentThreadId(), mark, WPARAM(0), LPARAM(0)) };
        if marked.is_err() {
            return drain_with(answered, &mut || false);
        }
        drain_with(answered, &mut || {
            let mut message = MSG::default();
            // SAFETY: `message` is a live `MSG` this call fills in; `PM_REMOVE` takes a posted
            // message off this thread's own queue.
            let taken = unsafe {
                PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE | PM_QS_POSTMESSAGE)
            };
            if !taken.as_bool() || (message.message == mark && message.hwnd.is_invalid()) {
                return false;
            }
            if message.message == WM_QUIT {
                // `PostQuitMessage` put its `int` into `wParam`; this takes the same `int` back.
                let code = message.wParam.0 as i32;
                // SAFETY: no pointer; the quit goes back on the queue for the loop that owns it.
                unsafe { PostQuitMessage(code) };
                return false;
            }
            // SAFETY: `message` was just filled in by `PeekMessageW` and is not read again
            // before it is refilled.
            unsafe {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
            true
        })
    }

    #[cfg(test)]
    mod tests {
        use std::cell::RefCell;
        use std::rc::Rc;

        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging;

        use super::super::{
            ENDSESSION_CLOSEAPP, ENDSESSION_CRITICAL, ENDSESSION_LOGOFF, RESTART_FLAGS,
            RESTART_NO_CRASH, RESTART_NO_HANG, RESTART_NO_PATCH, RESTART_NO_REBOOT,
            SYSTEM_ENDED_EXIT_CODE, SessionEnd, WM_ENDSESSION, WM_QUERYENDSESSION,
            register_restart, withdraw_restart,
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
            assert_eq!(
                ENDSESSION_CRITICAL.cast_unsigned(),
                WindowsAndMessaging::ENDSESSION_CRITICAL as usize
            );
            assert_eq!(
                ENDSESSION_LOGOFF.cast_unsigned(),
                WindowsAndMessaging::ENDSESSION_LOGOFF as usize
            );
            assert_eq!(
                SYSTEM_ENDED_EXIT_CODE,
                windows::Win32::Foundation::DBG_TERMINATE_PROCESS
                    .0
                    .cast_unsigned(),
                "the code a console process is ended with when its session ends"
            );
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
        /// `0` for `lParam` to `heard` in the subclass — the close is not heard. MUTATION (plan
        /// row 201): drop the `ending()` call — the session really ending holds nothing back.
        #[test]
        fn the_question_is_answered_yes_and_heard() {
            let heard = Rc::new(RefCell::new(Vec::new()));
            let into = Rc::clone(&heard);
            let ended = Rc::new(std::cell::Cell::new(0_u32));
            let ends = Rc::clone(&ended);
            let hook = SessionEndHook {
                hear: Box::new(move |end| into.borrow_mut().push(end)),
                reason: Box::new(|| "Saving your layout".to_owned()),
                ending: Rc::new(move || ends.set(ends.get() + 1)),
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
            assert_eq!(ended.get(), 0, "nothing before the end holds anything back");
            let _ = ask(WM_ENDSESSION, 1, 0);
            assert_eq!(
                *heard.borrow(),
                vec![SessionEnd::Asked, SessionEnd::TakenBack],
                "the session really ending says nothing new"
            );
            assert_eq!(
                ended.get(),
                1,
                "and the end is held for, once, before the answer"
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
            assert_eq!(ended.get(), 1, "the close is not the end: nobody ends the process");
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
            let length = line
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(line.len());
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
        /// A failed assertion takes the registration back too (review F6): a test process left
        /// registered would be started again by a reboot or a Restart Manager's close in the
        /// middle of the run.
        ///
        /// MUTATION: take `RESTART_NO_CRASH` out of `RESTART_FLAGS` — the system reads back 0x2;
        /// make `withdraw_restart` do nothing — the registration is still read back.
        #[test]
        fn the_registration_is_the_one_the_system_reads_back_and_is_taken_back() {
            /// Takes this process's registration back however the test ends.
            struct TakenBack;
            impl Drop for TakenBack {
                fn drop(&mut self) {
                    let _ = withdraw_restart();
                }
            }
            register_restart(RESTART_FLAGS).expect("the system registers the restart");
            let _taken_back = TakenBack;
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
        ending: std::rc::Rc<dyn Fn()>,
    ) -> Result<Self, String> {
        let _ = (window, hear, reason, ending);
        Ok(Self { _nothing: () })
    }

    /// Nothing is up to take down.
    pub fn saved(&self) {}
}

#[cfg(test)]
mod tests {
    use super::{
        ENDSESSION_CLOSEAPP, ENDSESSION_CRITICAL, ENDSESSION_LOGOFF, RESTART_FLAGS,
        RESTART_NO_CRASH, RESTART_NO_HANG, RESTART_NO_PATCH, RESTART_NO_REBOOT, SHUTDOWN_LEVEL,
        Drained, SessionEnd, WM_ENDSESSION, WM_QUERYENDSESSION, answer, arrival_line, drain_with,
        heard, the_session_really_ends,
    };

    /// RED (plan row 191) — **every session-end message is said with its flags, and nothing else
    /// is**: the question, a shutdown taken back, the session really ending — which nothing else
    /// hears — and the Restart Manager's close, each with the bits of its `lParam` named.
    ///
    /// MUTATION: answer `None` for `WM_ENDSESSION(TRUE)` — the last message before the system may
    /// end the process is not said; drop the `LOGOFF` row — a restart's sign-out reads `none`.
    #[test]
    fn every_session_end_message_is_said_with_its_flags() {
        assert_eq!(
            arrival_line(WM_QUERYENDSESSION, 0, ENDSESSION_LOGOFF).as_deref(),
            Some("BT_SESSION_END the system sent WM_QUERYENDSESSION, lParam 0x80000000 (LOGOFF)")
        );
        assert_eq!(
            arrival_line(WM_ENDSESSION, 1, ENDSESSION_CRITICAL | ENDSESSION_LOGOFF).as_deref(),
            Some(
                "BT_SESSION_END the system sent WM_ENDSESSION(TRUE), lParam 0xc0000000 \
                 (CRITICAL|LOGOFF)"
            )
        );
        assert_eq!(
            arrival_line(WM_ENDSESSION, 0, 0).as_deref(),
            Some("BT_SESSION_END the system sent WM_ENDSESSION(FALSE), lParam 0x00000000 (none)")
        );
        assert_eq!(
            arrival_line(WM_ENDSESSION, 1, ENDSESSION_CLOSEAPP).as_deref(),
            Some(
                "BT_SESSION_END the system sent WM_ENDSESSION(TRUE), lParam 0x00000001 (CLOSEAPP)"
            )
        );
        assert_eq!(
            arrival_line(0x0010, 0, ENDSESSION_CLOSEAPP),
            None,
            "WM_CLOSE is not one"
        );
    }

    /// `ENDSESSION_CRITICAL | ENDSESSION_LOGOFF`, the other two bits a session-end `lParam`
    /// carries.
    const CRITICAL_AND_LOGOFF: isize = ENDSESSION_CRITICAL | ENDSESSION_LOGOFF;

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
            answer(WM_ENDSESSION, 1, ENDSESSION_CLOSEAPP, &|_| told
                .set(told.get() + 1)),
            Some(0),
            "the close is processed"
        );
        assert_eq!(told.get(), 1, "and told");
    }

    /// RED (plan row 201) — **only `WM_ENDSESSION(TRUE)` without the Restart Manager's close is
    /// the session really ending**: the question, a shutdown taken back and the close are not,
    /// whatever else their `lParam` carries.
    ///
    /// MUTATION: drop the `ENDSESSION_CLOSEAPP` test — the close, after which nobody ends the
    /// process, would hold its answer for the end.
    #[test]
    fn only_the_session_really_ending_is_the_end() {
        assert!(the_session_really_ends(WM_ENDSESSION, 1, 0));
        assert!(the_session_really_ends(WM_ENDSESSION, 1, CRITICAL_AND_LOGOFF));
        assert!(!the_session_really_ends(WM_ENDSESSION, 0, 0), "taken back");
        assert!(
            !the_session_really_ends(WM_ENDSESSION, 1, ENDSESSION_CLOSEAPP),
            "the Restart Manager's close"
        );
        assert!(!the_session_really_ends(WM_QUERYENDSESSION, 1, 0), "the question");
        assert!(!the_session_really_ends(0x0010, 1, 0), "WM_CLOSE");
    }

    /// RED (plan row 201) — **the drain runs queued messages until the answer is there or
    /// nothing queued is left, and runs none once it is.**
    ///
    /// Three queues: one whose third message answers (two more stay queued, never run); one that
    /// runs dry with no answer; and one answered before anything runs.
    ///
    /// MUTATION: in `drain_with`, ask `answered` only after `next` says it has nothing left — the
    /// messages queued behind the answer run too, and an answer is said as none.
    #[test]
    fn the_drain_runs_until_the_answer_or_the_end_of_what_was_queued() {
        let queued = std::cell::Cell::new(5_u32);
        let ran = std::cell::Cell::new(0_u32);
        let mut next = || {
            if queued.get() == 0 {
                return false;
            }
            queued.set(queued.get() - 1);
            ran.set(ran.get() + 1);
            true
        };
        assert_eq!(drain_with(&|| ran.get() >= 3, &mut next), Drained::Answered);
        assert_eq!((ran.get(), queued.get()), (3, 2), "nothing behind the answer runs");

        ran.set(0);
        queued.set(2);
        assert_eq!(drain_with(&|| false, &mut next), Drained::NothingLeft);
        assert_eq!(ran.get(), 2, "everything queued ran, and then it stopped");

        ran.set(0);
        queued.set(4);
        assert_eq!(drain_with(&|| true, &mut next), Drained::Answered);
        assert_eq!(ran.get(), 0, "an answer already there runs nothing");
    }

    /// RED (T-RESTART-REGISTRATION; the coordinator's interim choice, pending the owner's ruling) — **Windows starts Folio again after
    /// an
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

    /// RED (plan row 191) — **the level Folio asks for is above the default and inside the
    /// application range**: `0x280` is every process's default, so a level at or below it would
    /// leave the order between Folio and its shells to the system again, and a level above
    /// `0x3FF` is reserved for the system's own components.
    ///
    /// MUTATION: `SHUTDOWN_LEVEL = 0x280` — the default, and the order is the system's again.
    #[test]
    fn folio_asks_to_be_ended_before_the_default_level_and_inside_the_application_range() {
        // Above the default (`0x280`), and no higher than the application range's top (`0x3FF`).
        const ABOVE_THE_DEFAULT: std::ops::RangeInclusive<u32> = 0x281..=0x3FF;
        assert!(
            ABOVE_THE_DEFAULT.contains(&SHUTDOWN_LEVEL),
            "{SHUTDOWN_LEVEL:#x} is not above the default inside the application range"
        );
    }
}
