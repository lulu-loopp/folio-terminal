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
    io::Cursor,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bt_platform::{PictureBytes, PictureEncoding};

/// **How many pasted pictures the folder keeps.**
///
/// Twenty is enough that the file a reader pasted this morning is still there
/// when the command they pasted it into finally asks for it, and small enough
/// that the folder never becomes a place megabytes go to be forgotten. It is a
/// number rather than a setting because the only question a setting would ask —
/// "how many screenshots do you want to keep in a temporary folder" — is one
/// nobody has an answer to.
pub const KEPT: usize = 20;

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
/// UTC and not the reader's own clock, for the reason `bt_persist`'s rejected
/// siblings are stamped the same way: the name has to sort the way the files
/// were written, and a local clock goes backwards an hour twice a year. The
/// calendar is `seed`'s, which is this crate's one Gregorian calendar.
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

/// What one write does to the folder: the name it takes, and the names it
/// retires to make room for itself.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WritePlan {
    pub name: String,
    pub delete: Vec<String>,
}

/// **The whole of the naming and the sweep, as a function of a listing.**
///
/// Pure, and that is the point: "the twenty newest survive" is a claim about a
/// folder, and a claim about a folder that can only be checked by making one is
/// a claim that gets checked once.
///
/// * The **index** is the first one free in this second, so two pastes inside
///   one second are two files rather than one overwritten twice.
/// * The **order** is `(stamp, index)` and not the filesystem's: a directory
///   listing has no order, and a modification time is a thing anything on the
///   machine can change.
/// * The new file **counts against the cap**, so the folder holds `kept` files
///   after the write rather than `kept + 1`.
#[must_use]
pub fn plan(existing: &[String], stamp: &str, kept: usize) -> WritePlan {
    let mut mine: Vec<(&str, u32, &String)> = existing
        .iter()
        .filter_map(|name| ours(name).map(|(at, index)| (at, index, name)))
        .collect();
    // Explicit and not `sort_by_key`: the key is borrowed out of the listing, which
    // is a key a sort-by-key closure cannot hand back.
    mine.sort_by(|left, right| left.0.cmp(right.0).then(left.1.cmp(&right.1)));
    let index = mine
        .iter()
        .filter(|(at, _, _)| *at == stamp)
        .map(|(_, index, _)| *index)
        .max()
        .map_or(1, |taken| taken.saturating_add(1));
    let over = mine.len().saturating_add(1).saturating_sub(kept);
    let delete = mine
        .into_iter()
        .take(over)
        .map(|(_, _, name)| name.clone())
        .collect();
    WritePlan {
        name: format!("{stamp}-{index}{EXTENSION}"),
        delete,
    }
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

/// The eight bytes every PNG file starts with (PNG specification §5.2).
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn png_from(picture: &PictureBytes) -> Result<Vec<u8>, String> {
    match picture.encoding {
        // **Handed straight through, after one question.** Re-encoding bytes that
        // are already PNG would cost a decode and an encode to arrive at the same
        // picture. The question is whether they really are PNG: a source may
        // advertise the format and render something else, and a path pasted to a
        // file with junk in it is worse than no paste at all.
        PictureEncoding::Png => {
            if picture.bytes.starts_with(&PNG_SIGNATURE) {
                Ok(picture.bytes.clone())
            } else {
                Err("the clipboard's PNG does not start like one".to_owned())
            }
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
fn png_from_dib(dib: &[u8]) -> Result<Vec<u8>, String> {
    let decoder = image::codecs::bmp::BmpDecoder::new_without_file_header(Cursor::new(dib))
        .map_err(|_| "the clipboard bitmap could not be read".to_owned())?;
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
    let plan = plan(&names_in(folder), &stamp(at), KEPT);
    let path = folder.join(&plan.name);
    fs::write(&path, &bytes).map_err(|error| format!("the file could not be written: {error}"))?;
    for name in plan.delete {
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

    /// PIN — the name is a date a person can read, and it sorts the way the
    /// files were written.
    #[test]
    fn the_stamp_is_the_calendar_and_it_sorts() {
        assert_eq!(stamp(UNIX_EPOCH), "19700101-000000");
        // 2026-09-16T14:30:12Z.
        assert_eq!(stamp(at(1_789_569_012)), "20260916-143012");
        assert!(stamp(at(1_789_569_012)) < stamp(at(1_789_403_413)));
        // A leap day, which is the case a lookup table gets wrong.
        assert_eq!(stamp(at(1_709_208_000)), "20240229-120000");
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
        let at_the_cap = plan(&full, "20260916-150000", KEPT);
        assert_eq!(at_the_cap.name, "20260916-150000-1.png");
        assert_eq!(at_the_cap.delete, ["20260916-143000-1.png"]);

        // One short of the cap: the write fits and nothing is retired.
        let with_room = plan(&full[1..], "20260916-150000", KEPT);
        assert!(with_room.delete.is_empty());

        // Far over the cap — a folder that was written to by an older build, or
        // by a Folio that was killed between the write and the sweep.
        let mut many = full.clone();
        many.extend((0..5).map(|index| format!("20260915-0000{index:02}-1.png")));
        let over = plan(&many, "20260916-150000", KEPT);
        assert_eq!(over.delete.len(), 6);
        assert!(over.delete.contains(&"20260915-000004-1.png".to_owned()));
        assert!(over.delete.contains(&"20260916-143000-1.png".to_owned()));
        assert!(!over.delete.contains(&"20260916-143001-1.png".to_owned()));
    }

    /// PIN — **two pastes in one second are two files**, and the index is the
    /// first one free rather than the count of what is there.
    #[test]
    fn a_second_paste_in_the_same_second_takes_the_next_index() {
        assert_eq!(
            plan(&[], "20260916-143012", KEPT).name,
            "20260916-143012-1.png"
        );
        let one = vec!["20260916-143012-1.png".to_owned()];
        assert_eq!(
            plan(&one, "20260916-143012", KEPT).name,
            "20260916-143012-2.png"
        );
        // A gap left by a file somebody deleted by hand is not filled in: the
        // index has to keep rising or the name stops sorting.
        let gap = vec![
            "20260916-143012-1.png".to_owned(),
            "20260916-143012-7.png".to_owned(),
        ];
        assert_eq!(
            plan(&gap, "20260916-143012", KEPT).name,
            "20260916-143012-8.png"
        );
        // Another second entirely starts again at one.
        assert_eq!(
            plan(&gap, "20260916-143013", KEPT).name,
            "20260916-143013-1.png"
        );
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
        let swept = plan(&folder, "20260916-150000", KEPT);
        assert_eq!(swept.delete, ["20260916-143000-1.png"]);
        assert_eq!(ours("20260916-143012-1.png"), Some(("20260916-143012", 1)));
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

        let real = PictureBytes {
            encoding: PictureEncoding::Png,
            bytes: {
                let mut bytes = Vec::new();
                image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                    1,
                    1,
                    image::Rgba([1, 2, 3, 255]),
                ))
                .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
                .expect("a one-pixel PNG encodes");
                bytes
            },
        };
        // The liar first: the second rung is what the paste ends up with.
        let chosen = png_bytes(&[liar, real.clone()]).expect("the second rung answers");
        assert_eq!(
            chosen, real.bytes,
            "an encoding that works is handed through"
        );
    }
}
