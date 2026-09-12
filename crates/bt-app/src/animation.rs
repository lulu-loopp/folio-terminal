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
//! * an [`AnimationCursor`] — a handle on the file, a sequential reader over it
//!   and the canvas its frames are composed onto — which is handed to a worker
//!   to be filled and comes back parked in the animation until it is wanted
//!   again.
//!
//! **The cursor is where the design is.** GIF is a sequential container: frame
//! *n* is a patch on the composition of every frame before it, so there is no
//! seeking, and a decoder that has read to the end of the file is a decoder that
//! must **start again from the first frame** to play the loop a second time.
//! That re-read is the honest cost of not holding the whole file, and it is what
//! every browser does with a long animation.
//!
//! # And the file is read, not held (user report 2026-09-12)
//!
//! **What was wrong, the third time.** The streaming above was written over an
//! `Arc<[u8]>` of the whole file: the cursor kept every byte because that was
//! the only way it knew back to frame zero, [`Animation::bytes_held`] charged
//! those bytes to the window's ceiling, and so a cap on the *file's length* had
//! to exist — [`MAX_ANIMATION_FILE_BYTES`], set to `bt_term::MAX_INLINE_IMAGE_BYTES`,
//! which is **eight megabytes** and was chosen for a picture a shell pastes into
//! a scrollback. A reader opened an 11.7 MB `simulation.gif` and got an empty
//! pane: the animation was refused for its length before a byte of it was
//! decoded, and the still that was to stand in for it was refused by the picture
//! lane's copy of the same eight megabytes. Two refusals for one file, both of
//! them about a number that has nothing to do with what the file costs to play.
//! A GIF of ten to thirty megabytes is the ordinary output of a screen recorder.
//!
//! So the loop comes round by **seeking the file**, not by holding it: an
//! [`AnimationSource`] is a reader that can be told to start again, the
//! production one is a bounded reader over a [`std::fs::File`], and what stays
//! in memory is only what the ring already bounds. The cap on file length stays
//! as a **sanity** bound and stops being a memory charge — see
//! [`MAX_ANIMATION_FILE_BYTES`] for the number and why it is the size it is.
//!
//! **A file may change under the loop.** The cursor keeps the [`AnimationStamp`]
//! it opened with — when the file was last written and how long it was, the same
//! identity the decode memo keys on (§7.1.3k ⑧) — and compares it when the loop
//! comes round. A file that has been rewritten ends its playback rather than
//! being decoded half-old and half-new; the window lets that playback go and
//! opens the file again down the ordinary path, which is what shows the new one.
//! A file *deleted* mid-play is not that case: the open handle keeps it readable
//! on Windows until this cursor drops it, so the animation plays to the end of
//! its loop, and it is the next open that fails — the pane then shows the
//! ordinary not-found foot, which is the same answer any other vanished file
//! gets.
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
//! window appearing not to work. The two are two sentences and not one
//! ([`AnimationRefusal::FrameTooLarge`] and [`AnimationRefusal::FileTooLong`]),
//! because "its frames are too big" and "it is longer than this window will
//! read" are different facts about different numbers and a reader who is told
//! the wrong one is told something untrue.
//!
//! # And that first frame is decoded here
//!
//! [`first_frame`] is the still lane's answer for a `.gif`: exactly one frame,
//! pulled through the same streaming cursor under the same
//! [`MAX_ANIMATION_FRAME_BYTES`]. It is here and not in the picture decoder
//! because the picture decoder reads a whole file into memory behind the eight
//! megabytes an inline image is worth, and a `.gif` this window is happy to
//! *play* may be forty times that. A file this lane will not open — a JPEG that
//! is called `.gif`, a frame past the ceiling — answers `None` and the picture
//! lane decodes it, which is the fork and not a fallback: the name said GIF and
//! only the bytes could say otherwise.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::num::NonZeroU64;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gif::{ColorOutput, DisposalMethod, MemoryLimit};
use image::ImageFormat;

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

/// **What one animation costs this window at its fullest**, all three
/// allocations of it (adversarial review 2026-09-11, B9; user report
/// 2026-09-12).
///
/// It was once two — the ring, and the file's own bytes — and B9 found two more
/// that no counter in this window had ever heard of:
///
/// * the **canvas** ([`AnimationCursor::over`]), one whole logical screen that
///   every frame is composed onto and that lives for as long as the cursor
///   does, which at 2048 square is another sixteen megabytes; and
/// * the **fill in flight**, up to a whole ring's worth of frames being
///   composed on the worker ([`frames_wanted`]) — outside the window's map from
///   the moment the cursor leaves until it is parked again.
///
/// **And then one of the original two went away.** The file's own bytes are no
/// longer held at all: the cursor reads the file through a handle and seeks back
/// to the top when the loop comes round (see the module note), so the length of
/// the file is not a number this window is holding and must not be a number it
/// is charged for. What is left is `32 + 16 + 32`, and every term of it is
/// pixels this process really has.
pub const MAX_ANIMATION_HELD_BYTES: u64 =
    MAX_ANIMATION_RING_BYTES + MAX_ANIMATION_FRAME_BYTES + MAX_ANIMATION_RING_BYTES;

/// **How many bytes of a file this window will read looking for frames** — a
/// **sanity** bound, and no longer a memory charge (user report 2026-09-12).
///
/// 512 MiB. It used to be `bt_term::MAX_INLINE_IMAGE_BYTES` — eight megabytes,
/// the allowance for a picture a shell pastes into a scrollback — and it had to
/// be a small number because the whole file was held in memory. It is not held
/// any more, so the only question this number still answers is "what length is
/// so large that a file claiming it cannot be an animation a reader meant to
/// open", and the number is chosen to be nowhere near anything real: the file
/// this was reported against was **11.7 MB**, the largest in the folder it came
/// from was **39.9 MB**, and this is more than a dozen times that.
///
/// It is not dead weight even so. The length is read off the handle the bytes
/// are then read through and the read itself is bounded by this cap rather than
/// by that length ([`FileAnimationSource`]), so a file being appended to between
/// the two — a capture still being written — cannot hand this window bytes
/// without end.
pub const MAX_ANIMATION_FILE_BYTES: u64 = 512 * 1024 * 1024;

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

/// **The first frame of one animated file, as the picture lane's pixels** — see
/// [`first_frame`].
///
/// RGBA and not the BGRA the ring carries, because the two are going to two
/// different places: a played frame goes to a texture created in the swapchain's
/// own order (§7.42 ②) and this goes into `bt_term::DecodedInlineImage`, which
/// is RGBA everywhere it is read. The conversion is one pass over one frame,
/// once per file, on the worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnimationStill {
    /// What the shared GPU cache is to call these pixels — see
    /// [`still_texture_key`].
    pub key: String,
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
}

/// **Which file this is, and which version of it**: when it was last written
/// and how long it is.
///
/// The identity `bt_term`'s decode memo already keys on (§7.1.3k ⑧), asked here
/// for the two questions this module has that a path alone cannot answer — is
/// the file under the loop still the file the loop started on, and are these
/// still pixels the same picture as the ones the renderer is holding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnimationStamp {
    modified: Option<SystemTime>,
    length: u64,
}

impl AnimationStamp {
    /// One `metadata` call, and `None` for a file that is not there to stat.
    #[must_use]
    pub fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: metadata.modified().ok(),
            length: metadata.len(),
        })
    }

    /// How this stamp is spelled in a texture key.
    fn spelled(self) -> String {
        let modified = self
            .modified
            .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or_else(|| "?".to_owned(), |since| since.as_nanos().to_string());
        format!("{modified}:{}", self.length)
    }
}

/// **Where one animation's bytes come from**, which is a file, and the two
/// things a looping decoder asks of them (user report 2026-09-12).
///
/// It is a trait for one reason and it is not abstraction for its own sake:
/// production reads a [`std::fs::File`] and the tests in this module hold their
/// fixtures as bytes, and a design in which every test of the loop had to write
/// a temporary file would be a design nobody would keep tests for. Both halves
/// go through the same two verbs below, so the thing under test is the thing
/// that ships.
///
/// **`Send`, because the cursor crosses to a worker** — see [`AnimationCursor`].
pub trait AnimationSource: Read + Send {
    /// **Start again at the first byte**, which is all that coming round to
    /// frame zero is once the file is not being held.
    ///
    /// # Errors
    ///
    /// Whatever the seek failed with. A source that cannot start again ends the
    /// animation on the frame it was standing on.
    fn restart(&mut self) -> std::io::Result<()>;

    /// **What the file looked like when this source opened it**, or `None` for
    /// a source that is not a file.
    fn opened_as(&self) -> Option<AnimationStamp>;

    /// **What the file at that name looks like now.**
    ///
    /// Compared with [`Self::opened_as`] when the loop comes round: a file that
    /// has been rewritten under the loop ends this playback rather than being
    /// decoded half-old and half-new. For a source with no file behind it the
    /// two are both `None`, which reads as "unchanged" and is the truth about a
    /// slice of bytes.
    fn looks_like_now(&self) -> Option<AnimationStamp>;
}

/// **A bounded reader over an open file** — what every animation this window
/// plays is read through.
///
/// The handle is kept open for the life of the cursor, which is what makes
/// [`Self::restart`] a seek rather than a second `open`, and is also why a file
/// deleted mid-play goes on playing: on Windows the name goes and the handle
/// keeps the bytes readable until it is dropped.
pub struct FileAnimationSource {
    file: std::fs::File,
    path: std::path::PathBuf,
    opened_as: AnimationStamp,
    /// How much of this pass has been handed out, against
    /// [`MAX_ANIMATION_FILE_BYTES`] — the read is bounded by the cap and not by
    /// the length that was stat-ed, so a capture still being written to cannot
    /// hand this window bytes without end.
    read: u64,
}

impl FileAnimationSource {
    /// Open `path` for reading, with the stamp it had at the moment it opened.
    ///
    /// # Errors
    ///
    /// The open itself. Nothing else is read here.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        // **Asked of the handle and not of the name**, which is the same
        // discipline the length has always been read under: a stat of the name
        // and an open of the name are two different files the moment somebody
        // writes between them, and what this stamp has to describe is the bytes
        // that are about to be read. [`Self::looks_like_now`] asks the *name*,
        // because that is the other half of the question — is the file at that
        // name still the one this handle holds.
        let metadata = file.metadata()?;
        Ok(Self {
            file,
            path: path.to_owned(),
            opened_as: AnimationStamp {
                modified: metadata.modified().ok(),
                length: metadata.len(),
            },
            read: 0,
        })
    }

    /// What the handle said this file was when it opened.
    #[must_use]
    pub fn stamp(&self) -> AnimationStamp {
        self.opened_as
    }
}

impl Read for FileAnimationSource {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let room = MAX_ANIMATION_FILE_BYTES.saturating_sub(self.read);
        if room == 0 {
            // **The cap reads as the end of the file**, deliberately: a pass
            // that reaches it ends the way a truncated file ends, on the frames
            // it did get, rather than by raising an error a reader would have to
            // be told about for a file no reader will ever open.
            return Ok(0);
        }
        let want = buffer
            .len()
            .min(usize::try_from(room).unwrap_or(usize::MAX));
        let read = self.file.read(&mut buffer[..want])?;
        self.read += read as u64;
        Ok(read)
    }
}

impl AnimationSource for FileAnimationSource {
    fn restart(&mut self) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.read = 0;
        Ok(())
    }

    fn opened_as(&self) -> Option<AnimationStamp> {
        Some(self.opened_as)
    }

    fn looks_like_now(&self) -> Option<AnimationStamp> {
        AnimationStamp::of(&self.path)
    }
}

/// **Bytes already in hand** — the half a test can hold, behind the same two
/// verbs the file answers.
///
/// `cfg(test)` and not shipped: every animation this window draws is a file on a
/// disk, and a second production way in would be a second set of answers to
/// "what happens when the file changes".
#[cfg(test)]
pub struct BytesAnimationSource {
    bytes: Arc<[u8]>,
    at: usize,
}

#[cfg(test)]
impl BytesAnimationSource {
    #[must_use]
    pub fn over(bytes: Arc<[u8]>) -> Self {
        Self { bytes, at: 0 }
    }
}

#[cfg(test)]
impl Read for BytesAnimationSource {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let rest = &self.bytes[self.at.min(self.bytes.len())..];
        let read = rest.len().min(buffer.len());
        buffer[..read].copy_from_slice(&rest[..read]);
        self.at += read;
        Ok(read)
    }
}

#[cfg(test)]
impl AnimationSource for BytesAnimationSource {
    fn restart(&mut self) -> std::io::Result<()> {
        self.at = 0;
        Ok(())
    }

    fn opened_as(&self) -> Option<AnimationStamp> {
        None
    }

    fn looks_like_now(&self) -> Option<AnimationStamp> {
        None
    }
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
    /// The decoder, and the source it owns.
    ///
    /// **An `Option` because coming round to frame zero is a move out of this
    /// field**: the decoder owns the handle, `gif::Decoder::into_inner` is by
    /// value, and the handle has to be recovered to be seeked and given to a new
    /// decoder. It is `Some` at every point a caller can observe; the one moment
    /// it is not is inside [`Self::start_again`], which either puts a decoder
    /// back or ends the cursor.
    reader: Option<gif::Decoder<Box<dyn AnimationSource>>>,
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
    /// **Set when the file was rewritten under the loop** (user report
    /// 2026-09-12): the stamp it was opened with and the stamp its name carries
    /// now disagree at the moment the loop comes round.
    ///
    /// It ends the cursor like any other end, and it is a separate fact from
    /// [`Self::ended`] because the window does a different thing with it: a
    /// playback that ran out of file stands on its last frame, and a playback
    /// whose file changed is let go of so the pane opens the new file down the
    /// ordinary path. See `adopt_animation_fill`.
    stale: bool,
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
    ///
    /// **It says nothing about *which* animation this is**, and the layer key
    /// the renderer compares it under has to (adversarial review 2026-09-11,
    /// B3): every animation starts this at one, so a surface handed a second
    /// file whose counter is behind the first's has its frames rejected until it
    /// catches up. The identity is the window's to mint — see
    /// `AnimationEntry::Ready` — and this counter only ever answers "is this the
    /// picture you already hold" *within* one playback.
    generation: u64,
    /// **The cursor's canvas**, counted whether the cursor is parked here or
    /// away on a worker (adversarial review 2026-09-11, B9): the allocation
    /// belongs to this animation either way, and a count that forgot it while it
    /// was away would be a ceiling that rose every time a fill was posted.
    canvas_bytes: u64,
    /// **What the fill now in flight will bring back**, reserved when the cursor
    /// leaves and released when it is parked again (B9).
    ///
    /// Frames being composed on the worker are bytes this process is holding and
    /// nothing had charged them: a window at its ceiling could have a whole
    /// second ring's worth of pixels in the air behind it.
    in_flight_bytes: u64,
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
    /// **One frame of it is too big to stream** — its declared screen is over
    /// [`MAX_ANIMATION_SIDE_PX`], or one composed frame of it would be over
    /// [`MAX_ANIMATION_FRAME_BYTES`].
    ///
    /// Never a verdict on how *many* frames a file has: that was this module's
    /// own limit and streaming retired it (user report 2026-09-10). A file that
    /// earns this is drawn as its first frame and the foot of the pane says why.
    FrameTooLarge,
    /// **The file is longer than [`MAX_ANIMATION_FILE_BYTES`]**, which is a
    /// sanity bound and not a memory one (user report 2026-09-12).
    ///
    /// A separate verdict from [`Self::FrameTooLarge`] because it is a separate
    /// fact about a separate number, and the foot of the pane says which: the
    /// two were one variant, spelled "First frame · too large", and a reader
    /// whose 11.7 MB recording was refused for its *length* was told its frames
    /// were too big. They were 2400 by 1200, which composes to 11.5 MB — well
    /// inside [`MAX_ANIMATION_FRAME_BYTES`].
    FileTooLong,
}

impl AnimationRefusal {
    /// **Whether a reader is owed a sentence about this.**
    ///
    /// Two of the five are ordinary answers to an ordinary question and say
    /// nothing: a file that is not an animation is being drawn by the lane that
    /// does draw it, and a one-frame `.gif` is a still picture that looks
    /// exactly like a still picture. The other three leave a reader looking at a
    /// picture that ought to be moving, which is a thing this window has to
    /// account for — see `Runtime::preview_foot_notice`.
    #[must_use]
    pub fn is_worth_saying(self) -> bool {
        match self {
            Self::NotAnAnimation | Self::OneFrame => false,
            Self::Undecodable | Self::FrameTooLarge | Self::FileTooLong => true,
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
pub fn decode(path: &Path) -> Result<Animation, AnimationRefusal> {
    stream(file_source(path)?)
}

/// **The handle one animated file is read through**, and the two things that can
/// be decided about it before a byte of it is read: the name, and the length.
///
/// The length is asked of the handle the bytes are then read through and the
/// read is bounded by the cap rather than by that length — the discipline
/// `pdf::read_capped` states in the same words, for the same reason — but since
/// 2026-09-12 the cap is a sanity bound rather than a memory one, because the
/// bytes are not kept. See [`MAX_ANIMATION_FILE_BYTES`].
fn file_source(path: &Path) -> Result<Box<dyn AnimationSource>, AnimationRefusal> {
    if !path_names_an_animation(path) {
        return Err(AnimationRefusal::NotAnAnimation);
    }
    let source = FileAnimationSource::open(path).map_err(|_| AnimationRefusal::Undecodable)?;
    if source.stamp().length > MAX_ANIMATION_FILE_BYTES {
        return Err(AnimationRefusal::FileTooLong);
    }
    Ok(Box::new(source))
}

/// The same, from bytes already in hand — the half a test can hold.
///
/// It takes the bytes rather than borrowing them because the source keeps them:
/// a looping decoder has to be able to start again, and a slice that has to
/// outlive this call is a lifetime on a thing that crosses to a worker.
#[cfg(test)]
pub fn decode_bytes(bytes: Vec<u8>) -> Result<Animation, AnimationRefusal> {
    stream(Box::new(BytesAnimationSource::over(Arc::from(bytes))))
}

/// **Open one animated file and read the ring's worth of frames behind it.**
fn stream(source: Box<dyn AnimationSource>) -> Result<Animation, AnimationRefusal> {
    let (mut cursor, width_px, height_px) = open(source)?;
    let frame_bytes = u64::from(width_px) * u64::from(height_px) * 4;
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
        cursor,
        Instant::now(),
    ))
}

/// **One frame of one animated file, for the lane that draws still pictures**
/// (user report 2026-09-12).
///
/// `None` is "not this lane's file", and the caller decodes it with the picture
/// decoder: a name that is not `.gif`, a path this window does not read unasked,
/// a container that turns out not to be a GIF after all, a frame past the
/// ceiling. Everything else comes back as exactly one composed frame — the same
/// cursor, the same bounds, one call of [`AnimationCursor::next_frames`] — which
/// is what lets a `.gif` far past the picture lane's eight-megabyte file cap
/// still show a picture when this lane will not *play* it.
#[must_use]
pub fn first_frame(path: &Path) -> Option<AnimationStill> {
    // **The gate the picture lane reads behind, asked here because this lane is
    // now reading the same files** (route A of the untrusted-path audit,
    // 2026-09-08). `bt_term`'s `read_and_decode_local_image` asks it one line
    // above its own open; a fork that skipped it would be a way to the disk that
    // did not.
    if !bt_transcript::paths::may_read_unasked_through_links(
        path,
        bt_transcript::paths::PathNamer::ThisWindow,
    ) {
        return None;
    }
    let source = file_source(path).ok()?;
    let stamp = source.opened_as();
    let (mut cursor, width_px, height_px) = open(source).ok()?;
    let frame = cursor.next_frames(1).into_iter().next()?;
    Some(AnimationStill {
        key: still_texture_key(path, stamp, width_px, height_px),
        // The ring's frames are in the swapchain's byte order because they go
        // to a texture created in it; these go to `DecodedInlineImage`, which
        // is RGBA. One pass over one frame, once per file, on the worker.
        rgba: Arc::from(
            frame
                .bgra
                .chunks_exact(4)
                .flat_map(|bgra| [bgra[2], bgra[1], bgra[0], bgra[3]])
                .collect::<Vec<u8>>(),
        ),
        width_px,
        height_px,
    })
}

/// **What the shared GPU cache calls one animation's first frame**: the file,
/// when it was last written, and the size it came back at.
///
/// `video_frame_texture_key`'s twin, for its reason said about the other kind of
/// moving picture — a still picture's texture is named by a hash of its own
/// bytes, and this one cannot be, because the bytes it was composed from are a
/// container this process never held whole.
fn still_texture_key(
    path: &Path,
    stamp: Option<AnimationStamp>,
    width_px: u32,
    height_px: u32,
) -> String {
    let stamp = stamp.map_or_else(|| "?".to_owned(), AnimationStamp::spelled);
    format!(
        "gif-frame:{}:{stamp}:{width_px}x{height_px}",
        path.display()
    )
}

/// **Judge one animated file's descriptor, and hand back the cursor over it.**
///
/// The order is the point (review row R1-7): everything that can be known from
/// the header is decided here, **before** [`AnimationCursor::over`] allocates a
/// canvas — and no frame is pulled here at all, because the two callers want
/// different numbers of them.
fn open(
    source: Box<dyn AnimationSource>,
) -> Result<(Box<AnimationCursor>, u32, u32), AnimationRefusal> {
    // The container is judged by its own header and not by the name that led
    // here, which is the discipline `decode_image_bytes_within` already keeps: a
    // `.gif` that is a JPEG is a JPEG. Read off the front of the stream and then
    // wound back, because a stream cannot be guessed at twice.
    let mut source = source;
    let mut head = [0_u8; 16];
    let read = read_head(&mut *source, &mut head).map_err(|_| AnimationRefusal::Undecodable)?;
    if image::guess_format(&head[..read]).ok() != Some(ImageFormat::Gif) {
        return Err(AnimationRefusal::NotAnAnimation);
    }
    source
        .restart()
        .map_err(|_| AnimationRefusal::Undecodable)?;
    let reader = gif_reader(source).map_err(|_| AnimationRefusal::Undecodable)?;
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
        return Err(AnimationRefusal::FrameTooLarge);
    }
    let frame_bytes = u64::from(width_px) * u64::from(height_px) * 4;
    if frame_bytes > MAX_ANIMATION_FRAME_BYTES {
        return Err(AnimationRefusal::FrameTooLarge);
    }
    let cursor = AnimationCursor::over(reader, width_px, height_px);
    Ok((Box::new(cursor), width_px, height_px))
}

/// Fill as much of `head` as the source has, so a short file is guessed at on
/// what it does have rather than on a buffer of zeroes.
fn read_head(source: &mut dyn AnimationSource, head: &mut [u8]) -> std::io::Result<usize> {
    let mut read = 0;
    while read < head.len() {
        match source.read(&mut head[read..])? {
            0 => break,
            more => read += more,
        }
    }
    Ok(read)
}

/// One decoder over one file, opened at the header.
fn gif_reader(
    source: Box<dyn AnimationSource>,
) -> Result<gif::Decoder<Box<dyn AnimationSource>>, gif::DecodingError> {
    // The frame ceiling goes onto the decoder as its own memory limit as well as
    // being checked above: one guards the composition this module writes, the
    // other guards the buffer the `gif` crate allocates for a frame whose
    // sub-rectangle is larger than the screen that declared it.
    const FRAME_LIMIT: NonZeroU64 = NonZeroU64::new(MAX_ANIMATION_FRAME_BYTES).unwrap();
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(ColorOutput::RGBA);
    options.set_memory_limit(MemoryLimit::Bytes(FRAME_LIMIT));
    options.read_info(source)
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
    fn over(reader: gif::Decoder<Box<dyn AnimationSource>>, width_px: u32, height_px: u32) -> Self {
        // Checked against [`MAX_ANIMATION_FRAME_BYTES`] by the only caller,
        // before this allocation is reached.
        let canvas = vec![0_u8; (width_px as usize) * (height_px as usize) * 4];
        Self {
            reader: Some(reader),
            width_px,
            height_px,
            canvas,
            pass_frames: 0,
            frames_in_file: None,
            ended: false,
            stale: false,
        }
    }

    /// **Whether the file was rewritten under the loop** (user report
    /// 2026-09-12) — see [`Self::stale`] and `adopt_animation_fill`.
    #[must_use]
    pub fn file_changed(&self) -> bool {
        self.stale
    }

    /// How many frames the file holds, once one whole pass of it has been read.
    #[must_use]
    pub fn frames_in_file(&self) -> Option<u64> {
        self.frames_in_file
    }

    /// **What the composition canvas costs** — one whole logical screen, alive
    /// for as long as this cursor is (adversarial review 2026-09-11, B9).
    #[must_use]
    pub fn canvas_bytes(&self) -> u64 {
        self.canvas.len() as u64
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
        let frame = match self.reader.as_mut()?.read_next_frame() {
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

    /// **Wind the file back to its first frame**, which is what a loop is (user
    /// report 2026-09-12).
    ///
    /// It used to be a second decoder over bytes this cursor was holding; now it
    /// is a seek on the handle it has had open all along, which is the whole of
    /// what made the file's length stop being this window's memory.
    ///
    /// **A file that has been rewritten ends here instead.** The stamp it opened
    /// with and the stamp its name carries now are compared at exactly this
    /// moment — the one moment the decoder is between passes and a change can be
    /// acted on rather than decoded into the middle of a picture. The window
    /// lets the playback go and opens the file again down the ordinary path,
    /// which is what shows the new one.
    fn start_again(&mut self) -> bool {
        let Some(reader) = self.reader.take() else {
            return false;
        };
        let mut source = reader.into_inner().into_inner();
        if source.looks_like_now() != source.opened_as() {
            self.stale = true;
            return false;
        }
        if source.restart().is_err() {
            return false;
        }
        let Ok(reader) = gif_reader(source) else {
            return false;
        };
        self.reader = Some(reader);
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
    /// Three allocations, and every one of them is pixels (adversarial review
    /// 2026-09-11, B9; user report 2026-09-12): the **ring**, the **canvas**
    /// every frame is composed onto — a whole logical screen, outliving any one
    /// frame — and the **fill in flight**, which is frames this process is
    /// holding on a worker thread. B9 found the last two; what went away after
    /// it was the file, which this animation no longer holds at all (see the
    /// module note), so charging its length here would be charging for memory
    /// nobody has.
    #[must_use]
    pub fn bytes_held(&self) -> u64 {
        self.ring_bytes() + self.canvas_bytes + self.in_flight_bytes
    }

    /// **Whether the file changed under the loop** — asked of the cursor, which
    /// is the thing that noticed, and `false` while the cursor is away.
    ///
    /// A playback that answers `true` is one the window lets go of: see
    /// `adopt_animation_fill`, which is the only caller and the only moment the
    /// cursor is certainly home.
    #[must_use]
    pub fn file_changed(&self) -> bool {
        self.cursor
            .as_ref()
            .is_some_and(|cursor| cursor.file_changed())
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
        cursor: Box<AnimationCursor>,
        started: Instant,
    ) -> Self {
        let ring: VecDeque<AnimationFrame> = frames.into();
        let first = ring
            .front()
            .map_or(DEFAULT_FRAME_DELAY, |frame| frame.delay);
        let canvas_bytes = cursor.canvas_bytes();
        Self {
            ring,
            width_px,
            height_px,
            cursor: Some(cursor),
            standing_seq: 0,
            // **A guess, and it is replaced the moment a reader can see it.**
            // This is stamped on the *worker* thread, one whole trip home before
            // the first frame is presented, and a completion that lands in a
            // busy turn used to have the first tick walk the ring to "catch up"
            // — a GIF that opened several frames in (adversarial review
            // 2026-09-11, B8). [`Self::present`] is what the window calls when
            // the picture is actually on the glass.
            due_at: started + first,
            generation: 1,
            canvas_bytes,
            in_flight_bytes: 0,
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
            cursor: None,
            standing_seq: 0,
            due_at: started + first,
            generation: 1,
            canvas_bytes: 0,
            in_flight_bytes: 0,
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
        let frame_bytes = self.frame_bytes();
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
        let cursor = self.cursor.take()?;
        // **Reserved before the request is posted, not charged when it lands**
        // (adversarial review 2026-09-11, B9). The bytes exist from the instant
        // the worker starts composing, so a ceiling that waited for them to
        // arrive was a ceiling with a ring's worth of pixels standing outside
        // it. The count is what the worker was *ordered* — the cursor may bring
        // fewer at the end of a truncated file, and [`Self::park_cursor`]
        // replaces the reservation with what actually came.
        self.in_flight_bytes = want as u64 * self.frame_bytes();
        Some((cursor, want))
    }

    /// What one composed frame of this animation costs.
    #[must_use]
    fn frame_bytes(&self) -> u64 {
        u64::from(self.width_px) * u64::from(self.height_px) * 4
    }

    /// **Take the cursor back, with what it decoded.**
    ///
    /// Also good for taking it back with nothing, which is what a request that
    /// could not be posted does: an animation whose cursor was lost would stand
    /// on its last frame for as long as the window is open.
    pub fn park_cursor(&mut self, cursor: Box<AnimationCursor>, frames: Vec<AnimationFrame>) {
        self.ring.extend(frames);
        self.canvas_bytes = cursor.canvas_bytes();
        self.cursor = Some(cursor);
        // The reservation ends where the frames it stood for begin: they are in
        // the ring now and `ring_bytes` counts them (B9).
        self.in_flight_bytes = 0;
    }

    /// **This animation's first frame is on the glass — start its clock here**
    /// (adversarial review 2026-09-11, B8).
    ///
    /// The clock used to start where the frames were *decoded*: on the worker
    /// thread, inside [`open`], one hand-off before any reader could see them.
    /// A completion that landed in a busy turn therefore arrived already late
    /// and [`Self::advance`] walked the ring to catch up, so a `.gif` opened
    /// several frames in — which is one half of "a GIF does not start from its
    /// first frame".
    ///
    /// It is the same sentence for the other half. An animation nobody is
    /// drawing does not advance, so its `due_at` is a moment in the past by the
    /// time a reader comes back to it; rebasing it here means a revealed
    /// animation stands its frame out from *now* instead of fast-forwarding
    /// through the time it spent hidden.
    ///
    /// Called by the window on the frame an animation goes from not-presented to
    /// presented, and on no other frame: an animation that is drawn every frame
    /// keeps the clock its own delays built.
    pub fn present(&mut self, now: Instant) {
        self.due_at = now
            + self
                .ring
                .front()
                .map_or(DEFAULT_FRAME_DELAY, |frame| frame.delay);
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
    use std::io::Cursor;

    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/assets/folio-anim-test.gif"),
        )
        .expect("the animation fixture is in tests/assets")
    }

    /// **A file a test may write**, under the build directory and therefore
    /// inside this checkout: the lane under test opens files now, so some of
    /// these tests need one, and none of them may go looking for somewhere to
    /// put it on the machine that is running them.
    ///
    /// Named for the test and the process, so two of them running at once are
    /// two files.
    fn scratch(name: &str) -> std::path::PathBuf {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/animation-tests");
        std::fs::create_dir_all(&directory).expect("a scratch directory under the build directory");
        directory.join(format!("{name}-{}.gif", std::process::id()))
    }

    /// The same, written and handed back — and removed by the caller.
    fn scratch_gif(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = scratch(name);
        std::fs::write(&path, bytes).expect("a scratch file this test owns");
        path
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
    /// they spend reaching it: both answer
    /// [`AnimationRefusal::FrameTooLarge`], and only one of them allocates
    /// seventeen gigabytes on the way. The order inside [`open`] is where the
    /// difference lives — the screen descriptor is read off the header and
    /// judged against [`MAX_ANIMATION_SIDE_PX`] and
    /// [`MAX_ANIMATION_FRAME_BYTES`] **before** [`AnimationCursor::over`]
    /// allocates a canvas — so the order is what is asserted on, and it is
    /// asserted on *before* the fixtures are decoded.
    ///
    /// **And no frame is pulled in there at all**, which is the shape since
    /// 2026-09-12: [`open`] judges and allocates the canvas, and its two callers
    /// pull as many frames as each of them wants — a ring for [`stream`], one
    /// for [`first_frame`].
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
        assert!(
            side < canvas && frame < canvas,
            "the screen is judged before a pixel is allocated:\n{walk}",
        );
        assert!(
            !walk.contains("next_frames("),
            "the judge does not decode: its callers ask for the frames they want:\n{walk}",
        );

        // And the verdict, at the size the review named and at one a machine can
        // survive being wrong about.
        for side in [16_384_u16, 65_535] {
            assert_eq!(
                decode_bytes(a_gif_declaring(side, side)).err(),
                Some(AnimationRefusal::FrameTooLarge),
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
    /// `.gif` pulled all of it into memory before anything looked at it. The cap
    /// it reads behind was `bt_term::MAX_INLINE_IMAGE_BYTES` until 2026-09-12
    /// and is now [`MAX_ANIMATION_FILE_BYTES`]'s own half-gigabyte sanity bound
    /// — the length of a file stopped being a thing this window holds — but the
    /// discipline is the same one and it is what this pins: the length is read
    /// off the handle the bytes are then read through, and nothing reads a whole
    /// file into memory to find out how long it is.
    ///
    /// MUTATION: go back to `std::fs::read` and the verdict is whatever the
    /// bytes happen to guess as, after all of them have been read.
    #[test]
    fn a_gif_past_the_encoded_cap_is_not_read_whole() {
        const SOURCE: &str = include_str!("animation.rs");
        for function in ["\npub fn decode(", "\nfn file_source("] {
            let at = SOURCE
                .find(function)
                .expect("the file reader is a free function in this file");
            let rest = &SOURCE[at..];
            let reader = &rest[..rest.find("\n}\n").expect("and it ends") + 3];
            assert!(
                !reader.contains("fs::read("),
                "a file is read behind a cap and not whole:\n{reader}",
            );
        }

        let path = scratch("past-the-cap");
        let _ = std::fs::remove_file(&path);
        let file = std::fs::File::create(&path).expect("a file this test owns");
        // Declared and not written: on NTFS this is a sparse file and costs
        // nothing, which is the only reason a half-gigabyte fixture is
        // affordable in a unit test.
        file.set_len(MAX_ANIMATION_FILE_BYTES + 1)
            .expect("a file of a declared length");
        drop(file);
        assert_eq!(
            decode(&path).err(),
            Some(AnimationRefusal::FileTooLong),
            "a file past the cap is refused on its length, and says so",
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
            Some(AnimationRefusal::FrameTooLarge)
        );
        // A frame too large to stream is the one shape `FrameTooLarge` means:
        // 2048 square is 16 MiB a frame, which is the whole ring twice over.
        assert_eq!(
            refusal(decode_bytes(a_gif_declaring(2_048, 2_049))),
            Some(AnimationRefusal::FrameTooLarge)
        );
        // The three a reader is owed a sentence about, and the two they are not.
        assert!(AnimationRefusal::FrameTooLarge.is_worth_saying());
        assert!(AnimationRefusal::FileTooLong.is_worth_saying());
        assert!(AnimationRefusal::Undecodable.is_worth_saying());
        assert!(!AnimationRefusal::OneFrame.is_worth_saying());
        assert!(!AnimationRefusal::NotAnAnimation.is_worth_saying());
        // The ceilings are constants a reader can find and not numbers buried in
        // a comparison.
        assert_eq!(MAX_ANIMATION_RING_BYTES, 32 * 1024 * 1024);
        assert_eq!(MAX_ANIMATION_FRAME_BYTES, 16 * 1024 * 1024);
        // 80 and not 88: the ring, the canvas and the fill in flight are what an
        // animation holds, and the file — which used to be the fourth term — is
        // read rather than held (user report 2026-09-12).
        assert_eq!(MAX_ANIMATION_HELD_BYTES, 80 * 1024 * 1024);
        assert_eq!(MAX_ANIMATION_SIDE_PX, 8192);
        // A sanity bound on a length this window no longer pays for, and it is
        // deliberately far away from anything a screen recorder writes.
        assert_eq!(MAX_ANIMATION_FILE_BYTES, 512 * 1024 * 1024);
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

    /// RED — **an animation's clock starts when a reader can see it**, not when
    /// a worker finished decoding it (adversarial review 2026-09-11, B8).
    ///
    /// RED EVIDENCE, and it is the first half of this test rather than a
    /// quotation: [`open`] stamps `due_at` from `Instant::now()` on the
    /// **decoration worker**, a whole hand-off before the frames reach the
    /// glass. [`Animation::advance`] then walks the ring to catch up on the
    /// first tick, so a completion that landed during a busy turn opened the
    /// file several frames in — which is the user's "a GIF does not start from
    /// its first frame", said by the half of the defect that is in this module.
    ///
    /// The same sentence covers the other half. An animation nobody draws does
    /// not advance (the window's `advance_drawn_animations`), so by the time a
    /// reader comes back to it its due time is a moment in the past; without a
    /// rebase the first tick after a tab switch would fast-forward through
    /// however long the tab was away.
    ///
    /// MUTATION: make `present` a no-op and the second block reads 2 instead of
    /// 0, which is the defect exactly.
    #[test]
    fn an_animations_clock_starts_when_its_first_frame_is_presented() {
        // ① the mechanism, unpresented: half a second between the decode and
        // the tick, and the ring is walked through it.
        let mut late = decode_bytes(fixture()).expect("four frames");
        pump(&mut late);
        late.advance(Instant::now() + Duration::from_millis(500));
        assert_eq!(
            late.frame_index(),
            2,
            "a clock stamped on the worker is already behind when the frames arrive",
        );

        // ② and presented, which is what the window does the frame the picture
        // is actually handed to the renderer.
        let mut animation = decode_bytes(fixture()).expect("four frames");
        pump(&mut animation);
        let presented = Instant::now() + Duration::from_millis(500);
        animation.present(presented);
        assert_eq!(animation.frame_index(), 0, "it starts where the file does");
        animation.advance(presented + Duration::from_millis(99));
        assert_eq!(
            animation.frame_index(),
            0,
            "the first frame stands its own hundred milliseconds, from here",
        );
        animation.advance(presented + Duration::from_millis(100));
        assert_eq!(animation.frame_index(), 1);

        // ③ and being presented again — a pane revealed after ten seconds
        // behind another tab — rebases rather than fast-forwards.
        let revealed = presented + Duration::from_secs(10);
        animation.present(revealed);
        animation.advance(revealed + Duration::from_millis(199));
        assert_eq!(
            animation.frame_index(),
            1,
            "the frame it was hidden on stands its own two hundred milliseconds",
        );
        animation.advance(revealed + Duration::from_millis(200));
        assert_eq!(animation.frame_index(), 2);
    }

    /// RED — **an animation is weighed by all three of its allocations**
    /// (adversarial review 2026-09-11, B9; user report 2026-09-12).
    ///
    /// RED EVIDENCE (2026-09-11), the count before B9:
    ///
    /// ```text
    /// bytes_held() = ring_bytes() + bytes.len()
    /// ```
    ///
    /// Two of four. The **canvas** every frame is composed onto is a whole
    /// logical screen that lives as long as the cursor does — sixteen megabytes
    /// at 2048 square — and the **fill in flight** is up to a whole ring's worth
    /// of frames being composed on a worker. Neither was counted anywhere, so a
    /// window standing exactly at `MAX_ANIMATION_CACHE_BYTES` was a process
    /// holding about twice it.
    ///
    /// **And the fourth term has since gone away**: `bytes.len()` was the file,
    /// and an animation reads its file rather than holding it (user report
    /// 2026-09-12). So this now pins three, and it pins the absence of the
    /// fourth — the count must not grow with the length of the file, because
    /// nothing in this process does.
    ///
    /// MUTATION: drop either remaining term from `bytes_held` and the first or
    /// the second block fails by exactly that allocation.
    #[test]
    fn an_animation_is_weighed_by_its_canvas_and_by_the_fill_in_flight() {
        const SIDE: u16 = 256;
        let frame_bytes = u64::from(SIDE) * u64::from(SIDE) * 4;
        let bytes = a_gif_of(240, SIDE, 5);
        let file_bytes = bytes.len() as u64;
        assert!(file_bytes > 0, "and the file is a real one");
        let mut animation = decode_bytes(bytes).expect("a long small capture");

        // ① the canvas, which is one whole logical screen and outlives any one
        // frame — and the file, which is not here at all.
        assert_eq!(
            animation.bytes_held(),
            animation.ring_bytes() + frame_bytes,
            "the composition canvas is held whether or not a frame is due",
        );

        // ② the fill, reserved before the request is posted rather than charged
        // when it lands.
        let parked = animation.bytes_held();
        let (mut cursor, want) = animation.take_cursor().expect("a ring with room in it");
        assert!(want > 0);
        assert_eq!(
            animation.bytes_held(),
            parked + want as u64 * frame_bytes,
            "frames being composed on the worker are this process's frames",
        );
        assert_eq!(
            animation.bytes_held(),
            animation.ring_bytes() + frame_bytes + want as u64 * frame_bytes,
        );
        // ③ and the peak — cursor away, ring as full as it will be — is under
        // the number the window's own ceiling is a multiple of.
        assert!(
            animation.bytes_held() <= MAX_ANIMATION_HELD_BYTES,
            "at its peak one animation is holding {} of {MAX_ANIMATION_HELD_BYTES}",
            animation.bytes_held(),
        );

        // ④ and the reservation ends where the frames it stood for begin.
        let frames = cursor.next_frames(want);
        animation.park_cursor(cursor, frames);
        assert_eq!(animation.in_flight_bytes, 0);
        assert_eq!(animation.bytes_held(), animation.ring_bytes() + frame_bytes,);
        assert!(animation.bytes_held() <= MAX_ANIMATION_HELD_BYTES);

        // ⑤ the ceiling itself: every one of the three, and no term for a length
        // this process is not holding.
        assert_eq!(
            MAX_ANIMATION_HELD_BYTES,
            MAX_ANIMATION_RING_BYTES * 2 + MAX_ANIMATION_FRAME_BYTES,
        );
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

    /// **A GIF of `frames` small frames whose pixels do not compress**, so that
    /// the file on disk is large while nothing decoded from it is.
    ///
    /// [`a_gif_of`] writes the opposite shape — an enormous decoded frame behind
    /// a tiny file — and that was the right fixture for a ceiling counted in
    /// *pixels*. The cap this one is about was counted in **bytes of file**, so
    /// the fixture has to be a long file, and the honest way to make one is the
    /// way a screen recorder makes one: many frames of picture that does not
    /// deflate. The indices come off a small deterministic generator so the
    /// file is the same file on every machine and every run.
    fn a_noisy_gif(frames: u16, side: u16, delay_hundredths: u16) -> Vec<u8> {
        let palette: Vec<u8> = (0..=255_u8)
            .flat_map(|index| [index, index.wrapping_mul(7), index.wrapping_mul(31)])
            .collect();
        let pixels = side as usize * side as usize;
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        let mut out = Vec::new();
        {
            let mut encoder =
                gif::Encoder::new(&mut out, side, side, &palette).expect("a writer this test owns");
            encoder
                .set_repeat(gif::Repeat::Infinite)
                .expect("and it takes a repeat");
            for _ in 0..frames {
                let indices: Vec<u8> = (0..pixels)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        (seed >> 24) as u8
                    })
                    .collect();
                let mut frame = gif::Frame::from_indexed_pixels(side, side, indices, None);
                frame.delay = delay_hundredths;
                frame.dispose = gif::DisposalMethod::Keep;
                encoder.write_frame(&frame).expect("a frame of noise");
            }
        }
        out
    }

    /// RED — **an animation longer than the old byte cap plays** (user report
    /// 2026-09-12).
    ///
    /// RED EVIDENCE (2026-09-12), before the file was streamed —
    /// `MAX_ANIMATION_FILE_BYTES` was `bt_term::MAX_INLINE_IMAGE_BYTES`, eight
    /// megabytes, and the length was judged before a byte was decoded:
    ///
    /// ```text
    /// an ordinary screen recording opens: Err(TooLarge)
    /// ```
    ///
    /// The reader's own file was an 11.7 MB `simulation.gif`; the fixture here
    /// is the same shape — small frames, a great many of them, and a file well
    /// over the cap that refused it. What is asserted is the whole of the fix:
    ///
    /// * it **opens**, so the length of a file is not a verdict on it any more;
    /// * it **moves**, with a worker filling the ring behind it; and
    /// * what it **holds** is the ring and the canvas and nothing else — the
    ///   file's own length is not in the number, which is the difference
    ///   between streaming a file and keeping it.
    ///
    /// MUTATION: put the file's bytes back into the cursor and the third block
    /// fails by exactly the length of the file.
    #[test]
    fn an_animation_longer_than_the_old_byte_cap_plays() {
        const OLD_CAP: u64 = 8 * 1024 * 1024;
        const SIDE: u16 = 64;
        let bytes = a_noisy_gif(2_400, SIDE, 5);
        assert!(
            bytes.len() as u64 > OLD_CAP,
            "the fixture is over the cap that refused the reader's file: {} bytes",
            bytes.len(),
        );
        let file_bytes = bytes.len() as u64;
        let path = scratch_gif("longer-than-the-old-cap", &bytes);
        drop(bytes);

        // ① it opens, from the file, standing on a ring.
        let mut animation = decode(&path).expect("an ordinary screen recording opens");
        assert!(animation.ring.len() >= 2, "and it opens with a ring");

        // ② it moves, and the ring refills behind it.
        let frame_bytes = u64::from(SIDE) * u64::from(SIDE) * 4;
        let canvas_bytes = frame_bytes;
        let mut now = Instant::now();
        animation.present(now);
        for step in 0..200_u64 {
            pump(&mut animation);
            // ③ and at no point in the walk is the file's length in the number.
            assert!(
                animation.bytes_held() <= MAX_ANIMATION_RING_BYTES + canvas_bytes,
                "at step {step} it holds {} bytes of a {file_bytes}-byte file",
                animation.bytes_held(),
            );
            assert!(
                animation.bytes_held() < file_bytes,
                "the file is being read, not held: {} of {file_bytes}",
                animation.bytes_held(),
            );
            now += Duration::from_millis(50);
            animation.advance(now);
        }
        assert_eq!(
            animation.frame_index(),
            200,
            "two hundred frames of the file, one every fifty milliseconds",
        );
        let _ = std::fs::remove_file(&path);
    }

    /// RED — **the loop comes round by seeking the file, not by holding it**
    /// (user report 2026-09-12).
    ///
    /// The old cursor kept an `Arc<[u8]>` of the whole file for exactly one
    /// reason — GIF has no seek, so playing the loop a second time meant opening
    /// the bytes again — and that one reason is what made a cap on the file's
    /// *length* necessary, which is what refused an 11.7 MB recording. A handle
    /// answers the same question: the file is still open, so coming round is a
    /// seek to nought and a new decoder over the same handle.
    ///
    /// Two halves, and both are needed. The **behaviour**: a file-backed
    /// animation walked past its own last frame is standing on frame zero again,
    /// with its delays intact. The **structure**: the cursor holds no copy of
    /// the file, and it is the source that is wound back.
    ///
    /// MUTATION: make `restart` a no-op and the second turn reads the frames
    /// after the end of the file, which is no frames at all — the walk stops.
    #[test]
    fn the_loop_comes_round_by_seeking_the_file_not_by_holding_it() {
        const FRAMES: u64 = 40;
        let path = scratch_gif("the-loop-seeks", &a_noisy_gif(FRAMES as u16, 32, 5));
        let mut animation = decode(&path).expect("a file-backed animation opens");
        let mut now = Instant::now();
        animation.present(now);
        let mut seen = Vec::new();
        for _ in 0..(2 * FRAMES + 1) {
            pump(&mut animation);
            seen.push(animation.frame_index());
            now += Duration::from_millis(50);
            animation.advance(now);
        }
        assert_eq!(animation.frames_in_file(), Some(FRAMES));
        let walk: Vec<u64> = (0..FRAMES).collect();
        assert_eq!(&seen[..FRAMES as usize], &walk[..], "the first turn");
        assert_eq!(
            &seen[FRAMES as usize..2 * FRAMES as usize],
            &walk[..],
            "and the second, which begins at frame zero again",
        );

        // And the structure: the way back to frame zero is the source winding
        // itself back, not a second decoder over bytes this window is carrying.
        const SOURCE: &str = include_str!("animation.rs");
        let at = SOURCE
            .find("    fn start_again(&mut self) -> bool {")
            .expect("the loop's own function is in this file");
        let rest = &SOURCE[at..];
        let again = &rest[..rest.find("\n    }\n").expect("and it ends") + 6];
        assert!(
            again.contains("restart()"),
            "coming round is a seek on the source:\n{again}",
        );
        assert!(
            !again.contains("Arc::clone(&self.bytes)"),
            "and not a second reader over a held copy of the file:\n{again}",
        );
        let at = SOURCE
            .find("pub struct AnimationCursor {")
            .expect("the cursor is declared in this file");
        let rest = &SOURCE[at..];
        let declared = &rest[..rest.find("\n}\n").expect("and it ends") + 3];
        assert!(
            !declared.contains("Arc<[u8]>"),
            "the cursor holds no copy of the file:\n{declared}",
        );
        let _ = std::fs::remove_file(&path);
    }

    /// RED — **a file rewritten under the loop ends its playback** (user report
    /// 2026-09-12, the cost of not holding the file).
    ///
    /// Holding the bytes made this question go away by making the animation a
    /// picture of a file that no longer existed; a handle makes it real, because
    /// a file being written to under an open handle is a file whose frame two
    /// hundred may belong to a different recording than its frame one. So the
    /// cursor keeps the stamp it opened with — when the file was last written
    /// and how long it was, the identity `bt_term`'s decode memo already keys on
    /// (§7.1.3k ⑧) — and compares it at the one moment a change can be acted on
    /// instead of decoded into the middle of a picture: the moment the loop
    /// comes round.
    ///
    /// The window's half of this is `adopt_animation_fill`, which lets the
    /// playback go so the pane opens the file again — see the test of that name
    /// in `main.rs`.
    ///
    /// MUTATION: drop the comparison and the second block reads `false`, which
    /// is a decoder reading the first half of one file and the second half of
    /// another.
    #[test]
    fn a_file_rewritten_under_the_loop_ends_the_playback_and_is_reopened() {
        const FRAMES: u16 = 40;
        let path = scratch_gif("rewritten-under-the-loop", &a_noisy_gif(FRAMES, 32, 5));

        // ① the control: a file nobody touches comes round as many times as it
        // is asked to, and says nothing about having changed.
        let (mut cursor, _, _) = open(file_source(&path).expect("the file opens")).expect("a GIF");
        let frames = cursor.next_frames(3 * FRAMES as usize);
        assert_eq!(frames.len(), 3 * FRAMES as usize, "three whole turns");
        assert!(!cursor.file_changed(), "nothing happened to the file");
        drop(cursor);

        // ② and a file rewritten while the loop is inside it ends there.
        let (mut cursor, _, _) = open(file_source(&path).expect("the file opens")).expect("a GIF");
        let head = cursor.next_frames(FRAMES as usize / 2);
        assert_eq!(head.len(), FRAMES as usize / 2, "half a turn");
        assert!(!cursor.file_changed(), "and it is the file it opened");
        // A different recording, under the same name and of a different length
        // — which is what an export written again, or a `mv` over it, is.
        std::fs::write(&path, a_noisy_gif(FRAMES + 7, 32, 5)).expect("the file is written again");
        let rest = cursor.next_frames(3 * FRAMES as usize);
        assert!(
            cursor.file_changed(),
            "the loop came round onto a file that is not the one it opened",
        );
        assert!(
            rest.len() < 3 * FRAMES as usize,
            "and it stopped there rather than decoding a stranger: {} frames",
            rest.len(),
        );
        let _ = std::fs::remove_file(&path);
    }

    /// RED — **a refused animation's first frame is decoded by the GIF
    /// decoder** (user report 2026-09-12).
    ///
    /// RED EVIDENCE (2026-09-12), the second sentence on the reader's empty
    /// pane, under a foot that had already declined to play the file:
    ///
    /// ```text
    /// Preview failed: inline image exceeds its decode limit
    /// ```
    ///
    /// The still that stands in for an animation this window will not play came
    /// from the picture decoder, which reads a file whole behind
    /// `bt_term::MAX_INLINE_IMAGE_BYTES` — eight megabytes, the allowance for a
    /// picture a shell pastes into a scrollback. So the one file that most needs
    /// a still is the one file that cannot have one. [`first_frame`] pulls
    /// exactly one frame through the same streaming cursor under the same
    /// [`MAX_ANIMATION_FRAME_BYTES`], and the length of the file is nothing to
    /// it.
    ///
    /// The four answers are here: a `.gif` this lane **refuses** still has a
    /// picture; a `.gif` past the picture lane's own file cap has one **from
    /// here** and provably not from there; a frame past this lane's ceiling
    /// answers `None`, so the picture decoder decides it; and so does a name
    /// that is not `.gif` at all. Those last two are the fork and not a
    /// fallback: the name said GIF and only the bytes could say otherwise.
    ///
    /// MUTATION: send the still back down the picture lane and the second block
    /// is the reader's empty pane again, word for word.
    #[test]
    fn a_refused_animations_first_frame_is_decoded_by_the_gif_decoder() {
        const OLD_CAP: u64 = 8 * 1024 * 1024;
        const SIDE: u16 = 64;

        // ① a `.gif` with one frame in it is refused as an animation — one frame
        // is not a thing that moves — and it still has a picture, from here.
        let path = scratch_gif("one-frame", &a_gif_of(1, 16, 10));
        assert_eq!(decode(&path).err(), Some(AnimationRefusal::OneFrame));
        let still = first_frame(&path).expect("a refused animation still has a first frame");
        assert_eq!((still.width_px, still.height_px), (16, 16));
        assert_eq!(
            still.rgba.len() as u64,
            u64::from(still.width_px) * u64::from(still.height_px) * 4,
            "one whole frame of pixels",
        );
        // RGBA and not the ring's BGRA, because these go to the picture lane.
        assert_eq!(
            still.rgba[3], 0xFF,
            "opaque, as a GIF without a transparent index is"
        );
        // The key names the file and the stamp it had, because the bytes it was
        // composed from are a container this process never held whole.
        assert!(still.key.starts_with("gif-frame:"), "{}", still.key);
        assert!(still.key.ends_with(":16x16"), "{}", still.key);
        let _ = std::fs::remove_file(&path);

        // ② and here is the lane the still used to come from, refusing the
        // reader's own kind of file — which is the sentence off their pane.
        let path = scratch_gif("long-and-playing", &a_noisy_gif(2_400, SIDE, 5));
        let length = std::fs::metadata(&path).expect("it is on disk").len();
        assert!(
            length > OLD_CAP,
            "the fixture is over the cap that refused the reader's file: {length}",
        );
        assert_eq!(
            bt_term::InlineImageDecoder::default()
                .decode(bt_term::InlineImageTask {
                    occurrence_id: 0,
                    source: bt_term::InlineImageSource::LocalPath(path.clone()),
                })
                .err(),
            Some(bt_term::InlineImageDecodeError::TooLarge),
            "the picture decoder reads a file whole behind eight megabytes",
        );
        assert!(decode(&path).is_ok(), "and this lane plays it");
        let still = first_frame(&path).expect("and draws its first frame");
        assert_eq!(
            (still.width_px, still.height_px),
            (u32::from(SIDE), u32::from(SIDE))
        );
        let _ = std::fs::remove_file(&path);

        // ③ a frame past this lane's own ceiling is not this lane's to draw
        // either, so the picture decoder is asked — `None`, not an empty
        // picture.
        let path = scratch_gif("frame-past-the-ceiling", &a_gif_declaring(2_048, 2_049));
        assert_eq!(decode(&path).err(), Some(AnimationRefusal::FrameTooLarge));
        assert!(
            first_frame(&path).is_none(),
            "the picture lane answers this"
        );
        let _ = std::fs::remove_file(&path);

        // ④ and neither is a file that is not named like one.
        assert!(first_frame(std::path::Path::new(r"D:\shots\a.png")).is_none());
    }
}
