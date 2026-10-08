//! **The crate root: file peek.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    a_decode, a_held_raster, chevron_button, glance_fixture, method_body, peek_open,
    the_three_chevrons,
};
use std::time::Duration;

/// PIN — **the window a card becomes opens with the card's own top-left,
/// held exactly where the hand is holding it.**
///
/// The ruling's second half: *一张卡,不论它是什么,拖头就变浮窗*, and
/// "拖动跟手" is the clause this is about. A peek float keeps its own
/// frame and so cannot move; a glance card becomes something bigger than it
/// was, so the question is which edge stays — and it is the top-left,
/// because the head is there and the hand is on the head.
///
/// MUTATIONS that must turn it red:
/// ① measure the grab before the clamp — a card near the window's bottom
///    edge promotes to a window that then drifts away from the pointer by
///    however far the clamp moved it, on every move after the first;
/// ② centre the window on the card instead of sharing its corner — the
///    thing being carried slides out from under the finger the moment it is
///    picked up;
/// ③ drop the clamp — a card at the bottom of the window promotes to a
///    window whose foot is off the glass.
#[test]
fn a_card_promoted_to_a_window_keeps_the_corner_the_hand_is_on() {
    const SCALE: f32 = 1.0;
    let viewport = [0.0_f32, 0.0, 1600.0, 900.0];
    // A card of the module's own width, standing clear of every edge.
    let card = [400.0_f32, 200.0, 700.0, 464.0];
    let size = [520.0_f32, 480.0];
    let pointer = [460.0_f32, 212.0];

    let (placed, grab) = file_peek_promotion(card, size, pointer, viewport, SCALE);
    assert_eq!(
        [placed[0], placed[1]],
        [card[0], card[1]],
        "the head does not move, because the hand is on the head"
    );
    assert_eq!(
        [placed[2] - placed[0], placed[3] - placed[1]],
        size,
        "and the window is the size it asked to be"
    );
    assert_eq!(
        grab,
        [60.0, 12.0],
        "the offset inside the frame that opened"
    );
    assert_eq!(
        float::float_dragged_to(placed, pointer, grab, viewport, SCALE),
        placed,
        "so the move that opened it moves it no further"
    );

    // **Near the bottom edge the clamp overrules the corner — once.** The
    // grab is measured after it, so the window is held where it actually
    // stands rather than where it asked to stand.
    let low = [400.0_f32, 700.0, 700.0, 880.0];
    let low_pointer = [460.0_f32, 712.0];
    let (placed, grab) = file_peek_promotion(low, size, low_pointer, viewport, SCALE);
    assert!(
        placed[3] <= viewport[3],
        "a window promoted at the bottom of the glass is still on the glass"
    );
    assert!(
        placed[1] < low[1],
        "which it can only be by having moved up"
    );
    assert_eq!(
        float::float_dragged_to(placed, low_pointer, grab, viewport, SCALE),
        placed,
        "and it does not drift away from the hand on the next move"
    );
    // **And the move after that is the hand's alone.** Carried up into open
    // ground it travels exactly as far as the hand did — not that plus the
    // distance the clamp had already corrected. Asked out here rather than
    // down against the edge, where the drag's own clamp would put a wrong
    // frame back where the right one is and hide the difference entirely.
    let carried = [low_pointer[0] - 40.0, low_pointer[1] - 300.0];
    assert_eq!(
        float::float_dragged_to(placed, carried, grab, viewport, SCALE),
        [
            placed[0] - 40.0,
            placed[1] - 300.0,
            placed[2] - 40.0,
            placed[3] - 300.0,
        ],
        "the window follows the hand and never the clamp's own correction"
    );
}

#[test]
fn peek_hover_settles_after_the_delay_and_slides_along_one_span_without_restarting() {
    let start = Instant::now();
    let path = PathBuf::from(r"C:\img\a.png");
    let mut hover = PeekHover::default();

    let subject = PeekSubject::from_path(path.clone());
    let pane = SeatId(1);
    assert!(!hover.observe(
        Some((subject.clone(), pane)),
        PhysicalPosition::new(10.0, 10.0),
        start
    ));
    assert!(
        hover
            .activate_if_due(start + Duration::from_millis(299))
            .is_none()
    );
    // Sliding along the same path span refreshes the anchor but keeps the original clock:
    // the flyout settles where the pointer last was, without ever restarting the delay.
    assert!(!hover.observe(
        Some((subject.clone(), pane)),
        PhysicalPosition::new(30.0, 12.0),
        start + Duration::from_millis(200)
    ));
    let settled = hover
        .activate_if_due(start + Duration::from_millis(300))
        .expect("original deadline must fire");
    assert_eq!(settled.subject, subject);
    assert_eq!(settled.pointer.x, 30.0);
    // While active, staying on the span neither hides nor re-arms.
    assert!(!hover.observe(
        Some((subject.clone(), pane)),
        PhysicalPosition::new(31.0, 12.0),
        start + Duration::from_millis(400)
    ));
    assert!(hover.show_at.is_none());
    // Leaving the span hides the flyout and drops all state.
    assert!(hover.observe(
        None,
        PhysicalPosition::new(31.0, 40.0),
        start + Duration::from_millis(500)
    ));
    assert!(hover.active.is_none());
}

/// PIN — a peek belongs to the pane the pointer was in, and carries it.
///
/// Two panes, and the *same file* printed in both — which is what makes the pin bite, because
/// subject identity alone cannot tell the two hovers apart. Sliding from one pane's copy to the
/// other's has genuinely left the reference the flyout was raised over: the old flyout must go
/// down, a fresh settle clock must run, and what settles must name the pane it settled in, so
/// the box can be sized and placed against that pane instead of against whichever one happens
/// to hold the keyboard.
///
/// Drop the seat from the span's identity and every assertion below reverses.
#[test]
fn one_file_printed_in_two_panes_is_two_hovers_and_each_names_its_own_pane() {
    let start = Instant::now();
    let subject = PeekSubject::from_path(PathBuf::from(r"C:\img\shared.png"));
    let left = SeatId(1);
    let right = SeatId(2);
    let mut hover = PeekHover::default();

    assert!(!hover.observe(
        Some((subject.clone(), left)),
        PhysicalPosition::new(100.0, 200.0),
        start
    ));
    let settled = hover
        .activate_if_due(start + Duration::from_millis(300))
        .expect("the left pane's hover settles");
    assert_eq!(
        settled.seat, left,
        "a settled peek names the pane whose cells the pointer was on",
    );

    // Straight across into the other pane's copy of the same file.
    assert!(
        hover.observe(
            Some((subject.clone(), right)),
            PhysicalPosition::new(900.0, 200.0),
            start + Duration::from_millis(400)
        ),
        "leaving the pane takes the flyout down even though the file is the same",
    );
    assert!(
        hover
            .activate_if_due(start + Duration::from_millis(600))
            .is_none(),
        "the second pane's hover runs its own settle clock",
    );
    let settled = hover
        .activate_if_due(start + Duration::from_millis(700))
        .expect("the right pane's hover settles in its turn");
    assert_eq!(settled.seat, right);
    assert_eq!(
        settled.pointer,
        PhysicalPosition::new(900.0, 200.0),
        "the anchor is the window point the hover settled on, untranslated",
    );

    // And staying inside one pane still slides along one span, as it always did.
    assert!(!hover.observe(
        Some((subject, right)),
        PhysicalPosition::new(920.0, 200.0),
        start + Duration::from_millis(800)
    ));
    assert!(hover.show_at.is_none());
}

#[test]
fn peek_hover_switching_paths_hides_the_old_flyout_and_restarts_the_clock() {
    let start = Instant::now();
    let first = PeekSubject::from_path(PathBuf::from(r"C:\img\a.png"));
    let second = PeekSubject::from_path(PathBuf::from(r"C:\img\b.png"));
    let mut hover = PeekHover::default();
    let pane = SeatId(1);
    hover.observe(
        Some((first, pane)),
        PhysicalPosition::new(10.0, 10.0),
        start,
    );
    assert!(
        hover
            .activate_if_due(start + Duration::from_millis(300))
            .is_some()
    );
    let hidden = hover.observe(
        Some((second.clone(), pane)),
        PhysicalPosition::new(50.0, 10.0),
        start + Duration::from_millis(400),
    );
    assert!(hidden, "switching spans must hide the visible flyout");
    assert!(
        hover
            .activate_if_due(start + Duration::from_millis(600))
            .is_none(),
        "the second span runs a fresh settle clock"
    );
    let settled = hover
        .activate_if_due(start + Duration::from_millis(700))
        .expect("second span settles on its own deadline");
    assert_eq!(settled.subject, second);
}

/// PIN (band retirement ruling, 2026-08-03, docs §6.1): the peek's third source is an OSC 1337
/// payload, which names no file, and the pipeline tells the two apart by exactly one property —
/// whether a cache miss has anything to read.
///
/// A named file's identity is its normalized path, so the same file spelled two ways is one
/// hover and one cache entry. A payload's identity is the decoder's content key, and it carries
/// no path at all: the bytes came through the stream and were remembered when the decode landed,
/// so a miss is a hover that arrived early, never a disk read to schedule. That `path: None` is
/// the whole of the difference is what keeps `show_or_request_peek` one function.
#[test]
fn a_stream_payload_is_a_peek_subject_with_nothing_to_read() {
    let by_path = PeekSubject::from_path(PathBuf::from(r"C:\img\a.png"));
    assert_eq!(
        by_path.key,
        normalized_local_image_path_key(std::path::Path::new(r"C:\img\a.png")),
        "a named file is identified the way the decoder identifies it",
    );
    assert!(by_path.path.is_some(), "a named file is readable on a miss");
    assert_eq!(
        PeekSubject::from_path(PathBuf::from(r"C:\IMG\A.PNG")),
        by_path,
        "one file spelled two ways is one hover and one cache entry",
    );

    let payload = PeekSubject::from_content_key("image:sha-abc".to_owned());
    assert_eq!(payload.key, "image:sha-abc");
    assert!(
        payload.path.is_none(),
        "a stream payload has no file behind it, so a cache miss reads nothing",
    );
    assert_ne!(payload, by_path);
}

/// PIN (verification ruling 2026-08-04, the warm peek): the decode a verified reference already
/// paid for is filed under the very key the hover looks up, so the flyout opens from cache and
/// no second read of the same file is ever scheduled.
///
/// `show_or_request_peek` sends a `PeekImage` task on exactly one condition — a `None` entry
/// under `PeekSubject::key`. So "the peek is warm" and "the two keys are the same string" are
/// the same statement, and it is the one asserted here. The stream-payload arm is asserted
/// beside it because both shapes go through this one function and must not converge: a payload
/// has no path to key by.
///
/// RED CHECK: keying a named file's verification decode by `decoded.key` (its content identity)
/// instead of its path leaves the hover's lookup missing, and the first assertion goes red —
/// which is precisely the "decoded twice, cached twice" defect the shared key rules out.
#[test]
fn a_verified_references_decode_is_filed_under_the_key_the_hover_asks_by() {
    let path = PathBuf::from(r"C:\img\Sunset.PNG");
    let decoded = bt_term::DecodedInlineImage {
        occurrence_id: 7,
        key: "image:0123456789abcdef0123456789abcdef".to_owned(),
        rgba: Arc::from(vec![0u8; 4]),
        width_px: 1,
        height_px: 1,
        native_size: None,
        animated: false,
    };

    assert_eq!(
        peek_cache_key_for_decode(
            &bt_term::InlineImageSource::LocalPath(path.clone()),
            &decoded
        ),
        PeekSubject::from_path(path.clone()).key,
        "the verification decode lands exactly where the hover will look for it",
    );
    assert_eq!(
        peek_cache_key_for_decode(
            &bt_term::InlineImageSource::LocalPath(PathBuf::from(r"c:/img/sunset.png")),
            &decoded
        ),
        PeekSubject::from_path(path).key,
        "and one file spelled two ways is still one warm entry",
    );
    assert_eq!(
        peek_cache_key_for_decode(
            &bt_term::InlineImageSource::Osc1337(b"AAAA".to_vec()),
            &decoded
        ),
        PeekSubject::from_content_key(decoded.key.clone()).key,
        "a stream payload has no path, so it stays keyed by content",
    );
}

/// RED — **this window's decoded pictures have a ceiling** (review row R1-8,
/// adversarial review 2026-09-08).
///
/// RED EVIDENCE (2026-09-08), before the budget:
///
/// ```text
/// eight thirty-two megabyte pictures into a 201326592 byte cache: it is holding 268436480
/// ```
///
/// `peek_cache` was a `HashMap` with one door in and one narrow door out —
/// [`forget_a_picture`], which is a *file watch* telling the window one named
/// file has changed. Nothing anywhere took an entry out because there were
/// too many of them, so every distinct picture a pointer had rested on since
/// the window opened was still decoded in it.
///
/// The entries here are the real ones: `PeekCacheEntry::Ready` holding real
/// `Arc<[u8]>`s, through the real budget, so what is being asserted is what
/// this window will actually hold.
///
/// MUTATION: build the cache with `u64::MAX` and the first assertion goes red
/// with everything ever inserted still in it.
#[test]
fn the_windows_decoded_pictures_are_bounded_and_the_oldest_goes_first() {
    const PICTURE_BYTES: usize = 32 * 1024 * 1024;
    let mut cache = PeekCache::with_budget(MAX_PEEK_CACHE_BYTES);
    for index in 0..8_u8 {
        cache.insert(
            format!(r"d:\shots\{index}.png"),
            PeekCacheEntry::Ready {
                key: format!("image:{index}"),
                rgba: Arc::from(vec![index; PICTURE_BYTES]),
                width_px: 2048,
                height_px: 4096,
                native_size: None,
            },
        );
    }
    assert!(
        cache.bytes_held() <= MAX_PEEK_CACHE_BYTES,
        "eight thirty-two megabyte pictures into a {} byte cache: it is holding {}",
        MAX_PEEK_CACHE_BYTES,
        cache.bytes_held(),
    );
    assert!(
        cache.get(r"d:\shots\7.png").is_some(),
        "the picture asked for last is the one it kept",
    );
    assert!(
        cache.get(r"d:\shots\0.png").is_none(),
        "and the one nothing has looked at since the window opened is gone",
    );
}

/// RED — **a picture pane that has been answered does not ask again when the
/// decode store lets its pixels go** (adversarial review 2026-09-11, row
/// RB-1; `docs/DESIGN.md` §7.1.3u ③).
///
/// RED EVIDENCE. §7.1.3u taught a markdown *page* that a miss in a bounded
/// cache is not "never asked". The standalone picture pane was the consumer
/// that fix did not touch: `refit_preview_picture` consulted
/// [`PeekCache`] and nothing else, and a miss fell straight through to
/// hiding the picture, asking for the file and filing a `Pending` — even
/// though the pane was standing on a `PeekThumbnail` of its own, made from
/// that very decode, and drawing it. With a visible working set over
/// [`MAX_PEEK_CACHE_BYTES`] each arrival evicts a picture another host is
/// drawing, the refit that arrival triggers finds the miss, asks again, and
/// the cycle sustains itself with no input at all.
///
/// MUTATION: make [`surface_pixels`] read a miss as
/// [`SurfacePixels::Nothing`] again — ignore the `standing` argument — and
/// the errand at the size the pane is already holding becomes
/// [`PictureErrand::Read`], which is the first turn of the loop.
#[test]
fn a_pane_whose_picture_was_evicted_does_not_ask_again() {
    const PIXELS: usize = 3 * 1024 * 1024;
    const BUDGET: u64 = 4 * 1024 * 1024;
    const NATIVE: (u32, u32) = (1024, 768);
    let paths = [
        PathBuf::from(r"D:\shots\a.png"),
        PathBuf::from(r"D:\shots\b.png"),
    ];
    let keys = paths
        .iter()
        .map(|path| bt_term::normalized_local_image_path_key(path))
        .collect::<Vec<_>>();
    let mut peek = PeekCache::with_budget(BUDGET);
    peek.insert(keys[0].clone(), a_decode("content-a", NATIVE, PIXELS));
    // The pane resampled that decode to the box it stands in, and holds the
    // answer: this is what is on the glass.
    let held = a_held_raster("content-a", (500, 375));
    let target: PeekThumbnailTarget = ("content-a".to_owned(), 500, 375);

    // A neighbouring surface's decode lands, and this one's is what the
    // bounded store lets go of to make room.
    peek.insert(keys[1].clone(), a_decode("content-b", NATIVE, PIXELS));
    assert!(
        peek.get(&keys[0]).is_none(),
        "the fixture is a store that cannot hold both decodes at once"
    );

    let pixels = surface_pixels(
        &mut peek,
        Some(held.content_key.as_str()),
        Some(NATIVE),
        None,
        &keys[0],
    );
    assert!(
        matches!(pixels, SurfacePixels::Standing { .. }),
        "the pane was answered once and is still drawing that answer: {pixels:?}"
    );
    assert_eq!(
        picture_errand(&pixels, held.matches(&target)),
        PictureErrand::Nothing,
        "a surface holding the very raster this frame wants has nothing to \
             ask anybody — a miss in a bounded cache is not 'never asked'",
    );

    // And the one case that *is* a question: the pane is made wider, so the
    // raster it holds is not the raster it wants, and the pixels a sharper
    // pass would be made from are not in this window.
    let wider: PeekThumbnailTarget = ("content-a".to_owned(), 700, 525);
    assert_eq!(
        picture_errand(&pixels, held.matches(&wider)),
        PictureErrand::Read,
        "a size it does not hold, with nothing to resample from, is one read"
    );
    peek.insert(keys[0].clone(), PeekCacheEntry::Pending);
    let asked = surface_pixels(
        &mut peek,
        Some(held.content_key.as_str()),
        Some(NATIVE),
        None,
        &keys[0],
    );
    assert_eq!(
        picture_errand(&asked, held.matches(&wider)),
        PictureErrand::Wait,
        "and the store's own `Pending` is what keeps it to one read"
    );
}

/// RED (the same report, the glance half) — **a hand resting on a floating
/// window arms no peek for the row beneath it.**
///
/// `row_under` is the glance clock's one question, and it consumed the
/// float's claim for a tree row only: every other part of a window fell
/// through to the docked columns, and then to the terminal's own printed
/// references. So a rest on a preview window's text armed the covered row's
/// glance, which matured into a card drawn on top of the window.
///
/// Red gate: drop the declining arm and the ordering assertions fail by
/// name; move it below the docked rows and the first one does.
#[test]
fn a_hover_inside_a_float_arms_no_peek_for_the_row_beneath() {
    let rows = method_body("Runtime", "row_under");
    let own = rows
        .find("Some(PointerTarget::Float(id, float::FloatPart::Row(index)))")
        .expect("a window's own tree row is that window's row");
    let declined = rows
        .find("Some(PointerTarget::Float(..)) => None,")
        .expect("and every other part of that window is no row at all");
    let docked = rows
        .find("Some(PointerTarget::Chrome(")
        .expect("the docked columns answer after the windows");
    assert!(
        own < declined && declined < docked,
        "the window's own row first, then its refusal, and only then the \
             chrome it is standing on"
    );
    let cell = rows
        .find("self.terminal_reference_cell()")
        .expect("a printed reference is the last row this question has");
    assert!(
        declined < cell,
        "a reference printed under a window is not under the pointer either"
    );
    let glancing = method_body("Runtime", "glancing_row_at");
    assert!(
        glancing.contains("self.row_under(position)?"),
        "and the glance's clock is armed from this one answer, so `None` \
             here is both `no row lights` and `the card already up is retired`"
    );
}

/// RED — **a picture opened again after its file was replaced is read off
/// the disk, not out of this window's memory** (user report 2026-08-31, the
/// second half).
///
/// The plainest gesture a reader has, and the one that named the second
/// cause: the pane was closed, the file was replaced by a `mv`, the pane was
/// opened again on the very same path — and the old picture came back. Two
/// caches keyed by a path alone were holding it, and only one of them is in
/// this crate. The other is [`bt_term::InlineImageDecoder`]'s, which its own
/// test answers for; this one drives **both**, in the order the app drives
/// them, over a real file that a real `std::fs::rename` really replaces —
/// because each road can regress on its own and a window served by a correct
/// decoder is still wrong if it never asks it.
///
/// MUTATIONS: take [`forget_a_picture`]'s `peek_cache.remove` away and the
/// reopened picture is 4×2 again — the report. Take the decoder's stamp
/// guard away (`bt-term`) and the same line goes red one layer down. Leave
/// the neighbouring file out of the removal — i.e. clear the whole cache —
/// and the last assertion goes red: forgetting one picture is not forgetting
/// every picture.
#[test]
fn a_picture_opened_again_after_a_rename_is_read_off_the_disk() {
    fn png_of(width: u32, height: u32, colour: [u8; 4]) -> Vec<u8> {
        let picture = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba(colour),
        ));
        let mut bytes = std::io::Cursor::new(Vec::new());
        picture
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("a PNG this process wrote");
        bytes.into_inner()
    }

    /// Opening a picture onto a pane, as far as the caches are concerned:
    /// the decode lane is asked, and the answer is remembered under the
    /// file's normalized path.
    fn open(decoder: &mut bt_term::InlineImageDecoder, peek_cache: &mut PeekCache, path: &Path) {
        let decoded = decoder
            .decode(bt_term::InlineImageTask {
                occurrence_id: 0,
                source: bt_term::InlineImageSource::LocalPath(path.to_path_buf()),
            })
            .expect("the picture decodes");
        peek_cache.insert(
            normalized_local_image_path_key(path),
            PeekCacheEntry::Ready {
                key: decoded.key,
                rgba: decoded.rgba,
                width_px: decoded.width_px,
                height_px: decoded.height_px,
                native_size: decoded.native_size,
            },
        );
    }

    fn size_of(peek_cache: &PeekCache, path: &Path) -> Option<(u32, u32)> {
        match peek_cache.get(&normalized_local_image_path_key(path))? {
            PeekCacheEntry::Ready {
                width_px,
                height_px,
                ..
            } => Some((*width_px, *height_px)),
            PeekCacheEntry::Pending | PeekCacheEntry::Failed(_) => None,
        }
    }

    let directory = bt_testpath::temp_path("folio-picture-reopen");
    std::fs::create_dir(&directory).expect("a scratch folder");
    let card = directory.join("card-3.png");
    let neighbour = directory.join("card-4.png");
    std::fs::write(&card, png_of(4, 2, [255, 0, 0, 255])).expect("the first card");
    std::fs::write(&neighbour, png_of(9, 9, [255, 255, 0, 255])).expect("its neighbour");

    let mut decoder = bt_term::InlineImageDecoder::default();
    let mut peek_cache = PeekCache::with_budget(MAX_PEEK_CACHE_BYTES);
    let mut video_facts = BTreeMap::new();
    let mut pictures = MarkdownPictures::default();

    open(&mut decoder, &mut peek_cache, &card);
    open(&mut decoder, &mut peek_cache, &neighbour);
    assert_eq!(size_of(&peek_cache, &card), Some((4, 2)));
    let generation = pictures.generation;

    // The pane is closed. Nothing forgets anything, and nothing should:
    // this is a picture a hover card or a markdown page may still be drawing.
    // Then another shell replaces the file — `mv` over an existing name is a
    // rename, so the bytes at that path are a different file entirely.
    let replacement = directory.join("card-3.new.png");
    std::fs::write(&replacement, png_of(6, 5, [0, 0, 255, 255])).expect("the new card");
    std::fs::rename(&replacement, &card).expect("the replacement lands on the name");

    // And the pane is opened again on the same path.
    forget_a_picture(&mut peek_cache, &mut video_facts, &mut pictures, &card);
    assert!(
        size_of(&peek_cache, &card).is_none(),
        "opening a picture ends this window's right to answer from memory"
    );
    assert!(
        pictures.generation > generation,
        "and every markdown page standing on that file is told to ask again"
    );
    open(&mut decoder, &mut peek_cache, &card);

    assert_eq!(
        size_of(&peek_cache, &card),
        Some((6, 5)),
        "the reader opened the file that is on the disk, so that is the \
             picture and that is the size the meta line states"
    );
    assert_eq!(
        size_of(&peek_cache, &neighbour),
        Some((9, 9)),
        "and the file nobody touched kept its decode: forgetting one picture \
             is not forgetting every picture"
    );

    std::fs::remove_file(&card).expect("the card goes");
    std::fs::remove_file(&neighbour).expect("its neighbour goes");
    std::fs::remove_dir(&directory).expect("and the folder with them");
}

/// RED — **the frame under a hover card and the frame in the preview pane
/// come out of one decoder, asked through one door** (user ruling
/// 2026-08-27; §7.23).
///
/// The whole of §7.10 ⑥ said about a *lane* rather than about a name: a
/// glance card and a pane over the same file must show the same picture, and
/// the only way that is true by construction is that both ask
/// [`WindowRuntime::request_peek_pixels`] and neither reads the file's name
/// for itself. The two surfaces are hundreds of lines apart and each has its
/// own cache guard, so a second `MathWorkerRequest::PeekVideoFrame` built at
/// one of them would compile, would work, and would be the place the two
/// drift the day the fork grows a third arm.
///
/// Asserted as text for [`files_locate_door_tests`]' reason exactly: what is
/// being pinned is *which function builds which request*, and no value any
/// assertion can read says that.
///
/// RED GATE: inline the fork back into `refit_preview_picture` — that
/// function starts naming a decoder and this names the function and the
/// lane it named.
#[test]
fn one_door_decides_which_decoder_a_hover_and_a_pane_ask() {
    // Built at run time so that this test's own text is not one of the
    // sites it is counting.
    let lane = |variant: &str| format!("MathWorkerRequest::{variant} {{");
    let door = method_body("Runtime", "request_peek_pixels");
    for variant in ["PeekImage", "PeekVideoFrame"] {
        assert!(
            door.contains(&lane(variant)),
            "the door does not build {variant}:\n{door}"
        );
    }
    // And the two surfaces that show pixels name neither: what they ask for
    // is "this file's pixels", and a second reading of the name at either of
    // them is where the card and the pane come to disagree.
    for surface in ["refit_preview_picture", "file_peek_fitted_pixels"] {
        let text = method_body("Runtime", surface);
        for variant in ["PeekImage", "PeekVideoFrame"] {
            assert!(
                !text.contains(&lane(variant)),
                "`{surface}` chooses a decoder for itself: it names {variant}"
            );
        }
        assert!(
            text.contains("self.request_peek_pixels("),
            "`{surface}` must ask through the one door"
        );
    }
    // The door reads the class through the same predicate the open lane
    // forks on, so a file that *opened* as a video is decoded as one.
    assert!(
        door.contains("preview::path_names_a_video("),
        "the door must ask the class's own predicate:\n{door}"
    );
}

/// RED — **the card keeps the last few pages of the document it is over, and
/// keeps the ones the hand is nearest** (user ruling 2026-08-26;
/// [`PEEK_PAGE_CACHE`]).
///
/// A rastered page is hundreds of kilobytes and a long report has hundreds of
/// pages, so the cache is bounded — and a bound is only useful if what it
/// throws away is what nobody is looking at. The order is written at one
/// place, [`PeekPageSlot::wanted`], which the request lane calls for every
/// page in view on every frame; that is what makes "least recently used"
/// mean "furthest from where the reader stopped" rather than "drawn longest
/// ago", and the two differ exactly when a reader winds back up a document.
///
/// RED GATE ①: let [`PeekPageSlot::keep`] push without evicting and the
/// second block fails — the cache is unbounded, which for a two-hundred-page
/// report is a hover that costs a third of a gigabyte. RED GATE ②: make
/// `wanted` a plain lookup that does not reorder and the last block fails:
/// page 0, which the reader has just scrolled back to, is thrown away while
/// it is the one on screen.
#[test]
fn the_cards_page_cache_keeps_what_the_hand_is_nearest() {
    let raster = |page: u32| PeekPageRaster {
        key: format!("page-{page}"),
        rgba: Arc::from(vec![0_u8; 4].into_boxed_slice()),
        width_px: 1,
        height_px: 1,
    };
    let mut slot = PeekPageSlot {
        path: PathBuf::from(r"D:\reports\long.pdf"),
        fit: (280, 160),
        mtime: None,
        asked: BTreeSet::new(),
        pages: Vec::new(),
    };
    for page in 0..PEEK_PAGE_CACHE as u32 {
        slot.keep(page, raster(page));
    }
    assert_eq!(slot.pages.len(), PEEK_PAGE_CACHE);
    assert!(
        (0..PEEK_PAGE_CACHE as u32).all(|page| slot.page(page).is_some()),
        "everything asked for so far is still here"
    );

    // One more page than the cache holds, with nothing having been re-read:
    // the oldest goes and only the oldest.
    let past = PEEK_PAGE_CACHE as u32;
    slot.keep(past, raster(past));
    assert_eq!(slot.pages.len(), PEEK_PAGE_CACHE, "the bound is a bound");
    assert!(slot.page(0).is_none(), "the page furthest behind is gone");
    assert!(slot.page(past).is_some(), "and the newest one is here");
    assert!(
        slot.page(1).is_some(),
        "and nothing else was thrown away with it"
    );

    // The same page drawn again replaces itself rather than joining the run
    // twice — a second entry would let the stale one be found first for ever.
    slot.keep(past, raster(past));
    assert_eq!(slot.pages.len(), PEEK_PAGE_CACHE);
    assert_eq!(
        slot.pages.iter().filter(|(page, _)| *page == past).count(),
        1
    );

    // **A page the reader has scrolled back to is not the oldest thing here,
    // whenever it was drawn.** This is the whole of what `wanted` records.
    assert!(
        slot.wanted(1),
        "page 1 is in the cache and is being looked at"
    );
    let next = past + 1;
    slot.keep(next, raster(next));
    assert!(
        slot.page(1).is_some(),
        "so the page under the pointer survived the eviction it was next in line for"
    );
    assert!(slot.page(2).is_none(), "and the one behind it left instead");
    assert!(
        !slot.wanted(0),
        "a page that is not held is not made recent by being asked about"
    );
}

/// RED (35) — **a second press on a `⌄` pins a peek, and closes only a pinned
/// menu.**
///
/// Ruling 4 of 2026-09-23: 「"再点即收"只在钉住态成立」. Since 2026-09-13 a press
/// on the control a popover hangs from was spent closing it; that is still the
/// rule for a pinned menu, and for every popover no `⌄` governs. For a peek —
/// a menu a rest raised — the same press pins it instead, and the menu stays.
/// Driven through the press rule the router asks
/// ([`press_spends_itself_closing`], then [`press_pins_a_peek`]) for each of
/// the three controls.
///
/// MUTATION: make `press_pins_a_peek` return `verdict` unchanged and the first
/// assertion goes red (the peek is spent closing, as on `main`).
#[test]
fn a_second_click_closes_only_a_pinned_menu() {
    let start = Instant::now();
    for (popup, control) in the_three_chevrons() {
        let mut gates = peek_open(popup, start);
        let first = press_pins_a_peek(
            press_spends_itself_closing(chevron_button(control), Some(control)),
            gates.gate(popup),
        );
        assert_eq!(
            first,
            OwnPress::Pinned,
            "{popup:?}: a press on a peek pins it"
        );
        assert!(!first.dismisses() && first.ends_the_press());
        assert!(gates.gate(popup).is_some_and(|gate| gate.is_pinned()));

        let second = press_pins_a_peek(
            press_spends_itself_closing(chevron_button(control), Some(control)),
            gates.gate(popup),
        );
        assert_eq!(
            second,
            OwnPress::Spent,
            "{popup:?}: a press on a pinned menu closes it"
        );
        assert!(second.dismisses() && second.ends_the_press());
    }
    // A popover no `⌄` governs keeps the 2026-09-13 rule untouched.
    let filter = PopoverTrigger::Chrome(seats::ChromeTarget::FilesRoot(SeatId(1)));
    assert_eq!(
        press_pins_a_peek(
            press_spends_itself_closing(chevron_button(filter), Some(filter)),
            None
        ),
        OwnPress::Spent
    );
}

/// RED (35) — **a click elsewhere closes a pinned menu, and the pin goes with
/// it**, for the pane head's `⌄` and the tab strip's.
///
/// A press that lands on anything but the menu's own button is
/// [`OwnPress::Elsewhere`] whatever the pin says: the dismissal arm puts the
/// menu away through its closer and the press goes on being the press it was.
/// That includes a press on *another* pane head's `⌄`, which is how a pinned
/// menu moves across a split — the toggle opens the new head's menu in place
/// of the old one, and that press pins the new one.
///
/// MUTATION: make `press_pins_a_peek` pin on any verdict (drop the
/// `verdict == OwnPress::Spent` guard) and the first assertion goes red —
/// a click elsewhere would stop dismissing.
#[test]
fn a_click_elsewhere_closes_a_pinned_menu() {
    let start = Instant::now();
    for (popup, control) in the_three_chevrons().into_iter().take(2) {
        let mut gates = peek_open(popup, start);
        gates.gate(popup).expect("governed").pin();
        for elsewhere in [
            None,
            Some(PopoverTrigger::Chrome(seats::ChromeTarget::Settings)),
            Some(PopoverTrigger::Chrome(seats::ChromeTarget::PaneMenu(
                SeatId(9),
            ))),
        ] {
            let verdict = press_pins_a_peek(
                press_spends_itself_closing(chevron_button(control), elsewhere),
                gates.gate(popup),
            );
            assert_eq!(
                verdict,
                OwnPress::Elsewhere,
                "{popup:?} pressed at {elsewhere:?}"
            );
            assert!(verdict.dismisses() && !verdict.ends_the_press());
        }
        // The dismissal.
        gates.menu_gone(popup);
        assert!(gates.gate(popup).is_some_and(|gate| !gate.is_pinned()));
    }
    // And the arms that dismiss do it through the closers that drop the pin.
    let router = method_body("Runtime", "mouse_input");
    assert!(router.contains("self.close_pane_menu()?"));
    assert!(router.contains("self.close_profile_menu()?"));
    assert!(router.contains("self.close_file_menu()?"));
}

/// PIN (**the card column reaches the peek's one predicate, and only it**) —
/// user ruling and report with screenshot, 2026-08-21.
///
/// A layout peek was dropping over the focus column, covering the two cards
/// below the one under the pointer. `peek_strip::eligible` now refuses a
/// window whose cards are unfolded — `a_column_of_unfolded_cards_refuses_
/// every_peek` is that policy driven directly — and what is left to pin here
/// is the *wiring*, which has two halves and can only be read as text:
///
/// * the posture reaches the predicate, from `rail_posture()` — the same
///   join the solver and every geometry are handed — rather than from
///   `window.focus_mode`, so nothing in this window has a second opinion
///   about whether a column is on screen;
/// * and `layout_peek_target_at` adds **no** judgment of its own. It is the
///   arming path; `hide_layout_peek`'s side is the retiring one; and
///   `layout_peek_eligible`'s own doc says why one predicate serves both —
///   "the two asking different questions is exactly how a popup survives the
///   death of its own subject". A peek that settled a frame before focus
///   mode came on would, with a second `if focus` at the arming site only,
///   have nobody left to retire it.
///
/// Red gate: pass `self.window.focus_mode` instead and the first assertion
/// fails by name; add the second author at the call site and the third does.
#[test]
fn the_focus_column_refuses_the_layout_peek_through_one_predicate() {
    let predicate = method_body("Runtime", "layout_peek_eligible");
    assert!(
        predicate.contains("self.rail_posture().draws_focus_rail(),"),
        "the peek's one predicate is told whether this window's cards are \
             unfolded, and told it by the posture every other geometry reads"
    );
    let arming = method_body("Runtime", "layout_peek_target_at");
    assert!(
        arming.contains("self.layout_peek_eligible(tab)"),
        "the arming path asks the one predicate"
    );
    assert!(
        !arming.contains("focus"),
        "and asks nothing else: a second author here is a peek the retiring \
             path can no longer take down"
    );
}

/// RED (ticket 12, user ruling 2026-09-20) — **a plain click on the foot finds
/// the file in the files column and selects it, by the locate verb's three
/// arms.**
///
/// The press is decided by [`peek_foot_press`] and landed by [`files_locate`] —
/// the decision [`Runtime::locate_folder_in_files_column`] makes, the verb every
/// "show this in the files column" in this window already means (rule 9: the
/// existing verb, not a second one). Its range rule is the 2026-08-25 ruling
/// 「打开文件不许重根文件树」: a folder inside the column's tree keeps the root and
/// selects the file's row under it; a folder outside it re-roots the column at
/// the folder and selects the file's row there; a tab with no column gets one
/// rooted at the folder, with the file's row selected.
///
/// MUTATION: route the plain click to the reveal door (answer
/// `PeekFootPress::Reveal` for `ClickIntent::Here` in `peek_foot_press`) — the
/// first assertion goes red.
#[test]
fn a_click_on_the_foot_selects_the_file_in_the_files_column() {
    let (folder, file) = glance_fixture("locate");
    for host in [RowHost::Column(SeatId(1)), RowHost::Terminal(SeatId(2))] {
        let Some(PeekFootPress::Locate {
            folder: at,
            file: name,
        }) = peek_foot_press(host, &file, false)
        else {
            panic!("a plain click on {host:?}'s foot must stay in the window and locate");
        };
        assert_eq!(at, folder.join("notes"));
        assert_eq!(name, "plan.md");

        // ① Inside the tree the column already shows: the root stays, and the
        //    file's row under the way down is the one selected.
        let root = folder.display().to_string();
        assert_eq!(
            files_locate(Some((SeatId(7), &root)), &at, Some(&name)),
            FilesLocate::Inside {
                seat: SeatId(7),
                select: "/notes/plan.md".to_owned(),
            }
        );
        // ② Outside it: the column is rooted at the folder, and the file's row
        //    — a child of the new root — is selected.
        let elsewhere = std::env::temp_dir()
            .join("folio-glance-foot-elsewhere")
            .display()
            .to_string();
        assert_eq!(
            files_locate(Some((SeatId(7), &elsewhere)), &at, Some(&name)),
            FilesLocate::Root {
                select: Some("/plan.md".to_owned()),
            }
        );
        // ③ No column at all: one is opened there, the same answer.
        assert_eq!(
            files_locate(None, &at, Some(&name)),
            FilesLocate::Root {
                select: Some("/plan.md".to_owned()),
            }
        );
    }
    // And the folder-only callers are the verb they always were.
    let root = folder.display().to_string();
    assert_eq!(
        files_locate(Some((SeatId(7), &root)), &folder.join("notes"), None),
        FilesLocate::Inside {
            seat: SeatId(7),
            select: "/notes".to_owned(),
        }
    );
    assert_eq!(
        files_locate(None, &folder, None),
        FilesLocate::Root { select: None }
    );
    let _ = std::fs::remove_dir_all(&folder);
}

/// RED (ticket 12, owner ruling 2026-09-23) — **the files column stays where the
/// foot took it when the card goes.**
///
/// The 2026-09-20 note said 「不在当前根下就让文件列临时切过去」, and "temporarily"
/// could be read as "switch back when the card goes". The owner settled it: "The
/// files column does not switch back after the glance-foot click; it is
/// navigation, not a peek." So the press takes the card down and then locates,
/// and nothing on the card's way down touches the column: the card holds no
/// root to restore, and the door that ends its life names no files-column verb.
///
/// MUTATION: have `hide_file_peek` re-root the column at a remembered root (a
/// `reroot_files_column` or `show_folder_in_files_column` call) — the second
/// loop goes red.
#[test]
fn the_column_stays_where_the_foot_took_it_when_the_card_goes() {
    let press = method_body("Runtime", "press_file_peek_foot");
    let down = press
        .find("self.hide_file_peek();")
        .expect("the foot's press takes the card down");
    let locate = press
        .find("self.locate_folder_in_files_column(&folder, Some(&file))")
        .expect("and locates through the existing verb");
    assert!(
        down < locate,
        "the card goes before the column moves, so it is never placed against a row that moved"
    );
    for verb in [
        "reroot_files_column",
        "show_folder_in_files_column",
        "seat_a_files_column",
        ".root =",
    ] {
        assert!(
            !press.contains(verb),
            "the foot moves the column only through the locate verb — found `{verb}`"
        );
    }
    for verb in [
        "reroot_files_column",
        "show_folder_in_files_column",
        "locate_folder_in_files_column",
        ".root =",
    ] {
        assert!(
            !method_body("Runtime", "hide_file_peek").contains(verb),
            "taking the card down moves the files column (`{verb}`): it would switch back"
        );
    }
}

/// RED (ticket 12, user ruling 2026-09-20) — **`Ctrl` (`⌘`) and a click on the
/// foot reveals the file, selected, through the door the card's surface already
/// uses for its own hand-over.**
///
/// A files row, a Git row and a folder card's row reveal through
/// [`Runtime::reveal_in_explorer`], the door a row's own menu takes; a reference
/// a program printed reveals through [`Runtime::reveal_verified`], with the
/// pane's ledger — exactly as the reference itself does under `Ctrl`, so the
/// card does not become a way round audit 3 C-2's rule. The file is revealed,
/// not its folder: Explorer and Finder open the folder with the file selected.
///
/// MUTATION: answer `Reveal` for a terminal host (the files door) — the second
/// assertion goes red; drop the reveal call from `press_file_peek_foot` — the
/// body pin does.
#[test]
fn a_ctrl_click_on_the_foot_reveals_the_file() {
    let (folder, file) = glance_fixture("reveal");
    for host in [
        RowHost::Column(SeatId(1)),
        RowHost::Float(3),
        RowHost::Git(SeatId(4)),
    ] {
        assert_eq!(
            peek_foot_press(host, &file, true),
            Some(PeekFootPress::Reveal(file.clone())),
            "{host:?}"
        );
    }
    assert_eq!(
        peek_foot_press(RowHost::Terminal(SeatId(2)), &file, true),
        Some(PeekFootPress::RevealVerified(SeatId(2), file.clone())),
        "a printed reference is handed over off its ledger"
    );
    let press = method_body("Runtime", "press_file_peek_foot");
    for door in [
        "self.reveal_in_explorer(&path);",
        "let facts = self.verified_target(seat, &path);",
        "self.reveal_verified(&path, facts);",
        "input::pointer_chord_held(self.window.modifiers_held)",
    ] {
        assert!(
            press.contains(door),
            "the foot's press reaches `{door}` rather than a door of its own"
        );
    }
    let _ = std::fs::remove_dir_all(&folder);
}
