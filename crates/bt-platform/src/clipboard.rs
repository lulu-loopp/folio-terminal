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
/// the order [`ClipboardPort::picture`] is asked to list its answers in, and a
/// reader takes the first one it can turn into a file.
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
    /// Every encoding the source offers, best first — see [`PictureEncoding`].
    /// The bytes are copied and nothing is decoded here: a screenshot is
    /// megabytes, the caller is the event-loop thread, and turning those bytes
    /// into a picture is the picture worker's job.
    fn picture(&mut self) -> Candidate<Vec<PictureBytes>>;
    fn finish(&mut self) -> Result<(), String>;
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
