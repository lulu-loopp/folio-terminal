//! **The files the palette can offer, and the bounded walk that finds them**
//! (`docs/DESIGN.md` §7.54 ④).
//!
//! The command palette's files section answers a question no other surface in
//! this window asks: *given four letters, which file under this root did you
//! mean?* Every other file surface here is asked about one directory at a time —
//! [`crate::files`] reads exactly the folders a person has opened, and it is
//! right to, because a tree only ever draws what is unfolded. A palette cannot
//! work that way. It has to have the names in hand **before** the first
//! keystroke, because the whole of its value is that you do not have to know
//! where the file is.
//!
//! So this module is the one place in the program that reads a directory nobody
//! asked to see. That is a debt, and everything below is the shape of paying it.
//!
//! # Why the index is bounded, and bounded in three different ways
//!
//! A files root is somebody's home directory as often as it is a checkout. The
//! honest answer to "index everything under it" is that there is no such thing:
//! a walk with no bound is a walk whose cost is a property of the machine it
//! runs on, and a feature that is instant on a repository and takes four minutes
//! on `C:\Users\me` is not one feature.
//!
//! The three bounds are deliberately *different kinds* of thing:
//!
//! - [`INDEX_MAX_DEPTH`] and [`SKIPPED_DIRECTORIES`] say **what the index is**.
//!   They are the same for every root and they never change, so a person who
//!   learns once that the palette does not know about `node_modules` has learned
//!   something true about the palette. Neither is reported as truncation,
//!   because neither is a failure to finish: they are the definition of what was
//!   being finished.
//! - [`INDEX_MAX_ENTRIES`] says **where this particular tree outgrew the
//!   index**. That *is* reported ([`FileIndex::truncated`]), because it is a
//!   fact about the root in front of you and not about the feature — and a
//!   palette that quietly stops knowing about half a repository, with no way to
//!   tell that from a repository that genuinely has no match, is the kind of
//!   silence this project has already paid for once elsewhere.
//!
//! # Why it is breadth-first
//!
//! The walk is a queue, level by level, and within one directory the names are
//! sorted. Two consequences, and both are load-bearing:
//!
//! 1. **The order is stable**, which is what lets the palette's scorer say "ties
//!    keep supplier order" and mean something. A `read_dir` order is the
//!    filesystem's, and it is not a promise.
//! 2. **Shallow files come first.** That is the right order for a list you scan
//!    with your eyes, and it is also the right order to be cut off in: when
//!    [`INDEX_MAX_ENTRIES`] is reached, what was lost is the deepest, most
//!    buried corner of the tree, and what was kept is everything a person is
//!    likely to have been reaching for.
//!
//! # Why the walk is on a thread and the register is not
//!
//! [`IndexWorker`] is the same shape as [`crate::files::FilesWorker`] and for
//! the same reason R-i gives: no `read_dir` on the event loop. What is different
//! is the traffic. A directory lane answers a question per fold; this lane
//! answers **once per root**, and then nothing at all until something on disk
//! moves. That is why there is no newest-per-target queue here — the coalescing
//! that [`crate::files`] needs a `PendingDirRequests` for, this module gets from
//! [`FileIndexes::claim`], which simply refuses to ask twice for a root that is
//! already being walked.
//!
//! # Why a root path is an address and a `TabId` would not have been
//!
//! §2.4's rule — an address on a shared queue must be unique all the way down to
//! the layer that takes the answer — is satisfied here without any wrapping,
//! because the address *is* a path. Two windows showing the same files root are
//! not two questions that can be confused with each other; they are one
//! question, and either window's answer is the correct answer for both. The
//! epoch beside it is not an address at all: it is a *version*, and it exists
//! only so that a walk which was already in flight when the tree moved can be
//! recognised as out of date and dropped.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use anyhow::{Context, Result};
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

/// How deep under the root the index reaches (`docs/DESIGN.md` §7.54 ④).
///
/// The root's own children are depth 1, so six levels covers
/// `crates/bt-app/src/palette_index.rs` — five — with a level to spare, and that
/// is the shape this bound was chosen against: source trees are wide and
/// shallow, and the directories that are deep are almost always the generated
/// ones that [`SKIPPED_DIRECTORIES`] already refuses. A person who keeps work
/// eight levels down is not served by this feature, and is served much worse by
/// a palette that takes a minute to open.
pub const INDEX_MAX_DEPTH: usize = 6;

/// How many files one root's index may hold (`docs/DESIGN.md` §7.54 ④).
///
/// Thirty thousand names is a large repository read whole — this workspace is
/// two orders of magnitude below it — and it is also about the point where the
/// cost stops being the walk and starts being the scoring, which happens per
/// keystroke on the event loop. The number therefore protects the *typing*, not
/// the disk: doubling it would double the work done between one letter and the
/// next.
pub const INDEX_MAX_ENTRIES: usize = 30_000;

/// Directory names never descended into.
///
/// Every name here is a directory whose contents are **derived** — a cache, a
/// build product, a dependency tree somebody else's tool wrote. Their files are
/// not files a person is trying to reach by name, and there are more of them
/// than of everything else put together: a single `node_modules` routinely holds
/// more entries than [`INDEX_MAX_ENTRIES`] allows for the whole root, so
/// indexing it would not merely waste the walk, it would spend the entire budget
/// before reaching any of the source.
///
/// The match is on the **whole name**, never a prefix or a substring. `target`
/// is skipped; `targets` and `my-target` are ordinary directories that happen to
/// share letters with one, and a check that could not tell them apart would make
/// this list quietly unpredictable.
pub const SKIPPED_DIRECTORIES: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    "dist",
    "build",
    ".next",
    ".cargo",
    ".gradle",
];

/// The name of the thread the walk runs on.
const INDEX_WORKER_THREAD: &str = "bt-index-worker";

/// One file the palette can offer.
///
/// Three fields and not one path, because the three are wanted at three
/// different moments and deriving any of them per keystroke would put string
/// work on the event loop: the name is what a query is scored against, the
/// relative path is what is drawn beside it, and the absolute path is what the
/// verb opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedFile {
    /// The file's own name — what the palette matches against and prints.
    pub name: String,
    /// Its path relative to the root, `/`-joined — the palette's grey hint.
    ///
    /// `/` and not the platform separator, for the reason [`crate::files`]
    /// gives its stable ids: this string is shown and compared, never pushed at
    /// the filesystem. [`Self::path`] is the one that reaches disk, and it was
    /// built segment by segment so it stayed a Windows path all the way down.
    pub relative: String,
    /// The absolute path the palette's verb opens.
    pub path: std::path::PathBuf,
}

/// One root's files, and whether that is all of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileIndex {
    files: Vec<IndexedFile>,
    /// Whether the walk stopped on a bound rather than because it ran out.
    truncated: bool,
}

impl FileIndex {
    /// The files, in the walk's order — breadth-first, sorted within each
    /// directory.
    #[must_use]
    pub fn files(&self) -> &[IndexedFile] {
        &self.files
    }

    /// Whether [`INDEX_MAX_ENTRIES`] cut the walk short.
    ///
    /// True only when a file was actually refused, so a root with exactly the
    /// cap's worth of files is not truncated — it ran out, which is a different
    /// thing to say and the only one that is true.
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// The walk itself — a pure function of a directory tree, runs on the worker.
///
/// Everything it can be asked is fixed by the three constants, so there is one
/// index per root and never a question of which parameters produced it.
#[must_use]
pub fn walk(root: &Path) -> FileIndex {
    walk_bounded(root, INDEX_MAX_DEPTH, INDEX_MAX_ENTRIES)
}

/// A directory waiting to be read, and everything the reading needs to know.
struct Pending {
    path: PathBuf,
    /// `/`-joined path from the root; empty for the root itself, which is why
    /// the root contributes no prefix to anything below it.
    relative: String,
    /// The root is 0, so a directory's entries are at `depth + 1`.
    depth: usize,
    /// The canonical paths of the root down to and including this directory —
    /// the cycle guard's whole memory.
    ancestors: Vec<PathBuf>,
}

/// [`walk`] with its bounds handed in.
///
/// They are parameters and not just constants read from scope for one reason
/// that is worth the extra argument: [`INDEX_MAX_ENTRIES`] is thirty thousand,
/// and a test that had to build thirty thousand files to prove the cap works
/// would be a test nobody runs. `max_depth` is the greatest depth an entry may
/// have, counting the root's own children as 1.
fn walk_bounded(root: &Path, max_depth: usize, max_entries: usize) -> FileIndex {
    let mut index = FileIndex::default();
    let mut queue: VecDeque<Pending> = VecDeque::new();
    enqueue(
        &mut queue,
        Pending {
            path: root.to_path_buf(),
            relative: String::new(),
            depth: 0,
            ancestors: canonical(root).into_iter().collect(),
        },
        max_depth,
    );

    'walk: while let Some(directory) = queue.pop_front() {
        // A directory that cannot be read is not a failure of the index — a
        // permission-denied folder is one the person could not have opened
        // either, and the rest of the root is still worth having.
        let Ok(reader) = std::fs::read_dir(&directory.path) else {
            continue;
        };

        // Read the whole directory before emitting any of it: the order of the
        // index is a promise the palette leans on, and `read_dir` makes no such
        // promise. `(name, is_directory)` and not the `DirEntry` itself, because
        // those two answers are the whole of what the sort and the emit below
        // need, and the second of them can cost a syscall that is better paid
        // once here than again after sorting.
        let mut entries: Vec<(String, bool)> = Vec::new();
        for entry in reader.flatten() {
            // **The cap bounds the reading and not only the emitting.** Without
            // this the names of one directory are all collected before the cap
            // is consulted below, so a single folder holding a million entries
            // — a download pile, an artefact store, a mail spool — allocates a
            // million strings inside the one module whose entire contract is
            // that it is bounded. Nothing past the cap could ever be emitted
            // anyway; stopping here is the same answer reached without the
            // allocation, and it is recorded as truncation for the same reason
            // the emit below records it.
            if entries.len() >= max_entries {
                index.truncated = true;
                break;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                // A name that is not UTF-8 cannot be typed into the palette, so
                // there is nothing an index could do with it.
                continue;
            };
            // `DirEntry::file_type` is the reading that does *not* traverse a
            // link — the cheap equivalent of `symlink_metadata`, already paid
            // for by the directory read on Windows. Whether a link points at a
            // directory then costs one more call, and only for links.
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let is_directory = if file_type.is_symlink() {
                std::fs::metadata(entry.path()).is_ok_and(|target| target.is_dir())
            } else {
                file_type.is_dir()
            };
            entries.push((name, is_directory));
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));

        for (name, is_directory) in entries {
            let relative = child_relative(&directory.relative, &name);
            if !is_directory {
                if index.files.len() >= max_entries {
                    index.truncated = true;
                    break 'walk;
                }
                index.files.push(IndexedFile {
                    path: directory.path.join(&name),
                    name,
                    relative,
                });
                continue;
            }
            if SKIPPED_DIRECTORIES.contains(&name.as_str()) {
                continue;
            }
            let path = directory.path.join(&name);
            let resolved = canonical(&path);
            if closes_a_cycle(&directory.ancestors, resolved.as_deref()) {
                continue;
            }
            let mut ancestors = directory.ancestors.clone();
            ancestors.extend(resolved);
            enqueue(
                &mut queue,
                Pending {
                    path,
                    relative,
                    depth: directory.depth + 1,
                    ancestors,
                },
                max_depth,
            );
        }
    }
    index
}

/// Queue a directory unless everything in it would be deeper than the walk goes.
///
/// The one place the depth bound is applied, so that the root is subject to it
/// on exactly the same terms as everything below it.
fn enqueue(queue: &mut VecDeque<Pending>, pending: Pending, max_depth: usize) {
    if pending.depth < max_depth {
        queue.push_back(pending);
    }
}

/// A child's `/`-joined path from the root.
fn child_relative(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

/// The resolved form of a directory, used only to compare two paths for
/// sameness.
///
/// This is `crate::files::canonical_path` written out again rather than called:
/// that one is private to its module, and the alternative — widening another
/// module's surface for one line — would be a worse trade than one line of
/// `std`.
fn canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Whether descending into a directory would re-enter one the walk is already
/// inside.
///
/// The guard [`crate::files`]'s `walk_dir` uses, in the same shape and for a
/// slightly different reason. There, a cycle would make the tree infinite.
/// Here it cannot: [`INDEX_MAX_DEPTH`] ends every path whatever the links do, so **termination is not what this buys**. What it buys is that a
/// file is not listed once for its own path and five more times for a route that
/// went round a loop to reach it — five rows in the palette that all open the
/// same file and none of which say where it is.
///
/// A directory whose canonical path could not be read is descended into and
/// contributes no ancestor: we cannot prove it closes a loop, and the depth
/// bound is already holding the floor.
fn closes_a_cycle(ancestors: &[PathBuf], candidate: Option<&Path>) -> bool {
    candidate.is_some_and(|resolved| ancestors.iter().any(|seen| seen == resolved))
}

/// A root to walk, and which walk of it this is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexRequest {
    pub root: PathBuf,
    pub epoch: u64,
}

/// A walk that finished, and the request it finished.
#[derive(Clone, Debug)]
pub struct IndexResponse {
    pub root: PathBuf,
    pub epoch: u64,
    pub index: FileIndex,
}

/// The thread, and the two ends of the conversation with it.
pub struct IndexWorker {
    requests: mpsc::Sender<IndexRequest>,
    responses: mpsc::Receiver<IndexResponse>,
}

impl IndexWorker {
    /// Spawn the lane. Mirrors [`crate::files::FilesWorker::spawn`].
    ///
    /// Below-normal priority, like every other answering thread in this process:
    /// an index that arrives a moment late costs a palette one frame of "still
    /// looking", while a frame that arrives late costs the person their cursor.
    pub fn spawn(proxy: EventLoopProxy<AppEvent>) -> Result<Self> {
        let (request_tx, request_rx) = mpsc::channel::<IndexRequest>();
        let (response_tx, response_rx) = mpsc::channel::<IndexResponse>();
        bt_platform::spawn_at_priority(
            INDEX_WORKER_THREAD,
            bt_platform::ThreadPriority::BelowNormal,
            move || {
                // A plain queue and no newest-per-target coalescing, unlike the
                // directory lane: `FileIndexes::claim` will not ask twice about
                // a root whose walk is already out, so there is no burst of
                // superseded questions for a queue to collapse.
                while let Ok(request) = request_rx.recv() {
                    let index = walk(&request.root);
                    if response_tx
                        .send(IndexResponse {
                            root: request.root,
                            epoch: request.epoch,
                            index,
                        })
                        .is_ok()
                    {
                        let _ = proxy.send_event(AppEvent::FileIndexReady);
                    }
                }
            },
        )
        .context("spawn palette file index worker")?;
        Ok(Self {
            requests: request_tx,
            responses: response_rx,
        })
    }

    /// `false` when the lane is gone.
    ///
    /// The only error this module has, and not one a session should end for:
    /// a palette without a files section is a smaller palette, not a broken
    /// window.
    #[must_use]
    pub fn request(&self, request: IndexRequest) -> bool {
        self.requests.send(request).is_ok()
    }

    /// Take every answer that has landed. `bool` is "the lane is gone".
    ///
    /// §2.4's second rule: the channel belongs to the process and is therefore
    /// drained here, once, by the thing that owns it. What comes out is handed
    /// to every register that wants it — and unlike the four decoration lanes,
    /// *wanting* it is not exclusive, because an index of a root is the same
    /// index whichever window asked.
    pub fn drain(&self) -> (Vec<IndexResponse>, bool) {
        let mut batch = Vec::new();
        let mut gone = false;
        loop {
            match self.responses.try_recv() {
                Ok(response) => batch.push(response),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    gone = true;
                    break;
                }
            }
        }
        (batch, gone)
    }
}

/// What the palette can say about a root right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexState {
    /// Nothing has been asked and nothing is on the way.
    Missing,
    /// A walk is out. The palette has a sentence to show and no rows yet.
    Building,
    /// There is an index. It may be about to be rebuilt, and it is still the
    /// best thing anyone can say about the root.
    Ready,
}

/// One root's index, its version, and whether either is still current.
#[derive(Debug, Default)]
struct Entry {
    /// The version of the most recent request. Starts at 0 so that the first
    /// [`FileIndexes::claim`] issues epoch 1 and no live request ever carries
    /// the same number as "never asked".
    epoch: u64,
    /// A walk for [`Self::epoch`] is out.
    building: bool,
    /// The last complete answer, deliberately **kept across a rebuild**: the
    /// files that were under this root a second ago are overwhelmingly still
    /// there, and blanking the palette while it re-reads them would be a worse
    /// lie than showing a list that is one rename out of date.
    index: Option<FileIndex>,
    /// Something under the root moved since the current walk was asked for.
    dirty: bool,
}

/// The per-root register (`docs/DESIGN.md` §7.54 ④).
///
/// One entry per files root, keyed by the root's path — see this module's header
/// for why that key needs nothing wrapped around it. A [`BTreeMap`] and not a
/// hash map because the register is small, is walked far more often than it is
/// written, and its `Debug` output is something a person reads.
#[derive(Debug, Default)]
pub struct FileIndexes {
    roots: BTreeMap<PathBuf, Entry>,
}

impl FileIndexes {
    /// What the palette can say about this root.
    #[must_use]
    pub fn state(&self, root: &Path) -> IndexState {
        match self.roots.get(root) {
            Some(entry) if entry.building => IndexState::Building,
            Some(entry) if entry.index.is_some() => IndexState::Ready,
            // An entry that is neither building nor holding an index knows
            // exactly as much about the root as no entry at all does, and says
            // so rather than claiming a readiness it cannot back.
            Some(_) | None => IndexState::Missing,
        }
    }

    /// This root's index, if one has ever finished.
    ///
    /// Answers during a rebuild too, for [`Entry::index`]'s reason: the caller
    /// asks [`Self::state`] when it wants to know whether the answer is fresh,
    /// and asks this when it wants rows.
    #[must_use]
    pub fn get(&self, root: &Path) -> Option<&FileIndex> {
        self.roots.get(root).and_then(|entry| entry.index.as_ref())
    }

    /// What to ask the worker for, if anything: `Some` when the root is missing
    /// or dirty and is not already being built for the current epoch. Marks it
    /// `Building` as a side effect (the caller is about to send it).
    ///
    /// This is the whole of the lane's coalescing. A palette that is opened,
    /// closed and opened again in a second asks three times and the worker hears
    /// once, because the second and third questions arrive at a root that is
    /// already being walked for the epoch they would have asked about.
    pub fn claim(&mut self, root: &Path) -> Option<IndexRequest> {
        let entry = self.roots.entry(root.to_path_buf()).or_default();
        if !entry.dirty && (entry.building || entry.index.is_some()) {
            return None;
        }
        // A dirty root supersedes its own walk rather than waiting for it: the
        // answer in flight was asked about a tree that has since moved, and
        // `accept` will drop it on the epoch.
        entry.dirty = false;
        entry.building = true;
        entry.epoch += 1;
        Some(IndexRequest {
            root: root.to_path_buf(),
            epoch: entry.epoch,
        })
    }

    /// Something under this root moved: the next open rebuilds it.
    ///
    /// Nothing is invalidated here — not the index and not the state. A root
    /// whose index is one file out of date is enormously more useful than a root
    /// with no index, and the rebuild costs a walk that is worth paying for when
    /// somebody actually opens the palette and not every time a compiler writes
    /// a file.
    ///
    /// A root with no entry is left with none: there is nothing to rebuild, and
    /// the first [`Self::claim`] will build it from scratch anyway.
    pub fn mark_dirty(&mut self, root: &Path) {
        if let Some(entry) = self.roots.get_mut(root) {
            entry.dirty = true;
        }
    }

    /// File an answer. A late answer for a superseded epoch is dropped.
    ///
    /// `dirty` is deliberately **not** cleared: news that arrived while the walk
    /// was out is news the walk may or may not have seen, and the only honest
    /// thing to do with a maybe is to rebuild on the next open.
    pub fn accept(&mut self, response: IndexResponse) {
        let Some(entry) = self.roots.get_mut(&response.root) else {
            return;
        };
        if entry.epoch != response.epoch {
            return;
        }
        entry.building = false;
        entry.index = Some(response.index);
    }

    /// Roots no longer wanted are forgotten, so a closed files column does not
    /// keep its index alive forever.
    ///
    /// Thirty thousand `IndexedFile`s is a few megabytes, and the register is
    /// the only thing in the process holding them. Called with the roots the
    /// windows can currently name; a root that comes back is walked again, which
    /// is the right price for a column somebody closed.
    pub fn retain(&mut self, wanted: &BTreeSet<PathBuf>) {
        self.roots.retain(|root, _| wanted.contains(root));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A scratch directory of this test's own, never shared with another.
    ///
    /// Process id, wall-clock nanoseconds and a counter, because two tests in
    /// one binary run at once and two binaries can run at once too — and a
    /// fixture that two walks are writing into is the "shared fixture hides a
    /// bug" family from `CONVENTIONS.md` §3 in its most literal form.
    fn scratch(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos());
        let dir = std::env::temp_dir().join(format!(
            "folio-palette-index-{name}-{}-{nanos}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    /// Write a file, making whatever directories it needs on the way.
    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a parent directory");
        }
        std::fs::write(path, b"folio").expect("a file");
    }

    /// The `/`-joined relative paths of an index, in the walk's own order.
    fn relatives(index: &FileIndex) -> Vec<String> {
        index
            .files()
            .iter()
            .map(|file| file.relative.clone())
            .collect()
    }

    fn ready(files: &[&str]) -> FileIndex {
        FileIndex {
            files: files
                .iter()
                .map(|relative| IndexedFile {
                    name: (*relative).to_owned(),
                    relative: (*relative).to_owned(),
                    path: PathBuf::from(relative),
                })
                .collect(),
            truncated: false,
        }
    }

    /// PIN — the depth bound is a bound on **entries**, and the root's own
    /// children are depth 1.
    ///
    /// The off-by-one here is the whole of the rule: a file six levels below the
    /// root is in the index, and the directory that holds it is the deepest one
    /// the walk ever reads. The directory at depth six is seen — it is just
    /// never opened.
    ///
    /// MUTATIONS:
    /// - `enqueue`'s `pending.depth < max_depth` → `<=`: `f7.txt` appears and
    ///   the second assertion goes red.
    /// - `pending.depth < max_depth` → `<`, with the root pushed at depth 1
    ///   instead of 0: `f6.txt` disappears and the first goes red.
    /// - dropping the `enqueue` guard entirely: `f8.txt` and `f9.txt` appear and
    ///   the last assertion goes red.
    #[test]
    fn a_file_six_levels_down_is_indexed_and_one_seven_levels_down_is_not() {
        let root = scratch("depth");
        // A chain eight directories deep, each holding one file whose name says
        // how deep that file is.
        let mut here = root.clone();
        touch(&here.join("f1.txt"));
        for level in 1..=8 {
            here = here.join(format!("l{level}"));
            touch(&here.join(format!("f{}.txt", level + 1)));
        }

        let index = walk(&root);
        let found = relatives(&index);

        assert!(
            found.contains(&"l1/l2/l3/l4/l5/f6.txt".to_owned()),
            "a file at depth {INDEX_MAX_DEPTH} is inside the bound: {found:?}"
        );
        assert!(
            !found.contains(&"l1/l2/l3/l4/l5/l6/f7.txt".to_owned()),
            "a file one level past the bound is not: {found:?}"
        );
        assert!(
            found
                .iter()
                .all(|relative| !relative.contains("f8.txt") && !relative.contains("f9.txt")),
            "and neither is anything below it: {found:?}"
        );
        assert!(
            !index.truncated(),
            "depth is what the index *is*, not somewhere it gave up"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — the entry cap stops the walk **and says so**.
    ///
    /// The cap is the one bound a person is told about, because it is the one
    /// that is a fact about their tree rather than about the feature.
    ///
    /// MUTATIONS:
    /// - `index.files.len() >= max_entries` → `> max_entries`: four files land
    ///   and the count assertion goes red.
    /// - deleting `index.truncated = true`: the second assertion goes red while
    ///   the count still passes, which is exactly the silent half.
    /// - `break 'walk` → `continue`: **not** caught, here or anywhere in this
    ///   file, and said out loud rather than claimed. The walk would go on
    ///   reading directories and refusing every file in them, and the index that
    ///   came out would be identical — the `break` buys time, not correctness,
    ///   and no assertion can see time.
    /// - the `entries.len() >= max_entries` guard on the *reading* loop: **not
    ///   caught either, and for the same reason.** It stops one directory's
    ///   names being collected past the point where any of them could be
    ///   emitted, so the index that comes out is the same index; what it buys
    ///   is that a folder holding a million entries costs the cap's worth of
    ///   memory rather than a million strings. That is a bound on space, and
    ///   this file's assertions can only see answers. Listed here rather than
    ///   left to be discovered, because a guard with no red gate is exactly the
    ///   thing this project asks to be told about.
    #[test]
    fn the_entry_cap_stops_the_walk_and_records_that_it_did() {
        let root = scratch("cap-hit");
        for number in 0..10 {
            touch(&root.join(format!("file-{number:02}.txt")));
        }

        let index = walk_bounded(&root, INDEX_MAX_DEPTH, 3);

        assert_eq!(index.len(), 3, "exactly the cap, and not one more");
        assert!(index.truncated(), "and the palette is told there was more");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — a walk that runs out before the cap is **not** truncated.
    ///
    /// The dual of the test above, over the same ten files, because a
    /// `truncated` that were simply always true would pass that one alone.
    ///
    /// MUTATIONS:
    /// - hard-coding `truncated: true` in [`walk_bounded`]'s initial
    ///   [`FileIndex`]: the second assertion goes red.
    /// - moving `index.truncated = true` outside the `len >= max_entries`
    ///   branch, so that any queued directory sets it: also red.
    #[test]
    fn a_walk_that_runs_out_before_the_cap_is_not_truncated() {
        let root = scratch("cap-clear");
        for number in 0..10 {
            touch(&root.join(format!("file-{number:02}.txt")));
        }

        let index = walk_bounded(&root, INDEX_MAX_DEPTH, 100);

        assert_eq!(index.len(), 10, "all ten, none refused");
        assert!(
            !index.truncated(),
            "nothing was cut off, so nothing is claimed"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — a skipped directory is skipped **by its whole name**.
    ///
    /// The second half is the one that matters and the one a substring check
    /// would fail: `targets` and `my-target` are ordinary directories that
    /// happen to contain the letters of one that is not.
    ///
    /// MUTATIONS:
    /// - `SKIPPED_DIRECTORIES.contains(&name.as_str())` →
    ///   `SKIPPED_DIRECTORIES.iter().any(|skip| name.contains(skip))`: `keep1`
    ///   and `keep2` vanish and the "present" assertions go red.
    /// - deleting the skip check: the three derived files appear and the
    ///   "absent" assertions go red.
    /// - moving the skip check above the `is_directory` branch so it also
    ///   applies to files: a file *named* `build` would vanish — pinned by
    ///   `build` appearing as a filename below.
    #[test]
    fn a_skipped_directory_is_skipped_by_its_whole_name_and_not_by_a_prefix() {
        let root = scratch("skips");
        touch(&root.join("keep.txt"));
        touch(&root.join("build")); // a *file* whose name is on the list
        touch(&root.join("node_modules").join("pkg.js"));
        touch(&root.join(".git").join("config.txt"));
        touch(&root.join("target").join("gone.txt"));
        touch(&root.join("targets").join("keep1.txt"));
        touch(&root.join("my-target").join("keep2.txt"));

        let found = relatives(&walk(&root));

        for present in [
            "keep.txt",
            "build",
            "targets/keep1.txt",
            "my-target/keep2.txt",
        ] {
            assert!(
                found.contains(&present.to_owned()),
                "{present} is not derived and belongs in the index: {found:?}"
            );
        }
        for absent in ["node_modules/pkg.js", ".git/config.txt", "target/gone.txt"] {
            assert!(
                !found.contains(&absent.to_owned()),
                "{absent} lives under a skipped directory: {found:?}"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — the cycle guard, without a filesystem.
    ///
    /// The half of the symlink story that is always checked, on every machine
    /// and in every privilege state. The `None` case is the one that could be
    /// written backwards without any test noticing: a directory whose canonical
    /// path could not be read has not been shown to close a loop.
    ///
    /// MUTATIONS:
    /// - `ancestors.iter().any(..)` → `!ancestors.iter().any(..)`: the first two
    ///   assertions swap and both go red.
    /// - `candidate.is_some_and(..)` → `candidate.is_none_or(..)`: the `None`
    ///   assertion goes red.
    #[test]
    fn the_cycle_guard_answers_before_any_directory_is_read() {
        let outer = PathBuf::from("/canon/a");
        let inner = PathBuf::from("/canon/a/b");
        let elsewhere = PathBuf::from("/canon/c");
        let ancestors = [outer.clone(), inner];

        assert!(closes_a_cycle(&ancestors, Some(&outer)));
        assert!(!closes_a_cycle(&ancestors, Some(&elsewhere)));
        assert!(
            !closes_a_cycle(&ancestors, None),
            "a path that could not be resolved has not been shown to be an ancestor"
        );
    }

    /// PIN — a directory link back to an ancestor does not list its files twice.
    ///
    /// The depth bound already guarantees the walk *ends*; what the guard is for
    /// is that `leaf.txt` appears once, under the path it actually has, rather
    /// than once more for every lap of the loop that fits inside six levels.
    ///
    /// **This test skips itself, loudly, on a Windows box without the
    /// symlink privilege.** Creating a directory link needs
    /// `SeCreateSymbolicLinkPrivilege` or Developer Mode, and a machine that has
    /// neither cannot be asked this question at all — so it says so on stderr
    /// rather than passing quietly, which would make an unrunnable test look
    /// like a passing one. The guard's own arithmetic is pinned unconditionally
    /// by the test above.
    ///
    /// MUTATIONS:
    /// - `closes_a_cycle(..)` forced to `false`: `leaf.txt` is reached again
    ///   through `loop/b`, the count assertion goes red with 2.
    /// - not extending `ancestors` with the child's canonical path: the guard
    ///   only ever sees the root, so the loop is entered and the same assertion
    ///   goes red.
    #[test]
    fn a_directory_link_back_to_an_ancestor_does_not_repeat_a_file() {
        let root = scratch("cycle");
        let inner = root.join("a").join("b");
        touch(&inner.join("leaf.txt"));

        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(root.join("a"), inner.join("loop"));
        #[cfg(not(windows))]
        let made = std::os::unix::fs::symlink(root.join("a"), inner.join("loop"));

        if let Err(error) = made {
            eprintln!(
                "SKIPPED a_directory_link_back_to_an_ancestor_does_not_repeat_a_file: \
                 this machine would not create a directory symlink ({error}). On Windows \
                 that needs SeCreateSymbolicLinkPrivilege or Developer Mode. The cycle \
                 guard's arithmetic is still checked by \
                 the_cycle_guard_answers_before_any_directory_is_read."
            );
            let _ = std::fs::remove_dir_all(&root);
            return;
        }

        let index = walk(&root);
        let leaves = index
            .files()
            .iter()
            .filter(|file| file.name == "leaf.txt")
            .count();

        assert_eq!(leaves, 1, "one file, one row: {:?}", relatives(&index));
        assert_eq!(
            relatives(&index),
            vec!["a/b/leaf.txt".to_owned()],
            "and under the path it really has"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — `name` is the file's own name and `relative` is the path from the
    /// root, with the root itself contributing nothing.
    ///
    /// The grey hint beside a palette row is this string, and a root prefix in
    /// it would be the same seventeen characters on every row in the list.
    ///
    /// MUTATIONS:
    /// - seeding the root `Pending` with anything but an empty `relative`: every
    ///   relative gains a prefix and both relative assertions go red.
    /// - `child_relative` joining with `\` or with `std::path::MAIN_SEPARATOR`:
    ///   the nested assertion goes red on Windows.
    /// - `child_relative` returning `format!("/{name}")` for an empty parent:
    ///   the `top.txt` assertion goes red.
    /// - building `path` from the relative string instead of from
    ///   `directory.path`: the path assertion goes red for a root that is not
    ///   the current directory.
    #[test]
    fn a_nested_file_carries_its_own_name_and_the_path_from_the_root() {
        let root = scratch("relative");
        touch(&root.join("top.txt"));
        touch(&root.join("alpha").join("beta").join("gamma.txt"));

        let index = walk(&root);
        let nested = index
            .files()
            .iter()
            .find(|file| file.name == "gamma.txt")
            .expect("the nested file is indexed");

        assert_eq!(nested.relative, "alpha/beta/gamma.txt");
        assert_eq!(nested.name, "gamma.txt");
        assert_eq!(
            nested.path,
            root.join("alpha").join("beta").join("gamma.txt")
        );

        let top = index
            .files()
            .iter()
            .find(|file| file.name == "top.txt")
            .expect("the root's own file is indexed");
        assert_eq!(top.relative, "top.txt", "the root contributes no prefix");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — the walk's order is shallow-first, and sorted inside each
    /// directory.
    ///
    /// The palette's scorer keeps supplier order on a tie, so this order is part
    /// of what a person sees. `read_dir` promises nothing, and a depth-first
    /// walk would put `m/deep/d.txt` above `z/z1.txt` — which is the same set of
    /// files and a worse list.
    ///
    /// MUTATIONS:
    /// - `entries.sort_by(..)` → `entries.reverse()`: the assertion goes red.
    ///   (Merely *deleting* the sort is red only on a volume whose own order is
    ///   not already alphabetical, which is why the mutation named here replaces
    ///   the sort rather than removing it.)
    /// - `queue.pop_front()` → `queue.pop_back()`: `m/deep/d.txt` climbs above
    ///   `z/z1.txt` and the assertion goes red.
    /// - emitting directories before files within a level: unchanged here, which
    ///   is why the fixture puts `a.txt` beside two directories rather than
    ///   after them.
    #[test]
    fn the_index_reads_shallow_first_and_alphabetically_within_each_directory() {
        let root = scratch("order");
        touch(&root.join("a.txt"));
        touch(&root.join("m").join("m1.txt"));
        touch(&root.join("m").join("deep").join("d.txt"));
        touch(&root.join("z").join("z1.txt"));

        assert_eq!(
            relatives(&walk(&root)),
            vec![
                "a.txt".to_owned(),
                "m/m1.txt".to_owned(),
                "z/z1.txt".to_owned(),
                "m/deep/d.txt".to_owned(),
            ]
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — a root is claimed once, and not again while it is being walked.
    ///
    /// This is the whole of the lane's coalescing, and the reason there is no
    /// newest-per-target queue on the worker side: a palette opened three times
    /// in a second must not put three walks of the same tree on one thread.
    ///
    /// MUTATIONS:
    /// - deleting the `entry.building` half of `claim`'s early return: the
    ///   second claim returns `Some` and both later assertions go red.
    /// - not setting `entry.building = true`: the same two go red.
    /// - starting `epoch` at 1 rather than 0: the first request carries 2 and
    ///   the epoch assertion goes red.
    #[test]
    fn a_root_is_claimed_once_and_not_again_while_it_is_being_walked() {
        let root = PathBuf::from("/roots/one");
        let mut register = FileIndexes::default();

        assert_eq!(register.state(&root), IndexState::Missing);
        let first = register.claim(&root).expect("an unindexed root is claimed");
        assert_eq!(first.root, root);
        assert_eq!(first.epoch, 1);
        assert_eq!(register.state(&root), IndexState::Building);
        assert!(register.get(&root).is_none(), "nothing has been walked yet");

        assert_eq!(
            register.claim(&root),
            None,
            "a root already being walked is not asked about again"
        );
        assert_eq!(register.state(&root), IndexState::Building);
    }

    /// PIN — an answer makes the root ready, and its index readable.
    ///
    /// MUTATIONS:
    /// - deleting `entry.building = false` in `accept`: the state stays
    ///   `Building` and the state assertion goes red.
    /// - deleting `entry.index = Some(..)`: `get` stays `None` and both the
    ///   `get` and the state assertions go red.
    /// - `accept` returning early for a root it has an entry for: same.
    /// - `claim` returning `Some` for a ready, clean root: the last assertion
    ///   goes red.
    #[test]
    fn an_answer_makes_the_root_ready_and_its_index_readable() {
        let root = PathBuf::from("/roots/two");
        let mut register = FileIndexes::default();
        let request = register.claim(&root).expect("a first claim");

        register.accept(IndexResponse {
            root: root.clone(),
            epoch: request.epoch,
            index: ready(&["one.txt", "two.txt"]),
        });

        assert_eq!(register.state(&root), IndexState::Ready);
        assert_eq!(register.get(&root).map(FileIndex::len), Some(2));
        assert_eq!(
            register.claim(&root),
            None,
            "a ready root that has not moved is not walked again"
        );
    }

    /// PIN — a root that moved is claimed again, with a higher epoch, and keeps
    /// its old index until the new one lands.
    ///
    /// The epoch has to rise for `accept` to be able to tell the two walks
    /// apart; the old index has to stay for the palette to have anything to draw
    /// while the second walk is out.
    ///
    /// MUTATIONS:
    /// - `mark_dirty` clearing `entry.index`: the "still readable" assertion
    ///   goes red.
    /// - `claim` not incrementing `epoch` when the entry already exists: the
    ///   epoch assertion goes red and every later answer is accepted as current.
    /// - `mark_dirty` written as `self.roots.entry(..).or_default()`: the last
    ///   assertion goes red. It is written against the register's own map and
    ///   not against [`FileIndexes::state`] on purpose — a phantom entry answers
    ///   `Missing` exactly like no entry at all, so the public surface cannot
    ///   see this one and only the map can.
    #[test]
    fn a_root_that_moved_is_claimed_again_with_a_higher_epoch() {
        let root = PathBuf::from("/roots/three");
        let mut register = FileIndexes::default();
        let first = register.claim(&root).expect("a first claim");
        register.accept(IndexResponse {
            root: root.clone(),
            epoch: first.epoch,
            index: ready(&["one.txt"]),
        });

        register.mark_dirty(&root);
        assert_eq!(
            register.state(&root),
            IndexState::Ready,
            "news alone does not take the index away"
        );

        let second = register
            .claim(&root)
            .expect("a dirty root is claimed again");
        assert!(
            second.epoch > first.epoch,
            "{} is not newer than {}",
            second.epoch,
            first.epoch
        );
        assert_eq!(
            register.get(&root).map(FileIndex::len),
            Some(1),
            "and the old index is still readable while the new walk is out"
        );

        let unknown = PathBuf::from("/roots/never-claimed");
        register.mark_dirty(&unknown);
        assert!(
            !register.roots.contains_key(&unknown),
            "news about a root nobody asked for creates nothing to rebuild"
        );
    }

    /// PIN — an answer from a superseded epoch is dropped, and does not
    /// overwrite a newer index.
    ///
    /// This is the case the epoch exists for: a slow walk that was already on
    /// the thread when the tree moved comes back describing a tree that is gone.
    /// Filing it would replace a correct index with an out-of-date one, and the
    /// state would say `Ready` about it.
    ///
    /// MUTATIONS:
    /// - deleting the `entry.epoch != response.epoch` guard in `accept`: the
    ///   stale answer lands, both length assertions go red.
    /// - `!=` → `<`: the stale answer still lands.
    /// - `accept` rewritten as `self.roots.entry(..).or_default()` **and** the
    ///   epoch guard removed together: the ghost lands and the last assertion
    ///   goes red. Named as a pair because it is a pair — either change alone
    ///   is caught by the other guard, since a freshly defaulted entry carries
    ///   epoch 0 and no real answer ever does.
    #[test]
    fn an_answer_from_a_superseded_epoch_does_not_overwrite_a_newer_index() {
        let root = PathBuf::from("/roots/four");
        let mut register = FileIndexes::default();
        let first = register.claim(&root).expect("a first claim");
        register.mark_dirty(&root);
        let second = register
            .claim(&root)
            .expect("a dirty root is claimed again");

        register.accept(IndexResponse {
            root: root.clone(),
            epoch: second.epoch,
            index: ready(&["new-one.txt", "new-two.txt"]),
        });
        assert_eq!(register.get(&root).map(FileIndex::len), Some(2));

        register.accept(IndexResponse {
            root: root.clone(),
            epoch: first.epoch,
            index: ready(&["stale.txt"]),
        });
        assert_eq!(
            register.get(&root).map(FileIndex::len),
            Some(2),
            "the walk that was already out described a tree that has moved"
        );
        assert_eq!(register.state(&root), IndexState::Ready);

        let stranger = PathBuf::from("/roots/never-asked");
        register.accept(IndexResponse {
            root: stranger.clone(),
            epoch: 1,
            index: ready(&["ghost.txt"]),
        });
        assert_eq!(
            register.state(&stranger),
            IndexState::Missing,
            "an answer about a root this register never asked about is not its answer"
        );
    }

    /// PIN — a root nobody wants any more is forgotten.
    ///
    /// A files column that is closed takes its index with it. Thirty thousand
    /// entries is megabytes, and this register is the only thing holding them.
    ///
    /// MUTATIONS:
    /// - `retain` inverted to `!wanted.contains(root)`: both assertions swap and
    ///   both go red.
    /// - `retain` doing nothing: the first assertion goes red.
    #[test]
    fn a_root_nobody_wants_any_more_is_forgotten() {
        let kept = PathBuf::from("/roots/kept");
        let dropped = PathBuf::from("/roots/dropped");
        let mut register = FileIndexes::default();
        for root in [&kept, &dropped] {
            let request = register.claim(root).expect("a first claim");
            register.accept(IndexResponse {
                root: root.clone(),
                epoch: request.epoch,
                index: ready(&["one.txt"]),
            });
        }

        let wanted: BTreeSet<PathBuf> = [kept.clone()].into_iter().collect();
        register.retain(&wanted);

        assert_eq!(register.state(&dropped), IndexState::Missing);
        assert!(register.get(&dropped).is_none());
        assert_eq!(register.state(&kept), IndexState::Ready);
        assert_eq!(register.get(&kept).map(FileIndex::len), Some(1));
    }
}
