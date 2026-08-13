//! Audited native bridges that are not exposed by winit's safe APIs.

use std::num::NonZeroIsize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WheelScrollAmount {
    Lines(u32),
    Page,
}

/// Geometry consumed by the pure half of the Win32 `WM_NCHITTEST` bridge.
/// All values are physical pixels; callers derive them from the live window DPI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustomFrameMetrics {
    pub width: i32,
    pub height: i32,
    pub title_bar_height: i32,
    pub tab_strip_right_px: i32,
    pub caption_button_width: i32,
    pub caption_button_count: i32,
    pub resize_border: i32,
    pub resizable: bool,
}

/// Win32 non-client regions expressed without Win32 constants so their mapping
/// can be pinned on every host used by the workspace tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustomFrameHit {
    Client,
    Caption,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Map one client-coordinate point to the native non-client region it represents.
/// Resize edges win over the title bar, and the complete settings/caption-button
/// run stays `Client` so the application can paint and handle those buttons.
#[must_use]
pub fn custom_frame_hit_test(metrics: CustomFrameMetrics, x: i32, y: i32) -> CustomFrameHit {
    let border = metrics.resize_border.max(0);
    let left = metrics.resizable && x >= 0 && x < border;
    let right = metrics.resizable && x >= metrics.width.saturating_sub(border) && x < metrics.width;
    let top = metrics.resizable && y >= 0 && y < border;
    let bottom =
        metrics.resizable && y >= metrics.height.saturating_sub(border) && y < metrics.height;

    match (left, right, top, bottom) {
        (true, _, true, _) => return CustomFrameHit::TopLeft,
        (_, true, true, _) => return CustomFrameHit::TopRight,
        (true, _, _, true) => return CustomFrameHit::BottomLeft,
        (_, true, _, true) => return CustomFrameHit::BottomRight,
        (true, _, _, _) => return CustomFrameHit::Left,
        (_, true, _, _) => return CustomFrameHit::Right,
        (_, _, true, _) => return CustomFrameHit::Top,
        (_, _, _, true) => return CustomFrameHit::Bottom,
        _ => {}
    }

    let buttons_width = metrics
        .caption_button_width
        .max(0)
        .saturating_mul(metrics.caption_button_count.max(0));
    let buttons_left = metrics.width.saturating_sub(buttons_width);
    if y < border || y >= metrics.title_bar_height.max(border) {
        return CustomFrameHit::Client;
    }
    if x < metrics.tab_strip_right_px.max(0) {
        return CustomFrameHit::Client;
    }
    if x < buttons_left || x >= metrics.width {
        CustomFrameHit::Caption
    } else {
        CustomFrameHit::Client
    }
}

/// Scale a logical chrome measurement using Win32's 96-DPI baseline.
#[must_use]
pub fn logical_px_for_dpi(logical_px: u32, dpi: u32) -> i32 {
    let scaled = u64::from(logical_px)
        .saturating_mul(u64::from(dpi.max(1)))
        .saturating_add(48)
        / 96;
    scaled.min(i32::MAX as u64) as i32
}

/// The parameters `explorer.exe` is handed to **reveal** a path (user ruling,
/// 2026-08-13).
///
/// # 「Show me where this is」, not 「open the folder it is in」
///
/// Every foot in this window carries a path and offers to take you to it. What
/// that used to mean was `ShellExecute("open", <the parent folder>)` — Explorer
/// opened on a directory and the file the foot was actually naming was one of
/// two hundred rows, indistinguishable from the rest. `/select` is the verb
/// that means what the foot says: the folder opens **with that item
/// highlighted**.
///
/// A directory keeps the old answer, and that is a judgement rather than a
/// limitation: `/select` on a folder opens its *parent* with the folder
/// highlighted, which is one level further out than a foot pointing at a root
/// is offering. Looking *inside* it is the natural reading of a tree's own
/// root, so a directory is opened and a file is selected.
///
/// Pure, because the one thing that can be wrong here is the string — Explorer
/// parses `/select,<path>` as a single token and wants the path quoted, and a
/// command line that is a quote out is a command line that silently opens
/// `Documents` instead.
#[must_use]
pub fn reveal_arguments(path: &std::path::Path, is_directory: bool) -> std::ffi::OsString {
    let mut arguments = std::ffi::OsString::new();
    if !is_directory {
        arguments.push("/select,");
    }
    arguments.push("\"");
    arguments.push(path.as_os_str());
    arguments.push("\"");
    arguments
}

#[cfg(windows)]
mod windows_impl {
    use std::{
        ffi::c_void,
        os::windows::ffi::OsStrExt,
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex, OnceLock,
            atomic::{AtomicI32, Ordering},
        },
    };
    use windows::core::{HRESULT, PCWSTR};

    use windows::Win32::{
        Foundation::{
            COLORREF, ERROR_CANCELLED, GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM,
            LRESULT, POINT, RECT, RPC_E_CHANGED_MODE, SetLastError, WIN32_ERROR, WPARAM,
        },
        Graphics::Dwm::{
            DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
            DwmSetWindowAttribute,
        },
        Graphics::Gdi::{
            CreateSolidBrush, DeleteObject, GetMonitorInfoW, HGDIOBJ, MONITOR_DEFAULTTONEAREST,
            MONITORINFO, MonitorFromWindow,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
            },
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        },
        UI::{
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            Input::KeyboardAndMouse::GetKeyboardLayout,
            Shell::{
                DefSubclassProc, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
                FileOpenDialog, IFileOpenDialog, IShellItem, RemoveWindowSubclass,
                SHCreateItemFromParsingName, SIGDN_FILESYSPATH, SetWindowSubclass, ShellExecuteW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateCaret, CreatePopupMenu, DestroyCaret, DestroyMenu,
                GCLP_HBRBACKGROUND, GetClientRect, GetCursorPos, GetWindowRect, HTBOTTOM,
                HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP,
                HTTOPLEFT, HTTOPRIGHT, IsIconic, IsZoomed, MF_STRING, MINMAXINFO,
                NCCALCSIZE_PARAMS, PostMessageW, SM_CXFRAME, SM_CXPADDEDBORDER,
                SPI_GETCLIENTAREAANIMATION, SPI_GETWHEELSCROLLLINES, SW_SHOWNORMAL,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SetCaretPos, SetClassLongPtrW, SetWindowPos, SystemParametersInfoW, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CLOSE, WM_GETMINMAXINFO, WM_NCCALCSIZE,
                WM_NCHITTEST,
            },
        },
    };

    use super::{
        CustomFrameHit, CustomFrameMetrics, NonZeroIsize, WheelScrollAmount, WindowRect,
        custom_frame_hit_test, logical_px_for_dpi,
    };

    /// GDI brush currently owned by this process and installed on winit's shared window class.
    /// The class itself outlives individual windows; theme switches replace this handle in place.
    static WINDOW_CLASS_BACKGROUND: OnceLock<Mutex<Option<isize>>> = OnceLock::new();
    const CF_UNICODETEXT: u32 = 13;
    const CLIPBOARD_OPEN_RETRY_DELAYS: [std::time::Duration; 4] = [
        std::time::Duration::from_millis(5),
        std::time::Duration::from_millis(10),
        std::time::Duration::from_millis(20),
        std::time::Duration::from_millis(40),
    ];
    const WHEEL_PAGESCROLL: u32 = u32::MAX;
    const DEFERRED_MATH_MENU_MESSAGE: u32 = WM_APP + 0x4b7;
    const DEFERRED_FOLDER_PICKER_MESSAGE: u32 = WM_APP + 0x4b8;
    const MATH_MENU_SUBCLASS_ID: usize = 0x4254_4d4d;
    const FOLDER_PICKER_SUBCLASS_ID: usize = 0x4254_4650;
    const CUSTOM_FRAME_SUBCLASS_ID: usize = 0x4254_4346;
    const TITLE_BAR_LOGICAL_PX: u32 = 40;
    const CAPTION_BUTTON_LOGICAL_PX: u32 = 46;
    const CAPTION_BUTTON_COUNT: i32 = 4;

    /// Keeps winit's ordinary overlapped-window styles (and therefore native
    /// snap, resize borders, minimize animation and system-menu semantics) while
    /// extending the client area through the system caption.
    pub struct CustomWindowFrame {
        hwnd: HWND,
        state: Box<CustomFrameState>,
    }

    /// Everything the subclass procedure reads out of the owning
    /// `CustomWindowFrame`. It is reached through the subclass reference data, so
    /// it must be a single stable allocation the frame keeps alive.
    #[derive(Default)]
    struct CustomFrameState {
        tab_strip_right_px: AtomicI32,
        /// Smallest client size the window may be dragged to, in logical pixels,
        /// or `(0, 0)` for "no minimum". Logical rather than physical so the
        /// constraint survives a DPI change without anyone recomputing it.
        min_client_logical_width: AtomicI32,
        min_client_logical_height: AtomicI32,
    }

    impl CustomWindowFrame {
        pub fn install(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Box::new(CustomFrameState::default());
            let reference_data = (&*state as *const CustomFrameState) as usize;
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(custom_frame_subclass),
                    CUSTOM_FRAME_SUBCLASS_ID,
                    reference_data,
                )
            };
            if !installed.as_bool() {
                return Err(format!(
                    "SetWindowSubclass(custom frame) failed: {}",
                    unsafe { GetLastError().0 }
                ));
            }
            // Re-run non-client calculation now that the subclass owns it. No
            // position, size or z-order changes are requested.
            if let Err(error) = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            } {
                let _ = unsafe {
                    RemoveWindowSubclass(
                        hwnd,
                        Some(custom_frame_subclass),
                        CUSTOM_FRAME_SUBCLASS_ID,
                    )
                };
                return Err(format!("SetWindowPos(SWP_FRAMECHANGED) failed: {error}"));
            }
            // Claiming the caption does not surrender the Windows 11 rounded
            // corners every ordinary window wears; state the preference
            // explicitly so DWM keeps clipping the frame like the system
            // default. Best-effort: Windows 10 has no such attribute and is
            // square everywhere, which is its default too.
            let preference = DWMWCP_ROUND;
            let _ = unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &preference as *const DWM_WINDOW_CORNER_PREFERENCE as *const c_void,
                    std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
                )
            };
            Ok(Self { hwnd, state })
        }

        pub fn set_tab_strip_right_px(&self, tab_strip_right_px: i32) {
            self.state
                .tab_strip_right_px
                .store(tab_strip_right_px.max(0), Ordering::Relaxed);
        }

        /// Constrain how far the window may be resized, in logical pixels of
        /// *client* area. `None` lifts the constraint.
        ///
        /// This is the self-drawn frame's replacement for winit's
        /// `set_min_inner_size`, and it exists because winit's setter cannot be
        /// used on a window whose `WM_NCCALCSIZE` has made the client area the
        /// entire outer rect: winit implements it by re-requesting the current
        /// inner size, and re-requesting runs `AdjustWindowRectExForDpi`, which
        /// adds a frame margin this window does not have. The window grew by that
        /// margin on every call. Owning `WM_GETMINMAXINFO` states the same
        /// constraint to the same OS with no size request at all.
        pub fn set_min_client_size(&self, logical: Option<(u32, u32)>) -> Result<(), String> {
            let (width, height) = logical.unwrap_or((0, 0));
            let width = i32::try_from(width).unwrap_or(i32::MAX);
            let height = i32::try_from(height).unwrap_or(i32::MAX);
            self.state
                .min_client_logical_width
                .store(width, Ordering::Relaxed);
            self.state
                .min_client_logical_height
                .store(height, Ordering::Relaxed);
            if width <= 0 || height <= 0 {
                return Ok(());
            }
            // `WM_GETMINMAXINFO` only governs future sizing, so a window that is
            // already smaller than a freshly raised minimum has to be grown here.
            // A maximized window is not user-resizable and its rect belongs to the
            // monitor, so it is left alone.
            // SAFETY: `self.hwnd` is the live window this frame is installed on.
            if unsafe { IsZoomed(self.hwnd) }.as_bool() {
                return Ok(());
            }
            // SAFETY: as above; both queries are read-only and `rect` is
            // exclusively borrowed for the call.
            let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
            let mut rect = RECT::default();
            unsafe { GetWindowRect(self.hwnd, &mut rect) }
                .map_err(|error| format!("GetWindowRect failed: {error}"))?;
            let current_width = rect.right.saturating_sub(rect.left);
            let current_height = rect.bottom.saturating_sub(rect.top);
            let target_width = current_width.max(logical_px_for_dpi(width as u32, dpi));
            let target_height = current_height.max(logical_px_for_dpi(height as u32, dpi));
            if target_width == current_width && target_height == current_height {
                return Ok(());
            }
            // SAFETY: `self.hwnd` is live; no insert-after handle is passed, so
            // the null `hwndinsertafter` is inert under `SWP_NOZORDER`.
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    target_width,
                    target_height,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            }
            .map_err(|error| format!("SetWindowPos(grow to minimum) failed: {error}"))
        }
    }

    impl Drop for CustomWindowFrame {
        fn drop(&mut self) {
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(custom_frame_subclass),
                    CUSTOM_FRAME_SUBCLASS_ID,
                )
            };
        }
    }

    unsafe extern "system" fn custom_frame_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        match message {
            WM_NCCALCSIZE => {
                // A zoomed overlapped window deliberately extends its outer
                // resize frame beyond the monitor. With the entire outer rect
                // made client, those pixels would clip our titlebar/content.
                // Keep that invisible native frame as a maximized inset while
                // still removing the ordinary system caption everywhere else.
                if wparam.0 != 0 && lparam.0 != 0 && unsafe { IsZoomed(hwnd) }.as_bool() {
                    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                    let border = native_resize_border(dpi);
                    let params = unsafe { &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS) };
                    params.rgrc[0].left = params.rgrc[0].left.saturating_add(border);
                    params.rgrc[0].top = params.rgrc[0].top.saturating_add(border);
                    params.rgrc[0].right = params.rgrc[0].right.saturating_sub(border);
                    params.rgrc[0].bottom = params.rgrc[0].bottom.saturating_sub(border);
                }
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                if lparam.0 == 0 {
                    return result;
                }
                // SAFETY: `CustomWindowFrame` owns this allocation and removes the subclass
                // before dropping it, so the reference-data pointer is live for every callback.
                let state = unsafe { &*(reference_data as *const CustomFrameState) };
                let width = state.min_client_logical_width.load(Ordering::Relaxed);
                let height = state.min_client_logical_height.load(Ordering::Relaxed);
                if width <= 0 || height <= 0 {
                    return result;
                }
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                // `ptMinTrackSize` is an *outer* rect bound. Under this frame the
                // client area is the outer rect, so a minimum client size is that
                // bound byte for byte, with no non-client margin to add.
                // SAFETY: for WM_GETMINMAXINFO the OS passes a live, writable
                // MINMAXINFO in `lparam`, checked non-null above.
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                info.ptMinTrackSize.x = logical_px_for_dpi(width as u32, dpi);
                info.ptMinTrackSize.y = logical_px_for_dpi(height as u32, dpi);
                result
            }
            WM_NCHITTEST => {
                let mut client = RECT::default();
                if unsafe { GetClientRect(hwnd, &mut client) }.is_err() {
                    return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                }
                let mut point = POINT {
                    x: low_word_signed(lparam.0),
                    y: high_word_signed(lparam.0),
                };
                if !unsafe { windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut point) }
                    .as_bool()
                {
                    return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                }
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                let resize_border = if unsafe { IsZoomed(hwnd) }.as_bool() {
                    0
                } else {
                    native_resize_border(dpi)
                };
                // SAFETY: `CustomWindowFrame` owns this allocation and removes the subclass
                // before dropping it, so the reference-data pointer is live for every callback.
                let tab_strip_right_px = unsafe { &*(reference_data as *const CustomFrameState) }
                    .tab_strip_right_px
                    .load(Ordering::Relaxed);
                let hit = custom_frame_hit_test(
                    CustomFrameMetrics {
                        width: client.right.saturating_sub(client.left),
                        height: client.bottom.saturating_sub(client.top),
                        title_bar_height: logical_px_for_dpi(TITLE_BAR_LOGICAL_PX, dpi),
                        tab_strip_right_px,
                        caption_button_width: logical_px_for_dpi(CAPTION_BUTTON_LOGICAL_PX, dpi),
                        caption_button_count: CAPTION_BUTTON_COUNT,
                        resize_border,
                        resizable: resize_border > 0,
                    },
                    point.x,
                    point.y,
                );
                LRESULT(match hit {
                    CustomFrameHit::Client => HTCLIENT,
                    CustomFrameHit::Caption => HTCAPTION,
                    CustomFrameHit::Left => HTLEFT,
                    CustomFrameHit::Right => HTRIGHT,
                    CustomFrameHit::Top => HTTOP,
                    CustomFrameHit::Bottom => HTBOTTOM,
                    CustomFrameHit::TopLeft => HTTOPLEFT,
                    CustomFrameHit::TopRight => HTTOPRIGHT,
                    CustomFrameHit::BottomLeft => HTBOTTOMLEFT,
                    CustomFrameHit::BottomRight => HTBOTTOMRIGHT,
                } as isize)
            }
            _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
        }
    }

    fn low_word_signed(value: isize) -> i32 {
        (value as u16 as i16) as i32
    }

    fn high_word_signed(value: isize) -> i32 {
        ((value as usize >> 16) as u16 as i16) as i32
    }

    fn native_resize_border(dpi: u32) -> i32 {
        unsafe {
            GetSystemMetricsForDpi(SM_CXFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
        }
        .max(1)
    }

    pub fn request_window_close(hwnd: NonZeroIsize) -> Result<(), String> {
        unsafe {
            PostMessageW(
                Some(HWND(hwnd.get() as *mut c_void)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
        }
        .map_err(|error| format!("PostMessageW(WM_CLOSE) failed: {error}"))
    }

    /// Ask Windows to open one already-policy-checked target with its registered default handler.
    /// Scheme allowlisting deliberately belongs to the caller; this bridge only supplies the
    /// audited UTF-16 and ShellExecuteW boundary.
    pub fn shell_execute(hwnd: NonZeroIsize, target: &str) -> Result<(), String> {
        if target.contains('\0') {
            return Err("ShellExecuteW target contains an embedded NUL".to_owned());
        }
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut target = target.encode_utf16().collect::<Vec<_>>();
        target.push(0);
        // SAFETY: both strings are live, NUL-terminated UTF-16 buffers for the duration of the
        // synchronous call. No parameters or working directory are supplied, so the URI is never
        // reparsed as a command line. `hwnd` is winit's live top-level window.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// Open one worker-validated local image with its registered default handler.
    ///
    /// The caller must obtain `path` from a successful image decode record, never directly from
    /// terminal text. This bridge independently enforces the slice's immutable syntax policy
    /// (drive-rooted, supported extension, no embedded NUL) and supplies no parameters or working
    /// directory, preventing command-line reinterpretation. It performs no event-thread file I/O.
    pub fn open_local_file(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        validate_local_image_path(path)?;
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut target = path.as_os_str().encode_wide().collect::<Vec<_>>();
        target.push(0);
        // SAFETY: the worker-success capability supplied an existing decoded image path, and both
        // buffers remain live and NUL-terminated for this synchronous call. Parameters and working
        // directory are null, so Windows never receives a command line to reparse.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// Open one file the user picked out of a directory listing with its
    /// registered default handler — and never run a program.
    ///
    /// **Why a second bridge rather than widening the first.** [`open_local_file`]
    /// serves paths *scraped out of terminal text*, where the only defence
    /// against a hostile line of output is that the syntax policy is narrow
    /// enough to be immutable. A row of the files tree has the opposite
    /// provenance: the user chose the root, this process enumerated the
    /// directory, and the user pressed the row. Making the two share one
    /// validator would mean either the tree can open nothing but pictures or
    /// terminal output can open anything.
    ///
    /// **Why programs are refused.** Not as a hedge — as the product rule
    /// `DESIGN.md` §7.1.3 already implies by making activation mean *open the
    /// preview*: the tree is a way of looking at files, and the thing next to it
    /// that runs programs is the terminal, where running one is a line you typed
    /// and can see. A double click that silently starts an executable is a verb
    /// this pane does not have, and the fact that the pane sits half an inch
    /// from a shell prompt is exactly why it should not acquire it by accident.
    pub fn open_local_path(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        validate_openable_path(path)?;
        if names_a_program(path, std::env::var("PATHEXT").unwrap_or_default().as_str()) {
            return Err(PROGRAM_REFUSED.to_owned());
        }
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut target = path.as_os_str().encode_wide().collect::<Vec<_>>();
        target.push(0);
        // SAFETY: the path was enumerated by this process from a directory the
        // user rooted a column at, both buffers stay live and NUL-terminated for
        // this synchronous call, and parameters and working directory are null,
        // so Windows never receives a command line to reparse.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// Open Explorer on a path, with a file **highlighted** inside its folder
    /// (user ruling, 2026-08-13).
    ///
    /// **A third bridge, and deliberately not a widening of the second.**
    /// [`open_local_path`] hands a path to *whatever the machine has registered
    /// for it* — which is why it reads `PATHEXT` and refuses programs, since
    /// the whole risk there is that opening a thing runs it. This one hands the
    /// path to `explorer.exe` **as text to look at**, and never executes the
    /// target at all: a `.exe` revealed is a `.exe` sitting highlighted in a
    /// folder window, which is precisely what somebody asking "where is this"
    /// wants to see and is not a way to start it. So the extension gate is
    /// absent on purpose, and the shape gate — absolute, nameable, no embedded
    /// NUL — is exactly the one its neighbour keeps.
    ///
    /// The one program this can ever launch is Explorer.
    pub fn reveal_in_explorer(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        validate_openable_path(path)?;
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut program = "explorer.exe".encode_utf16().collect::<Vec<_>>();
        program.push(0);
        let mut arguments = super::reveal_arguments(path, path.is_dir())
            .encode_wide()
            .collect::<Vec<_>>();
        arguments.push(0);
        // SAFETY: the target is a path this process enumerated or was given by
        // the user, `validate_openable_path` has refused any embedded NUL, all
        // three buffers stay live and NUL-terminated across this synchronous
        // call, and the only program named is Explorer.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(program.as_ptr()),
                PCWSTR(arguments.as_ptr()),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// The refusal's own words, so the caller can tell "this window will not do
    /// that" apart from "Windows could not".
    pub const PROGRAM_REFUSED: &str = "the files tree does not run programs";

    /// The extensions that are a program whatever this machine's `PATHEXT` says.
    ///
    /// `PATHEXT` is the system's own list of what a *command line* will execute
    /// and it is read as well, but it is not the whole answer: a `.lnk` is not
    /// on it and points at anything at all, a `.scr` is an executable wearing a
    /// screensaver's name, and `.hta`, `.reg`, `.msi` and `.url` are each opened
    /// by a handler whose whole job is to act. Reading both means the list grows
    /// with a machine that has added to `PATHEXT` without shrinking on one that
    /// has emptied it.
    const ALWAYS_A_PROGRAM: &[&str] = &[
        "appref-ms",
        "bat",
        "cmd",
        "com",
        "cpl",
        "exe",
        "hta",
        "jar",
        "js",
        "jse",
        "lnk",
        "msc",
        "msi",
        "msp",
        "ps1",
        "pif",
        "reg",
        "scf",
        "scr",
        "url",
        "vb",
        "vbe",
        "vbs",
        "wsf",
        "wsh",
    ];

    /// Whether opening this name would start something rather than show it.
    ///
    /// Split out and given `pathext` as an argument rather than reading the
    /// environment itself, so the rule is answerable in a test on a machine
    /// whose own `PATHEXT` is whatever it is.
    fn names_a_program(path: &Path, pathext: &str) -> bool {
        let Some(extension) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
        else {
            // No extension means no registered handler to speak of, and
            // `ShellExecute` falls back to the "open with" chooser rather than
            // to running anything. That is a dialog, not an execution.
            return false;
        };
        ALWAYS_A_PROGRAM.contains(&extension.as_str())
            || pathext.split(';').any(|entry| {
                entry
                    .trim()
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case(&extension)
            })
    }

    /// The shape gate the tree's own bridge keeps: absolute and nameable.
    ///
    /// Wider than [`validate_local_image_path`] in exactly one way — a UNC share
    /// is allowed — because a files column may legitimately be rooted at
    /// `\\server\share`, and a tree that can list a path it then refuses to open
    /// is a tree that lies about what its rows are.
    fn validate_openable_path(path: &Path) -> Result<(), String> {
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if units.contains(&0) {
            return Err("path contains an embedded NUL".to_owned());
        }
        let text = path.as_os_str().to_string_lossy();
        let bytes = text.as_bytes();
        let drive_rooted = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/');
        let unc = text.starts_with(r"\\") && text.len() > 2;
        if !drive_rooted && !unc {
            return Err("path must be absolute".to_owned());
        }
        Ok(())
    }

    fn validate_local_image_path(path: &Path) -> Result<(), String> {
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if units.contains(&0) {
            return Err("local image path contains an embedded NUL".to_owned());
        }
        let text = path.as_os_str().to_string_lossy();
        let bytes = text.as_bytes();
        if bytes.len() < 3
            || !bytes[0].is_ascii_alphabetic()
            || bytes[1] != b':'
            || !matches!(bytes[2], b'\\' | b'/')
        {
            return Err("local image path must be drive-rooted and absolute".to_owned());
        }
        let allowed_extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg"
                )
            });
        if !allowed_extension {
            return Err("local image path extension is not supported".to_owned());
        }
        Ok(())
    }

    pub fn wheel_scroll_amount() -> Result<WheelScrollAmount, String> {
        let mut lines = 0u32;
        // SAFETY: SPI_GETWHEELSCROLLLINES writes one u32 to the provided live stack pointer.
        unsafe {
            SystemParametersInfoW(
                SPI_GETWHEELSCROLLLINES,
                0,
                Some((&mut lines as *mut u32).cast()),
                Default::default(),
            )
        }
        .map_err(|error| {
            format!("SystemParametersInfoW(SPI_GETWHEELSCROLLLINES) failed: {error}")
        })?;
        Ok(if lines == WHEEL_PAGESCROLL {
            WheelScrollAmount::Page
        } else {
            WheelScrollAmount::Lines(lines)
        })
    }

    /// Whether the system wants animation inside a window's client area.
    ///
    /// `SPI_GETCLIENTAREAANIMATION` is Windows' own name for the preference a
    /// browser reports as `prefers-reduced-motion` — Settings → Accessibility →
    /// Visual effects → Animation effects. It is the setting the mock-up's
    /// `@media (prefers-reduced-motion: reduce)` blocks answer to, so reading it
    /// here is what makes those rules real on this platform rather than a
    /// stylesheet branch nothing ever takes.
    ///
    /// `TRUE` means animation is *wanted*, so the polarity is the opposite of
    /// the CSS query's — the caller does that mapping, and pins it.
    pub fn client_area_animation_enabled() -> Result<bool, String> {
        // `BOOL` is a 32-bit int across the Win32 ABI, and this is the shape
        // `SystemParametersInfoW` writes through the void pointer it is given.
        let mut enabled = 0_i32;
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the provided
        // live stack pointer, exactly as SPI_GETWHEELSCROLLLINES writes one u32.
        unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some((&mut enabled as *mut i32).cast()),
                Default::default(),
            )
        }
        .map_err(|error| {
            format!("SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION) failed: {error}")
        })?;
        Ok(enabled != 0)
    }

    fn retry_open_clipboard(
        mut open: impl FnMut() -> Result<(), String>,
        mut wait: impl FnMut(std::time::Duration),
    ) -> Result<(), String> {
        for delay in CLIPBOARD_OPEN_RETRY_DELAYS {
            match open() {
                Ok(()) => return Ok(()),
                Err(_) => wait(delay),
            }
        }
        open().map_err(|error| {
            format!(
                "OpenClipboard failed after {} attempts (retry wait capped at {} ms): {error}",
                CLIPBOARD_OPEN_RETRY_DELAYS.len() + 1,
                CLIPBOARD_OPEN_RETRY_DELAYS
                    .iter()
                    .map(std::time::Duration::as_millis)
                    .sum::<u128>()
            )
        })
    }

    fn open_clipboard_with_retry(hwnd: HWND) -> Result<(), String> {
        retry_open_clipboard(
            || {
                // SAFETY: the caller supplies winit's live HWND and all clipboard transactions run
                // on its event-loop thread. A failed open acquires no resource that needs cleanup.
                unsafe { OpenClipboard(Some(hwnd)) }.map_err(|error| error.to_string())
            },
            std::thread::sleep,
        )
    }

    pub fn clipboard_text(hwnd: NonZeroIsize) -> Result<String, String> {
        let hwnd = HWND(hwnd.get() as *mut c_void);
        // SAFETY: all calls run on winit's event-loop thread. The clipboard remains open while the
        // borrowed global-memory handle is locked, its UTF-16 content is copied, and then both the
        // memory and clipboard are released before returning.
        open_clipboard_with_retry(hwnd)?;
        unsafe {
            let result = (|| {
                IsClipboardFormatAvailable(CF_UNICODETEXT)
                    .map_err(|_| "clipboard has no Unicode text".to_owned())?;
                let handle = GetClipboardData(CF_UNICODETEXT)
                    .map_err(|error| format!("GetClipboardData(CF_UNICODETEXT) failed: {error}"))?;
                let global = HGLOBAL(handle.0);
                let byte_len = GlobalSize(global);
                if byte_len < size_of::<u16>() {
                    return Ok(String::new());
                }
                let pointer = GlobalLock(global).cast::<u16>();
                if pointer.is_null() {
                    return Err(format!(
                        "GlobalLock(clipboard text) failed: {}",
                        GetLastError().0
                    ));
                }
                let units = std::slice::from_raw_parts(pointer, byte_len / size_of::<u16>());
                let nul = units
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(units.len());
                let copied = units[..nul].to_vec();
                // GlobalUnlock returns false after releasing the final lock; GetLastError
                // distinguishes that successful case, so no recovery is attached to its Result.
                let _ = GlobalUnlock(global);
                String::from_utf16(&copied)
                    .map_err(|error| format!("clipboard text is invalid UTF-16: {error}"))
            })();
            let close = CloseClipboard().map_err(|error| format!("CloseClipboard failed: {error}"));
            match (result, close) {
                (Ok(text), Ok(())) => Ok(text),
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            }
        }
    }

    pub fn set_clipboard_text(hwnd: NonZeroIsize, text: &str) -> Result<(), String> {
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut units = text.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        // SAFETY: the event-loop thread owns the clipboard for this transaction. The movable
        // allocation is locked only while copying the NUL-terminated UTF-16 payload. After a
        // successful SetClipboardData Windows owns it; every earlier failure frees it locally.
        open_clipboard_with_retry(hwnd)?;
        unsafe {
            let result = (|| {
                EmptyClipboard().map_err(|error| format!("EmptyClipboard failed: {error}"))?;
                let byte_len = units.len() * size_of::<u16>();
                let global = GlobalAlloc(GMEM_MOVEABLE, byte_len)
                    .map_err(|error| format!("GlobalAlloc(clipboard text) failed: {error}"))?;
                let pointer = GlobalLock(global).cast::<u16>();
                if pointer.is_null() {
                    let error = GetLastError().0;
                    let _ = GlobalFree(Some(global));
                    return Err(format!("GlobalLock(clipboard text) failed: {error}"));
                }
                std::ptr::copy_nonoverlapping(units.as_ptr(), pointer, units.len());
                let _ = GlobalUnlock(global);
                if let Err(error) = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(global.0))) {
                    let _ = GlobalFree(Some(global));
                    return Err(format!("SetClipboardData(CF_UNICODETEXT) failed: {error}"));
                }
                Ok(())
            })();
            let close = CloseClipboard().map_err(|error| format!("CloseClipboard failed: {error}"));
            match (result, close) {
                (Ok(()), Ok(())) => Ok(()),
                (Err(error), _) | (Ok(()), Err(error)) => Err(error),
            }
        }
    }

    /// One deferred native gesture, from asked-for to answered.
    ///
    /// `Ask` is what the request carried and `Answer` is what it came back with.
    /// The request's own arguments ride in `Posted` rather than in a second
    /// field beside the phase, because they are only meaningful while the phase
    /// is `Posted`: a start folder left lying around after the dialog closed is
    /// a value with no owner and no expiry.
    #[derive(Debug)]
    enum DeferredPhase<Ask, Answer> {
        Idle,
        Posted(Ask),
        Showing,
        Complete(Answer),
    }

    /// The gate that keeps a nested native message pump out of a winit callback.
    ///
    /// Generic over both ends so the one state machine serves every gesture that
    /// has to be deferred this way — the formula menu, whose nested pump is
    /// `TrackPopupMenu`'s, and the folder picker, whose nested pump is
    /// `IFileDialog::Show`'s. Both have exactly the same hazard and therefore
    /// exactly the same shape: post a private message, do the blocking thing
    /// after the initiating callback has returned, and leave the answer where
    /// the next turn of the loop will find it.
    #[derive(Debug)]
    struct DeferredState<Ask, Answer> {
        phase: Mutex<DeferredPhase<Ask, Answer>>,
    }

    impl<Ask, Answer> DeferredState<Ask, Answer> {
        fn new() -> Self {
            Self {
                phase: Mutex::new(DeferredPhase::Idle),
            }
        }

        fn begin_request(&self, ask: Ask) -> bool {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(*phase, DeferredPhase::Idle) {
                return false;
            }
            *phase = DeferredPhase::Posted(ask);
            true
        }

        fn cancel_request(&self) {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if matches!(*phase, DeferredPhase::Posted(_)) {
                *phase = DeferredPhase::Idle;
            }
        }

        /// Take the gesture's arguments and move it to `Showing`, or `None` when
        /// this is not a posted request to begin.
        fn begin_showing(&self) -> Option<Ask> {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(*phase, DeferredPhase::Posted(_)) {
                return None;
            }
            let DeferredPhase::Posted(ask) = std::mem::replace(&mut *phase, DeferredPhase::Showing)
            else {
                unreachable!("phase was matched as posted immediately before replacement")
            };
            Some(ask)
        }

        fn complete(&self, answer: Answer) {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if matches!(*phase, DeferredPhase::Showing) {
                *phase = DeferredPhase::Complete(answer);
            }
        }

        fn take_result(&self) -> Option<Answer> {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            let DeferredPhase::Complete(_) = &*phase else {
                return None;
            };
            let DeferredPhase::Complete(answer) =
                std::mem::replace(&mut *phase, DeferredPhase::Idle)
            else {
                unreachable!("phase was matched as complete immediately before replacement")
            };
            Some(answer)
        }
    }

    type MathMenuState = DeferredState<(), Result<bool, String>>;

    /// Formula context-menu bridge whose nested native message pump never starts inside a winit
    /// application callback. `request` only posts a private window message. The subclass receives
    /// it after the current `DispatchMessageW`/winit callback has returned, so any RedrawRequested
    /// emitted by TrackPopupMenu's nested pump finds winit's event-handler slot restored.
    pub struct MathContextMenu {
        hwnd: HWND,
        state: Arc<MathMenuState>,
    }

    impl MathContextMenu {
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Arc::new(MathMenuState::new());
            // SAFETY: installation and removal occur on the HWND's event-loop thread. The Arc
            // keeps dwRefData live for the full installed interval; the callback takes its own
            // temporary strong reference before entering the nested menu loop.
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(math_context_menu_subclass),
                    MATH_MENU_SUBCLASS_ID,
                    Arc::as_ptr(&state) as usize,
                )
            };
            if !installed.as_bool() {
                return Err(format!("SetWindowSubclass(math menu) failed: {}", unsafe {
                    GetLastError().0
                }));
            }
            Ok(Self { hwnd, state })
        }

        /// Queue the native menu once. A second request while the first is posted, showing, or
        /// waiting to be consumed is an ordinary coalesced UI race and returns `Ok(false)`.
        pub fn request(&self) -> Result<bool, String> {
            if !self.state.begin_request(()) {
                return Ok(false);
            }
            // SAFETY: PostMessageW copies these value parameters into the owning thread's queue
            // and never dispatches the subclass synchronously on this callback stack.
            if let Err(error) = unsafe {
                PostMessageW(
                    Some(self.hwnd),
                    DEFERRED_MATH_MENU_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            } {
                self.state.cancel_request();
                return Err(format!("PostMessageW(math menu) failed: {error}"));
            }
            Ok(true)
        }

        pub fn take_result(&self) -> Option<Result<bool, String>> {
            self.state.take_result()
        }
    }

    impl Drop for MathContextMenu {
        fn drop(&mut self) {
            // SAFETY: this object is dropped on the same event-loop thread that installed the
            // subclass. A callback already inside TrackPopupMenu owns a temporary Arc, so nested
            // CloseRequested teardown cannot invalidate its state.
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(math_context_menu_subclass),
                    MATH_MENU_SUBCLASS_ID,
                )
            };
        }
    }

    unsafe extern "system" fn math_context_menu_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        if message != DEFERRED_MATH_MENU_MESSAGE {
            // SAFETY: forwarding untouched messages is the required subclass contract.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }
        let state_pointer = reference_data as *const MathMenuState;
        if state_pointer.is_null() {
            return LRESULT(0);
        }
        // SAFETY: the installed MathContextMenu owns one Arc at callback entry. Incrementing before
        // constructing the temporary Arc keeps state alive even if a nested CloseRequested drops
        // the Runtime while TrackPopupMenu is open.
        unsafe { Arc::increment_strong_count(state_pointer) };
        // SAFETY: the increment immediately above created the strong reference consumed here.
        let state = unsafe { Arc::from_raw(state_pointer) };
        if state.begin_showing().is_some() {
            state.complete(track_math_context_menu(hwnd));
        }
        LRESULT(0)
    }

    fn track_math_context_menu(hwnd: HWND) -> Result<bool, String> {
        // SAFETY: the HWND and menu belong to the current GUI thread. This function is reached only
        // from the posted-message subclass, after winit's initiating callback has returned.
        unsafe {
            let menu =
                CreatePopupMenu().map_err(|error| format!("CreatePopupMenu failed: {error}"))?;
            let result = (|| {
                AppendMenuW(menu, MF_STRING, 1, windows::core::w!("Copy LaTeX"))
                    .map_err(|error| format!("AppendMenuW(Copy LaTeX) failed: {error}"))?;
                let mut cursor = POINT::default();
                GetCursorPos(&mut cursor)
                    .map_err(|error| format!("GetCursorPos failed: {error}"))?;
                let command = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    cursor.x,
                    cursor.y,
                    None,
                    hwnd,
                    None,
                );
                Ok(command.0 as usize == 1)
            })();
            let destroy = DestroyMenu(menu).map_err(|error| format!("DestroyMenu failed: {error}"));
            match (result, destroy) {
                (Ok(selected), Ok(())) => Ok(selected),
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            }
        }
    }

    type FolderPickerState = DeferredState<Vec<u16>, Result<Option<PathBuf>, String>>;

    /// The system's own folder chooser, on the same deferred footing as
    /// [`MathContextMenu`] and for a sharper version of the same reason (E55).
    ///
    /// `IFileDialog::Show` is **modal**: it runs a nested message loop that goes
    /// on dispatching to this window for as long as the dialog is open. Started
    /// from inside a winit callback that would re-enter the application's own
    /// event handling while the `&mut` borrow that started it is still live —
    /// the exact hazard the formula menu's comment records, except that a menu
    /// closes in a moment and a folder chooser can stand open for a minute while
    /// somebody goes looking. So the press only *posts*: the dialog opens from
    /// the subclass, after the callback has returned, and the answer waits in
    /// this state until the next turn of the loop collects it.
    pub struct FolderPicker {
        hwnd: HWND,
        state: Arc<FolderPickerState>,
    }

    impl FolderPicker {
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Arc::new(FolderPickerState::new());
            // SAFETY: installation and removal occur on the HWND's event-loop thread. The Arc
            // keeps dwRefData live for the full installed interval; the callback takes its own
            // temporary strong reference before entering the nested dialog loop.
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(folder_picker_subclass),
                    FOLDER_PICKER_SUBCLASS_ID,
                    Arc::as_ptr(&state) as usize,
                )
            };
            if !installed.as_bool() {
                return Err(format!(
                    "SetWindowSubclass(folder picker) failed: {}",
                    unsafe { GetLastError().0 }
                ));
            }
            Ok(Self { hwnd, state })
        }

        /// Queue the chooser once, starting at `start` if that names a folder.
        ///
        /// A second request while one is posted, showing or waiting to be
        /// collected returns `Ok(false)` — the same coalescing the formula menu
        /// does, and here it is also what stops a second press on `Browse…`
        /// from stacking a second modal dialog behind the first.
        pub fn request(&self, start: Option<&Path>) -> Result<bool, String> {
            let start = start
                .map(|start| {
                    let mut units = start.as_os_str().encode_wide().collect::<Vec<_>>();
                    units.push(0);
                    units
                })
                .unwrap_or_default();
            if !self.state.begin_request(start) {
                return Ok(false);
            }
            // SAFETY: PostMessageW copies these value parameters into the owning thread's queue
            // and never dispatches the subclass synchronously on this callback stack.
            if let Err(error) = unsafe {
                PostMessageW(
                    Some(self.hwnd),
                    DEFERRED_FOLDER_PICKER_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            } {
                self.state.cancel_request();
                return Err(format!("PostMessageW(folder picker) failed: {error}"));
            }
            Ok(true)
        }

        /// The chosen folder, `None` for a cancelled dialog, or the reason the
        /// chooser could not be shown — once, and only once the dialog is shut.
        pub fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
            self.state.take_result()
        }
    }

    impl Drop for FolderPicker {
        fn drop(&mut self) {
            // SAFETY: this object is dropped on the same event-loop thread that installed the
            // subclass. A callback already inside the dialog owns a temporary Arc, so nested
            // CloseRequested teardown cannot invalidate its state.
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(folder_picker_subclass),
                    FOLDER_PICKER_SUBCLASS_ID,
                )
            };
        }
    }

    unsafe extern "system" fn folder_picker_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        if message != DEFERRED_FOLDER_PICKER_MESSAGE {
            // SAFETY: forwarding untouched messages is the required subclass contract.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }
        let state_pointer = reference_data as *const FolderPickerState;
        if state_pointer.is_null() {
            return LRESULT(0);
        }
        // SAFETY: the installed FolderPicker owns one Arc at callback entry. Incrementing before
        // constructing the temporary Arc keeps state alive even if a nested CloseRequested drops
        // the Runtime while the dialog is open.
        unsafe { Arc::increment_strong_count(state_pointer) };
        // SAFETY: the increment immediately above created the strong reference consumed here.
        let state = unsafe { Arc::from_raw(state_pointer) };
        if let Some(start) = state.begin_showing() {
            state.complete(show_folder_picker(hwnd, &start));
        }
        LRESULT(0)
    }

    /// Show `IFileOpenDialog` in its pick-a-folder guise and report what came
    /// back.
    ///
    /// `IFileOpenDialog` rather than the older `SHBrowseForFolder`, because the
    /// latter draws a tree from 1995 with no address bar, no typing, no
    /// favourites and no resize — and this row exists precisely for the folders
    /// the quick list above it could not name, which are the ones you need to
    /// navigate to.
    ///
    /// `FOS_FORCEFILESYSTEM` is what keeps the answer a *path*: without it the
    /// dialog will happily return a shell namespace item — a library, a phone
    /// over MTP, a search results folder — that has no directory behind it for
    /// anything here to enumerate.
    fn show_folder_picker(hwnd: HWND, start: &[u16]) -> Result<Option<PathBuf>, String> {
        // SAFETY: every call below runs on the window's own GUI thread, reached only from the
        // posted-message subclass after winit's initiating callback has returned. The apartment
        // is initialized and balanced here, and each COM object is dropped by the `windows`
        // crate's own `Release` at the end of its scope.
        unsafe {
            let apartment = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            if apartment == RPC_E_CHANGED_MODE {
                // The thread is already a multi-threaded apartment, which a shell
                // dialog may not be shown from. Reported rather than forced: the
                // caller's answer to a chooser it cannot show is to leave the
                // root where it was, which is the correct outcome and not one
                // worth breaking someone's apartment model to avoid.
                return Err("the event-loop thread is not a single-threaded apartment".to_owned());
            }
            // S_FALSE means "already initialized, and this call counted" — it is a
            // success that still owes a `CoUninitialize`, which is why the balance
            // is taken from `is_ok` rather than from `== S_OK`.
            let balance = apartment.is_ok();
            let result = (|| {
                let dialog: IFileOpenDialog =
                    CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                        .map_err(|error| format!("CoCreateInstance(FileOpenDialog): {error}"))?;
                let options = dialog
                    .GetOptions()
                    .map_err(|error| format!("IFileDialog::GetOptions: {error}"))?;
                dialog
                    .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
                    .map_err(|error| format!("IFileDialog::SetOptions: {error}"))?;
                // Where the column is already rooted, so the chooser opens where
                // you are looking rather than wherever Windows last left it. Its
                // failure is not this function's failure: a root that has since
                // been deleted or unplugged is a perfectly good reason to open at
                // the system's default instead of refusing to open at all.
                if start.len() > 1
                    && let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(
                        PCWSTR(start.as_ptr()),
                        None,
                    )
                {
                    let _ = dialog.SetFolder(&folder);
                }
                if let Err(error) = dialog.Show(Some(hwnd)) {
                    return if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                        Ok(None)
                    } else {
                        Err(format!("IFileDialog::Show: {error}"))
                    };
                }
                let item = dialog
                    .GetResult()
                    .map_err(|error| format!("IFileOpenDialog::GetResult: {error}"))?;
                let name = item
                    .GetDisplayName(SIGDN_FILESYSPATH)
                    .map_err(|error| format!("IShellItem::GetDisplayName: {error}"))?;
                let path = name.to_string();
                // The shell allocated it; the shell's allocator frees it. This is
                // the one allocation in this function that Rust does not own.
                CoTaskMemFree(Some(name.0.cast()));
                path.map(|path| Some(PathBuf::from(path)))
                    .map_err(|error| format!("the chosen folder's name is not UTF-16: {error}"))
            })();
            if balance {
                CoUninitialize();
            }
            result
        }
    }

    /// Thread-affine native positioning bridge used by Microsoft Pinyin.
    ///
    /// Some Chinese IMEs follow the Win32 system caret instead of IMM32 candidate-form requests.
    /// winit remains the sole owner of the HIMC candidate/composition forms; this bridge only
    /// supplies the otherwise invisible 1x1 caret signal.
    pub struct ImeSystemCaret {
        hwnd: HWND,
        active: bool,
    }

    impl ImeSystemCaret {
        pub fn new(hwnd: NonZeroIsize) -> Self {
            Self {
                hwnd: HWND(hwnd.get() as *mut c_void),
                active: false,
            }
        }

        pub fn update(&mut self, x: i32, y: i32) -> Result<(), String> {
            if !active_layout_is_chinese() {
                self.destroy();
                return Ok(());
            }

            // SAFETY: the HWND originates from winit and calls occur on its event-loop thread.
            // A Win32 caret is owned by that GUI thread. The null bitmap plus 1x1 dimensions is
            // the non-painted compatibility caret used only as an IME positioning signal.
            unsafe {
                if !self.active {
                    CreateCaret(self.hwnd, None, 1, 1)
                        .map_err(|error| format!("CreateCaret(1x1) failed: {error}"))?;
                    self.active = true;
                }
                SetCaretPos(x, y).map_err(|error| format!("SetCaretPos failed: {error}"))?;
            }
            Ok(())
        }

        pub fn destroy(&mut self) {
            if !self.active {
                return;
            }
            // SAFETY: this object is dropped/disabled on the same event-loop thread that created
            // the thread-affine caret. There is no borrowed memory and failure needs no recovery.
            unsafe {
                let _ = DestroyCaret();
            }
            self.active = false;
        }
    }

    impl Drop for ImeSystemCaret {
        fn drop(&mut self) {
            self.destroy();
        }
    }

    fn active_layout_is_chinese() -> bool {
        // SAFETY: thread id zero asks user32 for the calling event-loop thread's active layout.
        let layout = unsafe { GetKeyboardLayout(0) };
        primary_language_id(layout.0 as usize as u16) == 0x04
    }

    fn primary_language_id(language_id: u16) -> u16 {
        language_id & 0x03ff
    }

    pub fn get_dpi_for_window(hwnd: NonZeroIsize) -> Result<u32, String> {
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle. GetDpiForWindow only
        // reads the DPI associated with that window and returns zero for an invalid handle.
        let dpi = unsafe { GetDpiForWindow(HWND(hwnd.get() as *mut c_void)) };
        if dpi == 0 {
            Err("GetDpiForWindow returned zero".to_owned())
        } else {
            Ok(dpi)
        }
    }

    /// Whether the window is minimized (iconic).
    ///
    /// The companion of the `IsZoomed` test the snapshot already makes, and it
    /// exists for the same reason: while a window is iconic, `GetWindowRect`
    /// stops describing the window and starts describing the icon — Windows
    /// parks the rectangle far off-screen at a token size (`-32000, -32000` in
    /// the classic shell; a 157x25 rect at `x = -16000` was what this app
    /// actually recorded). That rectangle is not a place the user ever put the
    /// window, so nothing may be derived from it.
    ///
    /// Deliberately *not* `GetWindowPlacement().rcNormalPosition`, which looks
    /// like the more direct answer and is a trap: `rcNormalPosition` is stated
    /// in workspace coordinates, so on any machine with a taskbar it differs
    /// from `GetWindowRect`'s screen coordinates by the work-area origin. Mixing
    /// the two would reintroduce exactly the per-restart drift that making this
    /// module speak one rectangle — the outer rect — was meant to end.
    pub fn is_window_minimized(hwnd: NonZeroIsize) -> bool {
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle.
        // IsIconic only reads window state and reports false for a bad handle.
        unsafe { IsIconic(HWND(hwnd.get() as *mut c_void)) }.as_bool()
    }

    pub fn get_window_rect(hwnd: NonZeroIsize) -> Result<WindowRect, String> {
        let mut rect = RECT::default();
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle and `rect` remains valid
        // and exclusively borrowed for the duration of this read-only query.
        unsafe { GetWindowRect(HWND(hwnd.get() as *mut c_void), &mut rect) }
            .map_err(|error| format!("GetWindowRect failed: {error}"))?;
        Ok(WindowRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
    }

    /// Place the window's *outer* rectangle exactly, in physical screen pixels.
    ///
    /// The counterpart of [`get_window_rect`], and the only sizing call that is
    /// meaningful once [`CustomWindowFrame`] is installed: winit sizes by client
    /// area and derives the outer rect with `AdjustWindowRectExForDpi`, but this
    /// window's `WM_NCCALCSIZE` has made the two the same rectangle, so that
    /// derivation adds a frame margin the window does not wear. Passing the outer
    /// rect straight through is what makes `GetWindowRect` -> save -> restore ->
    /// `GetWindowRect` an identity.
    pub fn set_window_outer_rect(hwnd: NonZeroIsize, rect: WindowRect) -> Result<(), String> {
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle. No
        // insert-after handle is passed, which `SWP_NOZORDER` makes inert.
        unsafe {
            SetWindowPos(
                HWND(hwnd.get() as *mut c_void),
                None,
                rect.left,
                rect.top,
                rect.right.saturating_sub(rect.left).max(1),
                rect.bottom.saturating_sub(rect.top).max(1),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| format!("SetWindowPos(restore window rect) failed: {error}"))
    }

    /// The work area — the monitor minus the taskbar and other appbars — of the
    /// display this window sits on, in physical pixels.
    ///
    /// `docs/M2-layout-solver-spec.md` §2.6.5 clamps the window's minimum inner
    /// size to 60% of *this* rectangle, not of the monitor: a minimum computed
    /// against the full monitor would quietly include the taskbar's strip and let
    /// the window refuse to shrink past something the user can never see all of.
    /// Failure is reported rather than guessed at, because tiny-window §4.4 rules
    /// that a never-observed work area means "set no minimum at all".
    pub fn get_work_area(hwnd: NonZeroIsize) -> Result<WindowRect, String> {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle.
        // MonitorFromWindow with MONITOR_DEFAULTTONEAREST always returns a valid
        // monitor handle, and `info` stays valid and exclusively borrowed across
        // this read-only query with its `cbSize` set as the API requires.
        let ok = unsafe {
            let monitor =
                MonitorFromWindow(HWND(hwnd.get() as *mut c_void), MONITOR_DEFAULTTONEAREST);
            GetMonitorInfoW(monitor, &mut info)
        };
        if !ok.as_bool() {
            return Err("GetMonitorInfoW failed".to_owned());
        }
        Ok(WindowRect {
            left: info.rcWork.left,
            top: info.rcWork.top,
            right: info.rcWork.right,
            bottom: info.rcWork.bottom,
        })
    }

    pub fn install_window_class_background(hwnd: NonZeroIsize, rgb: [u8; 3]) -> Result<(), String> {
        let [r, g, b] = rgb;
        let color = COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16));
        let mut installed = WINDOW_CLASS_BACKGROUND
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "window class background brush lock poisoned".to_owned())?;
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle on the event-loop thread.
        // CreateSolidBrush returns an independent GDI brush. SetClassLongPtrW atomically replaces
        // the class brush; only after that succeeds do we delete the previous brush that *we* own.
        // The original winit brush returned on the first call is not ours and is never deleted here.
        unsafe {
            let brush = CreateSolidBrush(color);
            if brush.is_invalid() {
                return Err(format!("CreateSolidBrush failed: {}", GetLastError().0));
            }

            SetLastError(WIN32_ERROR(0));
            let previous = SetClassLongPtrW(
                HWND(hwnd.get() as *mut c_void),
                GCLP_HBRBACKGROUND,
                brush.0 as isize,
            );
            let error = GetLastError();
            if previous == 0 && error.0 != 0 {
                let _ = DeleteObject(HGDIOBJ(brush.0));
                return Err(format!(
                    "SetClassLongPtrW(GCLP_HBRBACKGROUND) failed: {}",
                    error.0
                ));
            }
            let old_owned = installed.replace(brush.0 as isize);
            if let Some(old_owned) = old_owned {
                let _ = DeleteObject(HGDIOBJ(old_owned as *mut c_void));
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::{
            CLIPBOARD_OPEN_RETRY_DELAYS, FolderPickerState, MathMenuState, names_a_program,
            primary_language_id, retry_open_clipboard, validate_local_image_path,
            validate_openable_path,
        };
        use std::path::{Path, PathBuf};

        #[test]
        fn clipboard_open_retry_is_bounded_and_can_recover() {
            let mut attempts = 0;
            let mut waits = Vec::new();
            let result = retry_open_clipboard(
                || {
                    attempts += 1;
                    (attempts == 3)
                        .then_some(())
                        .ok_or_else(|| "clipboard busy".to_owned())
                },
                |delay| waits.push(delay),
            );

            assert_eq!(result, Ok(()));
            assert_eq!(attempts, 3);
            assert_eq!(waits, CLIPBOARD_OPEN_RETRY_DELAYS[..2]);
        }

        #[test]
        fn clipboard_open_retry_reports_the_last_failure_after_its_wait_budget() {
            let mut attempts = 0;
            let mut waits = Vec::new();
            let error = retry_open_clipboard(
                || {
                    attempts += 1;
                    Err(format!("busy-{attempts}"))
                },
                |delay| waits.push(delay),
            )
            .unwrap_err();

            assert_eq!(attempts, CLIPBOARD_OPEN_RETRY_DELAYS.len() + 1);
            assert_eq!(waits, CLIPBOARD_OPEN_RETRY_DELAYS);
            assert!(error.contains("busy-5"));
            assert!(error.contains("75 ms"));
        }

        #[test]
        fn chinese_system_caret_gate_uses_primary_language_bits() {
            assert_eq!(primary_language_id(0x0804), 0x0004);
            assert_eq!(primary_language_id(0x0404), 0x0004);
            assert_ne!(primary_language_id(0x0409), 0x0004);
        }

        #[test]
        fn deferred_menu_state_requires_posted_dispatch_before_showing() {
            let state = MathMenuState::new();
            assert_eq!(state.take_result(), None);
            assert!(state.begin_request(()));
            assert!(!state.begin_request(()));
            assert_eq!(state.take_result(), None);
            assert!(state.begin_showing().is_some());
            assert!(state.begin_showing().is_none());
            state.complete(Ok(true));
            assert_eq!(state.take_result(), Some(Ok(true)));
            assert!(state.begin_request(()));
        }

        /// PIN — the gesture's own arguments survive the deferral, and only the
        /// dispatch that actually shows the dialog receives them.
        ///
        /// The red gate this stands in front of: a folder chooser whose start
        /// folder was read at *show* time rather than at *request* time would
        /// open wherever the column happened to be pointing by then — which,
        /// because the whole point of the deferral is that time passes, is not
        /// necessarily where it was pointing when the row was pressed.
        #[test]
        fn a_deferred_request_carries_its_arguments_to_the_dispatch_that_shows_it() {
            let state = FolderPickerState::new();
            let start: Vec<u16> = "C:\\work\0".encode_utf16().collect();
            assert!(state.begin_request(start.clone()));
            assert!(
                !state.begin_request(Vec::new()),
                "a second ask while one is queued is coalesced, not stacked"
            );
            assert_eq!(state.begin_showing(), Some(start));
            assert_eq!(
                state.begin_showing(),
                None,
                "and the arguments are handed out exactly once"
            );
            state.complete(Ok(Some(PathBuf::from(r"C:\chosen"))));
            assert_eq!(
                state.take_result(),
                Some(Ok(Some(PathBuf::from(r"C:\chosen"))))
            );
            assert_eq!(state.take_result(), None);
        }

        /// PIN — a cancelled chooser and a chooser that could not be shown are
        /// two different answers, and neither is "a folder was chosen".
        #[test]
        fn a_cancelled_chooser_is_not_a_failure_and_not_a_choice() {
            let state = FolderPickerState::new();
            assert!(state.begin_request(Vec::new()));
            assert!(state.begin_showing().is_some());
            state.complete(Ok(None));
            assert_eq!(state.take_result(), Some(Ok(None)));
        }

        #[test]
        fn local_file_bridge_rejects_every_path_outside_the_image_slice() {
            assert!(validate_local_image_path(Path::new(r"C:\tmp\image.png")).is_ok());
            assert!(validate_local_image_path(Path::new("C:/tmp/IMAGE.JPEG")).is_ok());
            assert!(validate_local_image_path(Path::new(r"relative\image.png")).is_err());
            assert!(validate_local_image_path(Path::new(r"\\server\share\image.png")).is_err());
            assert!(validate_local_image_path(Path::new(r"C:\tmp\image.svg")).is_ok());
            assert!(validate_local_image_path(Path::new(r"C:\tmp\image.bmp")).is_err());
            assert!(validate_local_image_path(Path::new("C:\\tmp\\bad\0.png")).is_err());
        }

        /// PIN — the tree's bridge opens documents and refuses programs, and the
        /// refusal does not depend on this machine's `PATHEXT` being anything in
        /// particular.
        #[test]
        fn the_tree_bridge_opens_what_it_can_show_and_never_what_it_would_run() {
            let empty = "";
            for document in [
                r"C:\notes\readme.md",
                r"C:\notes\report.pdf",
                r"C:\notes\archive.zip",
                r"C:\notes\NOEXTENSION",
                r"C:\notes\photo.PNG",
            ] {
                assert!(
                    !names_a_program(Path::new(document), empty),
                    "{document} is something to look at"
                );
            }
            for program in [
                r"C:\bin\tool.exe",
                r"C:\bin\TOOL.EXE",
                r"C:\bin\run.bat",
                r"C:\bin\install.msi",
                r"C:\bin\shortcut.lnk",
                r"C:\bin\saver.scr",
                r"C:\bin\page.hta",
                r"C:\bin\keys.reg",
                r"C:\bin\script.ps1",
            ] {
                assert!(
                    names_a_program(Path::new(program), empty),
                    "{program} would run"
                );
            }
        }

        /// PIN — a machine that has taught its command line to execute a new
        /// extension has taught this bridge to refuse it.
        #[test]
        fn a_machine_that_makes_something_executable_makes_it_refused_here() {
            let path = Path::new(r"C:\bin\macro.xyz");
            assert!(!names_a_program(path, ".EXE;.BAT"));
            assert!(names_a_program(path, ".EXE;.XYZ"));
            assert!(names_a_program(path, ".exe;.xyz"));
            // An emptied `PATHEXT` cannot open the door the fixed list shuts.
            assert!(names_a_program(Path::new(r"C:\bin\tool.exe"), ""));
        }

        /// A column may be rooted on a share, so its rows have to be openable —
        /// which is the one way this gate is wider than the image bridge's.
        #[test]
        fn the_tree_bridge_takes_a_share_and_still_refuses_a_relative_name() {
            assert!(validate_openable_path(Path::new(r"C:\notes\a.txt")).is_ok());
            assert!(validate_openable_path(Path::new(r"\\server\share\a.txt")).is_ok());
            assert!(validate_openable_path(Path::new(r"notes\a.txt")).is_err());
            assert!(validate_openable_path(Path::new("C:\\notes\\bad\0.txt")).is_err());
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{
    CustomWindowFrame, FolderPicker, ImeSystemCaret, MathContextMenu, PROGRAM_REFUSED,
    client_area_animation_enabled, clipboard_text, get_dpi_for_window, get_window_rect,
    get_work_area, install_window_class_background, is_window_minimized, open_local_file,
    open_local_path, request_window_close, reveal_in_explorer, set_clipboard_text,
    set_window_outer_rect, shell_execute, wheel_scroll_amount,
};

#[cfg(test)]
mod reveal_tests {
    use super::*;
    use std::path::Path;

    /// PIN (user ruling, 2026-08-13) — **a foot reveals the thing it names**,
    /// which for a file means Explorer opens with that file highlighted.
    ///
    /// The old answer was to open the containing folder, and against a folder
    /// of two hundred rows that is not an answer: the path the foot was
    /// printing arrived indistinguishable from everything beside it. `/select`
    /// is the verb that means what the foot says.
    ///
    /// The quoting is the whole of what can be wrong here. Explorer parses
    /// `/select,<path>` as one token and wants the path quoted; a command line
    /// one quote out opens `Documents` and reports success, which is the worst
    /// shape a failure can take.
    ///
    /// MUTATIONS:
    /// ① go back to a bare folder open — drop `/select,` — and the first
    ///    assertion goes red, which is the reported behaviour written down;
    /// ② drop the quotes and the third goes red: every path with a space in it
    ///    silently reveals the wrong thing.
    #[test]
    fn a_file_is_revealed_by_selecting_it_and_a_folder_by_opening_it() {
        let file = Path::new(r"C:\repo\test-assets\preview-samples\stress.md");
        let arguments = reveal_arguments(file, false);
        let text = arguments.to_string_lossy();
        assert!(
            text.starts_with("/select,"),
            "a file is highlighted where it lives, not merely surrounded: {text}"
        );
        assert!(
            text.contains(&*file.to_string_lossy()),
            "and it is that file that is named: {text}"
        );
        assert_eq!(
            text,
            format!("/select,\"{}\"", file.display()),
            "as one quoted token, which is the only form Explorer parses"
        );

        // A directory is opened rather than selected: `/select` on a folder
        // opens its *parent*, one level further out than a root is offering.
        let folder = Path::new(r"C:\repo\test-assets\preview-samples");
        let opened = reveal_arguments(folder, true);
        let text = opened.to_string_lossy();
        assert!(
            !text.contains("/select"),
            "a tree's own root is a place to look inside: {text}"
        );
        assert_eq!(text, format!("\"{}\"", folder.display()));

        // A path with a space survives, which is what the quotes are for.
        let spaced = Path::new(r"C:\My Documents\a file.md");
        assert_eq!(
            reveal_arguments(spaced, false).to_string_lossy(),
            "/select,\"C:\\My Documents\\a file.md\""
        );
    }
}

#[cfg(test)]
mod custom_frame_tests {
    use super::*;

    fn metrics(dpi: u32) -> CustomFrameMetrics {
        CustomFrameMetrics {
            width: logical_px_for_dpi(960, dpi),
            height: logical_px_for_dpi(600, dpi),
            title_bar_height: logical_px_for_dpi(40, dpi),
            tab_strip_right_px: logical_px_for_dpi(240, dpi),
            caption_button_width: logical_px_for_dpi(46, dpi),
            caption_button_count: 4,
            resize_border: logical_px_for_dpi(8, dpi),
            resizable: true,
        }
    }

    /// Red gate: every region returned to Windows is pinned, including corners
    /// (which must win over caption), the drag band and the app-owned buttons.
    #[test]
    fn custom_frame_hit_test_maps_drag_resize_edges_and_caption_buttons() {
        let m = metrics(96);
        assert_eq!(custom_frame_hit_test(m, 0, 0), CustomFrameHit::TopLeft);
        assert_eq!(custom_frame_hit_test(m, 959, 0), CustomFrameHit::TopRight);
        assert_eq!(custom_frame_hit_test(m, 0, 599), CustomFrameHit::BottomLeft);
        assert_eq!(
            custom_frame_hit_test(m, 959, 599),
            CustomFrameHit::BottomRight
        );
        assert_eq!(custom_frame_hit_test(m, 0, 300), CustomFrameHit::Left);
        assert_eq!(custom_frame_hit_test(m, 959, 300), CustomFrameHit::Right);
        assert_eq!(custom_frame_hit_test(m, 400, 0), CustomFrameHit::Top);
        assert_eq!(custom_frame_hit_test(m, 400, 599), CustomFrameHit::Bottom);
        assert_eq!(custom_frame_hit_test(m, 100, 20), CustomFrameHit::Client);
        assert_eq!(custom_frame_hit_test(m, 300, 20), CustomFrameHit::Caption);
        assert_eq!(custom_frame_hit_test(m, 800, 20), CustomFrameHit::Client);
        assert_eq!(custom_frame_hit_test(m, 300, 60), CustomFrameHit::Client);
    }

    /// DPI changes scale both the logical title/button geometry and the resize
    /// band; testing equivalent logical points catches a hard-coded 96-DPI map.
    #[test]
    fn custom_frame_hit_test_is_dpi_scaled() {
        for dpi in [96, 120, 144, 168, 192, 240] {
            let m = metrics(dpi);
            assert_eq!(
                custom_frame_hit_test(m, logical_px_for_dpi(120, dpi), logical_px_for_dpi(20, dpi),),
                CustomFrameHit::Client,
                "tab strip at {dpi} DPI"
            );
            assert_eq!(
                custom_frame_hit_test(m, logical_px_for_dpi(300, dpi), logical_px_for_dpi(20, dpi),),
                CustomFrameHit::Caption,
                "drag region beside tab strip at {dpi} DPI"
            );
            assert_eq!(
                custom_frame_hit_test(
                    m,
                    m.width - logical_px_for_dpi(23, dpi),
                    logical_px_for_dpi(20, dpi),
                ),
                CustomFrameHit::Client,
                "caption button at {dpi} DPI"
            );
            assert_eq!(
                custom_frame_hit_test(m, 1, m.height / 2),
                CustomFrameHit::Left,
                "resize border at {dpi} DPI"
            );
        }
    }

    #[test]
    fn maximized_frame_has_no_resize_hits_but_keeps_caption_dragging() {
        let mut m = metrics(144);
        m.resizable = false;
        m.resize_border = 0;
        assert_eq!(custom_frame_hit_test(m, 0, 0), CustomFrameHit::Client);
        assert_eq!(
            custom_frame_hit_test(m, m.tab_strip_right_px + 1, 1),
            CustomFrameHit::Caption
        );
        assert_eq!(
            custom_frame_hit_test(m, m.width - 1, 1),
            CustomFrameHit::Client
        );
    }
}
