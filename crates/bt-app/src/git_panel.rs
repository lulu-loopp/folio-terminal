//! **The Files column's second page** — what a repository looks like, drawn.
//!
//! The user direction this hangs from is one sentence (`design/ui-mockup.html`
//! 1577-1580): *a files pane is a PLACE's view; the repo status is the same place
//! seen another way — one chassis, a segmented switch, no metadata scattered into
//! tooltips.* Everything here follows from taking that literally. The column
//! keeps its head, its width, its drag, its dock and its foot; only the body
//! between the switch and the path strip is different. There is no Git *seat*, no
//! Git window, and nothing about a repository anywhere else on screen.
//!
//! **What this module is, and what it is not.** It is the third member of a
//! family [`crate::seats`] already has two of: a list that owns one derivation of
//! its rectangles, read by the painter, the hit test and the wheel alike, so that
//! the row you can press is the row you can see. It is *not* where the questions
//! are asked ([`crate::git`]) nor where they are filed (the same), and it holds no
//! state of its own — every frame is built from the cache and thrown away.
//!
//! **Why it is not in `seats.rs`.** The Files tree lives there, and this is its
//! sibling. But `seats.rs` is already the largest file in the crate and this page
//! is a whole vocabulary — six row kinds, five verbs, a badge alphabet, a mini
//! graph — that shares nothing with the tree except the rectangle it stands in.
//! The precedent is `peek_strip.rs` and `file_peek.rs`: a surface with its own
//! grammar gets its own file, and `seats.rs` keeps the chassis.
//!
//! **The verbs stop where the mock-up's wiring comment stops** (line 7928):
//! *panel verbs are LIGHT (see + toggle); heavy verbs (commit/merge/rebase)
//! belong to the terminal beside it.* So this page stages, unstages and discards,
//! and there is no commit box on it and never will be.
//!
//! **What is deliberately not here**, so that a reader does not go looking: the
//! full lane graph (R18/G-4) lives in [`crate::git_graph`], because a DAG needs
//! the width of a document and this column has 240 pixels. What *is* here and
//! looks like it belongs elsewhere is the last section of this file: the status
//! letters a **files tree** row wears (R32). They are here because they are the
//! same alphabet and the same four inks this page draws, and a second copy of
//! either would be two answers to "what colour is a deleted file".

use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, ChromeQuad};

use std::path::Path;

use crate::git::{
    GitCache, GitCommitFile, GitFault, GitGroup, GitSlot, GitStatusEntry, GitWriteVerb, StatusCode,
};
use crate::marks::{ChromeMark, ChromeSprite};
use crate::preview::PreviewSource;

// ── the page's measurements, all from the mock-up ──────────────────────────

/// `.git-view { padding: 4px 10px 10px }` — the body's own inset.
pub const GIT_VIEW_PADDING_X_LOGICAL_PX: f32 = 10.0;
pub const GIT_VIEW_PADDING_TOP_LOGICAL_PX: f32 = 4.0;
pub const GIT_VIEW_PADDING_BOTTOM_LOGICAL_PX: f32 = 10.0;
/// `.git-view.empty { font-size: 11.5px }` — the one empty state, centred (R17).
pub const GIT_EMPTY_FONT_LOGICAL_PX: f32 = 11.5;

/// `.git-branch { padding: 10px 2px 6px; font-size: 13.5px; font-weight: 600 }`.
///
/// 13.5 is the largest text on the page and it is the branch name, which is the
/// masthead's whole point: *the one fact you always need* (mock-up 1593). R20
/// settled it against the full graph's 14px — one fact, one size.
pub const GIT_HEAD_FONT_LOGICAL_PX: f32 = 13.5;
pub const GIT_HEAD_PADDING_TOP_LOGICAL_PX: f32 = 10.0;
pub const GIT_HEAD_PADDING_BOTTOM_LOGICAL_PX: f32 = 6.0;
/// The masthead's own left/right inset, inside `.git-view`'s ten.
pub const GIT_HEAD_PADDING_X_LOGICAL_PX: f32 = 2.0;
/// `.git-branch { gap: 8px }`.
pub const GIT_HEAD_GAP_LOGICAL_PX: f32 = 8.0;
/// The branch mark's box (R4).
///
/// Fourteen and not the head's own 13.5: a mark is measured across its box and a
/// letter across its cap height, so a mark cut to the type size always reads
/// small beside it. Half a pixel is what closes that on this ladder.
pub const GIT_HEAD_MARK_LOGICAL_PX: f32 = 14.0;

/// `.gud { font-size: 10.5px; border-radius: 9px; padding: 1px 7px }` — the
/// ahead/behind pills, and the shape every pill on this page will wear (R22).
pub const GIT_PILL_FONT_LOGICAL_PX: f32 = 10.5;
pub const GIT_PILL_RADIUS_LOGICAL_PX: f32 = 9.0;
pub const GIT_PILL_PADDING_X_LOGICAL_PX: f32 = 7.0;
/// `.gud { border: 1px solid var(--border) }`.
pub const GIT_PILL_EDGE_LOGICAL_PX: f32 = 1.0;
/// The pill's own height: `1px + line + 1px` at 10.5px, which lands on 16.
pub const GIT_PILL_HEIGHT_LOGICAL_PX: f32 = 16.0;

/// `.glabel { padding: 14px 2px 5px; font-size: 9.5px; letter-spacing: .09em }`.
///
/// The 14px top inset is the *group gap*: the design pulls the sections apart
/// with the heading's own padding rather than with a margin between cards, so a
/// heading and the card under it are one object with air above them.
pub const GIT_LABEL_FONT_LOGICAL_PX: f32 = 9.5;
pub const GIT_LABEL_TRACKING_EM: f32 = 0.09;
pub const GIT_LABEL_PADDING_TOP_LOGICAL_PX: f32 = 14.0;
pub const GIT_LABEL_PADDING_BOTTOM_LOGICAL_PX: f32 = 5.0;
pub const GIT_LABEL_PADDING_X_LOGICAL_PX: f32 = 2.0;
/// How tall the heading's own text sits: 9.5px at the browser's normal leading.
pub const GIT_LABEL_LINE_LOGICAL_PX: f32 = 12.0;

/// `.gsec { background: var(--panel); border-radius: 9px; padding: 4px }`.
///
/// *Each section is a soft region card — one fill says "this is a region" (the
/// math-block rule), which is what the flat first draft was missing* (mock-up
/// 1605). It is the only fill on the page that is not a state.
pub const GIT_SECTION_RADIUS_LOGICAL_PX: f32 = 9.0;
pub const GIT_SECTION_PADDING_LOGICAL_PX: f32 = 4.0;

/// `.grow`/`.gcommit` — 27 logical pixels, and the same 27 for both.
///
/// The commit row declares it (`height: 27px`) because the mini graph's SVG is
/// 27 tall and the line has to meet the line in the row below it. The change row
/// arrives at it: `5px + 17px badge + 5px`. That they agree is not a
/// coincidence to be relied on but a fact to be stated once, here.
pub const GIT_ROW_HEIGHT_LOGICAL_PX: f32 = 27.0;
/// `.grow { border-radius: 6px }`.
pub const GIT_ROW_RADIUS_LOGICAL_PX: f32 = 6.0;
/// `.grow { gap: 8px }`.
pub const GIT_ROW_GAP_LOGICAL_PX: f32 = 8.0;
/// `.grow { padding: 5px 7px }` — the horizontal half.
pub const GIT_ROW_PADDING_X_LOGICAL_PX: f32 = 7.0;
/// `.gcommit { padding: 0 7px 0 3px }` — the graph column gets the left edge.
pub const GIT_COMMIT_PADDING_LEFT_LOGICAL_PX: f32 = 3.0;
/// How far an expanded commit's file rows stand in from the list's edge (R15).
///
/// The commit row's left three pixels and its fourteen-pixel graph column,
/// exactly: the files hang under the *message*, and the lane's gutter stays
/// empty beside them. An indent chosen for looks would be a number that stopped
/// agreeing with the graph the first time either moved.
pub const GIT_COMMIT_FILE_INDENT_LOGICAL_PX: f32 =
    GIT_COMMIT_PADDING_LEFT_LOGICAL_PX + GIT_GRAPH_WIDTH_LOGICAL_PX;
/// `.grow bdi`, `.gmsg` — the page's body size.
pub const GIT_ROW_FONT_LOGICAL_PX: f32 = 12.5;

/// `.gst { width: 17px; height: 17px; border-radius: 5px; font: 600 10px mono }`
/// — one status letter's badge (R11).
pub const GIT_BADGE_LOGICAL_PX: f32 = 17.0;
pub const GIT_BADGE_RADIUS_LOGICAL_PX: f32 = 5.0;
pub const GIT_BADGE_FONT_LOGICAL_PX: f32 = 10.0;
/// How far apart two badges sit when a file is in two groups at once.
///
/// Three and not the row's eight: `MM` is *one* fact about one file wearing two
/// letters, and eight pixels between them would read as two columns.
pub const GIT_BADGE_GAP_LOGICAL_PX: f32 = 3.0;
/// How much of the status ink tints a badge's ground —
/// `color-mix(in srgb, <ink> 15%, transparent)`, in thousandths.
pub const GIT_BADGE_GROUND_ALPHA: i32 = 150;

/// `.gact { width: 18px; height: 18px; border-radius: 5px }` — a hover verb.
pub const GIT_ACT_LOGICAL_PX: f32 = 18.0;
pub const GIT_ACT_RADIUS_LOGICAL_PX: f32 = 5.0;
/// `.gact svg` — the glyph inside the box, on `.pv-tool`'s ratio.
pub const GIT_ACT_GLYPH_LOGICAL_PX: f32 = 11.0;
/// The gap between two verbs on one row.
pub const GIT_ACT_GAP_LOGICAL_PX: f32 = 2.0;
/// **R12's middle rung.** `.pane:hover .pv-tool { opacity: .7 }` — the same
/// three-step reveal every other hover verb in this product uses, applied here
/// in place of the mock-up's two-step `visibility: hidden` (mock-up 1642-1648).
///
/// The mock-up's own note beside that rule is why the box is reserved either
/// way: *appearing must not nudge the row (user report)*.
pub const GIT_ACT_REVEAL: f32 = crate::seats::PREVIEW_TOOL_REVEAL;

/// `.ggr { width: 14px; height: 27px }` — the mini graph's column.
pub const GIT_GRAPH_WIDTH_LOGICAL_PX: f32 = 14.0;
/// `.ggr line { stroke-width: 1.5 }`.
pub const GIT_GRAPH_STROKE_LOGICAL_PX: f32 = 1.5;
/// `<circle cx="7" cy="13.5" r="3.1"/>` — the node.
pub const GIT_GRAPH_DOT_RADIUS_LOGICAL_PX: f32 = 3.1;
/// `.ggr line { stroke: color-mix(in srgb, var(--ink3) 55%, transparent) }`.
pub const GIT_GRAPH_LINE_ALPHA: i32 = 550;

/// `.gcommit code { font: 10.5px mono }` — the short hash, **on both surfaces**.
///
/// R21's smaller half: the mock-up drew it at 10.5 here and at 11 in the full
/// graph, and one size is what one fact gets. The full graph reads this constant
/// rather than declaring its own, which is what stops the pair from drifting
/// apart again the next time either is touched.
pub const GIT_HASH_FONT_LOGICAL_PX: f32 = 10.5;
/// `.gtime { font-size: 10px }`.
pub const GIT_TIME_FONT_LOGICAL_PX: f32 = 10.0;

/// How far a row is faded while a write about it is in flight (R13).
///
/// Not hidden and not removed: the row is still the truth until git says
/// otherwise, and what the dimming says is "this is being worked on", which is
/// the honest thing to say about eighty milliseconds of `git add`.
pub const GIT_PENDING_FADE: f32 = 0.45;

// ── the words ──────────────────────────────────────────────────────────────

/// The one empty state (R17).
///
/// The mock-up had a second — *No graph for this folder* — for the case where a
/// repository exists but the graph has no rows. It was struck because it says the
/// same thing in words that sound like a missing feature: a reader who sees it
/// wonders what they did wrong, when the answer is only ever "there is no
/// repository here".
pub const GIT_NOT_A_REPOSITORY: &str = "Not a git repository";

/// What the page says while it is finding out.
///
/// Its own sentence rather than an empty list, for [`crate::files`]'s reason: a
/// list with nothing in it and a list that has not been read yet look identical,
/// and only one of them means the repository is clean.
pub const GIT_READING: &str = "Reading the repository…";

/// Who is speaking, on a notice raised by a refused verb (toast ruling,
/// 2026-08-16).
///
/// One word and a proper noun, because the sentence under it is git's own and a
/// title that paraphrased it would be this window putting words in git's mouth.
/// What the title is *for* is provenance: the card can be standing over a column
/// or in the corner of the window, and "Git" is what tells you the paragraph
/// under it came from a program and not from us.
pub const GIT_TOAST_TITLE: &str = "Git";

/// A repository with no commits yet (R7).
///
/// It *has* a branch name — `git status` prints one — but nothing points at it,
/// so every count on the page would be zero for a reason that is not "clean".
pub const GIT_UNBORN: &str = "no commits yet";

/// `HEAD` is on a commit rather than a branch.
pub const GIT_DETACHED: &str = "detached HEAD";

/// The row that asks for the next page of history (R16).
pub const GIT_LOAD_MORE: &str = "Load more";

/// An expansion with nothing under it (R15).
///
/// Almost always a **merge**: `--name-status` compares a commit against its
/// parent, and a merge has two, so git answers with nothing rather than choosing
/// one. Saying "no files" flatly would be a claim about the commit; saying it
/// this way is a claim about the question that was asked, which is the true one.
pub const GIT_COMMIT_NO_FILES: &str = "No files against the first parent";

/// What the three groups are called.
#[must_use]
pub fn group_heading(group: GitGroup) -> &'static str {
    match group {
        GitGroup::Staged => "STAGED",
        GitGroup::Changes => "CHANGES",
        GitGroup::Untracked => "UNTRACKED",
    }
}

/// The heading's tooltip — mock-up 4950-4952, with the third written to match.
///
/// These are *teaching* text, and that is the design's own reading of them: the
/// whole reason a heading explains what the index is, is that a person who does
/// not know has no other way to find out from this page. The `Changes` line lost
/// its second clause ("hover a row and press + to stage it") because R12 made the
/// verb visible at seven-tenths the moment the pointer is on the row, so the
/// tooltip no longer has to describe a control the user cannot see.
#[must_use]
pub fn group_tooltip(group: GitGroup) -> &'static str {
    match group {
        GitGroup::Staged => "Packed for the next commit — 'git commit' ships exactly these",
        GitGroup::Changes => "Edited but not packed yet",
        GitGroup::Untracked => "Not in the repository yet — git is not watching these",
    }
}

/// The history's own heading. A section, but not a status group — nothing about
/// it is a letter, so it is not a [`GitGroup`] and does not pretend to be.
/// `.gbdot { width:7px; height:7px }` (G35) — the dot that says which branch
/// `HEAD` is on. Filled with the accent when it is this one, an empty ring when
/// it is not: the same Fluent 2 rule the tab pin already follows.
pub const GIT_BRANCH_DOT_LOGICAL_PX: f32 = 7.0;
/// `.gbdot { border: 1.5px solid var(--ink3) }` — the ring's own weight.
pub const GIT_BRANCH_DOT_EDGE_LOGICAL_PX: f32 = 1.5;

pub const GIT_BRANCHES_HEADING: &str = "BRANCHES";
/// The teaching sentence over the list, in the voice its three siblings use.
///
/// It says what a press does, because the row itself cannot: unlike a change
/// row, a branch row has no hover button to point at, and a list of names with
/// no visible verb is a list nobody tries.
pub const GIT_BRANCHES_TOOLTIP: &str =
    "Local branches, current one first — click one to check it out";

/// The sub-group under BRANCHES (T9, v2 ③).
pub const GIT_REMOTES_HEADING: &str = "REMOTES";
/// The disclosure triangle in front of it — the files tree's own ten pixels.
pub const GIT_REMOTES_MARK_LOGICAL_PX: f32 = 10.0;
/// And the gap between it and the word.
pub const GIT_REMOTES_MARK_GAP_LOGICAL_PX: f32 = 4.0;
/// What the row says when it is shut, and when it is open.
pub const GIT_REMOTES_TOOLTIP_SHUT: &str = "Branches on remotes — click to show them";
pub const GIT_REMOTES_TOOLTIP_OPEN: &str = "Branches on remotes — click to fold them away";

pub const GIT_COMMITS_HEADING: &str = "COMMITS";
/// Mock-up 4952, cut to what this slice draws: the merge curve is here, the
/// lanes are G-4's.
pub const GIT_COMMITS_TOOLTIP: &str = "Recent history, newest first — the curve marks a merge, where a branch's history joins this line";

// ── the verbs ──────────────────────────────────────────────────────────────

/// One thing a press on this page does.
///
/// The five of R14, and no more. Multi-select, a context menu and keyboard
/// navigation over the list are named there as deferred and are deferred here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitAct {
    /// `+` on a change or an untracked row.
    Stage,
    /// `−` on a staged row.
    Unstage,
    /// `×` on a change or an untracked row — **behind the gate**.
    Discard,
    /// The heading's own `+`: everything under it, in one process.
    StageAll,
    /// The heading's own `−`.
    UnstageAll,
    /// The last row of the history.
    LoadMore,
    /// The masthead's own button: put the whole graph on the preview seat (G24).
    ///
    /// A verb on this list rather than a control of its own, because it is one:
    /// it lives on a row, it is hit-tested by the row, it wears a mark and it
    /// carries a sentence — every mechanism [`act_boxes`] already owns. The
    /// mock-up drew it as a naked text button reading `Graph`; R27 gave it the
    /// mark cut for it ([`ChromeMark::GitGraph`], three commits and two edges),
    /// because a word in a masthead beside a branch name reads as part of the
    /// name.
    OpenGraph,
}

impl GitAct {
    /// What the pointer is told this button does.
    #[must_use]
    pub fn tooltip(self, untracked: bool) -> &'static str {
        match self {
            Self::Stage => "Stage",
            Self::Unstage => "Unstage",
            // The two discards are one word to the user and two commands to git,
            // and the tooltip is where that difference has to be said: one puts a
            // file back, the other deletes it, and a person is entitled to know
            // which before the gate asks them to confirm it.
            Self::Discard if untracked => "Delete this file",
            Self::Discard => "Discard changes",
            Self::StageAll => "Stage all",
            Self::UnstageAll => "Unstage all",
            Self::LoadMore => "Load fifty more commits",
            Self::OpenGraph => "Open the full commit graph",
        }
    }

    /// The mark the button wears. Marks and not characters, because R12's three
    /// rungs are an opacity and a text run has none.
    #[must_use]
    pub fn mark(self) -> ChromeMark {
        match self {
            Self::Stage | Self::StageAll => ChromeMark::Plus,
            Self::Unstage | Self::UnstageAll => ChromeMark::Minus,
            Self::Discard => ChromeMark::PaneClose,
            // Never drawn as a glyph — the whole row is the button.
            Self::LoadMore => ChromeMark::Plus,
            Self::OpenGraph => ChromeMark::GitGraph,
        }
    }

    /// Which command this verb is, for a given row.
    ///
    /// `None` for [`Self::LoadMore`], which asks a question rather than changing
    /// anything, and is dispatched through the log's own paging instead.
    #[must_use]
    pub fn verb(self, untracked: bool) -> Option<GitWriteVerb> {
        match self {
            Self::Stage | Self::StageAll => Some(GitWriteVerb::Stage),
            Self::Unstage | Self::UnstageAll => Some(GitWriteVerb::Unstage),
            Self::Discard if untracked => Some(GitWriteVerb::DiscardUntracked),
            Self::Discard => Some(GitWriteVerb::Discard),
            Self::LoadMore | Self::OpenGraph => None,
        }
    }

    /// Whether this button is there before the pointer is.
    ///
    /// **One of the six is, and it is the one that is a door rather than a
    /// verb.** R12's three rungs are about hover verbs — controls that act on
    /// the row they sit in, and whose whole discipline is zero footprint at rest
    /// (mock-up 1611). The masthead's `Graph` is not one of those: it opens
    /// another surface, it is the only way in to that surface, and the mock-up
    /// draws it as a plain always-there button (`.gopen`, line 1567). A door
    /// nobody can see is a feature nobody finds.
    #[must_use]
    pub fn rests_visible(self) -> bool {
        matches!(self, Self::OpenGraph)
    }

    /// Whether pressing this needs a confirmation first (R14).
    ///
    /// **Discard and nothing else.** Staging is undone by unstaging and unstaging
    /// by staging; a discard is undone by nobody, because the bytes it throws away
    /// were never given to git. That asymmetry — not the number of files — is
    /// what puts one of the five behind a door.
    #[must_use]
    pub fn needs_gate(self) -> bool {
        matches!(self, Self::Discard)
    }
}

/// What a press on one of this page's verbs becomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitPress {
    /// Ask the repository, now.
    Write(GitWriteVerb),
    /// Ask the *user*, first (R14).
    Gate,
    /// Ask the repository for another page of history.
    MoreCommits,
    /// Put the graph on the preview seat. Asks the repository nothing by itself
    /// — the document does its own asking once it is open.
    Graph,
}

/// The one place a verb turns into an act.
///
/// **Split out of the press handler so that "a discard cannot reach git without
/// the gate" is a fact a test can hold**, rather than a property of a function
/// that also needs a window, a tab and a worker to run. The handler then has no
/// judgement left in it: it looks the answer up here and does what it says.
#[must_use]
pub fn press_outcome(act: GitAct, untracked: bool) -> GitPress {
    if act == GitAct::LoadMore {
        return GitPress::MoreCommits;
    }
    if act == GitAct::OpenGraph {
        return GitPress::Graph;
    }
    if act.needs_gate() {
        return GitPress::Gate;
    }
    match act.verb(untracked) {
        Some(verb) => GitPress::Write(verb),
        // Unreachable by construction: `verb` answers `None` only for
        // `LoadMore`, which left at the first line. Written as the gate rather
        // than as a panic because a bug that made it reachable should stop at a
        // question, not at a write.
        None => GitPress::Gate,
    }
}

// ── the badge alphabet (R11 / R29) ─────────────────────────────────────────

/// Which claim a status letter makes, and therefore which ink it wears.
///
/// Four claims and nine letters, which is the point of the indirection: the page
/// says *added*, *changed*, *gone* and *not yet mine* in four colours, and the
/// letters are git's own. A colour per letter would be nine hues on a 240-pixel
/// column and a legend nobody has.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitBadgeInk {
    /// `A`, `C` — something that was not in the tree is.
    Added,
    /// `M`, `T`, `R` — something that was in the tree is different.
    Changed,
    /// `D`, `U` — something is gone, or two histories disagree about it.
    Gone,
    /// `?` — git has never seen this file. Quiet, because nothing has *happened*
    /// to it: a new file is not an event, it is an absence of one.
    Untracked,
}

/// The letters one status entry wears, in git's own column order.
///
/// **One function and not two**, because the Git page and the files tree behind
/// it (R32) show the same file's state and would otherwise be two readings of one
/// porcelain line: the index's letter first, then the working tree's. A space is
/// an absence and draws nothing.
///
/// **`??` is one badge and not two.** Porcelain spells untracked with both
/// columns, but the columns are not two claims there — an untracked file has no
/// index side and no working-tree side to disagree, it has one state — and a
/// row that drew `? ?` would be reading git's notation as if it were git's
/// meaning. The Untracked heading already says the word; the badge only has to
/// say it once. (`!!` is the same shape and is asked for by nothing.)
#[must_use]
pub fn badges_of(entry: &GitStatusEntry) -> Vec<GitBadge> {
    if matches!(entry.staged, Some(StatusCode::Untracked)) {
        return vec![GitBadge {
            letter: StatusCode::Untracked.letter(),
            ink: GitBadgeInk::Untracked,
        }];
    }
    [entry.staged, entry.unstaged]
        .into_iter()
        .flatten()
        .filter(|code| !matches!(code, StatusCode::Ignored))
        .map(|code| GitBadge {
            letter: code.letter(),
            ink: if entry.is_conflict() {
                // A conflict outranks its letters: `AA` is two additions that
                // are a disagreement, and drawing it green would be the picture
                // of a bug (R29).
                GitBadgeInk::Gone
            } else {
                GitBadgeInk::of(code)
            },
        })
        .collect()
}

impl GitBadgeInk {
    /// R29's mapping, in one place.
    #[must_use]
    pub fn of(code: StatusCode) -> Self {
        match code {
            StatusCode::Added | StatusCode::Copied => Self::Added,
            StatusCode::Modified | StatusCode::Typechange | StatusCode::Renamed => Self::Changed,
            StatusCode::Deleted | StatusCode::Unmerged => Self::Gone,
            // Never drawn: `!!` is asked for by nothing and belongs to no group.
            StatusCode::Untracked | StatusCode::Ignored => Self::Untracked,
        }
    }

    /// The ink itself (R29): green for added, the accent's blue for changed, the
    /// error red for gone, and the body's own quiet grey for untracked.
    #[must_use]
    pub fn colour(self, palette: &ChromePalette) -> [u8; 3] {
        match self {
            Self::Added => palette.status_ok,
            Self::Changed => palette.accent,
            Self::Gone => palette.status_err,
            Self::Untracked => palette.git_row_muted,
        }
    }
}

// ── what one frame of the page is ──────────────────────────────────────────

/// The masthead's words and their measured widths.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GitHead {
    /// The branch, or the sentence that stands in for one when there is no
    /// branch to name (R7).
    pub branch: String,
    pub branch_width: f32,
    /// Whether `branch` is a name or one of this module's own sentences —
    /// which is the difference between drawing it at 600 and drawing it quietly.
    pub named: bool,
    /// `↑ 2` and `↓ 1`, each already measured. Empty when there is no upstream
    /// or nothing to say about it: **zero draws no pill** (mock-up 4938).
    pub pills: Vec<GitPill>,
    /// **Something about this repository is still being read** (T5, v2 ③).
    ///
    /// The branch goes `git_head_muted` while any of the three questions is in
    /// flight and comes back to the text ink when they land — which is the whole
    /// of what the graph's refresh button shows for itself. A spinner would be a
    /// second thing on the strip claiming attention for a reading that usually
    /// finishes inside a frame; a name that has gone quiet says "this is what I
    /// last knew" without asking anybody to watch it.
    ///
    /// A field on the head rather than an argument to the painter because the
    /// panel draws this same head and must not be made to answer a question it
    /// does not have: it is `false` there, always, and the painter reads one
    /// value from one place.
    pub muted: bool,
}

/// One `.gud`.
#[derive(Clone, Debug, PartialEq)]
pub struct GitPill {
    pub text: String,
    pub text_width: f32,
    /// "N commits ahead" / "N commits behind" (R5).
    ///
    /// The mock-up said "N commits to push" and "to pull", and R5 struck both:
    /// this page has no push and no pull (G12), so a tooltip naming them promises
    /// a button that is not there. Ahead and behind are counts, and the sentence
    /// now says only what the count is.
    pub tooltip: String,
}

/// One changed file, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct GitChangeRow {
    /// Repo-relative, in git's grammar.
    pub path: String,
    /// What the row says about itself in the tooltip — the path in full, and
    /// where a rename came from.
    pub tooltip: String,
    /// One badge, or two when the file is in both index and working tree.
    ///
    /// **Two badges and not a merged one** (R11): `MM` means the index holds one
    /// version and the working tree another, so the file is genuinely a staged
    /// change *and* an unstaged one. This row is the one in `group`; the other
    /// badge is what the *other* group would show, and it is drawn beside it
    /// because a row that hid it would claim the file was in one place.
    pub badges: Vec<GitBadge>,
    pub group: GitGroup,
    /// Where a rename came from, when this row is one — what the diff this row
    /// opens needs in order to *be* a rename (see
    /// [`crate::git::GitQuestion::Diff`]'s own field).
    pub renamed_from: Option<String>,
    /// Whether a discard here deletes rather than restores.
    pub untracked: bool,
    /// A write about this path is in flight (R13).
    pub pending: bool,
}

/// One letter, and the ink it wears.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitBadge {
    pub letter: char,
    pub ink: GitBadgeInk,
}

/// One local branch, ready to draw (§D).
#[derive(Clone, Debug, PartialEq)]
pub struct GitBranchRow {
    pub name: String,
    pub name_width: f32,
    /// Whether `HEAD` is on it — the one that leads the list (R9) and wears the
    /// filled dot (G35).
    pub current: bool,
    /// Whether it is somebody else's — a `refs/remotes/…` row under the REMOTES
    /// sub-group (T9, v2 ③).
    ///
    /// Drawn in `git_row_muted` and answering no press **in this slice**: a
    /// press on `origin/main` means "make me a local branch tracking that", which
    /// is a *write* with its own refusals and its own naming rules, and it is
    /// v2 ④'s (M10). A row that checked out a remote-tracking ref the way a local
    /// row does would put the reader on a detached HEAD without a word about it.
    pub remote: bool,
    /// A checkout is in flight, for this branch or another one (R10).
    ///
    /// **The whole list dims and not only the row pressed**, which is the
    /// difference between this and a change row's own pending flag: staging one
    /// file leaves every other row true, while a checkout is about to move the
    /// *whole* repository — every branch's ahead and behind, every row of the
    /// history, and which of these dots is filled. A list that stayed bright
    /// while that was in flight would be claiming to still be true.
    pub pending: bool,
    /// When it was last committed to, through the one relative-time table (R8).
    pub time: String,
    pub time_width: f32,
    /// `↑ 2` and `↓ 1` against its own upstream — the same pill the masthead
    /// wears, because it is the same fact about a different branch.
    pub pills: Vec<GitPill>,
    pub tooltip: String,
}

/// One commit, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct GitCommitRow {
    /// The whole forty characters — what `git show` is asked with, and what an
    /// expansion is keyed on.
    ///
    /// The **full** hash and not the abbreviation git already gave us to draw:
    /// an abbreviation is short enough to be ambiguous by construction (git
    /// lengthens it as a repository grows), and a key that can collide is a key
    /// that opens the wrong commit's files on the day the repository gets big
    /// enough.
    pub hash: String,
    /// git's own abbreviation, at 10.5 and **last** on the row (R21).
    pub short: String,
    pub short_width: f32,
    /// The local branches standing on this commit (R22), in git's own order.
    ///
    /// The same pills the full graph wears, on the same page of the same row —
    /// R21 again. A 240-pixel column will run out of room for them long before
    /// the graph does, and the one that does not fit is dropped rather than cut,
    /// which is [`push_commit`]'s own rule and the graph's.
    pub refs: Vec<GitRefPill>,
    pub subject: String,
    pub time: String,
    pub time_width: f32,
    pub tooltip: String,
    /// More than one parent — the curve joins here.
    pub merge: bool,
    /// Whether the mini graph's line runs off the top and off the bottom. The
    /// first row has nothing above it and the last nothing below, and cutting
    /// those two half-lines is what makes the column read as one road rather
    /// than as a stack of dashes.
    pub first: bool,
    pub last: bool,
    /// Whether this commit's file list is standing open below it (R15).
    ///
    /// Drawn as a row that stays lit: the accordion has no chevron — the design
    /// gave the commit row its whole width to the graph, the hash, the subject
    /// and the time, and there is no column left for one — so *being open* is
    /// said by the row keeping the ground its hover gave it.
    pub expanded: bool,
}

/// One `.gref` on a commit row (R22).
///
/// **No lane on it, unlike the graph's own pill.** The graph gives a pill its
/// lane's colour because it has lanes; this page draws *one* honest lane in the
/// accent (mock-up 1596-1599, `.ggr circle { fill: var(--accent) }`), so every
/// pill here is that lane's, and a field holding a number that could only ever be
/// zero would be a lane algorithm this page does not have.
#[derive(Clone, Debug, PartialEq)]
pub struct GitRefPill {
    pub name: String,
    pub text_width: f32,
    /// Whether `HEAD` is this ref — the one that wears the ring at full strength
    /// (R22).
    pub head: bool,
}

/// One file an expanded commit touched (R15).
#[derive(Clone, Debug, PartialEq)]
pub struct GitCommitFileRow {
    /// Which commit this row is a reading of. Carried on the row rather than
    /// looked up from the column's view state when the row is pressed, for
    /// [`GitRow`]'s own reason: a row that indexes into something the presser
    /// also has to hold is a row that can disagree with it.
    pub hash: String,
    /// Repo-relative, in git's grammar.
    pub path: String,
    /// Where a rename came from — [`GitChangeRow::renamed_from`]'s twin, and
    /// needed for the same reason: `git show <hash> -- <new path>` loses a
    /// rename exactly as `git diff` does.
    pub renamed_from: Option<String>,
    pub badge: GitBadge,
    pub tooltip: String,
}

/// One row of the page.
///
/// Self-contained — every row carries what it draws — because the alternative is
/// a row that indexes into a status the painter also has to be handed, and a
/// painter holding two collections that must agree about an index is the shape
/// this codebase has already been bitten by twice.
#[derive(Clone, Debug, PartialEq)]
pub enum GitRow {
    Masthead(GitHead),
    Heading {
        /// Which group's rows are under it, when it is over a group at all.
        ///
        /// `None` for the history, which is a section and not a status group —
        /// and the difference is load-bearing rather than tidy: it is exactly
        /// what stops a "stage all" being offered over a list of commits.
        group: Option<GitGroup>,
        /// The heading's own word — already upper case, because the chrome text
        /// path has no `text-transform` and the design's `.glabel` does.
        label: &'static str,
        tooltip: &'static str,
        /// How many rows are under it. **Every heading carries its number**
        /// (R7): the design gave one of its four a count and left three bare
        /// with no reason written down, and a heading that says how many is one
        /// you can read without counting.
        count: usize,
        /// The section's own verb, when it has one.
        act: Option<GitAct>,
    },
    Change(GitChangeRow),
    /// One branch — local (§D / R9), or remote-tracking under the sub-group
    /// [`Self::Remotes`] opens (T9, v2 ③).
    Branch(GitBranchRow),
    /// **REMOTES (N)** — the folded sub-group under BRANCHES (T9, v2 ③).
    ///
    /// Its own row and not a [`Self::Heading`] with a flag, because the two are
    /// different things: a heading is a word over a list and answers no press of
    /// its own, and this is a *control* — pressing it opens and shuts what is
    /// under it. Giving `Heading` an `open` field would have made every one of
    /// the four headings on this page pressable-looking to any reader of the
    /// type, which is exactly the kind of "one variant, two meanings" the row
    /// enum was flattened to avoid.
    ///
    /// **Under BRANCHES rather than beside it**, and folded by default: a column
    /// 240 pixels wide showing a fetched repository would otherwise open on a
    /// page whose first screenful is other people's branches. What the reader
    /// came for is the branch they are on.
    Remotes {
        count: usize,
        open: bool,
    },
    Commit(GitCommitRow),
    /// A file under the one expanded commit (R15).
    CommitFile(GitCommitFileRow),
    LoadMore,
    /// A sentence in the list's own voice: the status cap's surrender (R33),
    /// or a group that came back empty when the page expected one.
    Notice(String),
}

impl GitRow {
    /// Whether this row stands on a section card.
    ///
    /// The masthead and the headings do not — they sit on the pane's own body,
    /// which is why their inks are mixed over `--termbg` and every other ink on
    /// this page is mixed over `--panel`.
    #[must_use]
    fn on_card(&self) -> bool {
        matches!(
            self,
            Self::Change(_)
                | Self::Branch(_)
                | Self::Commit(_)
                | Self::CommitFile(_)
                | Self::LoadMore
                | Self::Notice(_)
        )
    }

    /// How tall this row is, at this scale.
    fn height(&self, scale: f32) -> f32 {
        match self {
            Self::Masthead(_) => ((GIT_HEAD_PADDING_TOP_LOGICAL_PX
                + GIT_HEAD_FONT_LOGICAL_PX.max(GIT_PILL_HEIGHT_LOGICAL_PX)
                + GIT_HEAD_PADDING_BOTTOM_LOGICAL_PX)
                * scale)
                .round(),
            // The REMOTES row is a heading's height because it *is* a heading
            // with a press on it — a sub-group's word standing over its own
            // list, in the same `.glabel` grammar as the four above it.
            Self::Heading { .. } | Self::Remotes { .. } => ((GIT_LABEL_PADDING_TOP_LOGICAL_PX
                + GIT_LABEL_LINE_LOGICAL_PX
                + GIT_LABEL_PADDING_BOTTOM_LOGICAL_PX)
                * scale)
                .round(),
            Self::Change(_)
            | Self::Branch(_)
            | Self::Commit(_)
            | Self::CommitFile(_)
            | Self::LoadMore
            | Self::Notice(_) => (GIT_ROW_HEIGHT_LOGICAL_PX * scale).round().max(1.0),
        }
    }
}

/// **What a press on a row's body opens** (R15/R25) — and nothing else does.
///
/// Split out of the press handler for [`press_outcome`]'s reason: which document
/// a row stands for is a fact about the page, and a fact about the page should be
/// holdable by a test that has no window, no tab and no worker. The handler then
/// looks the answer up here and carries it out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitRowOpen {
    /// Put this document on the preview seat.
    Document {
        source: PreviewSource,
        /// What the head and the switcher call it.
        name: String,
        /// The other half of a rename, for the question this open will ask.
        ///
        /// **Beside the identity rather than inside it.** Two readings of one
        /// file at one stage are one document however the file came by its name,
        /// so this is not part of [`PreviewSource`] — it is a fact the *command*
        /// needs (`crate::git::GitQuestion::Diff::renamed_from`) and nothing
        /// else does.
        renamed_from: Option<String>,
    },
    /// Turn this commit's file list over (R15).
    Expand { hash: String },
    /// **Stand somewhere else** (R10) — a branch row pressed, or a commit row
    /// double-clicked in the graph.
    ///
    /// There is no gate in front of it and that is the ruling rather than an
    /// omission: a checkout destroys nothing, and the one case where it would —
    /// a working tree whose changes it would overwrite — is refused by git
    /// itself, in git's own words, which the banner then prints. What this page
    /// does not offer is the two ways past that refusal: `--force` throws the
    /// work away and a stash hides it somewhere the panel does not show. Both
    /// are heavy verbs, and heavy verbs belong to the terminal beside this
    /// column (G12).
    Checkout {
        /// A branch name, or a commit when `detach` is set.
        target: String,
        detach: bool,
    },
}

/// The display name a git document wears: the file's base name and `.diff`.
///
/// **The suffix says what it is to a reader and to nothing else** (R24). The
/// mock-up added the same three letters so that the *view* would be chosen by
/// the name; that mechanism is retired, and what is left is a caption, which is
/// what it always should have been. A pane 240 pixels wide has room for a name,
/// so the folders above it are dropped exactly as a files row drops them.
#[must_use]
pub fn git_document_name(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    format!("{base}.diff")
}

/// Which document a row's body opens, if any.
#[must_use]
pub fn row_document(row: &GitRow, root: &Path) -> Option<GitRowOpen> {
    match row {
        // **R25's whole mapping.** A row under STAGED is a claim about the
        // index, so the diff it opens is the index's; every other group's row is
        // about the working tree. An untracked row asks the working tree too and
        // gets nothing, which is the honest answer — git has no copy of that
        // file to differ from, and the pane says so in one line.
        GitRow::Change(change) => Some(GitRowOpen::Document {
            source: PreviewSource::GitDiff {
                root: root.to_owned(),
                path: change.path.clone(),
                staged: change.group == GitGroup::Staged,
            },
            name: git_document_name(&change.path),
            renamed_from: change.renamed_from.clone(),
        }),
        GitRow::Commit(commit) => Some(GitRowOpen::Expand {
            hash: commit.hash.clone(),
        }),
        GitRow::CommitFile(file) => Some(GitRowOpen::Document {
            source: PreviewSource::GitShow {
                root: root.to_owned(),
                hash: file.hash.clone(),
                path: file.path.clone(),
            },
            name: git_document_name(&file.path),
            renamed_from: file.renamed_from.clone(),
        }),
        // **A branch row is a verb, not a document** (R10) — and the current
        // one is not even that: the mock-up gives it `cursor: default` because
        // there is nowhere for it to take you. A row already waiting on a
        // checkout answers nothing either, which is the same guard `+` has
        // against being pressed twice.
        //
        // A **remote** row answers nothing either, and see [`GitBranchRow::remote`]
        // for why that is the ruling and not a gap: checking out `origin/main`
        // is not what pressing it should mean.
        GitRow::Branch(branch) if branch.current || branch.pending || branch.remote => None,
        GitRow::Branch(branch) => Some(GitRowOpen::Checkout {
            target: branch.name.clone(),
            detach: false,
        }),
        // A masthead, a heading, the "load more" row and a notice are not
        // documents. The first three answer their own presses through
        // [`GitAct`]; the last answers none. The REMOTES row is a control and
        // answers its own press — see [`row_toggles_remotes`].
        GitRow::Masthead(_)
        | GitRow::Heading { .. }
        | GitRow::Remotes { .. }
        | GitRow::LoadMore
        | GitRow::Notice(_) => None,
    }
}

/// Whether a press on this row opens or shuts the REMOTES sub-group (T9).
///
/// Beside [`row_document`] rather than folded into it for the reason that
/// function's own doc gives: what a press *means* is a fact about the page, and
/// a sub-group toggle is not a document — putting it into `GitRowOpen` would
/// have added a variant that opens nothing to an enum whose whole name is what
/// opens.
#[must_use]
pub fn row_toggles_remotes(row: &GitRow) -> bool {
    matches!(row, GitRow::Remotes { .. })
}

/// **The accordion** (R15): one commit open, or none.
///
/// An `Option` and not a set, and that is the whole ruling: a 240-pixel column
/// with three commits' file lists standing open in it is a history you have to
/// scroll past your own expansions to read. Pressing the open one shuts it;
/// pressing another *replaces* it.
#[must_use]
pub fn toggled_expansion(current: Option<&str>, hash: &str) -> Option<String> {
    (current != Some(hash)).then(|| hash.to_owned())
}

/// One frame of the Git page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GitPanelContent {
    pub rows: Vec<GitRow>,
    /// How far the list is scrolled, in physical pixels. Clamped by whoever
    /// wrote it — [`clamp_git_scroll`] — on `FilesTreeContent::scroll_px`'s own
    /// ruling.
    pub scroll_px: f32,
    /// The whole page is one sentence: there is no repository here (R17), the
    /// answer has not arrived yet, or git would not read this repository at all.
    /// Drawn centred, and the rows are then empty.
    ///
    /// **A `String` and not a `&'static str`** since the toast ruling
    /// (2026-08-16). Two of the three sentences are constants; the third is
    /// git's own words about a repository it refused to open, which used to be
    /// printed in a red strip above a page that then said "Reading the
    /// repository…" forever. A persistent read fault is not a transient notice —
    /// nothing about it will change until the machine does — so it is not a
    /// toast, and it is not red either: it is simply what this page has to say,
    /// standing in the muted ink where the rows would have been.
    pub empty: Option<String>,
}

// ── building one from a cache ──────────────────────────────────────────────

/// How wide a run of text is, at a size — the caller's font, the caller's answer.
///
/// Everything measured on this page is measured through one closure for the
/// reason every other measured caption in this codebase is: only the thing
/// holding the font can say how wide a string is, and a second measurer is a
/// second answer.
pub type Measure<'a> = dyn FnMut(&str, f32) -> f32 + 'a;

/// **What one column is doing**, as against what its repository said.
///
/// Two fields that always travel together and always came from the same place —
/// the column's own [`crate::seats::FilesLeafState`] — gathered into one value
/// for [`crate::git_graph::GraphLook`]'s reason: `build` would otherwise take an
/// `Option<&str>` and a bare `bool` in a row, and a bare `bool` in a signature is
/// a flag whose meaning lives only in the caller's memory.
///
/// `expanded` is the one commit whose files are showing (R15). It is here rather
/// than off the cache because it is a *view* state — which commit this column is
/// looking into — and the cache is what the repository said. The cache holds the
/// answer; whether it is on screen is the column's.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GitPanelLook<'a> {
    pub expanded: Option<&'a str>,
    /// Whether the REMOTES sub-group is unfolded (T9, v2 ③).
    ///
    /// **Durable, like `view` beside it** — see
    /// [`bt_persist::FilesLeafV1::remotes_open`]. A reader who works against a
    /// fork all morning and finds the remotes folded again every time the window
    /// reopens is being told the product did not notice; and unlike the one open
    /// commit next to it, "I want to see the remotes" is a fact about the
    /// *column* rather than a glance at a history that may have moved on.
    pub remotes_open: bool,
}

/// Turn what a column knows into what it draws.
///
/// **The whole derivation, in one place.** The painter, the hit test and the
/// wheel all read the rows this returns, so a row that is drawn is a row that can
/// be pressed and a row that can be pressed is a row that is there.
///
/// [`GitPanelLook`] is what the *column* is doing, as against what the
/// repository said — see that type.
#[must_use]
pub fn build(
    cache: &GitCache,
    look: GitPanelLook<'_>,
    scale: f32,
    measure: &mut Measure<'_>,
) -> GitPanelContent {
    let GitPanelLook {
        expanded,
        remotes_open,
    } = look;
    let mut content = GitPanelContent::default();

    // The repository probe answers first and answers for everything: until it
    // has, there is no root to ask anything else about.
    match cache.repo() {
        GitSlot::Idle | GitSlot::Pending => {
            content.empty = Some(GIT_READING.to_owned());
            return content;
        }
        GitSlot::Failed(GitFault::NotARepository) => {
            content.empty = Some(GIT_NOT_A_REPOSITORY.to_owned());
            return content;
        }
        // A machine with no git, a repository git refuses to read, a question it
        // would not finish: three different sentences, none of them "not a
        // repository", and all three are git's own words — **standing where the
        // rows would be** rather than in a red strip over a page that then
        // claimed to be still reading (toast ruling, 2026-08-16). None of these
        // is transient: nothing about a machine with no git will change while
        // you look at it, so none of them is a toast either.
        GitSlot::Failed(fault) => {
            content.empty = Some(fault_sentence(fault));
            return content;
        }
        GitSlot::Ready(_) => {}
    }

    // The same ruling, one level down: a status git would not read is a fact
    // about this page and not a thing that just happened, so its sentence goes
    // in a [`GitRow::Notice`] where that status's groups would have stood — the
    // row kind the open-commit expansion already reports its own faults with.
    // The page around it is still true and still drawn, which is what a banner
    // over the whole column could never say.
    let (status, status_fault) = match cache.status() {
        GitSlot::Ready(status) => (Some(status), None),
        GitSlot::Failed(fault) => (None, Some(fault_sentence(fault))),
        GitSlot::Idle | GitSlot::Pending => (None, None),
    };

    content
        .rows
        .push(GitRow::Masthead(masthead(status, scale, measure)));

    // **Branches first** (G25's group order, R9's contents): the list of places
    // this repository can stand, with the one it is standing on at the top. The
    // sort is `parse_refs`' own — current first, then newest — and it is done
    // there rather than here because "the current one leads" is a fact about the
    // answer and not about the drawing of it.
    //
    // **Locals, then a folded REMOTES sub-group** (T9, v2 ③). Tags are
    // deliberately absent: v2 keeps this column compact, and a tag is a name
    // this page offers no verb for. They are drawn where they mean something,
    // which is on the commit they stand on, in the graph.
    match cache.refs() {
        GitSlot::Ready(refs)
            if refs
                .iter()
                .any(|entry| entry.kind == crate::git::GitRefKind::Local) =>
        {
            let locals: Vec<&crate::git::GitRefEntry> = crate::git::local_branches(refs).collect();
            let remotes: Vec<&crate::git::GitRefEntry> =
                crate::git::remote_branches(refs).collect();
            content.rows.push(GitRow::Heading {
                group: None,
                label: GIT_BRANCHES_HEADING,
                tooltip: GIT_BRANCHES_TOOLTIP,
                // **The locals and not the whole answer.** The number over a
                // heading is how many rows are under it, and the remotes are
                // under a heading of their own that carries its own count.
                count: locals.len(),
                // No group verb: "stage all" over a list of branches is not a
                // sentence, and a checkout of all of them is not a thing.
                act: None,
            });
            let waiting = cache.checkout_pending().is_some();
            for branch in locals {
                content
                    .rows
                    .push(GitRow::Branch(branch_row(branch, waiting, scale, measure)));
            }
            // A repository with no remote has no sub-group — a row reading
            // `REMOTES (0)` would be a control offering to open nothing.
            if !remotes.is_empty() {
                content.rows.push(GitRow::Remotes {
                    count: remotes.len(),
                    open: remotes_open,
                });
                if remotes_open {
                    for branch in remotes {
                        content
                            .rows
                            .push(GitRow::Branch(branch_row(branch, waiting, scale, measure)));
                    }
                }
            }
        }
        GitSlot::Failed(fault) => {
            content.rows.push(GitRow::Notice(fault_sentence(fault)));
        }
        // A repository with no local branches at all is an unborn one, and the
        // masthead has already said so — a heading over nothing would be a
        // second, quieter way of saying the same thing.
        GitSlot::Ready(_) | GitSlot::Idle | GitSlot::Pending => {}
    }

    // Where the three groups would have stood, when there is no status to build
    // them from.
    if let Some(words) = status_fault {
        content.rows.push(GitRow::Notice(words));
    }

    // The three groups, in the order the design fixes: what is packed, what is
    // edited, what git has not been told about. **A group with nothing in it
    // draws no heading and costs no height** (R7) — the same rule the settings
    // dialog's own headings follow.
    for group in [GitGroup::Staged, GitGroup::Changes, GitGroup::Untracked] {
        let Some(status) = status else { continue };
        let entries: Vec<&GitStatusEntry> = status.group(group).collect();
        if entries.is_empty() {
            continue;
        }
        content.rows.push(GitRow::Heading {
            group: Some(group),
            label: group_heading(group),
            tooltip: group_tooltip(group),
            count: entries.len(),
            act: group_act(group),
        });
        for entry in entries {
            content
                .rows
                .push(GitRow::Change(change_row(entry, group, cache)));
        }
        // R33: the cap read everything and shows the first two thousand, and it
        // says so in a row of its own rather than by quietly being short.
        if status.dropped > 0 && group == GitGroup::Untracked {
            content.rows.push(GitRow::Notice(format!(
                "{} more changed files not shown",
                status.dropped
            )));
        }
    }

    // History. Unconditional (R7 keeps the count on the heading and the heading
    // on the group), because a repository always has a history even when it is
    // empty — and when it is, the row that says so is the honest answer.
    match cache.log() {
        GitSlot::Ready(log) => {
            content.rows.push(GitRow::Heading {
                group: None,
                label: GIT_COMMITS_HEADING,
                tooltip: GIT_COMMITS_TOOLTIP,
                count: log.commits.len(),
                act: None,
            });
            let last = log.commits.len().saturating_sub(1);
            for (index, commit) in log.commits.iter().enumerate() {
                let open = expanded == Some(commit.hash.as_str());
                content.rows.push(GitRow::Commit(commit_row(
                    commit,
                    index == 0,
                    index == last && !log.has_more,
                    open,
                    scale,
                    measure,
                )));
                if open {
                    push_expansion(&mut content.rows, cache, &commit.hash);
                }
            }
            if log.has_more {
                content.rows.push(GitRow::LoadMore);
            }
        }
        GitSlot::Failed(fault) => {
            content.rows.push(GitRow::Notice(fault_sentence(fault)));
        }
        GitSlot::Idle | GitSlot::Pending => {}
    }

    content
}

/// The open commit's file list, or the one line that stands in for it (R15).
///
/// **All four states of the slot get a row**, on [`GitCache`]'s own reading of
/// why: an expansion that drew nothing while the subprocess ran would look like
/// a commit that touched no files, and then like one that touched some — a list
/// that flickers into existence under the row you just pressed.
///
/// The empty case is the interesting one and it is not an error: `--name-status`
/// on a **merge** compares against no parent and answers with nothing, so the
/// sentence says which kind of nothing this is rather than leaving a gap.
fn push_expansion(rows: &mut Vec<GitRow>, cache: &GitCache, hash: &str) {
    match cache.commit_files(hash) {
        Some(GitSlot::Ready(files)) if !files.is_empty() => {
            rows.extend(files.iter().map(|file| commit_file_row(file, hash)));
        }
        // **Only git answering with nothing earns the "no files" sentence.**
        Some(GitSlot::Ready(_)) => {
            rows.push(GitRow::Notice(GIT_COMMIT_NO_FILES.to_owned()));
        }
        // A hash this cache holds no answer for is a question not yet answered,
        // and that is what this sentence says. It is unreachable by
        // construction — an expansion and its question are begun in the same
        // call — and it is written this way rather than as the line above
        // because if it *were* ever reached, claiming the commit touched no
        // files would be a claim about a repository nobody has asked.
        None | Some(GitSlot::Idle | GitSlot::Pending) => {
            rows.push(GitRow::Notice(GIT_READING.to_owned()));
        }
        Some(GitSlot::Failed(fault)) => rows.push(GitRow::Notice(fault_sentence(fault))),
    }
}

fn commit_file_row(file: &GitCommitFile, hash: &str) -> GitRow {
    GitRow::CommitFile(GitCommitFileRow {
        hash: hash.to_owned(),
        tooltip: match &file.renamed_from {
            Some(from) => format!("{} — renamed from {from}", file.path),
            None => file.path.clone(),
        },
        path: file.path.clone(),
        renamed_from: file.renamed_from.clone(),
        badge: GitBadge {
            letter: file.code.letter(),
            ink: GitBadgeInk::of(file.code),
        },
    })
}

/// One branch row (the inventory's section D), local or remote (T9).
fn branch_row(
    branch: &crate::git::GitRefEntry,
    waiting: bool,
    scale: f32,
    measure: &mut Measure<'_>,
) -> GitBranchRow {
    let font = GIT_ROW_FONT_LOGICAL_PX * scale;
    let time_font = GIT_TIME_FONT_LOGICAL_PX * scale;
    let pill_font = GIT_PILL_FONT_LOGICAL_PX * scale;
    let mut pills = Vec::new();
    if branch.ahead > 0 {
        pills.push(pill(ARROW_UP, branch.ahead, "ahead", pill_font, measure));
    }
    if branch.behind > 0 {
        pills.push(pill(
            ARROW_DOWN,
            branch.behind,
            "behind",
            pill_font,
            measure,
        ));
    }
    GitBranchRow {
        name_width: measure(&branch.name, font),
        time_width: measure(&branch.committerdate_relative, time_font),
        // The mock-up's own two sentences (G36), and the second one is the only
        // place this page names the verb a branch row carries. A remote row
        // carries no verb (T9), so it says what it *is* instead of promising a
        // checkout it will not do.
        tooltip: match (branch.kind, branch.is_head) {
            (crate::git::GitRefKind::Remote, _) => {
                format!("{} - a branch on a remote", branch.name)
            }
            (_, true) => format!("{} - the branch you are on", branch.name),
            (_, false) => format!("Check out {}", branch.name),
        },
        remote: branch.kind == crate::git::GitRefKind::Remote,
        name: branch.name.clone(),
        current: branch.is_head,
        pending: waiting,
        time: branch.committerdate_relative.clone(),
        pills,
    }
}

/// The masthead a repository earns, asked for by a cache rather than a status.
///
/// **The graph's door to the same sentence** (R20): the panel builds this from
/// the status it already has in hand, and the graph has only a cache — so the
/// step from one to the other lives here, once, instead of in both.
#[must_use]
pub fn head_of(cache: &GitCache, scale: f32, measure: &mut Measure<'_>) -> GitHead {
    masthead(cache.status().ready(), scale, measure)
}

/// A repository whose history is empty — R7's own sentence for it.
pub const GIT_NO_COMMITS: &str = "No commits yet";

/// The masthead: which branch, and how far from its upstream.
fn masthead(
    status: Option<&crate::git::GitStatus>,
    scale: f32,
    measure: &mut Measure<'_>,
) -> GitHead {
    let font = GIT_HEAD_FONT_LOGICAL_PX * scale;
    let pill_font = GIT_PILL_FONT_LOGICAL_PX * scale;
    let Some(status) = status else {
        return GitHead {
            branch: GIT_READING.to_owned(),
            branch_width: measure(GIT_READING, font),
            named: false,
            pills: Vec::new(),
            muted: false,
        };
    };
    // Three states and three sentences (R7). A detached head has no branch to
    // name; an unborn one has a name that points at nothing, and saying only the
    // name would claim a branch that has never existed.
    let (branch, named) = match (&status.branch, status.detached, status.unborn) {
        (_, true, _) => (GIT_DETACHED.to_owned(), false),
        (Some(name), _, true) => (format!("{name} — {GIT_UNBORN}"), true),
        (Some(name), _, _) => (name.clone(), true),
        (None, _, _) => (GIT_DETACHED.to_owned(), false),
    };
    let mut pills = Vec::new();
    if status.ahead > 0 {
        pills.push(pill(ARROW_UP, status.ahead, "ahead", pill_font, measure));
    }
    if status.behind > 0 {
        pills.push(pill(
            ARROW_DOWN,
            status.behind,
            "behind",
            pill_font,
            measure,
        ));
    }
    GitHead {
        branch_width: measure(&branch, font),
        branch,
        named,
        pills,
        // The panel never says this — see the field.
        muted: false,
    }
}

/// The two arrows a count pill wears (G22), named once so a branch row and the
/// masthead cannot drift apart on which character they are.
const ARROW_UP: char = '↑';
const ARROW_DOWN: char = '↓';

fn pill(
    arrow: char,
    count: usize,
    direction: &str,
    font: f32,
    measure: &mut Measure<'_>,
) -> GitPill {
    let text = format!("{arrow} {count}");
    GitPill {
        text_width: measure(&text, font),
        text,
        // "1 commit ahead", not "1 commits ahead". The plural is one `if` and
        // the alternative is a sentence that is wrong every time the count is
        // one, which on a branch you have just committed to is most of the time.
        tooltip: if count == 1 {
            format!("1 commit {direction}")
        } else {
            format!("{count} commits {direction}")
        },
    }
}

/// Which verb a group's heading offers.
///
/// Staged unstages; the other two stage. There is no group-level discard, and
/// that is R14 being read strictly rather than an omission: "throw away every
/// change in the working tree" is exactly the kind of one-click irreversibility
/// that the terminal beside this panel is the right place for.
fn group_act(group: GitGroup) -> Option<GitAct> {
    match group {
        GitGroup::Staged => Some(GitAct::UnstageAll),
        GitGroup::Changes | GitGroup::Untracked => Some(GitAct::StageAll),
    }
}

fn change_row(entry: &GitStatusEntry, group: GitGroup, cache: &GitCache) -> GitChangeRow {
    let untracked = group == GitGroup::Untracked;
    let badges = badges_of(entry);
    GitChangeRow {
        tooltip: match &entry.renamed_from {
            Some(from) => format!("{} — renamed from {from}", entry.path),
            None => entry.path.clone(),
        },
        pending: cache.write_pending(&entry.path),
        path: entry.path.clone(),
        renamed_from: entry.renamed_from.clone(),
        badges,
        group,
        untracked,
    }
}

#[allow(clippy::fn_params_excessive_bools)]
fn commit_row(
    commit: &crate::git::GitCommit,
    first: bool,
    last: bool,
    expanded: bool,
    scale: f32,
    measure: &mut Measure<'_>,
) -> GitCommitRow {
    let merge = commit.parents.len() > 1;
    let hash_font = GIT_HASH_FONT_LOGICAL_PX * scale;
    let time_font = GIT_TIME_FONT_LOGICAL_PX * scale;
    let ref_font = crate::git_graph::GRAPH_REF_FONT_LOGICAL_PX * scale;
    GitCommitRow {
        short_width: measure(&commit.short, hash_font),
        time_width: measure(&commit.time_relative, time_font),
        refs: commit
            .refs
            .iter()
            .map(|reference| GitRefPill {
                text_width: measure(&reference.name, ref_font),
                name: reference.name.clone(),
                head: reference.head,
            })
            .collect(),
        // R16 puts the author in the tooltip and never in the row: a 240-pixel
        // column has room for the message or for who wrote it, and on a machine
        // where almost every commit is yours the message is the one that varies.
        //
        // **R16 still holds *here*** after v2 ① gave the graph an author column
        // (V1): the ruling was about a 240-pixel column, and this is still that
        // column. What the two surfaces share is the sentence — the graph writes
        // the same `Name <email>` in its own tooltip, through
        // [`crate::git_graph::author_sentence`], so one commit does not describe
        // its author two ways on one screen.
        tooltip: if merge {
            format!(
                "Merge commit — another branch's history joins here\n{}\n{}",
                commit.subject,
                crate::git_graph::author_sentence(commit)
            )
        } else {
            format!(
                "{}\n{}",
                commit.subject,
                crate::git_graph::author_sentence(commit)
            )
        },
        hash: commit.hash.clone(),
        short: commit.short.clone(),
        subject: commit.subject.clone(),
        time: commit.time_relative.clone(),
        merge,
        first,
        last,
        expanded,
    }
}

/// One line for a fault, in git's own words wherever there are any (W3).
#[must_use]
pub fn fault_sentence(fault: &GitFault) -> String {
    match fault {
        GitFault::GitMissing(words) | GitFault::Refused(words) => words.clone(),
        GitFault::TimedOut => "git did not answer and was stopped".to_owned(),
        GitFault::NotARepository => GIT_NOT_A_REPOSITORY.to_owned(),
    }
}

// ── where everything lands ─────────────────────────────────────────────────

/// One derivation of the page's rectangles, read by the painter, the hit test
/// and the wheel.
///
/// It owns a vector where [`crate::seats::FilesTreeGeometry`] owns four numbers,
/// and the reason is the page itself: a tree is one row height repeated, and this
/// is six row kinds of three different heights with cards drawn around runs of
/// them. Arithmetic that could place row `n` without walking `0..n` would have to
/// re-derive the card rule at every reader, and the card rule is exactly what a
/// second derivation would get wrong.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GitPanelGeometry {
    /// What the rows are laid into and clipped to — the body, whole.
    ///
    /// It used to be the body *less a banner strip*, and the strip is gone (toast
    /// ruling, 2026-08-16): a transient failure no longer costs the page twenty-two
    /// permanent pixels and no longer moves every row down while it is on screen.
    pub viewport: [f32; 4],
    pub scroll_px: f32,
    pub content_height: f32,
    pub max_scroll: f32,
    /// Row rectangles, already offset by the scroll.
    rows: Vec<[f32; 4]>,
    /// The section cards, behind the rows that stand on them.
    cards: Vec<[f32; 4]>,
}

impl GitPanelGeometry {
    /// Where row `index` is, or a degenerate rectangle past the end.
    #[must_use]
    pub fn row_rect(&self, index: usize) -> [f32; 4] {
        self.rows.get(index).copied().unwrap_or([0.0; 4])
    }

    /// The section cards, in the order they were laid down.
    #[must_use]
    pub fn cards(&self) -> &[[f32; 4]] {
        &self.cards
    }

    /// Which row the pointer is on, if any is.
    ///
    /// Clipped to the viewport first, which is what stops a row scrolled under
    /// the pane head from answering a press aimed at the head.
    #[must_use]
    pub fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        if x < self.viewport[0]
            || x >= self.viewport[2]
            || y < self.viewport[1]
            || y >= self.viewport[3]
        {
            return None;
        }
        self.rows
            .iter()
            .position(|rect| y >= rect[1] && y < rect[3] && x >= rect[0] && x < rect[2])
    }
}

/// Lay the page out inside a body.
///
/// **The whole body is the viewport.** It once had a strip carved off the top for
/// a failure banner; the toast ruling (2026-08-16) took the strip away, and with
/// it the one thing on this page whose height depended on whether something had
/// just gone wrong. A notice now stands *over* the body — see [`crate::toast`] —
/// so the rows are laid out in the same rectangle whatever git last said.
#[must_use]
pub fn git_panel_geometry(
    body: [f32; 4],
    content: &GitPanelContent,
    scale: f32,
) -> GitPanelGeometry {
    let viewport = body;

    let pad_x = (GIT_VIEW_PADDING_X_LOGICAL_PX * scale).round();
    let pad_top = (GIT_VIEW_PADDING_TOP_LOGICAL_PX * scale).round();
    let pad_bottom = (GIT_VIEW_PADDING_BOTTOM_LOGICAL_PX * scale).round();
    let card_pad = (GIT_SECTION_PADDING_LOGICAL_PX * scale).round();
    let head_pad_x = (GIT_HEAD_PADDING_X_LOGICAL_PX * scale).round();
    let label_pad_x = (GIT_LABEL_PADDING_X_LOGICAL_PX * scale).round();

    let left = viewport[0] + pad_x;
    let right = (viewport[2] - pad_x).max(left);

    let mut rows = Vec::with_capacity(content.rows.len());
    let mut cards = Vec::new();
    let mut y = pad_top;
    // Where the card under the current run of card-rows began, in content space.
    let mut card_top: Option<f32> = None;

    for row in &content.rows {
        let on_card = row.on_card();
        if on_card && card_top.is_none() {
            card_top = Some(y);
            y += card_pad;
        }
        if !on_card && let Some(top) = card_top.take() {
            y += card_pad;
            cards.push([left, top, right, y]);
        }
        let height = row.height(scale);
        let (row_left, row_right) = match row {
            GitRow::Masthead(_) => (left + head_pad_x, (right - head_pad_x).max(left)),
            GitRow::Heading { .. } => (left + label_pad_x, (right - label_pad_x).max(left)),
            // The one indented row on the page — under its commit's message,
            // beside its commit's lane.
            GitRow::CommitFile(_) => (
                left + card_pad + (GIT_COMMIT_FILE_INDENT_LOGICAL_PX * scale).round(),
                (right - card_pad).max(left),
            ),
            _ => (left + card_pad, (right - card_pad).max(left)),
        };
        rows.push([row_left, y, row_right, y + height]);
        y += height;
    }
    if let Some(top) = card_top.take() {
        y += card_pad;
        cards.push([left, top, right, y]);
    }
    let content_height = y + pad_bottom;
    let max_scroll = (content_height - (viewport[3] - viewport[1])).max(0.0);

    // Everything above was laid out in content space, starting at zero; one
    // translation puts it on screen. Doing it here rather than inside the loop
    // is what keeps `content_height` a property of the content.
    let offset = viewport[1] - content.scroll_px;
    for rect in rows.iter_mut().chain(cards.iter_mut()) {
        rect[1] += offset;
        rect[3] += offset;
    }

    GitPanelGeometry {
        viewport,
        scroll_px: content.scroll_px,
        content_height,
        max_scroll,
        rows,
        cards,
    }
}

/// The only scroll a Git page is allowed to hold.
///
/// Every write goes through here, on [`crate::seats::clamp_files_scroll`]'s own
/// ruling: the bound belongs where the writing happens, so the picture is then
/// simply what the number says.
#[must_use]
pub fn clamp_git_scroll(
    body: [f32; 4],
    content: &GitPanelContent,
    scroll_px: f32,
    scale: f32,
) -> f32 {
    let mut probe = content.clone();
    probe.scroll_px = 0.0;
    let max = git_panel_geometry(body, &probe, scale).max_scroll;
    scroll_px.clamp(0.0, max)
}

/// Where a row's verbs are, right to left from its trailing edge.
///
/// **One derivation for the painter and the hit test**, which is the rule this
/// module's siblings already follow: a list that computes its buttons twice is a
/// list whose press lands on the button beside the one you can see.
#[must_use]
pub fn act_boxes(row: &GitRow, rect: [f32; 4], scale: f32) -> Vec<(GitAct, [f32; 4])> {
    let acts: Vec<GitAct> = match row {
        // Right to left, so the *destructive* verb is furthest from the trailing
        // edge a pointer travels along: `+` sits at the end, `×` inside it. A
        // discard under the thumb's resting place is a discard that gets pressed.
        GitRow::Change(change) if change.pending => Vec::new(),
        GitRow::Change(change) => match change.group {
            GitGroup::Staged => vec![GitAct::Unstage],
            GitGroup::Changes | GitGroup::Untracked => vec![GitAct::Stage, GitAct::Discard],
        },
        GitRow::Heading { act, .. } => act.iter().copied().collect(),
        GitRow::Masthead(_) => vec![GitAct::OpenGraph],
        // **The whole row is the button.** It has no glyph and no reserved
        // corner — it is a sentence you press — so its box is the row's own, and
        // saying that here rather than in the press handler is what makes the
        // hit test, the tooltip and the verb one answer. It was *not* here
        // first, and the cost was exact: the row lit under the pointer, said
        // "Load fifty more commits", and did nothing when pressed, because the
        // hit test could only offer verbs this function knew about.
        GitRow::LoadMore => vec![GitAct::LoadMore],
        _ => Vec::new(),
    };
    if acts == [GitAct::LoadMore] {
        return vec![(GitAct::LoadMore, rect)];
    }
    let box_ = (GIT_ACT_LOGICAL_PX * scale).round().max(1.0);
    let gap = (GIT_ACT_GAP_LOGICAL_PX * scale).round();
    let middle = ((rect[1] + rect[3] - box_) / 2.0).round();
    let mut placed = Vec::with_capacity(acts.len());
    // **Inside the row's own padding, not flush with its edge** (user report,
    // 2026-08-16). The verbs are the last flex child of a padded row — `.grow`
    // is `padding: 5px 7px`, `.glabel` and the masthead `2px` — so their
    // trailing edge is the padding's, exactly where the row's text already
    // stops (`push_change` clips the name at `rect[2] - pad - reserved`). Drawn
    // from `rect[2]` itself the `+` touched the row's rounded corner and its
    // pill overran the ground it lit.
    let mut edge = rect[2] - (act_trailing_padding(row) * scale).round();
    for act in acts {
        let left = edge - box_;
        if left < rect[0] {
            break;
        }
        placed.push((act, [left, middle, left + box_, middle + box_]));
        edge = left - gap;
    }
    placed
}

/// The horizontal padding a row kind keeps between its trailing edge and its
/// last child, in logical pixels — each kind's own, from its own CSS rule.
fn act_trailing_padding(row: &GitRow) -> f32 {
    match row {
        GitRow::Masthead(_) => GIT_HEAD_PADDING_X_LOGICAL_PX,
        GitRow::Heading { .. } => GIT_LABEL_PADDING_X_LOGICAL_PX,
        _ => GIT_ROW_PADDING_X_LOGICAL_PX,
    }
}

/// Which verb the pointer is on, inside a row.
#[must_use]
pub fn act_at(row: &GitRow, rect: [f32; 4], scale: f32, x: f32, y: f32) -> Option<GitAct> {
    act_boxes(row, rect, scale)
        .into_iter()
        .find(|(_, box_)| x >= box_[0] && x < box_[2] && y >= box_[1] && y < box_[3])
        .map(|(act, _)| act)
}

/// Where the masthead's pills are, given how wide its branch name is drawn.
///
/// **One derivation for the paint and for the tooltip**, which is why this is a
/// function and not a loop inside the painter: R5's whole point is that these
/// pills carry a sentence, and a sentence anchored to a rectangle the painter
/// computed separately is a tip that appears next to nothing.
#[must_use]
pub fn pill_boxes(head: &GitHead, rect: [f32; 4], scale: f32) -> Vec<[f32; 4]> {
    let gap = (GIT_HEAD_GAP_LOGICAL_PX * scale).round();
    let mark = (GIT_HEAD_MARK_LOGICAL_PX * scale).round().max(1.0);
    let height = (GIT_PILL_HEIGHT_LOGICAL_PX * scale).round().max(1.0);
    let pad = (GIT_PILL_PADDING_X_LOGICAL_PX * scale).round();
    let top = ((rect[1] + rect[3] - height) / 2.0).round();
    // The `Graph` button holds the trailing edge (G24's `margin-left:auto`), so
    // the pills' room ends where its own box begins. Taken from [`act_boxes`]
    // rather than re-measured, on this function's own rule: two derivations of
    // one edge is a pill drawn under a button.
    let button = (GIT_ACT_LOGICAL_PX * scale).round().max(1.0) + gap;
    let limit = (rect[2] - button).max(rect[0]);
    let name_right = (rect[0] + mark + gap + head.branch_width).min(limit);
    let mut left = name_right + gap;
    let mut boxes = Vec::with_capacity(head.pills.len());
    for pill in &head.pills {
        let width = pill.text_width + pad * 2.0;
        if left + width > limit {
            break;
        }
        boxes.push([left, top, left + width, top + height]);
        left += width + gap;
    }
    boxes
}

/// Every tooltip this page offers, keyed by row.
///
/// Built beside the rows rather than asked for at hover time, for the reason the
/// rest of the chrome's tooltips are: the hit test is `&self` and cannot format.
#[must_use]
pub fn row_tooltip(row: &GitRow) -> Option<String> {
    match row {
        GitRow::Masthead(_) => None,
        GitRow::Heading { tooltip, .. } => Some((*tooltip).to_owned()),
        GitRow::Branch(branch) => Some(branch.tooltip.clone()),
        GitRow::Change(change) => Some(change.tooltip.clone()),
        GitRow::Commit(commit) => Some(commit.tooltip.clone()),
        GitRow::CommitFile(file) => Some(file.tooltip.clone()),
        GitRow::LoadMore => Some(GitAct::LoadMore.tooltip(false).to_owned()),
        GitRow::Remotes { open, .. } => Some(
            if *open {
                GIT_REMOTES_TOOLTIP_OPEN
            } else {
                GIT_REMOTES_TOOLTIP_SHUT
            }
            .to_owned(),
        ),
        GitRow::Notice(_) => None,
    }
}

// ── the paint ──────────────────────────────────────────────────────────────

/// What the pointer is on, as the painter needs to know it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GitHover {
    pub row: Option<usize>,
    pub act: Option<GitAct>,
}

/// Draw one Git page.
///
/// **This is the virtualization**, and it is [`crate::seats::push_files_tree`]'s:
/// rows outside the viewport are skipped before anything is built for them, in
/// the same loop that builds the rest. A repository with ten thousand changed
/// files draws the two dozen rows the column can show.
#[allow(clippy::too_many_arguments)]
pub fn push_git_panel(
    body: [f32; 4],
    content: &GitPanelContent,
    hover: GitHover,
    scale: f32,
    palette: &ChromePalette,
    out: (
        &mut Vec<ChromeQuad>,
        &mut Vec<ChromeLabel>,
        &mut Vec<ChromeSprite>,
    ),
) {
    let (quads, labels, sprites) = out;
    let geometry = git_panel_geometry(body, content, scale);

    if let Some(sentence) = content.empty.as_ref() {
        labels.push(ChromeLabel {
            text: sentence.clone(),
            rect: geometry.viewport,
            font_size_px: GIT_EMPTY_FONT_LOGICAL_PX * scale,
            color: palette.git_head_muted,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(geometry.viewport),
        });
        return;
    }

    let viewport = geometry.viewport;
    let crop = |rect: [f32; 4]| {
        [
            rect[0],
            rect[1].max(viewport[1]),
            rect[2],
            rect[3].min(viewport[3]),
        ]
    };
    let visible = |rect: [f32; 4]| rect[3] > viewport[1] && rect[1] < viewport[3];

    // The cards first, under everything: `.gsec` is a ground, and a ground drawn
    // after its rows would cover them.
    let card_radius = (GIT_SECTION_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    for card in geometry.cards() {
        if !visible(*card) {
            continue;
        }
        sprites.push(ChromeSprite::new(
            ChromeMark::ControlPill {
                radius_px: card_radius,
            },
            crop(*card),
            palette.git_section,
        ));
    }

    for (index, row) in content.rows.iter().enumerate() {
        let rect = geometry.row_rect(index);
        if !visible(rect) {
            continue;
        }
        let hovered = hover.row == Some(index);
        match row {
            GitRow::Masthead(head) => {
                push_git_masthead(head, rect, scale, palette, (labels, sprites), &crop);
                push_acts(row, rect, hover, hovered, scale, palette, sprites, &crop);
            }
            GitRow::Heading { label, count, .. } => {
                push_heading(label, *count, rect, scale, palette, labels, &crop);
                push_acts(row, rect, hover, hovered, scale, palette, sprites, &crop);
            }
            // The sub-group's own word, with a disclosure triangle in front of
            // it: the same glyph at the same angle a directory row turns, so
            // "this opens" is said the one way this window says it (T9).
            GitRow::Remotes { count, open } => {
                push_remotes_heading(
                    *count,
                    *open,
                    rect,
                    scale,
                    palette,
                    (labels, sprites),
                    &crop,
                );
            }
            GitRow::Change(change) => {
                push_row_ground(rect, hovered, scale, palette, sprites, &crop);
                push_change(
                    change,
                    rect,
                    hovered,
                    scale,
                    palette,
                    (labels, sprites),
                    &crop,
                );
                push_acts(row, rect, hover, hovered, scale, palette, sprites, &crop);
            }
            GitRow::Branch(branch) => {
                push_row_ground(
                    rect,
                    hovered && !branch.current,
                    scale,
                    palette,
                    sprites,
                    &crop,
                );
                push_branch(
                    branch,
                    rect,
                    hovered && !branch.current,
                    scale,
                    palette,
                    (labels, sprites),
                    &crop,
                );
            }
            GitRow::Commit(commit) => {
                // **An open commit keeps the ground its hover gave it** — the
                // accordion's whole affordance, and the reason the row's inks
                // are then the hovered set too: a lit row with unlit text reads
                // as a row the pointer has left, not as one that is open.
                let lit = hovered || commit.expanded;
                push_row_ground(rect, lit, scale, palette, sprites, &crop);
                push_commit(
                    commit,
                    rect,
                    lit,
                    scale,
                    palette,
                    (quads, labels, sprites),
                    &crop,
                );
            }
            GitRow::CommitFile(file) => {
                push_row_ground(rect, hovered, scale, palette, sprites, &crop);
                push_commit_file(
                    file,
                    rect,
                    hovered,
                    scale,
                    palette,
                    (labels, sprites),
                    &crop,
                );
            }
            GitRow::LoadMore => {
                push_row_ground(rect, hovered, scale, palette, sprites, &crop);
                labels.push(ChromeLabel {
                    text: GIT_LOAD_MORE.to_owned(),
                    rect,
                    font_size_px: GIT_ROW_FONT_LOGICAL_PX * scale,
                    color: if hovered {
                        palette.git_row_text_hover
                    } else {
                        palette.git_row_muted
                    },
                    align_right: false,
                    align_center: true,
                    letter_spacing_em: 0.0,
                    weight: ChromeLabelWeight::Regular,
                    tabular_numerals: false,
                    clip: Some(crop(rect)),
                });
            }
            GitRow::Notice(words) => {
                labels.push(ChromeLabel {
                    text: words.clone(),
                    rect: inset(rect, (GIT_ROW_PADDING_X_LOGICAL_PX * scale).round()),
                    font_size_px: GIT_TIME_FONT_LOGICAL_PX * scale,
                    color: palette.git_row_muted,
                    align_right: false,
                    align_center: false,
                    letter_spacing_em: 0.0,
                    weight: ChromeLabelWeight::Regular,
                    tabular_numerals: false,
                    clip: Some(crop(rect)),
                });
            }
        }
    }
}

fn inset(rect: [f32; 4], by: f32) -> [f32; 4] {
    [
        rect[0] + by,
        rect[1],
        (rect[2] - by).max(rect[0] + by),
        rect[3],
    ]
}

fn push_row_ground(
    rect: [f32; 4],
    hovered: bool,
    scale: f32,
    palette: &ChromePalette,
    sprites: &mut Vec<ChromeSprite>,
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    if !hovered {
        return;
    }
    sprites.push(ChromeSprite::new(
        ChromeMark::ControlPill {
            radius_px: (GIT_ROW_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32,
        },
        crop(rect),
        palette.git_row_hover,
    ));
}

pub fn push_git_masthead(
    head: &GitHead,
    rect: [f32; 4],
    scale: f32,
    palette: &ChromePalette,
    out: (&mut Vec<ChromeLabel>, &mut Vec<ChromeSprite>),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (labels, sprites) = out;
    let gap = (GIT_HEAD_GAP_LOGICAL_PX * scale).round();
    let mark = (GIT_HEAD_MARK_LOGICAL_PX * scale).round().max(1.0);
    let font = GIT_HEAD_FONT_LOGICAL_PX * scale;
    let middle = |size: f32| ((rect[1] + rect[3] - size) / 2.0).round();

    let mark_rect = [rect[0], middle(mark), rect[0] + mark, middle(mark) + mark];
    sprites.push(ChromeSprite::new(
        ChromeMark::GitBranch,
        crop(mark_rect),
        // The mark is the branch's own, so it wears the branch's ink rather than
        // the accent: this is a *label*, not an affordance, and the accent in
        // this product means "something over there wants you".
        palette.git_head_text,
    ));

    let name_left = mark_rect[2] + gap;
    let name_rect = [
        name_left,
        rect[1],
        (name_left + head.branch_width).min(rect[2]),
        rect[3],
    ];
    labels.push(ChromeLabel {
        text: head.branch.clone(),
        rect: name_rect,
        font_size_px: font,
        color: if head.named && !head.muted {
            palette.git_head_text
        } else {
            // "detached HEAD" is a state and not a name, so it is said quietly —
            // the same distinction `.gbr.cur` draws between the branch you are on
            // and the ones you are not.
            palette.git_head_muted
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: if head.named {
            ChromeLabelWeight::SemiBold
        } else {
            ChromeLabelWeight::Regular
        },
        tabular_numerals: false,
        clip: Some(crop(name_rect)),
    });

    let pill_radius = (GIT_PILL_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    let edge = (GIT_PILL_EDGE_LOGICAL_PX * scale).round().max(1.0) as u32;
    for (pill, box_) in head.pills.iter().zip(pill_boxes(head, rect, scale)) {
        sprites.push(ChromeSprite::new(
            ChromeMark::ControlPillRing {
                radius_px: pill_radius,
                stroke_px: edge,
            },
            crop(box_),
            palette.git_pill_border,
        ));
        labels.push(ChromeLabel {
            text: pill.text.clone(),
            rect: box_,
            font_size_px: GIT_PILL_FONT_LOGICAL_PX * scale,
            color: palette.git_pill_text,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: true,
            clip: Some(crop(box_)),
        });
    }
}

/// One branch row: a dot that says where `HEAD` is, a name, its counts and its
/// age.
#[allow(clippy::too_many_arguments)]
fn push_branch(
    branch: &GitBranchRow,
    rect: [f32; 4],
    hovered: bool,
    scale: f32,
    palette: &ChromePalette,
    out: (&mut Vec<ChromeLabel>, &mut Vec<ChromeSprite>),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (labels, sprites) = out;
    let pad = (GIT_ROW_PADDING_X_LOGICAL_PX * scale).round();
    let gap = (GIT_ROW_GAP_LOGICAL_PX * scale).round();
    let dot = (GIT_BRANCH_DOT_LOGICAL_PX * scale).round().max(1.0);
    let edge = (GIT_BRANCH_DOT_EDGE_LOGICAL_PX * scale).round().max(1.0) as u32;
    // **A checkout in flight fades the list, it does not empty it** (R13's own
    // pessimism, one list along): what is on screen is still what the repository
    // last said, and it is about to stop being true.
    let opacity = if branch.pending {
        GIT_PENDING_FADE
    } else {
        1.0
    };
    let middle = |size: f32| ((rect[1] + rect[3] - size) / 2.0).round();

    let dot_rect = [
        rect[0] + pad,
        middle(dot),
        rect[0] + pad + dot,
        middle(dot) + dot,
    ];
    let radius = (dot / 2.0).round().max(1.0) as u32;
    // Filled for the branch you are on, an empty ring for the ones you are not
    // (G35) — the tab pin's rule, and the reason it needs no legend: a filled
    // mark is a state and an outlined one is an offer.
    sprites.push(
        ChromeSprite::new(
            if branch.current {
                ChromeMark::ControlPill { radius_px: radius }
            } else {
                ChromeMark::ControlPillRing {
                    radius_px: radius,
                    stroke_px: edge,
                }
            },
            crop(dot_rect),
            if branch.current {
                palette.accent
            } else {
                palette.git_row_muted
            },
        )
        .with_opacity(opacity),
    );

    let muted = if hovered {
        palette.git_row_muted_hover
    } else {
        palette.git_row_muted
    };
    // The pills sit between the name and the age, so the age is measured off the
    // trailing edge first and everything else is fitted inside what is left.
    let time_left = (rect[2] - pad - branch.time_width).max(rect[0]);
    let time_rect = [time_left, rect[1], rect[2] - pad, rect[3]];
    labels.push(ChromeLabel {
        text: branch.time.clone(),
        rect: time_rect,
        font_size_px: GIT_TIME_FONT_LOGICAL_PX * scale,
        color: muted,
        align_right: true,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: true,
        clip: Some(crop(time_rect)),
    });

    let pill_height = (GIT_PILL_HEIGHT_LOGICAL_PX * scale).round().max(1.0);
    let pill_pad = (GIT_PILL_PADDING_X_LOGICAL_PX * scale).round();
    let pill_radius = (GIT_PILL_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    let pill_edge = (GIT_PILL_EDGE_LOGICAL_PX * scale).round().max(1.0) as u32;
    let mut pill_edge_x = time_rect[0] - gap;
    for pill in branch.pills.iter().rev() {
        let width = pill.text_width + pill_pad * 2.0;
        let left = pill_edge_x - width;
        if left < rect[0] + pad + dot + gap {
            break;
        }
        let box_ = [
            left,
            middle(pill_height),
            left + width,
            middle(pill_height) + pill_height,
        ];
        sprites.push(
            ChromeSprite::new(
                ChromeMark::ControlPillRing {
                    radius_px: pill_radius,
                    stroke_px: pill_edge,
                },
                crop(box_),
                palette.git_pill_border,
            )
            .with_opacity(opacity),
        );
        labels.push(ChromeLabel {
            text: pill.text.clone(),
            rect: box_,
            font_size_px: GIT_PILL_FONT_LOGICAL_PX * scale,
            color: palette.git_pill_text,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: true,
            clip: Some(crop(box_)),
        });
        pill_edge_x = left - gap;
    }

    let name_left = dot_rect[2] + gap;
    let name_rect = [
        name_left,
        rect[1],
        (pill_edge_x - gap).max(name_left),
        rect[3],
    ];
    labels.push(ChromeLabel {
        text: branch.name.clone(),
        rect: name_rect,
        font_size_px: GIT_ROW_FONT_LOGICAL_PX * scale,
        // `.gbr bdi { color: --ink2 }`, and `--ink` at 500 for the current one
        // (G34): the branch you are on is the fact, the rest are offers.
        //
        // **A remote row is quieter than both** (T9): `git_row_muted` is this
        // page's ink for *furniture* — a hash, an age — and a name that carries
        // no verb in this slice is exactly that. It does not brighten under the
        // pointer either, because there is nothing there to reach for.
        color: if branch.remote {
            palette.git_row_muted
        } else if branch.current {
            palette.git_row_text
        } else if hovered {
            palette.git_act_glyph_hover
        } else {
            palette.git_act_glyph
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: if branch.current {
            ChromeLabelWeight::Medium
        } else {
            ChromeLabelWeight::Regular
        },
        tabular_numerals: false,
        clip: Some(crop(name_rect)),
    });
}

fn push_heading(
    label: &str,
    count: usize,
    rect: [f32; 4],
    scale: f32,
    palette: &ChromePalette,
    labels: &mut Vec<ChromeLabel>,
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    // The heading's text sits at the *bottom* of its box, because the box's 14px
    // of top padding is the gap between two sections and not leading.
    let line = (GIT_LABEL_LINE_LOGICAL_PX * scale).round();
    let bottom_pad = (GIT_LABEL_PADDING_BOTTOM_LOGICAL_PX * scale).round();
    let text_rect = [
        rect[0],
        rect[3] - bottom_pad - line,
        rect[2],
        rect[3] - bottom_pad,
    ];
    labels.push(ChromeLabel {
        text: format!("{label} ({count})"),
        rect: text_rect,
        font_size_px: GIT_LABEL_FONT_LOGICAL_PX * scale,
        color: palette.git_head_muted,
        align_right: false,
        align_center: false,
        letter_spacing_em: GIT_LABEL_TRACKING_EM,
        weight: ChromeLabelWeight::SemiBold,
        tabular_numerals: false,
        clip: Some(crop(text_rect)),
    });
}

/// The REMOTES sub-group's own row (T9): a disclosure triangle and a heading.
///
/// The triangle is the files tree's own glyph at the two ends of its turn
/// ([`crate::marks::tree_disclosure`]), because "this opens" has exactly one
/// drawing in this window and a second one would be a second idiom for one idea.
/// It does not animate here and that is not a shortcut: a files row's triangle
/// turns because the row's children slide in under it, and this list has no
/// motion of its own to be in step with.
#[allow(clippy::too_many_arguments)]
fn push_remotes_heading(
    count: usize,
    open: bool,
    rect: [f32; 4],
    scale: f32,
    palette: &ChromePalette,
    out: (&mut Vec<ChromeLabel>, &mut Vec<ChromeSprite>),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (labels, sprites) = out;
    let line = (GIT_LABEL_LINE_LOGICAL_PX * scale).round();
    let bottom_pad = (GIT_LABEL_PADDING_BOTTOM_LOGICAL_PX * scale).round();
    let mark = (GIT_REMOTES_MARK_LOGICAL_PX * scale).round().max(1.0);
    let gap = (GIT_REMOTES_MARK_GAP_LOGICAL_PX * scale).round();
    let baseline_top = rect[3] - bottom_pad - line;
    let mark_top = (baseline_top + (line - mark) / 2.0).round();
    let mark_rect = [rect[0], mark_top, rect[0] + mark, mark_top + mark];
    sprites.push(ChromeSprite::new(
        crate::marks::tree_disclosure(if open { 1.0 } else { 0.0 }),
        crop(mark_rect),
        palette.git_head_muted,
    ));
    let text_rect = [
        mark_rect[2] + gap,
        baseline_top,
        rect[2],
        rect[3] - bottom_pad,
    ];
    labels.push(ChromeLabel {
        text: format!("{GIT_REMOTES_HEADING} ({count})"),
        rect: text_rect,
        font_size_px: GIT_LABEL_FONT_LOGICAL_PX * scale,
        color: palette.git_head_muted,
        align_right: false,
        align_center: false,
        letter_spacing_em: GIT_LABEL_TRACKING_EM,
        weight: ChromeLabelWeight::SemiBold,
        tabular_numerals: false,
        clip: Some(crop(text_rect)),
    });
}

#[allow(clippy::too_many_arguments)]
fn push_change(
    change: &GitChangeRow,
    rect: [f32; 4],
    hovered: bool,
    scale: f32,
    palette: &ChromePalette,
    out: (&mut Vec<ChromeLabel>, &mut Vec<ChromeSprite>),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (labels, sprites) = out;
    let pad = (GIT_ROW_PADDING_X_LOGICAL_PX * scale).round();
    let gap = (GIT_ROW_GAP_LOGICAL_PX * scale).round();
    let badge = (GIT_BADGE_LOGICAL_PX * scale).round().max(1.0);
    let badge_gap = (GIT_BADGE_GAP_LOGICAL_PX * scale).round();
    let badge_radius = (GIT_BADGE_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    let ground = if hovered {
        palette.git_row_hover
    } else {
        palette.git_section
    };
    let middle = |size: f32| ((rect[1] + rect[3] - size) / 2.0).round();
    let fade = if change.pending {
        GIT_PENDING_FADE
    } else {
        1.0
    };

    let mut left = rect[0] + pad;
    for mark in &change.badges {
        let box_ = [left, middle(badge), left + badge, middle(badge) + badge];
        let ink = mark.ink.colour(palette);
        sprites.push(
            ChromeSprite::new(
                ChromeMark::ControlPill {
                    radius_px: badge_radius,
                },
                crop(box_),
                // The design's `color-mix(… 15%, transparent)` over whatever the
                // row is standing on, composited by the one compositor.
                bt_render::ink_over(ground, ink, GIT_BADGE_GROUND_ALPHA),
            )
            .with_opacity(fade),
        );
        labels.push(ChromeLabel {
            text: mark.letter.to_string(),
            rect: box_,
            font_size_px: GIT_BADGE_FONT_LOGICAL_PX * scale,
            // A status letter is a *signal*, and signals are the one thing this
            // product's ink discipline lets hold colour at rest (mock-up 1612).
            color: if change.pending {
                bt_render::ink_over(ground, ink, (fade * 1000.0) as i32)
            } else {
                ink
            },
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::SemiBold,
            tabular_numerals: false,
            clip: Some(crop(box_)),
        });
        left = box_[2] + badge_gap;
    }
    // The badges' own gap is 3; the gap between the last badge and the path is
    // the row's 8.
    let name_left = left - badge_gap + gap;
    // The verbs' reserved width, whether or not they are showing: *appearing
    // must not nudge the row* (mock-up 1646).
    let reserved = reserved_act_width(
        match change.group {
            GitGroup::Staged => 1,
            GitGroup::Changes | GitGroup::Untracked => 2,
        },
        scale,
    );
    let name_rect = [
        name_left,
        rect[1],
        (rect[2] - pad - reserved).max(name_left),
        rect[3],
    ];
    let text = if hovered {
        palette.git_row_text_hover
    } else {
        palette.git_row_text
    };
    labels.push(ChromeLabel {
        text: change.path.clone(),
        rect: name_rect,
        font_size_px: GIT_ROW_FONT_LOGICAL_PX * scale,
        color: if change.pending {
            bt_render::ink_over(ground, text, (fade * 1000.0) as i32)
        } else {
            text
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(crop(name_rect)),
    });
}

/// One file under an expanded commit (R15): its letter, and its path.
///
/// The change row's vocabulary with everything a change row has and this does
/// not taken out — no second badge, because a commit has one story per file; no
/// verbs, because there is nothing to stage in a commit that has already
/// happened; no reserved corner, because reserving room for verbs that do not
/// exist would push the paths in for nothing.
#[allow(clippy::too_many_arguments)]
fn push_commit_file(
    file: &GitCommitFileRow,
    rect: [f32; 4],
    hovered: bool,
    scale: f32,
    palette: &ChromePalette,
    out: (&mut Vec<ChromeLabel>, &mut Vec<ChromeSprite>),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (labels, sprites) = out;
    let pad = (GIT_ROW_PADDING_X_LOGICAL_PX * scale).round();
    let gap = (GIT_ROW_GAP_LOGICAL_PX * scale).round();
    let badge = (GIT_BADGE_LOGICAL_PX * scale).round().max(1.0);
    let badge_radius = (GIT_BADGE_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    let ground = if hovered {
        palette.git_row_hover
    } else {
        palette.git_section
    };
    let top = ((rect[1] + rect[3] - badge) / 2.0).round();
    let box_ = [rect[0] + pad, top, rect[0] + pad + badge, top + badge];
    let ink = file.badge.ink.colour(palette);
    sprites.push(ChromeSprite::new(
        ChromeMark::ControlPill {
            radius_px: badge_radius,
        },
        crop(box_),
        bt_render::ink_over(ground, ink, GIT_BADGE_GROUND_ALPHA),
    ));
    labels.push(ChromeLabel {
        text: file.badge.letter.to_string(),
        rect: box_,
        font_size_px: GIT_BADGE_FONT_LOGICAL_PX * scale,
        color: ink,
        align_right: false,
        align_center: true,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::SemiBold,
        tabular_numerals: false,
        clip: Some(crop(box_)),
    });
    let name_rect = [
        box_[2] + gap,
        rect[1],
        (rect[2] - pad).max(box_[2]),
        rect[3],
    ];
    labels.push(ChromeLabel {
        text: file.path.clone(),
        rect: name_rect,
        font_size_px: GIT_ROW_FONT_LOGICAL_PX * scale,
        color: if hovered {
            palette.git_row_text_hover
        } else {
            palette.git_row_text
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(crop(name_rect)),
    });
}

/// How much room `count` verbs keep at a row's trailing edge.
fn reserved_act_width(count: usize, scale: f32) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let box_ = (GIT_ACT_LOGICAL_PX * scale).round().max(1.0);
    let gap = (GIT_ACT_GAP_LOGICAL_PX * scale).round();
    box_ * count as f32 + gap * (count - 1) as f32
}

#[allow(clippy::too_many_arguments)]
fn push_acts(
    row: &GitRow,
    rect: [f32; 4],
    hover: GitHover,
    hovered: bool,
    scale: f32,
    palette: &ChromePalette,
    sprites: &mut Vec<ChromeSprite>,
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    // **R12's three rungs.** Absent while the pointer is elsewhere, seven-tenths
    // once the row has it, whole — over its own pill — once the button does. The
    // mock-up's `.gact` had only the outer two; unifying on `.pv-tool`'s ladder
    // is what makes every hover verb in this product fade at one rate.
    let glyph = (GIT_ACT_GLYPH_LOGICAL_PX * scale).round().max(1.0);
    let radius = (GIT_ACT_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    // The masthead stands on the pane's own body and every other row on a
    // `--panel` card, so a button up there wears the body's inks — the same
    // split the branch name and the group headings already live by.
    let on_body = matches!(row, GitRow::Masthead(_));
    for (act, box_) in act_boxes(row, rect, scale) {
        if !hovered && !act.rests_visible() {
            continue;
        }
        let lit = hover.act == Some(act);
        if lit {
            sprites.push(ChromeSprite::new(
                ChromeMark::ControlPill { radius_px: radius },
                crop(box_),
                if on_body {
                    palette.files_row_hover
                } else {
                    palette.git_act_pill
                },
            ));
        }
        let inset = ((box_[2] - box_[0]) - glyph) / 2.0;
        let glyph_box = [
            box_[0] + inset,
            box_[1] + inset,
            box_[2] - inset,
            box_[3] - inset,
        ];
        let mut mark = ChromeSprite::new(
            act.mark(),
            crop(glyph_box),
            match (lit, on_body) {
                (true, true) => palette.git_head_text,
                (true, false) => palette.git_act_glyph_on_pill,
                (false, true) => palette.git_head_muted,
                (false, false) => palette.git_act_glyph_hover,
            },
        );
        // A button that is always there is always whole: the ladder is what a
        // hover verb climbs, and this one never left the ground.
        mark.opacity = if lit || act.rests_visible() {
            1.0
        } else {
            GIT_ACT_REVEAL
        };
        sprites.push(mark);
    }
}

#[allow(clippy::too_many_arguments)]
fn push_commit(
    commit: &GitCommitRow,
    rect: [f32; 4],
    hovered: bool,
    scale: f32,
    palette: &ChromePalette,
    out: (
        &mut Vec<ChromeQuad>,
        &mut Vec<ChromeLabel>,
        &mut Vec<ChromeSprite>,
    ),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (quads, labels, sprites) = out;
    let pad_left = (GIT_COMMIT_PADDING_LEFT_LOGICAL_PX * scale).round();
    let pad_right = (GIT_ROW_PADDING_X_LOGICAL_PX * scale).round();
    let gap = (GIT_ROW_GAP_LOGICAL_PX * scale).round();
    let ground = if hovered {
        palette.git_row_hover
    } else {
        palette.git_section
    };
    let muted = if hovered {
        palette.git_row_muted_hover
    } else {
        palette.git_row_muted
    };

    // ── the mini graph: one lane, honestly ──
    //
    // *A single honest lane with merges curving in — situational awareness, not
    // a GitKraken; the full graph is an IDE's furniture* (mock-up 1625). Lane
    // arithmetic from the real DAG is G-4's, and it lives in the preview seat
    // where there is width for it. What this column owes is that the line is
    // continuous down the list, which is why the first and last rows cut their
    // halves.
    let column = (GIT_GRAPH_WIDTH_LOGICAL_PX * scale).round();
    let stroke = (GIT_GRAPH_STROKE_LOGICAL_PX * scale).round().max(1.0);
    let dot = (GIT_GRAPH_DOT_RADIUS_LOGICAL_PX * scale).round().max(1.0) * 2.0;
    let lane_x = (rect[0] + pad_left + column / 2.0).round();
    let lane_mid = ((rect[1] + rect[3]) / 2.0).round();
    let line_ink = bt_render::ink_over(ground, muted, GIT_GRAPH_LINE_ALPHA);
    let mut lane = |top: f32, bottom: f32| {
        let clipped = crop([lane_x - stroke / 2.0, top, lane_x + stroke / 2.0, bottom]);
        if clipped[3] > clipped[1] {
            quads.push(ChromeQuad {
                rect: clipped,
                color: line_ink,
            });
        }
    };
    if !commit.first {
        lane(rect[1], lane_mid);
    }
    if !commit.last {
        lane(lane_mid, rect[3]);
    }
    if commit.merge {
        // The design's curve, in the design's own 14×27 box, laid over this row.
        let curve = [
            rect[0] + pad_left,
            rect[1],
            rect[0] + pad_left + column,
            rect[3],
        ];
        sprites.push(ChromeSprite::new(
            ChromeMark::GitMergeCurve,
            crop(curve),
            // `.ggr path.side { stroke: var(--ok) }` — the joining history wears
            // the green that means "this arrived", which is the one place on this
            // page the merge line and an `A` badge agree.
            palette.status_ok,
        ));
    }
    sprites.push(ChromeSprite::new(
        ChromeMark::ControlPill {
            radius_px: (dot / 2.0).round().max(1.0) as u32,
        },
        crop([
            lane_x - dot / 2.0,
            lane_mid - dot / 2.0,
            lane_x + dot / 2.0,
            lane_mid + dot / 2.0,
        ]),
        // `.ggr circle { fill: var(--accent) }`.
        palette.accent,
    ));

    // ── the columns (R21): graph, refs, message, time, hash ──
    //
    // **This is the order the full graph draws, and it is now the only order.**
    // R21 was filed against the mock-up because the two surfaces disagreed: the
    // panel put the hash before the message (mock-up 4886-4889) and the graph put
    // it last (4837-4843). It is ruled in the graph's favour (2026-08-16), and
    // the reason is what the columns are for rather than which draft came first —
    // the message is the one thing a reader is scanning for, and a fixed-width
    // hash standing in front of it indents every message on the page by seven
    // characters of something nobody reads down a list. Right-aligned at the far
    // edge it is a column you can look *across* when you want one, and out of the
    // way when you do not. The time follows it inward for the same reason it does
    // in the graph: both are addresses, and addresses live at the end of a line.
    let hash_right = rect[2] - pad_right;
    let hash_rect = [
        (hash_right - commit.short_width).max(rect[0]),
        rect[1],
        hash_right,
        rect[3],
    ];
    labels.push(ChromeLabel {
        text: commit.short.clone(),
        rect: hash_rect,
        font_size_px: GIT_HASH_FONT_LOGICAL_PX * scale,
        color: muted,
        align_right: true,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: true,
        clip: Some(crop(hash_rect)),
    });

    let time_right = hash_rect[0] - gap;
    let time_rect = [
        (time_right - commit.time_width).max(rect[0]),
        rect[1],
        time_right,
        rect[3],
    ];
    labels.push(ChromeLabel {
        text: commit.time.clone(),
        rect: time_rect,
        font_size_px: GIT_TIME_FONT_LOGICAL_PX * scale,
        color: muted,
        align_right: true,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: true,
        clip: Some(crop(time_rect)),
    });

    // The pills, left to right after the graph column — **the graph's own
    // arithmetic and the graph's own numbers** (`GRAPH_REF_*`), because R21 asks
    // for one row and not for two that look alike. One that would cross the time
    // stops the run rather than being cut: half a branch name is a different
    // branch name.
    let mut cursor = rect[0] + pad_left + column + gap;
    let pill_height = (crate::git_graph::GRAPH_REF_HEIGHT_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    let pill_pad = (crate::git_graph::GRAPH_REF_PADDING_X_LOGICAL_PX * scale).round();
    let pill_radius = (crate::git_graph::GRAPH_REF_RADIUS_LOGICAL_PX * scale)
        .round()
        .max(1.0) as u32;
    let pill_edge = (crate::git_graph::GRAPH_REF_EDGE_LOGICAL_PX * scale)
        .round()
        .max(1.0) as u32;
    let pill_top = ((rect[1] + rect[3] - pill_height) / 2.0).round();
    for pill in &commit.refs {
        let width = pill.text_width + pill_pad * 2.0;
        if cursor + width > time_rect[0] - gap {
            break;
        }
        let box_ = [cursor, pill_top, cursor + width, pill_top + pill_height];
        // The one lane this page has (G58): its dot is the accent, so its names
        // are the accent's too.
        let lane = palette.accent;
        sprites.push(
            ChromeSprite::new(
                ChromeMark::ControlPill {
                    radius_px: pill_radius,
                },
                crop(box_),
                lane,
            )
            .with_opacity(ref_alpha(crate::git_graph::GRAPH_REF_GROUND_ALPHA)),
        );
        // **`HEAD` wears the ring at full strength** (R22). In the graph the ring
        // also changes colour, because a lane's hue is not the accent; here every
        // pill is already accent-coloured and what separates "you are standing
        // here" from "a name lives here" is the edge going solid.
        sprites.push(
            ChromeSprite::new(
                ChromeMark::ControlPillRing {
                    radius_px: pill_radius,
                    stroke_px: pill_edge,
                },
                crop(box_),
                lane,
            )
            .with_opacity(if pill.head {
                1.0
            } else {
                ref_alpha(crate::git_graph::GRAPH_REF_EDGE_ALPHA)
            }),
        );
        labels.push(ChromeLabel {
            text: pill.name.clone(),
            rect: box_,
            font_size_px: crate::git_graph::GRAPH_REF_FONT_LOGICAL_PX * scale,
            color: lane,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::SemiBold,
            tabular_numerals: false,
            clip: Some(crop(box_)),
        });
        cursor += width + gap;
    }

    let subject_rect = [cursor, rect[1], (time_rect[0] - gap).max(cursor), rect[3]];
    labels.push(ChromeLabel {
        text: commit.subject.clone(),
        rect: subject_rect,
        font_size_px: GIT_ROW_FONT_LOGICAL_PX * scale,
        color: if hovered {
            palette.git_row_text_hover
        } else {
            palette.git_row_text
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(crop(subject_rect)),
    });
}

/// A thousandth-alpha as the opacity a sprite takes — [`crate::git_graph`]'s own
/// `alpha`, which is private to it.
fn ref_alpha(thousandths: i32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let value = thousandths as f32 / 1000.0;
    value.clamp(0.0, 1.0)
}

// ── the files tree's own badges (R32) ──────────────────────────────────────
//
// `PROBLEM-LIST` P3-6 asked for this and the mock-up never drew it: *the cheap
// reading of "sparse" — a sensible narrow default width plus **inline badges
// (git status / size filling the right-hand whitespace)**, without swapping the
// whole interaction.* So the numbers below are not transcribed from a stylesheet
// the way the rest of this file's are; they are chosen here, and each one says
// why.
//
// **The ruling that shapes all of it is R32**: a badge is shown only while a
// column of this tab is *standing on* its Git page, because that is the only
// state in which the status is being kept true (R31). A tree that kept drawing
// letters after the page was closed would be showing a photograph of a
// repository and calling it the repository. Nothing here ever asks git anything:
// this reads a cache somebody else's open page already paid for, and when there
// is no such page it is empty.

/// The status letter on a tree row: `.gst`'s glyph without `.gst`'s chip.
///
/// The chip is 17 pixels on a 27-pixel row and it is the *subject* of that row.
/// A tree row is 24 pixels and its subject is a file name; seventeen pixels of
/// tinted ground per changed file would turn the tree into the change list it is
/// standing next to. The letter alone carries the whole signal, which is what the
/// four inks are for.
pub const FILES_BADGE_FONT_LOGICAL_PX: f32 = GIT_BADGE_FONT_LOGICAL_PX;
/// How wide one letter's cell is.
///
/// **Reserved and not measured**, for `.gact`'s reason (*appearing must not nudge
/// the row*) and for one more: the tree painter holds no font, so a letter placed
/// by its own advance would need a measurer threaded through two hosts to draw
/// nine known glyphs. Nine logical pixels holds any of them at 10px mono with air
/// on both sides, and two of them side by side stay one object.
pub const FILES_BADGE_CELL_LOGICAL_PX: f32 = 9.0;
/// The air between the name and the first letter.
pub const FILES_BADGE_GAP_LOGICAL_PX: f32 = 6.0;
/// A folder's aggregate mark, across.
///
/// **A dot and not a letter**, because a folder has no status: git says nothing
/// about directories, and what is true of one is only that something under it is
/// changed. A letter would be this page inventing a claim git never made; a dot
/// says the one thing that is so, and the folder can be opened to read the rest.
pub const FILES_BADGE_DOT_LOGICAL_PX: f32 = 5.0;

/// What each row of one files tree wears, keyed by that tree's own row ids.
///
/// **The arithmetic is done once, here.** A column may be rooted anywhere inside
/// its repository, so a status path (`crates/bt-app/src/main.rs`, repo-relative)
/// and a row id (`/src/main.rs`, column-relative) are two spellings of one file.
/// Translating at build time means the painter looks a row up by the id it
/// already has, and the prefix cannot be forgotten by one of the two hosts that
/// draw trees.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GitTreeBadges {
    /// Row id → its letters, for the files git named.
    files: std::collections::BTreeMap<String, Vec<GitBadge>>,
    /// Every ancestor row id of every named file.
    ///
    /// Walked once into a set rather than asked per folder per frame: a
    /// thousand-entry status against thirty visible folder rows is thirty
    /// thousand prefix comparisons a frame, and this is one pass over the status
    /// and a lookup.
    dirs: std::collections::BTreeSet<String>,
}

impl GitTreeBadges {
    /// **R32's gate, and the whole of it.**
    ///
    /// `columns` is every Files column of the tab, each with the page it is
    /// standing on and what it knows about the repository under it. A column
    /// contributes only while it is on its Git page *and* the status has
    /// arrived — the two halves of "the data is already there and is being kept
    /// true". Everything else answers with an empty set, and an empty set draws
    /// nothing.
    ///
    /// When more than one open page covers this tree, the **deepest** repository
    /// wins: a submodule's own page is a truer account of a file inside it than
    /// the outer repository's, which does not track it at all.
    #[must_use]
    pub fn of(columns: &[(crate::seats::FilesView, &GitCache)], tree_root: &Path) -> Self {
        let mut best: Option<(usize, String, &crate::git::GitStatus)> = None;
        for (view, cache) in columns {
            if *view != crate::seats::FilesView::Git {
                continue;
            }
            let (Some(repo), Some(status)) = (cache.root(), cache.status().ready()) else {
                continue;
            };
            let Some(prefix) = repo_prefix(repo, tree_root) else {
                continue;
            };
            let depth = repo.components().count();
            if best.as_ref().is_none_or(|(deepest, ..)| depth > *deepest) {
                best = Some((depth, prefix, status));
            }
        }
        let Some((_, prefix, status)) = best else {
            return Self::default();
        };
        let mut badges = Self::default();
        for entry in &status.entries {
            // A rename's *old* name is not a row: it is gone from the disk, so
            // there is nothing in the tree to hang a letter on. The new name
            // carries the `R`, which is where a reader looking for the file will
            // be looking.
            let Some(key) = row_key(&prefix, &entry.path) else {
                continue;
            };
            let letters = badges_of(entry);
            if letters.is_empty() {
                continue;
            }
            let mut cut = key.as_str();
            while let Some(at) = cut.rfind('/') {
                if at == 0 {
                    break;
                }
                cut = &cut[..at];
                if !badges.dirs.insert(cut.to_owned()) {
                    // This whole spine is already marked, and so is everything
                    // above it.
                    break;
                }
            }
            badges.files.insert(key, letters);
        }
        badges
    }

    /// The letters this file row wears, if git named it.
    #[must_use]
    pub fn letters(&self, key: &str) -> &[GitBadge] {
        self.files.get(key).map_or(&[], Vec::as_slice)
    }

    /// Whether anything under this folder is changed.
    #[must_use]
    pub fn touched(&self, key: &str) -> bool {
        self.dirs.contains(key)
    }

    /// Whether git named nothing in this tree at all.
    ///
    /// Read only by the tests, and kept because "nothing at all" is the state
    /// R32's gate exists to produce — a test that could only ask about rows it
    /// had thought to name would pass on a badge appearing somewhere it had not.
    #[allow(dead_code)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.dirs.is_empty()
    }
}

/// Where `dir` stands inside `repo`, as a repo-relative prefix ending in `/`, or
/// `None` when it does not stand inside it at all.
///
/// Component by component and not `strip_prefix`, because on Windows two
/// spellings of one folder differ in case and `Path`'s own comparison is exact:
/// a column rooted at `d:\repo\src` and a `rev-parse --show-toplevel` that
/// answered `D:\repo` are the same place, and a tree that drew no badges because
/// of a drive letter would be a bug nobody could see the cause of.
fn repo_prefix(repo: &Path, dir: &Path) -> Option<String> {
    let mut steps = dir.components();
    for want in repo.components() {
        let have = steps.next()?;
        if !same_step(want.as_os_str(), have.as_os_str()) {
            return None;
        }
    }
    let mut prefix = String::new();
    for step in steps {
        prefix.push_str(&step.as_os_str().to_string_lossy());
        prefix.push('/');
    }
    Some(prefix)
}

fn same_step(want: &std::ffi::OsStr, have: &std::ffi::OsStr) -> bool {
    if cfg!(windows) {
        want.to_string_lossy()
            .eq_ignore_ascii_case(&have.to_string_lossy())
    } else {
        want == have
    }
}

/// The tree row id a repo-relative path has, seen from a column at `prefix`.
///
/// `None` when the file is somewhere else in the repository — which is most of
/// them when a column is rooted at a subdirectory, and is why this is a filter
/// and not a translation.
fn row_key(prefix: &str, path: &str) -> Option<String> {
    let head = path.get(..prefix.len())?;
    let rest = path.get(prefix.len()..)?;
    let matched = if cfg!(windows) {
        head.eq_ignore_ascii_case(prefix)
    } else {
        head == prefix
    };
    (matched && !rest.is_empty()).then(|| format!("/{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{GitAnswer, GitCommit, GitLog, parse_status};
    use std::path::{Path, PathBuf};

    const ROOT: &str = r"D:\repo";

    /// A measurer that costs one unit per character.
    ///
    /// Not a font, and it does not need to be: every width this module takes is
    /// used to place the *next* thing along, so what a test about layout has to
    /// know is that a longer string pushes further — not what the face's advance
    /// for `M` is. A fake with that one property makes the arithmetic checkable
    /// by hand, which is the point.
    fn ruler(text: &str, size: f32) -> f32 {
        text.chars().count() as f32 * size * 0.5
    }

    /// A cache that has been told everything, through the door every answer
    /// really comes through.
    ///
    /// Built by *accepting answers* rather than by reaching into the struct,
    /// which is deliberate: a fixture assembled by hand can be a shape the real
    /// pipeline never produces, and the first thing these tests are about is the
    /// real pipeline.
    fn answered(porcelain: &[u8], commits: Vec<GitCommit>, has_more: bool) -> GitCache {
        let mut cache = GitCache::default();
        cache.retarget(Path::new(ROOT));
        assert!(cache.accept(GitAnswer::Repo {
            dir: PathBuf::from(ROOT),
            outcome: Ok(PathBuf::from(ROOT)),
        }));
        assert!(cache.accept(GitAnswer::Status {
            root: PathBuf::from(ROOT),
            outcome: Ok(parse_status(porcelain)),
        }));
        assert!(cache.accept(GitAnswer::Log {
            root: PathBuf::from(ROOT),
            skip: 0,
            outcome: Ok(GitLog {
                skip: 0,
                commits,
                has_more,
            }),
        }));
        cache
    }

    fn commit(short: &str, subject: &str, parents: usize) -> GitCommit {
        GitCommit {
            hash: format!("{short}0000000000000000000000000000000000"),
            short: short.to_owned(),
            subject: subject.to_owned(),
            author_name: "Weiyi".to_owned(),
            author_email: "weiyi@example.com".to_owned(),
            committer_name: "Weiyi".to_owned(),
            committer_email: "weiyi@example.com".to_owned(),
            body: String::new(),
            committer_unix: 1_760_000_000,
            committer_offset: 0,
            time_relative: "2h".to_owned(),
            parents: (0..parents).map(|n| format!("parent{n}")).collect(),
            refs: Vec::new(),
        }
    }

    fn rows_of(cache: &GitCache) -> GitPanelContent {
        build(cache, GitPanelLook::default(), 1.0, &mut ruler)
    }

    /// The page with one commit open.
    fn look(expanded: &str) -> GitPanelLook<'_> {
        GitPanelLook {
            expanded: Some(expanded),
            remotes_open: false,
        }
    }

    /// The same page with the REMOTES sub-group unfolded (T9).
    fn rows_with_remotes_open(cache: &GitCache) -> GitPanelContent {
        build(
            cache,
            GitPanelLook {
                expanded: None,
                remotes_open: true,
            },
            1.0,
            &mut ruler,
        )
    }

    /// One ref git would have answered with.
    fn branch(name: &str, is_head: bool, ahead: usize, behind: usize) -> crate::git::GitRefEntry {
        reference(crate::git::GitRefKind::Local, name, is_head, ahead, behind)
    }

    fn reference(
        kind: crate::git::GitRefKind,
        name: &str,
        is_head: bool,
        ahead: usize,
        behind: usize,
    ) -> crate::git::GitRefEntry {
        crate::git::GitRefEntry {
            kind,
            name: name.to_owned(),
            object: "a1".to_owned(),
            upstream: None,
            is_head,
            ahead,
            behind,
            committer_unix: 1_760_000_000,
            committerdate_relative: "2h".to_owned(),
        }
    }

    fn with_branches(mut cache: GitCache, refs: Vec<crate::git::GitRefEntry>) -> GitCache {
        assert!(cache.accept(GitAnswer::Refs {
            root: PathBuf::from(ROOT),
            outcome: Ok(refs),
        }));
        cache
    }

    fn branch_rows(content: &GitPanelContent) -> Vec<GitBranchRow> {
        content
            .rows
            .iter()
            .filter_map(|row| match row {
                GitRow::Branch(branch) => Some(branch.clone()),
                _ => None,
            })
            .collect()
    }

    /// R9 — every local branch is listed, and the one you are standing on leads.
    ///
    /// The order is not a nicety: a repository with sixty branches shows the
    /// first handful without scrolling, and the one fact a reader always needs
    /// from this list is where they are. It is also **not** sorted here — the
    /// sort is `parse_refs`', so this holds the whole pipeline rather than a
    /// re-sort the page could quietly stop doing.
    #[test]
    fn the_branch_list_puts_the_current_branch_first() {
        let cache = with_branches(
            answered(b"", vec![commit("aaaaaaa", "one", 1)], false),
            vec![
                branch("main", true, 0, 0),
                branch("feat/latex", false, 0, 0),
                branch("spike/x", false, 0, 0),
            ],
        );
        let content = rows_of(&cache);
        let rows = branch_rows(&content);
        assert_eq!(rows.len(), 3, "{content:#?}");
        assert_eq!(rows[0].name, "main", "the current branch leads");
        assert!(rows[0].current);
        assert!(rows.iter().skip(1).all(|row| !row.current));
        // And the heading over them carries its count (R7).
        let heading = content.rows.iter().find_map(|row| match row {
            GitRow::Heading { label, count, .. } if *label == GIT_BRANCHES_HEADING => Some(*count),
            _ => None,
        });
        assert_eq!(heading, Some(3));
        // The group leads the page, right after the masthead (G25's order).
        let first = content.rows.iter().position(
            |row| matches!(row, GitRow::Heading { label, .. } if *label == GIT_BRANCHES_HEADING),
        );
        assert_eq!(first, Some(1), "branches come before anything else");

        // R5/G22 again, one list along: a branch's own distance from its own
        // upstream, and zero draws no pill at all.
        let counted = with_branches(
            answered(b"", Vec::new(), false),
            vec![branch("main", true, 2, 0), branch("side", false, 0, 0)],
        );
        let rows = branch_rows(&rows_of(&counted));
        assert_eq!(rows[0].pills.len(), 1, "ahead only");
        assert_eq!(rows[0].pills[0].tooltip, "2 commits ahead");
        assert!(rows[1].pills.is_empty(), "nothing to say draws nothing");
    }

    /// R10 — a branch row is a checkout, and the one you are on is not.
    #[test]
    fn a_branch_row_checks_out_and_the_current_one_does_nothing() {
        let cache = with_branches(
            answered(b"", Vec::new(), false),
            vec![branch("main", true, 0, 0), branch("side", false, 0, 0)],
        );
        let content = rows_of(&cache);
        let rows: Vec<&GitRow> = content
            .rows
            .iter()
            .filter(|row| matches!(row, GitRow::Branch(_)))
            .collect();
        assert_eq!(
            row_document(rows[0], Path::new(ROOT)),
            None,
            "the current one"
        );
        assert_eq!(
            row_document(rows[1], Path::new(ROOT)),
            Some(GitRowOpen::Checkout {
                target: "side".to_owned(),
                detach: false,
            })
        );
    }

    /// T9 (v2 ③) — the remotes stand in a sub-group of their own, folded, under
    /// the locals; and no tag reaches this column at all.
    #[test]
    fn the_remotes_are_a_folded_sub_group_under_the_branches_and_tags_are_not_here() {
        let cache = with_branches(
            answered(b"", Vec::new(), false),
            vec![
                branch("main", true, 0, 0),
                reference(crate::git::GitRefKind::Remote, "origin/main", false, 0, 0),
                reference(crate::git::GitRefKind::Remote, "origin/side", false, 0, 0),
                reference(crate::git::GitRefKind::Tag, "v1.0", false, 0, 0),
            ],
        );
        let shut = rows_of(&cache);
        assert_eq!(
            branch_rows(&shut)
                .iter()
                .map(|row| row.name.clone())
                .collect::<Vec<_>>(),
            vec!["main".to_owned()],
            "folded, the column lists the locals and nothing else"
        );
        let sub = shut
            .rows
            .iter()
            .find_map(|row| match row {
                GitRow::Remotes { count, open } => Some((*count, *open)),
                _ => None,
            })
            .expect("a repository with remotes gets the sub-group row");
        assert_eq!(sub, (2, false), "it counts them and starts shut");
        // And the heading above it counts the locals, not the whole answer.
        assert_eq!(
            shut.rows.iter().find_map(|row| match row {
                GitRow::Heading { label, count, .. } if *label == GIT_BRANCHES_HEADING =>
                    Some(*count),
                _ => None,
            }),
            Some(1)
        );

        let open = rows_with_remotes_open(&cache);
        assert_eq!(
            branch_rows(&open)
                .iter()
                .map(|row| (row.name.clone(), row.remote))
                .collect::<Vec<_>>(),
            vec![
                ("main".to_owned(), false),
                ("origin/main".to_owned(), true),
                ("origin/side".to_owned(), true),
            ],
            "unfolded, the remotes follow the locals and each says it is one"
        );
        // The sub-group row is what a press toggles, and a remote row is not a
        // checkout: that verb is v2 (4)'s.
        let sub_row = open
            .rows
            .iter()
            .find(|row| matches!(row, GitRow::Remotes { .. }))
            .expect("the row is still there when it is open");
        assert!(row_toggles_remotes(sub_row));
        assert_eq!(row_document(sub_row, Path::new(ROOT)), None);
        let remote_row = open
            .rows
            .iter()
            .find(|row| matches!(row, GitRow::Branch(branch) if branch.remote))
            .expect("a remote row is drawn");
        assert!(!row_toggles_remotes(remote_row));
        assert_eq!(
            row_document(remote_row, Path::new(ROOT)),
            None,
            "pressing `origin/main` must not check out a detached HEAD"
        );
        // No tag anywhere on the page: v2 keeps this column compact, and a tag
        // is drawn where it means something, which is on a commit in the graph.
        assert!(
            !branch_rows(&open).iter().any(|row| row.name == "v1.0"),
            "a tag is not a branch row"
        );
    }

    /// T9 — a repository with no remote gets no sub-group row at all.
    #[test]
    fn a_repository_with_no_remote_offers_nothing_to_unfold() {
        let cache = with_branches(
            answered(b"", Vec::new(), false),
            vec![branch("main", true, 0, 0)],
        );
        assert!(
            !rows_of(&cache)
                .rows
                .iter()
                .any(|row| matches!(row, GitRow::Remotes { .. })),
            "`REMOTES (0)` is a control offering to open nothing"
        );
    }

    /// G24/R27 — the masthead carries the door to the full graph.
    #[test]
    fn the_masthead_offers_the_graph() {
        let content = rows_of(&answered(b"", Vec::new(), false));
        let masthead = &content.rows[0];
        assert!(matches!(masthead, GitRow::Masthead(_)));
        let acts: Vec<GitAct> = act_boxes(masthead, [0.0, 0.0, 240.0, 30.0], 1.0)
            .into_iter()
            .map(|(act, _)| act)
            .collect();
        assert_eq!(acts, vec![GitAct::OpenGraph]);
        assert_eq!(press_outcome(GitAct::OpenGraph, false), GitPress::Graph);
    }

    fn change_rows(content: &GitPanelContent) -> Vec<GitChangeRow> {
        content
            .rows
            .iter()
            .filter_map(|row| match row {
                GitRow::Change(change) => Some(change.clone()),
                _ => None,
            })
            .collect()
    }

    /// PIN (R11) — **a file that is both staged and changed gets a row in both
    /// groups, and each row wears both letters.**
    ///
    /// This is the one thing the mock-up's data shape could not express at all:
    /// its rows are `[letter, path]` pairs in two arrays, so `MM` has to be
    /// spelled as either a staged `M` or an unstaged `M` and the other half of
    /// the truth is gone. What git means is that the index holds one version and
    /// the working tree another — staging again would stage something *different*
    /// from what is already staged — and a page that showed one row would be
    /// telling a user their file is in one place when it is in two.
    #[test]
    fn a_file_in_two_groups_is_two_rows_and_each_wears_both_letters() {
        let cache = answered(
            b"## main\0MM both.rs\0 M work.rs\0A  index.rs\0",
            Vec::new(),
            false,
        );
        let content = rows_of(&cache);
        let both: Vec<GitChangeRow> = change_rows(&content)
            .into_iter()
            .filter(|change| change.path == "both.rs")
            .collect();
        assert_eq!(both.len(), 2, "one row in Staged and one in Changes");
        assert_eq!(both[0].group, GitGroup::Staged);
        assert_eq!(both[1].group, GitGroup::Changes);
        for row in &both {
            let letters: Vec<char> = row.badges.iter().map(|badge| badge.letter).collect();
            assert_eq!(
                letters,
                vec!['M', 'M'],
                "both letters, in git's own column order"
            );
        }
        // And the files that really are in one group get one badge, so the pair
        // above is not simply "every row draws two".
        let single: Vec<usize> = change_rows(&content)
            .iter()
            .filter(|change| change.path != "both.rs")
            .map(|change| change.badges.len())
            .collect();
        assert_eq!(single, vec![1, 1]);
    }

    /// PIN (R11/R29) — every letter git can print lands in a group and wears one
    /// of the four claims, and a conflict outranks its own letters.
    #[test]
    fn the_whole_alphabet_is_drawable_and_a_conflict_is_never_green() {
        let cache = answered(
            // A copy and a rename each spend a *second* record on where they came
            // from, exactly as real `git status -z` writes them — which is also
            // what makes this fixture a test of the parser's step, not only of
            // the inks.
            b"## main\0M  m.rs\0T  t.rs\0A  a.rs\0D  d.rs\0C  c.rs\0src.rs\0R  new.rs\0old.rs\0UU u.rs\0?? q.rs\0",
            Vec::new(),
            false,
        );
        let content = rows_of(&cache);
        let inks: Vec<(String, GitBadgeInk)> = change_rows(&content)
            .into_iter()
            .filter_map(|change| Some((change.path.clone(), change.badges.first()?.ink)))
            .collect();
        let ink_of = |path: &str| {
            inks.iter()
                .find(|(name, _)| name == path)
                .map(|(_, ink)| *ink)
                .unwrap_or_else(|| panic!("{path} has no row at all"))
        };
        assert_eq!(ink_of("m.rs"), GitBadgeInk::Changed);
        assert_eq!(ink_of("t.rs"), GitBadgeInk::Changed);
        assert_eq!(ink_of("new.rs"), GitBadgeInk::Changed);
        assert_eq!(ink_of("a.rs"), GitBadgeInk::Added);
        assert_eq!(ink_of("c.rs"), GitBadgeInk::Added);
        assert_eq!(ink_of("d.rs"), GitBadgeInk::Gone);
        assert_eq!(ink_of("q.rs"), GitBadgeInk::Untracked);
        // `UU` is a conflict, and a conflict is red however its letters read —
        // which is the case `AA` and `DD` make sharper still.
        assert_eq!(ink_of("u.rs"), GitBadgeInk::Gone);
        let both_added = answered(b"## main\0AA both.rs\0", Vec::new(), false);
        let ink = change_rows(&rows_of(&both_added))
            .first()
            .and_then(|change| change.badges.first())
            .map(|badge| badge.ink)
            .expect("a conflicted row");
        assert_eq!(
            ink,
            GitBadgeInk::Gone,
            "`AA` is two additions that are a disagreement, and green would be \
             the picture of a bug"
        );
    }

    /// PIN (R7) — a group with nothing in it draws no heading, and every heading
    /// that is drawn carries its number.
    #[test]
    fn an_empty_group_costs_no_heading_and_every_heading_counts() {
        let cache = answered(b"## main\0 M one.rs\0 M two.rs\0", Vec::new(), false);
        let headings: Vec<String> = rows_of(&cache)
            .rows
            .iter()
            .filter_map(|row| match row {
                GitRow::Heading { label, count, .. } => Some(format!("{label} ({count})")),
                _ => None,
            })
            .collect();
        assert_eq!(
            headings,
            vec!["CHANGES (2)".to_owned(), "COMMITS (0)".to_owned()],
            "no STAGED and no UNTRACKED, because there is nothing under either"
        );
    }

    /// PIN (R17, amended by the toast ruling 2026-08-16) — **a repository this
    /// machine cannot read says so where the rows would be, and says it quietly.**
    ///
    /// R17's original claim was that "Not a git repository" is the only *empty*
    /// sentence and every other reason is a red banner over a page. The banner is
    /// gone: it was permanent furniture for a transient fact, and these three
    /// faults were never transient in the first place — a machine with no git
    /// will still have no git in six seconds. So each keeps git's own words, and
    /// each of them now stands in the muted ink in the middle of the page, which
    /// is where a page with nothing to show has always said so.
    ///
    /// What is *not* here is the important half: none of these raises a notice.
    /// The notice is for what just happened, and none of this just happened.
    #[test]
    fn a_repository_that_cannot_be_read_says_so_where_the_rows_would_be() {
        let mut cache = GitCache::default();
        cache.retarget(Path::new(ROOT));
        assert!(cache.accept(GitAnswer::Repo {
            dir: PathBuf::from(ROOT),
            outcome: Err(GitFault::NotARepository),
        }));
        let content = rows_of(&cache);
        assert_eq!(content.empty.as_deref(), Some(GIT_NOT_A_REPOSITORY));
        assert!(content.rows.is_empty(), "and nothing else at all");

        // The other three faults keep git's own words, in the same place.
        for fault in [
            GitFault::GitMissing(crate::git::GIT_NOT_FOUND.to_owned()),
            GitFault::Refused("fatal: detected dubious ownership".to_owned()),
            GitFault::TimedOut,
        ] {
            let mut cache = GitCache::default();
            cache.retarget(Path::new(ROOT));
            assert!(cache.accept(GitAnswer::Repo {
                dir: PathBuf::from(ROOT),
                outcome: Err(fault.clone()),
            }));
            let content = rows_of(&cache);
            assert_ne!(
                content.empty.as_deref(),
                Some(GIT_NOT_A_REPOSITORY),
                "{fault:?} is not the same claim as 'there is no repository here'"
            );
            assert_eq!(
                content.empty.as_deref(),
                Some(fault_sentence(&fault).as_str())
            );
            assert!(content.rows.is_empty(), "and the page is otherwise bare");
        }
    }

    /// PIN (the same ruling) — a *part* of the page git would not read reports
    /// itself in a notice row where that part's rows would have stood, and
    /// everything else on the page is still drawn.
    ///
    /// This is what the banner could never do: it was one strip for the whole
    /// column, so a branch list that failed made the page look as though the
    /// history had too.
    #[test]
    fn a_slot_that_failed_leaves_a_notice_where_its_own_rows_would_be() {
        let words = "fatal: bad object HEAD";
        let mut cache = answered(
            b"## main\0 M work.rs\0",
            vec![commit("aaaaaaa", "first", 1)],
            false,
        );
        assert!(cache.accept(GitAnswer::Refs {
            root: PathBuf::from(ROOT),
            outcome: Err(GitFault::Refused(words.to_owned())),
        }));
        let content = rows_of(&cache);
        assert_eq!(
            content.empty, None,
            "the page is not empty — only that list is"
        );
        let notices: Vec<&str> = content
            .rows
            .iter()
            .filter_map(|row| match row {
                GitRow::Notice(said) => Some(said.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(notices, vec![words]);
        assert!(
            change_rows(&content)
                .iter()
                .any(|row| row.path == "work.rs"),
            "and the changed files are still there"
        );
        assert!(
            content
                .rows
                .iter()
                .any(|row| matches!(row, GitRow::Commit(_))),
            "and so is the history"
        );
    }

    /// PIN (R16) — the "Load more" row is drawn exactly when there is more, and
    /// a second page extends the list rather than replacing it.
    #[test]
    fn load_more_appears_only_when_there_is_more_and_the_next_page_joins_on() {
        let ended = answered(b"## main\0", vec![commit("aaaaaaa", "first", 1)], false);
        assert!(
            !rows_of(&ended)
                .rows
                .iter()
                .any(|row| matches!(row, GitRow::LoadMore)),
            "a history that has ended offers nothing to load"
        );

        let mut cache = answered(b"## main\0", vec![commit("aaaaaaa", "newest", 1)], true);
        let content = rows_of(&cache);
        assert!(
            content
                .rows
                .iter()
                .any(|row| matches!(row, GitRow::LoadMore))
        );
        // The last commit on screen is *not* the end of the road while there is
        // another page, so its lane keeps running off the bottom.
        let last = content
            .rows
            .iter()
            .rev()
            .find_map(|row| match row {
                GitRow::Commit(commit) => Some(commit.clone()),
                _ => None,
            })
            .expect("a commit row");
        assert!(!last.last, "the line has somewhere to go");

        let question = cache.more_commits().expect("there is a next page");
        let crate::git::GitQuestion::Log { skip, .. } = question else {
            panic!("the next page is a log question");
        };
        assert_eq!(skip, 1, "it starts where the list currently ends");
        assert!(cache.accept(GitAnswer::Log {
            root: PathBuf::from(ROOT),
            skip: 1,
            outcome: Ok(GitLog {
                skip: 1,
                commits: vec![commit("bbbbbbb", "older", 1)],
                has_more: false,
            }),
        }));
        let content = rows_of(&cache);
        let subjects: Vec<String> = content
            .rows
            .iter()
            .filter_map(|row| match row {
                GitRow::Commit(commit) => Some(commit.subject.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            subjects,
            vec!["newest".to_owned(), "older".to_owned()],
            "the page joined on rather than replacing what was there"
        );
        assert!(
            !content
                .rows
                .iter()
                .any(|row| matches!(row, GitRow::LoadMore)),
            "and the road has ended"
        );
    }

    /// PIN — **the "Load more" row is a button over its whole body.**
    ///
    /// It has no glyph and no reserved corner: it is a sentence you press. The
    /// hit test offers a row's verbs from [`act_boxes`] and nowhere else, so a
    /// row whose verb this function does not know about is a row that lights up,
    /// says what it does, and does nothing — which is exactly what it did on the
    /// real machine before this test existed.
    #[test]
    fn the_load_more_row_answers_over_its_whole_body() {
        let cache = answered(b"## main\0", vec![commit("aaaaaaa", "newest", 1)], true);
        let content = rows_of(&cache);
        let index = content
            .rows
            .iter()
            .position(|row| matches!(row, GitRow::LoadMore))
            .expect("there is more to load");
        let row = &content.rows[index];
        let rect = [0.0, 100.0, 240.0, 127.0];
        assert_eq!(
            act_boxes(row, rect, 1.0),
            vec![(GitAct::LoadMore, rect)],
            "the box is the row"
        );
        // Every corner of it, not only the middle: a button whose left half was
        // dead would be the same defect in a smaller place.
        for (x, y) in [(2.0, 102.0), (120.0, 113.0), (238.0, 125.0)] {
            assert_eq!(act_at(row, rect, 1.0, x, y), Some(GitAct::LoadMore));
        }
        assert_eq!(
            press_outcome(GitAct::LoadMore, false),
            GitPress::MoreCommits,
            "and pressing it asks for the next page rather than writing anything"
        );
    }

    /// PIN (R14) — **a discard cannot reach git without the gate.**
    ///
    /// The four other verbs go straight through, which is what makes this a claim
    /// about discard rather than about caution in general: staging is undone by
    /// unstaging, and a door in front of it would be a door nobody reads.
    #[test]
    fn only_a_discard_stops_at_the_gate() {
        assert_eq!(
            press_outcome(GitAct::Discard, false),
            GitPress::Gate,
            "throwing a tracked file's work away asks first"
        );
        assert_eq!(
            press_outcome(GitAct::Discard, true),
            GitPress::Gate,
            "and deleting an untracked file asks first too — git has no copy at all"
        );
        assert_eq!(
            press_outcome(GitAct::Stage, false),
            GitPress::Write(GitWriteVerb::Stage)
        );
        assert_eq!(
            press_outcome(GitAct::Unstage, false),
            GitPress::Write(GitWriteVerb::Unstage)
        );
        assert_eq!(
            press_outcome(GitAct::StageAll, false),
            GitPress::Write(GitWriteVerb::Stage)
        );
        assert_eq!(
            press_outcome(GitAct::UnstageAll, false),
            GitPress::Write(GitWriteVerb::Unstage)
        );
        assert_eq!(
            press_outcome(GitAct::LoadMore, false),
            GitPress::MoreCommits
        );
        // And once the gate has been answered, which command runs is decided by
        // what git knows about the file, not by which button was pressed.
        assert_eq!(
            GitAct::Discard.verb(false),
            Some(GitWriteVerb::Discard),
            "a tracked file is restored"
        );
        assert_eq!(
            GitAct::Discard.verb(true),
            Some(GitWriteVerb::DiscardUntracked),
            "an untracked one is deleted"
        );
    }

    /// PIN (R13) — **the row dims and the list does not move until git answers.**
    ///
    /// The mock-up moves the row between two arrays on the click and hopes. What
    /// that shows for the eighty milliseconds a `git add` takes is a staged file;
    /// what it shows when the index was locked is a staged file that is not
    /// staged, with no moment at which anything says otherwise.
    #[test]
    fn a_staged_row_stays_where_it_is_until_git_says_it_moved() {
        let mut cache = answered(b"## main\0 M work.rs\0", Vec::new(), false);
        let before = rows_of(&cache);
        let question = cache
            .begin_write(GitWriteVerb::Stage, vec!["work.rs".to_owned()])
            .expect("the write starts");
        assert!(matches!(question, crate::git::GitQuestion::Write { .. }));

        let during = rows_of(&cache);
        let row = |content: &GitPanelContent| {
            change_rows(content)
                .into_iter()
                .next()
                .expect("the changed file has a row")
        };
        assert_eq!(
            row(&during).group,
            GitGroup::Changes,
            "the file has not moved, because git has not moved it"
        );
        assert!(!row(&before).pending);
        assert!(
            row(&during).pending,
            "but the row says it is being worked on"
        );
        // A pending row offers no verbs: pressing `+` twice must not be two
        // `git add`s racing each other for `index.lock`.
        assert!(act_boxes(&GitRow::Change(row(&during)), [0.0, 0.0, 200.0, 27.0], 1.0).is_empty());
        assert!(
            cache
                .begin_write(GitWriteVerb::Stage, vec!["work.rs".to_owned()])
                .is_none(),
            "and a second press starts nothing"
        );

        // The receipt: the row stops being pending and everything is asked again,
        // because what the status is now is a question for git and not arithmetic.
        assert!(cache.accept(GitAnswer::Write {
            root: PathBuf::from(ROOT),
            verb: GitWriteVerb::Stage,
            paths: vec!["work.rs".to_owned()],
            outcome: Ok(()),
        }));
        assert!(!cache.write_pending("work.rs"));
        assert!(
            matches!(cache.status(), GitSlot::Idle),
            "the status is asked again rather than guessed at"
        );
    }

    /// PIN (R13/W3, amended 2026-08-16) — **a refused write leaves the page
    /// exactly as it was**, and says nothing on it.
    ///
    /// The words used to be a red strip carved off the top of this column. They
    /// are now a notice raised by the runtime from the answer itself
    /// ([`crate::git::write_refusal`]), so what this test pins is the *silence*
    /// here: no empty state, no row moved, no dimming left behind, and — the
    /// point of the whole ruling — no permanent evidence of a thing that has
    /// finished happening.
    #[test]
    fn a_refused_write_leaves_the_page_exactly_as_it_was() {
        let mut cache = answered(
            b"## main\0 M work.rs\0",
            vec![commit("aaaaaaa", "x", 1)],
            false,
        );
        let _ = cache
            .begin_write(GitWriteVerb::Stage, vec!["work.rs".to_owned()])
            .expect("the write starts");
        assert!(cache.accept(GitAnswer::Write {
            root: PathBuf::from(ROOT),
            verb: GitWriteVerb::Stage,
            paths: vec!["work.rs".to_owned()],
            outcome: Err(GitFault::Refused(
                "fatal: Unable to create '.git/index.lock': File exists.".to_owned()
            )),
        }));
        let content = rows_of(&cache);
        assert_eq!(
            content.empty, None,
            "a failed write does not empty the page"
        );
        assert!(
            !content
                .rows
                .iter()
                .any(|row| matches!(row, GitRow::Notice(_))),
            "and it does not leave a sentence lying in the list either"
        );
        assert!(
            change_rows(&content)
                .iter()
                .any(|change| change.path == "work.rs" && !change.pending),
            "and the file is still there, un-dimmed"
        );
        assert!(
            matches!(cache.status(), GitSlot::Ready(_)),
            "nothing is re-read: the status is exactly as true as it was"
        );
        // The words the notice will carry are the answer's, and they are git's.
        assert_eq!(
            crate::git::write_refusal(&GitFault::Refused(
                "fatal: Unable to create '.git/index.lock': File exists.".to_owned()
            )),
            "fatal: Unable to create '.git/index.lock': File exists."
        );
    }

    /// PIN (R7/R5) — the three states of a head each get their own sentence, and
    /// zero draws no pill.
    #[test]
    fn a_head_with_no_branch_says_so_and_a_zero_count_wears_no_pill() {
        let plain = answered(b"## main...origin/main\0", Vec::new(), false);
        let GitRow::Masthead(head) = &rows_of(&plain).rows[0] else {
            panic!("the first row is the masthead");
        };
        assert_eq!(head.branch, "main");
        assert!(head.named);
        assert!(
            head.pills.is_empty(),
            "level with its upstream says nothing"
        );

        let ahead = answered(
            b"## main...origin/main [ahead 1, behind 12]\0",
            Vec::new(),
            false,
        );
        let GitRow::Masthead(head) = &rows_of(&ahead).rows[0] else {
            panic!("the first row is the masthead");
        };
        let said: Vec<(&str, &str)> = head
            .pills
            .iter()
            .map(|pill| (pill.text.as_str(), pill.tooltip.as_str()))
            .collect();
        assert_eq!(
            said,
            vec![("↑ 1", "1 commit ahead"), ("↓ 12", "12 commits behind")],
            "R5: counts, never 'push' or 'pull' — this page offers neither"
        );

        let detached = answered(b"## HEAD (no branch)\0", Vec::new(), false);
        let GitRow::Masthead(head) = &rows_of(&detached).rows[0] else {
            panic!("the first row is the masthead");
        };
        assert_eq!(head.branch, GIT_DETACHED);
        assert!(!head.named, "a state is said quietly, a name is not");

        let unborn = answered(b"## No commits yet on main\0", Vec::new(), false);
        let GitRow::Masthead(head) = &rows_of(&unborn).rows[0] else {
            panic!("the first row is the masthead");
        };
        assert_eq!(head.branch, format!("main — {GIT_UNBORN}"));
    }

    /// PIN (toast ruling, 2026-08-16) — **the page's rows stand in the same
    /// rectangle whatever git last said.**
    ///
    /// The body used to lose twenty-two pixels off its top the moment a verb was
    /// refused, and get them back at the next attempt: every row on the page
    /// moved down and then up again because of something that had already
    /// finished happening. There is nothing left that can carve the viewport, and
    /// this is the assertion that keeps it that way — the viewport *is* the body,
    /// for every content this page can hold.
    #[test]
    fn the_viewport_is_the_whole_body_whatever_the_page_is_saying() {
        let body = [0.0, 0.0, 240.0, 600.0];
        let busy = rows_of(&answered(
            b"## main\0 M work.rs\0",
            vec![commit("aaaaaaa", "x", 1)],
            false,
        ));
        let mut refused = GitCache::default();
        refused.retarget(Path::new(ROOT));
        assert!(refused.accept(GitAnswer::Repo {
            dir: PathBuf::from(ROOT),
            outcome: Err(GitFault::GitMissing("no git.exe here".to_owned())),
        }));
        let bare = rows_of(&refused);
        assert!(bare.empty.is_some(), "and it does have something to say");

        for content in [&busy, &bare, &GitPanelContent::default()] {
            assert_eq!(
                git_panel_geometry(body, content, 1.0).viewport,
                body,
                "no strip is carved off the top: {:?}",
                content.empty
            );
        }
    }

    /// PIN — the page's rectangles are one derivation, and the buttons are inside
    /// the rows the hit test finds.
    ///
    /// The pair this pins is the one every list in this codebase has been bitten
    /// by: a press that lands on the row above the one you can see, or on a
    /// button beside the one you aimed at.
    #[test]
    fn what_is_drawn_is_what_can_be_pressed() {
        let cache = answered(
            b"## main\0 M work.rs\0",
            vec![commit("aaaaaaa", "x", 1)],
            false,
        );
        let content = rows_of(&cache);
        let body = [0.0, 0.0, 240.0, 600.0];
        let geometry = git_panel_geometry(body, &content, 1.0);
        for (index, row) in content.rows.iter().enumerate() {
            let rect = geometry.row_rect(index);
            let centre = ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
            assert_eq!(
                geometry.row_at(centre.0, centre.1),
                Some(index),
                "row {index} answers for its own middle"
            );
            let boxes = act_boxes(row, rect, 1.0);
            if let (GitRow::Change(_), Some((_, outermost))) = (row, boxes.first()) {
                assert_eq!(
                    rect[2] - outermost[2],
                    GIT_ROW_PADDING_X_LOGICAL_PX,
                    "the trailing verb ends exactly at the row's padding"
                );
            }
            for (act, box_) in boxes {
                let inside = ((box_[0] + box_[2]) / 2.0, (box_[1] + box_[3]) / 2.0);
                assert_eq!(
                    act_at(row, rect, 1.0, inside.0, inside.1),
                    Some(act),
                    "the verb answers for its own box"
                );
                assert!(
                    box_[0] >= rect[0] && box_[2] <= rect[2],
                    "and it is inside the row it belongs to"
                );
                // Inside its padding, too (user report, 2026-08-16): a change
                // row's `+` stops where the row's name would — seven pixels
                // short of the edge — and never touches the rounded corner.
                if matches!(row, GitRow::Change(_)) {
                    assert!(
                        rect[2] - box_[2] >= GIT_ROW_PADDING_X_LOGICAL_PX,
                        "the {act:?} box ends {} inside the row, not less than the row's own padding",
                        rect[2] - box_[2]
                    );
                }
            }
        }
        // Nothing answers for a point outside the viewport, which is what stops a
        // row scrolled under the pane head taking a press aimed at the head.
        assert_eq!(geometry.row_at(120.0, -4.0), None);
        assert_eq!(geometry.row_at(120.0, 604.0), None);

        // And the cards stand behind runs of rows rather than behind headings.
        assert!(
            !geometry.cards().is_empty(),
            "the change list and the history each stand on one"
        );
    }

    /// PIN — a page taller than its body can be scrolled, and never past its
    /// ends.
    #[test]
    fn the_page_scrolls_between_its_own_two_ends_and_no_further() {
        let commits: Vec<GitCommit> = (0..40)
            .map(|n| commit(&format!("{n:07}"), "a commit", 1))
            .collect();
        let content = rows_of(&answered(b"## main\0", commits, false));
        let body = [0.0, 0.0, 240.0, 300.0];
        let geometry = git_panel_geometry(body, &content, 1.0);
        assert!(
            geometry.max_scroll > 0.0,
            "forty commits do not fit in 300px"
        );
        assert_eq!(clamp_git_scroll(body, &content, -50.0, 1.0), 0.0);
        assert_eq!(
            clamp_git_scroll(body, &content, geometry.max_scroll + 500.0, 1.0),
            geometry.max_scroll,
            "R2 乙案: the bound is where the writing happens"
        );
        // A body taller than the page has nowhere to scroll at all.
        let roomy = [0.0, 0.0, 240.0, 5_000.0];
        assert_eq!(clamp_git_scroll(roomy, &content, 40.0, 1.0), 0.0);
    }

    /// PIN (R33) — the status cap says what it left out rather than being
    /// silently short.
    #[test]
    fn the_cap_says_what_it_left_out() {
        let mut porcelain = b"## main\0".to_vec();
        for index in 0..(crate::git::GIT_STATUS_CAP + 3) {
            porcelain.extend_from_slice(format!("?? file{index}.txt\0").as_bytes());
        }
        let content = rows_of(&answered(&porcelain, Vec::new(), false));
        let notice = content
            .rows
            .iter()
            .find_map(|row| match row {
                GitRow::Notice(words) => Some(words.clone()),
                _ => None,
            })
            .expect("the cap speaks up");
        assert_eq!(notice, "3 more changed files not shown");
    }

    /// PIN — the mini graph's line runs from the first row to the last and stops
    /// there, and a merge is the only row that curves.
    #[test]
    fn the_lane_is_cut_at_both_ends_and_only_a_merge_curves() {
        let content = rows_of(&answered(
            b"## main\0",
            vec![
                commit("aaaaaaa", "newest", 1),
                commit("bbbbbbb", "a merge", 2),
                commit("ccccccc", "oldest", 0),
            ],
            false,
        ));
        let commits: Vec<GitCommitRow> = content
            .rows
            .iter()
            .filter_map(|row| match row {
                GitRow::Commit(commit) => Some(commit.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(commits.len(), 3);
        assert!(commits[0].first && !commits[0].last);
        assert!(!commits[1].first && !commits[1].last);
        assert!(!commits[2].first && commits[2].last);
        assert_eq!(
            commits.iter().map(|c| c.merge).collect::<Vec<_>>(),
            vec![false, true, false],
            "two parents is what makes a merge, and nothing else does"
        );
    }

    // ── G-3 ────────────────────────────────────────────────────────────────

    /// The porcelain the diff tests are written against: one file packed, one
    /// edited, one git has never seen.
    const THREE_WAYS: &[u8] = b"## main\0M  staged.txt\0 M mod.txt\0?? new.txt\0";

    fn change_named<'rows>(content: &'rows GitPanelContent, path: &str) -> &'rows GitRow {
        content
            .rows
            .iter()
            .find(|row| matches!(row, GitRow::Change(change) if change.path == path))
            .expect("the page draws this file")
    }

    /// **① R25's whole mapping, on the row that carries it.**
    ///
    /// A row in the STAGED group is about the *index*, and the index's diff is
    /// `git diff --cached`; a row in CHANGES is about the working tree. The
    /// mock-up's row had one `data-staged` attribute and one diff behind it, and
    /// which of the two it meant was never written down — this is where it is
    /// written down.
    ///
    /// MUTATION: hard-code `staged: false` in [`row_document`]. A staged file's
    /// row then opens the working tree's diff — which for a file that is *only*
    /// staged is empty, so the page says "no changes" about a file it is drawing
    /// under a heading that counts it.
    #[test]
    fn a_staged_row_opens_the_index_s_diff_and_a_changed_row_the_working_tree_s() {
        let content = rows_of(&answered(THREE_WAYS, Vec::new(), false));
        let root = Path::new(ROOT);

        let staged = row_document(change_named(&content, "staged.txt"), root)
            .expect("a change row opens a document");
        assert_eq!(
            staged,
            GitRowOpen::Document {
                source: PreviewSource::GitDiff {
                    root: PathBuf::from(ROOT),
                    path: "staged.txt".to_owned(),
                    staged: true,
                },
                name: "staged.txt.diff".to_owned(),
                renamed_from: None,
            },
            "a row under STAGED is about the index"
        );

        let changed = row_document(change_named(&content, "mod.txt"), root)
            .expect("a change row opens a document");
        assert_eq!(
            changed,
            GitRowOpen::Document {
                source: PreviewSource::GitDiff {
                    root: PathBuf::from(ROOT),
                    path: "mod.txt".to_owned(),
                    staged: false,
                },
                name: "mod.txt.diff".to_owned(),
                renamed_from: None,
            },
            "a row under CHANGES is about the working tree"
        );

        // The suffix is a *display name* and nothing is judged by it (R24): the
        // buffer that name goes on is a diff because its source says so.
        let GitRowOpen::Document { source, name, .. } =
            row_document(change_named(&content, "new.txt"), root).expect("untracked opens too")
        else {
            panic!("a change row is a document, not an expansion");
        };
        assert!(name.ends_with(".diff"), "the display name wears the suffix");
        assert!(
            matches!(source, PreviewSource::GitDiff { staged: false, .. }),
            "and the buffer is a diff because the *source* says so, not the name"
        );
    }

    /// **③ R15's accordion — one commit open, never two.**
    ///
    /// MUTATION: make [`toggled_expansion`] keep the old hash when a different
    /// one is pressed (a set instead of an option). Two commits' file lists then
    /// stand open in a 240-pixel column, and the history below the first is
    /// pushed off the bottom by a list nobody asked to keep.
    #[test]
    fn only_one_commit_is_ever_open() {
        assert_eq!(
            toggled_expansion(None, "aaa"),
            Some("aaa".to_owned()),
            "a press on a closed commit opens it"
        );
        assert_eq!(
            toggled_expansion(Some("aaa"), "aaa"),
            None,
            "a press on the open one closes it"
        );
        assert_eq!(
            toggled_expansion(Some("aaa"), "bbb"),
            Some("bbb".to_owned()),
            "and a press on another *replaces* it — the accordion, not a set"
        );

        // And the page draws it that way: two commits, one expansion, one list.
        let mut cache = answered(
            b"## main\0",
            vec![
                commit("aaaaaaa", "newest", 1),
                commit("bbbbbbb", "older", 1),
            ],
            false,
        );
        let open = commit("bbbbbbb", "older", 1).hash;
        assert!(cache.begin_commit_files(&open).is_some());
        assert!(cache.accept(GitAnswer::CommitFiles {
            root: PathBuf::from(ROOT),
            hash: open.clone(),
            outcome: Ok(vec![crate::git::GitCommitFile {
                path: "src/main.rs".to_owned(),
                code: StatusCode::Modified,
                renamed_from: None,
                stat: None,
            }]),
        }));
        let content = build(&cache, look(&open), 1.0, &mut ruler);
        let files: Vec<&GitCommitFileRow> = content
            .rows
            .iter()
            .filter_map(|row| match row {
                GitRow::CommitFile(file) => Some(file),
                _ => None,
            })
            .collect();
        assert_eq!(files.len(), 1, "one open commit contributes one file list");
        assert_eq!(files[0].hash, open);
        // Under the commit it belongs to, and not under the one above it.
        let commit_at = |hash: &str| {
            content
                .rows
                .iter()
                .position(|row| matches!(row, GitRow::Commit(c) if c.hash == hash))
                .expect("the commit is drawn")
        };
        let file_at = content
            .rows
            .iter()
            .position(|row| matches!(row, GitRow::CommitFile(_)))
            .expect("the file is drawn");
        assert!(
            commit_at(&open) < file_at,
            "the list hangs below its commit"
        );
        assert!(
            file_at > commit_at(&commit("aaaaaaa", "newest", 1).hash),
            "and nothing hangs below the commit that is shut"
        );
    }

    /// **④ A file row inside an expansion opens that commit's diff of that
    /// file** (R15) — not the working tree's, which is a different document
    /// about a different pair of bytes.
    ///
    /// MUTATION: build a `GitDiff` from the file row instead of a `GitShow`.
    /// Pressing a file inside a commit from last March then shows what *you*
    /// have edited today, under that commit's heading.
    #[test]
    fn a_commit_s_file_row_opens_that_commit_s_own_diff() {
        let hash = commit("aaaaaaa", "newest", 1).hash;
        let mut cache = answered(b"## main\0", vec![commit("aaaaaaa", "newest", 1)], false);
        assert!(cache.begin_commit_files(&hash).is_some());
        assert!(cache.accept(GitAnswer::CommitFiles {
            root: PathBuf::from(ROOT),
            hash: hash.clone(),
            outcome: Ok(vec![crate::git::GitCommitFile {
                path: "src/deep/main.rs".to_owned(),
                code: StatusCode::Modified,
                renamed_from: None,
                stat: None,
            }]),
        }));
        let content = build(&cache, look(&hash), 1.0, &mut ruler);
        let row = content
            .rows
            .iter()
            .find(|row| matches!(row, GitRow::CommitFile(_)))
            .expect("the file is drawn");
        assert_eq!(
            row_document(row, Path::new(ROOT)),
            Some(GitRowOpen::Document {
                source: PreviewSource::GitShow {
                    root: PathBuf::from(ROOT),
                    hash: hash.clone(),
                    path: "src/deep/main.rs".to_owned(),
                },
                // The base name and nothing of the folders above it — the same
                // naming a file row gets, and the same reason: a 240-pixel
                // switcher has room for a name.
                name: "main.rs.diff".to_owned(),
                renamed_from: None,
            })
        );

        // And a press on the commit row itself is the accordion, not a document.
        let commit_row = content
            .rows
            .iter()
            .find(|row| matches!(row, GitRow::Commit(_)))
            .expect("the commit is drawn");
        assert_eq!(
            row_document(commit_row, Path::new(ROOT)),
            Some(GitRowOpen::Expand { hash })
        );
    }

    /// **A renamed file's row carries both of its names** — because the command
    /// behind it needs both to *be* a rename.
    ///
    /// `git diff --cached -- <new path>` on a staged rename prints `new file
    /// mode 100644`: rename detection compares a pair, and a pathspec naming one
    /// half hides the other. Measured against real git 2.52 on a scratch
    /// repository while writing this. So a row wearing an `R` badge would open a
    /// diff claiming the file had just been created — the panel and the diff
    /// disagreeing on one screen, which is the failure the CLI backend was
    /// chosen to prevent.
    ///
    /// MUTATION: drop `renamed_from` from what [`row_document`] hands back (or
    /// from the pathspec [`crate::git::answer_question`] builds). The rename's
    /// diff becomes an addition of the whole file, under an `R`.
    #[test]
    fn a_renamed_row_hands_the_command_both_of_its_names() {
        // `R  new\0old\0` — porcelain spends a second record on where a rename
        // came from, and the **new** path comes first.
        let content = rows_of(&answered(
            b"## main\0R  renamed.txt\0was.txt\0",
            Vec::new(),
            false,
        ));
        assert_eq!(
            row_document(change_named(&content, "renamed.txt"), Path::new(ROOT)),
            Some(GitRowOpen::Document {
                source: PreviewSource::GitDiff {
                    root: PathBuf::from(ROOT),
                    path: "renamed.txt".to_owned(),
                    staged: true,
                },
                name: "renamed.txt.diff".to_owned(),
                renamed_from: Some("was.txt".to_owned()),
            }),
            "the row knows where the file came from, and the open carries it"
        );

        // And the same for a file inside an expanded commit, whose
        // `--name-status` said `R100 old new`.
        let hash = commit("aaaaaaa", "newest", 1).hash;
        let mut cache = answered(b"## main\0", vec![commit("aaaaaaa", "newest", 1)], false);
        assert!(cache.begin_commit_files(&hash).is_some());
        assert!(cache.accept(GitAnswer::CommitFiles {
            root: PathBuf::from(ROOT),
            hash: hash.clone(),
            outcome: Ok(vec![crate::git::GitCommitFile {
                path: "renamed.txt".to_owned(),
                code: StatusCode::Renamed,
                renamed_from: Some("was.txt".to_owned()),
                stat: None,
            }]),
        }));
        let page = build(&cache, look(&hash), 1.0, &mut ruler);
        let row = page
            .rows
            .iter()
            .find(|row| matches!(row, GitRow::CommitFile(_)))
            .expect("the file is drawn");
        assert_eq!(
            row_document(row, Path::new(ROOT)),
            Some(GitRowOpen::Document {
                source: PreviewSource::GitShow {
                    root: PathBuf::from(ROOT),
                    hash,
                    path: "renamed.txt".to_owned(),
                },
                name: "renamed.txt.diff".to_owned(),
                renamed_from: Some("was.txt".to_owned()),
            })
        );
    }

    // ── R21: one column order for both surfaces ────────────────────────────

    /// Everything one call of the painter put on the glass.
    #[derive(Default)]
    struct Painted {
        labels: Vec<ChromeLabel>,
        sprites: Vec<ChromeSprite>,
    }

    /// Draw a page into a 240-pixel column — the width the design gives it.
    fn painted(content: &GitPanelContent, height: f32) -> Painted {
        let mut quads = Vec::new();
        let mut out = Painted::default();
        push_git_panel(
            [0.0, 0.0, 240.0, height],
            content,
            GitHover::default(),
            1.0,
            &bt_render::chrome_palette(),
            (&mut quads, &mut out.labels, &mut out.sprites),
        );
        out
    }

    /// Where a piece of text was drawn: its left and its right edge.
    fn at(painted: &Painted, text: &str) -> (f32, f32) {
        let label = painted
            .labels
            .iter()
            .find(|label| label.text == text)
            .unwrap_or_else(|| panic!("`{text}` was drawn"));
        (label.rect[0], label.rect[2])
    }

    /// **R21, ruled 2026-08-16** — the panel's commit row is the graph's:
    /// graph, refs, message, time, hash.
    ///
    /// The mock-up drew two orders for one row (4886-4889 against 4837-4843) and
    /// this is the one that survived. The assertion is on *pixels* and not on a
    /// list of fields, because a column order is a fact about where things are:
    /// a row that built the four labels in the right sequence and laid them out
    /// in the old one would pass any test that only read the content.
    ///
    /// MUTATION: put the hash back where the mock-up's panel had it — laid from
    /// `rect[0] + pad + column + gap` with the subject after it. The first
    /// assertion fails, because the hash then starts left of the message.
    #[test]
    fn a_commit_row_reads_graph_refs_message_time_hash() {
        let content = rows_of(&answered(
            b"## main\0",
            vec![commit("abc1234", "the newest thing", 1)],
            false,
        ));
        let glass = painted(&content, 600.0);
        let (subject_left, _) = at(&glass, "the newest thing");
        let (time_left, time_right) = at(&glass, "2h");
        let (hash_left, hash_right) = at(&glass, "abc1234");

        assert!(
            subject_left < time_left,
            "the message comes before the time ({subject_left} < {time_left})"
        );
        assert!(
            time_right <= hash_left,
            "the time comes before the hash ({time_right} <= {hash_left})"
        );
        assert!(
            hash_right <= 240.0 - GIT_ROW_PADDING_X_LOGICAL_PX,
            "and the hash ends at the row's own right inset, not past it"
        );
    }

    /// R22 in the panel — the branch standing on a commit wears its pill there
    /// too, between the graph and the message.
    ///
    /// MUTATION: leave `GitCommitRow::refs` empty in `commit_row`. The pill's
    /// text is never drawn and `at` panics.
    #[test]
    fn a_commit_that_carries_a_branch_wears_it_before_the_message() {
        let mut carried = commit("abc1234", "the newest thing", 1);
        carried.refs = vec![
            crate::git::GitRef {
                name: "main".to_owned(),
                head: true,
                kind: crate::git::GitRefKind::Local,
            },
            crate::git::GitRef {
                name: "spike".to_owned(),
                head: false,
                kind: crate::git::GitRefKind::Local,
            },
        ];
        let content = rows_of(&answered(b"## main\0", vec![carried], false));
        let glass = painted(&content, 600.0);
        let (head_left, head_right) = at(&glass, "main");
        let (other_left, _) = at(&glass, "spike");
        let (subject_left, _) = at(&glass, "the newest thing");

        assert!(
            head_right <= other_left,
            "git's own order, left to right ({head_right} <= {other_left})"
        );
        assert!(
            other_left < subject_left,
            "and both stand before the message ({other_left} < {subject_left})"
        );
        assert!(
            head_left > GIT_COMMIT_PADDING_LEFT_LOGICAL_PX + GIT_GRAPH_WIDTH_LOGICAL_PX,
            "the graph column keeps its gutter"
        );
    }

    /// A name too long for what is left of the row is **dropped and not cut** —
    /// half a branch name is a different branch name.
    #[test]
    fn a_pill_with_no_room_is_left_off_rather_than_trimmed() {
        let mut carried = commit("abc1234", "s", 1);
        carried.refs = vec![crate::git::GitRef {
            name: "a-branch-name-far-longer-than-two-hundred-and-forty-pixels".to_owned(),
            head: false,
            kind: crate::git::GitRefKind::Local,
        }];
        let content = rows_of(&answered(b"## main\0", vec![carried], false));
        let glass = painted(&content, 600.0);
        assert!(
            !glass
                .labels
                .iter()
                .any(|label| label.text.starts_with("a-branch-name")),
            "no half a name anywhere on the row"
        );
        // And the row is still a row: the hash it ends with is still there.
        at(&glass, "abc1234");
    }

    // ── R32: the tree's own badges ─────────────────────────────────────────

    /// The tab's columns, as [`GitTreeBadges::of`] wants them.
    fn sources(
        columns: &[(crate::seats::FilesView, GitCache)],
    ) -> Vec<(crate::seats::FilesView, &GitCache)> {
        columns.iter().map(|(view, cache)| (*view, cache)).collect()
    }

    const PORCELAIN: &[u8] =
        b"## main\0 M src/main.rs\0A  src/new.rs\0D  docs/gone.md\0?? junk.tmp\0MM both.rs\0";

    /// **R32's gate.** A repository that nobody is looking at lends its letters
    /// to nobody: the column holding the cache is on its *tree*, so the status in
    /// it is a photograph, and a photograph is not shown as if it were live.
    ///
    /// **The other half of R32 — "and no extra read" — is structural and is held
    /// elsewhere**: this function takes caches by reference and can ask nothing,
    /// and the one place a question is born is pinned by
    /// `a_repository_is_read_only_for_a_column_that_is_showing_it`. A badge
    /// spends what an open page already spent, or it draws nothing.
    ///
    /// MUTATION: drop the `view != Git` guard in `GitTreeBadges::of`. Every
    /// assertion here fails at once, and the tree starts drawing letters from a
    /// status nothing is keeping true.
    #[test]
    fn a_tree_wears_no_badges_while_no_column_stands_on_its_git_page() {
        let columns = [(
            crate::seats::FilesView::Files,
            answered(PORCELAIN, Vec::new(), false),
        )];
        let badges = GitTreeBadges::of(&sources(&columns), Path::new(ROOT));
        assert!(badges.is_empty(), "nothing at all, not merely no letters");
        assert!(badges.letters("/src/main.rs").is_empty());
        assert!(!badges.touched("/src"));
    }

    /// And with a page open, every letter reaches the row it is about — in git's
    /// own alphabet and git's own column order.
    #[test]
    fn every_status_letter_reaches_the_row_it_names() {
        let columns = [(
            crate::seats::FilesView::Git,
            answered(PORCELAIN, Vec::new(), false),
        )];
        let badges = GitTreeBadges::of(&sources(&columns), Path::new(ROOT));
        let letters = |key: &str| -> String {
            badges
                .letters(key)
                .iter()
                .map(|badge| badge.letter)
                .collect()
        };
        assert_eq!(letters("/src/main.rs"), "M");
        assert_eq!(letters("/src/new.rs"), "A");
        assert_eq!(letters("/docs/gone.md"), "D");
        // `??` occupies both of porcelain's columns but is one state, so it
        // wears one letter — the same one the Git page draws for it. Two would
        // be reading git's notation as if it were two claims.
        assert_eq!(letters("/junk.tmp"), "?");
        assert_eq!(letters("/both.rs"), "MM", "index first, then working tree");
        assert_eq!(
            badges.letters("/src/new.rs")[0].ink,
            GitBadgeInk::Added,
            "and it is the page's own ink, not a second table"
        );
    }

    /// **The prefix, which is the whole of the arithmetic.** A column rooted at a
    /// subdirectory sees repo-relative paths it must shorten, and files outside
    /// its root that it must not draw at all.
    ///
    /// MUTATION: use the status path as the row key unchanged. `/main.rs` is
    /// never found, and `/src/main.rs` is claimed for a tree that has no such
    /// row.
    #[test]
    fn a_column_rooted_below_the_repository_shortens_every_path_by_its_own() {
        let columns = [(
            crate::seats::FilesView::Git,
            answered(PORCELAIN, Vec::new(), false),
        )];
        let badges = GitTreeBadges::of(&sources(&columns), &PathBuf::from(ROOT).join("src"));
        assert_eq!(
            badges.letters("/main.rs").len(),
            1,
            "seen from inside `src`"
        );
        assert!(
            badges.letters("/src/main.rs").is_empty(),
            "and never under the name the repository knows it by"
        );
        assert!(
            badges.letters("/junk.tmp").is_empty(),
            "a file outside this column's root is not this column's row"
        );
        assert!(
            !badges.touched("/docs"),
            "nor is a folder outside it a folder of this tree"
        );
    }

    /// A folder says only that something under it changed — one mark, however
    /// deep and however many.
    #[test]
    fn a_folder_is_marked_when_anything_beneath_it_is() {
        let columns = [(
            crate::seats::FilesView::Git,
            answered(b"## main\0 M a/b/c/deep.rs\0", Vec::new(), false),
        )];
        let badges = GitTreeBadges::of(&sources(&columns), Path::new(ROOT));
        assert!(badges.touched("/a"));
        assert!(badges.touched("/a/b"));
        assert!(badges.touched("/a/b/c"));
        assert!(!badges.touched("/a/b/c/deep.rs"), "a file is not a folder");
        assert!(!badges.touched("/other"));
    }

    /// Windows spells one folder two ways and means one folder.
    ///
    /// MUTATION: compare the components with `==`. A column rooted at the
    /// lower-cased drive letter finds no repository above it and the tree goes
    /// bare, which is a bug with no visible cause.
    #[test]
    #[cfg(windows)]
    fn a_drive_letter_in_the_other_case_is_the_same_place() {
        let columns = [(
            crate::seats::FilesView::Git,
            answered(PORCELAIN, Vec::new(), false),
        )];
        let badges = GitTreeBadges::of(&sources(&columns), Path::new(r"d:\REPO\src"));
        assert_eq!(badges.letters("/main.rs").len(), 1);
    }

    /// A tree in another repository entirely — or in none — gets nothing.
    #[test]
    fn a_tree_outside_the_open_repository_is_not_its_business() {
        let columns = [(
            crate::seats::FilesView::Git,
            answered(PORCELAIN, Vec::new(), false),
        )];
        assert!(
            GitTreeBadges::of(&sources(&columns), Path::new(r"D:\elsewhere")).is_empty(),
            "a neighbouring folder is not a subdirectory of this repository"
        );
        assert!(
            GitTreeBadges::of(&sources(&columns), Path::new(r"D:\repository")).is_empty(),
            "and neither is one whose name merely starts the same way"
        );
    }
}
