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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PictureEncoding {
    Png,
    DibV5,
    Dib,
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
    fn finish(&mut self) -> Result<(), String>;
}

/// The terminal context menu may survey types on the press that opens it, never fetch content.
/// A failed survey disables the future row without hiding it; ordinary Paste stays enabled.
/// T-PASTE-2 owns that row and the picture acquisition it enables; T-PASTE-1 adds neither.
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
        // No picture bytes are fetched in T-PASTE-1. A promise is only an advertised refusal.
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
                assert_eq!(
                    fake.fetched.len(),
                    if matches!(&file, Candidate::Unreadable(_))
                        || matches!(&file, Candidate::Present(f) if !f.is_empty())
                    {
                        1
                    } else {
                        2
                    }
                );
            }
        }
    }

    #[test]
    fn survey_selects_without_fetching_and_picture_only_is_silent_nothing() {
        let mut fake = Fake::new(
            Candidate::Present(vec!["not advertised".into()]),
            Candidate::Unreadable("not advertised".into()),
        );
        fake.types.files = false;
        fake.types.text = false;
        assert_eq!(read_payload(&mut fake), Ok(ClipboardPayload::Nothing));
        assert!(fake.fetched.is_empty());
        fake.types.promise = true;
        assert_eq!(
            read_payload(&mut fake),
            Ok(ClipboardPayload::Refused(UnsupportedKind::Promise))
        );
        assert!(picture_paste_enabled(Ok(fake.types), true));
        assert!(!picture_paste_enabled(Err("busy".into()), true));
        assert!(!picture_paste_enabled(Ok(fake.types), false));
        fake.types.picture = false;
        assert!(!picture_paste_enabled(Ok(fake.types), true));
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
