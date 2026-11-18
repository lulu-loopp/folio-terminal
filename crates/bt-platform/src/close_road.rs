//! **Which road a window's close came by** (0.4.10 plan row 103,
//! T-CLOSE-REQUEST-SAYS-WHICH-WINDOW-AND-HOW).
//!
//! # Why this exists
//!
//! winit turns a `WM_CLOSE` into `WindowEvent::CloseRequested` and keeps nothing of the message
//! (`winit-0.30.13` `event_loop.rs:1251-1258`), so before this module the run's last line was
//! `the person closed it` whoever asked: Folio's own caption `×`, the final tab, Alt+F4, the
//! taskbar, or another program's `PostMessage`. The owner's run of 2026-11-18 ended with that
//! line and he had not closed it. This module is the witness that tells those roads apart. It
//! reads and says; it refuses nothing (不当保姆, 2026-09-21): a program may close Folio, and Folio
//! only says so.
//!
//! # Folio's own requests carry their road in the message
//!
//! [`OwnCloseRoad`] rides in the `WM_CLOSE` Folio posts: `wParam` is the road's
//! [`OwnCloseRoad::mark_code`] and `lParam` is [`OWN_CLOSE_TAG`]. Both parameters are unused by the
//! message and dropped by winit, every Folio subclass passes them on untouched, and every other
//! poster in this tree sends `0, 0`. A mark kept beside the message instead (a flag set before
//! the post) would mark "the next close this window hears": a close sent from another thread is
//! delivered inside the next `PeekMessageW`, ahead of every posted message, and would take it.
//! On macOS the request is answered inside the call (`performClose:` queues the event while the
//! handler runs), so the mark is kept per window around that call ([`mark_own_close`]).
//!
//! # The witness is the window thread's own hooks
//!
//! Only a thread hook sees winit's event-target window and a thread message, so the witness rides
//! the hooks [`crate::pump`] installs: `WH_CALLWNDPROC` for sent messages (its `wParam` is
//! nonzero when another thread sent it, as measured on Windows 11 in `hook_tests`; the API page's
//! wording reads the other way), `WH_CALLWNDPROCRET` for the end of a sent `SC_CLOSE`, and
//! `WH_GETMESSAGE` for posted ones — taken from any pump, a nested modal loop's included, which
//! winit's message hook is not.
//!
//! * A `WM_CLOSE` with Folio's mark is [`CloseRoad::Own`].
//! * A `WM_CLOSE` this thread sends while an `SC_CLOSE` for the same window is open is
//!   [`CloseRoad::SystemClose`] — `DefWindowProc` turning Alt+F4, the system menu or the
//!   taskbar's command into a close — and its delivery is the `SC_CLOSE`'s own.
//! * Anything else is [`CloseRoad::CloseMessage`], with its delivery.
//!
//! The moment's two facts are read in the hook, on the window thread, as the message reaches the
//! window: whether the window is the foreground window, and where the pointer is. Reading the
//! pointer's window (`WindowFromPoint`) sends `WM_NCHITTEST` to this thread's windows under the
//! point, so the hook re-enters their procedures for that one message; see `pointer_over`.
//!
//! A close for a window `bt-app` registered ([`watch_closes`]) is kept, in order, until the
//! window's `CloseRequested` takes it ([`take_close_arrival`]): each `WM_CLOSE` to a winit
//! window yields exactly one `CloseRequested`, in order, so the pairing is exact. A close for an
//! unowned top-level window of the thread that is not Folio's — winit's own event target, which
//! the message destroys while every Folio window stays open (plan row 41) — or a close posted to
//! the thread itself is said at once through [`crate::stderr_road`], as `session_end`'s arrival
//! line is. A window Folio made that has no `CloseRequested` (the spare web parent) is named to
//! the witness ([`name_own_window`]), and its line names it as Folio's. Owned windows (pickers,
//! message boxes) and child windows are not this module's.

use std::collections::VecDeque;

/// `WM_CLOSE`, as `WinUser.h` numbers it. Written out so the pure half compiles everywhere.
pub const WM_CLOSE: u32 = 0x0010;
/// `WM_SYSCOMMAND`, as `WinUser.h` numbers it.
pub const WM_SYSCOMMAND: u32 = 0x0112;
/// `SC_CLOSE`, the system command a window's close is; the low four bits of `wParam` are the
/// system's own and are masked off before comparing.
pub const SC_CLOSE: usize = 0xF060;

/// **The tag Folio's own `WM_CLOSE` carries in `lParam`**: "FOLC". Its `wParam` is the road.
pub const OWN_CLOSE_TAG: isize = 0x464F_4C43;

/// The longest window class a line names, in characters.
const CLASS_CHARS: usize = 64;

/// **Which of Folio's own requests a close is.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnCloseRoad {
    /// The self-drawn caption's `×`.
    CaptionButton,
    /// The window's only tab closed by a verb (its `×`, a chord, a menu, a replay).
    FinalTab,
    /// The window's only tab closed because its shells exited.
    ShellsExited,
}

impl OwnCloseRoad {
    /// The road's number in a `WM_CLOSE`'s `wParam`.
    #[must_use]
    pub const fn mark_code(self) -> usize {
        match self {
            Self::CaptionButton => 1,
            Self::FinalTab => 2,
            Self::ShellsExited => 3,
        }
    }

    /// The road a `WM_CLOSE`'s two parameters name, if they are Folio's mark.
    #[must_use]
    pub const fn from_mark(wparam: usize, lparam: isize) -> Option<Self> {
        if lparam != OWN_CLOSE_TAG {
            return None;
        }
        match wparam {
            1 => Some(Self::CaptionButton),
            2 => Some(Self::FinalTab),
            3 => Some(Self::ShellsExited),
            _ => None,
        }
    }
}

/// **How an `SC_CLOSE` was chosen, as far as its `lParam` can say** (review P2-1): a screen point
/// when the pointer chose it (the system menu clicked), and no point when the keyboard did.
///
/// The API page gives a system accelerator (Alt+F4) the high word `-1` and a mnemonic (Alt+Space,
/// then C) `0`, and `DefWindowProc` is reported to post Alt+F4's command with `0`. Which form
/// Alt+F4 takes could not be measured here: a synthesized `WM_SYSKEYDOWN` of `VK_F4` with the Alt
/// bit, posted or sent, with or without the thread's key state saying Alt and F4 are down, makes
/// no `SC_CLOSE` at all off the input thread (`hook_tests`). So the two keyboard forms are one
/// word. Two points read as the keyboard: screen (0, 0), and any point whose y is -1 (a monitor
/// above the primary's top edge). The taskbar's Close window was not measurable either (there is
/// no shell in the build box's session) and reads as whichever form it sends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemCloseFrom {
    Keyboard,
    Pointer,
}

impl SystemCloseFrom {
    /// The reading of an `SC_CLOSE`'s `lParam`: `0`, or a high word of `-1`, is the keyboard's.
    #[must_use]
    pub const fn of_lparam(lparam: isize) -> Self {
        if lparam == 0 || (lparam as u32) >> 16 == 0xFFFF {
            Self::Keyboard
        } else {
            Self::Pointer
        }
    }
}

/// **How a macOS menu's Close Window was chosen**: by its key or by the pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuFrom {
    Key,
    Pointer,
}

/// **The road a close came by.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseRoad {
    /// One of Folio's own requests.
    Own(OwnCloseRoad),
    /// Windows: the system's close command (`SC_CLOSE`) — Alt+F4, the system menu, the taskbar.
    SystemClose(SystemCloseFrom),
    /// Windows: a `WM_CLOSE` nobody marked — another program, or a tool.
    CloseMessage,
    /// macOS: the window's own close button, pressed.
    CloseButton,
    /// macOS: Close Window from the menu.
    Menu(MenuFrom),
    /// macOS: none of these — an accessibility press, a script, another program.
    Outside,
}

impl CloseRoad {
    /// The road's words in a close line's `road=` field.
    #[must_use]
    pub const fn road_word(self) -> &'static str {
        match self {
            Self::Own(OwnCloseRoad::CaptionButton) => "caption-button",
            Self::Own(OwnCloseRoad::FinalTab) => "final-tab",
            Self::Own(OwnCloseRoad::ShellsExited) => "shell-exit",
            Self::SystemClose(SystemCloseFrom::Keyboard) => "system-close (keyboard)",
            Self::SystemClose(SystemCloseFrom::Pointer) => "system-close (pointer)",
            Self::CloseMessage => "close-message",
            Self::CloseButton => "close-button",
            Self::Menu(MenuFrom::Key) => "menu (key)",
            Self::Menu(MenuFrom::Pointer) => "menu (pointer)",
            Self::Outside => "outside",
        }
    }
}

/// **How the message that started a close reached the window** (Windows).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseDelivery {
    /// Taken off the thread's queue.
    Posted,
    /// Sent by this thread.
    SentHere,
    /// Sent by another thread, of this process or another one.
    SentFromAnotherThread,
}

impl CloseDelivery {
    /// The delivery's words in a close line's `delivery=` field.
    #[must_use]
    pub const fn delivery_word(self) -> &'static str {
        match self {
            Self::Posted => "posted",
            Self::SentHere => "sent here",
            Self::SentFromAnotherThread => "sent from another thread",
        }
    }

    /// **A sent message's delivery**, by `WH_CALLWNDPROC`'s `wParam`: nonzero when another
    /// thread sent it. Measured (review P2-2, `hook_tests`): a send from another thread of this
    /// process hooks with 1, one from this thread with 0, and the `WM_CLOSE` `DefWindowProc`
    /// sends inside another thread's `SC_CLOSE` with 0. `InSendMessageEx` is no substitute: it
    /// answers "sent" for that nested close too, because the thread is still inside the other
    /// thread's send.
    #[must_use]
    pub const fn of_sent(from_another_thread: bool) -> Self {
        if from_another_thread {
            Self::SentFromAnotherThread
        } else {
            Self::SentHere
        }
    }
}

/// **Where the pointer was when the close reached the window.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClosePointer {
    /// Not read.
    Unread,
    /// Over another window, or over none of this one.
    Outside,
    /// Over this window, at a point of its client area in physical pixels.
    InWindow { x: i32, y: i32 },
    /// macOS: over the window's own close button.
    OnCloseButton,
}

/// **What the platform saw of one close request.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloseArrival {
    pub road: CloseRoad,
    /// `None` where the platform has no such reading (macOS).
    pub delivery: Option<CloseDelivery>,
    /// Whether the window was the foreground window; `None` when it could not be read.
    pub foreground: Option<bool>,
    pub pointer: ClosePointer,
}

/// Whether a message is one the witness reads: `WM_CLOSE`, or `WM_SYSCOMMAND` with `SC_CLOSE`.
#[must_use]
pub const fn is_close_message(message: u32, wparam: usize) -> bool {
    message == WM_CLOSE || (message == WM_SYSCOMMAND && wparam & 0xFFF0 == SC_CLOSE)
}

/// **Where a close no `CloseRequested` will take arrived**: the window thread itself, a window
/// Folio made and named to the witness ([`name_own_window`]), or another window of the thread,
/// by its class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnwatchedCloseAt<'a> {
    Thread,
    FolioOwn(&'static str),
    Foreign(&'a str),
}

/// **The line said when a close reaches a window of this thread whose close no `CloseRequested`
/// takes**: winit's event target, a window Folio made and named, or the thread itself. One
/// physical line; a class name is the one foreign string and is cut to one line of at most 64
/// characters. It carries no `road=`: the ruled line has none (review N1).
#[must_use]
pub fn close_arrival_line(
    at: UnwatchedCloseAt<'_>,
    delivery: CloseDelivery,
    foreground: bool,
) -> String {
    let what = match at {
        UnwatchedCloseAt::Thread => {
            "a window that is not Folio's — the window thread (no window)".to_owned()
        }
        UnwatchedCloseAt::FolioOwn(name) => format!("Folio's own {name}"),
        UnwatchedCloseAt::Foreign(class) => {
            let one_line: String = class
                .chars()
                .map(|character| {
                    if character.is_control() {
                        ' '
                    } else {
                        character
                    }
                })
                .take(CLASS_CHARS)
                .collect();
            format!("a window that is not Folio's — class \"{one_line}\"")
        }
    };
    format!(
        "Folio: close message to {what}; delivery={}; foreground={}; Folio's windows stay open",
        delivery.delivery_word(),
        if foreground { "yes" } else { "no" },
    )
}

/// An `SC_CLOSE` whose processing has not finished.
#[derive(Clone, Copy, Debug)]
struct OpenSystemClose {
    window: isize,
    from: SystemCloseFrom,
    delivery: CloseDelivery,
}

/// **The witness's book, without a window system**: which windows are Folio's, the closes each
/// has heard and its `CloseRequested` has not taken yet, and the `SC_CLOSE`s still being
/// processed. The hooks feed it on Windows; its arithmetic is the whole of the classification.
#[derive(Debug, Default)]
pub struct CloseBook {
    watched: Vec<(isize, VecDeque<CloseArrival>)>,
    open: Vec<OpenSystemClose>,
    /// Folio's own windows that no `CloseRequested` answers for, by what they are.
    named: Vec<(isize, &'static str)>,
}

impl CloseBook {
    /// Folio's window `window` from now on: its closes are kept for its `CloseRequested`.
    pub fn watch_window(&mut self, window: isize) {
        if !self.watches(window) {
            self.watched.push((window, VecDeque::new()));
        }
    }

    /// Not Folio's any more (it has gone): whatever it heard and nobody took goes with it.
    pub fn unwatch_window(&mut self, window: isize) {
        self.watched.retain(|(watched, _)| *watched != window);
        self.open.retain(|open| open.window != window);
    }

    /// Folio made `window` and no `CloseRequested` answers for it: a close to it is said as
    /// reaching Folio's own `name`.
    pub fn name_window(&mut self, window: isize, name: &'static str) {
        self.named.retain(|(named, _)| *named != window);
        self.named.push((window, name));
    }

    /// The named window has gone.
    pub fn forget_named_window(&mut self, window: isize) {
        self.named.retain(|(named, _)| *named != window);
    }

    /// What Folio named `window`, if it did.
    #[must_use]
    pub fn name_of_own_window(&self, window: isize) -> Option<&'static str> {
        self.named
            .iter()
            .find(|(named, _)| *named == window)
            .map(|(_, name)| *name)
    }

    /// Whether `window` is one of Folio's.
    #[must_use]
    pub fn watches(&self, window: isize) -> bool {
        self.watched.iter().any(|(watched, _)| *watched == window)
    }

    /// The oldest close `window` heard that nobody has taken.
    pub fn take_arrival(&mut self, window: isize) -> Option<CloseArrival> {
        self.watched
            .iter_mut()
            .find(|(watched, _)| *watched == window)
            .and_then(|(_, heard)| heard.pop_front())
    }

    /// An `SC_CLOSE` began reaching `window`.
    pub fn system_close_began(&mut self, window: isize, lparam: isize, delivery: CloseDelivery) {
        self.open.push(OpenSystemClose {
            window,
            from: SystemCloseFrom::of_lparam(lparam),
            delivery,
        });
    }

    /// A sent `SC_CLOSE` to `window` has returned from its procedure.
    pub fn sent_system_close_returned(&mut self, window: isize) {
        if let Some(index) = self
            .open
            .iter()
            .rposition(|open| open.window == window && open.delivery != CloseDelivery::Posted)
        {
            self.open.remove(index);
        }
    }

    /// The thread took its next posted message, so every posted `SC_CLOSE` is over: its dispatch
    /// ended before the next retrieval.
    pub fn posted_message_taken(&mut self) {
        self.open
            .retain(|open| open.delivery != CloseDelivery::Posted);
    }

    /// Whether a posted `SC_CLOSE` is still open — the one thing a retrieval has to ask.
    #[must_use]
    pub fn a_posted_system_close_is_open(&self) -> bool {
        self.open
            .iter()
            .any(|open| open.delivery == CloseDelivery::Posted)
    }

    /// **The road and delivery of a `WM_CLOSE` to `window`** with these parameters, arriving by
    /// `delivery`.
    #[must_use]
    pub fn classify_close(
        &self,
        window: isize,
        wparam: usize,
        lparam: isize,
        delivery: CloseDelivery,
    ) -> (CloseRoad, CloseDelivery) {
        if let Some(own) = OwnCloseRoad::from_mark(wparam, lparam) {
            return (CloseRoad::Own(own), delivery);
        }
        if delivery == CloseDelivery::SentHere
            && let Some(open) = self.open.iter().rev().find(|open| open.window == window)
        {
            return (CloseRoad::SystemClose(open.from), open.delivery);
        }
        (CloseRoad::CloseMessage, delivery)
    }

    /// Keep `arrival` for `window`'s `CloseRequested`; `false` when `window` is not Folio's.
    pub fn keep_arrival(&mut self, window: isize, arrival: CloseArrival) -> bool {
        match self
            .watched
            .iter_mut()
            .find(|(watched, _)| *watched == window)
        {
            Some((_, heard)) => {
                heard.push_back(arrival);
                true
            }
            None => false,
        }
    }
}

/// **Folio's window `window` from now on**: its closes are kept for its `CloseRequested`
/// ([`take_close_arrival`]). Called on the window thread when the window is built.
///
/// macOS: the first watch starts recording the event AppKit is dispatching, which is what a
/// close's road is read off (review P1-1).
pub fn watch_closes(window: crate::NativeWindow) {
    #[cfg(windows)]
    windows_close_road::with_book(|book| book.watch_window(window.as_handle().get()));
    #[cfg(target_os = "macos")]
    {
        let _ = window;
        crate::macos_impl::watch_dispatched_events();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = window;
}

/// **A window Folio made whose close no `CloseRequested` answers for**, named for the line a
/// close to it says (review N2): the spare web parent, which `DefWindowProc` destroys on a
/// `WM_CLOSE` (plan row 41). Windows only; called on the window thread.
#[cfg(windows)]
pub(crate) fn name_own_window(window: crate::NativeWindow, name: &'static str) {
    windows_close_road::with_book(|book| book.name_window(window.as_handle().get(), name));
}

/// **The named window has gone.**
#[cfg(windows)]
pub(crate) fn forget_own_window(window: crate::NativeWindow) {
    windows_close_road::with_book(|book| book.forget_named_window(window.as_handle().get()));
}

/// **The window has gone**: what it heard and nobody took goes with it.
pub fn unwatch_closes(window: crate::NativeWindow) {
    #[cfg(windows)]
    windows_close_road::with_book(|book| book.unwatch_window(window.as_handle().get()));
    #[cfg(target_os = "macos")]
    mac_marks::forget_marks(window);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = window;
}

/// **What the platform saw of the close request `window` has just received**, taken: called once
/// per `CloseRequested`, on the window thread.
///
/// Windows: the oldest close the hooks kept for it, or `None` when they saw none (no hooks).
/// macOS: Folio's own mark when its request is the one being answered, else the moment read
/// now. Elsewhere `None`.
#[must_use]
pub fn take_close_arrival(window: crate::NativeWindow) -> Option<CloseArrival> {
    #[cfg(windows)]
    {
        windows_close_road::with_book(|book| book.take_arrival(window.as_handle().get()))
    }
    #[cfg(target_os = "macos")]
    {
        let own = mac_marks::take_mark(window);
        crate::macos_impl::close_context(window, own)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = window;
        None
    }
}

/// **Folio is about to ask `window` to close by `road`** (macOS): the request is answered inside
/// the call, so the mark taken here is the one its `CloseRequested` reads.
#[cfg(target_os = "macos")]
pub(crate) fn mark_own_close(window: crate::NativeWindow, road: OwnCloseRoad) {
    mac_marks::mark_close(window, road);
}

/// macOS: Folio's own marks, one per window, kept on the main thread.
#[cfg(target_os = "macos")]
mod mac_marks {
    use std::cell::RefCell;

    use super::OwnCloseRoad;
    use crate::NativeWindow;

    thread_local! {
        static MARKS: RefCell<Vec<(NativeWindow, OwnCloseRoad)>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn mark_close(window: NativeWindow, road: OwnCloseRoad) {
        MARKS.with(|marks| {
            let mut marks = marks.borrow_mut();
            marks.retain(|(marked, _)| *marked != window);
            marks.push((window, road));
        });
    }

    pub(super) fn take_mark(window: NativeWindow) -> Option<OwnCloseRoad> {
        MARKS.with(|marks| {
            let mut marks = marks.borrow_mut();
            let index = marks.iter().position(|(marked, _)| *marked == window)?;
            Some(marks.remove(index).1)
        })
    }

    pub(super) fn forget_marks(window: NativeWindow) {
        MARKS.with(|marks| marks.borrow_mut().retain(|(marked, _)| *marked != window));
    }
}

/// The hooks' half, on the window thread: the book, and what the moment says.
#[cfg(windows)]
pub(crate) mod windows_close_road {
    use std::cell::RefCell;

    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GW_OWNER, GWL_STYLE, GetAncestor, GetClassNameW, GetCursorPos,
        GetForegroundWindow, GetWindow, GetWindowLongW, WS_CHILD, WindowFromPoint,
    };

    use super::{
        CloseArrival, CloseBook, CloseDelivery, ClosePointer, SC_CLOSE, UnwatchedCloseAt, WM_CLOSE,
        WM_SYSCOMMAND, close_arrival_line,
    };

    thread_local! {
        static BOOK: RefCell<CloseBook> = RefCell::new(CloseBook::default());
    }

    /// The book, borrowed for one statement and never across a call out of this module.
    pub(super) fn with_book<T>(read: impl FnOnce(&mut CloseBook) -> T) -> T {
        BOOK.with(|book| read(&mut book.borrow_mut()))
    }

    /// `WH_CALLWNDPROC`: a close message is about to reach `hwnd`'s procedure, sent by this thread
    /// or by another one.
    pub(crate) fn heard_sent(
        hwnd: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
        from_another_thread: bool,
    ) {
        heard_close(
            hwnd,
            message,
            wparam,
            lparam,
            CloseDelivery::of_sent(from_another_thread),
        );
    }

    /// `WH_CALLWNDPROCRET`: a sent message to `hwnd` has returned from its procedure.
    pub(crate) fn sent_returned(hwnd: HWND, message: u32, wparam: usize) {
        if message == WM_SYSCOMMAND && wparam & 0xFFF0 == SC_CLOSE {
            with_book(|book| book.sent_system_close_returned(hwnd.0 as isize));
        }
    }

    /// `WH_GETMESSAGE` with `PM_REMOVE`: the thread took a posted message off its queue.
    pub(crate) fn heard_posted(hwnd: HWND, message: u32, wparam: usize, lparam: isize) {
        with_book(|book| {
            if book.a_posted_system_close_is_open() {
                book.posted_message_taken();
            }
        });
        if super::is_close_message(message, wparam) {
            heard_close(hwnd, message, wparam, lparam, CloseDelivery::Posted);
        }
    }

    fn heard_close(
        hwnd: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
        delivery: CloseDelivery,
    ) {
        let window = hwnd.0 as isize;
        if message == WM_SYSCOMMAND {
            with_book(|book| book.system_close_began(window, lparam, delivery));
            return;
        }
        if message != WM_CLOSE {
            return;
        }
        let (road, delivery, ours, named) = with_book(|book| {
            let (road, delivery) = book.classify_close(window, wparam, lparam, delivery);
            (
                road,
                delivery,
                book.watches(window),
                book.name_of_own_window(window),
            )
        });
        // SAFETY: a read with no arguments.
        let foreground = !hwnd.is_invalid() && unsafe { GetForegroundWindow() } == hwnd;
        if ours {
            let arrival = CloseArrival {
                road,
                delivery: Some(delivery),
                foreground: Some(foreground),
                pointer: pointer_over(hwnd),
            };
            with_book(|book| book.keep_arrival(window, arrival));
            return;
        }
        // No `CloseRequested` will take it: said at once if it is the thread's own (no window),
        // a window Folio named, or an unowned top-level window of the thread (winit's event
        // target), and left alone otherwise.
        let class;
        let at = if hwnd.is_invalid() {
            UnwatchedCloseAt::Thread
        } else if let Some(name) = named {
            UnwatchedCloseAt::FolioOwn(name)
        } else {
            if !unowned_top_level(hwnd) {
                return;
            }
            class = class_of(hwnd);
            UnwatchedCloseAt::Foreign(&class)
        };
        crate::stderr_road::say(close_arrival_line(at, delivery, foreground));
    }

    /// Where the pointer is: over `hwnd`'s client area, or not over `hwnd` at all.
    fn pointer_over(hwnd: HWND) -> ClosePointer {
        let mut point = POINT::default();
        // SAFETY: the point is writable for the call.
        if unsafe { GetCursorPos(&mut point) }.is_err() {
            return ClosePointer::Unread;
        }
        // SAFETY: a screen point and a window read back. `WindowFromPoint` is not a pure read: it
        // sends `WM_NCHITTEST` to this thread's windows under the point, so their procedures run
        // inside this hook. That is safe because the book is not borrowed across the call, the
        // custom frame's subclass answers the hit test from atomics, `GetClientRect` and
        // `IsZoomed`, and winit has no arm for it. A point no window covers answers null, which
        // `GetAncestor` answers null for.
        let root = unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) };
        if root != hwnd {
            return ClosePointer::Outside;
        }
        // SAFETY: `hwnd` is the window the message is for, alive for it; the point is writable.
        if !unsafe { ScreenToClient(hwnd, &mut point) }.as_bool() {
            return ClosePointer::Unread;
        }
        ClosePointer::InWindow {
            x: point.x,
            y: point.y,
        }
    }

    /// Whether `hwnd` is a top-level window nobody owns.
    fn unowned_top_level(hwnd: HWND) -> bool {
        // SAFETY: two reads of the window's own state.
        let owned = unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok_and(|owner| !owner.is_invalid());
        let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) };
        !owned && style & WS_CHILD.0.cast_signed() == 0
    }

    fn class_of(hwnd: HWND) -> String {
        let mut name = [0u16; 128];
        // SAFETY: the buffer outlives the call, which writes at most its length.
        let length = unsafe { GetClassNameW(hwnd, &mut name) };
        let length = usize::try_from(length).unwrap_or(0);
        String::from_utf16_lossy(&name[..length.min(name.len())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: isize = 0x1234;
    const TARGET: isize = 0x5678;

    fn book_arrival(road: CloseRoad, delivery: CloseDelivery) -> CloseArrival {
        CloseArrival {
            road,
            delivery: Some(delivery),
            foreground: Some(true),
            pointer: ClosePointer::Unread,
        }
    }

    /// RED (mutation: `from_mark` ignoring the tag) — **only Folio's tag makes a close Folio's
    /// own**, and each road keeps its number.
    #[test]
    fn a_close_is_folios_own_only_with_the_tag_and_a_known_road() {
        for road in [
            OwnCloseRoad::CaptionButton,
            OwnCloseRoad::FinalTab,
            OwnCloseRoad::ShellsExited,
        ] {
            assert_eq!(
                OwnCloseRoad::from_mark(road.mark_code(), OWN_CLOSE_TAG),
                Some(road)
            );
            assert_eq!(OwnCloseRoad::from_mark(road.mark_code(), 0), None);
        }
        assert_eq!(OwnCloseRoad::from_mark(0, OWN_CLOSE_TAG), None);
        assert_eq!(OwnCloseRoad::from_mark(9, OWN_CLOSE_TAG), None);
    }

    /// RED (mutation: drop the `-1` high word from the keyboard arm) — **an `SC_CLOSE`'s `lParam`
    /// says the keyboard when it holds no point, and the pointer otherwise** (review P2-1).
    #[test]
    fn a_system_close_is_read_as_the_keyboard_or_the_pointer() {
        assert_eq!(SystemCloseFrom::of_lparam(-1), SystemCloseFrom::Keyboard);
        assert_eq!(
            SystemCloseFrom::of_lparam(0xFFFF_0000_u32 as isize),
            SystemCloseFrom::Keyboard
        );
        assert_eq!(SystemCloseFrom::of_lparam(0), SystemCloseFrom::Keyboard);
        assert_eq!(
            SystemCloseFrom::of_lparam((300 << 16) | 120),
            SystemCloseFrom::Pointer
        );
    }

    /// RED (mutation: classify a tagged close as a close message) — **the classifier**: the tag
    /// wins on any delivery, a close sent here inside an open `SC_CLOSE` is the system's command
    /// with the command's delivery, an unknown road code is a close message, and a close sent
    /// from another thread is said as such.
    #[test]
    fn a_close_is_classified_by_its_mark_its_delivery_and_an_open_system_close() {
        let mut book = CloseBook::default();
        assert_eq!(
            book.classify_close(MAIN, 1, OWN_CLOSE_TAG, CloseDelivery::Posted),
            (
                CloseRoad::Own(OwnCloseRoad::CaptionButton),
                CloseDelivery::Posted
            )
        );
        assert_eq!(
            book.classify_close(MAIN, 7, OWN_CLOSE_TAG, CloseDelivery::Posted),
            (CloseRoad::CloseMessage, CloseDelivery::Posted)
        );
        assert_eq!(
            book.classify_close(MAIN, 0, 0, CloseDelivery::SentFromAnotherThread),
            (
                CloseRoad::CloseMessage,
                CloseDelivery::SentFromAnotherThread
            )
        );
        assert_eq!(
            book.classify_close(MAIN, 0, 0, CloseDelivery::SentHere),
            (CloseRoad::CloseMessage, CloseDelivery::SentHere),
            "a close sent here with no system command open is a close message"
        );
        book.system_close_began(MAIN, -1, CloseDelivery::SentHere);
        assert_eq!(
            book.classify_close(MAIN, 0, 0, CloseDelivery::SentHere),
            (
                CloseRoad::SystemClose(SystemCloseFrom::Keyboard),
                CloseDelivery::SentHere
            )
        );
        assert_eq!(
            book.classify_close(MAIN, 0, 0, CloseDelivery::SentFromAnotherThread),
            (
                CloseRoad::CloseMessage,
                CloseDelivery::SentFromAnotherThread
            ),
            "another thread's close is not the command this thread's procedure is running"
        );
        assert_eq!(
            book.classify_close(TARGET, 0, 0, CloseDelivery::SentHere),
            (CloseRoad::CloseMessage, CloseDelivery::SentHere),
            "another window's system command is not this window's"
        );
        book.sent_system_close_returned(MAIN);
        assert_eq!(
            book.classify_close(MAIN, 0, 0, CloseDelivery::SentHere),
            (CloseRoad::CloseMessage, CloseDelivery::SentHere),
            "the command is over once it has returned"
        );
    }

    /// RED (mutation: `of_sent` reading the hook's `wParam` the other way round) — **a sent close
    /// is another thread's when the hook says so, and this thread's otherwise.**
    #[test]
    fn a_sent_close_is_another_threads_only_when_the_hook_says_so() {
        assert_eq!(
            CloseDelivery::of_sent(true),
            CloseDelivery::SentFromAnotherThread
        );
        assert_eq!(CloseDelivery::of_sent(false), CloseDelivery::SentHere);
    }

    /// RED (mutation: `posted_message_taken` keeping posted commands) — **a posted `SC_CLOSE`
    /// lends its delivery to the close it causes, and is over at the thread's next retrieval.**
    #[test]
    fn a_posted_system_close_lasts_until_the_next_retrieval() {
        let mut book = CloseBook::default();
        book.system_close_began(MAIN, (40 << 16) | 900, CloseDelivery::Posted);
        assert!(book.a_posted_system_close_is_open());
        assert_eq!(
            book.classify_close(MAIN, 0, 0, CloseDelivery::SentHere),
            (
                CloseRoad::SystemClose(SystemCloseFrom::Pointer),
                CloseDelivery::Posted
            )
        );
        book.sent_system_close_returned(MAIN);
        assert!(
            book.a_posted_system_close_is_open(),
            "a sent command's return does not end a posted one"
        );
        book.posted_message_taken();
        assert!(!book.a_posted_system_close_is_open());
        assert_eq!(
            book.classify_close(MAIN, 0, 0, CloseDelivery::SentHere),
            (CloseRoad::CloseMessage, CloseDelivery::SentHere)
        );
    }

    /// RED (mutation: `take_arrival` reading without removing) — **a watched window's closes are kept in
    /// order and taken once each; an unwatched window's are not kept, and an unwatched window's
    /// book goes with it.**
    #[test]
    fn a_watched_windows_closes_are_taken_once_each_in_order() {
        let mut book = CloseBook::default();
        book.watch_window(MAIN);
        let first = book_arrival(
            CloseRoad::Own(OwnCloseRoad::FinalTab),
            CloseDelivery::Posted,
        );
        let second = book_arrival(
            CloseRoad::CloseMessage,
            CloseDelivery::SentFromAnotherThread,
        );
        assert!(book.keep_arrival(MAIN, first));
        assert!(book.keep_arrival(MAIN, second));
        assert!(
            !book.keep_arrival(TARGET, first),
            "the event target is not Folio's"
        );
        assert_eq!(book.take_arrival(MAIN), Some(first));
        assert_eq!(book.take_arrival(MAIN), Some(second));
        assert_eq!(book.take_arrival(MAIN), None);
        assert!(book.keep_arrival(MAIN, first));
        book.unwatch_window(MAIN);
        assert!(!book.watches(MAIN));
        assert_eq!(book.take_arrival(MAIN), None);
    }

    /// RED (mutation: the class name not cut to one line) — **the line for a window that is not
    /// Folio's names its class on one bounded line and says nothing closed**; it carries no
    /// `road=` (review N1).
    #[test]
    fn a_close_to_a_window_that_is_not_folios_says_its_class_on_one_line() {
        let line = close_arrival_line(
            UnwatchedCloseAt::Foreign("Winit Thread Event Target"),
            CloseDelivery::Posted,
            false,
        );
        assert_eq!(
            line,
            "Folio: close message to a window that is not Folio's — class \"Winit Thread Event \
             Target\"; delivery=posted; foreground=no; Folio's windows stay open"
        );
        let long = format!("planted\nclass{}", "x".repeat(300));
        let line = close_arrival_line(
            UnwatchedCloseAt::Foreign(&long),
            CloseDelivery::SentFromAnotherThread,
            true,
        );
        assert!(!line.contains(['\r', '\n']), "{line:?}");
        assert!(line.contains(&format!("class \"planted class{}\"", "x".repeat(51))));
        assert!(!line.contains(&"x".repeat(52)), "{line}");
        assert!(
            close_arrival_line(UnwatchedCloseAt::Thread, CloseDelivery::Posted, false)
                .contains("— the window thread (no window); delivery=posted")
        );
    }

    /// RED (mutation: `name_of_own_window` answering `None`) — **a close to a window Folio made and named is
    /// said as reaching Folio's own window, by its name** (review N2), until the window has gone.
    #[test]
    fn a_close_to_a_window_folio_named_says_it_is_folios_own() {
        let mut book = CloseBook::default();
        book.name_window(TARGET, "spare web parent");
        let named = book.name_of_own_window(TARGET).expect("named");
        assert_eq!(
            close_arrival_line(
                UnwatchedCloseAt::FolioOwn(named),
                CloseDelivery::Posted,
                false
            ),
            "Folio: close message to Folio's own spare web parent; delivery=posted; \
             foreground=no; Folio's windows stay open"
        );
        assert!(
            !book.watches(TARGET),
            "named is not watched: no arrival is kept for it"
        );
        book.forget_named_window(TARGET);
        assert_eq!(book.name_of_own_window(TARGET), None);
    }

    /// RED (mutation: a `WM_SYSCOMMAND` of any command read as a close) — **only `WM_CLOSE` and
    /// `SC_CLOSE` are close messages**, whatever the command's low four bits.
    #[test]
    fn only_wm_close_and_sc_close_are_close_messages() {
        assert!(is_close_message(WM_CLOSE, 0));
        assert!(is_close_message(WM_SYSCOMMAND, SC_CLOSE));
        assert!(is_close_message(WM_SYSCOMMAND, SC_CLOSE | 0x3));
        assert!(!is_close_message(WM_SYSCOMMAND, 0xF020), "SC_MINIMIZE");
        assert!(!is_close_message(0x0011, 0), "WM_QUERYENDSESSION");
    }
}

/// **The witness over real Win32** (0.4.10 plan row 103, review P2-2): this thread's own hooks,
/// a plain top-level window whose procedure swallows `WM_CLOSE` (so it stays to be closed again),
/// and the closes Windows itself delivers to it, read back as the window's `CloseRequested`
/// would read them.
#[cfg(all(test, windows))]
mod hook_tests {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE,
        PeekMessageW, PostMessageW, RegisterClassW, SendMessageW, TranslateMessage, WM_NULL,
        WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };
    use windows::core::{HSTRING, PCWSTR};

    use super::*;

    /// Every `SC_CLOSE` the window's procedure was handed, by its `lParam`, in order.
    static SYSTEM_CLOSES: Mutex<Vec<isize>> = Mutex::new(Vec::new());

    unsafe extern "system" fn swallowing_close_procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_CLOSE {
            return LRESULT(0);
        }
        if message == WM_SYSCOMMAND && wparam.0 & 0xFFF0 == SC_CLOSE {
            SYSTEM_CLOSES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(lparam.0);
        }
        // SAFETY: the arguments are the ones this procedure was called with.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    /// Take and dispatch every message on this thread's queue, sent ones included.
    fn pump_this_thread() {
        let mut message = MSG::default();
        // SAFETY: plain Win32 calls on this thread's own queue.
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    fn system_closes() -> Vec<isize> {
        std::mem::take(
            &mut *SYSTEM_CLOSES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// RED (one mutation per row) — **each road Windows delivers is read as that road**:
    ///
    /// * Folio's own post, marked: `Own(FinalTab)`, posted. MUTATION: drop `heard_posted` from
    ///   `taken_posted`.
    /// * Another program's `PostMessage(WM_CLOSE, 0, 0)`: a close message, posted.
    /// * `SendMessage(WM_CLOSE)` from another thread: a close message, sent from another
    ///   thread. MUTATION: drop `heard_sent` from `before_sent`, or invert `wparam.0 != 0`.
    /// * Another thread's `SendMessage(WM_SYSCOMMAND, SC_CLOSE)`: the system's close command,
    ///   sent from another thread, read off the `WM_CLOSE` `DefWindowProc` sends inside it.
    /// * A posted `SC_CLOSE`: the system's close command, posted.
    /// * A posted `SC_CLOSE` is over at the thread's next retrieval, so a close this thread sends
    ///   afterwards is a close message. MUTATION: drop `posted_message_taken` from
    ///   `heard_posted`.
    /// * The `SC_CLOSE` forms: a high word of `-1` and `0` are the keyboard's, a point the
    ///   pointer's. Alt+F4 itself cannot be made here: a synthesized `WM_SYSKEYDOWN` of `VK_F4`
    ///   with the Alt bit, posted or sent, with or without the thread's key state saying Alt and
    ///   F4 are down, made no `SC_CLOSE` on the build box (measured 2026-11-18).
    /// * A sent `SC_CLOSE`: the system's close command, sent here; once it has returned, a close
    ///   this thread sends is a close message. MUTATION: drop `sent_returned` from `after_sent`.
    #[test]
    fn the_hooks_read_each_road_windows_delivers() {
        crate::pump::install_witness().expect("this thread's hooks");
        let class = HSTRING::from("FolioCloseRoadProbe");
        let wnd = WNDCLASSW {
            lpfnWndProc: Some(swallowing_close_procedure),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // SAFETY: the class name outlives the call; a second registration of it answers zero and
        // the first class is used.
        unsafe { RegisterClassW(&wnd) };
        // SAFETY: a never-shown top-level window of this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                PCWSTR(class.as_ptr()),
                PCWSTR(HSTRING::new().as_ptr()),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                400,
                300,
                None,
                None,
                None,
                None,
            )
        }
        .expect("a window to close");
        let window = crate::NativeWindow::from_hwnd(hwnd).expect("a real window handle");
        watch_closes(window);
        let taken = || take_close_arrival(window).map(|arrival| (arrival.road, arrival.delivery));
        let read = |road: CloseRoad, delivery: CloseDelivery| Some((road, Some(delivery)));

        crate::request_window_close(window, OwnCloseRoad::FinalTab).expect("posted");
        pump_this_thread();
        assert_eq!(
            taken(),
            read(
                CloseRoad::Own(OwnCloseRoad::FinalTab),
                CloseDelivery::Posted
            ),
            "Folio's own post"
        );

        // SAFETY: a post to this thread's own live window.
        unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }.expect("posted");
        pump_this_thread();
        assert_eq!(
            taken(),
            read(CloseRoad::CloseMessage, CloseDelivery::Posted),
            "another program's post"
        );

        let target = hwnd.0 as isize;
        let sender = std::thread::spawn(move || {
            // SAFETY: a send to a live window of another thread, which pumps until it returns.
            unsafe { SendMessageW(HWND(target as *mut _), WM_CLOSE, None, None) };
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        while !sender.is_finished() && Instant::now() < deadline {
            pump_this_thread();
            std::thread::sleep(Duration::from_millis(5));
        }
        sender.join().expect("the sender returns");
        assert_eq!(
            taken(),
            read(
                CloseRoad::CloseMessage,
                CloseDelivery::SentFromAnotherThread
            ),
            "another thread's send"
        );

        let sender = std::thread::spawn(move || {
            // SAFETY: a send to a live window of another thread, which pumps until it returns.
            unsafe {
                SendMessageW(
                    HWND(target as *mut _),
                    WM_SYSCOMMAND,
                    Some(WPARAM(SC_CLOSE)),
                    Some(LPARAM(0)),
                )
            };
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        while !sender.is_finished() && Instant::now() < deadline {
            pump_this_thread();
            std::thread::sleep(Duration::from_millis(5));
        }
        sender.join().expect("the sender returns");
        assert_eq!(system_closes(), [0], "the command reached the window");
        assert_eq!(
            taken(),
            read(
                CloseRoad::SystemClose(SystemCloseFrom::of_lparam(0)),
                CloseDelivery::SentFromAnotherThread
            ),
            "another thread's system close: the close it causes is sent here, inside it"
        );

        // SAFETY: a post to this thread's own live window.
        unsafe { PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(SC_CLOSE), LPARAM(0)) }
            .expect("posted");
        pump_this_thread();
        assert_eq!(system_closes(), [0], "the command reached the window");
        assert_eq!(
            taken(),
            read(
                CloseRoad::SystemClose(SystemCloseFrom::of_lparam(0)),
                CloseDelivery::Posted
            ),
            "a posted system close"
        );

        // SAFETY: a post to this thread's own live window; its retrieval ends the posted command.
        unsafe { PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)) }.expect("posted");
        pump_this_thread();
        // SAFETY: a send to this thread's own live window.
        unsafe { SendMessageW(hwnd, WM_CLOSE, None, None) };
        assert_eq!(
            taken(),
            read(CloseRoad::CloseMessage, CloseDelivery::SentHere),
            "a posted command is over at the next retrieval"
        );

        for (lparam, from) in [
            (0xFFFF_0000_u32 as isize, SystemCloseFrom::Keyboard),
            ((300 << 16) | 120, SystemCloseFrom::Pointer),
        ] {
            // SAFETY: two posts to this thread's own live window; the second's retrieval ends the
            // first, posted, command.
            unsafe {
                PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(SC_CLOSE), LPARAM(lparam))
                    .expect("posted");
                PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)).expect("posted");
            }
            pump_this_thread();
            assert_eq!(system_closes(), [lparam], "the command reached the window");
            assert_eq!(
                taken(),
                read(CloseRoad::SystemClose(from), CloseDelivery::Posted),
                "a system close by its form {lparam:#x}"
            );
        }

        // SAFETY: a send to this thread's own live window.
        unsafe { SendMessageW(hwnd, WM_SYSCOMMAND, Some(WPARAM(SC_CLOSE)), Some(LPARAM(0))) };
        assert_eq!(
            taken(),
            read(
                CloseRoad::SystemClose(SystemCloseFrom::of_lparam(0)),
                CloseDelivery::SentHere
            ),
            "a sent system close"
        );
        // SAFETY: a send to this thread's own live window.
        unsafe { SendMessageW(hwnd, WM_CLOSE, None, None) };
        assert_eq!(
            taken(),
            read(CloseRoad::CloseMessage, CloseDelivery::SentHere),
            "a sent command is over once it has returned"
        );
        assert_eq!(taken(), None, "one arrival per close, and no more");

        unwatch_closes(window);
        // SAFETY: this thread's own window, destroyed once.
        let _ = unsafe { DestroyWindow(hwnd) };
    }
}
