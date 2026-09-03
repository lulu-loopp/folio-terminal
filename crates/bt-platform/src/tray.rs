//! **The icon in the notification area** — the summoned terminal's other door
//! (`docs/DESIGN.md` §7.54).
//!
//! A sixth unsafe boundary in this crate, against a sixth thing. `windows_impl`
//! is Win32 for the sake of a window this process owns, [`crate::webview`] is
//! WebView2, [`crate::hang`] is Win32 turned on this process,
//! [`crate::attention_pipe`] is a channel other processes speak into, and
//! [`crate::hotkey`] is the keyboard while somebody else has it. This is
//! **the shell's own strip of the taskbar** — a surface that belongs to
//! `explorer.exe`, that this program is a guest on, and that goes away and comes
//! back without asking.
//!
//! # Why there is one at all
//!
//! §7.54 ended with an open account: a reader who wants a summoned terminal that
//! stays will find it goes away with the last window, and what that needs is an
//! icon rather than a window nobody can see. This is that account settled. The
//! two halves of it are one subject and are written in one place: the icon is the
//! mouse's way to the same verb the chord has, and it is **the thing that makes
//! staying legible**. A program with no window and no icon is a program that has
//! silently not quit; a program with no window and an icon is a program that is
//! where its icon says it is.
//!
//! # What is pure and what is not
//!
//! [`tray_action_for`] is the whole of the decoding — which of the shell's
//! callbacks means what — and it is pure so a test can hold it on any host, the
//! rule [`crate::hotkey::is_our_hotkey`] is written under and for its reason: it
//! is the part that can be wrong without a taskbar. Everything below it needs a
//! window and a message queue and is gated on Windows.

/// **What a callback from the notification area means to this program.**
///
/// Two, out of the dozen the shell can send. Everything else — a hover, a
/// balloon closing, the pointer leaving — is a fact about the icon that this
/// program has no verb for, and a decoder that answered for them would be
/// inventing gestures nobody made.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayAction {
    /// The primary button was released on the icon: show the summoned terminal,
    /// or send it away if it is already showing. The same verb the chord has,
    /// which is the whole point of the icon.
    Toggle,
    /// The secondary button was released on the icon: the menu is wanted.
    Menu,
}

/// **One line of the icon's menu.**
///
/// The four verbs a person can want from a program that has no window on the
/// screen. `Summon` is the icon's own left click written down, because a menu
/// that did not name it would be a menu that hid the thing the icon is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayCommand {
    Summon,
    NewWindow,
    Settings,
    Quit,
}

impl TrayCommand {
    /// The number this line is identified by inside `TrackPopupMenu`.
    ///
    /// One-based, because `TrackPopupMenu` with `TPM_RETURNCMD` answers `0` for
    /// "nothing was chosen" and a command numbered zero could not be told from a
    /// menu the reader dismissed.
    const fn id(self) -> u32 {
        match self {
            Self::Summon => 1,
            Self::NewWindow => 2,
            Self::Settings => 3,
            Self::Quit => 4,
        }
    }

    /// The inverse, for reading `TrackPopupMenu`'s answer back.
    #[must_use]
    pub const fn from_id(id: u32) -> Option<Self> {
        match id {
            1 => Some(Self::Summon),
            2 => Some(Self::NewWindow),
            3 => Some(Self::Settings),
            4 => Some(Self::Quit),
            _ => None,
        }
    }

    /// The four, in the order the menu draws them.
    pub const ALL: [Self; 4] = [Self::Summon, Self::NewWindow, Self::Settings, Self::Quit];
}

/// **The words the menu is drawn with**, handed in rather than written here.
///
/// This crate has no opinion about language and no access to the table that
/// does; `bt_app::i18n` owns both. They are owned `String`s and replaceable
/// because the window's language is a thing a reader can change while the
/// program is running, and a menu that kept the words it was installed with
/// would be the one surface that did not follow.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrayLabels {
    pub summon: String,
    pub new_window: String,
    pub settings: String,
    pub quit: String,
    /// The line the shell shows when the pointer rests on the icon. Cut to what
    /// `NOTIFYICONDATAW` can hold — see [`TIP_CAPACITY`].
    pub tip: String,
}

impl TrayLabels {
    /// The label for one line.
    #[must_use]
    pub fn of(&self, command: TrayCommand) -> &str {
        match command {
            TrayCommand::Summon => &self.summon,
            TrayCommand::NewWindow => &self.new_window,
            TrayCommand::Settings => &self.settings,
            TrayCommand::Quit => &self.quit,
        }
    }
}

/// **How many UTF-16 units of tip the shell will hold**, including the
/// terminator — `NOTIFYICONDATAW::szTip` is 128 units wide.
///
/// Written down rather than read off the array so the cut can be tested without
/// Win32, and pinned against the real array by
/// `tray_tip_capacity_is_the_one_the_shell_publishes`.
pub const TIP_CAPACITY: usize = 128;

/// **Which callback this is**, or `None` when it is one this program has no verb
/// for.
///
/// Pure, and its own function for [`crate::hotkey::is_our_hotkey`]'s reason: it
/// is the part of a subclass that can be wrong, and a subclass is not a place a
/// test can reach.
///
/// **The classic callback shape**, which is what this program asks the shell for:
/// the message is the private one handed to `NOTIFYICONDATAW::uCallbackMessage`,
/// `wparam` is the icon's own id, and `lparam` is the mouse message that happened
/// over it. The version-4 shape packs coordinates into `wparam` and the event
/// into the low half of `lparam` instead, and it is not asked for, so it is not
/// decoded: two protocols read by one function would be a function that could
/// not tell an id of `0x0205` from a right-button release.
///
/// **Both buttons are read on the release.** A press over a tray icon is not yet
/// a gesture — the pointer may still leave — and the shell's own menus all open
/// on the way up.
#[must_use]
pub fn tray_action_for(
    message: u32,
    callback_message: u32,
    wparam: usize,
    lparam: usize,
    id: u32,
) -> Option<TrayAction> {
    if message != callback_message || wparam != id as usize {
        return None;
    }
    match u32::try_from(lparam).ok()? {
        WM_LBUTTONUP => Some(TrayAction::Toggle),
        WM_RBUTTONUP => Some(TrayAction::Menu),
        _ => None,
    }
}

/// Win32's numbers, written as the numbers they are — [`crate::hotkey`]'s rule
/// and its reason: a number written twice and checked once is one decision, and
/// a test that read it out of the same constant the code did is no check at all.
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;

/// **Cut a tip to what the shell will hold**, on a character boundary.
///
/// Pure, and separate from the copy that uses it, because "too long" is a case
/// that arrives from a translated string rather than from a keyboard and is
/// therefore a case nobody would find by hand.
///
/// The count is UTF-16 units and not characters, because that is what the array
/// holds: a tip of sixty Chinese characters fits and a tip of sixty emoji does
/// not. One unit is left for the terminator.
#[must_use]
pub fn tray_tip(tip: &str) -> Vec<u16> {
    let mut units: Vec<u16> = tip.encode_utf16().take(TIP_CAPACITY - 1).collect();
    // A surrogate pair cut in half is not a character, and the shell would draw
    // the replacement glyph for it. Dropping the orphan is the only way to end on
    // something that can be read.
    if units
        .last()
        .is_some_and(|unit| (0xd800..0xdc00).contains(unit))
    {
        units.pop();
    }
    units.push(0);
    units
}

#[cfg(windows)]
pub use windows_tray::TrayIcon;

#[cfg(windows)]
mod windows_tray {
    use std::ffi::c_void;
    use std::num::NonZeroIsize;
    use std::rc::Rc;
    use std::sync::{Mutex, OnceLock};

    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFY_ICON_MESSAGE,
        NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect, Shell_NotifyIconW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
        GWLP_HINSTANCE, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW, HICON, IMAGE_ICON,
        LR_DEFAULTCOLOR, LoadImageW, MF_STRING, PostMessageW, RegisterClassW,
        RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW, TPM_RETURNCMD,
        TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE, WM_APP, WM_NULL, WNDCLASSW, WS_POPUP,
    };
    use windows::core::PCWSTR;

    use super::{TrayAction, TrayCommand, TrayLabels, tray_action_for, tray_tip};
    use crate::WindowRect;

    /// **The id this process files its one icon under.**
    ///
    /// A notification-area id is unique per *window*, and this program adds one
    /// icon against one window, so a constant is the whole namespace.
    const TRAY_ICON_ID: u32 = 1;
    /// The private message the shell is asked to send back. `WM_APP + 0x4ba`, the
    /// next free one after the three deferred gestures in `lib.rs`.
    const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 0x4ba;
    /// The private message this module posts to **itself** to open the menu after
    /// the initiating callback has returned — see [`tray_window_proc`].
    const TRAY_MENU_MESSAGE: u32 = WM_APP + 0x4bb;
    /// **The application icon's resource group.** `crates/bt-app/build.rs` writes
    /// the executable's icon as group one and says why: Explorer draws an
    /// executable with its lowest-numbered group, and the shell integration
    /// registers `folio.exe,0` meaning exactly that one. The icon in the
    /// notification area is the icon on the taskbar is the icon in Explorer, which
    /// is what makes it findable.
    const APP_ICON_GROUP: u16 = 1;

    /// **The message the shell broadcasts when it has been restarted.**
    ///
    /// Registered by name rather than written down, because its number is
    /// allocated at run time and is the same number for every program that asks
    /// for it — that is the whole mechanism. Read once, because
    /// `RegisterWindowMessageW` answers the same value for the life of the
    /// session.
    fn taskbar_created_message() -> u32 {
        static MESSAGE: OnceLock<u32> = OnceLock::new();
        *MESSAGE.get_or_init(|| {
            // SAFETY: a call with one static null-terminated wide string, which the
            // API copies; it returns an atom in the system message range or zero.
            unsafe { RegisterWindowMessageW(windows::core::w!("TaskbarCreated")) }
        })
    }

    /// Everything the window procedure and the owner share.
    struct TrayState {
        /// **What has happened and not yet been acted on.**
        ///
        /// The window procedure parks and the loop spends, which is
        /// `SystemSettingsWatch`'s discipline at a third door: a procedure reached
        /// through `DispatchMessageW` runs before anything has been decided about
        /// the turn, and the only safe thing to do there is write down that
        /// something happened.
        pending: Mutex<Vec<TrayCommand>>,
        /// The words the menu draws with, replaceable while the program runs.
        labels: Mutex<TrayLabels>,
        /// What to call after parking something, and all it may do.
        wake: Box<dyn Fn()>,
    }

    impl TrayState {
        fn park(&self, command: TrayCommand) {
            self.pending
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(command);
            (self.wake)();
        }

        fn labels(&self) -> TrayLabels {
            self.labels
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }
    }

    /// **An icon in the notification area, alive for as long as this value is.**
    ///
    /// # Why it has a window of its own
    ///
    /// A notification icon is filed against an `HWND`, and the shell sends its
    /// callbacks there. The obvious host is one of the program's own windows, and
    /// it is the wrong one: the whole reason this icon exists is that a reader may
    /// close every window and still expect the program to be reachable, and an
    /// icon hung off a window that has been destroyed is an icon whose clicks go
    /// nowhere. So this owns a window that nobody ever sees and that nothing else
    /// can close.
    ///
    /// It is a **top-level** window and deliberately not a message-only one, even
    /// though it never draws. A message-only window cannot be brought to the
    /// foreground, and `SetForegroundWindow` before `TrackPopupMenu` is what makes
    /// the icon's menu close when the reader clicks somewhere else. It wears
    /// `WS_EX_TOOLWINDOW` so that never being shown is all it is: no taskbar
    /// button, no place in the window switcher.
    ///
    /// Neither `Send` nor `Sync` by construction: it holds an `HWND`, and a window
    /// may only be destroyed by the thread that created it.
    pub struct TrayIcon {
        hwnd: HWND,
        /// Held because the window procedure reaches it through the window's user
        /// data and nothing may move it while the window is alive.
        ///
        /// `Rc` and not `Arc`, which is the type saying what is already true: a
        /// window may only be spoken to by the thread that created it, so every
        /// hand that ever touches this is that one thread's. The reference count
        /// is not there for other threads — it is there for the *same* thread
        /// re-entering through the nested pump `TrackPopupMenu` runs.
        state: Rc<TrayState>,
    }

    impl TrayIcon {
        /// **Put the icon in the notification area**, or say why it could not go
        /// there.
        ///
        /// `anchor` is one of the program's live windows and is used for exactly
        /// one thing: reading the module this program was loaded as, which
        /// registering a window class requires. Nothing about the icon outlives
        /// that read, which is the point — the anchor may be closed a moment
        /// later.
        pub fn install(
            anchor: NonZeroIsize,
            labels: TrayLabels,
            wake: Box<dyn Fn()>,
        ) -> Result<Self, String> {
            let anchor = HWND(anchor.get() as *mut c_void);
            // SAFETY: a read of a live window's own instance handle; the value is
            // an opaque integer that is only handed back to Win32.
            let instance =
                HINSTANCE(unsafe { GetWindowLongPtrW(anchor, GWLP_HINSTANCE) } as *mut c_void);
            let class = tray_window_class(instance)?;
            let state = Rc::new(TrayState {
                pending: Mutex::new(Vec::new()),
                labels: Mutex::new(labels),
                wake,
            });
            // SAFETY: the class was registered immediately above against this same
            // instance. Every pointer argument is either a live null-terminated
            // wide string or `None`; the window is destroyed by `Drop`, on this
            // same thread.
            let hwnd = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(WS_EX_TOOLWINDOW),
                    class,
                    windows::core::w!("Folio"),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(instance),
                    None,
                )
            }
            .map_err(|error| format!("CreateWindowExW(tray) failed: {error}"))?;
            // The state is published **before** the icon is added, which is the
            // whole of the ordering: the first callback the shell can send arrives
            // after `NIM_ADD`, and a procedure that read its user data before this
            // line would read a null.
            //
            // SAFETY: the window was created on this thread and is alive; the
            // pointer is kept live by the `Rc` this value holds for the whole of
            // the window's life, and is cleared before the window is destroyed.
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::as_ptr(&state) as isize);
            }
            let icon = Self { hwnd, state };
            icon.notify(NIM_ADD)?;
            Ok(icon)
        }

        /// Change the words without taking the icon down.
        pub fn set_labels(&self, labels: TrayLabels) -> Result<(), String> {
            *self
                .state
                .labels
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = labels;
            self.notify(NIM_MODIFY)
        }

        /// **What the reader has asked for since this was last asked**, in the
        /// order they asked for it.
        #[must_use]
        pub fn take_commands(&self) -> Vec<TrayCommand> {
            std::mem::take(
                &mut *self
                    .state
                    .pending
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()),
            )
        }

        /// **Where the icon is on the screen**, for a probe that has to click it.
        ///
        /// `None` when the shell will not say, which it will not while the
        /// notification area is collapsed and this icon is inside the overflow — a
        /// real state, and one a caller can only report rather than fix.
        #[must_use]
        pub fn icon_rect(&self) -> Option<WindowRect> {
            let identifier = NOTIFYICONIDENTIFIER {
                cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
                hWnd: self.hwnd,
                uID: TRAY_ICON_ID,
                ..Default::default()
            };
            // SAFETY: `identifier` is a fully initialised local with its `cbSize`
            // set as the API requires; the call reads it and writes a `RECT`.
            let rect = unsafe { Shell_NotifyIconGetRect(&identifier) }.ok()?;
            Some(WindowRect {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            })
        }

        fn notify(&self, message: NOTIFY_ICON_MESSAGE) -> Result<(), String> {
            notify_icon(self.hwnd, &self.state.labels(), message)
        }
    }

    impl Drop for TrayIcon {
        fn drop(&mut self) {
            // **Three statements in one order.** The icon goes first, because a
            // window destroyed underneath a live icon leaves the shell drawing one
            // whose clicks reach nothing. The user data goes second, so that any
            // message still in the queue for this window finds a null rather than a
            // pointer to state that is about to be freed. The window goes last.
            //
            // SAFETY: dropped on the thread that created the window. The first two
            // are best-effort because there is nothing left to do about a failure
            // while a value is being destroyed.
            unsafe {
                let _ = notify_icon(self.hwnd, &self.state.labels(), NIM_DELETE);
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }

    /// **`WS_EX_TOOLWINDOW`**, written as the number it is — this module's rule,
    /// and `lib.rs`'s: a number written twice and checked once is one decision.
    pub(super) const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;

    /// **Register the class this program's tray window is made from**, once.
    ///
    /// The name is returned rather than the atom, because `CreateWindowExW` takes
    /// either and a name is the one of the two a person reading a debugger will
    /// recognise. A second call is not an error and not a second registration:
    /// `RegisterClassW` refuses a duplicate, and the `OnceLock` means it is never
    /// asked twice in the first place.
    fn tray_window_class(instance: HINSTANCE) -> Result<PCWSTR, String> {
        static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
        let name = windows::core::w!("FolioTrayWindow");
        REGISTERED
            .get_or_init(|| {
                let class = WNDCLASSW {
                    lpfnWndProc: Some(tray_window_proc),
                    hInstance: instance,
                    lpszClassName: name,
                    ..Default::default()
                };
                // SAFETY: every field is either zero or a live value that outlives
                // the call — the procedure is a `'static` function and the name is
                // a static wide string. The class is deliberately never
                // unregistered: it is unregistered for us when the process exits,
                // and a class unregistered while a window of it lives is a crash.
                if unsafe { RegisterClassW(&class) } == 0 {
                    return Err("RegisterClassW(tray) failed".to_owned());
                }
                Ok(())
            })
            .clone()
            .map(|()| name)
    }

    /// The one `Shell_NotifyIconW` call, said the same way for every message that
    /// takes a whole description — and reachable from the window procedure, which
    /// has the state but not the owner (see the shell-restart arm).
    fn notify_icon(
        hwnd: HWND,
        labels: &TrayLabels,
        message: NOTIFY_ICON_MESSAGE,
    ) -> Result<(), String> {
        let tip = tray_tip(&labels.tip);
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK_MESSAGE,
            hIcon: app_icon(hwnd),
            ..Default::default()
        };
        data.szTip[..tip.len()].copy_from_slice(&tip);
        // SAFETY: `data` is fully initialised, its `cbSize` names this build's own
        // structure, and the call copies out of it and does not retain it.
        let ok = unsafe { Shell_NotifyIconW(message, &data) };
        if ok.as_bool() {
            Ok(())
        } else {
            Err("Shell_NotifyIconW failed".to_owned())
        }
    }

    /// **The program's own icon.**
    ///
    /// `LoadImageW` with a zero size asks for the image at its stored size and lets
    /// the shell scale what it is given. The instance is read off the window rather
    /// than from `GetModuleHandleW`, which keeps this module inside the Win32
    /// features this crate already asks for.
    ///
    /// A null `HICON` is what `NOTIFYICONDATAW` is given when the resource cannot
    /// be found, and the shell draws its own placeholder for it. That is a worse
    /// icon and not a missing feature, so it is not an error: an icon nobody
    /// recognises is still an icon that answers the mouse.
    fn app_icon(hwnd: HWND) -> HICON {
        // SAFETY: a read of a live window's own instance handle; the value is an
        // opaque integer that is only handed back to Win32.
        let instance = HINSTANCE(unsafe { GetWindowLongPtrW(hwnd, GWLP_HINSTANCE) } as *mut c_void);
        // SAFETY: the name is an integer resource id in the low word, which is how
        // `MAKEINTRESOURCE` is spelled. The returned handle is shared and owned by
        // the module, so it is deliberately not destroyed.
        let loaded = unsafe {
            LoadImageW(
                Some(instance),
                PCWSTR(APP_ICON_GROUP as usize as *const u16),
                IMAGE_ICON,
                0,
                0,
                LR_DEFAULTCOLOR,
            )
        };
        loaded.map_or_else(|_| HICON(std::ptr::null_mut()), |handle| HICON(handle.0))
    }

    /// **The window procedure, which parks and wakes and does not act.**
    ///
    /// Three messages mean something and they are answered very differently.
    ///
    /// A **callback from the shell** is decoded by [`tray_action_for`] and nothing
    /// more: a left click parks the verb and wakes the loop, and a right click
    /// *posts a message to this same window*. It does not open the menu where it
    /// stands. `TrackPopupMenu` runs a nested message pump, and starting one inside
    /// a `DispatchMessageW` that winit is in the middle of is the hazard
    /// `MathContextMenu` was built to avoid — the posted message arrives after the
    /// initiating dispatch has returned, which is the earliest moment nothing is
    /// borrowed.
    ///
    /// The **posted message** is that later moment, and it is where the menu runs.
    ///
    /// **The shell restarting** takes the icon with it and says so by broadcasting
    /// (see [`taskbar_created_message`]). Adding the icon again is the whole
    /// answer, and it is not optional: without it, one `explorer.exe` crash leaves
    /// a program whose only door is a chord the reader may have bound weeks ago —
    /// which is the exact state this icon exists to prevent.
    unsafe extern "system" fn tray_window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: a read of this window's own user data, which `install` set to an
        // `Rc` pointer that outlives the window and clears before it is destroyed.
        let state_pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const TrayState;
        if state_pointer.is_null() {
            // SAFETY: the default handling of a message this window has no state to
            // answer with, which is the required contract for every other message.
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        }
        // SAFETY: the owning `TrayIcon` holds one `Rc` at entry. Incrementing
        // before constructing the temporary keeps the state alive even if a nested
        // teardown drops the owner while the menu is open — the hazard
        // `MathContextMenu` documents, and the same answer.
        unsafe { Rc::increment_strong_count(state_pointer) };
        // SAFETY: the increment immediately above created the reference consumed
        // here.
        let state = unsafe { Rc::from_raw(state_pointer) };
        if message == TRAY_MENU_MESSAGE {
            if let Some(command) = track_tray_menu(hwnd, &state) {
                state.park(command);
            }
            return LRESULT(0);
        }
        if message == taskbar_created_message() {
            let _ = notify_icon(hwnd, &state.labels(), NIM_ADD);
            return LRESULT(0);
        }
        match tray_action_for(
            message,
            TRAY_CALLBACK_MESSAGE,
            wparam.0,
            lparam.0 as usize,
            TRAY_ICON_ID,
        ) {
            Some(TrayAction::Toggle) => {
                state.park(TrayCommand::Summon);
                LRESULT(0)
            }
            Some(TrayAction::Menu) => {
                // SAFETY: `PostMessageW` copies these value parameters into this
                // thread's own queue and never dispatches synchronously.
                let _ =
                    unsafe { PostMessageW(Some(hwnd), TRAY_MENU_MESSAGE, WPARAM(0), LPARAM(0)) };
                LRESULT(0)
            }
            // SAFETY: everything this window does not answer is the system's to
            // answer, which for a window with no drawing of its own is all of it.
            None => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    /// **The menu itself**, run only from the posted message above.
    ///
    /// Two calls here are not decoration and are the documented price of putting a
    /// menu over a notification icon. `SetForegroundWindow` **before** the menu is
    /// what makes clicking somewhere else dismiss it — a menu whose owner is not in
    /// front stays on the screen after the reader has looked away, and it is also
    /// why the window this runs on is a real top-level one. The `WM_NULL` **after**
    /// it is the other half of the same note: without a message arriving, the menu
    /// can survive its own dismissal until something else happens to wake the
    /// queue.
    fn track_tray_menu(hwnd: HWND, state: &TrayState) -> Option<TrayCommand> {
        let labels = state.labels();
        // SAFETY: the window and the menu belong to this thread, which is the thread
        // this posted message was dispatched on. The menu created here is destroyed
        // here, on every path out.
        unsafe {
            let menu = CreatePopupMenu().ok()?;
            let mut cursor = POINT::default();
            let chosen = (|| {
                for command in TrayCommand::ALL {
                    let mut label: Vec<u16> = labels.of(command).encode_utf16().collect();
                    label.push(0);
                    AppendMenuW(
                        menu,
                        MF_STRING,
                        command.id() as usize,
                        PCWSTR(label.as_ptr()),
                    )
                    .ok()?;
                }
                GetCursorPos(&mut cursor).ok()?;
                let _ = SetForegroundWindow(hwnd);
                let command = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    cursor.x,
                    cursor.y,
                    None,
                    hwnd,
                    None,
                );
                let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                TrayCommand::from_id(command.0 as u32)
            })();
            let _ = DestroyMenu(menu);
            chosen
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TIP_CAPACITY, TrayAction, TrayCommand, tray_action_for, tray_tip};

    /// RED (§7.54) — **the two gestures the icon answers, and nothing else.**
    ///
    /// MUTATION: drop the `wparam` clause and a second icon this program might one
    /// day file under a different id summons the first one's window. Drop the
    /// message clause and every message this window receives is read as a tray
    /// callback, so a mouse move over the terminal opens the tray menu. Answer
    /// `WM_LBUTTONDOWN` as well as the release and one click both shows the window
    /// and hides it again, landing on whichever of the two the shell happened to
    /// deliver last.
    #[test]
    fn only_the_two_releases_over_our_own_icon_are_gestures() {
        const CALLBACK: u32 = 0x8000 + 0x4ba;
        let action = |lparam: usize| tray_action_for(CALLBACK, CALLBACK, 1, lparam, 1);
        assert_eq!(action(0x0202), Some(TrayAction::Toggle));
        assert_eq!(action(0x0205), Some(TrayAction::Menu));
        assert_eq!(action(0x0201), None, "a press is not yet a gesture");
        assert_eq!(action(0x0200), None, "and neither is a mouse move");
        assert_eq!(
            tray_action_for(0x0100, CALLBACK, 1, 0x0202, 1),
            None,
            "a WM_KEYDOWN is not a tray callback however its lparam reads"
        );
        assert_eq!(
            tray_action_for(CALLBACK, CALLBACK, 2, 0x0202, 1),
            None,
            "another icon's id is another icon"
        );
    }

    /// RED (§7.54) — **the menu's four lines survive the trip through Win32's
    /// numbering.**
    ///
    /// MUTATION: number any line `0` and `TrackPopupMenu`'s answer for it becomes
    /// indistinguishable from the answer for a menu the reader dismissed — the last
    /// assertion goes red. Give two lines the same number and the round trip stops
    /// being one.
    #[test]
    fn every_menu_line_is_its_own_number_and_none_of_them_is_zero() {
        for command in TrayCommand::ALL {
            assert_eq!(
                TrayCommand::from_id(command.id()),
                Some(command),
                "{command:?} comes back as itself"
            );
        }
        assert_eq!(
            TrayCommand::ALL.len(),
            4,
            "summon, a new window, settings, and leaving"
        );
        assert_eq!(
            TrayCommand::from_id(0),
            None,
            "zero is what a dismissed menu answers and can never be a line"
        );
    }

    /// RED (§7.54) — **a tip too long for the shell is cut where a reader can still
    /// read it.**
    ///
    /// MUTATION: take `TIP_CAPACITY` units instead of one fewer and the terminator
    /// overwrites the last character or runs off the array. Cut on a `char`
    /// boundary of the Rust string instead of a UTF-16 one and a tip of mostly
    /// astral text is cut far short of what fits. Drop the surrogate check and a tip
    /// cut mid-pair ends in the replacement glyph.
    #[test]
    fn a_tip_is_cut_to_what_the_shell_holds_and_never_mid_character() {
        let short = tray_tip("Folio");
        assert_eq!(short.len(), 6, "five units and the terminator");
        assert_eq!(short.last(), Some(&0));

        let long = tray_tip(&"x".repeat(500));
        assert_eq!(long.len(), TIP_CAPACITY, "exactly what the array holds");
        assert_eq!(long.last(), Some(&0), "and it still ends in a terminator");

        // An astral character is two UTF-16 units, so the cut lands mid-pair on
        // every odd capacity.
        let astral = tray_tip(&"\u{1f5dd}".repeat(200));
        assert_eq!(astral.last(), Some(&0));
        assert!(
            !astral[..astral.len() - 1]
                .last()
                .is_some_and(|unit| (0xd800..0xdc00).contains(unit)),
            "no half of a surrogate pair is left at the end"
        );
        assert!(
            String::from_utf16(&astral[..astral.len() - 1]).is_ok(),
            "what is left is text the shell can draw"
        );
    }
}

/// The numbers written down above, held against the ones Windows publishes.
#[cfg(all(test, windows))]
mod win32_constant_tests {
    use windows::Win32::UI::Shell::NOTIFYICONDATAW;
    use windows::Win32::UI::WindowsAndMessaging::{WM_LBUTTONUP, WM_RBUTTONUP, WS_EX_TOOLWINDOW};

    /// RED — this module's own copies of two Win32 constants are the Win32 ones.
    ///
    /// MUTATION: swap the two literals and the icon opens its menu on a left click
    /// and summons the window on a right one — both gestures wrong, and neither
    /// visible from inside this process.
    #[test]
    fn the_numbers_written_down_are_the_numbers_windows_publishes() {
        assert_eq!(super::WM_LBUTTONUP, WM_LBUTTONUP);
        assert_eq!(super::WM_RBUTTONUP, WM_RBUTTONUP);
        assert_eq!(
            super::windows_tray::WS_EX_TOOLWINDOW,
            WS_EX_TOOLWINDOW.0,
            "the style that keeps this window out of the taskbar and the              switcher is the one Windows means by that name"
        );
    }

    /// RED — the tip capacity written down is the array the shell actually has.
    ///
    /// MUTATION: raise `TIP_CAPACITY` and `notify`'s `copy_from_slice` writes past
    /// the end of `szTip`, which is a panic on a good day and a corrupted structure
    /// handed to the shell on a bad one.
    #[test]
    fn tray_tip_capacity_is_the_one_the_shell_publishes() {
        let data = NOTIFYICONDATAW::default();
        assert_eq!(super::TIP_CAPACITY, data.szTip.len());
    }
}
