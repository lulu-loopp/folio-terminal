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
    if y >= border
        && y < metrics.title_bar_height.max(border)
        && (x < buttons_left || x >= metrics.width)
    {
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

#[cfg(windows)]
mod windows_impl {
    use std::{
        ffi::c_void,
        os::windows::ffi::OsStrExt,
        path::Path,
        sync::{Arc, Mutex, OnceLock},
    };
    use windows::core::PCWSTR;

    use windows::Win32::{
        Foundation::{
            COLORREF, GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, POINT,
            RECT, SetLastError, WIN32_ERROR, WPARAM,
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
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        },
        UI::{
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            Input::KeyboardAndMouse::GetKeyboardLayout,
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, ShellExecuteW},
            WindowsAndMessaging::{
                AppendMenuW, CreateCaret, CreatePopupMenu, DestroyCaret, DestroyMenu,
                GCLP_HBRBACKGROUND, GetClientRect, GetCursorPos, GetWindowRect, HTBOTTOM,
                HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP,
                HTTOPLEFT, HTTOPRIGHT, IsZoomed, MF_STRING, NCCALCSIZE_PARAMS, PostMessageW,
                SM_CXFRAME, SM_CXPADDEDBORDER, SPI_GETWHEELSCROLLLINES, SW_SHOWNORMAL,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SetCaretPos, SetClassLongPtrW, SetWindowPos, SystemParametersInfoW, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CLOSE, WM_NCCALCSIZE, WM_NCHITTEST,
            },
        },
    };

    use super::{
        CustomFrameHit, CustomFrameMetrics, NonZeroIsize, WheelScrollAmount, WindowRect,
        custom_frame_hit_test, logical_px_for_dpi,
    };

    static WINDOW_CLASS_BACKGROUND: OnceLock<Result<(), String>> = OnceLock::new();
    const CF_UNICODETEXT: u32 = 13;
    const CLIPBOARD_OPEN_RETRY_DELAYS: [std::time::Duration; 4] = [
        std::time::Duration::from_millis(5),
        std::time::Duration::from_millis(10),
        std::time::Duration::from_millis(20),
        std::time::Duration::from_millis(40),
    ];
    const WHEEL_PAGESCROLL: u32 = u32::MAX;
    const DEFERRED_MATH_MENU_MESSAGE: u32 = WM_APP + 0x4b7;
    const MATH_MENU_SUBCLASS_ID: usize = 0x4254_4d4d;
    const CUSTOM_FRAME_SUBCLASS_ID: usize = 0x4254_4346;
    const TITLE_BAR_LOGICAL_PX: u32 = 40;
    const CAPTION_BUTTON_LOGICAL_PX: u32 = 46;
    const CAPTION_BUTTON_COUNT: i32 = 4;

    /// Keeps winit's ordinary overlapped-window styles (and therefore native
    /// snap, resize borders, minimize animation and system-menu semantics) while
    /// extending the client area through the system caption.
    pub struct CustomWindowFrame {
        hwnd: HWND,
    }

    impl CustomWindowFrame {
        pub fn install(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(custom_frame_subclass),
                    CUSTOM_FRAME_SUBCLASS_ID,
                    0,
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
            Ok(Self { hwnd })
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
        _reference_data: usize,
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
                let hit = custom_frame_hit_test(
                    CustomFrameMetrics {
                        width: client.right.saturating_sub(client.left),
                        height: client.bottom.saturating_sub(client.top),
                        title_bar_height: logical_px_for_dpi(TITLE_BAR_LOGICAL_PX, dpi),
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

    #[derive(Debug)]
    enum DeferredMenuPhase {
        Idle,
        Posted,
        Showing,
        Complete(Result<bool, String>),
    }

    #[derive(Debug)]
    struct DeferredMenuState {
        phase: Mutex<DeferredMenuPhase>,
    }

    impl DeferredMenuState {
        fn new() -> Self {
            Self {
                phase: Mutex::new(DeferredMenuPhase::Idle),
            }
        }

        fn begin_request(&self) -> bool {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(*phase, DeferredMenuPhase::Idle) {
                return false;
            }
            *phase = DeferredMenuPhase::Posted;
            true
        }

        fn cancel_request(&self) {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if matches!(*phase, DeferredMenuPhase::Posted) {
                *phase = DeferredMenuPhase::Idle;
            }
        }

        fn begin_showing(&self) -> bool {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(*phase, DeferredMenuPhase::Posted) {
                return false;
            }
            *phase = DeferredMenuPhase::Showing;
            true
        }

        fn complete(&self, result: Result<bool, String>) {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if matches!(*phase, DeferredMenuPhase::Showing) {
                *phase = DeferredMenuPhase::Complete(result);
            }
        }

        fn take_result(&self) -> Option<Result<bool, String>> {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            let DeferredMenuPhase::Complete(_) = &*phase else {
                return None;
            };
            let DeferredMenuPhase::Complete(result) =
                std::mem::replace(&mut *phase, DeferredMenuPhase::Idle)
            else {
                unreachable!("phase was matched as complete immediately before replacement")
            };
            Some(result)
        }
    }

    /// Formula context-menu bridge whose nested native message pump never starts inside a winit
    /// application callback. `request` only posts a private window message. The subclass receives
    /// it after the current `DispatchMessageW`/winit callback has returned, so any RedrawRequested
    /// emitted by TrackPopupMenu's nested pump finds winit's event-handler slot restored.
    pub struct MathContextMenu {
        hwnd: HWND,
        state: Arc<DeferredMenuState>,
    }

    impl MathContextMenu {
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Arc::new(DeferredMenuState::new());
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
            if !self.state.begin_request() {
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
        let state_pointer = reference_data as *const DeferredMenuState;
        if state_pointer.is_null() {
            return LRESULT(0);
        }
        // SAFETY: the installed MathContextMenu owns one Arc at callback entry. Incrementing before
        // constructing the temporary Arc keeps state alive even if a nested CloseRequested drops
        // the Runtime while TrackPopupMenu is open.
        unsafe { Arc::increment_strong_count(state_pointer) };
        // SAFETY: the increment immediately above created the strong reference consumed here.
        let state = unsafe { Arc::from_raw(state_pointer) };
        if state.begin_showing() {
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
        WINDOW_CLASS_BACKGROUND
            .get_or_init(|| install_window_class_background_once(hwnd, rgb))
            .clone()
    }

    fn install_window_class_background_once(
        hwnd: NonZeroIsize,
        [r, g, b]: [u8; 3],
    ) -> Result<(), String> {
        let color = COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16));
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle on the event-loop thread.
        // CreateSolidBrush returns an independent GDI brush. Once installed, the brush must remain
        // alive as long as winit's shared class; winit never unregisters that process class, so the
        // successful path intentionally transfers it to process lifetime. On failure we delete it.
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
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::{
            CLIPBOARD_OPEN_RETRY_DELAYS, DeferredMenuState, primary_language_id,
            retry_open_clipboard, validate_local_image_path,
        };
        use std::path::Path;

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
            let state = DeferredMenuState::new();
            assert_eq!(state.take_result(), None);
            assert!(state.begin_request());
            assert!(!state.begin_request());
            assert_eq!(state.take_result(), None);
            assert!(state.begin_showing());
            assert!(!state.begin_showing());
            state.complete(Ok(true));
            assert_eq!(state.take_result(), Some(Ok(true)));
            assert!(state.begin_request());
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
    }
}

#[cfg(windows)]
pub use windows_impl::{
    CustomWindowFrame, ImeSystemCaret, MathContextMenu, clipboard_text, get_dpi_for_window,
    get_window_rect, get_work_area, install_window_class_background, open_local_file,
    request_window_close, set_clipboard_text, shell_execute, wheel_scroll_amount,
};

#[cfg(test)]
mod custom_frame_tests {
    use super::*;

    fn metrics(dpi: u32) -> CustomFrameMetrics {
        CustomFrameMetrics {
            width: logical_px_for_dpi(960, dpi),
            height: logical_px_for_dpi(600, dpi),
            title_bar_height: logical_px_for_dpi(40, dpi),
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
                custom_frame_hit_test(m, logical_px_for_dpi(200, dpi), logical_px_for_dpi(20, dpi),),
                CustomFrameHit::Caption,
                "drag region at {dpi} DPI"
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
        assert_eq!(custom_frame_hit_test(m, 0, 0), CustomFrameHit::Caption);
        assert_eq!(
            custom_frame_hit_test(m, m.width - 1, 1),
            CustomFrameHit::Client
        );
    }
}
