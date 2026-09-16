//! The terminal clipboard door holds one OpenClipboard interval, including its survey.

use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

use windows::{
    Win32::{
        Foundation::HGLOBAL,
        System::{
            DataExchange::{
                CloseClipboard, GetClipboardData, GetClipboardSequenceNumber,
                IsClipboardFormatAvailable, RegisterClipboardFormatW,
            },
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        },
        UI::Shell::{DragQueryFileW, HDROP},
    },
    core::w,
};

use crate::clipboard::{
    Candidate, ClipboardPayload, ClipboardPort, ClipboardTypes, PictureBytes, PictureEncoding,
    read_payload,
};

/// WinUser.h standard format identifiers.
const CF_HDROP: u32 = 15;
const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;

#[derive(Default)]
struct WindowsClipboard {
    opened: bool,
}

impl ClipboardPort for WindowsClipboard {
    fn begin(&mut self) -> Result<(), String> {
        crate::windows_impl::open_clipboard_with_retry(crate::windows_impl::owner_window()?)?;
        self.opened = true;
        Ok(())
    }
    fn survey(&mut self) -> Result<ClipboardTypes, String> {
        // SAFETY: registrations and availability queries read no content. The open interval is held.
        unsafe {
            let png = RegisterClipboardFormatW(w!("PNG"));
            let descriptor = RegisterClipboardFormatW(w!("FileGroupDescriptorW"));
            let contents = RegisterClipboardFormatW(w!("FileContents"));
            if png == 0 || descriptor == 0 || contents == 0 {
                return Err("clipboard type registration failed".into());
            }
            Ok(ClipboardTypes {
                files: IsClipboardFormatAvailable(CF_HDROP).is_ok(),
                text: IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok(),
                picture: [png, CF_DIBV5, CF_DIB]
                    .into_iter()
                    .any(|format| IsClipboardFormatAvailable(format).is_ok()),
                promise: IsClipboardFormatAvailable(descriptor).is_ok()
                    || IsClipboardFormatAvailable(contents).is_ok(),
            })
        }
    }
    fn files(&mut self) -> Candidate<Vec<PathBuf>> {
        // SAFETY: GetClipboardData's borrowed HDROP stays owned by the clipboard throughout all
        // DragQueryFileW calls. Names are copied as UTF-16, including unpaired units for the app gate.
        unsafe {
            let Ok(handle) = GetClipboardData(CF_HDROP) else {
                return Candidate::Unreadable("clipboard file acquisition failed".into());
            };
            let drop = HDROP(handle.0);
            let count = DragQueryFileW(drop, u32::MAX, None);
            let mut paths = Vec::new();
            for index in 0..count {
                let length = DragQueryFileW(drop, index, None);
                if length == 0 {
                    return Candidate::Unreadable("clipboard file name acquisition failed".into());
                }
                let mut units = vec![0; length as usize + 1];
                if DragQueryFileW(drop, index, Some(&mut units)) != length {
                    return Candidate::Unreadable("clipboard file name acquisition failed".into());
                }
                units.truncate(length as usize);
                paths.push(PathBuf::from(OsString::from_wide(&units)));
            }
            if paths.is_empty() {
                Candidate::Absent
            } else {
                Candidate::Present(paths)
            }
        }
    }
    fn text(&mut self) -> Candidate<String> {
        // SAFETY: GlobalSize bounds the locked slice; the owned copy is made before unlocking.
        unsafe {
            let Ok(handle) = GetClipboardData(CF_UNICODETEXT) else {
                return Candidate::Unreadable("clipboard text acquisition failed".into());
            };
            let global = HGLOBAL(handle.0);
            let bytes = GlobalSize(global);
            if bytes < size_of::<u16>() {
                return Candidate::Unreadable("clipboard text has no terminator".into());
            }
            let pointer = GlobalLock(global).cast::<u16>();
            if pointer.is_null() {
                return Candidate::Unreadable("clipboard text lock failed".into());
            }
            let units = std::slice::from_raw_parts(pointer, bytes / size_of::<u16>());
            let end = units
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(units.len());
            let text = String::from_utf16(&units[..end]);
            let _ = GlobalUnlock(global);
            match text {
                Ok(text) => Candidate::Present(text),
                Err(_) => Candidate::Unreadable("clipboard text is invalid UTF-16".into()),
            }
        }
    }
    /// **Every shape the source offered, best first**, copied and not decoded.
    ///
    /// `PNG` is a registered format rather than a standard one, so it is asked
    /// for by the same name the survey registered it under — and it is asked for
    /// first, because a source that offers it has already done the encode this
    /// paste would otherwise have to do.
    ///
    /// A format that is advertised and then will not render is **not** an error
    /// on its own: `GetClipboardData` renders delayed formats, and a source that
    /// can produce a `CF_DIB` but not its own `PNG` is an ordinary source. The
    /// rung fails only when nothing at all came back, which is what the empty
    /// list and `Absent` say between them.
    fn picture(&mut self) -> Candidate<Vec<PictureBytes>> {
        // SAFETY: registering a format name that is already registered answers the same
        // identifier; the open interval this object holds covers every read below.
        let png = unsafe { RegisterClipboardFormatW(w!("PNG")) };
        let mut found = Vec::new();
        for (format, encoding) in [
            (png, PictureEncoding::Png),
            (CF_DIBV5, PictureEncoding::DibV5),
            (CF_DIB, PictureEncoding::Dib),
        ] {
            if format == 0 {
                continue;
            }
            // SAFETY: availability is asked before the handle is, and the interval is held.
            if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
                continue;
            }
            if let Some(bytes) = global_bytes(format) {
                found.push(PictureBytes { encoding, bytes });
            }
        }
        if found.is_empty() {
            Candidate::Absent
        } else {
            Candidate::Present(found)
        }
    }
    fn finish(&mut self) -> Result<(), String> {
        self.opened = false;
        // SAFETY: begin successfully opened this clipboard on the calling event-loop thread.
        unsafe { CloseClipboard() }.map_err(|_| "clipboard close failed".to_owned())
    }
}

/// **One clipboard format's bytes, copied out of the global it is rendered
/// into.**
///
/// `GlobalSize` bounds the locked slice and the copy is made before the unlock,
/// which is `text`'s own arrangement one door over. An empty global answers
/// `None` rather than an empty picture: a zero-byte `CF_DIB` is a format that
/// was advertised and not rendered, and a file written out of it would be a file
/// with no picture in it.
fn global_bytes(format: u32) -> Option<Vec<u8>> {
    // SAFETY: the handle stays owned by the clipboard for the whole of this open
    // interval; `GlobalSize` bounds the slice and the owned copy is made before the
    // unlock, so nothing borrowed from the lock outlives it.
    unsafe {
        let handle = GetClipboardData(format).ok()?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        if size == 0 {
            return None;
        }
        let pointer = GlobalLock(global).cast::<u8>();
        if pointer.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(pointer, size).to_vec();
        let _ = GlobalUnlock(global);
        Some(bytes)
    }
}

impl Drop for WindowsClipboard {
    fn drop(&mut self) {
        if self.opened {
            // SAFETY: unwind cleanup for this object's own open interval.
            let _ = unsafe { CloseClipboard() };
        }
    }
}

pub fn clipboard_payload() -> Result<ClipboardPayload, String> {
    // SAFETY: diagnostic context only. Delayed rendering may advance this value successfully;
    // equality after GetClipboardData would reject that success (design ruling ④).
    let sequence = unsafe { GetClipboardSequenceNumber() };
    eprintln!("clipboard read: sequence={sequence}");
    read_payload(&mut WindowsClipboard::default())
}
