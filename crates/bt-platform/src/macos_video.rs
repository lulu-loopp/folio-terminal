//! **One frame out of a video file on a Mac, and the two facts that come with
//! it** (M4-4; `docs/DESIGN.md` §7.23, §7.44 and §13.25).
//!
//! The macOS half of [`crate::video`], and the only file in this crate that is
//! AVFoundation. The Windows half is `video/mod.rs` and is Media Foundation;
//! what the two have in common is not code but a **contract**, and the contract
//! is the whole ticket: a [`VideoFrame`] out of this file and a `VideoFrame` out
//! of that one are the same bytes in the same order, so the hover card, the
//! preview seat and `bt-render`'s picture channel never learn which machine they
//! are on.
//!
//! # The contract, restated where it can be checked
//!
//! Straight — **not premultiplied** — RGBA8, row-major from the top row down,
//! packed at `width * 4` bytes a row, every alpha byte `255`, in **sRGB**. Four
//! of those five are decisions this file makes and one is a consequence:
//!
//! - **Straight and opaque.** A frame of video carries no transparency, and a
//!   card that composited one against the terminal's background would show the
//!   terminal through it. Core Graphics settles this for us in the same
//!   direction Media Foundation does: a 32-bit RGB bitmap context has no
//!   *straight*-alpha form at all — the supported pixel formats are
//!   premultiplied or skipped — so the context is asked for
//!   `kCGImageAlphaNoneSkipLast`, which is R, G, B and one byte the drawing
//!   leaves alone, and this file writes `255` over that byte. That is precisely
//!   what the Windows arm does with Media Foundation's BGR**X**.
//! - **Top row first.** A `CGBitmapContext`'s backing store is laid out from the
//!   top row down, and `CGContextDrawImage` with the identity transform puts the
//!   image's top row in it. Nothing is flipped here, and the Windows arm's
//!   negative-pitch branch has no twin on this platform because Core Graphics
//!   does not hand back a bottom-up frame.
//! - **Packed.** The context is created with `bytesPerRow` of `width * 4`, and
//!   the row width it reports back is read rather than assumed — the same
//!   read-back discipline the Windows arm applies to its media type, for the
//!   same reason: a stride that is not the one that was asked for is a shear in
//!   every row after the first.
//! - **sRGB by name.** The bitmap context is built on `kCGColorSpaceSRGB`, so
//!   Core Graphics colour-matches the decoded frame — which arrives tagged with
//!   whatever the file says, usually BT.709 — into the space every other picture
//!   in this window reaches the renderer in. The Windows arm gets there by a
//!   different road: its video processor converts YUV to RGB and hands the
//!   result over untagged, which the renderer then treats as sRGB. **Naming the
//!   space is the honest spelling of the assumption the other arm makes
//!   silently**, and `tests/video_first_frame.rs` measures what the two arms
//!   actually produce out of the same two files.
//!
//! # The frame is not frame zero, and that is not this platform's decision
//!
//! [`SEEK_FRACTION`] is a tenth, and the reason is the content and not the API:
//! a great many videos open on black — a fade-in, a slate, a leader, a camera
//! that had not metered yet — so a thumbnail lane that took frame zero answers a
//! hover over half a folder of screen captures with half a folder of identical
//! black rectangles. The Windows arm decided that in 2026-08-27's ruling and
//! this one inherits it, because a card that showed a different picture
//! depending on the machine would be two products.
//!
//! The ask is **exact**: both `requestedTimeToleranceBefore` and
//! `requestedTimeToleranceAfter` are zero, so the generator decodes forward from
//! the key frame before the target rather than handing back the key frame
//! itself. When that exact ask fails — a container with an edit list, a file
//! whose sample table has nothing at that time, a track whose last sample is
//! earlier than it claims — the tolerances are opened and the same time is asked
//! for again, which is the "nearest frame at or before" the Windows arm's
//! `SetCurrentPosition` gives it by default. Two attempts and not one, and the
//! second is not a fallback to a different picture: it is the same request with
//! the precision the file could not meet.
//!
//! # The thread, and why there is no main-thread gate in this file
//!
//! **Nothing here touches AppKit**, so there is no `window_thread()` at the top
//! of any door the way `macos_impl.rs` opens every one of its own. The citation
//! is the same one `handoff.rs` gives for `NSWorkspace`: the *Thread Safety
//! Summary* in Apple's Cocoa Multithreading Programming Guide lists the classes
//! that are the main thread's — `NSView`, `NSWindow`, `NSApplication` each say
//! so in their own reference too — and says of everything it does not list,
//! **"In most cases, you can use these classes from any thread as long as you
//! use them from only one thread at a time."** `AVAsset`,
//! `AVAssetImageGenerator` and `CGBitmapContext` are on neither list and none of
//! their references states a thread requirement.
//!
//! There is stronger evidence than an absence, and it is in the class itself:
//! `AVAssetImageGenerator`'s asynchronous form,
//! `generateCGImagesAsynchronouslyForTimes:completionHandler:`, documents that
//! it calls the handler back on a queue **AVFoundation owns**, not on the main
//! queue. A class that required the main thread could not offer that. And the
//! evidence this repository can produce itself: every case in
//! `tests/video_first_frame.rs` runs on a thread libtest spawned — measured in
//! M2-3, `MainThreadMarker::new()` is `None` there even under
//! `--test-threads=1` — so a green run of that file **is** the statement that
//! this lane needs no main thread. That is why M4-4 adds no `harness = false`
//! target beside `tests/macos_sheet.rs`: the pattern exists for code that must
//! own the process's first thread, and this is not that code.
//!
//! What [`first_frame`] does need is to be off the thread that draws, and for
//! the same reason the Windows arm does: `copyCGImageAtTime:` blocks — Apple's
//! own note on it says the generator "may have to block the calling thread" —
//! and a file nothing can decode is bounded only by [`FIRST_FRAME_BUDGET`],
//! which is three seconds and would be three seconds of frozen window.
//!
//! # Why the deprecated call is the right call
//!
//! `copyCGImageAtTime:actualTime:error:` and `tracksWithMediaType:` are both
//! marked deprecated since macOS 13 in favour of asynchronous forms that take a
//! completion handler. Those forms exist because the synchronous ones block, and
//! **blocking is what this lane is built to do**: it already owns a thread of
//! its own and a three-second budget over it ([`super::within_budget`]), so
//! taking the asynchronous shape would mean a `block2` closure, a channel, and a
//! second answer arriving after the budget had already given up — the same
//! bargain `handoff.rs` refused for `openURL:`. Neither call is removed, neither
//! is unavailable at this product's deployment target, and both are a straight
//! line where the replacement is a detour back to here.

use std::path::Path;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2_av_foundation::{AVAssetImageGenerator, AVMediaTypeVideo, AVURLAsset};
use objc2_core_foundation::{CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextGetBytesPerRow, CGBitmapContextGetData, CGColorSpace,
    CGContext, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo, kCGColorSpaceSRGB,
};
use objc2_core_media::{CMTime, CMTimeFlags, kCMTimePositiveInfinity, kCMTimeZero};

use super::{
    FIRST_FRAME_BUDGET, FirstFrameCost, SEEK_FRACTION, VideoFrame, contain, within_budget,
};
use crate::macos_files::file_url;

/// **A frame out of the video at `path`, fitted inside `fit_width` ×
/// `fit_height`**, or `None` for every refusal in the module's own note.
///
/// The fit is a **request and a cap, not a promise**. It is given to
/// `AVAssetImageGenerator` as its `maximumSize`, already contained and already
/// clamped by [`super::contain`], and what comes back carries its own dimensions
/// which no caller may assume. It is never an enlargement — a 320×240 clip asked
/// for at 1280×720 comes back at 320×240, because the pixels a scaler would
/// invent are not the file's and the window's own sampler already stretches what
/// it is given.
///
/// # Where it may be called from
///
/// **Never the thread that draws.** See the module note: the decode blocks, and
/// this door's answer is bounded by [`FIRST_FRAME_BUDGET`] rather than by the
/// file.
#[must_use]
pub fn first_frame(path: &Path, fit_width: u32, fit_height: u32) -> Option<VideoFrame> {
    // Resolved before the thread, so what crosses is an owned path and not a
    // borrow. The thread that overruns the budget is abandoned, so it may not
    // hold anything the caller is still using.
    let path = path.to_owned();
    let (fit_width, fit_height) = (fit_width.max(1), fit_height.max(1));
    within_budget(FIRST_FRAME_BUDGET, move || {
        read_first_frame(&path, fit_width, fit_height, &mut FirstFrameCost::default())
    })
}

/// **The whole AVFoundation conversation, with no clock over it** — the half of
/// [`first_frame`] that actually decodes.
///
/// Same answer and same silences; what it does not carry is
/// [`FIRST_FRAME_BUDGET`], so it returns when the platform is done rather than
/// when a caller has stopped waiting. It runs on the calling thread, which may
/// be any thread that is not the one that draws.
#[must_use]
pub fn decode_first_frame(path: &Path, fit_width: u32, fit_height: u32) -> Option<VideoFrame> {
    decode_first_frame_measured(path, fit_width, fit_height).0
}

/// [`decode_first_frame`] with the stopwatch left running: the same answer, and
/// where its milliseconds went.
///
/// The measurement is the caller's business and never this module's — nothing
/// here reads a [`FirstFrameCost`] back or decides anything by it. It exists so
/// that "the first frame is slow" can be answered with a segment rather than an
/// opinion; see [`FirstFrameCost`].
#[must_use]
pub fn decode_first_frame_measured(
    path: &Path,
    fit_width: u32,
    fit_height: u32,
) -> (Option<VideoFrame>, FirstFrameCost) {
    let mut cost = FirstFrameCost::default();
    let frame = read_first_frame(path, fit_width.max(1), fit_height.max(1), &mut cost);
    (frame, cost)
}

/// The conversation itself: a file, a generator, a time, a picture.
///
/// Every step is an `?` on the way out, and that is the module's one silence
/// rather than five: a file that is not there, a file that is not a video, a
/// video with no video track, a track with no size and a time nothing could be
/// decoded at all end the same way, because the card above has the same thing to
/// say about each of them.
#[expect(
    deprecated,
    reason = "the synchronous generator and track accessors are what a thread with a budget over \
              it wants; see the module note"
)]
fn read_first_frame(
    path: &Path,
    fit_width: u32,
    fit_height: u32,
    cost: &mut FirstFrameCost,
) -> Option<VideoFrame> {
    let mut segment = Instant::now();
    let mut lap = |cost: &mut Duration| {
        *cost = segment.elapsed();
        segment = Instant::now();
    };
    // The path crosses as bytes rather than as a lossy `String`, which is the
    // whole of why `macos_files::file_url` exists; see its own note.
    let url = file_url(path, false).ok()?;
    // SAFETY: every call below is an Objective-C message to an object this
    // function created and holds, or a Core Graphics call on a context it
    // created and holds. The statics read here — `AVMediaTypeVideo` and
    // `kCMTimeZero` — are framework constants that live for the process. The one
    // raw pointer that leaves an object is the bitmap context's backing store,
    // and it is read inside `copy_drawn_frame`, before the context that owns it
    // is dropped.
    unsafe {
        // `AVURLAsset` does not open anything here: it is a name, and the file
        // behind it is read when something is asked of it. A path that is not
        // there gets this far and is turned away at the track list below, which
        // is the honest place for it.
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
        lap(&mut cost.open);

        // A file with no video stream at all — an `.m4a`, an `.mp4` that carries
        // only audio, a text file with a video's name on it — has no track here
        // and stops.
        let track = asset.tracksWithMediaType(AVMediaTypeVideo?).firstObject()?;
        // **What the file says it is, before anything is asked of it**, and it
        // is the *displayed* size rather than the stored one: a portrait capture
        // from a phone is stored landscape with a quarter turn beside it, and
        // the pair the fact line prints is what a reader sees, not what the
        // container happens to hold. `appliesPreferredTrackTransform` below
        // makes the raster agree with it.
        let native = displayed_size(track.naturalSize(), track.preferredTransform())?;

        generator.setAppliesPreferredTrackTransform(true);
        // The cap is contained and clamped here rather than left to the
        // generator, so that the two arms fit a frame by the same arithmetic
        // rather than by two platforms' opinions of the word "maximum".
        let fitted = contain(native, (fit_width, fit_height));
        generator.setMaximumSize(CGSize::new(fitted.0 as CGFloat, fitted.1 as CGFloat));
        generator.setRequestedTimeToleranceBefore(kCMTimeZero);
        generator.setRequestedTimeToleranceAfter(kCMTimeZero);
        lap(&mut cost.output_type);

        let declared = asset.duration();
        let duration_ms = declared_length(declared);
        let at = seek_into(declared);
        lap(&mut cost.seek);

        let image = generate(&generator, at)?;
        lap(&mut cost.read_sample);

        // The size the generator actually settled on, read off the picture
        // rather than assumed: the cap above is a cap.
        let width = u32::try_from(CGImage::width(Some(&image))).ok()?;
        let height = u32::try_from(CGImage::height(Some(&image))).ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        let rgba = copy_drawn_frame(&image, width, height)?;
        lap(&mut cost.copy);

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

/// **Ask for the frame exactly, and if the file cannot answer exactly, ask for
/// the same frame as closely as it can.**
///
/// The first ask carries the zero tolerances the caller set. The second opens
/// them to infinity, which is what makes the generator answer with the nearest
/// decodable frame rather than with an error — the behaviour Media Foundation's
/// `SetCurrentPosition` has by default and which the Windows arm therefore never
/// had to ask for. A container with an edit list is the case that needs it: the
/// media timeline and the presentation timeline are not the same line, and an
/// exact request in one of them can land where the other has nothing.
///
/// Both attempts failing is `None`, which is the module's one silence.
///
/// # SAFETY
///
/// Called from [`read_first_frame`], on its thread, with a generator it owns;
/// see its own note. The null `actualTime` is the documented way of saying the
/// caller does not want the time back.
#[expect(
    deprecated,
    reason = "the synchronous generator is what a thread with a budget over it wants; see the \
              module note"
)]
unsafe fn generate(generator: &AVAssetImageGenerator, at: CMTime) -> Option<Retained<CGImage>> {
    unsafe {
        if let Ok(exact) = generator.copyCGImageAtTime_actualTime_error(at, null_mut()) {
            return Some(exact);
        }
        generator.setRequestedTimeToleranceBefore(kCMTimePositiveInfinity);
        generator.setRequestedTimeToleranceAfter(kCMTimePositiveInfinity);
        generator
            .copyCGImageAtTime_actualTime_error(at, null_mut())
            .ok()
    }
}

/// **The video's declared length in milliseconds**, or `None` when the container
/// does not carry one.
///
/// A `CMTime` says so itself, in its flags: a live source, a stream still being
/// written and a container with no index all answer with something that is not a
/// number — invalid, indefinite, or one of the two infinities — and nothing is
/// what the fact line then says. A zero-length answer is nothing too, for the
/// same reason the Windows arm rejects a zero `MF_PD_DURATION`.
fn declared_length(duration: CMTime) -> Option<u64> {
    if !valid_length(duration) {
        return None;
    }
    // SAFETY: a `CMTime` this function has already established is valid,
    // numeric and positively scaled; `CMTimeGetSeconds` is a division.
    let millis = unsafe { duration.seconds() } * 1_000.0;
    (millis >= 1.0 && millis <= u64::MAX as f64).then(|| millis.round() as u64)
}

/// **The time a frame is asked for**: [`SEEK_FRACTION`] of the way into a video
/// that declares a length, and the very start of one that does not.
///
/// The fraction is taken on the `CMTime`'s own numerator so that the result
/// keeps the asset's timescale — a tenth of 150/30 is 15/30, not a rounded
/// number of milliseconds re-expressed in a clock the file does not use. A
/// fraction of an unknown length is not a position, which is why the other arm
/// of this reads from wherever the file starts.
fn seek_into(duration: CMTime) -> CMTime {
    // SAFETY: both are framework constants that live for the process, and
    // `CMTimeMake` is arithmetic.
    unsafe {
        if !valid_length(duration) {
            return kCMTimeZero;
        }
        let value = (duration.value as f64 * SEEK_FRACTION) as i64;
        if value <= 0 {
            return kCMTimeZero;
        }
        CMTime::new(value, duration.timescale)
    }
}

/// Whether a `CMTime` is a length this file can take a fraction of: valid,
/// numeric rather than infinite or indefinite, positively scaled and positive.
fn valid_length(duration: CMTime) -> bool {
    duration.flags.contains(CMTimeFlags::Valid)
        && !duration
            .flags
            .intersects(CMTimeFlags::ImpliedValueFlagsMask)
        && duration.timescale > 0
        && duration.value > 0
}

/// **A track's stored size turned the way it asks to be displayed**, rounded to
/// whole pixels.
///
/// The preferred transform is usually the identity and is not always: a phone
/// records portrait video as landscape pixels with a quarter turn in the track
/// header, and `appliesPreferredTrackTransform` makes the generator honour it.
/// The pair this returns is the bounding box of the stored rectangle under that
/// transform, which for the identity is the stored size and for a quarter turn
/// is the stored size with its axes swapped — so the raster and the fact line
/// agree however the file was written.
fn displayed_size(stored: CGSize, turn: CGAffineTransform) -> Option<(u32, u32)> {
    // The components are used as they come. `CGFloat` *is* this arithmetic's
    // type on every Apple target this crate is built for, so wrapping each one
    // in `f64::from` converts a value to the type it already has —
    // `clippy::useless_conversion`, which the `aarch64-apple-darwin` lane
    // refuses and which was caught there at the T-MAC-LIGHTS merge. The one
    // conversion below is a real one: `u32::MAX` is not a `CGFloat`.
    let width = (stored.width * turn.a).abs() + (stored.height * turn.c).abs();
    let height = (stored.width * turn.b).abs() + (stored.height * turn.d).abs();
    let (width, height) = (width.round(), height.round());
    (width >= 1.0 && height >= 1.0 && width <= f64::from(u32::MAX) && height <= f64::from(u32::MAX))
        .then_some((width as u32, height as u32))
}

/// **Draw one `CGImage` into a bitmap of this window's own shape and take the
/// rows out of it** — straight, opaque, top-down RGBA8.
///
/// There is no other supported way to read a `CGImage`: it is an opaque handle
/// whose backing store may be compressed, subsampled, in a colour space nothing
/// here uses, or on the GPU. Drawing it into a context whose format this file
/// chose is the conversion, and it is one call rather than a decoder of our own.
///
/// The context's backing store is Core Graphics' rather than a `Vec` of ours —
/// a null `data` pointer is the documented way to ask for that — so there is no
/// question about alignment and no buffer alive for longer than the draw. The
/// row width is **read back** rather than assumed, because a stride that is not
/// the one that was asked for would shear every row after the first, and the
/// copy walks the frame's own width and leaves any padding where it is.
///
/// # SAFETY
///
/// Called from [`read_first_frame`], on its thread, with an image it owns; see
/// its own note. The pointer obtained here is read only while the context that
/// owns it is alive, and every read is bounded by the row count and the row
/// width the context itself declared.
unsafe fn copy_drawn_frame(image: &CGImage, width: u32, height: u32) -> Option<Vec<u8>> {
    let row_bytes = (width as usize).checked_mul(4)?;
    let rows = height as usize;
    let needed = row_bytes.checked_mul(rows)?;
    // SAFETY: a framework constant that lives for the process.
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    // `kCGImageAlphaNoneSkipLast` with a big-endian 32-bit order is R, G, B and
    // one byte the drawing does not define — the one supported 32-bit RGB layout
    // whose colour components land in the order this window wants them. The
    // fourth byte becomes a real opaque alpha below.
    let layout = CGImageAlphaInfo::NoneSkipLast.0 | CGImageByteOrderInfo::Order32Big.0;
    // SAFETY: a null `data` asks Core Graphics to allocate and own the backing
    // store, which is exactly what the documented contract of this parameter
    // says; everything else is a number this function computed.
    let context = unsafe {
        CGBitmapContextCreate(
            null_mut(),
            width as usize,
            rows,
            8,
            row_bytes,
            Some(&space),
            layout,
        )
    }?;
    // The identity transform, a rect of the context's own size: the image's top
    // row lands in the store's first row, which is what makes this frame
    // top-down without anything being flipped.
    CGContext::draw_image(
        Some(&context),
        CGRect::new(
            CGPoint::ZERO,
            CGSize::new(width as CGFloat, height as CGFloat),
        ),
        Some(image),
    );

    let store = CGBitmapContextGetData(Some(&context));
    let stride = CGBitmapContextGetBytesPerRow(Some(&context));
    if store.is_null() || stride < row_bytes || stride.checked_mul(rows).is_none() {
        return None;
    }
    let mut rgba = vec![0_u8; needed];
    for row in 0..rows {
        // SAFETY: the context is `rows` rows of `stride` bytes and is alive
        // until this function returns; `row < rows` and `row_bytes <= stride`,
        // so this slice is inside the row it names.
        let source =
            unsafe { std::slice::from_raw_parts(store.cast::<u8>().add(row * stride), row_bytes) };
        let target = &mut rgba[row * row_bytes..(row + 1) * row_bytes];
        for (out, pixel) in target.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
            out[0] = pixel[0];
            out[1] = pixel[1];
            out[2] = pixel[2];
            out[3] = 255;
        }
    }
    Some(rgba)
}
