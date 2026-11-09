//! **The program's icon on the system's message box** (0.4.10 plan row 72,
//! T-MESSAGE-BOX-WEARS-THE-ICON).
//!
//! `MessageBoxTimeoutW` makes a window of the dialog class `#32770`, which has no class icon,
//! and sends it no `WM_SETICON`: the shell then draws the box's Alt+Tab entry with no icon and
//! its taskbar button with a generic window glyph (the owner looked on 0.4.9, 2026-11-05). A box
//! that has a Folio window on the screen to be owned by is owned by it and has no entry of its
//! own; every other box — a launch about to exit, a road process's failure window, the panic
//! hook's alert — is the process's only window, and wears the program's icon itself.
//!
//! **How it gets it.** The box is the system's; its window exists only inside the call. So for
//! exactly as long as the call is up this thread holds a `WH_CBT` hook ([`DressedBox`]). The
//! first `#32770` window *made* on this thread while the hook is held is the box this call
//! raises (`HCBT_CREATEWND`); a dialog that stood before the call is not it. When that window is
//! activated (`HCBT_ACTIVATE`) the hook loads the executable's icon group at the scale of the
//! display the box is on ([`ProgramIcons`]) and sends both sides as the window's own icons. The
//! box keeps its layout, its text, its OK in the reader's language and its fifteen-minute bound.
//! The icons are destroyed after the call has returned, when the window they were given to is
//! gone.
//!
//! **The display's own scale, in every process.** A box raised before any window of Folio's —
//! the front door, the uninstall door, the update's appliers and the recovery build, the
//! remover, a panic before the first window — is raised by a process that is not yet DPI aware
//! (the executable has no manifest; winit's event loop makes the process aware), where
//! `GetDpiForWindow` answers 96 on every display. The display's DPI is therefore read with this
//! thread switched to per-monitor awareness for the one read and switched back
//! ([`display_dpi_of`]); in an aware process it is the answer `GetDpiForWindow` gives.
//!
//! **Loaded from this module, not from the file**: the image the process is running, so the
//! icon is the right one after an update has replaced the file on disk, and in the uninstall
//! remover, which is a copy of the executable. A load the module refuses leaves the box as it
//! was; a last-resort box has nowhere it could say so. So does a thread whose storage is being
//! torn down: the dresser is reached with `LocalKey::try_with`, and a box raised from a
//! thread-local's destructor is raised undressed rather than turned into an abort.
//!
//! **Every call here is one the call graph follows** (`bt-source`'s native-labels guard walks
//! what `message_box` reaches): a method of the standard library is called by its path
//! (`Result::ok(…)`, `LocalKey::try_with(&…)`), because a bare `.ok()` or `.with(…)` is ambiguous
//! with the package's own methods of those names, and the walk would follow every one of them.

use std::cell::RefCell;
use std::ffi::c_void;
use std::thread::LocalKey;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
    MDT_EFFECTIVE_DPI, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DestroyIcon, GetClassNameW, HCBT_ACTIVATE, HCBT_CREATEWND, HHOOK, HICON,
    ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SendMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_CBT, WM_SETICON,
};
use windows::core::PCWSTR;

/// **The program's icon at the shell's two sides for one display** — the executable's own icon
/// group ([`bt_winres::APP_ICON_GROUP`]) at [`crate::program_icon_sides`], each side an icon
/// this value owns and destroys.
pub struct ProgramIcons {
    small: HICON,
    big: HICON,
}

impl ProgramIcons {
    /// **Both sides at `dpi`**, loaded from the module this process is running; `None` when the
    /// module refuses either.
    #[must_use]
    pub fn at_dpi(dpi: u32) -> Option<Self> {
        let (small_side, big_side) = crate::program_icon_sides(f64::from(dpi) / 96.0);
        let small = icon_of_this_module(small_side)?;
        let Some(big) = icon_of_this_module(big_side) else {
            // SAFETY: `small` was loaded above, is owned here and was given to nobody.
            let _ = unsafe { DestroyIcon(small) };
            return None;
        };
        Some(Self { small, big })
    }
}

impl Drop for ProgramIcons {
    fn drop(&mut self) {
        // SAFETY: both icons were made for this value alone (`LoadImageW` without
        // `LR_SHARED`), and nothing holds either once the window they were sent to is gone.
        unsafe {
            let _ = DestroyIcon(self.small);
            let _ = DestroyIcon(self.big);
        }
    }
}

/// One side of the program's icon group, from the module this process is running.
fn icon_of_this_module(side: u32) -> Option<HICON> {
    let side = Result::ok(i32::try_from(side))?;
    // SAFETY: no name, so the call answers the executable's own module, which lives as long as
    // the process.
    let module = Result::ok(unsafe { GetModuleHandleW(PCWSTR::null()) })?;
    // `MAKEINTRESOURCEW`: a resource named by its number rather than by a string.
    let group = PCWSTR(std::ptr::without_provenance(usize::from(
        bt_winres::APP_ICON_GROUP,
    )));
    // SAFETY: a live module, a resource number, and a size; the handle is owned by the caller.
    let image = Result::ok(unsafe {
        LoadImageW(
            Some(HINSTANCE::from(module)),
            group,
            IMAGE_ICON,
            side,
            side,
            LR_DEFAULTCOLOR,
        )
    })?;
    Some(HICON(image.0))
}

/// **The effective DPI of the display `window` is on**, read with this thread per-monitor aware
/// for the one read and then put back as it was: what an aware process's `GetDpiForWindow`
/// answers, and the display's own scale in a process that is not aware yet. 96 when the system
/// will not say.
fn display_dpi_of(window: HWND) -> u32 {
    let mut dpi_x = 0_u32;
    let mut dpi_y = 0_u32;
    // SAFETY: the awareness context is this thread's, set for the two reads below and put back
    // before the block ends (a refused switch answers an invalid context, and nothing is put
    // back); `MonitorFromWindow` with `MONITOR_DEFAULTTONEAREST` always answers a monitor; both
    // out-parameters are exclusively borrowed for the call.
    let read = unsafe {
        let before = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        let read = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        if !DPI_AWARENESS_CONTEXT::is_invalid(&before) {
            let _ = SetThreadDpiAwarenessContext(before);
        }
        read
    };
    match read {
        Ok(()) if dpi_x > 0 => dpi_x,
        _ => 96,
    }
}

/// **The dressing of one box**: the box once it is made, then the icons it wears.
struct BoxDresser {
    the_box: Option<HWND>,
    worn: Option<ProgramIcons>,
}

impl BoxDresser {
    /// A dresser waiting for its box.
    const fn waiting() -> Self {
        Self {
            the_box: None,
            worn: None,
        }
    }

    /// **A window is being made on this thread**: the first `#32770` window made while the
    /// dresser waits is the box this call raises. Every later window, and any dialog that stood
    /// before the call, is not.
    fn note_a_new_window(&mut self, window: HWND) {
        if Option::is_none(&self.the_box) && is_a_dialog_box(window) {
            self.the_box = Some(window);
        }
    }

    /// **The DPI the box is to be dressed at, when `window` is it**: the box this call made,
    /// activated before it wears the icon. Every other window, and the box once it is dressed,
    /// answers `None`.
    fn the_box_at(&self, window: HWND) -> Option<u32> {
        if Option::is_some(&self.worn) || self.the_box != Some(window) {
            return None;
        }
        Some(display_dpi_of(window))
    }

    /// **The box puts the program's icon on**: both sides sent as its window's own icons, and
    /// kept alive by this dresser until the box is gone.
    fn put_on(&mut self, window: HWND, icons: ProgramIcons) {
        // SAFETY: a live window, and two icons this dresser keeps alive until the window is gone.
        unsafe {
            SendMessageW(
                window,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(icons.small.0 as isize)),
            );
            SendMessageW(
                window,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(icons.big.0 as isize)),
            );
        }
        self.worn = Some(icons);
    }
}

/// Whether `window` is of the dialog class every message box is made of.
fn is_a_dialog_box(window: HWND) -> bool {
    let mut name = [0u16; 8];
    // SAFETY: the buffer outlives the call, which writes at most its length.
    let length = unsafe { GetClassNameW(window, &mut name) };
    let Ok(length) = usize::try_from(length) else {
        return false;
    };
    name.get(..length)
        .is_some_and(|class| Iterator::eq(class.iter().copied(), "#32770".encode_utf16()))
}

thread_local! {
    /// The dresser of the box this thread is raising, while it is up; the one a nested box put
    /// aside is held by that box's [`DressedBox`].
    static DRESSER: RefCell<Option<BoxDresser>> = const { RefCell::new(None) };
}

/// **This thread's `WH_CBT` hook, held around one message box** — installed by
/// [`DressedBox::around`] before the call, removed by its drop after the call has returned, and
/// then the icons it gave the box destroyed. A box raised while another is up on this thread
/// (a panic in a window procedure the first box's loop dispatched) gets a dresser of its own and
/// gives the first one back when it ends.
pub(crate) struct DressedBox {
    hook: Option<HHOOK>,
    put_aside: Option<BoxDresser>,
}

impl DressedBox {
    /// The hook for the box about to be raised on this thread, dressing it in the program's icon
    /// ([`ProgramIcons::at_dpi`]). A hook Windows refuses, or a thread whose storage is being torn
    /// down, leaves the box undressed.
    pub(crate) fn around() -> Self {
        let Ok(put_aside) = LocalKey::try_with(&DRESSER, |slot| {
            RefCell::replace(slot, Some(BoxDresser::waiting()))
        }) else {
            return Self {
                hook: None,
                put_aside: None,
            };
        };
        // SAFETY: a read with no arguments.
        let thread = unsafe { GetCurrentThreadId() };
        // SAFETY: `box_hook` is an `extern "system"` function with the `HOOKPROC` signature that
        // lives for the whole program; no module, because the hook is this thread's.
        let hook = Result::ok(unsafe { SetWindowsHookExW(WH_CBT, Some(box_hook), None, thread) });
        Self { hook, put_aside }
    }
}

impl Drop for DressedBox {
    fn drop(&mut self) {
        if let Some(hook) = Option::take(&mut self.hook) {
            // SAFETY: the hook this value installed, removed once.
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        }
        let put_aside = Option::take(&mut self.put_aside);
        let this_box = LocalKey::try_with(&DRESSER, |slot| RefCell::replace(slot, put_aside));
        drop(this_box);
    }
}

/// `WH_CBT`: something is about to happen to a window of this thread. A window being made is
/// shown to the box's dresser, which keeps the first dialog as the box; that box's activation
/// dresses it in the program's icon at its display's scale. Every code is passed on, as every
/// hook procedure must, and nothing here can panic across the system's frame.
unsafe extern "system" fn box_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match u32::try_from(code) {
        Ok(HCBT_CREATEWND) => {
            let window = HWND(wparam.0 as *mut c_void);
            let _ = LocalKey::try_with(&DRESSER, |slot| {
                if let Ok(mut dresser) = RefCell::try_borrow_mut(slot)
                    && let Some(dresser) = Option::as_mut(&mut *dresser)
                {
                    dresser.note_a_new_window(window);
                }
            });
        }
        Ok(HCBT_ACTIVATE) => {
            let window = HWND(wparam.0 as *mut c_void);
            let _ = LocalKey::try_with(&DRESSER, |slot| {
                if let Ok(mut dresser) = RefCell::try_borrow_mut(slot)
                    && let Some(dresser) = Option::as_mut(&mut *dresser)
                    && let Some(dpi) = dresser.the_box_at(window)
                    && let Some(icons) = ProgramIcons::at_dpi(dpi)
                {
                    dresser.put_on(window, icons);
                }
            });
        }
        _ => {}
    }
    // SAFETY: the arguments are the ones this procedure was called with.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::HiDpi::{
        AreDpiAwarenessContextsEqual, GetDpiForWindow, GetThreadDpiAwarenessContext,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIcon, CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WM_GETICON, WS_POPUP,
    };
    use windows::core::w;

    use super::*;
    use crate::source_test;
    use bt_source::ItemQuery;

    /// Two icons drawn here, standing in for the executable's group: this test executable carries
    /// no icon resource (`bt-app`'s build links `folio.res` into its own bins only; the real
    /// load is `bt-app`'s `the_box_loads_the_programs_icon_at_every_scale_the_icon_file_was_drawn_for`).
    fn two_drawn_icons() -> ProgramIcons {
        let and_mask = [0u8; 32];
        let colour = [0xffu8; 32];
        // SAFETY: 16 × 16 at one bit per pixel is 32 bytes a plane, which both buffers are.
        let drawn = || {
            unsafe { CreateIcon(None, 16, 16, 1, 1, and_mask.as_ptr(), colour.as_ptr()) }
                .expect("an icon drawn from two masks")
        };
        ProgramIcons {
            small: drawn(),
            big: drawn(),
        }
    }

    /// A window of `class` that is never shown and never activated.
    fn hidden(class: PCWSTR) -> HWND {
        // SAFETY: a system class, no parent, no menu, no creation data; destroyed by the test.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("row 72"),
                WS_POPUP,
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
        }
        .expect("a hidden window of a system class")
    }

    /// The window's own small and big icons, as the shell reads them.
    fn icons_of(window: HWND) -> (isize, isize) {
        // SAFETY: a live window; `WM_GETICON` only reads.
        unsafe {
            (
                SendMessageW(window, WM_GETICON, Some(WPARAM(ICON_SMALL as usize)), None).0,
                SendMessageW(window, WM_GETICON, Some(WPARAM(ICON_BIG as usize)), None).0,
            )
        }
    }

    /// A dresser as the slot holds it: the box it noted, and the two icons it wears by handle.
    type SlotHolds = (Option<HWND>, Option<(isize, isize)>);

    /// What this thread's dresser slot holds; `None` when no box is being raised.
    fn the_slot() -> Option<SlotHolds> {
        DRESSER.with(|slot| {
            slot.borrow().as_ref().map(|dresser| {
                (
                    dresser.the_box,
                    dresser
                        .worn
                        .as_ref()
                        .map(|icons| (icons.small.0 as isize, icons.big.0 as isize)),
                )
            })
        })
    }

    /// PIN (0.4.10 plan row 71, moved here with the rule by row 72) — **the shell's two sides at
    /// the four scales `folio.ico` was drawn for are the system's own** (`SM_CXSMICON` and
    /// `SM_CXICON` at 96, 120, 144 and 192 DPI), each an image the file holds.
    #[test]
    fn the_two_sides_are_the_systems_at_every_scale_the_icon_file_was_drawn_for() {
        assert_eq!(
            [1.0, 1.25, 1.5, 2.0].map(crate::program_icon_sides),
            [(16, 32), (20, 40), (24, 48), (32, 64)]
        );
    }

    /// RED (0.4.10 plan row 72, review N4) — **the box wears the program's icon, at its display's
    /// scale, and nothing else does**: the first `#32770` window made while the dresser waits is
    /// the box, dressed when it is activated at the DPI of the display it is on, with both sides.
    /// A dialog that stood before the call, a window of another class, and a second `#32770` made
    /// after the box are no box and keep no icon. No window is shown: the creations and the
    /// activations are handed to the dresser as the hook hands them.
    ///
    /// MUTATION (`plant-72-c-the-box-gets-no-big-icon.py`): drop the `ICON_BIG` send, and the
    /// box's big icon is empty.
    #[test]
    fn the_box_wears_the_programs_icon_and_nothing_else_does() {
        let older = hidden(w!("#32770"));
        let mut dresser = BoxDresser::waiting();
        let other = hidden(w!("STATIC"));
        dresser.note_a_new_window(other);
        let the_box = hidden(w!("#32770"));
        dresser.note_a_new_window(the_box);
        let another_box = hidden(w!("#32770"));
        dresser.note_a_new_window(another_box);

        assert_eq!(
            dresser.the_box_at(older),
            None,
            "a dialog that stood before the call is not the box"
        );
        assert_eq!(
            dresser.the_box_at(other),
            None,
            "a window that is no box is not dressed"
        );
        assert_eq!(
            dresser.the_box_at(the_box),
            Some(display_dpi_of(the_box)),
            "the box is dressed at its display's scale"
        );
        dresser.put_on(the_box, two_drawn_icons());
        let worn = dresser.worn.as_ref().expect("the box was dressed");
        assert_eq!(
            icons_of(the_box),
            (worn.small.0 as isize, worn.big.0 as isize),
            "the box wears both sides as its own icons"
        );
        assert_eq!(
            dresser.the_box_at(another_box),
            None,
            "one box is dressed per call, the first made"
        );
        assert_eq!(dresser.the_box_at(the_box), None, "and dressed once");
        for window in [older, other, another_box] {
            assert_eq!(icons_of(window), (0, 0), "every other window keeps no icon");
        }

        for window in [older, other, the_box, another_box] {
            // SAFETY: windows this test made.
            let _ = unsafe { DestroyWindow(window) };
        }
    }

    /// RED (0.4.10 plan row 72, review N3; re-review R1) — **the display's scale is read aware,
    /// and the thread is left as it was**: `display_dpi_of` switches this thread to per-monitor
    /// awareness before it reads the monitor's DPI and puts the thread's own context back after
    /// it, and the box is dressed at what it answers. Read from this test's thread — the
    /// executable has no manifest, so the thread is the unaware one a box before Folio's first
    /// window is raised from, where an unaware read answers 96 on every display — the answer is
    /// the DPI a per-monitor-aware window on that display has, and the thread's awareness context
    /// after the read is the one before it.
    ///
    /// MUTATION (`plant-72-g-the-scale-read-unaware.py`): drop the switch, and the order is
    /// refused (and on a scaled display the answer is 96, not the display's DPI).
    #[test]
    fn the_displays_scale_is_read_aware_and_the_thread_is_left_as_it_was() {
        let read =
            source_test::body(&ItemQuery::function("display_dpi_of").in_module("crate::box_icon"));
        let switched = read
            .find("SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)")
            .expect("the thread is switched to per-monitor awareness");
        let asked = read
            .find("GetDpiForMonitor(")
            .expect("the monitor's DPI is read");
        let put_back = read
            .find("SetThreadDpiAwarenessContext(before)")
            .expect("the thread's own context is put back");
        assert!(
            switched < asked && asked < put_back,
            "the thread is switched before the read and put back after it:\n{read}"
        );
        let dressed_at = source_test::body(
            &ItemQuery::method("BoxDresser", "the_box_at").in_module("crate::box_icon"),
        );
        assert!(
            dressed_at.contains("display_dpi_of(window)"),
            "the box is dressed at the display's scale read aware:\n{dressed_at}"
        );

        // A window made per-monitor aware knows its display's DPI whatever the thread reading it.
        // SAFETY: this thread's own context, switched for the one creation and put back.
        let window = unsafe {
            let own = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let window = hidden(w!("#32770"));
            let _ = SetThreadDpiAwarenessContext(own);
            window
        };
        // SAFETY: a live window.
        let displays = unsafe { GetDpiForWindow(window) };
        // SAFETY: reads of this thread's own context.
        let before = unsafe { GetThreadDpiAwarenessContext() };
        let dpi = display_dpi_of(window);
        // SAFETY: as above.
        let after = unsafe { GetThreadDpiAwarenessContext() };
        // SAFETY: two contexts this thread was answered.
        let same = unsafe { AreDpiAwarenessContextsEqual(before, after) };
        assert!(same.as_bool(), "the thread's awareness is put back");
        assert_eq!(dpi, displays, "the display's DPI, read aware");
        // SAFETY: a window this test made.
        let _ = unsafe { DestroyWindow(window) };
    }

    /// RED (0.4.10 plan row 72, review N4) — **a box raised inside another puts the first one's
    /// dressing aside and gives it back**: the nested box's hook gets a waiting dresser of its
    /// own, the first box's icons are kept while it is up and are back in the slot when it ends,
    /// and the slot is empty once the first box has ended too. No window is needed.
    #[test]
    fn a_box_inside_a_box_puts_the_first_ones_dressing_aside_and_gives_it_back() {
        assert_eq!(the_slot(), None, "no box is being raised");
        let first = DressedBox::around();
        assert_eq!(
            the_slot(),
            Some((None, None)),
            "the first box's dresser waits"
        );
        let icons = two_drawn_icons();
        let worn = (icons.small.0 as isize, icons.big.0 as isize);
        DRESSER.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .expect("the first box's dresser")
                .worn = Some(icons);
        });
        let second = DressedBox::around();
        assert_eq!(
            the_slot(),
            Some((None, None)),
            "the nested box gets a dresser of its own"
        );
        drop(second);
        assert_eq!(
            the_slot(),
            Some((None, Some(worn))),
            "and gives the first one back, still wearing its icons"
        );
        drop(first);
        assert_eq!(
            the_slot(),
            None,
            "and the slot is empty after the first box"
        );
    }

    /// RED (0.4.10 plan row 72) — **the hook is held for exactly as long as the box is up, and
    /// dresses the box it made**: `windows_impl::message_box` takes it before
    /// `MessageBoxTimeoutW` and lets it go after the call has returned, so the box's creation and
    /// first activation are seen and no window of this thread's is dressed after it; and the hook
    /// procedure notes the windows made, and loads the program's icon at the DPI the dresser
    /// answers and puts it on.
    ///
    /// MUTATION (`plant-72-d-the-hook-after-the-box.py`): take the hook after the call, and the
    /// order is refused.
    #[test]
    fn the_hook_is_held_around_the_box_and_only_around_it() {
        let body =
            source_test::body(&ItemQuery::function("message_box").in_module("crate::windows_impl"));
        let taken = body
            .find("let dressed = crate::box_icon::DressedBox::around();")
            .expect("the box's hook is taken");
        // The last mention is the call; the first is the function's `extern` declaration.
        let raised = body
            .rfind("MessageBoxTimeoutW(")
            .expect("the box is raised");
        let let_go = body
            .find("drop(dressed);")
            .expect("the box's hook is let go");
        assert!(
            taken < raised && raised < let_go,
            "the hook is taken before the box is raised and let go after it returns:\n{body}"
        );
        let hook = source_test::body(&ItemQuery::function("box_hook").in_module("crate::box_icon"));
        for step in [
            "Ok(HCBT_CREATEWND) => {",
            "dresser.note_a_new_window(window);",
            "Ok(HCBT_ACTIVATE) => {",
            "&& let Some(dpi) = dresser.the_box_at(window)",
            "&& let Some(icons) = ProgramIcons::at_dpi(dpi)",
            "dresser.put_on(window, icons);",
        ] {
            assert!(
                hook.contains(step),
                "the hook procedure dresses the box it made (`{step}`):\n{hook}"
            );
        }
    }
}
