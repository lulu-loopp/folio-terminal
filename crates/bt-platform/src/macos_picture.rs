//! **The one picture encoding this platform offers that no Rust decoder in this
//! tree can read** (§7.61).
//!
//! `public.tiff` is what an application that copies a picture through AppKit —
//! Preview, Safari, the Finder — puts on the pasteboard, and often the only
//! thing it puts there. The workspace's `image` crate is built without its TIFF
//! feature and turning that feature on is a package this lock file does not
//! have, so the decoder used here is the system's own: `NSBitmapImageRep` reads
//! the data and writes it back out as PNG, which is one object and two messages.
//!
//! # Why this is not on the window thread
//!
//! It is the encode, and a screenshot's encode is tens of milliseconds. The
//! caller is `bt_app`'s picture worker, and this file's claim to be callable
//! from it is `macos_player.rs`'s, in the same words: the *Thread Safety
//! Summary* in Apple's Cocoa Multithreading Programming Guide does not name
//! `NSBitmapImageRep` among the classes that belong to the main thread, and says
//! of everything it does not name that "in most cases, you can use these classes
//! from any thread as long as you use them from only one thread at a time". One
//! thread at a time is exactly what this is: the representation is born inside
//! this call, never leaves it, and dies in it.
//!
//! The pool is this function's own for the same reason the player's pump has
//! one per turn: everything AppKit autoreleases on a thread `bt_app` started
//! would otherwise be held until that thread ends, and the thread ends when the
//! process does.

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep};
use objc2_foundation::{NSData, NSDictionary, NSString};

/// **TIFF bytes in, PNG bytes out** — or a sentence saying which half refused.
///
/// The reasons carry no picture and no name: a clipboard diagnostic that
/// repeated what was on the clipboard would be this process writing the
/// reader's own screenshot into a log.
///
/// # The shape is shown before the picture is decoded (audit 3 B-2)
///
/// `judge` is the caller's ceiling — the same one the PNG and the DIB arms
/// apply to their own decoders' `dimensions()` and `total_bytes()` — and it is
/// asked here rather than left to the caller because this is the only place the
/// shape exists before the pixels do. It is handed the width, the height and
/// the byte count the rasterization is about to ask the allocator for, and a
/// refusal returns without the rasterization happening at all.
///
/// **Why these three reads and not a decode.** `+[NSBitmapImageRep
/// imageRepWithData:]` reads the container — it answers `nil` for data it
/// cannot interpret, which is the refusal this function already stood on — and
/// `pixelsWide`, `pixelsHigh` and `bitsPerPixel` are properties read off what
/// it parsed. The pixels are asked for by `-representationUsingType:properties:`,
/// one line below. So the claimed shape is knowable at a point where nothing
/// the size of the picture has been allocated, which is the same moment
/// `image`'s decoders offer and the same moment the other two arms take.
///
/// **What a malformed file can do here, and what contains it.** A raise out of
/// AppKit is an abort in this build — `objc2` is used without `catch-all`, so
/// an `NSMallocException` unwinding through these `extern "C"` frames does not
/// become an `Err` — which is precisely why nothing below this line may be
/// reached on a claim this process would not carry. The containment for the
/// header read is that it is a header read, and that claim was measured rather
/// than assumed: on the Mac mini (macOS 26.6.2) a two-hundred-byte TIFF whose
/// IFD says 65,535 square returns from `imageRepWithData:` in milliseconds at
/// flat memory, and states **0 × 0** — AppKit checks the strip byte count
/// against the claim and does not honour it. A picture whose bytes really are
/// all there states its real shape, and that is the one this ceiling turns
/// away. Both rows are `macos_picture::tests`. If a future macOS were to
/// allocate for the claim inside that call, this ceiling would not be the door
/// that holds, and those two rows are what would say so.
pub fn png_from_tiff(
    tiff: &[u8],
    judge: impl FnOnce(u32, u32, u64) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    autoreleasepool(|_pool| {
        let data = NSData::with_bytes(tiff);
        let representation = NSBitmapImageRep::imageRepWithData(&data).ok_or_else(|| {
            "the clipboard picture is not a picture this machine reads".to_owned()
        })?;
        // **The claim, before the pixels.** The three reads are `isize` on this
        // platform and a negative or absurd one is a claim this process will not
        // carry either, so they are saturated into the caller's own numbers
        // rather than cast — a width that came back as -1 must not arrive at the
        // ceiling as 4,294,967,295 and be refused for the wrong reason, nor as 0
        // and be believed.
        let width = shape_number(representation.pixelsWide());
        let height = shape_number(representation.pixelsHigh());
        // The bytes a rasterization of this claim asks for. `bitsPerPixel` is
        // what the representation says one pixel costs once it is drawn, which
        // is the number the allocation is made from; a representation that
        // answers nothing useful for it is judged at one byte a pixel, and the
        // side ceilings are what hold such a claim.
        let bits = shape_number(representation.bitsPerPixel());
        let bytes_per_pixel = u64::from(bits.div_ceil(8)).max(1);
        let decoded = u64::from(width)
            .saturating_mul(u64::from(height))
            .saturating_mul(bytes_per_pixel);
        judge(width, height, decoded)?;
        // An empty properties dictionary asks for the format's own defaults, which for
        // PNG is lossless and uninterlaced. There is no quality to choose.
        let properties: Retained<NSDictionary<NSString, AnyObject>> =
            // SAFETY: the dictionary is empty, so the generic parameters it is being
            // given describe nothing that is in it.
            unsafe { Retained::cast_unchecked(NSDictionary::<AnyObject, AnyObject>::new()) };
        // SAFETY: the keys this dictionary would hold are `NSString`s, which is the
        // type the parameter is declared in; it is empty, and it outlives the call.
        let png = unsafe {
            representation
                .representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
        }
        .ok_or_else(|| "the clipboard picture could not be written as PNG".to_owned())?;
        let bytes = png.to_vec();
        if bytes.is_empty() {
            return Err("the clipboard picture encoded to nothing".to_owned());
        }
        Ok(bytes)
    })
}

/// **One of AppKit's shape numbers as a number this process can reason about.**
///
/// `pixelsWide` and its neighbours are `NSInteger`, which is signed and which
/// `NSBitmapImageRep` answers `-1` from for a representation that does not know.
/// A cast would turn that into four billion and a `0` would be believed; both
/// are answers the ceiling would read as something other than "this is not a
/// shape". Clamped into `u32`, where zero is refused by [`crate::clipboard`]'s
/// caller and anything past a real picture's size is refused by the ceiling.
fn shape_number(value: isize) -> u32 {
    u32::try_from(value).unwrap_or(if value < 0 { 0 } else { u32::MAX })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A TIFF that is all claim and no picture**: a little over two hundred
    /// bytes whose IFD says `width` by `height` at three samples of eight bits,
    /// with one strip that names as many bytes as `strip` has in it.
    ///
    /// Built here rather than committed as a file because it is arithmetic — a
    /// header, eleven entries, two little arrays and a strip. Nothing in it is
    /// a picture, so nothing in it is anybody's.
    ///
    /// At 65,535 square this is the reproduction: 65,535 × 65,535 × 3 is 12.9
    /// GB, and the file that claims it is smaller than this comment.
    fn a_tiff_claiming(width: u32, height: u32, strip: usize) -> Vec<u8> {
        const HEADER: usize = 8;
        const ENTRIES: usize = 11;
        // Header, the entry count, the entries, the next-IFD pointer, then the
        // two values too long to live inside an entry's four bytes.
        let after_ifd = HEADER + 2 + ENTRIES * 12 + 4;
        let bits_at = u32::try_from(after_ifd).expect("a small offset");
        let resolution_at = bits_at + 6;
        let strip_at = resolution_at + 8;

        let mut bytes = Vec::with_capacity(strip_at as usize + strip);
        bytes.extend_from_slice(b"II\x2a\x00"); // little-endian, and the answer
        bytes.extend_from_slice(&u32::try_from(HEADER).expect("8").to_le_bytes());
        bytes.extend_from_slice(&u16::try_from(ENTRIES).expect("eleven").to_le_bytes());

        // (tag, type, count, value-or-offset). SHORT is 3, LONG is 4,
        // RATIONAL is 5; a value of four bytes or fewer lives in the field.
        let entries: [(u16, u16, u32, u32); ENTRIES] = [
            (0x00fe, 4, 1, 0),                                  // NewSubfileType
            (0x0100, 4, 1, width),                              // ImageWidth
            (0x0101, 4, 1, height),                             // ImageLength
            (0x0102, 3, 3, bits_at),                            // BitsPerSample
            (0x0103, 3, 1, 1),                                  // Compression: none
            (0x0106, 3, 1, 2),                                  // Photometric: RGB
            (0x0111, 4, 1, strip_at),                           // StripOffsets
            (0x0115, 3, 1, 3),                                  // SamplesPerPixel
            (0x0116, 4, 1, height),                             // RowsPerStrip
            (0x0117, 4, 1, u32::try_from(strip).expect("few")), // StripByteCounts
            (0x011c, 3, 1, 1),                                  // PlanarConfiguration
        ];
        for (tag, kind, count, value) in entries {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            // A SHORT of count one occupies the first two bytes of the field and
            // the rest is padding, which writing the whole `u32` produces.
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes()); // no second IFD
        for _ in 0..3 {
            bytes.extend_from_slice(&8u16.to_le_bytes()); // BitsPerSample = 8,8,8
        }
        bytes.extend_from_slice(&72u32.to_le_bytes()); // an unused RATIONAL's room
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.resize(strip_at as usize, 0);
        bytes.resize(strip_at as usize + strip, 0);
        bytes
    }

    /// What one call told the ceiling, and what it answered.
    struct Judged {
        /// The shape the ceiling was shown, or `None` if it was never asked —
        /// which is the failure this whole section exists to make visible.
        seen: Option<(u32, u32, u64)>,
        answer: Result<Vec<u8>, String>,
    }

    /// **Ask the door with a ceiling that records and refuses.**
    ///
    /// A judge that says no to everything makes the *order* observable without
    /// a real ceiling in this crate: a refusal in the judge's own words can
    /// only come back if the judge was asked, and no PNG can come back at all
    /// if the drawing is on the far side of it.
    fn judged(tiff: &[u8]) -> Judged {
        let seen = std::cell::Cell::new(None);
        let answer = png_from_tiff(tiff, |width, height, decoded| {
            seen.set(Some((width, height, decoded)));
            Err(format!("{width}x{height} is {decoded} bytes, refused here"))
        });
        Judged {
            seen: seen.get(),
            answer,
        }
    }

    /// RED — **a TIFF that claims a shape this process will not carry is
    /// refused before anything is decoded for it** (audit 3 B-2).
    ///
    /// The whole of the defect in one value: a two-hundred-byte file, under
    /// every ceiling on the road to here, whose IFD claims 65,535 square. The
    /// PNG and the DIB arms refuse the identical claim off their decoders'
    /// headers; this arm handed it to AppKit, and an `NSMallocException` out of
    /// `representationUsingType:` is an abort rather than an `Err` in this
    /// build.
    ///
    /// The judge here is the caller's ceiling said as a value, so what is
    /// asserted is the *order*: the refusal is the judge's own sentence, which
    /// only exists if the judge was asked, and it is asked before the
    /// rasterization.
    ///
    /// MUTATION: move the `judge` call below `representationUsingType_properties`
    /// — or delete it — and this test stops seeing its sentence (and, on a
    /// machine that honours the claim, takes the process with it).
    #[test]
    fn a_tiff_that_claims_a_huge_shape_is_refused_before_it_is_decoded() {
        // Two hundred and some bytes, well under every ceiling on the road to
        // here, claiming 65,535 square — 12.9 GB once it is drawn.
        let tiff = a_tiff_claiming(65_535, 65_535, 24);
        assert!(tiff.len() < 1_024, "the claim is the file: {}", tiff.len());

        let Judged { seen, answer } = judged(&tiff);
        let (width, height, _) = seen
            .expect("the judge was asked, which is the whole of the order this ticket is about");
        // **Measured on the Mac mini, macOS 26.6.2 — not reasoned about.**
        // AppKit checks the strip byte count against the claim, so
        // `imageRepWithData:` answers a representation that states **0 x 0**
        // for this file rather than honouring the IFD's 65,535. That is a
        // refusal too, and the ceiling's first clause — a picture with no
        // pixels is not a picture — is what turns it into one. The row is
        // written as the two answers this door may get, because which of them
        // arrives is Apple's and may move: what this asserts is that in either
        // case the judge is asked and nothing is drawn.
        assert!(
            (width, height) == (0, 0) || (width, height) == (65_535, 65_535),
            "this machine states {width}x{height} for a file claiming 65,535 square"
        );
        let refusal = answer.expect_err("nothing this large is decoded");
        assert!(
            refusal.contains("refused here"),
            "the refusal is the judge's own, so it came before the drawing: {refusal}"
        );
    }

    /// RED — **a real picture past the ceiling is refused before it is drawn**
    /// (audit 3 B-2).
    ///
    /// The reachable half of the finding, and the one the crafted fixture above
    /// cannot show on this macOS: not a malformed claim but an honest TIFF
    /// whose shape is simply larger than this paste will carry — the shape a
    /// scanner's output or a stitched screenshot has, copied out of Preview.
    /// Every byte it claims is present, so AppKit states its real dimensions,
    /// and it is the ceiling and nothing else that stops the drawing. About a
    /// megabyte and a half on disk, because one side past `MAX_SIDE` (65,535
    /// since ticket 66) is enough to be past the ceiling — which is also why
    /// the file in this test is small enough to build.
    ///
    /// MUTATION: delete the `judge` call and this decodes instead of refusing.
    #[test]
    fn a_real_tiff_past_the_ceiling_is_refused_before_it_is_drawn() {
        // 65,536 is one past `bt_app::clipboard_picture::MAX_SIDE`.
        let wide = 65_536_u32;
        let tiff = a_tiff_claiming(wide, 8, wide as usize * 8 * 3);
        let Judged { seen, answer } = judged(&tiff);
        assert_eq!(
            seen.map(|(width, height, _)| (width, height)),
            Some((wide, 8)),
            "a picture whose bytes are all there states its own shape"
        );
        let decoded = seen.expect("the judge was asked").2;
        assert!(
            decoded >= u64::from(wide) * 8 * 3,
            "the number handed over is the drawing's: {decoded}"
        );
        assert!(
            answer.is_err(),
            "a shape past the ceiling is not drawn: {} bytes came back",
            answer.map(|bytes| bytes.len()).unwrap_or(0)
        );
    }

    /// RED — **the ceiling is asked about every TIFF, not only an absurd one**
    /// (audit 3 B-2).
    ///
    /// The order is the rule and a small picture is where it is cheapest to
    /// see: a real 4 × 4 RGB TIFF whose pixels are all there, a judge that
    /// refuses it anyway, and no PNG comes back. Without this, a fix that only
    /// refused implausible shapes would still be a decode that happens first.
    ///
    /// MUTATION: move the `judge` call below `representationUsingType_properties`
    /// and the answer becomes `Ok`.
    #[test]
    fn the_ceiling_is_asked_before_an_ordinary_picture_is_drawn() {
        let tiff = a_tiff_claiming(4, 4, 4 * 4 * 3);
        let Judged { seen, answer } = judged(&tiff);
        assert_eq!(
            seen.map(|(width, height, _)| (width, height)),
            Some((4, 4)),
            "a picture this process would carry is still shown to the ceiling first"
        );
        assert!(
            answer.is_err(),
            "the judge refused, so no PNG was written: {} bytes",
            answer.map(|bytes| bytes.len()).unwrap_or(0)
        );

        // And the same picture with a ceiling that says yes really is decoded,
        // so the refusal above is the judge's and not a broken fixture.
        let png = png_from_tiff(&tiff, |_, _, _| Ok(())).expect("a 4x4 TIFF is a picture");
        assert_eq!(
            &png[..8],
            b"\x89PNG\r\n\x1a\n",
            "what comes back is PNG bytes"
        );
    }

    /// PIN — **a shape AppKit does not know is not a shape.**
    ///
    /// `NSInteger` is signed and `-1` is the answer for a representation that
    /// cannot say; a cast would hand the ceiling four billion and a clamp to
    /// zero is what the ceiling refuses by its own first clause.
    #[test]
    fn a_shape_appkit_does_not_know_is_zero_and_not_four_billion() {
        assert_eq!(shape_number(-1), 0);
        assert_eq!(shape_number(0), 0);
        assert_eq!(shape_number(1_920), 1_920);
        assert_eq!(shape_number(isize::MAX), u32::MAX);
    }
}
