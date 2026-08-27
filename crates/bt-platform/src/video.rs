//! **One frame out of a video file, and the two facts that come with it**
//! (user ruling 2026-08-27; `docs/DESIGN.md` §7.21).
//!
//! A fifth unsafe boundary in this crate, against a fifth thing. `windows_impl`
//! is Win32 for the window's sake, [`crate::webview`] is WebView2, [`crate::hang`]
//! is Win32 turned on this process, [`crate::attention_pipe`] is a channel other
//! programs speak into — and this is **Media Foundation**, the decoder Windows
//! ships, asked one question: what does this file look like, and how long is it.
//!
//! # Why it exists at all
//!
//! Until this module a `.mp4` in the files column was `PreviewFtype::Unknown`:
//! the hover card said "No preview for this file type" and a double click said
//! the same thing larger. The engine on the preview seat cannot help — measured
//! on 2026-08-25 and written up in `docs/DESIGN.md` §7.16, a top-level media
//! response in WebView2 becomes a *download* and the platform bridge cancels
//! every download, so a video routed to the page lane draws a browser error
//! where an honest refusal used to be. And there is no video decoder in this
//! build to write one with: the whole lock file has neither an `ffmpeg`, a
//! `symphonia`, an `openh264` nor a `dav1d` in it, and `docs/DESIGN.md` §8's bar
//! for this product is that a feature costs **no new package**.
//!
//! Windows has the decoder. `IMFSourceReader` with
//! `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING` is the official shape of exactly
//! this request — Microsoft's own words on that attribute are "**This feature is
//! intended for applications that process a small number of frames—for example,
//! to create a video thumbnail**" — and reaching it costs one `windows` crate
//! feature rather than one crate.
//!
//! # The thread this runs on, and why it is its own
//!
//! Media Foundation asks to be spoken to from a **multithreaded apartment**:
//! "Work queues always have multithreaded apartment (MTA) threads … it is
//! recommended to call **CoInitializeEx** with the **COINIT_MULTITHREADED**
//! flag" ([Media Foundation and
//! COM](https://learn.microsoft.com/en-us/windows/win32/medfound/media-foundation-and-com)).
//! Every apartment this process had before today is a single-threaded one on the
//! window's own event loop (the taskbar button, the toast notifier, the file
//! chooser), and the decoration worker — the thread that answers a hover with a
//! formula, a decoded picture or a rastered PDF page — has no apartment at all.
//!
//! So [`first_frame`] does not run where it is called. It **spawns a thread**,
//! and that thread owns the whole apartment for the length of one question:
//! `CoInitializeEx(COINIT_MULTITHREADED)` and `MFStartup` on the way in,
//! `MFShutdown` and `CoUninitialize` on the way out, paired on every path
//! including the failing ones ("For every call to **MFStartup**, your application
//! must call **MFShutdown**"). Nothing about Media Foundation outlives the
//! answer, and no other lane's COM semantics are touched.
//!
//! **A thread per question rather than one kept warm**, and the reason is the
//! budget below. `MFStartup`/`MFShutdown` are not free, but a file is asked
//! about once — the window caches the pixels against the file's modification
//! time, exactly as it does a PDF's page — so the cost is paid on a first hover
//! and never again. What a fresh thread buys is the one thing a shared one
//! cannot give: a caller that can **stop waiting**. `ReadSample` is synchronous
//! and a malformed file can send a demuxer a long way; [`FIRST_FRAME_BUDGET`] is
//! honoured by the caller giving up on the channel, and the thread it walked
//! away from finishes into a receiver nobody is holding and unwinds itself.
//!
//! # What it refuses, and the shape of every refusal
//!
//! `None`, always — one silence rather than a vocabulary of failures, which is
//! the same word the PDF rasteriser on the other side of this lane answers with.
//! It is the answer for a file that is not a video, a container Media
//! Foundation has no source for (`.mkv` — Matroska is not among the eight
//! containers the platform ships), a codec with no decoder installed, a file
//! truncated mid-download, and a question that ran past [`FIRST_FRAME_BUDGET`].
//! The card and the pane draw the same thing for all five: no picture, and the
//! two lines that are still true.
//!
//! # The set of files this can draw is not the set that could be played
//!
//! It is worth stating because the two look like one table and are not.
//! `.mov` is a **MPEG-4 File Source** container to Media Foundation and reads
//! here; the same file's `canPlayType` in the engine on the preview seat is the
//! empty string, measured. The opposite mismatch exists too — a VP9 or AV1
//! `.webm` plays in that engine, which carries its own decoders, and may have no
//! Media Foundation decoder on a stock machine at all. A picture is a promise
//! that this window can *show* you something; playing is a different promise and
//! will get a different table when it is made.

use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFMediaBuffer, IMFSourceReader, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READER_MEDIASOURCE, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFCreateAttributes,
    MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Video, MFSTARTUP_NOSOCKET,
    MFShutdown, MFStartup, MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::{HSTRING, Interface};

/// **How long one question may take before the caller stops waiting.**
///
/// A hover is a gesture, and three seconds is already well past the point where
/// a reader has concluded that nothing is coming; what the number is really for
/// is the file that would take *minutes* — a several-gigabyte capture on a
/// network share, a container whose index is at the far end of it, a demuxer
/// walking a stream that never yields a key frame. Past this the answer is
/// [`None`] and the card degrades to the two lines it can always state.
///
/// It bounds the **wait** and not the work: `ReadSample` is a synchronous call
/// into somebody else's demuxer and there is no supported way to interrupt one.
/// The thread that overran is left to finish and unwind on its own — see the
/// module note — which is why it holds no lock and writes to nothing but its own
/// channel.
pub const FIRST_FRAME_BUDGET: Duration = Duration::from_secs(3);

/// **Where in the video the frame is taken from**, as a fraction of its length.
///
/// Not zero, and that is the whole of this constant. A great many videos open on
/// black — a fade-in, a slate, a leader, a camera that had not metered yet — and
/// a thumbnail lane that took the first frame would answer a hover over half a
/// folder of screen captures with half a folder of identical black rectangles.
/// One tenth in is past every opening of that kind and still inside the first
/// shot of almost everything, which is the same reasoning Microsoft's own
/// `VideoThumbnail` sample uses when it seeks by a proportion of the duration
/// rather than to a timestamp.
///
/// The seek is not exact and is not asked to be: `IMFSourceReader::SetCurrentPosition`
/// "typically" lands on the nearest key frame at or before the target, and a key
/// frame is precisely what this wants — the alternative is decoding forward from
/// it one sample at a time to reach a picture no reader could tell apart.
///
/// A file whose duration the container does not declare is read from wherever it
/// starts, because a fraction of an unknown length is not a position.
pub const SEEK_FRACTION: f64 = 0.10;

/// **One frame, and everything the same open already knew.**
///
/// `rgba` is straight (non-premultiplied) RGBA8, row-major, fully opaque — the
/// shape every other picture in this window reaches the renderer as, so the
/// frame joins the picture channel rather than needing one of its own.
///
/// **`width`/`height` are the raster's and `native_width`/`native_height` are the
/// video's**, and they are two facts rather than one because Media Foundation may
/// answer a request for a smaller output type by scaling. What the card fits into
/// its box is the raster; what the fact line says the video *is* is the native
/// size, which is what a reader hovering a capture wants to know and would be
/// quietly wrong if it were read off the pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// How long the video is, in milliseconds, or `None` when the container does
    /// not declare a duration. Read off `MF_PD_DURATION`, which is in units of
    /// 100 nanoseconds.
    pub duration_ms: Option<u64>,
    pub native_width: u32,
    pub native_height: u32,
}

/// **A frame out of the video at `path`, fitted inside `fit_width` ×
/// `fit_height`**, or `None` for every refusal in the module's own note.
///
/// The fit is a **request and a cap, not a promise**. It is passed to Media
/// Foundation as the output type's frame size, which the video processor honours
/// when it can and ignores when it cannot; either way the frame that comes back
/// carries its own dimensions and no caller may assume the box. It is never an
/// enlargement — a 320×240 clip asked for at 1280×720 comes back at 320×240,
/// because the pixels a scaler would invent are not the file's and the window's
/// own sampler already stretches what it is given.
///
/// # Where it may be called from
///
/// **Never the thread that draws.** Opening a container, seeking it and decoding
/// a frame is tens to hundreds of milliseconds and on the window's thread that is
/// a hover freezing the window — the same sentence that already keeps the PDF
/// rasteriser on the decoration worker, which is also this function's one caller.
#[must_use]
pub fn first_frame(path: &Path, fit_width: u32, fit_height: u32) -> Option<VideoFrame> {
    // Resolved before the thread, so what crosses is a string and not a borrow.
    // A path with no valid UTF-16 spelling is not a path this can be handed to
    // `MFCreateSourceReaderFromURL`.
    let url = HSTRING::from(path.as_os_str());
    let (fit_width, fit_height) = (fit_width.max(1), fit_height.max(1));
    let (answer, wait) = std::sync::mpsc::channel();
    // The apartment, Media Foundation's startup and every interface below all
    // live and die inside this closure; see the module note for why it is a
    // thread of its own and why the caller is allowed to walk away from it.
    std::thread::Builder::new()
        .name("folio-video-frame".to_owned())
        .spawn(move || {
            let frame = in_its_own_apartment(|| read_first_frame(&url, fit_width, fit_height));
            // The receiver is gone when the budget ran out. That is an ordinary
            // ending for this thread and not an error: the answer is dropped
            // here exactly as it would have been dropped there.
            let _ = answer.send(frame);
        })
        .ok()?;
    wait.recv_timeout(FIRST_FRAME_BUDGET).ok().flatten()
}

/// Run `work` on this thread with an MTA and a started Media Foundation, and
/// give both back afterwards **whatever `work` did**.
///
/// The pairing the platform asks for, written once so no early return can skip
/// half of it: `CoUninitialize` only for an apartment this call actually
/// entered, and `MFShutdown` only for a `MFStartup` that actually returned.
fn in_its_own_apartment(work: impl FnOnce() -> Option<VideoFrame>) -> Option<VideoFrame> {
    // SAFETY: this runs on a thread this function has just created and is the
    // only code that will ever run on it. Nothing else in this process holds an
    // apartment here, so `CoInitializeEx` cannot be changing the mode of one
    // somebody else is using, and the two releases below run on every path.
    unsafe {
        let apartment = CoInitializeEx(None, COINIT_MULTITHREADED);
        // A fresh thread has no apartment, so the only way this fails is the
        // system refusing outright — in which case Media Foundation has nothing
        // to stand on and the answer is the module's one silence.
        if apartment.is_err() {
            return None;
        }
        // `NOSOCKET` rather than `FULL`: this reads local files and the sockets
        // half of the platform is the network source it will never open.
        let started = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).is_ok();
        let frame = if started { work() } else { None };
        if started {
            let _ = MFShutdown();
        }
        CoUninitialize();
        frame
    }
}

/// The whole of the Media Foundation conversation, on a thread that already has
/// an apartment and a started platform.
fn read_first_frame(url: &HSTRING, fit_width: u32, fit_height: u32) -> Option<VideoFrame> {
    let deadline = Instant::now() + FIRST_FRAME_BUDGET;
    // SAFETY: every call below is a COM method on an interface this function
    // created and holds, on the thread that created it, inside the apartment
    // `in_its_own_apartment` entered. The one raw pointer that leaves an
    // interface is the locked buffer, and it is read inside the lock and not
    // kept past `copy_locked_frame`.
    unsafe {
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1).ok()?;
        let attributes = attributes?;
        // **The one attribute this lane is built on.** With it the reader will
        // convert YUV to RGB-32 in software for us; without it a reader whose
        // stream is anything but RGB simply refuses the output type below and
        // every video in the world answers `None`. Microsoft's own note on it
        // names this exact use — a video thumbnail — and its two companions
        // (`MF_SOURCE_READER_D3D_MANAGER`, `MF_READWRITE_DISABLE_CONVERTERS`)
        // must stay unset, which they are: this is the only attribute set here.
        attributes
            .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
            .ok()?;
        let reader = MFCreateSourceReaderFromURL(url, &attributes).ok()?;

        // Nothing but the video. An audio stream that is still selected is a
        // stream the reader will hand us samples from, and a sample from it is
        // a `ReadSample` that returned no picture.
        reader
            .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
            .ok()?;
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        // A file with no video stream at all — an `.m4a`, an `.mp4` that carries
        // only audio — fails here, which is the honest place for it to fail.
        reader.SetStreamSelection(stream, true).ok()?;

        // What the file says it is, before anything is asked of it. This is the
        // pair the fact line prints, and it is read off the *native* type so
        // that it stays the video's own size however the output below is scaled.
        let native = reader
            .GetNativeMediaType(stream, 0)
            .ok()
            .and_then(|kind| kind.GetUINT64(&MF_MT_FRAME_SIZE).ok())
            .map(unpack_size)
            .filter(|(width, height)| *width > 0 && *height > 0)?;

        request_rgb32(&reader, stream, native, (fit_width, fit_height));

        // The size the reader actually settled on, read back rather than
        // assumed: the request above is a request.
        let current = reader.GetCurrentMediaType(stream).ok()?;
        let (width, height) = current
            .GetUINT64(&MF_MT_FRAME_SIZE)
            .ok()
            .map(unpack_size)
            .filter(|(width, height)| *width > 0 && *height > 0)?;
        // Present on the types the video processor produces and absent on some
        // others; a missing stride is the contiguous one the width implies.
        let declared_stride = current
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .ok()
            .map(|s| s as i32);

        let duration_ms = duration_of(&reader);
        seek_into(&reader, duration_ms);

        let sample = loop {
            if Instant::now() >= deadline {
                return None;
            }
            let mut flags = 0_u32;
            let mut sample = None;
            reader
                .ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))
                .ok()?;
            if let Some(sample) = sample {
                break sample;
            }
            // No sample and no end: a stream tick, or a format change the reader
            // is working through. Both are answered by asking again — but a file
            // that answers this way for ever is what the deadline above is for.
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return None;
            }
        };

        let buffer = sample.ConvertToContiguousBuffer().ok()?;
        let rgba = copy_locked_frame(&buffer, width, height, declared_stride)?;
        Some(VideoFrame {
            rgba,
            width,
            height,
            duration_ms,
            native_width: native.0,
            native_height: native.1,
        })
    }
}

/// Ask the reader for RGB-32 at the fitted size, and settle for RGB-32 at
/// whatever size it likes.
///
/// Two attempts and not one, because the frame size is the half that may be
/// refused: the video processor scales on most machines and formats and there is
/// no promise anywhere that it scales on all of them. The subtype is the half
/// this lane cannot do without, so it is asked for again on its own — a frame
/// that comes back at native size is a frame the window's own resampler fits,
/// and a frame that comes back in YUV is one nothing here can read.
///
/// Neither failure is reported: the caller reads the media type back and works
/// from what is really there. If both attempts fail, the current type is
/// whatever the reader chose for itself and the size read-back is what decides
/// whether this can go on.
///
/// # SAFETY
///
/// Called from [`read_first_frame`], on its thread, inside its apartment, with a
/// reader it owns.
unsafe fn request_rgb32(
    reader: &IMFSourceReader,
    stream: u32,
    native: (u32, u32),
    fit: (u32, u32),
) {
    unsafe {
        let describe = |size: Option<(u32, u32)>| {
            let kind = MFCreateMediaType().ok()?;
            kind.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
            kind.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).ok()?;
            if let Some((width, height)) = size {
                kind.SetUINT64(&MF_MT_FRAME_SIZE, pack_size(width, height))
                    .ok()?;
            }
            Some(kind)
        };
        if let Some(fitted) = describe(Some(contain(native, fit)))
            && reader.SetCurrentMediaType(stream, None, &fitted).is_ok()
        {
            return;
        }
        if let Some(plain) = describe(None) {
            let _ = reader.SetCurrentMediaType(stream, None, &plain);
        }
    }
}

/// The video's declared length in milliseconds, or `None` when the container
/// does not carry one.
///
/// `MF_PD_DURATION` is a presentation descriptor attribute in units of 100
/// nanoseconds, asked of the source rather than of the stream because it is a
/// fact about the file. A live source, a stream still being written and a
/// container with no index all answer with nothing, and nothing is what the fact
/// line then says.
///
/// # SAFETY
///
/// Called from [`read_first_frame`]; see its own note.
unsafe fn duration_of(reader: &IMFSourceReader) -> Option<u64> {
    unsafe {
        let value = reader
            .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
            .ok()?;
        let hundred_nanos = u64::try_from(&value).ok()?;
        (hundred_nanos > 0).then_some(hundred_nanos / 10_000)
    }
}

/// Wind the reader to [`SEEK_FRACTION`] of the way in, when there is a length to
/// take a fraction of.
///
/// A refusal is ignored on purpose. Some sources cannot seek and some refuse a
/// position they consider out of range; both mean the frame comes from the
/// beginning, which is a worse picture than the one that was asked for and a far
/// better one than none.
///
/// # SAFETY
///
/// Called from [`read_first_frame`]; see its own note.
unsafe fn seek_into(reader: &IMFSourceReader, duration_ms: Option<u64>) {
    let Some(duration_ms) = duration_ms else {
        return;
    };
    let hundred_nanos = (duration_ms as f64 * SEEK_FRACTION * 10_000.0) as i64;
    if hundred_nanos <= 0 {
        return;
    }
    let position = PROPVARIANT::from(hundred_nanos);
    unsafe {
        // A null time format is the default one, which is the 100-nanosecond
        // clock the position above is expressed in.
        let _ = reader.SetCurrentPosition(&windows::core::GUID::zeroed(), &position);
    }
}

/// Copy one locked frame out of Media Foundation's buffer as straight, opaque,
/// top-down RGBA8.
///
/// **Two ways in, and the first is the one that cannot be got wrong.**
/// `IMF2DBuffer::Lock2D` hands back a pointer to *scanline zero* and a signed
/// pitch, so a bottom-up frame — which is what RGB-32 is by default in this
/// platform's DIB heritage — arrives already described rather than needing to be
/// recognised. Only a buffer that does not offer that interface falls to the flat
/// `Lock`, where the sign of the media type's declared stride is the only thing
/// that says which way up the rows are.
///
/// The bytes themselves are BGRX: Media Foundation's `RGB32` names the DWORD and
/// not the byte order, so each pixel is reversed on the way out and its unused
/// fourth byte is replaced by a real opaque alpha. A frame of video has no
/// transparency to carry and a card that composited one against the terminal's
/// background would show the terminal through it.
///
/// # SAFETY
///
/// Called from [`read_first_frame`]; see its own note. The pointer obtained here
/// is read only inside the lock this function takes and releases, and every read
/// is bounded by the row count and pitch the buffer itself declared.
unsafe fn copy_locked_frame(
    buffer: &IMFMediaBuffer,
    width: u32,
    height: u32,
    declared_stride: Option<i32>,
) -> Option<Vec<u8>> {
    let (row_bytes, rows) = (width.checked_mul(4)? as usize, height as usize);
    unsafe {
        if let Ok(two_d) = buffer.cast::<IMF2DBuffer>() {
            let mut scanline0: *mut u8 = std::ptr::null_mut();
            let mut pitch = 0_i32;
            if two_d.Lock2D(&mut scanline0, &mut pitch).is_ok() {
                let rgba = (!scanline0.is_null())
                    .then(|| swizzle(scanline0, pitch, row_bytes, rows))
                    .flatten();
                let _ = two_d.Unlock2D();
                return rgba;
            }
        }
        let mut start: *mut u8 = std::ptr::null_mut();
        let mut length = 0_u32;
        buffer.Lock(&mut start, None, Some(&mut length)).ok()?;
        // A flat lock hands back the buffer's first byte, which is scanline zero
        // only when the rows run downwards. A negative stride says they do not:
        // the first byte is the *bottom* row, and scanline zero is the last one.
        let pitch = declared_stride.unwrap_or(row_bytes as i32);
        let stride = pitch.unsigned_abs() as usize;
        let needed = stride.checked_mul(rows)?;
        let rgba = (!start.is_null() && length as usize >= needed && stride >= row_bytes)
            .then(|| {
                let scanline0 = if pitch < 0 {
                    start.add(stride * (rows - 1))
                } else {
                    start
                };
                swizzle(scanline0, pitch, row_bytes, rows)
            })
            .flatten();
        let _ = buffer.Unlock();
        rgba
    }
}

/// BGRX rows at `pitch` apart, starting at scanline zero, out as opaque RGBA8.
///
/// # SAFETY
///
/// `scanline0` must be the first byte of the top row of a locked buffer holding
/// `rows` rows of at least `row_bytes` bytes, `pitch` apart and in that
/// direction — which is exactly what [`copy_locked_frame`] establishes before it
/// calls this, on both of its two paths.
unsafe fn swizzle(
    scanline0: *mut u8,
    pitch: i32,
    row_bytes: usize,
    rows: usize,
) -> Option<Vec<u8>> {
    let mut rgba = vec![0_u8; row_bytes.checked_mul(rows)?];
    for row in 0..rows {
        // SAFETY: the caller's contract. `pitch` may be negative, which walks
        // the source upwards from scanline zero; the destination always walks
        // downwards, which is what turns a bottom-up frame the right way up.
        let source = unsafe { scanline0.offset(pitch as isize * row as isize) };
        let source = unsafe { std::slice::from_raw_parts(source, row_bytes) };
        let target = &mut rgba[row * row_bytes..(row + 1) * row_bytes];
        for (out, pixel) in target.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
            out[0] = pixel[2];
            out[1] = pixel[1];
            out[2] = pixel[0];
            out[3] = 255;
        }
    }
    Some(rgba)
}

/// `MF_MT_FRAME_SIZE` is a `UINT64` with the width in its high half.
fn unpack_size(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, (packed & 0xffff_ffff) as u32)
}

/// The other direction of [`unpack_size`].
fn pack_size(width: u32, height: u32) -> u64 {
    (u64::from(width) << 32) | u64::from(height)
}

/// `size` fitted inside `fit` with its proportions kept, and **never enlarged**.
///
/// `contain` rather than `cover`, which is the same bargain every other picture
/// in this window is fitted by: a wide frame and a tall one are both themselves,
/// and the host centres what is left over. The clamp against `size` is what
/// keeps a small clip from being asked for at a size whose pixels do not exist —
/// see [`first_frame`].
fn contain(size: (u32, u32), fit: (u32, u32)) -> (u32, u32) {
    let scale = (f64::from(fit.0) / f64::from(size.0)).min(f64::from(fit.1) / f64::from(size.1));
    if scale >= 1.0 {
        return size;
    }
    (
        ((f64::from(size.0) * scale).round() as u32).clamp(1, size.0),
        ((f64::from(size.1) * scale).round() as u32).clamp(1, size.1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — **the two halves of `MF_MT_FRAME_SIZE` do not swap places.**
    ///
    /// MUTATION: pack the height into the high half and a 1920×1080 video is
    /// asked for as 1080×1920 — a frame the reader either refuses or scales into
    /// the wrong shape, and a fact line that reports a portrait capture.
    #[test]
    fn a_frame_size_survives_the_round_trip() {
        assert_eq!(unpack_size(pack_size(1920, 1080)), (1920, 1080));
        assert_eq!(unpack_size(pack_size(1, 0xffff_ffff)), (1, 0xffff_ffff));
        assert_eq!(pack_size(1920, 1080) >> 32, 1920);
    }

    /// PIN — **the fit contains and never enlarges.**
    ///
    /// The first two cases are the card's own box against the two shapes a
    /// capture comes in; the third is the whole of why the clamp is there — a
    /// clip smaller than the box keeps its own pixels rather than being asked
    /// for at a size the file does not have.
    ///
    /// MUTATION: drop the `scale >= 1.0` arm and the small clip is requested at
    /// 280×158, which is a scaler inventing pixels this window would then
    /// resample a second time.
    #[test]
    fn the_fit_contains_and_never_enlarges() {
        assert_eq!(contain((1920, 1080), (280, 160)), (280, 158));
        assert_eq!(contain((1080, 1920), (280, 160)), (90, 160));
        assert_eq!(contain((160, 90), (280, 160)), (160, 90));
        // And a fit narrower on one axis is answered by that axis.
        assert_eq!(contain((100, 100), (50, 400)), (50, 50));
    }

    /// PIN — **a bottom-up frame comes out the right way up, and BGRX comes out
    /// RGBA.**
    ///
    /// Two rows, two pixels each, laid out the way Media Foundation lays an
    /// RGB-32 frame out by default: the last row first, with the pointer handed
    /// to scanline zero and a negative pitch. What must come back is the *first*
    /// row first, each pixel byte-reversed and opaque.
    ///
    /// MUTATION ①: ignore the sign of the pitch and the picture is upside down —
    /// which on a card is a frame nobody reads as wrong, only as strange.
    /// MUTATION ②: copy the fourth byte through instead of writing 255 and a
    /// frame whose unused byte happens to be zero is drawn fully transparent.
    #[test]
    fn a_bottom_up_bgrx_frame_comes_out_top_down_rgba() {
        // Row 0 is `(1,2,3)`,`(4,5,6)`; row 1 is `(7,8,9)`,`(10,11,12)`. In
        // memory the bottom row stands first, so the buffer holds row 1 then
        // row 0, and every pixel is stored blue first.
        let mut buffer: Vec<u8> = vec![
            9, 8, 7, 0, 12, 11, 10, 0, // row 1, BGRX
            3, 2, 1, 0, 6, 5, 4, 0, // row 0, BGRX
        ];
        let row_bytes = 8;
        // Scanline zero is the last row of the buffer, and the pitch walks back.
        // SAFETY: the pointer is into a live `Vec` of exactly two rows of
        // `row_bytes`, and the walk from the second row backwards by `row_bytes`
        // reaches the first — the contract `swizzle` asks for.
        let rgba = unsafe {
            let scanline0 = buffer.as_mut_ptr().add(row_bytes);
            swizzle(scanline0, -(row_bytes as i32), row_bytes, 2)
        }
        .expect("two rows copy");
        assert_eq!(
            rgba,
            vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
        );
    }

    /// PIN — **a top-down frame is copied as it stands.**
    ///
    /// The other orientation, which is what `Lock2D` hands back for a positive
    /// pitch and what the flat lock's path assumes when the media type declares
    /// no stride at all.
    #[test]
    fn a_top_down_frame_keeps_its_order() {
        let mut buffer: Vec<u8> = vec![3, 2, 1, 0, 9, 8, 7, 0];
        // SAFETY: one row of four bytes per pixel, two pixels, walked forwards.
        let rgba = unsafe { swizzle(buffer.as_mut_ptr(), 8, 8, 1) }.expect("one row copies");
        assert_eq!(rgba, vec![1, 2, 3, 255, 7, 8, 9, 255]);
    }

    /// PIN — **a padded row is read at its own width and not at its pitch.**
    ///
    /// A video processor may hand back rows aligned past the pixels in them, and
    /// a copy that took `pitch` bytes per row would drag the padding into the
    /// picture and shear every row after the first.
    ///
    /// MUTATION: pass `pitch` where `row_bytes` is passed and the second pixel
    /// of the second row is the first row's padding.
    #[test]
    fn padding_past_the_pixels_is_not_copied() {
        // Two rows of one pixel, each row padded to eight bytes.
        let mut buffer: Vec<u8> = vec![3, 2, 1, 0, 0, 0, 0, 0, 9, 8, 7, 0, 0, 0, 0, 0];
        // SAFETY: two rows, eight bytes apart, four of which are pixels.
        let rgba = unsafe { swizzle(buffer.as_mut_ptr(), 8, 4, 2) }.expect("two rows copy");
        assert_eq!(rgba, vec![1, 2, 3, 255, 7, 8, 9, 255]);
    }

    /// PIN — **nothing that is not a video is drawn, and nothing panics.**
    ///
    /// The refusal path through the real platform: a file that is not there, a
    /// file that is empty, and a file whose bytes are text with a video's name
    /// on it. All three must be the module's one silence, on the caller's thread,
    /// with the apartment and the platform given back — and the last of the three
    /// is the one that actually reaches Media Foundation and is turned away by it.
    ///
    /// MUTATION: `unwrap` any of the `ok()?`s in `read_first_frame` and the third
    /// case takes the decoration worker down, which on the machine is a hover
    /// over a renamed archive ending the formula lane for the session.
    #[test]
    fn nothing_that_is_not_a_video_is_drawn() {
        let dir = std::env::temp_dir().join(format!(
            "folio-video-refusals-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let missing = dir.join("no-such-file.mp4");
        assert_eq!(first_frame(&missing, 280, 160), None);
        let empty = dir.join("empty.mp4");
        std::fs::write(&empty, b"").expect("an empty file");
        assert_eq!(first_frame(&empty, 280, 160), None);
        let text = dir.join("renamed.mp4");
        std::fs::write(&text, b"this is not a video at all, whatever it is called")
            .expect("a text file");
        assert_eq!(first_frame(&text, 280, 160), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RED — **the shipped fixture gives up a frame, its size and its length.**
    ///
    /// The one assertion in this module that a decoder cannot satisfy by
    /// agreeing with synthetic bytes: a real H.264 file, opened through the real
    /// platform, seeked and decoded. It is what says the whole chain is wired —
    /// the attribute, the output type, the seek, the read, the lock and the
    /// swizzle — because every one of those failing answers `None` and this
    /// asserts a picture.
    ///
    /// The ink assertion separates a decoder that ran from one that handed back a
    /// correctly sized nothing, and it is the reason [`SEEK_FRACTION`] exists:
    /// the fixture opens on black and closes on colour, so a lane that took frame
    /// zero would pass every other assertion here and fail this one.
    ///
    /// RED GATE: set `SEEK_FRACTION` to `0.0` and the ink assertion goes red
    /// while the size and duration assertions stay green — which is exactly the
    /// defect that would otherwise have shipped as "all my clips are black".
    #[test]
    fn the_shipped_fixture_gives_up_a_frame() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-assets/folio-video-test.mp4");
        let frame = first_frame(&fixture, 280, 160).expect("a real mp4 gives up a frame");
        assert_eq!(
            (frame.native_width, frame.native_height),
            (160, 120),
            "the native size is the video's own"
        );
        assert!(
            frame.width <= 280 && frame.height <= 160,
            "fitted inside the box it was asked for: {}x{}",
            frame.width,
            frame.height
        );
        assert_eq!(
            frame.rgba.len(),
            frame.width as usize * frame.height as usize * 4,
            "straight RGBA8, one row after another"
        );
        assert!(
            frame.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255),
            "a frame of video is opaque"
        );
        let ink = frame
            .rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] != [0, 0, 0])
            .count();
        assert!(
            ink > frame.rgba.len() / 8,
            "a tenth of the way in is past the black opening: {ink} lit pixels"
        );
        let duration_ms = frame.duration_ms.expect("the container declares a length");
        assert!(
            (4_800..=5_200).contains(&duration_ms),
            "a five-second fixture: {duration_ms}ms"
        );
    }
}
