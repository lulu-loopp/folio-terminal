//! The terminal clipboard door holds one OpenClipboard interval, including its survey.

use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
};

use windows::{
    Win32::{
        Foundation::{DV_E_FORMATETC, HGLOBAL, STG_E_FILEALREADYEXISTS},
        Graphics::Gdi::{
            BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CBM_INIT, CreateDIBitmap, DIB_RGB_COLORS,
            DeleteObject, GetDC, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ, ReleaseDC,
        },
        System::{
            Com::{
                DVASPECT_CONTENT, FORMATETC, IDataObject, IStream, STGM_FAILIFTHERE,
                STGM_READWRITE, STGM_SHARE_EXCLUSIVE, STGMEDIUM,
                StructuredStorage::StgCreateDocfile, TYMED_HGLOBAL, TYMED_ISTORAGE, TYMED_ISTREAM,
            },
            DataExchange::{
                CloseClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
                GetClipboardSequenceNumber, IsClipboardFormatAvailable, RegisterClipboardFormatW,
            },
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
            Ole::{OleInitialize, OleUninitialize, ReleaseStgMedium},
        },
        UI::{
            Shell::{DragQueryFileW, HDROP},
            WindowsAndMessaging::{
                DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
            },
        },
    },
    core::{HSTRING, w},
};

use crate::admission::WorkerCtx;
use crate::clipboard::{
    Candidate, ClipboardPort, ClipboardRead, ClipboardSequence, ClipboardTypes, DibHeader,
    MAX_PICTURE_BYTES, MAX_PICTURE_SIDE, PictureBytes, PictureEncoding, PictureSource,
    PromisedEntry, TopDownPixels, WINDOWS_PICTURE_ORDER, dib_is_intact, first_offered_picture,
    list_offered, read_payload_offering, standard_format_name,
};
use crate::clipboard_promise::{
    MAX_DESCRIPTOR_BYTES, PASTE_LIMITS, PromiseRefused, PromisedContents, PromisedFile,
    RenderProgress, RenderedPromise, parse_file_group_descriptor, render_promise,
};

/// WinUser.h standard format identifiers.
const CF_HDROP: u32 = 15;
const CF_UNICODETEXT: u32 = 13;
const CF_BITMAP: u32 = 2;
const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;

/// One open interval of the clipboard, held by the clipboard lane's worker (`worker`).
struct WindowsClipboard<'a> {
    worker: &'a WorkerCtx,
    opened: bool,
}

impl ClipboardPort for WindowsClipboard<'_> {
    fn begin(&mut self) -> Result<(), String> {
        crate::windows_impl::open_clipboard_with_retry(self.worker)?;
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
    /// **The promise's descriptor** (plan row 81): `FileGroupDescriptorW`'s bytes, copied up to
    /// [`MAX_DESCRIPTOR_BYTES`] and read by [`parse_file_group_descriptor`].
    fn promised(&mut self) -> Candidate<Vec<PromisedEntry>> {
        // SAFETY: registering an already registered name answers its identifier; the open
        // interval this object holds covers the read.
        let descriptor = unsafe { RegisterClipboardFormatW(w!("FileGroupDescriptorW")) };
        if descriptor == 0 {
            return Candidate::Absent;
        }
        match bounded_global_bytes(descriptor, MAX_DESCRIPTOR_BYTES) {
            None => Candidate::Absent,
            Some(bytes) => match parse_file_group_descriptor(&bytes) {
                Some(entries) => Candidate::Present(entries),
                None => Candidate::Unreadable("the file descriptor does not read".to_owned()),
            },
        }
    }
    /// **The format names on the board**, standard ones by their `CF_` names and registered ones
    /// by the system's ([`list_offered`] caps the list). Names only, never content.
    fn offered(&mut self) -> Vec<String> {
        let mut formats = Vec::new();
        // SAFETY: enumeration and name queries read no content; the open interval is held.
        unsafe {
            let mut format = EnumClipboardFormats(0);
            while format != 0 {
                formats.push(format);
                format = EnumClipboardFormats(format);
            }
        }
        list_offered(formats.into_iter().map(|format| {
            if format < 0xC000 {
                return standard_format_name(format);
            }
            let mut units = [0_u16; 128];
            // SAFETY: the buffer is the call's to fill, up to its length.
            let written = unsafe { GetClipboardFormatNameW(format, &mut units) };
            match usize::try_from(written) {
                Ok(length) if length > 0 => String::from_utf16_lossy(&units[..length]),
                _ => format!("0x{format:04X}"),
            }
        }))
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
                    crate::stderr_road::say(format!(
                        "clipboard picture: CF_BITMAP passed over: {reason}"
                    ))
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
    bounded_global_bytes(format, MAX_PICTURE_BYTES)
}

/// [`global_bytes`] under a ceiling of the caller's.
fn bounded_global_bytes(format: u32, ceiling: usize) -> Option<Vec<u8>> {
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
        if size == 0 || size > ceiling {
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

impl Drop for WindowsClipboard<'_> {
    fn drop(&mut self) {
        if self.opened {
            // SAFETY: unwind cleanup for this object's own open interval.
            let _ = unsafe { CloseClipboard() };
        }
    }
}

/// **Whatever the clipboard holds, as a paste reads it** — on the clipboard lane's worker
/// (`worker`), which holds the one open interval, its survey included — with the sequence it was
/// read at and the format names offered when the answer is nothing or a promise; the read line
/// is the caller's to write (plan row 81).
pub fn clipboard_payload(worker: &WorkerCtx) -> ClipboardRead {
    // SAFETY: diagnostic context only. Delayed rendering may advance this value successfully;
    // equality after GetClipboardData would reject that success (design ruling ④). Read before
    // the open, so a read whose open fails still has one.
    let sequence = unsafe { GetClipboardSequenceNumber() };
    let read = read_payload_offering(&mut WindowsClipboard {
        worker,
        opened: false,
    });
    let (answer, offered) = match read {
        Ok(read) => (Ok(read.payload), read.offered),
        Err(reason) => (Err(reason), Vec::new()),
    };
    ClipboardRead {
        sequence: Some(ClipboardSequence(u64::from(sequence))),
        offered,
        answer,
    }
}

/// **Render a clipboard promise into `into`** (plan row 81) — on a worker of its own
/// (`worker`), never the window thread and never the clipboard lane's: the OLE clipboard is asked
/// for its data object through the clipboard's one declared contention wait, and
/// [`render_promise`] reads the descriptor again, which must be `offer`, and writes each file,
/// each step advancing `progress`. The thread joins a single-threaded apartment for the call and
/// leaves it on every road out, a panic's too ([`Apartment`]); the data object is let go before
/// it does.
///
/// # Errors
/// Every [`PromiseRefused`]; an apartment or a clipboard that cannot be had is `NotRendered`
/// with its result code.
pub fn render_clipboard_promise(
    worker: &WorkerCtx,
    offer: &[PromisedEntry],
    into: &Path,
    progress: &RenderProgress,
) -> Result<RenderedPromise, PromiseRefused> {
    let _apartment = Apartment::single_threaded().map_err(|error| not_rendered(&error))?;
    let data = crate::windows_impl::ole_clipboard_with_retry(worker)
        .map_err(|error| not_rendered(&error))?;
    // Declared after the apartment, so it is let go first.
    let mut contents = OleContents::of(data)?;
    render_promise(worker, &mut contents, offer, into, PASTE_LIMITS, progress)
}

/// **This thread's single-threaded apartment, joined for a render and left when the value goes**
/// (review N1) — on every road out, a panic's unwinding too, so a render whose source panicked
/// does not leave its thread inside an apartment OLE set up. Declared before the data object, which
/// is then let go first. Not `Send`: it is left on the thread that joined.
pub(crate) struct Apartment(std::marker::PhantomData<*const ()>);

impl Apartment {
    pub(crate) fn single_threaded() -> windows::core::Result<Self> {
        // SAFETY: a thread of this process's own, joining an apartment this value leaves.
        unsafe { OleInitialize(None) }?;
        Ok(Self(std::marker::PhantomData))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balances `single_threaded`'s `OleInitialize`, on the thread that joined (the value
        // is not `Send`).
        unsafe { OleUninitialize() };
    }
}

fn not_rendered(error: &windows::core::Error) -> PromiseRefused {
    PromiseRefused::NotRendered(Some(error.code().0.cast_unsigned()))
}

/// **A promise's bytes, asked of a data object** — the OLE clipboard's in the product, an
/// in-process one under test — by `FileGroupDescriptorW` and `FileContents` with the entry's
/// index as its `lindex`.
pub(crate) struct OleContents {
    data: IDataObject,
    descriptor: u16,
    contents: u16,
}

/// **One medium a data object handed over**, released when it goes.
struct Medium(STGMEDIUM);

impl Drop for Medium {
    fn drop(&mut self) {
        // SAFETY: the medium `GetData` answered, released exactly once.
        unsafe { ReleaseStgMedium(&raw mut self.0) };
    }
}

/// The chunk a promised stream is read in.
const STREAM_CHUNK: usize = 64 * 1024;

impl OleContents {
    pub(crate) fn of(data: IDataObject) -> Result<Self, PromiseRefused> {
        // SAFETY: registering an already registered name answers its identifier.
        let (descriptor, contents) = unsafe {
            (
                RegisterClipboardFormatW(w!("FileGroupDescriptorW")),
                RegisterClipboardFormatW(w!("FileContents")),
            )
        };
        match (u16::try_from(descriptor), u16::try_from(contents)) {
            (Ok(descriptor), Ok(contents)) if descriptor != 0 && contents != 0 => Ok(Self {
                data,
                descriptor,
                contents,
            }),
            _ => Err(PromiseRefused::NotRendered(None)),
        }
    }

    /// **One medium asked for**, through the clipboard's contention wait (review N3): the OLE
    /// clipboard's data object opens the clipboard to answer `GetData`, and answers
    /// `CLIPBRD_E_CANT_OPEN` while another program holds it, as `OleGetClipboard` does.
    fn get(
        &self,
        worker: &WorkerCtx,
        format: u16,
        lindex: i32,
        tymed: u32,
    ) -> Result<Medium, PromiseRefused> {
        let asked = FORMATETC {
            cfFormat: format,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex,
            tymed,
        };
        let mut contention = crate::windows_impl::ClipboardContention::new();
        loop {
            // SAFETY: `asked` lives across each call; the medium answered is owned by `Medium`.
            let answer = unsafe { self.data.GetData(&raw const asked) };
            if !contention.ask_again(worker, &answer) {
                return answer.map(Medium).map_err(|error| not_rendered(&error));
            }
        }
    }
}

/// **Let the thread's own messages through** between two chunks: an apartment that holds OLE's
/// clipboard window must not keep a broadcast waiting while it writes.
fn let_messages_through() {
    let mut message = MSG::default();
    // SAFETY: `message` is a live local; only this thread's own queue is read.
    unsafe {
        while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
}

/// `TYMED` as `FORMATETC` and `STGMEDIUM` spell it.
fn tymed(kind: windows::Win32::System::Com::TYMED) -> u32 {
    kind.0.cast_unsigned()
}

impl PromisedContents for OleContents {
    /// The descriptor read again. A board that no longer offers one (`DV_E_FORMATETC`: the
    /// format is not there) holds something else now: the paste is refused `changed`, as a
    /// descriptor that differs is (review N9).
    fn descriptor(&mut self, worker: &WorkerCtx) -> Result<Vec<PromisedEntry>, PromiseRefused> {
        let medium = self
            .get(worker, self.descriptor, -1, tymed(TYMED_HGLOBAL))
            .map_err(|refused| match refused {
                PromiseRefused::NotRendered(Some(code))
                    if code == DV_E_FORMATETC.0.cast_unsigned() =>
                {
                    PromiseRefused::Changed
                }
                other => other,
            })?;
        if medium.0.tymed != tymed(TYMED_HGLOBAL) {
            return Err(PromiseRefused::NotRendered(None));
        }
        // SAFETY: an HGLOBAL medium; `GlobalSize` bounds the slice and the copy is made before
        // the unlock.
        let bytes = unsafe {
            let global = medium.0.u.hGlobal;
            let size = GlobalSize(global);
            if size == 0 || size > MAX_DESCRIPTOR_BYTES {
                return Err(PromiseRefused::NotRendered(None));
            }
            let pointer = GlobalLock(global).cast::<u8>();
            if pointer.is_null() {
                return Err(PromiseRefused::NotRendered(None));
            }
            let bytes = std::slice::from_raw_parts(pointer, size).to_vec();
            let _ = GlobalUnlock(global);
            bytes
        };
        parse_file_group_descriptor(&bytes).ok_or(PromiseRefused::NotRendered(None))
    }

    fn write_contents(
        &mut self,
        worker: &WorkerCtx,
        index: usize,
        file: &mut PromisedFile<'_>,
    ) -> Result<(), PromiseRefused> {
        let lindex = i32::try_from(index).map_err(|_| PromiseRefused::NotRendered(None))?;
        let medium = self.get(
            worker,
            self.contents,
            lindex,
            tymed(TYMED_ISTREAM) | tymed(TYMED_HGLOBAL) | tymed(TYMED_ISTORAGE),
        )?;
        let answered = medium.0.tymed;
        if answered == tymed(TYMED_ISTREAM) {
            // SAFETY: an ISTREAM medium carries a stream for as long as the medium is held.
            let Some(stream) = (unsafe { medium.0.u.pstm.as_ref() }) else {
                return Err(PromiseRefused::NotRendered(None));
            };
            return copy_stream(stream, file);
        }
        if answered == tymed(TYMED_HGLOBAL) {
            // SAFETY: an HGLOBAL medium; the slice is bounded by `GlobalSize` (and by the
            // declared size, a global being rounded up) and written before the unlock.
            unsafe {
                let global = medium.0.u.hGlobal;
                let size = GlobalSize(global);
                let length = file
                    .declared()
                    .and_then(|declared| usize::try_from(declared).ok())
                    .map_or(size, |declared| declared.min(size));
                if length == 0 {
                    return Ok(());
                }
                let pointer = GlobalLock(global).cast::<u8>();
                if pointer.is_null() {
                    return Err(PromiseRefused::NotRendered(None));
                }
                let written = file.write(std::slice::from_raw_parts(pointer, length));
                let _ = GlobalUnlock(global);
                return written;
            }
        }
        if answered == tymed(TYMED_ISTORAGE) {
            // SAFETY: an ISTORAGE medium carries a storage for as long as the medium is held.
            let Some(storage) = (unsafe { medium.0.u.pstg.as_ref() }) else {
                return Err(PromiseRefused::NotRendered(None));
            };
            let path = HSTRING::from(file.path().as_os_str());
            // SAFETY: a new compound file at the paste's own path, written whole by `CopyTo`
            // and committed before it is let go. `STGM_FAILIFTHERE`: a file already at the path
            // is never written over (review N4), as `PromisedFile`'s own files are made with
            // `create_new`.
            unsafe {
                let copy = StgCreateDocfile(
                    &path,
                    STGM_FAILIFTHERE | STGM_READWRITE | STGM_SHARE_EXCLUSIVE,
                    None,
                )
                .map_err(|error| {
                    if error.code() == STG_E_FILEALREADYEXISTS {
                        PromiseRefused::WriteFailed(std::io::ErrorKind::AlreadyExists)
                    } else {
                        not_rendered(&error)
                    }
                })?;
                storage
                    .CopyTo(None, None, &copy)
                    .map_err(|error| not_rendered(&error))?;
                copy.Commit(0).map_err(|error| not_rendered(&error))?;
            }
            return file.written_by_the_source(worker);
        }
        Err(PromiseRefused::NotRendered(None))
    }
}

/// **A promised stream, copied chunk by chunk** into its file, the paste's bytes charged as each
/// chunk lands.
fn copy_stream(stream: &IStream, file: &mut PromisedFile<'_>) -> Result<(), PromiseRefused> {
    let mut chunk = vec![0_u8; STREAM_CHUNK];
    loop {
        let mut read = 0_u32;
        // SAFETY: `chunk` is `STREAM_CHUNK` bytes the call may fill; `read` is a live local.
        let result = unsafe {
            stream.Read(
                chunk.as_mut_ptr().cast(),
                u32::try_from(STREAM_CHUNK).expect("a small chunk"),
                Some(&raw mut read),
            )
        };
        if result.is_err() {
            return Err(PromiseRefused::NotRendered(Some(result.0.cast_unsigned())));
        }
        let read = usize::try_from(read).expect("at most a chunk");
        if read == 0 {
            return Ok(());
        }
        file.write(&chunk[..read])?;
        let_messages_through();
    }
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

/// **The promise's OLE adapter, driven by a data object of the test's own** (plan row 81): every
/// medium a source may hand `FileContents` over in, and a source that fails, through
/// [`OleContents`] and the platform's renderer — the machine's clipboard is never opened.
#[cfg(test)]
mod promise_tests {
    use std::mem::ManuallyDrop;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows::Win32::Foundation::{CLIPBRD_E_CANT_OPEN, E_NOTIMPL, HGLOBAL};
    use windows::Win32::System::Com::StructuredStorage::{
        CreateILockBytesOnHGlobal, CreateStreamOnHGlobal, StgCreateDocfileOnILockBytes,
    };
    use windows::Win32::System::Com::{
        IAdviseSink, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, STGM_CREATE, STGM_WRITE,
        STGMEDIUM_0, STREAM_SEEK_SET, TYMED,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc};
    use windows::core::{BOOL, HRESULT, Ref, implement};

    use super::*;
    use crate::clipboard_promise::tests::{descriptor_bytes, on_a_worker};
    use crate::clipboard_promise::{FILE_DESCRIPTOR_BYTES, RenderProgress, render_promise};

    /// How one entry's contents are handed over.
    #[derive(Clone)]
    enum Handed {
        Global(Vec<u8>),
        Stream(Vec<u8>),
        /// A structured storage holding one stream of these bytes, as a mail client hands a
        /// message over.
        Storage(Vec<u8>),
        Fails(HRESULT),
    }

    /// The test's data object. An empty `descriptor` is a board that no longer offers one;
    /// `contended` asks are answered `CLIPBRD_E_CANT_OPEN` first, as while another program holds
    /// the clipboard open.
    #[implement(IDataObject)]
    struct Board {
        descriptor: Vec<u8>,
        contents: Vec<Handed>,
        contended: AtomicUsize,
    }

    fn global_of(bytes: &[u8]) -> windows::core::Result<HGLOBAL> {
        // SAFETY: a fresh global, filled while locked, and handed to the medium that frees it.
        unsafe {
            let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))?;
            let pointer = GlobalLock(global).cast::<u8>();
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
            let _ = GlobalUnlock(global);
            Ok(global)
        }
    }

    fn stream_of(bytes: &[u8]) -> windows::core::Result<IStream> {
        // SAFETY: a stream over a global of its own, written and wound back to its start.
        unsafe {
            let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true)?;
            let mut written = 0_u32;
            stream
                .Write(
                    bytes.as_ptr().cast(),
                    u32::try_from(bytes.len()).expect("a test's bytes"),
                    Some(&raw mut written),
                )
                .ok()?;
            stream.Seek(0, STREAM_SEEK_SET, None)?;
            Ok(stream)
        }
    }

    fn medium(tymed: TYMED, u: STGMEDIUM_0) -> STGMEDIUM {
        STGMEDIUM {
            tymed: super::tymed(tymed),
            u,
            pUnkForRelease: ManuallyDrop::new(None),
        }
    }

    impl IDataObject_Impl for Board_Impl {
        fn GetData(&self, asked: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
            if self
                .contended
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |left| {
                    left.checked_sub(1)
                })
                .is_ok()
            {
                return Err(CLIPBRD_E_CANT_OPEN.into());
            }
            // SAFETY: the caller's FORMATETC, read while the call lasts.
            let asked = unsafe { &*asked };
            // SAFETY: registering an already registered name answers its identifier.
            let (descriptor, contents) = unsafe {
                (
                    RegisterClipboardFormatW(w!("FileGroupDescriptorW")),
                    RegisterClipboardFormatW(w!("FileContents")),
                )
            };
            if u32::from(asked.cfFormat) == descriptor {
                if self.descriptor.is_empty() {
                    return Err(windows::Win32::Foundation::DV_E_FORMATETC.into());
                }
                return Ok(medium(
                    TYMED_HGLOBAL,
                    STGMEDIUM_0 {
                        hGlobal: global_of(&self.descriptor)?,
                    },
                ));
            }
            assert_eq!(
                u32::from(asked.cfFormat),
                contents,
                "only the promise is asked for"
            );
            let index = usize::try_from(asked.lindex).expect("an entry's index");
            match &self.contents[index] {
                Handed::Global(bytes) => Ok(medium(
                    TYMED_HGLOBAL,
                    STGMEDIUM_0 {
                        hGlobal: global_of(bytes)?,
                    },
                )),
                Handed::Stream(bytes) => Ok(medium(
                    TYMED_ISTREAM,
                    STGMEDIUM_0 {
                        pstm: ManuallyDrop::new(Some(stream_of(bytes)?)),
                    },
                )),
                Handed::Storage(bytes) => {
                    // SAFETY: a compound file over a global of its own, one stream written into
                    // it and committed before it is handed over.
                    let storage = unsafe {
                        let lock_bytes = CreateILockBytesOnHGlobal(None, true)?;
                        let storage = StgCreateDocfileOnILockBytes(
                            &lock_bytes,
                            STGM_CREATE | STGM_READWRITE | STGM_SHARE_EXCLUSIVE,
                            0,
                        )?;
                        let body = storage.CreateStream(
                            w!("body"),
                            STGM_CREATE | STGM_WRITE | STGM_SHARE_EXCLUSIVE,
                            0,
                            0,
                        )?;
                        body.Write(
                            bytes.as_ptr().cast(),
                            u32::try_from(bytes.len()).expect("a test's bytes"),
                            None,
                        )
                        .ok()?;
                        storage.Commit(0)?;
                        storage
                    };
                    Ok(medium(
                        TYMED_ISTORAGE,
                        STGMEDIUM_0 {
                            pstg: ManuallyDrop::new(Some(storage)),
                        },
                    ))
                }
                Handed::Fails(code) => Err((*code).into()),
            }
        }
        fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn QueryGetData(&self, _: *const FORMATETC) -> HRESULT {
            E_NOTIMPL
        }
        fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, _: *mut FORMATETC) -> HRESULT {
            E_NOTIMPL
        }
        fn SetData(
            &self,
            _: *const FORMATETC,
            _: *const STGMEDIUM,
            _: BOOL,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn EnumFormatEtc(&self, _: u32) -> windows::core::Result<IEnumFORMATETC> {
            Err(E_NOTIMPL.into())
        }
        fn DAdvise(
            &self,
            _: *const FORMATETC,
            _: u32,
            _: Ref<IAdviseSink>,
        ) -> windows::core::Result<u32> {
            Err(E_NOTIMPL.into())
        }
        fn DUnadvise(&self, _: u32) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
            Err(E_NOTIMPL.into())
        }
    }

    /// `FD_FILESIZE`, `FD_ATTRIBUTES` and `FILE_ATTRIBUTE_DIRECTORY`.
    const SIZED: u32 = 0x40;
    const ATTRIBUTED: u32 = 0x4;
    const DIRECTORY: u32 = 0x10;

    fn units(name: &str) -> Vec<u16> {
        name.encode_utf16().collect()
    }

    /// The data object, as `OleGetClipboard` would answer it, and the offer the lane would have
    /// read from its descriptor.
    fn board(
        entries: &[(u32, u32, u64, &str)],
        contents: Vec<Handed>,
    ) -> (OleContents, Vec<PromisedEntry>) {
        let named: Vec<Vec<u16>> = entries.iter().map(|entry| units(entry.3)).collect();
        let rows: Vec<(u32, u32, u64, &[u16])> = entries
            .iter()
            .zip(&named)
            .map(|(entry, name)| (entry.0, entry.1, entry.2, name.as_slice()))
            .collect();
        let descriptor = descriptor_bytes(&rows);
        let offer = parse_file_group_descriptor(&descriptor).expect("the descriptor reads");
        (data_object(descriptor, contents, 0), offer)
    }

    /// [`OleContents`] over a [`Board`] of these bytes, its first `contended` asks refused.
    fn data_object(descriptor: Vec<u8>, contents: Vec<Handed>, contended: usize) -> OleContents {
        let data: IDataObject = Board {
            descriptor,
            contents,
            contended: AtomicUsize::new(contended),
        }
        .into();
        OleContents::of(data).expect("the formats register")
    }

    /// `render_promise` with a progress of its own.
    fn rendered(
        worker: &WorkerCtx,
        contents: &mut OleContents,
        offer: &[PromisedEntry],
        into: &std::path::Path,
    ) -> Result<RenderedPromise, PromiseRefused> {
        render_promise(
            worker,
            contents,
            offer,
            into,
            PASTE_LIMITS,
            &RenderProgress::default(),
        )
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        bt_testpath::temp_path(&format!("bt-platform-ole-promise-{tag}"))
    }

    /// PIN — **`FILEDESCRIPTORW` is the 592 bytes the descriptor is read at**: the system's own
    /// layout, so a header that changed would be read wrong rather than quietly.
    #[test]
    fn a_file_descriptor_is_the_size_it_is_read_at() {
        assert_eq!(
            size_of::<windows::Win32::UI::Shell::FILEDESCRIPTORW>(),
            FILE_DESCRIPTOR_BYTES
        );
    }

    /// RED (review N1) — **the render's apartment is left on every road out, a panic's too**: a
    /// thread whose source panics inside the render unwinds through the apartment, and the thread
    /// is outside it after — joining one anew is `S_OK`, not the `S_FALSE` of a thread still in.
    ///
    /// MUTATION: empty `Apartment`'s `Drop` — the panicked thread is still inside its apartment.
    #[test]
    fn the_apartment_is_left_on_every_road_out() {
        use windows::Win32::Foundation::{S_FALSE, S_OK};
        use windows::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
        };
        std::thread::spawn(|| {
            let unwound = std::panic::catch_unwind(|| {
                let _apartment = Apartment::single_threaded().expect("an apartment");
                panic!("the source's code panics inside the render (planted by the test)");
            });
            assert!(unwound.is_err(), "the planted panic unwound");
            // SAFETY: the test's own thread, balanced by the `CoUninitialize` below.
            let joined = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            // SAFETY: balances the call above, on this thread, whichever success it answered.
            unsafe { CoUninitialize() };
            assert_ne!(
                joined, S_FALSE,
                "the thread was still inside the render's apartment"
            );
            assert_eq!(joined, S_OK);
        })
        .join()
        .expect("the test's thread ends");
    }

    /// RED (plan row 81, R5) — **every medium a source hands a promise over in is written**: a
    /// global (cut to the declared size, a global being rounded up), a stream longer than one
    /// chunk, a structured storage (written as a compound file) and a folder that is made and
    /// never asked for — through `OleContents` and the platform's renderer, and pasted as the
    /// top-level entries.
    ///
    /// MUTATION: ignore `TYMED_ISTREAM` in `OleContents::write_contents` — the stream's entry is
    /// `not-rendered`.
    #[test]
    fn every_medium_a_source_hands_over_is_written() {
        on_a_worker(|worker| {
            let apartment = Apartment::single_threaded().expect("an apartment");
            let long: Vec<u8> = (0..200_000_u32).map(|n| (n % 251) as u8).collect();
            let (mut contents, offer) = board(
                &[
                    (SIZED, 0, 5, "a 文件.txt"),
                    (0, 0, 0, "long.bin"),
                    (0, 0, 0, "mail.msg"),
                    (ATTRIBUTED, DIRECTORY, 0, "photos"),
                    (SIZED, 0, 3, "photos\\b.png"),
                ],
                vec![
                    Handed::Global(b"hello and more".to_vec()),
                    Handed::Stream(long.clone()),
                    Handed::Storage(b"message body".to_vec()),
                    Handed::Fails(E_NOTIMPL),
                    Handed::Global(b"png".to_vec()),
                ],
            );
            let into = scratch("media");
            std::fs::create_dir_all(&into).expect("the paste's folder");
            let rendered = rendered(worker, &mut contents, &offer, &into);
            // The data object is let go before the apartment is left (review N8).
            drop(contents);
            drop(apartment);
            let rendered = rendered.expect("every medium is written");
            assert_eq!(
                rendered.pasted,
                [
                    into.join("a 文件.txt"),
                    into.join("long.bin"),
                    into.join("mail.msg"),
                    into.join("photos"),
                ]
            );
            assert_eq!(rendered.files, 4);
            assert_eq!(std::fs::read(into.join("a 文件.txt")).unwrap(), b"hello");
            assert_eq!(std::fs::read(into.join("long.bin")).unwrap(), long);
            let message = std::fs::read(into.join("mail.msg")).unwrap();
            assert_eq!(
                &message[..8],
                [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
                "a compound file's signature"
            );
            assert_eq!(
                std::fs::read(into.join("photos").join("b.png")).unwrap(),
                b"png"
            );
            let _ = std::fs::remove_dir_all(&into);
        });
    }

    /// RED (plan row 81) — **a source that fails, or whose board changed, refuses the paste with
    /// its result code and leaves nothing**: the code the data object answered is what the refusal
    /// says; a descriptor that is not the offer is `changed` before any content is asked for.
    #[test]
    fn a_source_that_fails_or_changed_leaves_nothing() {
        on_a_worker(|worker| {
            let apartment = Apartment::single_threaded().expect("an apartment");
            let gone = HRESULT(0x8001_0108_u32.cast_signed());
            let (mut contents, offer) = board(
                &[(SIZED, 0, 2, "a.txt"), (SIZED, 0, 2, "b.txt")],
                vec![Handed::Global(b"aa".to_vec()), Handed::Fails(gone)],
            );
            let into = scratch("fails");
            std::fs::create_dir_all(&into).expect("the paste's folder");
            let failed = rendered(worker, &mut contents, &offer, &into);
            let (mut changed, _) = board(&[(SIZED, 0, 2, "c.txt")], vec![Handed::Fails(gone)]);
            let elsewhere = scratch("changed");
            std::fs::create_dir_all(&elsewhere).expect("the paste's folder");
            let raced = rendered(worker, &mut changed, &offer, &elsewhere);
            let mut emptied = data_object(Vec::new(), Vec::new(), 0);
            let emptied_into = scratch("emptied");
            std::fs::create_dir_all(&emptied_into).expect("the paste's folder");
            let no_longer = rendered(worker, &mut emptied, &offer, &emptied_into);
            // The data objects are let go before the apartment is left (review N8).
            drop((contents, changed, emptied));
            drop(apartment);
            assert_eq!(failed, Err(PromiseRefused::NotRendered(Some(0x8001_0108))));
            assert!(!into.exists(), "the first file went with the refusal");
            assert_eq!(raced, Err(PromiseRefused::Changed));
            assert!(!elsewhere.exists());
            assert_eq!(
                no_longer,
                Err(PromiseRefused::Changed),
                "a board that no longer offers a descriptor holds something else (review N9)"
            );
            assert!(!emptied_into.exists());
        });
    }

    /// RED (review N3) — **a data object that answers `CLIPBRD_E_CANT_OPEN` is asked again**
    /// through the clipboard's contention wait, as `OleGetClipboard` is: four refused asks and the
    /// paste is written; five, and it is refused with the code.
    ///
    /// MUTATION: ask `GetData` once in `OleContents::get` — the contended board is `not-rendered`.
    #[test]
    fn a_contended_data_object_is_asked_again_through_the_wait() {
        on_a_worker(|worker| {
            let apartment = Apartment::single_threaded().expect("an apartment");
            let entries = [(SIZED, 0, 2, "a.txt")];
            let (_, offer) = board(&entries, vec![Handed::Global(b"aa".to_vec())]);
            let descriptor = descriptor_bytes(&[(SIZED, 0, 2, units("a.txt").as_slice())]);
            let mut patient =
                data_object(descriptor.clone(), vec![Handed::Global(b"aa".to_vec())], 4);
            let into = scratch("contended");
            std::fs::create_dir_all(&into).expect("the paste's folder");
            let written = rendered(worker, &mut patient, &offer, &into);
            let mut held = data_object(descriptor, vec![Handed::Global(b"aa".to_vec())], 5);
            let refused_into = scratch("held");
            std::fs::create_dir_all(&refused_into).expect("the paste's folder");
            let refused = rendered(worker, &mut held, &offer, &refused_into);
            drop((patient, held));
            drop(apartment);
            assert_eq!(written.map(|promise| promise.files), Ok(1));
            assert_eq!(std::fs::read(into.join("a.txt")).unwrap(), b"aa");
            assert_eq!(
                refused,
                Err(PromiseRefused::NotRendered(Some(
                    CLIPBRD_E_CANT_OPEN.0.cast_unsigned()
                )))
            );
            assert!(!refused_into.exists());
            let _ = std::fs::remove_dir_all(&into);
        });
    }

    /// RED (reviews N4, N5) — **a storage is never written over a file already there, and a byte
    /// medium shorter than its declaration did not render**: a structured storage whose path is
    /// taken is refused `write-failed: AlreadyExists`, as a stream or a global is by `create_new`;
    /// an empty global, or a stream that ends early, under a declared size is `not-rendered`.
    ///
    /// MUTATIONS: `STGM_CREATE` back in `OleContents::write_contents` — the storage is written over
    /// the file (`Ok`); drop the declared-size check in `PromisedFile::finish` — the short entries
    /// are written.
    #[test]
    fn a_storage_never_writes_over_and_a_short_medium_did_not_render() {
        on_a_worker(|worker| {
            let apartment = Apartment::single_threaded().expect("an apartment");
            let (mut storage, offer) = board(
                &[(0, 0, 0, "mail.msg")],
                vec![Handed::Storage(b"message body".to_vec())],
            );
            let into = scratch("taken");
            std::fs::create_dir_all(&into).expect("the paste's folder");
            std::fs::write(into.join("mail.msg"), b"already here").expect("a file in the way");
            let over = rendered(worker, &mut storage, &offer, &into);
            let mut short = Vec::new();
            for handed in [Handed::Global(Vec::new()), Handed::Stream(b"ab".to_vec())] {
                let (mut contents, offer) = board(&[(SIZED, 0, 64, "short.bin")], vec![handed]);
                let short_into = scratch("short");
                std::fs::create_dir_all(&short_into).expect("the paste's folder");
                short.push((
                    rendered(worker, &mut contents, &offer, &short_into),
                    short_into.exists(),
                ));
                drop(contents);
            }
            drop(storage);
            drop(apartment);
            assert_eq!(
                over,
                Err(PromiseRefused::WriteFailed(
                    std::io::ErrorKind::AlreadyExists
                ))
            );
            assert_eq!(
                short,
                [
                    (Err(PromiseRefused::NotRendered(None)), false),
                    (Err(PromiseRefused::NotRendered(None)), false),
                ]
            );
        });
    }
}
