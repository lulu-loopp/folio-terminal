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
/// **And `None` for a chord no modifier holds down** ([`holds_a_summon_modifier`],
/// R2-14). This used to read "a chord with no modifier is allowed", on the
/// grounds that the product's opinion about bare keys belonged upstairs with the
/// recorder. It does — the *sentence* a person reads is
/// `bt_app::shortcuts::chord_verdict`'s — but the consequence of getting it wrong
/// is not a sentence: `RegisterHotKey` takes the key out of the input stream for
/// **the whole desktop**, so a bare `k` recorded once means no program on the
/// machine sees that letter again until Folio exits, and a persisted one means it
/// comes back at every launch. A refusal that lives only where a person can read
/// it is a refusal a hand-edited file walks past, so it is stated here as well,
/// at the one call that makes the claim.
#[must_use]
pub fn registration_bits(hotkey: Hotkey) -> Option<(u32, u32)> {
    if hotkey.virtual_key == 0 || !holds_a_summon_modifier(hotkey) {
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

/// **Whether a chord is one a desktop-wide claim may be made for** (R2-14).
///
/// At least one of Ctrl, Alt and the Windows key. **Shift alone is not enough**,
/// and that is the clause worth writing down: `Shift+A` is how a capital `A` is
/// typed, so claiming it would take a letter away from every program on the
/// machine exactly as surely as claiming the bare letter would — the difference
/// between them is a shape, not a consequence.
///
/// A function key alone is refused by the same rule, and deliberately: `F9` is a
/// key `less`, `gdb`, an IDE and a spreadsheet all answer to, and a terminal that
/// swallowed it desktop-wide would be taking it from all of them for one window
/// nobody is looking at.
#[must_use]
pub const fn holds_a_summon_modifier(hotkey: Hotkey) -> bool {
    hotkey.ctrl || hotkey.alt || hotkey.win
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
    /// **The chord holds none of Ctrl, Alt or the Windows key** — see
    /// [`holds_a_summon_modifier`], and R2-14 for what claiming one costs.
    NoModifier,
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

/// **Whether a message is one this process should act on** (R2-5).
///
/// [`is_our_hotkey`] answers the *shape* of a message, and a shape is all a
/// message has: `WM_HOTKEY` with a null window and a `wparam` of one is four
/// integers, and `PostThreadMessage` is a call any process running as this user
/// can make. So the shape is not the whole question. The other half is whether
/// **this** process is currently holding a claim under that id — because a
/// summon nobody registered is a summon Windows was never going to send, and a
/// message claiming otherwise is a message from somewhere else.
///
/// It closes the two states the shape alone could not see: the shortcut cleared
/// or turned off in the table, and a `RegisterHotKey` Windows refused because a
/// second copy of Folio, or another program entirely, got the chord first. In
/// both of those the window used to come down for a message it had no claim
/// behind.
#[must_use]
pub fn summon_should_act(
    message: u32,
    hwnd: isize,
    wparam: usize,
    id: i32,
    registration_is_live: bool,
) -> bool {
    registration_is_live && is_our_hotkey(message, hwnd, wparam, id)
}

/// **The ids this process holds a live `RegisterHotKey` claim under.**
///
/// A list rather than a flag because [`is_our_hotkey`]'s own note says this
/// process may one day hold more than one, and a `bool` would be the place that
/// stopped being true. It is written at exactly two moments — a `register` that
/// Windows accepted, and the `Drop` that releases it — so "is the claim live"
/// has one answer and it is the same one Windows has.
///
/// Kept out of the `#[cfg(windows)]` module on purpose: it is bookkeeping with
/// no Win32 in it, and it is the half of [`summon_should_act`] a test can drive.
static LIVE_CLAIMS: std::sync::Mutex<Vec<i32>> = std::sync::Mutex::new(Vec::new());

fn claims() -> std::sync::MutexGuard<'static, Vec<i32>> {
    LIVE_CLAIMS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether this process holds a claim under `id` right now.
#[must_use]
pub fn registration_is_live(id: i32) -> bool {
    claims().contains(&id)
}

/// Record a claim Windows accepted.
///
/// `#[cfg(windows)]` with the registration that calls it: off Windows nothing
/// claims a chord yet (M4-8), so the ledger [`registration_is_live`] reads is
/// only ever written on the platform that has one. The reader stays ungated,
/// because "is this chord ours" has an answer everywhere and that answer is no.
#[cfg(windows)]
fn note_claimed(id: i32) {
    let mut live = claims();
    if !live.contains(&id) {
        live.push(id);
    }
}

/// Record a claim that has been released, or that Windows refused.
#[cfg(windows)]
fn note_released(id: i32) {
    claims().retain(|held| *held != id);
}

#[cfg(windows)]
pub use windows_hotkey::{
    GlobalHotkey, allow_foreground_for, foreground_window, give_foreground_to, register,
    summon_message_hook,
};

#[cfg(windows)]
mod windows_hotkey {
    use std::ffi::c_void;
    use std::marker::PhantomData;
    use std::time::Instant;

    use windows::Win32::Foundation::ERROR_HOTKEY_ALREADY_REGISTERED;
    // `AttachThreadInput` is filed under `Threading` and not under
    // `KeyboardAndMouse` beside the three below it, which reads oddly until you
    // remember what it does: it joins two *threads'* input queues, and the
    // keyboard is only the thing that arrives on them.
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, RegisterHotKey, UnregisterHotKey,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
        IsHungAppWindow, IsWindow, MSG, SetForegroundWindow,
    };

    use super::{Hotkey, HotkeyFault, registration_bits};
    use crate::NativeWindow;

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
            // **And the ledger goes down with the claim** (R2-5). From here a
            // `WM_HOTKEY` carrying this id is a message nothing of ours asked
            // for, and `summon_should_act` says so.
            super::note_released(self.id);
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
        // **Refused before Windows is asked** (R2-14). The two are told apart
        // because their remedies are: a key this layout has no code for wants a
        // different key, and a chord with no modifier on it wants a modifier.
        if !super::holds_a_summon_modifier(hotkey) {
            return Err(HotkeyFault::NoModifier);
        }
        let Some((modifiers, virtual_key)) = registration_bits(hotkey) else {
            return Err(HotkeyFault::NoSuchKey);
        };
        // SAFETY: the two integers come from `registration_bits`, which refuses a
        // zero key; no window handle is passed, so nothing here can outlive an
        // `HWND`. The claim is released by `GlobalHotkey::drop` on this thread.
        match unsafe { RegisterHotKey(None, id, HOT_KEY_MODIFIERS(modifiers), virtual_key) } {
            Ok(()) => {
                // **The ledger goes up with the claim and not before** (R2-5):
                // it is what `summon_should_act` reads to tell a message Windows
                // sent from one another process posted.
                super::note_claimed(id);
                Ok(GlobalHotkey {
                    id,
                    _thread_bound: PhantomData,
                })
            }
            Err(error) if error.code() == ERROR_HOTKEY_ALREADY_REGISTERED.to_hresult() => {
                super::note_released(id);
                Err(HotkeyFault::AlreadyRegistered)
            }
            Err(error) => {
                super::note_released(id);
                Err(HotkeyFault::Refused(format!("RegisterHotKey: {error}")))
            }
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
            // **Shape and claim, not shape alone** (R2-5). See
            // [`super::summon_should_act`]: a `WM_HOTKEY` is four integers any
            // process of this user can post with `PostThreadMessage`, and the
            // only thing that separates one Windows sent from one somebody else
            // did is whether this process is holding a claim under that id.
            if super::summon_should_act(
                message.message,
                message.hwnd.0 as isize,
                message.wParam.0,
                id,
                super::registration_is_live(id),
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
    pub fn foreground_window() -> Option<NativeWindow> {
        // SAFETY: a read with no arguments and no lifetime; the handle is
        // immediately narrowed to an integer and never dereferenced.
        let hwnd = unsafe { GetForegroundWindow() };
        NativeWindow::from_hwnd(hwnd)
    }

    /// **Hand this process's foreground rights to another process** (`docs/DESIGN.md` §7.59).
    ///
    /// The other half of [`give_foreground_to`], seen from the side that *has*
    /// the keyboard: that function is a window trying to come to the front, and
    /// Windows refuses it unless the process that owns the foreground has said
    /// otherwise first. This is that sentence. A second `folio.exe` started from
    /// Explorer, a shortcut or a pinned icon holds foreground rights because the
    /// user just started it, and it spends them here — on the Folio that is
    /// already running — before it exits.
    ///
    /// **Not `ASFW_ANY`, and the refusal is in the code and not only in this
    /// sentence** (review C-7, 2026-09-11). `ASFW_ANY` is the same call with
    /// `(DWORD)-1` and grants the right to whatever asks next — this program
    /// lifting the foreground lock for the whole machine. The paragraph above
    /// used to be the only thing standing between that value and this call, and
    /// the pid came off a wire, out of a field a peer filled in. It now comes
    /// from the kernel ([`crate::launch_pipe::hand_over`]) **and** `u32::MAX` and
    /// `0` are refused here, because a rule stated at one door is a rule until
    /// somebody opens a second one.
    ///
    /// The answer is read back and handed to the caller for [`give_foreground_to`]'s
    /// reason: it fails by answering `false` rather than by raising, and a caller
    /// that did not look would report a handover that never happened. Failure is
    /// never reported to a reader — there is nothing a person can do about a
    /// foreground lock, and the worst it costs is a window that opens behind
    /// another one.
    pub fn allow_foreground_for(process: u32) -> bool {
        // `u32::MAX` is `ASFW_ANY` and `0` is `ASFW_NONE` — the two values that
        // are not a process, and the two this call must never be asked with.
        if process == 0 || process == u32::MAX {
            return false;
        }
        // SAFETY: a call taking one integer; it names a process id and
        // dereferences nothing. A process id that has gone is a legal argument
        // and answers `false`.
        unsafe { AllowSetForegroundWindow(process) }.is_ok()
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
    pub fn give_foreground_to(window: NativeWindow) -> bool {
        let target = window.as_hwnd();
        // **The handle is revalidated before it is used** (R2-3). It was read at
        // the moment the summon came down, and between then and now the window it
        // named may have closed — an `HWND` is reused by Windows the moment a
        // window is destroyed, so a stale one does not fail, it names somebody
        // else's window.
        //
        // SAFETY: a read of a handle that is only ever compared and passed back
        // to Win32; `IsWindow` is defined on a handle that is no longer one.
        if !unsafe { IsWindow(Some(target)) }.as_bool() {
            return false;
        }
        let began = Instant::now();
        for attempt in 0..FOREGROUND_ATTEMPTS {
            if !super::another_round(attempt, began.elapsed()) {
                return false;
            }
            // SAFETY: a read with no arguments; the handle is only compared.
            if unsafe { GetForegroundWindow() } == target {
                return true;
            }
            // SAFETY: `GetForegroundWindow` may answer null, which
            // `GetWindowThreadProcessId` accepts and reports as thread 0 — the
            // "nobody has it" case the rule below declines to attach to. The
            // process-id out-parameter is deliberately `None`, which the API
            // documents as "do not report it".
            let foreground = unsafe { GetForegroundWindow() };
            let theirs = unsafe { GetWindowThreadProcessId(foreground, None) };
            // SAFETY: no arguments, no handle.
            let mine = unsafe { GetCurrentThreadId() };
            // **The one question that has to be asked before the queues are
            // joined** (R2-3). `AttachThreadInput` makes two threads share one
            // input queue, and a queue is only as responsive as the slower of the
            // two: attaching to an application that has stopped reading its
            // messages hands Folio that application's paralysis for as long as
            // the attachment lasts. `IsHungAppWindow` is Windows' own answer to
            // "has this window stopped answering", the same one the shell reads
            // before it draws the ghost frame.
            //
            // SAFETY: a read of a handle Win32 just gave us; null is a legal
            // argument and answers false.
            let hung = !foreground.is_invalid() && unsafe { IsHungAppWindow(foreground) }.as_bool();
            let step = super::handover_step(theirs, mine, hung);
            let attached = step == super::HandoverStep::AttachAndActivate
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
            if unsafe { GetForegroundWindow() } == target {
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

/// **The second bound on the handover, and it is in time** (R2-3).
///
/// A count of attempts bounds the loop only if every attempt is quick, and the
/// calls inside one are not guaranteed to be: `SetForegroundWindow` and
/// `BringWindowToTop` both talk to whichever window station and desktop thread is
/// on the other end. Five attempts that each take a second is five seconds in
/// which this program answers no keystroke, no present and no shell — the exact
/// cost the "no sleep between them" note was protecting against, arriving by a
/// different door.
///
/// Pure, so that "the loop cannot run past its budget" is a claim a test makes
/// with numbers rather than a claim about a machine with a wedged window on it.
#[must_use]
pub fn another_round(attempt: usize, elapsed: std::time::Duration) -> bool {
    attempt == 0 || elapsed < FOREGROUND_BUDGET
}

/// The whole of what the handover may spend on somebody else's window.
///
/// A quarter of the frame budget of a 4 Hz redraw, and far more than the
/// transition takes on a machine that is answering at all: on this one the
/// handover completes on the first or second attempt inside a millisecond. What
/// it bounds is the machine that is not answering.
pub const FOREGROUND_BUDGET: std::time::Duration = std::time::Duration::from_millis(250);

/// **What one round of the handover may do**, from the facts about the two
/// threads — see [`handover_step`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandoverStep {
    /// Ask for the foreground without joining anybody's input queue.
    ///
    /// Either there is nobody to join — no foreground window, or it is already
    /// this thread's — or the foreground belongs to an application that has
    /// stopped answering, and joining *that* is how one program's hang becomes
    /// two. The bare `SetForegroundWindow` will very likely be refused, which is
    /// a summon that did not take the keyboard; the alternative was a window that
    /// stopped drawing.
    ActivateAlone,
    /// Join the foreground thread's queue for the length of the call, then ask.
    ///
    /// The documented way past the foreground lock, and it is safe precisely when
    /// the other thread is reading its own messages.
    AttachAndActivate,
}

/// The rule [`give_foreground_to`] applies once per round, with the Win32 taken
/// out of it (R2-3).
#[must_use]
pub const fn handover_step(
    foreground_thread: u32,
    our_thread: u32,
    foreground_is_hung: bool,
) -> HandoverStep {
    if foreground_thread == 0 || foreground_thread == our_thread || foreground_is_hung {
        HandoverStep::ActivateAlone
    } else {
        HandoverStep::AttachAndActivate
    }
}

/// **The global summon key, on a platform whose event tap is M4-8's** (gated
/// behind X-5, because Accessibility is granted against a code signature and an
/// agent that re-signs on every build would be granting it again every time).
///
/// `CGEventTap` is the mechanism the owner ruled for (§8 Q2), authorized
/// through an in-app *Enable global shortcut* action rather than at first
/// summon — a chord that cannot be heard has no first summon to ask at. So the
/// fault this arm answers with is the one M4-8 turns into that row's
/// *not authorized* state, and it is `Refused` with a sentence rather than a
/// new variant, because the variant M4-8 adds is about a permission that has
/// been asked for and declined, which is a different thing from a mechanism
/// that has not been written.
#[cfg(not(windows))]
#[derive(Debug)]
pub struct GlobalHotkey {
    /// Never constructed: [`register`] refuses.
    _never: std::convert::Infallible,
}

#[cfg(not(windows))]
impl GlobalHotkey {
    /// The id this claim was made under. Unreachable: there is no claim.
    #[must_use]
    pub const fn id(&self) -> i32 {
        match self._never {}
    }
}

/// Claim the chord. Refused; M4-8.
#[cfg(not(windows))]
pub fn register(id: i32, hotkey: Hotkey) -> Result<GlobalHotkey, HotkeyFault> {
    let _ = id;
    // **The product's own refusal first, exactly as the Windows arm orders
    // them** (R2-14): a chord with no modifier on it is refused for a reason
    // that is true on every platform, and telling the reader "not on this
    // platform" about a chord that would be refused anyway sends them to fix
    // the wrong thing.
    if !holds_a_summon_modifier(hotkey) {
        return Err(HotkeyFault::NoModifier);
    }
    Err(HotkeyFault::Refused(
        "the global summon key is not on this platform yet".to_owned(),
    ))
}

/// **Let the process we are handing a launch to come to the front.**
///
/// A no-op answering `false`, and one of §4.4's class-N items rather than
/// deferred work: `AllowSetForegroundWindow` exists because Windows has a
/// foreground *lock* to ask permission from, and macOS has none — the launch
/// handover simply activates the other application. The `false` says no
/// permission was granted, which is true, and the caller's own next step is the
/// activation that needs none.
#[cfg(not(windows))]
#[must_use]
pub fn allow_foreground_for(process: u32) -> bool {
    let _ = process;
    false
}

/// The handover, on a host with no foreground to hand.
///
/// **Not the same statement as "there is no frontmost application"** — macOS
/// has one, `NSWorkspace.frontmostApplication`, and M4-8 gives this arm a real
/// answer when the quake terminal's foreground rules are ported. What this arm
/// says is that nobody has asked yet, and the caller's own reading of `None`
/// (`bt_app::quake`: remember nothing, give nothing back) is the honest
/// behaviour until then.
#[cfg(not(windows))]
#[must_use]
pub fn foreground_window() -> Option<crate::NativeWindow> {
    None
}

/// The handover, on a host with no foreground to hand.
///
/// The `bool` is read by `bt-app`, which prints one line when the window it
/// summoned could not take the keyboard — so the refusal is visible in
/// `diagnostics.log` rather than silent. M4-8 owns the real arm.
#[cfg(not(windows))]
#[must_use]
pub fn give_foreground_to(_window: crate::NativeWindow) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{
        HandoverStep, Hotkey, another_round, handover_step, holds_a_summon_modifier, is_our_hotkey,
        registration_bits,
    };
    // **The ledger's two writers are Windows'**, because nothing claims a chord
    // anywhere else yet (M4-8), and so is `summon_should_act`, which reads a
    // `WM_HOTKEY` — the inventory classifies that predicate as a compile-time
    // absence for exactly that reason. `is_our_hotkey` stays above with the
    // rest: it is a pure predicate over four integers and its test is a claim
    // about arithmetic, which is true on every platform.
    #[cfg(windows)]
    use super::{note_claimed, note_released, registration_is_live, summon_should_act};

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
        let one = registration_bits(chord(true, false, false, false, 0x70))
            .expect("a modified function key is a hotkey");
        assert_eq!(one.1, 0x70, "the virtual key is carried unchanged");
        // The no-repeat bit and nothing else, written out rather than read back
        // off a bare chord: since R2-14 a bare chord is not a hotkey at all, so
        // the baseline has to be stated instead of measured.
        let only_repeat = 0x4000;
        // Shift is asked about beside Ctrl rather than alone, and by the rule
        // R2-14 wrote: shift alone is not a chord this crate will claim, so the
        // only way to see its bit is to add it to one that is.
        for (held, expected) in [
            (chord(true, false, false, false, 0x70), only_repeat | 0x0002),
            (chord(false, true, false, false, 0x70), only_repeat | 0x0001),
            (
                chord(true, false, true, false, 0x70),
                only_repeat | 0x0002 | 0x0004,
            ),
            (chord(false, false, false, true, 0x70), only_repeat | 0x0008),
        ] {
            let (modifiers, _) = registration_bits(held).expect("a modified key is a hotkey");
            assert_eq!(
                modifiers, expected,
                "exactly the bits this chord names, and no others, for {held:?}"
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
    /// RED (R2-14) — **a chord no modifier holds down is never claimed
    /// desktop-wide.**
    ///
    /// `RegisterHotKey` takes a key out of the input stream for every program on
    /// the machine. A bare letter recorded here means nothing on this desktop
    /// sees that letter again until Folio exits, and a persisted one brings the
    /// state back at every launch. Shift alone does not answer: `Shift+A` is how
    /// a capital `A` is typed.
    ///
    /// MUTATION: let the bare chord through and the first four assertions pass a
    /// claim on `k`, `Shift+k` and `F9` to Windows.
    #[test]
    fn a_summon_with_no_modifier_is_never_claimed() {
        // `k`, `Shift+k`, `F9` and `Shift+F9`: a letter, a capital, a function
        // key and a shifted function key. None of them is a summon.
        for bare in [
            chord(false, false, false, false, 0x4b),
            chord(false, false, true, false, 0x4b),
            chord(false, false, false, false, 0x78),
            chord(false, false, true, false, 0x78),
        ] {
            assert!(
                !holds_a_summon_modifier(bare),
                "shift alone is not a modifier a desktop-wide claim may be made on: {bare:?}"
            );
            assert_eq!(
                registration_bits(bare),
                None,
                "a chord with no modifier must not reach RegisterHotKey: {bare:?}"
            );
        }
        for held in [
            chord(true, false, false, false, 0x4b),
            chord(false, true, false, false, 0x4b),
            chord(false, false, false, true, 0xc0),
            chord(true, false, true, false, 0x78),
        ] {
            assert!(holds_a_summon_modifier(held), "{held:?}");
            assert!(registration_bits(held).is_some(), "{held:?}");
        }
    }

    /// RED (R2-5) — **a hotkey message is acted on only while this process holds
    /// the claim.**
    ///
    /// `WM_HOTKEY` with a null window and a `wparam` of one is four integers, and
    /// `PostThreadMessage` is a call any process of this user can make. The shape
    /// alone was enough to bring the window down over whatever the reader was
    /// doing — including when the shortcut had been cleared, and when Windows had
    /// refused the registration to a second copy of Folio.
    ///
    /// MUTATION: drop the liveness clause and the two `!` assertions below fail,
    /// which is a window that answers a key nobody registered.
    #[cfg(windows)]
    #[test]
    fn a_summon_is_acted_on_only_while_this_process_holds_the_claim() {
        // An id of this test's own: the ledger is process-wide, and the product's
        // own id belongs to whatever else is running beside this.
        const ID: i32 = 0x5eed;
        note_released(ID);
        assert!(!registration_is_live(ID));
        assert!(
            !summon_should_act(0x0312, 0, ID as usize, ID, registration_is_live(ID)),
            "a forged message with nothing registered must be dropped"
        );
        note_claimed(ID);
        assert!(registration_is_live(ID));
        assert!(
            summon_should_act(0x0312, 0, ID as usize, ID, registration_is_live(ID)),
            "the key Windows sent while we hold the claim is ours"
        );
        assert!(
            !summon_should_act(0x0312, 0x1234, ID as usize, ID, true),
            "a WM_HOTKEY with a window is still somebody else's"
        );
        note_released(ID);
        assert!(
            !summon_should_act(0x0312, 0, ID as usize, ID, registration_is_live(ID)),
            "a claim that has been given up is a key this process no longer answers"
        );
    }

    /// RED (R2-3) — **the handover never joins a hung application's input
    /// queue, and never runs past its budget.**
    ///
    /// `AttachThreadInput` makes two threads share one input queue, so attaching
    /// to a window that has stopped reading its messages hands Folio that
    /// application's paralysis — five rounds of it, on the event loop's own
    /// thread, with nothing in the loop that could time it out.
    ///
    /// MUTATION: drop the `foreground_is_hung` clause and the first assertion
    /// asks to be attached to a wedged program; drop the elapsed clause in
    /// `another_round` and the loop runs its five rounds however long each takes.
    #[test]
    fn the_handover_declines_a_hung_foreground_and_stops_at_its_budget() {
        use std::time::Duration;
        assert_eq!(
            handover_step(4242, 99, true),
            HandoverStep::ActivateAlone,
            "a foreground that has stopped answering is not a queue to join"
        );
        assert_eq!(
            handover_step(4242, 99, false),
            HandoverStep::AttachAndActivate,
            "an application that is answering is the documented way past the lock"
        );
        assert_eq!(
            handover_step(0, 99, false),
            HandoverStep::ActivateAlone,
            "nobody has the foreground, so there is nobody to join"
        );
        assert_eq!(
            handover_step(99, 99, false),
            HandoverStep::ActivateAlone,
            "a thread does not attach to itself"
        );
        assert!(
            another_round(0, Duration::from_secs(30)),
            "the first attempt is always made — the budget bounds the retries"
        );
        assert!(another_round(1, Duration::from_millis(1)));
        assert!(
            !another_round(1, super::FOREGROUND_BUDGET),
            "a round that would start past the budget is not started"
        );
        assert!(!another_round(4, Duration::from_secs(30)));
    }

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
