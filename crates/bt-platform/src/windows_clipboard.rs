//! The terminal clipboard door holds one OpenClipboard interval, including its survey.

use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

use windows::{
    Win32::{
        Foundation::HGLOBAL,
        Graphics::Gdi::{
            BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CBM_INIT, CreateDIBitmap, DIB_RGB_COLORS,
            DeleteObject, GetDC, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ, ReleaseDC,
        },
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
    Candidate, ClipboardPayload, ClipboardPort, ClipboardTypes, DibHeader, MAX_PICTURE_BYTES,
    MAX_PICTURE_SIDE, PictureBytes, PictureEncoding, PictureSource, TopDownPixels,
    WINDOWS_PICTURE_ORDER, dib_is_intact, first_offered_picture, read_payload,
};

/// WinUser.h standard format identifiers.
const CF_HDROP: u32 = 15;
const CF_UNICODETEXT: u32 = 13;
const CF_BITMAP: u32 = 2;
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
                picture: [png, CF_BITMAP, CF_DIBV5, CF_DIB]
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
    /// **The best shape this source will hand over, and only that one**, copied
    /// and not decoded.
    ///
    /// `PNG` is a registered format rather than a standard one, so it is asked
    /// for by the same name the survey registered it under — and it is asked for
    /// first, because a source that offers it has already done the encode this
    /// paste would otherwise have to do. The order is
    /// [`WINDOWS_PICTURE_ORDER`]; the walk that stops at the first answer is
    /// [`first_offered_picture`].
    ///
    /// A format that is advertised and then will not render is **not** an error
    /// on its own: `GetClipboardData` renders delayed formats, and a source that
    /// can produce a `CF_DIB` but not its own `PNG` is an ordinary source — the
    /// walk simply moves to the next shape. Neither is a format that renders
    /// something which is not the shape it claims to be: the walk reads the
    /// header and moves on, which is what keeps the sources that put unreadable
    /// `PNG`s on the board pasting their `CF_DIB`. The rung fails only when
    /// nothing at all came back, which is what `Absent` says.
    fn picture(&mut self) -> Candidate<Vec<PictureBytes>> {
        // SAFETY: registering a format name that is already registered answers the same
        // identifier; the open interval this object holds covers every read below.
        let png = unsafe { RegisterClipboardFormatW(w!("PNG")) };
        first_offered_picture(&WINDOWS_PICTURE_ORDER, &mut WindowsPictures { png })
    }
    fn finish(&mut self) -> Result<(), String> {
        self.opened = false;
        // SAFETY: begin successfully opened this clipboard on the calling event-loop thread.
        unsafe { CloseClipboard() }.map_err(|_| "clipboard close failed".to_owned())
    }
}

/// **The open clipboard, answering one picture shape at a time** for
/// [`first_offered_picture`].
///
/// It holds the registered `PNG` identifier because registration is a call and
/// the walk may ask about `Png` twice — once to see whether it is offered, once
/// to render it.
struct WindowsPictures {
    /// `RegisterClipboardFormatW(w!("PNG"))`, or `0` if registration failed.
    png: u32,
}

impl WindowsPictures {
    /// The clipboard format identifier this shape is carried in, or `None` when
    /// no Windows clipboard format carries it. `Tiff` is macOS' shape; a `0`
    /// identifier is a registration that failed, which is the same answer as a
    /// format the board does not have.
    fn format(&self, encoding: PictureEncoding) -> Option<u32> {
        let format = match encoding {
            PictureEncoding::Png => self.png,
            PictureEncoding::Bitmap => CF_BITMAP,
            PictureEncoding::DibV5 => CF_DIBV5,
            PictureEncoding::Dib => CF_DIB,
            PictureEncoding::Tiff => return None,
        };
        (format != 0).then_some(format)
    }
}

impl PictureSource for WindowsPictures {
    fn offers(&mut self, encoding: PictureEncoding) -> bool {
        let Some(format) = self.format(encoding) else {
            return false;
        };
        // SAFETY: an availability query renders nothing and reads no content; the open
        // interval `WindowsClipboard` holds covers it.
        unsafe { IsClipboardFormatAvailable(format) }.is_ok()
    }
    fn read(&mut self, encoding: PictureEncoding) -> Option<Vec<u8>> {
        if encoding == PictureEncoding::Bitmap {
            // A `CF_BITMAP` that will not come back is an ordinary board — a
            // `BI_PNG` body has none a screen can draw — and the walk goes on to
            // the raw bitmaps. The line is what says, next time, which of the
            // two rungs the picture was read from and why this one was passed.
            return clipboard_bitmap()
                .inspect_err(|reason| {
                    eprintln!("clipboard picture: CF_BITMAP passed over: {reason}")
                })
                .ok();
        }
        global_bytes(self.format(encoding)?)
    }
}

/// **The clipboard's `CF_BITMAP`, drawn by Windows into Folio's one layout.**
///
/// The handle is the clipboard's — owned by it for the open interval and never
/// deleted here. When the source put a `CF_DIB` or `CF_DIBV5` on the board and
/// no `CF_BITMAP`, this call is where Windows synthesises one, from its own
/// reading of the source's header.
fn clipboard_bitmap() -> Result<Vec<u8>, String> {
    // SAFETY: the open interval `WindowsClipboard` holds covers the call; the
    // handle is only borrowed.
    let handle = unsafe { GetClipboardData(CF_BITMAP) }
        .map_err(|error| format!("GetClipboardData failed: {error}"))?;
    top_down_pixels(HBITMAP(handle.0))
}

/// **Any bitmap GDI holds, as a [`TopDownPixels`] buffer** (ticket 66).
///
/// `GetDIBits` is asked for the one layout Folio reads — 32 bits, `BI_RGB`,
/// top-down — and does the conversion from whatever the bitmap is: a
/// device-dependent bitmap Windows made out of a `CF_DIB`, bit fields with or
/// without an alpha mask, a V4 or V5 header with its colour-space block, 16
/// bits a pixel, a bottom-up or top-down source, run-length encoding. That is
/// the list the `image` crate's BMP parser read part of, and reading it is
/// Windows' job: it is the reading every other program on the machine draws
/// the same clipboard with.
///
/// **The shape is judged before a byte is asked for** (review X-4):
/// `GetObjectW` reads the handle's width and height, and a picture whose
/// buffer would pass [`MAX_PICTURE_BYTES`] — or whose side passes
/// [`MAX_PICTURE_SIDE`] — is refused with its shape, before the allocation.
///
/// Its cost is on the window thread, where the clipboard may be read: one copy
/// of the pixels, converted — a few milliseconds for a 4K screenshot, the same
/// order as the `GlobalLock` copy of a `CF_DIB` it replaces.
pub(crate) fn top_down_pixels(bitmap: HBITMAP) -> Result<Vec<u8>, String> {
    let mut shape = BITMAP::default();
    let wanted = i32::try_from(size_of::<BITMAP>()).expect("a small structure");
    // SAFETY: `shape` is a `BITMAP` and `wanted` is its size, which is what the
    // call writes for a bitmap handle.
    let written = unsafe { GetObjectW(HGDIOBJ(bitmap.0), wanted, Some((&raw mut shape).cast())) };
    if written != wanted {
        return Err("the handle is not a bitmap".to_owned());
    }
    let (width, height) = (shape.bmWidth.unsigned_abs(), shape.bmHeight.unsigned_abs());
    let length = TopDownPixels::length(width, height);
    if width == 0
        || height == 0
        || width > MAX_PICTURE_SIDE
        || height > MAX_PICTURE_SIDE
        || length.is_none_or(|length| length > MAX_PICTURE_BYTES)
    {
        return Err(format!(
            "the bitmap is {width}x{height}, which is past this paste's ceiling"
        ));
    }
    let length = length.expect("checked above");
    let mut bytes = vec![0u8; length];
    bytes[..TopDownPixels::HEADER].copy_from_slice(&TopDownPixels::header(width, height));
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).expect("forty bytes"),
            biWidth: i32::try_from(width).expect("inside the side ceiling"),
            // Negative: top-down, which is the layout `TopDownPixels` reads.
            biHeight: -i32::try_from(height).expect("inside the side ceiling"),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..BITMAPINFOHEADER::default()
        },
        ..BITMAPINFO::default()
    };
    // SAFETY: the screen's device context is released on every road out of this
    // block; the pixel buffer is `width x height x 4` bytes after the header,
    // which is exactly what a 32-bit, top-down request of `height` rows writes,
    // and a 32-bit `BI_RGB` request writes no colour table past `bmiColors`.
    let rows = unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("the screen's device context is unavailable".to_owned());
        }
        let rows = GetDIBits(
            screen,
            bitmap,
            0,
            height,
            Some(bytes[TopDownPixels::HEADER..].as_mut_ptr().cast()),
            &raw mut info,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, screen);
        rows
    };
    if u32::try_from(rows).ok() != Some(height) {
        return Err(format!(
            "GetDIBits copied {rows} of {height} rows of a {width}x{height} bitmap"
        ));
    }
    Ok(bytes)
}

/// **What a program asking the clipboard for `CF_BITMAP` is handed, for a
/// source that put these `CF_DIB` bytes there** — the seam the `GetDIBits` road
/// is tested through, since no test reads the real clipboard (it is shared with
/// the person at the machine, and with their other machines).
///
/// `CreateDIBitmap` on the screen's device context is the conversion the
/// clipboard's own synthesis makes: the source's header, read by GDI, into a
/// bitmap the display can draw. Then the same [`top_down_pixels`] the
/// clipboard's handle goes through, and the bitmap is deleted — it is this
/// function's, unlike the clipboard's.
///
/// The bytes are read for shape first, by the walk's own [`dib_is_intact`],
/// because GDI believes the header: it is handed a pointer and no length.
pub fn pixels_through_gdi(dib: &[u8]) -> Result<Vec<u8>, String> {
    if !dib_is_intact(dib) {
        return Err("the bitmap is not whole".to_owned());
    }
    let header = DibHeader::parse(dib).ok_or("the bitmap has no header")?;
    let body = header
        .body_offset()
        .ok_or("the bitmap's palette is past any buffer")?;
    // Copied into `u32`s so the header GDI reads through a structure pointer is
    // aligned as one; the bytes are the caller's, unchanged.
    let mut aligned = vec![0u32; dib.len().div_ceil(4)];
    // SAFETY: `aligned` holds at least `dib.len()` bytes and the two do not overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(dib.as_ptr(), aligned.as_mut_ptr().cast(), dib.len());
    }
    let start = aligned.as_ptr().cast::<u8>();
    // SAFETY: the header and the body are inside `aligned` — `dib_is_intact` has
    // checked the header's size and the pixels its arithmetic asks for, and
    // `body` is the offset that arithmetic starts from. The screen's device
    // context is released before anything else can leave the block.
    let bitmap = unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("the screen's device context is unavailable".to_owned());
        }
        let bitmap = CreateDIBitmap(
            screen,
            Some(start.cast::<BITMAPINFOHEADER>()),
            CBM_INIT as u32,
            Some(start.add(body).cast()),
            Some(start.cast::<BITMAPINFO>()),
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, screen);
        bitmap
    };
    if bitmap.is_invalid() {
        return Err(format!("GDI made no bitmap of it ({header})"));
    }
    let answer = top_down_pixels(bitmap);
    // SAFETY: this bitmap was made above, is selected into no device context,
    // and is deleted exactly once.
    let _ = unsafe { DeleteObject(HGDIOBJ(bitmap.0)) };
    answer
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
        // Nothing, and far too much, are both refused before the copy is made
        // (review X-4): a zero-byte global is a format that was advertised and
        // not rendered, and one past the ceiling is a source this paste will not
        // carry into memory to find out about.
        if size == 0 || size > MAX_PICTURE_BYTES {
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

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Gdi::CreateBitmap;

    use super::*;

    /// A colour per pixel, top row first, in multiples of eight so a 16-bit
    /// bitmap carries it exactly.
    fn colour(x: u32, y: u32) -> [u8; 3] {
        [
            ((x * 40 + y * 8) % 256) as u8 & 0xF8,
            ((x * 16 + y * 56 + 64) % 256) as u8 & 0xF8,
            ((x * 72 + y * 24 + 128) % 256) as u8 & 0xF8,
        ]
    }

    /// A packed DIB under a header of `size` bytes: `compression` 0 (plain
    /// BGR), 3 (bit fields: in the header for V4/V5, after it for 40), or 1
    /// (run-length 8-bit, over a 256-entry palette holding the colours used).
    fn packed(
        size: u32,
        width: u32,
        height: u32,
        depth: u16,
        compression: u32,
        top_down: bool,
    ) -> Vec<u8> {
        let mut dib: Vec<u8> = Vec::new();
        dib.extend(size.to_le_bytes());
        dib.extend(i32::try_from(width).unwrap().to_le_bytes());
        let rows = i32::try_from(height).unwrap();
        dib.extend((if top_down { -rows } else { rows }).to_le_bytes());
        dib.extend(1_u16.to_le_bytes());
        dib.extend(depth.to_le_bytes());
        dib.extend(compression.to_le_bytes());
        dib.extend([0; 20]);
        let masks: [u32; 4] = if depth == 32 {
            [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000]
        } else {
            [0xF800, 0x07E0, 0x001F, 0]
        };
        if size > 40 {
            for mask in masks {
                dib.extend(mask.to_le_bytes());
            }
            // An sRGB colour space and nothing else.
            dib.extend(0x7352_4742_u32.to_le_bytes());
        }
        dib.resize(size as usize, 0);
        if size == 40 && compression == 3 {
            for mask in &masks[..3] {
                dib.extend(mask.to_le_bytes());
            }
        }
        let order = |row: u32| if top_down { row } else { height - 1 - row };
        if compression == 1 {
            // Palette: index `i` is `colour(i, 0)`, so row `y` of the picture is
            // drawn with the same colours as row 0 — `colour(x, 0)` everywhere.
            for index in 0..256 {
                let [red, green, blue] = colour(index % width, 0);
                dib.extend([blue, green, red, 0]);
            }
            for _ in 0..height {
                // Encoded mode: one run per pixel, then end of line.
                for x in 0..width {
                    dib.extend([1, u8::try_from(x).unwrap()]);
                }
                dib.extend([0, 0]);
            }
            dib.extend([0, 1]); // end of bitmap
            // A compressed body's length is `biSizeImage`, and GDI reads no
            // further than it says.
            let body = u32::try_from(dib.len() - size as usize - 256 * 4).unwrap();
            dib[20..24].copy_from_slice(&body.to_le_bytes());
            return dib;
        }
        let stride = (width * u32::from(depth)).div_ceil(32) * 4;
        for row in 0..height {
            let y = order(row);
            let start = dib.len();
            for x in 0..width {
                let [red, green, blue] = colour(x, y);
                match depth {
                    32 => dib.extend([blue, green, red, 255]),
                    24 => dib.extend([blue, green, red]),
                    _ => {
                        let packed = (u16::from(red >> 3) << 11)
                            | (u16::from(green >> 2) << 5)
                            | u16::from(blue >> 3);
                        dib.extend(packed.to_le_bytes());
                    }
                }
            }
            dib.resize(start + stride as usize, 0);
        }
        dib
    }

    /// RED (66) — **Every flavour Windows can draw comes back from GDI as
    /// Folio's one layout, with its pixels in place.**
    ///
    /// Generated bitmaps only — no clipboard is read or written — made into a
    /// GDI bitmap the way the clipboard's own synthesis makes `CF_BITMAP`, and
    /// read back through the same `top_down_pixels` the clipboard's handle
    /// goes through. The flavours are the ones the `image` crate's BMP decoder
    /// read wrongly or not at all, and the ordinary ones beside them.
    ///
    /// MUTATION: ask `GetDIBits` for a bottom-up picture (positive height) and
    /// every row is upside down.
    #[test]
    fn every_flavour_windows_can_draw_comes_back_as_the_one_layout() {
        for (size, depth, compression, top_down, tolerance, what) in [
            (124, 32, 3, false, 0, "V5, bit fields with an alpha mask"),
            (124, 32, 3, true, 0, "V5, bit fields, top-down"),
            (108, 32, 3, false, 0, "V4, bit fields"),
            (40, 32, 3, false, 0, "info header, masks after it"),
            (124, 16, 3, false, 8, "V5, 16-bit 5-6-5"),
            (40, 16, 3, true, 8, "info header, 16-bit 5-6-5, top-down"),
            (40, 24, 0, false, 0, "info header, 24-bit"),
            (40, 24, 0, true, 0, "info header, 24-bit, top-down"),
            (40, 32, 0, false, 0, "info header, 32-bit"),
            (40, 8, 1, false, 0, "info header, run-length 8-bit"),
        ] {
            let (width, height) = (7_u32, 5_u32);
            let dib = packed(size, width, height, depth, compression, top_down);
            let bytes =
                pixels_through_gdi(&dib).unwrap_or_else(|reason| panic!("{what}: {reason}"));
            let pixels =
                TopDownPixels::parse(&bytes).unwrap_or_else(|| panic!("{what}: not the layout"));
            assert_eq!((pixels.width, pixels.height), (width, height), "{what}");
            for (index, pixel) in pixels.bgrx.chunks_exact(4).enumerate() {
                let index = u32::try_from(index).unwrap();
                let (x, y) = (index % width, index / width);
                let want = if compression == 1 {
                    colour(x, 0)
                } else {
                    colour(x, y)
                };
                let got = [pixel[2], pixel[1], pixel[0]];
                for channel in 0..3 {
                    assert!(
                        got[channel].abs_diff(want[channel]) <= tolerance,
                        "{what}: ({x}, {y}) is {got:?}, not {want:?}"
                    );
                }
            }
        }
    }

    /// PIN (66) — **A long capture comes through GDI whole**: a thousand across
    /// and twenty thousand down, the shape of a scrolling screenshot.
    #[test]
    fn a_long_capture_comes_through_gdi_whole() {
        let (width, height) = (1_000_u32, 20_000_u32);
        let dib = packed(40, width, height, 24, 0, false);
        let bytes = pixels_through_gdi(&dib).expect("a long capture draws");
        let pixels = TopDownPixels::parse(&bytes).expect("the layout");
        assert_eq!((pixels.width, pixels.height), (width, height));
        for (x, y) in [(0, 0), (999, 0), (0, 19_999), (517, 12_345), (999, 19_999)] {
            let at = ((y * width + x) * 4) as usize;
            let pixel = &pixels.bgrx[at..at + 4];
            assert_eq!([pixel[2], pixel[1], pixel[0]], colour(x, y), "({x}, {y})");
        }
    }

    /// PIN (66) — **What GDI answers for a bitmap a screen cannot draw**: a
    /// `BI_PNG` body makes no bitmap, which is why the raw rungs stay behind
    /// `CF_BITMAP` in `WINDOWS_PICTURE_ORDER`.
    #[test]
    fn a_png_bodied_bitmap_is_not_one_gdi_draws() {
        let mut dib = packed(40, 4, 4, 32, 0, false);
        dib[14..16].copy_from_slice(&0_u16.to_le_bytes());
        dib[16..20].copy_from_slice(&5_u32.to_le_bytes()); // BI_PNG
        let reason = pixels_through_gdi(&dib).expect_err("no screen draws a PNG body");
        assert!(reason.contains("GDI made no bitmap"), "{reason}");
    }

    /// RED (66) — **A bitmap handle whose shape is past the ceiling is refused
    /// before its pixels are asked for** (review X-4, extended to the new
    /// rung): the shape is `GetObjectW`'s, and the refusal names it.
    ///
    /// The bitmaps are one bit deep, so making them costs this test little —
    /// 16,000 square is 32 MB of GDI bitmap — while the 32-bit copy the rung
    /// would make of it is a gigabyte.
    ///
    /// MUTATION: drop the ceiling from `top_down_pixels` and the first row asks
    /// for a gigabyte.
    #[test]
    fn a_bitmap_past_the_ceiling_is_refused_before_its_pixels_are_copied() {
        for (width, height) in [(16_000_i32, 16_000_i32), (70_000, 8), (8, 70_000)] {
            // SAFETY: a monochrome bitmap with no initial bits; deleted below.
            let bitmap = unsafe { CreateBitmap(width, height, 1, 1, None) };
            assert!(!bitmap.is_invalid(), "{width}x{height} was made");
            let answer = top_down_pixels(bitmap);
            // SAFETY: made above, selected into nothing, deleted once.
            let _ = unsafe { DeleteObject(HGDIOBJ(bitmap.0)) };
            let reason = answer.expect_err("past the ceiling");
            assert!(
                reason.contains(&format!("{width}x{height}")) && reason.contains("ceiling"),
                "{reason}"
            );
        }
    }
}
