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
    /// **One** encoding — the best one this source will hand over, chosen by the
    /// platform's preference list through [`first_offered_picture`]. The bytes
    /// are copied and nothing is decoded here: a screenshot is megabytes, the
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
/// A shape that is present and then fails to *decode* is not this function's
/// question: nothing is decoded here, and the clipboard is closed before the
/// worker looks. The worker still walks whatever list it is handed
/// (`bt_app::clipboard_picture::png_bytes`), so that behaviour is unchanged for
/// a list of one — a source that renders a `PNG` its own decoder cannot read now
/// ends the paste with that rung's reason instead of falling to `CF_DIB`.
pub fn first_offered_picture(
    order: &[PictureEncoding],
    source: &mut impl PictureSource,
) -> Candidate<Vec<PictureBytes>> {
    for &encoding in order {
        if !source.offers(encoding) {
            continue;
        }
        if let Some(bytes) = source.read(encoding) {
            return Candidate::Present(vec![PictureBytes { encoding, bytes }]);
        }
    }
    Candidate::Absent
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
        fn with(
            shapes: impl IntoIterator<Item = (PictureEncoding, Option<&'static [u8]>)>,
        ) -> Self {
            Self {
                shapes: shapes
                    .into_iter()
                    .map(|(encoding, bytes)| (encoding, bytes.map(<[u8]>::to_vec)))
                    .collect(),
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

    const PNG_BYTES: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2];
    const V5_BYTES: &[u8] = b"V5";
    const DIB_BYTES: &[u8] = b"D";

    /// One walk of a preference list, as the fallback cases below spell it out:
    /// what the board carries, which shapes it is asked to render, and which
    /// shape — if any — the walk comes back with.
    type Walk = (
        &'static [(PictureEncoding, Option<&'static [u8]>)],
        &'static [PictureEncoding],
        Option<(PictureEncoding, &'static [u8])>,
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
            (PictureEncoding::Png, Some(PNG_BYTES)),
            (PictureEncoding::DibV5, Some(V5_BYTES)),
            (PictureEncoding::Dib, Some(DIB_BYTES)),
        ]);
        let answer = first_offered_picture(&WINDOWS_PICTURE_ORDER, &mut board);
        // The count first, because it is the fact this repair is about.
        assert_eq!(board.rendered, [PictureEncoding::Png]);
        assert_eq!(board.asked, [PictureEncoding::Png]);
        // The budget, by construction: the bytes of the one shape that won, never
        // the sum of the shapes that were on offer.
        assert_eq!(board.copied, PNG_BYTES.len());
        assert_eq!(
            answer,
            Candidate::Present(vec![PictureBytes {
                encoding: PictureEncoding::Png,
                bytes: PNG_BYTES.to_vec(),
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
                &[
                    (PictureEncoding::DibV5, Some(V5_BYTES)),
                    (PictureEncoding::Dib, Some(DIB_BYTES)),
                ],
                &[PictureEncoding::DibV5],
                Some((PictureEncoding::DibV5, V5_BYTES)),
            ),
            // First choice present and unrenderable (lock failed, empty global,
            // past the ceiling): it is rendered, refuses, and the second wins.
            (
                &[
                    (PictureEncoding::Png, None),
                    (PictureEncoding::DibV5, Some(V5_BYTES)),
                ],
                &[PictureEncoding::Png, PictureEncoding::DibV5],
                Some((PictureEncoding::DibV5, V5_BYTES)),
            ),
            // Two in a row refuse: the walk reaches the last shape.
            (
                &[
                    (PictureEncoding::Png, None),
                    (PictureEncoding::DibV5, None),
                    (PictureEncoding::Dib, Some(DIB_BYTES)),
                ],
                &[
                    PictureEncoding::Png,
                    PictureEncoding::DibV5,
                    PictureEncoding::Dib,
                ],
                Some((PictureEncoding::Dib, DIB_BYTES)),
            ),
            // Only the worst shape is on offer, which is every source that is not
            // a screenshot tool.
            (
                &[(PictureEncoding::Dib, Some(DIB_BYTES))],
                &[PictureEncoding::Dib],
                Some((PictureEncoding::Dib, DIB_BYTES)),
            ),
            // Advertised by the survey, rendered by nobody: `Absent`, as today.
            (
                &[
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
            let mut board = Board::with(shapes.iter().copied());
            let answer = first_offered_picture(&WINDOWS_PICTURE_ORDER, &mut board);
            match winner {
                Some((encoding, bytes)) => {
                    assert_eq!(
                        answer,
                        Candidate::Present(vec![PictureBytes {
                            encoding,
                            bytes: bytes.to_vec(),
                        }])
                    );
                    assert_eq!(board.copied, bytes.len());
                }
                None => {
                    assert_eq!(answer, Candidate::Absent);
                    assert_eq!(board.copied, 0);
                }
            }
            assert_eq!(board.rendered, rendered);
            // A shape the board does not carry is asked about and never
            // rendered: the availability query is the cheap one. The walk stops
            // asking the moment one answers, so the questions are the preference
            // list up to and including the winner.
            let stop = winner.map_or(WINDOWS_PICTURE_ORDER.len(), |(encoding, _)| {
                WINDOWS_PICTURE_ORDER
                    .iter()
                    .position(|shape| *shape == encoding)
                    .expect("the winner came out of the preference list")
                    + 1
            });
            assert_eq!(board.asked, WINDOWS_PICTURE_ORDER[..stop]);
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
                    (PictureEncoding::Png, Some(PNG_BYTES)),
                    (PictureEncoding::DibV5, Some(V5_BYTES)),
                    (PictureEncoding::Dib, Some(DIB_BYTES)),
                ]),
            };
            assert_eq!(read_payload(&mut door), Ok(expected));
            assert!(door.board.asked.is_empty());
            assert!(door.board.rendered.is_empty());
            assert_eq!(door.board.copied, 0);
        }
        // And when nothing else answers, the picture rung hands the worker one
        // shape — the best one — out of the three that were on offer.
        let mut door = Door {
            files: Candidate::Absent,
            text: Candidate::Absent,
            board: Board::with([
                (PictureEncoding::DibV5, Some(V5_BYTES)),
                (PictureEncoding::Dib, Some(DIB_BYTES)),
            ]),
        };
        assert_eq!(
            read_payload(&mut door),
            Ok(ClipboardPayload::Picture(vec![PictureBytes {
                encoding: PictureEncoding::DibV5,
                bytes: V5_BYTES.to_vec(),
            }]))
        );
        assert_eq!(door.board.copied, V5_BYTES.len());
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
