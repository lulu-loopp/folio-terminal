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

#[cfg(windows)]
mod windows_impl {
    use std::{
        ffi::c_void,
        sync::{Arc, Mutex, OnceLock},
    };

    use windows::Win32::{
        Foundation::{
            COLORREF, GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, POINT,
            RECT, SetLastError, WIN32_ERROR, WPARAM,
        },
        Graphics::Gdi::{CreateSolidBrush, DeleteObject, HGDIOBJ},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        },
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::GetKeyboardLayout,
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                AppendMenuW, CreateCaret, CreatePopupMenu, DestroyCaret, DestroyMenu,
                GCLP_HBRBACKGROUND, GetCursorPos, GetWindowRect, MF_STRING, PostMessageW,
                SPI_GETWHEELSCROLLLINES, SetCaretPos, SetClassLongPtrW, SystemParametersInfoW,
                TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP,
            },
        },
    };

    use super::{NonZeroIsize, WheelScrollAmount, WindowRect};

    static WINDOW_CLASS_BACKGROUND: OnceLock<Result<(), String>> = OnceLock::new();
    const CF_UNICODETEXT: u32 = 13;
    const WHEEL_PAGESCROLL: u32 = u32::MAX;
    const DEFERRED_MATH_MENU_MESSAGE: u32 = WM_APP + 0x4b7;
    const MATH_MENU_SUBCLASS_ID: usize = 0x4254_4d4d;

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

    pub fn clipboard_text(hwnd: NonZeroIsize) -> Result<String, String> {
        let hwnd = HWND(hwnd.get() as *mut c_void);
        // SAFETY: all calls run on winit's event-loop thread. The clipboard remains open while the
        // borrowed global-memory handle is locked, its UTF-16 content is copied, and then both the
        // memory and clipboard are released before returning.
        unsafe {
            OpenClipboard(Some(hwnd)).map_err(|error| format!("OpenClipboard failed: {error}"))?;
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
        unsafe {
            OpenClipboard(Some(hwnd)).map_err(|error| format!("OpenClipboard failed: {error}"))?;
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

        fn begin_request(&self) -> Result<(), String> {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(*phase, DeferredMenuPhase::Idle) {
                return Err("formula context menu request is already pending".to_owned());
            }
            *phase = DeferredMenuPhase::Posted;
            Ok(())
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

        pub fn request(&self) -> Result<(), String> {
            self.state.begin_request()?;
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
            Ok(())
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
        use super::{DeferredMenuState, primary_language_id};

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
            state.begin_request().unwrap();
            assert!(state.begin_request().is_err());
            assert_eq!(state.take_result(), None);
            assert!(state.begin_showing());
            assert!(!state.begin_showing());
            state.complete(Ok(true));
            assert_eq!(state.take_result(), Some(Ok(true)));
            state.begin_request().unwrap();
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{
    ImeSystemCaret, MathContextMenu, clipboard_text, get_dpi_for_window, get_window_rect,
    install_window_class_background, set_clipboard_text, wheel_scroll_amount,
};
