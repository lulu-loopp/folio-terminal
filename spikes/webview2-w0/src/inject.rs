//! The two input paths gate 5 and gate 4 could not reach from inside the page.
//!
//! `clipboard` is here because "the page saw a `Ctrl+C` keydown" and "the engine
//! actually copied something" are different claims, and only the second one
//! matters to a person trying to get a URL out of a preview. `touch` is here
//! because `SendPointerInput` proves the *forwarding* path and says nothing
//! about the path a finger really takes — a driver posting `WM_POINTER*` to the
//! window that owns the pixels under the contact.

/// The clipboard, read and written by the host.
///
/// Whatever the person had on their clipboard is saved and put back by the
/// caller: a probe that measures the copy path by destroying somebody's
/// clipboard has broken more than it measured.
pub mod clipboard {
    use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    /// Whatever unicode text the clipboard holds, if any.
    pub fn text() -> Option<String> {
        unsafe {
            OpenClipboard(Some(HWND::default())).ok()?;
            let text = match GetClipboardData(u32::from(CF_UNICODETEXT.0)) {
                Ok(handle) if !handle.is_invalid() => {
                    let global = HGLOBAL(handle.0);
                    let pointer = GlobalLock(global) as *const u16;
                    let value = if pointer.is_null() {
                        None
                    } else {
                        let mut length = 0usize;
                        while *pointer.add(length) != 0 {
                            length += 1;
                        }
                        Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                            pointer, length,
                        )))
                    };
                    let _ = GlobalUnlock(global);
                    value
                }
                _ => None,
            };
            let _ = CloseClipboard();
            text
        }
    }

    /// Put `value` on the clipboard. Returns whether it took.
    pub fn set_text(value: &str) -> bool {
        let mut wide: Vec<u16> = value.encode_utf16().collect();
        wide.push(0);
        unsafe {
            if OpenClipboard(Some(HWND::default())).is_err() {
                return false;
            }
            let ok = (|| -> windows::core::Result<()> {
                EmptyClipboard()?;
                let global = GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of_val(&wide[..]))?;
                let pointer = GlobalLock(global) as *mut u16;
                if pointer.is_null() {
                    return Err(windows::core::Error::from_thread());
                }
                std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer, wide.len());
                let _ = GlobalUnlock(global);
                // Ownership passes to the clipboard on success, which is why
                // there is deliberately no free on this path.
                SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(global.0)))?;
                Ok(())
            })()
            .is_ok();
            let _ = CloseClipboard();
            ok
        }
    }
}

/// Touch the screen for real.
///
/// `InjectTouchInput` is the driver path without the hardware: the contacts go
/// through the same input stack a digitizer's would, so a `WM_POINTERDOWN`
/// arriving at the host window is the route this machine has no digitizer to
/// exercise.
pub mod touch {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::UI::Input::Pointer::{
        InitializeTouchInjection, InjectTouchInput, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT,
        POINTER_FLAG_INRANGE, POINTER_FLAG_UP, POINTER_FLAG_UPDATE, POINTER_FLAGS, POINTER_INFO,
        POINTER_TOUCH_INFO, TOUCH_FEEDBACK_NONE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{PT_TOUCH, TOUCH_MASK_CONTACTAREA};

    /// Ask the OS for an injection device with room for `contacts` fingers.
    pub fn initialize(contacts: u32) -> Result<(), String> {
        unsafe { InitializeTouchInjection(contacts, TOUCH_FEEDBACK_NONE) }
            .map_err(|error| format!("{error}"))
    }

    fn contact(id: u32, at: POINT, flags: POINTER_FLAGS) -> POINTER_TOUCH_INFO {
        POINTER_TOUCH_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_TOUCH,
                pointerId: id,
                pointerFlags: flags,
                ptPixelLocation: at,
                ..Default::default()
            },
            touchFlags: 0,
            touchMask: TOUCH_MASK_CONTACTAREA,
            rcContact: RECT {
                left: at.x - 2,
                top: at.y - 2,
                right: at.x + 2,
                bottom: at.y + 2,
            },
            rcContactRaw: RECT {
                left: at.x - 2,
                top: at.y - 2,
                right: at.x + 2,
                bottom: at.y + 2,
            },
            ..Default::default()
        }
    }

    /// One whole gesture: every point in `points` goes down together, moves by
    /// `step` together and comes up together. Sending them in one call is what
    /// separates a multi-touch stack from one that only ever has one finger.
    ///
    /// Coordinates are **screen** pixels, because that is what the injection
    /// device speaks and what decides which window receives the contact.
    pub fn drag_together(points: &[POINT], step: POINT) -> Vec<String> {
        let mut errors = Vec::new();
        let mut send = |flags: POINTER_FLAGS, offset: POINT| {
            let contacts: Vec<POINTER_TOUCH_INFO> = points
                .iter()
                .enumerate()
                .map(|(index, at)| {
                    // **Zero-based, and below the count `initialize` was given.**
                    // `InjectTouchInput` requires every contact's id to fall in
                    // `[0, maxCount)`; ids starting at one put the last finger
                    // out of range and the whole frame comes back
                    // `E_INVALIDARG`, which reads as "this machine cannot
                    // inject touch" when it is an off-by-one.
                    contact(
                        index as u32,
                        POINT {
                            x: at.x + offset.x,
                            y: at.y + offset.y,
                        },
                        flags,
                    )
                })
                .collect();
            if let Err(error) = unsafe { InjectTouchInput(&contacts) } {
                errors.push(format!("{:#x}: {error}", flags.0));
            }
        };
        let down = POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT;
        let update = POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT;
        send(down, POINT { x: 0, y: 0 });
        send(update, step);
        send(POINTER_FLAG_UP, step);
        errors
    }
}
