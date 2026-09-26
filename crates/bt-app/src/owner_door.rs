//! **The window's own native calls that the window thread waits on, each an owner-thread door**
//! (design note `docs/plans/design/thread-door-2026-09-26.md`, revisions (d)3 and (e)2).
//!
//! winit's `Window` methods are foreign, so each one the registry names gets one small door
//! here, wrapping the one call and taking that door's [`WaitToken`] by value. The token is minted
//! by `admitted::<doors::X>` at the statement that used to make the call, and a refusal is handled
//! there, before anything the call would have changed. These calls stay on the window thread by
//! §5.2 (native affinity); what the door adds is that they happen only there, only in their
//! phases, and measured.

use bt_platform::admission::{WaitToken, doors};
use winit::dpi::{Position, Size};
use winit::window::{Cursor, Window};

/// `Window::set_title` — row 14's residue, written only by `Runtime::flush_title`.
pub(crate) fn set_title(token: WaitToken<'_, doors::TitleFlush>, window: &Window, title: &str) {
    let _ = token;
    window.set_title(title);
}

/// `Window::set_ime_cursor_area` — row 22's residue, told only by
/// `Runtime::apply_ime_cursor_area`.
pub(crate) fn set_ime_cursor_area(
    token: WaitToken<'_, doors::ImeCaretArea>,
    window: &Window,
    position: Position,
    size: Size,
) {
    let _ = token;
    window.set_ime_cursor_area(position, size);
}

/// `Window::focus_window` (§5.2), asked only by `Runtime::open_from_notification`.
pub(crate) fn focus_window(token: WaitToken<'_, doors::FocusWindow>, window: &Window) {
    let _ = token;
    window.focus_window();
}

/// `Window::set_visible` (§5.2): shown by `Runtime::put_the_window_on_the_glass`, hidden by
/// `Runtime::hide_quake_window` and `Runtime::let_go_of_this_window`.
pub(crate) fn set_visible(token: WaitToken<'_, doors::SetVisible>, window: &Window, visible: bool) {
    let _ = token;
    window.set_visible(visible);
}

/// `Window::set_cursor` (§5.2), set only by `Runtime::apply_pointer_cursor`.
pub(crate) fn set_cursor(token: WaitToken<'_, doors::SetCursor>, window: &Window, cursor: Cursor) {
    let _ = token;
    window.set_cursor(cursor);
}
