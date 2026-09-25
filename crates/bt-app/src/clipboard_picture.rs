//! **A picture on the clipboard becomes a file, and the file's path is what is
//! pasted** (GitHub issue #2, `docs/DESIGN.md` §7.61).
//!
//! A shell takes arguments, and a screenshot is not one. What a person who
//! presses `Ctrl+V` in a terminal with a screenshot on the clipboard wants is
//! what they would have got by saving that screenshot and dragging it in — a
//! path — so this module is the saving half: it turns whatever encoding the
//! clipboard offered into PNG, gives it a name a person can read, and keeps the
//! folder from growing without end. The spelling of the path is not here; it is
//! `shell_literal`'s, which is the same speller a copied *file* goes through, so
//! the two cases cannot drift apart.
//!
//! # The folder, and why it is swept
//!
//! `<the system's temporary directory>/folio/clipboard`. A pasted picture is a
//! file the reader did not ask for by name and will not think to delete, so
//! something has to, and the rule is the smallest one that is still a rule:
//! **the newest [`KEPT`] files stay and the rest go, checked on every write.**
//! Not an age — a screenshot pasted into a command that is still running has to
//! survive for as long as that command does, and no clock here knows how long
//! that is — and not "on exit", because a Folio that is killed never gets there.
//!
//! **Only Folio's own names take part.** A file in that folder whose name this
//! module cannot read is left exactly where it is: the sweep is a promise about
//! what Folio wrote, not a licence to delete what it finds.
//!
//! # What runs where
//!
//! Everything in this file is CPU and disk, and a screenshot's PNG encode is
//! tens of milliseconds — so none of it runs on the window thread. The
//! acquisition does, because both platforms' clipboards may only be read there;
//! what crosses to the worker is the bytes, and what comes back is the path.
//! See `main.rs`'s picture mailbox for that half.

use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bt_platform::{
    PictureBytes, PictureEncoding,
    clipboard::{DibHeader, TopDownPixels},
};
use image::{ImageDecoder, ImageEncoder};

/// **How many pasted pictures the folder keeps.**
///
/// Twenty is enough that the file a reader pasted this morning is still there
/// when the command they pasted it into finally asks for it, and small enough
/// that the folder never becomes a place megabytes go to be forgotten. It is a
/// number rather than a setting because the only question a setting would ask —
/// "how many screenshots do you want to keep in a temporary folder" — is one
/// nobody has an answer to.
pub const KEPT: usize = 20;

/// **The widest and tallest picture this paste will decode** (review X-4).
///
/// A device-independent bitmap is a header and then pixels, and the header is
/// believed by every decoder before the pixels are read: `image`'s own BMP
/// decoder accepts dimensions up to 65,535 a side and `DynamicImage` allocates
/// the whole output before the body is parsed, so a clipboard provider offering
/// a fifty-byte header claiming 32,768 x 32,768 asks this process for about
/// three gigabytes and gets an abort rather than an error. The header is
/// arithmetic; believing it is the defect.
///
/// **65,535, because a long screenshot is tall** (ticket 66). The number used
/// to be 16,384 — "above every display and every screenshot a person takes" —
/// and a scrolling capture of a web page or a chat is exactly the screenshot it
/// was wrong about: one screen wide and tens of thousands of rows tall, a few
/// tens of megabytes, and nowhere near [`MAX_DECODED_BYTES`], which is the
/// bound that costs memory. The side is only the bound on a *shape*: at 1,024
/// pixels across the byte ceiling already stops at 65,536 rows, so any larger
/// side would admit only slivers narrower than that. 65,535 is also the largest
/// side the `image` crate's BMP reader and a `BITMAPCOREHEADER` carry, so no
/// rung on this path is asked for a shape one of them cannot state.
pub const MAX_SIDE: u32 = 65_535;

/// **And the decoded size, which is the number that actually costs memory.**
///
/// A picture can be inside [`MAX_SIDE`] on both axes and still be enormous:
/// 16,384 square at four bytes a pixel is a gigabyte. This is the bound that
/// matters. Where a decoder states it, it is checked against the decoder's own
/// `total_bytes` — its arithmetic over the dimensions and the colour type it is
/// about to allocate for. Where the rung reads the shape itself (Windows'
/// drawing, and a bitmap's header before the BMP decoder believes it) it is
/// the allocation that rung makes, at the bytes a pixel it makes it at.
///
/// 256 MiB decodes a picture of about 67 megapixels in RGBA, which is far above
/// any screen and far below the point where a refusal is worse than an abort.
pub const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;

/// **How many bytes of one encoding are worth copying off the clipboard at
/// all.**
///
/// The acquisition copies what the source offers before anything has looked at
/// it, so the ceiling has to exist on that side too — see
/// `bt_platform::clipboard::MAX_PICTURE_BYTES`, which is this number and is
/// where it is enforced. Re-stated here as the one this module refuses at,
/// because a clipboard reader is not the only way bytes could ever arrive.
pub const MAX_ENCODED_BYTES: usize = 256 * 1024 * 1024;

/// The folder's own name inside Folio's temporary directory.
const FOLDER: &str = "clipboard";

/// The one extension this module writes and the only one it sweeps.
const EXTENSION: &str = ".png";

const SECONDS_PER_DAY: i64 = 86_400;

/// **Where the files go** — `<temp>/folio/clipboard`.
///
/// The `folio` half is `bt_platform`'s, because *which* temporary directory a
/// process is entitled to is a question about the machine: on macOS it is the
/// per-user directory the system names rather than the one `$TMPDIR` names, and
/// on Windows it is `%TEMP%`. The `clipboard` half is this module's, so that a
/// sweep with a twenty-file rule in it can never be pointed at a folder holding
/// anything else.
#[must_use]
pub fn directory() -> PathBuf {
    bt_platform::instance::temporary_directory().join(FOLDER)
}

/// **`yyyymmdd-hhmmss`, in UTC.**
///
/// **UTC and not the reader's own clock, deliberately**, and the trade is worth
/// stating because the other side of it is real: a reader in UTC+8 who pastes at
/// half past ten at night gets a file named `143012`, which is not the time on
/// their wall. What buys that is the one property the folder's rule is built out
/// of — [`plan`] decides which files the cap retires by *comparing these strings*,
/// so the name has to rise every second, for ever. A local clock does not: it
/// goes backwards an hour once a year, and for that hour the newest file sorts
/// oldest and the sweep deletes the picture that was just pasted. The same
/// choice, for the same reason, as `bt_persist`'s rejected siblings — and the
/// same one every other timestamp this product writes down is made with.
///
/// **What the other answer would cost**, said here so it can be reconsidered
/// with the price in view rather than as a preference: this workspace has no
/// date-time dependency and no time-zone door at all, so a local name needs a
/// new platform call on both arms (`GetTimeZoneInformationForYear` /
/// `localtime_r`), and the cap's order would have to stop being the name and
/// start being something recorded beside it.
///
/// The calendar is `seed`'s, which is this crate's one Gregorian calendar.
#[must_use]
pub fn stamp(at: SystemTime) -> String {
    let seconds = match at.duration_since(UNIX_EPOCH) {
        Ok(delta) => i64::try_from(delta.as_secs()).unwrap_or(i64::MAX),
        // Pre-epoch: `duration_since` reports how far back it was, so negate it.
        Err(error) => -i64::try_from(error.duration().as_secs()).unwrap_or(i64::MAX),
    };
    let days = seconds.div_euclid(SECONDS_PER_DAY);
    let rest = seconds.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = crate::seed::civil_from_days(days);
    let (hour, minute, second) = (rest / 3_600, (rest % 3_600) / 60, rest % 60);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

/// One of this module's own file names, taken apart — or `None`, which means
/// the file belongs to somebody else and the sweep must not touch it.
///
/// The index is required to be digits and nothing else: `u32`'s own parser
/// accepts a leading `+`, and a name this module never writes is a name it does
/// not own.
fn ours(name: &str) -> Option<(&str, u32)> {
    let body = name.strip_suffix(EXTENSION)?;
    let (stamp, index) = body.rsplit_once('-')?;
    let (date, time) = stamp.split_once('-')?;
    if date.len() != 8 || time.len() != 6 {
        return None;
    }
    if !date
        .bytes()
        .chain(time.bytes())
        .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((stamp, index.parse().ok()?))
}

/// **Where in this second to start looking for a free name.**
///
/// A *guess*, and saying so is the whole of review X-2. This used to be the
/// answer: the worker read the folder, computed the next suffix and wrote it.
/// Two windows pasting inside one second read the same folder, computed the
/// same suffix and wrote the same path, so one reader was handed the other
/// reader's picture and an overlapping write could leave a torn file. A
/// listing taken now is a statement about the past the instant it is taken, and
/// no amount of care here can change that.
///
/// So this only says where to *begin*; [`reserve`] is what actually takes a
/// name, and it takes it from the filesystem rather than from this.
#[must_use]
pub fn first_free_index(existing: &[String], stamp: &str) -> u32 {
    existing
        .iter()
        .filter_map(|name| ours(name))
        .filter(|(at, _)| *at == stamp)
        .map(|(_, index)| index)
        .max()
        .map_or(1, |taken| taken.saturating_add(1))
}

/// **Which of this module's own files the cap retires**, given everything in
/// the folder *including* the file just written.
///
/// Pure, and that is the point: "the twenty newest survive" is a claim about a
/// folder, and a claim about a folder that can only be checked by making one is
/// a claim that gets checked once.
///
/// * The **order** is `(stamp, index)` and not the filesystem's: a directory
///   listing has no order, and a modification time is a thing anything on the
///   machine can change.
/// * The listing is taken **after** the write, so the new file counts against
///   the cap and the folder holds `kept` files rather than `kept + 1`.
/// * A file this module did not write is neither counted nor deleted.
#[must_use]
pub fn retire(existing: &[String], kept: usize) -> Vec<String> {
    let mut mine: Vec<(&str, u32, &String)> = existing
        .iter()
        .filter_map(|name| ours(name).map(|(at, index)| (at, index, name)))
        .collect();
    // Explicit and not `sort_by_key`: the key is borrowed out of the listing, which
    // is a key a sort-by-key closure cannot hand back.
    mine.sort_by(|left, right| left.0.cmp(right.0).then(left.1.cmp(&right.1)));
    let over = mine.len().saturating_sub(kept);
    mine.into_iter()
        .take(over)
        .map(|(_, _, name)| name.clone())
        .collect()
}

/// How many names one write will try before giving up.
///
/// The loop ends because each turn takes one name that is now certainly taken,
/// so a folder can only push it round as many times as it has files in this
/// second. The bound exists so that a folder somebody has filled with
/// `<this second>-<n>.png` cannot spin the worker for ever, and it is far above
/// [`KEPT`] because the names in the way need not be ours to count.
const NAME_ATTEMPTS: u32 = 4_096;

/// **Take a name, by creating the file** (review X-2).
///
/// `create_new` is the whole fix: it is one filesystem operation that both
/// tests for the name and takes it, so two workers — in this process or in
/// another Folio, on this machine — cannot both believe a name is free. The one
/// that loses gets `AlreadyExists` and moves to the next index. Nothing here
/// trusts the listing; the listing only says where to start looking.
///
/// The file comes back **open and empty**, and the bytes go into it under the
/// name it will keep. There is no temporary name and no rename, because a
/// rename would take the name a second time and hand the race back: the
/// reservation *is* the final path. What can see the empty file in between is
/// only another Folio's own sweep, and a file stamped now is the newest in the
/// folder and is never what a cap retires.
fn reserve(folder: &Path, stamp: &str) -> Result<(PathBuf, fs::File), String> {
    let mut index = first_free_index(&names_in(folder), stamp);
    for _ in 0..NAME_ATTEMPTS {
        let path = folder.join(format!("{stamp}-{index}{EXTENSION}"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                index = index.saturating_add(1);
            }
            Err(error) => return Err(format!("the file could not be made: {error}")),
        }
    }
    Err("no name in this second was free".to_owned())
}

/// **PNG bytes out of whatever the clipboard offered**, taking the first
/// encoding that works.
///
/// The list arrives best first — see `bt_platform::PictureEncoding` — and a
/// source that advertises an encoding it then renders badly is an ordinary
/// source, so a failure here moves to the next rung rather than ending the
/// paste. The reason reported is the **last** one, which is the rung that was
/// closest to being readable.
pub fn png_bytes(offered: &[PictureBytes]) -> Result<Vec<u8>, String> {
    let mut refused = None;
    for picture in offered {
        match png_from(picture) {
            Ok(bytes) => return Ok(bytes),
            Err(reason) => refused = Some(reason),
        }
    }
    Err(refused.unwrap_or_else(|| "the clipboard offered no picture".to_owned()))
}

/// **Is a picture of this shape one this paste will decode?** (review X-4).
///
/// Pure, over the three numbers a decoder can be asked for *before* it
/// allocates anything — which is what makes the ceiling checkable without
/// building the picture that would prove it. `decoded` is the decoder's own
/// `total_bytes`, so what is bounded is the allocation that is actually about
/// to be made rather than a second guess at it.
#[must_use]
pub fn fits(width: u32, height: u32, decoded: u64) -> bool {
    width > 0
        && height > 0
        && width <= MAX_SIDE
        && height <= MAX_SIDE
        && decoded <= MAX_DECODED_BYTES
}

/// The ceiling, said once, in the words the card will carry.
fn refuse_oversize(width: u32, height: u32, decoded: u64) -> Result<(), String> {
    if fits(width, height, decoded) {
        Ok(())
    } else {
        // The shape and never the picture: these three numbers are the header's,
        // and the header is the part that was not believed.
        Err(format!(
            "the clipboard picture is {width}x{height} and would decode to \
             {decoded} bytes, which is past this paste's ceiling"
        ))
    }
}

/// What every decoder on this path is told before it is asked for pixels.
///
/// Belt beside the explicit check above rather than instead of it: `set_limits`
/// is honoured by the decoders that implement it, and `image`'s BMP decoder does
/// not — it inherits the default, which checks the dimensions and lets the
/// allocation through. The refusal that actually holds is [`refuse_oversize`].
fn decoder_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    limits
}

fn png_from(picture: &PictureBytes) -> Result<Vec<u8>, String> {
    // Before the encoding is even looked at: bytes this large are not a paste,
    // and the copy that produced them is the one the platform arm already
    // refuses (review X-4).
    if picture.bytes.len() > MAX_ENCODED_BYTES {
        return Err("the clipboard picture is too large to save".to_owned());
    }
    match picture.encoding {
        // **Handed straight through, after its header is read.** Re-encoding
        // bytes that are already PNG would cost a decode and an encode to arrive
        // at the same picture, so the bytes are kept — but they are kept only
        // once a PNG decoder has read their header and agreed.
        //
        // Eight bytes used to be the whole question, and review X-4 is right that
        // it is not one: a source that advertises PNG and renders eight bytes
        // passed, and *won*, so a `CF_DIB` sitting behind it that would have
        // decoded perfectly was never tried. Reading the header both refuses that
        // and is where this rung's ceiling is applied.
        PictureEncoding::Png => png_handed_through(&picture.bytes, |error| {
            format!("the clipboard's PNG could not be read: {error}")
        }),
        PictureEncoding::Bitmap => png_from_pixels(&picture.bytes),
        PictureEncoding::DibV5 | PictureEncoding::Dib => {
            png_from_dib(picture.encoding, &picture.bytes)
        }
        // **The system's own decoder, judged by this module's own ceiling**
        // (audit 3 B-2). There is no Rust TIFF decoder in this tree to read a
        // header off, so the shape comes back *out* of the platform arm — read
        // from the representation AppKit built out of the container, before it
        // is asked for a single pixel — and is refused here, by the one
        // `refuse_oversize` the other two arms call. The arm that decodes
        // cannot skip it, because the judgement is the argument it is called
        // with: a fourth encoding added beside these three arrives at the same
        // sentence or does not decode.
        PictureEncoding::Tiff => bt_platform::png_from_tiff(&picture.bytes, refuse_oversize),
    }
}

/// **PNG bytes that are kept as they are, once a PNG decoder has read their
/// header and the shape it states is inside the ceiling** — the registered
/// `PNG` rung, and the PNG a `BI_PNG` bitmap carries as its body.
fn png_handed_through(
    png: &[u8],
    unread: impl FnOnce(image::ImageError) -> String,
) -> Result<Vec<u8>, String> {
    let decoder = image::codecs::png::PngDecoder::new(Cursor::new(png)).map_err(unread)?;
    let (width, height) = decoder.dimensions();
    refuse_oversize(width, height, decoder.total_bytes())?;
    Ok(png.to_vec())
}

/// **Windows' own drawing of the clipboard's bitmap, written out as PNG**
/// (ticket 66).
///
/// The bytes are the one layout the Windows arm asked `GetDIBits` for —
/// `bt_platform::clipboard::TopDownPixels`: top-down rows of blue, green, red
/// and a fourth byte. Whatever flavour the source put on the board, Windows
/// has already converted it into this, so nothing here reads a source's
/// header: this is the rung that retired the `image` crate's BMP parser from
/// the ordinary paste.
///
/// **The picture is opaque.** A 32-bit `BI_RGB` bitmap's fourth byte is
/// defined as unused (`BITMAPINFOHEADER`), and the device-dependent bitmap
/// Windows makes out of a `CF_DIB` carries no alpha — reading it as alpha would
/// make an ordinary screenshot transparent wherever the source wrote a zero
/// there. A source whose picture really has transparency offers the registered
/// `PNG`, which is the rung above this one.
///
/// **The shape is judged before the one allocation this makes** (review X-4):
/// three bytes a pixel is what the PNG encoder is handed, and that is the
/// number the ceiling is told.
fn png_from_pixels(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let pixels = TopDownPixels::parse(bytes).ok_or_else(|| {
        format!(
            "the clipboard bitmap could not be read (CF_BITMAP: {} bytes that are not \
             the layout Folio asked Windows for)",
            bytes.len()
        )
    })?;
    let (width, height) = (pixels.width, pixels.height);
    refuse_oversize(width, height, u64::from(width) * u64::from(height) * 3)?;
    let mut rgb = Vec::with_capacity(pixels.bgrx.len() / 4 * 3);
    for pixel in pixels.bgrx.chunks_exact(4) {
        rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
    }
    encode_png(&rgb, width, height, image::ExtendedColorType::Rgb8)
}

/// **A Windows device-independent bitmap, read by Folio** — the last rung,
/// reached only when Windows would not draw the board's bitmap itself.
///
/// On a board that has a `CF_DIB` or `CF_DIBV5`, Windows synthesises a
/// `CF_BITMAP` and the rung above answers. What reaches this one is the bitmap
/// a screen cannot draw: a `BI_PNG` or `BI_JPEG` body — a whole PNG or JPEG
/// file behind the header, which is what some capture tools switch to for a
/// large or long capture — and those are read as the files they are. Any other
/// flavour is still handed to `image`'s BMP decoder here, as before this rung
/// had a rung above it.
///
/// **Every failure names the header** (ticket 66): the rung, the header's
/// size, bits a pixel, compression, shape and row order, and the decoder's own
/// reason — `the clipboard bitmap could not be read (CF_DIBV5: header 124,
/// 32 bpp, compression 3, 1920x1080 (bottom-up)): …` — so a report is one line
/// and not a guess among a dozen flavours.
///
/// **The dimensions are refused before any decoder believes them** (review
/// X-4), at four bytes a pixel — the most any decoder here allocates for one —
/// for a body whose shape is the header's; a PNG or JPEG body states its own
/// shape, and that one is judged instead.
fn png_from_dib(encoding: PictureEncoding, dib: &[u8]) -> Result<Vec<u8>, String> {
    let rung = if encoding == PictureEncoding::DibV5 {
        "CF_DIBV5"
    } else {
        "CF_DIB"
    };
    let header = DibHeader::parse(dib).ok_or_else(|| {
        format!(
            "the clipboard bitmap could not be read ({rung}: {} bytes with no header \
             Windows defines)",
            dib.len()
        )
    })?;
    let unread = |why: &dyn std::fmt::Display| {
        format!("the clipboard bitmap could not be read ({rung}: {header}): {why}")
    };
    if matches!(header.compression, DibHeader::PNG | DibHeader::JPEG) {
        let body = header
            .body_offset()
            .and_then(|start| dib.get(start..))
            .ok_or_else(|| unread(&"its body starts past its end"))?;
        // `biSizeImage` is the embedded file's length; a zero is read as "the
        // rest", which is where the file ends in any case.
        let body = match header.image_size {
            0 => body,
            length => body.get(..length as usize).ok_or_else(|| {
                unread(&format!(
                    "biSizeImage is {length} and {} bytes follow the header",
                    body.len()
                ))
            })?,
        };
        if header.compression == DibHeader::PNG {
            return png_handed_through(body, |error| unread(&error));
        }
        let mut decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(body))
            .map_err(|error| unread(&error))?;
        let (width, height) = decoder.dimensions();
        refuse_oversize(width, height, decoder.total_bytes())?;
        decoder
            .set_limits(decoder_limits())
            .map_err(|_| "the clipboard bitmap is past this paste's ceiling".to_owned())?;
        let picture = image::DynamicImage::from_decoder(decoder).map_err(|error| unread(&error))?;
        return encode_png(
            picture.as_bytes(),
            picture.width(),
            picture.height(),
            picture.color().into(),
        );
    }
    let (width, height) = (header.width.unsigned_abs(), header.height.unsigned_abs());
    refuse_oversize(width, height, u64::from(width) * u64::from(height) * 4)?;
    let mut decoder = image::codecs::bmp::BmpDecoder::new_without_file_header(Cursor::new(dib))
        .map_err(|error| unread(&error))?;
    decoder
        .set_limits(decoder_limits())
        .map_err(|_| "the clipboard bitmap is past this paste's ceiling".to_owned())?;
    let picture = image::DynamicImage::from_decoder(decoder).map_err(|error| unread(&error))?;
    encode_png(
        picture.as_bytes(),
        picture.width(),
        picture.height(),
        picture.color().into(),
    )
}

/// Pixels to PNG bytes, for every rung that decodes rather than hands through.
fn encode_png(
    pixels: &[u8],
    width: u32,
    height: u32,
    colour: image::ExtendedColorType,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(pixels, width, height, colour)
        .map_err(|_| "the clipboard picture could not be written as PNG".to_owned())?;
    Ok(bytes)
}

/// **The whole job the picture worker runs**: encode, make the folder, write the
/// file, sweep what the write pushed out, and answer with the path.
///
/// The sweep happens **after** the write and not before it, so an encode that
/// fails costs nothing: a paste that could not produce a file must not be the
/// reason an older one disappeared.
///
/// A file that will not delete is not an error. On Windows a file another
/// program has open cannot be removed, and the cap is housekeeping rather than a
/// promise — the next write tries again.
pub fn save(folder: &Path, offered: &[PictureBytes], at: SystemTime) -> Result<PathBuf, String> {
    let bytes = png_bytes(offered)?;
    // The system's own sentence about the failure and never the path: the folder is
    // Folio's and carries no name of the reader's, but what is written into it is
    // their screenshot, and a diagnostic is not the place for either.
    fs::create_dir_all(folder).map_err(|error| format!("the folder could not be made: {error}"))?;
    let (path, mut file) = reserve(folder, &stamp(at))?;
    file.write_all(&bytes)
        .map_err(|error| format!("the file could not be written: {error}"))?;
    // Closed before the folder is read again, so the listing the cap is taken over
    // is one this file is finished in.
    drop(file);
    // **Read again rather than reusing the listing `reserve` started from**: that
    // one is older than this write, and on a machine with two Folios on it the
    // folder has moved since. The name just taken is in this listing, which is
    // what makes it count against the cap.
    for name in retire(&names_in(folder), KEPT) {
        let _ = fs::remove_file(folder.join(name));
    }
    Ok(path)
}

/// Every name in the folder, as the sweep sees them.
///
/// A folder that cannot be read is an empty listing and not a refusal: the write
/// that follows is what decides whether this paste can happen, and it will say
/// so in its own words. A name that is not UTF-8 is not one of ours, and `ours`
/// would say so anyway — this drops it one step earlier.
fn names_in(folder: &Path) -> Vec<String> {
    let Ok(listing) = fs::read_dir(folder) else {
        return Vec::new();
    };
    listing
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    /// The eight bytes every PNG file starts with (PNG specification §5.2).
    ///
    /// A test's constant and no longer the module's: eight bytes used to be the
    /// whole of what `png_from` asked of a PNG, and review X-4 is why they are
    /// not — what production reads now is the header, through a decoder. Here
    /// they are only how a test recognises the answer it was handed.
    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    /// PIN — the name is a date a person can read, and it sorts the way the
    /// files were written.
    #[test]
    fn the_stamp_is_the_calendar_and_it_sorts() {
        assert_eq!(stamp(UNIX_EPOCH), "19700101-000000");
        // 2026-09-16T14:30:12Z.
        assert_eq!(stamp(at(1_789_569_012)), "20260916-143012");
        // A leap day, which is the case a lookup table gets wrong.
        assert_eq!(stamp(at(1_709_208_000)), "20240229-120000");
        // The shape, over inputs this test names rather than over a clock: no
        // machine's own time zone, no today, nothing this can answer differently
        // on a runner than on the machine it was written on.
        for seconds in [0, 1, 1_709_208_000, 1_789_569_012, 4_102_444_799] {
            let name = stamp(at(seconds));
            assert_eq!(name.len(), 15, "{name}");
            assert_eq!(name.as_bytes()[8], b'-', "{name}");
            assert!(
                name.bytes()
                    .enumerate()
                    .all(|(index, byte)| index == 8 || byte.is_ascii_digit()),
                "{name}"
            );
            // **One second later is one name later, in the order the sweep reads
            // them**: the cap is decided by comparing these strings, so a name
            // that did not rise would be a newer file swept before an older one.
            assert!(stamp(at(seconds)) < stamp(at(seconds + 1)), "{name}");
        }
    }

    /// PIN — **twenty files survive a write and the twenty-first pushes the
    /// oldest out**, and the count is taken over this module's own names.
    ///
    /// MUTATION: sweep before the new file is counted and the folder settles at
    /// twenty-one; order by the listing instead of by the name and the file that
    /// goes is whichever one the filesystem happened to hand over first.
    #[test]
    fn the_folder_keeps_the_newest_and_the_new_file_counts_against_the_cap() {
        let full: Vec<String> = (0..KEPT)
            .map(|index| format!("20260916-1430{index:02}-1.png"))
            .collect();
        // The sweep is taken over the folder as it stands *after* the write, so
        // the written file is in the listing it is counted in.
        let mut at_the_cap = full.clone();
        at_the_cap.push("20260916-150000-1.png".to_owned());
        assert_eq!(retire(&at_the_cap, KEPT), ["20260916-143000-1.png"]);

        // One short of the cap: the write fits and nothing is retired.
        let mut with_room: Vec<String> = full[1..].to_vec();
        with_room.push("20260916-150000-1.png".to_owned());
        assert!(retire(&with_room, KEPT).is_empty());

        // Far over the cap — a folder that was written to by an older build, or
        // by a Folio that was killed between the write and the sweep.
        let mut many = at_the_cap.clone();
        many.extend((0..5).map(|index| format!("20260915-0000{index:02}-1.png")));
        let over = retire(&many, KEPT);
        assert_eq!(over.len(), 6);
        assert!(over.contains(&"20260915-000004-1.png".to_owned()));
        assert!(over.contains(&"20260916-143000-1.png".to_owned()));
        assert!(!over.contains(&"20260916-143001-1.png".to_owned()));
    }

    /// PIN — **two pastes in one second are two files**, and the index the
    /// search starts at is the first one free rather than the count of what is
    /// there.
    #[test]
    fn a_second_paste_in_the_same_second_takes_the_next_index() {
        assert_eq!(first_free_index(&[], "20260916-143012"), 1);
        let one = vec!["20260916-143012-1.png".to_owned()];
        assert_eq!(first_free_index(&one, "20260916-143012"), 2);
        // A gap left by a file somebody deleted by hand is not filled in: the
        // index has to keep rising or the name stops sorting.
        let gap = vec![
            "20260916-143012-1.png".to_owned(),
            "20260916-143012-7.png".to_owned(),
        ];
        assert_eq!(first_free_index(&gap, "20260916-143012"), 8);
        // Another second entirely starts again at one.
        assert_eq!(first_free_index(&gap, "20260916-143013"), 1);
    }

    /// PIN — **a file this module did not write is never deleted and never
    /// counted**, however full the folder is.
    ///
    /// The folder lives in the system's temporary directory. A rule that swept
    /// "the oldest files" rather than "the oldest files of ours" would be a
    /// twenty-file cap pointed at whatever else ended up beside them.
    #[test]
    fn a_name_this_module_does_not_own_is_left_alone() {
        let strangers = [
            "notes.txt",
            "20260916-143012.png",
            "20260916-143012-1.jpg",
            "20260916-143012--1.png",
            "20260916-143012-+1.png",
            "2026916-143012-1.png",
            "20260916-14301-1.png",
            "20260916-14301x-1.png",
            "screenshot.png",
        ];
        for name in strangers {
            assert_eq!(ours(name), None, "{name} was read as one of ours");
        }
        let mut folder: Vec<String> = strangers.iter().map(|name| (*name).to_owned()).collect();
        folder.extend((0..KEPT).map(|index| format!("20260916-1430{index:02}-1.png")));
        folder.push("20260916-150000-1.png".to_owned());
        assert_eq!(retire(&folder, KEPT), ["20260916-143000-1.png"]);
        assert_eq!(ours("20260916-143012-1.png"), Some(("20260916-143012", 1)));
    }

    /// A folder of this test's own, under the machine's temporary directory.
    fn scratch(tag: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "folio-clipboard-picture-test-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a scratch folder");
        folder
    }

    /// RED GATE (review X-2) — **two writes in one second never take one
    /// name**, and neither of them is the other's picture.
    ///
    /// The old arrangement read the folder, computed the next suffix and wrote
    /// it, which is safe only for as long as one worker exists. Two windows
    /// pasting in the same second read the same folder, computed the same
    /// suffix and truncated each other. This is that scenario with the listing
    /// frozen at its worst: both writers start from the *same* stale view, which
    /// is exactly what the filesystem has to break the tie for.
    ///
    /// MUTATION: put `create(true)` back in place of `create_new(true)` and the
    /// second reservation hands back the first one's path, so the two are equal
    /// and this goes red on the first assertion.
    #[test]
    fn two_writes_in_one_second_never_reserve_the_same_name() {
        let folder = scratch("reserve");
        let stamp = "20260916-143012";
        let mut taken = Vec::new();
        for _ in 0..5 {
            let (path, file) = reserve(&folder, stamp).expect("a free name");
            drop(file);
            assert!(path.exists(), "the reservation is the file");
            taken.push(path);
        }
        for (index, path) in taken.iter().enumerate() {
            for other in &taken[index + 1..] {
                assert_ne!(path, other, "two reservations took one name");
            }
        }
        // And the names really are this module's own, in order.
        let names: Vec<String> = taken
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names[0], "20260916-143012-1.png");
        assert_eq!(names[4], "20260916-143012-5.png");

        // A name that is already standing — left by another Folio, or by a
        // build that died between the reservation and the write — is stepped
        // over rather than truncated.
        let squatter = folder.join("20260916-143013-1.png");
        fs::write(&squatter, b"not ours to overwrite").expect("a squatting file");
        let (stepped, file) = reserve(&folder, "20260916-143013").expect("a free name");
        drop(file);
        assert_ne!(stepped, squatter);
        assert_eq!(
            fs::read(&squatter).expect("the squatter survives"),
            b"not ours to overwrite"
        );
        let _ = fs::remove_dir_all(&folder);
    }

    /// **A whole write, end to end**, which is what says the reservation and
    /// the sweep still add up to the rule.
    #[test]
    fn a_saved_picture_lands_under_the_cap_with_its_bytes_in_it() {
        let folder = scratch("save");
        let mut written = Vec::new();
        for second in 0..(KEPT + 2) {
            let at = UNIX_EPOCH + Duration::from_secs(1_789_569_012 + second as u64);
            written.push(save(&folder, &[a_png()], at).expect("a picture saves"));
        }
        let left = names_in(&folder);
        assert_eq!(left.len(), KEPT, "the folder settles at the cap");
        assert!(
            !left.contains(&"20260916-143012-1.png".to_owned()),
            "the oldest two were retired"
        );
        let last = written.last().expect("a last write");
        assert_eq!(
            fs::read(last).expect("the newest file reads back"),
            a_png().bytes,
            "the bytes in the file are the bytes that were pasted"
        );
        let _ = fs::remove_dir_all(&folder);
    }

    /// RED GATE (review X-4) — **a header claiming an enormous picture is
    /// refused before anything is allocated for it.**
    ///
    /// The fixture is a header and nothing else: forty bytes saying 32,768 by
    /// 32,768 at 24 bits, with no pixels behind them at all. That is the whole
    /// attack — `DynamicImage::from_decoder` allocates the output before it
    /// reads the body, so three gigabytes are asked for and the process is
    /// aborted rather than told no. **No giant allocation is attempted here**,
    /// which is the point: the refusal arrives from the header.
    ///
    /// MUTATION: take `refuse_oversize` out of `png_from_dib` and this test
    /// stops being a test and starts being an out-of-memory probe.
    #[test]
    fn a_header_claiming_an_enormous_picture_is_refused_before_it_is_decoded() {
        assert!(fits(2, 2, 16));
        assert!(fits(MAX_SIDE, 1, MAX_DECODED_BYTES));
        assert!(!fits(MAX_SIDE + 1, 1, 16));
        assert!(!fits(1, MAX_SIDE + 1, 16));
        assert!(!fits(0, 1, 0), "a picture with no pixels is not a picture");
        assert!(
            !fits(MAX_SIDE, MAX_SIDE, MAX_DECODED_BYTES + 1),
            "inside both sides and still too much memory"
        );

        for (width, height) in [(32_768_i32, 32_768_i32), (70_000, 2), (2, 70_000)] {
            let refusal = png_bytes(&[PictureBytes {
                encoding: PictureEncoding::Dib,
                bytes: header_only_dib(width, height),
            }])
            .expect_err("a header past the ceiling is refused");
            assert!(
                refusal.contains("ceiling"),
                "the refusal says what was wrong: {refusal}"
            );
        }

        // And a header inside the ceiling is refused for the honest reason
        // instead — there are no pixels behind it — rather than being waved
        // through by the size check.
        let truncated = png_bytes(&[PictureBytes {
            encoding: PictureEncoding::Dib,
            bytes: header_only_dib(8, 8),
        }])
        .expect_err("a truncated body is still refused");
        assert!(!truncated.contains("ceiling"), "{truncated}");
    }

    /// RED — **one ceiling, and every arm of the dispatch is judged by it**
    /// (audit 3 B-2).
    ///
    /// The rule the module states for itself is about *a* clipboard picture,
    /// not about two of the three encodings it reads. The TIFF arm was the one
    /// that did not state it: there is no Rust TIFF decoder in this tree to
    /// read a header off, so the arm handed the bytes to AppKit and the
    /// system's decoder rasterized whatever the IFD claimed — a ~180-byte file
    /// claiming 65,535 square asks for 17.2 GB, and an `NSMallocException` out
    /// of AppKit is an abort in this build rather than an `Err`. The shape now
    /// comes back out of the platform arm and is judged here, by this function.
    ///
    /// Two halves, because the defect had two. The **value** half: one shape,
    /// one answer, whichever encoding claimed it — the numbers below are the
    /// three fixtures' own claims. The **source** half: that every arm of
    /// `png_from` really reaches this judge, which is the half a value cannot
    /// carry, because an arm that skips the ceiling answers nothing at all on a
    /// machine the encoding does not exist on.
    ///
    /// MUTATION: drop the `refuse_oversize` argument from the TIFF arm — the
    /// shape the defect had — and the source half names it. Widen `fits` and
    /// the value half does.
    #[test]
    fn every_encoding_is_judged_by_the_one_ceiling() {
        for (width, height, decoded) in [
            // The TIFF fixture's claim, four samples a pixel.
            (65_535_u32, 65_535_u32, 65_535_u64 * 65_535 * 4),
            // The DIB header's, at 24 bits.
            (32_768, 32_768, 32_768_u64 * 32_768 * 3),
            // And a shape inside both sides whose pixels are still too much.
            (MAX_SIDE, MAX_SIDE, MAX_DECODED_BYTES + 1),
        ] {
            assert!(!fits(width, height, decoded), "{width}x{height}");
            let refusal = refuse_oversize(width, height, decoded)
                .expect_err("a picture this process will not carry is refused");
            assert!(
                refusal.contains("ceiling") && refusal.contains(&format!("{width}x{height}")),
                "one sentence, carrying the shape and no picture: {refusal}"
            );
        }
        assert!(
            fits(1_920, 1_080, 1_920 * 1_080 * 4),
            "and an ordinary screenshot is a picture this paste decodes"
        );

        const SOURCE: &str = include_str!("clipboard_picture.rs");
        let body = |signature: &str| -> &'static str {
            let at = SOURCE
                .find(signature)
                .unwrap_or_else(|| panic!("{signature} is declared in this file"));
            let rest = &SOURCE[at + signature.len()..];
            &rest[..rest.find("\n}\n").unwrap_or(rest.len())]
        };
        let dispatch = body("fn png_from(picture: &PictureBytes) -> Result<Vec<u8>, String> {");
        // One arm per line of the match, which is how they are written — a
        // `DibV5 | Dib` arm names the type twice and is still one arm.
        let arms: Vec<&str> = dispatch
            .split("\n        PictureEncoding::")
            .skip(1)
            .collect();
        assert_eq!(
            arms.len(),
            4,
            "an encoding was added or removed:\n{dispatch}"
        );
        let delegates = ["png_handed_through", "png_from_pixels", "png_from_dib"];
        for arm in arms {
            assert!(
                arm.contains("refuse_oversize") || delegates.iter().any(|name| arm.contains(name)),
                "an encoding is decoded without its shape being judged:\n{arm}"
            );
        }
        for signature in [
            "fn png_handed_through(",
            "fn png_from_pixels(bytes: &[u8]) -> Result<Vec<u8>, String> {",
            "fn png_from_dib(encoding: PictureEncoding, dib: &[u8]) -> Result<Vec<u8>, String> {",
        ] {
            assert!(
                body(signature).contains("refuse_oversize"),
                "an arm the dispatch delegates to no longer judges the shape: {signature}"
            );
        }
    }

    /// A `BITMAPINFOHEADER` with the given shape and not one pixel behind it.
    fn header_only_dib(width: i32, height: i32) -> Vec<u8> {
        let mut dib: Vec<u8> = Vec::new();
        dib.extend(40_u32.to_le_bytes());
        dib.extend(width.to_le_bytes());
        dib.extend(height.to_le_bytes());
        dib.extend(1_u16.to_le_bytes());
        dib.extend(24_u16.to_le_bytes());
        dib.extend(0_u32.to_le_bytes());
        dib.extend(0_u32.to_le_bytes());
        dib.extend(2835_i32.to_le_bytes());
        dib.extend(2835_i32.to_le_bytes());
        dib.extend(0_u32.to_le_bytes());
        dib.extend(0_u32.to_le_bytes());
        dib
    }

    /// One real one-pixel PNG, as a clipboard would offer it.
    fn a_png() -> PictureBytes {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([1, 2, 3, 255]),
        ))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("a one-pixel PNG encodes");
        PictureBytes {
            encoding: PictureEncoding::Png,
            bytes,
        }
    }

    /// **A device-independent bitmap becomes PNG bytes** — the decode the
    /// Windows clipboard's `CF_DIB` needs, run on the CPU and on any machine.
    ///
    /// The fixture is a 2×2 24-bit bitmap written out by hand: a
    /// `BITMAPINFOHEADER` and four bottom-up BGR pixels, which is exactly what
    /// lands on the clipboard with no `BITMAPFILEHEADER` in front of it. What is
    /// checked is that the answer is a PNG and that it carries the picture back
    /// — the four colours, the right way up.
    ///
    /// MUTATION: hand the same bytes to a decoder that expects a file header and
    /// the first fourteen bytes are read as a header, which fails.
    #[test]
    fn a_small_dib_decodes_into_png_bytes() {
        let mut dib: Vec<u8> = Vec::new();
        dib.extend(40_u32.to_le_bytes()); // biSize
        dib.extend(2_i32.to_le_bytes()); // biWidth
        dib.extend(2_i32.to_le_bytes()); // biHeight, positive: bottom-up
        dib.extend(1_u16.to_le_bytes()); // biPlanes
        dib.extend(24_u16.to_le_bytes()); // biBitCount
        dib.extend(0_u32.to_le_bytes()); // biCompression: BI_RGB
        dib.extend(0_u32.to_le_bytes()); // biSizeImage
        dib.extend(2835_i32.to_le_bytes()); // biXPelsPerMeter
        dib.extend(2835_i32.to_le_bytes()); // biYPelsPerMeter
        dib.extend(0_u32.to_le_bytes()); // biClrUsed
        dib.extend(0_u32.to_le_bytes()); // biClrImportant
        // Bottom row first, BGR, each row padded to four bytes.
        dib.extend([0, 0, 255, 255, 0, 0]); // red, blue
        dib.extend([0, 0]);
        dib.extend([0, 255, 0, 255, 255, 255]); // green, white
        dib.extend([0, 0]);

        for encoding in [PictureEncoding::Dib, PictureEncoding::DibV5] {
            let png = png_bytes(&[PictureBytes {
                encoding,
                bytes: dib.clone(),
            }])
            .expect("a 2x2 bitmap decodes");
            assert!(png.starts_with(&PNG_SIGNATURE), "the answer is not a PNG");
            let back = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                .expect("the PNG this module wrote reads back")
                .to_rgba8();
            assert_eq!(back.dimensions(), (2, 2));
            // Top row of the picture is the *last* row of a bottom-up bitmap.
            assert_eq!(back.get_pixel(0, 0).0[..3], [0, 255, 0]);
            assert_eq!(back.get_pixel(1, 0).0[..3], [255, 255, 255]);
            assert_eq!(back.get_pixel(0, 1).0[..3], [255, 0, 0]);
            assert_eq!(back.get_pixel(1, 1).0[..3], [0, 0, 255]);
        }
    }

    /// The colour the synthetic bitmaps below carry at `(x, y)`, top row first:
    /// every channel differs from its neighbours, so a picture read upside
    /// down, shifted by a few bytes or with two channels swapped cannot match.
    /// Multiples of eight, so a 16-bit bitmap carries each one exactly in its
    /// five or six bits.
    fn colour(x: u32, y: u32) -> [u8; 3] {
        [
            ((x * 40 + y * 8) % 256) as u8 & 0xF8,
            ((x * 16 + y * 56 + 64) % 256) as u8 & 0xF8,
            ((x * 72 + y * 24 + 128) % 256) as u8 & 0xF8,
        ]
    }

    /// **A `CF_DIBV5` as browsers and screenshot tools write one**: a
    /// `BITMAPV5HEADER` with `BI_BITFIELDS`, its four masks — alpha included —
    /// inside the header and nothing between the header and the pixels, an
    /// sRGB colour space, and 32 or 16 bits a pixel, bottom-up or top-down.
    ///
    /// `profile` appends an embedded colour profile after the pixels and points
    /// the header's `bV5ProfileData` at it, which is the "colour-space block" a
    /// V5 header can carry.
    fn dib_v5(width: u32, height: u32, depth: u16, top_down: bool, profile: bool) -> Vec<u8> {
        let stride = (width * u32::from(depth)).div_ceil(32) * 4;
        let pixels = stride * height;
        let mut dib: Vec<u8> = Vec::new();
        dib.extend(124_u32.to_le_bytes());
        dib.extend(i32::try_from(width).unwrap().to_le_bytes());
        let rows = i32::try_from(height).unwrap();
        dib.extend((if top_down { -rows } else { rows }).to_le_bytes());
        dib.extend(1_u16.to_le_bytes());
        dib.extend(depth.to_le_bytes());
        dib.extend(3_u32.to_le_bytes()); // BI_BITFIELDS
        dib.extend(pixels.to_le_bytes());
        dib.extend(2835_i32.to_le_bytes());
        dib.extend(2835_i32.to_le_bytes());
        dib.extend(0_u32.to_le_bytes());
        dib.extend(0_u32.to_le_bytes());
        let masks: [u32; 4] = if depth == 32 {
            [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000]
        } else {
            [0xF800, 0x07E0, 0x001F, 0]
        };
        for mask in masks {
            dib.extend(mask.to_le_bytes());
        }
        // bV5CSType: `PROFILE_EMBEDDED` ('MBED') or `LCS_sRGB` ('sRGB').
        dib.extend(
            (if profile {
                0x4D42_4544_u32
            } else {
                0x7352_4742_u32
            })
            .to_le_bytes(),
        );
        dib.extend([0; 36]); // endpoints
        dib.extend([0; 12]); // gamma
        dib.extend(4_u32.to_le_bytes()); // LCS_GM_IMAGES
        let profile_bytes: &[u8] = if profile { &[0x5A; 128] } else { &[] };
        dib.extend((if profile { 124 + pixels } else { 0 }).to_le_bytes());
        dib.extend(u32::try_from(profile_bytes.len()).unwrap().to_le_bytes());
        dib.extend(0_u32.to_le_bytes());
        assert_eq!(dib.len(), 124);
        for row in 0..height {
            let y = if top_down { row } else { height - 1 - row };
            let start = dib.len();
            for x in 0..width {
                let [red, green, blue] = colour(x, y);
                if depth == 32 {
                    dib.extend([blue, green, red, 255]);
                } else {
                    let packed = (u16::from(red >> 3) << 11)
                        | (u16::from(green >> 2) << 5)
                        | u16::from(blue >> 3);
                    dib.extend(packed.to_le_bytes());
                }
            }
            dib.resize(start + stride as usize, 0);
        }
        dib.extend(profile_bytes);
        dib
    }

    /// What the Windows arm hands the worker for a picture Windows drew:
    /// Folio's one layout, built here from the same colours.
    fn top_down(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = TopDownPixels::header(width, height).to_vec();
        for y in 0..height {
            for x in 0..width {
                let [red, green, blue] = colour(x, y);
                // The fourth byte is `BI_RGB`'s unused one: zero, as a
                // device-dependent bitmap hands it back.
                bytes.extend([blue, green, red, 0]);
            }
        }
        bytes
    }

    /// The PNG a rung answered, read back and compared with [`colour`] pixel by
    /// pixel — opaque, the right way up, every channel in its place.
    fn assert_the_picture(png: &[u8], width: u32, height: u32, tolerance: u8, what: &str) {
        assert!(
            png.starts_with(&PNG_SIGNATURE),
            "{what}: the answer is not a PNG"
        );
        let back = image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .expect("the PNG this module wrote reads back")
            .to_rgba8();
        assert_eq!(back.dimensions(), (width, height), "{what}");
        for (x, y, pixel) in back.enumerate_pixels() {
            let want = colour(x, y);
            for channel in 0..3 {
                assert!(
                    pixel.0[channel].abs_diff(want[channel]) <= tolerance,
                    "{what}: ({x}, {y}) is {:?}, not {want:?}",
                    pixel.0
                );
            }
            assert_eq!(pixel.0[3], 255, "{what}: ({x}, {y}) is not opaque");
        }
    }

    /// Windows' conversion of a `CF_DIB`, through the seam, or `None` on a
    /// machine that has no GDI — where `PictureEncoding::Bitmap` is never
    /// produced, so there is nothing for these rows to say.
    fn drawn_by_windows(dib: &[u8]) -> Option<PictureBytes> {
        if bt_platform::host_platform() != bt_platform::HostPlatform::Windows {
            return None;
        }
        let bytes = bt_platform::pixels_through_gdi(dib).expect("GDI draws this bitmap");
        Some(PictureBytes {
            encoding: PictureEncoding::Bitmap,
            bytes,
        })
    }

    /// RED (66) — **A 32-bpp BI_BITFIELDS bitmap with alpha masks on the
    /// clipboard is pasted as a PNG with its pixels intact.**
    ///
    /// The owner's report (next94): a picture on the clipboard, and the paste
    /// said "the clipboard bitmap could not be read" and nothing else. A V5
    /// header with `BI_BITFIELDS` is the flavour browsers and capture tools
    /// write, and the `image` crate's BMP decoder skips twelve bytes of masks
    /// after a V5 header that carries them inside itself — so the body is read
    /// twelve bytes late and ends twelve bytes early. The paste now asks
    /// Windows for the pixels (`CF_BITMAP` through `GetDIBits`), and this is
    /// that road end to end: generated bytes, GDI's own conversion, Folio's
    /// layout, the PNG, and every pixel compared. The same rows go through
    /// the portable half — the layout the Windows arm produces — on every
    /// machine.
    ///
    /// MUTATION: hand the V5 bytes to `png_bytes` as `PictureEncoding::DibV5`
    /// — the old decoder — instead of through the seam, and this is red.
    #[test]
    fn a_bitfields_bitmap_with_an_alpha_mask_is_pasted_with_its_pixels_intact() {
        let (width, height) = (7, 5);
        let dib = dib_v5(width, height, 32, false, false);
        if let Some(drawn) = drawn_by_windows(&dib) {
            let png = png_bytes(&[drawn]).expect("Windows' drawing of a V5 bitmap pastes");
            assert_the_picture(&png, width, height, 0, "V5 BI_BITFIELDS through GDI");
        }
        let png = png_bytes(&[PictureBytes {
            encoding: PictureEncoding::Bitmap,
            bytes: top_down(width, height),
        }])
        .expect("the layout the Windows arm hands over pastes");
        assert_the_picture(&png, width, height, 0, "the Windows arm's layout");
    }

    /// RED (66) — **A top-down bitmap and a 16-bpp one are pasted.**
    ///
    /// The other two flavours the brief names, and a third the coordinator
    /// added: a negative height (top-down rows), 16 bits a pixel with 5-6-5
    /// masks, and a V5 header whose colour space is an embedded profile placed
    /// after the pixels. Each goes through GDI and comes back as the one
    /// layout; the 16-bit one is compared within the eight levels five bits
    /// can hold, which is Windows' rounding and not Folio's.
    ///
    /// MUTATION: hand any of them to `png_bytes` as `PictureEncoding::DibV5`
    /// — the old decoder — and it is red.
    #[test]
    fn a_top_down_bitmap_and_a_sixteen_bit_one_are_pasted() {
        for (depth, top_down, profile, tolerance, what) in [
            (32, true, false, 0, "top-down 32-bit"),
            (16, false, false, 8, "16-bit 5-6-5"),
            (16, true, false, 8, "top-down 16-bit"),
            (32, false, true, 0, "V5 with an embedded profile"),
        ] {
            let (width, height) = (9, 6);
            let dib = dib_v5(width, height, depth, top_down, profile);
            let Some(drawn) = drawn_by_windows(&dib) else {
                return;
            };
            let png = png_bytes(&[drawn]).unwrap_or_else(|reason| panic!("{what}: {reason}"));
            assert_the_picture(&png, width, height, tolerance, what);
        }
    }

    /// RED (66) — **A bitmap that cannot be read names its header in the
    /// diagnostics line.**
    ///
    /// The line the owner's report carried was "the clipboard bitmap could not
    /// be read", twice, and it could not say which of a dozen flavours it was
    /// about. The reason `save` returns is the one `adopt_clipboard_picture`
    /// writes into the diagnostics line, so it is what is asserted: the rung,
    /// the header's size, bits a pixel, compression, shape and row order, and
    /// the decoder's own words after them.
    ///
    /// MUTATION: put the one-sentence error back in `png_from_dib` and every
    /// row here is red.
    #[test]
    fn a_bitmap_that_cannot_be_read_names_its_header() {
        // CMYK (`BI_CMYK` = 11), which neither Windows' screen nor the decoder
        // draws: this is the last rung's own failure.
        let mut cmyk = dib_v5(4, 3, 32, false, false);
        cmyk[16..20].copy_from_slice(&11_u32.to_le_bytes());
        let reason = png_bytes(&[PictureBytes {
            encoding: PictureEncoding::DibV5,
            bytes: cmyk,
        }])
        .expect_err("a CMYK bitmap is not a picture this paste reads");
        assert!(
            reason.contains("CF_DIBV5: header 124, 32 bpp, compression 11, 4x3 (bottom-up)"),
            "{reason}"
        );
        // A top-down `CF_DIB` that stops half way through its pixels.
        let mut short = header_only_dib(6, -4);
        short.extend([0; 20]);
        let reason = png_bytes(&[PictureBytes {
            encoding: PictureEncoding::Dib,
            bytes: short,
        }])
        .expect_err("a bitmap missing its pixels is refused");
        assert!(
            reason.contains("CF_DIB: header 40, 24 bpp, compression 0, 6x-4 (top-down)"),
            "{reason}"
        );
        // Bytes with no header Windows defines still say which rung and how many.
        let reason = png_bytes(&[PictureBytes {
            encoding: PictureEncoding::Dib,
            bytes: vec![7; 30],
        }])
        .expect_err("thirty bytes are not a bitmap");
        assert!(reason.contains("CF_DIB: 30 bytes"), "{reason}");
        // And the layout the Windows arm produces, when it is not that layout.
        let reason = png_bytes(&[PictureBytes {
            encoding: PictureEncoding::Bitmap,
            bytes: vec![0; 44],
        }])
        .expect_err("not Folio's layout");
        assert!(reason.contains("CF_BITMAP: 44 bytes"), "{reason}");
    }

    /// RED (66) — **A bitmap whose body is a PNG file (`BI_PNG`) is pasted as
    /// that PNG**, and one whose body is a JPEG (`BI_JPEG`) as the picture the
    /// JPEG holds.
    ///
    /// Capture tools switch to a compressed body for a large or long capture,
    /// and a screen cannot draw one — Windows makes no `CF_BITMAP` of it — so
    /// this is the rung that meets it, and the `image` crate's BMP decoder
    /// refuses both compressions outright. The embedded file is the picture:
    /// the PNG is handed through after its header is read and judged, and the
    /// JPEG is decoded and written out as PNG.
    ///
    /// MUTATION: send the `BI_PNG` and `BI_JPEG` bodies to the BMP decoder
    /// like every other compression, and both rows are red.
    #[test]
    fn a_bitmap_whose_body_is_a_png_or_a_jpeg_is_pasted() {
        let (width, height) = (12_u32, 20_u32);
        let picture = image::RgbImage::from_fn(width, height, |x, y| image::Rgb(colour(x, y)));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(picture.clone())
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("a PNG encodes");
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(picture)
            .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .expect("a JPEG encodes");
        let wrap = |compression: u32, body: &[u8]| {
            let mut dib: Vec<u8> = Vec::new();
            dib.extend(40_u32.to_le_bytes());
            dib.extend(i32::try_from(width).unwrap().to_le_bytes());
            dib.extend(i32::try_from(height).unwrap().to_le_bytes());
            dib.extend(1_u16.to_le_bytes());
            dib.extend(0_u16.to_le_bytes()); // biBitCount: 0, the body says
            dib.extend(compression.to_le_bytes());
            dib.extend(u32::try_from(body.len()).unwrap().to_le_bytes());
            dib.extend([0; 16]);
            dib.extend_from_slice(body);
            dib
        };
        for encoding in [PictureEncoding::Dib, PictureEncoding::DibV5] {
            let answer = png_bytes(&[PictureBytes {
                encoding,
                bytes: wrap(DibHeader::PNG, &png),
            }])
            .expect("a BI_PNG bitmap pastes");
            assert_eq!(answer, png, "the embedded PNG is the file written");
            assert_the_picture(&answer, width, height, 0, "BI_PNG");

            let answer = png_bytes(&[PictureBytes {
                encoding,
                bytes: wrap(DibHeader::JPEG, &jpeg),
            }])
            .expect("a BI_JPEG bitmap pastes");
            let back = image::load_from_memory_with_format(&answer, image::ImageFormat::Png)
                .expect("the PNG written for a JPEG reads back");
            assert_eq!((back.width(), back.height()), (width, height));
        }
    }

    /// RED (66) — **A long screenshot — a thousand pixels across and twenty
    /// thousand down — is pasted, and a picture past the ceiling is refused in
    /// words that are not "could not be read".**
    ///
    /// The owner's failing picture was a long capture from a screenshot tool.
    /// 16,384 rows was this module's side ceiling, and 20,000 rows at a
    /// thousand across is 80 MB — far inside the byte ceiling, which is the
    /// bound that costs memory. Both roads a long capture takes are here: the
    /// layout Windows' drawing arrives in, and a 24-bit `CF_DIB` read by the
    /// last rung. A picture that really is past the ceiling says so, with its
    /// shape, and not in the sentence a broken bitmap gets.
    ///
    /// MUTATION: put `MAX_SIDE` back to 16,384 and both pastes are refused.
    #[test]
    fn a_long_screenshot_is_pasted_and_an_oversized_one_says_why() {
        let (width, height) = (1_000_u32, 20_000_u32);
        let png = png_bytes(&[PictureBytes {
            encoding: PictureEncoding::Bitmap,
            bytes: top_down(width, height),
        }])
        .expect("a long capture in the Windows arm's layout pastes");
        assert_the_picture(&png, width, height, 0, "1000x20000 through CF_BITMAP");

        let mut dib = header_only_dib(
            i32::try_from(width).unwrap(),
            i32::try_from(height).unwrap(),
        );
        for row in 0..height {
            let y = height - 1 - row;
            for x in 0..width {
                let [red, green, blue] = colour(x, y);
                dib.extend([blue, green, red]);
            }
        }
        let png = png_bytes(&[PictureBytes {
            encoding: PictureEncoding::Dib,
            bytes: dib,
        }])
        .expect("a long 24-bit CF_DIB pastes");
        assert_the_picture(&png, width, height, 0, "1000x20000 through CF_DIB");

        let refusal = png_bytes(&[PictureBytes {
            encoding: PictureEncoding::Dib,
            bytes: header_only_dib(1_000, 70_000),
        }])
        .expect_err("a picture past the side ceiling is refused");
        assert!(
            refusal.contains("1000x70000") && refusal.contains("ceiling"),
            "{refusal}"
        );
        assert!(!refusal.contains("could not be read"), "{refusal}");
    }

    /// PIN — **the rungs are tried in order and a bad one does not end the
    /// paste**, and bytes that only claim to be PNG are refused.
    #[test]
    fn a_refused_encoding_falls_to_the_next_and_a_false_png_is_not_written() {
        let liar = PictureBytes {
            encoding: PictureEncoding::Png,
            bytes: b"<html>not a picture</html>".to_vec(),
        };
        assert!(png_bytes(std::slice::from_ref(&liar)).is_err());
        assert!(png_bytes(&[]).is_err());

        // **Eight bytes are not a PNG** (review X-4's last paragraph). This used
        // to pass, and passing is worse than failing: a source that advertises
        // PNG and renders only the signature won the rung outright, so the
        // `CF_DIB` behind it — which would have decoded perfectly — was never
        // tried, and the file written was eight bytes long.
        let signature = PictureBytes {
            encoding: PictureEncoding::Png,
            bytes: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        };
        assert!(png_bytes(std::slice::from_ref(&signature)).is_err());

        let real = a_png();
        // The liar first: the second rung is what the paste ends up with.
        let chosen = png_bytes(&[liar, real.clone()]).expect("the second rung answers");
        assert_eq!(
            chosen, real.bytes,
            "an encoding that works is handed through"
        );
    }
}
