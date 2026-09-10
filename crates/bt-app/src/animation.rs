//! **A picture that moves on its own, and the clock it moves by** (user ruling
//! 2026-08-28, route B slice ②; `docs/DESIGN.md` §7.44 ⑤).
//!
//! # What was wrong, in one sentence
//!
//! This window has known that a `.gif` is animated since the day it learned to
//! decode one — `bt_term`'s decoder sets `DecodedImagePayload::animated` and has
//! always set it — and it drew the first frame and stopped. A reader hovering a
//! `loading.gif` saw a spinner frozen at twelve o'clock. The ruling is one
//! phrase: *「能动的就动」*.
//!
//! # Why the frames go down the video lane and not the picture lane
//!
//! §7.42 ⑥ wrote the argument for a playing video and every line of it is true
//! of an animation, which is the whole reason this module decodes frames and is
//! not a second upload path:
//!
//! * The picture channel is an **LRU keyed by content**, so a raster that
//!   changes ten times a second would insert ten entries a second and evict
//!   everything else in the window within a minute.
//! * It does not want a **new texture per frame** either — the size never
//!   changes, and creating a texture is the expensive half of an upload. What it
//!   wants is one texture written over, which is [`bt_render::VideoLayer`].
//! * A float has a **rounded corner** and a card has a **ground** its letterbox
//!   bars have to be painted in. The video layer carries both; the picture
//!   channel carries neither.
//!
//! So an animation is drawn by the same layer, fitted by the same
//! `video_fit_extent`, staged by the same [`bt_render::VideoStage`], and
//! therefore appears on all three surfaces for free. The one thing it does
//! **not** borrow is the control bar: a video is a recording somebody is
//! watching and an animation is a picture that moves, and putting a scrubber
//! over an eight-frame spinner would be this window mistaking one for the other.
//!
//! # The frames are streamed, because a file is longer than a window is wide
//!
//! **What was wrong, the second time** (user report 2026-09-10): every frame was
//! decoded into memory before the first one was drawn, and an animation whose
//! frames did not all fit was refused and drawn as its first frame, still. The
//! ceiling was 256 MiB of decoded pixels, which sounds generous until a real
//! file arrives: an 820×462 capture of a simulation, 6.6 MB on disk, **374
//! frames at eighty milliseconds** — 540 MB of pixels, and a picture that did
//! not move. At that size the old ceiling bought about 177 frames, so *any*
//! screen capture long enough to be worth capturing failed.
//!
//! A whole-decode cannot be fixed by raising the number, because the number is
//! not the shape of the problem: what a window shows of an animation is **one
//! frame**, and what it is about to show is a handful more. So an
//! [`Animation`] holds
//!
//! * a **ring** — the standing frame and the frames queued behind it, bounded by
//!   [`MAX_ANIMATION_RING_BYTES`] and by [`MAX_ANIMATION_RING_LEAD`], whichever
//!   is reached first; and
//! * an [`AnimationCursor`] — the file's bytes, a sequential reader over them
//!   and the canvas its frames are composed onto — which is handed to a worker
//!   to be filled and comes back parked in the animation until it is wanted
//!   again.
//!
//! **The cursor is where the design is.** GIF is a sequential container: frame
//! *n* is a patch on the composition of every frame before it, so there is no
//! seeking, and a decoder that has read to the end of the file is a decoder that
//! must **start again from the first frame** to play the loop a second time.
//! That re-decode is the honest cost of not holding the whole file, it is what
//! every browser does with a long animation, and it is why the cursor keeps the
//! file's bytes: they are at most [`MAX_ANIMATION_FILE_BYTES`], and they are the
//! only way back to frame zero.
//!
//! **And never on the thread that draws.** [`AnimationCursor::next_frames`] is
//! tens of milliseconds of pixels; it runs on the decoration worker that
//! [`decode`] already ran on. The window's side of that is two moves — hand the
//! cursor out ([`Animation::take_cursor`]), take it back with what it decoded
//! ([`Animation::park_cursor`]) — and while the cursor is away the animation
//! plays out of the ring. If the ring runs dry before the worker answers, the
//! standing frame stands and its successor is due the instant it lands: a
//! stalled decoder makes an animation late, never fast.
//!
//! # The delays are the file's
//!
//! Each frame is due for as long as its own header says, and **the delays are
//! read and never assumed**: a GIF may declare a different delay on every frame,
//! and a build that advanced one frame per redraw, or one every hundred
//! milliseconds, is a build that plays every animation at the wrong speed and
//! most of them at a speed that changes with the window's load. Two bounds sit
//! on the reading — [`MIN_FRAME_DELAY`] and [`DEFAULT_FRAME_DELAY`] — and both
//! are the browsers' own.
//!
//! # And three bounds, because a GIF may be enormous
//!
//! Streaming answers *how many* frames; it does not answer *how big one is*, and
//! a frame is a texture. So the bounds stand where they stood (review row R1-7,
//! adversarial review 2026-09-08), read off the file's own descriptor before a
//! single pixel is allocated:
//!
//! * [`MAX_ANIMATION_FILE_BYTES`] is how much of the file is read at all;
//! * [`MAX_ANIMATION_SIDE_PX`] is the widest frame any adapter this window is
//!   willing to require could draw;
//! * [`MAX_ANIMATION_FRAME_BYTES`] is half the ring, so that the two frames a
//!   stream needs — the one on the glass and the one after it — always fit.
//!
//! An animation over any of them is drawn as its **first frame, still**, and the
//! foot of the pane says so: `Runtime::preview_foot_notice` carries the reason
//! to the reader, which is the difference between this window declining and this
//! window appearing not to work.

use std::collections::VecDeque;
use std::io::{Cursor, Read};
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gif::{ColorOutput, DisposalMethod, MemoryLimit};
use image::{ImageFormat, ImageReader};

/// **How many bytes of decoded frames one animation keeps queued.**
///
/// 32 MiB, and the number is chosen against the two ends of what a `.gif` is:
///
/// * It is **half of `bt_term::MAX_INLINE_IMAGE_RGBA_BYTES`**, the allowance one
///   *still* picture gets. An animation is one object a reader pointed at, the
///   same as a still is, and it has no business costing more than one — which is
///   exactly what the old whole-decode ceiling let it do at four times over.
/// * It is **two frames of anything this window will play** and a great many
///   frames of what it usually plays: at the 820×462 of the capture that opened
///   this (1.5 MB a frame) it is twenty-one frames, at 1080p four, and at a
///   `loading.gif`'s usual 64×64 it is two thousand — of which
///   [`MAX_ANIMATION_RING_LEAD`] will take ten.
///
/// What it is emphatically not is a ceiling on the *animation*: the file behind
/// it may be any length at all, because the ring is a window over it and not a
/// copy of it.
pub const MAX_ANIMATION_RING_BYTES: u64 = 32 * 1024 * 1024;

/// **How far ahead of the standing frame the ring runs**, when the frames are
/// small enough that [`MAX_ANIMATION_RING_BYTES`] is not what stops it.
///
/// One second, which is two orders of magnitude more than the worker round trip
/// it is there to cover (a request posted this frame is answered within a frame
/// or two of the window's own clock) and small enough that a 64×64 spinner
/// queues ten frames rather than the two thousand its size would allow. A ring
/// measured only in bytes gets both of those wrong at once: it starves a large
/// animation and hoards a small one.
pub const MAX_ANIMATION_RING_LEAD: Duration = Duration::from_secs(1);

/// **The most one composed frame may be**, which is half the ring — so the frame
/// on the glass and the frame after it always fit in it together.
///
/// 16 MiB is a hair over 1080p (8.3 MB) and a little under 2048 square. An
/// animation whose frames are larger than this is refused with
/// [`AnimationRefusal::TooLarge`] and drawn as its first frame: it is not that
/// such a file cannot be decoded, it is that a stream of it would hold more
/// pixels for one hover than every other picture in the window together.
pub const MAX_ANIMATION_FRAME_BYTES: u64 = MAX_ANIMATION_RING_BYTES / 2;

/// **What one animation costs this window at its fullest** — the ring, plus the
/// file's own bytes, which the cursor keeps because they are the only way back
/// to frame zero when the loop comes round.
///
/// This is what `MAX_ANIMATION_CACHE_BYTES` is counted in: the window's ceiling
/// over every animation at once is a multiple of this rather than a number of
/// its own.
pub const MAX_ANIMATION_HELD_BYTES: u64 = MAX_ANIMATION_RING_BYTES + MAX_ANIMATION_FILE_BYTES;

/// **How many bytes of a file this window will read looking for frames**
/// (review row R1-7, adversarial review 2026-09-08).
///
/// `bt_term::MAX_INLINE_IMAGE_BYTES`, and it is that number rather than one of
/// this module's own because the two lanes are looking at the same files: the
/// picture lane has refused a local image past this cap since it was written, so
/// a `.gif` over it cannot be *drawn* by this window at all. Reading it whole to
/// find frames for a picture that will never appear was the plainest form of the
/// defect — one hover, one `std::fs::read`, no ceiling.
pub const MAX_ANIMATION_FILE_BYTES: u64 = bt_term::MAX_INLINE_IMAGE_BYTES as u64;

/// **The largest side this window will decode an animation's frames at.**
///
/// Every frame goes to the GPU through [`bt_render::VideoLayer`]'s one texture,
/// created at the frame's own size, and `wgpu::Limits::default()` puts the
/// portability floor for `max_texture_dimension_2d` at 8192 — so a frame wider
/// or taller than this is one no adapter this window is willing to require could
/// draw. Refusing it at the descriptor rather than at the upload is the
/// difference between a picture that does not move and a gigabyte allocated to
/// find that out.
pub const MAX_ANIMATION_SIDE_PX: u32 = 8192;

/// **The shortest delay a frame is honoured at.**
///
/// A GIF may declare a hundredth of a second, and historically many do — the
/// convention the browsers settled on is that such a frame means "as fast as is
/// reasonable" rather than "spin a core", and this window reads it the same way
/// for the same reason: honouring it literally would make one animation cost
/// more than every other thing on the glass together.
pub const MIN_FRAME_DELAY: Duration = Duration::from_millis(20);

/// The delay a frame that declares nothing at all is given — the browsers'
/// tenth-of-a-second reading.
///
/// GIF has no way to distinguish "no delay declared" from "a delay of zero": a
/// frame with no graphic control extension reads as zero, and so does one whose
/// extension says zero. Both mean the file did not choose, and a tenth of a
/// second is what every browser plays them at. It is also what an animation is
/// assumed to run at before any of its frames have been read — see
/// [`frames_wanted`].
pub const DEFAULT_FRAME_DELAY: Duration = Duration::from_millis(100);

/// **One composed frame of an animation, in the byte order the layer wants.**
///
/// BGRA and not RGBA, and it is converted while the frame is composed rather
/// than at upload time for the reason the whole module exists: this happens once
/// per frame of the file, on a worker, and the alternative happens once per
/// frame of the *window*, on the thread that draws.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnimationFrame {
    pub bgra: Arc<[u8]>,
    /// How long this frame stands before the next is due.
    pub delay: Duration,
}

/// **A sequential reader over one animated file, and the canvas its frames are
/// composed onto.**
///
/// # Why this module composes frames itself
///
/// `image`'s `AnimationDecoder` composes them too — disposal methods,
/// transparency, sub-rectangles and all — and this module used it until the day
/// it had to stream. What it hands back is a `Frames<'a>`, which is a
/// `Box<dyn Iterator>`: **not `Send`**, so it cannot be the thing that travels
/// to a worker and back, which is the one job a cursor has. Composition is
/// ninety lines of the GIF specification, it is pinned against `image`'s own
/// answer by a test in this file, and having it here is what makes the streaming
/// design possible at all.
///
/// # Where it may be used
///
/// **A worker, never the thread that draws** — see [`Self::next_frames`].
pub struct AnimationCursor {
    /// The file, kept for the same reason a looping animation needs it: GIF has
    /// no seek, so coming round to frame zero is opening these bytes again.
    bytes: Arc<[u8]>,
    reader: gif::Decoder<Cursor<Arc<[u8]>>>,
    width_px: u32,
    height_px: u32,
    /// The composition every frame is a patch on, in BGRA — carried from frame
    /// to frame, and cleared when the file starts again.
    canvas: Vec<u8>,
    /// How many frames this pass of the file has produced.
    pass_frames: u64,
    /// Learnt when the first pass ends: how many frames the file holds.
    frames_in_file: Option<u64>,
    /// Set when the file has nothing more to give and starting it again did not
    /// help — a truncated frame on the very first frame of a pass.
    ended: bool,
}

/// **This cursor crosses threads, and the whole design rests on it.**
///
/// Stated as a compile-time claim rather than left to be discovered when a field
/// is added: the cursor is moved to a worker inside a request and moved back
/// inside a completion, and a field that is not `Send` — `image`'s own frame
/// iterator, for one — turns that into a compile error at the call site, where
/// it reads as a mystery. Here it reads as the rule.
const _: fn() = || {
    fn is_send<T: Send>() {}
    is_send::<AnimationCursor>();
    is_send::<AnimationFrame>();
};

/// **The frames of one animated file, as far as they have been read** (§7.44 ⑤).
///
/// Held per *file* and not per surface, which is what makes three surfaces
/// showing one `loading.gif` show the same picture at the same instant rather
/// than three spinners at three phases.
pub struct Animation {
    /// The standing frame at the front, the frames queued behind it. Never
    /// empty: a frame is popped only when there is another to stand on.
    ring: VecDeque<AnimationFrame>,
    width_px: u32,
    height_px: u32,
    /// The file's bytes, counted once for the cache — the cursor holds the same
    /// allocation.
    bytes: Arc<[u8]>,
    /// The decoder, parked here between fills. `None` while it is away on a
    /// worker, which is also this window's "a fill is in flight" — one fact, in
    /// one place, and no flag to fall out of step with it.
    ///
    /// **It is also where the file's length lives.** How many frames the file
    /// holds and how long one turn of it takes are the cursor's own findings —
    /// it is the thing that read them — and copying them up here would be two
    /// records of one fact with a fill in flight between them.
    cursor: Option<Box<AnimationCursor>>,
    /// How many frames this animation has stood on since it opened. Strictly
    /// increasing, so it is never a question of which pass we are in; the file's
    /// own index is this modulo [`Self::frames_in_file`].
    standing_seq: u64,
    /// When the standing frame's successor falls due — the file's own delays,
    /// added up, so a loop takes exactly as long as the file says it does.
    due_at: Instant,
    /// Strictly increasing, and what the renderer's upload gate reads. It counts
    /// *changes of frame* and not redraws, which is the whole of why a still
    /// window showing a paused spinner costs no bus at all.
    generation: u64,
}

/// **Whether a name is one this window will look inside for frames.**
///
/// GIF alone, and the omission is deliberate: an animated PNG is also
/// `DecodedImagePayload::animated`, and `image`'s `PngDecoder` will hand back
/// its frames — but no fixture in this repository is one, and a lane with no
/// fixture is a lane nobody has seen work. §7.44 ⑤ says so out loud rather than
/// shipping it untested.
#[must_use]
pub fn path_names_an_animation(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gif"))
}

/// **What an attempt to read one file's frames came back with.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnimationRefusal {
    /// Not a container this module opens — see [`path_names_an_animation`].
    NotAnAnimation,
    /// The bytes would not decode, or the file was read and had no frames.
    Undecodable,
    /// One frame, so there is nothing to animate. A `.gif` may be a still
    /// picture, and this is the answer for one: the picture channel already
    /// draws it and does not need help.
    OneFrame,
    /// **One frame of it is too big to stream**, or the file is over
    /// [`MAX_ANIMATION_FILE_BYTES`], or its declared screen is over
    /// [`MAX_ANIMATION_SIDE_PX`].
    ///
    /// Never again a verdict on how *many* frames a file has: that was this
    /// module's own limit and streaming retired it (user report 2026-09-10). A
    /// file that earns this is drawn as its first frame and the foot of the pane
    /// says why.
    TooLarge,
}

impl AnimationRefusal {
    /// **Whether a reader is owed a sentence about this.**
    ///
    /// Two of the four are ordinary answers to an ordinary question and say
    /// nothing: a file that is not an animation is being drawn by the lane that
    /// does draw it, and a one-frame `.gif` is a still picture that looks
    /// exactly like a still picture. The other two leave a reader looking at a
    /// picture that ought to be moving, which is a thing this window has to
    /// account for — see `Runtime::preview_foot_notice`.
    #[must_use]
    pub fn is_worth_saying(self) -> bool {
        match self {
            Self::NotAnAnimation | Self::OneFrame => false,
            Self::Undecodable | Self::TooLarge => true,
        }
    }
}

/// **Read the head of `path`'s frames, or say why not.**
///
/// What comes back is an animation already standing on its first frame with a
/// ring behind it and a cursor parked in it, not a whole file: see the module
/// note.
///
/// # Where it may be called from
///
/// **A worker, never the thread that draws.** Decoding even a ring of frames is
/// tens of milliseconds and this window has one thread that must not spend them.
pub fn decode(path: &std::path::Path) -> Result<Animation, AnimationRefusal> {
    if !path_names_an_animation(path) {
        return Err(AnimationRefusal::NotAnAnimation);
    }
    // **The length is asked of the handle the bytes are then read through, and
    // the read is bounded by the cap rather than by that length** — the
    // discipline `pdf::read_capped` states in the same words, for the same
    // reason: a file being appended to between the two calls is a file this
    // window may not be handed unboundedly much of.
    let mut file = std::fs::File::open(path).map_err(|_| AnimationRefusal::Undecodable)?;
    let length = file
        .metadata()
        .map_err(|_| AnimationRefusal::Undecodable)?
        .len();
    if length > MAX_ANIMATION_FILE_BYTES {
        return Err(AnimationRefusal::TooLarge);
    }
    let mut bytes = Vec::with_capacity(length as usize);
    (&mut file)
        .take(MAX_ANIMATION_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AnimationRefusal::Undecodable)?;
    if bytes.len() as u64 > MAX_ANIMATION_FILE_BYTES {
        return Err(AnimationRefusal::TooLarge);
    }
    decode_bytes(bytes)
}

/// The same, from bytes already in hand — the half a test can hold.
///
/// It takes the bytes rather than borrowing them because the animation keeps
/// them: a looping decoder has to be able to open the file again, and copying
/// eight megabytes to hand them over would be a copy per hover.
pub fn decode_bytes(bytes: Vec<u8>) -> Result<Animation, AnimationRefusal> {
    open(Arc::from(bytes))
}

/// **Open one animated file: judge its descriptor, then read the ring's worth of
/// frames behind it.**
///
/// The order is the point (review row R1-7): everything that can be known from
/// the header is decided here, **before** [`AnimationCursor::over`] allocates a
/// canvas and before [`AnimationCursor::next_frames`] pulls a frame.
fn open(bytes: Arc<[u8]>) -> Result<Animation, AnimationRefusal> {
    // The container is judged by its own header and not by the name that led
    // here, which is the discipline `decode_image_bytes_within` already keeps: a
    // `.gif` that is a JPEG is a JPEG.
    let format = ImageReader::new(Cursor::new(&bytes[..]))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.format());
    if format != Some(ImageFormat::Gif) {
        return Err(AnimationRefusal::NotAnAnimation);
    }
    let reader = gif_reader(Arc::clone(&bytes)).map_err(|_| AnimationRefusal::Undecodable)?;
    // **The logical screen descriptor is the whole judgement, and it is read off
    // the header with no buffer anywhere.** A file declaring a 65535-square
    // screen asks a decoder for seventeen gigabytes on its first frame, and the
    // refusal that used to exist counted pixels that had already been made.
    let width_px = u32::from(reader.width());
    let height_px = u32::from(reader.height());
    if width_px == 0 || height_px == 0 {
        return Err(AnimationRefusal::Undecodable);
    }
    if width_px > MAX_ANIMATION_SIDE_PX || height_px > MAX_ANIMATION_SIDE_PX {
        return Err(AnimationRefusal::TooLarge);
    }
    let frame_bytes = u64::from(width_px) * u64::from(height_px) * 4;
    if frame_bytes > MAX_ANIMATION_FRAME_BYTES {
        return Err(AnimationRefusal::TooLarge);
    }

    let mut cursor = AnimationCursor::over(Arc::clone(&bytes), reader, width_px, height_px);
    // An empty ring, no lead, and no frame yet to read a delay off — so the
    // opening batch is asked for at the browsers' assumed tenth of a second.
    let want = frames_wanted(frame_bytes, 0, Duration::ZERO, DEFAULT_FRAME_DELAY);
    let frames = cursor.next_frames(want);
    if frames.is_empty() {
        return Err(AnimationRefusal::Undecodable);
    }
    if cursor.frames_in_file() == Some(1) {
        return Err(AnimationRefusal::OneFrame);
    }
    Ok(Animation::streaming(
        frames,
        width_px,
        height_px,
        bytes,
        Box::new(cursor),
        Instant::now(),
    ))
}

/// One decoder over one file's bytes, opened at the header.
fn gif_reader(bytes: Arc<[u8]>) -> Result<gif::Decoder<Cursor<Arc<[u8]>>>, gif::DecodingError> {
    // The frame ceiling goes onto the decoder as its own memory limit as well as
    // being checked above: one guards the composition this module writes, the
    // other guards the buffer the `gif` crate allocates for a frame whose
    // sub-rectangle is larger than the screen that declared it.
    const FRAME_LIMIT: NonZeroU64 = NonZeroU64::new(MAX_ANIMATION_FRAME_BYTES).unwrap();
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(ColorOutput::RGBA);
    options.set_memory_limit(MemoryLimit::Bytes(FRAME_LIMIT));
    options.read_info(Cursor::new(bytes))
}

/// **How many frames to ask the cursor for**, given what one costs, what the
/// ring is already holding, how much play is queued behind the standing frame
/// and how long a frame of this file tends to stand.
///
/// Two ceilings, and the smaller wins: [`MAX_ANIMATION_RING_BYTES`] is what the
/// window can afford and [`MAX_ANIMATION_RING_LEAD`] is what it can use. A ring
/// asked only for bytes queues two thousand frames of a spinner and four of a
/// capture; asked only for time it queues one second of 1080p, which is a
/// hundred megabytes.
#[must_use]
fn frames_wanted(frame_bytes: u64, held_bytes: u64, lead: Duration, typical: Duration) -> usize {
    let room = MAX_ANIMATION_RING_BYTES.saturating_sub(held_bytes) / frame_bytes.max(1);
    let missing = MAX_ANIMATION_RING_LEAD.saturating_sub(lead);
    if room == 0 || missing.is_zero() {
        return 0;
    }
    // `typical` is a delay read off frames that have already been through
    // [`compose`], so it is at least [`MIN_FRAME_DELAY`] and this division is at
    // most fifty.
    let by_time = missing
        .as_nanos()
        .div_ceil(typical.as_nanos().max(1))
        .try_into()
        .unwrap_or(u64::MAX);
    usize::try_from(room.min(by_time)).unwrap_or(usize::MAX)
}

impl AnimationCursor {
    fn over(
        bytes: Arc<[u8]>,
        reader: gif::Decoder<Cursor<Arc<[u8]>>>,
        width_px: u32,
        height_px: u32,
    ) -> Self {
        // Checked against [`MAX_ANIMATION_FRAME_BYTES`] by the only caller,
        // before this allocation is reached.
        let canvas = vec![0_u8; (width_px as usize) * (height_px as usize) * 4];
        Self {
            bytes,
            reader,
            width_px,
            height_px,
            canvas,
            pass_frames: 0,
            frames_in_file: None,
            ended: false,
        }
    }

    /// How many frames the file holds, once one whole pass of it has been read.
    #[must_use]
    pub fn frames_in_file(&self) -> Option<u64> {
        self.frames_in_file
    }

    /// **Decode the next `want` frames, starting the file again when it ends.**
    ///
    /// # Where it may be called from
    ///
    /// **A worker, never the thread that draws.** This is the expensive half of
    /// the module: a frame is a composition over the whole logical screen.
    ///
    /// Fewer than `want` frames come back only when the file has stopped giving
    /// them — a truncated first frame — which is a cursor that is done. Anything
    /// else loops: the end of the file is not an end, it is the point where the
    /// bytes are opened again and frame zero follows the last one.
    #[must_use]
    pub fn next_frames(&mut self, want: usize) -> Vec<AnimationFrame> {
        let mut frames = Vec::with_capacity(want.min(64));
        while frames.len() < want && !self.ended {
            match self.step() {
                Some(frame) => {
                    self.pass_frames += 1;
                    frames.push(frame);
                }
                None => {
                    // **A pass that produced nothing cannot produce anything**,
                    // and starting the file again would be this loop spinning on
                    // a broken header for as long as the want lasts.
                    if self.pass_frames == 0 {
                        self.ended = true;
                        break;
                    }
                    if self.frames_in_file.is_none() {
                        self.frames_in_file = Some(self.pass_frames);
                    }
                    if !self.start_again() {
                        self.ended = true;
                        break;
                    }
                }
            }
        }
        frames
    }

    /// The next composed frame, or `None` at the end of this pass of the file.
    ///
    /// **A frame that will not decode ends the pass rather than the animation.**
    /// A truncated GIF is a file with fewer frames than it meant to have, which
    /// is what a browser plays it as, and the alternative — refusing the whole
    /// animation on the two hundredth frame — would stop a picture that had been
    /// moving for twenty seconds.
    fn step(&mut self) -> Option<AnimationFrame> {
        let frame = match self.reader.read_next_frame() {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => return None,
        };
        Some(compose(
            frame,
            &mut self.canvas,
            self.width_px,
            self.height_px,
        ))
    }

    /// **Open the file again at its first frame**, which is what a loop is.
    fn start_again(&mut self) -> bool {
        let Ok(reader) = gif_reader(Arc::clone(&self.bytes)) else {
            return false;
        };
        self.reader = reader;
        self.canvas.fill(0);
        self.pass_frames = 0;
        true
    }
}

/// **Compose one frame onto the running canvas, and dispose of it.**
///
/// The GIF specification's own algorithm, and `image`'s reading of it, which
/// `an_animation_composes_its_frames_the_way_the_specification_does` pins
/// against `image`'s answer frame for frame:
///
/// * a pixel the frame declares **transparent** shows whatever the canvas holds
///   under it, which is how a two-colour spinner is drawn as one small patch per
///   frame;
/// * a pixel **outside** the frame's own rectangle is the canvas untouched, and
///   the disposal below does not reach it either — disposal is about the
///   rectangle the frame drew, not about the screen;
/// * and then the frame is disposed of *into the canvas the next frame will be
///   composed onto*: `Keep` leaves what was drawn, `Background` clears the
///   rectangle to transparent, `Previous` leaves the canvas as it was before
///   this frame — which is why it is the one method that writes nothing.
fn compose(
    frame: &gif::Frame<'_>,
    canvas: &mut [u8],
    width_px: u32,
    height_px: u32,
) -> AnimationFrame {
    let mut composed = canvas.to_vec();
    let frame_width = u32::from(frame.width);
    let frame_height = u32::from(frame.height);
    for row in 0..frame_height {
        let canvas_row = u32::from(frame.top) + row;
        if canvas_row >= height_px {
            break;
        }
        for column in 0..frame_width {
            let canvas_column = u32::from(frame.left) + column;
            if canvas_column >= width_px {
                break;
            }
            let source = ((row * frame_width + column) * 4) as usize;
            let Some(rgba) = frame.buffer.get(source..source + 4) else {
                break;
            };
            let target = ((canvas_row * width_px + canvas_column) * 4) as usize;
            let pixel: [u8; 4] = if rgba[3] == 0 {
                [
                    canvas[target],
                    canvas[target + 1],
                    canvas[target + 2],
                    canvas[target + 3],
                ]
            } else {
                // RGBA to BGRA: the layer's texture is created in the
                // swapchain's own order (§7.42 ②) and this is the one place in
                // the animation's life where a pixel is touched by this process.
                [rgba[2], rgba[1], rgba[0], rgba[3]]
            };
            composed[target..target + 4].copy_from_slice(&pixel);
            match frame.dispose {
                DisposalMethod::Any | DisposalMethod::Keep => {
                    canvas[target..target + 4].copy_from_slice(&pixel);
                }
                DisposalMethod::Background => {
                    canvas[target..target + 4].copy_from_slice(&[0, 0, 0, 0]);
                }
                DisposalMethod::Previous => {}
            }
        }
    }
    AnimationFrame {
        bgra: Arc::from(composed),
        delay: frame_delay(frame.delay),
    }
}

/// **How long one frame stands**, from the hundredths of a second its header
/// declares — see [`MIN_FRAME_DELAY`] and [`DEFAULT_FRAME_DELAY`].
fn frame_delay(hundredths: u16) -> Duration {
    if hundredths == 0 {
        return DEFAULT_FRAME_DELAY;
    }
    Duration::from_millis(u64::from(hundredths) * 10).max(MIN_FRAME_DELAY)
}

impl Animation {
    /// **How many bytes this animation is holding** — what the window's own
    /// ceiling over every animation at once is counted against.
    ///
    /// The ring and the file, and the file is counted once: the cursor's copy of
    /// it is the same allocation.
    #[must_use]
    pub fn bytes_held(&self) -> u64 {
        self.ring_bytes() + self.bytes.len() as u64
    }

    /// What the ring alone is holding.
    #[must_use]
    pub fn ring_bytes(&self) -> u64 {
        self.ring.iter().map(|frame| frame.bgra.len() as u64).sum()
    }

    /// **How many frames the file holds**, once one whole pass has been read —
    /// asked of the cursor, which is the thing that read it.
    ///
    /// `None` while the cursor is away on a worker, which is the honest answer:
    /// the length of the file is not a fact this half of the design keeps.
    #[cfg(test)]
    #[must_use]
    pub fn frames_in_file(&self) -> Option<u64> {
        self.cursor
            .as_ref()
            .and_then(|cursor| cursor.frames_in_file())
    }

    /// **Which frame of the file is on the glass.**
    ///
    /// The count of frames stood on, wrapped by the file's length — so the frame
    /// after the last one is frame zero, which is what a loop looks like from
    /// outside.
    ///
    /// Nothing this window draws asks the question — a frame is pixels and a
    /// generation, and neither of them is an index — so this is here for the
    /// tests, which cannot pin "it came round" any other way.
    #[cfg(test)]
    #[must_use]
    pub fn frame_index(&self) -> u64 {
        match self.frames_in_file() {
            Some(frames) if frames > 0 => self.standing_seq % frames,
            _ => self.standing_seq,
        }
    }

    /// The animation [`open`] builds: a ring, and the cursor that fills it.
    fn streaming(
        frames: Vec<AnimationFrame>,
        width_px: u32,
        height_px: u32,
        bytes: Arc<[u8]>,
        cursor: Box<AnimationCursor>,
        started: Instant,
    ) -> Self {
        let ring: VecDeque<AnimationFrame> = frames.into();
        let first = ring
            .front()
            .map_or(DEFAULT_FRAME_DELAY, |frame| frame.delay);
        Self {
            ring,
            width_px,
            height_px,
            bytes,
            cursor: Some(cursor),
            standing_seq: 0,
            due_at: started + first,
            generation: 1,
        }
    }

    /// **An animation with no decoder behind it, holding exactly these frames**
    /// — the pure constructor, what a test builds without a file.
    ///
    /// It plays them once and then stands on the last: with no cursor there is
    /// no way back to frame zero, and inventing one by rewinding the ring would
    /// be a second kind of animation with a second set of rules for a caller
    /// that does not exist. The window builds every animation it draws through
    /// [`decode`], and `main.rs`'s own tests build one of these to weigh.
    #[cfg(test)]
    #[must_use]
    pub fn of(
        frames: Vec<AnimationFrame>,
        width_px: u32,
        height_px: u32,
        started: Instant,
    ) -> Self {
        let ring: VecDeque<AnimationFrame> = frames.into();
        let first = ring
            .front()
            .map_or(DEFAULT_FRAME_DELAY, |frame| frame.delay);
        Self {
            ring,
            width_px,
            height_px,
            bytes: Arc::from(Vec::new()),
            cursor: None,
            standing_seq: 0,
            due_at: started + first,
            generation: 1,
        }
    }

    /// **Move to the frame that is due, and say whether that is a new one.**
    ///
    /// `true` is what owes the window a redraw, and it is false for every tick
    /// that lands inside the standing frame's own delay — which for a
    /// hundred-millisecond frame at sixty hertz is five ticks out of six.
    ///
    /// **A tick that arrives late walks the ring**, so a window that was busy for
    /// half a second resumes at the frame that is due now rather than half a
    /// second behind. **A tick that arrives to an empty ring stands still** and
    /// keeps the successor due from this instant, so a decoder that fell behind
    /// makes the animation late once rather than owing it a debt it will spend
    /// the next second fast-forwarding through.
    pub fn advance(&mut self, now: Instant) -> bool {
        let mut moved = false;
        while now >= self.due_at {
            if self.ring.len() < 2 {
                self.due_at = now;
                break;
            }
            self.ring.pop_front();
            self.standing_seq += 1;
            self.generation += 1;
            self.due_at += self
                .ring
                .front()
                .map_or(DEFAULT_FRAME_DELAY, |frame| frame.delay);
            moved = true;
        }
        moved
    }

    /// **How many frames this animation would like decoded next**, which is zero
    /// while its cursor is away or its ring is as full as either bound allows.
    #[must_use]
    pub fn frames_wanted(&self) -> usize {
        if self.cursor.is_none() {
            return 0;
        }
        let frame_bytes = u64::from(self.width_px) * u64::from(self.height_px) * 4;
        // The lead is what stands *behind* the frame on the glass: the standing
        // frame's own remaining time is not a buffer, it is the picture.
        let lead: Duration = self.ring.iter().skip(1).map(|frame| frame.delay).sum();
        let typical = self
            .ring
            .front()
            .map_or(DEFAULT_FRAME_DELAY, |frame| frame.delay);
        frames_wanted(frame_bytes, self.ring_bytes(), lead, typical)
    }

    /// **Hand the cursor out to a worker**, with how many frames to ask it for.
    ///
    /// `None` when the ring wants nothing or the cursor is already away — and
    /// the cursor being away *is* the record that a fill is in flight, so a
    /// second request cannot be posted for the same animation.
    #[must_use]
    pub fn take_cursor(&mut self) -> Option<(Box<AnimationCursor>, usize)> {
        let want = self.frames_wanted();
        if want == 0 {
            return None;
        }
        Some((self.cursor.take()?, want))
    }

    /// **Take the cursor back, with what it decoded.**
    ///
    /// Also good for taking it back with nothing, which is what a request that
    /// could not be posted does: an animation whose cursor was lost would stand
    /// on its last frame for as long as the window is open.
    pub fn park_cursor(&mut self, cursor: Box<AnimationCursor>, frames: Vec<AnimationFrame>) {
        self.ring.extend(frames);
        self.cursor = Some(cursor);
    }

    /// The standing frame, as the renderer's upload.
    #[must_use]
    pub fn upload(&self) -> bt_render::VideoFrameUpload {
        let frame = self
            .ring
            .front()
            .expect("an animation always stands on a frame");
        bt_render::VideoFrameUpload {
            bgra: Arc::clone(&frame.bgra),
            width_px: self.width_px,
            height_px: self.height_px,
            generation: self.generation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/assets/folio-anim-test.gif"),
        )
        .expect("the animation fixture is in tests/assets")
    }

    /// **The worker's half of the loop, run here on this thread.**
    ///
    /// Every test that plays an animation for longer than its opening ring has
    /// to be the worker, because that is the design: the window hands the cursor
    /// out and takes it back. One fill, or none if the ring is full.
    fn pump(animation: &mut Animation) -> usize {
        let Some((mut cursor, want)) = animation.take_cursor() else {
            return 0;
        };
        let frames = cursor.next_frames(want);
        let filled = frames.len();
        animation.park_cursor(cursor, frames);
        filled
    }

    /// **A GIF that declares a logical screen of `width` × `height` and holds
    /// one 1×1 frame** — nine bytes of picture behind a header that claims a
    /// rectangle of any size at all.
    ///
    /// It is written out by hand rather than encoded, because the whole point of
    /// it is a header that disagrees with its own contents, and no encoder will
    /// write one.
    fn a_gif_declaring(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"GIF89a");
        // Logical screen descriptor: the two numbers under test, then a packed
        // byte saying "a global colour table of two entries follows".
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&[0x80, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF]);
        // One image, 1×1, at the origin, with no local colour table.
        bytes.push(0x2C);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(&[1, 0, 1, 0]);
        bytes.push(0x00);
        // LZW at two bits: clear (4), the single pixel (0), end of information
        // (5), packed three bits at a time from the low end.
        bytes.push(0x02);
        bytes.extend_from_slice(&[0x02, 0x44, 0x01, 0x00]);
        bytes.push(0x3B);
        bytes
    }

    /// **A GIF of `frames` frames on a `side`-square screen**, whose first frame
    /// paints the whole screen and whose every frame after it is a single pixel
    /// of its own colour.
    ///
    /// The shape is deliberate and it is what makes the test that uses it cheap:
    /// what a decoded frame *costs* is the whole logical screen, `side² × 4`,
    /// however small the patch that made it — so this writes a file whose
    /// decoded size is enormous and whose encoded size is a few kilobytes, which
    /// is exactly the shape of the file that opened this defect (an 820×462
    /// capture, 6.6 MB on disk, 540 MB of pixels).
    fn a_gif_of(frames: u16, side: u16, delay_hundredths: u16) -> Vec<u8> {
        let palette: Vec<u8> = (0..=255_u8)
            .flat_map(|index| [index, index, index])
            .collect();
        let mut out = Vec::new();
        {
            let mut encoder =
                gif::Encoder::new(&mut out, side, side, &palette).expect("a writer this test owns");
            encoder
                .set_repeat(gif::Repeat::Infinite)
                .expect("and it takes a repeat");
            let mut first = gif::Frame::from_indexed_pixels(
                side,
                side,
                vec![0; side as usize * side as usize],
                None,
            );
            first.delay = delay_hundredths;
            encoder.write_frame(&first).expect("the opening screen");
            for index in 1..frames {
                // One pixel, walking the top row and changing colour, so that no
                // two composed frames of this file are the same picture.
                let mut frame =
                    gif::Frame::from_indexed_pixels(1, 1, vec![(index % 255 + 1) as u8], None);
                frame.left = index % side;
                frame.top = 0;
                frame.delay = delay_hundredths;
                frame.dispose = gif::DisposalMethod::Keep;
                encoder.write_frame(&frame).expect("and a patch on it");
            }
        }
        out
    }

    /// RED — **an animation longer than this window can hold plays, instead of
    /// standing on its first frame** (user report 2026-09-10).
    ///
    /// RED EVIDENCE (2026-09-10), before the ring — the whole file was decoded
    /// into memory and refused when it did not fit:
    ///
    /// ```text
    /// a three hundred frame animation opens: Err(TooLarge)
    /// ```
    ///
    /// The file under test is 300 frames of a 512-square screen: **300 MiB of
    /// decoded pixels**, over the 256 MiB ceiling the old design counted against
    /// and under a third of the 540 MB the reader's own file needed. The
    /// assertions are the three halves of "it plays":
    ///
    /// * it **opens** — the refusal for frame count is gone;
    /// * it **moves** — the frame index walks the file, on the file's own delays,
    ///   with a worker filling the ring behind it;
    /// * it **stays bounded** — at no point in the walk does it hold more than
    ///   [`MAX_ANIMATION_HELD_BYTES`], which is a fortieth of what the same file
    ///   decoded whole would have been.
    ///
    /// MUTATION: give the ring no bound and the third assertion goes red at the
    /// first loop; never fill it and the second stops at the opening batch.
    #[test]
    fn an_animation_over_the_old_ceiling_plays_instead_of_standing_still() {
        const FRAMES: u16 = 300;
        const SIDE: u16 = 512;
        // Five hundredths of a second a frame: fast enough that one second of
        // lead is twenty frames, so the ring is bounded by its bytes and not by
        // its clock.
        let bytes = a_gif_of(FRAMES, SIDE, 5);
        let frame_bytes = u64::from(SIDE) * u64::from(SIDE) * 4;
        assert!(
            u64::from(FRAMES) * frame_bytes > 256 * 1024 * 1024,
            "the fixture is over the ceiling this defect was about",
        );

        let mut animation = decode_bytes(bytes).expect("a long animation opens");
        assert!(animation.ring.len() >= 2, "and it opens with a ring");
        assert!(
            animation.ring_bytes() <= MAX_ANIMATION_RING_BYTES,
            "the opening ring is inside its bound: {}",
            animation.ring_bytes()
        );

        // Walk two whole turns of the loop, one frame at a time, with the worker
        // filling behind the play head.
        let mut now = Instant::now();
        let mut seen = Vec::new();
        for _ in 0..(2 * u64::from(FRAMES) + 2) {
            pump(&mut animation);
            assert!(
                animation.bytes_held() <= MAX_ANIMATION_HELD_BYTES,
                "the ring stays inside its bound: {} frames, {} bytes",
                animation.ring.len(),
                animation.bytes_held(),
            );
            seen.push(animation.frame_index());
            now += Duration::from_millis(50);
            animation.advance(now);
        }
        assert_eq!(
            animation.frames_in_file(),
            Some(u64::from(FRAMES)),
            "one whole pass told this window how long the file is",
        );
        // ① it walked the file in order, ② it came round rather than stopping on
        // the last frame, and ③ it did both of those twice.
        let walk: Vec<u64> = (0..u64::from(FRAMES)).collect();
        assert_eq!(&seen[..FRAMES as usize], &walk[..], "the first turn");
        assert_eq!(
            &seen[FRAMES as usize..2 * FRAMES as usize],
            &walk[..],
            "and the second, which begins at frame zero",
        );
    }

    /// RED — **a ring that is never filled does not run away with the clock**
    /// (the same report's second half).
    ///
    /// A window that was busy — a tab dragged, a pane scrolled away and back —
    /// is a window whose worker did not answer for a while, and the two ways to
    /// get that wrong are opposite: an animation that keeps a debt fast-forwards
    /// through everything the decoder hands it afterwards, and one that resets
    /// its whole clock starts the file again. What this pins is the middle: the
    /// standing frame stands, nothing is skipped, and the moment frames arrive
    /// it goes on from where it was.
    #[test]
    fn an_animation_whose_ring_runs_dry_waits_and_then_goes_on() {
        let bytes = a_gif_of(120, 256, 5);
        let mut animation = decode_bytes(bytes).expect("a long animation opens");
        let mut now = Instant::now();
        // Play out the opening ring without ever filling it.
        while animation.ring.len() > 1 {
            now += Duration::from_millis(50);
            animation.advance(now);
        }
        let stalled_at = animation.frame_index();
        assert!(stalled_at > 0, "it played what it had: {stalled_at}");

        // An hour of nothing: no frames, and no debt either.
        now += Duration::from_secs(3600);
        assert!(!animation.advance(now), "an empty ring cannot move");
        assert_eq!(animation.frame_index(), stalled_at);

        // The worker answers, and the very next tick is the next frame — not the
        // three hundred and sixty thousandth.
        pump(&mut animation);
        now += Duration::from_millis(1);
        assert!(animation.advance(now), "the frame that arrived is due");
        assert_eq!(animation.frame_index(), stalled_at + 1);
        // And from here it plays at the file's own speed again.
        now += Duration::from_millis(10);
        assert!(!animation.advance(now), "inside the new frame's own delay");
        now += Duration::from_millis(50);
        assert!(animation.advance(now));
        assert_eq!(animation.frame_index(), stalled_at + 2);
    }

    /// RED — **a header cannot ask this window for gigabytes** (review row
    /// R1-7, adversarial review 2026-09-08).
    ///
    /// RED EVIDENCE (2026-09-08), before the limits — the test harness process
    /// did not survive to print an assertion:
    ///
    /// ```text
    /// test animation::tests::a_declared_screen_this_window_will_not_hold_is_refused_before_it_is_allocated
    /// memory allocation of 17179344900 bytes failed
    /// process didn't exit successfully: ... (exit code: 0xc0000409)
    /// ```
    ///
    /// A decoder's first act is to allocate the whole **declared** logical
    /// screen and compose the file's frames into it, so a refusal counted after
    /// that allocation is a refusal to *keep* pixels that have already been
    /// made: nine bytes of picture behind a 65535-square header asked the
    /// allocator for seventeen gigabytes, on a hover, with no click anywhere.
    ///
    /// **The structural half is the load-bearing one and it stands first**,
    /// because the two readings agree about the verdict and differ only in what
    /// they spend reaching it: both answer [`AnimationRefusal::TooLarge`], and
    /// only one of them allocates seventeen gigabytes on the way. The order
    /// inside [`open`] is where the difference lives — the screen descriptor is
    /// read off the header and judged against [`MAX_ANIMATION_SIDE_PX`] and
    /// [`MAX_ANIMATION_FRAME_BYTES`] **before** [`AnimationCursor::over`]
    /// allocates a canvas and before a frame is pulled — so the order is what is
    /// asserted on, and it is asserted on *before* the fixtures are decoded.
    ///
    /// MUTATION: move either ceiling below `AnimationCursor::over` and the first
    /// assertion goes red; drop it and the verdicts below go red.
    #[test]
    fn a_declared_screen_this_window_will_not_hold_is_refused_before_it_is_allocated() {
        const SOURCE: &str = include_str!("animation.rs");
        let at = SOURCE
            .find("\nfn open(")
            .expect("the container walk is a free function in this file");
        let rest = &SOURCE[at..];
        let walk = &rest[..rest.find("\n}\n").expect("and it ends") + 3];
        let side = walk
            .find("MAX_ANIMATION_SIDE_PX")
            .expect("the declared screen is judged against the side this window can draw");
        let frame = walk
            .find("MAX_ANIMATION_FRAME_BYTES")
            .expect("and against what one frame of it would cost");
        let canvas = walk
            .find("AnimationCursor::over(")
            .expect("and the canvas comes after");
        let frames = walk
            .find("next_frames(")
            .expect("and the frames are pulled from it");
        assert!(
            side < canvas && frame < canvas && canvas < frames,
            "the screen is judged before a pixel is allocated:\n{walk}",
        );

        // And the verdict, at the size the review named and at one a machine can
        // survive being wrong about.
        for side in [16_384_u16, 65_535] {
            assert_eq!(
                decode_bytes(a_gif_declaring(side, side)).err(),
                Some(AnimationRefusal::TooLarge),
                "a {side}-square logical screen is over this window's ceiling",
            );
        }
        // A screen this window can hold is untouched: the fixture is 64 square.
        assert!(decode_bytes(fixture()).is_ok());
    }

    /// RED — **a `.gif` past the encoded cap is not read whole** (the same row's
    /// other half).
    ///
    /// RED EVIDENCE (2026-09-08), before the capped read:
    ///
    /// ```text
    /// a file is read behind a cap and not whole:
    /// pub fn decode(path: &std::path::Path) -> Result<Animation, AnimationRefusal> {
    ///     ...
    ///     let bytes = std::fs::read(path).map_err(|_| AnimationRefusal::Undecodable)?;
    ///     decode_bytes(bytes)
    /// }
    /// ```
    ///
    /// `decode` was a bare `std::fs::read`, so a hover over any file named
    /// `.gif` pulled all of it into memory before anything looked at it — while
    /// the still-picture lane next door has refused past
    /// `bt_term::MAX_INLINE_IMAGE_BYTES` since it was written, which means a file
    /// over that cap could not be *shown* by this window and was read whole
    /// anyway. The animation lane now reads behind the same number.
    ///
    /// MUTATION: go back to `std::fs::read` and the verdict is whatever the
    /// bytes happen to guess as, after all of them have been read.
    #[test]
    fn a_gif_past_the_encoded_cap_is_not_read_whole() {
        const SOURCE: &str = include_str!("animation.rs");
        let at = SOURCE
            .find("\npub fn decode(")
            .expect("the file reader is a free function in this file");
        let rest = &SOURCE[at..];
        let reader = &rest[..rest.find("\n}\n").expect("and it ends") + 3];
        assert!(
            !reader.contains("fs::read("),
            "a file is read behind a cap and not whole:\n{reader}",
        );

        let path = std::env::temp_dir().join(format!("bt-anim-huge-{}.gif", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let file = std::fs::File::create(&path).expect("a file in the temp directory");
        file.set_len(MAX_ANIMATION_FILE_BYTES + 1)
            .expect("a file of a declared length");
        drop(file);
        assert_eq!(
            decode(&path).err(),
            Some(AnimationRefusal::TooLarge),
            "a file past the cap is refused on its size",
        );
        let _ = std::fs::remove_file(&path);
    }

    /// RED — **an animation advances by the delays its own file declares**
    /// (user ruling 2026-08-28: *「能动的就动」*; `docs/DESIGN.md` §7.44 ⑤).
    ///
    /// The fixture's four frames declare **100, 200, 300 and 400 ms** — unequal
    /// on purpose, and that is the whole gate. Every wrong implementation of
    /// this passes a uniform GIF:
    ///
    /// * one frame per redraw — the animation runs at the window's frame rate
    ///   and changes speed with the machine's load;
    /// * a constant hundred milliseconds — three of these four frames are wrong;
    /// * the *first* frame's delay applied to all of them — same;
    /// * the delays summed in the wrong unit — the loop is a thousand times too
    ///   long or too short, which reads as "it does not move".
    ///
    /// So the assertions walk one whole loop and name the frame due at each
    /// boundary, on both sides of it, and then walk a second loop to pin that it
    /// wraps rather than stopping on its last frame.
    ///
    /// RED GATE: advance by a constant — which is what "animate a GIF" looks
    /// like when it is written without reading the file — and every assertion
    /// after the first fails.
    #[test]
    fn a_gif_advances_by_its_own_frame_delays() {
        let animation = decode_bytes(fixture()).expect("four frames");
        assert_eq!(animation.frames_in_file(), Some(4));
        assert_eq!((animation.width_px, animation.height_px), (64, 64));
        // ① the delays are the file's, to the millisecond.
        let delays: Vec<u64> = animation
            .ring
            .iter()
            .take(4)
            .map(|frame| frame.delay.as_millis() as u64)
            .collect();
        assert_eq!(delays, [100, 200, 300, 400], "the file's own delays");
        assert_eq!(delays.iter().sum::<u64>(), 1_000, "one turn of the loop");

        // ② the frame due at every boundary, on both sides of it. The worker
        // fills at every step, which is what the window does.
        let start = Instant::now();
        let mut animation = decode_bytes(fixture()).expect("four frames");
        animation.due_at = start + Duration::from_millis(100);
        for (ms, expected) in [
            (0, 0),
            (99, 0),
            (100, 1),
            (299, 1),
            (300, 2),
            (599, 2),
            (600, 3),
            (999, 3),
            // ③ and then it comes round, rather than standing on the last one.
            (1_000, 0),
            (1_100, 1),
            (1_600, 3),
            (2_050, 0),
        ] {
            pump(&mut animation);
            animation.advance(start + Duration::from_millis(ms));
            assert_eq!(animation.frame_index(), expected, "at {ms}ms");
        }

        // ④ the four frames are four different pictures, so "it moved" is a
        // thing a reader can see and not only a number that changed.
        let mut animation = decode_bytes(fixture()).expect("four frames");
        pump(&mut animation);
        let colours: std::collections::BTreeSet<[u8; 4]> = animation
            .ring
            .iter()
            .take(4)
            .map(|frame| [frame.bgra[0], frame.bgra[1], frame.bgra[2], frame.bgra[3]])
            .collect();
        assert_eq!(colours.len(), 4, "four frames, four colours: {colours:?}");
        // And they arrived in the swapchain's byte order: the first frame is
        // `0xE04B2F` written blue-first.
        assert_eq!(&animation.ring[0].bgra[..4], &[0x2F, 0x4B, 0xE0, 0xFF]);
    }

    /// PIN — **this module composes a frame the way the specification does**,
    /// pinned against `image`'s own answer.
    ///
    /// The composition moved into this file so that the cursor could cross a
    /// thread (see [`AnimationCursor`]), and a hand-written reading of disposal
    /// methods, transparent indices and sub-rectangles is exactly the kind of
    /// code that is subtly wrong in a way no eye catches — a spinner that leaves
    /// a smear, a capture whose background creeps. So the oracle is the library
    /// this module used to call: every frame of a fixture built to exercise all
    /// three disposal methods, a transparent index and an off-origin
    /// sub-rectangle is composed both ways and compared pixel for pixel.
    ///
    /// MUTATION: drop the transparency clause in `compose` and frame two
    /// differs; treat `Previous` as `Keep` and frame four does.
    #[test]
    fn an_animation_composes_its_frames_the_way_the_specification_does() {
        use image::AnimationDecoder;

        let palette: Vec<u8> = vec![
            0x00, 0x00, 0x00, // 0 black
            0xFF, 0x00, 0x00, // 1 red
            0x00, 0xFF, 0x00, // 2 green
            0x00, 0x00, 0xFF, // 3 blue
        ];
        let mut bytes = Vec::new();
        {
            let mut encoder =
                gif::Encoder::new(&mut bytes, 8, 8, &palette).expect("a writer this test owns");
            // ① the whole screen, ② a transparent patch over part of it,
            // ③ a patch disposed to the background, ④ a patch disposed to what
            // was there before it, ⑤ one more, so ④'s disposal is observable.
            let mut opener = gif::Frame::from_indexed_pixels(8, 8, vec![1; 64], None);
            opener.delay = 10;
            encoder.write_frame(&opener).expect("the opening screen");
            for (index, (left, top, dispose, transparent)) in [
                (2_u16, 2_u16, gif::DisposalMethod::Keep, Some(0_u8)),
                (1, 4, gif::DisposalMethod::Background, None),
                (3, 1, gif::DisposalMethod::Previous, None),
                (0, 0, gif::DisposalMethod::Any, Some(3)),
            ]
            .into_iter()
            .enumerate()
            {
                let pixels = vec![
                    (index % 3) as u8,
                    0,
                    3,
                    (index % 4) as u8,
                    2,
                    0,
                    1,
                    3,
                    (index % 2) as u8,
                ];
                let mut frame = gif::Frame::from_indexed_pixels(3, 3, pixels, transparent);
                frame.left = left;
                frame.top = top;
                frame.dispose = dispose;
                frame.delay = 10;
                encoder.write_frame(&frame).expect("and a patch on it");
            }
        }

        // The oracle: the library this module used to hand its frames to.
        let oracle: Vec<Vec<u8>> = image::codecs::gif::GifDecoder::new(Cursor::new(&bytes[..]))
            .expect("image opens the same fixture")
            .into_frames()
            .collect_frames()
            .expect("and decodes all of it")
            .into_iter()
            .map(|frame| frame.into_buffer().into_raw())
            .collect();
        assert_eq!(oracle.len(), 5, "five frames, five compositions");

        let animation = decode_bytes(bytes).expect("and this module opens it too");
        assert_eq!(
            animation.frames_in_file(),
            Some(5),
            "and reads the same five frames"
        );
        for (index, (mine, theirs)) in animation.ring.iter().take(5).zip(&oracle).enumerate() {
            // The one difference between the two is deliberate and is the reason
            // this module touches a pixel at all: ours is in the swapchain's
            // byte order.
            let bgra: Vec<u8> = theirs
                .chunks_exact(4)
                .flat_map(|rgba| [rgba[2], rgba[1], rgba[0], rgba[3]])
                .collect();
            assert_eq!(
                mine.bgra.as_ref(),
                bgra.as_slice(),
                "frame {index} is composed the way the specification says",
            );
        }
    }

    /// PIN — **the four ways this module declines, and none of them is a
    /// panic.**
    ///
    /// A `.png` is not this lane's; bytes that announce no container at all are
    /// not either; bytes that announce a GIF and then will not open are a broken
    /// GIF rather than a stranger; a single-frame GIF is a picture the picture
    /// channel already draws; and a file whose frames are too large to stream is
    /// drawn as its first frame and left still. Every one of those is a `.gif` a
    /// reader can hover, and the last two of them are the two a reader is owed a
    /// sentence about.
    #[test]
    fn a_file_that_is_not_an_animation_says_so_rather_than_pretending() {
        // `Result<Animation, _>` is deliberately not `PartialEq` — an animation
        // is megabytes of pixels and comparing two of them is never what a
        // caller means — so these read the error rather than the whole answer.
        let refusal = |result: Result<Animation, AnimationRefusal>| result.err();
        assert_eq!(
            refusal(decode(std::path::Path::new(r"D:\shots\a.png"))),
            Some(AnimationRefusal::NotAnAnimation)
        );
        assert!(path_names_an_animation(std::path::Path::new(r"D:\a\b.GIF")));
        assert!(!path_names_an_animation(std::path::Path::new(
            r"D:\a\b.gifx"
        )));
        // **A header that says GIF is taken at its word, and then the bytes
        // behind it are the thing that fails.** These two are a pair and the
        // difference between them is the whole of why there are two refusals:
        // the first announces the container this module opens and then has
        // nothing openable in it, which is `Undecodable`; the second announces
        // nothing at all, which is `NotAnAnimation`. Answering the first with
        // "not an animation" would be this module telling a reader their `.gif`
        // is not a `.gif`, when what is true is that it is a broken one.
        //
        // The screen descriptor is written out — a 16-square screen with no
        // global colour table — rather than left to whatever the prose after
        // `GIF89a` happened to spell. It used to be the prose, and once the
        // decoder was given limits (review row R1-7) the two bytes of `" b"`
        // turned out to declare a screen 25120 pixels wide, so the refusal that
        // came back was `TooLarge`: a true answer to a fixture that had never
        // meant to ask that question.
        assert_eq!(
            refusal(decode_bytes(
                b"GIF89a\x10\x00\x10\x00\x00\x00\x00 but not really".to_vec()
            )),
            Some(AnimationRefusal::Undecodable)
        );
        assert_eq!(
            refusal(decode_bytes(Vec::new())),
            Some(AnimationRefusal::NotAnAnimation)
        );
        // A `.gif` with one frame in it is a still picture, and says so.
        assert_eq!(
            refusal(decode_bytes(a_gif_of(1, 16, 10))),
            Some(AnimationRefusal::OneFrame)
        );
        // And a header whose screen this window could never draw is refused for
        // that, before anything behind it is read.
        assert_eq!(
            refusal(decode_bytes(a_gif_declaring(20_000, 4))),
            Some(AnimationRefusal::TooLarge)
        );
        // A frame too large to stream is the one shape `TooLarge` still means:
        // 2048 square is 16 MiB a frame, which is the whole ring twice over.
        assert_eq!(
            refusal(decode_bytes(a_gif_declaring(2_048, 2_049))),
            Some(AnimationRefusal::TooLarge)
        );
        // The two a reader is owed a sentence about, and the two they are not.
        assert!(AnimationRefusal::TooLarge.is_worth_saying());
        assert!(AnimationRefusal::Undecodable.is_worth_saying());
        assert!(!AnimationRefusal::OneFrame.is_worth_saying());
        assert!(!AnimationRefusal::NotAnAnimation.is_worth_saying());
        // The ceilings are constants a reader can find and not numbers buried in
        // a comparison.
        assert_eq!(MAX_ANIMATION_RING_BYTES, 32 * 1024 * 1024);
        assert_eq!(MAX_ANIMATION_FRAME_BYTES, 16 * 1024 * 1024);
        assert_eq!(MAX_ANIMATION_HELD_BYTES, 40 * 1024 * 1024);
        assert_eq!(MAX_ANIMATION_SIDE_PX, 8192);
        assert_eq!(MAX_ANIMATION_FILE_BYTES, 8 * 1024 * 1024);
    }

    /// PIN — **a redraw inside the standing frame's own delay uploads nothing**
    /// (§7.44 ⑤, on §7.42 ⑥'s gate).
    ///
    /// The generation is the renderer's upload gate, and it counts *changes of
    /// frame*. A build that bumped it per tick would spend a megabyte of bus per
    /// redraw writing the pixels that are already there — which is the exact
    /// cost `VideoFrameUpload::generation` exists to refuse.
    #[test]
    fn an_animation_that_has_not_changed_frame_uploads_nothing() {
        let mut animation = decode_bytes(fixture()).expect("four frames");
        let start = Instant::now();
        animation.due_at = start + Duration::from_millis(100);
        let first = animation.upload().generation;
        // Five ticks inside the first frame's hundred milliseconds.
        for tick in [0_u64, 16, 32, 48, 64, 80, 99] {
            assert!(
                !animation.advance(start + Duration::from_millis(tick)),
                "at {tick}ms the frame has not changed"
            );
            assert_eq!(animation.upload().generation, first);
        }
        assert!(animation.advance(start + Duration::from_millis(100)));
        assert_eq!(animation.upload().generation, first + 1);
        // And a tick that skipped a whole frame lands on the one that is *due*,
        // not on the next one along.
        assert!(animation.advance(start + Duration::from_millis(650)));
        assert_eq!(animation.frame_index(), 3);
    }

    /// PIN — **a declared delay of nothing is a tenth of a second, and a very
    /// short one is twenty milliseconds.**
    ///
    /// The reading every browser settled on, and this window reads it the same
    /// way for the same reason: honouring a hundredth literally is a request to
    /// spin a core, and an animation is not entitled to more of one than
    /// everything else on the glass together.
    #[test]
    fn a_frame_that_declares_no_time_is_given_the_browsers_reading() {
        assert_eq!(frame_delay(0), DEFAULT_FRAME_DELAY);
        assert_eq!(frame_delay(1), MIN_FRAME_DELAY);
        assert_eq!(frame_delay(5), Duration::from_millis(50));
        let frames = vec![
            AnimationFrame {
                bgra: Arc::from(vec![0_u8; 4]),
                delay: MIN_FRAME_DELAY,
            },
            AnimationFrame {
                bgra: Arc::from(vec![255_u8; 4]),
                delay: DEFAULT_FRAME_DELAY,
            },
        ];
        let animation = Animation::of(frames, 1, 1, Instant::now());
        let turn: Duration = animation.ring.iter().map(|frame| frame.delay).sum();
        assert_eq!(turn, MIN_FRAME_DELAY + DEFAULT_FRAME_DELAY);
        assert_eq!(DEFAULT_FRAME_DELAY, Duration::from_millis(100));
        assert_eq!(MIN_FRAME_DELAY, Duration::from_millis(20));
    }

    /// PIN — **the ring is asked for what it can hold and what it can use**,
    /// whichever is the smaller.
    ///
    /// The two bounds are meant to bind at opposite ends of the range of files
    /// this window sees, and a build that dropped either of them would look
    /// right on the other's fixture: bytes alone queue two thousand frames of a
    /// spinner, time alone queues a hundred megabytes of a capture.
    #[test]
    fn the_ring_is_bounded_by_its_bytes_and_by_its_clock() {
        // A spinner: sixteen kilobytes a frame, so what binds is the clock — ten
        // frames of a tenth of a second, and not the two thousand it could hold.
        let tiny = u64::from(64_u32 * 64 * 4);
        assert_eq!(
            frames_wanted(tiny, 0, Duration::ZERO, DEFAULT_FRAME_DELAY),
            10
        );
        // The capture that opened this: one and a half megabytes a frame at
        // eighty milliseconds, and the clock binds again at thirteen.
        let capture = 820 * 462 * 4;
        assert_eq!(
            frames_wanted(capture, 0, Duration::ZERO, Duration::from_millis(80)),
            13
        );
        // The same capture at 1080p, where a second of it is a hundred megabytes
        // — so the bytes bind, at four.
        let full_hd = 1_920 * 1_080 * 4;
        assert_eq!(
            frames_wanted(full_hd, 0, Duration::ZERO, Duration::from_millis(80)),
            (MAX_ANIMATION_RING_BYTES / full_hd) as usize,
        );
        // A full ring wants nothing, by either reading.
        assert_eq!(
            frames_wanted(
                tiny,
                MAX_ANIMATION_RING_BYTES,
                Duration::ZERO,
                MIN_FRAME_DELAY
            ),
            0
        );
        assert_eq!(
            frames_wanted(tiny, 0, MAX_ANIMATION_RING_LEAD, MIN_FRAME_DELAY),
            0
        );
    }
}
