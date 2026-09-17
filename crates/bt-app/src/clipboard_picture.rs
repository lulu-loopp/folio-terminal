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

use bt_platform::{PictureBytes, PictureEncoding};
use image::ImageDecoder;

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
/// 16,384 is above every display and every screenshot a person takes — an
/// 8K screen is 7,680 across — and it is the number a real picture stays under.
pub const MAX_SIDE: u32 = 16_384;

/// **And the decoded size, which is the number that actually costs memory.**
///
/// A picture can be inside [`MAX_SIDE`] on both axes and still be enormous:
/// 16,384 square at four bytes a pixel is a gigabyte. This is the bound that
/// matters, and it is checked against the decoder's own `total_bytes` — its
/// arithmetic over the dimensions and the colour type it is about to allocate
/// for — rather than against a number this module derives a second time.
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
        PictureEncoding::Png => {
            let decoder = image::codecs::png::PngDecoder::new(Cursor::new(&picture.bytes))
                .map_err(|_| "the clipboard's PNG could not be read".to_owned())?;
            let (width, height) = decoder.dimensions();
            refuse_oversize(width, height, decoder.total_bytes())?;
            Ok(picture.bytes.clone())
        }
        PictureEncoding::DibV5 | PictureEncoding::Dib => png_from_dib(&picture.bytes),
        PictureEncoding::Tiff => bt_platform::png_from_tiff(&picture.bytes),
    }
}

/// **A Windows device-independent bitmap, which is a BMP with its file header
/// cut off.**
///
/// `new_without_file_header` is the decoder's own entry point for exactly this —
/// its documentation names `CF_DIB` — so the fourteen bytes are not invented
/// here and the V4 and V5 headers `CF_DIBV5` carries are read by the same call.
///
/// **The dimensions are refused before the picture is built** (review X-4).
/// `BmpDecoder::new_without_file_header` parses the header and nothing else, so
/// between it and `from_decoder` there is exactly one moment at which the shape
/// is known and no memory has been asked for. That moment is where the ceiling
/// goes; after it, the allocation has already happened or failed.
fn png_from_dib(dib: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = image::codecs::bmp::BmpDecoder::new_without_file_header(Cursor::new(dib))
        .map_err(|_| "the clipboard bitmap could not be read".to_owned())?;
    let (width, height) = decoder.dimensions();
    refuse_oversize(width, height, decoder.total_bytes())?;
    decoder
        .set_limits(decoder_limits())
        .map_err(|_| "the clipboard bitmap is past this paste's ceiling".to_owned())?;
    let picture = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| "the clipboard bitmap could not be read".to_owned())?;
    let mut bytes = Vec::new();
    picture
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
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

        for (width, height) in [(32_768_i32, 32_768_i32), (40_000, 2), (2, 40_000)] {
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
