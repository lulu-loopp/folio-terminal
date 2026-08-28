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
//! of an animation, which is the whole reason this module is thirty lines of
//! decoding and not a second upload path:
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
//! # The frames are the file's, and so are the delays
//!
//! `image`'s `AnimationDecoder` hands back every frame already composed against
//! the ones before it — disposal methods, transparency, sub-rectangles and all —
//! so what is kept here is a list of finished pictures and the time each is due
//! for. **The delays are read and never assumed**: a GIF may declare a different
//! delay on every frame, and a build that advanced one frame per redraw, or one
//! every hundred milliseconds, is a build that plays every animation at the
//! wrong speed and most of them at a speed that changes with the window's load.
//!
//! # And a bound, because a GIF may be enormous
//!
//! Frames are held decoded, so the cost is `frames × width × height × 4` and a
//! reader can hover something pathological — a thousand-frame screen capture at
//! 1080p is eight gigabytes. [`MAX_ANIMATION_RGBA_BYTES`] is the ceiling; over
//! it the file is shown as its **first frame, still**, which is exactly what
//! this window did for every animation until today and is a good deal better
//! than a decode that takes the process with it.

use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, ImageFormat, ImageReader};

/// **How many bytes of decoded frames one animation may hold.**
///
/// 256 MiB, which is `MAX_INLINE_IMAGE_RGBA_BYTES`'s reasoning at the scale an
/// animation works at: a picture nobody asked for gets 64 MiB because a
/// screenful of them costs that each, and an animation is *one* object a reader
/// pointed at — but it is one object made of hundreds of pictures, so the
/// allowance is per animation rather than per frame and is four times a single
/// picture's rather than four hundred.
///
/// At 1080p that is about thirty frames; at a `loading.gif`'s usual 64×64 it is
/// sixteen thousand. Both of those are the right answer: the first is a screen
/// capture somebody would rather scrub than watch in a hover card, and the
/// second is every spinner ever made.
pub const MAX_ANIMATION_RGBA_BYTES: u64 = 256 * 1024 * 1024;

/// **The shortest delay a frame is honoured at.**
///
/// A GIF may declare zero, and historically many do — the convention the
/// browsers settled on is that zero and one hundredth of a second both mean
/// "as fast as is reasonable", which every one of them reads as a tenth. This
/// window reads it the same way for the same reason: a zero-delay frame list is
/// not a request to spin a CPU, and honouring it literally would make one
/// animation cost more than every other thing on the glass together.
pub const MIN_FRAME_DELAY: Duration = Duration::from_millis(20);

/// The delay a frame that declares nothing at all is given — the browsers'
/// hundredth-of-a-second reading, which is [`MIN_FRAME_DELAY`]'s own case.
pub const DEFAULT_FRAME_DELAY: Duration = Duration::from_millis(100);

/// **One composed frame of an animation, in the byte order the layer wants.**
///
/// BGRA and not RGBA, and it is converted here rather than at upload time for
/// the reason the whole module exists: this happens once per frame of the file,
/// on a worker, and the alternative happens once per frame of the *window*, on
/// the thread that draws.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnimationFrame {
    pub bgra: Arc<[u8]>,
    /// How long this frame stands before the next is due.
    pub delay: Duration,
}

/// **Every frame of one animated file, and how long the whole loop takes.**
///
/// Held per *file* and not per surface, which is what makes three surfaces
/// showing one `loading.gif` show the same picture at the same instant rather
/// than three spinners at three phases.
#[derive(Clone, Debug)]
pub struct Animation {
    frames: Vec<AnimationFrame>,
    width_px: u32,
    height_px: u32,
    /// The sum of every delay — one turn of the loop.
    loop_length: Duration,
    /// When this animation's clock started. Set when the frames arrive, so a
    /// GIF begins at its own first frame however long the file took to decode.
    started: Instant,
    /// Which frame was last handed out, so a redraw that lands inside the same
    /// frame's own delay is one comparison and no upload.
    standing: usize,
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
    /// `frames × width × height × 4` is over [`MAX_ANIMATION_RGBA_BYTES`]. The
    /// surface draws the first frame and does not move — see the module note.
    TooLarge,
}

/// **Read every frame of `path`, or say why not.**
///
/// # Where it may be called from
///
/// **A worker, never the thread that draws.** Decoding a hundred frames is tens
/// of milliseconds and this window has one thread that must not spend them.
pub fn decode(path: &std::path::Path) -> Result<Animation, AnimationRefusal> {
    if !path_names_an_animation(path) {
        return Err(AnimationRefusal::NotAnAnimation);
    }
    let bytes = std::fs::read(path).map_err(|_| AnimationRefusal::Undecodable)?;
    decode_bytes(&bytes)
}

/// The same, from bytes already in hand — the half a test can hold.
pub fn decode_bytes(bytes: &[u8]) -> Result<Animation, AnimationRefusal> {
    // The container is judged by its own header and not by the name that led
    // here, which is the discipline `decode_image_bytes_within` already keeps: a
    // `.gif` that is a JPEG is a JPEG.
    let format = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.format());
    if format != Some(ImageFormat::Gif) {
        return Err(AnimationRefusal::NotAnAnimation);
    }
    let decoder = GifDecoder::new(Cursor::new(bytes)).map_err(|_| AnimationRefusal::Undecodable)?;
    let mut frames = Vec::new();
    let mut width_px = 0_u32;
    let mut height_px = 0_u32;
    let mut bytes_held = 0_u64;
    for frame in decoder.into_frames() {
        let frame = frame.map_err(|_| AnimationRefusal::Undecodable)?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let delay = if denominator == 0 {
            DEFAULT_FRAME_DELAY
        } else {
            Duration::from_micros(u64::from(numerator) * 1_000 / u64::from(denominator))
        }
        .max(MIN_FRAME_DELAY);
        let buffer = frame.into_buffer();
        let (this_width, this_height) = buffer.dimensions();
        if this_width == 0 || this_height == 0 {
            return Err(AnimationRefusal::Undecodable);
        }
        // Every frame of a GIF is composed to the logical screen by the decoder,
        // so they are all one size; a file that says otherwise is one this
        // window has no rectangle for.
        if width_px == 0 {
            (width_px, height_px) = (this_width, this_height);
        } else if (this_width, this_height) != (width_px, height_px) {
            return Err(AnimationRefusal::Undecodable);
        }
        let mut raw = buffer.into_raw();
        // RGBA to BGRA, in place: the layer's texture is created in the
        // swapchain's own order (§7.42 ②) and this is the one place in the
        // animation's life where a pixel is touched by this process.
        for pixel in raw.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        bytes_held += raw.len() as u64;
        if bytes_held > MAX_ANIMATION_RGBA_BYTES {
            return Err(AnimationRefusal::TooLarge);
        }
        frames.push(AnimationFrame {
            bgra: Arc::from(raw),
            delay,
        });
    }
    match frames.len() {
        0 => Err(AnimationRefusal::Undecodable),
        1 => Err(AnimationRefusal::OneFrame),
        _ => Ok(Animation::of(frames, width_px, height_px, Instant::now())),
    }
}

impl Animation {
    /// The pure constructor — what a test builds without a file.
    #[must_use]
    pub fn of(
        frames: Vec<AnimationFrame>,
        width_px: u32,
        height_px: u32,
        started: Instant,
    ) -> Self {
        let loop_length = frames.iter().map(|frame| frame.delay).sum();
        Self {
            frames,
            width_px,
            height_px,
            loop_length,
            started,
            standing: 0,
            generation: 1,
        }
    }

    /// **Which frame is due at `now`** — the file's own delays, accumulated, and
    /// the whole thing wrapped.
    ///
    /// A *sampled* answer and not a counter something advances, on
    /// [`crate::termscroll::visibility`]'s discipline: a window that missed
    /// three frames because it was busy resumes at the frame that is due now
    /// rather than three behind, and two surfaces showing one file are at the
    /// same frame because they asked the same question.
    #[must_use]
    pub fn frame_at(&self, now: Instant) -> usize {
        if self.loop_length.is_zero() {
            return 0;
        }
        let elapsed = now.saturating_duration_since(self.started);
        let mut into = Duration::from_nanos(
            u64::try_from(elapsed.as_nanos() % self.loop_length.as_nanos()).unwrap_or(0),
        );
        for (index, frame) in self.frames.iter().enumerate() {
            if into < frame.delay {
                return index;
            }
            into -= frame.delay;
        }
        // Unreachable by construction — the remainder is inside the sum — and
        // answered rather than asserted, because a rounding that put it one
        // nanosecond past the end is not a reason to bring down a terminal.
        self.frames.len() - 1
    }

    /// **Move to the frame that is due, and say whether that is a new one.**
    ///
    /// `true` is what owes the window a redraw, and it is false for every tick
    /// that lands inside the standing frame's own delay — which for a
    /// hundred-millisecond frame at sixty hertz is five ticks out of six.
    pub fn advance(&mut self, now: Instant) -> bool {
        let due = self.frame_at(now);
        if due == self.standing {
            return false;
        }
        self.standing = due;
        self.generation += 1;
        true
    }

    /// The standing frame, as the renderer's upload.
    #[must_use]
    pub fn upload(&self) -> bt_render::VideoFrameUpload {
        let frame = &self.frames[self.standing];
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
                .join("../../test-assets/folio-anim-test.gif"),
        )
        .expect("the animation fixture is in test-assets")
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
    /// RED GATE: return `index` from `frame_at` by dividing the elapsed time by
    /// a constant — which is what "animate a GIF" looks like when it is written
    /// without reading the file — and every assertion after the first fails.
    #[test]
    fn a_gif_advances_by_its_own_frame_delays() {
        let animation = decode_bytes(&fixture()).expect("four frames");
        assert_eq!(animation.frames.len(), 4);
        assert_eq!((animation.width_px, animation.height_px), (64, 64));
        // ① the delays are the file's, to the millisecond.
        let delays: Vec<u64> = animation
            .frames
            .iter()
            .map(|frame| frame.delay.as_millis() as u64)
            .collect();
        assert_eq!(delays, [100, 200, 300, 400], "the file's own delays");
        assert_eq!(animation.loop_length, Duration::from_millis(1_000));

        // ② the frame due at every boundary, on both sides of it.
        let start = animation.started;
        let at = |ms: u64| animation.frame_at(start + Duration::from_millis(ms));
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
            assert_eq!(at(ms), expected, "at {ms}ms");
        }

        // ④ the four frames are four different pictures, so "it moved" is a
        // thing a reader can see and not only a number that changed.
        let colours: std::collections::BTreeSet<[u8; 4]> = animation
            .frames
            .iter()
            .map(|frame| [frame.bgra[0], frame.bgra[1], frame.bgra[2], frame.bgra[3]])
            .collect();
        assert_eq!(colours.len(), 4, "four frames, four colours: {colours:?}");
        // And they arrived in the swapchain's byte order: the first frame is
        // `0xE04B2F` written blue-first.
        assert_eq!(&animation.frames[0].bgra[..4], &[0x2F, 0x4B, 0xE0, 0xFF]);
    }

    /// RED — **a redraw inside the standing frame's own delay uploads nothing**
    /// (§7.44 ⑤, on §7.42 ⑥'s gate).
    ///
    /// The generation is the renderer's upload gate, and it counts *changes of
    /// frame*. A build that bumped it per tick would spend a megabyte of bus per
    /// redraw writing the pixels that are already there — which is the exact
    /// cost `VideoFrameUpload::generation` exists to refuse.
    #[test]
    fn an_animation_that_has_not_changed_frame_uploads_nothing() {
        let mut animation = decode_bytes(&fixture()).expect("four frames");
        let start = animation.started;
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
        assert_eq!(animation.frame_at(start + Duration::from_millis(650)), 3);
    }

    /// PIN — **the four ways this module declines, and none of them is a
    /// panic.**
    ///
    /// A `.png` is not this lane's; bytes that are not a container are not
    /// either; a single-frame GIF is a picture the picture channel already
    /// draws; and a file over the ceiling is drawn as its first frame and left
    /// still. Every one of those is a `.gif` a reader can hover.
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
        assert_eq!(
            refusal(decode_bytes(b"GIF89a but not really")),
            Some(AnimationRefusal::NotAnAnimation)
        );
        assert_eq!(
            refusal(decode_bytes(&[])),
            Some(AnimationRefusal::NotAnAnimation)
        );
        // The ceiling is a constant a reader can find and not a number buried in
        // a comparison.
        assert_eq!(MAX_ANIMATION_RGBA_BYTES, 256 * 1024 * 1024);
    }

    /// PIN — **a declared delay of nothing is a tenth of a second, and a very
    /// short one is twenty milliseconds.**
    ///
    /// The reading every browser settled on, and this window reads it the same
    /// way for the same reason: honouring a zero literally is a request to spin
    /// a core, and an animation is not entitled to more of one than everything
    /// else on the glass together.
    #[test]
    fn a_frame_that_declares_no_time_is_given_the_browsers_reading() {
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
        assert_eq!(animation.loop_length, MIN_FRAME_DELAY + DEFAULT_FRAME_DELAY);
        assert_eq!(DEFAULT_FRAME_DELAY, Duration::from_millis(100));
        assert_eq!(MIN_FRAME_DELAY, Duration::from_millis(20));
    }
}
