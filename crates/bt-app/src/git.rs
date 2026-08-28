//! What a repository says about itself: the thread that asks `git`, the words
//! it answers in, and the pure rules that turn those words into rows.
//!
//! **Why a subprocess and not a library.** Adjudicated 2026-08-13
//! (`git-backend-adjudication.md`): the panel this feeds will sit three inches
//! from a pane where the user types `git status` themselves, so any disagreement
//! between our answer and git's own — over `.gitignore` corner cases,
//! `safe.directory`, `includeIf`, worktrees, submodules, sparse-checkout, LFS,
//! `core.fsmonitor` — is not a rare defect but two answers on one screen. A
//! library has to earn that agreement forever; the CLI *is* the agreement. The
//! costs are known and paid: this machine must have a `git.exe` (it already must
//! — the Git Bash profile and `bt-term`'s shell-integration tests both assume
//! one), the output is text we parse, and every question is a process. The
//! revival conditions for the two rejected backends are written in §7 of that
//! document; the one worth repeating here is the trigger, not the verdict — if a
//! single `git log` on a real repository ever measures over 200ms, measure
//! before changing anything.
//!
//! **Why a thread.** The same reason [`crate::files`] has one, one step further
//! along: a `git status` on a fifty-thousand-file repository without fsmonitor
//! takes about three seconds, and a frame that waits for it is a frame that is
//! not drawn. So this module owns the shape `bt-files-worker` already owns — a
//! named thread, a request channel, a response channel, an [`crate::AppEvent`]
//! to wake the loop, newest-per-target coalescing, and a one-way degradation
//! when the thread is gone.
//!
//! **What this worker will not do (R31).** It answers questions and asks none of
//! its own. There is no timer, no poll, no watcher: a repository is read when a
//! Git page is looked at and when something the user did could have changed it.
//! A terminal that reads the repository next to it sixty times a minute forever
//! is a terminal that heats a laptop for a panel nobody has open.
//!
//! **What is on which side.** Parsing, grouping, capping and the relative-time
//! table happen on the worker, because they are work proportional to the
//! repository and the whole point of asking off-thread is that the answer
//! arrives in the shape the panel wants. What stays on the event loop is filing
//! the answer into the seat that asked.
//!
//! **Half of this module has no caller yet, and says so.** This slice is the
//! data plane; the page that draws it is G-2 and the diff wiring is G-3. The
//! items only that page will call carry `#[allow(dead_code)]` rather than being
//! left out, for the reason `PreviewSource`'s two git cases carry one: a model
//! built one question at a time as the panel needs it is a model shaped by the
//! panel's drawing order, and the shape that matters here is git's. Each such
//! item names the slice that will call it, so an `allow` that outlives its excuse
//! is visible rather than merely tolerated.
//!
//! **The locale is forced to `C`.** Every child gets `LC_ALL=C`, because two of
//! git's machine-readable outputs are not purely machine-readable: `git
//! for-each-ref`'s `%(upstream:track)` says `[ahead 2, behind 1]` in words, and
//! `rev-parse`'s refusal says *why* in a sentence — and both of those words are
//! translated on a localised Git. Parsing a translated string is parsing a
//! string that changes under us; forcing the locale is what makes "not a
//! repository" a fact rather than a guess. It is the same move VS Code's git
//! extension makes for the same reason.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

use crate::{AppEvent, LeafId};

/// How many changed files one status will show.
///
/// [`crate::files::DIR_ENTRY_CAP`] by name and not by value, because it is the
/// same law about the same pane: a column 240 logical pixels wide showing
/// 24-pixel rows can put perhaps two dozen names in front of you, and a
/// generated-file avalanche of a hundred thousand is not browsable by scrolling
/// however patient the scroller is. The surrender has the same shape too (R33) —
/// read the whole answer, keep the first `N` in git's own order, and *say so*
/// rather than pretending the rest are not there.
pub const GIT_STATUS_CAP: usize = crate::files::DIR_ENTRY_CAP;

/// How many commits one page of history is (R16).
pub const GIT_LOG_PAGE: usize = 50;

/// How many commits one search will name (T4, v2 ③).
///
/// **A cap and not a promise of completeness**, on [`GIT_STATUS_CAP`]'s own
/// reasoning read into a different list: a one-letter query against a hundred
/// thousand commits matches most of them, and a reader stepping through matches
/// with `Enter` will never reach the thousandth. What the cap costs is a count
/// that says `1000` when the truth is more; what it buys is that the field
/// stays answerable in a kernel tree.
pub const GIT_SEARCH_CAP: usize = 1000;

/// How long one `git` invocation may take before it is killed.
///
/// Not a performance budget — a deadlock guard. A legitimate `status` on a cold
/// fifty-thousand-file repository can take seconds and must not be cut off, so
/// this sits an order of magnitude above anything a local read should cost. What
/// it defends against is the other kind of slow: a `git` waiting on a lock a
/// crashed process left behind, or on a network filesystem that has stopped
/// answering. One of those must cost one answer, not the worker thread and every
/// question queued behind it forever.
pub const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// How often a running child is asked whether it has finished.
///
/// Polling rather than waiting on a handle because waiting on a handle is
/// `unsafe` FFI and this workspace denies `unsafe_code`. The cost is bounded and
/// small: Windows' sleep granularity is around 15ms, so a `git status` that
/// takes 40ms is looked in on three times, and the latency this adds to an
/// answer is at most one of those ticks — against a subprocess that cost tens of
/// milliseconds to start.
const GIT_POLL_INTERVAL: Duration = Duration::from_millis(2);

/// The notice shown once when the git worker has stopped.
///
/// Worded like [`crate::files::files_worker_stopped_notice()`] and for the same
/// reason: a worker dying is a feature going away, not a session ending, and the
/// sentence has to say which half still works.
#[must_use]
pub fn git_worker_stopped_notice() -> &'static str {
    crate::i18n::Text::GitWorkerStopped.text()
}

/// What [`GitFault::GitMissing`] says when there is no `git.exe` at all.
///
/// It says what to do about it, not only what is wrong (user ruling,
/// 2026-08-16): the Git page is on by default so that it can be *found*, and a
/// page found on a machine without git must turn the discovery into a next
/// step rather than a dead end. Kept in one sentence because it stands where
/// the rows would, in the muted ink, and a paragraph there would read as an
/// error page.
#[must_use]
pub fn git_not_found() -> &'static str {
    crate::i18n::Text::GitNotFound.text()
}

// ── The questions ──────────────────────────────────────────────────────────

/// "Ask the repository this, for this Files column."
///
/// **The host is a docked seat and nothing else.** A floating tree gets no Git
/// page (R2): the float is a peek at a folder, not a seat in a tab, and the
/// panel's whole vocabulary — a page beside a Files page, a branch head, a list
/// you stage from — is seat-shaped. So there is no `Float` variant to address
/// here, and the epoch dance [`crate::files::FilesHost`] needs does not arise.
///
/// **And the window the host is in** (user report, 2026-08-23). Every spelling of
/// [`GitHost`] is minted by a counter that starts again in every window — a
/// `TabId` on its own, one inside a [`crate::LeafId`], a float's epoch — so none
/// of them names one surface across the process. The pair does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRequest {
    pub window: WindowId,
    pub host: GitHost,
    pub question: GitQuestion,
}

/// **Who is asking**, and therefore where the answer is filed.
///
/// Two surfaces read repositories and they are addressed differently, which is
/// the whole reason this is an enum rather than a seat id. A Git *page* belongs
/// to a Files column and dies with it. A *graph* belongs to a tab and to a
/// repository — it is a document on the preview seat, it outlives the column it
/// was opened from, and two columns rooted in one repository open the same one.
/// Addressing it by the seat that happened to launch it would make the document
/// go blank the moment that column was closed or re-rooted, which is a page
/// forgetting its own subject because something else moved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitHost {
    /// The Git page in one docked Files column.
    Column(LeafId),
    /// The commit graph document of one repository, in one tab.
    Graph { tab: crate::TabId, root: PathBuf },
    /// **The Git page in one floating window**, by the epoch that minted its
    /// view (user ruling, 2026-08-19).
    ///
    /// R2 used to say this variant could not exist — "a floating tree gets no
    /// Git page: the float is a peek at a folder, not a seat in a tab, and the
    /// panel's whole vocabulary is seat-shaped". The report that overturned it
    /// is the plainest kind: a column *standing on* its Git page was popped out
    /// and the window silently showed the file tree instead. A pop-out is a
    /// move, and a move that changes what you were looking at is not one.
    ///
    /// The **page** is addressed by epoch, which is the dance
    /// [`crate::files::FilesHost::Float`] already does for the same window's
    /// directory reads and for the same reason: a pinned window floats across
    /// tabs by ruling (§7.1.2), so no tab can be asked whether it is still
    /// there. `live_mut` finds the window that asked, and two windows on one
    /// folder never fill in with each other's answers.
    ///
    /// `tab` is here for the *other* kind of answer this host can receive. Two
    /// of git's six questions come back as **documents**, and a document goes
    /// into a pool, which is a tab's (§7.1.3) — so the window records which tab
    /// was in front when it opened one, because that is the pool the reader is
    /// looking at it in. It is unused by the five answers that make the page.
    Float { id: u64, tab: crate::TabId },
}

impl GitHost {
    /// Which tab the answer belongs in.
    #[must_use]
    pub fn tab(&self) -> crate::TabId {
        match self {
            Self::Column(leaf) => leaf.tab,
            Self::Graph { tab, .. } | Self::Float { tab, .. } => *tab,
        }
    }
}

/// The six things this product ever asks a repository.
///
/// Each is one process and one answer — see §8 of the backend adjudication,
/// which is where these command lines come from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitQuestion {
    /// "Is there a repository at or above this folder, and where does it start?"
    ///
    /// This is the whole of R3's search for an ancestor: `rev-parse
    /// --show-toplevel` already climbs, so climbing is not something we
    /// implement, it is something we stop implementing.
    RepoProbe { dir: PathBuf },
    /// "What has changed?" — the changed-file list and the branch head in one.
    Status { root: PathBuf },
    /// "What names are there — here, over there, and nailed on?" (T6, v2 ③).
    ///
    /// One `for-each-ref` over all three trees, replacing the `Branches`
    /// question that walked `refs/heads` alone. The panel still draws the locals
    /// and nothing else; what the other two are for is the graph's pills and its
    /// filter menu, and asking for them separately would be two more subprocesses
    /// for a file git reads once.
    Refs { root: PathBuf },
    /// "One page of history, please" (R16).
    Log {
        root: PathBuf,
        /// Which roads to walk — git's own rev arguments, in the position it
        /// puts them (T2/T3, v2 ③).
        ///
        /// **Empty is not `--all`, it is `HEAD`**, which is what `git log` with
        /// no revision means and what this question meant for its whole first
        /// life. The graph's "All branches" now passes `--all` (or the narrower
        /// spellings the filter's two checkboxes ask for) *explicitly*, so what
        /// is walked is always something a caller said rather than a default two
        /// readers could disagree about.
        ///
        /// Not part of the question's identity ([`Self::same_target`]): a page
        /// of history is identified by which page it is, and a filter changed
        /// under a page in flight is exactly the case where newest must win.
        refs: Vec<String>,
        skip: usize,
        count: usize,
    },
    /// "Which commits match this?" (T4, v2 ③) — the graph's search field.
    ///
    /// **The one question in this module that is more than one process, and the
    /// reason is that git has no OR.** `git log --grep=Q --author=Q` *ands* the
    /// two: it answers commits whose message matches **and** whose author
    /// matches, which is essentially none of them, and `--all-match` only makes
    /// that stricter. What a search field means by "Q" is "anything about this
    /// commit says Q" — so the two are asked separately and unioned here, and a
    /// third process asks git to resolve `Q` as a revision outright so that
    /// pasting a hash lands on the commit rather than searching for its text.
    ///
    /// See [`answer`]'s arm for the three command lines.
    Search { root: PathBuf, query: String },
    /// "How does this file differ?" — `against` carries the `--cached`
    /// mapping (R25). Asked when a changed-file row opens a diff.
    Diff {
        root: PathBuf,
        path: String,
        against: crate::preview::GitDiffAgainst,
        /// **Where a rename came from**, when this file is one.
        ///
        /// Not decoration: `git diff --cached -- <new path>` on a staged rename
        /// prints *a new file* — rename detection needs both halves of the pair
        /// and a pathspec naming one of them hides the other. So the row that
        /// wears an `R` badge would open a diff claiming the file had just been
        /// created, which is the panel and the diff disagreeing on one screen —
        /// the one thing §7 of the backend adjudication chose the CLI to
        /// prevent. Handing git both paths is what makes it answer with the
        /// rename it already knows about.
        ///
        /// It is **not** part of the question's identity ([`Self::same_target`]):
        /// the same file at the same stage is the same question however it got
        /// its name.
        renamed_from: Option<String>,
    },
    /// "How did this commit change this file?" (R15).
    Show {
        root: PathBuf,
        hash: String,
        path: String,
        /// The same pair, for the same reason — `git show <hash> -- <new path>`
        /// loses a rename exactly as `git diff` does.
        renamed_from: Option<String>,
    },
    /// "How does this one file differ between these two places?" (D6, v2 ②).
    ///
    /// [`Self::CompareFiles`]'s document to [`Self::Show`]'s: the compare block
    /// lists the files, and pressing one asks this. `b` absent is the working
    /// tree, in git's own grammar, exactly as it is there.
    DiffRange {
        root: PathBuf,
        a: String,
        b: Option<String>,
        path: String,
        /// The same pair as everywhere else — `git diff a b -- <new path>` on a
        /// rename prints a brand-new file for the reason [`Self::Diff`] spells
        /// out at length.
        renamed_from: Option<String>,
    },
    /// "Which files did this commit touch?" — what R15's accordion lists.
    ///
    /// Separate from [`Self::Show`] rather than the same question with an empty
    /// path, because the two answers are different kinds of thing: this one is
    /// *rows on the page* and is filed into the column's cache, while a `Show`
    /// is a document and goes to the preview pool. One question, one answer, one
    /// home.
    CommitFiles { root: PathBuf, hash: String },
    /// "What is different between these two places?" (D6, v2 ②).
    ///
    /// [`Self::CommitFiles`]'s twin and filed the same way — rows, not a
    /// document — because a comparison is a *list of files* on the page and each
    /// of those files then opens a document of its own.
    ///
    /// **`b` is optional and its absence is the working tree**, which is git's
    /// own grammar (`git diff <commit>` with no second revision) rather than an
    /// invention: the alternative spelling, `<a> HEAD`, is a different question
    /// — it would compare two commits and say nothing about what is on disk.
    CompareFiles {
        root: PathBuf,
        /// The **older** end, in the graph's own order.
        a: String,
        /// The newer end, or the working tree when absent.
        b: Option<String>,
    },
    /// **The one question that changes something** (R14).
    ///
    /// Four verbs and one shape, because what the panel does with the answer is
    /// the same in all four cases: unpend the rows, and ask the repository
    /// everything again. There is no attempt to predict the new status from the
    /// verb — a `git add` of a file that changed again between the click and the
    /// process is a different status than arithmetic would give, and git is the
    /// only thing that knows which.
    Write {
        root: PathBuf,
        verb: GitWriteVerb,
        /// Repo-relative, in git's grammar. **Plural**, because a group heading's
        /// "stage all" is one process over a list and not a list of processes:
        /// fifty children racing each other for `index.lock` is fifty ways to
        /// half-succeed.
        paths: Vec<String>,
    },
    /// **Stand somewhere else** (R10) — `git checkout`.
    ///
    /// Its own question rather than a fifth [`GitWriteVerb`], because the four
    /// verbs there take a *pathspec* and this one takes a *ref*, and a field
    /// called `paths` holding a branch name is a lie the type system would help
    /// tell. What it shares with them is the shape of the answer — a receipt
    /// with nothing in it, or git's own sentence about why not.
    ///
    /// **No stash and no force** (R10, ruled 2026-08-15). A checkout onto a
    /// dirty tree that would lose work is refused *by git*, in git's words, and
    /// that refusal is the whole of the failure design: the panel prints the
    /// sentence and the branch does not change. Offering to stash or to force
    /// from a one-click row would be this panel deciding what to do with work it
    /// did not write — the heavy verbs belong to the terminal beside it (G12).
    Checkout {
        root: PathBuf,
        /// A branch name, or the commit to stand on when `detach` is set.
        target: String,
        /// Whether this is a detached checkout of a commit (R23's double click)
        /// rather than a move onto a branch.
        ///
        /// Explicit rather than sniffed from the string: `git checkout <thing>`
        /// resolves a name that is *both* a branch and a hash prefix as the
        /// branch, so a double click on a commit whose abbreviation happens to
        /// spell a branch name would silently move the branch instead of
        /// detaching. `--detach` says which of the two was meant.
        detach: bool,
    },
}

/// The things this product will do to a repository (R14 / G12 / M10).
///
/// **Every one is a light verb**, which is the whole of the boundary the mock-up's
/// own wiring comment draws (line 7928): the panel sees, toggles and throws away;
/// `commit`, `merge`, `rebase`, `push` and `pull` belong to the terminal standing
/// beside it. Every one of these is reversible from git's own reflog or index
/// except the two discards, which is exactly why those two are the ones behind a
/// confirmation gate.
///
/// # The six that arrived with the context menus (v2 ④, 2026-08-16)
///
/// They are **named** verbs where the first four are *pathspec* verbs, and that
/// is the only difference in kind: a `git branch -d` takes a ref and a `git add`
/// takes a list of files. The user ruling that opened this slice draws the line
/// they all sit inside — *read/navigate verbs and one-command-undoable local
/// writes only* — so a branch created here is undone by deleting it, a branch
/// deleted here is `-d` and therefore merged (its commits are still on the branch
/// that holds them), a rename is renamed back, and a tag is one command in each
/// direction. Nothing on this list rewrites history, moves `HEAD` over work, or
/// talks to another machine. [`GIT_NEVER_WORDS`] pins that as a test rather than
/// as a promise in a comment.
///
/// **Not `Copy` since v2 ④**, because half of these carry the name they are
/// about. The alternative — a `paths: Vec<String>` holding a branch name — is the
/// lie [`GitQuestion::Checkout`]'s own note refuses to tell, so a verb that is
/// about a *ref* carries the ref.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitWriteVerb {
    /// `git add` — works for a modification, a deletion and an untracked file
    /// alike, which is why staging needs no per-entry branch.
    Stage,
    /// `git restore --staged`.
    Unstage,
    /// `git restore --worktree` — put a tracked file back the way the index has
    /// it.
    Discard,
    /// `git clean -f` — delete an untracked file.
    ///
    /// A separate verb rather than a flag because it is a genuinely different
    /// act: `restore` rewrites a file that git is tracking and can always
    /// reconstruct, while this one *removes* a file git has never seen a copy of.
    /// The word "discard" is right for both — a user who made a file by mistake
    /// means the same thing by it — but the command, and the size of the promise
    /// broken if it is wrong, are not the same.
    DiscardUntracked,
    /// `git branch <name> <at>` — a new name on a commit that is already there.
    ///
    /// It moves nothing: the working tree, `HEAD` and every other branch are
    /// exactly where they were, and the whole of its effect is one more line in
    /// `refs/heads`. Undone by `Delete branch`, which is on the same menu.
    CreateBranch { name: String, at: String },
    /// `git tag <name> <at>` — **lightweight**, and that is the ruling rather
    /// than a shortcut.
    ///
    /// An annotated tag needs a message, and a message needs an editor or a
    /// second field; a signature needs a key. Both are the heavy end of tagging
    /// and belong to the terminal beside this pane (G12). What a context menu can
    /// honestly offer is the one-command, one-command-undoable half.
    CreateTag { name: String, at: String },
    /// `git branch -m <from> <to>`.
    ///
    /// **Allowed on the branch you are standing on**, which is git's own
    /// position: `-m` on the current branch renames it and leaves `HEAD` pointing
    /// at the new name, so nothing is detached and nothing is lost. Refusing it
    /// would be this window being more frightened than git is.
    RenameBranch { from: String, to: String },
    /// `git branch -d <name>` — **merged only, and never `-D`**.
    ///
    /// The lower-case `-d` is the whole of why this verb is inside the boundary:
    /// git refuses it for a branch whose commits are nowhere else, and that
    /// refusal comes back in git's own words on a card. `-D` is the button that
    /// says "yes, throw those commits away", and there is no such button here —
    /// the reader who means it has a terminal.
    DeleteBranch { name: String },
    /// `git tag -d <name>`.
    DeleteTag { name: String },
    /// `git checkout -b <local> --track <name>` — start a local branch from
    /// somebody else's (M10).
    ///
    /// `name` is the **remote-tracking ref as git spells it** (`origin/main`),
    /// because that is what the pill says and what `--track` is handed; the local
    /// name is [`tracking_local_name`]'s reading of it, which is the same reading
    /// git's own DWIM does. It is a write rather than a [`GitQuestion::Checkout`]
    /// because it *creates* something — the checkout is the second half of what
    /// the one command does.
    ///
    /// **When a local of that name already exists this verb is never issued**:
    /// the menu checks first and issues an ordinary checkout instead, because
    /// `-b` on a name that is taken is a refusal and what the reader meant by
    /// pressing the row is "put me on that branch".
    CheckoutTracking { name: String },
}

/// **The words no command this window builds may contain** (user ruling, v2 ④).
///
/// The boundary is a list of verbs and not a feeling: merge, rebase, reset,
/// cherry-pick, revert, push, pull and fetch rewrite history, move work, or talk
/// to another machine, and `-D` and `--force` are the two flags that turn a
/// refusal into a loss. Pinned by a test that enumerates every
/// [`GitWriteVerb`] and reads its argument vector, so the day somebody adds a
/// seventh verb the list is checked by the build rather than by a reviewer.
///
/// Read by that test and by nothing else, which is exactly what a boundary
/// written as data looks like: the commands themselves come from
/// [`write_arguments`], and this is the list they are held against.
#[allow(dead_code)]
pub const GIT_NEVER_WORDS: [&str; 10] = [
    "merge",
    "rebase",
    "reset",
    "cherry-pick",
    "revert",
    "push",
    "pull",
    "fetch",
    "-D",
    "--force",
];

/// The local branch a remote-tracking ref becomes (`origin/main` → `main`).
///
/// **git's own DWIM, written down.** `git checkout --track origin/main` creates
/// `main`, and it does so by dropping the remote's name from the front — so this
/// is not a convention this window invented, it is the one it has to agree with
/// in order to ask "does that local already exist?" before issuing the command.
/// A ref with no `/` in it is handed back whole rather than emptied, which is the
/// only honest answer for a name that is not a remote's.
#[must_use]
pub fn tracking_local_name(remote: &str) -> &str {
    remote.split_once('/').map_or(remote, |(_, rest)| rest)
}

/// **Why a name is not a name** (v2 ④) — one class per sentence the prompt says.
///
/// A closed set rather than a `String`, because the hint under the field is a
/// fixed line of copy and a fault that could carry any text at all is a fault
/// nobody can pin. The classes are `git check-ref-format --branch`'s own rules,
/// grouped by what a person can *do* about them: the four the ticket names get a
/// sentence each because each is a different typo, and everything else git
/// forbids — a leading or trailing `/`, an empty path component, a name ending in
/// `.`, `@{`, `@` alone, a control character — is one class, because the answer
/// to all of them is the same and a menu row is not a manual page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefNameFault {
    /// Nothing typed yet. **Not an error the reader made** — it is the state the
    /// field opens in — which is why the prompt only shows it once Enter is
    /// pressed on an empty field.
    Empty,
    /// A space, a tab, or anything else git calls whitespace.
    Space,
    /// `..`, which is a range operator in every revision grammar git has.
    Range,
    /// One of `~^:?*[\`, each of which means something to a revision parser.
    Reserved,
    /// A leading `-`, which every parse-options command would read as a flag.
    Dash,
    /// A trailing `.lock`, which is what git calls the file it takes while it
    /// writes a ref.
    Lock,
    /// Everything else `check-ref-format` refuses.
    Shape,
}

impl RefNameFault {
    /// The red line under the field.
    ///
    /// **git's rule in the reader's words, not git's.** `check-ref-format`
    /// answers with an exit status and nothing else, so there is no sentence of
    /// git's own to quote here the way a refused write quotes one — which is the
    /// whole reason this validation is local: a name the field can already tell
    /// is impossible should never cost a subprocess, and a hint that appeared
    /// eighty milliseconds after the keystroke would be a hint about the letter
    /// before last.
    #[must_use]
    pub fn sentence(self) -> &'static str {
        match self {
            Self::Empty => crate::i18n::Text::RefNameEmpty,
            Self::Space => crate::i18n::Text::RefNameSpace,
            Self::Range => crate::i18n::Text::RefNameRange,
            Self::Reserved => crate::i18n::Text::RefNameReserved,
            Self::Dash => crate::i18n::Text::RefNameDash,
            Self::Lock => crate::i18n::Text::RefNameLock,
            Self::Shape => crate::i18n::Text::RefNameShape,
        }
        .text()
    }
}

/// Whether git would take this as a branch or tag name — **`git check-ref-format
/// --branch`'s rules, answered here** (v2 ④).
///
/// Local by ruling. The alternative is a subprocess per keystroke, which is a
/// process pool spent on a question whose whole answer is a dozen character
/// tests; and the answer has to arrive *with* the keystroke, because what it
/// draws is a hint under the field the reader is still typing into.
///
/// The rules are git's, in `refs.c`'s own order: no whitespace, no `..`, none of
/// `~^:?*[\`, no leading `-`, no trailing `.lock`, no empty path component
/// (which covers a leading `/`, a trailing `/` and a `//`), no component that
/// begins or ends with a `.`, no `@{`, not `@` alone, and no ASCII control
/// characters or `DEL`.
#[must_use]
pub fn ref_name_fault(name: &str) -> Option<RefNameFault> {
    if name.is_empty() {
        return Some(RefNameFault::Empty);
    }
    if name.starts_with('-') {
        return Some(RefNameFault::Dash);
    }
    if name.chars().any(char::is_whitespace) {
        return Some(RefNameFault::Space);
    }
    if name.contains("..") {
        return Some(RefNameFault::Range);
    }
    if name.contains(['~', '^', ':', '?', '*', '[', '\\']) {
        return Some(RefNameFault::Reserved);
    }
    if name.ends_with(".lock") {
        return Some(RefNameFault::Lock);
    }
    if name.contains("@{") || name == "@" {
        return Some(RefNameFault::Shape);
    }
    if name
        .chars()
        .any(|character| character.is_control() || character == '\u{7f}')
    {
        return Some(RefNameFault::Shape);
    }
    // Component by component, which is how `check-ref-format` reads a name: a
    // leading `/`, a trailing `/` and a `//` are all one rule — "no empty
    // component" — and a leading or trailing `.` is the same rule about dots.
    for component in name.split('/') {
        if component.is_empty()
            || component.starts_with('.')
            || component.ends_with('.')
            || component.ends_with(".lock")
        {
            return Some(RefNameFault::Shape);
        }
    }
    None
}

/// **The command line one file's diff is** — the three readings, in one place.
///
/// Lifted out of [`answer`] on [`write_arguments`]'s own rule: a command line
/// built inside a `match` in a function that needs a subprocess to run is a
/// command line nothing can read. The three arms are three commands and not one
/// command with a flag, which is the whole of the untracked ruling (see
/// [`crate::preview::GitDiffAgainst`]).
///
/// `renamed_from` puts **both halves of a rename** in front of git, in git's own
/// order — see `GitQuestion::Diff::renamed_from` for why a pathspec naming only
/// the new name turns a rename into a brand-new file. It has no business in the
/// untracked arm: `--no-index` takes two *operands* and not a pathspec, and a
/// file git has never seen was never renamed.
///
/// Every path goes in as its own argument, after `--`, and never through a
/// shell — so a space, a quote or an ideograph in a name needs no escaping and
/// gets none. `core.quotepath=false` is set for every question in
/// [`git_command`], so what comes back names the file the same way.
#[must_use]
pub fn diff_arguments(
    against: crate::preview::GitDiffAgainst,
    path: &str,
    renamed_from: Option<&str>,
) -> Vec<String> {
    use crate::preview::GitDiffAgainst;
    let mut words = vec!["diff".to_owned(), "--no-color".to_owned()];
    match against {
        GitDiffAgainst::Index => words.push("--cached".to_owned()),
        GitDiffAgainst::WorkingTree => {}
        // **The whole file, as one addition.** `/dev/null` is git's own spelling
        // of "nothing" here and it is a string comparison inside
        // `diff-no-index.c`, not a path the operating system has to have — so it
        // is as true on Windows as anywhere else. `--no-index` is also the only
        // way to ask: `git diff` walks the index, and a file that is not in the
        // index is a pathspec that matches nothing.
        GitDiffAgainst::Nothing => {
            words.push("--no-index".to_owned());
            words.push("--".to_owned());
            words.push("/dev/null".to_owned());
            words.push(path.to_owned());
            return words;
        }
    }
    words.push("--".to_owned());
    if let Some(from) = renamed_from {
        words.push(from.to_owned());
    }
    words.push(path.to_owned());
    words
}

/// Whether a `diff` that exited non-zero did so because the two sides **differ**.
///
/// True only for the untracked reading, and only for git's documented `1`: a
/// `--no-index` diff is a comparison of two files and reports its verdict in the
/// exit code, the way `cmp` does. Every other exit code from it, and every
/// non-zero from the other two readings, is still a fault — an exit status is
/// not a licence to swallow whatever git said on the way out.
fn diff_differed(against: crate::preview::GitDiffAgainst, run: &GitRun) -> bool {
    against == crate::preview::GitDiffAgainst::Nothing && run.code == Some(1)
}

/// **The command line one write verb is**, and what goes down its standard input.
///
/// Split out of [`answer`] so that the boundary this slice was given can be
/// *read* rather than promised: a test enumerates every [`GitWriteVerb`], asks
/// for its arguments, and asserts that none of [`GIT_NEVER_WORDS`] is among them.
/// A rule that lives only inside a `match` in a function that needs a
/// subprocess to run is a rule nothing can check.
///
/// Three of the four pathspec verbs take their paths down the pipe;
/// `clean` is the exception because it is the one git subcommand of the four that
/// never learned `--pathspec-from-file` (checked against git 2.52), and it does
/// not need to: a discard is one file by construction (R14 puts no group-level
/// discard on the page), so its pathspec is one argument and always will be.
///
/// The named verbs take no pathspec at all and pass nothing down the pipe. Each
/// puts its name **last**, after every flag, which is where git's own synopsis
/// puts it — and each name has already been through [`ref_name_fault`], which is
/// what makes a leading `-` impossible here rather than merely unlikely.
#[must_use]
pub fn write_arguments(verb: &GitWriteVerb, paths: &[String]) -> (Vec<String>, Vec<u8>) {
    let pathspec: Vec<u8> = paths
        .iter()
        .flat_map(|path| path.as_bytes().iter().copied().chain(std::iter::once(0u8)))
        .collect();
    let from_stdin = || {
        vec![
            "--pathspec-from-file=-".to_owned(),
            "--pathspec-file-nul".to_owned(),
        ]
    };
    let with = |head: &[&str], tail: Vec<String>| {
        let mut arguments: Vec<String> = head.iter().map(|word| (*word).to_owned()).collect();
        arguments.extend(tail);
        arguments
    };
    match verb {
        GitWriteVerb::Stage => (with(&["add"], from_stdin()), pathspec),
        GitWriteVerb::Unstage => (with(&["restore", "--staged"], from_stdin()), pathspec),
        GitWriteVerb::Discard => (with(&["restore", "--worktree"], from_stdin()), pathspec),
        GitWriteVerb::DiscardUntracked => (
            with(&["clean", "-f", "-q", "--"], paths.to_vec()),
            Vec::new(),
        ),
        GitWriteVerb::CreateBranch { name, at } => (
            with(&["branch"], vec![name.clone(), at.clone()]),
            Vec::new(),
        ),
        GitWriteVerb::CreateTag { name, at } => {
            (with(&["tag"], vec![name.clone(), at.clone()]), Vec::new())
        }
        GitWriteVerb::RenameBranch { from, to } => (
            with(&["branch", "-m"], vec![from.clone(), to.clone()]),
            Vec::new(),
        ),
        GitWriteVerb::DeleteBranch { name } => {
            (with(&["branch", "-d"], vec![name.clone()]), Vec::new())
        }
        GitWriteVerb::DeleteTag { name } => (with(&["tag", "-d"], vec![name.clone()]), Vec::new()),
        // `--track` and not `--track=direct`: the plain flag is what every
        // version of git since 1.5 understands, and the direct/inherit spelling
        // is about *which* upstream a branch inherits when it is started from
        // another local — which this never is.
        GitWriteVerb::CheckoutTracking { name } => (
            with(
                &["checkout", "-b"],
                vec![
                    tracking_local_name(name).to_owned(),
                    "--track".to_owned(),
                    name.clone(),
                ],
            ),
            Vec::new(),
        ),
    }
}

impl GitWriteVerb {
    /// The ref this verb is about, when it is about one.
    ///
    /// **The pending key for a named verb**, and the reason it is one function
    /// rather than a `match` at each call site: the guard that stops a second
    /// press starting a second `git branch -d` and the flag that dims the row are
    /// the same fact, and two readings of it are two chances to disagree about
    /// which row is busy.
    ///
    /// A rename answers with the name that is *there now* — `from` — because that
    /// is the row on screen, and the row on screen is what has to stop answering
    /// presses.
    #[must_use]
    pub fn ref_subject(&self) -> Option<&str> {
        match self {
            Self::Stage | Self::Unstage | Self::Discard | Self::DiscardUntracked => None,
            Self::CreateBranch { name, .. }
            | Self::CreateTag { name, .. }
            | Self::DeleteBranch { name }
            | Self::DeleteTag { name }
            | Self::CheckoutTracking { name } => Some(name),
            Self::RenameBranch { from, .. } => Some(from),
        }
    }

    /// Whether carrying this out changes where `HEAD` is or which refs exist —
    /// and therefore whether **every** surface on this repository is now drawing
    /// something that was true a moment ago.
    ///
    /// The four pathspec verbs answer `false` not because they change nothing but
    /// because what they change is the *status*, which is the one thing each
    /// column re-reads for itself. A ref verb changes the branch list, the pills
    /// on every row of the history and, for a tracking checkout, the branch you
    /// are standing on — facts a graph in one pane and a panel in another both
    /// draw, and the disagreement between them is what this flag prevents.
    #[must_use]
    pub fn moves_refs(&self) -> bool {
        self.ref_subject().is_some()
    }
}

impl GitQuestion {
    /// Two questions are the same question when a newer answer to one makes the
    /// older answer to the other worthless.
    ///
    /// The subtle pairs are the ones this is written for. A **staged** diff and
    /// an **unstaged** diff of the same file are two documents, not one asked
    /// twice, so they must not coalesce — R25's mapping would otherwise show the
    /// index's diff under the working tree's heading depending on which process
    /// finished last. Two **pages** of history are likewise two answers: page two
    /// arriving does not make page one stale, and "Load more" while page one is
    /// still in flight must not cancel the page the list is about to draw. The
    /// page *size* is deliberately not part of the identity — asking for the same
    /// page again with a different count is the same question re-asked, and
    /// newest wins, exactly as [`crate::files::DirRequest::same_target`] rules
    /// for a directory whose path changed under its key.
    fn same_target(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::RepoProbe { dir: left }, Self::RepoProbe { dir: right }) => left == right,
            (Self::Status { root: left }, Self::Status { root: right }) => left == right,
            (Self::Refs { root: left }, Self::Refs { root: right }) => left == right,
            // **Every search is the same target**, whichever text it is about:
            // there is one search field, so a result for a query the reader has
            // already typed past is worthless the moment the next one is asked.
            (Self::Search { root: left, .. }, Self::Search { root: right, .. }) => left == right,
            (
                Self::Log {
                    root: left,
                    skip: from,
                    ..
                },
                Self::Log {
                    root: right,
                    skip: to,
                    ..
                },
            ) => left == right && from == to,
            (
                Self::Diff {
                    root: left,
                    path: from,
                    against: was,
                    ..
                },
                Self::Diff {
                    root: right,
                    path: to,
                    against: is,
                    ..
                },
            ) => left == right && from == to && was == is,
            (
                Self::Show {
                    root: left,
                    hash: old,
                    path: from,
                    ..
                },
                Self::Show {
                    root: right,
                    hash: new,
                    path: to,
                    ..
                },
            ) => left == right && old == new && from == to,
            (
                Self::DiffRange {
                    root: left,
                    a: old_a,
                    b: old_b,
                    path: from,
                    ..
                },
                Self::DiffRange {
                    root: right,
                    a: new_a,
                    b: new_b,
                    path: to,
                    ..
                },
            ) => left == right && old_a == new_a && old_b == new_b && from == to,
            // **Every expansion is the same target**, whichever commit it is
            // about: R15's accordion has exactly one open commit, so a second
            // press while the first list is still loading has already replaced
            // the list the answer was for. Coalescing on the root alone is what
            // stops a fast walk down the history spending a subprocess on every
            // commit it passed through.
            (Self::CommitFiles { root: left, .. }, Self::CommitFiles { root: right, .. }) => {
                left == right
            }
            // A comparison coalesces for the identical reason: there is one
            // compare block and moving its far end replaces the list the first
            // answer was for.
            (Self::CompareFiles { root: left, .. }, Self::CompareFiles { root: right, .. }) => {
                left == right
            }
            // **Two writes are never one question.** Every read above coalesces
            // because a newer answer makes an older one worthless; a write has no
            // answer to make worthless, it has an *effect*, and dropping the
            // older of two staging requests because a newer one arrived would
            // silently not stage a file the user asked for. The queue may
            // reorder work; it may not decline to do it.
            //
            // A checkout is the same kind of thing and gets the same answer, for
            // one extra reason of its own: two checkouts in flight are two
            // places to stand, and the second is where the user asked to be.
            (Self::Write { .. }, Self::Write { .. })
            | (Self::Checkout { .. }, Self::Checkout { .. }) => false,
            _ => false,
        }
    }
}

impl GitRequest {
    fn same_target(&self, other: &Self) -> bool {
        self.window == other.window
            && self.host == other.host
            && self.question.same_target(&other.question)
    }
}

// ── The answers ────────────────────────────────────────────────────────────

/// What the worker learned, addressed back to the seat that asked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitResponse {
    /// The window the asking surface was in — see [`GitRequest::window`].
    pub window: WindowId,
    pub host: GitHost,
    pub answer: GitAnswer,
}

impl GitResponse {
    /// **Who this answer belongs to** (F1b).
    ///
    /// A column and a graph belong to their tab and travel with it between
    /// windows. A **float** belongs to the window that minted its epoch, which is
    /// `codex-final.md` §2's instruction read literally: a host that is not a tab
    /// keeps the window routing it already had rather than being pressed into the
    /// `TabId` branch. That is also what the applying end already checks first —
    /// "is this float still live in me" — and it holds for the float's two
    /// *document* answers too, even though those land in a tab's pool: a diff
    /// asked from a float and answered after the tab it recorded has been carried
    /// into another window is dropped, which is the same cancellation a float
    /// dismissed mid-read already was.
    pub fn owner(&self) -> crate::AnswerOwner {
        match self.host {
            GitHost::Column(leaf) => crate::AnswerOwner::Tab(leaf.tab),
            GitHost::Graph { tab, .. } => crate::AnswerOwner::Tab(tab),
            GitHost::Float { .. } => crate::AnswerOwner::Window(self.window),
        }
    }
}

/// Every answer carries the question's own subject back with it.
///
/// Not for tidiness: by the time an answer lands the column may have been
/// re-rooted, and a payload that cannot say which repository it is about can
/// only be filed by faith. The cache checks and drops what no longer applies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitAnswer {
    Repo {
        dir: PathBuf,
        outcome: GitOutcome<PathBuf>,
    },
    Status {
        root: PathBuf,
        outcome: GitOutcome<GitStatus>,
    },
    Refs {
        root: PathBuf,
        outcome: GitOutcome<Vec<GitRefEntry>>,
    },
    /// The hashes one search matched, newest first (T4).
    ///
    /// The query travels back with them for the reason every other answer
    /// carries its own subject: a result that arrived after the reader typed
    /// another letter is a result about a question nobody is asking any more,
    /// and one that could not say which query it was about could only be filed
    /// by faith.
    Search {
        root: PathBuf,
        query: String,
        outcome: GitOutcome<Vec<String>>,
    },
    Log {
        root: PathBuf,
        skip: usize,
        outcome: GitOutcome<GitLog>,
    },
    Diff {
        root: PathBuf,
        path: String,
        against: crate::preview::GitDiffAgainst,
        outcome: GitOutcome<String>,
    },
    Show {
        root: PathBuf,
        hash: String,
        path: String,
        outcome: GitOutcome<String>,
    },
    DiffRange {
        root: PathBuf,
        a: String,
        b: Option<String>,
        path: String,
        outcome: GitOutcome<String>,
    },
    CommitFiles {
        root: PathBuf,
        hash: String,
        outcome: GitOutcome<Vec<GitCommitFile>>,
    },
    CompareFiles {
        root: PathBuf,
        a: String,
        b: Option<String>,
        outcome: GitOutcome<Vec<GitCommitFile>>,
    },
    /// A write finished. **The receipt** (R13).
    ///
    /// It carries the paths back because the panel dimmed exactly those rows
    /// while the process ran, and a receipt that could not say which rows it was
    /// about could only clear them all — including rows a *second* write, still
    /// running, is about.
    ///
    /// `Ok(())` and not `Ok(String)`: a successful `git add` prints nothing, and
    /// there is nothing to show. What the panel does with a success is ask the
    /// repository again, because the answer to "what changed" is a status and not
    /// an inference.
    Write {
        root: PathBuf,
        verb: GitWriteVerb,
        paths: Vec<String>,
        outcome: GitOutcome<()>,
    },
    /// A checkout finished, or git said why it would not (R10).
    Checkout {
        root: PathBuf,
        target: String,
        outcome: GitOutcome<()>,
    },
}

/// An answer or the reason there is none. A plain [`Result`] because that is
/// what this is, and because every consumer already knows how to read one.
pub type GitOutcome<T> = Result<T, GitFault>;

/// The four ways a question goes unanswered — all of them data, none of them a
/// crash, and none of them carrying its own wording (the sentences are G-2's).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitFault {
    /// This machine has no git we can run, and the string says exactly why —
    /// nothing on `PATH` and nothing in the three install defaults, or a
    /// `git.exe` that would not start. Honest degradation and not a defect (W5):
    /// the Git page says so once, and every other part of the product is
    /// untouched.
    GitMissing(String),
    /// The folder is not inside a repository. **The only empty state** (R17) —
    /// the mock-up's second sentence, "No graph for this folder", was struck for
    /// saying the same thing in a way that sounded like a missing feature.
    NotARepository,
    /// git ran, refused, and said why — dubious ownership (W3), a path longer
    /// than `core.longpaths` allows (W4), a corrupt index. **Its own words**,
    /// passed through rather than translated: the point of choosing the CLI was
    /// that the panel and the terminal beside it never disagree, and a refusal
    /// we paraphrase is a disagreement.
    Refused(String),
    /// git did not finish inside [`GIT_COMMAND_TIMEOUT`] and was killed.
    TimedOut,
}

// ── The model ──────────────────────────────────────────────────────────────

/// One letter of git's status alphabet.
///
/// R11 asks for the full set and this is it, plus the one letter R11's list left
/// out: `T`, a type change (a file became a symlink, or a symlink became a
/// file). Leaving it unparsed would not have shown a wrong row, it would have
/// shown *no* row — an entry whose two letters both fail to parse belongs to no
/// group and vanishes — which is the worst failure a status list has.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusCode {
    Modified,
    Typechange,
    Added,
    Deleted,
    Renamed,
    Copied,
    Unmerged,
    Untracked,
    Ignored,
}

impl StatusCode {
    /// A space is "nothing happened on this side", which is an absence and not a
    /// state — hence `Option` rather than a `None` variant that would have to be
    /// excluded from every match.
    #[must_use]
    pub fn from_letter(letter: char) -> Option<Self> {
        match letter {
            'M' => Some(Self::Modified),
            'T' => Some(Self::Typechange),
            'A' => Some(Self::Added),
            'D' => Some(Self::Deleted),
            'R' => Some(Self::Renamed),
            'C' => Some(Self::Copied),
            'U' => Some(Self::Unmerged),
            '?' => Some(Self::Untracked),
            '!' => Some(Self::Ignored),
            _ => None,
        }
    }

    /// The badge's letter — git's own, so that a row and a `git status` in the
    /// pane beside it read the same. Drawn by G-2's two badges (R11).
    #[allow(dead_code)]
    #[must_use]
    pub fn letter(self) -> char {
        match self {
            Self::Modified => 'M',
            Self::Typechange => 'T',
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::Unmerged => 'U',
            Self::Untracked => '?',
            Self::Ignored => '!',
        }
    }
}

/// The three lists a Git page shows changes in (R6).
///
/// **A file can be in two of them at once**, and that is the whole reason this
/// is not a field on an entry: `MM` means the index has one version of this file
/// and the working tree has another, so it is genuinely a staged change *and* an
/// unstaged one, and staging it again would stage something different from what
/// is already staged. One row per group is what git means and what every other
/// client shows.
///
/// The three headings are G-2's to draw; the membership rule is here, because it
/// is a fact about git's letters rather than about a list.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitGroup {
    Staged,
    Changes,
    Untracked,
}

impl GitGroup {
    /// **Which of a repository's copies a row in this group is measured against**
    /// — R25's mapping, and the one place it is written.
    ///
    /// Three groups, three comparisons, one function: the panel, the graph's
    /// working-tree rows and the right-click menu all opened a diff of their own
    /// accord and all three said `group == Staged` to themselves, which is how
    /// an untracked file came to be asked the changed file's question and
    /// answered with an empty page (user report, 2026-08-17). A mapping that
    /// lives in three `match`es is a mapping that can be wrong in two of them.
    #[must_use]
    pub fn diff_against(self) -> crate::preview::GitDiffAgainst {
        match self {
            Self::Staged => crate::preview::GitDiffAgainst::Index,
            Self::Changes => crate::preview::GitDiffAgainst::WorkingTree,
            Self::Untracked => crate::preview::GitDiffAgainst::Nothing,
        }
    }
}

/// One file a commit touched (R15) — one record of `--name-status`.
///
/// **One letter and not two.** A status entry carries the index's column and the
/// working tree's because a file can be in both at once; a commit is a single
/// point and has one story about each file it touched. The letter comes out of
/// the same [`StatusCode`] alphabet so that a badge drawn here and a badge drawn
/// in the changed list are the same badge, meaning the same thing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitCommitFile {
    /// Repo-relative, in git's grammar. For a rename this is where the file
    /// **went**, which is the row the page draws.
    pub path: String,
    pub code: StatusCode,
    /// Where a rename or a copy came from. `None` for everything else.
    pub renamed_from: Option<String>,
    /// How many lines went in and how many came out (D4, v2 ②), or `None` when
    /// git would not count them.
    ///
    /// **`None` is "binary" and not "zero".** git prints `-\t-` for a file it
    /// has no lines in, and a pair of zeroes there would be a claim that
    /// nothing changed about a file that may have been replaced entirely. The
    /// row draws an em dash for it, which is the same sentence the numbers are
    /// not.
    pub stat: Option<GitFileStat>,
}

/// The two ends of a comparison, older first — its identity (D6).
///
/// A name for the pair rather than a bare tuple in the cache, because it is the
/// *key*: it is compared, it decides whether a question is asked, and a reader
/// of that field has to be able to see at a glance that the far end being absent
/// is the working tree and not a missing value.
pub type ComparePair = (String, Option<String>);

/// `+N −M` for one file — `--numstat`'s two columns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GitFileStat {
    pub added: u32,
    pub removed: u32,
}

/// One line of `git status --porcelain`, in full.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitStatusEntry {
    /// What happened between `HEAD` and the index — the `X` column.
    pub staged: Option<StatusCode>,
    /// What happened between the index and the working tree — the `Y` column.
    pub unstaged: Option<StatusCode>,
    /// Repo-relative, in git's grammar: forward slashes, no drive.
    pub path: String,
    /// Where a rename or a copy came from. `None` for everything else.
    pub renamed_from: Option<String>,
}

impl GitStatusEntry {
    /// Whether this file is a merge conflict.
    ///
    /// git's own rule (`git status` documentation): unmerged means either column
    /// is `U`, or the pair is `AA` (both added) or `DD` (both deleted). Those two
    /// pairs are why this is not simply "contains a U" — they are conflicts whose
    /// letters say nothing about it.
    ///
    /// Read by G-2, which colours a conflicted row with the error ink (R29).
    #[allow(dead_code)]
    #[must_use]
    pub fn is_conflict(&self) -> bool {
        matches!(self.staged, Some(StatusCode::Unmerged))
            || matches!(self.unstaged, Some(StatusCode::Unmerged))
            || matches!(
                (self.staged, self.unstaged),
                (Some(StatusCode::Added), Some(StatusCode::Added))
                    | (Some(StatusCode::Deleted), Some(StatusCode::Deleted))
            )
    }

    /// Whether this entry belongs in one of the three lists.
    ///
    /// Untracked is decided by the letter and not by the column, because `??`
    /// occupies both. Ignored (`!!`) belongs to no list at all: we never ask for
    /// it, and if a caller ever does, the honest place for a file git was told to
    /// forget is nowhere rather than under a heading that claims it changed.
    #[allow(dead_code)]
    #[must_use]
    pub fn in_group(&self, group: GitGroup) -> bool {
        let real = |code: Option<StatusCode>| {
            !matches!(
                code,
                None | Some(StatusCode::Untracked) | Some(StatusCode::Ignored)
            )
        };
        match group {
            GitGroup::Staged => real(self.staged),
            GitGroup::Changes => real(self.unstaged),
            GitGroup::Untracked => {
                matches!(self.unstaged, Some(StatusCode::Untracked))
                    || matches!(self.staged, Some(StatusCode::Untracked))
            }
        }
    }
}

/// One `git status`: the head, the changes, and what the cap left out.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitStatus {
    /// The branch `HEAD` is on, or `None` when it is detached.
    pub branch: Option<String>,
    /// The upstream this branch tracks, when it has one.
    pub upstream: Option<String>,
    /// How far ahead of and behind that upstream — the two pills of G22, said as
    /// "N commits ahead" and never as "push" (R5).
    pub ahead: usize,
    pub behind: usize,
    /// `HEAD` names a commit rather than a branch.
    pub detached: bool,
    /// A repository with no commits yet: it *has* a branch name, but nothing to
    /// compare against, and its own sentence to say so (R7).
    pub unborn: bool,
    /// In git's order, cut to [`GIT_STATUS_CAP`].
    pub entries: Vec<GitStatusEntry>,
    /// How many entries the cap left out. Zero in the overwhelming majority of
    /// repositories, which is why the cap is allowed to be silent about itself
    /// until it is not.
    pub dropped: usize,
}

impl GitStatus {
    /// The entries of one list, in git's order.
    #[allow(dead_code)]
    pub fn group(&self, group: GitGroup) -> impl Iterator<Item = &GitStatusEntry> {
        self.entries
            .iter()
            .filter(move |entry| entry.in_group(group))
    }

    /// What a group heading counts (R7 — every heading carries its number).
    #[allow(dead_code)]
    #[must_use]
    pub fn count(&self, group: GitGroup) -> usize {
        self.group(group).count()
    }
}

/// One commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitCommit {
    pub hash: String,
    /// git's own abbreviation, not a prefix we cut: git shortens to whatever is
    /// unambiguous in *this* repository, and a fixed cut is a collision waiting
    /// for a big enough history.
    pub short: String,
    pub subject: String,
    /// Who wrote it — `%an`, git's own spelling of the name.
    ///
    /// **In the row since v2 ① (2026-08-16)**, where it is a column of its own
    /// (V1/V4). It was "in the tooltip and never in the row" (R16) for as long as
    /// the graph had four columns and no room for a fifth; the table has room,
    /// and a history nobody can see the authorship of is the one question a
    /// reader brings to a graph that the graph would not answer.
    pub author_name: String,
    /// The same person's `%ae`.
    ///
    /// **Tooltip only, and that is the ruling rather than a shortage of pixels**
    /// (V4): an address is how you *reach* somebody, and a column of them would
    /// be a column of the same domain repeated down the page. The name is the
    /// fact a reader is scanning for; the address is the fact they ask for once.
    pub author_email: String,
    /// **The rest of the message** — `%b`, git's own word for "everything after
    /// the subject and the blank line under it" (D1, v2 ②).
    ///
    /// A `String` with the newlines still in it and not a `Vec<String>`, because
    /// where the lines break is a fact about the *pane it is drawn in* and not
    /// about the commit: the same body wrapped at two widths is two different
    /// lists of lines and only one commit. What is stripped here is the trailing
    /// blank git always leaves at the end of `%b`, which is punctuation of the
    /// format rather than part of what was written.
    ///
    /// Empty for the overwhelming majority of commits, and that is not a missing
    /// value — a one-line commit message genuinely has no body, and what the
    /// expanded row draws for it is nothing at all rather than an empty box.
    pub body: String,
    /// Who *committed* it — `%cn`, which is the same person as the author on
    /// almost every commit and is a different person on a cherry-pick, a rebase,
    /// a patch applied from a mailing list, or anything a bot rewrote.
    ///
    /// Carried always and shown only when it differs (D2): a meta line that said
    /// "committed by" under every single row would be a line saying nothing, and
    /// one that never said it would lose the one case the field exists for.
    pub committer_name: String,
    /// The same person's `%ce`.
    pub committer_email: String,
    pub committer_unix: i64,
    /// The commit's own UTC offset in seconds — what makes `Aug 5` the date the
    /// commit was made in, which is the date git itself prints.
    pub committer_offset: i32,
    pub time_relative: String,
    /// Full hashes, in git's order — the first is the first parent. **The only
    /// input the lane algorithm has** (G-4), which is why it is carried from the
    /// first slice rather than added when the graph is drawn.
    pub parents: Vec<String>,
    /// Every name standing on this commit (R22), each saying which *kind* of
    /// name it is.
    ///
    /// **All three kinds since v2 ③ (2026-08-16).** The first slice kept locals
    /// only and said why: a remote tracking ref and a tag are different kinds of
    /// claim about a commit — one is where another machine last said it was, the
    /// other is a name somebody nailed on — and giving all three the one pill it
    /// drew would have said they were the same kind. The answer was never to
    /// throw two of them away for good; it was to wait until there were three
    /// pills. There are, so the parse keeps all three and carries the kind that
    /// decides which pill each gets.
    ///
    /// In [`GitRefKind`]'s own order rather than git's, because the order a row
    /// wears its names in is a fact about the row: `HEAD`'s local leads, the
    /// other locals follow it, then the remotes, then the tags.
    pub refs: Vec<GitRef>,
}

/// Which of the three kinds of name a ref is.
///
/// **Stated and never inferred from the text.** A branch genuinely called
/// `origin/main` exists, and so does a tag called `main`; the only place the
/// kind is unambiguous is git's own full ref name, which is why every parse in
/// this module reads `refs/heads/…`, `refs/remotes/…` and `refs/tags/…` rather
/// than counting slashes in a short one.
///
/// The `Ord` is the drawing order (v2 ③) and not alphabetical: locals, then
/// remotes, then tags. A derived order over the variants in the order they are
/// written is the same order twice, which is what makes the pill sort and the
/// panel sort agree without either of them naming the other.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GitRefKind {
    /// `refs/heads/…` — a branch in this repository.
    Local,
    /// `refs/remotes/…` — where another machine last said one of its branches
    /// was. Its short name keeps the remote on the front (`origin/main`),
    /// because the remote is half of what the name says.
    Remote,
    /// `refs/tags/…` — a name somebody nailed on.
    Tag,
}

/// One name worn on a commit row (R22).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRef {
    /// The ref's short name — `main`, `origin/main`, `v1.0`; never
    /// `refs/heads/main`.
    pub name: String,
    /// Whether `HEAD` is this ref. The pill wears the accent ring for it, and
    /// there is at most one on the whole page.
    pub head: bool,
    /// Which of the three pills it gets (v2 ③).
    pub kind: GitRefKind,
}

/// One row of `git for-each-ref` — a branch here, a branch over there, or a tag
/// (T6/T7, v2 ③).
///
/// **One type for all three and one question that answers them**, which is the
/// whole of T6: the panel's branch list, the graph's filter menu and the day a
/// tag needs a row are three readers of one `for-each-ref`, and three questions
/// would be three subprocesses spent on one file of refs. What tells the three
/// apart is [`Self::kind`], and what makes that safe is that it is read off
/// git's full ref name rather than guessed at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRefEntry {
    pub kind: GitRefKind,
    /// The short name, in the grammar [`GitRef::name`] uses.
    pub name: String,
    /// What it points at — `%(objectname)`.
    ///
    /// **The tag object for an annotated tag, and not the commit under it.**
    /// That is git's own answer to `%(objectname)` and it is kept as given: the
    /// dereferencing spelling (`%(*objectname)`) is empty for every ref that is
    /// not an annotated tag, so a format that used it would need git's `%(if)`
    /// to fall back — a second grammar in the format string to answer a question
    /// nothing on this page asks. What draws a tag on a *row* is the log's own
    /// decoration, which git has already resolved to the commit.
    pub object: String,
    /// The upstream this ref tracks, short (`origin/main`), when it has one.
    pub upstream: Option<String>,
    /// Against that upstream — `%(upstream:track)`, through the one parse the
    /// status head also uses.
    pub ahead: usize,
    pub behind: usize,
    /// Whether `HEAD` is on it — `%(HEAD)`, which git writes as `*`.
    pub is_head: bool,
    /// When it was last committed to, in seconds since the epoch.
    pub committer_unix: i64,
    /// That fact through [`relative_time`], so a ref row and a commit row say
    /// ages the same way.
    pub committerdate_relative: String,
}

/// One page of history (R16).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitLog {
    /// How many commits were skipped to reach this page.
    pub skip: usize,
    pub commits: Vec<GitCommit>,
    /// Whether there is a page after this one — asked for by requesting one
    /// commit more than the page holds and noticing it arrived. The alternative,
    /// counting the whole history to compare against, walks the entire repository
    /// to draw one row.
    pub has_more: bool,
}

// ── The relative-time table (R8) ───────────────────────────────────────────

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const SECONDS_PER_DAY: i64 = 86_400;

/// How long ago, in the panel's words.
///
/// The ladder is R8's, ruled 2026-08-15: under a minute is `now`, then `5m`,
/// then `3h`, then `2d` out to a week, then the calendar date `Aug 5`, and past
/// a year the year with it, `2024 Nov`. Two properties are load-bearing. It is
/// **short** — these labels sit at the right edge of a 240-pixel column beside a
/// branch name that must not be cut for them. And it **stops counting and starts
/// dating** at a week, because "37d" is a number a reader has to convert and
/// "Aug 5" is one they can just look at.
///
/// Written here rather than taken from git's own `--date=relative`, which offers
/// `3 weeks ago` in a form we would only have to re-cut, and which says it
/// differently in every locale — including the `C` one this module forces on
/// every child for exactly the reason that its wording is not a contract.
///
/// The calendar buckets are rendered in the **commit's own** timezone, which is
/// the offset git recorded and the date `git log` itself would print. A clock
/// that has moved backwards makes `then` later than `now`; that reads as `now`,
/// which is the closest true thing there is to say about it.
#[must_use]
pub fn relative_time(then_unix: i64, offset_seconds: i32, now_unix: i64) -> String {
    let delta = now_unix.saturating_sub(then_unix).max(0);
    if delta < 60 {
        return "now".to_owned();
    }
    if delta < 60 * 60 {
        return format!("{}m", delta / 60);
    }
    if delta < SECONDS_PER_DAY {
        return format!("{}h", delta / (60 * 60));
    }
    if delta < 7 * SECONDS_PER_DAY {
        return format!("{}d", delta / SECONDS_PER_DAY);
    }
    let local = then_unix + i64::from(offset_seconds);
    let (year, month, day) = crate::seed::civil_from_days(local.div_euclid(SECONDS_PER_DAY));
    let name = MONTHS[usize::try_from(month - 1).unwrap_or(0).min(11)];
    if delta < 365 * SECONDS_PER_DAY {
        format!("{name} {day}")
    } else {
        format!("{year} {name}")
    }
}

/// The same instant said in full: `YYYY-MM-DD HH:mm`, in the zone it was
/// written in (D2/V3).
///
/// **Beside [`relative_time`] and not instead of it**, because the two answer
/// different questions and the page asks both: a column of rows is scanned, and
/// `3d` is what a scan wants; one row is *opened*, and an opened row is being
/// read rather than scanned, at which point "three days ago" stops being an
/// answer and the date starts being one.
///
/// The offset is the commit's own, exactly as it is for the relative table, so
/// the date printed here is the date git prints — the date it was made where it
/// was made, and not the date it happens to be here.
#[must_use]
pub fn absolute_time(then_unix: i64, offset_seconds: i32) -> String {
    let local = then_unix + i64::from(offset_seconds);
    let days = local.div_euclid(SECONDS_PER_DAY);
    let seconds = local.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = crate::seed::civil_from_days(days);
    let (hour, minute) = (seconds / 3600, (seconds % 3600) / 60);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

// ── The parsers ────────────────────────────────────────────────────────────

/// `git status --porcelain=v1 -z --branch`, decoded.
///
/// **Why v1 and not v2.** v2 carries file modes and object ids this panel never
/// draws, at the cost of five fields per line to skip; v1 is the two letters, the
/// path, and — for a rename — where it came from, which is exactly the model.
/// R11's full alphabet is expressible in both.
///
/// **Why `-z`.** The record separator is a NUL, which is the one byte a path
/// cannot contain, so nothing has to be unquoted and nothing has to be guessed.
/// Without it git escapes unusual paths into C string literals and a Windows path
/// full of backslashes becomes a puzzle. The recorded shape, from real git:
/// `## main...origin/main [ahead 2]\0 M mod.txt\0R  new.txt\0old.txt\0`; a rename
/// spends two records and the **new** path comes first.
#[must_use]
pub fn parse_status(bytes: &[u8]) -> GitStatus {
    let text = String::from_utf8_lossy(bytes);
    let mut status = GitStatus::default();
    let mut records = text.split('\0');
    while let Some(record) = records.next() {
        // The last record is followed by its terminator, so the split ends on an
        // empty string; nothing else in this stream is empty.
        if record.is_empty() {
            continue;
        }
        if let Some(head) = record.strip_prefix("## ") {
            parse_status_head(head, &mut status);
            continue;
        }
        let mut letters = record.chars();
        let (Some(x), Some(y)) = (letters.next(), letters.next()) else {
            continue;
        };
        let staged = StatusCode::from_letter(x);
        let unstaged = StatusCode::from_letter(y);
        // `XY ` is three ASCII bytes whatever the path is, so the rest is the
        // path exactly — no split on a space that a filename is allowed to
        // contain.
        let path = record.get(3..).unwrap_or_default().to_owned();
        // A rename or a copy spends a second record on where it came from, and it
        // is consumed here whether or not this entry survives the cap: falling
        // out of step by one record would make every entry after the cap the
        // wrong file.
        let moved = [staged, unstaged]
            .into_iter()
            .flatten()
            .any(|code| matches!(code, StatusCode::Renamed | StatusCode::Copied));
        let renamed_from = moved.then(|| records.next().unwrap_or_default().to_owned());
        if status.entries.len() < GIT_STATUS_CAP {
            status.entries.push(GitStatusEntry {
                staged,
                unstaged,
                path,
                renamed_from,
            });
        } else {
            status.dropped += 1;
        }
    }
    status
}

/// The `## ` line: branch, upstream, and how far apart they are.
///
/// Four grammars, and the two unusual ones are the ones worth naming: `HEAD (no
/// branch)` is a detached head, and `No commits yet on main` is a repository
/// whose first commit has not happened — which *has* a branch name even though
/// nothing points at it, and which needs its own sentence (R7) rather than
/// looking like a branch that lost its commits.
fn parse_status_head(head: &str, status: &mut GitStatus) {
    if head == "HEAD (no branch)" {
        status.detached = true;
        return;
    }
    let named = match head.strip_prefix("No commits yet on ") {
        Some(rest) => {
            status.unborn = true;
            rest
        }
        None => head,
    };
    // A ref name can hold neither a space nor `..`, so neither of these two cuts
    // can land inside a branch name — which is what makes cutting on them safe
    // rather than merely usual.
    let (names, track) = match named.find(" [") {
        Some(at) => (&named[..at], &named[at + 1..]),
        None => (named, ""),
    };
    let (branch, upstream) = match names.find("...") {
        Some(at) => (&names[..at], Some(names[at + 3..].to_owned())),
        None => (names, None),
    };
    status.branch = (!branch.is_empty()).then(|| branch.to_owned());
    status.upstream = upstream;
    (status.ahead, status.behind) = parse_track(track);
}

/// `[ahead 2, behind 1]`, `[gone]`, or nothing — the same grammar in the status
/// head and in `for-each-ref`'s `%(upstream:track)`, so it is parsed once.
///
/// `[gone]` — an upstream that has been deleted — is zero and zero rather than a
/// state of its own: there is nothing to be ahead of. Saying so is R9's, when the
/// branch list learns to draw remotes at all.
fn parse_track(track: &str) -> (usize, usize) {
    let inner = track.trim().trim_start_matches('[').trim_end_matches(']');
    let mut ahead = 0;
    let mut behind = 0;
    for part in inner.split(',') {
        let part = part.trim();
        if let Some(count) = part.strip_prefix("ahead ") {
            ahead = count.trim().parse().unwrap_or(0);
        } else if let Some(count) = part.strip_prefix("behind ") {
            behind = count.trim().parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

/// `git for-each-ref refs/heads refs/remotes refs/tags`, decoded (T6, v2 ③).
///
/// **The sort is ours and git's `--sort` is not asked for.** `for-each-ref`
/// takes a `--sort` and it would answer *one* of the two orders this list wants
/// — the order is "the branch you are on, then the other locals newest first,
/// then the remotes, then the tags", which is a sort on a key git has no name
/// for. A `--sort=-committerdate` would put a stale local under a fresh tag, and
/// a `--sort=refname` would lose the recency the branch list is read for. So the
/// refs arrive in whatever order git likes and are ordered here, once, beside
/// the parse — the same reason [`crate::files`] sorts a directory on its own
/// thread: the answer should arrive in the shape its readers draw.
///
/// Within each of the three groups the sort is **stable**, so remotes and tags
/// keep git's own alphabetical order rather than being shuffled for having been
/// compared.
///
/// **`refs/remotes/<remote>/HEAD` is dropped.** It is a symbolic ref naming
/// which branch that remote considers its default — a *pointer to* a row this
/// list already has, and drawn as a row of its own it would be `origin/HEAD`
/// sitting beside `origin/main` claiming to be a second branch.
#[must_use]
pub fn parse_refs(bytes: &[u8], now_unix: i64) -> Vec<GitRefEntry> {
    let text = String::from_utf8_lossy(bytes);
    let mut refs: Vec<GitRefEntry> = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\0');
            let (kind, name) = split_ref_name(fields.next().unwrap_or_default())?;
            if kind == GitRefKind::Remote && (name == "HEAD" || name.ends_with("/HEAD")) {
                return None;
            }
            let object = fields.next().unwrap_or_default().to_owned();
            let upstream = fields
                .next()
                .filter(|text| !text.is_empty())
                .map(str::to_owned);
            let (ahead, behind) = parse_track(fields.next().unwrap_or_default());
            let is_head = fields.next() == Some("*");
            let (committer_unix, offset) =
                parse_iso_strict(fields.next().unwrap_or_default()).unwrap_or((0, 0));
            Some(GitRefEntry {
                kind,
                name,
                object,
                upstream,
                ahead,
                behind,
                is_head,
                committer_unix,
                committerdate_relative: relative_time(committer_unix, offset, now_unix),
            })
        })
        .collect();
    refs.sort_by_key(|entry| {
        (
            entry.kind,
            // Only the locals are ordered among themselves: the branch you are
            // on leads (R9), and the rest are newest first, which is the order a
            // branch list is actually read in. Negated rather than reversed so
            // the whole key stays one ascending tuple.
            match entry.kind {
                GitRefKind::Local => (!entry.is_head, -entry.committer_unix),
                GitRefKind::Remote | GitRefKind::Tag => (false, 0),
            },
        )
    });
    refs
}

/// A full ref name split into what kind it is and what it is called.
///
/// The one place this module turns `refs/…` into a kind, so the log's decoration
/// and `for-each-ref`'s rows cannot come to two different readings of the same
/// name. Anything outside the three trees — `refs/stash`, `refs/notes/…`, a
/// namespace some tool invented — is not a name this product draws, and is
/// dropped rather than guessed at.
fn split_ref_name(refname: &str) -> Option<(GitRefKind, String)> {
    for (prefix, kind) in [
        ("refs/heads/", GitRefKind::Local),
        ("refs/remotes/", GitRefKind::Remote),
        ("refs/tags/", GitRefKind::Tag),
    ] {
        if let Some(name) = refname.strip_prefix(prefix)
            && !name.is_empty()
        {
            return Some((kind, name.to_owned()));
        }
    }
    None
}

/// The local branches of a refs answer, in the order they arrived — what the
/// panel's BRANCHES group and the filter menu's checkboxes both list.
pub fn local_branches(refs: &[GitRefEntry]) -> impl Iterator<Item = &GitRefEntry> {
    refs.iter().filter(|entry| entry.kind == GitRefKind::Local)
}

/// The remote-tracking branches of a refs answer (T9's REMOTES sub-group).
pub fn remote_branches(refs: &[GitRefEntry]) -> impl Iterator<Item = &GitRefEntry> {
    refs.iter().filter(|entry| entry.kind == GitRefKind::Remote)
}

/// `git show --raw --numstat -z --format=` (and `git diff` in the same clothes),
/// decoded (R15, D4).
///
/// **One process and two blocks, because git refuses to print the two facts in
/// one.** The obvious command is `--name-status --numstat`, and it does not
/// exist: those two are one setting with two values, and git silently prints
/// only the name-status half whichever order they are given in (checked on the
/// real machine, 2026-08-16, with and without `-z`). `--raw` is a *different*
/// output format and does combine — so the stream is the raw block, whole, and
/// then the numstat block, whole, in the same file order. What that costs over
/// the impossible command is one `:` per file; what it saves is a second
/// subprocess per expansion, which is exactly the reading R31 is about.
///
/// **The record grammars.** A raw record is
/// `:<mode> <mode> <sha> <sha> <STATUS>\0<path>\0`, with a rename or a copy
/// spending a third field for the name it went to; the similarity score rides
/// on the letter (`R075`), which is why only the first character is read. A
/// numstat record is `<added>\t<removed>\t<path>\0`, with a rename writing an
/// *empty* path there and its two names in the two fields after — git's own
/// `show_numstat` puts a NUL straight after the second tab — and a binary file
/// writing `-\t-` where the numbers would be.
///
/// The two blocks are told apart by their first byte and not by counting: raw
/// records open with `:` and numstat records open with a digit or a dash, so a
/// commit that touched nothing, or a merge (which `--raw` says nothing about,
/// because it compares against no parent by default), simply produces neither.
/// That is an empty answer and not a failure, and it is the accordion's own
/// empty sentence rather than anything this function has to detect.
#[must_use]
pub fn parse_diff_files(bytes: &[u8]) -> Vec<GitCommitFile> {
    let text = String::from_utf8_lossy(bytes);
    let mut fields = text.split('\0').peekable();
    let mut files: Vec<GitCommitFile> = Vec::new();
    // The raw block, while the stream is still in it.
    while let Some(record) = fields.peek() {
        let Some(rest) = record.strip_prefix(':') else {
            break;
        };
        // `:<mode> <mode> <sha> <sha> <STATUS>` — the letter is the last word,
        // and reading it from the end rather than by counting spaces is what
        // keeps this honest about the combined-diff spellings git may one day
        // hand back for a merge.
        let letter = rest.split_whitespace().next_back().unwrap_or_default();
        let code = letter.chars().next().and_then(StatusCode::from_letter);
        fields.next();
        let Some(code) = code else { continue };
        let moved = matches!(code, StatusCode::Renamed | StatusCode::Copied);
        let first = fields.next().unwrap_or_default().to_owned();
        // A rename's *second* path is the row: the file is at the new name now,
        // and that is the name a press has to be able to open.
        let (path, renamed_from) = if moved {
            (fields.next().unwrap_or_default().to_owned(), Some(first))
        } else {
            (first, None)
        };
        if path.is_empty() {
            continue;
        }
        files.push(GitCommitFile {
            path,
            code,
            renamed_from,
            stat: None,
        });
    }
    // The numstat block. Matched onto the rows already built **by path**, not by
    // position: the two blocks are in the same order today, and a pairing that
    // depended on that would be a silent mis-attribution of `+900 −3` to the
    // wrong file on the day git reorders one of them.
    while let Some(record) = fields.next() {
        if record.is_empty() {
            continue;
        }
        let mut parts = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(tail)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // A rename writes its two names in the two fields *after* this one and
        // leaves the path here empty; the row is at the name it went to, which
        // is the second of them.
        let path = if tail.is_empty() {
            let _from = fields.next();
            fields.next().unwrap_or_default().to_owned()
        } else {
            tail.to_owned()
        };
        let stat = match (added.parse::<u32>(), removed.parse::<u32>()) {
            (Ok(added), Ok(removed)) => Some(GitFileStat { added, removed }),
            // `-\t-`: git has no lines to count in this file.
            _ => None,
        };
        if let Some(file) = files.iter_mut().find(|file| file.path == path) {
            file.stat = stat;
        }
    }
    files
}

/// How many NUL-separated fields one commit of `--format=` above is spelled in.
///
/// Written once, beside the parse that strides by it, because the format string
/// and this number are one fact: a field added to one and not the other reads
/// every commit's message as the next commit's hash, and does it silently.
const LOG_FIELDS: usize = 11;

/// `git log -z --parents`, decoded.
///
/// `wanted` is the page size the caller asked for; the command asks git for one
/// more than that, so a page that comes back longer than `wanted` is how "there
/// is more" is known without counting the repository.
#[must_use]
pub fn parse_log(bytes: &[u8], now_unix: i64, skip: usize, wanted: usize) -> GitLog {
    let text = String::from_utf8_lossy(bytes);
    let fields: Vec<&str> = text.split('\0').collect();
    // [`LOG_FIELDS`] fields per commit, NUL between them and a NUL after the
    // last — so the whole stream divides evenly and `chunks_exact` drops the
    // terminator's empty tail without having to know whether git wrote a
    // separator or a terminator.
    //
    // Eight since v2 ① (2026-08-16): `%ae` sits beside `%an` because the author
    // column and its tooltip are one fact read twice, and a second `git log` to
    // learn the address of a commit already on screen would be a second reading
    // of the same history (R31's whole objection). Eleven since v2 ②, for the
    // same reason again: `%cn`, `%ce` and `%b` are what the expanded row says,
    // and the stride is still honest because `%b` is last — see the format
    // string, which is where that argument is written down.
    let mut commits: Vec<GitCommit> = fields
        .chunks_exact(LOG_FIELDS)
        .map(|record| {
            let (committer_unix, committer_offset) = parse_iso_strict(record[6]).unwrap_or((0, 0));
            GitCommit {
                hash: record[0].to_owned(),
                short: record[1].to_owned(),
                author_name: record[2].to_owned(),
                author_email: record[3].to_owned(),
                committer_name: record[4].to_owned(),
                committer_email: record[5].to_owned(),
                committer_unix,
                committer_offset,
                time_relative: relative_time(committer_unix, committer_offset, now_unix),
                subject: record[7].to_owned(),
                // Space-separated by `%P`, and empty for the root commit — which
                // is an empty list rather than a missing one, because the root
                // genuinely has no parents.
                parents: record[8].split_whitespace().map(str::to_owned).collect(),
                refs: parse_decoration(record[9]),
                // `%b` ends with the newline that separated it from the record
                // terminator; a body's *own* trailing blank lines are not
                // something anybody typed on purpose either, so the whole tail
                // goes. The breaks *inside* it are kept exactly as written —
                // they are the paragraphs (D1).
                body: record[10].trim_end().to_owned(),
            }
        })
        .collect();
    let has_more = commits.len() > wanted;
    commits.truncate(wanted);
    GitLog {
        skip,
        commits,
        has_more,
    }
}

/// `%D` under `--decorate=full`, decoded into the pills one row wears (R22).
///
/// **Full ref names and not short ones**, and the flag is the whole reason this
/// is readable: `%D`'s default spelling gives `main` and `origin/main` and
/// `v1.0` side by side, three kinds of claim that can only be told apart by
/// guessing at slashes — and a branch genuinely called `origin/main` exists.
/// Under `--decorate=full` they arrive as `refs/heads/…`, `refs/remotes/…` and
/// `refs/tags/…`, and the kind is stated rather than inferred.
///
/// git separates them with `", "` and prefixes the checked-out one with
/// `"HEAD -> "`. A detached head is the bare word `HEAD`, which is a ref
/// standing on this commit with no branch behind it — a pill, because "you are
/// here" is exactly what the graph is for.
///
/// **All three kinds since v2 ③, and the order is the drawing order.** The pills
/// of a row read `HEAD`'s local, the other locals, the remotes, the tags — see
/// [`GitCommit::refs`]. Sorted here rather than at the paint because it is one
/// order for every reader of the field, and a second sort in the painter would
/// be a second place for it to be written differently.
#[must_use]
fn parse_decoration(text: &str) -> Vec<GitRef> {
    const HEAD_ARROW: &str = "HEAD -> ";
    let mut refs: Vec<GitRef> = text
        .split(", ")
        .filter_map(|item| {
            let item = item.trim();
            if item == "HEAD" {
                return Some(GitRef {
                    name: "HEAD".to_owned(),
                    head: true,
                    kind: GitRefKind::Local,
                });
            }
            let (head, refname) = match item.strip_prefix(HEAD_ARROW) {
                Some(rest) => (true, rest),
                None => (false, item),
            };
            // A ref outside the three trees is not a ref this row can draw.
            let (kind, name) = split_ref_name(refname)?;
            Some(GitRef { name, head, kind })
        })
        .collect();
    refs.sort_by_key(|reference| (reference.kind, !reference.head));
    refs
}

/// `2026-08-15T10:18:07-04:00` → the instant and the offset it was written in.
///
/// Both halves are needed and they answer different questions: the instant is
/// what "how long ago" is measured from, and the offset is what makes the date
/// git prints and the date we print the same date.
fn parse_iso_strict(text: &str) -> Option<(i64, i32)> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let number = |from: usize, to: usize| text.get(from..to)?.parse::<i64>().ok();
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    let offset = match bytes[19] {
        b'Z' => 0,
        sign @ (b'+' | b'-') => {
            if bytes.len() < 25 || bytes[22] != b':' {
                return None;
            }
            let magnitude = number(20, 22)? * 3600 + number(23, 25)? * 60;
            let signed = if sign == b'-' { -magnitude } else { magnitude };
            i32::try_from(signed).ok()?
        }
        _ => return None,
    };
    let wall = crate::seed::days_from_civil(year, month, day) * SECONDS_PER_DAY
        + hour * 3600
        + minute * 60
        + second;
    Some((wall - i64::from(offset), offset))
}

/// Which kind of "no" git just said.
///
/// The one string match in this module, and it is only sound because every child
/// runs under `LC_ALL=C`: git's refusals are translated, and the English
/// sentence is a contract only when English is what we asked for. Everything
/// that is not this one sentence keeps git's own words and is shown as they are
/// (W3, W4) — the panel is not in the business of explaining git to itself.
fn classify_failure(stderr: &str) -> GitFault {
    let first = stderr.lines().next().unwrap_or_default().trim_end();
    if first.contains("not a git repository") {
        GitFault::NotARepository
    } else {
        // The first line and not all of them: git follows a refusal with the
        // remedy, which is several lines of shell commands and belongs in the
        // terminal beside this rather than in a panel's banner.
        GitFault::Refused(first.to_owned())
    }
}

// ── Running git ────────────────────────────────────────────────────────────

/// A finished child: what it printed, and whether it was happy.
struct GitRun {
    ok: bool,
    /// What the child exited with, when it exited at all.
    ///
    /// `ok` is `status.success()` and answers the only question almost every
    /// question here has. One does not fit it: `git diff --no-index` is
    /// documented to exit **1 when the two files differ**, which for the
    /// untracked reading is the ordinary outcome and not a failure — so that one
    /// arm reads the number rather than the verdict. `None` on the platforms
    /// where a signal took the child, which on Windows is never.
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: String,
}

/// A `git` invocation with everything this module always wants already on it.
///
/// * `-C <dir>` rather than a working directory, so that a folder which has been
///   deleted under us fails as git's own sentence rather than as a spawn error
///   we would have to invent words for.
/// * `--no-optional-locks`, so that reading the repository never takes a lock the
///   user's own `git` in the pane beside this one would then wait for. A panel
///   that blocks the terminal it lives next to is worse than a panel.
/// * `core.quotepath=false`, so a path with a non-ASCII name arrives as itself
///   rather than as `\303\251` octal escapes.
/// * `LC_ALL=C`, for the reason the module header gives.
/// * `GIT_TERMINAL_PROMPT=0`, so no read can ever turn into a child sitting
///   forever waiting for a password nobody can see it asking for.
///
/// Through [`bt_platform::quiet_command`] and not `Command::new`, which is where
/// the console this child must not be given is refused (§7.40 ①). It used to be
/// a `no_window` helper of this module's own, whose note said the flag was "not
/// needed today — this build is still a console subsystem application": that
/// stopped being true the day `main.rs` grew `#![windows_subsystem = "windows"]`,
/// and the flag has been load-bearing ever since.
fn git_command(program: &Path, dir: &Path, arguments: &[&OsStr]) -> Command {
    let mut command = bt_platform::quiet_command(program);
    command
        .arg("-C")
        .arg(dir)
        .args(["--no-optional-locks", "-c", "core.quotepath=false"])
        .args(arguments)
        .env("LC_ALL", "C")
        .env("LANGUAGE", "")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Read one pipe to its end on a thread of its own.
///
/// Both pipes are drained concurrently because a child that fills one while we
/// are blocked reading the other is a child that never exits — the classic
/// deadlock, and the reason this is not two sequential reads.
/// **In the worker's band, not the frame's.** A new thread starts at
/// `THREAD_PRIORITY_NORMAL` whatever the thread that spawned it was running at,
/// so these two — spawned by [`GitWorker`]'s own below-normal thread — would
/// otherwise come back up to stand beside the window's loop every time a `git`
/// runs, which is precisely while the page they are for is waiting.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    bt_platform::spawn_at_priority(
        "bt-git-pipe",
        bt_platform::ThreadPriority::BelowNormal,
        move || {
            let mut buffer = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buffer);
            }
            buffer
        },
    )
    // `thread::spawn`, which this replaces, panics when the kernel will not give
    // out a thread; the behaviour is kept rather than turned into a quiet
    // half-drained pipe, which is a deadlock wearing a shrug.
    .expect("spawn a git pipe reader")
}

/// Run one `git`, and never wait for it longer than `timeout`.
fn run_git(command: Command, timeout: Duration) -> GitOutcome<GitRun> {
    run_git_with_input(command, timeout, Vec::new())
}

/// The same, with a pathspec list fed down the child's own standard input.
///
/// **Why the list goes through a pipe at all.** A write verb's pathspec is a
/// group's whole contents, which on a generated-file avalanche is thousands of
/// paths — and a Windows command line is bounded at 32767 characters, so a
/// `git add -- a b c …` long enough to matter simply fails to start. Git's
/// `--pathspec-from-file=- --pathspec-file-nul` reads the same list off stdin,
/// NUL-separated, with no bound and no quoting: the one byte a path cannot
/// contain is the one separating them, so a path with a space, a quote or a
/// newline in it needs no escaping and gets none.
///
/// The write is on a thread of its own, and the handle is dropped afterwards to
/// close the pipe, for [`drain`]'s reason turned around: a child reading a
/// pathspec longer than the pipe's buffer blocks until somebody reads it, and a
/// parent that had written its list from the polling loop would be blocked in the
/// write while the child was blocked waiting for the rest of it.
fn run_git_with_input(
    mut command: Command,
    timeout: Duration,
    input: Vec<u8>,
) -> GitOutcome<GitRun> {
    let feeding = !input.is_empty();
    if feeding {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .spawn()
        .map_err(|error| GitFault::GitMissing(format!("git.exe would not start: {error}")))?;
    if feeding {
        let mut pipe = child.stdin.take();
        // Below normal for the reason [`drain`] gives: a thread does not inherit
        // the band of the thread that spawned it.
        bt_platform::spawn_at_priority(
            "bt-git-stdin",
            bt_platform::ThreadPriority::BelowNormal,
            move || {
                if let Some(pipe) = pipe.as_mut() {
                    use std::io::Write as _;
                    let _ = pipe.write_all(&input);
                }
                // Dropped here, which is what closes the pipe and tells git the
                // list has ended. Without it a child waits for a list that is
                // complete.
                drop(pipe);
            },
        )
        .expect("spawn a git stdin writer");
    }
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = out.join().unwrap_or_default();
                let stderr = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
                return Ok(GitRun {
                    ok: status.success(),
                    code: status.code(),
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                return Err(GitFault::GitMissing(format!(
                    "git.exe could not be waited for: {error}"
                )));
            }
        }
        if Instant::now() >= deadline {
            // Killed, reaped, and reported as a timeout **before** anything is
            // read back from it. A killed child does exit, and exit is what
            // `try_wait` reports — so a guard that went round the loop once more
            // after the kill would find the process finished and hand its half of
            // an answer back as though it had merely failed. The pipes close with
            // the child, so the two reader threads end on their own; their
            // buffers are what is being thrown away here, deliberately.
            let _ = child.kill();
            let _ = child.wait();
            return Err(GitFault::TimedOut);
        }
        thread::sleep(GIT_POLL_INTERVAL);
    }
}

/// Every question, wearing one fault.
///
/// A fault is about the machine or the repository rather than about the question,
/// so one function can dress any of them in it — which is what lets "there is no
/// git here" be answered without a single command being built.
fn faulted(question: &GitQuestion, fault: GitFault) -> GitAnswer {
    match question {
        GitQuestion::RepoProbe { dir } => GitAnswer::Repo {
            dir: dir.clone(),
            outcome: Err(fault),
        },
        GitQuestion::Status { root } => GitAnswer::Status {
            root: root.clone(),
            outcome: Err(fault),
        },
        GitQuestion::Refs { root } => GitAnswer::Refs {
            root: root.clone(),
            outcome: Err(fault),
        },
        GitQuestion::Search { root, query } => GitAnswer::Search {
            root: root.clone(),
            query: query.clone(),
            outcome: Err(fault),
        },
        GitQuestion::Log { root, skip, .. } => GitAnswer::Log {
            root: root.clone(),
            skip: *skip,
            outcome: Err(fault),
        },
        GitQuestion::Diff {
            root,
            path,
            against,
            ..
        } => GitAnswer::Diff {
            root: root.clone(),
            path: path.clone(),
            against: *against,
            outcome: Err(fault),
        },
        GitQuestion::CommitFiles { root, hash } => GitAnswer::CommitFiles {
            root: root.clone(),
            hash: hash.clone(),
            outcome: Err(fault),
        },
        GitQuestion::CompareFiles { root, a, b } => GitAnswer::CompareFiles {
            root: root.clone(),
            a: a.clone(),
            b: b.clone(),
            outcome: Err(fault),
        },
        GitQuestion::DiffRange {
            root, a, b, path, ..
        } => GitAnswer::DiffRange {
            root: root.clone(),
            a: a.clone(),
            b: b.clone(),
            path: path.clone(),
            outcome: Err(fault),
        },
        GitQuestion::Show {
            root, hash, path, ..
        } => GitAnswer::Show {
            root: root.clone(),
            hash: hash.clone(),
            path: path.clone(),
            outcome: Err(fault),
        },
        GitQuestion::Write { root, verb, paths } => GitAnswer::Write {
            root: root.clone(),
            verb: verb.clone(),
            paths: paths.clone(),
            outcome: Err(fault),
        },
        GitQuestion::Checkout { root, target, .. } => GitAnswer::Checkout {
            root: root.clone(),
            target: target.clone(),
            outcome: Err(fault),
        },
    }
}

/// `rev-parse --show-toplevel` answers in git's own path grammar — forward
/// slashes, even on Windows. The rest of this window spells Windows paths with
/// backslashes, and a repository root that disagrees with the Files column's own
/// foot about how to spell the same folder is one place with two names.
fn repo_root(text: &str) -> PathBuf {
    let trimmed = text.trim_end_matches(['\r', '\n']);
    if cfg!(windows) {
        PathBuf::from(trimmed.replace('/', "\\"))
    } else {
        PathBuf::from(trimmed)
    }
}

/// **Ask git one question. Runs on the worker.**
///
/// Split out and public so that a test can put a real question to the real `git`
/// in a real repository without a thread or an event loop between them — the same
/// split [`crate::files::read_directory`] has.
#[must_use]
pub fn answer(
    program: &Path,
    question: &GitQuestion,
    timeout: Duration,
    now_unix: i64,
) -> GitAnswer {
    match question {
        GitQuestion::RepoProbe { dir } => {
            let command = git_command(
                program,
                dir,
                &[OsStr::new("rev-parse"), OsStr::new("--show-toplevel")],
            );
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::Repo {
                    dir: dir.clone(),
                    outcome: Ok(repo_root(&String::from_utf8_lossy(&run.stdout))),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        GitQuestion::Status { root } => {
            let command = git_command(
                program,
                root,
                &[
                    OsStr::new("status"),
                    OsStr::new("--porcelain=v1"),
                    OsStr::new("-z"),
                    OsStr::new("--untracked-files=all"),
                    OsStr::new("--branch"),
                    OsStr::new("--ignore-submodules=none"),
                ],
            );
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::Status {
                    root: root.clone(),
                    outcome: Ok(parse_status(&run.stdout)),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        GitQuestion::Refs { root } => {
            let command = git_command(
                program,
                root,
                &[
                    OsStr::new("for-each-ref"),
                    // **The full ref name, not `%(refname:short)`.** The short
                    // spelling is what `%D` gives without `--decorate=full`, and
                    // it is unreadable for the same reason: `origin/main` is a
                    // remote-tracking ref in almost every repository and a local
                    // branch in the one where somebody made it, and nothing but
                    // the full name can tell those two apart. `split_ref_name`
                    // cuts the short name back off, which is the same cut git
                    // would have made and is now made where the kind is kept.
                    //
                    // **No `--sort`**: see [`parse_refs`] for why the order this
                    // list wants is not one git has a name for.
                    OsStr::new(
                        "--format=%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:track)%00%(HEAD)%00%(committerdate:iso-strict)",
                    ),
                    OsStr::new("refs/heads"),
                    OsStr::new("refs/remotes"),
                    OsStr::new("refs/tags"),
                ],
            );
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::Refs {
                    root: root.clone(),
                    outcome: Ok(parse_refs(&run.stdout, now_unix)),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        GitQuestion::Search { root, query } => {
            // Three processes, and [`GitQuestion::Search`] says why: git ANDs
            // `--grep` with `--author`, and a search field means OR.
            //
            // `rev-list` and not `log`: what is wanted is a list of hashes and
            // nothing else, and `rev-list` is `log`'s own plumbing for exactly
            // that — no format string to get wrong and no decoration to parse.
            let mut hits: Vec<String> = Vec::new();
            let mut push = |text: &str| {
                for line in text.lines() {
                    let line = line.trim();
                    if !line.is_empty() && !hits.iter().any(|held| held == line) {
                        hits.push(line.to_owned());
                    }
                }
            };
            // The direct jump first, so a pasted hash lands on its commit rather
            // than under whatever its text happens to match. `^{commit}` is
            // git's own way of saying "and it had better be a commit": without
            // it a branch name, a tag, or the word `HEAD` would all resolve, and
            // resolving `main` to its tip is a *jump* and not a search — which
            // is exactly what a reader typing a ref name wants.
            let revision = format!("{query}^{{commit}}");
            let verify = git_command(
                program,
                root,
                &[
                    OsStr::new("rev-parse"),
                    OsStr::new("--verify"),
                    OsStr::new("--quiet"),
                    OsStr::new(&revision),
                ],
            );
            // A query that resolves to nothing exits non-zero and says nothing,
            // which is the ordinary case and not a failure: `--quiet` is asked
            // for precisely so that "no such revision" is silence.
            if let Ok(run) = run_git(verify, timeout)
                && run.ok
            {
                push(&String::from_utf8_lossy(&run.stdout));
            }
            let cap = format!("--max-count={GIT_SEARCH_CAP}");
            let mut sweep = |flag: &str| -> Option<GitFault> {
                let matcher = format!("{flag}{query}");
                let command = git_command(
                    program,
                    root,
                    &[
                        OsStr::new("rev-list"),
                        OsStr::new("--all"),
                        OsStr::new("-i"),
                        OsStr::new(&matcher),
                        OsStr::new(&cap),
                    ],
                );
                match run_git(command, timeout) {
                    Ok(run) if run.ok => {
                        push(&String::from_utf8_lossy(&run.stdout));
                        None
                    }
                    Ok(run) => Some(classify_failure(&run.stderr)),
                    Err(fault) => Some(fault),
                }
            };
            // The message first and the author second, so the order the matches
            // are stepped through leads with what a search field is usually for.
            if let Some(fault) = sweep("--grep=").or_else(|| sweep("--author=")) {
                return faulted(question, fault);
            }
            GitAnswer::Search {
                root: root.clone(),
                query: query.clone(),
                outcome: Ok(hits),
            }
        }
        GitQuestion::Log {
            root,
            refs,
            skip,
            count,
        } => {
            // One more than the page, which is how `has_more` is known.
            let limit = format!("--max-count={}", count.saturating_add(1));
            let skipped = format!("--skip={skip}");
            let command = git_command(
                program,
                root,
                &[
                    OsStr::new("log"),
                    OsStr::new("--parents"),
                    OsStr::new("--topo-order"),
                    OsStr::new("-z"),
                    // `--decorate=full` is what makes `%D` say which *kind* of
                    // ref each name is — see [`parse_decoration`].
                    OsStr::new("--decorate=full"),
                    // `%ae` joined `%an` in v2 ① (2026-08-16). It is a *field of
                    // the same record*, not a second command: one more NUL and
                    // one more `chunks_exact` stride, which is why the page
                    // append invariant in `GitCache::accept` is untouched —
                    // that invariant is about how many commits a page holds and
                    // where it starts, and neither is a function of how many
                    // fields a commit is spelled with.
                    // `%cn`, `%ce` and `%b` joined the record in v2 ②
                    // (2026-08-16) for `%ae`'s own reason: the expanded row
                    // wants the whole message and the committer, and a second
                    // `git log` to learn them about a commit already on screen
                    // is a second reading of the same history (R31).
                    //
                    // **`%b` last, and that is load-bearing.** It is the one
                    // field that can contain newlines; putting it at the end of
                    // the record means the stride is still arithmetic — every
                    // separator before it is a NUL git wrote, and the one after
                    // it is `-z`'s own terminator. A commit message cannot
                    // contain a NUL (git refuses one), so no body can ever open
                    // a field this parse would count.
                    OsStr::new(
                        "--format=%H%x00%h%x00%an%x00%ae%x00%cn%x00%ce%x00%cI%x00%s%x00%P%x00%D%x00%b",
                    ),
                    OsStr::new(&limit),
                    OsStr::new(&skipped),
                ],
            );
            // The revisions git walks, in the position git puts them — last,
            // after every option (T2/T3). They are pushed rather than folded
            // into the array above because how many there are is the filter's
            // answer and not this arm's: "All branches" is one word, three
            // branches picked by hand are three.
            //
            // **No `--`.** Everything here is a rev argument or a rev pseudo-
            // argument (`--all`, `--branches`, `--tags`), and the separator
            // would tell git the words after it are *paths* — which is the one
            // reading that would turn a branch called `main` into a file called
            // `main` that does not exist.
            let mut command = command;
            command.args(refs);
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::Log {
                    root: root.clone(),
                    skip: *skip,
                    outcome: Ok(parse_log(&run.stdout, now_unix, *skip, *count)),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        GitQuestion::Diff {
            root,
            path,
            against,
            renamed_from,
        } => {
            let words = diff_arguments(*against, path, renamed_from.as_deref());
            let arguments: Vec<&OsStr> = words.iter().map(OsStr::new).collect();
            let command = git_command(program, root, &arguments);
            match run_git(command, timeout) {
                // **The one question here whose success is not `status.success()`.**
                // `--no-index` is a `diff` between two files and answers the
                // shell's own convention for that: 0 when they are the same, 1
                // when they are not. One is what an untracked file always gets,
                // because a file against `/dev/null` always differs, and reading
                // it as a failure would put git's silence on the page under a
                // refusal card. Anything else is still a fault, and still says
                // whatever git said.
                Ok(run) if run.ok || diff_differed(*against, &run) => GitAnswer::Diff {
                    root: root.clone(),
                    path: path.clone(),
                    against: *against,
                    outcome: Ok(String::from_utf8_lossy(&run.stdout).into_owned()),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        // **The one branch that writes**, and it builds nothing of its own: the
        // command line is [`write_arguments`]'s, so that what this process is
        // about to run is the same vector a test with no subprocess can read.
        GitQuestion::Write { root, verb, paths } => {
            let (words, input) = write_arguments(verb, paths);
            let arguments: Vec<&OsStr> = words.iter().map(OsStr::new).collect();
            let command = git_command(program, root, &arguments);
            match run_git_with_input(command, timeout, input) {
                Ok(run) if run.ok => GitAnswer::Write {
                    root: root.clone(),
                    verb: verb.clone(),
                    paths: paths.clone(),
                    outcome: Ok(()),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        // `--format=` empties the header git would otherwise print above the
        // patch: a `show` restricted to one path is being asked for a diff, and
        // the commit's subject, author and date are already on the row that was
        // pressed to get here.
        GitQuestion::Show {
            root,
            hash,
            path,
            renamed_from,
        } => {
            let mut arguments = vec![
                OsStr::new("show"),
                OsStr::new("--no-color"),
                OsStr::new("--format="),
                OsStr::new(hash),
                OsStr::new("--"),
            ];
            if let Some(from) = renamed_from {
                arguments.push(OsStr::new(from));
            }
            arguments.push(OsStr::new(path));
            let command = git_command(program, root, &arguments);
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::Show {
                    root: root.clone(),
                    hash: hash.clone(),
                    path: path.clone(),
                    outcome: Ok(String::from_utf8_lossy(&run.stdout).into_owned()),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        // The compare block's document — [`GitQuestion::Show`]'s shape with two
        // ends instead of one, and the same insistence on handing git **both
        // halves of a rename** for the reason written out on `GitQuestion::Diff`.
        GitQuestion::DiffRange {
            root,
            a,
            b,
            path,
            renamed_from,
        } => {
            let mut arguments = vec![OsStr::new("diff"), OsStr::new("--no-color"), OsStr::new(a)];
            if let Some(b) = b {
                arguments.push(OsStr::new(b));
            }
            arguments.push(OsStr::new("--"));
            if let Some(from) = renamed_from {
                arguments.push(OsStr::new(from));
            }
            arguments.push(OsStr::new(path));
            let command = git_command(program, root, &arguments);
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::DiffRange {
                    root: root.clone(),
                    a: a.clone(),
                    b: b.clone(),
                    path: path.clone(),
                    outcome: Ok(String::from_utf8_lossy(&run.stdout).into_owned()),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        // **One command and no cleverness** (R10). No `--force`, no `--merge`,
        // no stash around it: what git does with a dirty tree here is exactly
        // what it does in the pane next door, and the sentence it prints when it
        // declines is the sentence the panel shows.
        //
        // **No `--` either**, and that is not an omission: after `--` git reads
        // its argument as a *pathspec*, so `git checkout -- main` restores a
        // file called `main` from the index instead of moving onto the branch.
        // The disambiguator that would be safe here is the one that says which
        // kind of thing was meant, and for the only ambiguous case — a commit
        // whose abbreviation spells a branch name — that is `--detach`, which
        // this question already carries.
        GitQuestion::Checkout {
            root,
            target,
            detach,
        } => {
            let mut arguments = vec![OsStr::new("checkout")];
            if *detach {
                arguments.push(OsStr::new("--detach"));
            }
            arguments.push(OsStr::new(target));
            let command = git_command(program, root, &arguments);
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::Checkout {
                    root: root.clone(),
                    target: target.clone(),
                    outcome: Ok(()),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        GitQuestion::CommitFiles { root, hash } => {
            let command = git_command(
                program,
                root,
                &[
                    OsStr::new("show"),
                    OsStr::new("--no-color"),
                    // **`--raw` and not `--name-status`**, so that `--numstat`
                    // can stand beside it — see [`parse_diff_files`], which is
                    // where that ruling is written down.
                    OsStr::new("--raw"),
                    OsStr::new("--numstat"),
                    OsStr::new("-z"),
                    OsStr::new("--format="),
                    OsStr::new(hash),
                ],
            );
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::CommitFiles {
                    root: root.clone(),
                    hash: hash.clone(),
                    outcome: Ok(parse_diff_files(&run.stdout)),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        // **The same two blocks, between two places instead of across one
        // commit** (D6). It is `git diff` and not `git show` because a range has
        // two ends and `show` only ever has one; everything downstream of the
        // bytes — the parse, the rows, the badges, the counts — is shared, which
        // is the whole reason the answer carries the same `GitCommitFile`.
        //
        // `b` absent is the working tree, spelled the way git spells it: one
        // revision and no second, which git reads as "this commit against what
        // is on disk". Not `<a> HEAD`, which would be a different question with
        // the same name.
        GitQuestion::CompareFiles { root, a, b } => {
            let mut arguments = vec![
                OsStr::new("diff"),
                OsStr::new("--no-color"),
                OsStr::new("--raw"),
                OsStr::new("--numstat"),
                OsStr::new("-z"),
                OsStr::new(a),
            ];
            if let Some(b) = b {
                arguments.push(OsStr::new(b));
            }
            let command = git_command(program, root, &arguments);
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::CompareFiles {
                    root: root.clone(),
                    a: a.clone(),
                    b: b.clone(),
                    outcome: Ok(parse_diff_files(&run.stdout)),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
    }
}

// ── What one column knows ──────────────────────────────────────────────────

/// One answer's worth of a cache: never asked, asked, answered, refused.
///
/// The pending state is a state of the *cache* rather than of an answer — which
/// is why it is here and not in [`GitOutcome`] — and it is what stops a panel
/// asking the same question again on the next frame while the first is still in
/// flight. [`crate::files::DirNode`] draws the same distinction for the same
/// reason.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum GitSlot<T> {
    #[default]
    Idle,
    Pending,
    Ready(T),
    Failed(GitFault),
}

impl<T> GitSlot<T> {
    /// **A question that was asked and will never be answered was not asked**
    /// (F1b) — see [`GitCache::forget_pending`], the one caller.
    fn forget_pending(&mut self) {
        if matches!(self, Self::Pending) {
            *self = Self::Idle;
        }
    }

    #[must_use]
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }

    /// The reason there is no answer — G-2's empty states and its one-line
    /// banner (R17, W3).
    #[allow(dead_code)]
    #[must_use]
    pub fn fault(&self) -> Option<&GitFault> {
        match self {
            Self::Failed(fault) => Some(fault),
            _ => None,
        }
    }

    fn take(outcome: GitOutcome<T>) -> Self {
        match outcome {
            Ok(value) => Self::Ready(value),
            Err(fault) => Self::Failed(fault),
        }
    }
}

/// Everything one Files column knows about the repository under its root.
///
/// **Why it is not on the column's state.** The same split [`crate::files::DirCache`]
/// makes against `FilesLeafState`: that struct is the column's durable truth, it
/// crosses the disk, and it is read by clone; this is neither durable nor cheap
/// to clone, and folding it in would quietly copy a repository's history every
/// time somebody asked where a column is rooted.
///
/// **When it goes stale (R31).** Three moments and no others: the column is
/// re-rooted somewhere else, one of this window's own write verbs came back, or
/// **something happened that could have changed it** — a shell in this tab
/// finishing a command inside this repository ([`should_reread`]), the kernel
/// reporting a change under it ([`crate::git_watch`]), the window coming back to
/// the foreground, or a press on a refresh button. There is no timer here and no
/// polling: a repository is not read because time passed. The watcher is not a
/// counter-example and the distinction is the whole of why it is allowed — it
/// speaks only when the filesystem does.
#[derive(Clone, Debug, Default)]
pub struct GitCache {
    /// The folder this cache is about — the column's root, not the repository's.
    dir: Option<PathBuf>,
    repo: GitSlot<PathBuf>,
    status: GitSlot<GitStatus>,
    refs: GitSlot<Vec<GitRefEntry>>,
    log: GitSlot<GitLog>,
    /// Which roads the history is being walked down (T2/T3, v2 ③) — the graph
    /// filter's answer, in git's own rev grammar.
    ///
    /// **On the cache and not on the view**, because it is part of the question
    /// and the question is this module's. Every page of one history has to be
    /// asked with the same revisions — a "load more" that walked a different set
    /// than the page above it would append commits from a branch the page it
    /// extends does not contain — and `pending_questions` and `more_commits` are
    /// the two places those pages are built. Leaving the revisions in the seat's
    /// view state would mean handing them to both, which is two chances to hand
    /// them one that has just changed.
    ///
    /// Empty is `HEAD`, which is what it meant before there was a filter: see
    /// [`GitQuestion::Log::refs`].
    log_refs: Vec<String>,
    /// How many answers a re-read is still owed (T5, v2 ③) — see
    /// [`Self::begin_reread`].
    ///
    /// A count and not a flag, because the three questions come back separately
    /// and the head must stay quiet until the *last* of them does. Nothing about
    /// it is Pending: the answers already in the slots are still what the page is
    /// drawn from, which is the whole point of the verb.
    rereading: u8,
    /// The one search, keyed on the text it is about (T4).
    ///
    /// [`Self::commit_files`]'s shape and held for its reason: there is one
    /// search field on the page, so a map keyed by query would be a cache of
    /// answers to questions nobody is asking. The query lives beside the slot so
    /// that a result arriving after another letter was typed can be recognised
    /// as being about the wrong text and dropped.
    search: Option<(String, GitSlot<Vec<String>>)>,
    /// The one expanded commit's file list, and which commit it is about (R15).
    ///
    /// **One, because the page has one.** The accordion opens a single commit,
    /// so a map keyed by hash would be a cache of lists for commits that are
    /// shut — a repository's worth of answers kept warm for presses that may
    /// never come. Replacing it on every expansion is not forgetting anything a
    /// reader could still see.
    ///
    /// The *hash* lives here beside the answer rather than only in the column's
    /// view state, because an answer must be able to say which question it
    /// belongs to: a list that arrives after the user has opened a different
    /// commit is filed against the commit it is about, and drawn only if that is
    /// still the open one.
    commit_files: Option<(String, GitSlot<Vec<GitCommitFile>>)>,
    /// The one comparison, keyed on its pair (D6) — [`Self::commit_files`]'s
    /// twin, held for the same reason and thrown away the same way.
    ///
    /// One and not a map because there is one compare block on the page, and the
    /// key is the *pair* because moving either end is a different question with
    /// the same shape.
    compare_files: Option<(ComparePair, GitSlot<Vec<GitCommitFile>>)>,
    /// The paths a write is in flight for — **the whole of R13's pessimism**.
    ///
    /// A row whose path is in here is drawn dimmed and answers no verb, and it
    /// leaves only when git's receipt says so. The alternative the mock-up
    /// implements is to move the row between two arrays on the click and hope;
    /// what that shows during the eighty milliseconds a `git add` takes is a
    /// staged file, and what it shows if the index was locked is a staged file
    /// that is not staged, with no moment at which anything says otherwise.
    ///
    /// A set of paths rather than a flag on the panel because two writes can be
    /// in flight at once — a group's "stage all" while one row's `+` is still
    /// running — and a flag could only unpend both when the first came back.
    pending_writes: std::collections::BTreeSet<String>,
    /// The **refs** a named write is in flight for (v2 ④) — `pending_writes`'
    /// twin, and a second set rather than a second kind of key in the first one.
    ///
    /// Two sets because they hold two different alphabets. A repo-relative path
    /// and a branch name are both strings and a repository may perfectly well
    /// contain a file called `main`; one set would make deleting the branch dim
    /// the file's row, which is a wrong answer that would only ever show up on
    /// somebody else's repository.
    pending_refs: std::collections::BTreeSet<String>,
    /// The ref a checkout is in flight for (R10) — the branch row that is dimmed
    /// while git works, and the guard that makes a second click on it do
    /// nothing rather than start a second `git checkout`.
    checkout: Option<String>,
    /// What this cache is for, and therefore what it asks (R31).
    role: GitRole,
}

/// Which surface a cache feeds, and therefore which questions it owes.
///
/// **A distinction about work and not about types.** The graph and the panel
/// hold the same four answers in the same four slots; what differs is that the
/// graph has no branch list on it, and a cache that asked for one anyway would
/// spend a `for-each-ref` per graph opened on an answer nothing draws. R31 is
/// the whole reason this exists: a repository is read for a surface that is
/// looking at it, and only for what that surface shows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GitRole {
    /// The Git page in a Files column: status, branches and history.
    #[default]
    Page,
    /// The commit graph document (G-4): history, and the head to name it by.
    Graph,
}

/// **R31's third invalidation moment, as one predicate** — should the repository
/// a surface is showing be read again, because a shell standing inside it just
/// finished a command?
///
/// Two conditions, and the second is not the first restated. A surface nobody is
/// looking at is never read, whatever happens in any shell: that is the same gate
/// [`crate::columns_wanting_git`] keeps for the first reading, and it is kept
/// again here because "something changed" is not on its own a reason to spend a
/// subprocess. And the command has to have happened *in* this repository — a
/// `cargo build` in an unrelated tree is not news about this one.
///
/// `pane_cwd` is an `Option` because it genuinely is one: a shell that has never
/// reported over OSC 7 has no folder this window knows about, and the honest
/// answer for it is *no* rather than a guess at where it might be standing.
///
/// A free function taking the facts rather than a method, for
/// [`crate::columns_wanting_git`]'s reason exactly: the promise is about what is
/// *not* read, and a promise that can only be checked by starting a window is a
/// promise nothing checks.
#[must_use]
pub fn should_reread(root: &Path, pane_cwd: Option<&Path>, page_showing: bool) -> bool {
    page_showing && pane_cwd.is_some_and(|cwd| stands_inside(root, cwd))
}

/// Is `path` the folder `root`, or somewhere under it?
///
/// **Component by component, never by string prefix.** `C:\a\bc` starts with the
/// characters of `C:\a\b` and is not inside it, and a trigger that read it as one
/// would re-read a repository every time a command finished in the directory
/// *next door* to it — a wrong answer that only ever shows up on somebody else's
/// disk. Case-insensitively on Windows, on [`same_step`]'s own reasoning: `d:\repo`
/// and `D:\repo` are one place.
#[must_use]
pub fn stands_inside(root: &Path, path: &Path) -> bool {
    let mut steps = path.components();
    for want in root.components() {
        match steps.next() {
            Some(have) if same_step(want.as_os_str(), have.as_os_str()) => {}
            _ => return false,
        }
    }
    true
}

/// Whether two path components name the same thing.
///
/// The one place this platform's case rule is written down for a path, so a tree
/// that drew no badges and a trigger that never fired cannot disagree about
/// whether `D:` and `d:` are the same drive.
#[must_use]
pub fn same_step(want: &std::ffi::OsStr, have: &std::ffi::OsStr) -> bool {
    if cfg!(windows) {
        want.to_string_lossy()
            .eq_ignore_ascii_case(&have.to_string_lossy())
    } else {
        want == have
    }
}

impl GitCache {
    /// A cache for a repository whose root is already known.
    ///
    /// The graph is opened *from* a page that has already probed, so the probe
    /// is an answer it has rather than a question it owes — and starting one
    /// would blank the document's own heading for the length of a subprocess to
    /// re-learn a path it was handed.
    #[must_use]
    pub fn at_root(root: PathBuf, role: GitRole) -> Self {
        Self {
            dir: Some(root.clone()),
            repo: GitSlot::Ready(root),
            role,
            ..Self::default()
        }
    }
    /// Point this cache at a folder, forgetting everything if it is a different
    /// one. Answers whether anything was forgotten.
    pub fn retarget(&mut self, dir: &Path) -> bool {
        if self.dir.as_deref() == Some(dir) {
            return false;
        }
        *self = Self {
            dir: Some(dir.to_owned()),
            // What a cache is *for* survives what it is *about*: a column that
            // moves to another folder is still a page.
            role: self.role,
            ..Self::default()
        };
        true
    }

    /// Ask everything again, keeping the answers on screen until the new ones
    /// arrive.
    ///
    /// The repository root is deliberately kept: it is the one answer that a
    /// commit or a checkout cannot change, and re-probing for it would blank the
    /// whole page — including its heading — for the length of a subprocess, every
    /// time a file was staged.
    ///
    /// Called by G-2 after every write verb — staging, unstaging, a checkout —
    /// and by the manual refresh. It is the whole of R31's second invalidation
    /// moment: nothing else in this module ever decides on its own that what it
    /// knows has gone stale.
    #[allow(dead_code)]
    pub fn refresh(&mut self) {
        self.status = GitSlot::Idle;
        self.refs = GitSlot::Idle;
        self.log = GitSlot::Idle;
        self.rereading = 0;
    }

    /// **Read it all again, without taking the page down** (T5, v2 ③).
    ///
    /// [`Self::refresh`]'s twin, and the difference between them is the whole
    /// reason there are two. That one *forgets*: it is what a write owes, because
    /// after a `git add` what is on screen is no longer true and the honest thing
    /// to show while the truth is being fetched is that it is being fetched. This
    /// one *re-asks*: it is what the toolbar's button owes, and what is on screen
    /// when a reader presses it is still perfectly true — it may simply have got
    /// older. Blanking a whole history to a `Reading the repository…` for the
    /// length of three subprocesses, in answer to a press that meant "check", is
    /// the page punishing the reader for asking.
    ///
    /// So the answers stay in their slots and the *count of answers still owed*
    /// goes up. Nothing is Pending, so [`Self::pending_questions`] keeps
    /// answering "nothing to ask" and cannot flood; the count is what
    /// [`Self::rereading`] reports, and it is the whole of what makes the head go
    /// quiet.
    ///
    /// **The history is re-asked to the depth it is currently showing**, which is
    /// [`Self::reread_depth`]. A first page answers a `skip: 0` question by
    /// *replacing* the list — that is what makes a rewritten history land
    /// honestly — so a re-read that asked for one page would throw away every
    /// page the reader had paged in, drop the list from five hundred rows to
    /// fifty under their scroll, and then crawl back up it fifty at a time. Under
    /// a repository something else is writing to, that is not a refresh: it is
    /// the page collapsing and rebuilding itself every couple of seconds.
    pub fn begin_reread(&mut self) -> Vec<GitQuestion> {
        let Some(root) = self.repo.ready().cloned() else {
            return Vec::new();
        };
        let questions = vec![
            GitQuestion::Status { root: root.clone() },
            GitQuestion::Refs { root: root.clone() },
            GitQuestion::Log {
                root,
                refs: self.log_refs.clone(),
                skip: 0,
                count: self.reread_depth(),
            },
        ];
        // Set rather than added to: a second press while the first is still out
        // is the same press, and it is owed the same three answers.
        self.rereading = u8::try_from(questions.len()).unwrap_or(u8::MAX);
        questions
    }

    /// **How much history a re-read asks for**: everything that is loaded, and
    /// never less than one page.
    ///
    /// One page is what a *first* reading asks for, because nobody has scrolled
    /// yet. Every reading after that is a reading of a document the reader has
    /// already made as long as they wanted it, and the length of what is on
    /// screen is not something a refresh is entitled to change — a page that came
    /// back shorter than it went out is a page that lost rows the reader was
    /// looking at.
    #[must_use]
    fn reread_depth(&self) -> usize {
        self.log
            .ready()
            .map_or(GIT_LOG_PAGE, |log| log.commits.len().max(GIT_LOG_PAGE))
    }

    /// **The same three questions, unless the last set is still owed** — what an
    /// invalidation moment nobody asked for out loud gets (R31's third moment).
    ///
    /// [`Self::begin_reread`]'s guarded twin, and the guard is the whole
    /// difference. A press on a refresh button is a person saying *now*, and it
    /// is answered every time it is made. A command ending and a window coming
    /// back are not gestures at this page at all: they arrive in bursts — ten
    /// commands pasted into a shell end ten times — and a burst that started ten
    /// re-reads would spend thirty subprocesses to learn what the first three
    /// were already on their way to say. So while anything is still owed this
    /// asks nothing and answers with an empty list, which is also how a caller
    /// knows there is nothing to tell the worker about.
    ///
    /// The predicate is [`Self::reading`] and not the re-read counter alone: the
    /// very first reading of a page counts too. A `Pending` status is a question
    /// already out for exactly the answer this would ask for again.
    pub fn begin_reread_unless_owed(&mut self) -> Vec<GitQuestion> {
        if self.reading() {
            return Vec::new();
        }
        self.begin_reread()
    }

    /// Whether a re-read is still out (T5) — what draws the head quietly.
    #[must_use]
    pub fn rereading(&self) -> bool {
        self.rereading > 0
    }

    /// **Is anything about this repository still on its way?** (T5)
    ///
    /// "Is any of the three still coming" rather than "was refresh pressed",
    /// because the same three questions are asked by a checkout, by a filter
    /// change, by the first frame of a page and by R31's third moment — and a
    /// reader watching a head that only went quiet when *they* pressed refresh
    /// would be being told the other three had already finished.
    ///
    /// One derivation with three readers: the graph's head, the panel's head, and
    /// [`Self::begin_reread_unless_owed`]'s guard against stacking. It was the
    /// graph's own local `busy` until the panel needed the same sentence — and
    /// the guard needed exactly the same one, which is what settled where it
    /// belongs.
    #[must_use]
    pub fn reading(&self) -> bool {
        self.rereading()
            || matches!(self.status, GitSlot::Pending)
            || matches!(self.refs, GitSlot::Pending)
            || matches!(self.log, GitSlot::Pending)
    }

    /// One of a re-read's three answers has landed.
    fn reread_answered(&mut self) {
        self.rereading = self.rereading.saturating_sub(1);
    }

    #[must_use]
    pub fn root(&self) -> Option<&Path> {
        self.repo.ready().map(PathBuf::as_path)
    }

    // The four readers the Git page is drawn from (G-2). Each hands back the
    // whole slot rather than its contents, because "not asked yet", "asked",
    // "here it is" and "here is why not" are four different rows and a page that
    // could only see the third would draw an empty list for all four.
    #[allow(dead_code)]
    #[must_use]
    pub fn repo(&self) -> &GitSlot<PathBuf> {
        &self.repo
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn status(&self) -> &GitSlot<GitStatus> {
        &self.status
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn refs(&self) -> &GitSlot<Vec<GitRefEntry>> {
        &self.refs
    }

    /// Which revisions the history is being walked down (T2/T3).
    ///
    /// Read by the tests that pin `set_log_refs`' contract; the window itself
    /// only ever *writes* it, because what the filter is is the seat's own state
    /// and this is where the question is built from.
    #[allow(dead_code)]
    #[must_use]
    pub fn log_refs(&self) -> &[String] {
        &self.log_refs
    }

    /// Walk a different set of roads, and say whether anything changed.
    ///
    /// **Throws the history away when it does.** A filter is not a view of the
    /// commits already loaded — a branch that was excluded was never walked, so
    /// there is nothing on hand to reveal — which makes this the third and last
    /// of R31's invalidation moments and the only one a *reader's* gesture
    /// reaches directly.
    pub fn set_log_refs(&mut self, refs: Vec<String>) -> bool {
        if self.log_refs == refs {
            return false;
        }
        self.log_refs = refs;
        self.log = GitSlot::Idle;
        true
    }

    /// The one search's answer, when it is about this text (T4).
    #[must_use]
    pub fn search(&self, query: &str) -> Option<&GitSlot<Vec<String>>> {
        self.search
            .as_ref()
            .filter(|(asked, _)| asked == query)
            .map(|(_, slot)| slot)
    }

    /// Ask git which commits match, and hand back the question to send.
    ///
    /// Asked at the moment of the press rather than derived by
    /// [`Self::pending_questions`], for [`Self::begin_commit_files`]'s reason: a
    /// search is something a *gesture* wants, and a question the paint could
    /// re-derive is a question the paint would re-ask on every frame the field
    /// was empty of results.
    pub fn begin_search(&mut self, query: &str) -> Option<GitQuestion> {
        let root = self.repo.ready()?.clone();
        self.search = Some((query.to_owned(), GitSlot::Pending));
        Some(GitQuestion::Search {
            root,
            query: query.to_owned(),
        })
    }

    /// Forget the search — what `Esc` in the field leaves behind.
    pub fn clear_search(&mut self) {
        self.search = None;
    }

    #[must_use]
    pub fn log(&self) -> &GitSlot<GitLog> {
        &self.log
    }

    /// Whether a write is in flight for this path (R13).
    #[must_use]
    pub fn write_pending(&self, path: &str) -> bool {
        self.pending_writes.contains(path)
    }

    /// Whether a named write is in flight for this ref (v2 ④) —
    /// [`Self::write_pending`]'s twin for a branch or a tag.
    #[must_use]
    pub fn ref_write_pending(&self, name: &str) -> bool {
        self.pending_refs.contains(name)
    }

    /// Build the write, and dim its rows.
    ///
    /// Returns `None` — and starts nothing — when there is no repository, when
    /// the subject is empty, or when that subject is already being written to.
    /// The last is the guard that makes a double click on `+` one `git add` and
    /// not two: the second finds its own path pending and declines, rather than
    /// racing the first for `index.lock`.
    ///
    /// **A named verb is guarded the same way against its own ref** (v2 ④), and
    /// carries no pathspec: `git branch -d main` and `git add` are the same shape
    /// of promise — one process, one receipt, the row that asked it dimmed until
    /// the receipt arrives — and the only thing that differs is which alphabet
    /// the subject is spelled in. A named verb handed paths, or a pathspec verb
    /// handed none, is a caller that has confused the two and starts nothing.
    #[must_use]
    pub fn begin_write(&mut self, verb: GitWriteVerb, paths: Vec<String>) -> Option<GitQuestion> {
        let root = self.repo.ready()?.clone();
        if let Some(name) = verb.ref_subject() {
            if !paths.is_empty() || name.is_empty() || self.pending_refs.contains(name) {
                return None;
            }
            self.pending_refs.insert(name.to_owned());
            return Some(GitQuestion::Write { root, verb, paths });
        }
        if paths.is_empty() || paths.iter().any(|path| self.pending_writes.contains(path)) {
            return None;
        }
        self.pending_writes.extend(paths.iter().cloned());
        Some(GitQuestion::Write { root, verb, paths })
    }

    /// Whether this repository already has a local branch of this name.
    ///
    /// Read before a `Checkout as local branch` is issued (M10): `git checkout -b
    /// <name>` on a name that is taken is a refusal, and what the reader meant by
    /// pressing that row is "put me on that branch". An unanswered ref list says
    /// `false`, which sends the press down the creating path and lets git — which
    /// does know — be the one that refuses it.
    #[must_use]
    pub fn has_local_branch(&self, name: &str) -> bool {
        self.refs.ready().is_some_and(|refs| {
            refs.iter()
                .any(|entry| entry.kind == GitRefKind::Local && entry.name == name)
        })
    }

    /// Which ref a checkout is in flight for (R10).
    #[must_use]
    pub fn checkout_pending(&self) -> Option<&str> {
        self.checkout.as_deref()
    }

    /// Build the checkout, and dim the row it is about (R10).
    ///
    /// Returns `None` — and starts nothing — when there is no repository, or
    /// when a checkout is already running. **One at a time and not one per
    /// click**: two `git checkout` processes racing over one working tree is two
    /// ways to end up somewhere neither of them was asked for, and the second
    /// click is almost always the first one arriving twice.
    ///
    /// There is no confirmation in front of this. R10 ruled a checkout a
    /// *browsing* verb: nothing is destroyed by standing somewhere else, and git
    /// itself refuses the one case where something would be — which is the whole
    /// of the failure design, printed in git's own words by the banner.
    #[must_use]
    pub fn begin_checkout(&mut self, target: String, detach: bool) -> Option<GitQuestion> {
        let root = self.repo.ready()?.clone();
        if self.checkout.is_some() {
            return None;
        }
        self.checkout = Some(target.clone());
        Some(GitQuestion::Checkout {
            root,
            target,
            detach,
        })
    }

    /// What this cache still needs to ask to be complete.
    ///
    /// The whole driver, in one place: probe first, and only once there is a
    /// repository, the three questions a page is made of. Deriving it from the
    /// cache rather than remembering a plan is what makes it safe to call on
    /// every frame — a slot that is `Pending` or answered asks for nothing, so
    /// this is idempotent by construction and R31's "no polling" holds even
    /// though it is consulted at paint.
    #[must_use]
    pub fn pending_questions(&self) -> Vec<GitQuestion> {
        let Some(dir) = self.dir.as_ref() else {
            return Vec::new();
        };
        if matches!(self.repo, GitSlot::Idle) {
            return vec![GitQuestion::RepoProbe { dir: dir.clone() }];
        }
        let Some(root) = self.repo.ready() else {
            return Vec::new();
        };
        let mut questions = Vec::new();
        if matches!(self.status, GitSlot::Idle) {
            questions.push(GitQuestion::Status { root: root.clone() });
        }
        // **Both roles ask for the refs since v2 ③.** The note that used to
        // stand here said the graph had no branch list on it and would be
        // spending a `for-each-ref` on an answer nothing drew. It has one now —
        // the filter menu lists every local branch by name — so the reading is
        // no longer unread, and R31's rule is that a question is owed when
        // something draws its answer, not that the graph is cheap by habit.
        if matches!(self.refs, GitSlot::Idle) {
            questions.push(GitQuestion::Refs { root: root.clone() });
        }
        if matches!(self.log, GitSlot::Idle) {
            questions.push(GitQuestion::Log {
                root: root.clone(),
                refs: self.log_refs.clone(),
                skip: 0,
                count: GIT_LOG_PAGE,
            });
        }
        questions
    }

    /// The next page of history, when there is one and the list wants it (R16) —
    /// what G-2's "Load more" row asks for, and the reason that row is drawn.
    /// **What one commit touched** (R15) — the accordion's question, and the
    /// slot it will land in.
    ///
    /// Asked at the moment of the press rather than derived by
    /// [`Self::pending_questions`], for [`Self::more_commits`]'s reason: it is
    /// not something a complete page needs, it is something a *gesture* wants,
    /// and a question the paint could re-derive is a question the paint would
    /// re-ask every frame the list was empty.
    #[must_use]
    pub fn begin_commit_files(&mut self, hash: &str) -> Option<GitQuestion> {
        let root = self.repo.ready()?.clone();
        self.commit_files = Some((hash.to_owned(), GitSlot::Pending));
        Some(GitQuestion::CommitFiles {
            root,
            hash: hash.to_owned(),
        })
    }

    /// The expanded commit's files, when the expansion asked about is the one
    /// this cache holds an answer for.
    #[must_use]
    pub fn commit_files(&self, hash: &str) -> Option<&GitSlot<Vec<GitCommitFile>>> {
        match &self.commit_files {
            Some((held, slot)) if held == hash => Some(slot),
            _ => None,
        }
    }

    /// **What two places differ by** (D6) — the compare block's question, and
    /// the slot it will land in.
    ///
    /// Returns `None` — and starts nothing — when this pair is already the one
    /// in hand, which is what makes the block cost one subprocess per pair and
    /// not one per frame: the paint asks on every build, and the second ask
    /// finds its own pair already recorded and declines. [`Self::begin_commit_files`]
    /// does not need that guard because the gesture that calls it happens once;
    /// this one is reached from a *derivation*, so the guard is where the
    /// derivation is.
    #[must_use]
    pub fn begin_compare_files(&mut self, a: &str, b: Option<&str>) -> Option<GitQuestion> {
        let root = self.repo.ready()?.clone();
        let pair = (a.to_owned(), b.map(str::to_owned));
        if self
            .compare_files
            .as_ref()
            .is_some_and(|(held, _)| *held == pair)
        {
            return None;
        }
        self.compare_files = Some((pair, GitSlot::Pending));
        Some(GitQuestion::CompareFiles {
            root,
            a: a.to_owned(),
            b: b.map(str::to_owned),
        })
    }

    /// The comparison's files, when the pair asked about is the one this cache
    /// holds an answer for.
    #[must_use]
    pub fn compare_files(&self, a: &str, b: Option<&str>) -> Option<&GitSlot<Vec<GitCommitFile>>> {
        match &self.compare_files {
            Some(((held_a, held_b), slot))
                if held_a == a && held_b.as_deref() == b.map(str::as_ref) =>
            {
                Some(slot)
            }
            _ => None,
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn more_commits(&self) -> Option<GitQuestion> {
        let root = self.repo.ready()?;
        let log = self.log.ready()?;
        log.has_more.then(|| GitQuestion::Log {
            root: root.clone(),
            // The same roads the page above it was walked down — see
            // [`Self::log_refs`].
            refs: self.log_refs.clone(),
            skip: log.skip + log.commits.len(),
            count: GIT_LOG_PAGE,
        })
    }

    /// Record that a question has been sent, so that it is not sent again.
    /// **Give up on every question that is still out** (F1b, `plan.md` v4 增补
    /// ②) — [`crate::files::DirCache::forget_pending`]'s twin, for the same
    /// reason and at the same moment.
    ///
    /// The pane this cache belongs to has been given a new [`crate::LeafId`], so
    /// every answer in flight is addressed to a surface that no longer exists and
    /// will be dropped where it lands. `Pending` therefore goes back to `Idle`,
    /// which is what [`Self::pending_questions`] reads as "owed", and the three
    /// one-shot ledgers are cleared so the rows they dim can be pressed again.
    ///
    /// **Only the pending slots.** An answer already in hand stays: a column that
    /// merely changed tabs must not blink back to "Loading …" and re-run the
    /// `git status` that told it where it is standing.
    pub fn forget_pending(&mut self) {
        self.repo.forget_pending();
        self.status.forget_pending();
        self.refs.forget_pending();
        self.log.forget_pending();
        self.pending_writes.clear();
        self.pending_refs.clear();
        self.checkout = None;
    }

    pub fn mark_pending(&mut self, question: &GitQuestion) {
        match question {
            GitQuestion::RepoProbe { .. } => self.repo = GitSlot::Pending,
            GitQuestion::Status { .. } => self.status = GitSlot::Pending,
            GitQuestion::Refs { .. } => self.refs = GitSlot::Pending,
            // Only the first page owns the slot: a "load more" leaves the page
            // already on screen exactly where it is, because it is not being
            // replaced by anything.
            GitQuestion::Log { skip: 0, .. } => self.log = GitSlot::Pending,
            // A write's own "already asked" bookkeeping is `pending_writes`,
            // written by `begin_write` at the moment the question is built.
            // An expansion's own "already asked" bookkeeping is written by
            // `begin_commit_files`, at the moment the question is built.
            // A checkout's own "already asked" bookkeeping is `checkout`,
            // written by `begin_checkout` at the moment the question is built.
            // A comparison's own "already asked" bookkeeping is written by
            // `begin_compare_files`, at the moment the question is built.
            // A search's own "already asked" bookkeeping is written by
            // `begin_search`, at the moment the question is built.
            GitQuestion::Search { .. }
            | GitQuestion::Log { .. }
            | GitQuestion::Diff { .. }
            | GitQuestion::Show { .. }
            | GitQuestion::DiffRange { .. }
            | GitQuestion::CommitFiles { .. }
            | GitQuestion::CompareFiles { .. }
            | GitQuestion::Write { .. }
            | GitQuestion::Checkout { .. } => {}
        }
    }

    /// File an answer, and say whether it was still wanted.
    ///
    /// An answer about a repository this column is no longer looking at is
    /// dropped rather than filed — that is the cancellation, arriving late, and
    /// the check is what stops a re-rooted column filling in with the old
    /// repository's branches.
    pub fn accept(&mut self, answer: GitAnswer) -> bool {
        match answer {
            GitAnswer::Repo { dir, outcome } => {
                if self.dir.as_deref() != Some(dir.as_path()) {
                    return false;
                }
                self.repo = GitSlot::take(outcome);
                true
            }
            GitAnswer::Status { root, outcome } => {
                if self.root() != Some(root.as_path()) {
                    return false;
                }
                self.status = GitSlot::take(outcome);
                self.reread_answered();
                true
            }
            GitAnswer::Refs { root, outcome } => {
                if self.root() != Some(root.as_path()) {
                    return false;
                }
                self.refs = GitSlot::take(outcome);
                self.reread_answered();
                true
            }
            GitAnswer::Search {
                root,
                query,
                outcome,
            } => {
                if self.root() != Some(root.as_path()) {
                    return false;
                }
                // An answer about text the reader has already typed past is
                // dropped rather than filed — the same cancellation-by-subject
                // every other arm here performs, with the query standing where a
                // root or a hash stands.
                let Some((asked, slot)) = self.search.as_mut() else {
                    return false;
                };
                if *asked != query {
                    return false;
                }
                *slot = GitSlot::take(outcome);
                true
            }
            GitAnswer::Log {
                root,
                skip,
                outcome,
            } => {
                if self.root() != Some(root.as_path()) {
                    return false;
                }
                match (skip, outcome) {
                    (0, outcome) => {
                        self.log = GitSlot::take(outcome);
                        self.reread_answered();
                    }
                    // A later page extends the list rather than replacing it, and
                    // only if it starts where the list currently ends: a page that
                    // does not is an answer to a question about a history that has
                    // since been rewritten under us.
                    (skip, Ok(page)) => {
                        let GitSlot::Ready(log) = &mut self.log else {
                            return false;
                        };
                        if log.skip + log.commits.len() != skip {
                            return false;
                        }
                        log.commits.extend(page.commits);
                        log.has_more = page.has_more;
                    }
                    // A failed extra page leaves the pages already read alone —
                    // losing fifty commits because the fifty-first would not load
                    // is a worse answer than the fifty.
                    (_, Err(_)) => return false,
                }
                true
            }
            // **The receipt** (R13). Two things happen and their order is the
            // point: the rows stop being dimmed, and — only on success —
            // everything is asked again. Re-asking on a *failure* would be
            // spending three subprocesses to re-learn a status that by definition
            // did not change.
            //
            // The refusal itself is no longer filed here (toast ruling,
            // 2026-08-16). It used to be kept as `write_error` until the *next*
            // attempt, which the panel printed as a red strip across its own top
            // — a page holding a grudge about something that had already
            // finished happening. The runtime raises a notice straight off this
            // answer instead, so git's sentence is said once, over the column
            // that asked, and then leaves on its own.
            GitAnswer::Write {
                root,
                verb,
                paths,
                outcome,
            } => {
                if self.root() != Some(root.as_path()) {
                    return false;
                }
                for path in &paths {
                    self.pending_writes.remove(path);
                }
                // The named verbs unpend the ref they were about, in the same
                // breath and by the same rule (v2 ④): the row stops being dimmed
                // when git's receipt arrives and never before.
                if let Some(name) = verb.ref_subject() {
                    self.pending_refs.remove(name);
                }
                if outcome.is_ok() {
                    self.refresh();
                }
                true
            }
            // **A commit's file list is rows, not a document** (R15). It is
            // filed here beside the status for the same reason the status is
            // here: it is thrown away when the column re-roots, it is drawn and
            // never read, and it has no body. A list that arrives about a commit
            // that is no longer the open one is dropped, which is the accordion's
            // cancellation arriving late.
            GitAnswer::CommitFiles {
                root,
                hash,
                outcome,
            } => {
                if self.root() != Some(root.as_path()) {
                    return false;
                }
                let Some((held, slot)) = &mut self.commit_files else {
                    return false;
                };
                if *held != hash {
                    return false;
                }
                *slot = GitSlot::take(outcome);
                true
            }
            // The comparison's list, filed exactly as an expansion's is — and
            // dropped exactly as one is when the far end has moved since.
            GitAnswer::CompareFiles {
                root,
                a,
                b,
                outcome,
            } => {
                if self.root() != Some(root.as_path()) {
                    return false;
                }
                let Some((held, slot)) = &mut self.compare_files else {
                    return false;
                };
                if *held != (a, b) {
                    return false;
                }
                *slot = GitSlot::take(outcome);
                true
            }
            // **The checkout's receipt** (R10), and the asymmetry with a write's
            // is the ruling: a checkout that succeeded changes the branch, the
            // status and the whole history the page is drawn from, so everything
            // is asked again; one that git refused changed *nothing at all* —
            // that is what "refused" means here — so the page it was asked from
            // is still true, and all that is added to it is git's sentence.
            GitAnswer::Checkout {
                root,
                target,
                outcome,
            } => {
                if self.root() != Some(root.as_path()) {
                    return false;
                }
                if self.checkout.as_deref() != Some(target.as_str()) {
                    return false;
                }
                self.checkout = None;
                if outcome.is_ok() {
                    self.refresh();
                }
                true
            }
            // Diffs and file histories are documents, not column state: they
            // belong to the preview pool, keyed by their own `PreviewSource`.
            // Nothing here has a slot for them, and inventing one would give the
            // same document two homes.
            GitAnswer::Diff { .. } | GitAnswer::Show { .. } | GitAnswer::DiffRange { .. } => false,
        }
    }
}

/// One line for a write that would not go through — **the words a notice
/// carries** (toast ruling, 2026-08-16).
///
/// git's own sentence wherever there is one, and this module's only wording
/// otherwise — a killed child and a missing executable have no sentence of their
/// own to pass through, so the two of them are the whole of what is written here.
/// [`GitFault::NotARepository`] cannot reach this function: a write is only
/// offered from a page that already found a repository.
///
/// Public since the ruling, because the caller moved rather than the words: it
/// used to be read by the cache filing a refusal, and it is now read by the
/// runtime raising a toast off the same answer.
#[must_use]
pub fn write_refusal(fault: &GitFault) -> String {
    match fault {
        GitFault::Refused(words) => words.clone(),
        GitFault::GitMissing(words) => words.clone(),
        GitFault::TimedOut => "git did not answer and was stopped".to_owned(),
        GitFault::NotARepository => "the repository is no longer there".to_owned(),
    }
}

// ── The worker ─────────────────────────────────────────────────────────────

/// Newest-per-target queue, the shape [`crate::files::DirRequest`] already gives
/// the directory lane.
///
/// Asking the repository twice because a page was opened, closed and opened
/// again while a cold `status` was still running is work nobody is waiting for,
/// and the older answer can only ever be the staler one.
#[derive(Default)]
struct PendingGitRequests {
    requests: std::collections::VecDeque<GitRequest>,
}

impl PendingGitRequests {
    fn push_latest(&mut self, request: GitRequest) {
        if let Some(index) = self
            .requests
            .iter()
            .position(|queued| queued.same_target(&request))
        {
            self.requests.remove(index);
        }
        self.requests.push_back(request);
    }

    fn pop_front(&mut self) -> Option<GitRequest> {
        self.requests.pop_front()
    }

    fn contains_target(&self, request: &GitRequest) -> bool {
        self.requests
            .iter()
            .any(|queued| queued.same_target(request))
    }

    fn drain_channel(&mut self, receiver: &mpsc::Receiver<GitRequest>) {
        while let Ok(request) = receiver.try_recv() {
            self.push_latest(request);
        }
    }
}

/// Serve git questions, newest question per target first.
///
/// Split from [`GitWorker::spawn`] so the coalescing can be tested without a
/// repository or an event loop, exactly as `run_dir_worker` is.
fn run_git_worker(receiver: mpsc::Receiver<GitRequest>, mut execute: impl FnMut(GitRequest)) {
    let mut pending = PendingGitRequests::default();
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

/// Seconds since the epoch, for the relative-time table.
fn now_unix() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(delta) => i64::try_from(delta.as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

/// The thread, and the two ends of the conversation with it.
pub struct GitWorker {
    requests: mpsc::Sender<GitRequest>,
    pub responses: mpsc::Receiver<GitResponse>,
}

impl GitWorker {
    /// Start the thread. **Where `git.exe` is, is decided here, once.**
    ///
    /// On the worker rather than at startup because it is a `PATH` walk and a
    /// handful of `is_file` probes, and the main thread has a window to open. Once
    /// rather than per question, and never retried, for the reason `WslProbe`
    /// gives about `wsl.exe`: a machine does not grow a git while a window is
    /// open, and asking again on every question would spend a `PATH` walk to
    /// re-learn the same "no".
    pub fn spawn(proxy: EventLoopProxy<AppEvent>) -> Result<Self> {
        let (request_tx, request_rx) = mpsc::channel::<GitRequest>();
        let (response_tx, response_rx) = mpsc::channel::<GitResponse>();
        bt_platform::spawn_at_priority(
            "bt-git-worker",
            bt_platform::ThreadPriority::BelowNormal,
            move || {
                let program = crate::profiles::find_git(&bt_pty::SystemShellEnvironment);
                run_git_worker(request_rx, |request| {
                    let answer = match program.as_deref() {
                        Some(program) => {
                            answer(program, &request.question, GIT_COMMAND_TIMEOUT, now_unix())
                        }
                        None => faulted(
                            &request.question,
                            GitFault::GitMissing(git_not_found().to_owned()),
                        ),
                    };
                    if response_tx
                        .send(GitResponse {
                            window: request.window,
                            host: request.host,
                            answer,
                        })
                        .is_ok()
                    {
                        let _ = proxy.send_event(AppEvent::GitReady);
                    }
                });
            },
        )
        .context("spawn git worker")?;
        Ok(Self {
            requests: request_tx,
            responses: response_rx,
        })
    }

    /// Ask, reporting whether the worker was still there to be asked.
    #[must_use]
    pub fn request(&self, request: GitRequest) -> bool {
        self.requests.send(request).is_ok()
    }
}

/// Turn git reading off for the rest of the run, once.
///
/// The twin of [`crate::files::disable_files_worker_state`], and a one-way door
/// for the same reason: the thread is not coming back, so the only question left
/// is whether this is the first time anyone noticed.
pub fn disable_git_worker_state(running: &mut bool, notice_pending: &mut bool) -> bool {
    if !*running {
        return false;
    }
    *running = false;
    *notice_pending = true;
    eprintln!("git worker stopped; terminal input and output remain available");
    true
}

pub fn take_git_worker_notice(notice_pending: &mut bool) -> Option<&'static str> {
    if std::mem::take(notice_pending) {
        Some(git_worker_stopped_notice())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Recorded bytes ─────────────────────────────────────────────────────
    //
    // Every fixture below came out of a real `git.exe` on a repository built to
    // produce it, byte for byte, rather than from the documentation. The
    // documentation is what said a rename spends two records; only the recording
    // says which of the two paths comes first.
    //
    // The separators are written `\x00` rather than `\0` because a `\0` followed
    // by a digit reads as an octal escape to everyone who has ever written C, and
    // half of these records are followed by a hash.

    /// `status --porcelain=v1 -z --untracked-files=all --branch
    /// --ignore-submodules=none`: a deletion, a modification, a rename, a staged
    /// add that was then modified again, and two untracked files one of which is
    /// in a subdirectory.
    const STATUS_Z: &[u8] = b"## main\x00 D del.txt\x00 M mod.txt\x00R  new.txt\x00old.txt\x00AM staged.txt\x00?? sub/deep.txt\x00?? untracked.txt\x00";

    /// The same command on a repository stopped in the middle of a conflicted
    /// merge, asked with `--ignored` so that a `!!` row is in the recording.
    const STATUS_CONFLICT_Z: &[u8] = b"## main\x00UU c.txt\x00!! ignored.log\x00";

    /// A branch two commits ahead of its upstream, and nothing changed.
    const STATUS_AHEAD_Z: &[u8] = b"## main...origin/main [ahead 2]\x00";

    /// `for-each-ref` with this module's format (v2 ③): four local branches —
    /// one diverged from its upstream, one tracking nothing, `HEAD` on the third
    /// — a remote's `HEAD` pointer and one real remote branch, and a tag.
    ///
    /// In git's own order, which is alphabetical inside each tree and the trees
    /// in the order the command line names them — and which is *not* the order
    /// the list is drawn in.
    const REFS: &[u8] = b"refs/heads/diverge\x00a1\x00origin/diverge\x00[ahead 1, behind 1]\x00 \x002026-08-15T10:24:37-04:00\n\
refs/heads/goner\x00a2\x00\x00\x00 \x002026-08-15T10:17:57-04:00\n\
refs/heads/main\x00a3\x00origin/main\x00[ahead 4]\x00*\x002026-08-15T10:18:24-04:00\n\
refs/heads/other\x00a4\x00\x00\x00 \x002026-08-15T10:17:57-04:00\n\
refs/remotes/origin/HEAD\x00a3\x00\x00\x00 \x002026-08-15T10:18:24-04:00\n\
refs/remotes/origin/main\x00a3\x00\x00\x00 \x002026-08-15T10:18:24-04:00\n\
refs/tags/v1.0\x00b1\x00\x00\x00 \x002026-08-01T09:00:00-04:00\n";

    /// `log --parents --topo-order -z` with this module's format: a merge commit
    /// with **two** parents, then two ordinary commits.
    ///
    /// Re-recorded 2026-08-16 with `%ae` beside `%an` (v2 ①) — eight fields a
    /// record where there were seven — and again the same day with `%cn`, `%ce`
    /// and `%b` (v2 ②), which makes it eleven.
    ///
    /// The three commits are deliberately unalike in exactly the three ways the
    /// new fields can be: the merge has no body, the middle one has a body with
    /// a paragraph break in it, and the oldest was committed by somebody other
    /// than its author.
    const LOG_Z: &[u8] = concat!(
        "36d3949271716f6d8cd1395f6f5606245c08b914\u{0}36d3949\u{0}",
        "T\u{0}t@example.com\u{0}T\u{0}t@example.com\u{0}",
        "2026-08-15T10:18:24-04:00\u{0}merge other\u{0}",
        "5a18cfe67ca341203166040bfc8f954b899e275e 91d138a3d39811755e479ec386b450a8c8465302\u{0}",
        "HEAD -> refs/heads/main, refs/remotes/origin/main\u{0}\u{0}",
        "91d138a3d39811755e479ec386b450a8c8465302\u{0}91d138a\u{0}",
        "T\u{0}t@example.com\u{0}T\u{0}t@example.com\u{0}",
        "2026-08-15T10:17:57-04:00\u{0}other\u{0}",
        "a4499ab318aa13e08d780a084fe865fa8d18e558\u{0}refs/heads/other\u{0}",
        "Why the other branch had to exist.\n\nAnd what it will cost to keep.\n\u{0}",
        "5a18cfe67ca341203166040bfc8f954b899e275e\u{0}5a18cfe\u{0}",
        "T\u{0}t@example.com\u{0}Rebase Bot\u{0}bot@example.com\u{0}",
        "2026-08-15T10:18:07-04:00\u{0}ahead2\u{0}",
        "452220ba3687b9dcf3399962a69310de387b7af9\u{0}\u{0}\u{0}",
    )
    .as_bytes();

    /// The instant the `BRANCHES` and `LOG_Z` recordings were made, so that every
    /// age in these tests is a fixed number rather than whatever the clock says
    /// while they run.
    const RECORDED_AT: i64 = 1_786_803_504;

    fn seat(id: u64) -> LeafId {
        LeafId {
            tab: crate::TabId(1),
            seat: crate::SeatId(id),
        }
    }

    fn entry<'a>(status: &'a GitStatus, path: &str) -> &'a GitStatusEntry {
        status
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .expect("the recording contains this path")
    }

    /// Where the real `git.exe` is, or the reason a test that needs one cannot
    /// run. Every test that talks to a real repository goes through here.
    fn real_git() -> PathBuf {
        crate::profiles::find_git(&bt_pty::SystemShellEnvironment).expect(
            "these tests need a git on this machine, as the shell-integration tests already do",
        )
    }

    /// This workspace's own checkout — a real repository, with a real history and
    /// a real working tree, which is the only fixture that can prove the
    /// ancestor search actually climbs.
    fn this_repository() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crates/bt-app sits two levels under the workspace root")
            .to_owned()
    }

    // ── ① The ancestor search (R3) ─────────────────────────────────────────

    /// PIN — a probe from *inside* a repository answers with the repository, not
    /// with the folder that was asked about.
    ///
    /// R3 asked for a walk up the tree looking for `.git`; choosing the CLI made
    /// that walk something we do not write, because `rev-parse --show-toplevel`
    /// already does it. This is the test that the claim is true rather than
    /// merely quoted: `crates/bt-app/src` is three levels down, and the answer is
    /// the root.
    #[test]
    fn a_probe_from_a_deep_subdirectory_answers_with_the_repository_root() {
        let deep = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let question = GitQuestion::RepoProbe { dir: deep.clone() };
        let GitAnswer::Repo { dir, outcome } =
            answer(&real_git(), &question, GIT_COMMAND_TIMEOUT, RECORDED_AT)
        else {
            panic!("a repository probe is answered with a repository");
        };
        assert_eq!(
            dir, deep,
            "the answer carries the folder that was asked about"
        );
        assert_eq!(
            outcome.expect("this workspace is a git repository"),
            this_repository(),
            "three levels up, and spelled the way the rest of the window spells a path"
        );
    }

    // ── ② The status alphabet in full (R11, R6) ────────────────────────────

    /// PIN — every one of git's two-letter states survives the parse, including
    /// the ones that are two states at once.
    #[test]
    fn both_columns_of_every_status_are_kept_apart() {
        let status = parse_status(STATUS_Z);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.entries.len(), 6, "six paths, one of them a rename");

        let deleted = entry(&status, "del.txt");
        assert_eq!(deleted.staged, None, "nothing was staged");
        assert_eq!(deleted.unstaged, Some(StatusCode::Deleted));

        let modified = entry(&status, "mod.txt");
        assert_eq!(modified.staged, None);
        assert_eq!(modified.unstaged, Some(StatusCode::Modified));

        // `AM` — added to the index and then changed again. The one entry that
        // is genuinely in two lists, and the reason grouping is not a field.
        let staged = entry(&status, "staged.txt");
        assert_eq!(staged.staged, Some(StatusCode::Added));
        assert_eq!(staged.unstaged, Some(StatusCode::Modified));
        assert!(staged.in_group(GitGroup::Staged));
        assert!(staged.in_group(GitGroup::Changes));
        assert!(!staged.in_group(GitGroup::Untracked));

        assert_eq!(status.count(GitGroup::Staged), 2, "the rename and the add");
        assert_eq!(
            status.count(GitGroup::Changes),
            3,
            "the deletion, the modification and the staged file's later edit"
        );
        assert_eq!(status.count(GitGroup::Untracked), 2);
    }

    /// PIN — a rename carries where it came from, and the recorded order is the
    /// one the parser reads.
    ///
    /// With `-z` the two paths are `new` then `original`, which is the reverse of
    /// the arrow git prints without it. Reading them the other way round would
    /// not fail loudly — it would quietly show every rename backwards.
    #[test]
    fn a_rename_keeps_the_name_it_had() {
        let status = parse_status(STATUS_Z);
        let renamed = entry(&status, "new.txt");
        assert_eq!(renamed.staged, Some(StatusCode::Renamed));
        assert_eq!(renamed.unstaged, None);
        assert_eq!(renamed.renamed_from.as_deref(), Some("old.txt"));
        assert!(
            status.entries.iter().all(|entry| entry.path != "old.txt"),
            "the original is a field of the rename, never a row of its own"
        );
    }

    /// PIN — `??` is its own list (R6) and `!!` is in none of them.
    #[test]
    fn untracked_is_a_list_of_its_own_and_ignored_is_not_a_list_at_all() {
        let status = parse_status(STATUS_Z);
        let untracked: Vec<&str> = status
            .group(GitGroup::Untracked)
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(untracked, vec!["sub/deep.txt", "untracked.txt"]);
        assert!(
            status
                .group(GitGroup::Changes)
                .all(|e| e.path != "untracked.txt"),
            "an untracked file is not a change to a tracked one"
        );

        let conflicted = parse_status(STATUS_CONFLICT_Z);
        let ignored = entry(&conflicted, "ignored.log");
        assert_eq!(ignored.staged, Some(StatusCode::Ignored));
        assert_eq!(ignored.unstaged, Some(StatusCode::Ignored));
        assert!(
            [GitGroup::Staged, GitGroup::Changes, GitGroup::Untracked]
                .into_iter()
                .all(|group| !ignored.in_group(group)),
            "a file git was told to forget belongs under no heading"
        );
    }

    /// PIN — `UU` is a conflict, and it says so.
    #[test]
    fn an_unmerged_file_is_a_conflict_and_still_a_change() {
        let status = parse_status(STATUS_CONFLICT_Z);
        let conflict = entry(&status, "c.txt");
        assert_eq!(conflict.staged, Some(StatusCode::Unmerged));
        assert_eq!(conflict.unstaged, Some(StatusCode::Unmerged));
        assert!(conflict.is_conflict());
        assert!(conflict.in_group(GitGroup::Changes));
    }

    /// PIN — every letter of git's alphabet parses, and nothing else does.
    #[test]
    fn the_status_alphabet_is_complete() {
        for letter in ['M', 'T', 'A', 'D', 'R', 'C', 'U', '?', '!'] {
            assert!(
                StatusCode::from_letter(letter).is_some_and(|code| code.letter() == letter),
                "{letter} is one of git's own letters and survives the round trip"
            );
        }
        assert_eq!(StatusCode::from_letter(' '), None, "a space is an absence");
        assert_eq!(StatusCode::from_letter('X'), None);
    }

    /// PIN — the two conflict pairs whose letters do not contain a `U`.
    #[test]
    fn both_added_and_both_deleted_are_conflicts_too() {
        let both = |x: char, y: char| GitStatusEntry {
            staged: StatusCode::from_letter(x),
            unstaged: StatusCode::from_letter(y),
            path: "c.txt".to_owned(),
            renamed_from: None,
        };
        assert!(both('A', 'A').is_conflict());
        assert!(both('D', 'D').is_conflict());
        assert!(
            !both('A', 'M').is_conflict(),
            "a staged add that was edited again is not a conflict"
        );
    }

    /// PIN — the `##` head: branch, upstream, and both pills.
    #[test]
    fn the_branch_head_carries_the_upstream_and_the_distance_from_it() {
        let ahead = parse_status(STATUS_AHEAD_Z);
        assert_eq!(ahead.branch.as_deref(), Some("main"));
        assert_eq!(ahead.upstream.as_deref(), Some("origin/main"));
        assert_eq!((ahead.ahead, ahead.behind), (2, 0));
        assert!(ahead.entries.is_empty(), "a clean tree is an empty list");

        let plain = parse_status(STATUS_Z);
        assert_eq!(plain.branch.as_deref(), Some("main"));
        assert_eq!(
            plain.upstream, None,
            "no upstream is not an upstream of nothing"
        );
        assert_eq!((plain.ahead, plain.behind), (0, 0));

        let detached = parse_status(b"## HEAD (no branch)\0");
        assert!(detached.detached);
        assert_eq!(detached.branch, None);

        let unborn = parse_status(b"## No commits yet on main\0");
        assert!(unborn.unborn);
        assert_eq!(unborn.branch.as_deref(), Some("main"));

        assert_eq!(parse_track("[ahead 1, behind 2]"), (1, 2));
        assert_eq!(parse_track("[behind 3]"), (0, 3));
        assert_eq!(parse_track("[gone]"), (0, 0));
        assert_eq!(parse_track(""), (0, 0));
    }

    // ── ③ The cap (R33) ────────────────────────────────────────────────────

    /// PIN — a repository with more changes than the column can show keeps the
    /// first [`GIT_STATUS_CAP`] and *counts* the rest.
    ///
    /// The count is the whole point: `DIR_ENTRY_CAP`'s own note is that a cap is
    /// only honest when the surrender says how much it gave up. Dropping the
    /// remainder silently would make a generated-file avalanche look like a
    /// tidy repository.
    #[test]
    fn a_status_longer_than_the_cap_keeps_the_first_page_and_counts_the_rest() {
        let mut recorded = b"## main\0".to_vec();
        for index in 0..GIT_STATUS_CAP + 1 {
            recorded.extend_from_slice(format!(" M file{index:05}.txt\0").as_bytes());
        }
        let status = parse_status(&recorded);
        assert_eq!(status.entries.len(), GIT_STATUS_CAP);
        assert_eq!(status.dropped, 1);
        assert_eq!(status.entries[0].path, "file00000.txt");
        assert_eq!(
            status.entries[GIT_STATUS_CAP - 1].path,
            format!("file{:05}.txt", GIT_STATUS_CAP - 1),
            "the first N in git's own order, not a sample from anywhere"
        );

        let short = parse_status(STATUS_Z);
        assert_eq!(
            short.dropped, 0,
            "the overwhelmingly common case says nothing"
        );
    }

    // ── ④ The relative-time table (R8) ─────────────────────────────────────

    /// PIN — every rung of R8's ladder, including both sides of each edge.
    #[test]
    fn the_relative_time_table_reads_the_way_it_was_ruled() {
        let day = SECONDS_PER_DAY;
        // 2026-08-15T10:18:24-04:00, the instant the fixtures were recorded.
        let then = RECORDED_AT;
        let at = |delta: i64| relative_time(then, -4 * 3600, then + delta);
        assert_eq!(at(0), "now");
        assert_eq!(at(59), "now");
        assert_eq!(at(60), "1m");
        assert_eq!(at(59 * 60 + 59), "59m");
        assert_eq!(at(60 * 60), "1h");
        assert_eq!(at(23 * 3600 + 3599), "23h");
        assert_eq!(at(day), "1d");
        assert_eq!(at(6 * day + 3599), "6d");
        // A week is where counting stops and dating starts — in the commit's own
        // timezone, which is where git itself would date it.
        assert_eq!(at(7 * day), "Aug 15");
        assert_eq!(at(364 * day), "Aug 15");
        assert_eq!(at(365 * day), "2026 Aug");
        assert_eq!(
            relative_time(then, 0, then + 7 * day),
            "Aug 15",
            "UTC puts the same commit on the same day here"
        );
        assert_eq!(
            at(-90),
            "now",
            "a clock that moved backwards says the closest true thing"
        );
    }

    /// PIN — the calendar buckets are the commit's own date, not ours.
    ///
    /// The recorded commit is 2026-08-15T10:18:24 at `-04:00`, which is
    /// 2026-08-**14**T18:18:24 in UTC. A formatter that ignored the offset would
    /// date it a day early, and the only way to see that is to ask on a fixture
    /// whose local date and UTC date differ.
    #[test]
    fn an_old_commit_is_dated_in_the_timezone_it_was_made_in() {
        let (unix, offset) =
            parse_iso_strict("2026-08-15T00:30:00+08:00").expect("git's strict ISO parses");
        assert_eq!(offset, 8 * 3600);
        assert_eq!(
            relative_time(unix, offset, unix + 30 * SECONDS_PER_DAY),
            "Aug 15",
            "the date it was made, though in UTC it was still the 14th"
        );
        assert_eq!(
            relative_time(unix, 0, unix + 30 * SECONDS_PER_DAY),
            "Aug 14",
            "and the UTC reading of the same instant, to prove the offset is used"
        );
    }

    // ── ⑤ The log, parents and all (G-4's only input) ──────────────────────

    /// PIN — a merge commit arrives with both its parents, in git's order.
    #[test]
    fn a_merge_commit_keeps_both_parents() {
        let page = parse_log(LOG_Z, RECORDED_AT, 0, GIT_LOG_PAGE);
        assert_eq!(page.commits.len(), 3);
        assert!(!page.has_more, "three commits is less than a page");
        assert_eq!(page.skip, 0);

        let merge = &page.commits[0];
        assert_eq!(merge.hash, "36d3949271716f6d8cd1395f6f5606245c08b914");
        assert_eq!(merge.short, "36d3949", "git's abbreviation, not our cut");
        assert_eq!(merge.subject, "merge other");
        assert_eq!(merge.author_name, "T");
        assert_eq!(merge.author_email, "t@example.com");
        assert_eq!(
            merge.parents,
            vec![
                "5a18cfe67ca341203166040bfc8f954b899e275e".to_owned(),
                "91d138a3d39811755e479ec386b450a8c8465302".to_owned(),
            ],
            "two parents, first parent first"
        );
        assert_eq!(merge.committer_unix, 1_786_803_504);
        assert_eq!(merge.committer_offset, -4 * 3600);
        assert_eq!(merge.time_relative, "now");
        // R22 — the pills, all three kinds since v2 ③: the remote tracking ref
        // beside `main` in the recording gets a pill of its own, after the local.
        assert_eq!(
            merge.refs,
            vec![
                GitRef {
                    name: "main".to_owned(),
                    head: true,
                    kind: GitRefKind::Local,
                },
                GitRef {
                    name: "origin/main".to_owned(),
                    head: false,
                    kind: GitRefKind::Remote,
                },
            ]
        );
        assert_eq!(page.commits[2].refs, Vec::new(), "an undecorated commit");

        let root_ward = &page.commits[2];
        assert_eq!(root_ward.subject, "ahead2");
        assert_eq!(
            root_ward.parents,
            vec!["452220ba3687b9dcf3399962a69310de387b7af9".to_owned()],
            "one parent is a list of one, not an absence"
        );
    }

    /// v2 ① (V1/V4) — the author column's two facts arrive with the page.
    ///
    /// The assertion is on **every** record and not on the first, because what
    /// `%ae` really changed is the *stride*: a record is eight fields where it
    /// was seven, and a parser that read the new field but kept counting in
    /// sevens would still get the first commit right and every commit after it
    /// wrong. Checking the last one is checking the arithmetic.
    #[test]
    fn every_commit_of_a_page_carries_the_name_and_the_address_of_its_author() {
        let page = parse_log(LOG_Z, RECORDED_AT, 0, GIT_LOG_PAGE);
        assert_eq!(page.commits.len(), 3);
        for commit in &page.commits {
            assert_eq!(commit.author_name, "T");
            assert_eq!(commit.author_email, "t@example.com");
        }
        // And the fields *after* the two new ones still land where they belong.
        assert_eq!(page.commits[2].subject, "ahead2");
        assert_eq!(page.commits[2].short, "5a18cfe");
    }

    /// v2 ② (D1/D2) — the whole message and the committer arrive with the page.
    ///
    /// Its sibling above is about the *stride*, and so is this: `%b` is the one
    /// field that can hold a newline, and a parse that let a body's second
    /// paragraph be counted as the next commit's hash would still get the first
    /// commit right. So the assertion walks to the last record and reads a field
    /// past every body in the page.
    #[test]
    fn a_commit_carries_its_body_with_the_paragraphs_it_was_written_with() {
        let page = parse_log(LOG_Z, RECORDED_AT, 0, GIT_LOG_PAGE);
        assert_eq!(page.commits.len(), 3);
        assert_eq!(page.commits[0].body, "", "a merge with a one-line message");
        assert_eq!(
            page.commits[1].body,
            "Why the other branch had to exist.\n\nAnd what it will cost to keep.",
            "the blank line between the paragraphs is kept; the one git puts at \
             the end of every body is not"
        );
        // The fields *after* the body of a commit that has one still land where
        // they belong — which is the stride.
        assert_eq!(page.commits[2].short, "5a18cfe");
        assert_eq!(page.commits[2].subject, "ahead2");
        assert_eq!(
            page.commits[2].parents,
            vec!["452220ba3687b9dcf3399962a69310de387b7af9".to_owned()]
        );
    }

    /// D2 — who committed it, which is only interesting when it is not who wrote
    /// it.
    #[test]
    fn a_commit_carries_its_committer_beside_its_author() {
        let page = parse_log(LOG_Z, RECORDED_AT, 0, GIT_LOG_PAGE);
        assert_eq!(page.commits[0].committer_name, "T");
        assert_eq!(page.commits[0].committer_email, "t@example.com");
        assert_eq!(page.commits[2].author_name, "T");
        assert_eq!(page.commits[2].committer_name, "Rebase Bot");
        assert_eq!(page.commits[2].committer_email, "bot@example.com");
    }

    /// PIN — a page knows whether there is another, without counting the
    /// repository.
    #[test]
    fn a_full_page_that_had_one_more_behind_it_says_so() {
        // Two commits asked for; three came back, because the command asks for
        // one more than it shows.
        let page = parse_log(LOG_Z, RECORDED_AT, 0, 2);
        assert_eq!(
            page.commits.len(),
            2,
            "the extra commit is a signal, not a row"
        );
        assert!(page.has_more);
        assert_eq!(parse_log(LOG_Z, RECORDED_AT, 0, 3).commits.len(), 3);
        assert!(!parse_log(LOG_Z, RECORDED_AT, 0, 3).has_more);
        assert_eq!(
            parse_log(LOG_Z, RECORDED_AT, 50, 3).skip,
            50,
            "a page remembers where it started"
        );
        assert!(parse_log(b"", RECORDED_AT, 0, 50).commits.is_empty());
    }

    // ── ⑥ Coalescing: what is and is not the same question ─────────────────

    /// PIN — the staged and unstaged diffs of one file are two questions.
    ///
    /// R25 maps the staged row onto `--cached`, which means the two rows differ by
    /// one boolean and by nothing else. Were they one target, opening both in
    /// quick succession would leave whichever process finished last answering for
    /// both — the index's diff drawn under the working tree's heading, which is
    /// not a wrong pixel but a wrong fact.
    #[test]
    fn a_staged_diff_and_an_unstaged_diff_are_not_the_same_question() {
        let (tx, rx) = mpsc::channel();
        let diff = |against: crate::preview::GitDiffAgainst| GitRequest {
            window: winit::window::WindowId::from(1_u64),
            host: GitHost::Column(seat(1)),
            question: GitQuestion::Diff {
                root: PathBuf::from(r"D:\repo"),
                path: "src/main.rs".to_owned(),
                against,
                renamed_from: None,
            },
        };
        tx.send(diff(crate::preview::GitDiffAgainst::WorkingTree))
            .unwrap();
        tx.send(diff(crate::preview::GitDiffAgainst::Index))
            .unwrap();
        tx.send(diff(crate::preview::GitDiffAgainst::WorkingTree))
            .unwrap();
        drop(tx);
        let mut served = Vec::new();
        run_git_worker(rx, |request| served.push(request));
        assert_eq!(
            served.len(),
            2,
            "the repeated unstaged question collapses; the staged one does not"
        );
        assert_eq!(served[0], diff(crate::preview::GitDiffAgainst::Index));
        assert_eq!(served[1], diff(crate::preview::GitDiffAgainst::WorkingTree));
    }

    /// PIN — two pages of history are two questions; the same page twice is one.
    #[test]
    fn two_pages_of_history_are_two_questions_and_one_page_asked_twice_is_one() {
        let (tx, rx) = mpsc::channel();
        let page = |skip: usize, count: usize| GitRequest {
            window: winit::window::WindowId::from(1_u64),
            host: GitHost::Column(seat(1)),
            question: GitQuestion::Log {
                root: PathBuf::from(r"D:\repo"),
                refs: Vec::new(),
                skip,
                count,
            },
        };
        tx.send(page(0, 50)).unwrap();
        tx.send(page(50, 50)).unwrap();
        // The same page again, asked for a different number of commits: still the
        // same page, and the newer request is the one that should win.
        tx.send(page(0, 20)).unwrap();
        drop(tx);
        let mut served = Vec::new();
        run_git_worker(rx, |request| served.push(request));
        assert_eq!(served, vec![page(50, 50), page(0, 20)]);
    }

    /// PIN — the same question from two columns is two questions.
    #[test]
    fn two_columns_asking_the_same_thing_both_get_answered() {
        let (tx, rx) = mpsc::channel();
        let ask = |host: LeafId| GitRequest {
            window: winit::window::WindowId::from(1_u64),
            host: GitHost::Column(host),
            question: GitQuestion::Status {
                root: PathBuf::from(r"D:\repo"),
            },
        };
        tx.send(ask(seat(1))).unwrap();
        tx.send(ask(seat(2))).unwrap();
        drop(tx);
        let mut served = Vec::new();
        run_git_worker(rx, |request| served.push(request));
        assert_eq!(served.len(), 2, "an answer is addressed, not broadcast");
    }

    /// PIN — a question superseded *while an earlier one is running* is dropped
    /// rather than asked, which is the case the queue exists for.
    #[test]
    fn a_status_superseded_during_a_slow_read_is_never_run() {
        let (tx, rx) = mpsc::channel();
        let ask = |root: &str| GitRequest {
            window: winit::window::WindowId::from(1_u64),
            host: GitHost::Column(seat(1)),
            question: GitQuestion::Status {
                root: PathBuf::from(root),
            },
        };
        tx.send(ask("slow")).unwrap();
        tx.send(ask("target")).unwrap();
        let mut late = Some(tx.clone());
        drop(tx);
        let mut served = Vec::new();
        run_git_worker(rx, |request| {
            if request.question
                == (GitQuestion::Status {
                    root: PathBuf::from("slow"),
                })
                && let Some(sender) = late.take()
            {
                sender.send(ask("target")).unwrap();
            }
            served.push(request);
        });
        assert_eq!(
            served,
            vec![ask("slow"), ask("target")],
            "the stale question is discarded unasked, and only the newest is served"
        );
    }

    // ── ⑦ The deadlock guard ───────────────────────────────────────────────

    /// PIN — a `git` that never finishes costs one answer, not the worker.
    ///
    /// Stood in for by `ping -t`, which is on every Windows and, unlike the
    /// `ping -n 10` this used to use, **never stops on its own**. That is the
    /// whole of the shape: the assertion is that the call *came back*, and the
    /// only road back is through the kill — [`run_git_with_input`] waits for the
    /// process it killed before it reports, and a wait on a `ping -t` nobody
    /// killed does not end. So the kill, the reaping and the report are all one
    /// fact now, and there is no clock in any of them.
    ///
    /// It used to be `ping -n 10` and `waited < 5s`. The five seconds were
    /// standing in for "less than the nine the child would have taken", which
    /// made it a wall clock on a machine that is often carrying twenty
    /// compilers: a hundred-and-fifty-millisecond poll loop that overruns by
    /// thirty times is a scheduling story and not a `run_git` story, and this
    /// test could not tell the difference.
    ///
    /// The guard runs on a thread of its own and its answer comes back down a
    /// channel, so a `run_git` that really did wait for its child fails this in a
    /// minute rather than hanging the suite for ever. **The minute is not a
    /// budget**: the call is given a hundred and fifty milliseconds and no load
    /// turns that into sixty seconds. It is the difference between "returned"
    /// and "never returns", which is the only difference this test is about — and
    /// on the day it is spent, the stray `ping` left behind is the defect itself.
    #[test]
    fn a_child_that_will_not_finish_is_killed_and_reported() {
        /// Long enough that only a guard which never returns can reach it.
        const NEVER: Duration = Duration::from_secs(60);

        let mut command = bt_platform::quiet_command("ping");
        command
            .args(["-t", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(run_git(command, Duration::from_millis(150)).err());
        });
        let fault = rx.recv_timeout(NEVER).expect(
            "the guard came back, so it killed the child and reaped it: a guard that waited for \
             this child would be waiting still",
        );
        assert_eq!(fault, Some(GitFault::TimedOut));
    }

    // ── ⑧ The two soft landings ────────────────────────────────────────────

    /// PIN — a folder outside any repository is [`GitFault::NotARepository`] and
    /// nothing else. R17's single empty state depends on this classification.
    #[test]
    fn a_folder_outside_a_repository_lands_softly() {
        let outside = std::env::temp_dir().join(format!(
            "folio-git-probe-{}-{}",
            std::process::id(),
            RECORDED_AT
        ));
        std::fs::create_dir_all(&outside).expect("a scratch folder can be made");
        let question = GitQuestion::RepoProbe {
            dir: outside.clone(),
        };
        let GitAnswer::Repo { outcome, .. } =
            answer(&real_git(), &question, GIT_COMMAND_TIMEOUT, RECORDED_AT)
        else {
            panic!("a repository probe is answered with a repository");
        };
        let _ = std::fs::remove_dir_all(&outside);
        assert_eq!(outcome.err(), Some(GitFault::NotARepository));
    }

    /// PIN — a machine with no git answers every question with the same reason,
    /// and no question is left unanswered.
    #[test]
    fn a_machine_without_git_answers_every_question_anyway() {
        let nowhere = PathBuf::from(r"C:\nowhere\there-is-no-git-here.exe");
        let question = GitQuestion::Status {
            root: this_repository(),
        };
        let GitAnswer::Status { outcome, .. } =
            answer(&nowhere, &question, GIT_COMMAND_TIMEOUT, RECORDED_AT)
        else {
            panic!("a status question is answered with a status");
        };
        assert!(
            matches!(outcome, Err(GitFault::GitMissing(_))),
            "a git that will not start is a missing git, with the reason attached"
        );

        // And the same when there was never a program to try: every one of the
        // six questions comes back wearing the fault rather than being dropped.
        let fault = GitFault::GitMissing(git_not_found().to_owned());
        for question in [
            GitQuestion::RepoProbe {
                dir: PathBuf::from(r"D:\repo"),
            },
            GitQuestion::Status {
                root: PathBuf::from(r"D:\repo"),
            },
            GitQuestion::Refs {
                root: PathBuf::from(r"D:\repo"),
            },
            GitQuestion::Search {
                root: PathBuf::from(r"D:\repo"),
                query: "fix".to_owned(),
            },
            GitQuestion::Log {
                root: PathBuf::from(r"D:\repo"),
                refs: Vec::new(),
                skip: 0,
                count: GIT_LOG_PAGE,
            },
            GitQuestion::Diff {
                root: PathBuf::from(r"D:\repo"),
                path: "a.rs".to_owned(),
                against: crate::preview::GitDiffAgainst::Index,
                renamed_from: None,
            },
            GitQuestion::Show {
                root: PathBuf::from(r"D:\repo"),
                hash: "abc".to_owned(),
                path: "a.rs".to_owned(),
                renamed_from: None,
            },
            GitQuestion::CommitFiles {
                root: PathBuf::from(r"D:\repo"),
                hash: "abc".to_owned(),
            },
            GitQuestion::CompareFiles {
                root: PathBuf::from(r"D:\repo"),
                a: "abc".to_owned(),
                b: Some("def".to_owned()),
            },
            GitQuestion::DiffRange {
                root: PathBuf::from(r"D:\repo"),
                a: "abc".to_owned(),
                b: None,
                path: "src/main.rs".to_owned(),
                renamed_from: None,
            },
        ] {
            let answer = faulted(&question, fault.clone());
            let carried = match &answer {
                GitAnswer::Repo { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Status { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Refs { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Search { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Log { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Diff { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Show { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::DiffRange { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::CommitFiles { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::CompareFiles { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Write { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Checkout { outcome, .. } => outcome.as_ref().err(),
            };
            assert_eq!(carried, Some(&fault), "{question:?} came back unanswered");
        }
    }

    /// PIN — git's own refusals keep git's own words.
    ///
    /// The whole argument for the CLI was that the panel and the terminal beside
    /// it never disagree; paraphrasing a refusal is exactly that disagreement,
    /// dressed as helpfulness.
    #[test]
    fn a_refusal_is_passed_through_and_only_one_sentence_is_recognised() {
        assert_eq!(
            classify_failure(
                "fatal: not a git repository (or any of the parent directories): .git\n"
            ),
            GitFault::NotARepository
        );
        let dubious = "fatal: detected dubious ownership in repository at 'D:/repo'\n\
                       To add an exception for this directory, call:\n";
        assert_eq!(
            classify_failure(dubious),
            GitFault::Refused(
                "fatal: detected dubious ownership in repository at 'D:/repo'".into()
            ),
            "the first line, verbatim"
        );
        assert_eq!(
            classify_failure(""),
            GitFault::Refused(String::new()),
            "a refusal with nothing to say is still a refusal, not a missing repository"
        );
    }

    // ── The ref list ───────────────────────────────────────────────────────

    /// PIN — the branch `HEAD` is on leads the list (R9), and each branch carries
    /// its own distance from its own upstream.
    #[test]
    fn the_current_branch_leads_and_each_branch_counts_its_own_upstream() {
        let refs = parse_refs(REFS, RECORDED_AT + 300);
        let branches: Vec<&GitRefEntry> = local_branches(&refs).collect();
        let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["main", "diverge", "goner", "other"],
            "the current branch first, then the rest newest first"
        );
        assert!(branches[0].is_head);
        assert!(branches[1..].iter().all(|branch| !branch.is_head));
        assert_eq!((branches[0].ahead, branches[0].behind), (4, 0));
        assert_eq!(branches[0].upstream.as_deref(), Some("origin/main"));
        assert_eq!((branches[1].ahead, branches[1].behind), (1, 1));
        assert_eq!(
            (branches[2].ahead, branches[2].behind),
            (0, 0),
            "a branch tracking nothing is not behind anything"
        );
        assert_eq!(
            branches[2].upstream, None,
            "and it names no upstream either, rather than an empty one"
        );
        assert_eq!(branches[0].committer_unix, 1_786_803_504);
        assert_eq!(branches[0].committerdate_relative, "5m");
        assert_eq!(branches[1].committerdate_relative, "now");
    }

    /// T6 (v2 ③) — one `for-each-ref` answers all three kinds, each saying which
    /// it is, in the order they are drawn.
    #[test]
    fn one_question_answers_locals_remotes_and_tags_in_that_order() {
        let refs = parse_refs(REFS, RECORDED_AT + 300);
        let seen: Vec<(GitRefKind, &str)> = refs
            .iter()
            .map(|entry| (entry.kind, entry.name.as_str()))
            .collect();
        assert_eq!(
            seen,
            vec![
                (GitRefKind::Local, "main"),
                (GitRefKind::Local, "diverge"),
                (GitRefKind::Local, "goner"),
                (GitRefKind::Local, "other"),
                (GitRefKind::Remote, "origin/main"),
                (GitRefKind::Tag, "v1.0"),
            ],
            "locals current-first and newest-first, then the remotes, then the tags"
        );
        assert_eq!(
            remote_branches(&refs)
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["origin/main"],
            "`origin/HEAD` is a pointer at a row this list already has, not a row"
        );
        assert_eq!(refs[0].object, "a3", "each ref says what it points at");
    }

    /// T6 — a ref outside the three trees is not a ref this product draws.
    #[test]
    fn a_ref_that_is_neither_branch_nor_tag_is_left_out() {
        let refs = parse_refs(
            b"refs/stash\x00a1\x00\x00\x00 \x002026-08-15T10:18:24-04:00\n\
refs/notes/commits\x00a2\x00\x00\x00 \x002026-08-15T10:18:24-04:00\n\
refs/heads/main\x00a3\x00\x00\x00*\x002026-08-15T10:18:24-04:00\n",
            RECORDED_AT,
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "main");
    }

    // ── What a column knows ────────────────────────────────────────────────

    /// PIN — the driver asks the probe first and the three page questions only
    /// once there is a repository to ask them of.
    #[test]
    fn a_cache_asks_for_the_repository_before_it_asks_about_it() {
        let dir = PathBuf::from(r"D:\repo\crates");
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::default();
        assert!(
            cache.pending_questions().is_empty(),
            "an unrooted column asks nothing"
        );
        assert!(cache.retarget(&dir));
        assert_eq!(
            cache.pending_questions(),
            vec![GitQuestion::RepoProbe { dir: dir.clone() }]
        );

        for question in cache.pending_questions() {
            cache.mark_pending(&question);
        }
        assert!(
            cache.pending_questions().is_empty(),
            "a question already in flight is not asked again — this is what makes the \
             driver safe to consult on every frame (R31)"
        );

        assert!(cache.accept(GitAnswer::Repo {
            dir: dir.clone(),
            outcome: Ok(root.clone()),
        }));
        assert_eq!(cache.root(), Some(root.as_path()));
        assert_eq!(
            cache.pending_questions(),
            vec![
                GitQuestion::Status { root: root.clone() },
                GitQuestion::Refs { root: root.clone() },
                GitQuestion::Log {
                    root: root.clone(),
                    refs: Vec::new(),
                    skip: 0,
                    count: GIT_LOG_PAGE,
                },
            ]
        );
    }

    /// PIN — a folder that is not a repository stops the driver rather than
    /// making it ask three questions it already knows the answer to.
    #[test]
    fn a_column_outside_a_repository_asks_nothing_further() {
        let dir = PathBuf::from(r"D:\elsewhere");
        let mut cache = GitCache::default();
        cache.retarget(&dir);
        cache.mark_pending(&GitQuestion::RepoProbe { dir: dir.clone() });
        assert!(cache.accept(GitAnswer::Repo {
            dir,
            outcome: Err(GitFault::NotARepository),
        }));
        assert_eq!(cache.repo().fault(), Some(&GitFault::NotARepository));
        assert!(cache.pending_questions().is_empty());
    }

    /// PIN — an answer about a repository this column has left is dropped.
    #[test]
    fn an_answer_that_arrives_after_a_re_root_is_dropped() {
        let mut cache = GitCache::default();
        cache.retarget(Path::new(r"D:\one"));
        cache.mark_pending(&GitQuestion::RepoProbe {
            dir: PathBuf::from(r"D:\one"),
        });
        assert!(
            cache.retarget(Path::new(r"D:\two")),
            "a new root forgets the old one"
        );
        assert!(
            !cache.accept(GitAnswer::Repo {
                dir: PathBuf::from(r"D:\one"),
                outcome: Ok(PathBuf::from(r"D:\one")),
            }),
            "the answer to the question the old root asked is not the new root's"
        );
        assert!(matches!(cache.repo(), GitSlot::Idle));
        assert!(
            !cache.retarget(Path::new(r"D:\two")),
            "the same root forgets nothing"
        );
    }

    /// PIN (user report, 2026-08-19) — **a re-read asks for as much history as
    /// is on screen**, not for the first page.
    ///
    /// A `skip: 0` answer *replaces* the list, which is what makes a rewritten
    /// history land honestly. A re-read that asked for one page therefore threw
    /// away every page the reader had paged in: five hundred rows became fifty
    /// under their scroll, the scroll was clamped to the shorter list, and the
    /// auto-pager then crawled back up fifty at a time. Under a repository
    /// something else is writing to — a build, another window, another
    /// checkout — the kernel says so every couple of seconds, and what the
    /// reader sees is the page collapsing and rebuilding itself on a loop.
    ///
    /// MUTATION: put `GIT_LOG_PAGE` back and the second half of this fails; the
    /// first half still passes, because a page that has not been paged is one
    /// page deep already.
    #[test]
    fn a_re_read_asks_for_every_page_the_reader_has_loaded() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::at_root(root.clone(), GitRole::Graph);
        let depth_of = |cache: &mut GitCache| match cache
            .begin_reread()
            .into_iter()
            .find(|question| matches!(question, GitQuestion::Log { .. }))
        {
            Some(GitQuestion::Log { count, skip, .. }) => {
                assert_eq!(skip, 0, "a re-read re-reads from the top");
                count
            }
            _ => panic!("a re-read asks about the history"),
        };

        // Nothing read yet: one page, which is what a first reading asks for.
        assert_eq!(depth_of(&mut cache), GIT_LOG_PAGE);

        let page = |from: usize, count: usize| GitLog {
            skip: from,
            commits: (0..count)
                .map(|at| {
                    let hash = format!("{:040x}", from + at);
                    GitCommit {
                        short: hash[..7].to_owned(),
                        hash,
                        subject: String::new(),
                        author_name: String::new(),
                        author_email: String::new(),
                        committer_name: String::new(),
                        committer_email: String::new(),
                        body: String::new(),
                        committer_unix: 0,
                        committer_offset: 0,
                        time_relative: String::new(),
                        parents: Vec::new(),
                        refs: Vec::new(),
                    }
                })
                .collect(),
            has_more: true,
        };

        assert!(cache.accept(GitAnswer::Log {
            root: root.clone(),
            skip: 0,
            outcome: Ok(page(0, GIT_LOG_PAGE)),
        }));
        assert_eq!(depth_of(&mut cache), GIT_LOG_PAGE);

        // The reader pages twice more. The re-read now has to come back with all
        // of it, in one answer, or the page it lands on is shorter than the page
        // it replaced.
        for from in [GIT_LOG_PAGE, GIT_LOG_PAGE * 2] {
            assert!(cache.accept(GitAnswer::Log {
                root: root.clone(),
                skip: from,
                outcome: Ok(page(from, GIT_LOG_PAGE)),
            }));
        }
        assert_eq!(
            cache.log().ready().map(|log| log.commits.len()),
            Some(GIT_LOG_PAGE * 3)
        );
        assert_eq!(depth_of(&mut cache), GIT_LOG_PAGE * 3);
    }

    /// PIN (R10) — a checkout git refused changes nothing but the banner.
    ///
    /// The whole failure design of the verb, in one test. git is the thing that
    /// decides a dirty tree may not be walked away from, and what comes back is
    /// **git's own sentence**; the panel prints it and stays exactly where it
    /// was. What must *not* happen is the two halves of the mock-up's version:
    /// moving the branch on the click and hoping, or paraphrasing the refusal
    /// into something friendlier than what the terminal beside it would print.
    #[test]
    fn a_refused_checkout_keeps_gits_words_and_leaves_the_branch_alone() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::at_root(root.clone(), GitRole::Page);
        assert!(cache.accept(GitAnswer::Refs {
            root: root.clone(),
            outcome: Ok(parse_refs(
                b"refs/heads/main\0a1\0\0\0*\0\nrefs/heads/side\0a2\0\0\0 \0\n",
                RECORDED_AT
            )),
        }));
        assert!(cache.accept(GitAnswer::Status {
            root: root.clone(),
            outcome: Ok(parse_status(STATUS_Z)),
        }));
        assert!(cache.accept(GitAnswer::Log {
            root: root.clone(),
            skip: 0,
            outcome: Ok(parse_log(LOG_Z, RECORDED_AT, 0, GIT_LOG_PAGE)),
        }));
        assert!(
            cache.pending_questions().is_empty(),
            "a cache that has been told everything asks nothing"
        );
        let question = cache
            .begin_checkout("side".to_owned(), false)
            .expect("a repository that is known can be checked out of");
        assert_eq!(
            question,
            GitQuestion::Checkout {
                root: root.clone(),
                target: "side".to_owned(),
                detach: false,
            }
        );
        // **One at a time**: the second click while the first is running is the
        // first click arriving twice.
        assert_eq!(cache.begin_checkout("side".to_owned(), false), None);
        assert_eq!(cache.checkout_pending(), Some("side"));

        let refusal =
            "error: Your local changes to the following files would be overwritten by checkout:";
        assert!(cache.accept(GitAnswer::Checkout {
            root: root.clone(),
            target: "side".to_owned(),
            outcome: Err(GitFault::Refused(refusal.to_owned())),
        }));
        // git's own words are carried by the *answer*, and the runtime raises a
        // notice from it (toast ruling, 2026-08-16). The cache keeps no copy: a
        // refusal is a thing that happened, not a state this repository is in.
        assert_eq!(
            write_refusal(&GitFault::Refused(refusal.to_owned())),
            refusal
        );
        assert_eq!(cache.checkout_pending(), None, "the row stops waiting");
        // The list is untouched: the refusal changed nothing, so nothing is
        // re-asked and `main` is still the branch.
        let branches = cache.refs().ready().expect("still answered");
        assert_eq!(branches[0].name, "main");
        assert!(branches[0].is_head);
        assert!(
            cache.pending_questions().is_empty(),
            "a refusal re-asks nothing"
        );

        // A checkout that *succeeded* invalidates everything, because after it
        // the branch, the status and the history are all about somewhere else.
        let question = cache
            .begin_checkout("side".to_owned(), false)
            .expect("the guard cleared with the receipt");
        cache.mark_pending(&question);
        assert!(cache.accept(GitAnswer::Checkout {
            root,
            target: "side".to_owned(),
            outcome: Ok(()),
        }));
        assert_eq!(cache.pending_questions().len(), 3, "status, branches, log");
    }

    /// A detached checkout says so on the command line rather than hoping.
    #[test]
    fn a_detached_checkout_is_spelled_out() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::at_root(root.clone(), GitRole::Graph);
        assert_eq!(
            cache.begin_checkout("deadbeef".to_owned(), true),
            Some(GitQuestion::Checkout {
                root,
                target: "deadbeef".to_owned(),
                detach: true,
            })
        );
        // And a graph asks for the refs too since v2 ③ — its filter menu lists
        // every local branch by name, so the answer is no longer unread (T6).
        assert!(
            cache
                .pending_questions()
                .iter()
                .any(|question| matches!(question, GitQuestion::Refs { .. })),
            "the graph never asked for the refs its filter menu is made of"
        );
    }

    /// R22 — the pills a commit wears are the local branches and nothing else.
    #[test]
    fn a_decoration_keeps_every_kind_of_name_and_puts_them_in_drawing_order() {
        assert_eq!(parse_decoration(""), Vec::new());
        let named = |name: &str, head: bool, kind: GitRefKind| GitRef {
            name: name.to_owned(),
            head,
            kind,
        };
        assert_eq!(
            parse_decoration(
                "refs/tags/v1.0, refs/remotes/origin/main, HEAD -> refs/heads/main, refs/heads/side"
            ),
            vec![
                named("main", true, GitRefKind::Local),
                named("side", false, GitRefKind::Local),
                named("origin/main", false, GitRefKind::Remote),
                named("v1.0", false, GitRefKind::Tag),
            ],
            "HEAD's local leads, then the other locals, then the remotes, then the tags"
        );
        // A detached head is a place you are standing with no branch behind it,
        // and the graph is exactly the surface that should say so.
        assert_eq!(
            parse_decoration("HEAD, refs/heads/main"),
            vec![
                named("HEAD", true, GitRefKind::Local),
                named("main", false, GitRefKind::Local),
            ]
        );
        // A branch genuinely called `origin/main` is a local branch, and the
        // full spelling is the only thing that can tell it from the remote one.
        assert_eq!(
            parse_decoration("refs/heads/origin/main"),
            vec![named("origin/main", false, GitRefKind::Local)]
        );
        // A ref outside the three trees still is not a pill.
        assert_eq!(parse_decoration("refs/stash"), Vec::new());
    }

    /// PIN — "Load more" extends the list rather than replacing it, and a page
    /// that does not start where the list ends is not filed at all.
    #[test]
    fn a_second_page_of_history_is_added_to_the_first() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::default();
        cache.retarget(&root);
        cache.mark_pending(&GitQuestion::RepoProbe { dir: root.clone() });
        cache.accept(GitAnswer::Repo {
            dir: root.clone(),
            outcome: Ok(root.clone()),
        });
        let commit = |subject: &str| GitCommit {
            hash: subject.to_owned(),
            short: subject.to_owned(),
            subject: subject.to_owned(),
            author_name: "T".to_owned(),
            author_email: "t@example.com".to_owned(),
            committer_name: "T".to_owned(),
            committer_email: "t@example.com".to_owned(),
            body: String::new(),
            committer_unix: RECORDED_AT,
            committer_offset: 0,
            time_relative: "now".to_owned(),
            parents: Vec::new(),
            refs: Vec::new(),
        };
        cache.accept(GitAnswer::Log {
            root: root.clone(),
            skip: 0,
            outcome: Ok(GitLog {
                skip: 0,
                commits: vec![commit("one"), commit("two")],
                has_more: true,
            }),
        });
        assert_eq!(
            cache.more_commits(),
            Some(GitQuestion::Log {
                root: root.clone(),
                refs: Vec::new(),
                skip: 2,
                count: GIT_LOG_PAGE,
            })
        );
        assert!(cache.accept(GitAnswer::Log {
            root: root.clone(),
            skip: 2,
            outcome: Ok(GitLog {
                skip: 2,
                commits: vec![commit("three")],
                has_more: false,
            }),
        }));
        let log = cache.log().ready().expect("the pages are filed");
        assert_eq!(log.commits.len(), 3);
        assert!(!log.has_more);
        assert_eq!(
            cache.more_commits(),
            None,
            "there is no page after the last one"
        );

        assert!(
            !cache.accept(GitAnswer::Log {
                root: root.clone(),
                skip: 99,
                outcome: Ok(GitLog {
                    skip: 99,
                    commits: vec![commit("stray")],
                    has_more: false,
                }),
            }),
            "a page from a history that has moved under us is not appended to this one"
        );
        assert_eq!(cache.log().ready().map(|log| log.commits.len()), Some(3));
    }

    /// PIN — a refresh re-asks the three page questions and keeps the root.
    ///
    /// Re-probing would blank the page's own heading for the length of a
    /// subprocess every time a file was staged, and the root is the one answer
    /// staging cannot change.
    #[test]
    fn a_refresh_asks_again_without_forgetting_where_the_repository_is() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::default();
        cache.retarget(&root);
        cache.mark_pending(&GitQuestion::RepoProbe { dir: root.clone() });
        cache.accept(GitAnswer::Repo {
            dir: root.clone(),
            outcome: Ok(root.clone()),
        });
        for question in cache.pending_questions() {
            cache.mark_pending(&question);
        }
        cache.accept(GitAnswer::Status {
            root: root.clone(),
            outcome: Ok(parse_status(STATUS_Z)),
        });
        cache.accept(GitAnswer::Refs {
            root: root.clone(),
            outcome: Ok(parse_refs(REFS, RECORDED_AT)),
        });
        assert!(cache.status().ready().is_some());
        assert_eq!(
            cache.refs().ready().map(Vec::len),
            Some(6),
            "the six refs of the recording are filed under the root that was asked"
        );
        cache.refresh();
        assert!(
            cache.refs().ready().is_none(),
            "a refresh asks again rather than keeping an answer it has decided is stale"
        );
        assert_eq!(
            cache.root(),
            Some(root.as_path()),
            "the root survives a refresh"
        );
        assert_eq!(cache.pending_questions().len(), 3);
    }

    /// T5 (v2 ③) — the toolbar's refresh re-asks **exactly** the three questions
    /// a page is made of, asks each once, and **leaves the page standing** while
    /// it waits.
    ///
    /// Three claims, and each is a way this could have been wrong. The *count*:
    /// a refresh that asked twice would spend two subprocesses per press, and one
    /// that asked a fourth would be reading something the button did not promise.
    /// The *page*: `refresh` — the verb a write owes — takes the history down to
    /// `Reading the repository…`, which is honest after a `git add` and is the
    /// page punishing the reader after a press that only meant "check". The
    /// *head*: it goes quiet while the three are out and comes back when the last
    /// of them lands, which is the whole of the feedback this button gets.
    #[test]
    fn a_reread_asks_the_three_page_questions_once_and_leaves_the_page_up() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::at_root(root.clone(), GitRole::Graph);
        for question in cache.pending_questions() {
            cache.mark_pending(&question);
        }
        cache.accept(GitAnswer::Status {
            root: root.clone(),
            outcome: Ok(parse_status(STATUS_Z)),
        });
        cache.accept(GitAnswer::Refs {
            root: root.clone(),
            outcome: Ok(parse_refs(REFS, RECORDED_AT)),
        });
        cache.accept(GitAnswer::Log {
            root: root.clone(),
            skip: 0,
            outcome: Ok(parse_log(LOG_Z, RECORDED_AT, 0, GIT_LOG_PAGE)),
        });
        assert!(cache.pending_questions().is_empty());
        assert!(!cache.rereading(), "nothing is owed yet");

        let asked = cache.begin_reread();
        assert_eq!(
            asked,
            vec![
                GitQuestion::Status { root: root.clone() },
                GitQuestion::Refs { root: root.clone() },
                GitQuestion::Log {
                    root: root.clone(),
                    refs: Vec::new(),
                    skip: 0,
                    count: GIT_LOG_PAGE,
                },
            ],
            "the status, the refs and the first page — and nothing else"
        );
        assert!(
            cache.rereading(),
            "and the head goes quiet while they are out"
        );
        assert!(
            cache.log().ready().is_some() && cache.refs().ready().is_some(),
            "the page a reader is looking at is still there"
        );
        assert!(
            cache.pending_questions().is_empty(),
            "and each of them once: nothing is owed by the driver on top of these"
        );
        // A second press while the first is still out is the same press.
        assert_eq!(cache.begin_reread().len(), 3);

        cache.accept(GitAnswer::Status {
            root: root.clone(),
            outcome: Ok(parse_status(STATUS_Z)),
        });
        cache.accept(GitAnswer::Refs {
            root: root.clone(),
            outcome: Ok(parse_refs(REFS, RECORDED_AT)),
        });
        assert!(
            cache.rereading(),
            "two of three back is still a page being read"
        );
        cache.accept(GitAnswer::Log {
            root,
            skip: 0,
            outcome: Ok(parse_log(LOG_Z, RECORDED_AT, 0, GIT_LOG_PAGE)),
        });
        assert!(!cache.rereading(), "and the last one hands the ink back");
    }

    /// PIN (R31's third invalidation moment, A) — **a command that ended
    /// somewhere else is not news about this repository**, and "somewhere else"
    /// is decided on path components rather than on characters.
    ///
    /// The `C:\a\bc` case is the one this exists for. Its characters begin with
    /// the characters of `C:\a\b`, so a string prefix says it is inside — and a
    /// trigger that believed it would re-read a repository every time a command
    /// finished in the directory *next door* to it, which is a wrong answer that
    /// only ever appears on somebody else's disk.
    #[test]
    fn a_command_re_reads_only_the_repository_it_was_run_inside() {
        let root = Path::new(r"C:\a\b");

        assert!(
            should_reread(root, Some(Path::new(r"C:\a\b")), true),
            "the root itself is inside the root"
        );
        assert!(
            should_reread(root, Some(Path::new(r"C:\a\b\c\d")), true),
            "and so is anywhere under it, however deep"
        );
        assert!(
            !should_reread(root, Some(Path::new(r"C:\a\bc")), true),
            "but not the folder next door whose name merely starts the same way"
        );
        assert!(
            !should_reread(root, Some(Path::new(r"C:\a")), true),
            "and not the folder above it: a parent is not inside its child"
        );
        assert!(
            !should_reread(root, Some(Path::new(r"D:\a\b")), true),
            "nor the same path on another drive"
        );

        // Windows is case-insensitive about all of it, which is the same rule
        // `repo_prefix` keeps for the badges — `d:\repo` and `D:\repo` are one
        // place, and a trigger that disagreed with the tree about that would be
        // wrong exactly where nobody would think to look.
        assert_eq!(
            should_reread(Path::new(r"d:\Repo"), Some(Path::new(r"D:\repo\src")), true),
            cfg!(windows)
        );

        // And the two conditions that are not about the path at all.
        assert!(
            !should_reread(root, Some(Path::new(r"C:\a\b")), false),
            "a page nobody is looking at is never read, whatever happened in any \
             shell — R31's gate, kept a second time"
        );
        assert!(
            !should_reread(root, None, true),
            "and a shell that has never reported a folder is answered no rather \
             than guessed at"
        );
    }

    /// PIN (R31's third invalidation moment, coalescing) — **a burst is one
    /// piece of news**: while any answer is owed, an automatic re-read asks
    /// nothing at all.
    ///
    /// Ten commands pasted into a shell end ten times. Ten unguarded re-reads
    /// would be thirty subprocesses spent to learn what the first three were
    /// already on their way to say — the polling R31 forbids, arriving by another
    /// door. The guard is the cache's own [`GitCache::reading`] and therefore
    /// also covers the *first* reading of a page: a `Pending` status is a
    /// question already out for exactly the answer this would ask for again.
    ///
    /// The button is deliberately not guarded. A person pressing refresh twice
    /// means it twice.
    #[test]
    fn an_automatic_reread_does_not_stack_on_one_that_is_still_owed() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::at_root(root.clone(), GitRole::Page);

        // The first reading is out: three questions are Pending and nothing else
        // may ask for them.
        for question in cache.pending_questions() {
            cache.mark_pending(&question);
        }
        assert!(cache.reading(), "the page's own first reading is a reading");
        assert!(
            cache.begin_reread_unless_owed().is_empty(),
            "a command that ends while the page is still loading asks nothing"
        );

        let answer_all = |cache: &mut GitCache| {
            cache.accept(GitAnswer::Status {
                root: root.clone(),
                outcome: Ok(parse_status(STATUS_Z)),
            });
            cache.accept(GitAnswer::Refs {
                root: root.clone(),
                outcome: Ok(parse_refs(REFS, RECORDED_AT)),
            });
            cache.accept(GitAnswer::Log {
                root: root.clone(),
                skip: 0,
                outcome: Ok(parse_log(LOG_Z, RECORDED_AT, 0, GIT_LOG_PAGE)),
            });
        };
        answer_all(&mut cache);
        assert!(!cache.reading(), "settled");

        assert_eq!(
            cache.begin_reread_unless_owed().len(),
            3,
            "settled, so the first command end asks the three"
        );
        for _ in 0..9 {
            assert!(
                cache.begin_reread_unless_owed().is_empty(),
                "and the nine behind it ask nothing while those three are owed"
            );
        }
        assert_eq!(
            cache.begin_reread().len(),
            3,
            "the button is not the burst: a person pressing it means it"
        );

        answer_all(&mut cache);
        assert!(!cache.reading());
        assert_eq!(
            cache.begin_reread_unless_owed().len(),
            3,
            "and once the answers are in, the next thing that happens is heard"
        );
    }

    /// T2/T3 — changing the filter throws the history away and re-asks it with
    /// the revisions the filter names.
    ///
    /// **Thrown away and not filtered in place**: a branch that was not walked
    /// has no commits on hand, so there is nothing already loaded that could have
    /// answered this. The `Load more` question carries the same revisions, or the
    /// second page would be of a different history from the first.
    #[test]
    fn a_new_filter_re_asks_the_history_with_the_revisions_it_names() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::at_root(root.clone(), GitRole::Graph);
        let commit = |subject: &str| GitCommit {
            hash: subject.to_owned(),
            short: subject.to_owned(),
            subject: subject.to_owned(),
            author_name: "T".to_owned(),
            author_email: "t@example.com".to_owned(),
            committer_name: "T".to_owned(),
            committer_email: "t@example.com".to_owned(),
            body: String::new(),
            committer_unix: RECORDED_AT,
            committer_offset: 0,
            time_relative: "now".to_owned(),
            parents: Vec::new(),
            refs: Vec::new(),
        };
        for question in cache.pending_questions() {
            cache.mark_pending(&question);
        }
        cache.accept(GitAnswer::Log {
            root: root.clone(),
            skip: 0,
            outcome: Ok(GitLog {
                skip: 0,
                commits: vec![commit("one"), commit("two")],
                has_more: true,
            }),
        });
        assert_eq!(cache.log_refs(), Vec::<String>::new());

        assert!(cache.set_log_refs(vec!["main".to_owned(), "side".to_owned()]));
        assert!(
            cache.log().ready().is_none(),
            "a filter change is a different history, not a view of this one"
        );
        assert!(cache.pending_questions().contains(&GitQuestion::Log {
            root: root.clone(),
            refs: vec!["main".to_owned(), "side".to_owned()],
            skip: 0,
            count: GIT_LOG_PAGE,
        }));
        assert!(
            !cache.set_log_refs(vec!["main".to_owned(), "side".to_owned()]),
            "the same filter changes nothing and re-asks nothing"
        );

        // And the page after it walks the same roads.
        for question in cache.pending_questions() {
            cache.mark_pending(&question);
        }
        cache.accept(GitAnswer::Log {
            root: root.clone(),
            skip: 0,
            outcome: Ok(GitLog {
                skip: 0,
                commits: vec![commit("one")],
                has_more: true,
            }),
        });
        assert_eq!(
            cache.more_commits(),
            Some(GitQuestion::Log {
                root,
                refs: vec!["main".to_owned(), "side".to_owned()],
                skip: 1,
                count: GIT_LOG_PAGE,
            })
        );
    }

    /// T4 — a search's answer is filed only while it is about the text that is
    /// still being asked.
    #[test]
    fn a_search_result_for_a_query_already_typed_past_is_dropped() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::at_root(root.clone(), GitRole::Graph);
        assert_eq!(cache.search("fix"), None, "nothing has been asked");
        assert_eq!(
            cache.begin_search("fix"),
            Some(GitQuestion::Search {
                root: root.clone(),
                query: "fix".to_owned(),
            })
        );
        assert!(matches!(cache.search("fix"), Some(GitSlot::Pending)));
        // The reader types on before the first answer lands.
        cache.begin_search("fixup");
        assert!(
            !cache.accept(GitAnswer::Search {
                root: root.clone(),
                query: "fix".to_owned(),
                outcome: Ok(vec!["a".to_owned()]),
            }),
            "an answer about text nobody is asking about any more is not filed"
        );
        assert!(cache.accept(GitAnswer::Search {
            root,
            query: "fixup".to_owned(),
            outcome: Ok(vec!["b".to_owned()]),
        }));
        assert_eq!(
            cache.search("fixup").and_then(GitSlot::ready),
            Some(&vec!["b".to_owned()])
        );
        assert_eq!(cache.search("fix"), None, "and only under its own key");
        cache.clear_search();
        assert_eq!(cache.search("fixup"), None);
    }

    #[test]
    fn a_stopped_worker_is_announced_once_and_then_never_again() {
        let mut running = true;
        let mut pending = false;
        assert!(disable_git_worker_state(&mut running, &mut pending));
        assert!(!disable_git_worker_state(&mut running, &mut pending));
        assert_eq!(
            take_git_worker_notice(&mut pending),
            Some(git_worker_stopped_notice())
        );
        assert_eq!(take_git_worker_notice(&mut pending), None);
    }

    // ── The real repository ────────────────────────────────────────────────

    /// PIN — the three page questions, put to this workspace's own checkout.
    ///
    /// The parsers above are fed recordings, which is what makes them testable at
    /// all; this is the test that the recordings are of the right thing. It asks
    /// the real `git.exe` about the real repository these lines are in, and
    /// asserts only what is true of any checkout of it — that it has a history,
    /// that it has branches, that `HEAD` is on one of them, and that the status
    /// parses. It deliberately does not assert a clean tree: the repository is
    /// dirty exactly when somebody is working in it, which is whenever this runs.
    #[test]
    fn this_workspace_answers_all_three_page_questions() {
        let git = real_git();
        let root = this_repository();
        let ask = |question| answer(&git, &question, GIT_COMMAND_TIMEOUT, now_unix());

        let GitAnswer::Status { outcome, .. } = ask(GitQuestion::Status { root: root.clone() })
        else {
            panic!("a status question is answered with a status");
        };
        let status = outcome.expect("this workspace's status reads");
        assert!(
            status.branch.is_some() || status.detached,
            "HEAD is on a branch or it is detached, and it says which"
        );

        let GitAnswer::Refs { outcome, .. } = ask(GitQuestion::Refs { root: root.clone() }) else {
            panic!("a refs question is answered with refs");
        };
        let refs = outcome.expect("this workspace's refs read");
        let branches: Vec<&GitRefEntry> = local_branches(&refs).collect();
        assert!(
            !branches.is_empty(),
            "a repository with commits has a branch"
        );
        assert!(
            branches.iter().filter(|branch| branch.is_head).count() <= 1,
            "HEAD is on at most one local branch"
        );
        assert!(
            refs.iter().all(|entry| !entry.name.starts_with("refs/")),
            "every name arrived short, whichever tree it came out of"
        );

        let GitAnswer::Log { outcome, .. } = ask(GitQuestion::Log {
            root: root.clone(),
            refs: Vec::new(),
            skip: 0,
            count: GIT_LOG_PAGE,
        }) else {
            panic!("a log question is answered with a log");
        };
        let page = outcome.expect("this workspace's history reads");
        assert_eq!(page.commits.len(), GIT_LOG_PAGE, "a full first page");
        assert!(page.has_more, "this repository has more than fifty commits");
        assert!(
            page.commits
                .iter()
                .all(|commit| commit.hash.len() == 40 && !commit.short.is_empty()),
            "every commit has a full hash and an abbreviation"
        );
        assert!(
            page.commits.iter().any(|commit| commit.parents.len() > 1)
                || page.commits.iter().all(|commit| !commit.parents.is_empty()),
            "every commit but the root has a parent"
        );
    }

    /// T4 (v2 ③) — a search is an **OR** of the message and the author, plus a
    /// direct hit on anything git can resolve as a revision.
    ///
    /// Put to the real `git.exe` and the real repository these lines are in,
    /// because the whole of this question is *what three command lines do*: git
    /// ANDs `--grep` with `--author`, so the union has to be built here, and the
    /// only way to find out that it was is to run the commands.
    ///
    /// It asserts nothing about how many commits match — that is a fact about
    /// whoever has been working here — only the three properties the union has
    /// to have: an author-only query answers, a message-only query answers, and a
    /// hash resolves to itself and leads the list.
    #[test]
    fn a_search_ors_the_message_against_the_author_and_a_hash_lands_outright() {
        let git = real_git();
        let root = this_repository();
        let ask = |question| answer(&git, &question, GIT_COMMAND_TIMEOUT, now_unix());
        let search = |query: &str| {
            let GitAnswer::Search { outcome, .. } = ask(GitQuestion::Search {
                root: root.clone(),
                query: query.to_owned(),
            }) else {
                panic!("a search question is answered with a search");
            };
            outcome.expect("this workspace's history is searchable")
        };
        // The author of every commit in this repository, off the first page.
        let GitAnswer::Log { outcome, .. } = ask(GitQuestion::Log {
            root: root.clone(),
            refs: Vec::new(),
            skip: 0,
            count: 1,
        }) else {
            panic!("a log question is answered with a log");
        };
        let page = outcome.expect("this workspace's history reads");
        let head = page.commits.first().expect("a repository with commits");

        // The author's name matches no commit *message* in this repository's
        // subjects — and it does not have to: what this asserts is that the
        // author half of the union runs at all, which a `--grep`-only search
        // could not answer.
        let by_author = search(&head.author_name);
        assert!(
            by_author.contains(&head.hash),
            "a query that is only ever an author still finds their commits"
        );

        // A full hash resolves outright and leads the list, because the direct
        // jump is asked first: pasting a hash is a *jump* and not a search.
        let by_hash = search(&head.hash);
        assert_eq!(
            by_hash.first(),
            Some(&head.hash),
            "a revision this repository knows lands on itself, first"
        );
        // And so does the abbreviation git itself printed, which is the form a
        // reader actually copies off a row.
        let by_short = search(&head.short);
        assert_eq!(by_short.first(), Some(&head.hash));

        // Nothing matches nothing, and that is an answer rather than a failure.
        assert!(
            search("zzq-no-such-text-anywhere-in-this-history-zzq").is_empty(),
            "a query nothing says is an empty list, not a refusal"
        );
    }

    // ── G-3 ────────────────────────────────────────────────────────────────

    /// **② A diff is a document, and documents do not live in a column's
    /// cache.**
    ///
    /// The two answers that carry a *body* — one file's diff, and one commit's
    /// diff of one file — are filed into the tab's preview pool against their
    /// own [`crate::preview::PreviewSource`], which is what makes the staged and
    /// unstaged readings of one file two buffers. A commit's *file list* is the
    /// opposite kind of answer: it is rows on the page, it is thrown away when
    /// the column re-roots, and it belongs here beside the status.
    ///
    /// MUTATION: give `GitCache` a `diff: GitSlot<String>` and file
    /// `GitAnswer::Diff` into it. One cache then holds one diff, so opening a
    /// second file's diff silently replaces the first — while the pool, which is
    /// what the panes actually read, still holds both.
    #[test]
    fn a_diff_is_a_document_and_a_commit_s_file_list_is_column_state() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = GitCache::default();
        cache.retarget(&root);
        assert!(cache.accept(GitAnswer::Repo {
            dir: root.clone(),
            outcome: Ok(root.clone()),
        }));
        let before = format!("{cache:?}");

        assert!(
            !cache.accept(GitAnswer::Diff {
                root: root.clone(),
                path: "src/main.rs".to_owned(),
                against: crate::preview::GitDiffAgainst::Index,
                outcome: Ok("diff --git a/src/main.rs b/src/main.rs\n".to_owned()),
            }),
            "a diff is not this cache's to keep"
        );
        assert!(
            !cache.accept(GitAnswer::Show {
                root: root.clone(),
                hash: "a".repeat(40),
                path: "src/main.rs".to_owned(),
                outcome: Ok("diff --git a/src/main.rs b/src/main.rs\n".to_owned()),
            }),
            "nor is one commit's reading of a file"
        );
        assert_eq!(
            before,
            format!("{cache:?}"),
            "and neither of them left a mark on it"
        );

        // The file list, on the other hand, is rows.
        let hash = "b".repeat(40);
        assert!(
            cache.commit_files(&hash).is_none(),
            "nothing is expanded until something asks"
        );
        let question = cache
            .begin_commit_files(&hash)
            .expect("a repository can be asked what a commit touched");
        assert_eq!(
            question,
            GitQuestion::CommitFiles {
                root: root.clone(),
                hash: hash.clone(),
            }
        );
        assert!(
            matches!(cache.commit_files(&hash), Some(GitSlot::Pending)),
            "and the slot says the question is out"
        );
        assert!(cache.accept(GitAnswer::CommitFiles {
            root: root.clone(),
            hash: hash.clone(),
            outcome: Ok(vec![GitCommitFile {
                path: "src/main.rs".to_owned(),
                code: StatusCode::Modified,
                renamed_from: None,
                stat: None,
            }]),
        }));
        assert_eq!(
            cache
                .commit_files(&hash)
                .and_then(GitSlot::ready)
                .map(Vec::len),
            Some(1)
        );
        // One expansion at a time: asking about another commit forgets the
        // first, because the page only ever has one open (R15).
        let other = "c".repeat(40);
        assert!(cache.begin_commit_files(&other).is_some());
        assert!(
            cache.commit_files(&hash).is_none(),
            "the shut commit's list is not kept warm for a press that may never come"
        );
    }

    /// D6 — a comparison is asked **once per pair**, however many frames look at
    /// it.
    ///
    /// The guard is in the cache and not in the caller because the compare block
    /// is derived at paint: the question is reached from a build that runs every
    /// frame, so "have I asked this already" has to be answerable by the thing
    /// that holds the answer.
    ///
    /// MUTATION: drop the pair check in `begin_compare_files` and this goes red
    /// on the second call — which on the real machine is a `git diff` per frame.
    #[test]
    fn a_comparison_is_asked_once_per_pair_and_again_when_an_end_moves() {
        let root = std::path::PathBuf::from(r"D:\repo");
        let mut cache = GitCache::at_root(root.clone(), GitRole::Graph);
        let (a, b) = ("a".repeat(40), "b".repeat(40));
        assert_eq!(
            cache.begin_compare_files(&a, Some(&b)),
            Some(GitQuestion::CompareFiles {
                root: root.clone(),
                a: a.clone(),
                b: Some(b.clone()),
            })
        );
        assert_eq!(
            cache.begin_compare_files(&a, Some(&b)),
            None,
            "the same pair, a frame later, is not a second subprocess"
        );
        assert!(matches!(
            cache.compare_files(&a, Some(&b)),
            Some(GitSlot::Pending)
        ));
        assert!(cache.accept(GitAnswer::CompareFiles {
            root: root.clone(),
            a: a.clone(),
            b: Some(b.clone()),
            outcome: Ok(vec![GitCommitFile {
                path: "src/main.rs".to_owned(),
                code: StatusCode::Modified,
                renamed_from: None,
                stat: Some(GitFileStat {
                    added: 12,
                    removed: 3,
                }),
            }]),
        }));
        assert_eq!(
            cache
                .compare_files(&a, Some(&b))
                .and_then(GitSlot::ready)
                .map(Vec::len),
            Some(1)
        );
        // The working tree is a *different* far end, not the same one spelled
        // another way.
        assert!(cache.begin_compare_files(&a, None).is_some());
        assert!(
            cache.compare_files(&a, Some(&b)).is_none(),
            "one compare block, one pair in hand"
        );
    }

    /// A recording of `git show --raw --numstat -z --format=` from the real
    /// machine (2026-08-16), covering the three shapes that are not one plain
    /// record per file: a rename, a binary file, and the fact that the stream is
    /// two blocks rather than one.
    ///
    /// Taken verbatim off a scratch repository built for it — one file added,
    /// one binary rewritten, one text file renamed and edited — because the one
    /// thing a hand-written fixture cannot pin is the grammar git actually uses,
    /// and the rename's *extra* NUL after the second tab is exactly the kind of
    /// detail a hand-written fixture would have left out.
    const SHOW_RAW_NUMSTAT_Z: &[u8] = concat!(
        ":000000 100644 000000 587be6b A\u{0}added.txt\u{0}",
        ":100644 100644 0f49c4a 53aa893 M\u{0}bin.dat\u{0}",
        ":100644 100644 de98044 d68dd40 R075\u{0}old.txt\u{0}new.txt\u{0}",
        "1\t0\tadded.txt\u{0}",
        "-\t-\tbin.dat\u{0}",
        "1\t0\t\u{0}old.txt\u{0}new.txt\u{0}",
    )
    .as_bytes();

    /// `git show --raw --numstat -z --format=`, decoded — including the shapes
    /// that are not one record per file.
    #[test]
    fn a_commit_s_file_list_reads_letters_paths_and_where_a_rename_came_from() {
        let files = parse_diff_files(SHOW_RAW_NUMSTAT_Z);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "added.txt");
        assert_eq!(files[0].code, StatusCode::Added);
        assert_eq!(files[1].path, "bin.dat");
        assert_eq!(files[1].code, StatusCode::Modified);
        // A rename spends three fields, and the **new** path is the one the row
        // is about — the same grammar `parse_status` already reads.
        assert_eq!(files[2].code, StatusCode::Renamed);
        assert_eq!(files[2].path, "new.txt");
        assert_eq!(files[2].renamed_from.as_deref(), Some("old.txt"));

        // A merge commit's `--raw` is empty against its first parent, and empty
        // is an answer rather than a parse failure.
        assert!(parse_diff_files(b"").is_empty());
    }

    /// D4 — the counts arrive in the same breath as the letters, and a file git
    /// would not count says so rather than saying zero.
    ///
    /// MUTATION: give the binary row `Some(GitFileStat::default())` and the row
    /// claims nothing changed about a file that was replaced whole.
    #[test]
    fn the_numstat_block_lands_on_the_rows_the_raw_block_built() {
        let files = parse_diff_files(SHOW_RAW_NUMSTAT_Z);
        assert_eq!(
            files[0].stat,
            Some(GitFileStat {
                added: 1,
                removed: 0
            })
        );
        assert_eq!(
            files[1].stat, None,
            "`-\t-` is `git has no lines here`, which is not `nothing changed`"
        );
        // A rename's numstat record leaves its own path field empty and writes
        // the two names after it; the counts belong to the name it went to.
        assert_eq!(
            files[2].stat,
            Some(GitFileStat {
                added: 1,
                removed: 0
            })
        );
    }

    /// **A staged rename's diff is a rename, and only both names make it one.**
    ///
    /// Measured against real git rather than asserted about an argument list,
    /// because the fact being pinned is git's own behaviour: `git diff --cached
    /// -- <new path>` prints `new file mode`, and the same command given both
    /// halves prints `rename from` / `rename to`. Rename detection compares a
    /// *pair*, and a pathspec naming one half hides the other.
    ///
    /// What it costs to get wrong is a row wearing an `R` badge opening a diff
    /// that says the file was just created — the panel and the diff disagreeing
    /// on one screen, which is the failure §7 of the backend adjudication chose
    /// the CLI to prevent.
    ///
    /// MUTATION: drop the `renamed_from` push from the `Diff` branch of
    /// [`answer`]. The second half of this test starts printing `new file mode`
    /// like the first.
    #[test]
    fn a_staged_rename_needs_both_names_to_read_as_a_rename() {
        let git = real_git();
        let root = std::env::temp_dir().join(format!(
            "folio-git-rename-{}-{}",
            std::process::id(),
            RECORDED_AT
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch folder can be made");
        let run = |arguments: &[&str]| {
            let owned: Vec<&OsStr> = arguments.iter().map(OsStr::new).collect();
            run_git(git_command(&git, &root, &owned), GIT_COMMAND_TIMEOUT)
                .expect("the scratch repository is built with the same git under test")
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.name", "folio-test"]);
        run(&["config", "user.email", "folio@example.invalid"]);
        std::fs::write(root.join("was.txt"), "one\ntwo\nthree\n").expect("a file can be written");
        run(&["add", "was.txt"]);
        run(&["commit", "-q", "-m", "seed"]);
        run(&["mv", "was.txt", "renamed.txt"]);

        let ask = |renamed_from: Option<&str>| {
            let question = GitQuestion::Diff {
                root: root.clone(),
                path: "renamed.txt".to_owned(),
                against: crate::preview::GitDiffAgainst::Index,
                renamed_from: renamed_from.map(str::to_owned),
            };
            let GitAnswer::Diff { outcome, .. } =
                answer(&git, &question, GIT_COMMAND_TIMEOUT, RECORDED_AT)
            else {
                panic!("a diff question is answered with a diff");
            };
            outcome.expect("the scratch repository answers")
        };

        let half = ask(None);
        let whole = ask(Some("was.txt"));
        let _ = std::fs::remove_dir_all(&root);

        assert!(
            half.contains("new file mode"),
            "git's own behaviour, and the reason the field exists: one name \
             turns a rename into an addition — {half}"
        );
        assert!(
            whole.contains("rename from was.txt") && whole.contains("rename to renamed.txt"),
            "and both names get the rename git already knows about — {whole}"
        );
        assert!(
            !whole.contains("new file mode"),
            "with nothing left claiming the file was created — {whole}"
        );
    }

    /// **The documents a change list can open, against a real repository** —
    /// R25's mapping and G-3's honest states, end to end.
    ///
    /// Every claim here is checked against what a person typing the command in
    /// the pane beside the panel would see, which is the whole argument for the
    /// CLI backend: each reading is compared **byte for byte** with the command
    /// line it stands for, not merely inspected for plausibility.
    ///
    /// **The untracked reading is the one this test was reopened for** (user
    /// report, 2026-08-17). It used to assert that an untracked file's document
    /// is *empty* — which was true of the command being run and false of the
    /// question being asked. A file git has never had a copy of has no working
    /// tree/index difference to report, so the page said "No changes to show"
    /// about a file that is nothing but change. The reading it gets now is the
    /// whole file against nothing, and the last three cases hold it for the
    /// three names that break naive quoting: a subdirectory, a space, and an
    /// ideograph.
    ///
    /// MUTATION: map `GitGroup::Untracked` to `GitDiffAgainst::WorkingTree` in
    /// `diff_against`. Every untracked assertion below goes empty at once.
    #[test]
    fn a_real_repository_answers_every_way_and_each_matches_the_command_line() {
        let git = real_git();
        let root = std::env::temp_dir().join(format!(
            "folio-git-diff-{}-{}",
            std::process::id(),
            RECORDED_AT
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch folder can be made");
        let run = |arguments: &[&str]| {
            let owned: Vec<&OsStr> = arguments.iter().map(OsStr::new).collect();
            run_git(git_command(&git, &root, &owned), GIT_COMMAND_TIMEOUT)
                .expect("the scratch repository is built with the same git under test")
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.name", "folio-test"]);
        run(&["config", "user.email", "folio@example.invalid"]);
        std::fs::write(root.join("text.txt"), "one\ntwo\nthree\n").expect("a file is written");
        // A NUL in the first bytes is what makes git call a file binary.
        std::fs::write(root.join("blob.bin"), [0u8, 1, 2, 3, 4, 5, 6, 7])
            .expect("a file is written");
        run(&["add", "text.txt", "blob.bin"]);
        run(&["commit", "-q", "-m", "seed"]);
        // Staged: a line appended and packed. Unstaged: another line on top of
        // it, so the two readings are genuinely different documents.
        std::fs::write(root.join("text.txt"), "one\ntwo\nthree\nstaged\n").expect("written");
        run(&["add", "text.txt"]);
        std::fs::write(root.join("text.txt"), "one\ntwo\nthree\nstaged\nworking\n")
            .expect("written");
        std::fs::write(root.join("blob.bin"), [0u8, 9, 9, 9]).expect("written");
        std::fs::write(root.join("fresh.txt"), "never seen\n").expect("written");
        // The three names a naive command line loses: one inside a folder, one
        // with a space in it, one written in characters that are not ASCII.
        std::fs::create_dir_all(root.join("sub/deep")).expect("a folder is made");
        std::fs::write(root.join("sub/deep/nested.txt"), "in a folder\n").expect("written");
        std::fs::write(root.join("a name with spaces.txt"), "spaced out\n").expect("written");
        std::fs::write(root.join("\u{4e2d}\u{6587}.txt"), "\u{4e2d}\u{6587}\n").expect("written");

        let ask = |path: &str, against: crate::preview::GitDiffAgainst| {
            let question = GitQuestion::Diff {
                root: root.clone(),
                path: path.to_owned(),
                against,
                renamed_from: None,
            };
            let GitAnswer::Diff { outcome, .. } =
                answer(&git, &question, GIT_COMMAND_TIMEOUT, RECORDED_AT)
            else {
                panic!("a diff question is answered with a diff");
            };
            outcome.expect("the scratch repository answers")
        };
        let command_line =
            |arguments: &[&str]| String::from_utf8_lossy(&run(arguments).stdout).into_owned();

        // ① R25's mapping: a STAGED row's document *is* `git diff --cached`.
        let staged = ask("text.txt", crate::preview::GitDiffAgainst::Index);
        assert_eq!(
            staged,
            command_line(&["diff", "--no-color", "--cached", "--", "text.txt"]),
            "the index's reading, byte for byte with the command the pane beside \
             it would run"
        );
        assert!(staged.contains("+staged") && !staged.contains("+working"));

        // ② And a CHANGES row's is the working tree's — a different document.
        let unstaged = ask("text.txt", crate::preview::GitDiffAgainst::WorkingTree);
        assert_eq!(
            unstaged,
            command_line(&["diff", "--no-color", "--", "text.txt"])
        );
        assert!(unstaged.contains("+working") && !unstaged.contains("+staged"));
        assert_ne!(staged, unstaged, "two readings, two documents (R25)");

        // ③ The binary honesty: git's own one line, and nothing invented.
        let binary = ask("blob.bin", crate::preview::GitDiffAgainst::WorkingTree);
        assert!(
            binary.contains("Binary files a/blob.bin and b/blob.bin differ"),
            "git says it in one line — {binary}"
        );
        assert!(
            !binary.contains("@@"),
            "and there is no hunk, because there is nothing to show line by line"
        );

        // ④ **An untracked file is a whole file of additions**, and asking the
        // working tree/index question about it is what used to answer nothing.
        let asked_the_old_way = ask("fresh.txt", crate::preview::GitDiffAgainst::WorkingTree);
        assert!(
            asked_the_old_way.is_empty(),
            "the question that had no answer still has none — this is the bug, \
             recorded rather than removed"
        );
        let fresh = ask("fresh.txt", crate::preview::GitDiffAgainst::Nothing);
        assert!(
            fresh.contains("+never seen"),
            "and the question that has an answer gets the file, line by line — {fresh}"
        );
        assert!(
            fresh.contains("--- /dev/null"),
            "against nothing, which is what makes every line an addition — {fresh}"
        );
        assert!(
            !fresh
                .lines()
                .any(|line| line.starts_with('-') && !line.starts_with("---")),
            "and nothing is removed, because there was nothing there — {fresh}"
        );

        // ⑤ The three awkward names, each through the same door. A path is one
        // argument to `CreateProcess` and never a word in a shell, so a space
        // needs no quoting; `core.quotepath=false` is what keeps the ideograph
        // out of octal escapes in what comes back.
        for (path, line) in [
            ("sub/deep/nested.txt", "+in a folder"),
            ("a name with spaces.txt", "+spaced out"),
            ("\u{4e2d}\u{6587}.txt", "+\u{4e2d}\u{6587}"),
        ] {
            let whole = ask(path, crate::preview::GitDiffAgainst::Nothing);
            assert!(
                whole.contains(line),
                "`{path}` reads as its own contents — {whole}"
            );
            assert!(
                whole.contains(path),
                "and git names it the way the row does, unescaped — {whole}"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// **The command line each of the three readings is** — no subprocess, so
    /// the argv itself is the assertion.
    ///
    /// [`write_arguments`]'s own rule, applied to the read side: a command built
    /// inside a `match` in a function that needs a repository to run is a
    /// command nothing can read. The untracked arm in particular has to be
    /// *seen* — `--no-index` with `/dev/null` in front of the path is not a flag
    /// on the other two commands, it is a different command, and that is the
    /// whole of the ruling.
    ///
    /// MUTATION: drop `/dev/null` from the untracked arm. `git diff --no-index`
    /// with one operand is a usage error, and the pane gets git's complaint
    /// where its file should be.
    #[test]
    fn each_of_the_three_readings_is_its_own_command_line() {
        use crate::preview::GitDiffAgainst;
        assert_eq!(
            diff_arguments(GitDiffAgainst::Index, "src/main.rs", None),
            ["diff", "--no-color", "--cached", "--", "src/main.rs"]
        );
        assert_eq!(
            diff_arguments(GitDiffAgainst::WorkingTree, "src/main.rs", None),
            ["diff", "--no-color", "--", "src/main.rs"]
        );
        assert_eq!(
            diff_arguments(GitDiffAgainst::Nothing, "src/main.rs", None),
            [
                "diff",
                "--no-color",
                "--no-index",
                "--",
                "/dev/null",
                "src/main.rs"
            ]
        );
        // Both halves of a rename, in git's own order, on the two readings a
        // rename can happen in.
        assert_eq!(
            diff_arguments(GitDiffAgainst::Index, "new.txt", Some("old.txt")),
            ["diff", "--no-color", "--cached", "--", "old.txt", "new.txt"]
        );
        assert_eq!(
            diff_arguments(GitDiffAgainst::WorkingTree, "new.txt", Some("old.txt")),
            ["diff", "--no-color", "--", "old.txt", "new.txt"]
        );
        // And never on the third: `--no-index` takes two operands and not a
        // pathspec, and a file git has never seen was never renamed.
        assert_eq!(
            diff_arguments(GitDiffAgainst::Nothing, "new.txt", Some("old.txt")),
            [
                "diff",
                "--no-color",
                "--no-index",
                "--",
                "/dev/null",
                "new.txt"
            ]
        );
        // A name with a space and a name with an ideograph are each one
        // argument, whole, and nothing quotes or escapes them on the way.
        for path in [
            "a name with spaces.txt",
            "\u{4e2d}\u{6587}.txt",
            "sub/deep/n.txt",
        ] {
            for against in [
                GitDiffAgainst::Index,
                GitDiffAgainst::WorkingTree,
                GitDiffAgainst::Nothing,
            ] {
                let words = diff_arguments(against, path, None);
                assert_eq!(
                    words.last().map(String::as_str),
                    Some(path),
                    "{against:?} hands `{path}` over as itself"
                );
            }
        }
    }

    /// **The one comparison whose exit code is a verdict and not a failure.**
    ///
    /// `git diff --no-index` reports the way `cmp` does: 0 for the same, 1 for
    /// different. Every untracked file is different from `/dev/null`, so 1 is
    /// the ordinary outcome — and reading it as a failure would put git's silent
    /// exit under a refusal card on the page. Every other code, and every
    /// non-zero from the other two readings, is still a fault.
    ///
    /// MUTATION: return `true` from `diff_differed` for any non-zero code. A
    /// repository git refused to read stops saying so and shows an empty patch.
    #[test]
    fn only_the_untracked_reading_may_exit_one() {
        use crate::preview::GitDiffAgainst;
        let ran = |code: Option<i32>| GitRun {
            ok: code == Some(0),
            code,
            stdout: Vec::new(),
            stderr: String::new(),
        };
        assert!(diff_differed(GitDiffAgainst::Nothing, &ran(Some(1))));
        assert!(!diff_differed(GitDiffAgainst::Nothing, &ran(Some(128))));
        assert!(!diff_differed(GitDiffAgainst::Nothing, &ran(None)));
        for against in [GitDiffAgainst::Index, GitDiffAgainst::WorkingTree] {
            assert!(
                !diff_differed(against, &ran(Some(1))),
                "{against:?} has no verdict to report in its exit code"
            );
        }
    }

    // ── v2 ④: the named write verbs, and the boundary they sit inside ──────

    /// Every verb this window can build, so that a test which enumerates them
    /// really does enumerate them.
    ///
    /// A hand-written list and not a derive, which is the point: adding a
    /// [`GitWriteVerb`] and forgetting this list makes the count assertion below
    /// go red, and the count assertion is the tripwire for the boundary pins
    /// underneath it.
    fn every_write_verb() -> Vec<GitWriteVerb> {
        vec![
            GitWriteVerb::Stage,
            GitWriteVerb::Unstage,
            GitWriteVerb::Discard,
            GitWriteVerb::DiscardUntracked,
            GitWriteVerb::CreateBranch {
                name: "feature".to_owned(),
                at: "a1b2c3d4".to_owned(),
            },
            GitWriteVerb::CreateTag {
                name: "v1.0".to_owned(),
                at: "a1b2c3d4".to_owned(),
            },
            GitWriteVerb::RenameBranch {
                from: "main".to_owned(),
                to: "trunk".to_owned(),
            },
            GitWriteVerb::DeleteBranch {
                name: "goner".to_owned(),
            },
            GitWriteVerb::DeleteTag {
                name: "v0.9".to_owned(),
            },
            GitWriteVerb::CheckoutTracking {
                name: "origin/feature".to_owned(),
            },
        ]
    }

    /// PIN (user ruling, v2 ④) — **the boundary is a list of words, and no
    /// command this window builds contains one of them.**
    ///
    /// The ruling that opened this slice is "read/navigate verbs and
    /// one-command-undoable local writes only"; the way that is kept honest is
    /// not a promise in a doc comment but this: enumerate every verb, build its
    /// real argument vector through the real function the worker calls, and read
    /// every word of it.
    ///
    /// MUTATION: add `--force` to any verb's vector — or add a `Reset` verb —
    /// and this goes red on the exact word.
    #[test]
    fn no_command_this_window_builds_carries_a_word_the_ruling_forbids() {
        let verbs = every_write_verb();
        assert_eq!(
            verbs.len(),
            10,
            "ten verbs — add one and this list has to grow with it, which is what \
             makes the pin below cover the new one"
        );
        let paths = vec!["src/main.rs".to_owned()];
        for verb in &verbs {
            let (words, _) = write_arguments(verb, if verb.moves_refs() { &[] } else { &paths });
            for word in &words {
                assert!(
                    !GIT_NEVER_WORDS.contains(&word.as_str()),
                    "{verb:?} builds {words:?}, which carries the forbidden word {word}"
                );
            }
            assert!(
                !words.is_empty(),
                "{verb:?} has to be some command, not none"
            );
        }
        // And the list itself is the ruling's own list, so that a reviewer who
        // reads only this test reads the whole boundary.
        assert_eq!(
            GIT_NEVER_WORDS,
            [
                "merge",
                "rebase",
                "reset",
                "cherry-pick",
                "revert",
                "push",
                "pull",
                "fetch",
                "-D",
                "--force",
            ]
        );
    }

    /// PIN (v2 ④) — **each named verb is the command line git documents**, with
    /// the name last and no flag between it and the subcommand that takes it.
    ///
    /// MUTATION: swap `-d` for `-D` on the branch deletion and both this and the
    /// pin above go red — one on the vector, one on the word.
    #[test]
    fn each_named_verb_is_the_command_line_git_documents() {
        let words = |verb: &GitWriteVerb| write_arguments(verb, &[]).0;
        assert_eq!(
            words(&GitWriteVerb::CreateBranch {
                name: "feature".to_owned(),
                at: "a1b2c3d4".to_owned(),
            }),
            vec!["branch", "feature", "a1b2c3d4"]
        );
        assert_eq!(
            words(&GitWriteVerb::CreateTag {
                name: "v1.0".to_owned(),
                at: "a1b2c3d4".to_owned(),
            }),
            vec!["tag", "v1.0", "a1b2c3d4"],
            "lightweight: no -a, no -m, no -s"
        );
        assert_eq!(
            words(&GitWriteVerb::RenameBranch {
                from: "main".to_owned(),
                to: "trunk".to_owned(),
            }),
            vec!["branch", "-m", "main", "trunk"]
        );
        assert_eq!(
            words(&GitWriteVerb::DeleteBranch {
                name: "goner".to_owned(),
            }),
            vec!["branch", "-d", "goner"],
            "merged-only, and there is no other spelling of this on the menu"
        );
        assert_eq!(
            words(&GitWriteVerb::DeleteTag {
                name: "v0.9".to_owned(),
            }),
            vec!["tag", "-d", "v0.9"]
        );
        assert_eq!(
            words(&GitWriteVerb::CheckoutTracking {
                name: "origin/feature".to_owned(),
            }),
            vec!["checkout", "-b", "feature", "--track", "origin/feature"],
            "the local name is git's own DWIM reading of the remote ref"
        );
        // And the four pathspec verbs still send their paths down the pipe,
        // which is what stops fifty rows from becoming fifty processes.
        let paths = vec!["a b.txt".to_owned(), "c/d.rs".to_owned()];
        let (words, input) = write_arguments(&GitWriteVerb::Stage, &paths);
        assert_eq!(
            words,
            vec!["add", "--pathspec-from-file=-", "--pathspec-file-nul"]
        );
        assert_eq!(input, b"a b.txt\0c/d.rs\0");
    }

    /// PIN (v2 ④) — **the local a remote-tracking ref becomes**, which is git's
    /// own reading and the reason the menu can ask "is that already here?"
    /// before it issues a `-b`.
    #[test]
    fn a_remote_ref_names_the_local_branch_git_would_make_from_it() {
        assert_eq!(tracking_local_name("origin/main"), "main");
        assert_eq!(
            tracking_local_name("upstream/release/2.0"),
            "release/2.0",
            "only the remote's own name comes off the front"
        );
        assert_eq!(
            tracking_local_name("main"),
            "main",
            "a name with no remote on it is handed back whole, not emptied"
        );
    }

    /// PIN (v2 ④) — **`git check-ref-format --branch`'s rules, answered here**,
    /// one class per line of copy the prompt can show.
    ///
    /// The table is the ticket's own list of rejected classes plus the ones git
    /// adds; every accepted name in it is a name a person actually types.
    ///
    /// MUTATION: drop the leading-`-` rule and the `-D` pin two tests up stops
    /// being enough — a branch called `-D` would be a flag on somebody's command
    /// line. That is why this rule is checked before any other.
    #[test]
    fn a_ref_name_is_refused_for_exactly_the_reasons_git_refuses_one() {
        for good in [
            "feature",
            "feature/login",
            "release-2.0",
            "v1.0.0",
            "a_b",
            "fix.the.thing",
            "\u{4e2d}\u{6587}\u{5206}\u{652f}",
        ] {
            assert_eq!(ref_name_fault(good), None, "{good} is a name git takes");
        }
        for (bad, fault) in [
            ("", RefNameFault::Empty),
            ("my branch", RefNameFault::Space),
            ("my\tbranch", RefNameFault::Space),
            ("a..b", RefNameFault::Range),
            ("a~b", RefNameFault::Reserved),
            ("a^b", RefNameFault::Reserved),
            ("a:b", RefNameFault::Reserved),
            ("a?b", RefNameFault::Reserved),
            ("a*b", RefNameFault::Reserved),
            ("a[b", RefNameFault::Reserved),
            ("a\\b", RefNameFault::Reserved),
            ("-feature", RefNameFault::Dash),
            ("-D", RefNameFault::Dash),
            ("feature.lock", RefNameFault::Lock),
            ("/feature", RefNameFault::Shape),
            ("feature/", RefNameFault::Shape),
            ("a//b", RefNameFault::Shape),
            (".hidden", RefNameFault::Shape),
            ("trailing.", RefNameFault::Shape),
            ("a@{0}", RefNameFault::Shape),
            ("@", RefNameFault::Shape),
            ("a\u{7f}b", RefNameFault::Shape),
        ] {
            assert_eq!(
                ref_name_fault(bad),
                Some(fault),
                "{bad:?} is a name git refuses, for {fault:?}"
            );
        }
        // Every class says something, and no two classes say the same thing —
        // a hint that could not tell you which rule you broke is a hint that
        // makes you guess.
        let sentences: Vec<&str> = [
            RefNameFault::Empty,
            RefNameFault::Space,
            RefNameFault::Range,
            RefNameFault::Reserved,
            RefNameFault::Dash,
            RefNameFault::Lock,
            RefNameFault::Shape,
        ]
        .iter()
        .map(|fault| fault.sentence())
        .collect();
        assert!(sentences.iter().all(|line| !line.is_empty()));
        let mut unique = sentences.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), sentences.len(), "one sentence per class");
    }

    /// A cache with a repository under it and nothing asked yet — the shape
    /// every write test below starts from.
    fn rooted_cache(root: &Path) -> GitCache {
        let mut cache = GitCache::default();
        cache.retarget(root);
        cache.mark_pending(&GitQuestion::RepoProbe {
            dir: root.to_owned(),
        });
        assert!(cache.accept(GitAnswer::Repo {
            dir: root.to_owned(),
            outcome: Ok(root.to_owned()),
        }));
        cache
    }

    /// PIN (v2 ④) — **one press, one process, and the row waits for the
    /// receipt.**
    ///
    /// R13's pessimism, said about a *name* instead of about a path: the second
    /// press finds its own ref pending and starts nothing, rather than racing
    /// the first for the ref's lock file. And on success the whole repository is
    /// asked again — a branch that has just been renamed is a branch every pill
    /// on every row of the history is now wrong about.
    ///
    /// MUTATION: unpend before the answer arrives and the double press starts
    /// two `git branch -m`; forget the `refresh` and the branch list on screen
    /// keeps the old name until something else happens to invalidate it.
    #[test]
    fn a_named_write_runs_once_and_the_whole_repository_is_read_again_after_it() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = rooted_cache(&root);
        for question in cache.pending_questions() {
            cache.mark_pending(&question);
        }
        assert!(
            cache.pending_questions().is_empty(),
            "everything a page needs is in flight"
        );

        let verb = GitWriteVerb::RenameBranch {
            from: "main".to_owned(),
            to: "trunk".to_owned(),
        };
        let question = cache
            .begin_write(verb.clone(), Vec::new())
            .expect("a rooted cache takes the write");
        assert_eq!(
            question,
            GitQuestion::Write {
                root: root.clone(),
                verb: verb.clone(),
                paths: Vec::new(),
            }
        );
        assert!(
            cache.ref_write_pending("main"),
            "the row on screen is the one that waits — a rename is about the name that is there"
        );
        assert!(
            cache.begin_write(verb.clone(), Vec::new()).is_none(),
            "a second press starts nothing while the first is in flight"
        );
        assert!(
            !cache.write_pending("main"),
            "a branch called main and a file called main are two different subjects"
        );

        assert!(cache.accept(GitAnswer::Write {
            root: root.clone(),
            verb: verb.clone(),
            paths: Vec::new(),
            outcome: Ok(()),
        }));
        assert!(!cache.ref_write_pending("main"), "the receipt unpends it");
        assert_eq!(
            cache.pending_questions(),
            vec![
                GitQuestion::Status { root: root.clone() },
                GitQuestion::Refs { root: root.clone() },
                GitQuestion::Log {
                    root: root.clone(),
                    refs: Vec::new(),
                    skip: 0,
                    count: GIT_LOG_PAGE,
                },
            ],
            "the status, the refs and the history are all about somewhere that has changed"
        );
        assert!(
            verb.moves_refs(),
            "and the runtime is told so, which is what makes every other surface on this \
             repository re-read too"
        );
    }

    /// PIN (v2 ④) — **a refusal changes nothing and unpends anyway.**
    ///
    /// `git branch -d` on an unmerged branch is the case this whole design is
    /// pointed at: git says no, in git's own words, and the page it was asked
    /// from is still true. So the row stops waiting and nothing is re-read.
    #[test]
    fn a_refused_named_write_leaves_the_page_exactly_as_it_was() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = rooted_cache(&root);
        for question in cache.pending_questions() {
            cache.mark_pending(&question);
        }
        let verb = GitWriteVerb::DeleteBranch {
            name: "goner".to_owned(),
        };
        cache
            .begin_write(verb.clone(), Vec::new())
            .expect("the write starts");
        assert!(cache.ref_write_pending("goner"));
        let refusal =
            GitFault::Refused("error: the branch 'goner' is not fully merged.".to_owned());
        assert!(cache.accept(GitAnswer::Write {
            root,
            verb,
            paths: Vec::new(),
            outcome: Err(refusal.clone()),
        }));
        assert!(!cache.ref_write_pending("goner"), "the row stops waiting");
        assert!(
            cache.pending_questions().is_empty(),
            "and nothing is asked again, because nothing changed"
        );
        assert_eq!(
            write_refusal(&refusal),
            "error: the branch 'goner' is not fully merged.",
            "git's own sentence, unparaphrased — it is what the card carries"
        );
    }

    /// PIN (v2 ④) — a named verb refuses a pathspec and a pathspec verb refuses
    /// a name, because the two are different alphabets.
    #[test]
    fn a_write_that_confuses_a_ref_for_a_path_starts_nothing() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = rooted_cache(&root);
        assert!(
            cache
                .begin_write(
                    GitWriteVerb::DeleteTag {
                        name: "v1.0".to_owned(),
                    },
                    vec!["src/main.rs".to_owned()],
                )
                .is_none(),
            "a tag deletion handed a file list is a caller that has confused the two"
        );
        assert!(
            cache.begin_write(GitWriteVerb::Stage, Vec::new()).is_none(),
            "and a `git add` of nothing is not a process worth starting"
        );
    }

    /// PIN (M10, v2 ④) — the menu asks the ref list whether the local is
    /// already there, so that pressing `Checkout as local branch` twice is a
    /// checkout rather than a refusal.
    #[test]
    fn a_cache_says_whether_a_local_branch_of_that_name_is_already_here() {
        let root = PathBuf::from(r"D:\repo");
        let mut cache = rooted_cache(&root);
        assert!(
            !cache.has_local_branch("main"),
            "an unanswered ref list says no, and lets git be the one that refuses"
        );
        for question in cache.pending_questions() {
            cache.mark_pending(&question);
        }
        assert!(cache.accept(GitAnswer::Refs {
            root,
            outcome: Ok(parse_refs(REFS, RECORDED_AT + 300)),
        }));
        assert!(cache.has_local_branch("main"));
        assert!(
            !cache.has_local_branch("origin/main"),
            "a remote-tracking ref is not a local branch, whatever it is called"
        );
        assert!(!cache.has_local_branch("v1.0"), "and neither is a tag");
    }
}
