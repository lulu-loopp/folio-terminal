//! **The storage folder's named, shallow ear** (§7.1.6c-6d;
//! `docs/plans/design/settings-reread-2026-10-11.md` §2.2 and revision (b)9).
//!
//! `%APPDATA%\Folio\` holds four documents a person may edit by hand —
//! `settings.json`, `keybindings.json`, `profiles.json` and `pins.json`. One
//! member on the watch steward listens to the folder's own entries, by name
//! ([`Depth::ShallowNamed`]). The `schemes` subfolder has its own ear
//! ([`crate::scheme_watch`]), and a shallow watch does not hear inside it.
//!
//! # Names are filtered on the watcher thread
//!
//! [`documents_named`] reduces each report to the documents it names, the way
//! the program and index ears filter theirs. A name counts whichever half of a
//! rename it is, so an editor's rename-into-place counts; the temporaries that
//! Folio's own writer, editors and sync clients use (`<name>.tmp-…`,
//! `<name>~`, `.syncthing.<name>.tmp`) and the kept `.rejected-…` /
//! `.displaced-…` copies name nothing. So a `session.json` flush, which is a
//! write in this folder every 1.5 s while a layout changes, wakes nothing. Names
//! compare with ASCII case folding on every platform: the volume decides what
//! names one file, and a write through any spelling of it reports the spelling
//! on disk. A short (8.3) name cannot be told apart from the four, so one that
//! looks like a JSON name stamps all four. An overflow (`Unknown`) stamps all
//! four; the steward's terminal `Ended` stamps none.
//!
//! # One slot per document, one clock for the folder
//!
//! The mailbox keeps the latest [`Instant`] per document, not a queue: every
//! report about one document says the same thing. One [`WatchClock`] serves
//! the folder, so a `git checkout` that rewrites all four is one burst and one
//! pass, not four.
//!
//! # What a stamp is worth
//!
//! A stamp says a document *may* have changed. Whether it did is answered by
//! re-reading it — on the storage worker, never here (`document_request`) — and
//! comparing its bytes with the store's revision — the bytes
//! Folio last read from it or wrote to it ([`persist::Revision`]) — because
//! this window's own writes name the same documents and the comparison is what
//! tells them from a hand edit. The document in memory is never the
//! comparison: a change the store holds but has not written is not undone by a
//! read of the older file. The pass re-reads every stamped document: all four
//! are followed live (R33; owner ruling Q3, 2026-10-11) —
//! `settings.json` merged through its rows' doors, `keybindings.json` merged
//! by action and chord into the one shortcut table, `profiles.json` and
//! `pins.json` merged by row and by pin, their tables put in force after.
//!
//! # Arming, failure and silence
//!
//! A first [`MemberReport::Armed`] owes a pass over all four: the documents
//! were read when their stores opened, before the steward was listening, and a
//! write in between reached nobody. That debt is the window thread's — it goes
//! straight into the pass's pending set and the clock — so the mailbox keeps
//! one writer, the listener. A `Failed` member is tried again once at the next
//! reader-triggered event — a store accepting a write of one of the four — and
//! never because time passed. [`StorageWatch::advance`] hears the steward first
//! and only then compares the stores' write generations, so a `Failed`
//! published in the same turn as the first accepted write is retried by that
//! write. Silence starts no clock and schedules no wake. Where `DirWatch` refuses (the portable
//! arm), the member stays `Failed` and the documents are the ones read at
//! start.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use bt_platform::DirChange;
use winit::event_loop::EventLoopProxy;

use crate::watch_clock::WatchClock;
use crate::watch_steward::{Depth, EarHandle, Listener, MemberReport};
use crate::{AppEvent, persist, pins};

/// One of the storage folder's four hand-editable documents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Document {
    Settings,
    Keybindings,
    Profiles,
    Pins,
}

impl Document {
    const ALL: [Self; 4] = [
        Self::Settings,
        Self::Keybindings,
        Self::Profiles,
        Self::Pins,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Settings => 0,
            Self::Keybindings => 1,
            Self::Profiles => 2,
            Self::Pins => 3,
        }
    }

    const fn bit(self) -> u8 {
        1 << self.index()
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Settings => persist::SETTINGS_FILE_NAME,
            Self::Keybindings => persist::KEYBINDINGS_FILE_NAME,
            Self::Profiles => persist::PROFILES_FILE_NAME,
            Self::Pins => pins::PINS_FILE_NAME,
        }
    }
}

/// A set of the four documents: the ones a report names, or the ones a pass is
/// owed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DocumentSet(u8);

impl DocumentSet {
    const ALL: Self = Self(0b1111);

    fn insert(&mut self, document: Document) {
        self.0 |= document.bit();
    }

    fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    #[must_use]
    pub(crate) const fn contains(self, document: Document) -> bool {
        self.0 & document.bit() != 0
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// **Which of the four documents a shallow, named report may have changed.**
///
/// Pure, and run on the watcher thread. `Named` stamps each document whose
/// name appears, compared with ASCII case folding; a name containing `~` whose
/// extension folds to `.JSO` or `.JSON` is a short (8.3) name and stamps all
/// four. `Unknown` stamps all four; `Ended` stamps none.
pub(crate) fn documents_named(change: DirChange<'_>) -> DocumentSet {
    match change {
        DirChange::Unknown => DocumentSet::ALL,
        DirChange::Ended => DocumentSet::default(),
        DirChange::Named(names) => {
            let mut documents = DocumentSet::default();
            for name in names {
                let name = name.to_string_lossy();
                if is_short_json_name(&name) {
                    return DocumentSet::ALL;
                }
                for document in Document::ALL {
                    if name.eq_ignore_ascii_case(document.name()) {
                        documents.insert(document);
                    }
                }
            }
            documents
        }
    }
}

/// A name containing `~` whose extension folds to `.JSO` or `.JSON` — the shape
/// a volume's 8.3 name for any of the four takes.
fn is_short_json_name(name: &str) -> bool {
    if !name.contains('~') {
        return false;
    }
    let Some(dot) = name.rfind('.') else {
        return false;
    };
    let extension = &name[dot..];
    (4..=5).contains(&extension.len()) && ".JSON"[..extension.len()].eq_ignore_ascii_case(extension)
}

/// The latest time each document was named, written on the watcher thread.
#[derive(Clone, Copy, Debug, Default)]
struct Mailbox([Option<Instant>; 4]);

impl Mailbox {
    fn stamp(&mut self, documents: DocumentSet, at: Instant) {
        for document in Document::ALL {
            if documents.contains(document) {
                self.0[document.index()] = Some(at);
            }
        }
    }
}

/// The storage folder's steward member, its per-document mailbox and its one
/// clock.
#[derive(Default)]
pub struct StorageWatch {
    /// One writer, the listener on the watcher thread; drained by
    /// [`Self::advance`] on the window thread, which never writes it.
    news: Arc<Mutex<Mailbox>>,
    /// The documents owed by the next pass, waiting for the clock: the drained
    /// mailbox stamps, and all four after a first `Armed`. Window thread only.
    pending: DocumentSet,
    clock: WatchClock,
    /// The folder's ear on the watch steward, opened by [`Self::arm`].
    ear: Option<EarHandle>,
    /// The folder the ear asks for.
    directory: Option<PathBuf>,
    /// What the steward last said: `None` before its first answer, `Some(true)`
    /// while the folder is heard, `Some(false)` while it is not.
    heard: Option<bool>,
    /// The four stores' write generations as last seen, in [`Document`] order.
    write_generations: [u64; 4],
}

impl StorageWatch {
    /// Open the folder's ear, once, after the four stores have opened.
    /// `write_generations` are the stores' generations at that moment.
    pub fn arm(&mut self, proxy: &EventLoopProxy<AppEvent>, write_generations: [u64; 4]) {
        let directory = persist::storage_dir();
        let news = Arc::clone(&self.news);
        let proxy = proxy.clone();
        self.arm_with(&directory, write_generations, move || {
            crate::watch_steward::open_ear(
                Depth::ShallowNamed,
                listener(&news, move || {
                    // The loop is woken, not told what to do.
                    let _ = proxy.send_event(AppEvent::StorageChanged);
                }),
            )
        });
    }

    /// [`Self::arm`] with the opening of the ear handed in — the product's
    /// steward, or a test's.
    fn arm_with(
        &mut self,
        directory: &Path,
        write_generations: [u64; 4],
        open_ear: impl FnOnce() -> EarHandle,
    ) {
        self.write_generations = write_generations;
        self.directory = Some(directory.to_path_buf());
        let ear = self.ear.get_or_insert_with(open_ear);
        ear.want(&BTreeSet::from([directory.to_path_buf()]), BTreeSet::new());
    }

    /// A moved write generation while the folder is unheard is the
    /// reader-triggered event the steward's `Failed` rule waits for: the member
    /// is tried again once.
    fn reader_writes(&mut self, write_generations: [u64; 4]) {
        if !self.retry_owed(write_generations) {
            return;
        }
        let (Some(ear), Some(directory)) = (self.ear.as_mut(), self.directory.as_ref()) else {
            return;
        };
        let wanted = BTreeSet::from([directory.clone()]);
        ear.want(&wanted, wanted.clone());
    }

    /// Whether these generations owe the unheard folder a retry: a turn whose
    /// generations did not move owes nothing, and neither does a heard folder.
    fn retry_owed(&mut self, write_generations: [u64; 4]) -> bool {
        let moved = self.write_generations != write_generations;
        self.write_generations = write_generations;
        moved && self.heard == Some(false)
    }

    /// **One turn of the storage watch, and the documents one due pass is
    /// owed** — empty while no pass is due.
    ///
    /// In this order: the steward's reports first (a first `Armed` owes all
    /// four; a `Failed` makes the folder unheard), then the stores' write
    /// generations (`write_generations`, in [`Document`] order — a store's
    /// moves when it accepts a write the reader asked for, and a moved one
    /// while the folder is unheard retries it), then the mailbox, then the
    /// clock. Hearing first is what lets the write that arrives in the same
    /// turn as a just-published `Failed` retry it.
    pub fn advance(&mut self, now: Instant, write_generations: [u64; 4]) -> DocumentSet {
        self.hear_the_steward(now);
        self.reader_writes(write_generations);
        let news = std::mem::take(&mut *lock(&self.news));
        for document in Document::ALL {
            if let Some(at) = news.0[document.index()] {
                self.pending.insert(document);
                self.clock.note_event(at);
            }
        }
        if self.clock.take_due(now) {
            self.pending.take()
        } else {
            DocumentSet::default()
        }
    }

    /// When the loop must wake to answer news it already has, if it has any.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.clock.due_at()
    }

    /// Fold in what the steward said about the folder: a first `Armed` (or one
    /// after `Failed`) owes a pass over all four, noted in the window thread's
    /// own pending set and clock; a transition to `Failed` is said once.
    fn hear_the_steward(&mut self, now: Instant) {
        let Some(ear) = self.ear.as_ref() else {
            return;
        };
        for (directory, numbered) in ear.take_reports() {
            if self.directory.as_ref() != Some(&directory) {
                continue;
            }
            match numbered.report {
                MemberReport::Armed => {
                    if self.heard != Some(true) {
                        self.pending = DocumentSet::ALL;
                        self.clock.note_event(now);
                    }
                    self.heard = Some(true);
                }
                MemberReport::Failed(cause) => {
                    if self.heard != Some(false) {
                        eprintln!(
                            "recoverable storage watch failure on {}: {cause}",
                            directory.display()
                        );
                    }
                    self.heard = Some(false);
                }
            }
        }
    }

    /// Whether the steward says the folder is heard.
    #[cfg(test)]
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.heard == Some(true)
    }

    /// **The listener this watch's ear is opened with**, its clock handed in —
    /// for a test outside this module that names documents on the watcher
    /// thread's side of the mailbox and drives the folder's one clock without a
    /// steward.
    #[cfg(test)]
    pub(crate) fn listener_on(
        &self,
        now: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> Listener {
        listener_at(&self.news, || {}, now)
    }
}

fn listener(news: &Arc<Mutex<Mailbox>>, wake: impl Fn() + Send + Sync + 'static) -> Listener {
    listener_at(news, wake, Instant::now)
}

/// The listener with its clock handed in — the product's `Instant::now`, or a
/// test's fake time.
fn listener_at(
    news: &Arc<Mutex<Mailbox>>,
    wake: impl Fn() + Send + Sync + 'static,
    now: impl Fn() -> Instant + Send + Sync + 'static,
) -> Listener {
    let news = Arc::clone(news);
    Arc::new(move |_, change| {
        let documents = documents_named(change);
        if documents.is_empty() {
            return;
        }
        lock(&news).stamp(documents, now());
        wake();
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lane::wait_for_a_wake;
    use crate::watch_clock::{WATCH_FLOOR, WATCH_QUIET};
    use crate::watch_steward::Steward;
    use crate::watch_steward::fake::FakeDoor;
    use std::ffi::OsString;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    fn named(names: &[&str]) -> Vec<OsString> {
        names.iter().map(OsString::from).collect()
    }

    fn set(documents: &[Document]) -> DocumentSet {
        let mut set = DocumentSet::default();
        for &document in documents {
            set.insert(document);
        }
        set
    }

    fn stamps(names: &[&str]) -> DocumentSet {
        documents_named(DirChange::Named(&named(names)))
    }

    /// A storage ear over the fake steward door, its listener on a fake clock.
    struct Rig {
        watch: StorageWatch,
        steward: &'static Steward,
        door: FakeDoor,
        wakes: mpsc::Receiver<()>,
        directory: PathBuf,
        listener_now: Arc<Mutex<Instant>>,
        /// How many times the listener woke the loop.
        loop_wakes: Arc<AtomicUsize>,
        /// The four stores' write generations a turn hands over.
        generations: [u64; 4],
    }

    impl Rig {
        fn new(start: Instant) -> Self {
            let door = FakeDoor::default();
            let steward = Box::leak(Box::new(Steward::new(door.clone(), |_| {})));
            let (woke, wakes) = mpsc::channel();
            steward.install_wake(move || {
                let _ = woke.send(());
            });
            Self {
                watch: StorageWatch::default(),
                steward,
                door,
                wakes,
                directory: std::env::temp_dir()
                    .join("folio-storage-ear")
                    .join("项目")
                    .join("folio 笔记"),
                listener_now: Arc::new(Mutex::new(start)),
                loop_wakes: Arc::new(AtomicUsize::new(0)),
                generations: [0; 4],
            }
        }

        fn arm(&mut self, generations: [u64; 4]) {
            let steward = self.steward;
            let news = Arc::clone(&self.watch.news);
            let listener_now = Arc::clone(&self.listener_now);
            let loop_wakes = Arc::clone(&self.loop_wakes);
            self.watch.arm_with(&self.directory, generations, || {
                steward.open_ear(
                    Depth::ShallowNamed,
                    listener_at(
                        &news,
                        move || {
                            loop_wakes.fetch_add(1, Ordering::SeqCst);
                        },
                        move || *lock(&listener_now),
                    ),
                )
            });
        }

        /// Arm, hear the steward's `Armed`, and spend the covering pass it owes,
        /// at `start + WATCH_QUIET`.
        fn armed_and_settled(start: Instant) -> Self {
            let mut rig = Self::new(start);
            rig.arm([0; 4]);
            wait_for_a_wake(&rig.wakes, "the storage ear arming");
            assert_eq!(rig.due(start), DocumentSet::default());
            assert_eq!(rig.due(start + WATCH_QUIET), DocumentSet::ALL);
            rig
        }

        /// One turn of the loop at `now`, with the stores' current generations.
        fn due(&mut self, now: Instant) -> DocumentSet {
            self.watch.advance(now, self.generations)
        }

        /// The kernel names `names` at `at`, on the watcher thread.
        fn kernel_names(&self, at: Instant, names: &[&str]) {
            *lock(&self.listener_now) = at;
            self.door.named(&self.directory, &named(names));
        }
    }

    /// RED (T-STORAGE-EAR-NAMED; design note §8 test 1) — a hand edit to one
    /// document while Folio runs is one pass a quiet window later, owing that
    /// document and no other.
    /// MUTATION: `documents_named` drops `settings.json`, or the stamp is not
    /// folded into the clock.
    #[test]
    fn a_hand_edit_while_running_is_read_within_the_clock() {
        let start = Instant::now();
        let mut rig = Rig::armed_and_settled(start);
        let edit = start + WATCH_QUIET + WATCH_FLOOR;
        rig.kernel_names(edit, &["settings.json.tmp-更纱黑体", "settings.json"]);
        assert_eq!(rig.due(edit), DocumentSet::default());
        assert_eq!(rig.watch.deadline(), Some(edit + WATCH_QUIET));
        assert_eq!(
            rig.due(edit + WATCH_QUIET - Duration::from_millis(1)),
            DocumentSet::default()
        );
        assert_eq!(rig.due(edit + WATCH_QUIET), set(&[Document::Settings]));
        assert_eq!(rig.watch.deadline(), None);
    }

    /// RED (T-STORAGE-EAR-NAMED) — a `session.json` flush (E-3: the temporary's
    /// add and modify, the target's removal, the rename) stamps nothing, wakes
    /// nothing and starts no clock.
    /// MUTATION: the filter answers all four for any name.
    #[test]
    fn a_session_flush_wakes_nothing() {
        let flush: [&[&str]; 4] = [
            &["session.json.tmp-4242-1700000000000000000-0"],
            &["session.json.tmp-4242-1700000000000000000-0"],
            &["session.json"],
            &[
                "session.json.tmp-4242-1700000000000000000-0",
                "session.json",
            ],
        ];
        for names in flush {
            assert_eq!(stamps(names), DocumentSet::default(), "{names:?}");
        }

        let start = Instant::now();
        let mut rig = Rig::armed_and_settled(start);
        let woken = rig.loop_wakes.load(Ordering::SeqCst);
        let at = start + WATCH_QUIET + WATCH_FLOOR;
        for names in flush {
            rig.kernel_names(at, names);
        }
        assert_eq!(rig.loop_wakes.load(Ordering::SeqCst), woken);
        assert_eq!(rig.due(at), DocumentSet::default());
        assert_eq!(rig.watch.deadline(), None);
    }

    /// RED (T-STORAGE-EAR-NAMED) — the first `Armed` covers the gap between the
    /// stores' opening reads and the steward listening: one pass owing all four.
    /// MUTATION: `Armed` stamps nothing.
    #[test]
    fn a_first_armed_reads_all_four() {
        let start = Instant::now();
        let mut rig = Rig::new(start);
        rig.arm([0; 4]);
        wait_for_a_wake(&rig.wakes, "the storage ear arming");
        assert_eq!(rig.due(start), DocumentSet::default());
        assert!(rig.watch.is_armed());
        assert_eq!(rig.watch.deadline(), Some(start + WATCH_QUIET));
        assert_eq!(rig.due(start + WATCH_QUIET), DocumentSet::ALL);
        assert_eq!(rig.due(start + WATCH_QUIET), DocumentSet::default());
        assert_eq!(rig.watch.deadline(), None);
    }

    /// RED (T-STORAGE-EAR-NAMED round 2) — the window thread never writes the
    /// mailbox: hearing a first `Armed` puts the pass over all four in the
    /// window's own pending set and clock, and the mailbox stays the listener's.
    /// MUTATION: stamp the `Armed` debt into the mailbox (two writers).
    #[test]
    fn hearing_armed_leaves_the_mailbox_to_the_listener() {
        let start = Instant::now();
        let mut rig = Rig::new(start);
        rig.arm([0; 4]);
        wait_for_a_wake(&rig.wakes, "the storage ear arming");
        rig.watch.hear_the_steward(start);
        assert!(lock(&rig.watch.news).0.iter().all(Option::is_none));
        assert_eq!(rig.watch.pending, DocumentSet::ALL);
        assert_eq!(rig.watch.deadline(), Some(start + WATCH_QUIET));
    }

    /// RED (T-STORAGE-EAR-NAMED round 2) — a `Failed` the steward published
    /// but the window has not heard yet, met in the same turn by the first
    /// accepted document write, is retried by that write.
    /// MUTATION: compare the write generations before hearing the steward.
    #[test]
    fn a_failure_heard_in_the_same_turn_as_the_first_write_is_retried_by_it() {
        let start = Instant::now();
        let mut rig = Rig::armed_and_settled(start);
        assert_eq!(rig.door.attempts(&rig.directory), 1);
        rig.door.end(&rig.directory);
        wait_for_a_wake(&rig.wakes, "the storage watch ending on its own");
        rig.generations = [0, 0, 0, 1];
        assert_eq!(rig.due(start + WATCH_QUIET), DocumentSet::default());
        wait_for_a_wake(&rig.wakes, "the retry by the same turn's write");
        let _ = rig.due(start + WATCH_QUIET);
        assert_eq!(rig.door.attempts(&rig.directory), 2);
        assert!(rig.watch.is_armed());
    }

    /// RED (T-STORAGE-EAR-NAMED; design note (b)14-14) — a long name in any
    /// case names its document (E-3: a write through `Settings.Json` reports
    /// the on-disk `SETTINGS.JSON`), and a short JSON name stamps all four.
    /// MUTATION: exact-case comparison.
    #[test]
    fn case_equivalent_and_short_names_stamp_the_document() {
        assert_eq!(
            stamps(&["SETTINGS.JSON", "Profiles.Json"]),
            set(&[Document::Settings, Document::Profiles])
        );
        assert_eq!(stamps(&["Pins.JSON"]), set(&[Document::Pins]));
        assert_eq!(stamps(&["KeyBindings.json"]), set(&[Document::Keybindings]));
        for short in [
            "SETTIN~1.JSO",
            "KEYBIN~1.JSON",
            "profil~1.jso",
            "笔记~2.Json",
        ] {
            assert_eq!(stamps(&[short]), DocumentSet::ALL, "{short}");
        }
        for not_short in ["settings.json~", "PROFIL~1.TMP", "PINS~1.JS", "pins.jso"] {
            assert_ne!(stamps(&[not_short]), DocumentSet::ALL, "{not_short}");
        }
    }

    /// RED (T-STORAGE-EAR-NAMED) — the filter over E-3's Windows corpus: each
    /// row is one case's names as `ReadDirectoryChangesW` delivered them.
    /// MUTATIONS: match by prefix (temporaries and kept copies would count);
    /// ignore the second name of a rename; answer nothing for `Unknown`; answer
    /// all four for `Ended`.
    #[test]
    fn the_e3_corpus_stamps_only_the_documents_it_bears_on() {
        let rows: &[(&str, &[&str], DocumentSet)] = &[
            (
                "Folio's atomic write: the temporary alone",
                &["settings.json.tmp-4242-1700000000000000000-0"],
                DocumentSet::default(),
            ),
            (
                "Folio's atomic write: the rename into place",
                &[
                    "settings.json.tmp-4242-1700000000000000000-0",
                    "settings.json",
                ],
                set(&[Document::Settings]),
            ),
            (
                "the target removed before the rename",
                &["settings.json"],
                set(&[Document::Settings]),
            ),
            (
                "an in-place save",
                &["keybindings.json"],
                set(&[Document::Keybindings]),
            ),
            (
                "an editor's rename into place",
                &["profiles.json.tmp-Claude 代理", "profiles.json"],
                set(&[Document::Profiles]),
            ),
            (
                "Syncthing's temporary",
                &[".syncthing.pins.json.tmp"],
                DocumentSet::default(),
            ),
            (
                "Syncthing's rename into place",
                &[".syncthing.pins.json.tmp", "pins.json"],
                set(&[Document::Pins]),
            ),
            (
                "vim's backup copy",
                &["keybindings.json~"],
                DocumentSet::default(),
            ),
            (
                "vim renaming the document away to its backup",
                &["keybindings.json", "keybindings.json~"],
                set(&[Document::Keybindings]),
            ),
            (
                "kept copies",
                &[
                    "profiles.json.rejected-20261011-代理",
                    "pins.json.displaced-20261011-笔记",
                ],
                DocumentSet::default(),
            ),
            (
                "the scheme folder's entry and git's own",
                &["schemes", ".git", "项目.json"],
                DocumentSet::default(),
            ),
            (
                "a git checkout of all four",
                &[
                    "keybindings.json",
                    "pins.json",
                    "profiles.json",
                    "settings.json",
                    ".git",
                ],
                DocumentSet::ALL,
            ),
        ];
        for &(case, names, expected) in rows {
            assert_eq!(stamps(names), expected, "{case}: {names:?}");
        }
        assert_eq!(documents_named(DirChange::Unknown), DocumentSet::ALL);
        assert_eq!(documents_named(DirChange::Ended), DocumentSet::default());
    }

    /// RED (T-STORAGE-EAR-NAMED) — a `git checkout` of all four documents (E-4:
    /// remove, add and modify per document, about 1 ms apart, then `.git`) is
    /// ONE pass, a quiet window after its last name, owing all four.
    /// MUTATION: one clock per document (each document's pass falls due a
    /// quiet window after its own last stamp).
    #[test]
    fn a_git_checkout_shaped_burst_is_one_pass() {
        let start = Instant::now();
        let mut rig = Rig::armed_and_settled(start);
        let checkout = start + WATCH_QUIET + WATCH_FLOOR;
        let mut at = checkout;
        for document in [
            "keybindings.json",
            "pins.json",
            "profiles.json",
            "settings.json",
        ] {
            for _ in ["removed", "added", "modified"] {
                rig.kernel_names(at, &[document]);
                at += Duration::from_micros(400);
            }
        }
        rig.kernel_names(at, &[".git"]);
        let last = at - Duration::from_micros(400);
        assert_eq!(rig.due(at), DocumentSet::default());
        assert_eq!(rig.watch.deadline(), Some(last + WATCH_QUIET));
        let first_document_quiet = checkout + Duration::from_micros(800);
        assert_eq!(
            rig.due(first_document_quiet + WATCH_QUIET),
            DocumentSet::default(),
            "the first document's quiet window is not the folder's"
        );
        assert_eq!(rig.due(last + WATCH_QUIET), DocumentSet::ALL);
        assert_eq!(rig.due(last + WATCH_QUIET), DocumentSet::default());
        assert_eq!(rig.watch.deadline(), None);
    }

    /// RED (T-STORAGE-EAR-NAMED) — a `Failed` member is retried by the next
    /// accepted document write, and a turn whose write generations did not move
    /// owes no retry.
    /// MUTATIONS: retry on every turn (drop the generation comparison); never
    /// name the failed folder in `rearm`.
    #[test]
    fn a_failed_ear_is_retried_by_the_next_document_write() {
        let start = Instant::now();
        let mut rig = Rig::new(start);
        rig.door.refuse(&rig.directory);
        rig.arm([0; 4]);
        wait_for_a_wake(&rig.wakes, "the storage ear's refusal");
        assert_eq!(rig.due(start), DocumentSet::default());
        assert!(!rig.watch.is_armed());
        assert_eq!(rig.door.attempts(&rig.directory), 1);

        assert!(!rig.watch.retry_owed([0; 4]), "a turn is no retry");
        rig.door.admit(&rig.directory);
        rig.generations = [0, 0, 1, 0];
        assert_eq!(rig.due(start), DocumentSet::default());
        wait_for_a_wake(&rig.wakes, "the reader-triggered retry");
        assert_eq!(rig.due(start), DocumentSet::default());
        assert_eq!(rig.door.attempts(&rig.directory), 2);
        assert!(rig.watch.is_armed());
        assert_eq!(
            rig.due(start + WATCH_QUIET),
            DocumentSet::ALL,
            "a member that has just armed is news"
        );
    }

    /// A watch that never opened still answers every question, and a window
    /// holding one simply never hears about a hand edit.
    #[test]
    fn an_unarmed_watch_is_inert_rather_than_absent() {
        let mut watch = StorageWatch::default();
        assert!(!watch.is_armed());
        assert_eq!(
            watch.advance(Instant::now(), [0; 4]),
            DocumentSet::default()
        );
        assert_eq!(watch.deadline(), None);
    }
}
