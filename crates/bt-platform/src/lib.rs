//! Audited native bridges that are not exposed by winit's safe APIs.

use std::num::NonZeroIsize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[cfg(windows)]
mod windows_impl {
    use std::{ffi::c_void, sync::OnceLock};

    use windows::Win32::{
        Foundation::{COLORREF, GetLastError, HGLOBAL, HWND, RECT, SetLastError, WIN32_ERROR},
        Graphics::Gdi::{CreateSolidBrush, DeleteObject, HGDIOBJ},
        System::{
            DataExchange::{
                CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
            },
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        },
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::GetKeyboardLayout,
            WindowsAndMessaging::{
                CreateCaret, DestroyCaret, GCLP_HBRBACKGROUND, GetWindowRect, SetCaretPos,
                SetClassLongPtrW,
            },
        },
    };

    use super::{NonZeroIsize, WindowRect};

    static WINDOW_CLASS_BACKGROUND: OnceLock<Result<(), String>> = OnceLock::new();
    const CF_UNICODETEXT: u32 = 13;

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
        use super::primary_language_id;

        #[test]
        fn chinese_system_caret_gate_uses_primary_language_bits() {
            assert_eq!(primary_language_id(0x0804), 0x0004);
            assert_eq!(primary_language_id(0x0404), 0x0004);
            assert_ne!(primary_language_id(0x0409), 0x0004);
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{
    ImeSystemCaret, clipboard_text, get_dpi_for_window, get_window_rect,
    install_window_class_background,
};
