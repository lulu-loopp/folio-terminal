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
//! the hooks [`crate::pump`] installs: `WH_CALLWNDPROC` for sent messages (its `wParam` says
//! whether this thread sent it), `WH_CALLWNDPROCRET` for the end of a sent `SC_CLOSE`, and
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
//! window: whether the window is the foreground window, and where the pointer is.
//!
//! A close for a window `bt-app` registered ([`watch_closes`]) is kept, in order, until the
//! window's `CloseRequested` takes it ([`take_close_arrival`]): each `WM_CLOSE` to a winit
//! window yields exactly one `CloseRequested`, in order, so the pairing is exact. A close for an
//! unowned top-level window of the thread that is not Folio's — winit's own event target, which
//! the message destroys while every Folio window stays open (plan row 41) — or a close posted to
//! the thread itself is said at once through [`crate::stderr_road`], as `session_end`'s arrival
//! line is. Owned windows (pickers, message boxes) and child windows are not this module's.

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

/// **How an `SC_CLOSE` was chosen**, read off its `lParam` as the API documents it: the high word
/// is `-1` for a system accelerator (Alt+F4), the whole is `0` for a mnemonic, and otherwise it
/// is the pointer's screen position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemCloseFrom {
    Accelerator,
    Mnemonic,
    Pointer,
}

impl SystemCloseFrom {
    /// The documented reading of an `SC_CLOSE`'s `lParam`.
    #[must_use]
    pub const fn of_lparam(lparam: isize) -> Self {
        if (lparam as u32) >> 16 == 0xFFFF {
            Self::Accelerator
        } else if lparam == 0 {
            Self::Mnemonic
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
            Self::SystemClose(SystemCloseFrom::Accelerator) => "system-close (accelerator)",
            Self::SystemClose(SystemCloseFrom::Mnemonic) => "system-close (mnemonic)",
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

    /// **A sent message's delivery**, by `WH_CALLWNDPROC`'s `wParam`: nonzero when this thread
    /// sent it.
    #[must_use]
    pub const fn of_sent(this_thread: bool) -> Self {
        if this_thread {
            Self::SentHere
        } else {
            Self::SentFromAnotherThread
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

/// **The line said when a close reaches a window of this thread that is not Folio's** — winit's
/// event target, or the thread itself (`class` `None`). One physical line; the class name is the
/// one foreign string and is cut to one line of at most 64 characters.
#[must_use]
pub fn close_arrival_line(
    class: Option<&str>,
    road: CloseRoad,
    delivery: CloseDelivery,
    foreground: bool,
) -> String {
    let what = class.map_or_else(
        || "the window thread (no window)".to_owned(),
        |class| {
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
            format!("class \"{one_line}\"")
        },
    );
    format!(
        "Folio: close message to a window that is not Folio's — {what}; road={}; delivery={}; \
         foreground={}; Folio's windows stay open",
        road.road_word(),
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
pub fn watch_closes(window: crate::NativeWindow) {
    #[cfg(windows)]
    windows_close_road::with_book(|book| book.watch_window(window.as_handle().get()));
    #[cfg(not(windows))]
    let _ = window;
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
        CloseArrival, CloseBook, CloseDelivery, ClosePointer, SC_CLOSE, WM_CLOSE, WM_SYSCOMMAND,
        close_arrival_line,
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
        this_thread: bool,
    ) {
        heard_close(
            hwnd,
            message,
            wparam,
            lparam,
            CloseDelivery::of_sent(this_thread),
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
        let (road, delivery, ours) = with_book(|book| {
            let (road, delivery) = book.classify_close(window, wparam, lparam, delivery);
            (road, delivery, book.watches(window))
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
        // Not Folio's: said at once if it is the thread's own (no window) or an unowned
        // top-level window of the thread — winit's event target — and left alone otherwise.
        let class = if hwnd.is_invalid() {
            None
        } else {
            if !unowned_top_level(hwnd) {
                return;
            }
            Some(class_of(hwnd))
        };
        crate::stderr_road::say(close_arrival_line(
            class.as_deref(),
            road,
            delivery,
            foreground,
        ));
    }

    /// Where the pointer is: over `hwnd`'s client area, or not over `hwnd` at all.
    fn pointer_over(hwnd: HWND) -> ClosePointer {
        let mut point = POINT::default();
        // SAFETY: the point is writable for the call.
        if unsafe { GetCursorPos(&mut point) }.is_err() {
            return ClosePointer::Unread;
        }
        // SAFETY: two read-only hit tests over a screen point; a point no window covers answers
        // null, which `GetAncestor` answers null for.
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

    /// RED (mutation: drop the accelerator arm) — **an `SC_CLOSE`'s `lParam` is read as the API
    /// documents it.**
    #[test]
    fn a_system_close_is_read_as_accelerator_mnemonic_or_pointer() {
        assert_eq!(SystemCloseFrom::of_lparam(-1), SystemCloseFrom::Accelerator);
        assert_eq!(
            SystemCloseFrom::of_lparam(0xFFFF_0000_u32 as isize),
            SystemCloseFrom::Accelerator
        );
        assert_eq!(SystemCloseFrom::of_lparam(0), SystemCloseFrom::Mnemonic);
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
                CloseRoad::SystemClose(SystemCloseFrom::Accelerator),
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
    /// is this thread's when the hook says so, and another thread's otherwise.**
    #[test]
    fn a_sent_close_is_this_threads_only_when_the_hook_says_so() {
        assert_eq!(CloseDelivery::of_sent(true), CloseDelivery::SentHere);
        assert_eq!(
            CloseDelivery::of_sent(false),
            CloseDelivery::SentFromAnotherThread
        );
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
    /// Folio's names its class on one bounded line and says nothing closed.**
    #[test]
    fn a_close_to_a_window_that_is_not_folios_says_its_class_on_one_line() {
        let line = close_arrival_line(
            Some("Winit Thread Event Target"),
            CloseRoad::CloseMessage,
            CloseDelivery::Posted,
            false,
        );
        assert_eq!(
            line,
            "Folio: close message to a window that is not Folio's — class \"Winit Thread Event \
             Target\"; road=close-message; delivery=posted; foreground=no; Folio's windows stay \
             open"
        );
        let long = format!("planted\nclass{}", "x".repeat(300));
        let line = close_arrival_line(
            Some(&long),
            CloseRoad::CloseMessage,
            CloseDelivery::SentFromAnotherThread,
            true,
        );
        assert!(!line.contains(['\r', '\n']), "{line:?}");
        assert!(line.contains(&format!("class \"planted class{}\"", "x".repeat(51))));
        assert!(!line.contains(&"x".repeat(52)), "{line}");
        assert!(
            close_arrival_line(None, CloseRoad::CloseMessage, CloseDelivery::Posted, false)
                .contains("— the window thread (no window); road=close-message")
        );
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
