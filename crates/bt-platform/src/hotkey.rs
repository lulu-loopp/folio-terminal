//! **The key a window answers to when it is not on the screen** — the quake
//! terminal's summon (`docs/DESIGN.md` §7.54).
//!
//! A fifth unsafe boundary in this crate, against a fifth thing. `windows_impl`
//! is Win32 for the sake of a window this process owns, [`crate::webview`] is
//! WebView2, [`crate::hang`] is Win32 turned on this process, and
//! [`crate::attention_pipe`] is a channel other processes speak into. This is
//! Win32 turned on **the keyboard while somebody else has it** — a chord that
//! has to arrive when no window of ours is focused, and a foreground that has to
//! go back to whoever held it.
//!
//! # Why the two halves are one module
//!
//! A summon that cannot hand the foreground back is half a summon. The window
//! comes up over whatever the reader was doing; when it goes away again the
//! keyboard belongs to that other window, and nothing in this process will be
//! told to give it back. So the registration and [`give_foreground_to`] are one
//! subject and are written in one place — and the two Win32 dances they perform,
//! `RegisterHotKey` on a thread with no window and `AttachThreadInput` around a
//! `SetForegroundWindow`, are the same dance seen from either end: both exist
//! because the foreground is a thing Windows will not simply hand to a process
//! that does not already have it.
//!
//! # What is pure and what is not
//!
//! [`registration_bits`] is the whole of the translation from a chord to the two
//! integers `RegisterHotKey` takes, and it is pure so a test can hold it on any
//! host — the rule [`crate::custom_frame_hit_test`] is written under, for its
//! reason: it is the part that can be wrong without a keyboard. Everything below
//! it needs a message queue and is gated on Windows.

/// **A chord as `RegisterHotKey` understands one**: four modifier flags and the
/// virtual key they are held with.
///
/// A virtual key and not a character, because the layout question is answered
/// before a chord gets here — [`crate::virtual_key_for_character`] is the one
/// call that answers it, and it answers for the layout actually installed. This
/// type is what is left once that answer is in hand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The Windows key. No row of this product's own table wears it, but a
    /// global summon is exactly the kind of key a person reaches for it on, and
    /// dropping the flag here would make that a decision this crate had taken on
    /// their behalf.
    pub win: bool,
    pub virtual_key: u16,
}

/// Win32's `MOD_*` values, written as the numbers they are.
///
/// Constants of this crate's own rather than the `windows` crate's, for
/// [`crate::CustomFrameHit`]'s reason: the mapping is the part with an opinion
/// in it, so it is expressed without Win32 constants and then pinned *against*
/// them by a test that only builds where they exist. A number written twice and
/// checked once is one decision; a number a test reads out of the same constant
/// the code did is no check at all.
const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;
/// **Held is not pressed again.**
///
/// Without it Windows repeats `WM_HOTKEY` for as long as the key is down, at the
/// keyboard's own repeat rate — and this key's verb is a *toggle*, so a summon
/// held for half a second would show and hide the window a dozen times and land
/// on whichever parity the finger happened to lift on. One press of this chord
/// means one thing, so exactly one message is asked for.
const MOD_NOREPEAT: u32 = 0x4000;

/// `WM_HOTKEY`, as its own number — see [`MOD_ALT`] for why this module writes
/// Win32's numbers down rather than reading them out of the constant its test
/// would also read.
const WM_HOTKEY: u32 = 0x0312;

/// **The translation, and the only part of this module a test can hold without a
/// keyboard**: a chord in, the two integers `RegisterHotKey` takes out.
///
/// `None` for a chord with no key at all. A virtual key of zero is what
/// [`crate::virtual_key_for_character`] answers with when the installed layout
/// cannot produce the character the chord names, and registering it would claim
/// whatever key Windows decides `0` means rather than the one nobody can press.
///
/// **A chord with no modifier is allowed.** `F9` alone is a summon key people
/// genuinely choose, and this crate is not where the product's opinion about
/// bare function keys lives — `bt_app::shortcuts::chord_verdict` is, and it says
/// so to the person recording the chord rather than silently here.
#[must_use]
pub fn registration_bits(hotkey: Hotkey) -> Option<(u32, u32)> {
    if hotkey.virtual_key == 0 {
        return None;
    }
    let mut modifiers = MOD_NOREPEAT;
    if hotkey.ctrl {
        modifiers |= MOD_CONTROL;
    }
    if hotkey.alt {
        modifiers |= MOD_ALT;
    }
    if hotkey.shift {
        modifiers |= MOD_SHIFT;
    }
    if hotkey.win {
        modifiers |= MOD_WIN;
    }
    Some((modifiers, u32::from(hotkey.virtual_key)))
}

/// Why a chord could not be claimed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HotkeyFault {
    /// **Somebody else has this key** — `ERROR_HOTKEY_ALREADY_REGISTERED`.
    ///
    /// The one refusal that is neither the reader's mistake nor this program's:
    /// another running program registered the chord first, and there is nothing
    /// to fix except to choose a different one. It is also what a *second* copy
    /// of this program is told, which is how "the summon belongs to the instance
    /// that started first" comes to be true — enforced by Windows, rather than by
    /// a lock of ours.
    AlreadyRegistered,
    /// The chord names a key this machine has no code for.
    NoSuchKey,
    /// Anything else Windows said, carried verbatim.
    Refused(String),
}

impl HotkeyFault {
    /// Whether this is the refusal a *second* instance of this program gets, and
    /// therefore the one that is expected rather than reported.
    #[must_use]
    pub const fn is_already_registered(&self) -> bool {
        matches!(self, Self::AlreadyRegistered)
    }
}

/// **Whether this message is the hotkey we asked for.**
///
/// Pure, and its own function for `is_system_preference_message`'s reason: it is
/// the part of a message hook that can be wrong, and a hook is not a place a
/// test can reach.
///
/// Three facts and not one. `WM_HOTKEY` is a **thread** message — Windows posts
/// it with no window at all when the registration named none — so a message
/// carrying an `hwnd` is one of winit's own windows talking and must be passed
/// through untouched. The `wparam` is the id handed to `RegisterHotKey`, and
/// this process may one day hold more than one.
#[must_use]
pub fn is_our_hotkey(message: u32, hwnd: isize, wparam: usize, id: i32) -> bool {
    message == WM_HOTKEY && hwnd == 0 && wparam == id as usize
}

#[cfg(windows)]
pub use windows_hotkey::{
    GlobalHotkey, foreground_window, give_foreground_to, register, summon_message_hook,
};

#[cfg(windows)]
mod windows_hotkey {
    use std::ffi::c_void;
    use std::marker::PhantomData;
    use std::num::NonZeroIsize;

    use windows::Win32::Foundation::{ERROR_HOTKEY_ALREADY_REGISTERED, HWND};
    // `AttachThreadInput` is filed under `Threading` and not under
    // `KeyboardAndMouse` beside the three below it, which reads oddly until you
    // remember what it does: it joins two *threads'* input queues, and the
    // keyboard is only the thing that arrives on them.
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, RegisterHotKey, UnregisterHotKey,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, MSG, SetForegroundWindow,
    };

    use super::{Hotkey, HotkeyFault, registration_bits};

    /// **A claim on a chord, held for as long as this value is alive.**
    ///
    /// The registration is a fact about a *thread*, not about a process or a
    /// window: `RegisterHotKey` with no window posts `WM_HOTKEY` to the queue of
    /// whichever thread called it, and `UnregisterHotKey` may only be called by
    /// that same thread. So this is deliberately neither `Send` nor `Sync` — a
    /// claim that travelled to another thread could not be released by the
    /// thread holding it, and the chord would stay taken for the life of the
    /// process.
    #[derive(Debug)]
    pub struct GlobalHotkey {
        id: i32,
        /// What makes the type thread-bound; it holds no pointer.
        _thread_bound: PhantomData<*const ()>,
    }

    impl GlobalHotkey {
        /// The id this claim was made under — the `wparam` its `WM_HOTKEY`
        /// carries, and what [`super::is_our_hotkey`] is asked about.
        #[must_use]
        pub const fn id(&self) -> i32 {
            self.id
        }
    }

    impl Drop for GlobalHotkey {
        fn drop(&mut self) {
            // SAFETY: `GlobalHotkey` is not `Send`, so this runs on the thread
            // that registered — `UnregisterHotKey`'s own requirement — and the id
            // was accepted by `RegisterHotKey` on that thread.
            //
            // Best-effort: a claim Windows has already dropped (a session change,
            // an unregister from a debugger) answers an error, and there is
            // nothing left to do about it while a value is being destroyed.
            let _ = unsafe { UnregisterHotKey(None, self.id) };
        }
    }

    /// **Claim a chord for this thread**, or say why it could not be claimed.
    ///
    /// `None` for the window, which is what makes the message a thread message
    /// and what makes this survivable at all: the quake window is hidden for most
    /// of its life and *destroyed* when the reader closes it, and a registration
    /// hung off its `HWND` would die with it — leaving the key that summons the
    /// window in the hands of the window it was supposed to summon.
    pub fn register(id: i32, hotkey: Hotkey) -> Result<GlobalHotkey, HotkeyFault> {
        let Some((modifiers, virtual_key)) = registration_bits(hotkey) else {
            return Err(HotkeyFault::NoSuchKey);
        };
        // SAFETY: the two integers come from `registration_bits`, which refuses a
        // zero key; no window handle is passed, so nothing here can outlive an
        // `HWND`. The claim is released by `GlobalHotkey::drop` on this thread.
        match unsafe { RegisterHotKey(None, id, HOT_KEY_MODIFIERS(modifiers), virtual_key) } {
            Ok(()) => Ok(GlobalHotkey {
                id,
                _thread_bound: PhantomData,
            }),
            Err(error) if error.code() == ERROR_HOTKEY_ALREADY_REGISTERED.to_hresult() => {
                Err(HotkeyFault::AlreadyRegistered)
            }
            Err(error) => Err(HotkeyFault::Refused(format!("RegisterHotKey: {error}"))),
        }
    }

    /// **The message hook winit's `with_msg_hook` takes**, already knowing how to
    /// recognise our hotkey and how little to do about it.
    ///
    /// The whole closure and not a predicate the caller writes the `unsafe`
    /// around, because `bt-app` is under the workspace's `unsafe_code = "deny"`
    /// and reading a raw `MSG` is precisely the sort of thing this crate exists
    /// to do on its behalf — the layout of a `MSG` is a Win32 fact, and Win32
    /// facts live on this side of the boundary.
    ///
    /// The pointer's validity is winit's contract: it documents the callback as
    /// receiving a live `*const MSG` for the length of the call, and this keeps
    /// nothing beyond it. The null check is not that contract being doubted; it
    /// is the one failure mode a `*const` can have that costs a comparison to
    /// rule out.
    ///
    /// **`wake` must do nothing but wake the loop.** This runs inside winit's own
    /// `PeekMessageW` dispatch, before anything has been decided about the turn;
    /// it is `SystemSettingsWatch`'s discipline at a second door and for a
    /// stronger version of its reason.
    ///
    /// **Always `false`**, which is winit's word for "dispatch this normally".
    /// Two different reasons agree on it: a message that is not ours is winit's
    /// to dispatch, and a `WM_HOTKEY` that *is* ours carries no window, so there
    /// is no window procedure for a dispatch to reach and letting it through
    /// costs nothing.
    pub fn summon_message_hook(
        id: i32,
        wake: impl Fn() + 'static,
    ) -> impl FnMut(*const c_void) -> bool {
        move |message: *const c_void| {
            if message.is_null() {
                return false;
            }
            // SAFETY: winit documents this pointer as a live `*const MSG` for the
            // duration of the call. The three fields are read by value and the
            // reference does not outlive the statement.
            let message = unsafe { &*message.cast::<MSG>() };
            if super::is_our_hotkey(
                message.message,
                message.hwnd.0 as isize,
                message.wParam.0,
                id,
            ) {
                wake();
            }
            false
        }
    }

    /// Whoever has the keyboard right now, or `None` when no window does.
    ///
    /// Asked **before** the summoned window is shown and kept until it is
    /// dismissed: it is the whole of what "give it back" means, and there is no
    /// second chance to read it — by the time the quake window is going away, the
    /// foreground is the quake window.
    #[must_use]
    pub fn foreground_window() -> Option<NonZeroIsize> {
        // SAFETY: a read with no arguments and no lifetime; the handle is
        // immediately narrowed to an integer and never dereferenced.
        let hwnd = unsafe { GetForegroundWindow() };
        NonZeroIsize::new(hwnd.0 as isize)
    }

    /// **Put this window back in front**, and say whether it actually got there.
    ///
    /// Windows refuses a bare `SetForegroundWindow` from a process that does not
    /// already own the foreground, and it refuses by answering `false` rather
    /// than by raising — so a caller that did not check would believe it had
    /// handed the keyboard back while the reader was still typing into a window
    /// that is no longer on the screen. Joining the foreground thread's input
    /// queue for the length of the call is the documented way round the lock, and
    /// the result is **read back** rather than assumed.
    ///
    /// The retry is the shape `scripts/release/smoke.ps1` and
    /// `scripts/dev/ui-probe.ps1` have used against real windows since August:
    /// the transition is not instantaneous, and a single attempt loses to a
    /// window still finishing an animation of its own.
    ///
    /// **Failure is silent to the reader and reported to the caller.** There is
    /// nothing a person can do about a foreground lock, and a card appearing over
    /// their editor to say the terminal could not give the keyboard back would be
    /// a worse interruption than the one it was reporting.
    pub fn give_foreground_to(hwnd: NonZeroIsize) -> bool {
        let target = HWND(hwnd.get() as *mut c_void);
        for _ in 0..FOREGROUND_ATTEMPTS {
            // SAFETY: a read with no arguments; the handle is only compared.
            if unsafe { GetForegroundWindow() }.0 as isize == hwnd.get() {
                return true;
            }
            // SAFETY: `GetForegroundWindow` may answer null, which
            // `GetWindowThreadProcessId` accepts and reports as thread 0 — the
            // "nobody has it" case the condition below declines to attach to. The
            // process-id out-parameter is deliberately `None`, which the API
            // documents as "do not report it".
            let theirs = unsafe { GetWindowThreadProcessId(GetForegroundWindow(), None) };
            // SAFETY: no arguments, no handle.
            let mine = unsafe { GetCurrentThreadId() };
            let attached = theirs != 0
                && theirs != mine
                // SAFETY: attaching two live thread input queues; detached below
                // on every path out of this iteration.
                && unsafe { AttachThreadInput(mine, theirs, true) }.as_bool();
            // SAFETY: `target` is the caller's live top-level window.
            let _ = unsafe { BringWindowToTop(target) };
            // SAFETY: same handle; the boolean answer is deliberately ignored in
            // favour of reading the foreground back below, which is the only
            // report that cannot be wrong.
            let _ = unsafe { SetForegroundWindow(target) };
            if attached {
                // SAFETY: undoing exactly the attachment made above, with the
                // same two thread ids.
                let _ = unsafe { AttachThreadInput(mine, theirs, false) };
            }
            // SAFETY: a read with no arguments.
            if unsafe { GetForegroundWindow() }.0 as isize == hwnd.get() {
                return true;
            }
        }
        false
    }

    /// How many times the handover is attempted before it is given up on.
    ///
    /// Five, which is `smoke.ps1`'s number, and no sleep between them: the
    /// scripts wait 400ms because they are photographing a window and a
    /// half-finished transition would be in the picture. This runs **on the event
    /// loop's own thread**, where four hundred milliseconds of sleep is four
    /// hundred milliseconds in which this program answers no keystroke, no
    /// present and no shell — a cure considerably worse than a foreground that
    /// went somewhere else.
    const FOREGROUND_ATTEMPTS: usize = 5;
}

/// The handover, on a host with no foreground to hand.
#[cfg(not(windows))]
#[must_use]
pub fn foreground_window() -> Option<std::num::NonZeroIsize> {
    None
}

/// The handover, on a host with no foreground to hand.
#[cfg(not(windows))]
#[must_use]
pub fn give_foreground_to(_hwnd: std::num::NonZeroIsize) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{Hotkey, is_our_hotkey, registration_bits};

    const fn chord(ctrl: bool, alt: bool, shift: bool, win: bool, virtual_key: u16) -> Hotkey {
        Hotkey {
            ctrl,
            alt,
            shift,
            win,
            virtual_key,
        }
    }

    /// RED — **the four flags reach the four bits, and nothing else does.**
    ///
    /// MUTATION: swap the `MOD_ALT` and `MOD_CONTROL` literals and a chord bound
    /// with Ctrl registers as one held with Alt; the window then answers a chord
    /// nobody bound and never answers the one they did. Both halves of that are
    /// invisible from inside this process, which is why the numbers are asserted
    /// rather than a round trip.
    #[test]
    fn every_modifier_reaches_its_own_bit() {
        let bare = registration_bits(chord(false, false, false, false, 0x70))
            .expect("a bare function key is a hotkey");
        assert_eq!(bare.1, 0x70, "the virtual key is carried unchanged");
        let only_repeat = bare.0;
        for (held, bit) in [
            (chord(true, false, false, false, 0x70), 0x0002),
            (chord(false, true, false, false, 0x70), 0x0001),
            (chord(false, false, true, false, 0x70), 0x0004),
            (chord(false, false, false, true, 0x70), 0x0008),
        ] {
            let (modifiers, _) = registration_bits(held).expect("a modified key is a hotkey");
            assert_eq!(
                modifiers,
                only_repeat | bit,
                "exactly one bit is added for {held:?}"
            );
        }
    }

    /// RED — **a held key is one press.**
    ///
    /// MUTATION: drop `MOD_NOREPEAT` from the mask and a summon held for half a
    /// second toggles the window at the keyboard's repeat rate, landing on
    /// whichever parity the finger lifted on.
    #[test]
    fn a_hotkey_never_repeats_while_it_is_held() {
        let (modifiers, _) = registration_bits(chord(true, false, true, false, 0xc0))
            .expect("a modified punctuation key is a hotkey");
        assert_eq!(
            modifiers & 0x4000,
            0x4000,
            "MOD_NOREPEAT is on every registration this crate makes"
        );
    }

    /// RED — **a key this layout cannot produce is not registered at all.**
    ///
    /// MUTATION: pass the zero through and `RegisterHotKey` claims whatever
    /// virtual key 0 means — a chord the reader cannot press and cannot get rid
    /// of.
    #[test]
    fn a_chord_with_no_key_is_refused_before_windows_sees_it() {
        assert_eq!(registration_bits(chord(true, false, true, false, 0)), None);
    }

    /// RED — **the hook claims a thread message with our id, and nothing else.**
    ///
    /// MUTATION: drop the `hwnd == 0` clause and the hook starts eating
    /// `WM_HOTKEY` messages addressed to windows, which in this process means any
    /// a library registers against its own `HWND`. Drop the id clause and a
    /// second registration's key summons the first one's window.
    #[test]
    fn only_a_thread_wm_hotkey_carrying_our_id_is_ours() {
        assert!(is_our_hotkey(0x0312, 0, 1, 1));
        assert!(
            !is_our_hotkey(0x0312, 0x1234, 1, 1),
            "a WM_HOTKEY with a window is somebody else's"
        );
        assert!(
            !is_our_hotkey(0x0312, 0, 2, 1),
            "another id is another claim"
        );
        assert!(
            !is_our_hotkey(0x0100, 0, 1, 1),
            "WM_KEYDOWN is not WM_HOTKEY"
        );
    }
}

/// The numbers written down above, held against the ones Windows publishes.
#[cfg(all(test, windows))]
mod win32_constant_tests {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    };
    use windows::Win32::UI::WindowsAndMessaging::WM_HOTKEY;

    /// RED — this module's own copies of six Win32 constants are the Win32 ones.
    ///
    /// MUTATION: change any one of the six literals in this file and this fails
    /// naming it. It is the price of writing them down, and writing them down is
    /// what lets the translation above be tested on a host with no `windows`
    /// crate at all.
    #[test]
    fn the_numbers_written_down_are_the_numbers_windows_publishes() {
        assert_eq!(super::MOD_ALT, MOD_ALT.0);
        assert_eq!(super::MOD_CONTROL, MOD_CONTROL.0);
        assert_eq!(super::MOD_SHIFT, MOD_SHIFT.0);
        assert_eq!(super::MOD_WIN, MOD_WIN.0);
        assert_eq!(super::MOD_NOREPEAT, MOD_NOREPEAT.0);
        assert_eq!(super::WM_HOTKEY, WM_HOTKEY);
    }
}
