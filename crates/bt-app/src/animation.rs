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
//! # And three bounds, because a GIF may be enormous
//!
//! Frames are held decoded, so the cost is `frames × width × height × 4` and a
//! reader can hover something pathological — a thousand-frame screen capture at
//! 1080p is eight gigabytes. [`MAX_ANIMATION_RGBA_BYTES`] is the ceiling; over
//! it the file is shown as its **first frame, still**, which is exactly what
//! this window did for every animation until today and is a good deal better
//! than a decode that takes the process with it.
//!
//! That ceiling counts what is *kept*, and on its own it was not enough (review
//! row R1-7, adversarial review 2026-09-08): a file is read before it is
//! decoded and a frame is allocated before it is counted, so a hover could
//! spend both without ever reaching the total. The other two bounds stand in
//! front of it. [`MAX_ANIMATION_FILE_BYTES`] is how much of the file is read at
//! all, and [`MAX_ANIMATION_SIDE_PX`] goes onto the decoder as an `image`
//! `Limits` before the first frame is pulled, so a header declaring a
//! 65535-square logical screen is refused by the descriptor rather than by the
//! seventeen gigabytes it asked for.

use std::io::{Cursor, Read};
use std::sync::Arc;
use std::time::{Duration, Instant};

use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, ImageDecoder, ImageFormat, ImageReader, Limits};

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
/// draw. Refusing it at the decoder rather than at the upload is the difference
/// between a picture that does not move and a gigabyte allocated to find that
/// out.
pub const MAX_ANIMATION_SIDE_PX: u32 = 8192;

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
    let mut decoder =
        GifDecoder::new(Cursor::new(bytes)).map_err(|_| AnimationRefusal::Undecodable)?;
    // **The limits go on before a frame is pulled** (review row R1-7,
    // adversarial review 2026-09-08). `GifDecoder::new` opens every file with
    // `Limits::no_limits()`, and the frame iterator's first act is to allocate
    // the whole *declared* logical screen — so the total counted below, which is
    // the only ceiling this module used to have, was a refusal to keep pixels
    // that had already been made. `set_limits` reads the logical screen
    // descriptor and fails here, with no buffer anywhere.
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_ANIMATION_SIDE_PX);
    limits.max_image_height = Some(MAX_ANIMATION_SIDE_PX);
    limits.max_alloc = Some(MAX_ANIMATION_RGBA_BYTES);
    decoder
        .set_limits(limits)
        .map_err(|_| AnimationRefusal::TooLarge)?;
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
    /// **How many bytes of decoded frames this animation is holding** — what the
    /// window's own ceiling over every animation at once is counted against.
    #[must_use]
    pub fn bytes_held(&self) -> u64 {
        self.frames
            .iter()
            .map(|frame| frame.bgra.len() as u64)
            .sum()
    }

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
                .join("../../tests/assets/folio-anim-test.gif"),
        )
        .expect("the animation fixture is in tests/assets")
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
    /// `GifDecoder::new` opens every GIF with `Limits::no_limits()`, and the
    /// frame iterator's first act is to allocate the whole **declared** logical
    /// screen and compose the file's frames into it. The refusal that existed —
    /// the running total against [`MAX_ANIMATION_RGBA_BYTES`] — is counted after
    /// that allocation, so it was a refusal to *keep* the pixels rather than a
    /// refusal to make them: nine bytes of picture behind a 65535-square header
    /// asked the allocator for seventeen gigabytes, on a hover, with no click
    /// anywhere.
    ///
    /// **The structural half is the load-bearing one and it stands first**,
    /// because the two readings agree about the verdict and differ only in what
    /// they spend reaching it: both answer [`AnimationRefusal::TooLarge`], and
    /// only one of them allocates seventeen gigabytes on the way. `set_limits`
    /// is where the difference lives — it checks the logical screen descriptor
    /// against [`MAX_ANIMATION_SIDE_PX`] and fails there, with the frame
    /// iterator not yet built — so the call is what is asserted on, and it is
    /// asserted on *before* the fixtures are decoded. That order is also the
    /// safety: the tree that produced the evidence above went on to ask for the
    /// seventeen gigabytes, and one that drops the call today is caught by the
    /// first assertion instead.
    ///
    /// MUTATION: drop the `.set_limits(` call and the first assertion goes red;
    /// move it after `into_frames()` and it goes red the same way.
    #[test]
    fn a_declared_screen_this_window_will_not_hold_is_refused_before_it_is_allocated() {
        const SOURCE: &str = include_str!("animation.rs");
        let at = SOURCE
            .find("\npub fn decode_bytes(")
            .expect("the container walk is a free function in this file");
        let rest = &SOURCE[at..];
        let walk = &rest[..rest.find("\n}\n").expect("and it ends") + 3];
        let limits = walk
            .find(".set_limits(")
            .expect("the decoder is opened under limits, before a frame is pulled");
        let frames = walk
            .find("into_frames()")
            .expect("and the frames are pulled from it");
        assert!(
            limits < frames,
            "the limits are set before the first frame is asked for:\n{walk}",
        );

        // And the verdict, at the size the review named and at one a machine can
        // survive being wrong about.
        for side in [16_384_u16, 65_535] {
            assert_eq!(
                decode_bytes(&a_gif_declaring(side, side)).err(),
                Some(AnimationRefusal::TooLarge),
                "a {side}-square logical screen is over this window's ceiling",
            );
        }
        // A screen this window can hold is untouched: the fixture is 64 square.
        assert!(decode_bytes(&fixture()).is_ok());
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
    ///     decode_bytes(&bytes)
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
    /// A `.png` is not this lane's; bytes that announce no container at all are
    /// not either; bytes that announce a GIF and then will not open are a broken
    /// GIF rather than a stranger; a single-frame GIF is a picture the picture
    /// channel already draws; and a file over the ceiling is drawn as its first
    /// frame and left still. Every one of those is a `.gif` a reader can hover.
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
                b"GIF89a\x10\x00\x10\x00\x00\x00\x00 but not really"
            )),
            Some(AnimationRefusal::Undecodable)
        );
        assert_eq!(
            refusal(decode_bytes(&[])),
            Some(AnimationRefusal::NotAnAnimation)
        );
        // And a header whose screen this window could never draw is refused for
        // that, before anything behind it is read.
        assert_eq!(
            refusal(decode_bytes(&a_gif_declaring(20_000, 4))),
            Some(AnimationRefusal::TooLarge)
        );
        // The ceilings are constants a reader can find and not numbers buried in
        // a comparison.
        assert_eq!(MAX_ANIMATION_RGBA_BYTES, 256 * 1024 * 1024);
        assert_eq!(MAX_ANIMATION_SIDE_PX, 8192);
        assert_eq!(MAX_ANIMATION_FILE_BYTES, 8 * 1024 * 1024);
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
