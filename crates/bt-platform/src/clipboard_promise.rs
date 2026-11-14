//! **A clipboard promise, rendered into a folder** (plan row 81, T-PASTE-REFUSAL-SAYS-WHY Part B).
//!
//! A picture or a file copied from inside a program — an attachment in a mail or chat client, a
//! photo in Phone Link, a file inside a zip folder — reaches the clipboard as a *promise*: a
//! descriptor (`FileGroupDescriptorW`) listing the files, and their contents (`FileContents`),
//! rendered by the source when asked. The clipboard lane reads the descriptor and nothing else
//! ([`crate::ClipboardPayload::Promise`]); the bytes are asked for here, on a worker of their own,
//! and written into a folder the caller reserved, so what is pasted is the paths, exactly as a
//! copied file list is pasted.
//!
//! The rule for everything below: **the clipboard lane answers in a second or says why; anything
//! that renders goes to a worker of its own, as the picture save does.** A render can take
//! seconds (a mail attachment fetched from a server, a file unpacked out of an archive), and on
//! the lane it would run past the paste's deadline and hold every later copy and paste behind it.
//!
//! What is judged, and in which order ([`render_promise`]): the count, the declared sizes and
//! every name, before the source is asked for a byte; then the descriptor is read again and must
//! be the one the lane read (the clipboard sequence cannot say so — delayed rendering advances it
//! on a read that succeeded); then each file is written, the bytes counted as they arrive, because
//! a descriptor may declare less than its stream holds. Any refusal removes the folder it was
//! writing into, so a refused paste leaves nothing behind. After the last entry the descriptor is
//! read once more and must still be the one the lane read: a copy made while a slow render ran
//! would otherwise hand the paste the new promise's bytes under the old one's names.
//!
//! **A render is watched, not timed** (review N14): every byte that lands and every entry that is
//! finished advances its [`RenderProgress`], and the thread that watches it — the caller's —
//! gives the render up when nothing has advanced for its stall bound ([`RenderProgress::mark_given_up`]).
//! A render given up stops at its next step ([`PromiseRefused::Stalled`]); a source that never
//! returns from its call is left where it is, and the watcher has already answered.
//!
//! Windows only: macOS has no general-pasteboard promise any source is known to put there, and
//! its pasteboard is read on the main thread, where nothing may render.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::admission::WorkerCtx;
use crate::clipboard::PromisedEntry;

/// **How many entries one promise may list** — files and folders together. Sixty-four quoted
/// paths of a temporary folder stay inside the 8,191 characters a `cmd` line holds.
pub const MAX_PROMISED_ENTRIES: usize = 64;

/// **How many bytes one promise may write**, every file together — the clipboard's one ceiling
/// (`crate::clipboard::MAX_PICTURE_BYTES`). Past it the paste is refused rather than a temporary
/// folder being filled with something the person can save and copy themselves.
pub const MAX_PROMISED_BYTES: u64 = 256 * 1024 * 1024;

/// **What one paste may render**: [`MAX_PROMISED_ENTRIES`] and [`MAX_PROMISED_BYTES`] in the
/// product; smaller under test, so a cap is reached without writing a quarter of a gigabyte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PromiseLimits {
    pub entries: usize,
    pub bytes: u64,
}

/// The product's limits.
pub const PASTE_LIMITS: PromiseLimits = PromiseLimits {
    entries: MAX_PROMISED_ENTRIES,
    bytes: MAX_PROMISED_BYTES,
};

/// **The most bytes of a descriptor worth copying**: its header and 4,096 entries.
pub const MAX_DESCRIPTOR_BYTES: usize = 4 + 4_096 * FILE_DESCRIPTOR_BYTES;

/// The size of one `FILEDESCRIPTORW`: flags, class, size, point, attributes, three times, the
/// size's two halves and 260 UTF-16 units of name.
pub const FILE_DESCRIPTOR_BYTES: usize = 592;

/// `FD_ATTRIBUTES` and `FD_FILESIZE`: which of an entry's fields are meant.
const FD_ATTRIBUTES: u32 = 0x4;
const FD_FILESIZE: u32 = 0x40;
/// `FILE_ATTRIBUTE_DIRECTORY`.
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

/// **Why a promise was not written** — every word of it fit for a log line: kinds, counts and a
/// result code, never a name, a path or content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromiseRefused {
    /// More entries than the limit `cap`; `offered` is how many the descriptor lists.
    TooMany { offered: usize, cap: usize },
    /// More bytes than the limit, declared or streamed.
    TooLarge,
    /// A name that is empty, climbs out of the folder, names a device or carries a character a
    /// file name cannot, or two names that are one name to the file system.
    Name,
    /// The descriptor on the board is not the one the paste read.
    Changed,
    /// The source did not hand the bytes over: its call failed with this result code (a source
    /// that has quit lands here), or answered in a medium nothing here reads (`None`).
    NotRendered(Option<u32>),
    /// The file or folder could not be made or written, for this reason.
    WriteFailed(std::io::ErrorKind),
    /// The source handed nothing over for the watcher's stall bound, and the render was given up
    /// ([`RenderProgress::mark_given_up`]).
    Stalled,
}

impl PromiseRefused {
    /// The words a refusal line says (`paste refused: promise (<words>) …`).
    #[must_use]
    pub fn words(self) -> String {
        match self {
            Self::TooMany { offered, cap } => format!("too-many {offered} of {cap}"),
            Self::TooLarge => "too-large".to_owned(),
            Self::Name => "name".to_owned(),
            Self::Changed => "changed".to_owned(),
            Self::NotRendered(Some(code)) => format!("not-rendered 0x{code:08X}"),
            Self::NotRendered(None) => "not-rendered".to_owned(),
            Self::WriteFailed(kind) => format!("write-failed: {kind:?}"),
            Self::Stalled => "stalled".to_owned(),
        }
    }

    /// Whether the refusal is the size of what was copied, which the person can do something
    /// about by saving it first.
    #[must_use]
    pub fn is_too_large(self) -> bool {
        matches!(self, Self::TooMany { .. } | Self::TooLarge)
    }
}

/// **How far a render has come, and whether it was given up** (review N14) — shared by the
/// thread that renders and the one that watches it. Every byte written and every entry finished
/// advances [`Self::steps_taken`]; nothing here reads a clock: the watcher keeps the time, and when it
/// has seen no step for its bound it calls [`Self::mark_given_up`], after which the render's next step
/// refuses [`PromiseRefused::Stalled`].
#[derive(Debug, Default)]
pub struct RenderProgress {
    steps: AtomicU64,
    abandoned: AtomicBool,
}

impl RenderProgress {
    /// How many steps the render has taken: bytes written, plus one per entry finished.
    #[must_use]
    pub fn steps_taken(&self) -> u64 {
        self.steps.load(Ordering::Acquire)
    }

    /// The render is given up: its next step refuses [`PromiseRefused::Stalled`].
    pub fn mark_given_up(&self) {
        self.abandoned.store(true, Ordering::Release);
    }

    fn step_forward(&self, by: u64) {
        self.steps.fetch_add(by, Ordering::AcqRel);
    }

    /// Whether the render may take its next step.
    fn still_wanted(&self) -> Result<(), PromiseRefused> {
        if self.abandoned.load(Ordering::Acquire) {
            return Err(PromiseRefused::Stalled);
        }
        Ok(())
    }
}

/// **The source of a promise's bytes** — the OLE clipboard's data object in the product, a
/// scripted one under test.
pub trait PromisedContents {
    /// The descriptor as the source answers it now, asked on the render's worker (`worker`).
    fn descriptor(&mut self, worker: &WorkerCtx) -> Result<Vec<PromisedEntry>, PromiseRefused>;
    /// Entry `index`'s contents, written through `file`, on the render's worker (`worker`).
    fn write_contents(
        &mut self,
        worker: &WorkerCtx,
        index: usize,
        file: &mut PromisedFile<'_>,
    ) -> Result<(), PromiseRefused>;
}

/// **One promised file being written**: where it goes, what its entry declares, and the bytes the
/// paste has left. The file is made on its first write, with `create_new`, so nothing that was
/// there is ever written over.
pub struct PromisedFile<'a> {
    path: &'a Path,
    declared: Option<u64>,
    left: &'a mut u64,
    progress: &'a RenderProgress,
    file: Option<fs::File>,
    /// The bytes written into it so far.
    written: u64,
    stored: bool,
}

impl PromisedFile<'_> {
    /// Where the file goes — for a source that writes the file itself (a structured storage).
    #[must_use]
    pub fn path(&self) -> &Path {
        self.path
    }

    /// The size the descriptor declares for it, if it declares one.
    #[must_use]
    pub fn declared(&self) -> Option<u64> {
        self.declared
    }

    /// **Write these bytes next**, charged against what the paste has left: past it the write is
    /// refused before a byte of it lands.
    ///
    /// # Errors
    /// [`PromiseRefused::Stalled`] once the render was given up; [`PromiseRefused::TooLarge`] past
    /// the paste's bytes; [`PromiseRefused::WriteFailed`] when the file cannot be made or written.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), PromiseRefused> {
        self.progress.still_wanted()?;
        let length = bytes.len() as u64;
        if length > *self.left {
            return Err(PromiseRefused::TooLarge);
        }
        *self.left -= length;
        let file = match &mut self.file {
            Some(file) => file,
            None => self.file.insert(make(self.path)?),
        };
        file.write_all(bytes)
            .map_err(|error| PromiseRefused::WriteFailed(error.kind()))?;
        self.written += length;
        self.progress.step_forward(length);
        Ok(())
    }

    /// **The source wrote the whole file at [`Self::path`] itself**: its length is charged now,
    /// read on the render's worker (`worker`).
    ///
    /// # Errors
    /// [`PromiseRefused::Stalled`] once the render was given up; [`PromiseRefused::TooLarge`] past
    /// the paste's bytes; [`PromiseRefused::WriteFailed`] when the file is not there.
    pub fn written_by_the_source(&mut self, _worker: &WorkerCtx) -> Result<(), PromiseRefused> {
        self.progress.still_wanted()?;
        let length = fs::metadata(self.path)
            .map_err(|error| PromiseRefused::WriteFailed(error.kind()))?
            .len();
        if length > *self.left {
            return Err(PromiseRefused::TooLarge);
        }
        *self.left -= length;
        self.written = length;
        self.stored = true;
        self.progress.step_forward(length);
        Ok(())
    }

    /// **The entry is done**: a source that handed over fewer bytes than its entry declares did
    /// not render it (review N5 — a zero-length global, a stream that ended early); an empty file
    /// is still a file, and one whose source wrote nothing (and declared nothing more) is made now.
    /// A storage the source wrote itself is not measured against the declaration: `CopyTo` lays
    /// its sectors out afresh, so its length is not the source's.
    fn finish(self) -> Result<(), PromiseRefused> {
        if !self.stored
            && self
                .declared
                .is_some_and(|declared| self.written < declared)
        {
            return Err(PromiseRefused::NotRendered(None));
        }
        if self.file.is_none() && !self.stored {
            make(self.path)?;
        }
        self.progress.step_forward(1);
        Ok(())
    }
}

fn make(path: &Path) -> Result<fs::File, PromiseRefused> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| PromiseRefused::WriteFailed(error.kind()))
}

/// **A promise's descriptor, read out of its bytes** (`FILEGROUPDESCRIPTORW`: a count, then that
/// many 592-byte `FILEDESCRIPTORW`s), or `None` when the bytes are not one. A name that is not
/// valid UTF-16 reads as empty, which [`promised_path`] refuses.
#[must_use]
pub fn parse_file_group_descriptor(bytes: &[u8]) -> Option<Vec<PromisedEntry>> {
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    let count = usize::try_from(word(0)?).ok()?;
    let needed = count.checked_mul(FILE_DESCRIPTOR_BYTES)?.checked_add(4)?;
    if needed > bytes.len() {
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let at = 4 + index * FILE_DESCRIPTOR_BYTES;
        let flags = word(at)?;
        let attributes = word(at + 36)?;
        let size = (u64::from(word(at + 64)?) << 32) | u64::from(word(at + 68)?);
        let units: Vec<u16> = bytes[at + 72..at + FILE_DESCRIPTOR_BYTES]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        entries.push(PromisedEntry {
            name: String::from_utf16(&units).unwrap_or_default(),
            size: (flags & FD_FILESIZE != 0).then_some(size),
            folder: flags & FD_ATTRIBUTES != 0 && attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        });
    }
    Some(entries)
}

/// **The path inside the paste's folder a descriptor name stands for**, or `None` when the name
/// is not one a file may be given: every component (split on `\` and `/`) non-empty, not `.` or
/// `..`, free of `< > : " | ? *` and of controls, not ending in a space or a dot, not a device
/// name, at most 255 UTF-16 units. Nothing is renamed or guessed.
#[must_use]
pub fn promised_path(name: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for component in name.split(['\\', '/']) {
        if !component_is_a_file_name(component) {
            return None;
        }
        path.push(component);
    }
    Some(path)
}

fn component_is_a_file_name(component: &str) -> bool {
    !component.is_empty()
        && component != "."
        && component != ".."
        && component.encode_utf16().count() <= 255
        && !component
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        && !component.ends_with([' ', '.'])
        && !names_a_device(component)
}

/// `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM0`–`COM9`, `LPT0`–`LPT9` and their
/// superscript siblings — with or without an extension, which Windows ignores for them.
fn names_a_device(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let mut chars = stem.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    let rest: Vec<char> = chars.collect();
    matches!(prefix.as_str(), "COM" | "LPT")
        && matches!(rest.as_slice(), [c] if c.is_ascii_digit() || matches!(c, '¹' | '²' | '³'))
}

/// **The paths a promise is pasted as, judged before the source is asked for a byte**: the
/// count, the declared sizes and every name. Answers each entry's path inside the paste's folder.
///
/// # Errors
/// [`PromiseRefused::TooMany`], [`PromiseRefused::TooLarge`] or [`PromiseRefused::Name`].
pub fn judge_promise(
    offer: &[PromisedEntry],
    limits: PromiseLimits,
) -> Result<Vec<PathBuf>, PromiseRefused> {
    if offer.len() > limits.entries {
        return Err(PromiseRefused::TooMany {
            offered: offer.len(),
            cap: limits.entries,
        });
    }
    let declared = offer
        .iter()
        .filter(|entry| !entry.folder)
        .filter_map(|entry| entry.size)
        .try_fold(0_u64, u64::checked_add);
    if declared.is_none_or(|bytes| bytes > limits.bytes) {
        return Err(PromiseRefused::TooLarge);
    }
    let mut seen = std::collections::HashSet::new();
    offer
        .iter()
        .map(|entry| {
            let path = promised_path(&entry.name).ok_or(PromiseRefused::Name)?;
            // One name to the file system, whatever its case.
            if !seen.insert(entry.name.replace('/', "\\").to_lowercase()) {
                return Err(PromiseRefused::Name);
            }
            Ok(path)
        })
        .collect()
}

/// **A promise written**: the paths to paste — its top-level entries, in the descriptor's order —
/// and how many files and bytes were written, for the line that says so (no names).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedPromise {
    pub pasted: Vec<PathBuf>,
    pub files: usize,
    pub bytes: u64,
}

/// **Write the promise `offer` into `into`** — a folder the caller reserved for this paste — and
/// answer what was written ([`RenderedPromise`]). Any refusal removes `into`. A worker's door
/// (`worker`): every folder, file and stat below is the render's worker's, never the window's.
/// Each step advances `progress`, and a render its watcher gave up stops at the next one.
///
/// # Errors
/// Every [`PromiseRefused`].
pub fn render_promise(
    worker: &WorkerCtx,
    source: &mut impl PromisedContents,
    offer: &[PromisedEntry],
    into: &Path,
    limits: PromiseLimits,
    progress: &RenderProgress,
) -> Result<RenderedPromise, PromiseRefused> {
    let rendered = render_into(worker, source, offer, into, limits, progress);
    if rendered.is_err() {
        let _ = fs::remove_dir_all(into);
    }
    rendered
}

fn render_into(
    worker: &WorkerCtx,
    source: &mut impl PromisedContents,
    offer: &[PromisedEntry],
    into: &Path,
    limits: PromiseLimits,
    progress: &RenderProgress,
) -> Result<RenderedPromise, PromiseRefused> {
    let places = judge_promise(offer, limits)?;
    if source.descriptor(worker)? != offer {
        return Err(PromiseRefused::Changed);
    }
    let made = |result: std::io::Result<()>| {
        result.map_err(|error| PromiseRefused::WriteFailed(error.kind()))
    };
    let mut left = limits.bytes;
    let mut files = 0;
    for (index, (entry, place)) in offer.iter().zip(&places).enumerate() {
        progress.still_wanted()?;
        let path = into.join(place);
        if entry.folder {
            made(fs::create_dir_all(&path))?;
            progress.step_forward(1);
            continue;
        }
        if let Some(parent) = path.parent() {
            made(fs::create_dir_all(parent))?;
        }
        let mut file = PromisedFile {
            path: &path,
            declared: entry.size,
            left: &mut left,
            progress,
            file: None,
            written: 0,
            stored: false,
        };
        source.write_contents(worker, index, &mut file)?;
        file.finish()?;
        files += 1;
    }
    // **Still the promise the paste read** (review N2): asked once more after the last entry, so a
    // copy made while the entries were being written refuses the paste rather than handing it
    // the new promise's bytes under the old one's names.
    if source.descriptor(worker)? != offer {
        return Err(PromiseRefused::Changed);
    }
    progress.still_wanted()?;
    let mut pasted: Vec<PathBuf> = Vec::new();
    for place in &places {
        let Some(top) = place.components().next() else {
            continue;
        };
        let top = into.join(top);
        if !pasted.contains(&top) {
            pasted.push(top);
        }
    }
    Ok(RenderedPromise {
        pasted,
        files,
        bytes: limits.bytes - left,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn entry(name: &str, size: Option<u64>) -> PromisedEntry {
        PromisedEntry {
            name: name.to_owned(),
            size,
            folder: false,
        }
    }

    fn folder(name: &str) -> PromisedEntry {
        PromisedEntry {
            name: name.to_owned(),
            size: None,
            folder: true,
        }
    }

    /// **A scripted source**: its descriptor, each entry's bytes (or the code its render fails
    /// with), and the entries it was asked for.
    struct Fake {
        descriptor: Vec<PromisedEntry>,
        contents: Vec<Result<Vec<u8>, u32>>,
        asked: Vec<usize>,
        /// The descriptor the board holds once an entry has been asked for: a copy made while
        /// the render ran.
        copied_meanwhile: Option<Vec<PromisedEntry>>,
        /// The render's progress, given up between an entry's two writes: a watcher that gave
        /// up while the source was handing an entry over.
        gives_up: Option<std::sync::Arc<RenderProgress>>,
    }

    impl Fake {
        fn of(entries: &[PromisedEntry], contents: Vec<Result<Vec<u8>, u32>>) -> Self {
            Self {
                descriptor: entries.to_vec(),
                contents,
                asked: Vec::new(),
                copied_meanwhile: None,
                gives_up: None,
            }
        }
    }

    impl PromisedContents for Fake {
        fn descriptor(
            &mut self,
            _worker: &WorkerCtx,
        ) -> Result<Vec<PromisedEntry>, PromiseRefused> {
            match &self.copied_meanwhile {
                Some(copied) if !self.asked.is_empty() => Ok(copied.clone()),
                _ => Ok(self.descriptor.clone()),
            }
        }
        fn write_contents(
            &mut self,
            _worker: &WorkerCtx,
            index: usize,
            file: &mut PromisedFile<'_>,
        ) -> Result<(), PromiseRefused> {
            self.asked.push(index);
            match &self.contents[index] {
                // In two writes, so the budget is charged as a stream would charge it.
                Ok(bytes) => {
                    let (first, second) = bytes.split_at(bytes.len() / 2);
                    file.write(first)?;
                    if let Some(progress) = &self.gives_up {
                        progress.mark_given_up();
                    }
                    file.write(second)
                }
                Err(code) => Err(PromiseRefused::NotRendered(Some(*code))),
            }
        }
    }

    /// `body` on a worker the thread door started, as the render's worker is.
    pub(crate) fn on_a_worker<T: Send + 'static>(
        body: impl FnOnce(&WorkerCtx) -> T + Send + 'static,
    ) -> T {
        crate::spawn_at_priority("bt-promise-test", crate::ThreadPriority::Normal, body)
            .expect("start a worker for the render")
            .join()
            .expect("the worker answered")
    }

    fn scratch(tag: &str) -> PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-platform-promise-{tag}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the scratch folder is made");
        root
    }

    /// RED (plan row 81) — **a promise is written under its own names and pasted as its
    /// top-level entries**: two files and a folder holding one, in the descriptor's order, each
    /// file's bytes as the source handed them over.
    ///
    /// MUTATION: paste every entry's path instead of the top-level ones — the folder's file is
    /// pasted beside its folder.
    #[test]
    fn a_promise_is_written_under_its_own_names_and_pasted_by_its_top_entries() {
        on_a_worker(|worker| {
            let into = scratch("written");
            let offer = [
                entry("report 报告.pdf", Some(6)),
                folder("photos"),
                entry("photos\\a.png", None),
                entry("notes.txt", Some(0)),
            ];
            let mut source = Fake::of(
                &offer,
                vec![
                    Ok(b"pdf pdf".to_vec()),
                    Ok(Vec::new()),
                    Ok(b"png".to_vec()),
                    Ok(Vec::new()),
                ],
            );
            let rendered = render_promise(
                worker,
                &mut source,
                &offer,
                &into,
                PASTE_LIMITS,
                &RenderProgress::default(),
            )
            .expect("the promise is written");
            assert_eq!(
                (rendered.files, rendered.bytes),
                (3, 10),
                "three files, ten bytes"
            );
            assert_eq!(
                rendered.pasted,
                [
                    into.join("report 报告.pdf"),
                    into.join("photos"),
                    into.join("notes.txt")
                ]
            );
            assert_eq!(fs::read(into.join("report 报告.pdf")).unwrap(), b"pdf pdf");
            assert_eq!(fs::read(into.join("photos").join("a.png")).unwrap(), b"png");
            assert_eq!(fs::read(into.join("notes.txt")).unwrap(), b"");
            assert_eq!(
                source.asked,
                [0, 2, 3],
                "a folder entry is made, never read"
            );
            let _ = fs::remove_dir_all(&into);
        });
    }

    /// RED (plan row 81) — **a name a file may not be given refuses the whole promise before a
    /// byte is asked for**, and so do two names that are one name to the file system.
    ///
    /// MUTATION: let `component_is_a_file_name` accept `..` — the second row is written above the
    /// folder.
    #[test]
    fn a_name_a_file_may_not_be_given_refuses_the_whole_promise() {
        on_a_worker(|worker| {
            let long = "a".repeat(256);
            for name in [
                "",
                "..",
                "a\\..\\..\\b",
                ".",
                "\\rooted",
                "c:\\x",
                "a:b",
                "stream.txt:hidden",
                "CON",
                "con.txt",
                "Nul .log",
                "COM1",
                "lpt9.dat",
                "COM¹",
                "CONIN$",
                "trailing.",
                "trailing ",
                "tab\there",
                "what?",
                "star*",
                "pipe|",
                "quote\"",
                "<angle>",
                long.as_str(),
                "folder\\",
            ] {
                assert_eq!(
                    promised_path(name),
                    None,
                    "{name:?} is not a name a file may be given"
                );
            }
            for name in [
                "a.txt",
                "COM10",
                "console.log",
                "a/b.txt",
                "数据 1.csv",
                "..a",
            ] {
                assert!(promised_path(name).is_some(), "{name:?} is a file name");
            }
            let into = scratch("named");
            let offer = [entry("Same.txt", Some(1)), entry("same.TXT", Some(1))];
            let mut source = Fake::of(&offer, vec![Ok(b"a".to_vec()), Ok(b"b".to_vec())]);
            assert_eq!(
                render_promise(
                    worker,
                    &mut source,
                    &offer,
                    &into,
                    PASTE_LIMITS,
                    &RenderProgress::default(),
                ),
                Err(PromiseRefused::Name)
            );
            assert!(source.asked.is_empty(), "nothing was asked for");
            assert!(!into.exists(), "the folder went with the refusal");
        });
    }

    /// RED (plan row 81) — **the caps are judged on the declaration and again on the stream**: a
    /// descriptor listing too many entries, or declaring too many bytes, is refused before the
    /// source is asked; one declaring little and streaming more is stopped at the cap and its
    /// folder removed.
    ///
    /// MUTATION: charge only the declared sizes (drop the check in `PromisedFile::write`) — the
    /// lying stream is written whole.
    #[test]
    fn the_caps_are_judged_on_the_declaration_and_on_the_stream() {
        on_a_worker(|worker| {
            let limits = PromiseLimits {
                entries: 3,
                bytes: 10,
            };
            let many: Vec<PromisedEntry> = (0..4)
                .map(|index| entry(&format!("{index}.txt"), Some(1)))
                .collect();
            assert_eq!(
                judge_promise(&many, limits),
                Err(PromiseRefused::TooMany { offered: 4, cap: 3 })
            );
            assert_eq!(
                judge_promise(&many[..3], limits).map(|paths| paths.len()),
                Ok(3)
            );
            let heavy = [entry("a.bin", Some(10)), entry("b.bin", Some(1))];
            assert_eq!(judge_promise(&heavy, limits), Err(PromiseRefused::TooLarge));
            assert_eq!(
                judge_promise(
                    &[entry("a.bin", Some(u64::MAX)), entry("b.bin", Some(2))],
                    PASTE_LIMITS
                ),
                Err(PromiseRefused::TooLarge),
                "a sum that overflows is too large"
            );
            assert_eq!(
                (PASTE_LIMITS.entries, PASTE_LIMITS.bytes),
                (64, 256 * 1024 * 1024),
                "the product's limits are the ruled ones (R2)"
            );
            let into = scratch("lying");
            let offer = [entry("small.bin", Some(1)), entry("second.bin", Some(1))];
            let mut source = Fake::of(&offer, vec![Ok(vec![7; 8]), Ok(vec![7; 4])]);
            assert_eq!(
                render_promise(
                    worker,
                    &mut source,
                    &offer,
                    &into,
                    limits,
                    &RenderProgress::default(),
                ),
                Err(PromiseRefused::TooLarge)
            );
            assert_eq!(source.asked, [0, 1], "the second stream passed the cap");
            assert!(!into.exists(), "what was written went with the refusal");
        });
    }

    /// RED (plan row 81) — **the promise on the board must be the one the paste read**, and **a
    /// source that does not render an entry refuses the paste and leaves nothing**: the code it
    /// failed with is what the refusal says.
    ///
    /// MUTATIONS: skip the descriptor comparison — the changed board is written; keep the folder
    /// on a refusal (`render_promise` without its `remove_dir_all`) — the first file stays.
    #[test]
    fn a_changed_board_or_an_unrendered_entry_refuses_and_leaves_nothing() {
        on_a_worker(|worker| {
            let into = scratch("changed");
            let offer = [entry("a.txt", Some(1))];
            let mut source = Fake::of(&[entry("b.txt", Some(1))], vec![Ok(b"b".to_vec())]);
            assert_eq!(
                render_promise(
                    worker,
                    &mut source,
                    &offer,
                    &into,
                    PASTE_LIMITS,
                    &RenderProgress::default(),
                ),
                Err(PromiseRefused::Changed)
            );
            assert!(source.asked.is_empty());
            let into = scratch("unrendered");
            let offer = [entry("a.txt", Some(1)), entry("b.txt", Some(1))];
            let mut source = Fake::of(&offer, vec![Ok(b"a".to_vec()), Err(0x8001_0108)]);
            assert_eq!(
                render_promise(
                    worker,
                    &mut source,
                    &offer,
                    &into,
                    PASTE_LIMITS,
                    &RenderProgress::default(),
                ),
                Err(PromiseRefused::NotRendered(Some(0x8001_0108)))
            );
            assert!(!into.exists(), "the first file went with the refusal");
        });
    }

    /// RED (review N2) — **the board is read again after the last entry**: a copy made while the
    /// entries were being written refuses the paste as `changed`, and the folder goes with it.
    ///
    /// MUTATION: drop the descriptor read after the loop in `render_into` — the paste is written
    /// from the old promise's names.
    #[test]
    fn a_copy_made_while_the_entries_were_written_refuses_the_paste() {
        on_a_worker(|worker| {
            let into = scratch("copied-meanwhile");
            let offer = [entry("a.txt", Some(1)), entry("b.txt", Some(1))];
            let mut source = Fake::of(&offer, vec![Ok(b"a".to_vec()), Ok(b"b".to_vec())]);
            source.copied_meanwhile = Some(vec![
                entry("a.txt", Some(1)),
                entry("b.txt", Some(1)),
                entry("c.txt", None),
            ]);
            assert_eq!(
                render_promise(
                    worker,
                    &mut source,
                    &offer,
                    &into,
                    PASTE_LIMITS,
                    &RenderProgress::default(),
                ),
                Err(PromiseRefused::Changed)
            );
            assert_eq!(source.asked, [0, 1], "both entries were written first");
            assert!(!into.exists(), "what was written went with the refusal");
        });
    }

    /// RED (review N5) — **a source that hands over fewer bytes than its entry declares did not
    /// render it**: three of five declared bytes, or none of one, is `not-rendered` and leaves
    /// nothing; an entry that declares nothing, or declares nought, is written as handed over.
    ///
    /// MUTATION: drop the declared-size check in `PromisedFile::finish` — the short file is pasted.
    #[test]
    fn fewer_bytes_than_declared_is_not_rendered() {
        on_a_worker(|worker| {
            for (declared, handed) in [(5, 3), (1, 0)] {
                let into = scratch("short");
                let offer = [entry("short.bin", Some(declared))];
                let mut source = Fake::of(&offer, vec![Ok(vec![7; handed])]);
                assert_eq!(
                    render_promise(
                        worker,
                        &mut source,
                        &offer,
                        &into,
                        PASTE_LIMITS,
                        &RenderProgress::default(),
                    ),
                    Err(PromiseRefused::NotRendered(None)),
                    "{handed} of {declared} declared bytes"
                );
                assert!(!into.exists(), "the short file went with the refusal");
            }
            let into = scratch("undeclared");
            let offer = [entry("empty.txt", Some(0)), entry("any.bin", None)];
            let mut source = Fake::of(&offer, vec![Ok(Vec::new()), Ok(Vec::new())]);
            let rendered = render_promise(
                worker,
                &mut source,
                &offer,
                &into,
                PASTE_LIMITS,
                &RenderProgress::default(),
            )
            .expect("nothing declared, nothing owed");
            assert_eq!((rendered.files, rendered.bytes), (2, 0));
            let _ = fs::remove_dir_all(&into);
        });
    }

    /// RED (review N14) — **a render advances its progress at every step and stops at the next
    /// one once it is given up**: a written promise counts its bytes and its entries; a render
    /// whose watcher abandoned it asks the source for nothing more, refuses `stalled` and leaves
    /// nothing; one given up while the source is handing an entry over writes not a byte more.
    ///
    /// MUTATIONS: drop `progress.still_wanted()` from `render_into`'s loop — the abandoned render
    /// asks for its entry and is written; drop it from `PromisedFile::write` — the write after
    /// the give-up lands.
    #[test]
    fn a_render_advances_its_progress_and_stops_once_given_up() {
        on_a_worker(|worker| {
            let into = scratch("progress");
            let offer = [folder("d"), entry("d/a.txt", Some(3)), entry("b.txt", None)];
            let mut source = Fake::of(
                &offer,
                vec![Ok(Vec::new()), Ok(b"abc".to_vec()), Ok(b"xy".to_vec())],
            );
            let progress = RenderProgress::default();
            render_promise(worker, &mut source, &offer, &into, PASTE_LIMITS, &progress)
                .expect("the promise is written");
            assert_eq!(
                progress.steps_taken(),
                5 + 3,
                "five bytes and three entries"
            );
            let _ = fs::remove_dir_all(&into);

            let into = scratch("abandoned");
            let offer = [entry("a.txt", Some(1))];
            let mut source = Fake::of(&offer, vec![Ok(b"a".to_vec())]);
            let progress = RenderProgress::default();
            progress.mark_given_up();
            assert_eq!(
                render_promise(worker, &mut source, &offer, &into, PASTE_LIMITS, &progress),
                Err(PromiseRefused::Stalled)
            );
            assert!(source.asked.is_empty(), "nothing more was asked for");
            assert!(!into.exists(), "the folder went with the refusal");

            let into = scratch("given-up-mid-entry");
            let offer = [entry("a.txt", Some(2))];
            let progress = std::sync::Arc::new(RenderProgress::default());
            let mut source = Fake::of(&offer, vec![Ok(b"ab".to_vec())]);
            source.gives_up = Some(std::sync::Arc::clone(&progress));
            assert_eq!(
                render_promise(worker, &mut source, &offer, &into, PASTE_LIMITS, &progress),
                Err(PromiseRefused::Stalled)
            );
            assert_eq!(
                progress.steps_taken(),
                1,
                "the byte before the give-up, and not one after"
            );
            assert!(!into.exists(), "the folder went with the refusal");
        });
    }

    /// PIN (plan row 81) — **a refusal's words carry kinds, counts and codes only.**
    #[test]
    fn a_refusals_words_are_kinds_and_counts() {
        for (refused, words) in [
            (
                PromiseRefused::TooMany {
                    offered: 120,
                    cap: 64,
                },
                "too-many 120 of 64",
            ),
            (PromiseRefused::TooLarge, "too-large"),
            (PromiseRefused::Name, "name"),
            (PromiseRefused::Changed, "changed"),
            (
                PromiseRefused::NotRendered(Some(0x8004_01D3)),
                "not-rendered 0x800401D3",
            ),
            (PromiseRefused::NotRendered(None), "not-rendered"),
            (PromiseRefused::Stalled, "stalled"),
            (
                PromiseRefused::WriteFailed(std::io::ErrorKind::StorageFull),
                "write-failed: StorageFull",
            ),
        ] {
            assert_eq!(refused.words(), words);
        }
    }

    /// One `FILEDESCRIPTORW`, laid out as the shell writes it.
    pub(crate) fn descriptor_bytes(entries: &[(u32, u32, u64, &[u16])]) -> Vec<u8> {
        let count = u32::try_from(entries.len()).unwrap();
        let mut bytes = count.to_le_bytes().to_vec();
        for (flags, attributes, size, name) in entries {
            let mut one = vec![0_u8; FILE_DESCRIPTOR_BYTES];
            one[0..4].copy_from_slice(&flags.to_le_bytes());
            one[36..40].copy_from_slice(&attributes.to_le_bytes());
            one[64..68].copy_from_slice(&u32::try_from(size >> 32).unwrap().to_le_bytes());
            one[68..72].copy_from_slice(&u32::try_from(size & 0xFFFF_FFFF).unwrap().to_le_bytes());
            for (at, unit) in name.iter().enumerate() {
                one[72 + 2 * at..74 + 2 * at].copy_from_slice(&unit.to_le_bytes());
            }
            bytes.extend(one);
        }
        bytes
    }

    /// RED (plan row 81) — **a descriptor reads as the shell writes it**: the size only under
    /// `FD_FILESIZE`, a folder only under `FD_ATTRIBUTES`, a name up to its terminator, a name
    /// that is not UTF-16 as empty, and bytes too short for the count they declare as nothing.
    ///
    /// MUTATION: read the size without its flag — the second entry declares a size.
    #[test]
    fn a_descriptor_reads_as_the_shell_writes_it() {
        let name = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        let (a, b, c) = (name("a 文件.pdf"), name("dir"), name("x"));
        let bytes = descriptor_bytes(&[
            (FD_FILESIZE, 0, 5_000_000_000, &a),
            (0, 0, 7, &c),
            (FD_ATTRIBUTES, FILE_ATTRIBUTE_DIRECTORY, 0, &b),
            (FD_ATTRIBUTES, 0, 0, &[0xD800, u16::from(b'x')]),
        ]);
        assert_eq!(
            parse_file_group_descriptor(&bytes),
            Some(vec![
                entry("a 文件.pdf", Some(5_000_000_000)),
                entry("x", None),
                folder("dir"),
                entry("", None),
            ])
        );
        assert_eq!(parse_file_group_descriptor(&bytes[..bytes.len() - 1]), None);
        assert_eq!(parse_file_group_descriptor(&[1, 0]), None);
        assert_eq!(
            parse_file_group_descriptor(&0_u32.to_le_bytes()),
            Some(Vec::new())
        );
    }
}
