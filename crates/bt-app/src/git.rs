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
/// Worded like [`crate::files::FILES_WORKER_STOPPED_NOTICE`] and for the same
/// reason: a worker dying is a feature going away, not a session ending, and the
/// sentence has to say which half still works.
pub const GIT_WORKER_STOPPED_NOTICE: &str =
    "Git reading stopped; terminal input and output remain available";

/// What [`GitFault::GitMissing`] says when there is no `git.exe` at all.
pub const GIT_NOT_FOUND: &str = "git.exe was not found on this machine";

// ── The questions ──────────────────────────────────────────────────────────

/// "Ask the repository this, for this Files column."
///
/// **The host is a docked seat and nothing else.** A floating tree gets no Git
/// page (R2): the float is a peek at a folder, not a seat in a tab, and the
/// panel's whole vocabulary — a page beside a Files page, a branch head, a list
/// you stage from — is seat-shaped. So there is no `Float` variant to address
/// here, and the epoch dance [`crate::files::FilesHost`] needs does not arise.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRequest {
    pub host: LeafId,
    pub question: GitQuestion,
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
    /// "What local branches are there?"
    Branches { root: PathBuf },
    /// "One page of history, please" (R16).
    Log {
        root: PathBuf,
        skip: usize,
        count: usize,
    },
    /// "How does this file differ?" — `staged` is the whole of the `--cached`
    /// mapping (R25). Asked by G-3, when a changed-file row opens a diff.
    #[allow(dead_code)]
    Diff {
        root: PathBuf,
        path: String,
        staged: bool,
    },
    /// "How did this commit change this file?" (R15). Asked by G-3.
    #[allow(dead_code)]
    Show {
        root: PathBuf,
        hash: String,
        path: String,
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
}

/// The four things this panel will do to a repository (R14 / G12).
///
/// **All four are light verbs**, which is the whole of the boundary the mock-up's
/// own wiring comment draws (line 7928): the panel sees, toggles and throws away;
/// `commit`, `merge`, `rebase`, `push` and `pull` belong to the terminal standing
/// beside it. Every one of these is reversible from git's own reflog or index
/// except the two discards, which is exactly why those two are the ones behind a
/// confirmation gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
            (Self::Branches { root: left }, Self::Branches { root: right }) => left == right,
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
                    staged: was,
                },
                Self::Diff {
                    root: right,
                    path: to,
                    staged: is,
                },
            ) => left == right && from == to && was == is,
            (
                Self::Show {
                    root: left,
                    hash: old,
                    path: from,
                },
                Self::Show {
                    root: right,
                    hash: new,
                    path: to,
                },
            ) => left == right && old == new && from == to,
            // **Two writes are never one question.** Every read above coalesces
            // because a newer answer makes an older one worthless; a write has no
            // answer to make worthless, it has an *effect*, and dropping the
            // older of two staging requests because a newer one arrived would
            // silently not stage a file the user asked for. The queue may
            // reorder work; it may not decline to do it.
            (Self::Write { .. }, Self::Write { .. }) => false,
            _ => false,
        }
    }
}

impl GitRequest {
    fn same_target(&self, other: &Self) -> bool {
        self.host == other.host && self.question.same_target(&other.question)
    }
}

// ── The answers ────────────────────────────────────────────────────────────

/// What the worker learned, addressed back to the seat that asked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitResponse {
    pub host: LeafId,
    pub answer: GitAnswer,
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
    Branches {
        root: PathBuf,
        outcome: GitOutcome<Vec<GitBranch>>,
    },
    Log {
        root: PathBuf,
        skip: usize,
        outcome: GitOutcome<GitLog>,
    },
    Diff {
        root: PathBuf,
        path: String,
        staged: bool,
        outcome: GitOutcome<String>,
    },
    Show {
        root: PathBuf,
        hash: String,
        path: String,
        outcome: GitOutcome<String>,
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

/// One local branch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitBranch {
    pub name: String,
    /// Whether `HEAD` is on it — the one that leads the list (R9) and wears the
    /// accent ring (R22).
    pub is_head: bool,
    /// Against its own upstream, when it has one.
    pub ahead: usize,
    pub behind: usize,
    /// The fact: when it was last committed to, in seconds since the epoch.
    pub committer_unix: i64,
    /// The sentence: that fact through [`relative_time`], so a branch row and a
    /// commit row say ages the same way.
    pub committerdate_relative: String,
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
    /// In the tooltip and never in the row (R16).
    pub author: String,
    pub committer_unix: i64,
    /// The commit's own UTC offset in seconds — what makes `Aug 5` the date the
    /// commit was made in, which is the date git itself prints.
    pub committer_offset: i32,
    pub time_relative: String,
    /// Full hashes, in git's order — the first is the first parent. **The only
    /// input the lane algorithm has** (G-4), which is why it is carried from the
    /// first slice rather than added when the graph is drawn.
    pub parents: Vec<String>,
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

/// `git for-each-ref refs/heads`, decoded.
///
/// **The current branch is moved to the front here**, on the worker, rather than
/// left for the list to sort: R9 puts it first, and the sort belongs beside the
/// parse for the reason [`crate::files`] gives for sorting a directory on its own
/// thread — the answer should arrive in the shape the panel draws. A stable sort
/// on one boolean is what keeps the rest in git's own alphabetical order instead
/// of shuffling them for having been compared.
#[must_use]
pub fn parse_branches(bytes: &[u8], now_unix: i64) -> Vec<GitBranch> {
    let text = String::from_utf8_lossy(bytes);
    let mut branches: Vec<GitBranch> = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\0');
            let name = fields.next().filter(|name| !name.is_empty())?;
            let is_head = fields.next() == Some("*");
            let (committer_unix, offset) =
                parse_iso_strict(fields.next().unwrap_or_default()).unwrap_or((0, 0));
            let (ahead, behind) = parse_track(fields.next().unwrap_or_default());
            Some(GitBranch {
                name: name.to_owned(),
                is_head,
                ahead,
                behind,
                committer_unix,
                committerdate_relative: relative_time(committer_unix, offset, now_unix),
            })
        })
        .collect();
    branches.sort_by_key(|branch| !branch.is_head);
    branches
}

/// `git log -z --parents`, decoded.
///
/// `wanted` is the page size the caller asked for; the command asks git for one
/// more than that, so a page that comes back longer than `wanted` is how "there
/// is more" is known without counting the repository.
#[must_use]
pub fn parse_log(bytes: &[u8], now_unix: i64, skip: usize, wanted: usize) -> GitLog {
    let text = String::from_utf8_lossy(bytes);
    let fields: Vec<&str> = text.split('\0').collect();
    // Six fields per commit, NUL between them and a NUL after the last — so the
    // whole stream divides evenly and `chunks_exact` drops the terminator's empty
    // tail without having to know whether git wrote a separator or a terminator.
    let mut commits: Vec<GitCommit> = fields
        .chunks_exact(6)
        .map(|record| {
            let (committer_unix, committer_offset) = parse_iso_strict(record[3]).unwrap_or((0, 0));
            GitCommit {
                hash: record[0].to_owned(),
                short: record[1].to_owned(),
                author: record[2].to_owned(),
                committer_unix,
                committer_offset,
                time_relative: relative_time(committer_unix, committer_offset, now_unix),
                subject: record[4].to_owned(),
                // Space-separated by `%P`, and empty for the root commit — which
                // is an empty list rather than a missing one, because the root
                // genuinely has no parents.
                parents: record[5].split_whitespace().map(str::to_owned).collect(),
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
    stdout: Vec<u8>,
    stderr: String,
}

/// Keep the console out of it (W1).
///
/// Not needed today — this build is still a console subsystem application, so a
/// child inherits our console and flashes nothing — and set anyway, because the
/// day the subsystem flips is the day every `git` in the product would start
/// blinking a black rectangle at the user, and nobody would connect the two.
#[cfg(windows)]
fn no_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_window(_command: &mut Command) {}

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
fn git_command(program: &Path, dir: &Path, arguments: &[&OsStr]) -> Command {
    let mut command = Command::new(program);
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
    no_window(&mut command);
    command
}

/// Read one pipe to its end on a thread of its own.
///
/// Both pipes are drained concurrently because a child that fills one while we
/// are blocked reading the other is a child that never exits — the classic
/// deadlock, and the reason this is not two sequential reads.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buffer);
        }
        buffer
    })
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
        thread::spawn(move || {
            if let Some(pipe) = pipe.as_mut() {
                use std::io::Write as _;
                let _ = pipe.write_all(&input);
            }
            // Dropped here, which is what closes the pipe and tells git the list
            // has ended. Without it a child waits for a list that is complete.
            drop(pipe);
        });
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
        GitQuestion::Branches { root } => GitAnswer::Branches {
            root: root.clone(),
            outcome: Err(fault),
        },
        GitQuestion::Log { root, skip, .. } => GitAnswer::Log {
            root: root.clone(),
            skip: *skip,
            outcome: Err(fault),
        },
        GitQuestion::Diff { root, path, staged } => GitAnswer::Diff {
            root: root.clone(),
            path: path.clone(),
            staged: *staged,
            outcome: Err(fault),
        },
        GitQuestion::Show { root, hash, path } => GitAnswer::Show {
            root: root.clone(),
            hash: hash.clone(),
            path: path.clone(),
            outcome: Err(fault),
        },
        GitQuestion::Write { root, verb, paths } => GitAnswer::Write {
            root: root.clone(),
            verb: *verb,
            paths: paths.clone(),
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
        GitQuestion::Branches { root } => {
            let command = git_command(
                program,
                root,
                &[
                    OsStr::new("for-each-ref"),
                    OsStr::new(
                        "--format=%(refname:short)%00%(HEAD)%00%(committerdate:iso-strict)%00%(upstream:track)",
                    ),
                    OsStr::new("refs/heads"),
                ],
            );
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::Branches {
                    root: root.clone(),
                    outcome: Ok(parse_branches(&run.stdout, now_unix)),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        GitQuestion::Log { root, skip, count } => {
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
                    OsStr::new("--format=%H%x00%h%x00%an%x00%cI%x00%s%x00%P"),
                    OsStr::new(&limit),
                    OsStr::new(&skipped),
                ],
            );
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
        GitQuestion::Diff { root, path, staged } => {
            let mut arguments = vec![OsStr::new("diff"), OsStr::new("--no-color")];
            if *staged {
                arguments.push(OsStr::new("--cached"));
            }
            arguments.push(OsStr::new("--"));
            arguments.push(OsStr::new(path));
            let command = git_command(program, root, &arguments);
            match run_git(command, timeout) {
                Ok(run) if run.ok => GitAnswer::Diff {
                    root: root.clone(),
                    path: path.clone(),
                    staged: *staged,
                    outcome: Ok(String::from_utf8_lossy(&run.stdout).into_owned()),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        // **The one branch that writes.** Three of the four verbs take their
        // pathspec down the pipe; `clean` is the exception because it is the one
        // git subcommand of the four that never learned `--pathspec-from-file`
        // (checked against git 2.52), and it does not need to: a discard is one
        // file by construction (R14 puts no group-level discard on the page),
        // so its pathspec is one argument and always will be.
        GitQuestion::Write { root, verb, paths } => {
            let pathspec: Vec<u8> = paths
                .iter()
                .flat_map(|path| path.as_bytes().iter().copied().chain(std::iter::once(0u8)))
                .collect();
            let from_stdin = [
                OsStr::new("--pathspec-from-file=-"),
                OsStr::new("--pathspec-file-nul"),
            ];
            let (arguments, input) = match verb {
                GitWriteVerb::Stage => (
                    vec![OsStr::new("add"), from_stdin[0], from_stdin[1]],
                    pathspec,
                ),
                GitWriteVerb::Unstage => (
                    vec![
                        OsStr::new("restore"),
                        OsStr::new("--staged"),
                        from_stdin[0],
                        from_stdin[1],
                    ],
                    pathspec,
                ),
                GitWriteVerb::Discard => (
                    vec![
                        OsStr::new("restore"),
                        OsStr::new("--worktree"),
                        from_stdin[0],
                        from_stdin[1],
                    ],
                    pathspec,
                ),
                GitWriteVerb::DiscardUntracked => {
                    let mut arguments = vec![
                        OsStr::new("clean"),
                        OsStr::new("-f"),
                        OsStr::new("-q"),
                        OsStr::new("--"),
                    ];
                    arguments.extend(paths.iter().map(|path| OsStr::new(path.as_str())));
                    (arguments, Vec::new())
                }
            };
            let command = git_command(program, root, &arguments);
            match run_git_with_input(command, timeout, input) {
                Ok(run) if run.ok => GitAnswer::Write {
                    root: root.clone(),
                    verb: *verb,
                    paths: paths.clone(),
                    outcome: Ok(()),
                },
                Ok(run) => faulted(question, classify_failure(&run.stderr)),
                Err(fault) => faulted(question, fault),
            }
        }
        GitQuestion::Show { root, hash, path } => {
            let command = git_command(
                program,
                root,
                &[
                    OsStr::new("show"),
                    OsStr::new("--no-color"),
                    OsStr::new(hash),
                    OsStr::new("--"),
                    OsStr::new(path),
                ],
            );
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
/// **When it goes stale (R31).** Two moments and no others: the column is
/// re-rooted somewhere else, or something asks for a refresh. There is no timer
/// here and no watcher — a repository is not read because time passed.
#[derive(Clone, Debug, Default)]
pub struct GitCache {
    /// The folder this cache is about — the column's root, not the repository's.
    dir: Option<PathBuf>,
    repo: GitSlot<PathBuf>,
    status: GitSlot<GitStatus>,
    branches: GitSlot<Vec<GitBranch>>,
    log: GitSlot<GitLog>,
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
    /// git's own words for the last write that would not go through (W3).
    ///
    /// Kept until the next write is *started* rather than until the next one
    /// finishes, so the sentence outlives the frame it arrived in and can be
    /// read. It is cleared by a new attempt because that is the moment the user
    /// has said they know.
    write_error: Option<String>,
}

impl GitCache {
    /// Point this cache at a folder, forgetting everything if it is a different
    /// one. Answers whether anything was forgotten.
    pub fn retarget(&mut self, dir: &Path) -> bool {
        if self.dir.as_deref() == Some(dir) {
            return false;
        }
        *self = Self {
            dir: Some(dir.to_owned()),
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
        self.branches = GitSlot::Idle;
        self.log = GitSlot::Idle;
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
    pub fn branches(&self) -> &GitSlot<Vec<GitBranch>> {
        &self.branches
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

    /// The last refusal, in git's own words (W3).
    #[must_use]
    pub fn write_error(&self) -> Option<&str> {
        self.write_error.as_deref()
    }

    /// Build the write, and dim its rows.
    ///
    /// Returns `None` — and starts nothing — when there is no repository, when
    /// the list is empty, or when any of these paths is already being written to.
    /// The last is the guard that makes a double click on `+` one `git add` and
    /// not two: the second finds its own path pending and declines, rather than
    /// racing the first for `index.lock`.
    #[must_use]
    pub fn begin_write(&mut self, verb: GitWriteVerb, paths: Vec<String>) -> Option<GitQuestion> {
        let root = self.repo.ready()?.clone();
        if paths.is_empty() || paths.iter().any(|path| self.pending_writes.contains(path)) {
            return None;
        }
        // Cleared here and not on the receipt: the banner is answering "did the
        // thing I just asked for happen", and the moment a new thing is asked
        // for is the moment the old answer stops being about anything.
        self.write_error = None;
        self.pending_writes.extend(paths.iter().cloned());
        Some(GitQuestion::Write { root, verb, paths })
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
        if matches!(self.branches, GitSlot::Idle) {
            questions.push(GitQuestion::Branches { root: root.clone() });
        }
        if matches!(self.log, GitSlot::Idle) {
            questions.push(GitQuestion::Log {
                root: root.clone(),
                skip: 0,
                count: GIT_LOG_PAGE,
            });
        }
        questions
    }

    /// The next page of history, when there is one and the list wants it (R16) —
    /// what G-2's "Load more" row asks for, and the reason that row is drawn.
    #[allow(dead_code)]
    #[must_use]
    pub fn more_commits(&self) -> Option<GitQuestion> {
        let root = self.repo.ready()?;
        let log = self.log.ready()?;
        log.has_more.then(|| GitQuestion::Log {
            root: root.clone(),
            skip: log.skip + log.commits.len(),
            count: GIT_LOG_PAGE,
        })
    }

    /// Record that a question has been sent, so that it is not sent again.
    pub fn mark_pending(&mut self, question: &GitQuestion) {
        match question {
            GitQuestion::RepoProbe { .. } => self.repo = GitSlot::Pending,
            GitQuestion::Status { .. } => self.status = GitSlot::Pending,
            GitQuestion::Branches { .. } => self.branches = GitSlot::Pending,
            // Only the first page owns the slot: a "load more" leaves the page
            // already on screen exactly where it is, because it is not being
            // replaced by anything.
            GitQuestion::Log { skip: 0, .. } => self.log = GitSlot::Pending,
            // A write's own "already asked" bookkeeping is `pending_writes`,
            // written by `begin_write` at the moment the question is built.
            GitQuestion::Log { .. }
            | GitQuestion::Diff { .. }
            | GitQuestion::Show { .. }
            | GitQuestion::Write { .. } => {}
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
                true
            }
            GitAnswer::Branches { root, outcome } => {
                if self.root() != Some(root.as_path()) {
                    return false;
                }
                self.branches = GitSlot::take(outcome);
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
                    (0, outcome) => self.log = GitSlot::take(outcome),
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
            // **The receipt** (R13). Three things happen and their order is the
            // point: the rows stop being dimmed, a refusal takes git's own words,
            // and — only on success — everything is asked again. Re-asking on a
            // *failure* would be spending three subprocesses to re-learn a status
            // that by definition did not change.
            GitAnswer::Write {
                root,
                paths,
                outcome,
                ..
            } => {
                if self.root() != Some(root.as_path()) {
                    return false;
                }
                for path in &paths {
                    self.pending_writes.remove(path);
                }
                match outcome {
                    Ok(()) => self.refresh(),
                    Err(fault) => self.write_error = Some(write_refusal(&fault)),
                }
                true
            }
            // Diffs and file histories are documents, not column state: they
            // belong to the preview pool, keyed by their own `PreviewSource`.
            // Nothing here has a slot for them, and inventing one would give the
            // same document two homes.
            GitAnswer::Diff { .. } | GitAnswer::Show { .. } => false,
        }
    }
}

/// One line for a write that would not go through.
///
/// git's own sentence wherever there is one, and this module's only wording
/// otherwise — a killed child and a missing executable have no sentence of their
/// own to pass through, so the two of them are the whole of what is written here.
/// [`GitFault::NotARepository`] cannot reach this function: a write is only
/// offered from a page that already found a repository.
#[must_use]
fn write_refusal(fault: &GitFault) -> String {
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
        thread::Builder::new()
            .name("bt-git-worker".to_owned())
            .spawn(move || {
                let program = crate::profiles::find_git(&bt_pty::SystemShellEnvironment);
                run_git_worker(request_rx, |request| {
                    let answer = match program.as_deref() {
                        Some(program) => {
                            answer(program, &request.question, GIT_COMMAND_TIMEOUT, now_unix())
                        }
                        None => faulted(
                            &request.question,
                            GitFault::GitMissing(GIT_NOT_FOUND.to_owned()),
                        ),
                    };
                    if response_tx
                        .send(GitResponse {
                            host: request.host,
                            answer,
                        })
                        .is_ok()
                    {
                        let _ = proxy.send_event(AppEvent::GitReady);
                    }
                });
            })
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
        Some(GIT_WORKER_STOPPED_NOTICE)
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

    /// `for-each-ref` with this module's format: one branch diverged from its
    /// upstream, one tracking nothing, `HEAD` on the third, and git's own
    /// alphabetical order — which is *not* the order the list is drawn in.
    const BRANCHES: &[u8] = b"diverge\x00 \x002026-08-15T10:24:37-04:00\x00[ahead 1, behind 1]\ngoner\x00 \x002026-08-15T10:17:57-04:00\x00\nmain\x00*\x002026-08-15T10:18:24-04:00\x00[ahead 4]\nother\x00 \x002026-08-15T10:17:57-04:00\x00\n";

    /// `log --parents --topo-order -z` with this module's format: a merge commit
    /// with **two** parents, then two ordinary commits.
    const LOG_Z: &[u8] = b"36d3949271716f6d8cd1395f6f5606245c08b914\x0036d3949\x00T\x002026-08-15T10:18:24-04:00\x00merge other\x005a18cfe67ca341203166040bfc8f954b899e275e 91d138a3d39811755e479ec386b450a8c8465302\x0091d138a3d39811755e479ec386b450a8c8465302\x0091d138a\x00T\x002026-08-15T10:17:57-04:00\x00other\x00a4499ab318aa13e08d780a084fe865fa8d18e558\x005a18cfe67ca341203166040bfc8f954b899e275e\x005a18cfe\x00T\x002026-08-15T10:18:07-04:00\x00ahead2\x00452220ba3687b9dcf3399962a69310de387b7af9\x00";

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
        assert_eq!(merge.author, "T");
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

        let root_ward = &page.commits[2];
        assert_eq!(root_ward.subject, "ahead2");
        assert_eq!(
            root_ward.parents,
            vec!["452220ba3687b9dcf3399962a69310de387b7af9".to_owned()],
            "one parent is a list of one, not an absence"
        );
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
        let diff = |staged: bool| GitRequest {
            host: seat(1),
            question: GitQuestion::Diff {
                root: PathBuf::from(r"D:\repo"),
                path: "src/main.rs".to_owned(),
                staged,
            },
        };
        tx.send(diff(false)).unwrap();
        tx.send(diff(true)).unwrap();
        tx.send(diff(false)).unwrap();
        drop(tx);
        let mut served = Vec::new();
        run_git_worker(rx, |request| served.push(request));
        assert_eq!(
            served.len(),
            2,
            "the repeated unstaged question collapses; the staged one does not"
        );
        assert_eq!(served[0], diff(true));
        assert_eq!(served[1], diff(false));
    }

    /// PIN — two pages of history are two questions; the same page twice is one.
    #[test]
    fn two_pages_of_history_are_two_questions_and_one_page_asked_twice_is_one() {
        let (tx, rx) = mpsc::channel();
        let page = |skip: usize, count: usize| GitRequest {
            host: seat(1),
            question: GitQuestion::Log {
                root: PathBuf::from(r"D:\repo"),
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
            host,
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
            host: seat(1),
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
    /// Stood in for by `ping`, which is on every Windows and whose only job here
    /// is to take longer than it is given. The assertion that matters is not that
    /// the fault is `TimedOut` but that the call *returned* — a guard that
    /// reports a timeout after waiting for the process anyway is not a guard.
    #[test]
    fn a_child_that_will_not_finish_is_killed_and_reported() {
        let mut command = Command::new("ping");
        command
            .args(["-n", "10", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        no_window(&mut command);
        let started = Instant::now();
        let outcome = run_git(command, Duration::from_millis(150));
        let waited = started.elapsed();
        assert_eq!(outcome.err(), Some(GitFault::TimedOut));
        assert!(
            waited < Duration::from_secs(5),
            "the guard returned after {waited:?}, not after the child felt like it"
        );
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
        let fault = GitFault::GitMissing(GIT_NOT_FOUND.to_owned());
        for question in [
            GitQuestion::RepoProbe {
                dir: PathBuf::from(r"D:\repo"),
            },
            GitQuestion::Status {
                root: PathBuf::from(r"D:\repo"),
            },
            GitQuestion::Branches {
                root: PathBuf::from(r"D:\repo"),
            },
            GitQuestion::Log {
                root: PathBuf::from(r"D:\repo"),
                skip: 0,
                count: GIT_LOG_PAGE,
            },
            GitQuestion::Diff {
                root: PathBuf::from(r"D:\repo"),
                path: "a.rs".to_owned(),
                staged: true,
            },
            GitQuestion::Show {
                root: PathBuf::from(r"D:\repo"),
                hash: "abc".to_owned(),
                path: "a.rs".to_owned(),
            },
        ] {
            let answer = faulted(&question, fault.clone());
            let carried = match &answer {
                GitAnswer::Repo { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Status { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Branches { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Log { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Diff { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Show { outcome, .. } => outcome.as_ref().err(),
                GitAnswer::Write { outcome, .. } => outcome.as_ref().err(),
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

    // ── The branch list ────────────────────────────────────────────────────

    /// PIN — the branch `HEAD` is on leads the list (R9), and each branch carries
    /// its own distance from its own upstream.
    #[test]
    fn the_current_branch_leads_and_each_branch_counts_its_own_upstream() {
        let branches = parse_branches(BRANCHES, RECORDED_AT + 300);
        let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["main", "diverge", "goner", "other"],
            "the current branch first, the rest in git's own order"
        );
        assert!(branches[0].is_head);
        assert!(branches[1..].iter().all(|branch| !branch.is_head));
        assert_eq!((branches[0].ahead, branches[0].behind), (4, 0));
        assert_eq!((branches[1].ahead, branches[1].behind), (1, 1));
        assert_eq!(
            (branches[2].ahead, branches[2].behind),
            (0, 0),
            "a branch tracking nothing is not behind anything"
        );
        assert_eq!(branches[0].committer_unix, 1_786_803_504);
        assert_eq!(branches[0].committerdate_relative, "5m");
        assert_eq!(branches[1].committerdate_relative, "now");
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
                GitQuestion::Branches { root: root.clone() },
                GitQuestion::Log {
                    root: root.clone(),
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
            author: "T".to_owned(),
            committer_unix: RECORDED_AT,
            committer_offset: 0,
            time_relative: "now".to_owned(),
            parents: Vec::new(),
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
        cache.accept(GitAnswer::Branches {
            root: root.clone(),
            outcome: Ok(parse_branches(BRANCHES, RECORDED_AT)),
        });
        assert!(cache.status().ready().is_some());
        assert_eq!(
            cache.branches().ready().map(Vec::len),
            Some(4),
            "the four branches of the recording are filed under the root that was asked"
        );
        cache.refresh();
        assert!(
            cache.branches().ready().is_none(),
            "a refresh asks again rather than keeping an answer it has decided is stale"
        );
        assert_eq!(
            cache.root(),
            Some(root.as_path()),
            "the root survives a refresh"
        );
        assert_eq!(cache.pending_questions().len(), 3);
    }

    #[test]
    fn a_stopped_worker_is_announced_once_and_then_never_again() {
        let mut running = true;
        let mut pending = false;
        assert!(disable_git_worker_state(&mut running, &mut pending));
        assert!(!disable_git_worker_state(&mut running, &mut pending));
        assert_eq!(
            take_git_worker_notice(&mut pending),
            Some(GIT_WORKER_STOPPED_NOTICE)
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

        let GitAnswer::Branches { outcome, .. } = ask(GitQuestion::Branches { root: root.clone() })
        else {
            panic!("a branches question is answered with branches");
        };
        let branches = outcome.expect("this workspace's branches read");
        assert!(
            !branches.is_empty(),
            "a repository with commits has a branch"
        );
        assert!(
            branches.iter().filter(|branch| branch.is_head).count() <= 1,
            "HEAD is on at most one local branch"
        );

        let GitAnswer::Log { outcome, .. } = ask(GitQuestion::Log {
            root,
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
}
