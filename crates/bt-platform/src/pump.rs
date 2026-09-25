//! **Each message the window thread's pump dispatches, timed** (ticket 64).
//!
//! # Why this exists
//!
//! `bt_app::hang_watch` charges what the window thread does between one of
//! Folio's handlers and the next to the station `message pump` (ticket 43).
//! On next93 two holds read `message pump 516 ms` and `205 ms` while every
//! named child was small: the thread was inside Windows' message dispatch, in
//! something the self-report could not name — the input method's own
//! windows, the compositor, a WebView2 callback, a hook another program
//! installed. This module says which message it was.
//!
//! # The two roads a message reaches a window procedure by
//!
//! * **Posted** — taken off the queue by winit's `PeekMessageW` loop and
//!   dispatched there. winit's one door onto that loop is `with_msg_hook`,
//!   which runs before winit would translate and dispatch the message and lets
//!   the hook say it has handled it. [`timed_dispatch`] is that hook: it makes
//!   winit's own two calls, `TranslateMessage` then `DispatchMessageW`, with a
//!   timestamp either side, and answers "handled" so winit does not make them
//!   a second time.
//! * **Sent** — `SendMessage` from this thread, or from another thread and
//!   delivered inside this thread's `PeekMessageW`. This is how the input
//!   method talks to a window (`WM_IME_*`), and how the compositor and the
//!   shell tell it things. No queue is involved, so no pump hook sees them;
//!   the thread's own `WH_CALLWNDPROC` and `WH_CALLWNDPROCRET` hooks, which
//!   Windows calls either side of every sent message's window procedure, do.
//!
//! Both roads call the same pair, [`Timing::began`] and [`Timing::ended`],
//! and the pairs nest: a message sent while a posted one is being dispatched
//! begins and ends inside it. What the pair does with the clock is
//! `hang_watch`'s business; this module only says when.
//!
//! # macOS and the rest
//!
//! The pump there is winit's run loop over `NSApplication`, which offers no
//! per-event hook this program could time around, so [`time_messages`] does
//! nothing there and the self-report prints nothing extra.
//!
//! # What a message is called
//!
//! [`message_label`]: the names in [`MESSAGES`], which are the messages winit
//! handles and the input-method, compositor and pointer ones Folio's windows
//! see; the two private ranges as offsets from `WM_USER` and `WM_APP`; and a
//! registered message (`RegisterWindowMessage`, which is how winit names its
//! own wake-up and how other components name theirs) by the name it was
//! registered under, read back from the atom table it lives in. Anything else
//! prints as hex.

/// **The pair every dispatched message is bracketed by.**
///
/// Function pointers rather than closures: the Win32 hook procedures that call
/// them are `extern "system"` functions with nowhere to keep a closure, and
/// the callee is one static heartbeat.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// A message with this id is about to reach its window procedure.
    pub began: fn(u32),
    /// The most recent message that began has returned from it.
    pub ended: fn(),
}

/// The first id of the range a window class defines for itself.
const WM_USER: u32 = 0x0400;
/// The first id of the range an application defines for itself.
const WM_APP: u32 = 0x8000;
/// The first id `RegisterWindowMessage` hands out; the range ends at `0xFFFF`.
const FIRST_REGISTERED: u32 = 0xC000;
/// The last id `RegisterWindowMessage` hands out.
const LAST_REGISTERED: u32 = 0xFFFF;

/// **The names of the messages this program's windows are known to see.**
///
/// The messages winit's window procedure handles, the input method's
/// (`WM_IME_*`), the compositor's (`WM_DWM*`), the pointer and touch family,
/// and the ones Windows sends around every one of those (activation, sizing,
/// hit testing, painting). Values are `WinUser.h`'s; the Windows test
/// `every_name_is_the_value_windows_gives_it` holds a sample of them against
/// the `windows` crate's constants.
pub const MESSAGES: &[(u32, &str)] = &[
    (0x0000, "WM_NULL"),
    (0x0001, "WM_CREATE"),
    (0x0002, "WM_DESTROY"),
    (0x0003, "WM_MOVE"),
    (0x0005, "WM_SIZE"),
    (0x0006, "WM_ACTIVATE"),
    (0x0007, "WM_SETFOCUS"),
    (0x0008, "WM_KILLFOCUS"),
    (0x000A, "WM_ENABLE"),
    (0x000B, "WM_SETREDRAW"),
    (0x000C, "WM_SETTEXT"),
    (0x000D, "WM_GETTEXT"),
    (0x000E, "WM_GETTEXTLENGTH"),
    (0x000F, "WM_PAINT"),
    (0x0010, "WM_CLOSE"),
    (0x0011, "WM_QUERYENDSESSION"),
    (0x0012, "WM_QUIT"),
    (0x0014, "WM_ERASEBKGND"),
    (0x0015, "WM_SYSCOLORCHANGE"),
    (0x0016, "WM_ENDSESSION"),
    (0x0018, "WM_SHOWWINDOW"),
    (0x001A, "WM_SETTINGCHANGE"),
    (0x001C, "WM_ACTIVATEAPP"),
    (0x001D, "WM_FONTCHANGE"),
    (0x001E, "WM_TIMECHANGE"),
    (0x001F, "WM_CANCELMODE"),
    (0x0020, "WM_SETCURSOR"),
    (0x0021, "WM_MOUSEACTIVATE"),
    (0x0022, "WM_CHILDACTIVATE"),
    (0x0024, "WM_GETMINMAXINFO"),
    (0x003D, "WM_GETOBJECT"),
    (0x0046, "WM_WINDOWPOSCHANGING"),
    (0x0047, "WM_WINDOWPOSCHANGED"),
    (0x004A, "WM_COPYDATA"),
    (0x004E, "WM_NOTIFY"),
    (0x0050, "WM_INPUTLANGCHANGEREQUEST"),
    (0x0051, "WM_INPUTLANGCHANGE"),
    (0x007B, "WM_CONTEXTMENU"),
    (0x007C, "WM_STYLECHANGING"),
    (0x007D, "WM_STYLECHANGED"),
    (0x007E, "WM_DISPLAYCHANGE"),
    (0x007F, "WM_GETICON"),
    (0x0080, "WM_SETICON"),
    (0x0081, "WM_NCCREATE"),
    (0x0082, "WM_NCDESTROY"),
    (0x0083, "WM_NCCALCSIZE"),
    (0x0084, "WM_NCHITTEST"),
    (0x0085, "WM_NCPAINT"),
    (0x0086, "WM_NCACTIVATE"),
    (0x0087, "WM_GETDLGCODE"),
    (0x0088, "WM_SYNCPAINT"),
    (0x00A0, "WM_NCMOUSEMOVE"),
    (0x00A1, "WM_NCLBUTTONDOWN"),
    (0x00A2, "WM_NCLBUTTONUP"),
    (0x00A3, "WM_NCLBUTTONDBLCLK"),
    (0x00A4, "WM_NCRBUTTONDOWN"),
    (0x00A5, "WM_NCRBUTTONUP"),
    (0x00FE, "WM_INPUT_DEVICE_CHANGE"),
    (0x00FF, "WM_INPUT"),
    (0x0100, "WM_KEYDOWN"),
    (0x0101, "WM_KEYUP"),
    (0x0102, "WM_CHAR"),
    (0x0103, "WM_DEADCHAR"),
    (0x0104, "WM_SYSKEYDOWN"),
    (0x0105, "WM_SYSKEYUP"),
    (0x0106, "WM_SYSCHAR"),
    (0x0107, "WM_SYSDEADCHAR"),
    (0x0109, "WM_UNICHAR"),
    (0x010D, "WM_IME_STARTCOMPOSITION"),
    (0x010E, "WM_IME_ENDCOMPOSITION"),
    (0x010F, "WM_IME_COMPOSITION"),
    (0x0111, "WM_COMMAND"),
    (0x0112, "WM_SYSCOMMAND"),
    (0x0113, "WM_TIMER"),
    (0x0116, "WM_INITMENU"),
    (0x0117, "WM_INITMENUPOPUP"),
    (0x0119, "WM_GESTURE"),
    (0x011A, "WM_GESTURENOTIFY"),
    (0x011F, "WM_MENUSELECT"),
    (0x0120, "WM_MENUCHAR"),
    (0x0121, "WM_ENTERIDLE"),
    (0x0125, "WM_UNINITMENUPOPUP"),
    (0x0127, "WM_CHANGEUISTATE"),
    (0x0128, "WM_UPDATEUISTATE"),
    (0x0129, "WM_QUERYUISTATE"),
    (0x0200, "WM_MOUSEMOVE"),
    (0x0201, "WM_LBUTTONDOWN"),
    (0x0202, "WM_LBUTTONUP"),
    (0x0203, "WM_LBUTTONDBLCLK"),
    (0x0204, "WM_RBUTTONDOWN"),
    (0x0205, "WM_RBUTTONUP"),
    (0x0206, "WM_RBUTTONDBLCLK"),
    (0x0207, "WM_MBUTTONDOWN"),
    (0x0208, "WM_MBUTTONUP"),
    (0x0209, "WM_MBUTTONDBLCLK"),
    (0x020A, "WM_MOUSEWHEEL"),
    (0x020B, "WM_XBUTTONDOWN"),
    (0x020C, "WM_XBUTTONUP"),
    (0x020D, "WM_XBUTTONDBLCLK"),
    (0x020E, "WM_MOUSEHWHEEL"),
    (0x0210, "WM_PARENTNOTIFY"),
    (0x0211, "WM_ENTERMENULOOP"),
    (0x0212, "WM_EXITMENULOOP"),
    (0x0214, "WM_SIZING"),
    (0x0215, "WM_CAPTURECHANGED"),
    (0x0216, "WM_MOVING"),
    (0x0218, "WM_POWERBROADCAST"),
    (0x0219, "WM_DEVICECHANGE"),
    (0x0231, "WM_ENTERSIZEMOVE"),
    (0x0232, "WM_EXITSIZEMOVE"),
    (0x0233, "WM_DROPFILES"),
    (0x0238, "WM_POINTERDEVICECHANGE"),
    (0x0239, "WM_POINTERDEVICEINRANGE"),
    (0x023A, "WM_POINTERDEVICEOUTOFRANGE"),
    (0x0240, "WM_TOUCH"),
    (0x0241, "WM_NCPOINTERUPDATE"),
    (0x0242, "WM_NCPOINTERDOWN"),
    (0x0243, "WM_NCPOINTERUP"),
    (0x0245, "WM_POINTERUPDATE"),
    (0x0246, "WM_POINTERDOWN"),
    (0x0247, "WM_POINTERUP"),
    (0x0249, "WM_POINTERENTER"),
    (0x024A, "WM_POINTERLEAVE"),
    (0x024B, "WM_POINTERACTIVATE"),
    (0x024C, "WM_POINTERCAPTURECHANGED"),
    (0x024D, "WM_TOUCHHITTESTING"),
    (0x024E, "WM_POINTERWHEEL"),
    (0x024F, "WM_POINTERHWHEEL"),
    (0x0251, "WM_POINTERROUTEDTO"),
    (0x0252, "WM_POINTERROUTEDAWAY"),
    (0x0253, "WM_POINTERROUTEDRELEASED"),
    (0x0281, "WM_IME_SETCONTEXT"),
    (0x0282, "WM_IME_NOTIFY"),
    (0x0283, "WM_IME_CONTROL"),
    (0x0284, "WM_IME_COMPOSITIONFULL"),
    (0x0285, "WM_IME_SELECT"),
    (0x0286, "WM_IME_CHAR"),
    (0x0288, "WM_IME_REQUEST"),
    (0x0290, "WM_IME_KEYDOWN"),
    (0x0291, "WM_IME_KEYUP"),
    (0x02A0, "WM_NCMOUSEHOVER"),
    (0x02A1, "WM_MOUSEHOVER"),
    (0x02A2, "WM_NCMOUSELEAVE"),
    (0x02A3, "WM_MOUSELEAVE"),
    (0x02B1, "WM_WTSSESSION_CHANGE"),
    (0x02E0, "WM_DPICHANGED"),
    (0x02E2, "WM_DPICHANGED_BEFOREPARENT"),
    (0x02E3, "WM_DPICHANGED_AFTERPARENT"),
    (0x02E4, "WM_GETDPISCALEDSIZE"),
    (0x0300, "WM_CUT"),
    (0x0301, "WM_COPY"),
    (0x0302, "WM_PASTE"),
    (0x0303, "WM_CLEAR"),
    (0x0304, "WM_UNDO"),
    (0x0307, "WM_DESTROYCLIPBOARD"),
    (0x0312, "WM_HOTKEY"),
    (0x0317, "WM_PRINT"),
    (0x0318, "WM_PRINTCLIENT"),
    (0x0319, "WM_APPCOMMAND"),
    (0x031A, "WM_THEMECHANGED"),
    (0x031D, "WM_CLIPBOARDUPDATE"),
    (0x031E, "WM_DWMCOMPOSITIONCHANGED"),
    (0x031F, "WM_DWMNCRENDERINGCHANGED"),
    (0x0320, "WM_DWMCOLORIZATIONCOLORCHANGED"),
    (0x0321, "WM_DWMWINDOWMAXIMIZEDCHANGE"),
    (0x0323, "WM_DWMSENDICONICTHUMBNAIL"),
    (0x0326, "WM_DWMSENDICONICLIVEPREVIEWBITMAP"),
    (0x033F, "WM_GETTITLEBARINFOEX"),
];

/// The name [`MESSAGES`] gives `id`, if it gives one.
#[must_use]
pub fn message_name(id: u32) -> Option<&'static str> {
    MESSAGES
        .iter()
        .find(|(value, _)| *value == id)
        .map(|(_, name)| *name)
}

/// **What the self-report calls message `id`.** See the module header.
#[must_use]
pub fn message_label(id: u32) -> String {
    if let Some(name) = message_name(id) {
        return name.to_owned();
    }
    match id {
        WM_USER..WM_APP => format!("WM_USER+0x{:X}", id - WM_USER),
        WM_APP..FIRST_REGISTERED => format!("WM_APP+0x{:X}", id - WM_APP),
        FIRST_REGISTERED..=LAST_REGISTERED => {
            registered_name(id).unwrap_or_else(|| format!("0x{id:04X}"))
        }
        _ => format!("0x{id:04X}"),
    }
}

/// The name a registered message was registered under.
///
/// `RegisterWindowMessage` and `RegisterClipboardFormat` share one atom table,
/// and `GetClipboardFormatName` is the documented reader of it: a lookup of a
/// name by its atom, which opens no clipboard and reads no clipboard content.
#[cfg(windows)]
fn registered_name(id: u32) -> Option<String> {
    use windows::Win32::System::DataExchange::GetClipboardFormatNameW;
    let mut name = [0u16; 128];
    // SAFETY: the buffer is a live, writable slice for the length of the call;
    // the answer is the count of UTF-16 units written into it, without the nul.
    let written = unsafe { GetClipboardFormatNameW(id, &mut name) };
    let written = usize::try_from(written).ok().filter(|count| *count > 0)?;
    Some(String::from_utf16_lossy(&name[..written]))
}

/// There is no registered-message table off Windows.
#[cfg(not(windows))]
fn registered_name(_id: u32) -> Option<String> {
    None
}

/// **Start timing every message the calling thread's window procedures are
/// sent**, and give [`timed_dispatch`] the pair to time posted ones with.
///
/// Called once, on the window thread, before the event loop is built: the two
/// hooks are this thread's (`SetWindowsHookEx` with this thread's id and no
/// module), so they see this thread's windows and nobody else's, and they end
/// with the thread.
///
/// # Errors
///
/// The text of `SetWindowsHookEx`'s refusal when Windows would not install a
/// hook.
#[cfg(windows)]
pub fn time_messages(timing: Timing) -> Result<(), String> {
    windows_pump::install(timing)
}

/// Nothing to install: winit's run loop here offers no per-message hook. See
/// the module header.
///
/// # Errors
///
/// Never; the signature is the Windows arm's.
#[cfg(not(windows))]
pub fn time_messages(_timing: Timing) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
pub use windows_pump::timed_dispatch;

#[cfg(windows)]
mod windows_pump {
    use std::ffi::c_void;
    use std::sync::OnceLock;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CWPSTRUCT, CallNextHookEx, DispatchMessageW, MSG, SetWindowsHookExW, TranslateMessage,
        WH_CALLWNDPROC, WH_CALLWNDPROCRET,
    };

    use super::Timing;

    /// The pair, once [`install`] has run. Read by both hook procedures and by
    /// [`timed_dispatch`], on the window thread.
    static TIMING: OnceLock<Timing> = OnceLock::new();

    pub(super) fn install(timing: Timing) -> Result<(), String> {
        let _ = TIMING.set(timing);
        // SAFETY: a read with no arguments.
        let thread = unsafe { GetCurrentThreadId() };
        // SAFETY: both procedures are `extern "system"` functions with the
        // `HOOKPROC` signature that live for the whole program; no module is
        // passed because the hook is this thread's and the procedures are in
        // this image.
        unsafe {
            SetWindowsHookExW(WH_CALLWNDPROC, Some(before_sent), None, thread)
                .map_err(|error| format!("SetWindowsHookEx(WH_CALLWNDPROC): {error}"))?;
            SetWindowsHookExW(WH_CALLWNDPROCRET, Some(after_sent), None, thread)
                .map_err(|error| format!("SetWindowsHookEx(WH_CALLWNDPROCRET): {error}"))?;
        }
        Ok(())
    }

    /// `WH_CALLWNDPROC`: a sent message is about to reach its procedure.
    ///
    /// A negative code is Windows' word for "pass this on and do nothing
    /// else", which is the documented contract of every hook procedure.
    unsafe extern "system" fn before_sent(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0
            && let Some(timing) = TIMING.get()
        {
            // SAFETY: for this hook `lparam` is a live `*const CWPSTRUCT` for
            // the length of the call; one field is read by value.
            let message = unsafe { &*(lparam.0 as *const CWPSTRUCT) }.message;
            (timing.began)(message);
        }
        // SAFETY: the arguments are the ones this procedure was called with.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// `WH_CALLWNDPROCRET`: the procedure a sent message reached has returned.
    unsafe extern "system" fn after_sent(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0
            && let Some(timing) = TIMING.get()
        {
            (timing.ended)();
        }
        // SAFETY: the arguments are the ones this procedure was called with.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// **The hook winit's `with_msg_hook` takes**, dispatching every posted
    /// message itself between the pair [`install`] was given.
    ///
    /// `inner` is asked first and keeps its word: a message it says it has
    /// handled is not dispatched, exactly as winit would not have dispatched
    /// it. Everything else gets winit's own two calls, in winit's order —
    /// `TranslateMessage`, which is where a key becomes a character and where
    /// the input method sees it, then `DispatchMessageW` — and the answer
    /// "handled", so winit does not make them a second time. winit's checks
    /// after a dispatch (a panic to resume, an exit, an interrupted drain)
    /// follow the hook's return and are made as before.
    ///
    /// Before [`install`] has run there is no pair, and the message is
    /// dispatched untimed.
    pub fn timed_dispatch(
        mut inner: impl FnMut(*const c_void) -> bool,
    ) -> impl FnMut(*const c_void) -> bool {
        move |message: *const c_void| {
            if message.is_null() {
                return false;
            }
            if inner(message) {
                return true;
            }
            // SAFETY: winit documents the pointer as a live `*const MSG` for
            // the length of the call; it is only read.
            let message = unsafe { &*message.cast::<MSG>() };
            let timing = TIMING.get();
            if let Some(timing) = timing {
                (timing.began)(message.message);
            }
            // SAFETY: `message` is the `MSG` winit's `PeekMessageW` filled in,
            // passed to the two calls winit itself would have made with it.
            unsafe {
                let _ = TranslateMessage(message);
                DispatchMessageW(message);
            }
            if let Some(timing) = timing {
                (timing.ended)();
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MESSAGES, message_label, message_name};

    /// RED (64) — **the self-report has a word for the messages a pump stall
    /// is suspected of, and every word names one message.**
    ///
    /// The owner's next93 holds were taken while hovering a card and typing
    /// through the input method; the suspects are the input method's own
    /// messages, the compositor's, the pointer's and the ones every window
    /// is sent. A duplicate id would make the table's answer depend on its
    /// order.
    ///
    /// MUTATION: delete the `WM_IME_COMPOSITION` row and this goes red.
    #[test]
    fn the_self_report_has_a_word_for_the_messages_a_pump_stall_is_suspected_of() {
        for (index, (value, name)) in MESSAGES.iter().enumerate() {
            assert!(
                MESSAGES[index + 1..]
                    .iter()
                    .all(|(other, _)| other != value),
                "{name} shares its id with another row"
            );
            assert!(name.starts_with("WM_"), "{name}");
        }
        for (id, name) in [
            (0x010F, "WM_IME_COMPOSITION"),
            (0x0282, "WM_IME_NOTIFY"),
            (0x0281, "WM_IME_SETCONTEXT"),
            (0x0100, "WM_KEYDOWN"),
            (0x0200, "WM_MOUSEMOVE"),
            (0x02A3, "WM_MOUSELEAVE"),
            (0x0245, "WM_POINTERUPDATE"),
            (0x000F, "WM_PAINT"),
            (0x031F, "WM_DWMNCRENDERINGCHANGED"),
            (0x0113, "WM_TIMER"),
        ] {
            assert_eq!(message_name(id), Some(name));
            assert_eq!(message_label(id), name);
        }
    }

    /// **An id the table does not know prints as itself**: the two private
    /// ranges as offsets from where they start, and anything else as hex.
    #[test]
    fn an_id_the_table_does_not_know_prints_as_hex() {
        assert_eq!(message_label(0x0401), "WM_USER+0x1");
        assert_eq!(message_label(0x8003), "WM_APP+0x3");
        assert_eq!(message_label(0x0118), "0x0118");
        assert_eq!(message_label(0x1_0000), "0x10000");
    }

    /// **The table's numbers are Windows' numbers**, a row from each family
    /// held against the `windows` crate's constant of the same name.
    #[cfg(windows)]
    #[test]
    fn every_name_is_the_value_windows_gives_it() {
        use windows::Win32::UI::WindowsAndMessaging as w;
        let declared = [
            (w::WM_NULL, "WM_NULL"),
            (w::WM_SIZE, "WM_SIZE"),
            (w::WM_PAINT, "WM_PAINT"),
            (w::WM_SETTINGCHANGE, "WM_SETTINGCHANGE"),
            (w::WM_SETCURSOR, "WM_SETCURSOR"),
            (w::WM_GETOBJECT, "WM_GETOBJECT"),
            (w::WM_WINDOWPOSCHANGED, "WM_WINDOWPOSCHANGED"),
            (w::WM_NCHITTEST, "WM_NCHITTEST"),
            (w::WM_SYNCPAINT, "WM_SYNCPAINT"),
            (w::WM_INPUT, "WM_INPUT"),
            (w::WM_KEYDOWN, "WM_KEYDOWN"),
            (w::WM_UNICHAR, "WM_UNICHAR"),
            (w::WM_IME_STARTCOMPOSITION, "WM_IME_STARTCOMPOSITION"),
            (w::WM_IME_COMPOSITION, "WM_IME_COMPOSITION"),
            (w::WM_TIMER, "WM_TIMER"),
            (w::WM_GESTURE, "WM_GESTURE"),
            (w::WM_MOUSEMOVE, "WM_MOUSEMOVE"),
            (w::WM_MOUSEHWHEEL, "WM_MOUSEHWHEEL"),
            (w::WM_SIZING, "WM_SIZING"),
            (w::WM_ENTERSIZEMOVE, "WM_ENTERSIZEMOVE"),
            (w::WM_TOUCH, "WM_TOUCH"),
            (w::WM_POINTERUPDATE, "WM_POINTERUPDATE"),
            (w::WM_POINTERHWHEEL, "WM_POINTERHWHEEL"),
            (w::WM_POINTERROUTEDRELEASED, "WM_POINTERROUTEDRELEASED"),
            (w::WM_IME_SETCONTEXT, "WM_IME_SETCONTEXT"),
            (w::WM_IME_NOTIFY, "WM_IME_NOTIFY"),
            (w::WM_IME_REQUEST, "WM_IME_REQUEST"),
            (w::WM_IME_KEYUP, "WM_IME_KEYUP"),
            (w::WM_NCMOUSELEAVE, "WM_NCMOUSELEAVE"),
            (w::WM_WTSSESSION_CHANGE, "WM_WTSSESSION_CHANGE"),
            (w::WM_DPICHANGED, "WM_DPICHANGED"),
            (w::WM_GETDPISCALEDSIZE, "WM_GETDPISCALEDSIZE"),
            (w::WM_HOTKEY, "WM_HOTKEY"),
            (w::WM_CLIPBOARDUPDATE, "WM_CLIPBOARDUPDATE"),
            (w::WM_DWMCOMPOSITIONCHANGED, "WM_DWMCOMPOSITIONCHANGED"),
            (
                w::WM_DWMSENDICONICLIVEPREVIEWBITMAP,
                "WM_DWMSENDICONICLIVEPREVIEWBITMAP",
            ),
            (w::WM_GETTITLEBARINFOEX, "WM_GETTITLEBARINFOEX"),
        ];
        for (value, name) in declared {
            assert_eq!(message_name(value), Some(name), "{name} = 0x{value:04X}");
        }
    }

    /// **The real hook dispatches a real posted message between the pair**,
    /// and a message the inner hook has handled is neither dispatched nor
    /// timed.
    ///
    /// A thread message posted to this test's own thread and taken off its
    /// queue by `PeekMessageW`, exactly as winit takes one; no window is
    /// created. The same test installs the thread's two sent-message hooks,
    /// because the pair is the process's one.
    ///
    /// MUTATION: drop the `began` call from `timed_dispatch` and the id is
    /// never seen.
    #[cfg(windows)]
    #[test]
    fn a_posted_message_is_dispatched_between_the_pair() {
        use std::ffi::c_void;
        use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{
            MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW, PostThreadMessageW, WM_APP,
        };

        use super::{Timing, time_messages, timed_dispatch};

        static BEGAN: AtomicU32 = AtomicU32::new(0);
        static ENDED: AtomicUsize = AtomicUsize::new(0);
        fn began(id: u32) {
            BEGAN.store(id, Ordering::Relaxed);
        }
        fn ended() {
            ENDED.fetch_add(1, Ordering::Relaxed);
        }
        time_messages(Timing { began, ended }).expect("this thread's hooks");

        let post = |id: u32| -> MSG {
            let mut message = MSG::default();
            // SAFETY: plain Win32 calls on this thread's own queue; the first
            // peek creates the queue a thread message needs.
            unsafe {
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                PostThreadMessageW(GetCurrentThreadId(), id, WPARAM(0), LPARAM(0))
                    .expect("post to this thread");
                assert!(PeekMessageW(&mut message, None, id, id, PM_REMOVE).as_bool());
            }
            message
        };

        let mut hook = timed_dispatch(|_| false);
        let message = post(WM_APP + 7);
        assert!(hook(std::ptr::from_ref(&message).cast::<c_void>()));
        assert_eq!(BEGAN.load(Ordering::Relaxed), WM_APP + 7);
        assert_eq!(ENDED.load(Ordering::Relaxed), 1);

        let mut taken = timed_dispatch(|_| true);
        let message = post(WM_APP + 8);
        assert!(taken(std::ptr::from_ref(&message).cast::<c_void>()));
        assert_eq!(BEGAN.load(Ordering::Relaxed), WM_APP + 7);
        assert_eq!(ENDED.load(Ordering::Relaxed), 1);
    }

    /// **A registered message is called by the name it was registered under**
    /// — how winit's own wake-up reads in a line.
    #[cfg(windows)]
    #[test]
    fn a_registered_message_is_called_by_its_registered_name() {
        use windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;
        use windows::core::w;

        // SAFETY: a nul-terminated literal; the answer is an id or zero.
        let id = unsafe { RegisterWindowMessageW(w!("Folio.PumpNameProbe")) };
        assert!(id >= 0xC000, "registered ids start at 0xC000: {id:#X}");
        assert_eq!(message_label(id), "Folio.PumpNameProbe");
    }
}
