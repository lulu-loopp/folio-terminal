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
//! **What is deliberately not here yet**, so that a reader does not go looking:
//! the branch list and `checkout` (R9/R10), the ref pills and the full lane graph
//! (R22/R18, G-4), a change row opening its diff and a commit expanding its files
//! (R15/R25, G-3). Each is named at the place it will attach.

use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, ChromeQuad};

use crate::git::{GitCache, GitFault, GitGroup, GitSlot, GitStatusEntry, GitWriteVerb, StatusCode};
use crate::marks::{ChromeMark, ChromeSprite};

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

/// `.gcommit code { font: 10.5px mono }` — the short hash (R21).
pub const GIT_HASH_FONT_LOGICAL_PX: f32 = 10.5;
/// `.gtime { font-size: 10px }`.
pub const GIT_TIME_FONT_LOGICAL_PX: f32 = 10.0;

/// The failure banner: one line, at the top, in the error ink.
pub const GIT_BANNER_HEIGHT_LOGICAL_PX: f32 = 22.0;
pub const GIT_BANNER_FONT_LOGICAL_PX: f32 = 11.0;

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

/// A repository with no commits yet (R7).
///
/// It *has* a branch name — `git status` prints one — but nothing points at it,
/// so every count on the page would be zero for a reason that is not "clean".
pub const GIT_UNBORN: &str = "no commits yet";

/// `HEAD` is on a commit rather than a branch.
pub const GIT_DETACHED: &str = "detached HEAD";

/// The row that asks for the next page of history (R16).
pub const GIT_LOAD_MORE: &str = "Load more";

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
            Self::LoadMore => None,
        }
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

/// One commit, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct GitCommitRow {
    /// git's own abbreviation (R21 puts it at 10.5, before the subject).
    pub short: String,
    pub short_width: f32,
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
    Commit(GitCommitRow),
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
            Self::Change(_) | Self::Commit(_) | Self::LoadMore | Self::Notice(_)
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
            Self::Heading { .. } => ((GIT_LABEL_PADDING_TOP_LOGICAL_PX
                + GIT_LABEL_LINE_LOGICAL_PX
                + GIT_LABEL_PADDING_BOTTOM_LOGICAL_PX)
                * scale)
                .round(),
            Self::Change(_) | Self::Commit(_) | Self::LoadMore | Self::Notice(_) => {
                (GIT_ROW_HEIGHT_LOGICAL_PX * scale).round().max(1.0)
            }
        }
    }
}

/// One frame of the Git page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GitPanelContent {
    pub rows: Vec<GitRow>,
    /// How far the list is scrolled, in physical pixels. Clamped by whoever
    /// wrote it — [`clamp_git_scroll`] — on `FilesTreeContent::scroll_px`'s own
    /// ruling.
    pub scroll_px: f32,
    /// The whole page is one sentence: there is no repository here (R17), or the
    /// answer has not arrived yet. Drawn centred, and the rows are then empty.
    pub empty: Option<&'static str>,
    /// One line at the top, in git's own words, about something that would not
    /// go through (W3/R13). It does **not** replace the list: a `git add` that
    /// failed leaves everything else on the page true.
    pub banner: Option<String>,
}

// ── building one from a cache ──────────────────────────────────────────────

/// How wide a run of text is, at a size — the caller's font, the caller's answer.
///
/// Everything measured on this page is measured through one closure for the
/// reason every other measured caption in this codebase is: only the thing
/// holding the font can say how wide a string is, and a second measurer is a
/// second answer.
pub type Measure<'a> = dyn FnMut(&str, f32) -> f32 + 'a;

/// Turn what a column knows into what it draws.
///
/// **The whole derivation, in one place.** The painter, the hit test and the
/// wheel all read the rows this returns, so a row that is drawn is a row that can
/// be pressed and a row that can be pressed is a row that is there.
#[must_use]
pub fn build(cache: &GitCache, scale: f32, measure: &mut Measure<'_>) -> GitPanelContent {
    let mut content = GitPanelContent::default();

    // The repository probe answers first and answers for everything: until it
    // has, there is no root to ask anything else about.
    match cache.repo() {
        GitSlot::Idle | GitSlot::Pending => {
            content.empty = Some(GIT_READING);
            return content;
        }
        GitSlot::Failed(GitFault::NotARepository) => {
            content.empty = Some(GIT_NOT_A_REPOSITORY);
            return content;
        }
        // A machine with no git, a repository git refuses to read, a question it
        // would not finish: three different sentences, none of them "not a
        // repository", and all three are git's own words on one line.
        GitSlot::Failed(fault) => {
            content.empty = Some(GIT_READING);
            content.banner = Some(fault_sentence(fault));
            return content;
        }
        GitSlot::Ready(_) => {}
    }

    if let Some(words) = cache.write_error() {
        content.banner = Some(words.to_owned());
    }

    let status = match cache.status() {
        GitSlot::Ready(status) => Some(status),
        GitSlot::Failed(fault) => {
            content.banner.get_or_insert_with(|| fault_sentence(fault));
            None
        }
        GitSlot::Idle | GitSlot::Pending => None,
    };

    content
        .rows
        .push(GitRow::Masthead(masthead(status, scale, measure)));

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
                content.rows.push(GitRow::Commit(commit_row(
                    commit,
                    index == 0,
                    index == last && !log.has_more,
                    scale,
                    measure,
                )));
            }
            if log.has_more {
                content.rows.push(GitRow::LoadMore);
            }
        }
        GitSlot::Failed(fault) => {
            content.banner.get_or_insert_with(|| fault_sentence(fault));
        }
        GitSlot::Idle | GitSlot::Pending => {}
    }

    content
}

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
        pills.push(pill('↑', status.ahead, "ahead", pill_font, measure));
    }
    if status.behind > 0 {
        pills.push(pill('↓', status.behind, "behind", pill_font, measure));
    }
    GitHead {
        branch_width: measure(&branch, font),
        branch,
        named,
        pills,
    }
}

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
    // Both letters, in git's own column order: what happened to the index, then
    // what happened to the working tree. A space is an absence and draws nothing.
    let badges: Vec<GitBadge> = [entry.staged, entry.unstaged]
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
        .collect();
    GitChangeRow {
        tooltip: match &entry.renamed_from {
            Some(from) => format!("{} — renamed from {from}", entry.path),
            None => entry.path.clone(),
        },
        pending: cache.write_pending(&entry.path),
        path: entry.path.clone(),
        badges,
        group,
        untracked,
    }
}

fn commit_row(
    commit: &crate::git::GitCommit,
    first: bool,
    last: bool,
    scale: f32,
    measure: &mut Measure<'_>,
) -> GitCommitRow {
    let merge = commit.parents.len() > 1;
    let hash_font = GIT_HASH_FONT_LOGICAL_PX * scale;
    let time_font = GIT_TIME_FONT_LOGICAL_PX * scale;
    GitCommitRow {
        short_width: measure(&commit.short, hash_font),
        time_width: measure(&commit.time_relative, time_font),
        // R16 puts the author in the tooltip and never in the row: a 240-pixel
        // column has room for the message or for who wrote it, and on a machine
        // where almost every commit is yours the message is the one that varies.
        tooltip: if merge {
            format!(
                "Merge commit — another branch's history joins here\n{}\n{}",
                commit.subject, commit.author
            )
        } else {
            format!("{}\n{}", commit.subject, commit.author)
        },
        short: commit.short.clone(),
        subject: commit.subject.clone(),
        time: commit.time_relative.clone(),
        merge,
        first,
        last,
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
    /// The body less the banner — what the rows are laid into and clipped to.
    pub viewport: [f32; 4],
    /// The banner's own strip, when there is one.
    pub banner: Option<[f32; 4]>,
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
/// **The banner is carved off the top and the rows never see it** — a banner that
/// scrolled away with the list would be a sentence about a failure that a user
/// scrolls past and never reads, and this one is the only report a refused write
/// gets.
#[must_use]
pub fn git_panel_geometry(
    body: [f32; 4],
    content: &GitPanelContent,
    scale: f32,
) -> GitPanelGeometry {
    let banner_height = (GIT_BANNER_HEIGHT_LOGICAL_PX * scale).round();
    let (banner, viewport) = match content.banner {
        Some(_) => {
            let bottom = (body[1] + banner_height).min(body[3]);
            (
                Some([body[0], body[1], body[2], bottom]),
                [body[0], bottom, body[2], body[3]],
            )
        }
        None => (None, body),
    };

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
        banner,
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
    let mut edge = rect[2];
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
    let name_right = (rect[0] + mark + gap + head.branch_width).min(rect[2]);
    let mut left = name_right + gap;
    let mut boxes = Vec::with_capacity(head.pills.len());
    for pill in &head.pills {
        let width = pill.text_width + pad * 2.0;
        if left + width > rect[2] {
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
        GitRow::Change(change) => Some(change.tooltip.clone()),
        GitRow::Commit(commit) => Some(commit.tooltip.clone()),
        GitRow::LoadMore => Some(GitAct::LoadMore.tooltip(false).to_owned()),
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

    if let (Some(strip), Some(words)) = (geometry.banner, content.banner.as_ref()) {
        push_banner(strip, words, scale, palette, labels);
    }

    if let Some(sentence) = content.empty {
        labels.push(ChromeLabel {
            text: sentence.to_owned(),
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
                push_masthead(head, rect, scale, palette, (labels, sprites), &crop);
            }
            GitRow::Heading { label, count, .. } => {
                push_heading(label, *count, rect, scale, palette, labels, &crop);
                push_acts(row, rect, hover, hovered, scale, palette, sprites, &crop);
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
            GitRow::Commit(commit) => {
                push_row_ground(rect, hovered, scale, palette, sprites, &crop);
                push_commit(
                    commit,
                    rect,
                    hovered,
                    scale,
                    palette,
                    (quads, labels, sprites),
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

fn push_banner(
    strip: [f32; 4],
    words: &str,
    scale: f32,
    palette: &ChromePalette,
    labels: &mut Vec<ChromeLabel>,
) {
    let pad = (GIT_VIEW_PADDING_X_LOGICAL_PX * scale).round();
    let box_ = inset(strip, pad);
    labels.push(ChromeLabel {
        text: words.to_owned(),
        rect: box_,
        font_size_px: GIT_BANNER_FONT_LOGICAL_PX * scale,
        // The error ink and no fill: a fail-soft banner in this product is a
        // sentence, not a dialog, and a filled strip would read as a state the
        // page is stuck in rather than as the last thing that happened.
        color: palette.status_err,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(box_),
    });
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

fn push_masthead(
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
        color: if head.named {
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
    if !hovered {
        return;
    }
    let glyph = (GIT_ACT_GLYPH_LOGICAL_PX * scale).round().max(1.0);
    let radius = (GIT_ACT_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    for (act, box_) in act_boxes(row, rect, scale) {
        let lit = hover.act == Some(act);
        if lit {
            sprites.push(ChromeSprite::new(
                ChromeMark::ControlPill { radius_px: radius },
                crop(box_),
                palette.git_act_pill,
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
            if lit {
                palette.git_act_glyph_on_pill
            } else {
                palette.git_act_glyph_hover
            },
        );
        mark.opacity = if lit { 1.0 } else { GIT_ACT_REVEAL };
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

    // ── the four columns (R21): graph, hash, subject, time ──
    let hash_left = rect[0] + pad_left + column + gap;
    let hash_rect = [hash_left, rect[1], hash_left + commit.short_width, rect[3]];
    labels.push(ChromeLabel {
        text: commit.short.clone(),
        rect: hash_rect,
        font_size_px: GIT_HASH_FONT_LOGICAL_PX * scale,
        color: muted,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: true,
        clip: Some(crop(hash_rect)),
    });

    let time_left = (rect[2] - pad_right - commit.time_width).max(hash_rect[2]);
    let time_rect = [time_left, rect[1], rect[2] - pad_right, rect[3]];
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

    let subject_left = hash_rect[2] + gap;
    let subject_rect = [
        subject_left,
        rect[1],
        (time_rect[0] - gap).max(subject_left),
        rect[3],
    ];
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
            author: "Weiyi".to_owned(),
            committer_unix: 1_760_000_000,
            committer_offset: 0,
            time_relative: "2h".to_owned(),
            parents: (0..parents).map(|n| format!("parent{n}")).collect(),
        }
    }

    fn rows_of(cache: &GitCache) -> GitPanelContent {
        build(cache, 1.0, &mut ruler)
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

    /// PIN (R17) — **there is one empty state and it is the only one.**
    ///
    /// The mock-up had a second, "No graph for this folder", and it was struck
    /// for saying the same thing in words that sound like a missing feature.
    /// Every other reason a page has nothing to show is a *banner* over a page,
    /// not a sentence instead of one.
    #[test]
    fn not_a_repository_is_the_only_empty_state() {
        let mut cache = GitCache::default();
        cache.retarget(Path::new(ROOT));
        assert!(cache.accept(GitAnswer::Repo {
            dir: PathBuf::from(ROOT),
            outcome: Err(GitFault::NotARepository),
        }));
        let content = rows_of(&cache);
        assert_eq!(content.empty, Some(GIT_NOT_A_REPOSITORY));
        assert!(content.rows.is_empty(), "and nothing else at all");
        assert_eq!(
            content.banner, None,
            "a folder that is not a repository is not a failure"
        );

        // The other three faults keep git's own words and say them in a banner.
        for fault in [
            GitFault::GitMissing("git.exe was not found on this machine".to_owned()),
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
                content.empty,
                Some(GIT_NOT_A_REPOSITORY),
                "{fault:?} is not the same claim as 'there is no repository here'"
            );
            assert_eq!(
                content.banner.as_deref(),
                Some(fault_sentence(&fault).as_str())
            );
        }
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
        let cache = answered(b"## main ", vec![commit("aaaaaaa", "newest", 1)], true);
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

    /// PIN (R13/W3) — a refused write says git's own words, once, over a page
    /// that is otherwise untouched.
    #[test]
    fn a_refused_write_is_one_line_of_gits_own_words_over_a_whole_page() {
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
            content.banner.as_deref(),
            Some("fatal: Unable to create '.git/index.lock': File exists.")
        );
        assert_eq!(
            content.empty, None,
            "a failed write does not empty the page"
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

        // The banner is cleared by the *next attempt*, which is the moment the
        // user has said they know.
        let _ = cache.begin_write(GitWriteVerb::Stage, vec!["work.rs".to_owned()]);
        assert_eq!(cache.write_error(), None);
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
            for (act, box_) in act_boxes(row, rect, 1.0) {
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
}
