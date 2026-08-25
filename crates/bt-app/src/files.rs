//! The files column's data plane: the thread that reads directories, and the
//! pure rules that turn what it read into the rows a tree draws.
//!
//! **Why a thread at all.** `DESIGN.md` §7.1.3 asks for directory enumeration
//! that is asynchronous and cancellable, and R-i draws the consequence in one
//! line: no `read_dir` on the event loop. A directory is not a data structure,
//! it is a question for a disk — a cold network share answers in seconds and a
//! `node_modules` answers with a hundred thousand names — and a frame that waits
//! for either is a frame that is not drawn. So this module owns the same shape
//! `bt-math-worker` already owns: a named thread, a request channel, a response
//! channel, an [`crate::AppEvent`] to wake the loop, newest-per-target
//! coalescing, and a one-way degradation when the thread is gone.
//!
//! **What is on which side.** Sorting, hiding and capping happen on the worker,
//! not here on the way out: they are per-name work proportional to the
//! directory, and the whole point of asking off-thread is that the answer
//! arrives already in the shape the tree wants. What stays on the event loop is
//! only the walk over already-answered directories, which is proportional to
//! what is *visible*.
//!
//! **Identity.** A node's stable id (`DESIGN.md` §7.1.3, schema §3.4) is its
//! root-relative path with `/` separators and a leading `/` — `/src`,
//! `/src/main.rs` — and the root itself is the empty string. This is the id that
//! `open` and `sel` are written to disk as, so it is also the id this module
//! keys its cache on: one vocabulary, not a disk one and a memory one that have
//! to agree.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

use crate::seats::FilesLeafState;
use crate::{AppEvent, LeafId};

/// How many entries of one directory the tree will show.
///
/// A cap is not a fear of large directories, it is the only honest thing to say
/// about them: a pane 240 logical pixels wide showing 24-pixel rows can put
/// perhaps two dozen names in front of you, and a directory of a hundred
/// thousand is not browsable by scrolling however patient the scroller is. L166
/// asks for exactly this shape of surrender — read the whole directory, show the
/// first `N` in sort order, and *say so on a row of its own* rather than
/// pretending the rest are not there.
pub const DIR_ENTRY_CAP: usize = 2000;

/// The notice shown once when the files worker has stopped.
///
/// Worded like [`crate::math_worker_stopped_notice()`] and for the same reason: a
/// worker dying is a feature going away, not a session ending, and the sentence
/// has to say which half still works.
pub fn files_worker_stopped_notice() -> &'static str {
    crate::i18n::Text::FilesWorkerStopped.text()
}

/// **Who is asking.**
///
/// There are two kinds of file tree in this window and they are addressed
/// differently, because they *are* different things. A docked column is a seat
/// in a tab, and the pair names it. A floating window is not a seat and is not in
/// a tab (`M2-layout-solver-spec.md` §2.6.4: 浮窗不是座位、不进树) — there is at
/// most one of them in the whole window (§7.1.2「全窗口单例」), so what has to be
/// carried is not *which* float but *which view* the one float was showing when
/// the question was asked.
///
/// That is the epoch. Redirecting the peek to another trigger, or tearing a
/// column out into a window, replaces the view behind the same singleton; an
/// answer that was already in flight for the old one would otherwise be filed
/// into the new one's cache, where its keys mean somewhere else entirely. The
/// docked side solves the same problem by checking the seat still exists and by
/// dropping the cache on re-root; this is that guard for a surface that has no
/// seat to check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilesHost {
    /// A column on the tree: the seat, in the tab that holds it.
    ///
    /// Addressed by [`LeafId`] rather than by [`crate::seats::SeatId`] alone for
    /// the reason written over `MathWorkerRequest`: a seat id is only unique
    /// inside its tab, and a worker that answers the wrong tab's pane is the bug
    /// that comment records having already been paid for once.
    Docked(LeafId),
    /// The floating window, showing the view minted at this epoch.
    Float(u64),
}

/// "Read this directory for this tree of this window."
///
/// **The window is part of the address** (user report, 2026-08-23). Both spellings
/// of [`FilesHost`] are minted by counters that start again in every window — a
/// `TabId` inside [`crate::LeafId`], and a float's epoch — so neither names one
/// tree on its own. The pair does, and until it did, the first window in the
/// opening order took every answer off the one shared channel and dropped the ones
/// that were not its own: the second window's column stood empty for ever.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirRequest {
    pub window: WindowId,
    pub host: FilesHost,
    /// The stable id of the directory being read — `""` for the column's root.
    pub key: String,
    /// Where on disk that id currently points.
    pub path: PathBuf,
}

impl DirRequest {
    /// Two requests are the same question when they name the same directory of
    /// the same column.
    ///
    /// The path is deliberately not part of this: within one column a key *is* a
    /// path, and if a re-root ever makes the same key mean somewhere else, the
    /// newer request is the one that should win — which is exactly what
    /// coalescing on the pair already does.
    fn same_target(&self, other: &Self) -> bool {
        self.window == other.window && self.host == other.host && self.key == other.key
    }
}

/// What the worker found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirResponse {
    /// The window the asking tree was in — see [`DirRequest::window`].
    pub window: WindowId,
    pub host: FilesHost,
    pub key: String,
    pub outcome: DirOutcome,
}

impl DirResponse {
    /// **Who this answer belongs to** (F1b) — a docked column belongs to its
    /// tab and travels with it between windows; a float belongs to the window
    /// that minted its epoch and cannot travel at all.
    pub fn owner(&self) -> crate::AnswerOwner {
        match self.host {
            FilesHost::Docked(leaf) => crate::AnswerOwner::Tab(leaf.tab),
            FilesHost::Float(_) => crate::AnswerOwner::Window(self.window),
        }
    }
}

/// A directory either lists or it does not, and both are answers.
///
/// There is no third "empty" case: an empty directory is a [`Self::Listed`] with
/// no entries, and it draws as a directory that opened and had nothing in it,
/// which is the truth. Conflating it with a failure is how a permissions problem
/// comes to look like an empty folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DirOutcome {
    Listed(DirListing),
    Failed(DirFault),
}

/// One directory, already in display order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DirListing {
    /// Sorted by [`compare_entries`], concealed names already dropped, and cut
    /// to at most [`DIR_ENTRY_CAP`].
    pub entries: Vec<DirEntry>,
    /// How many names the cap left out. Zero when the whole directory fits,
    /// which is the overwhelmingly common case.
    pub omitted: usize,
    /// This directory with every link on the way in resolved.
    ///
    /// It is the only thing that makes a cycle nameable: `a/link -> a` is two
    /// different ids for one directory, and only the resolved form shows they
    /// are the same place. `None` when the resolution itself failed, in which
    /// case no cycle can be claimed — an unprovable cycle is not one.
    pub canonical: Option<PathBuf>,
}

/// One name in a directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntry {
    pub name: String,
    /// Whether opening this shows more rows. A symlink pointing at a directory
    /// is a directory here, because that is what following it does (Q5b).
    pub is_dir: bool,
    /// Whether the name is a link, junction or other reparse point. Kept
    /// separate from `is_dir` because it is a fact about the *name*, not about
    /// what is behind it, and it is what tells the cycle guard which rows are
    /// even capable of pointing backwards.
    pub is_symlink: bool,
}

/// The ways a directory can decline to be read.
///
/// These are three of the four states §7.1.3 insists have a display of their
/// own; the fourth, "still loading", is not a fault and lives on
/// [`DirNode::Pending`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirFault {
    /// The directory exists and is not ours to read.
    PermissionDenied,
    /// It is not there — deleted under us, or a restored root that has moved.
    NotFound,
    /// It is there and the read still failed.
    Unreadable,
}

impl DirFault {
    fn from_io(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::NotFound => Self::NotFound,
            _ => Self::Unreadable,
        }
    }

    /// The row's own words for itself.
    pub fn notice(self) -> &'static str {
        match self {
            Self::PermissionDenied => crate::i18n::Text::FilesPermissionDenied.text(),
            Self::NotFound => crate::i18n::Text::FilesNotFound.text(),
            Self::Unreadable => crate::i18n::Text::FilesUnreadable.text(),
        }
    }
}

/// What is known about one directory of one column.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DirNode {
    /// Asked for, not yet answered. This is the "loading" state, and it is a
    /// state of the *cache* rather than of the response, which is why it is here
    /// and not in [`DirOutcome`].
    Pending,
    Listed(DirListing),
    Failed(DirFault),
}

/// Every directory this column has asked about, by stable id.
///
/// **Why it is not on [`FilesLeafState`].** That struct is the column's durable
/// truth — the three facts that cross the disk — and it is read by clone through
/// a single accessor. A directory cache is neither durable nor cheap to clone,
/// and putting it there would quietly make every read of `root` copy a hundred
/// thousand names. The split is the same one `sessions` and `files` already
/// make: what the column *is* beside what the column currently *knows*.
///
/// **Why collapsing does not evict.** A fold is a statement about what you want
/// to look at, not about what you want forgotten, and re-opening a folder you
/// just closed should not stutter. The cache lives until the column does; a
/// re-open re-asks anyway (there is no watcher yet, so re-opening a folder is
/// the manual refresh), and the fresh answer replaces the kept one.
#[derive(Clone, Debug, Default)]
pub struct DirCache {
    dirs: BTreeMap<String, DirNode>,
    /// Where each directory row's disclosure triangle is through its turn (C33).
    ///
    /// Keyed by the same stable id and living in the same table for the same
    /// lifetime reason: a turn is about one row of one column, and when the
    /// column goes the turn has nothing left to be about. Absent for every row
    /// nobody has clicked, which is the overwhelming majority — a restored tree
    /// comes up with its folders already open and no turns in flight, because
    /// they did not turn, they were found that way.
    turns: BTreeMap<String, crate::RevealTween>,
    /// Where the tree is scrolled to, in rows-worth of logical pixels.
    ///
    /// Transient by ruling: a restored column comes back at the top, because a
    /// scroll offset into a tree whose directories have not been read yet is an
    /// offset into nothing.
    pub scroll_px: f32,
    /// How many times [`Self::dirs`] has been written — this cache's **damage
    /// counter** (`docs/DESIGN.md` §7.1.6b′ F2).
    ///
    /// A reader that wants to know whether re-deriving something from this tree
    /// could possibly give a different answer compares one integer instead of
    /// walking it. The focus column's thumbnails are that reader: a card showing
    /// four rows of a files column has to re-walk the tree to find them, and
    /// re-walking a hundred thousand cached names to discover that nothing moved
    /// is the cost the whole budget exists to refuse.
    ///
    /// **Content, not clock.** It counts writes to the directory table and
    /// nothing else — not the scroll, not a triangle's angle, not the selection,
    /// because none of those change which rows exist. A reader that cares about
    /// one of those compares that one, and [`FilesLeafState`] carries the rest.
    ///
    /// [`FilesLeafState`]: crate::seats::FilesLeafState
    revision: u64,
}

/// A column that has read nothing — "no cache yet", as something a reader can
/// borrow.
///
/// The `NO_FILES_TREES` idiom one module over, and a `static` rather than an
/// associated `const` for the reason the compiler gives: a `const` is a value
/// materialised at each use, so borrowing one produces a temporary that cannot
/// outlive the expression. Named, so that a reader saying "this column has read
/// nothing" is visibly saying it rather than looking like one that forgot to pass
/// a cache.
pub static EMPTY_DIR_CACHE: DirCache = DirCache {
    dirs: BTreeMap::new(),
    turns: BTreeMap::new(),
    scroll_px: 0.0,
    revision: 0,
};

impl DirCache {
    pub fn get(&self, key: &str) -> Option<&DirNode> {
        self.dirs.get(key)
    }

    /// How many times the directory table has been written — see
    /// [`Self::revision`].
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Record that a directory has been asked about.
    pub fn mark_pending(&mut self, key: &str) {
        self.dirs.insert(key.to_owned(), DirNode::Pending);
        self.revision = self.revision.wrapping_add(1);
    }

    /// **Give up on every read that is still out** (F1b, `plan.md` v4 增补 ②).
    ///
    /// Called when the column this cache belongs to is given a new [`crate::LeafId`]
    /// — a pane promoted into a tab of its own, or moved into another tab. Every
    /// question it has out carries the old address and will be dropped when it
    /// lands, so the ledger that stops it being asked twice has to stop saying it
    /// was asked at all: `Pending` becomes *unheard of*, which is what
    /// [`tree_view`] turns into a `wanted` entry.
    ///
    /// **Only the pending nodes.** A directory already listed is an answer in
    /// hand, and throwing it away would blink a column that merely changed tabs
    /// back to "Loading …" — the flicker `pane_into_new_tab` carries these caches
    /// across tabs to prevent.
    pub fn forget_pending(&mut self) {
        let before = self.dirs.len();
        self.dirs
            .retain(|_, node| !matches!(node, DirNode::Pending));
        if self.dirs.len() != before {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Take an answer from the worker.
    pub fn accept(&mut self, key: &str, outcome: DirOutcome) {
        let node = match outcome {
            DirOutcome::Listed(listing) => DirNode::Listed(listing),
            DirOutcome::Failed(fault) => DirNode::Failed(fault),
        };
        self.dirs.insert(key.to_owned(), node);
        self.revision = self.revision.wrapping_add(1);
    }

    /// Start a row's triangle turning towards `open`.
    ///
    /// The tween is minted standing at the *other* end rather than at rest,
    /// because a row being clicked for the first time is a row whose triangle
    /// has been sitting still at its old angle: easing it from where it is is
    /// the whole point, and easing it from where it is going is a snap.
    pub fn turn_row(&mut self, key: &str, open: bool, now: Instant, motion: crate::Motion) {
        let target = f32::from(u8::from(open));
        self.turns
            .entry(key.to_owned())
            .or_insert_with(|| {
                crate::RevealTween::resting_on(
                    1.0 - target,
                    Duration::from_millis(crate::seats::FILES_ROW_TRI_TURN_MS),
                    // `.frow .tri { transition: transform 120ms
                    // cubic-bezier(.2,0,0,1) }` (C33) — the mock-up's snappy
                    // curve, not CSS `ease`. It is the same curve the tab
                    // reorder and the profile chevron turn on, and it matters
                    // most on a short travel like this one: `ease` spends its
                    // first third barely moving, which on 120ms reads as the
                    // triangle hesitating before it turns.
                    crate::GRAB_EASE,
                )
            })
            .retarget(target, now, motion);
    }

    /// How far through its turn a row's triangle is, and whether it is still
    /// moving.
    ///
    /// A row with no tween answers with its own state and "not moving", which is
    /// what makes this total over rows that have never been clicked.
    pub fn row_turn(
        &self,
        key: &str,
        open: bool,
        now: Instant,
        motion: crate::Motion,
    ) -> (f32, bool) {
        match self.turns.get(key) {
            Some(tween) => tween.sample(now, motion),
            None => (f32::from(u8::from(open)), false),
        }
    }

    /// Whether any triangle in this column is mid-turn.
    pub fn any_turning(&self, now: Instant, motion: crate::Motion) -> bool {
        self.turns.values().any(|tween| tween.sample(now, motion).1)
    }

    /// The resolved location of a directory, if it has been read.
    fn canonical(&self, key: &str) -> Option<&Path> {
        match self.dirs.get(key) {
            Some(DirNode::Listed(listing)) => listing.canonical.as_deref(),
            _ => None,
        }
    }
}

/// One drawable line of the tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeRow {
    /// The stable id of the node this row is, or of the directory a notice row
    /// belongs to.
    pub key: String,
    pub name: String,
    /// How many ancestors stand between this row and the root, which is all the
    /// indent formula needs (C31).
    pub depth: usize,
    pub kind: RowKind,
}

impl TreeRow {
    /// Whether clicking this row means anything.
    ///
    /// Notice rows are sentences, not nodes: they have no id to select and
    /// nothing to open, and treating them as rows that merely happen to do
    /// nothing is how a stray click lands a selection on the word "Loading".
    pub fn is_node(&self) -> bool {
        !matches!(self.kind, RowKind::Notice(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowKind {
    Directory {
        open: bool,
    },
    File,
    /// A directory that resolves to one of its own ancestors.
    ///
    /// It is drawn as a directory and refuses to open, which is the honest
    /// account: the folder is really there, and what is behind it is where you
    /// already are.
    Cycle,
    Notice(RowNotice),
}

/// A row that says something about the directory it sits under rather than
/// naming a node in it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowNotice {
    Loading,
    Fault(DirFault),
    /// The tail row the cap owes: how many names are not shown.
    More(usize),
    /// The root read, and there was nothing in it.
    ///
    /// Its own state and not silence, because silence is what a *broken* pane
    /// looks like — the argument `placeholder_seat_notice()` already makes.
    Empty,
    /// The column has no root at all: opened with no focused shell and no
    /// `HOME` to fall back to. Not a folder that failed — a column that was
    /// never pointed anywhere, which is a different sentence.
    Unrooted,
}

/// The rows to draw, and the directories that have to be asked about before the
/// picture is complete.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TreeView {
    pub rows: Vec<TreeRow>,
    /// Open directories with nothing in the cache — the walk's own list of what
    /// it could not answer, so the caller does not have to re-derive it.
    pub wanted: Vec<String>,
}

impl TreeView {
    /// Whether every row here is a **final** answer about what is in the tree.
    ///
    /// A `Loading` row is a placeholder for however many names are on their way,
    /// so the row count of an unsettled view is not the height of anything — it
    /// is one line standing in for an unknown number of them. Asking it for a
    /// content height is what opened a floating tree as a bare title strip in
    /// the corner (user report, 2026-08-13): every float is born with an empty
    /// [`DirCache`], so the first frame after `place_float` always had exactly
    /// one row, and the window sized itself to it and then had to grow.
    ///
    /// A folder that is genuinely empty answers `true` here and gets its one
    /// `Empty` row's height, which is right: that *is* the whole of what it has
    /// to show.
    #[must_use]
    pub fn settled(&self) -> bool {
        !self
            .rows
            .iter()
            .any(|row| matches!(row.kind, RowKind::Notice(RowNotice::Loading)))
    }
}

/// Walk the open directories and produce the visible rows.
///
/// This is `filesVisibleRows` (C38) with the one difference that separates a
/// real filesystem from the mock-up's hand-written tree: a directory that is
/// open may not be *known*, and the row it gets in that case is a notice rather
/// than nothing. Depth-first, in cache order, descending only where `open` says
/// to — so its cost is the number of rows on screen and not the size of
/// anything on disk.
pub fn tree_view(state: &FilesLeafState, cache: &DirCache) -> TreeView {
    let mut view = TreeView::default();
    if !root_is_addressable(&state.root) {
        view.rows.push(notice_row("", 0, RowNotice::Unrooted));
        return view;
    }
    let mut ancestors: Vec<PathBuf> = Vec::new();
    if let Some(canonical) = cache.canonical("") {
        ancestors.push(canonical.to_path_buf());
    }
    walk_dir(state, cache, "", 0, &mut ancestors, &mut view);
    if view.rows.is_empty() {
        view.rows.push(notice_row("", 0, RowNotice::Empty));
    }
    view
}

/// One directory's contribution to the row list, and its open children's.
fn walk_dir(
    state: &FilesLeafState,
    cache: &DirCache,
    key: &str,
    depth: usize,
    ancestors: &mut Vec<PathBuf>,
    view: &mut TreeView,
) {
    let listing = match cache.get(key) {
        Some(DirNode::Listed(listing)) => listing,
        Some(DirNode::Pending) => {
            view.rows.push(notice_row(key, depth, RowNotice::Loading));
            return;
        }
        Some(DirNode::Failed(fault)) => {
            view.rows
                .push(notice_row(key, depth, RowNotice::Fault(*fault)));
            return;
        }
        None => {
            view.wanted.push(key.to_owned());
            view.rows.push(notice_row(key, depth, RowNotice::Loading));
            return;
        }
    };
    for entry in &listing.entries {
        let child = child_key(key, &entry.name);
        if !entry.is_dir {
            view.rows.push(TreeRow {
                key: child,
                name: entry.name.clone(),
                depth,
                kind: RowKind::File,
            });
            continue;
        }
        let open = state.open.contains(&child);
        let cycles = open
            && cache
                .canonical(&child)
                .is_some_and(|canonical| ancestors.iter().any(|seen| seen == canonical));
        view.rows.push(TreeRow {
            key: child.clone(),
            name: entry.name.clone(),
            depth,
            kind: if cycles {
                RowKind::Cycle
            } else {
                RowKind::Directory { open }
            },
        });
        if !open || cycles {
            continue;
        }
        let pushed = cache.canonical(&child).map(Path::to_path_buf);
        if let Some(canonical) = pushed.clone() {
            ancestors.push(canonical);
        }
        walk_dir(state, cache, &child, depth + 1, ancestors, view);
        if pushed.is_some() {
            ancestors.pop();
        }
    }
    if listing.omitted > 0 {
        view.rows
            .push(notice_row(key, depth, RowNotice::More(listing.omitted)));
    }
}

fn notice_row(key: &str, depth: usize, notice: RowNotice) -> TreeRow {
    TreeRow {
        key: key.to_owned(),
        name: match notice {
            RowNotice::Loading => crate::i18n::Text::FilesLoading.text().to_owned(),
            RowNotice::Fault(fault) => fault.notice().to_owned(),
            RowNotice::More(count) => crate::i18n::files_more_not_shown(count),
            RowNotice::Empty => crate::i18n::Text::FilesEmpty.text().to_owned(),
            RowNotice::Unrooted => crate::i18n::Text::FilesUnrooted.text().to_owned(),
        },
        depth,
        kind: RowKind::Notice(notice),
    }
}

/// The stable id of a child of `parent`.
pub fn child_key(parent: &str, name: &str) -> String {
    format!("{parent}/{name}")
}

/// Where a stable id points on disk right now.
///
/// The id's `/` separators are not reused as path separators — each segment is
/// pushed in turn, so the platform's own separator is what reaches the
/// filesystem and a root like `C:\Users\me` stays a Windows path all the way
/// down (L167).
pub fn full_path(root: &str, key: &str) -> PathBuf {
    let mut path = PathBuf::from(root);
    for segment in key.split('/').filter(|segment| !segment.is_empty()) {
        path.push(segment);
    }
    path
}

/// Drop a selection that the tree can prove is gone (C35).
///
/// **Why it is not simply "not in `rows`".** The mock-up could ask that, because
/// its tree was entirely in memory: a path missing from the visible rows was a
/// path that did not exist. Here a selection is missing from the rows for two
/// very different reasons — its folder was collapsed, or its folder was read and
/// it was not in it — and only the second is a dead selection. Healing on the
/// first would mean that restoring a session, whose directories are all unread
/// for one frame, throws the restored selection away before it can ever be
/// shown. So the question asked here is the narrow one: is there an ancestor
/// that has been *read* and does not contain the next step of the path.
pub fn selection_is_dead(state: &FilesLeafState, cache: &DirCache) -> bool {
    let Some(sel) = state.sel.as_deref() else {
        return false;
    };
    let mut parent = String::new();
    for segment in sel.split('/').filter(|segment| !segment.is_empty()) {
        match cache.get(&parent) {
            Some(DirNode::Listed(listing)) => {
                if !listing.entries.iter().any(|entry| entry.name == segment) {
                    return true;
                }
            }
            // Unread or unreadable: nothing is proven either way, and an
            // unproven death is not one.
            _ => return false,
        }
        parent = child_key(&parent, segment);
    }
    false
}

/// One keystroke of the tree's keyboard contract, with the keyboard already
/// decided to be the tree's.
///
/// Decoded from a key *before* the column is consulted, so that the question
/// "what does this key mean here" is answered once and in one place. The
/// vocabulary is VS Code's tree (`DESIGN.md` §7.1.3) and nothing wider: a tree
/// has one axis of travel, one axis of disclosure and one verb, and every other
/// key that arrives while the tree holds the keyboard is a key the tree owns and
/// does nothing with (D49) rather than one it passes along to a shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeCommand {
    Up,
    Down,
    /// Fold, or step out to the parent row.
    Left,
    /// Unfold, or step in to the first child row.
    Right,
    Home,
    End,
    /// Enter or Space: a folder folds and unfolds, a file opens.
    Activate,
    /// The Menu key, or `Shift+F10`: raise the selected row's own menu (K143;
    /// a folder row's too, since the user ruling of 2026-08-25).
    ///
    /// The keyboard's half of a verb the pointer already has. §7.1.3 requires
    /// the file row's menu to be "可键盘化", and a menu that can only be reached
    /// by right-clicking is a menu a keyboard cannot reach at all — which would
    /// leave `Insert path into terminal`, whose whole reason for existing is to
    /// be the *discoverable* home of a verb taken off the drag, unreachable
    /// without a mouse.
    ContextMenu,
    /// Esc: hand the keyboard back to the terminal.
    Release,
}

/// The keys the tree answers to, and only when they arrive unmodified.
///
/// A modifier makes a chord, and a chord is the window's business — the
/// shortcut table has already had its say by the time a key reaches here, and
/// `Shift+PageUp` is a request to scroll a terminal's history whatever pane is
/// lit. What the tree claims is the bare navigation set, which is exactly the
/// set no chord table contains.
///
/// **`Shift+F10` is the one chord, and it is not an exception to that rule.**
/// It is the system-wide name of the Menu key on the very many keyboards that
/// do not have one, which makes it part of the same *bare* vocabulary rather
/// than a shortcut competing with the table: no chord table anywhere contains
/// it either, because on every platform it already means this.
pub fn tree_command(
    key: &winit::keyboard::Key,
    modifiers: winit::keyboard::ModifiersState,
) -> Option<TreeCommand> {
    use winit::keyboard::{Key, NamedKey};
    if modifiers == winit::keyboard::ModifiersState::SHIFT
        && matches!(key, Key::Named(NamedKey::F10))
    {
        return Some(TreeCommand::ContextMenu);
    }
    if !modifiers.is_empty() {
        return None;
    }
    Some(match key {
        Key::Named(NamedKey::ContextMenu) => TreeCommand::ContextMenu,
        Key::Named(NamedKey::ArrowUp) => TreeCommand::Up,
        Key::Named(NamedKey::ArrowDown) => TreeCommand::Down,
        Key::Named(NamedKey::ArrowLeft) => TreeCommand::Left,
        Key::Named(NamedKey::ArrowRight) => TreeCommand::Right,
        Key::Named(NamedKey::Home) => TreeCommand::Home,
        Key::Named(NamedKey::End) => TreeCommand::End,
        // Space arrives named rather than as a character, which is the one
        // difference between this list and the mock-up's `case " "`.
        Key::Named(NamedKey::Enter | NamedKey::Space) => TreeCommand::Activate,
        Key::Named(NamedKey::Escape) => TreeCommand::Release,
        _ => return None,
    })
}

/// What a [`TreeCommand`] turned out to mean for one column.
///
/// Reported rather than performed for the reason `press_files_node` is: opening
/// a folder owes a directory read, activating a file owes a window an
/// application, and neither is answerable without the runtime — while *which* of
/// them is owed is answerable with nothing but the rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreeAction {
    /// The tree owns the key and there was nothing for it to do.
    None,
    /// Only the selection moved.
    Select(String),
    /// A folder unfolded, and its rows are owed a read.
    Opened(String),
    Closed(String),
    /// A file was opened.
    Activate(String),
    /// A row asked for its own menu (K143; either kind of node since the user
    /// ruling of 2026-08-25).
    ContextMenu(String),
    /// The keyboard goes back to the terminal.
    Release,
}

/// The VS Code tree contract (D44), applied to one column's durable state.
///
/// **Notice rows are not travelled through.** "Loading…", a permission refusal
/// and the tail row counting what a cap left out are sentences about the list,
/// and a selection that can land on a sentence is a selection that can be asked
/// to open one. [`TreeRow::is_node`] already draws that line for the mouse; this
/// walks the same subset, which is what keeps ↑↓ and a click agreeing about
/// what the row after this one is.
///
/// **Travel clamps and never wraps** (D45). A tree is a list with a top and a
/// bottom, and an ↑ at the top that lands you at the bottom is a navigation you
/// have to undo before you can trust where you are.
pub fn apply_tree_command(
    state: &mut FilesLeafState,
    rows: &[TreeRow],
    command: TreeCommand,
) -> TreeAction {
    if command == TreeCommand::Release {
        return TreeAction::Release;
    }
    let nodes: Vec<&TreeRow> = rows.iter().filter(|row| row.is_node()).collect();
    if nodes.is_empty() {
        return TreeAction::None;
    }
    let at = state
        .sel
        .as_deref()
        .and_then(|sel| nodes.iter().position(|row| row.key == sel));
    let select = |state: &mut FilesLeafState, position: usize| {
        let key = nodes[position.min(nodes.len() - 1)].key.clone();
        state.sel = Some(key.clone());
        TreeAction::Select(key)
    };
    // A column nobody has pointed at yet answers its first travelling key with
    // its first row rather than its second. The mock-up's `if (i < 0) i = 0`
    // then moves off that zero, so ↓ into a fresh tree lands on the *second*
    // name — the row you can see being skipped by the key that means "go down
    // one" is the kind of off-by-one that reads as a dropped keystroke.
    let Some(at) = at else {
        return match command {
            TreeCommand::End => select(state, nodes.len() - 1),
            // Nothing is selected, so there is nothing to act *on*. Both of
            // these ask a question about a particular row, and neither should
            // answer it by first picking one — a menu that appeared over a row
            // the user had not chosen would be a menu about the wrong file.
            TreeCommand::Activate | TreeCommand::ContextMenu => TreeAction::None,
            _ => select(state, 0),
        };
    };
    let row = nodes[at];
    let key = row.key.clone();
    match command {
        TreeCommand::Up => select(state, at.saturating_sub(1)),
        TreeCommand::Down => select(state, at + 1),
        TreeCommand::Home => select(state, 0),
        TreeCommand::End => select(state, nodes.len() - 1),
        TreeCommand::Right => match row.kind {
            RowKind::Directory { open: false } => {
                state.open.insert(key.clone());
                TreeAction::Opened(key)
            }
            _ => select(state, at + 1),
        },
        TreeCommand::Left => match row.kind {
            RowKind::Directory { open: true } => {
                state.open.remove(&key);
                TreeAction::Closed(key)
            }
            // Out to the parent row — and a row whose parent *is* the root has
            // none, because the root is the column's head and not a line in its
            // list. Standing still is the honest answer there.
            _ => match parent_key(&key)
                .and_then(|parent| nodes.iter().position(|row| row.key == parent))
            {
                Some(parent) => select(state, parent),
                None => TreeAction::None,
            },
        },
        TreeCommand::Activate => match row.kind {
            RowKind::Directory { open: true } => {
                state.open.remove(&key);
                TreeAction::Closed(key)
            }
            RowKind::Directory { open: false } => {
                state.open.insert(key.clone());
                TreeAction::Opened(key)
            }
            RowKind::File => TreeAction::Activate(key),
            // A folder that resolves to one of its own ancestors has nothing to
            // show that is not already on screen, exactly as under the mouse.
            RowKind::Cycle | RowKind::Notice(_) => TreeAction::None,
        },
        // **Both kinds of node**, which is the same line the right button is
        // held to (user ruling 2026-08-25). K143's "目录行不弹" was the right
        // answer while the menu was three verbs that were all about a file; a
        // folder row now has a menu of its own — its fold, a shell standing in
        // it, and the same three things to do with a path — so the key that asks
        // a row about itself gets an answer here too.
        //
        // The two rows that lead nowhere still answer with silence, and it is
        // the same silence they give the second press: a cycle would open onto
        // the place you are already standing, and a notice names no folder at
        // all. `crate::files_row_menu_subject` is where that judgement lives.
        TreeCommand::ContextMenu => match row.kind {
            RowKind::File | RowKind::Directory { .. } => TreeAction::ContextMenu(key),
            RowKind::Cycle | RowKind::Notice(_) => TreeAction::None,
        },
        TreeCommand::Release => TreeAction::Release,
    }
}

/// The stable id of the directory a node sits in, or `None` when that is the
/// root itself.
pub fn parent_key(key: &str) -> Option<String> {
    let cut = key.rfind('/')?;
    (cut > 0).then(|| key[..cut].to_owned())
}

/// Folders first, then names, each group case-insensitively alphabetical (Q4).
///
/// The tie-break on the raw name is not decoration: without it `README` and
/// `readme` are equal, and the order two equal elements come out in is whatever
/// the directory happened to hand over. That is a difference the same session
/// can show twice in one run, so the comparator is made total instead.
pub fn compare_entries(a: &DirEntry, b: &DirEntry) -> Ordering {
    b.is_dir
        .cmp(&a.is_dir)
        .then_with(|| fold_case(&a.name).cmp(&fold_case(&b.name)))
        .then_with(|| a.name.cmp(&b.name))
}

fn fold_case(name: &str) -> String {
    name.to_lowercase()
}

/// Whether a name is one the tree does not show (Q5a).
///
/// A leading dot is *not* concealment here — `.gitignore` and `.cargo` are the
/// files a developer most wants in front of them, and the mock-up's own tree put
/// `.gitignore` on screen. What is concealed is what the platform itself marks
/// concealed: `desktop.ini`, `System Volume Information`, the pagefile. The
/// distinction is whose opinion it is — the author's or the operating system's.
#[cfg(windows)]
fn is_concealed(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    metadata.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
}

/// Elsewhere concealment is a naming convention rather than a bit, and the
/// ruling is that this tree does not honour that convention (Q5a).
#[cfg(not(windows))]
fn is_concealed(_metadata: &std::fs::Metadata) -> bool {
    false
}

/// Read one directory into the shape the tree draws. **Runs on the worker.**
pub fn read_directory(path: &Path) -> DirOutcome {
    let reader = match std::fs::read_dir(path) {
        Ok(reader) => reader,
        Err(error) => return DirOutcome::Failed(DirFault::from_io(&error)),
    };
    let mut entries = Vec::new();
    for entry in reader {
        // A name that vanishes between the directory being opened and being
        // walked is a name that is no longer in the directory, which is exactly
        // what leaving it out says.
        let Ok(entry) = entry else { continue };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if is_concealed(&metadata) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let is_symlink = file_type.is_symlink();
        // What is behind a link decides whether the row opens, so the link — and
        // only the link — is worth the extra call to look through.
        let is_dir = if is_symlink {
            std::fs::metadata(entry.path()).is_ok_and(|target| target.is_dir())
        } else {
            file_type.is_dir()
        };
        entries.push(DirEntry {
            name,
            is_dir,
            is_symlink,
        });
    }
    entries.sort_by(compare_entries);
    let omitted = entries.len().saturating_sub(DIR_ENTRY_CAP);
    entries.truncate(DIR_ENTRY_CAP);
    DirOutcome::Listed(DirListing {
        entries,
        omitted,
        canonical: canonical_path(path),
    })
}

/// The resolved form of a directory, used only to compare two ids for sameness.
fn canonical_path(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Newest-per-target queue, the shape `PendingScaleRequests` already gives the
/// resampling lane.
///
/// Reading a directory twice because it was folded and unfolded while a slow
/// share was still answering is work nobody is waiting for, and worse, the older
/// answer can only ever be the staler one. Keeping one request per
/// (column, directory) means the queue holds questions and not history.
#[derive(Default)]
struct PendingDirRequests {
    requests: std::collections::VecDeque<DirRequest>,
}

impl PendingDirRequests {
    fn push_latest(&mut self, request: DirRequest) {
        if let Some(index) = self
            .requests
            .iter()
            .position(|queued| queued.same_target(&request))
        {
            self.requests.remove(index);
        }
        self.requests.push_back(request);
    }

    fn pop_front(&mut self) -> Option<DirRequest> {
        self.requests.pop_front()
    }

    fn contains_target(&self, request: &DirRequest) -> bool {
        self.requests
            .iter()
            .any(|queued| queued.same_target(request))
    }

    fn drain_channel(&mut self, receiver: &mpsc::Receiver<DirRequest>) {
        while let Ok(request) = receiver.try_recv() {
            self.push_latest(request);
        }
    }
}

/// Serve directory reads, newest question per target first.
///
/// Split from [`FilesWorker::spawn`] so the coalescing can be tested without a
/// filesystem or an event loop, exactly as `run_scale_worker` is.
fn run_dir_worker(receiver: mpsc::Receiver<DirRequest>, mut execute: impl FnMut(DirRequest)) {
    let mut pending = PendingDirRequests::default();
    while let Ok(request) = receiver.recv() {
        pending.push_latest(request);
        pending.drain_channel(&receiver);
        while let Some(request) = pending.pop_front() {
            pending.drain_channel(&receiver);
            if pending.contains_target(&request) {
                continue;
            }
            execute(request);
        }
    }
}

/// The thread, and the two ends of the conversation with it.
pub struct FilesWorker {
    requests: mpsc::Sender<DirRequest>,
    pub responses: mpsc::Receiver<DirResponse>,
}

impl FilesWorker {
    pub fn spawn(proxy: EventLoopProxy<AppEvent>) -> Result<Self> {
        let (request_tx, request_rx) = mpsc::channel::<DirRequest>();
        let (response_tx, response_rx) = mpsc::channel::<DirResponse>();
        bt_platform::spawn_at_priority(
            "bt-files-worker",
            bt_platform::ThreadPriority::BelowNormal,
            move || {
                run_dir_worker(request_rx, |request| {
                    let outcome = read_directory(&request.path);
                    if response_tx
                        .send(DirResponse {
                            window: request.window,
                            host: request.host,
                            key: request.key,
                            outcome,
                        })
                        .is_ok()
                    {
                        let _ = proxy.send_event(AppEvent::FilesReady);
                    }
                });
            },
        )
        .context("spawn directory reading worker")?;
        Ok(Self {
            requests: request_tx,
            responses: response_rx,
        })
    }

    /// Ask, reporting whether the worker was still there to be asked.
    ///
    /// A `false` here is the one error this module has, and it is not an error
    /// the session should end for — see `disable_files_worker_state`.
    #[must_use]
    pub fn request(&self, request: DirRequest) -> bool {
        self.requests.send(request).is_ok()
    }
}

/// Turn directory reading off for the rest of the run, once.
///
/// The twin of `disable_math_worker_state`, and a one-way door for the same
/// reason: the thread is not coming back, so the only question left is whether
/// this is the first time anyone noticed.
pub fn disable_files_worker_state(running: &mut bool, notice_pending: &mut bool) -> bool {
    if !*running {
        return false;
    }
    *running = false;
    *notice_pending = true;
    eprintln!("directory reading worker stopped; terminal input and output remain available");
    true
}

pub fn take_files_worker_notice(notice_pending: &mut bool) -> Option<&'static str> {
    if std::mem::take(notice_pending) {
        Some(files_worker_stopped_notice())
    } else {
        None
    }
}

/// Whether a root string names somewhere a directory read could even begin.
///
/// A column opened with no focused shell and no `HOME` has an empty root, and an
/// empty root is not a path that failed — it is a column that was never pointed
/// anywhere. It gets its own silence rather than "Folder not found".
pub fn root_is_addressable(root: &str) -> bool {
    !root.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool) -> DirEntry {
        DirEntry {
            name: name.to_owned(),
            is_dir,
            is_symlink: false,
        }
    }

    fn listed(cache: &mut DirCache, key: &str, entries: Vec<DirEntry>) {
        cache.accept(
            key,
            DirOutcome::Listed(DirListing {
                entries,
                omitted: 0,
                canonical: Some(PathBuf::from(format!("/canon{key}"))),
            }),
        );
    }

    fn state(root: &str, open: &[&str], sel: Option<&str>) -> FilesLeafState {
        FilesLeafState {
            root: root.to_owned(),
            open: open.iter().map(|key| (*key).to_owned()).collect(),
            sel: sel.map(str::to_owned),
            view: crate::seats::FilesView::default(),
            git_expanded: None,
            git_sel: None,
            git_remotes_open: false,
        }
    }

    #[test]
    fn folders_lead_and_each_group_reads_alphabetically_whatever_the_case() {
        let mut entries = [
            entry("zebra.txt", false),
            entry("Apple.txt", false),
            entry("src", true),
            entry("banana.txt", false),
            entry("Docs", true),
        ];
        entries.sort_by(compare_entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Docs", "src", "Apple.txt", "banana.txt", "zebra.txt"]
        );
    }

    #[test]
    fn two_names_differing_only_in_case_still_have_one_order() {
        let mut entries = [entry("readme", false), entry("README", false)];
        entries.sort_by(compare_entries);
        let first = entries[0].name.clone();
        let mut reversed = [entry("README", false), entry("readme", false)];
        reversed.sort_by(compare_entries);
        assert_eq!(first, reversed[0].name, "the comparator has to be total");
    }

    #[test]
    fn an_unread_root_asks_for_itself_and_says_it_is_loading() {
        let view = tree_view(&state("/r", &[], None), &DirCache::default());
        assert_eq!(view.wanted, vec![String::new()]);
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].kind, RowKind::Notice(RowNotice::Loading));
        assert!(!view.rows[0].is_node());
    }

    #[test]
    fn only_opened_folders_contribute_rows() {
        let mut cache = DirCache::default();
        listed(
            &mut cache,
            "",
            vec![entry("src", true), entry("a.txt", false)],
        );
        listed(&mut cache, "/src", vec![entry("main.rs", false)]);

        let shut = tree_view(&state("/r", &[], None), &cache);
        assert_eq!(
            shut.rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            vec!["/src", "/a.txt"]
        );
        assert!(shut.wanted.is_empty());

        let open = tree_view(&state("/r", &["/src"], None), &cache);
        assert_eq!(
            open.rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            vec!["/src", "/src/main.rs", "/a.txt"]
        );
        assert_eq!(open.rows[1].depth, 1);
        assert_eq!(open.rows[0].kind, RowKind::Directory { open: true });
    }

    #[test]
    fn opening_a_folder_nobody_has_read_yet_is_what_puts_it_on_the_wanted_list() {
        let mut cache = DirCache::default();
        listed(&mut cache, "", vec![entry("src", true)]);
        let view = tree_view(&state("/r", &["/src"], None), &cache);
        assert_eq!(view.wanted, vec!["/src".to_owned()]);
        assert_eq!(view.rows[1].kind, RowKind::Notice(RowNotice::Loading));
    }

    #[test]
    fn a_folder_already_asked_about_is_not_asked_about_again() {
        let mut cache = DirCache::default();
        listed(&mut cache, "", vec![entry("src", true)]);
        cache.mark_pending("/src");
        let view = tree_view(&state("/r", &["/src"], None), &cache);
        assert!(view.wanted.is_empty(), "pending is already asked");
        assert_eq!(view.rows[1].kind, RowKind::Notice(RowNotice::Loading));
    }

    #[test]
    fn each_of_the_three_faults_gets_a_row_that_says_which_one_it_was() {
        for fault in [
            DirFault::PermissionDenied,
            DirFault::NotFound,
            DirFault::Unreadable,
        ] {
            let mut cache = DirCache::default();
            listed(&mut cache, "", vec![entry("locked", true)]);
            cache.accept("/locked", DirOutcome::Failed(fault));
            let view = tree_view(&state("/r", &["/locked"], None), &cache);
            assert_eq!(view.rows[1].kind, RowKind::Notice(RowNotice::Fault(fault)));
            assert_eq!(view.rows[1].name, fault.notice());
        }
        // The three are told apart on the row, not merely in the enum.
        let mut said: Vec<&str> = [
            DirFault::PermissionDenied,
            DirFault::NotFound,
            DirFault::Unreadable,
        ]
        .iter()
        .map(|fault| fault.notice())
        .collect();
        said.sort_unstable();
        said.dedup();
        assert_eq!(said.len(), 3);
    }

    #[test]
    fn a_capped_directory_ends_in_a_row_that_counts_what_is_missing() {
        let mut cache = DirCache::default();
        cache.accept(
            "",
            DirOutcome::Listed(DirListing {
                entries: vec![entry("a", false)],
                omitted: 4321,
                canonical: None,
            }),
        );
        let view = tree_view(&state("/r", &[], None), &cache);
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[1].kind, RowKind::Notice(RowNotice::More(4321)));
        assert_eq!(view.rows[1].name, "4321 more not shown");
    }

    #[test]
    fn a_whole_directory_that_fits_owes_no_tail_row() {
        let mut cache = DirCache::default();
        listed(&mut cache, "", vec![entry("a", false)]);
        let view = tree_view(&state("/r", &[], None), &cache);
        assert_eq!(view.rows.len(), 1);
    }

    #[test]
    fn a_link_back_to_an_ancestor_opens_once_and_stops() {
        let mut cache = DirCache::default();
        cache.accept(
            "",
            DirOutcome::Listed(DirListing {
                entries: vec![entry("link", true)],
                omitted: 0,
                canonical: Some(PathBuf::from("/real")),
            }),
        );
        // The link resolves to the very directory it sits in.
        cache.accept(
            "/link",
            DirOutcome::Listed(DirListing {
                entries: vec![entry("link", true)],
                omitted: 0,
                canonical: Some(PathBuf::from("/real")),
            }),
        );
        let view = tree_view(&state("/r", &["/link", "/link/link"], None), &cache);
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].kind, RowKind::Cycle);
    }

    #[test]
    fn a_link_that_merely_points_elsewhere_still_opens() {
        let mut cache = DirCache::default();
        cache.accept(
            "",
            DirOutcome::Listed(DirListing {
                entries: vec![entry("link", true)],
                omitted: 0,
                canonical: Some(PathBuf::from("/here")),
            }),
        );
        cache.accept(
            "/link",
            DirOutcome::Listed(DirListing {
                entries: vec![entry("inside.txt", false)],
                omitted: 0,
                canonical: Some(PathBuf::from("/somewhere-else")),
            }),
        );
        let view = tree_view(&state("/r", &["/link"], None), &cache);
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[0].kind, RowKind::Directory { open: true });
    }

    #[test]
    fn a_selection_whose_folder_was_read_without_it_is_dead() {
        let mut cache = DirCache::default();
        listed(&mut cache, "", vec![entry("src", true)]);
        listed(&mut cache, "/src", vec![entry("main.rs", false)]);
        assert!(selection_is_dead(
            &state("/r", &[], Some("/src/gone.rs")),
            &cache
        ));
        assert!(!selection_is_dead(
            &state("/r", &[], Some("/src/main.rs")),
            &cache
        ));
    }

    #[test]
    fn a_selection_inside_a_folder_nobody_has_read_is_merely_out_of_sight() {
        let mut cache = DirCache::default();
        listed(&mut cache, "", vec![entry("src", true)]);
        assert!(
            !selection_is_dead(&state("/r", &[], Some("/src/main.rs")), &cache),
            "a collapsed or unread folder proves nothing about what is in it"
        );
        assert!(!selection_is_dead(
            &state("/r", &[], Some("/src/main.rs")),
            &DirCache::default()
        ));
    }

    #[test]
    fn ids_and_paths_convert_both_ways() {
        assert_eq!(child_key("", "src"), "/src");
        assert_eq!(child_key("/src", "main.rs"), "/src/main.rs");
        assert_eq!(
            full_path("/home/me", "/src/main.rs"),
            PathBuf::from("/home/me").join("src").join("main.rs")
        );
        assert_eq!(full_path("/home/me", ""), PathBuf::from("/home/me"));
    }

    #[test]
    fn the_queue_keeps_the_newest_question_per_column_and_folder() {
        let (tx, rx) = mpsc::channel();
        let host = FilesHost::Docked(LeafId {
            tab: crate::TabId(1),
            seat: crate::SeatId(1),
        });
        let ask = |key: &str, path: &str| DirRequest {
            window: winit::window::WindowId::from(1_u64),
            host,
            key: key.to_owned(),
            path: PathBuf::from(path),
        };
        tx.send(ask("/src", "first")).unwrap();
        tx.send(ask("/src", "second")).unwrap();
        tx.send(ask("/docs", "only")).unwrap();
        drop(tx);
        let mut served = Vec::new();
        run_dir_worker(rx, |request| served.push(request));
        assert_eq!(served.len(), 2, "the superseded read is never performed");
        assert_eq!(served[0].path, PathBuf::from("second"));
        assert_eq!(served[1].path, PathBuf::from("only"));
    }

    /// PIN — a question that is superseded *while an earlier read is running* is
    /// never asked.
    ///
    /// This is the case the queue exists for and the one the test above cannot
    /// reach: with everything already in the channel, `push_latest` alone
    /// collapses the duplicates. The supersession that costs real time is the
    /// one that arrives during a slow read — a share that takes a second while
    /// the user folds and unfolds the same directory — and only the re-check
    /// after the pop catches it.
    #[test]
    fn a_question_superseded_during_a_slow_read_is_dropped_rather_than_asked() {
        let (tx, rx) = mpsc::channel();
        let host = FilesHost::Docked(LeafId {
            tab: crate::TabId(1),
            seat: crate::SeatId(1),
        });
        let ask = |key: &str, path: &str| DirRequest {
            window: winit::window::WindowId::from(1_u64),
            host,
            key: key.to_owned(),
            path: PathBuf::from(path),
        };
        tx.send(ask("/slow", "slow")).unwrap();
        tx.send(ask("/src", "first")).unwrap();
        // Held as an `Option` and taken on first use so that it is dropped the
        // moment it has spoken. A live sender kept for the length of the run
        // would mean the worker's own `recv` never ends, which is exactly right
        // in production and a hang in a test.
        let mut late = Some(tx.clone());
        drop(tx);
        let mut served = Vec::new();
        run_dir_worker(rx, |request| {
            // Reading `/slow` takes long enough for the user to fold and unfold
            // `/src`, which arrives as a newer question for a target already
            // queued.
            if request.key == "/slow"
                && let Some(sender) = late.take()
            {
                sender.send(ask("/src", "second")).unwrap();
            }
            served.push(request);
        });
        let paths: Vec<&str> = served
            .iter()
            .map(|request| request.path.to_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            vec!["slow", "second"],
            "the stale `/src` is discarded unread, and only the newest is served"
        );
    }

    #[test]
    fn a_stopped_worker_is_announced_once_and_then_never_again() {
        let mut running = true;
        let mut pending = false;
        assert!(disable_files_worker_state(&mut running, &mut pending));
        assert!(!disable_files_worker_state(&mut running, &mut pending));
        assert_eq!(
            take_files_worker_notice(&mut pending),
            Some(files_worker_stopped_notice())
        );
        assert_eq!(take_files_worker_notice(&mut pending), None);
    }

    #[test]
    fn a_real_directory_reads_sorted_and_without_the_platforms_hidden_names() {
        let dir = std::env::temp_dir().join(format!("bt-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("b.txt"), b"b").unwrap();
        std::fs::write(dir.join("A.txt"), b"a").unwrap();
        std::fs::write(dir.join(".gitignore"), b"x").unwrap();
        let DirOutcome::Listed(listing) = read_directory(&dir) else {
            panic!("a directory that exists lists");
        };
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["sub", ".gitignore", "A.txt", "b.txt"]);
        assert_eq!(listing.omitted, 0);
        assert!(listing.canonical.is_some());
        assert!(listing.entries[0].is_dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// PIN — Q5a. The dot is the author's opinion and is honoured; the attribute
    /// is the platform's and is not.
    ///
    /// This is the whole of the hidden-file ruling and it cannot be tested with
    /// dotfiles alone, because a leading dot sets no attribute on Windows — a
    /// tree that filtered nothing at all would pass a test that only checked
    /// `.gitignore` was shown.
    #[cfg(windows)]
    #[test]
    fn the_platforms_own_hidden_and_system_names_are_the_ones_left_out() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

        let dir = std::env::temp_dir().join(format!("bt-files-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("visible.txt"), b"v").unwrap();
        std::fs::write(dir.join(".dotfile"), b"d").unwrap();
        for (name, attributes) in [
            ("desktop.ini", FILE_ATTRIBUTE_HIDDEN),
            ("pagefile.sys", FILE_ATTRIBUTE_SYSTEM),
        ] {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .attributes(attributes)
                .open(dir.join(name))
                .unwrap();
        }

        let DirOutcome::Listed(listing) = read_directory(&dir) else {
            panic!("a directory that exists lists");
        };
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec![".dotfile", "visible.txt"],
            "the dotfile stays and the two the platform marked are gone"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_that_is_not_there_faults_rather_than_reads_empty() {
        let missing = std::env::temp_dir().join("bt-files-no-such-directory-ever");
        let _ = std::fs::remove_dir_all(&missing);
        assert_eq!(
            read_directory(&missing),
            DirOutcome::Failed(DirFault::NotFound)
        );
    }

    /// The tree used by the keyboard tests: a root of two folders and a file,
    /// with the first folder open over one file of its own.
    ///
    /// ```text
    /// /docs        (open)
    ///   /docs/a.md
    /// /src         (shut)
    /// /read.me
    /// ```
    fn keyboard_rows() -> Vec<TreeRow> {
        let mut cache = DirCache::default();
        listed(
            &mut cache,
            "",
            vec![
                entry("docs", true),
                entry("src", true),
                entry("read.me", false),
            ],
        );
        listed(&mut cache, "/docs", vec![entry("a.md", false)]);
        tree_view(&state("/r", &["/docs"], None), &cache).rows
    }

    fn press(sel: Option<&str>, command: TreeCommand) -> (FilesLeafState, TreeAction) {
        let mut column = state("/r", &["/docs"], sel);
        let action = apply_tree_command(&mut column, &keyboard_rows(), command);
        (column, action)
    }

    #[test]
    fn up_and_down_walk_the_visible_rows_one_at_a_time() {
        let rows = keyboard_rows();
        assert_eq!(
            rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
            vec!["/docs", "/docs/a.md", "/src", "/read.me"]
        );
        let (column, action) = press(Some("/docs"), TreeCommand::Down);
        assert_eq!(action, TreeAction::Select("/docs/a.md".to_owned()));
        assert_eq!(column.sel.as_deref(), Some("/docs/a.md"));
        let (_, action) = press(Some("/src"), TreeCommand::Up);
        assert_eq!(action, TreeAction::Select("/docs/a.md".to_owned()));
    }

    /// PIN — C33. The row's triangle turns over its own 120ms, and reduced
    /// motion leaves it standing at the end rather than part-way.
    ///
    /// The red gate this fills: nothing anywhere referenced `turn_row` or
    /// `row_turn`, so the disclosure triangle's whole animation — the one
    /// constant the mock-up spells out for it and the degradation the accessible
    /// preference owes — was carried entirely by a shared tween that only the
    /// *chevron* had a test for. A triangle wired to the wrong span, or one
    /// whose reduced branch left it frozen half-turned, would have been drawn
    /// wrong with every test in the workspace still green.
    #[test]
    fn a_row_triangle_turns_over_its_own_120ms_and_snaps_under_reduced_motion() {
        let now = Instant::now();
        let turn = Duration::from_millis(crate::seats::FILES_ROW_TRI_TURN_MS);
        assert_eq!(turn, Duration::from_millis(120), "the mock-up's own number");

        let mut cache = DirCache::default();
        cache.turn_row("/docs", true, now, crate::Motion::Full);
        assert_eq!(
            cache.row_turn("/docs", true, now, crate::Motion::Full),
            (0.0, true),
            "it starts shut and owes a frame"
        );
        let (part, moving) = cache.row_turn("/docs", true, now + turn / 2, crate::Motion::Full);
        assert!(
            part > 0.0 && part < 1.0 && moving,
            "half way through it is half way over, not at one end"
        );
        // And it is on the mock-up's curve rather than CSS `ease`. A quarter of
        // the way through the span, `cubic-bezier(.2,0,0,1)` is already past
        // halfway (≈.61) where `ease` is not (≈.42) — the two are only told
        // apart early, which is exactly where a 120ms turn is watched.
        let (quarter, _) = cache.row_turn("/docs", true, now + turn / 4, crate::Motion::Full);
        assert!(
            quarter > 0.55,
            "the triangle leads with its travel: {quarter} should be past half by a quarter of the way"
        );
        assert_eq!(
            cache.row_turn("/docs", true, now + turn, crate::Motion::Full),
            (1.0, false),
            "and 120ms is when it arrives"
        );

        let mut cache = DirCache::default();
        cache.turn_row("/docs", true, now, crate::Motion::Reduced);
        assert_eq!(
            cache.row_turn("/docs", true, now, crate::Motion::Reduced),
            (1.0, false),
            "with the preference set it is simply turned, on the first frame"
        );
        assert!(!cache.any_turning(now, crate::Motion::Reduced));
    }

    /// PIN — K143's keyboard half. The Menu key and `Shift+F10` are the same
    /// request, and they are the *only* chord the tree takes.
    ///
    /// The red gate: without this the file row's menu is reachable only by right
    /// button, which makes `Insert path into terminal` — a verb moved here in
    /// 2026-07-17 expressly to be discoverable and keyboard-reachable —
    /// unreachable without a mouse.
    #[test]
    fn the_menu_key_and_shift_f10_are_the_two_names_of_one_request() {
        use winit::keyboard::{Key, ModifiersState, NamedKey};
        assert_eq!(
            tree_command(&Key::Named(NamedKey::ContextMenu), ModifiersState::empty()),
            Some(TreeCommand::ContextMenu)
        );
        assert_eq!(
            tree_command(&Key::Named(NamedKey::F10), ModifiersState::SHIFT),
            Some(TreeCommand::ContextMenu)
        );
        assert_eq!(
            tree_command(&Key::Named(NamedKey::F10), ModifiersState::empty()),
            None,
            "a bare F10 is not this window's key"
        );
        assert_eq!(
            tree_command(&Key::Named(NamedKey::F10), ModifiersState::CONTROL),
            None,
            "and neither is any other chord on it"
        );
        assert_eq!(
            tree_command(
                &Key::Named(NamedKey::ContextMenu),
                ModifiersState::SHIFT | ModifiersState::CONTROL
            ),
            None,
            "the bare navigation set stays bare"
        );
    }

    /// PIN — **both kinds of node answer the menu key** (user ruling
    /// 2026-08-25), and the two that lead nowhere still do not.
    ///
    /// K143's "目录行不弹" is overturned here for the reason the ruling gives:
    /// the menu is no longer three verbs that are all about a file. A folder has
    /// a path, so `Copy path`, `Insert path into terminal` and
    /// `Reveal in Explorer` are answerable over one, and it has two verbs of its
    /// own — the fold, and a shell standing in it.
    ///
    /// RED GATE: put `RowKind::Directory` back on the silent arm of
    /// `apply_tree_command`'s `ContextMenu` match and the two folder lines fail.
    #[test]
    fn both_kinds_of_node_answer_the_menu_key() {
        let (column, action) = press(Some("/read.me"), TreeCommand::ContextMenu);
        assert_eq!(action, TreeAction::ContextMenu("/read.me".to_owned()));
        assert_eq!(
            column.sel.as_deref(),
            Some("/read.me"),
            "asking a row about itself does not move the selection"
        );
        let (column, action) = press(Some("/docs"), TreeCommand::ContextMenu);
        assert_eq!(
            action,
            TreeAction::ContextMenu("/docs".to_owned()),
            "an open folder has a menu of its own"
        );
        assert_eq!(column.sel.as_deref(), Some("/docs"));
        assert_eq!(
            press(Some("/src"), TreeCommand::ContextMenu).1,
            TreeAction::ContextMenu("/src".to_owned()),
            "and so has a shut one"
        );
        assert_eq!(
            press(None, TreeCommand::ContextMenu).1,
            TreeAction::None,
            "and a column standing nowhere does not pick a row to be asked about"
        );
    }

    /// PIN — D45. Travel stops at the ends rather than coming out the other one.
    #[test]
    fn travel_stops_at_both_ends_instead_of_wrapping_round() {
        let (column, _) = press(Some("/docs"), TreeCommand::Up);
        assert_eq!(column.sel.as_deref(), Some("/docs"));
        let (column, _) = press(Some("/read.me"), TreeCommand::Down);
        assert_eq!(column.sel.as_deref(), Some("/read.me"));
    }

    #[test]
    fn home_and_end_go_to_the_first_and_last_row_there_is() {
        let (column, _) = press(Some("/src"), TreeCommand::Home);
        assert_eq!(column.sel.as_deref(), Some("/docs"));
        let (column, _) = press(Some("/src"), TreeCommand::End);
        assert_eq!(column.sel.as_deref(), Some("/read.me"));
    }

    #[test]
    fn right_unfolds_a_shut_folder_and_otherwise_steps_in() {
        let (column, action) = press(Some("/src"), TreeCommand::Right);
        assert_eq!(action, TreeAction::Opened("/src".to_owned()));
        assert!(column.open.contains("/src"));
        // Already open: the key means "in", and in is the next row.
        let (column, action) = press(Some("/docs"), TreeCommand::Right);
        assert_eq!(action, TreeAction::Select("/docs/a.md".to_owned()));
        assert!(column.open.contains("/docs"), "stepping in folds nothing");
        // A file has nothing to open, so the key still travels.
        let (column, _) = press(Some("/docs/a.md"), TreeCommand::Right);
        assert_eq!(column.sel.as_deref(), Some("/src"));
    }

    #[test]
    fn left_folds_an_open_folder_and_otherwise_steps_out_to_the_parent() {
        let (column, action) = press(Some("/docs"), TreeCommand::Left);
        assert_eq!(action, TreeAction::Closed("/docs".to_owned()));
        assert!(!column.open.contains("/docs"));
        let (column, action) = press(Some("/docs/a.md"), TreeCommand::Left);
        assert_eq!(action, TreeAction::Select("/docs".to_owned()));
        assert!(column.open.contains("/docs"), "stepping out folds nothing");
    }

    /// PIN — a top-level row's parent is the root, and the root is the head
    /// rather than a row. Stepping out of it must stand still, not travel to
    /// whatever happens to be above.
    #[test]
    fn stepping_out_of_a_top_level_row_has_nowhere_to_go_and_stays() {
        let (column, action) = press(Some("/src"), TreeCommand::Left);
        assert_eq!(action, TreeAction::None);
        assert_eq!(column.sel.as_deref(), Some("/src"));
        assert_eq!(parent_key("/src"), None);
        assert_eq!(parent_key("/docs/a.md"), Some("/docs".to_owned()));
    }

    #[test]
    fn enter_folds_a_folder_and_opens_a_file() {
        let (column, action) = press(Some("/docs"), TreeCommand::Activate);
        assert_eq!(action, TreeAction::Closed("/docs".to_owned()));
        assert!(!column.open.contains("/docs"));
        let (_, action) = press(Some("/src"), TreeCommand::Activate);
        assert_eq!(action, TreeAction::Opened("/src".to_owned()));
        let (_, action) = press(Some("/read.me"), TreeCommand::Activate);
        assert_eq!(action, TreeAction::Activate("/read.me".to_owned()));
    }

    /// PIN — a column nobody has selected in answers its first travelling key
    /// with its *first* row.
    ///
    /// The mock-up's `if (i < 0) i = 0` followed by `select(i + 1)` lands on the
    /// second row instead, which shows as the first press of ↓ doing nothing you
    /// can see and the second one moving two.
    #[test]
    fn the_first_travelling_key_into_an_unselected_column_lands_on_its_first_row() {
        let (column, action) = press(None, TreeCommand::Down);
        assert_eq!(action, TreeAction::Select("/docs".to_owned()));
        assert_eq!(column.sel.as_deref(), Some("/docs"));
        let (column, _) = press(None, TreeCommand::Up);
        assert_eq!(column.sel.as_deref(), Some("/docs"));
        let (column, _) = press(None, TreeCommand::End);
        assert_eq!(column.sel.as_deref(), Some("/read.me"));
        // Nothing is selected, so there is nothing to open.
        let (column, action) = press(None, TreeCommand::Activate);
        assert_eq!(action, TreeAction::None);
        assert_eq!(column.sel, None);
    }

    /// PIN — a sentence about the list is not a row you can stand on.
    #[test]
    fn travel_steps_over_the_rows_that_are_only_sentences() {
        let mut cache = DirCache::default();
        listed(
            &mut cache,
            "",
            vec![entry("locked", true), entry("after.txt", false)],
        );
        cache.accept("/locked", DirOutcome::Failed(DirFault::PermissionDenied));
        let rows = tree_view(&state("/r", &["/locked"], None), &cache).rows;
        assert_eq!(rows.len(), 3, "the refusal has a row of its own");
        assert!(!rows[1].is_node());

        let mut column = state("/r", &["/locked"], Some("/locked"));
        assert_eq!(
            apply_tree_command(&mut column, &rows, TreeCommand::Down),
            TreeAction::Select("/after.txt".to_owned()),
            "the refusal is read, not selected"
        );
    }

    #[test]
    fn a_folder_that_eats_itself_refuses_the_keyboard_as_it_refuses_the_mouse() {
        let mut cache = DirCache::default();
        cache.accept(
            "",
            DirOutcome::Listed(DirListing {
                entries: vec![entry("link", true)],
                omitted: 0,
                canonical: Some(PathBuf::from("/real")),
            }),
        );
        cache.accept(
            "/link",
            DirOutcome::Listed(DirListing {
                entries: vec![entry("link", true)],
                omitted: 0,
                canonical: Some(PathBuf::from("/real")),
            }),
        );
        let rows = tree_view(&state("/r", &["/link", "/link/link"], None), &cache).rows;
        assert_eq!(rows[0].kind, RowKind::Cycle);
        let mut column = state("/r", &[], Some("/link"));
        assert_eq!(
            apply_tree_command(&mut column, &rows, TreeCommand::Activate),
            TreeAction::None
        );
        assert!(column.open.is_empty());
    }

    #[test]
    fn escape_is_the_one_key_that_gives_the_keyboard_back() {
        let mut column = state("/r", &[], Some("/src"));
        assert_eq!(
            apply_tree_command(&mut column, &[], TreeCommand::Release),
            TreeAction::Release,
            "even an empty column can hand the keyboard back"
        );
        assert_eq!(
            apply_tree_command(&mut column, &[], TreeCommand::Down),
            TreeAction::None
        );
    }

    /// PIN — the tree claims bare keys and never a chord.
    ///
    /// A modifier means the window's own vocabulary, and the shortcut table has
    /// already been asked by the time a key reaches the tree. Claiming
    /// `Ctrl+Home` here would take a terminal's jump-to-top away from it for as
    /// long as a column had the keyboard.
    #[test]
    fn only_unmodified_keys_are_the_trees_and_the_rest_are_nobodys_here() {
        use winit::keyboard::{Key, ModifiersState, NamedKey};
        let none = ModifiersState::empty();
        assert_eq!(
            tree_command(&Key::Named(NamedKey::ArrowDown), none),
            Some(TreeCommand::Down)
        );
        assert_eq!(
            tree_command(&Key::Named(NamedKey::Space), none),
            Some(TreeCommand::Activate)
        );
        assert_eq!(
            tree_command(&Key::Named(NamedKey::Enter), none),
            Some(TreeCommand::Activate)
        );
        assert_eq!(
            tree_command(&Key::Named(NamedKey::Escape), none),
            Some(TreeCommand::Release)
        );
        for modifiers in [
            ModifiersState::CONTROL,
            ModifiersState::SHIFT,
            ModifiersState::ALT,
        ] {
            assert_eq!(
                tree_command(&Key::Named(NamedKey::Home), modifiers),
                None,
                "a chord is the window's, not the tree's"
            );
        }
        assert_eq!(tree_command(&Key::Character("a".into()), none), None);
    }

    #[test]
    fn an_unrooted_column_is_not_a_missing_folder() {
        assert!(!root_is_addressable(""));
        assert!(!root_is_addressable("   "));
        assert!(root_is_addressable("C:\\Users\\me"));
        assert!(root_is_addressable("/home/me"));
    }
}
