//! **The first frame, asked of whichever decoder this machine has** (M4-4;
//! `docs/DESIGN.md` §13.25).
//!
//! Every other test of this lane lives inside one arm and can only be true
//! there: `video/mod.rs`'s cases are Media Foundation's and never compile on a
//! Mac, and `macos_video.rs` is AVFoundation's and never compiles here. **This
//! file is the one that is the same on both**, which is the whole claim M4-4
//! makes — that a `VideoFrame` out of AVFoundation and a `VideoFrame` out of
//! Media Foundation are the same bytes in the same order — and the only way to
//! state a claim about two arms is to write it once and run it twice.
//!
//! It is an integration test rather than a unit one for the same reason: it sees
//! exactly what `bt-app` sees, which is the public surface and nothing else.
//!
//! # The two recordings, and what is known about them before anything decodes
//!
//! `tests/assets/PROVENANCE.md` records how the five fixtures were made and what
//! is in them: 160×120, a fifth of a second of black and then one solid colour,
//! the `.mp4` five seconds long and the rest three. The two this file uses are
//! the two §7.23 shipped — the `.mp4` in **orange `0xE07A2F`** and the `.mov` in
//! **blue `0x2F7AE0`** — and they are the pair that makes this a colour test
//! rather than a "some pixels are not black" test: a lane that swapped red and
//! blue on the way out would answer the orange file with a blue frame and pass
//! every assertion about size, stride and opacity.
//!
//! The black opening is the other half of the fixture, and it is what
//! `video::SEEK_FRACTION` exists for: a tenth of the way into either file is
//! past it, and a lane that took frame zero would come back with a black
//! rectangle and fail the colour assertion here while passing everything else.
//!
//! # Why the colour is asserted with a tolerance and the numbers are printed
//!
//! Neither arm is asked to reproduce `0xE07A2F` exactly and neither can: the
//! fixtures are `yuv420p` H.264, so the colour has already been through a
//! subsampled, quantised round trip before any decoder sees it, and the two
//! platforms then convert to RGB by roads of their own — Media Foundation's
//! video processor hands back untagged RGB, Core Graphics colour-matches the
//! tagged frame into sRGB. [`TOLERANCE`] is set from what the two machines
//! actually measured rather than from a theory, and every case prints its mean
//! so that the next person to change either arm can see the number move before
//! the assertion catches it.

use std::path::{Path, PathBuf};

use bt_platform::video::{VideoFrame, decode_first_frame_measured, first_frame};

/// The hover card's own box, which is the size the product asks for.
const CARD: (u32, u32) = (280, 160);

/// Both recordings are this, and it is the pair the fact line prints.
const NATIVE: (u32, u32) = (160, 120);

/// **How far a decoded channel may sit from the colour the fixture was
/// authored in**, out of 255.
///
/// Not a theory and not a shrug. Measured on 2026-09-12, the four means are in
/// `docs/DESIGN.md` §13.25: Media Foundation lands within **1** of the authored
/// colour on both files, and Core Graphics lands within **20**, because it
/// colour-matches a frame the other arm hands over untagged. Thirty-two is that
/// twenty with room for a macOS release whose matching moves a little, and it is
/// still nowhere near the defects this assertion is for — a red and blue swapped
/// on the way out is ~180 off, and a frame taken at time zero instead of a tenth
/// in is the whole distance to black.
const TOLERANCE: i32 = 32;

/// Whether this machine has a video decoder wired up at all. A third platform
/// answers `None` to everything here, and that is the assertion rather than a
/// skip — a build whose refusal quietly became a panic is the failure this
/// catches.
const HAS_DECODER: bool = cfg!(any(windows, target_os = "macos"));

/// One of the shipped recordings, found the way the in-crate cases find them.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/assets")
        .join(name)
}

/// The mean of a frame's three colour channels, which for these fixtures is the
/// one colour in them.
fn mean_colour(frame: &VideoFrame) -> (i32, i32, i32) {
    let pixels = frame.rgba.len() / 4;
    assert!(pixels > 0, "a frame with no pixels in it");
    let (mut red, mut green, mut blue) = (0_u64, 0_u64, 0_u64);
    for pixel in frame.rgba.chunks_exact(4) {
        red += u64::from(pixel[0]);
        green += u64::from(pixel[1]);
        blue += u64::from(pixel[2]);
    }
    let mean = |total: u64| (total / pixels as u64) as i32;
    (mean(red), mean(green), mean(blue))
}

/// RED — **both shipped recordings give up a frame, at the size, the length and
/// the colour they were made with.**
///
/// The one assertion in this lane that a decoder cannot satisfy by agreeing with
/// synthetic bytes: two real H.264 files in two containers, opened through
/// whichever real platform this machine has, wound forward and decoded. It is
/// what says the whole chain is wired — the asset, the track, the cap, the time,
/// the decode and the draw — because every one of those failing answers `None`,
/// and this asserts a picture of a named colour.
///
/// RED GATE: set `SEEK_FRACTION` to `0.0` on either arm and the colour
/// assertions go red while the size and length assertions stay green, which is
/// exactly the defect that would otherwise ship as "all my clips are black".
/// Swap `out[0]` and `out[2]` in either arm's copy and the orange file answers
/// blue.
#[test]
fn both_shipped_recordings_give_up_a_frame_of_the_colour_they_were_made_in() {
    for (name, colour, length_ms) in [
        ("folio-video-test.mp4", (0xE0, 0x7A, 0x2F), 4_800..=5_200),
        ("folio-video-test.mov", (0x2F, 0x7A, 0xE0), 2_800..=3_200),
    ] {
        let file = fixture(name);
        let (frame, cost) = decode_first_frame_measured(&file, CARD.0, CARD.1);
        if !HAS_DECODER {
            assert_eq!(
                frame, None,
                "{name}: a machine with no decoder draws nothing"
            );
            continue;
        }
        let frame = frame.unwrap_or_else(|| panic!("{name}: a real recording gives up a frame"));
        let mean = mean_colour(&frame);
        eprintln!(
            "VIDEO_FIRST_FRAME {name} raster={}x{} native={}x{} duration={:?}ms mean_rgb=({},{},{}) \
             total={:?} session={:?} open={:?} output_type={:?} seek={:?} read_sample={:?} \
             copy={:?}",
            frame.width,
            frame.height,
            frame.native_width,
            frame.native_height,
            frame.duration_ms,
            mean.0,
            mean.1,
            mean.2,
            cost.total(),
            cost.session,
            cost.open,
            cost.output_type,
            cost.seek,
            cost.read_sample,
            cost.copy,
        );

        assert_eq!(
            (frame.native_width, frame.native_height),
            NATIVE,
            "{name}: the native size is the video's own"
        );
        assert!(
            frame.width <= CARD.0 && frame.height <= CARD.1,
            "{name}: fitted inside the box it was asked for: {}x{}",
            frame.width,
            frame.height
        );
        assert_eq!(
            frame.rgba.len(),
            frame.width as usize * frame.height as usize * 4,
            "{name}: straight RGBA8, packed, one row after another"
        );
        assert!(
            frame.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255),
            "{name}: a frame of video is opaque"
        );
        for (channel, measured, wanted) in [
            ("red", mean.0, colour.0),
            ("green", mean.1, colour.1),
            ("blue", mean.2, colour.2),
        ] {
            assert!(
                (measured - wanted).abs() <= TOLERANCE,
                "{name}: {channel} came back {measured}, and the recording was made at {wanted} \
                 (mean {mean:?})"
            );
        }
        let duration_ms = frame
            .duration_ms
            .unwrap_or_else(|| panic!("{name}: the container declares a length"));
        assert!(
            length_ms.contains(&duration_ms),
            "{name}: {duration_ms}ms is not the length this fixture was made at"
        );
    }
}

/// RED — **a frame asked for smaller comes back smaller with its proportions
/// kept.**
///
/// The card's own box is larger than these recordings, so the case above never
/// exercises the fit at all — it asks for 280×160 and gets the file's own
/// 160×120 back, which is the *clamp* rather than the scale. This asks for a box
/// both narrower and shorter than the file and states what must come out of it.
///
/// **The cap is a request and a cap, not a promise**, and that sentence is in
/// both arms' headers: Media Foundation's video processor scales on most
/// machines and formats and nothing promises it does on all of them, and
/// `AVAssetImageGenerator`'s `maximumSize` is documented the same way. **The two
/// machines really do answer differently**, measured on 2026-09-12 with this
/// very case: asked for 80×80, `AVAssetImageGenerator` returns **80×60** and
/// Media Foundation returns the file's own **160×120** — it refused the smaller
/// output type for this clip and the arm settled for RGB-32 at native size, as
/// its `request_rgb32` says it will. Neither is wrong and neither is visible: the
/// window's own sampler fits whichever it is given into the same box. So what is
/// asserted is the pair of things that are true either way — nothing comes back
/// larger than the file, and whatever size comes back has the file's own shape.
///
/// MUTATION: fit by `cover` rather than by `contain` in either arm and the
/// frame comes back 80×80 with the aspect assertion naming it; drop the clamp
/// that refuses to enlarge and the case above comes back 280×210.
#[test]
fn a_frame_asked_for_smaller_keeps_its_proportions() {
    let file = fixture("folio-video-test.mp4");
    let box_ = (80_u32, 80_u32);
    let frame = first_frame(&file, box_.0, box_.1);
    if !HAS_DECODER {
        assert_eq!(frame, None, "a machine with no decoder draws nothing");
        return;
    }
    let frame = frame.expect("a real recording gives up a frame");
    eprintln!(
        "VIDEO_FIT asked={}x{} got={}x{} native={}x{}",
        box_.0, box_.1, frame.width, frame.height, frame.native_width, frame.native_height
    );
    assert!(
        frame.width <= NATIVE.0 && frame.height <= NATIVE.1,
        "a fit is never an enlargement: {}x{}",
        frame.width,
        frame.height
    );
    assert!(
        frame.width <= box_.0 || (frame.width, frame.height) == NATIVE,
        "a cap the platform honoured, or the file's own size because it did not: {}x{}",
        frame.width,
        frame.height
    );
    // 160×120 is 4:3, and one pixel of rounding is the whole of what a scale of
    // whole pixels may cost.
    let expected_height =
        (f64::from(frame.width) * f64::from(NATIVE.1) / f64::from(NATIVE.0)).round() as i64;
    assert!(
        (i64::from(frame.height) - expected_height).abs() <= 1,
        "the recording is 4:3 and this frame is {}x{}",
        frame.width,
        frame.height
    );
    assert_eq!(
        frame.rgba.len(),
        frame.width as usize * frame.height as usize * 4,
        "straight RGBA8, packed, one row after another"
    );
}

/// RED — **nothing that is not a video is drawn, and nothing panics.**
///
/// The refusal path through whichever real platform this is: a file that is not
/// there, a file that is empty, and a file whose bytes are text with a video's
/// name on it. All three must be the module's one silence, on the caller's
/// thread, with whatever the platform was given back — and the last of the three
/// is the one that actually reaches the decoder and is turned away by it.
///
/// MUTATION: `unwrap` any of the `?`s in either arm's decode and the third case
/// takes the decoration worker down, which on the machine is a hover over a
/// renamed archive ending the formula lane for the session.
#[test]
fn nothing_that_is_not_a_video_is_drawn() {
    let dir = std::env::temp_dir().join(format!(
        "folio-video-refusals-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("a scratch directory");

    let missing = dir.join("no-such-file.mp4");
    assert_eq!(first_frame(&missing, CARD.0, CARD.1), None);

    let empty = dir.join("empty.mp4");
    std::fs::write(&empty, b"").expect("an empty file");
    assert_eq!(first_frame(&empty, CARD.0, CARD.1), None);

    let text = dir.join("renamed.mp4");
    std::fs::write(&text, b"this is not a video at all, whatever it is called")
        .expect("a text file");
    assert_eq!(first_frame(&text, CARD.0, CARD.1), None);

    // A directory carrying a video's name, which is the one shape a file column
    // can hand this that is not a file at all.
    let folder = dir.join("folder.mp4");
    std::fs::create_dir_all(&folder).expect("a directory with a video's name");
    assert_eq!(first_frame(&folder, CARD.0, CARD.1), None);

    let _ = std::fs::remove_dir_all(&dir);
}
