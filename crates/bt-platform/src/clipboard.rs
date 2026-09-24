//! Clipboard acquisition port (paste-paths design §1). Content is read only on a gesture.
//! A candidate survey is not a payload: an empty file list may require fetching the next rung.

use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClipboardPayload {
    Files(Vec<PathBuf>),
    Text(String),
    Picture(Vec<PictureBytes>),
    Refused(UnsupportedKind),
    Nothing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedKind {
    Promise,
}

/// **How many bytes of one picture encoding are worth copying at all**
/// (review X-4).
///
/// The acquisition copies what a source offers before anything has looked at
/// it, and what a source offers is the source's choice: a clipboard provider
/// that advertises a 4 GiB `CF_DIB` gets one `GlobalLock` and one `to_vec` and
/// this process is out of memory before any decoder has had an opinion. The
/// ceiling belongs here, at the copy, because here is where the bytes first
/// exist.
///
/// 256 MiB is far above any screenshot — a 4K screen in 32-bit colour is 33 MB —
/// and far below the point at which refusing is worse than dying. It is the same
/// number `bt_app::clipboard_picture::MAX_ENCODED_BYTES` refuses at, one door
/// further in.
pub const MAX_PICTURE_BYTES: usize = 256 * 1024 * 1024;

/// The shapes a picture is offered in, **best first**: the order of this enum is
/// the order the platform preference lists below are written in.
///
/// `Png` is first because it is already the bytes Folio writes — a source that
/// offers it has done the encode, and re-encoding what a screenshot tool
/// produced would be a second lossless pass for nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PictureEncoding {
    Png,
    DibV5,
    Dib,
    /// macOS' own second shape (`public.tiff`), which is what an application
    /// that copies a picture through AppKit rather than through a screenshot
    /// puts on the pasteboard. No Windows source offers it.
    Tiff,
}

/// **Which shape of a clipboard picture Folio uses, on Windows** — best first,
/// and the only place that answers the question.
///
/// `PNG` before `CF_DIBV5` before `CF_DIB`: the registered `PNG` is lossless,
/// carries alpha, is the smallest of the three by an order of magnitude, and is
/// already the encoding this paste writes out. `CF_DIBV5` comes next because its
/// header can carry alpha where `CF_DIB`'s cannot, and `CF_DIB` last because
/// every source offers it and it is the one that always works.
pub const WINDOWS_PICTURE_ORDER: [PictureEncoding; 3] = [
    PictureEncoding::Png,
    PictureEncoding::DibV5,
    PictureEncoding::Dib,
];

/// **Which shape of a pasteboard picture Folio uses, on macOS** — the same rule
/// over the two shapes AppKit offers. A source that copies through AppKit rather
/// than through the screenshot key offers `public.tiff` and nothing else
/// (§7.61), so `Tiff` is the fallback rather than an alternative.
pub const MACOS_PICTURE_ORDER: [PictureEncoding; 2] = [PictureEncoding::Png, PictureEncoding::Tiff];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PictureBytes {
    pub encoding: PictureEncoding,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Candidate<T> {
    Absent,
    Present(T),
    Unreadable(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClipboardTypes {
    pub files: bool,
    pub text: bool,
    pub picture: bool,
    pub promise: bool,
}

/// Implementations hold one Windows open interval or one macOS changeCount interval.
/// `finish` releases the interval even on an acquisition failure. No retry after acquisition.
pub trait ClipboardPort {
    fn begin(&mut self) -> Result<(), String>;
    fn survey(&mut self) -> Result<ClipboardTypes, String>;
    fn files(&mut self) -> Candidate<Vec<PathBuf>>;
    fn text(&mut self) -> Candidate<String>;
    /// **One** encoding — the best whole one this source will hand over, chosen
    /// by the platform's preference list through [`first_offered_picture`]. The
    /// bytes are copied and nothing is decoded here: a screenshot is megabytes, the
    /// caller is the event-loop thread, and turning those bytes into a picture is
    /// the picture worker's job.
    ///
    /// The answer is still a list because the worker consumes a list, and because
    /// a platform that had two shapes worth carrying at once could say so without
    /// this door changing. No platform does today.
    fn picture(&mut self) -> Candidate<Vec<PictureBytes>>;
    fn finish(&mut self) -> Result<(), String>;
}

/// **The clipboard, asked one shape of picture at a time.**
///
/// Split from [`ClipboardPort`] because the choice between shapes is the same
/// choice on every platform while the two questions underneath it are not: on
/// Windows they are `IsClipboardFormatAvailable` and `GetClipboardData`, on
/// macOS `dataForType` on its own. Splitting them is also what lets the choice be
/// tested without a clipboard.
pub trait PictureSource {
    /// **Does the source advertise this shape?** — asked without rendering it,
    /// where the platform can tell the two apart. A platform whose only question
    /// is the one that renders answers `true` here and says everything in
    /// [`read`](PictureSource::read).
    fn offers(&mut self, _encoding: PictureEncoding) -> bool {
        true
    }
    /// **The shape's bytes, rendered and copied**, or `None` when the source
    /// advertised it and then would not hand it over — a delayed format the owner
    /// declines to render, an empty global, one past [`MAX_PICTURE_BYTES`], or a
    /// lock that failed. None of those is an error on its own: the next shape in
    /// the preference list is the answer to them.
    fn read(&mut self, encoding: PictureEncoding) -> Option<Vec<u8>>;
}

/// **The first shape in `order` this source will actually hand over** — and the
/// only one it is asked to render.
///
/// This is the whole of the repair the window thread needed. Reading every shape
/// and choosing afterwards cost a full copy of each — about 66 MB for a 4K
/// screenshot offered as `PNG`, `CF_DIBV5` and `CF_DIB` — and up to three
/// synchronous round trips into the application the picture was copied from,
/// all on the thread that is meant to be drawing. Walking the same order and
/// stopping at the first answer copies exactly one shape and asks the source
/// once.
///
/// **The shape that is handed over is also read for shape** before the walk
/// accepts it, by [`shape_is_intact`]. Choosing the first shape a source *renders*
/// would have been a change to what gets pasted: sources exist — browsers,
/// remote-desktop clients — that put a `PNG` representation on the board which
/// no PNG decoder can read, and until this walk existed the worker one door later
/// simply used the `CF_DIB` that had also been copied. Reading the chosen shape's
/// header here, for a few hundred bytes and no decode, gives the walk the same
/// answer the eager copy used to buy.
///
/// **A board on which nothing is intact still hands over the best thing that
/// rendered**, rather than answering `Absent`. `Absent` here means "no picture",
/// and the paste falls to silence; a picture that is present and broken deserves
/// the worker's sentence about *why*, which is the sentence it gave before this
/// change. The extra copy in that case is the old cost, paid only on the board
/// where the old cost was the only way to an answer.
pub fn first_offered_picture(
    order: &[PictureEncoding],
    source: &mut impl PictureSource,
) -> Candidate<Vec<PictureBytes>> {
    let mut best_broken: Option<PictureBytes> = None;
    for &encoding in order {
        if !source.offers(encoding) {
            continue;
        }
        let Some(bytes) = source.read(encoding) else {
            continue;
        };
        if shape_is_intact(encoding, &bytes) {
            return Candidate::Present(vec![PictureBytes { encoding, bytes }]);
        }
        if best_broken.is_none() {
            best_broken = Some(PictureBytes { encoding, bytes });
        }
    }
    match best_broken {
        Some(picture) => Candidate::Present(vec![picture]),
        None => Candidate::Absent,
    }
}

/// **Is this byte string the shape it says it is?** — read structurally, on the
/// window thread, in bounded work and without decoding anything.
///
/// Not a second decoder and not a second opinion about whether a picture is
/// *good*: the worker's decoder remains the authority on that, and this answers
/// the one question the walk has to ask before the clipboard closes — *is there
/// any point preferring this representation over the next one*. It reads headers
/// only. For a `PNG` that is the 8-byte signature, the 25-byte `IHDR` chunk and
/// at most [`PNG_CHUNKS_BEFORE_PIXELS`] further 8-byte chunk headers looking for
/// `IDAT`, so at most 545 bytes are examined however large the picture is; for a
/// bitmap it is the info header, at most 124 bytes, plus arithmetic; for a TIFF
/// it is 8 bytes. Nothing is inflated and no pixel is touched.
#[must_use]
pub fn shape_is_intact(encoding: PictureEncoding, bytes: &[u8]) -> bool {
    match encoding {
        PictureEncoding::Png => png_is_intact(bytes),
        PictureEncoding::DibV5 | PictureEncoding::Dib => dib_is_intact(bytes),
        PictureEncoding::Tiff => tiff_is_intact(bytes),
    }
}

/// **The widest and tallest a representation may claim to be** before the walk
/// stops preferring it.
///
/// The same number `bt_app::clipboard_picture::MAX_SIDE` refuses at, one door
/// further in, and re-stated here for the reason [`MAX_PICTURE_BYTES`] is: the
/// dependency runs from `bt-app` to `bt-platform`, so the door cannot read the
/// worker's copy. Keeping the two equal is what makes the door's fall-through
/// agree with the worker's refusal instead of preferring a shape the worker will
/// then turn down.
pub const MAX_PICTURE_SIDE: u32 = 16_384;

/// **How far into a `PNG` the walk will look for the first `IDAT`.**
///
/// A real picture puts a handful of chunks before its pixels; sixty-four is far
/// past any of them and bounds this walk at 8 bytes apiece, so a header that
/// chains chunk after chunk cannot make the window thread walk a clipboard-sized
/// buffer looking for pixels that are not there.
pub const PNG_CHUNKS_BEFORE_PIXELS: usize = 64;

/// A side length a real picture has: present, and inside the ceiling the worker
/// decodes at.
fn side_is_sane(side: u32) -> bool {
    side > 0 && side <= MAX_PICTURE_SIDE
}

/// The eight bytes every PNG begins with (PNG spec §5.2).
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// **A `PNG` header, read as far as the first `IDAT`.**
///
/// The signature, then `IHDR` — which the spec requires to be the first chunk and
/// to be thirteen bytes — then a walk over chunk headers until the pixels start.
/// A truncated representation runs out of buffer during that walk and a
/// representation that is not a PNG at all fails at the signature, which are the
/// two shapes the sources that put unreadable `PNG`s on the clipboard produce.
fn png_is_intact(bytes: &[u8]) -> bool {
    let Some(rest) = bytes.strip_prefix(&PNG_SIGNATURE) else {
        return false;
    };
    // 4 length + 4 type + 13 data + 4 CRC, and the length and type are fixed by
    // the spec, so they are compared rather than parsed.
    if !rest.starts_with(b"\x00\x00\x00\x0dIHDR") || rest.len() < 25 {
        return false;
    }
    let number = |at: usize| u32::from_be_bytes(rest[at..at + 4].try_into().expect("four bytes"));
    if !side_is_sane(number(8)) || !side_is_sane(number(12)) {
        return false;
    }
    // Bit depth, colour type, compression, filter and interlace: each is a small
    // set in the spec, and a byte outside it is a header nobody wrote (§11.2.2).
    if !matches!(rest[16], 1 | 2 | 4 | 8 | 16)
        || !matches!(rest[17], 0 | 2 | 3 | 4 | 6)
        || rest[18] != 0
        || rest[19] != 0
        || rest[20] > 1
    {
        return false;
    }
    let mut at = 25;
    for _ in 0..PNG_CHUNKS_BEFORE_PIXELS {
        let Some(header) = rest.get(at..at + 8) else {
            return false;
        };
        let length = u32::from_be_bytes(header[..4].try_into().expect("four bytes")) as usize;
        let kind = &header[4..8];
        if !kind.iter().all(u8::is_ascii_alphabetic) {
            return false;
        }
        if kind == b"IDAT" {
            // Pixels that are declared and then are not there are the truncation
            // this walk exists to catch.
            return length > 0 && at + 12 + length <= rest.len();
        }
        // The chunk's data and CRC must be inside what was copied off the board.
        let Some(next) = length.checked_add(12).and_then(|step| at.checked_add(step)) else {
            return false;
        };
        if next > rest.len() {
            return false;
        }
        at = next;
    }
    false
}

/// **A device-independent bitmap's info header, and whether the pixels it
/// describes fit in what was copied.**
///
/// The header sizes are Windows' own — 12 is `BITMAPCOREHEADER`, 40
/// `BITMAPINFOHEADER`, 108 and 124 the V4 and V5 headers `CF_DIBV5` carries, and
/// 52, 56 and 64 the variants `image`'s BMP decoder also reads. The arithmetic at
/// the end is the one review X-4 named: a header is believed before the pixels
/// are read, so a fifty-byte global claiming a large picture is a header that is
/// lying, and the next shape is the answer to it.
fn dib_is_intact(bytes: &[u8]) -> bool {
    /// Uncompressed, and the two bit-field forms: the only ones whose pixel
    /// length is arithmetic over the header.
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    const BI_ALPHABITFIELDS: u32 = 6;

    let Some(size) = bytes.get(..4) else {
        return false;
    };
    let header = u32::from_le_bytes(size.try_into().expect("four bytes")) as usize;
    if !matches!(header, 12 | 40 | 52 | 56 | 64 | 108 | 124) || bytes.len() < header {
        return false;
    }
    let short = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().expect("two bytes"));
    let long = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
    // The core header carries 16-bit dimensions; every later one carries 32-bit
    // dimensions, of which the height may be negative for a top-down bitmap.
    let (width, height, planes, depth, compression, entry) = if header == 12 {
        (
            u32::from(short(4)),
            u32::from(short(6)),
            short(8),
            short(10),
            BI_RGB,
            3u64,
        )
    } else {
        if long(4) < 0 {
            return false;
        }
        (
            long(4).unsigned_abs(),
            long(8).unsigned_abs(),
            short(12),
            short(14),
            long(16).cast_unsigned(),
            4u64,
        )
    };
    if !side_is_sane(width) || !side_is_sane(height) || planes != 1 {
        return false;
    }
    if !matches!(depth, 1 | 2 | 4 | 8 | 16 | 24 | 32) {
        return false;
    }
    if !matches!(compression, BI_RGB | BI_BITFIELDS | BI_ALPHABITFIELDS) {
        // RLE4, RLE8, and a JPEG or PNG carried inside a DIB: the body's length
        // is not arithmetic over the header, so the header is all there is to
        // check. An unknown compression is a header nobody wrote.
        return matches!(compression, 1 | 2 | 4 | 5);
    }
    // A palette is present when the header says so, and always for the depths
    // that index one. `BITMAPINFOHEADER` puts the bit-field masks where the
    // palette would start; the later headers carry them inside themselves.
    let used = if header == 12 {
        0
    } else {
        u64::from(long(32).cast_unsigned())
    };
    let colours = match (used, depth) {
        (0, 1..=8) => 1u64 << depth,
        (used, _) => used,
    };
    let masks = match (header, compression) {
        (40, BI_BITFIELDS) => 12,
        (40, BI_ALPHABITFIELDS) => 16,
        _ => 0,
    };
    let stride = (u64::from(width) * u64::from(depth)).div_ceil(32) * 4;
    let needed = (header as u64)
        .saturating_add(masks)
        .saturating_add(colours.saturating_mul(entry))
        .saturating_add(stride.saturating_mul(u64::from(height)));
    needed <= bytes.len() as u64
}

/// **A TIFF's eight-byte header** (TIFF 6.0 §2): the byte order, the answer 42,
/// and a first directory that is inside what was copied.
///
/// macOS' `public.tiff` is the shape an AppKit copy offers, and it is the shape
/// this walk falls to when a `public.png` beside it will not read.
fn tiff_is_intact(bytes: &[u8]) -> bool {
    let Some(header) = bytes.get(..8) else {
        return false;
    };
    let big = match &header[..2] {
        b"II" => false,
        b"MM" => true,
        _ => return false,
    };
    let short = |at: usize| {
        let pair = header[at..at + 2].try_into().expect("two bytes");
        if big {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        }
    };
    let long = |at: usize| {
        let quad = header[at..at + 4].try_into().expect("four bytes");
        if big {
            u32::from_be_bytes(quad)
        } else {
            u32::from_le_bytes(quad)
        }
    };
    // The directory count is the two bytes the offset points at; a directory that
    // starts past what was copied is a representation that was cut short.
    short(2) == 42 && u64::from(long(4)) + 2 <= bytes.len() as u64 && long(4) >= 8
}

/// The terminal context menu may survey types on the press that opens it, never fetch content.
/// A failed survey disables the future row without hiding it; ordinary Paste stays enabled.
/// The row itself is still unwritten; the acquisition it would enable is the rung below.
#[must_use]
pub fn picture_paste_enabled(types: Result<ClipboardTypes, String>, save_picture: bool) -> bool {
    save_picture && types.is_ok_and(|types| types.picture)
}

pub fn read_payload(port: &mut impl ClipboardPort) -> Result<ClipboardPayload, String> {
    port.begin()?;
    let answer = (|| {
        let types = port.survey()?;
        if types.files {
            match port.files() {
                Candidate::Present(files) if !files.is_empty() => {
                    return Ok(ClipboardPayload::Files(files));
                }
                Candidate::Unreadable(reason) => return Err(reason),
                Candidate::Absent | Candidate::Present(_) => {}
            }
        }
        if types.text {
            match port.text() {
                Candidate::Present(text) => return Ok(ClipboardPayload::Text(text)),
                Candidate::Unreadable(reason) => return Err(reason),
                Candidate::Absent => {}
            }
        }
        if types.picture {
            match port.picture() {
                Candidate::Present(pictures) if !pictures.is_empty() => {
                    return Ok(ClipboardPayload::Picture(pictures));
                }
                Candidate::Unreadable(reason) => return Err(reason),
                Candidate::Absent | Candidate::Present(_) => {}
            }
        }
        // A promise is only an advertised refusal.
        Ok(if types.promise {
            ClipboardPayload::Refused(UnsupportedKind::Promise)
        } else {
            ClipboardPayload::Nothing
        })
    })();
    let finished = port.finish();
    finished.and(answer)
}

/// Log-free shared file-URL decoder. Native acquisition reports missing absoluteString as None.
/// Non-file URLs are absent; a file representation that cannot be decoded stops the rung.
pub fn file_urls(urls: impl IntoIterator<Item = Option<String>>) -> Candidate<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for url in urls {
        let Some(url) = url else {
            return Candidate::Unreadable("file URL acquisition failed".to_owned());
        };
        if !url
            .get(..5)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
        {
            continue;
        }
        match crate::app_delegate::path_from_file_url(&url) {
            Ok(path) => paths.push(path),
            // The underlying Service decoder's reasons contain URLs. Clipboard diagnostics must not.
            Err(_) => return Candidate::Unreadable("file URL could not be read".to_owned()),
        }
    }
    if paths.is_empty() {
        Candidate::Absent
    } else {
        Candidate::Present(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        types: ClipboardTypes,
        files: Candidate<Vec<PathBuf>>,
        text: Candidate<String>,
        picture: Candidate<Vec<PictureBytes>>,
        held: bool,
        opened: usize,
        fetched: Vec<&'static str>,
        sequence: u64,
        change_count: Option<(u64, u64)>,
        competing_open_failed: bool,
        fail_begin: bool,
        fail_survey: bool,
    }

    impl Fake {
        fn new(files: Candidate<Vec<PathBuf>>, text: Candidate<String>) -> Self {
            Self {
                types: ClipboardTypes {
                    files: true,
                    text: true,
                    picture: true,
                    promise: false,
                },
                files,
                text,
                picture: Candidate::Absent,
                held: false,
                opened: 0,
                fetched: Vec::new(),
                sequence: 4,
                change_count: None,
                competing_open_failed: false,
                fail_begin: false,
                fail_survey: false,
            }
        }
        fn competing_copy(&mut self, text: &str) -> bool {
            if self.held {
                return false;
            }
            self.files = Candidate::Absent;
            self.text = Candidate::Present(text.to_owned());
            self.sequence += 1;
            true
        }
    }
    impl ClipboardPort for Fake {
        fn begin(&mut self) -> Result<(), String> {
            assert!(!self.held);
            if self.fail_begin {
                return Err("open failed".into());
            }
            self.held = true;
            self.opened += 1;
            Ok(())
        }
        fn survey(&mut self) -> Result<ClipboardTypes, String> {
            assert!(self.held);
            assert!(self.fetched.is_empty());
            if self.fail_survey {
                return Err("survey failed".into());
            }
            Ok(self.types)
        }
        fn files(&mut self) -> Candidate<Vec<PathBuf>> {
            assert!(self.held);
            self.fetched.push("files");
            self.files.clone()
        }
        fn text(&mut self) -> Candidate<String> {
            assert!(self.held);
            self.fetched.push("text");
            // Rendering advances sequence; a competing open is excluded by the same held interval.
            self.sequence += 1;
            self.competing_open_failed = !self.competing_copy("replacement");
            self.text.clone()
        }
        fn picture(&mut self) -> Candidate<Vec<PictureBytes>> {
            assert!(self.held);
            self.fetched.push("picture");
            self.picture.clone()
        }
        fn finish(&mut self) -> Result<(), String> {
            assert!(self.held);
            self.held = false;
            if self
                .change_count
                .is_some_and(|(before, after)| before != after)
            {
                Err("pasteboard changed during read".to_owned())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn open_and_survey_failures_fetch_nothing_and_release_only_an_acquired_interval() {
        for fail_begin in [true, false] {
            let mut fake = Fake::new(
                Candidate::Present(vec!["file".into()]),
                Candidate::Present("text".into()),
            );
            fake.fail_begin = fail_begin;
            fake.fail_survey = !fail_begin;
            assert!(read_payload(&mut fake).is_err());
            assert!(!fake.held);
            assert!(fake.fetched.is_empty());
            assert_eq!(fake.opened, usize::from(!fail_begin));
        }
    }

    #[test]
    fn candidate_product_stops_at_first_answer_including_empty_text() {
        let files = [
            Candidate::Absent,
            Candidate::Present(Vec::new()),
            Candidate::Present(vec![PathBuf::from("one")]),
            Candidate::Unreadable("files failed".into()),
        ];
        let texts = [
            Candidate::Absent,
            Candidate::Present(String::new()),
            Candidate::Present("text".into()),
            Candidate::Unreadable("text failed".into()),
        ];
        for file in files {
            for text in &texts {
                let mut fake = Fake::new(file.clone(), text.clone());
                let result = read_payload(&mut fake);
                let expected = match &file {
                    Candidate::Unreadable(reason) => Err(reason.clone()),
                    Candidate::Present(files) if !files.is_empty() => {
                        Ok(ClipboardPayload::Files(files.clone()))
                    }
                    _ => match text {
                        Candidate::Absent => Ok(ClipboardPayload::Nothing),
                        Candidate::Present(text) => Ok(ClipboardPayload::Text(text.clone())),
                        Candidate::Unreadable(reason) => Err(reason.clone()),
                    },
                };
                assert_eq!(result, expected);
                assert_eq!(fake.opened, 1);
                assert!(!fake.held);
                // One rung per answer that was not given: the file rung stops the walk
                // when it answers or fails, the text rung stops it the same way, and the
                // picture rung is only reached when both were silent.
                let expected_rungs: &[&str] = if matches!(&file, Candidate::Unreadable(_))
                    || matches!(&file, Candidate::Present(f) if !f.is_empty())
                {
                    &["files"]
                } else if matches!(text, Candidate::Absent) {
                    &["files", "text", "picture"]
                } else {
                    &["files", "text"]
                };
                assert_eq!(fake.fetched, expected_rungs);
            }
        }
    }

    #[test]
    fn survey_selects_without_fetching_and_an_unoffered_picture_is_silent_nothing() {
        let mut fake = Fake::new(
            Candidate::Present(vec!["not advertised".into()]),
            Candidate::Unreadable("not advertised".into()),
        );
        fake.picture = Candidate::Present(vec![shot(PictureEncoding::Png)]);
        fake.types.files = false;
        fake.types.text = false;
        fake.types.picture = false;
        assert_eq!(read_payload(&mut fake), Ok(ClipboardPayload::Nothing));
        assert!(fake.fetched.is_empty());
        fake.types.promise = true;
        assert_eq!(
            read_payload(&mut fake),
            Ok(ClipboardPayload::Refused(UnsupportedKind::Promise))
        );
        assert!(!picture_paste_enabled(Ok(fake.types), true));
        fake.types.picture = true;
        assert!(picture_paste_enabled(Ok(fake.types), true));
        assert!(!picture_paste_enabled(Err("busy".into()), true));
        assert!(!picture_paste_enabled(Ok(fake.types), false));
    }

    /// **A source that offers the shapes it was given and counts every question
    /// it is asked** — the clipboard double for the picture choice.
    ///
    /// `offers` is the availability query (`IsClipboardFormatAvailable`) and
    /// `read` is the one that renders and copies (`GetClipboardData` +
    /// `GlobalLock` + `to_vec`, or `dataForType`). They are counted separately
    /// because the whole point of the choice is how many times the *second* one
    /// runs: it is the one that copies megabytes and calls synchronously into the
    /// application the picture was copied from.
    #[derive(Default)]
    struct Board {
        /// What the source advertises, in the order the source happens to hold
        /// them; `None` is a shape that is advertised and then will not be handed
        /// over — a delayed format the owner declines to render, an empty global,
        /// one past the ceiling, or a lock that failed.
        shapes: Vec<(PictureEncoding, Option<Vec<u8>>)>,
        asked: Vec<PictureEncoding>,
        rendered: Vec<PictureEncoding>,
        copied: usize,
    }

    impl Board {
        fn with(shapes: impl IntoIterator<Item = (PictureEncoding, Option<Vec<u8>>)>) -> Self {
            Self {
                shapes: shapes.into_iter().collect(),
                ..Self::default()
            }
        }
    }

    impl PictureSource for Board {
        fn offers(&mut self, encoding: PictureEncoding) -> bool {
            self.asked.push(encoding);
            self.shapes.iter().any(|(shape, _)| *shape == encoding)
        }
        fn read(&mut self, encoding: PictureEncoding) -> Option<Vec<u8>> {
            self.rendered.push(encoding);
            let bytes = self
                .shapes
                .iter()
                .find(|(shape, _)| *shape == encoding)
                .and_then(|(_, bytes)| bytes.clone())?;
            self.copied += bytes.len();
            Some(bytes)
        }
    }

    /// **A structurally whole PNG**: the signature, an `IHDR` and an `IDAT` whose
    /// pixels are there. Nothing is compressed, because nothing at the door
    /// inflates — inside an `IDAT` the door knows only a length.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::from(PNG_SIGNATURE);
        bytes.extend_from_slice(b"\x00\x00\x00\x0dIHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        // Depth 8, truecolour with alpha, and the only compression, filter and
        // interlace bytes the spec allows.
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes.extend_from_slice(&[0; 4]); // the CRC, which the door does not check
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(b"IDAT");
        bytes.extend_from_slice(&[0; 12]); // eight bytes of pixels and a CRC
        bytes
    }

    /// **A structurally whole bottom-up 32-bit bitmap** under the info header of
    /// the given size — 40 for `CF_DIB`, 124 for the V5 header `CF_DIBV5` carries.
    fn bitmap(header: u32, width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::from(header.to_le_bytes());
        bytes.extend_from_slice(&i32::try_from(width).expect("a test width").to_le_bytes());
        bytes.extend_from_slice(&i32::try_from(height).expect("a test height").to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // one plane
        bytes.extend_from_slice(&32u16.to_le_bytes()); // and 32 bits of it
        bytes.resize(header as usize, 0);
        bytes.resize(header as usize + (width * height * 4) as usize, 0);
        bytes
    }

    fn dib(width: u32, height: u32) -> Vec<u8> {
        bitmap(40, width, height)
    }

    fn dib_v5(width: u32, height: u32) -> Vec<u8> {
        bitmap(124, width, height)
    }

    /// **A structurally whole TIFF header** — little-endian, the answer 42, and a
    /// first directory that is inside the buffer.
    fn tiff() -> Vec<u8> {
        Vec::from(*b"II\x2a\x00\x08\x00\x00\x00\x00\x00")
    }

    /// One walk of a preference list, as the fallback cases below spell it out:
    /// what the board carries, which shapes it is asked to render, and which
    /// shape — if any — the walk comes back with.
    type Walk = (
        Vec<(PictureEncoding, Option<Vec<u8>>)>,
        &'static [PictureEncoding],
        Option<(PictureEncoding, Vec<u8>)>,
    );

    /// **A source offering all three Windows shapes is asked to render exactly
    /// one** (acceptance A1).
    ///
    /// Before this change the window thread rendered and copied all three —
    /// `rendered` was `[Png, DibV5, Dib]` and `copied` the sum — although the
    /// worker one door later has always used the first that decodes. For a 4K
    /// screenshot offered as `PNG` + `CF_DIBV5` + `CF_DIB` those two extra copies
    /// are about 66 MB, and each is a synchronous round trip into the application
    /// the picture came from.
    ///
    /// MUTATION: let the walk keep going after an answer — collect into a list
    /// instead of returning — and `rendered` and `copied` both go back to three
    /// shapes' worth, while the payload assertion fails on the extra elements.
    #[test]
    fn the_best_shape_on_offer_is_the_only_one_rendered() {
        let mut board = Board::with([
            (PictureEncoding::Png, Some(png(8, 8))),
            (PictureEncoding::DibV5, Some(dib_v5(8, 8))),
            (PictureEncoding::Dib, Some(dib(8, 8))),
        ]);
        let answer = first_offered_picture(&WINDOWS_PICTURE_ORDER, &mut board);
        // The count first, because it is the fact this repair is about.
        assert_eq!(board.rendered, [PictureEncoding::Png]);
        assert_eq!(board.asked, [PictureEncoding::Png]);
        // The budget, by construction: the bytes of the one shape that won, never
        // the sum of the shapes that were on offer.
        assert_eq!(board.copied, png(8, 8).len());
        assert_eq!(
            answer,
            Candidate::Present(vec![PictureBytes {
                encoding: PictureEncoding::Png,
                bytes: png(8, 8),
            }])
        );
    }

    /// **Absent, or present and unrenderable, both fall to the next shape — and
    /// nothing at all is still `Absent`** (acceptance A2).
    ///
    /// The two failures are deliberately not distinguished: `GetClipboardData`
    /// renders delayed formats, and a source that advertises its own `PNG` and
    /// then cannot produce it is an ordinary source, not a broken clipboard. What
    /// the rung must not do is turn either one into a refusal, which is the
    /// reason today's `Absent`/empty answer is kept exactly as it was.
    #[test]
    fn a_shape_that_is_missing_or_will_not_render_falls_to_the_next() {
        let cases: [Walk; 5] = [
            // First choice absent: the second is asked and rendered, alone.
            (
                vec![
                    (PictureEncoding::DibV5, Some(dib_v5(4, 4))),
                    (PictureEncoding::Dib, Some(dib(4, 4))),
                ],
                &[PictureEncoding::DibV5],
                Some((PictureEncoding::DibV5, dib_v5(4, 4))),
            ),
            // First choice present and unrenderable (lock failed, empty global,
            // past the ceiling): it is rendered, refuses, and the second wins.
            (
                vec![
                    (PictureEncoding::Png, None),
                    (PictureEncoding::DibV5, Some(dib_v5(4, 4))),
                ],
                &[PictureEncoding::Png, PictureEncoding::DibV5],
                Some((PictureEncoding::DibV5, dib_v5(4, 4))),
            ),
            // Two in a row refuse: the walk reaches the last shape.
            (
                vec![
                    (PictureEncoding::Png, None),
                    (PictureEncoding::DibV5, None),
                    (PictureEncoding::Dib, Some(dib(4, 4))),
                ],
                &[
                    PictureEncoding::Png,
                    PictureEncoding::DibV5,
                    PictureEncoding::Dib,
                ],
                Some((PictureEncoding::Dib, dib(4, 4))),
            ),
            // Only the worst shape is on offer, which is every source that is not
            // a screenshot tool.
            (
                vec![(PictureEncoding::Dib, Some(dib(4, 4)))],
                &[PictureEncoding::Dib],
                Some((PictureEncoding::Dib, dib(4, 4))),
            ),
            // Advertised by the survey, rendered by nobody: `Absent`, as today.
            (
                vec![
                    (PictureEncoding::Png, None),
                    (PictureEncoding::DibV5, None),
                    (PictureEncoding::Dib, None),
                ],
                &[
                    PictureEncoding::Png,
                    PictureEncoding::DibV5,
                    PictureEncoding::Dib,
                ],
                None,
            ),
        ];
        for (shapes, rendered, winner) in cases {
            let mut board = Board::with(shapes);
            let answer = first_offered_picture(&WINDOWS_PICTURE_ORDER, &mut board);
            // A shape the board does not carry is asked about and never
            // rendered: the availability query is the cheap one. The walk stops
            // asking the moment one answers, so the questions are the preference
            // list up to and including the winner.
            let stop = winner
                .as_ref()
                .map_or(WINDOWS_PICTURE_ORDER.len(), |(encoding, _)| {
                    WINDOWS_PICTURE_ORDER
                        .iter()
                        .position(|shape| shape == encoding)
                        .expect("the winner came out of the preference list")
                        + 1
                });
            assert_eq!(board.asked, WINDOWS_PICTURE_ORDER[..stop]);
            assert_eq!(board.rendered, rendered);
            match winner {
                Some((encoding, bytes)) => {
                    assert_eq!(board.copied, bytes.len());
                    assert_eq!(
                        answer,
                        Candidate::Present(vec![PictureBytes { encoding, bytes }])
                    );
                }
                None => {
                    assert_eq!(answer, Candidate::Absent);
                    assert_eq!(board.copied, 0);
                }
            }
        }
    }

    /// **Neither preference list ever asks for a shape the platform cannot
    /// carry**, and each is written best first.
    #[test]
    fn the_preference_lists_are_each_platforms_own_shapes_best_first() {
        assert_eq!(
            WINDOWS_PICTURE_ORDER,
            [
                PictureEncoding::Png,
                PictureEncoding::DibV5,
                PictureEncoding::Dib
            ]
        );
        assert_eq!(
            MACOS_PICTURE_ORDER,
            [PictureEncoding::Png, PictureEncoding::Tiff]
        );
        // A shape one platform carries is never asked for on the other: a `PNG`
        // is the only name both boards know.
        for order in [&WINDOWS_PICTURE_ORDER[..], &MACOS_PICTURE_ORDER[..]] {
            let mut board = Board::default();
            assert_eq!(first_offered_picture(order, &mut board), Candidate::Absent);
            assert_eq!(board.asked, order);
            assert!(board.rendered.is_empty());
        }
    }

    /// **A paste that text or a file list answers renders no picture at all**
    /// (acceptance A3) — through the whole door, not just the choice.
    ///
    /// The rung order was already `read_payload`'s and is unchanged; what this
    /// pins is that the saving is not undone by something rendering a picture
    /// before the walk is reached.
    #[test]
    fn text_and_files_win_without_a_single_picture_being_rendered() {
        for (files, text, expected) in [
            (
                Candidate::Present(vec![PathBuf::from("/shot.png")]),
                Candidate::Absent,
                ClipboardPayload::Files(vec![PathBuf::from("/shot.png")]),
            ),
            (
                Candidate::Absent,
                Candidate::Present("https://example.test/a.png".to_owned()),
                ClipboardPayload::Text("https://example.test/a.png".to_owned()),
            ),
        ] {
            let mut door = Door {
                files,
                text,
                board: Board::with([
                    (PictureEncoding::Png, Some(png(8, 8))),
                    (PictureEncoding::DibV5, Some(dib_v5(8, 8))),
                    (PictureEncoding::Dib, Some(dib(8, 8))),
                ]),
            };
            assert_eq!(read_payload(&mut door), Ok(expected));
            assert!(door.board.asked.is_empty());
            assert!(door.board.rendered.is_empty());
            assert_eq!(door.board.copied, 0);
        }
        // And when nothing else answers, the picture rung hands the worker one
        // shape — the best one — out of the two that were on offer.
        let mut door = Door {
            files: Candidate::Absent,
            text: Candidate::Absent,
            board: Board::with([
                (PictureEncoding::DibV5, Some(dib_v5(8, 8))),
                (PictureEncoding::Dib, Some(dib(8, 8))),
            ]),
        };
        assert_eq!(
            read_payload(&mut door),
            Ok(ClipboardPayload::Picture(vec![PictureBytes {
                encoding: PictureEncoding::DibV5,
                bytes: dib_v5(8, 8),
            }]))
        );
        assert_eq!(door.board.copied, dib_v5(8, 8).len());
    }

    /// **A `PNG` that is not a PNG loses to the bitmap beside it, and costs
    /// exactly one extra read** — the behaviour the eager copy used to buy, at
    /// the price of a header instead of the price of every shape.
    ///
    /// Sources that do this are real: browsers and remote-desktop clients put a
    /// `PNG` representation on the board that no PNG decoder will read. Before
    /// the walk existed, all three shapes were copied and the worker quietly used
    /// the `CF_DIB`. Choosing the first shape that merely *rendered* would have
    /// ended those pastes; choosing the first shape that is structurally whole
    /// does not.
    ///
    /// MUTATION: drop the [`shape_is_intact`] call from the walk and every case
    /// here answers `Png` after one read.
    #[test]
    fn a_png_that_no_decoder_could_read_loses_to_the_bitmap_beside_it() {
        let mut truncated = png(8, 8);
        truncated.truncate(20); // partway through IHDR
        let mut bad_signature = png(8, 8);
        bad_signature[1] = b'Q';
        let mut no_pixels = png(8, 8);
        // An IDAT that declares eight bytes of pixels the source did not send.
        no_pixels.truncate(no_pixels.len() - 6);
        for broken in [
            bad_signature,
            truncated,
            no_pixels,
            png(0, 8),
            png(8, 0),
            png(MAX_PICTURE_SIDE + 1, 8),
            Vec::new(),
        ] {
            let mut board = Board::with([
                (PictureEncoding::Png, Some(broken.clone())),
                (PictureEncoding::DibV5, Some(dib_v5(8, 8))),
                (PictureEncoding::Dib, Some(dib(8, 8))),
            ]);
            assert_eq!(
                first_offered_picture(&WINDOWS_PICTURE_ORDER, &mut board),
                Candidate::Present(vec![PictureBytes {
                    encoding: PictureEncoding::DibV5,
                    bytes: dib_v5(8, 8),
                }])
            );
            // Two reads, not three: the `CF_DIB` behind the winner is never
            // rendered, which is the whole saving this walk exists for.
            assert_eq!(
                board.rendered,
                [PictureEncoding::Png, PictureEncoding::DibV5]
            );
            assert_eq!(board.copied, broken.len() + dib_v5(8, 8).len());
        }
        // And the whole PNG that the same board would otherwise offer still costs
        // exactly one read.
        let mut board = Board::with([
            (PictureEncoding::Png, Some(png(8, 8))),
            (PictureEncoding::DibV5, Some(dib_v5(8, 8))),
            (PictureEncoding::Dib, Some(dib(8, 8))),
        ]);
        assert_eq!(
            first_offered_picture(&WINDOWS_PICTURE_ORDER, &mut board),
            Candidate::Present(vec![PictureBytes {
                encoding: PictureEncoding::Png,
                bytes: png(8, 8),
            }])
        );
        assert_eq!(board.rendered, [PictureEncoding::Png]);
    }

    /// **A board on which nothing is whole still hands the worker the best thing
    /// that rendered**, so the paste ends in the worker's sentence about why and
    /// not in silence.
    ///
    /// `Absent` means "no picture" and falls to `Nothing`, which is a Ctrl+V that
    /// does nothing at all. A picture that is present and broken is not that, and
    /// before this change the reader said so — by handing over bytes no decoder
    /// could read and letting the decoder explain.
    #[test]
    fn a_board_where_nothing_is_whole_still_gives_the_worker_something_to_refuse() {
        let broken = vec![0x89, b'P', b'N', b'G', 9, 9];
        let mut board = Board::with([
            (PictureEncoding::Png, Some(broken.clone())),
            (PictureEncoding::DibV5, Some(vec![9; 40])),
            (PictureEncoding::Dib, Some(vec![9; 40])),
        ]);
        assert_eq!(
            first_offered_picture(&WINDOWS_PICTURE_ORDER, &mut board),
            Candidate::Present(vec![PictureBytes {
                encoding: PictureEncoding::Png,
                bytes: broken,
            }])
        );
        // Only here — the board where the old cost was the only way to an answer
        // — is every shape still read.
        assert_eq!(
            board.rendered,
            [
                PictureEncoding::Png,
                PictureEncoding::DibV5,
                PictureEncoding::Dib
            ]
        );
    }

    /// **The macOS order falls the same way**: a `public.png` that will not read
    /// loses to the `public.tiff` beside it, in two reads.
    #[test]
    fn a_broken_pasteboard_png_falls_to_the_tiff() {
        let mut board = Board::with([
            (PictureEncoding::Png, Some(vec![0x89, b'P', b'N', b'G'])),
            (PictureEncoding::Tiff, Some(tiff())),
        ]);
        assert_eq!(
            first_offered_picture(&MACOS_PICTURE_ORDER, &mut board),
            Candidate::Present(vec![PictureBytes {
                encoding: PictureEncoding::Tiff,
                bytes: tiff(),
            }])
        );
        assert_eq!(
            board.rendered,
            [PictureEncoding::Png, PictureEncoding::Tiff]
        );
        // A whole PNG on the same board is still the only shape read.
        let mut board = Board::with([
            (PictureEncoding::Png, Some(png(8, 8))),
            (PictureEncoding::Tiff, Some(tiff())),
        ]);
        assert_eq!(
            first_offered_picture(&MACOS_PICTURE_ORDER, &mut board),
            Candidate::Present(vec![PictureBytes {
                encoding: PictureEncoding::Png,
                bytes: png(8, 8),
            }])
        );
        assert_eq!(board.rendered, [PictureEncoding::Png]);
    }

    /// **What the door reads a header for, shape by shape** — each rule with the
    /// smallest change to a whole representation that breaks it.
    #[test]
    fn a_header_is_read_for_shape_and_never_for_pixels() {
        assert!(shape_is_intact(PictureEncoding::Png, &png(1, 1)));
        assert!(shape_is_intact(
            PictureEncoding::Png,
            &png(MAX_PICTURE_SIDE, MAX_PICTURE_SIDE)
        ));
        for (rule, broken) in [
            ("the signature", {
                let mut bytes = png(8, 8);
                bytes[7] = 0;
                bytes
            }),
            ("IHDR is the first chunk", {
                let mut bytes = png(8, 8);
                bytes[12..16].copy_from_slice(b"gAMA");
                bytes
            }),
            ("IHDR is thirteen bytes", {
                let mut bytes = png(8, 8);
                bytes[11] = 12;
                bytes
            }),
            ("a bit depth the spec names", {
                let mut bytes = png(8, 8);
                bytes[24] = 7;
                bytes
            }),
            ("a colour type the spec names", {
                let mut bytes = png(8, 8);
                bytes[25] = 5;
                bytes
            }),
            ("the one compression method", {
                let mut bytes = png(8, 8);
                bytes[26] = 1;
                bytes
            }),
            ("chunk types are letters", {
                let mut bytes = png(8, 8);
                bytes[37] = 0;
                bytes
            }),
            ("the pixels are there", {
                let mut bytes = png(8, 8);
                bytes.pop();
                bytes
            }),
        ] {
            assert!(
                !shape_is_intact(PictureEncoding::Png, &broken),
                "a PNG passed without {rule}"
            );
        }
        // A chunk chain that never reaches IDAT is bounded rather than walked.
        let mut endless = Vec::from(PNG_SIGNATURE);
        endless.extend_from_slice(&png(8, 8)[8..33]);
        for _ in 0..PNG_CHUNKS_BEFORE_PIXELS + 1 {
            endless.extend_from_slice(&0u32.to_be_bytes());
            endless.extend_from_slice(b"tEXt");
            endless.extend_from_slice(&[0; 4]);
        }
        assert!(!shape_is_intact(PictureEncoding::Png, &endless));

        for header in [40, 108, 124] {
            assert!(shape_is_intact(PictureEncoding::Dib, &bitmap(header, 4, 4)));
        }
        for (rule, broken) in [
            ("a header size Windows defines", {
                let mut bytes = dib(4, 4);
                bytes[0] = 41;
                bytes
            }),
            ("one plane", {
                let mut bytes = dib(4, 4);
                bytes[12] = 2;
                bytes
            }),
            ("a bit count a bitmap has", {
                let mut bytes = dib(4, 4);
                bytes[14] = 7;
                bytes
            }),
            ("a width", {
                let mut bytes = dib(4, 4);
                bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
                bytes
            }),
            ("a width that is not negative", {
                let mut bytes = dib(4, 4);
                bytes[4..8].copy_from_slice(&(-4i32).to_le_bytes());
                bytes
            }),
            ("a compression Windows defines", {
                let mut bytes = dib(4, 4);
                bytes[16] = 9;
                bytes
            }),
            ("the pixels the header describes", {
                let mut bytes = dib(4, 4);
                bytes.truncate(bytes.len() - 1);
                bytes
            }),
            ("a header that is all there", vec![40, 0, 0, 0, 1, 2]),
        ] {
            assert!(
                !shape_is_intact(PictureEncoding::Dib, &broken),
                "a bitmap passed without {rule}"
            );
        }
        // A top-down bitmap is an ordinary bitmap: the height is what may be
        // negative, and the pixel count is its magnitude.
        let mut top_down = dib(4, 4);
        top_down[8..12].copy_from_slice(&(-4i32).to_le_bytes());
        assert!(shape_is_intact(PictureEncoding::Dib, &top_down));
        // A fifty-byte global claiming a large picture is the header review X-4
        // named: believed, it asks for gigabytes; read, it fits nothing.
        let mut liar = dib(4, 4);
        liar[4..8].copy_from_slice(&4096u32.to_le_bytes());
        liar[8..12].copy_from_slice(&4096u32.to_le_bytes());
        assert!(!shape_is_intact(PictureEncoding::Dib, &liar));

        assert!(shape_is_intact(PictureEncoding::Tiff, &tiff()));
        assert!(shape_is_intact(
            PictureEncoding::Tiff,
            &[b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0]
        ));
        for (rule, broken) in [
            ("a byte order", vec![b'I', b'J', 0x2a, 0, 8, 0, 0, 0, 0, 0]),
            ("the answer 42", vec![b'I', b'I', 41, 0, 8, 0, 0, 0, 0, 0]),
            (
                "a directory inside the buffer",
                vec![b'I', b'I', 0x2a, 0, 99, 0, 0, 0, 0, 0],
            ),
            (
                "a directory after the header",
                vec![b'I', b'I', 0x2a, 0, 4, 0, 0, 0, 0, 0],
            ),
            ("eight bytes at all", vec![b'I', b'I', 0x2a]),
        ] {
            assert!(
                !shape_is_intact(PictureEncoding::Tiff, &broken),
                "a TIFF passed without {rule}"
            );
        }
    }

    /// The acquisition door as both platform arms assemble it: the rung walk of
    /// [`read_payload`] over a [`Board`] reached through [`first_offered_picture`].
    struct Door {
        files: Candidate<Vec<PathBuf>>,
        text: Candidate<String>,
        board: Board,
    }

    impl ClipboardPort for Door {
        fn begin(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn survey(&mut self) -> Result<ClipboardTypes, String> {
            Ok(ClipboardTypes {
                files: true,
                text: true,
                picture: true,
                promise: false,
            })
        }
        fn files(&mut self) -> Candidate<Vec<PathBuf>> {
            self.files.clone()
        }
        fn text(&mut self) -> Candidate<String> {
            self.text.clone()
        }
        fn picture(&mut self) -> Candidate<Vec<PictureBytes>> {
            first_offered_picture(&WINDOWS_PICTURE_ORDER, &mut self.board)
        }
        fn finish(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    fn shot(encoding: PictureEncoding) -> PictureBytes {
        PictureBytes {
            encoding,
            bytes: vec![0x89, b'P', b'N', b'G'],
        }
    }

    /// **The decision order, all four rungs at once** — files, then text, then a
    /// picture, then silence.
    ///
    /// The rule this pins is the one a reader states as "a screenshot pastes as
    /// a path *only* when there is nothing else on the clipboard": a source that
    /// puts both a picture and its own text on the board — every browser does —
    /// must still paste the text, and a source that puts a file beside a
    /// thumbnail of it must still paste the file's path.
    ///
    /// MUTATION: move the picture rung above the text rung and the second case
    /// fails; drop the `!pictures.is_empty()` guard and the fourth does.
    #[test]
    fn a_picture_is_read_only_when_no_file_and_no_text_answered_first() {
        let png = shot(PictureEncoding::Png);
        let cases: [(Candidate<Vec<PathBuf>>, Candidate<String>, ClipboardPayload); 4] = [
            (
                Candidate::Present(vec![PathBuf::from("/shot.png")]),
                Candidate::Present("/shot.png".to_owned()),
                ClipboardPayload::Files(vec![PathBuf::from("/shot.png")]),
            ),
            (
                Candidate::Absent,
                Candidate::Present("https://example.test/a.png".to_owned()),
                ClipboardPayload::Text("https://example.test/a.png".to_owned()),
            ),
            (
                Candidate::Absent,
                Candidate::Absent,
                ClipboardPayload::Picture(vec![png.clone()]),
            ),
            (
                Candidate::Absent,
                Candidate::Absent,
                ClipboardPayload::Nothing,
            ),
        ];
        for (index, (files, text, expected)) in cases.into_iter().enumerate() {
            let mut fake = Fake::new(files, text);
            // The last case is the clipboard that advertised a picture and then had
            // none to give: an empty list is not a payload, and nothing is said.
            fake.picture = if index == 3 {
                Candidate::Present(Vec::new())
            } else {
                Candidate::Present(vec![png.clone()])
            };
            assert_eq!(read_payload(&mut fake), Ok(expected));
        }
    }

    /// A picture rung that fails is a read that failed, not an empty clipboard:
    /// the reader is told, exactly as a failed file or text rung tells them.
    #[test]
    fn an_unreadable_picture_is_an_error_rather_than_silence() {
        let mut fake = Fake::new(Candidate::Absent, Candidate::Absent);
        fake.picture = Candidate::Unreadable("clipboard picture acquisition failed".into());
        assert_eq!(
            read_payload(&mut fake),
            Err("clipboard picture acquisition failed".to_owned())
        );
        assert!(!fake.held);
    }

    #[test]
    fn empty_hdrop_and_non_file_urls_fall_to_text() {
        for files in [
            Candidate::Present(Vec::new()),
            file_urls([Some("https://example.test/".into())]),
        ] {
            let mut fake = Fake::new(files, Candidate::Present("leaf name".into()));
            assert_eq!(
                read_payload(&mut fake),
                Ok(ClipboardPayload::Text("leaf name".into()))
            );
            assert_eq!(fake.fetched, ["files", "text"]);
        }
    }

    #[test]
    fn windows_delayed_render_succeeds_and_replacement_waits_for_next_gesture() {
        let mut fake = Fake::new(Candidate::Absent, Candidate::Present("first".into()));
        assert_eq!(
            read_payload(&mut fake),
            Ok(ClipboardPayload::Text("first".into()))
        );
        assert!(fake.sequence > 4);
        assert!(fake.competing_open_failed);
        assert!(fake.competing_copy("next"));
        fake.fetched.clear();
        assert_eq!(
            read_payload(&mut fake),
            Ok(ClipboardPayload::Text("next".into()))
        );
        assert_eq!(fake.opened, 2);
    }

    #[test]
    fn macos_changed_count_discards_without_reopening_and_equal_count_delivers() {
        for after in [7, 8] {
            let mut fake = Fake::new(Candidate::Absent, Candidate::Present("text".into()));
            fake.change_count = Some((7, after));
            assert_eq!(read_payload(&mut fake).is_ok(), after == 7);
            assert_eq!(fake.opened, 1);
            assert!(!fake.held);
        }
    }

    #[test]
    fn urls_keep_order_percent_bytes_and_never_put_content_in_errors() {
        assert_eq!(
            file_urls([Some("https://example.test".into())]),
            Candidate::Absent
        );
        assert_eq!(
            file_urls([
                Some("file:///first%20name".into()),
                Some("file:///second".into())
            ]),
            Candidate::Present(vec!["/first name".into(), "/second".into()])
        );
        assert_eq!(
            file_urls([None]),
            Candidate::Unreadable("file URL acquisition failed".into())
        );
        assert_eq!(
            file_urls([Some("file:///private%ZZ".into())]),
            Candidate::Unreadable("file URL could not be read".into())
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let Candidate::Present(paths) = file_urls([Some("file:///bad%FF".into())]) else {
                panic!("byte path lost")
            };
            assert_eq!(paths[0].as_os_str().as_bytes(), b"/bad\xff");
        }
    }
}
