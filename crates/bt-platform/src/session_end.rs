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
//! * `WM_ENDSESSION(TRUE)` — nothing: everything owed was handed over at the question.
//!
//! [`SessionEndWatch::saved`] takes the line down once the loop has written the layout.
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

/// **What the system said about the session ending.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionEnd {
    /// `WM_QUERYENDSESSION`: the session is about to end, and the question is answered yes.
    Asked,
    /// `WM_ENDSESSION` with `wParam` `FALSE`: it is not ending after all.
    TakenBack,
}

/// **Which session-end message this is, if it is one.** Pure; `WM_ENDSESSION(TRUE)` is `None`,
/// because by then there is nothing left to hear.
#[must_use]
pub fn heard(message: u32, wparam: usize) -> Option<SessionEnd> {
    match message {
        WM_QUERYENDSESSION => Some(SessionEnd::Asked),
        WM_ENDSESSION if wparam == 0 => Some(SessionEnd::TakenBack),
        _ => None,
    }
}

/// **What the window procedure answers a session-end message, once `hear` has been told.**
///
/// `TRUE` (1) to the question whatever `hear` does — a layout that could not be written in time
/// is not a reason to keep somebody's machine from shutting down, and the last completed save is
/// still on the disk — and 0 to `WM_ENDSESSION`, which is what an application that processed it
/// returns. `None` for every other message: the procedure forwards it untouched.
pub fn answer(message: u32, wparam: usize, hear: &dyn Fn(SessionEnd)) -> Option<isize> {
    let end = heard(message, wparam)?;
    hear(end);
    Some(match end {
        SessionEnd::Asked => 1,
        SessionEnd::TakenBack => 0,
    })
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
        match heard(message, wparam.0) {
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
            None => {}
        }
        match answer(message, wparam.0, &*hook.hear) {
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

        use super::super::{SessionEnd, WM_ENDSESSION, WM_QUERYENDSESSION};
        use super::{SESSION_END_SUBCLASS_ID, SessionEndHook, session_end_subclass};

        /// PIN — **the two numbers are the ones Windows gives the two messages.**
        #[test]
        fn the_two_messages_are_the_numbers_windows_gives_them() {
            assert_eq!(WM_QUERYENDSESSION, WindowsAndMessaging::WM_QUERYENDSESSION);
            assert_eq!(WM_ENDSESSION, WindowsAndMessaging::WM_ENDSESSION);
        }

        /// RED (B-ENDSESSION) — **the window procedure answers the system's question yes, and
        /// the news reaches the caller on the way.**
        ///
        /// The real subclass procedure, called in-process with the message the system would
        /// send (no window: nothing here reaches another process or the machine's session).
        /// Before this ticket the message went to `DefWindowProc` and nobody heard it.
        ///
        /// MUTATION: answer `answer`'s `Asked` arm with 0, or drop the `hear(end)` call.
        #[test]
        fn the_question_is_answered_yes_and_heard() {
            let heard = Rc::new(RefCell::new(Vec::new()));
            let into = Rc::clone(&heard);
            let hook = SessionEndHook {
                hear: Box::new(move |end| into.borrow_mut().push(end)),
                reason: Box::new(|| "Saving your layout".to_owned()),
            };
            let reference = (&hook as *const SessionEndHook) as usize;
            let ask = |message: u32, wparam: usize| {
                // SAFETY: the hook outlives the call; the null window makes every system call
                // inside refuse, which the procedure ignores.
                unsafe {
                    session_end_subclass(
                        HWND::default(),
                        message,
                        WPARAM(wparam),
                        LPARAM(0),
                        SESSION_END_SUBCLASS_ID,
                        reference,
                    )
                }
                .0
            };
            assert_eq!(
                ask(WM_QUERYENDSESSION, 0),
                1,
                "the question is answered TRUE"
            );
            assert_eq!(
                ask(WM_ENDSESSION, 0),
                0,
                "a shutdown taken back is processed"
            );
            let _ = ask(WM_ENDSESSION, 1);
            assert_eq!(
                *heard.borrow(),
                vec![SessionEnd::Asked, SessionEnd::TakenBack],
                "the session really ending says nothing new"
            );
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
    use super::{SessionEnd, WM_ENDSESSION, WM_QUERYENDSESSION, answer, heard};

    /// RED (B-ENDSESSION) — **the question and a shutdown taken back are heard; the session
    /// really ending, and everything else, is not.**
    ///
    /// MUTATION: hear `WM_ENDSESSION` whatever its `wParam`.
    #[test]
    fn only_the_question_and_a_shutdown_taken_back_are_heard() {
        assert_eq!(heard(WM_QUERYENDSESSION, 0), Some(SessionEnd::Asked));
        assert_eq!(heard(WM_ENDSESSION, 0), Some(SessionEnd::TakenBack));
        assert_eq!(heard(WM_ENDSESSION, 1), None);
        assert_eq!(heard(0x0010, 0), None, "WM_CLOSE is not the session ending");
        let told = std::cell::Cell::new(0);
        assert_eq!(answer(0x0010, 0, &|_| told.set(told.get() + 1)), None);
        assert_eq!(told.get(), 0, "a message that is not heard tells nobody");
    }
}
