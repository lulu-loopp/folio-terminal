//! The full commit graph (G-4) — lanes computed from the real DAG.
//!
//! **This module is the picture and nothing else.** What the repository said is
//! [`crate::git`]'s; where the picture sits is [`crate::preview`]'s; what colour
//! a lane wears is the palette's. What is here is the one thing neither of those
//! can answer: given a page of commits and the parents each of them names, which
//! vertical road does each commit stand on, and which roads pass behind it.
//!
//! The mock-up hand-authored eight rows over two lanes and said so in its own
//! comment (§G G89: "the native line computes lanes from the real DAG"). This is
//! that line.

use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, ChromeQuad};

use crate::git::GitCommit;
use crate::marks::{ChromeMark, ChromeSprite};

// ── The lane algorithm ─────────────────────────────────────────────────────

/// One row of the graph column: which lanes are drawn, and how.
///
/// **Segments and not a lane set**, because the four ways a lane can appear in
/// one row are four different drawings and a set could only say "present". A
/// lane that arrives from above and stops here is half a line; a lane that a
/// merge curves into is a curve; a lane that merely passes is a whole line. The
/// painter reads these four lists and draws exactly what is in them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphRow {
    /// The lane this commit's dot stands in.
    pub dot: usize,
    /// Lanes with a straight segment from the row's top edge to its middle.
    ///
    /// **Contains [`Self::dot`] exactly when the dot's own lane came from above**
    /// — which is the whole of R19. The mock-up's code drew the dot's lane full
    /// height unconditionally and its comment said "unless this row opens or
    /// closes it"; the comment was right and the code was the bug, so what is
    /// implemented is the comment.
    pub upper: Vec<usize>,
    /// Lanes with a straight segment from the row's middle to its bottom edge.
    ///
    /// Contains [`Self::dot`] exactly when a parent carries the lane onward —
    /// the other half of R19.
    pub lower: Vec<usize>,
    /// Lanes that curve from their own top edge into this row's dot: the other
    /// heads of a merge, arriving.
    pub close: Vec<usize>,
    /// Lanes that curve out of this row's dot to their own bottom edge: this
    /// commit's second and later parents, leaving.
    pub open: Vec<usize>,
    /// How many lanes this row needs room for — one past the rightmost lane it
    /// touches, at either edge.
    pub width: usize,
}

/// Walks a history and lays its lanes out, one page at a time.
///
/// **Incremental by construction.** A page of fifty arriving on top of nine
/// hundred already laid out must not re-walk the nine hundred: lanes are a
/// running state, so the walk resumes where it stopped. That is also what makes
/// R23's virtual window honest — the layout is O(commits) *once per page*, and
/// the build that measures text is O(visible) *per frame*.
#[derive(Clone, Debug, Default)]
pub struct LaneWalker {
    /// Which commit each lane is waiting for. `None` is a free lane — kept
    /// rather than compacted so that a lane a commit is standing in does not
    /// slide sideways under the reader when an unrelated branch two lanes over
    /// ends.
    lanes: Vec<Option<String>>,
    rows: Vec<GraphRow>,
    /// The **Uncommitted Changes** row's lanes (V5), when the working tree is
    /// dirty — a synthetic tip standing above the whole history whose one parent
    /// is `HEAD`.
    ///
    /// Kept beside [`Self::rows`] rather than in front of them so that a row
    /// index is still a *commit* index: the alternative — pushing it in at zero —
    /// would shift every commit's lanes by one and make every `lanes.get(at)` in
    /// this module a subtraction that some caller would eventually forget.
    head: Option<GraphRow>,
}

impl LaneWalker {
    #[must_use]
    pub fn rows(&self) -> &[GraphRow] {
        &self.rows
    }

    /// The synthetic row above the history, when there is one (V5).
    #[must_use]
    pub fn head_row(&self) -> Option<&GraphRow> {
        self.head.as_ref()
    }

    /// Open the walk with the **Uncommitted Changes** row (V5).
    ///
    /// **Walked and not painted.** The mock-up's own instinct here would be to
    /// draw a hollow dot at lane zero and a line down to the row below it, and
    /// that drawing would be a lie the first time a repository's newest commit
    /// did not stand in lane zero. What this does instead is hand the walker a
    /// commit that does not exist — no hash, one parent, and that parent is
    /// `HEAD` — and let `step` answer: the dot lands in the free lane it would
    /// have landed in, the parent claims that lane, and the very next row this
    /// walker lays out therefore finds a lane already waiting for it and draws
    /// its own line *arriving from above*. The road between the two is real
    /// because both of its ends were computed by the same algorithm.
    ///
    /// Must be called on a fresh walker, before the first [`Self::extend`] —
    /// which is what the caller's reset-on-change already guarantees, because
    /// whether this row exists at all is a fact about the whole picture and not
    /// about one row of it.
    pub fn open_with_uncommitted(&mut self, head: &str) {
        debug_assert!(
            self.rows.is_empty() && self.head.is_none(),
            "the uncommitted row stands above the history, so it is laid out first"
        );
        let parents = [head.to_owned()];
        self.head = Some(self.step("", &parents));
    }

    /// Lay out every commit this walker has not seen yet.
    ///
    /// `commits` is the whole list, newest first, and the walk starts at the
    /// first row it has no answer for. Handing the whole list rather than the
    /// tail is deliberate: the caller holds one growing `Vec` and would
    /// otherwise have to remember where it had got to, which is the same number
    /// this walker already knows.
    pub fn extend(&mut self, commits: &[GitCommit]) {
        for commit in commits.iter().skip(self.rows.len()) {
            let row = self.step(&commit.hash, &commit.parents);
            self.rows.push(row);
        }
    }

    /// Forget everything — a history that was rewritten under us is not a
    /// history this walker can resume.
    pub fn reset(&mut self) {
        self.lanes.clear();
        self.rows.clear();
        self.head = None;
    }

    /// One row, from a hash and the parents it names.
    ///
    /// Takes the two fields rather than a [`GitCommit`] because the synthetic
    /// head of [`Self::open_with_uncommitted`] is not a commit and never was:
    /// asking it for a subject, an author and a time would be inventing three
    /// facts to satisfy a signature.
    fn step(&mut self, hash: &str, parents: &[String]) -> GraphRow {
        let before: Vec<bool> = self.lanes.iter().map(Option::is_some).collect();

        // Every lane already waiting for this commit. The first is where the dot
        // stands; the rest are other branches whose next step is this same
        // commit, and they curve in.
        //
        // The empty hash of the synthetic head claims nothing, because no lane is
        // ever waiting for a commit git did not name — which is exactly right: a
        // tip is what it is.
        let claims: Vec<usize> = self
            .lanes
            .iter()
            .enumerate()
            .filter(|(_, lane)| !hash.is_empty() && lane.as_deref() == Some(hash))
            .map(|(index, _)| index)
            .collect();

        let (dot, close) = match claims.split_first() {
            Some((first, rest)) => (*first, rest.to_vec()),
            // Nothing above expects this commit: it is a tip, and its lane opens
            // here.
            None => (self.free_lane(), Vec::new()),
        };
        for lane in &close {
            self.lanes[*lane] = None;
        }

        let mut open = Vec::new();
        match parents.split_first() {
            Some((first, rest)) => {
                self.lanes[dot] = Some(first.clone());
                for parent in rest {
                    // A parent some other lane is already waiting for does not
                    // get a lane of its own — the merge edge joins the road that
                    // is already there, which is what `git log --graph` draws.
                    let lane = self
                        .lanes
                        .iter()
                        .position(|lane| lane.as_deref() == Some(parent.as_str()))
                        .unwrap_or_else(|| {
                            let lane = self.free_lane();
                            self.lanes[lane] = Some(parent.clone());
                            lane
                        });
                    open.push(lane);
                }
            }
            // A root commit: the road ends here.
            None => self.lanes[dot] = None,
        }

        let after: Vec<bool> = self.lanes.iter().map(Option::is_some).collect();
        // Padded to the row's full width, because a lane this row *opened* has
        // no entry in the state that existed before it — and "there was nothing
        // here" is an answer this row has to be able to give about every lane it
        // draws, not only about the ones that already existed.
        let mut before = before;
        before.resize(after.len(), false);
        let width = self
            .lanes
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |l| l + 1);
        let width = width.max(before.iter().rposition(|on| *on).map_or(0, |l| l + 1));
        let width = width.max(dot + 1);

        let upper: Vec<usize> = (0..after.len())
            .filter(|lane| before[*lane] && !close.contains(lane))
            .collect();
        let lower: Vec<usize> = (0..after.len()).filter(|lane| after[*lane]).collect();

        GraphRow {
            dot,
            upper,
            lower,
            close,
            open,
            width,
        }
    }

    /// The leftmost lane nobody is using, making a new one if they all are.
    fn free_lane(&mut self) -> usize {
        match self.lanes.iter().position(Option::is_none) {
            Some(lane) => lane,
            None => {
                self.lanes.push(None);
                self.lanes.len() - 1
            }
        }
    }
}

// ── The colour wheel (R18) ─────────────────────────────────────────────────

/// How many colours the lane wheel has.
///
/// **Eight, and the ruling is a floor rather than a taste** (R18): the mock-up
/// declared three and indexed straight into them, so a fourth lane painted
/// itself `undefined`. A repository with nine concurrent branches on screen at
/// once exists, and what it gets here is a repeated colour — two roads that look
/// alike — which is a legible picture. What it must never get is a road with no
/// colour at all.
pub const GRAPH_LANE_COLOURS: usize = 8;

/// Which colour of the wheel a lane wears.
///
/// The whole of R18's "no out-of-range colour": there is no lane index this
/// cannot answer for, because the answer is arithmetic and not a lookup.
#[must_use]
pub fn lane_colour_index(lane: usize) -> usize {
    lane % GRAPH_LANE_COLOURS
}

// ── The document: what one frame of the graph is ───────────────────────────

/// `const LANE_W = 16` (G68) — one road's width.
pub const GRAPH_LANE_WIDTH_LOGICAL_PX: f32 = 16.0;
/// `const GROW_H = 30` / `.ggrow { height:30px }` (G68/G82).
pub const GRAPH_ROW_HEIGHT_LOGICAL_PX: f32 = 30.0;
/// `.ggcell line, .ggcell path { stroke-width:1.7 }` (G78).
pub const GRAPH_STROKE_LOGICAL_PX: f32 = 1.7;
/// `<circle r="3.6"/>` (G75) — bigger than the panel's 3.1, because this graph
/// has the room the column did not.
pub const GRAPH_DOT_RADIUS_LOGICAL_PX: f32 = 3.6;
/// `.ggcell line, .ggcell path { opacity:.55 }` (G78), premixed over the row's
/// own ground: the lines lie behind the dots, and a road that competed with its
/// own commits for attention would be a map with no towns on it.
pub const GRAPH_LINE_ALPHA: i32 = 550;
/// `.ggv { padding: 6px 10px 14px }` (G79).
pub const GRAPH_PADDING_X_LOGICAL_PX: f32 = 10.0;
pub const GRAPH_PADDING_TOP_LOGICAL_PX: f32 = 6.0;
pub const GRAPH_PADDING_BOTTOM_LOGICAL_PX: f32 = 14.0;
/// `.ggrow { gap:9px; padding:0 8px; border-radius:7px }` (G82).
pub const GRAPH_ROW_GAP_LOGICAL_PX: f32 = 9.0;
pub const GRAPH_ROW_PADDING_X_LOGICAL_PX: f32 = 8.0;
pub const GRAPH_ROW_RADIUS_LOGICAL_PX: f32 = 7.0;
/// `.ggv-head { padding:10px 6px; font-size:14px }` (G80) — **at 13.5**, which
/// is R20: the mock-up gave the same fact two sizes half a pixel apart in two
/// places, and one of them had to go. The panel's masthead is the one this
/// product had already built, so the graph's head is the one that moved.
pub const GRAPH_HEAD_PADDING_X_LOGICAL_PX: f32 = 6.0;
pub const GRAPH_HEAD_PADDING_Y_LOGICAL_PX: f32 = 10.0;
/// `.gref` (G84): the ref pill worn on a commit row.
pub const GRAPH_REF_FONT_LOGICAL_PX: f32 = 10.5;
pub const GRAPH_REF_RADIUS_LOGICAL_PX: f32 = 9.0;
pub const GRAPH_REF_PADDING_X_LOGICAL_PX: f32 = 8.0;
pub const GRAPH_REF_HEIGHT_LOGICAL_PX: f32 = 16.0;
pub const GRAPH_REF_EDGE_LOGICAL_PX: f32 = 1.0;
/// `border: 1px solid color-mix(--lane 45%)` and `background: color-mix(--lane
/// 10%)` (G84) — premixed here as this product premixes every `color-mix`.
pub const GRAPH_REF_EDGE_ALPHA: i32 = 450;
pub const GRAPH_REF_GROUND_ALPHA: i32 = 100;
/// `.ggf { padding-left:56px }` (G87) — the expanded commit's files, indented
/// past the graph column.
pub const GRAPH_FILE_INDENT_LOGICAL_PX: f32 = 56.0;

// ── the open row's own story (v2 ②, D1/D2/D6/D7 — 2026-08-16) ──────────────

/// The body prose, at the row text's own size less a step.
///
/// **12 and not the row's 12.5**: a commit's body is the same *kind* of writing
/// the subject is and wears the same ink, so it cannot be a step quieter without
/// reading as a caption; a hair smaller is what says "this is the continuation
/// and that was the headline".
pub const GRAPH_BODY_FONT_LOGICAL_PX: f32 = 12.0;
/// The leading prose is set on — [`crate::tooltip`]'s own `font * 1.4`, which is
/// the line box every multi-line piece of chrome text in this product is shaped
/// into. A third number invented here would be a guess at what Segoe reports.
pub const GRAPH_BODY_LINE_LOGICAL_PX: f32 = GRAPH_BODY_FONT_LOGICAL_PX * 1.4;
/// How many lines of body the block will draw before it stops (D1).
///
/// **Twelve, and the twelfth says it stopped.** A commit message is allowed to
/// be a essay and some of them are; a graph row that unfolded into three
/// screens of prose would have turned the list into a document. Twelve is about
/// a paragraph and a half — enough that a normal "why" is whole — and the full
/// text is one copy verb away on the same line (D7), which is the honest exit:
/// this surface is a list, and the thing that reads whole documents is the
/// document surface next to it.
pub const GRAPH_BODY_MAX_LINES: usize = 12;
/// What the last line of a body that did not fit ends with.
pub const GRAPH_BODY_ELLIPSIS: &str = "\u{2026}";
/// The meta line — author, date, parents (D2) — at the hash's own size, because
/// it is the same kind of sentence: facts about the commit rather than the
/// commit's own words.
pub const GRAPH_META_FONT_LOGICAL_PX: f32 = crate::git_panel::GIT_HASH_FONT_LOGICAL_PX;
/// The block's own breathing room, top and bottom.
pub const GRAPH_DETAIL_PADDING_Y_LOGICAL_PX: f32 = 6.0;
/// Between the last line of prose and the meta line under it.
pub const GRAPH_DETAIL_GAP_LOGICAL_PX: f32 = 6.0;
/// The gap between two parent hashes on the meta line.
pub const GRAPH_PARENT_GAP_LOGICAL_PX: f32 = 8.0;
/// What the meta line says before the parents.
pub const GRAPH_META_PARENTS: &str = "parents: ";
/// What it says before a committer who is not the author.
pub const GRAPH_META_COMMITTED_BY: &str = "committed by ";
/// The `\u{b7}` every clause of the meta line is joined with — the same
/// separator [`crate::preview::PreviewSource::composed_lead`] uses, because it
/// is the same job: two facts about one thing on one line.
pub const GRAPH_META_SEPARATOR: &str = " \u{b7} ";
/// `Comparing abc1234 \u{2192} def5678` (D6).
pub const GRAPH_COMPARE_LEAD: &str = "Comparing ";
pub const GRAPH_COMPARE_ARROW: &str = " \u{2192} ";
/// What the newer end of a comparison is called when it is the working tree.
pub const GRAPH_COMPARE_WORKING_TREE: &str = "working tree";
/// How far a seek will page before it gives up (D2).
///
/// **Twenty pages is a thousand commits**, which is further back than any parent
/// of a commit on screen has ever been in a history laid out in topological
/// order — and it is also a bound, which is the point: a parent git rewrote out
/// from under us, or one on a branch this log was never asked about, would
/// otherwise page to the end of the repository one subprocess at a time while
/// the reader watched.
pub const GRAPH_SEEK_MAX_PAGES: usize = 20;

// ── the table's columns (v2 ①, V1/V2 — 2026-08-16) ─────────────────────────
//
// **Reserved widths and not measured ones**, which is the whole difference
// between a table and five labels that happen to line up. The row painter used
// to right-align the hash and the age at whatever width the font gave *that*
// string, so `3 days ago` and `now` started in two different places and no
// column header could have pointed at either of them. A reserved width is one
// number for the page: the header stands over its column, the column stands
// under its header, and a date that outgrows its box is ellipsised rather than
// allowed to move the box.
//
// They are also what makes the collapse below expressible. A measured column
// has no width until it is measured, so "is there room for it" could only be
// asked after building the very row the answer decides the shape of.

/// The author column (V1/V4) — and therefore what a name is ellipsised to.
///
/// 120 logical pixels is about sixteen characters at this size: enough for
/// `Weiyi Shi` or `dependabot[bot]` whole, and enough of `A Very Long Name…`
/// to tell two colleagues apart, which is the only question this column is
/// ever asked.
pub const GRAPH_AUTHOR_COLUMN_LOGICAL_PX: f32 = 120.0;
/// `.ggauthor { font-size: 11px }` — a step below the message and a step above
/// the age, because a name is neither what you are reading nor furniture.
pub const GRAPH_AUTHOR_FONT_LOGICAL_PX: f32 = 11.0;
/// The date column — `11 months ago` is the longest sentence R8's table makes.
pub const GRAPH_DATE_COLUMN_LOGICAL_PX: f32 = 84.0;
/// The hash column. git abbreviates to seven characters in every repository
/// small enough to be ambiguous at six and grows from there; ten fit here.
pub const GRAPH_HASH_COLUMN_LOGICAL_PX: f32 = 66.0;

// The three widths at which a column leaves, in the order the ruling gives:
// **author first, then date, then hash** (V1), and the description never. The
// order is not arbitrary — it is least-asked-for first. A reader scanning a
// narrow graph is reading messages; the author is the column they would give up
// first and the message is the one they would never give up, which is why the
// description is what every collapse hands its pixels to.
//
// Measured against the seat's own body width, in logical pixels, so a column
// leaves at the same apparent width on every display.

/// Below this body width the author column is not drawn (V1).
///
/// The graph column, the description and the three right-hand columns want
/// about 500 logical pixels between them before the message is down to a few
/// words; this is that, with the gaps counted.
pub const GRAPH_AUTHOR_MIN_BODY_LOGICAL_PX: f32 = 520.0;
/// Below this, the date goes too.
pub const GRAPH_DATE_MIN_BODY_LOGICAL_PX: f32 = 380.0;
/// And below this, the hash — the last column to leave, because a short hash is
/// the one thing on the row you cannot get anywhere else on the page.
pub const GRAPH_HASH_MIN_BODY_LOGICAL_PX: f32 = 260.0;

/// The column header row's height (V2).
///
/// `.glabel`'s own line and its bottom padding, without its 14px of top padding:
/// that padding is the gap *between two sections* of the Git page, and there is
/// nothing above this row but the masthead's own.
pub const GRAPH_HEADER_HEIGHT_LOGICAL_PX: f32 = crate::git_panel::GIT_LABEL_LINE_LOGICAL_PX
    + crate::git_panel::GIT_LABEL_PADDING_BOTTOM_LOGICAL_PX
    + 4.0;

/// The header's five words, in `.glabel` grammar (V2).
pub const GRAPH_HEADING_GRAPH: &str = "GRAPH";
/// The narrowest graph column that gets the word GRAPH over it — five tracked
/// capitals at 9.5 px are about forty pixels, and a column of three lanes is
/// the first that clears it. Below this the column has no heading (see
/// `push_column_header`).
pub const GRAPH_HEADING_GRAPH_MIN_LOGICAL_PX: f32 = 44.0;
pub const GRAPH_HEADING_DESCRIPTION: &str = "DESCRIPTION";
pub const GRAPH_HEADING_AUTHOR: &str = "AUTHOR";
pub const GRAPH_HEADING_DATE: &str = "DATE";
pub const GRAPH_HEADING_COMMIT: &str = "COMMIT";

/// What the **Uncommitted Changes** row answers to where a commit would give a
/// hash (V5).
///
/// A hash git could never write — every real one is hex — so the one expansion
/// this page allows can be keyed on it without a second field saying which kind
/// of row is open. It is also literally what the row draws in its hash column,
/// which is not a coincidence: the sentinel and the picture are the same claim,
/// that this row is not a commit.
pub const GRAPH_UNCOMMITTED_HASH: &str = "*";
/// What the row says, before its count.
pub const GRAPH_UNCOMMITTED: &str = "Uncommitted Changes";
/// What stands in its date column: the working tree is now, by definition.
pub const GRAPH_UNCOMMITTED_TIME: &str = "now";
/// `<circle fill="none"/>` — the working tree is not a commit, and a hollow dot
/// is how a graph has always said so.
pub const GRAPH_UNCOMMITTED_DOT_STROKE_LOGICAL_PX: f32 = 1.7;

/// How many rows above and below the window are built anyway (R23).
///
/// **Two, and the number is a claim about the wheel rather than about taste.**
/// A notch moves the list by a fraction of a row and the frame that follows is
/// built from the new offset, so the only rows that can appear before a build
/// are the ones a single frame's motion uncovers. Two is that, doubled. Larger
/// buys nothing a reader could see; smaller is a strip of empty pixels at the
/// edge of a fast scroll.
pub const GRAPH_WINDOW_BUFFER_ROWS: usize = 2;

/// How near the bottom the list has to be before the next page is asked for.
///
/// Measured in rows and not in pixels: a page is fifty commits, so asking when
/// the reader is a screenful from the end is asking about as often as they
/// arrive. Asking *at* the end would show them the end.
pub const GRAPH_PREFETCH_ROWS: usize = 20;

/// What the graph's head says: the branch, and how far from its upstream.
///
/// The panel's own [`crate::git_panel::GitHead`], because it is the same
/// sentence about the same repository — R20 unified the two sizes, and sharing
/// the type is what stops them separating again.
pub type GraphHead = crate::git_panel::GitHead;

/// Which of the table's right-hand columns this seat is wide enough for (V1).
///
/// A value carried on the frame rather than re-derived by each painter, for the
/// reason [`crate::git_panel::head_of`] is one reader: a header that decided for
/// itself whether the author column exists is a header that can disagree with
/// the rows under it, and the disagreement would only show at exactly the width
/// where one of them was wrong.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphColumns {
    pub author: bool,
    pub date: bool,
    pub hash: bool,
}

impl Default for GraphColumns {
    /// Everything, which is what a body wide enough for it gets.
    fn default() -> Self {
        Self {
            author: true,
            date: true,
            hash: true,
        }
    }
}

impl GraphColumns {
    /// Which columns a body this wide draws (V1's collapse order).
    ///
    /// `width` is the seat body's own, in **logical** pixels — the same number
    /// on every display, so a column does not leave a 4K screen sooner than a
    /// 1080p one showing the same pane.
    #[must_use]
    pub fn at_width(width: f32) -> Self {
        Self {
            author: width >= GRAPH_AUTHOR_MIN_BODY_LOGICAL_PX,
            date: width >= GRAPH_DATE_MIN_BODY_LOGICAL_PX,
            hash: width >= GRAPH_HASH_MIN_BODY_LOGICAL_PX,
        }
    }
}

/// Where each of a row's right-hand columns stands, and what is left over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphColumnRects {
    /// `None` when the column is collapsed at this width.
    pub author: Option<[f32; 4]>,
    pub date: Option<[f32; 4]>,
    pub hash: Option<[f32; 4]>,
    /// The right edge the description — pills and message — may reach.
    pub description_right: f32,
}

/// Lay the right-hand columns out inside one row's rectangle.
///
/// **One function for the header and for every row** (V2). The header is the
/// same five boxes drawn one row higher, so it is laid out by the same call
/// rather than by a second arithmetic that agrees today — which is what makes
/// "the header is over its column" a property this module can be asked about
/// instead of a screenshot somebody has to look at.
///
/// Right to left, because that is the order the columns are pinned in: the hash
/// is against the row's right edge, and everything else is measured back from
/// it. A collapsed column takes its gap with it — the columns beside it close
/// up rather than leaving the hole where it was.
#[must_use]
pub fn graph_column_rects(rect: [f32; 4], columns: GraphColumns, scale: f32) -> GraphColumnRects {
    let pad = (GRAPH_ROW_PADDING_X_LOGICAL_PX * scale).round();
    let gap = (GRAPH_ROW_GAP_LOGICAL_PX * scale).round();
    let mut right = rect[2] - pad;
    let mut column = |width_logical: f32, present: bool| -> Option<[f32; 4]> {
        if !present {
            return None;
        }
        let width = (width_logical * scale).round();
        let left = (right - width).max(rect[0]);
        let box_ = [left, rect[1], right, rect[3]];
        right = left - gap;
        Some(box_)
    };
    let hash = column(GRAPH_HASH_COLUMN_LOGICAL_PX, columns.hash);
    let date = column(GRAPH_DATE_COLUMN_LOGICAL_PX, columns.date);
    let author = column(GRAPH_AUTHOR_COLUMN_LOGICAL_PX, columns.author);
    GraphColumnRects {
        author,
        date,
        hash,
        description_right: right.max(rect[0]),
    }
}

/// One ref pill (R22).
#[derive(Clone, Debug, PartialEq)]
pub struct GraphRefPill {
    pub name: String,
    pub text_width: f32,
    /// Whether `HEAD` is this ref — the one that wears the accent ring.
    pub head: bool,
    /// Which lane's colour it borrows: the commit's own (G85 carries the lane
    /// on the ref, and the commit's lane is the honest source for it).
    pub lane: usize,
}

/// One drawn row of the graph.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphViewRow {
    /// The **Uncommitted Changes** row (V5) — the working tree, standing where
    /// the commit it is going to become would stand.
    Uncommitted(GraphUncommittedRow),
    Commit(GraphCommitRow),
    /// A file the open commit touched (R15's accordion, in the graph's seat), or
    /// a file the working tree has something to say about (V5).
    File(GraphFileRow),
    /// **The open row's own story** (v2 ②) — the block that stands between a
    /// commit and its files, holding the rest of its message and the facts about
    /// it; or, in compare mode, the head of the comparison instead.
    ///
    /// One `GraphViewRow` spanning several list rows and not one row per line,
    /// which is the whole of how a variable-height thing lives in a list whose
    /// arithmetic is a multiplication: the block claims a *whole number of rows*
    /// (see [`GraphDetailRow::rows`]) and everything the geometry knows —
    /// `row_rect`, `row_at`, `window`, `reveal`, the clamp — goes on being one
    /// height times one index.
    Detail(GraphDetailRow),
}

/// One commit, laid out and measured.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphCommitRow {
    /// Where this row sits in the whole list — what a press is answered with,
    /// and what the painter offsets by.
    pub index: usize,
    pub hash: String,
    pub short: String,
    pub subject: String,
    /// Who wrote it, already cut to [`GRAPH_AUTHOR_COLUMN_LOGICAL_PX`] (V1).
    ///
    /// Ellipsised **here** and not by the painter, because the cut needs the
    /// font and the build is the one place in this module that holds it.
    pub author: String,
    pub time: String,
    pub tooltip: String,
    pub refs: Vec<GraphRefPill>,
    /// The lanes this row draws — [`GraphRow`], carried on the row so the
    /// painter never indexes a second collection.
    pub lanes: GraphRow,
    pub expanded: bool,
}

/// The working tree's own row (V5).
///
/// It wears the columns a commit wears and answers each of them honestly rather
/// than leaving them blank: its date is `now` because that is when the working
/// tree is, its hash is `*` because it has none, and its author column is empty
/// because nobody has claimed this work yet — which is a different statement
/// from "the column is not drawn".
#[derive(Clone, Debug, PartialEq)]
pub struct GraphUncommittedRow {
    pub index: usize,
    /// How many distinct paths — the `(N)` the row says out loud.
    pub count: usize,
    pub tooltip: String,
    /// The lanes it draws, walked by [`LaneWalker::open_with_uncommitted`].
    pub lanes: GraphRow,
    pub expanded: bool,
}

/// One file of the open commit, or of the open working tree.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphFileRow {
    pub index: usize,
    /// The commit this file belongs to, or [`GRAPH_UNCOMMITTED_HASH`] when it is
    /// the working tree's.
    pub hash: String,
    pub path: String,
    pub renamed_from: Option<String>,
    pub tooltip: String,
    /// Set when this row is the **working tree's** and not a commit's (V5): the
    /// letters it wears, and which side of the index its diff is.
    ///
    /// An `Option` rather than a second row type because everything else about
    /// the two is identical — one path, one indent, one press that opens a
    /// document — and the two facts that differ are exactly these two.
    pub working: Option<GraphWorkingFile>,
    /// The one letter a **commit's** file wears (D4), through the same
    /// [`crate::git_panel::GitBadgeInk`] mapping the working tree's letters go
    /// through — because they mean the same things.
    ///
    /// One and not a list, which is the honest difference between the two kinds
    /// of row: a status entry has an index column and a working-tree column and
    /// can be two things at once, and a commit is a point with one story about
    /// each file it touched.
    pub badge: Option<crate::git_panel::GitBadge>,
    /// `+N \u{2212}M` (D4) — `None` when git had no lines to count, which is a
    /// binary file and is drawn as an em dash rather than as two zeroes.
    pub stat: Option<crate::git::GitFileStat>,
    /// Set when this row belongs to a **comparison** (D6) rather than to one
    /// commit: the two ends the diff is between, older first.
    ///
    /// It decides which document the row opens and nothing else, which is why it
    /// is a field beside [`Self::hash`] rather than a third row type: everything
    /// about the drawing — the indent, the badge, the counts, the path — is the
    /// same picture of the same file.
    pub range: Option<(String, Option<String>)>,
}

/// The block under an open row (D1/D2/D6/D7).
#[derive(Clone, Debug, PartialEq)]
pub struct GraphDetailRow {
    /// The **first** list row the block stands on.
    pub index: usize,
    /// How many list rows it claims. Always at least one.
    pub rows: usize,
    pub detail: GraphDetail,
}

/// Which of the two things a detail block can be.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphDetail {
    /// One commit, opened: the rest of its message and the facts about it.
    Commit(GraphCommitDetail),
    /// Two places, compared (D6) — which *replaces* the story rather than
    /// standing beside it, because a comparison is not a fact about either of
    /// its ends.
    Compare(GraphCompareDetail),
}

/// The open commit's own story.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphCommitDetail {
    /// The full hash — what the copy verb puts on the clipboard, and what a
    /// press has to be able to name the commit by.
    pub hash: String,
    pub short: String,
    /// The subject, carried for the copy verb and **not drawn**: it is already
    /// the row this block hangs off, and a block that repeated it would be the
    /// page saying the same sentence twice six pixels apart.
    pub subject: String,
    /// The body, already wrapped to the description column and already capped
    /// (D1). Empty when the commit has none, which is most of them.
    pub body: Vec<String>,
    /// Everything on the meta line **up to** the parents: the author, the date,
    /// and the committer when there is one worth naming.
    pub meta: String,
    /// How wide [`Self::meta`] is, measured through the font at build time —
    /// because where the first parent chip stands is where that string ends, and
    /// only the thing holding the font can say where that is.
    pub meta_width: f32,
    pub parents: Vec<GraphParentChip>,
}

/// One clickable parent hash on the meta line (D2).
#[derive(Clone, Debug, PartialEq)]
pub struct GraphParentChip {
    /// git's own abbreviation of the parent, cut here because a `%P` field is
    /// full hashes and there is no second question to ask for their short forms.
    ///
    /// Seven characters, which is git's own floor and is what every other short
    /// hash on this page happens to be; unlike the row's own `%h` this one is a
    /// cut and says so, because the alternative is a `rev-parse` per parent per
    /// expansion — exactly the reading R31 is about.
    pub short: String,
    /// The whole hash, which is what the seek looks for.
    pub hash: String,
    pub width: f32,
}

/// The head of a comparison (D6).
#[derive(Clone, Debug, PartialEq)]
pub struct GraphCompareDetail {
    /// `Comparing abc1234 \u{2192} def5678`, older on the left.
    pub head: String,
    /// The older end — always a commit, because the working tree cannot be
    /// older than anything.
    pub a: String,
    /// The newer end, or the working tree when absent.
    pub b: Option<String>,
}

/// A part of a detail block that answers a press of its own.
///
/// Its own enum rather than a rectangle list because a hit test's answer is
/// *which thing*, and the geometry that produced it is not something the caller
/// should have to keep.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphDetailPart {
    /// The nth parent hash on the meta line: press it and the graph goes there.
    Parent(usize),
    /// Copy the whole hash (D7).
    CopyHash,
    /// Copy the subject (D7).
    CopySubject,
    /// The compare block's `\u{d7}` — stop comparing.
    LeaveCompare,
}

/// What a working-tree file row knows that a commit's file row does not (V5).
#[derive(Clone, Debug, PartialEq)]
pub struct GraphWorkingFile {
    /// `crate::git_panel::badges_of`'s own answer — the same letters the Git
    /// page draws for the same file, because they are the same file.
    pub badges: Vec<crate::git_panel::GitBadge>,
    /// R25's mapping: a row under the staged heading opens `--cached`.
    pub staged: bool,
}

impl GraphViewRow {
    #[must_use]
    pub fn index(&self) -> usize {
        match self {
            Self::Uncommitted(row) => row.index,
            Self::Commit(row) => row.index,
            Self::File(row) => row.index,
            Self::Detail(row) => row.index,
        }
    }

    /// How many list rows this row claims — one for everything but the detail
    /// block.
    #[must_use]
    pub fn rows(&self) -> usize {
        match self {
            Self::Detail(row) => row.rows,
            _ => 1,
        }
    }
}

/// One frame of the graph document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphContent {
    pub head: Option<GraphHead>,
    /// **Only the rows the window can show**, plus [`GRAPH_WINDOW_BUFFER_ROWS`]
    /// on each side. This is R23: a repository with ten thousand commits builds
    /// two dozen rows a frame, because a row that cannot be seen costs a
    /// measurement, a tooltip and three `String`s, and ten thousand of those is
    /// a frame nobody gets.
    pub rows: Vec<GraphViewRow>,
    /// How many rows there are altogether — what the scrollbar and the clamp are
    /// about.
    pub total_rows: usize,
    /// How wide the graph column is drawn, in lanes (R18's hysteresis).
    pub lane_width: usize,
    pub scroll_px: f32,
    /// The whole page in one sentence, in the muted ink, where the rows would be.
    ///
    /// A `String` since the toast ruling (2026-08-16), for
    /// [`crate::git_panel::GitPanelContent::empty`]'s reason: the third sentence
    /// this can carry is git's own words about a history it would not read, and
    /// that is a persistent fact rather than a notice — so it stands here instead
    /// of in the red strip this page used to carve off its own top.
    pub empty: Option<String>,
    /// The last row index the held width was needed for — see [`LaneWidthHold`].
    pub lane_width_until: usize,
    /// The reader is near the end and there is more history (R23's auto-paging).
    pub wants_more: bool,
    /// Which of the table's right-hand columns this width draws (V1).
    pub columns: GraphColumns,
    /// The row wearing the selected ground (V8), whether or not it is in the
    /// window: the painter needs it to decide a ground, and the keyboard needs it
    /// to decide where `↑` goes from.
    pub selected: Option<usize>,
    /// Whether row zero is the Uncommitted Changes row (V5) — what
    /// [`item_at`]'s arithmetic turns on, carried so the hit test and the
    /// keyboard read the same list the painter drew.
    pub uncommitted_rows: Option<usize>,
    /// Where the open row stands and how many rows it unfolded, in list
    /// coordinates. `None` when nothing is open.
    pub open_rows: Option<(usize, usize)>,
    /// Where the detail block stands and how many rows it claims, in list
    /// coordinates (v2 ②) — what the keyboard steps **over**.
    ///
    /// A block with no press of its own is not a row the selection should be
    /// able to land on: `↓` walking through five blank rows of prose would be
    /// the arrow keys measuring pixels instead of rows, which is the one thing
    /// V14 said they must not do.
    pub detail_rows: Option<(usize, usize)>,
    /// The two rows a comparison is between, in list coordinates (D6) — both of
    /// which wear the selected ground.
    ///
    /// `None` when nothing is being compared, which is the ordinary state.
    pub compare_rows: Option<(usize, usize)>,
    /// The two ends the comparison is between, older first — what the question
    /// is asked about.
    ///
    /// Carried on the frame rather than left to be re-derived by the caller for
    /// [`Self::columns`]'s reason: the ordering rule is written once, here,
    /// where the rows it is about were laid out, and a caller that re-derived it
    /// could disagree with the block it is standing under.
    pub compare_pair: Option<(String, Option<String>)>,
    /// Where the one expansion hangs, **in commit indices**, and how many rows
    /// it adds — which is what turns a commit index back into a list row.
    pub open_commit: Option<(usize, usize)>,
}

impl GraphContent {
    /// Which list row the commit at this index of the log stands on.
    ///
    /// The inverse of [`item_at`], and the same arithmetic read backwards: the
    /// working tree's rows above, plus the expansion's when the expansion is
    /// above this commit. Needed by the seek (D2), which is handed a *commit*
    /// by the log and has to say which row to scroll to.
    #[must_use]
    pub fn commit_row(&self, at: usize) -> usize {
        let above = self.uncommitted_rows.map_or(0, |files| files + 1);
        let shift = self
            .open_commit
            .map_or(0, |(open, rows)| if at > open { rows } else { 0 });
        above + at + shift
    }
}

/// Everything one open graph document knows.
///
/// The cache is what the repository said; the walker is the picture that was
/// derived from it; `lane_width` is the one piece of *hysteresis* on the page.
#[derive(Clone, Debug)]
pub struct GraphState {
    pub cache: crate::git::GitCache,
    lanes: LaneWalker,
    /// How many commits the walker has already been shown, so a log that
    /// *shrank* — a checkout onto another branch — is detected rather than
    /// walked onto the end of the old one.
    walked: usize,
    /// Whether the last walk was laid out with an Uncommitted Changes row above
    /// it (V5).
    ///
    /// The picture is different from its first row down when this changes —
    /// `HEAD`'s own lane either arrives from above or opens — so it is not
    /// something a later page can be appended around. Remembered, and compared,
    /// so the re-walk happens on the transition and not on every status.
    uncommitted: bool,
}

impl GraphState {
    #[must_use]
    pub fn new(root: std::path::PathBuf) -> Self {
        Self {
            cache: crate::git::GitCache::at_root(root, crate::git::GitRole::Graph),
            lanes: LaneWalker::default(),
            walked: 0,
            uncommitted: false,
        }
    }

    /// How many distinct paths the working tree has something to say about, or
    /// `None` when it has nothing (V5).
    ///
    /// **`None` and not `Some(0)`**, because a zero here is not a small number,
    /// it is the absence of a row: a clean repository has no Uncommitted Changes
    /// line at all, rather than one reading `(0)`.
    ///
    /// One path and not one claim: a file that is both staged and changed is one
    /// thing the working tree has to say, said in two places. The status list is
    /// already one entry per path, so this is its length and nothing cleverer.
    #[must_use]
    pub fn uncommitted(&self) -> Option<usize> {
        let status = self.cache.status().ready()?;
        (!status.entries.is_empty()).then_some(status.entries.len())
    }

    /// Bring the lane layout up to date with whatever the cache now holds.
    ///
    /// **A shorter list is a different history.** Pages only ever extend a log
    /// (`GitCache::accept` refuses one that does not start where the list ends),
    /// so the single way the count can fall is a refresh — a checkout, a commit
    /// — and what that gives is a history the running lane state is not about.
    /// Resuming into it would draw roads that belong to the branch you left.
    pub fn sync(&mut self) {
        // **The working tree is part of the layout** (V5). Whether the tree is
        // dirty decides whether `HEAD`'s lane arrives from above or opens on its
        // own row, so a status that has just crossed between clean and dirty is
        // a different picture from its first row down — not something the next
        // page can be appended around. It is compared rather than re-derived so
        // that the re-walk costs a transition and not a status: `git status`
        // answers whenever anything at all is staged or saved, and almost none of
        // those answers cross this line.
        let dirty = self.uncommitted().is_some();
        if dirty != self.uncommitted {
            self.uncommitted = dirty;
            self.lanes.reset();
            self.walked = 0;
        }
        let Some(log) = self.cache.log().ready() else {
            return;
        };
        if log.commits.len() < self.walked {
            self.lanes.reset();
        }
        if self.uncommitted
            && self.lanes.head_row().is_none()
            && let Some(head) = log.commits.first()
        {
            // A dirty tree with no history has nothing to hang the row off, and
            // an empty repository already says so in its own sentence.
            self.lanes.open_with_uncommitted(&head.hash);
        }
        self.lanes.extend(&log.commits);
        self.walked = log.commits.len();
    }

    /// Throw the picture away, keeping the repository.
    ///
    /// **What a checkout owes every graph of that repository.** The lane state
    /// is a running reading of one history; after a checkout it is a reading of
    /// a history this repository is no longer on, and resuming into the next
    /// answer would draw the branch you left behind the branch you moved to.
    /// Not left to [`Self::sync`]'s shorter-list guard, which only catches it
    /// when the new history is *shorter* — moving to a longer branch would slip
    /// straight past it.
    pub fn invalidate(&mut self) {
        self.lanes.reset();
        self.walked = 0;
    }

    #[must_use]
    pub fn lanes(&self) -> &[GraphRow] {
        self.lanes.rows()
    }

    /// The Uncommitted Changes row's lanes, when the picture has one (V5).
    #[must_use]
    pub fn head_lanes(&self) -> Option<&GraphRow> {
        self.lanes.head_row()
    }
}

/// The one open commit, or none — the graph's half of R15's accordion.
#[must_use]
pub fn toggled_expansion(current: Option<&str>, hash: &str) -> Option<String> {
    crate::git_panel::toggled_expansion(current, hash)
}

/// Where the rows of a graph fall inside a body.
///
/// **Arithmetic and not a list**, which is the other half of R23: every row is
/// the same height, so "where is row nine thousand" is a multiplication rather
/// than a walk down nine thousand rectangles. The panel can afford a `Vec` of
/// rects because it holds a page; this one is asked about a repository.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphGeometry {
    /// The rows' own box — the body less the head.
    ///
    /// It was once "less any banner" too; the strip is gone with the toast ruling
    /// (2026-08-16), so a graph's rows stand in the same rectangle whether or not
    /// a checkout was just refused.
    pub viewport: [f32; 4],
    pub head: Option<[f32; 4]>,
    /// The column header's own strip (V2), between the masthead and the rows.
    ///
    /// **Outside [`Self::viewport`]**, which is the whole of "it is a fixed row
    /// above the virtual window and not a virtual row": the rows' box begins
    /// under it, so it neither scrolls with them, nor takes an index, nor
    /// answers a hit test. There is nothing in this module that could
    /// accidentally treat it as row zero, because it is not in the list the row
    /// arithmetic is about.
    pub header: Option<[f32; 4]>,
    pub row_height: f32,
    pub scroll_px: f32,
    pub content_height: f32,
    pub max_scroll: f32,
    top: f32,
}

impl GraphGeometry {
    /// Where row `index` is on screen.
    #[must_use]
    pub fn row_rect(&self, index: usize) -> [f32; 4] {
        #[allow(clippy::cast_precision_loss)]
        let offset = index as f32 * self.row_height;
        let top = self.top + offset;
        let pad = (self.viewport[2] - self.viewport[0]).min(0.0);
        let _ = pad;
        [
            self.viewport[0],
            top,
            self.viewport[2],
            top + self.row_height,
        ]
    }

    /// Which row the pointer is on, if any.
    #[must_use]
    pub fn row_at(&self, x: f32, y: f32, total_rows: usize) -> Option<usize> {
        if x < self.viewport[0]
            || x >= self.viewport[2]
            || y < self.viewport[1]
            || y >= self.viewport[3]
        {
            return None;
        }
        if self.row_height <= 0.0 {
            return None;
        }
        let offset = y - self.top;
        if offset < 0.0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let index = (offset / self.row_height) as usize;
        (index < total_rows).then_some(index)
    }

    /// The half-open range of rows worth building (R23).
    #[must_use]
    pub fn window(&self, total_rows: usize) -> std::ops::Range<usize> {
        if self.row_height <= 0.0 || total_rows == 0 {
            return 0..0;
        }
        let height = (self.viewport[3] - self.viewport[1]).max(0.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let first = (self.scroll_px / self.row_height) as usize;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let span = ((height / self.row_height).ceil() as usize) + 1;
        let start = first.saturating_sub(GRAPH_WINDOW_BUFFER_ROWS);
        let end = first
            .saturating_add(span)
            .saturating_add(GRAPH_WINDOW_BUFFER_ROWS)
            .min(total_rows);
        start..end.max(start)
    }

    /// The scroll that brings row `index` fully into the body (V14).
    ///
    /// **The nearest scroll and not a centring one**: a selection walked down
    /// with `↓` should push the list by exactly one row when it reaches the
    /// bottom edge, the way every list on this platform behaves — a jump that
    /// re-centred the selection would move the page under a reader who asked for
    /// one step. A row already whole on screen moves nothing at all, which is
    /// why this returns the scroll it was given rather than an `Option`.
    #[must_use]
    pub fn reveal(&self, index: usize) -> f32 {
        if self.row_height <= 0.0 {
            return self.scroll_px;
        }
        // `top` is the first row's top in *screen* pixels with the current
        // scroll already taken out, so putting it back is what turns a row's
        // rectangle into its place in the content.
        let pad_top = self.top - self.viewport[1] + self.scroll_px;
        #[allow(clippy::cast_precision_loss)]
        let row_top = pad_top + index as f32 * self.row_height;
        let row_bottom = row_top + self.row_height;
        let height = (self.viewport[3] - self.viewport[1]).max(0.0);
        let wanted = if row_top < self.scroll_px {
            row_top
        } else if row_bottom > self.scroll_px + height {
            row_bottom - height
        } else {
            self.scroll_px
        };
        wanted.clamp(0.0, self.max_scroll.max(0.0))
    }
}

/// Lay a graph out inside a preview body.
#[must_use]
pub fn graph_geometry(body: [f32; 4], content: &GraphContent, scale: f32) -> GraphGeometry {
    let rest = body;
    let pad_x = (GRAPH_PADDING_X_LOGICAL_PX * scale).round();
    let head_height = ((GRAPH_HEAD_PADDING_Y_LOGICAL_PX * 2.0
        + crate::git_panel::GIT_HEAD_FONT_LOGICAL_PX
            .max(crate::git_panel::GIT_PILL_HEIGHT_LOGICAL_PX))
        * scale)
        .round();
    let head_pad_x = (GRAPH_HEAD_PADDING_X_LOGICAL_PX * scale).round();
    let (head, viewport) = match content.head {
        Some(_) => {
            let bottom = (rest[1] + head_height).min(rest[3]);
            (
                Some([
                    rest[0] + pad_x + head_pad_x,
                    rest[1],
                    rest[2] - pad_x - head_pad_x,
                    bottom,
                ]),
                [rest[0] + pad_x, bottom, rest[2] - pad_x, rest[3]],
            )
        }
        None => (None, [rest[0] + pad_x, rest[1], rest[2] - pad_x, rest[3]]),
    };
    // The column header (V2), carved off the rows' box before anything measures
    // it: it is a fixed strip and not a row, so every number below — the window,
    // the content height, the clamp — is about a viewport that already has it
    // taken out. A page with nothing in it has no columns to name, so it has no
    // header either; the sentence standing where the rows would be gets the
    // whole box, which is where it was already centred.
    let header_height = (GRAPH_HEADER_HEIGHT_LOGICAL_PX * scale).round();
    let (header, viewport) = if content.empty.is_none() && content.total_rows > 0 {
        let bottom = (viewport[1] + header_height).min(viewport[3]);
        (
            Some([viewport[0], viewport[1], viewport[2], bottom]),
            [viewport[0], bottom, viewport[2], viewport[3]],
        )
    } else {
        (None, viewport)
    };
    let row_height = (GRAPH_ROW_HEIGHT_LOGICAL_PX * scale).round().max(1.0);
    let pad_top = (GRAPH_PADDING_TOP_LOGICAL_PX * scale).round();
    let pad_bottom = (GRAPH_PADDING_BOTTOM_LOGICAL_PX * scale).round();
    #[allow(clippy::cast_precision_loss)]
    let content_height = pad_top + content.total_rows as f32 * row_height + pad_bottom;
    let max_scroll = (content_height - (viewport[3] - viewport[1])).max(0.0);
    GraphGeometry {
        viewport,
        head,
        header,
        row_height,
        scroll_px: content.scroll_px,
        content_height,
        max_scroll,
        top: viewport[1] + pad_top - content.scroll_px,
    }
}

/// The only scroll a graph is allowed to hold — [`crate::git_panel::clamp_git_scroll`]'s
/// twin, and here for its reason: the bound belongs where the number is written.
#[must_use]
pub fn clamp_graph_scroll(
    body: [f32; 4],
    content: &GraphContent,
    scroll_px: f32,
    scale: f32,
) -> f32 {
    let mut probe = content.clone();
    probe.scroll_px = 0.0;
    let max = graph_geometry(body, &probe, scale).max_scroll;
    scroll_px.clamp(0.0, max)
}

// ── building one frame ─────────────────────────────────────────────────────

/// How wide a run of text is — [`crate::git_panel::Measure`]'s own closure, for
/// its own reason: only the thing holding the font can say.
pub type Measure<'a> = crate::git_panel::Measure<'a>;

/// The graph column's width, and how long it is being held (R18).
///
/// **Only ever grows while the reader is still looking at what made it grow.**
/// A window that slides down past a nine-lane knot and into a stretch of one
/// lane would, without this, snap the whole page eight lanes to the left the
/// instant the last of the knot left the screen — every message, every date,
/// every hash jumping sideways because something scrolled off the top. So the
/// width is held until the window has moved entirely past the row that asked
/// for it, and only then does it fall to what the window now needs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LaneWidthHold {
    pub width: usize,
    /// The last row index that needed [`Self::width`]. Once the window starts
    /// after this, the hold is over.
    pub until: usize,
}

/// **What one seat is looking at**, as against what its repository said.
///
/// Three fields that always travel together and always came from the same place
/// — the seat's own [`crate::GraphView`] — gathered into one value since v2 ②,
/// where a fourth argument of the same type would have made `build` take three
/// `Option<&str>`s in a row and let any two of them be swapped silently.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GraphLook<'a> {
    /// The one row turned over (R15's accordion), or
    /// [`GRAPH_UNCOMMITTED_HASH`] for the working tree's (V5).
    pub expanded: Option<&'a str>,
    /// The **other** end of a comparison (D6), when one is running. Compare mode
    /// is a property of the expanded row, so this means nothing without
    /// [`Self::expanded`] and is ignored when it names the same row.
    pub compare: Option<&'a str>,
    /// The row wearing the selected ground (V8).
    pub selected: Option<usize>,
}

/// Turn what a graph document knows into what it draws.
///
/// `body` and `scroll_px` are here rather than in the painter because **the
/// window is part of the derivation** (R23): which rows exist on this frame is
/// decided by where the body is and how far down the list is, and a build that
/// did not know those two things could only build all of them.
#[must_use]
pub fn build(
    state: &GraphState,
    look: GraphLook<'_>,
    body: [f32; 4],
    scroll_px: f32,
    hold: LaneWidthHold,
    scale: f32,
    measure: &mut Measure<'_>,
) -> GraphContent {
    let GraphLook {
        expanded,
        compare,
        selected,
    } = look;
    let mut content = GraphContent {
        scroll_px,
        selected,
        // The columns are a fact about the *body*, decided before anything is
        // measured, because what the author column is ellipsised to depends on
        // whether there is one (V1).
        columns: GraphColumns::at_width((body[2] - body[0]) / scale.max(f32::EPSILON)),
        ..GraphContent::default()
    };
    let cache = &state.cache;

    let log = match cache.log() {
        crate::git::GitSlot::Ready(log) => log,
        // git's own words, standing where the commits would be. Persistent, so
        // not a toast (2026-08-16): a history git refuses to read stays refused
        // until something outside this window changes.
        crate::git::GitSlot::Failed(fault) => {
            content.empty = Some(crate::git_panel::fault_sentence(fault));
            return content;
        }
        crate::git::GitSlot::Idle | crate::git::GitSlot::Pending => {
            content.empty = Some(crate::git_panel::GIT_READING.to_owned());
            return content;
        }
    };

    // The head is the panel's own sentence about the same repository (R20).
    content.head = Some(crate::git_panel::head_of(cache, scale, measure));

    if log.commits.is_empty() {
        content.empty = Some(crate::git_panel::GIT_NO_COMMITS.to_owned());
        return content;
    }

    // **The working tree's own row** (V5), and its files when it is open. The
    // status is already in this cache — it is one of the three questions every
    // role asks — so this row costs no reading whatever, which is the whole of
    // why the row is allowed to exist under R31.
    let head_files: Vec<WorkingFile> =
        if state.head_lanes().is_some() && expanded == Some(GRAPH_UNCOMMITTED_HASH) {
            working_files(cache)
        } else {
            Vec::new()
        };
    let head = state.head_lanes().map(|_| head_files.len());

    let above = head.map_or(0, |files| files + 1);

    // Where each end of a comparison stands in the graph's own order: the
    // working tree is above everything and a commit is where the log puts it.
    // **Signed, because the working tree's place is "before the first row"** and
    // an unsigned index has no way to say that — which is exactly the fact the
    // older/newer ruling below turns on.
    let order_of = |hash: &str| -> Option<isize> {
        if hash == GRAPH_UNCOMMITTED_HASH {
            return head.map(|_| -1);
        }
        log.commits
            .iter()
            .position(|commit| commit.hash == hash)
            .and_then(|at| isize::try_from(at).ok())
    };
    // **The comparison, if there is one** (D6). It needs both ends and they have
    // to be different rows: a commit compared with itself is not a comparison,
    // and a `compare` naming a row that is not in the loaded pages is a
    // comparison whose picture cannot be drawn.
    let pair = match (expanded.and_then(order_of), compare.and_then(order_of)) {
        (Some(one), Some(other)) if one != other => Some((one.max(other), one.min(other))),
        _ => None,
    };
    // That order, back in list coordinates. The working tree's `-1` is row zero
    // and not "one before the first commit" — it is the only row above them, so
    // its place is the top of the list rather than an offset from anywhere.
    let list_row =
        |order: isize| -> usize { usize::try_from(order).map_or(0, |order| above + order) };

    // **Where the accordion is, as a row index.** Held as an index rather than
    // as a hash so that mapping a row number back to what stands on it is
    // arithmetic — see [`item_at`] — which is the other half of what makes a
    // ten-thousand-row list cost a screenful.
    //
    // Under compare it hangs off the **older** of the two rows, which is the
    // lower one on the page: a block that opened above one of its own ends would
    // read as belonging to whatever row happened to be under it.
    let detail_room = detail_text_room(body, content.columns, scale);
    let expansion = match pair {
        Some((older, newer)) => {
            #[allow(clippy::cast_sign_loss)]
            let at = older.max(0) as usize;
            #[allow(clippy::cast_sign_loss)]
            let b = usize::try_from(newer)
                .ok()
                .and_then(|newer| log.commits.get(newer))
                .map(|commit| commit.hash.clone());
            log.commits.get(at).map(|commit| {
                let a = commit.hash.clone();
                let files = match cache.compare_files(&a, b.as_deref()) {
                    Some(crate::git::GitSlot::Ready(files)) => files.clone(),
                    _ => Vec::new(),
                };
                content.compare_rows = Some((above + at, list_row(newer)));
                content.compare_pair = Some((a.clone(), b.clone()));
                Expansion {
                    at,
                    detail: Some(GraphDetail::Compare(GraphCompareDetail {
                        head: compare_sentence(&a, b.as_deref(), log),
                        a: a.clone(),
                        b: b.clone(),
                    })),
                    range: Some((a, b)),
                    files,
                }
            })
        }
        None => expanded.and_then(|hash| {
            let at = log.commits.iter().position(|commit| commit.hash == hash)?;
            let commit = log.commits.get(at)?;
            let files = match cache.commit_files(hash) {
                Some(crate::git::GitSlot::Ready(files)) => files.clone(),
                _ => Vec::new(),
            };
            Some(Expansion {
                at,
                detail: Some(GraphDetail::Commit(commit_detail(
                    commit,
                    detail_room,
                    scale,
                    measure,
                ))),
                range: None,
                files,
            })
        }),
    };
    let open = expansion.as_ref().map(|expansion| GraphOpen {
        at: expansion.at,
        detail: expansion
            .detail
            .as_ref()
            .map_or(0, |detail| detail_rows(detail, scale)),
        files: expansion.files.len(),
    });
    content.total_rows = above + log.commits.len() + open.map_or(0, GraphOpen::rows);
    content.uncommitted_rows = head;
    if let Some(open) = open.filter(|open| open.detail > 0) {
        content.detail_rows = Some((above + open.at + 1, open.detail));
    }
    content.open_commit = open.map(|open| (open.at, open.rows()));
    // What `Esc` has to collapse, in list coordinates — and `None` when nothing
    // is open, which is the difference between "the working tree's row is drawn"
    // and "the working tree's row is unfolded". It names the row the **reader**
    // turned over, which under compare is not the row the block hangs off.
    content.open_rows = match (open, expanded.and_then(order_of), head) {
        (Some(open), Some(at), _) => Some((list_row(at), open.rows())),
        (None, _, Some(files)) if expanded == Some(GRAPH_UNCOMMITTED_HASH) => Some((0, files)),
        _ => None,
    };

    let geometry = graph_geometry(body, &content, scale);
    let window = geometry.window(content.total_rows);

    // R18's hysteresis, decided over the window that is about to be drawn.
    let lanes = state.lanes();
    let mut needed = 1;
    let mut widest_at = window.start;
    for index in window.clone() {
        let Some(item) = item_at(index, head, open) else {
            continue;
        };
        // The uncommitted row's own width counts too: it is one lane wide and
        // could never be the widest, but a picture that asked only about commits
        // would be one whose top row was not part of it.
        let row = match item {
            GraphItem::Uncommitted => state.head_lanes(),
            GraphItem::Commit(at) => lanes.get(at),
            GraphItem::Working(_) | GraphItem::File { .. } | GraphItem::Detail => continue,
        };
        let Some(row) = row else { continue };
        if row.width > needed {
            needed = row.width;
            widest_at = index;
        }
    }
    let held = hold.width > needed && window.start <= hold.until;
    content.lane_width = if held { hold.width } else { needed };
    content.lane_width_until = if held { hold.until } else { widest_at };

    // R23's auto-paging: the reader is within a screenful of the end and there
    // is another page. Asking is the caller's — this only says when.
    content.wants_more = log.has_more && window.end + GRAPH_PREFETCH_ROWS >= content.total_rows;

    let ref_font = GRAPH_REF_FONT_LOGICAL_PX * scale;
    let author_font = GRAPH_AUTHOR_FONT_LOGICAL_PX * scale;
    let author_room = GRAPH_AUTHOR_COLUMN_LOGICAL_PX * scale;
    // The detail block spans several rows, so the window can start *inside* it:
    // it is pushed on the first index that lands on it, at the block's own first
    // row, and not again.
    let mut detail_pushed = false;
    for index in window {
        match item_at(index, head, open) {
            Some(GraphItem::Uncommitted) => {
                let Some(count) = state.uncommitted() else {
                    continue;
                };
                let Some(lane_row) = state.head_lanes().cloned() else {
                    continue;
                };
                content
                    .rows
                    .push(GraphViewRow::Uncommitted(GraphUncommittedRow {
                        index,
                        count,
                        tooltip: uncommitted_tooltip(count),
                        lanes: lane_row,
                        expanded: expanded == Some(GRAPH_UNCOMMITTED_HASH),
                    }));
            }
            Some(GraphItem::Working(at)) => {
                let Some(file) = head_files.get(at) else {
                    continue;
                };
                content.rows.push(GraphViewRow::File(GraphFileRow {
                    index,
                    hash: GRAPH_UNCOMMITTED_HASH.to_owned(),
                    tooltip: match &file.path.1 {
                        Some(from) => format!("{} - renamed from {from}", file.path.0),
                        None => file.path.0.clone(),
                    },
                    path: file.path.0.clone(),
                    renamed_from: file.path.1.clone(),
                    working: Some(GraphWorkingFile {
                        badges: file.badges.clone(),
                        staged: file.staged,
                    }),
                    // The working tree's letters are the two it already wears;
                    // its counts are not asked for, because the status this row
                    // is read off is a list of *what* changed and never of how
                    // much (R31: one more question per keystroke is a question
                    // too many).
                    badge: None,
                    stat: None,
                    range: None,
                }));
            }
            Some(GraphItem::Detail) => {
                let (Some(expansion), Some(open)) = (expansion.as_ref(), open) else {
                    continue;
                };
                let Some(detail) = expansion.detail.clone() else {
                    continue;
                };
                if std::mem::replace(&mut detail_pushed, true) {
                    continue;
                }
                content.rows.push(GraphViewRow::Detail(GraphDetailRow {
                    index: above + open.at + 1,
                    rows: open.detail,
                    detail,
                }));
            }
            Some(GraphItem::Commit(at)) => {
                let Some(commit) = log.commits.get(at) else {
                    continue;
                };
                let lane_row = lanes.get(at).cloned().unwrap_or_default();
                let dot = lane_row.dot;
                content.rows.push(GraphViewRow::Commit(GraphCommitRow {
                    index,
                    tooltip: commit_tooltip(commit),
                    // Cut here rather than at the paint, and only when the column
                    // is drawn at all: a name nobody can see costs no binary
                    // search (V1's collapse, felt in the measurer).
                    author: if content.columns.author {
                        crate::settings::ellipsized(
                            &commit.author_name,
                            author_room,
                            author_font,
                            measure,
                        )
                    } else {
                        String::new()
                    },
                    refs: commit
                        .refs
                        .iter()
                        .map(|reference| GraphRefPill {
                            text_width: measure(&reference.name, ref_font),
                            name: reference.name.clone(),
                            head: reference.head,
                            lane: dot,
                        })
                        .collect(),
                    hash: commit.hash.clone(),
                    short: commit.short.clone(),
                    subject: commit.subject.clone(),
                    time: commit.time_relative.clone(),
                    lanes: lane_row,
                    expanded: expanded == Some(commit.hash.as_str()),
                }));
            }
            Some(GraphItem::File { commit, file }) => {
                let Some(expansion) = expansion.as_ref() else {
                    continue;
                };
                let Some(entry) = expansion.files.get(file) else {
                    continue;
                };
                // A comparison's rows belong to its **older** end, which is also
                // the commit the block hangs off — so one lookup answers both
                // kinds of expansion.
                let Some(hash) = log.commits.get(commit).map(|c| c.hash.clone()) else {
                    continue;
                };
                content.rows.push(GraphViewRow::File(GraphFileRow {
                    index,
                    hash,
                    tooltip: file_tooltip(entry),
                    path: entry.path.clone(),
                    renamed_from: entry.renamed_from.clone(),
                    working: None,
                    badge: Some(crate::git_panel::GitBadge {
                        letter: entry.code.letter(),
                        ink: crate::git_panel::GitBadgeInk::of(entry.code),
                    }),
                    stat: entry.stat,
                    range: expansion.range.clone(),
                }));
            }
            None => {}
        }
    }
    content
}

/// What the one open row unfolded into, before it is turned into rows.
///
/// Held together rather than as three parallel `Option`s because the three are
/// one answer to one question — *what is open, and what is under it* — and the
/// combinations that a triple of `Option`s would admit (files with no block, a
/// range with no files) are combinations that cannot happen.
struct Expansion {
    /// The commit the block hangs off, **in commit indices**.
    at: usize,
    detail: Option<GraphDetail>,
    /// Set when the files are a comparison's rather than a commit's.
    range: Option<(String, Option<String>)>,
    files: Vec<crate::git::GitCommitFile>,
}

/// Where the detail block's text may stand: its left edge and how wide it is.
///
/// **The file rows' own indent and the description column's own right edge**,
/// because the block belongs to the same column as everything else under an open
/// row — prose that started further left than the file paths under it would read
/// as belonging to the row above rather than to this one.
fn detail_text_room(body: [f32; 4], columns: GraphColumns, scale: f32) -> f32 {
    let pad_x = (GRAPH_PADDING_X_LOGICAL_PX * scale).round();
    let row_box = [body[0] + pad_x, 0.0, body[2] - pad_x, 0.0];
    let rects = graph_column_rects(row_box, columns, scale);
    let left = row_box[0] + (GRAPH_FILE_INDENT_LOGICAL_PX * scale).round();
    (rects.description_right - left).max(1.0)
}

/// The meta line's own height — the taller of its text and the hover verbs
/// standing on it, because a button that overflowed its line would overlap the
/// prose above it.
fn detail_meta_height(scale: f32) -> f32 {
    ((GRAPH_META_FONT_LOGICAL_PX * 1.4).max(crate::git_panel::GIT_ACT_LOGICAL_PX) * scale)
        .round()
        .max(1.0)
}

/// How tall a detail block wants to be, in physical pixels.
fn detail_height(detail: &GraphDetail, scale: f32) -> f32 {
    let pad = (GRAPH_DETAIL_PADDING_Y_LOGICAL_PX * scale).round();
    let meta = detail_meta_height(scale);
    match detail {
        GraphDetail::Compare(_) => pad * 2.0 + meta,
        GraphDetail::Commit(commit) => {
            let line = (GRAPH_BODY_LINE_LOGICAL_PX * scale).round().max(1.0);
            let gap = (GRAPH_DETAIL_GAP_LOGICAL_PX * scale).round();
            #[allow(clippy::cast_precision_loss)]
            let prose = if commit.body.is_empty() {
                0.0
            } else {
                commit.body.len() as f32 * line + gap
            };
            pad * 2.0 + prose + meta
        }
    }
}

/// How many **list rows** a detail block claims.
///
/// **A whole number of rows, and that is the whole trick** (v2 ②): the graph's
/// geometry is one height times one index, from `row_rect` to `reveal` to the
/// scrollbar, and a block of arbitrary height dropped into the middle of it
/// would have made every one of those a three-case piece of arithmetic that some
/// caller would eventually get wrong. Rounding *up* buys the block a strip of
/// slack at its bottom, which reads as padding and costs nothing else.
fn detail_rows(detail: &GraphDetail, scale: f32) -> usize {
    let row = (GRAPH_ROW_HEIGHT_LOGICAL_PX * scale).round().max(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rows = (detail_height(detail, scale) / row).ceil() as usize;
    rows.max(1)
}

/// The open commit's story, wrapped and measured (D1/D2/D7).
fn commit_detail(
    commit: &GitCommit,
    room: f32,
    scale: f32,
    measure: &mut Measure<'_>,
) -> GraphCommitDetail {
    let body_font = GRAPH_BODY_FONT_LOGICAL_PX * scale;
    // **[`crate::tooltip::wrap`] and not the preview's `WrapLayout`**, and the
    // reason is what each of the two is for: `WrapLayout` wraps a *document* —
    // it owns byte offsets, a caret, a selection and a mapping back to the
    // buffer, none of which a line of prose in a list row has or wants — while
    // `wrap` is the one this product already uses for every multi-line piece of
    // *chrome* text, takes exactly the measure closure this build is holding,
    // and treats a `\n` as a hard break, which is precisely what "keep the
    // body's own paragraph breaks" means.
    //
    // Fed **one paragraph at a time**, which is not a different wrap — `wrap`
    // splits on `\n` itself and would produce the same lines — but a bound on
    // the work: this runs on every frame the row is open, and a body is capped
    // at twelve lines, so wrapping the fortieth paragraph of a release note to
    // then throw it away is measuring text nobody will ever be shown.
    //
    // A commit with nothing after its subject has **no block**, which is not the
    // same as a block with one empty line in it — and `"".split('\n')` is one
    // empty paragraph, so the emptiness is checked before the walk rather than
    // discovered inside it.
    let mut body: Vec<String> = Vec::new();
    if !commit.body.is_empty() {
        for paragraph in commit.body.split('\n') {
            if body.len() > GRAPH_BODY_MAX_LINES {
                break;
            }
            body.extend(crate::tooltip::wrap(paragraph, room, |text| {
                measure(text, body_font)
            }));
        }
    }
    if body.len() > GRAPH_BODY_MAX_LINES {
        body.truncate(GRAPH_BODY_MAX_LINES);
        // The last line says it stopped. Appended rather than replacing the
        // line's tail, because a line that already fitted plus one character is
        // over the bound by one character — which is a hair of overhang at the
        // right margin, and the alternative is cutting a word to make room for
        // the mark that says a word was cut.
        if let Some(last) = body.last_mut() {
            last.push_str(GRAPH_BODY_ELLIPSIS);
        }
    }
    let mut meta = author_sentence(commit);
    meta.push_str(GRAPH_META_SEPARATOR);
    meta.push_str(&crate::git::absolute_time(
        commit.committer_unix,
        commit.committer_offset,
    ));
    // **Only when they differ** (D2), and by the pair rather than by the name: a
    // rebase keeps the author's name and takes the address, and two people
    // called the same thing are two people.
    if commit.committer_name != commit.author_name || commit.committer_email != commit.author_email
    {
        meta.push_str(GRAPH_META_SEPARATOR);
        meta.push_str(GRAPH_META_COMMITTED_BY);
        meta.push_str(&committer_sentence(commit));
    }
    if !commit.parents.is_empty() {
        meta.push_str(GRAPH_META_SEPARATOR);
        meta.push_str(GRAPH_META_PARENTS);
    }
    let meta_font = GRAPH_META_FONT_LOGICAL_PX * scale;
    let meta_width = measure(&meta, meta_font);
    let parents = commit
        .parents
        .iter()
        .map(|parent| {
            let short = short_hash(parent);
            GraphParentChip {
                width: measure(&short, meta_font),
                short,
                hash: parent.clone(),
            }
        })
        .collect();
    GraphCommitDetail {
        hash: commit.hash.clone(),
        short: commit.short.clone(),
        subject: commit.subject.clone(),
        body,
        meta,
        meta_width,
        parents,
    }
}

/// How many characters of a full hash a parent chip shows.
///
/// git's own floor, and the reason it is a cut here rather than git's own
/// abbreviation is that `%P` hands back full hashes: asking git to shorten each
/// of them would be a `rev-parse` per parent per expansion, which is a
/// subprocess spent on seven characters somebody may never look at.
const GRAPH_PARENT_SHORT: usize = 7;

fn short_hash(hash: &str) -> String {
    hash.chars().take(GRAPH_PARENT_SHORT).collect()
}

/// `Comparing abc1234 \u{2192} def5678`, older on the left (D6).
///
/// The arrow points the way history runs, which is the only direction a reader
/// can be expected to read it in: what the diff *says* is how you get from the
/// left to the right.
fn compare_sentence(a: &str, b: Option<&str>, log: &crate::git::GitLog) -> String {
    let named = |hash: &str| {
        log.commits
            .iter()
            .find(|commit| commit.hash == hash)
            .map_or_else(|| short_hash(hash), |commit| commit.short.clone())
    };
    let right = b.map_or_else(|| GRAPH_COMPARE_WORKING_TREE.to_owned(), &named);
    format!(
        "{GRAPH_COMPARE_LEAD}{}{GRAPH_COMPARE_ARROW}{right}",
        named(a)
    )
}

/// How this product names the person who committed one: `Name <email>`.
#[must_use]
pub fn committer_sentence(commit: &GitCommit) -> String {
    if commit.committer_email.is_empty() {
        commit.committer_name.clone()
    } else {
        format!("{} <{}>", commit.committer_name, commit.committer_email)
    }
}

/// What a commit's file row says when you rest on it.
fn file_tooltip(entry: &crate::git::GitCommitFile) -> String {
    let mut text = match &entry.renamed_from {
        Some(from) => format!("{} - renamed from {from}", entry.path),
        None => entry.path.clone(),
    };
    // The counts again, in words, because the row draws them as two numbers and
    // two numbers side by side do not say which is which.
    match entry.stat {
        Some(stat) => {
            let added = if stat.added == 1 { "line" } else { "lines" };
            let removed = if stat.removed == 1 { "line" } else { "lines" };
            text.push_str(&format!(
                "\n{} {added} added, {} {removed} removed",
                stat.added, stat.removed
            ));
        }
        None => text.push_str("\nBinary - git has no lines to count here"),
    }
    text
}

/// One working-tree file under the Uncommitted Changes row (V5).
///
/// The path and where a rename came from travel as a pair because they are one
/// fact about one row and git needs both halves of it to answer about a rename
/// (`crate::git::GitQuestion::Diff::renamed_from`).
struct WorkingFile {
    path: (String, Option<String>),
    badges: Vec<crate::git_panel::GitBadge>,
    staged: bool,
}

/// What the working tree unfolds into: **staged, then changed, then untracked**.
///
/// The Git page's own three groups in the Git page's own order (R6/R11), read
/// off the same status and through the same [`crate::git_panel::badges_of`], so
/// a file drawn here and the same file drawn in a column wear the same letters.
/// A file that is both staged and changed appears twice, which is R11 and is
/// honest: two things happened to it, and only one of them is in the index.
fn working_files(cache: &crate::git::GitCache) -> Vec<WorkingFile> {
    let Some(status) = cache.status().ready() else {
        return Vec::new();
    };
    [
        crate::git::GitGroup::Staged,
        crate::git::GitGroup::Changes,
        crate::git::GitGroup::Untracked,
    ]
    .into_iter()
    .flat_map(|group| {
        status.group(group).map(move |entry| WorkingFile {
            path: (entry.path.clone(), entry.renamed_from.clone()),
            badges: crate::git_panel::badges_of(entry),
            staged: group == crate::git::GitGroup::Staged,
        })
    })
    .collect()
}

/// What the Uncommitted Changes row says when you rest on it.
fn uncommitted_tooltip(count: usize) -> String {
    let files = if count == 1 { "file" } else { "files" };
    format!(
        "{GRAPH_UNCOMMITTED} - {count} {files} the working tree has something to say about\nClick to list them"
    )
}

/// How this product names the person who wrote a commit: `Name <email>` (V4).
///
/// git's own spelling, and one function for both surfaces — the graph's tooltip
/// and the Git page's — because a commit describing its author two ways on one
/// screen is exactly the disagreement §7's CLI ruling picked the CLI to prevent.
/// A commit with no address (git allows one, and some import tools write them)
/// gets the bare name rather than an empty pair of brackets.
#[must_use]
pub fn author_sentence(commit: &GitCommit) -> String {
    if commit.author_email.is_empty() {
        commit.author_name.clone()
    } else {
        format!("{} <{}>", commit.author_name, commit.author_email)
    }
}

/// The tooltip a commit row carries — the author, and the merge's own line.
///
/// The name is in the row since v2 ① and the **address** is what the tooltip now
/// adds (V4): a tooltip that repeated only what the column already says would be
/// a tooltip nobody would open twice.
fn commit_tooltip(commit: &GitCommit) -> String {
    if commit.parents.len() > 1 {
        format!(
            "Merge commit - another branch's history joins here\n{}\n{}\nDouble-click to check this commit out",
            commit.subject,
            author_sentence(commit)
        )
    } else {
        format!(
            "{}\n{}\nDouble-click to check this commit out",
            commit.subject,
            author_sentence(commit)
        )
    }
}

/// What stands on one row of the list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphItem {
    /// The Uncommitted Changes row (V5) — always row zero when it exists.
    Uncommitted,
    /// A file the working tree has something to say about, under that row.
    Working(usize),
    /// The commit at this index of the log.
    Commit(usize),
    /// One of the rows the detail block claims (v2 ②).
    Detail,
    /// A file of the one open commit.
    File { commit: usize, file: usize },
}

/// Where the one open expansion is, and what it unfolded into.
///
/// A named triple since v2 ②, where the expansion stopped being one list: the
/// detail block and the files are two kinds of thing at one place, and three
/// bare `usize`s in a tuple is three chances for a caller to hand them over in
/// the wrong order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GraphOpen {
    /// Where the expansion hangs, **in commit indices** — a fact about the log,
    /// which [`item_at`] translates into list coordinates rather than the caller.
    pub at: usize,
    /// How many rows the detail block claims (zero when there is none).
    pub detail: usize,
    /// How many file rows follow it.
    pub files: usize,
}

impl GraphOpen {
    /// How many rows the whole expansion adds to the list.
    #[must_use]
    pub fn rows(self) -> usize {
        self.detail + self.files
    }
}

/// Which item row `index` is, given the two things that can unfold above it.
///
/// **Arithmetic** (R23): one virtual row at a known place and one expansion at
/// another, so the whole mapping is five comparisons. A `Vec` of rows would be a
/// list the length of the repository, built to answer a question about
/// twenty-five of them.
///
/// `head` is `Some(files)` when the working tree's row is drawn, `files` being
/// how many of its own rows it has unfolded (zero while collapsed); `open` is
/// where a commit's accordion stands, **in commit indices** — it is a fact about
/// the log, and translating it into list coordinates is this function's job
/// rather than the caller's.
#[must_use]
pub fn item_at(index: usize, head: Option<usize>, open: Option<GraphOpen>) -> Option<GraphItem> {
    let index = match head {
        Some(files) => {
            if index == 0 {
                return Some(GraphItem::Uncommitted);
            }
            if index <= files {
                return Some(GraphItem::Working(index - 1));
            }
            index - 1 - files
        }
        None => index,
    };
    let Some(open) = open else {
        return Some(GraphItem::Commit(index));
    };
    if index <= open.at {
        return Some(GraphItem::Commit(index));
    }
    // The detail block first, then the files: that is the order they are drawn
    // in, and the order is the ruling — the story of a commit stands above the
    // list of what it did, because the list is the *evidence* for the story.
    if index <= open.at + open.detail {
        return Some(GraphItem::Detail);
    }
    if index <= open.at + open.rows() {
        return Some(GraphItem::File {
            commit: open.at,
            file: index - open.at - open.detail - 1,
        });
    }
    Some(GraphItem::Commit(index - open.rows()))
}

/// What a press on a graph row does.
#[must_use]
pub fn row_open(
    row: &GraphViewRow,
    root: &std::path::Path,
) -> Option<crate::git_panel::GitRowOpen> {
    match row {
        // The working tree turns over exactly as a commit does — one accordion,
        // one gesture, and [`GRAPH_UNCOMMITTED_HASH`] is what it is keyed on.
        GraphViewRow::Uncommitted(_) => Some(crate::git_panel::GitRowOpen::Expand {
            hash: GRAPH_UNCOMMITTED_HASH.to_owned(),
        }),
        GraphViewRow::Commit(commit) => Some(crate::git_panel::GitRowOpen::Expand {
            hash: commit.hash.clone(),
        }),
        // **A working-tree file opens a working-tree diff** (V5), through R25's
        // one mapping: the staged group is a claim about the index and asks
        // `--cached`, every other group is about the tree. A `git show` of `*`
        // would be a question about a commit that does not exist.
        GraphViewRow::File(file) => Some(crate::git_panel::GitRowOpen::Document {
            source: match (&file.working, &file.range) {
                (Some(working), _) => crate::preview::PreviewSource::GitDiff {
                    root: root.to_owned(),
                    path: file.path.clone(),
                    staged: working.staged,
                },
                // **A comparison's file opens the comparison's diff** (D6), and
                // not this commit's: the row is a claim about what is different
                // between two places, so pressing it has to show that difference
                // and never one end of it.
                (None, Some((a, b))) => crate::preview::PreviewSource::GitDiffRange {
                    root: root.to_owned(),
                    a: a.clone(),
                    b: b.clone(),
                    path: file.path.clone(),
                },
                (None, None) => crate::preview::PreviewSource::GitShow {
                    root: root.to_owned(),
                    hash: file.hash.clone(),
                    path: file.path.clone(),
                },
            },
            name: crate::git_panel::git_document_name(&file.path),
            renamed_from: file.renamed_from.clone(),
        }),
        // **The block is not a row and has no press of its own** — its parts
        // have (see [`detail_part_at`]), and a press that missed all of them
        // landed on the padding between them.
        GraphViewRow::Detail(_) => None,
    }
}

/// What a **double** press on a graph row does (R23): stand on that commit.
///
/// A detached checkout, and it goes through the same door a branch row's does —
/// no gate, git's own refusal if the tree is dirty, and no way past it from
/// here (R10).
#[must_use]
pub fn row_double_open(row: &GraphViewRow) -> Option<crate::git_panel::GitRowOpen> {
    match row {
        GraphViewRow::Commit(commit) => Some(crate::git_panel::GitRowOpen::Checkout {
            target: commit.hash.clone(),
            detach: true,
        }),
        // A file row's second click is its first click again: opening the same
        // document twice is asking for it again, which is what pressing a row
        // that is already open has always meant here.
        GraphViewRow::File(_) => None,
        // **You cannot check out the working tree** — you are standing in it.
        // The second click is the first click again, which folds the row back.
        GraphViewRow::Uncommitted(_) => None,
        GraphViewRow::Detail(_) => None,
    }
}

// ── the detail block's own geometry (v2 ②) ─────────────────────────────────

/// Where everything inside a detail block stands.
///
/// **One function for the paint and for the hit test** — [`graph_column_rects`]'s
/// own reason, and here it is what makes "the `\u{d7}` you can press is the
/// `\u{d7}` you can see" a property of this module rather than something
/// somebody has to check on a screenshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphDetailLayout {
    /// One box per wrapped line of body, top to bottom.
    pub body: Vec<[f32; 4]>,
    /// The meta line — or, in compare mode, the comparison's own sentence.
    pub meta: [f32; 4],
    /// The parent hashes standing on the meta line, in git's order.
    pub parents: Vec<[f32; 4]>,
    /// The hover verbs at the line's right, right to left as they are laid out.
    pub tools: Vec<([f32; 4], GraphDetailPart)>,
}

/// Lay one detail block out inside the rectangle it claims.
///
/// `rect` is the block's **whole** box — every row it spans, joined — which is
/// what the painter has and what the hit test can rebuild from
/// [`GraphGeometry::row_rect`] and [`GraphDetailRow::rows`].
#[must_use]
pub fn detail_layout(
    rect: [f32; 4],
    row: &GraphDetailRow,
    columns: GraphColumns,
    scale: f32,
) -> GraphDetailLayout {
    let pad = (GRAPH_DETAIL_PADDING_Y_LOGICAL_PX * scale).round();
    let indent = (GRAPH_FILE_INDENT_LOGICAL_PX * scale).round();
    let left = rect[0] + indent;
    // The right edge is the description column's, so the block ends where every
    // message on the page ends rather than under the hash column.
    let right = graph_column_rects(rect, columns, scale)
        .description_right
        .max(left);
    let line = (GRAPH_BODY_LINE_LOGICAL_PX * scale).round().max(1.0);
    let gap = (GRAPH_DETAIL_GAP_LOGICAL_PX * scale).round();
    let meta_height = detail_meta_height(scale);
    let mut layout = GraphDetailLayout::default();
    let mut top = rect[1] + pad;
    if let GraphDetail::Commit(commit) = &row.detail {
        for _ in &commit.body {
            layout.body.push([left, top, right, top + line]);
            top += line;
        }
        if !commit.body.is_empty() {
            top += gap;
        }
    }
    layout.meta = [left, top, right, top + meta_height];

    // The verbs, from the right edge inwards — the order they are pinned in, so
    // that a block with one of them and a block with two put the first one in
    // the same place.
    let act = (crate::git_panel::GIT_ACT_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    let act_gap = (crate::git_panel::GIT_ACT_GAP_LOGICAL_PX * scale).round();
    let act_top = ((layout.meta[1] + layout.meta[3] - act) / 2.0).round();
    let mut cursor = right;
    let mut verb = |part: GraphDetailPart, layout: &mut GraphDetailLayout| {
        let box_ = [cursor - act, act_top, cursor, act_top + act];
        cursor = box_[0] - act_gap;
        layout.tools.push((box_, part));
    };
    match &row.detail {
        GraphDetail::Commit(commit) => {
            verb(GraphDetailPart::CopySubject, &mut layout);
            verb(GraphDetailPart::CopyHash, &mut layout);
            // The parents, immediately after the text that names them — which is
            // why the meta string is measured at build time and carried here.
            let chip_gap = (GRAPH_PARENT_GAP_LOGICAL_PX * scale).round();
            let mut chip_left = left + commit.meta_width;
            for chip in &commit.parents {
                let box_ = [
                    chip_left,
                    layout.meta[1],
                    chip_left + chip.width,
                    layout.meta[3],
                ];
                chip_left = box_[2] + chip_gap;
                layout.parents.push(box_);
            }
        }
        GraphDetail::Compare(_) => verb(GraphDetailPart::LeaveCompare, &mut layout),
    }
    layout
}

/// Which part of a detail block the pointer is on, if any.
#[must_use]
pub fn detail_part_at(
    rect: [f32; 4],
    row: &GraphDetailRow,
    columns: GraphColumns,
    scale: f32,
    x: f32,
    y: f32,
) -> Option<GraphDetailPart> {
    let layout = detail_layout(rect, row, columns, scale);
    let inside = |box_: [f32; 4]| x >= box_[0] && x < box_[2] && y >= box_[1] && y < box_[3];
    // **The verbs first**, because they stand *over* the meta line and a parent
    // chip that answered a press aimed at the copy button would be the block
    // seeking to a commit somebody asked to have on their clipboard.
    if let Some((_, part)) = layout.tools.iter().find(|(box_, _)| inside(*box_)) {
        return Some(*part);
    }
    layout
        .parents
        .iter()
        .position(|box_| inside(*box_))
        .map(GraphDetailPart::Parent)
}

// ── the seek (D2) ──────────────────────────────────────────────────────────

/// A walk towards a commit that may not be loaded yet.
///
/// **A state and not a loop**, because the thing it is waiting for arrives on
/// another thread: each page it asks for comes back as an answer to the window,
/// and the next question can only be decided once it has. So the seek is a
/// couple of fields the seat carries and one function that says what to do next
/// — which is also what makes the rule assertable without a window, a worker or
/// a repository.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphSeek {
    /// The full hash being looked for.
    pub hash: String,
    /// How many pages have been asked for on its account.
    pub pages: usize,
}

/// What a seek should do next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphSeekStep {
    /// It is here, at this commit index — reveal it, select it, and stop.
    Arrived(usize),
    /// Not yet, and there is more history: ask for another page.
    NeedPage,
    /// The repository has not said anything yet — a page is already on its way,
    /// so the seek costs nothing and asks for nothing.
    ///
    /// Its own answer and not `NeedPage`, because the two differ in the one
    /// thing that matters here: this one must **not** count against the cap, and
    /// a seek that spent its twenty pages waiting for the first one would give
    /// up on a commit it had never looked for.
    Waiting,
    /// Not here, and there is no honest way to keep looking — say so and stop.
    GaveUp,
}

/// One step of a seek, given what the repository has answered so far (D2).
///
/// The three answers are decided in this order and the order is the ruling: a
/// commit that is *here* is arrived at even on the last allowed page, because
/// the cap is a bound on how long we look and not on when we stop believing
/// what we found.
#[must_use]
pub fn graph_seek_step(state: &GraphState, seek: &GraphSeek) -> GraphSeekStep {
    let Some(log) = state.cache.log().ready() else {
        return GraphSeekStep::Waiting;
    };
    if let Some(at) = log
        .commits
        .iter()
        .position(|commit| commit.hash == seek.hash)
    {
        return GraphSeekStep::Arrived(at);
    }
    if log.has_more && seek.pages < GRAPH_SEEK_MAX_PAGES {
        return GraphSeekStep::NeedPage;
    }
    GraphSeekStep::GaveUp
}

/// How much of what was copied the notice repeats before it elides (D7).
///
/// A short hash is seven characters and every subject this window has room to
/// draw is longer; forty is about where a subject stops being a name and starts
/// being a sentence, and a card that repeated a whole release note back would be
/// a notice you had to read to learn something you already knew.
pub const GRAPH_COPIED_MAX_CHARS: usize = 40;

/// What the window says when something has gone on the clipboard (D7).
#[must_use]
pub fn graph_copied(said: &str) -> String {
    let mut short: String = said.chars().take(GRAPH_COPIED_MAX_CHARS).collect();
    if said.chars().count() > GRAPH_COPIED_MAX_CHARS {
        short.push_str(GRAPH_BODY_ELLIPSIS);
    }
    format!("Copied {short}")
}

/// What the window says when a seek runs out of history (D2).
///
/// A notice and not a refusal: nothing failed, and nothing the reader did was
/// wrong — the commit is simply further back than this window has read, which is
/// a fact about the reading rather than about the repository.
#[must_use]
pub fn graph_seek_gave_up(hash: &str) -> String {
    format!(
        "Commit {} is further back than the loaded history",
        short_hash(hash)
    )
}

// ── the keyboard (V14) ─────────────────────────────────────────────────────

/// The six keys a focused graph answers (V14).
///
/// An enum and not the winit key, for [`crate::files::TreeCommand`]'s reason: the
/// rule about what `↓` does at the end of a loaded page is a property of the
/// list, and a property of the list should be assertable without a window, a
/// keyboard layout or a modifier state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphKey {
    Up,
    Down,
    Home,
    End,
    Enter,
    /// `Ctrl+Enter` — the keyboard's half of D6's `Ctrl`+click.
    ///
    /// The **one** chorded key on this list, and it is here rather than in
    /// `shortcuts::BINDINGS` for the same reason its five neighbours are: it
    /// only exists while a graph holds the keyboard, and what it does is a
    /// property of the list. The table is the registry of chords this window
    /// *claims from the shell*, and there is no shell listening behind a focused
    /// preview seat.
    Compare,
    Escape,
}

/// What one of those keys does to a graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphKeyAction {
    /// The key was the graph's, and it changed nothing.
    ///
    /// **Still the graph's**: a focused preview owns every key it is offered
    /// (`preview_browse_key`'s own `_ => Ok(true)`), so this is "nothing moved",
    /// not "pass it on".
    None,
    /// Put the selection on this row and scroll it into view.
    Select(usize),
    /// Turn this row over — the same verb a press on it is.
    Toggle(usize),
    /// Fold the open row shut and stand on it.
    Collapse(usize),
    /// Compare the open row with this one (D6) — or, when this row is already
    /// the far end, stop comparing.
    Compare(usize),
    /// Stop comparing, keeping the row that was open, open.
    ///
    /// `Esc`'s **first** meaning while a comparison is running, which is the
    /// ladder every dismissible thing on this platform has: the innermost thing
    /// goes first, and the accordion is still there behind it.
    LeaveCompare,
    /// **The one key this surface does not claim.** `Esc` with nothing to
    /// collapse belongs to the layer under this one — the float dismissal, and
    /// under that the shell — and eating it here would be this page holding a
    /// key it has no use for.
    Pass,
}

/// What one key does to a graph, given the list it is looking at (V14).
///
/// The auto-paging is deliberately **not** here: `↓` at the last loaded row
/// selects that row and scrolls to it, and the next build then sees a window
/// within [`GRAPH_PREFETCH_ROWS`] of the end and asks for the next page — R23's
/// own rule, unchanged, doing the same job for a keyboard that it already does
/// for a wheel. A second paging rule written for the keyboard would be a second
/// place for "how near the end is near enough" to be decided.
#[must_use]
pub fn graph_key(content: &GraphContent, key: GraphKey) -> GraphKeyAction {
    let total = content.total_rows;
    if total == 0 {
        // An empty page has no rows to walk and nothing open, so `Esc` is not
        // its either.
        return match key {
            GraphKey::Escape => GraphKeyAction::Pass,
            _ => GraphKeyAction::None,
        };
    }
    let last = total - 1;
    match key {
        // With nothing selected, the first press lands on the top row rather
        // than on the bottom one: the reader is looking at the top of a list
        // they have just given the keyboard to, and a selection that appeared
        // ten thousand rows away would be the page answering a different
        // question.
        GraphKey::Up => GraphKeyAction::Select(step_over_detail(
            content,
            content
                .selected
                .map_or(0, |row| row.min(last).saturating_sub(1)),
            false,
        )),
        GraphKey::Down => GraphKeyAction::Select(step_over_detail(
            content,
            content.selected.map_or(0, |row| (row + 1).min(last)),
            true,
        )),
        GraphKey::Home => GraphKeyAction::Select(0),
        GraphKey::End => GraphKeyAction::Select(last),
        GraphKey::Enter => match content.selected {
            Some(row) if row <= last => GraphKeyAction::Toggle(row),
            _ => GraphKeyAction::None,
        },
        // **Only with something already open**, because a comparison is a
        // property of the open row: `Ctrl+Enter` with nothing turned over has no
        // first end to be the far end of.
        GraphKey::Compare => match (content.selected, content.open_rows) {
            (Some(row), Some(_)) if row <= last => GraphKeyAction::Compare(row),
            _ => GraphKeyAction::None,
        },
        GraphKey::Escape => match (content.compare_rows, content.open_rows) {
            (Some(_), _) => GraphKeyAction::LeaveCompare,
            (None, Some((row, _))) => GraphKeyAction::Collapse(row),
            (None, None) => GraphKeyAction::Pass,
        },
    }
}

/// Push a row number past the detail block, in the direction it was travelling.
///
/// The block claims list rows so that the geometry stays a multiplication, and
/// this is the price of that: those rows answer no press, so the selection may
/// not stand on one. Walking *through* rather than jumping over the whole block
/// would be the same thing said worse — the reader pressed `↓` once and would
/// have to press it five more times to leave a paragraph.
///
/// A row past the end of the list is clamped back, which is the case where the
/// block is the last thing on the page.
fn step_over_detail(content: &GraphContent, row: usize, downwards: bool) -> usize {
    let Some((start, rows)) = content.detail_rows else {
        return row;
    };
    if row < start || row >= start + rows {
        return row;
    }
    if downwards {
        (start + rows).min(content.total_rows.saturating_sub(1))
    } else {
        start.saturating_sub(1)
    }
}

// ── the paint ──────────────────────────────────────────────────────────────

/// What the pointer is on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GraphHover {
    pub row: Option<usize>,
    /// Which part of the detail block the pointer is on, when it is on one —
    /// what lights a hover verb's pill (R12's top rung).
    pub part: Option<GraphDetailPart>,
}

/// Draw one commit graph.
///
/// **Chrome and not a preview body**, and the reason is geometric rather than
/// architectural: a `PreviewBody` is axis-aligned rectangles and text runs, and
/// this picture is made of circles and curves. The chrome layer already draws
/// both — the Git panel's mini graph is the proof — so the graph is built the
/// way that panel is built, in the preview seat's own body rectangle, and the
/// document's body is left empty because the document *is* this.
#[allow(clippy::too_many_arguments)]
pub fn push_graph(
    body: [f32; 4],
    content: &GraphContent,
    hover: GraphHover,
    scale: f32,
    palette: &ChromePalette,
    out: (
        &mut Vec<ChromeQuad>,
        &mut Vec<ChromeLabel>,
        &mut Vec<ChromeSprite>,
    ),
) {
    let (quads, labels, sprites) = out;
    let geometry = graph_geometry(body, content, scale);
    let _ = quads;

    if let (Some(rect), Some(head)) = (geometry.head, content.head.as_ref()) {
        crate::git_panel::push_git_masthead(head, rect, scale, palette, (labels, sprites), &|r| r);
    }

    if let Some(sentence) = content.empty.as_ref() {
        labels.push(ChromeLabel {
            text: sentence.clone(),
            rect: geometry.viewport,
            font_size_px: crate::git_panel::GIT_EMPTY_FONT_LOGICAL_PX * scale,
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

    if let Some(rect) = geometry.header {
        push_column_header(
            rect,
            content.columns,
            content.lane_width,
            scale,
            palette,
            labels,
        );
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

    for row in &content.rows {
        // A row's box is one row tall; the detail block's is every row it
        // claims, joined — which is the one place the uniform arithmetic has to
        // be asked twice.
        let rect = {
            let first = geometry.row_rect(row.index());
            let last = geometry.row_rect(row.index() + row.rows() - 1);
            [first[0], first[1], first[2], last[3]]
        };
        if !visible(rect) {
            continue;
        }
        let hovered = hover.row == Some(row.index());
        // **Selected outranks hovered** (V8), because they answer two different
        // questions and only one of them is about where the pointer happens to
        // be: the selection is where the *keyboard* is, and a row that dimmed
        // back to a hover ground when the pointer wandered onto it would be the
        // page forgetting.
        //
        // **Both ends of a comparison are selected** (D6): the pair is what the
        // block under them is about, and a page where only one of the two rows
        // was lit would be asking the reader to remember the other one.
        let selected = content.selected == Some(row.index())
            || content
                .compare_rows
                .is_some_and(|(a, b)| a == row.index() || b == row.index());
        let ground = RowGround {
            selected,
            hovered,
            scale,
        };
        match row {
            GraphViewRow::Uncommitted(head) => {
                push_row_ground(rect, ground, palette, sprites, &crop);
                push_uncommitted_row(
                    head,
                    rect,
                    content.lane_width,
                    content.columns,
                    ground,
                    palette,
                    (labels, sprites),
                    &crop,
                );
            }
            GraphViewRow::Commit(commit) => {
                push_row_ground(rect, ground, palette, sprites, &crop);
                push_commit_row(
                    commit,
                    rect,
                    content.lane_width,
                    content.columns,
                    ground,
                    palette,
                    (labels, sprites),
                    &crop,
                );
            }
            GraphViewRow::File(file) => {
                push_row_ground(rect, ground, palette, sprites, &crop);
                push_file_row(
                    file,
                    rect,
                    content.columns,
                    ground,
                    palette,
                    (labels, sprites),
                    &crop,
                );
            }
            // **No ground of its own.** The block belongs to the row above it and
            // a fill under it would cut that row off from what it opened; what
            // separates it from the list is the indent and the ink, exactly as
            // for the file rows under it.
            GraphViewRow::Detail(detail) => push_detail(
                detail,
                rect,
                content.columns,
                hover,
                ground,
                palette,
                (labels, sprites),
                &crop,
            ),
        }
    }
}

/// Which of a row's three grounds it is standing on, and at what scale.
///
/// One value rather than two bools and a float at five call sites, for
/// [`crate::git::GitStatusEntry`]'s reason: the pair that decides a ground and
/// the pair that decides an ink are the same pair, and passing them separately
/// is how one painter eventually reads them in the other order.
#[derive(Clone, Copy, Debug)]
struct RowGround {
    selected: bool,
    hovered: bool,
    scale: f32,
}

impl RowGround {
    /// The message ink for this ground.
    fn text(self, palette: &ChromePalette) -> [u8; 3] {
        if self.selected {
            palette.files_row_text_selected
        } else if self.hovered {
            palette.files_row_text_hover
        } else {
            palette.files_row_text
        }
    }

    /// The quiet ink — the author, the date, the hash, a file's path.
    fn muted(self, palette: &ChromePalette) -> [u8; 3] {
        if self.selected {
            palette.files_row_muted_selected
        } else if self.hovered {
            palette.files_row_muted_hover
        } else {
            palette.files_row_muted
        }
    }

    /// What the row itself is filled with, for the badges to composite over.
    fn fill(self, palette: &ChromePalette) -> Option<[u8; 3]> {
        if self.selected {
            Some(palette.git_row_selected)
        } else if self.hovered {
            Some(palette.files_row_hover)
        } else {
            None
        }
    }
}

/// The column header (V2): five words in `.glabel` grammar, each over its own
/// column.
///
/// Its ink is the Git page's own heading ink — `git_head_muted`, which is
/// `--ink3` over the pane body, and is the ink every `.glabel` in this product
/// already wears. There is no separate `git_label` token to reach for and there
/// should not be: a heading over a graph column and a heading over a change
/// group are the same kind of word about the same kind of thing.
///
/// A collapsed column has no header, which falls out of the layout rather than
/// being checked here — [`graph_column_rects`] hands back `None` for it, and a
/// word with nowhere to stand is a word that is not drawn.
fn push_column_header(
    rect: [f32; 4],
    columns: GraphColumns,
    lane_width: usize,
    scale: f32,
    palette: &ChromePalette,
    labels: &mut Vec<ChromeLabel>,
) {
    let rects = graph_column_rects(rect, columns, scale);
    let pad = (GRAPH_ROW_PADDING_X_LOGICAL_PX * scale).round();
    let gap = (GRAPH_ROW_GAP_LOGICAL_PX * scale).round();
    // The graph column's right edge, by the rows' own arithmetic (`push_lanes`):
    // the lanes on screen, at least one, each a lane wide.
    #[allow(clippy::cast_precision_loss)]
    let column_right = rect[0]
        + pad
        + (GRAPH_LANE_WIDTH_LOGICAL_PX * scale).round().max(1.0) * lane_width.max(1) as f32;
    let bottom_pad = (crate::git_panel::GIT_LABEL_PADDING_BOTTOM_LOGICAL_PX * scale).round();
    let line = (crate::git_panel::GIT_LABEL_LINE_LOGICAL_PX * scale).round();
    // The words sit on the *bottom* of the strip, as every `.glabel` does: the
    // padding above a heading is the gap between it and what came before, not
    // leading.
    let text_box = |box_: [f32; 4]| {
        [
            box_[0],
            rect[3] - bottom_pad - line,
            box_[2],
            rect[3] - bottom_pad,
        ]
    };
    let mut word = |text: &str, box_: [f32; 4], align_right: bool| {
        let box_ = text_box(box_);
        labels.push(ChromeLabel {
            text: text.to_owned(),
            rect: box_,
            font_size_px: crate::git_panel::GIT_LABEL_FONT_LOGICAL_PX * scale,
            color: palette.git_head_muted,
            align_right,
            align_center: false,
            letter_spacing_em: crate::git_panel::GIT_LABEL_TRACKING_EM,
            weight: ChromeLabelWeight::SemiBold,
            tabular_numerals: false,
            clip: Some(box_),
        });
    };
    // **Only when the column can hold the word** (real machine, 2026-08-16): a
    // one-lane graph is sixteen pixels wide and GRAPH is not, and drawn anyway
    // it ran into DESCRIPTION and the two read as one smeared word. A heading
    // is a word over a column, so a column too narrow for the word gets no
    // heading — the lanes say "graph" well enough on their own.
    if column_right - (rect[0] + pad) >= (GRAPH_HEADING_GRAPH_MIN_LOGICAL_PX * scale).round() {
        word(
            GRAPH_HEADING_GRAPH,
            [rect[0] + pad, rect[1], column_right, rect[3]],
            false,
        );
    }
    if let Some(box_) = rects.author {
        word(GRAPH_HEADING_AUTHOR, box_, true);
    }
    if let Some(box_) = rects.date {
        word(GRAPH_HEADING_DATE, box_, true);
    }
    if let Some(box_) = rects.hash {
        word(GRAPH_HEADING_COMMIT, box_, true);
    }
    // Last, because where the description *starts* is the one column edge that
    // is not fixed: it begins after the graph column, which is as wide as the
    // lanes on screen need it to be — and the heading starts exactly where
    // every row's own text starts (`column_right + gap` in `push_commit_row`),
    // so it stands over its column the way the other four do. The lane count
    // is already held steady across a scroll by R18's hysteresis, and when it
    // does change the rows' text moves with it; a heading that stayed put would
    // then be the one thing on the page not over what it names.
    word(
        GRAPH_HEADING_DESCRIPTION,
        [
            column_right + gap,
            rect[1],
            rects.description_right.max(column_right + gap),
            rect[3],
        ],
        false,
    );
}

/// One file row — a commit's with its letter and its counts (D4), or the
/// working tree's with its two letters (V5).
#[allow(clippy::too_many_arguments)]
fn push_file_row(
    file: &GraphFileRow,
    rect: [f32; 4],
    columns: GraphColumns,
    ground: RowGround,
    palette: &ChromePalette,
    out: (&mut Vec<ChromeLabel>, &mut Vec<ChromeSprite>),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (labels, sprites) = out;
    let scale = ground.scale;
    let indent = (GRAPH_FILE_INDENT_LOGICAL_PX * scale).round();
    let mut left = rect[0] + indent;
    // **One badge for a commit's file and two for the working tree's**, which is
    // the honest difference between the two rows: a status entry has an index
    // column and a working-tree column, and a commit is a point with one story
    // about each file. The drawing is identical, so it is one loop over whichever
    // list this row has.
    let badges: Vec<crate::git_panel::GitBadge> = match (&file.working, &file.badge) {
        (Some(working), _) => working.badges.clone(),
        (None, Some(badge)) => vec![*badge],
        (None, None) => Vec::new(),
    };
    {
        // The Git page's badge, byte for byte: same size, same radius, same
        // fifteen-percent ground under the same letter. It is composited over
        // whatever *this* row is standing on, which is the pane body here and a
        // card there — the one thing that could not be shared.
        let badge = (crate::git_panel::GIT_BADGE_LOGICAL_PX * scale)
            .round()
            .max(1.0);
        let badge_gap = (crate::git_panel::GIT_BADGE_GAP_LOGICAL_PX * scale).round();
        let radius = (crate::git_panel::GIT_BADGE_RADIUS_LOGICAL_PX * scale)
            .round()
            .max(1.0) as u32;
        let under = ground.fill(palette).unwrap_or(palette.pane_head);
        let top = ((rect[1] + rect[3] - badge) / 2.0).round();
        for mark in &badges {
            let box_ = [left, top, left + badge, top + badge];
            let ink = mark.ink.colour(palette);
            sprites.push(ChromeSprite::new(
                ChromeMark::ControlPill { radius_px: radius },
                crop(box_),
                bt_render::ink_over(under, ink, crate::git_panel::GIT_BADGE_GROUND_ALPHA),
            ));
            labels.push(ChromeLabel {
                text: mark.letter.to_string(),
                rect: box_,
                font_size_px: crate::git_panel::GIT_BADGE_FONT_LOGICAL_PX * scale,
                color: ink,
                align_right: false,
                align_center: true,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::SemiBold,
                tabular_numerals: false,
                clip: Some(crop(box_)),
            });
            left = box_[2] + badge_gap;
        }
        if !badges.is_empty() {
            left = left - badge_gap + (GRAPH_ROW_GAP_LOGICAL_PX * scale).round();
        }
    }
    // **The counts stand in the date and hash columns' own space** (D4), which is
    // the whole reason those columns are reserved widths: the numbers line up
    // down the page under `DATE` and `COMMIT` because that is where those columns
    // are, and a file row that measured its own numbers would put every row's in
    // a different place.
    let rects = graph_column_rects(rect, columns, scale);
    let mut right = rects.description_right;
    if file.working.is_none() {
        let stat_font = crate::git_panel::GIT_HASH_FONT_LOGICAL_PX * scale;
        let gap = (GRAPH_ROW_GAP_LOGICAL_PX * scale).round();
        let mut number = |text: String, box_: Option<[f32; 4]>, ink: [u8; 3]| {
            let Some(box_) = box_ else { return };
            labels.push(ChromeLabel {
                text,
                rect: box_,
                font_size_px: stat_font,
                color: ink,
                align_right: true,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                // Tabular, so `+9` and `+11` end on the same pixel — the one
                // thing a column of numbers is for.
                tabular_numerals: true,
                clip: Some(crop(box_)),
            });
            right = box_[0] - gap;
        };
        match file.stat {
            Some(stat) => {
                number(
                    format!("\u{2212}{}", stat.removed),
                    rects.hash,
                    palette.status_err,
                );
                number(format!("+{}", stat.added), rects.date, palette.status_ok);
            }
            // A binary file: git has no lines here, and an em dash is how a
            // table has always said "this column does not apply to this row".
            None => number("\u{2014}".to_owned(), rects.hash, ground.muted(palette)),
        }
    }
    let box_ = [
        left,
        rect[1],
        (right - (GRAPH_ROW_PADDING_X_LOGICAL_PX * scale).round()).max(left),
        rect[3],
    ];
    labels.push(ChromeLabel {
        text: file.path.clone(),
        rect: box_,
        font_size_px: crate::git_panel::GIT_TIME_FONT_LOGICAL_PX * scale,
        color: ground.muted(palette),
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(crop(box_)),
    });
}

/// The detail block: the rest of the message, the facts about it, and the two
/// verbs that copy it (D1/D2/D7) — or the head of a comparison (D6).
#[allow(clippy::too_many_arguments)]
fn push_detail(
    row: &GraphDetailRow,
    rect: [f32; 4],
    columns: GraphColumns,
    hover: GraphHover,
    ground: RowGround,
    palette: &ChromePalette,
    out: (&mut Vec<ChromeLabel>, &mut Vec<ChromeSprite>),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (labels, sprites) = out;
    let scale = ground.scale;
    let layout = detail_layout(rect, row, columns, scale);
    let mut line = |text: String, box_: [f32; 4], font: f32, ink: [u8; 3]| {
        labels.push(ChromeLabel {
            text,
            rect: box_,
            font_size_px: font,
            color: ink,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(crop(box_)),
        });
    };
    let body_font = GRAPH_BODY_FONT_LOGICAL_PX * scale;
    let meta_font = GRAPH_META_FONT_LOGICAL_PX * scale;
    match &row.detail {
        GraphDetail::Commit(commit) => {
            // **The row's own ink and not the muted one** (D1): a commit's body
            // is the commit *talking*, exactly as its subject is, and setting it
            // in the quiet grey the dates and paths wear would file the one piece
            // of writing on this page under furniture.
            for (text, box_) in commit.body.iter().zip(&layout.body) {
                line(text.clone(), *box_, body_font, ground.text(palette));
            }
            line(
                commit.meta.clone(),
                layout.meta,
                meta_font,
                ground.muted(palette),
            );
            // **The parents wear the accent**, because they are the one thing on
            // this line you can press — the same claim the accent makes
            // everywhere else in this window.
            for (chip, box_) in commit.parents.iter().zip(&layout.parents) {
                labels.push(ChromeLabel {
                    text: chip.short.clone(),
                    rect: *box_,
                    font_size_px: meta_font,
                    color: palette.accent,
                    align_right: false,
                    align_center: false,
                    letter_spacing_em: 0.0,
                    weight: ChromeLabelWeight::Regular,
                    tabular_numerals: true,
                    clip: Some(crop(*box_)),
                });
            }
        }
        GraphDetail::Compare(compare) => line(
            compare.head.clone(),
            layout.meta,
            meta_font,
            ground.text(palette),
        ),
    }

    // R12's three rungs again, on `.pv-tool`'s ladder: absent while the pointer
    // is elsewhere on the page, seven-tenths once this block has it, whole over
    // its own pill once the button does.
    let glyph = (crate::git_panel::GIT_ACT_GLYPH_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    let radius = (crate::git_panel::GIT_ACT_RADIUS_LOGICAL_PX * scale)
        .round()
        .max(1.0) as u32;
    let shown = hover.row == Some(row.index);
    for (box_, part) in &layout.tools {
        if !shown {
            continue;
        }
        let lit = hover.part == Some(*part);
        if lit {
            sprites.push(ChromeSprite::new(
                ChromeMark::ControlPill { radius_px: radius },
                crop(*box_),
                palette.files_row_hover,
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
            detail_part_mark(*part),
            crop(glyph_box),
            if lit {
                palette.git_head_text
            } else {
                palette.git_head_muted
            },
        );
        mark.opacity = if lit {
            1.0
        } else {
            crate::git_panel::GIT_ACT_REVEAL
        };
        sprites.push(mark);
    }
}

/// The mark a detail verb wears.
///
/// The hash gets `Code` and the subject gets `Copy`, and the pairing is not
/// arbitrary: both verbs copy, so a glyph that only said "copy" would have to be
/// drawn twice and the reader would have to guess which was which. What
/// distinguishes them is *what* goes on the clipboard — a hexadecimal name, or
/// the sentence — and `< >` against the two-rectangles copy idiom is exactly
/// that difference, in the two marks this product already cut.
fn detail_part_mark(part: GraphDetailPart) -> ChromeMark {
    match part {
        GraphDetailPart::CopyHash => ChromeMark::Code,
        GraphDetailPart::CopySubject => ChromeMark::Copy,
        GraphDetailPart::LeaveCompare => ChromeMark::PaneClose,
        // Never drawn as a glyph — a parent is the text of its own hash.
        GraphDetailPart::Parent(_) => ChromeMark::Code,
    }
}

/// `.ggrow:hover, .ggrow.open { background: var(--hover) }` (G82).
///
/// The tree's `--hover` and not the panel's, because the graph stands on the
/// pane's own body exactly as the tree does — the panel's rows sit on a `--panel`
/// card and their hover is mixed over that.
fn push_row_ground(
    rect: [f32; 4],
    ground: RowGround,
    palette: &ChromePalette,
    sprites: &mut Vec<ChromeSprite>,
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let Some(fill) = ground.fill(palette) else {
        return;
    };
    sprites.push(ChromeSprite::new(
        ChromeMark::ControlPill {
            radius_px: (GRAPH_ROW_RADIUS_LOGICAL_PX * ground.scale)
                .round()
                .max(1.0) as u32,
        },
        crop(rect),
        fill,
    ));
}

/// The picture in the graph column: the roads, and the node standing on one.
///
/// Its own function since v2 ① because two kinds of row draw it — a commit, and
/// the working tree above them all (V5) — and the second differs from the first
/// in exactly one particular: its node is hollow. Everything else about the
/// picture is identical, and it has to be, because it is one road running
/// through both of them.
///
/// Answers where the graph column ends, which is where the description begins.
#[allow(clippy::too_many_arguments)]
fn push_lanes(
    lanes: &GraphRow,
    rect: [f32; 4],
    lane_width: usize,
    hollow: bool,
    scale: f32,
    palette: &ChromePalette,
    sprites: &mut Vec<ChromeSprite>,
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) -> f32 {
    let pad = (GRAPH_ROW_PADDING_X_LOGICAL_PX * scale).round();
    let lane_w = (GRAPH_LANE_WIDTH_LOGICAL_PX * scale).round().max(1.0);
    let stroke = (GRAPH_STROKE_LOGICAL_PX * scale).round().max(1.0);
    let dot = (GRAPH_DOT_RADIUS_LOGICAL_PX * scale).round().max(1.0) * 2.0;
    #[allow(clippy::cast_precision_loss)]
    let column = lane_w * lane_width.max(1) as f32;
    let left = rect[0] + pad;
    let mid = ((rect[1] + rect[3]) / 2.0).round();
    #[allow(clippy::cast_precision_loss)]
    let lane_x = |lane: usize| (left + lane as f32 * lane_w + lane_w / 2.0).round();
    let ink = |lane: usize| palette.graph_lanes[lane_colour_index(lane)];

    // The straight halves. Drawn as fills at the design's own .55 rather than
    // premixed over a ground, because the ground under a graph row is the pane's
    // body on one frame and its hover on the next, and a premix would be right
    // on exactly one of them.
    let mut segment = |lane: usize, top: f32, bottom: f32| {
        let box_ = crop([
            lane_x(lane) - stroke / 2.0,
            top,
            lane_x(lane) + stroke / 2.0,
            bottom,
        ]);
        if box_[3] > box_[1] {
            sprites.push(
                ChromeSprite::new(ChromeMark::Fill, box_, ink(lane))
                    .with_opacity(alpha(GRAPH_LINE_ALPHA)),
            );
        }
    };
    for lane in &lanes.upper {
        segment(*lane, rect[1], mid);
    }
    for lane in &lanes.lower {
        segment(*lane, mid, rect[3]);
    }

    // The curves. One shape, four mirrorings — see [`ChromeMark::GraphCurve`].
    let mut curve = |lane: usize, opening: bool| {
        let dot_x = lane_x(lanes.dot);
        let lane_edge = lane_x(lane);
        let (top, bottom) = if opening {
            (mid, rect[3])
        } else {
            (rect[1], mid)
        };
        let bleed = stroke / 2.0;
        let box_ = [
            dot_x.min(lane_edge) - bleed,
            top - bleed,
            dot_x.max(lane_edge) + bleed,
            bottom + bleed,
        ];
        sprites.push(
            ChromeSprite::new(
                ChromeMark::GraphCurve {
                    stroke_px: stroke as u32,
                    // The dot is at the box's right when the lane is to its left.
                    flip_x: lane < lanes.dot,
                    // A closing edge arrives at the dot from above, so the dot is
                    // at the box's bottom.
                    flip_y: !opening,
                },
                crop(box_),
                ink(lane),
            )
            .with_opacity(alpha(GRAPH_LINE_ALPHA)),
        );
    };
    for lane in &lanes.close {
        curve(*lane, false);
    }
    for lane in &lanes.open {
        curve(*lane, true);
    }

    // The node, at full strength: `.ggcell circle { opacity:1 }` (G78) — it is
    // brighter than the line it stands on, which is the whole of the layering.
    //
    // **Hollow for the working tree** (V5), and the hollowness is the statement:
    // every filled dot on this page is a commit that exists, and what stands at
    // the top of a dirty tree is work that does not exist yet. It borrows
    // `HEAD`'s own lane colour rather than a grey of its own, because it is on
    // `HEAD`'s road — it is the next step along it.
    let dot_x = lane_x(lanes.dot);
    let box_ = crop([
        dot_x - dot / 2.0,
        mid - dot / 2.0,
        dot_x + dot / 2.0,
        mid + dot / 2.0,
    ]);
    let radius_px = (dot / 2.0).round().max(1.0) as u32;
    sprites.push(ChromeSprite::new(
        if hollow {
            ChromeMark::ControlPillRing {
                radius_px,
                stroke_px: (GRAPH_UNCOMMITTED_DOT_STROKE_LOGICAL_PX * scale)
                    .round()
                    .max(1.0) as u32,
            }
        } else {
            ChromeMark::ControlPill { radius_px }
        },
        box_,
        ink(lanes.dot),
    ));
    left + column
}

/// One commit row: the lanes, the names it wears, and R21's columns.
///
/// **Graph / refs / message / author / time / hash** since v2 ① (V1,
/// 2026-08-16). The author joined the row between the message and the age, which
/// is where a column that varies belongs: everything to its right is a fixed
/// width, so the message — the one thing the reader is scanning — is the only
/// column that grows when the pane does.
///
/// R21's ruling that the panel and the graph draw one row in two widths still
/// stands, and the author is where the two now part: the panel's 240-pixel
/// column has no room for it (R16), and this one does. A change to the four
/// columns they *share*, in either surface without the other, re-opens the
/// ticket.
#[allow(clippy::too_many_arguments)]
fn push_commit_row(
    commit: &GraphCommitRow,
    rect: [f32; 4],
    lane_width: usize,
    columns: GraphColumns,
    ground: RowGround,
    palette: &ChromePalette,
    out: (&mut Vec<ChromeLabel>, &mut Vec<ChromeSprite>),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (labels, sprites) = out;
    let scale = ground.scale;
    let gap = (GRAPH_ROW_GAP_LOGICAL_PX * scale).round();
    let column_right = push_lanes(
        &commit.lanes,
        rect,
        lane_width,
        false,
        scale,
        palette,
        sprites,
        crop,
    );

    // ── the columns (V1): graph, refs, message, author, time, hash ──
    let muted = ground.muted(palette);
    let rects = graph_column_rects(rect, columns, scale);
    let mut column_label = |text: &str, box_: Option<[f32; 4]>, font: f32, tabular: bool| {
        let Some(box_) = box_ else { return };
        if text.is_empty() {
            return;
        }
        labels.push(ChromeLabel {
            text: text.to_owned(),
            rect: box_,
            font_size_px: font,
            color: muted,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: tabular,
            clip: Some(crop(box_)),
        });
    };
    column_label(
        &commit.short,
        rects.hash,
        crate::git_panel::GIT_HASH_FONT_LOGICAL_PX * scale,
        true,
    );
    column_label(
        &commit.time,
        rects.date,
        crate::git_panel::GIT_TIME_FONT_LOGICAL_PX * scale,
        true,
    );
    column_label(
        &commit.author,
        rects.author,
        GRAPH_AUTHOR_FONT_LOGICAL_PX * scale,
        false,
    );

    // The pills, left to right after the graph column.
    let pill_height = (GRAPH_REF_HEIGHT_LOGICAL_PX * scale).round().max(1.0);
    let pill_pad = (GRAPH_REF_PADDING_X_LOGICAL_PX * scale).round();
    let pill_radius = (GRAPH_REF_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    let pill_edge = (GRAPH_REF_EDGE_LOGICAL_PX * scale).round().max(1.0) as u32;
    let pill_top = ((rect[1] + rect[3] - pill_height) / 2.0).round();
    let mut cursor = column_right + gap;
    for pill in &commit.refs {
        let width = pill.text_width + pill_pad * 2.0;
        if cursor + width > rects.description_right {
            break;
        }
        let box_ = [cursor, pill_top, cursor + width, pill_top + pill_height];
        let lane = palette.graph_lanes[lane_colour_index(pill.lane)];
        // `background: color-mix(--lane 10%)` — a solid so quiet it is a tint.
        sprites.push(
            ChromeSprite::new(
                ChromeMark::ControlPill {
                    radius_px: pill_radius,
                },
                crop(box_),
                lane,
            )
            .with_opacity(alpha(GRAPH_REF_GROUND_ALPHA)),
        );
        // **`HEAD` wears the accent ring** (R22): every other name is a place in
        // this repository, and this one is where you are standing. The ring is
        // the accent's and not the lane's, because "you are here" is not a fact
        // about which road the commit is on.
        sprites.push(
            ChromeSprite::new(
                ChromeMark::ControlPillRing {
                    radius_px: pill_radius,
                    stroke_px: pill_edge,
                },
                crop(box_),
                if pill.head { palette.accent } else { lane },
            )
            .with_opacity(if pill.head {
                1.0
            } else {
                alpha(GRAPH_REF_EDGE_ALPHA)
            }),
        );
        labels.push(ChromeLabel {
            text: pill.name.clone(),
            rect: box_,
            font_size_px: GRAPH_REF_FONT_LOGICAL_PX * scale,
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

    let subject_rect = [
        cursor,
        rect[1],
        rects.description_right.max(cursor),
        rect[3],
    ];
    labels.push(ChromeLabel {
        text: commit.subject.clone(),
        rect: subject_rect,
        font_size_px: crate::git_panel::GIT_ROW_FONT_LOGICAL_PX * scale,
        color: ground.text(palette),
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(crop(subject_rect)),
    });
}

/// The **Uncommitted Changes** row (V5) — a commit row's columns, answered by
/// the working tree.
///
/// It is drawn by its own function and not by [`push_commit_row`] with three
/// fields left empty, because what it says in each column is a different *kind*
/// of answer: `now` is not a relative time out of R8's table, `*` is not an
/// abbreviation of anything, and the empty author column is a claim that nobody
/// has signed this work rather than a name that would not fit.
#[allow(clippy::too_many_arguments)]
fn push_uncommitted_row(
    head: &GraphUncommittedRow,
    rect: [f32; 4],
    lane_width: usize,
    columns: GraphColumns,
    ground: RowGround,
    palette: &ChromePalette,
    out: (&mut Vec<ChromeLabel>, &mut Vec<ChromeSprite>),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (labels, sprites) = out;
    let scale = ground.scale;
    let gap = (GRAPH_ROW_GAP_LOGICAL_PX * scale).round();
    let column_right = push_lanes(
        &head.lanes,
        rect,
        lane_width,
        true,
        scale,
        palette,
        sprites,
        crop,
    );
    let rects = graph_column_rects(rect, columns, scale);
    let muted = ground.muted(palette);
    let mut column_label = |text: String, box_: Option<[f32; 4]>, font: f32, tabular: bool| {
        let Some(box_) = box_ else { return };
        labels.push(ChromeLabel {
            text,
            rect: box_,
            font_size_px: font,
            color: muted,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: tabular,
            clip: Some(crop(box_)),
        });
    };
    column_label(
        GRAPH_UNCOMMITTED_HASH.to_owned(),
        rects.hash,
        crate::git_panel::GIT_HASH_FONT_LOGICAL_PX * scale,
        true,
    );
    column_label(
        GRAPH_UNCOMMITTED_TIME.to_owned(),
        rects.date,
        crate::git_panel::GIT_TIME_FONT_LOGICAL_PX * scale,
        true,
    );
    let text_rect = [
        column_right + gap,
        rect[1],
        rects.description_right.max(column_right + gap),
        rect[3],
    ];
    labels.push(ChromeLabel {
        text: format!("{GRAPH_UNCOMMITTED} ({})", head.count),
        rect: text_rect,
        font_size_px: crate::git_panel::GIT_ROW_FONT_LOGICAL_PX * scale,
        color: ground.text(palette),
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(crop(text_rect)),
    });
}

/// A thousandth-alpha as the opacity a sprite takes.
fn alpha(thousandths: i32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let value = thousandths as f32 / 1000.0;
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A commit with the parents it names and nothing else that matters here.
    fn commit(hash: &str, parents: &[&str]) -> GitCommit {
        GitCommit {
            hash: hash.to_owned(),
            short: hash.to_owned(),
            subject: hash.to_owned(),
            author_name: "t".to_owned(),
            author_email: "t@example.com".to_owned(),
            committer_name: "t".to_owned(),
            committer_email: "t@example.com".to_owned(),
            body: String::new(),
            committer_unix: 0,
            committer_offset: 0,
            time_relative: "now".to_owned(),
            parents: parents.iter().map(|p| (*p).to_owned()).collect(),
            refs: Vec::new(),
        }
    }

    fn layout(commits: &[GitCommit]) -> Vec<GraphRow> {
        let mut walker = LaneWalker::default();
        walker.extend(commits);
        walker.rows().to_vec()
    }

    #[test]
    fn a_straight_history_is_one_lane() {
        let rows = layout(&[commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])]);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.dot == 0), "{rows:#?}");
        assert!(rows.iter().all(|row| row.width == 1), "{rows:#?}");
        assert!(
            rows.iter()
                .all(|row| row.open.is_empty() && row.close.is_empty())
        );
    }

    #[test]
    fn one_merge_opens_a_second_lane_and_takes_it_back() {
        // m merges side (s) into main (b); s's parent is a, which is also b's
        // parent — so the second lane closes back into the first at `a`.
        let rows = layout(&[
            commit("m", &["b", "s"]),
            commit("b", &["a"]),
            commit("s", &["a"]),
            commit("a", &[]),
        ]);
        assert_eq!(rows[0].dot, 0, "the merge stands on the first lane");
        assert_eq!(rows[0].open, vec![1], "its second parent opens lane 1");
        assert_eq!(rows[1].dot, 0);
        assert_eq!(
            rows[2].dot, 1,
            "the side commit stands in the lane opened for it"
        );
        assert_eq!(rows[3].dot, 0, "the shared parent lands back on lane 0");
        assert_eq!(rows[3].close, vec![1], "and lane 1 curves into it");
        assert!(
            rows.iter().all(|row| row.width <= 2),
            "two lanes and never three: {rows:#?}"
        );
        // Reclaimed: nothing after the join draws lane 1 at all.
        assert!(!rows[3].lower.contains(&1));
    }

    #[test]
    fn three_parallel_branches_hold_three_lanes() {
        let rows = layout(&[
            commit("x", &["r"]),
            commit("y", &["r"]),
            commit("z", &["r"]),
            commit("r", &[]),
        ]);
        assert_eq!(rows[0].dot, 0);
        assert_eq!(rows[1].dot, 1);
        assert_eq!(rows[2].dot, 2);
        assert_eq!(rows[2].width, 3, "three tips, three lanes: {rows:#?}");
        assert_eq!(rows[3].dot, 0, "the root lands on the leftmost claim");
        assert_eq!(rows[3].close, vec![1, 2], "and both others curve into it");
    }

    #[test]
    fn an_octopus_merge_opens_every_extra_parent() {
        let rows = layout(&[
            commit("o", &["p", "q", "r"]),
            commit("p", &[]),
            commit("q", &[]),
            commit("r", &[]),
        ]);
        assert_eq!(rows[0].dot, 0);
        assert_eq!(
            rows[0].open,
            vec![1, 2],
            "two extra parents, two lanes opened"
        );
        assert_eq!(rows[0].width, 3);
        assert_eq!(rows[1].dot, 0);
        assert_eq!(rows[2].dot, 1);
        assert_eq!(rows[3].dot, 2);
    }

    /// R19 — the dot's lane runs through the row **unless this row opens or
    /// closes it**.
    #[test]
    fn the_dots_lane_runs_through_unless_this_row_opens_or_closes_it() {
        let rows = layout(&[commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])]);
        // The newest commit opens its lane: nothing above it.
        assert!(!rows[0].upper.contains(&0), "the tip has no line above it");
        assert!(rows[0].lower.contains(&0), "but its parent carries it down");
        // The middle commit neither opens nor closes: the lane runs through.
        assert!(rows[1].upper.contains(&0) && rows[1].lower.contains(&0));
        // The root closes its lane: nothing below it.
        assert!(rows[2].upper.contains(&0), "the root has a line above it");
        assert!(!rows[2].lower.contains(&0), "and none below");

        // The same rule where a second lane is in play: the side commit is the
        // tip of a lane a merge two rows above opened, so *that* lane does run
        // above it — the row did not open it, it merely stands on it.
        let merged = layout(&[
            commit("m", &["b", "s"]),
            commit("b", &["a"]),
            commit("s", &["a"]),
            commit("a", &[]),
        ]);
        assert_eq!(merged[2].dot, 1);
        assert!(
            merged[2].upper.contains(&1),
            "the side lane was opened above, so it arrives from above: {merged:#?}"
        );
        // And the merge itself opened lane 1 on its own row, so nothing of lane
        // 1 is drawn above the merge — only the curve leaving it.
        assert!(!merged[0].upper.contains(&1));
        assert_eq!(merged[0].open, vec![1]);
    }

    /// A state holding exactly this history, as if git had answered with it.
    fn state_of(commits: Vec<GitCommit>, has_more: bool) -> GraphState {
        let root = std::path::PathBuf::from(r"D:\repo");
        let mut state = GraphState::new(root.clone());
        state.cache.accept(crate::git::GitAnswer::Log {
            root,
            skip: 0,
            outcome: Ok(crate::git::GitLog {
                skip: 0,
                commits,
                has_more,
            }),
        });
        state.sync();
        state
    }

    /// The same, with a `git status` answered into it as well.
    ///
    /// `porcelain` is a recording of `status --porcelain=v1 -z --branch`, so what
    /// this hands the state is the same bytes the worker would have.
    fn state_with_status(commits: Vec<GitCommit>, porcelain: &[u8]) -> GraphState {
        let root = std::path::PathBuf::from(r"D:\repo");
        let mut state = state_of(commits, false);
        state.cache.accept(crate::git::GitAnswer::Status {
            root,
            outcome: Ok(crate::git::parse_status(porcelain)),
        });
        state.sync();
        state
    }

    /// One file staged, one changed, one untracked — three groups, three rows.
    const DIRTY: &[u8] = b"## main\x00M  staged.txt\x00 M mod.txt\x00?? new.txt\x00";
    /// A repository with nothing to say about its working tree.
    const CLEAN: &[u8] = b"## main\x00";

    /// A body wide enough for every column, and one narrow enough for none of
    /// the optional three.
    const WIDE: [f32; 4] = [0.0, 0.0, 900.0, 600.0];

    fn frame(state: &GraphState, expanded: Option<&str>, body: [f32; 4]) -> GraphContent {
        looked(
            state,
            GraphLook {
                expanded,
                compare: None,
                selected: None,
            },
            body,
        )
    }

    /// The same frame, with the whole of what a seat is looking at (v2 ②).
    ///
    /// The measurer is six pixels a character, so every width in these tests is
    /// arithmetic somebody can do in their head — which is what lets a wrap
    /// assertion say how many characters fit rather than "about this many".
    fn looked(state: &GraphState, look: GraphLook<'_>, body: [f32; 4]) -> GraphContent {
        let mut measure = |text: &str, _: f32| text.chars().count() as f32 * 6.0;
        build(
            state,
            look,
            body,
            0.0,
            LaneWidthHold::default(),
            1.0,
            &mut measure,
        )
    }

    /// The one detail block a frame has, if it has one.
    fn detail_of(content: &GraphContent) -> Option<&GraphDetailRow> {
        content.rows.iter().find_map(|row| match row {
            GraphViewRow::Detail(detail) => Some(detail),
            _ => None,
        })
    }

    /// The open commit's story, as against a comparison's head.
    fn story_of(content: &GraphContent) -> Option<&GraphCommitDetail> {
        match &detail_of(content)?.detail {
            GraphDetail::Commit(commit) => Some(commit),
            GraphDetail::Compare(_) => None,
        }
    }

    fn straight(count: usize) -> Vec<GitCommit> {
        (0..count)
            .map(|index| {
                let parents: Vec<&str> = if index + 1 < count {
                    vec![Box::leak(format!("c{}", index + 1).into_boxed_str())]
                } else {
                    Vec::new()
                };
                commit(
                    Box::leak(format!("c{index}").into_boxed_str()),
                    parents.as_slice(),
                )
            })
            .collect()
    }

    /// R23 — a repository of ten thousand commits builds a screenful.
    ///
    /// The assertion is on the **measurer**, because that is what a row costs:
    /// every row that exists has had its hash and its age measured through the
    /// font, and a build that measured ten thousand of them to draw twenty is
    /// the frame this ruling was written against. The bound is stated in terms
    /// of the window rather than as a magic number, so a change to the buffer
    /// size cannot quietly turn this into a test of nothing.
    #[test]
    fn a_ten_thousand_commit_history_builds_only_the_window() {
        let state = state_of(straight(10_000), false);
        let body = [0.0, 0.0, 900.0, 600.0];
        let mut calls = 0_usize;
        let mut measure = |text: &str, _size: f32| {
            calls += 1;
            text.len() as f32 * 6.0
        };
        let content = build(
            &state,
            GraphLook {
                expanded: None,
                compare: None,
                selected: None,
            },
            body,
            0.0,
            LaneWidthHold::default(),
            1.0,
            &mut measure,
        );
        assert_eq!(content.total_rows, 10_000, "the list is the whole history");
        // 600 pixels of body, less the head, over 30-pixel rows: twenty-odd
        // rows plus the buffer at each end.
        let visible = (600.0 / GRAPH_ROW_HEIGHT_LOGICAL_PX).ceil() as usize
            + 1
            + GRAPH_WINDOW_BUFFER_ROWS * 2;
        assert!(
            content.rows.len() <= visible,
            "built {} rows for a window of {visible}",
            content.rows.len()
        );
        // Two measurements a row — the short hash and the age — and nothing per
        // commit that is not on screen.
        assert!(
            calls <= visible * 4,
            "{calls} measurements for {} rows",
            content.rows.len()
        );
        // And the window is *where the reader is*, not the top of the list.
        let mut deep = |_: &str, _: f32| 30.0;
        let scrolled = build(
            &state,
            GraphLook {
                expanded: None,
                compare: None,
                selected: None,
            },
            body,
            5_000.0 * GRAPH_ROW_HEIGHT_LOGICAL_PX,
            LaneWidthHold::default(),
            1.0,
            &mut deep,
        );
        assert!(
            scrolled.rows.iter().all(|row| row.index() > 4_900),
            "the window did not follow the scroll"
        );
        assert!(scrolled.rows.len() <= visible);
    }

    /// R23's other half — the next page is asked for before the reader arrives
    /// at the end, and never while they are nowhere near it.
    #[test]
    fn the_next_page_is_wanted_only_near_the_end() {
        let state = state_of(straight(200), true);
        let body = [0.0, 0.0, 900.0, 600.0];
        let mut measure = |_: &str, _: f32| 30.0;
        let top = build(
            &state,
            GraphLook {
                expanded: None,
                compare: None,
                selected: None,
            },
            body,
            0.0,
            LaneWidthHold::default(),
            1.0,
            &mut measure,
        );
        assert!(!top.wants_more, "the top of two hundred wants nothing yet");
        let bottom = build(
            &state,
            GraphLook {
                expanded: None,
                compare: None,
                selected: None,
            },
            body,
            190.0 * GRAPH_ROW_HEIGHT_LOGICAL_PX,
            LaneWidthHold::default(),
            1.0,
            &mut measure,
        );
        assert!(bottom.wants_more, "the end of the list wants the next page");
        // A history with nothing after it never asks, however far down it is.
        let whole = state_of(straight(200), false);
        let ended = build(
            &whole,
            GraphLook {
                expanded: None,
                compare: None,
                selected: None,
            },
            body,
            190.0 * GRAPH_ROW_HEIGHT_LOGICAL_PX,
            LaneWidthHold::default(),
            1.0,
            &mut measure,
        );
        assert!(!ended.wants_more);
    }

    /// R18's hysteresis: the column does not narrow under a reader who is still
    /// looking at what widened it.
    #[test]
    fn the_lane_column_holds_its_width_until_the_window_leaves_it() {
        let state = state_of(straight(200), false);
        let body = [0.0, 0.0, 900.0, 600.0];
        let mut measure = |_: &str, _: f32| 30.0;
        // A width held for a row inside the window survives, even though this
        // straight history needs only one lane.
        let held = build(
            &state,
            GraphLook {
                expanded: None,
                compare: None,
                selected: None,
            },
            body,
            0.0,
            LaneWidthHold { width: 5, until: 3 },
            1.0,
            &mut measure,
        );
        assert_eq!(held.lane_width, 5, "the hold was inside the window");
        // Scrolled past it, the column falls back to what the window needs.
        let released = build(
            &state,
            GraphLook {
                expanded: None,
                compare: None,
                selected: None,
            },
            body,
            100.0 * GRAPH_ROW_HEIGHT_LOGICAL_PX,
            LaneWidthHold { width: 5, until: 3 },
            1.0,
            &mut measure,
        );
        assert_eq!(released.lane_width, 1, "the knot is long gone");
    }

    /// The accordion's arithmetic: one expansion, at one place.
    #[test]
    fn a_row_index_finds_its_item_through_the_open_commit() {
        assert_eq!(item_at(7, None, None), Some(GraphItem::Commit(7)));
        let open = Some(GraphOpen {
            at: 2,
            detail: 0,
            files: 3,
        });
        assert_eq!(item_at(2, None, open), Some(GraphItem::Commit(2)));
        assert_eq!(
            item_at(3, None, open),
            Some(GraphItem::File { commit: 2, file: 0 })
        );
        assert_eq!(
            item_at(5, None, open),
            Some(GraphItem::File { commit: 2, file: 2 })
        );
        assert_eq!(item_at(6, None, open), Some(GraphItem::Commit(3)));
    }

    /// The same arithmetic with the working tree's row on top of it (V5).
    ///
    /// Two things unfold on this page and they unfold at different places, so
    /// the test that matters is the one where **both** are open: an index has to
    /// walk past the working tree's own rows and then past a commit's, and a
    /// mapping that subtracted them in the wrong order would still be right
    /// whenever only one of the two was showing.
    #[test]
    fn a_row_index_walks_past_the_working_tree_before_it_reaches_the_commits() {
        // Collapsed: the row exists, and every commit is one lower.
        let head = Some(0);
        assert_eq!(item_at(0, head, None), Some(GraphItem::Uncommitted));
        assert_eq!(item_at(1, head, None), Some(GraphItem::Commit(0)));
        assert_eq!(item_at(9, head, None), Some(GraphItem::Commit(8)));

        // Unfolded into three files of its own.
        let head = Some(3);
        assert_eq!(item_at(0, head, None), Some(GraphItem::Uncommitted));
        assert_eq!(item_at(1, head, None), Some(GraphItem::Working(0)));
        assert_eq!(item_at(3, head, None), Some(GraphItem::Working(2)));
        assert_eq!(item_at(4, head, None), Some(GraphItem::Commit(0)));

        // And with a commit open under it. The commit's index is a *log* index,
        // so `(1, 2)` is the second commit — row five of a list whose first four
        // rows are the working tree's.
        let open = Some(GraphOpen {
            at: 1,
            detail: 0,
            files: 2,
        });
        assert_eq!(item_at(4, head, open), Some(GraphItem::Commit(0)));
        assert_eq!(item_at(5, head, open), Some(GraphItem::Commit(1)));
        assert_eq!(
            item_at(6, head, open),
            Some(GraphItem::File { commit: 1, file: 0 })
        );
        assert_eq!(
            item_at(7, head, open),
            Some(GraphItem::File { commit: 1, file: 1 })
        );
        assert_eq!(item_at(8, head, open), Some(GraphItem::Commit(2)));
    }

    #[test]
    fn the_colour_wheel_answers_for_every_lane() {
        for lane in 0..64_usize {
            assert!(
                lane_colour_index(lane) < GRAPH_LANE_COLOURS,
                "lane {lane} fell off the wheel"
            );
        }
        assert_eq!(lane_colour_index(20), 20 % GRAPH_LANE_COLOURS);
    }

    /// A later page is laid out on top of the pages already walked, and the
    /// answer is the same as walking the whole thing at once.
    #[test]
    fn a_second_page_continues_the_first() {
        let all = [
            commit("m", &["b", "s"]),
            commit("b", &["a"]),
            commit("s", &["a"]),
            commit("a", &[]),
        ];
        let mut walker = LaneWalker::default();
        walker.extend(&all[..2]);
        assert_eq!(walker.rows().len(), 2);
        walker.extend(&all);
        assert_eq!(walker.rows(), layout(&all).as_slice());
    }

    // ── v2 ①: the table (2026-08-16) ───────────────────────────────────────

    /// V1 — the columns stand in the ruled order, and none of them overlaps the
    /// next.
    ///
    /// The order is asserted on the *rectangles* and not on a list of names,
    /// because "the author is left of the date" is a claim about pixels: a row
    /// that named its columns correctly and then laid them out right to left
    /// would pass a test of the names and draw a row nobody could read.
    #[test]
    fn the_columns_run_description_author_date_hash_from_left_to_right() {
        let row = [0.0, 0.0, 900.0, 30.0];
        let rects = graph_column_rects(row, GraphColumns::default(), 1.0);
        let author = rects.author.expect("a wide row draws its author");
        let date = rects.date.expect("and its date");
        let hash = rects.hash.expect("and its hash");
        assert!(
            rects.description_right <= author[0],
            "the description ends before the author begins"
        );
        assert!(author[2] <= date[0], "author is left of date");
        assert!(date[2] <= hash[0], "date is left of hash");
        assert!(hash[2] <= row[2], "and nothing hangs off the row's edge");
        // Reserved widths, so two rows of different text share one column.
        assert_eq!(author[2] - author[0], GRAPH_AUTHOR_COLUMN_LOGICAL_PX);
        assert_eq!(date[2] - date[0], GRAPH_DATE_COLUMN_LOGICAL_PX);
        assert_eq!(hash[2] - hash[0], GRAPH_HASH_COLUMN_LOGICAL_PX);
    }

    /// V1 — a narrowing seat sheds its columns in the ruled order, and never
    /// the description.
    #[test]
    fn a_narrowing_seat_sheds_the_author_then_the_date_then_the_hash() {
        let all = GraphColumns::at_width(GRAPH_AUTHOR_MIN_BODY_LOGICAL_PX);
        assert_eq!(
            (all.author, all.date, all.hash),
            (true, true, true),
            "at the threshold the column is still drawn"
        );
        let no_author = GraphColumns::at_width(GRAPH_AUTHOR_MIN_BODY_LOGICAL_PX - 1.0);
        assert_eq!(
            (no_author.author, no_author.date, no_author.hash),
            (false, true, true)
        );
        let no_date = GraphColumns::at_width(GRAPH_DATE_MIN_BODY_LOGICAL_PX - 1.0);
        assert_eq!(
            (no_date.author, no_date.date, no_date.hash),
            (false, false, true)
        );
        let bare = GraphColumns::at_width(GRAPH_HASH_MIN_BODY_LOGICAL_PX - 1.0);
        assert_eq!((bare.author, bare.date, bare.hash), (false, false, false));
        // The order is a *ladder*: there is no width at which the date is gone
        // and the author is not.
        let mut width = 0.0_f32;
        while width < 1_200.0 {
            let columns = GraphColumns::at_width(width);
            assert!(
                !columns.author || columns.date,
                "author outlived date at {width}"
            );
            assert!(
                !columns.date || columns.hash,
                "date outlived hash at {width}"
            );
            width += 7.0;
        }
        // And whatever leaves, the description keeps the room: a bare row's
        // description reaches the row's own padding.
        let row = [0.0, 0.0, 200.0, 30.0];
        let rects = graph_column_rects(row, bare, 1.0);
        assert_eq!(
            rects.description_right,
            200.0 - GRAPH_ROW_PADDING_X_LOGICAL_PX
        );
    }

    /// V2 — the header is a fixed strip above the rows, and each word stands
    /// over its own column.
    ///
    /// The alignment is checked through the *same* function the rows lay their
    /// columns out with, which is the only way this can be a property rather
    /// than a coincidence: if the two ever part, they part here.
    #[test]
    fn the_column_header_is_a_fixed_strip_whose_words_stand_over_their_columns() {
        let state = state_of(straight(50), false);
        let content = frame(&state, None, WIDE);
        let geometry = graph_geometry(WIDE, &content, 1.0);
        let header = geometry.header.expect("a page with rows names its columns");
        // Above the rows and outside them: it is not row zero, so no scroll
        // moves it and no hit test finds it.
        assert!(header[3] <= geometry.viewport[1]);
        assert_eq!(
            geometry.row_at(header[0] + 1.0, header[1] + 1.0, content.total_rows),
            None
        );
        // And it does not move when the list is scrolled.
        let mut measure = |text: &str, _: f32| text.len() as f32 * 6.0;
        let scrolled = build(
            &state,
            GraphLook {
                expanded: None,
                compare: None,
                selected: None,
            },
            WIDE,
            20.0 * GRAPH_ROW_HEIGHT_LOGICAL_PX,
            LaneWidthHold::default(),
            1.0,
            &mut measure,
        );
        assert_eq!(graph_geometry(WIDE, &scrolled, 1.0).header, Some(header));

        // Each word's box is its column's box, to the pixel.
        let head_rects = graph_column_rects(header, content.columns, 1.0);
        let row_rects = graph_column_rects(geometry.row_rect(0), content.columns, 1.0);
        for (head, row) in [
            (head_rects.author, row_rects.author),
            (head_rects.date, row_rects.date),
            (head_rects.hash, row_rects.hash),
        ] {
            let (head, row) = (head.expect("wide"), row.expect("wide"));
            assert_eq!((head[0], head[2]), (row[0], row[2]));
        }

        // The words themselves (real machine, 2026-08-16): a one-lane graph is
        // too narrow for the word GRAPH, which then ran into DESCRIPTION; now
        // the narrow column has no heading and DESCRIPTION starts where a
        // row's own text starts — one lane and a gap in.
        let palette = bt_render::chrome_palette();
        let mut labels = Vec::new();
        push_column_header(header, content.columns, 1, 1.0, &palette, &mut labels);
        let words: Vec<&str> = labels.iter().map(|label| label.text.as_str()).collect();
        assert!(
            !words.contains(&GRAPH_HEADING_GRAPH),
            "a one-lane column has no room for its heading: {words:?}"
        );
        let description = labels
            .iter()
            .find(|label| label.text == GRAPH_HEADING_DESCRIPTION)
            .expect("the description is always headed");
        assert_eq!(
            description.rect[0],
            header[0]
                + GRAPH_ROW_PADDING_X_LOGICAL_PX
                + GRAPH_LANE_WIDTH_LOGICAL_PX
                + GRAPH_ROW_GAP_LOGICAL_PX,
            "over the rows' own text start"
        );
        // Three lanes hold the word, and the description heading moves out
        // with the column it follows.
        let mut labels = Vec::new();
        push_column_header(header, content.columns, 3, 1.0, &palette, &mut labels);
        let graph = labels
            .iter()
            .find(|label| label.text == GRAPH_HEADING_GRAPH)
            .expect("three lanes are wide enough for the word");
        assert_eq!(
            graph.rect[2],
            header[0] + GRAPH_ROW_PADDING_X_LOGICAL_PX + 3.0 * GRAPH_LANE_WIDTH_LOGICAL_PX,
            "and it is clipped to its own column"
        );
        let description = labels
            .iter()
            .find(|label| label.text == GRAPH_HEADING_DESCRIPTION)
            .expect("headed");
        assert_eq!(
            description.rect[0],
            graph.rect[2] + GRAPH_ROW_GAP_LOGICAL_PX
        );
    }

    /// V2 — a hidden column takes its heading with it.
    #[test]
    fn a_collapsed_column_has_no_heading_either() {
        let narrow = [0.0, 0.0, GRAPH_DATE_MIN_BODY_LOGICAL_PX - 10.0, 600.0];
        let state = state_of(straight(50), false);
        let content = frame(&state, None, narrow);
        assert!(!content.columns.author);
        assert!(!content.columns.date);
        let header = graph_geometry(narrow, &content, 1.0)
            .header
            .expect("even a narrow page has a header");
        let rects = graph_column_rects(header, content.columns, 1.0);
        assert_eq!(rects.author, None);
        assert_eq!(rects.date, None);
        assert!(rects.hash.is_some(), "the hash is the last to go");
    }

    /// V1 — the author reaches the row already cut to its column.
    #[test]
    fn a_long_author_arrives_at_the_row_already_ellipsised() {
        let mut commits = straight(3);
        commits[0].author_name = "A Name Far Too Long To Fit In A Hundred And Twenty".to_owned();
        let state = state_of(commits, false);
        let content = frame(&state, None, WIDE);
        let GraphViewRow::Commit(row) = &content.rows[0] else {
            panic!("the first row is a commit");
        };
        assert!(row.author.ends_with('…'), "{}", row.author);
        // 6 pixels a character in this measurer, so twenty characters fit.
        assert!(row.author.chars().count() <= 21, "{}", row.author);
        // And a name that fits is untouched.
        assert!(matches!(&content.rows[1], GraphViewRow::Commit(row) if row.author == "t"));
    }

    /// V5 — the working tree's row exists exactly when it has something to say.
    #[test]
    fn the_uncommitted_row_exists_only_while_the_working_tree_is_dirty() {
        let clean = state_with_status(straight(5), CLEAN);
        assert_eq!(clean.uncommitted(), None, "a zero row is no row");
        assert!(clean.head_lanes().is_none());
        let content = frame(&clean, None, WIDE);
        assert_eq!(content.total_rows, 5);
        assert_eq!(content.uncommitted_rows, None);
        assert!(
            content
                .rows
                .iter()
                .all(|row| !matches!(row, GraphViewRow::Uncommitted(_)))
        );

        let dirty = state_with_status(straight(5), DIRTY);
        assert_eq!(dirty.uncommitted(), Some(3), "three distinct paths");
        let content = frame(&dirty, None, WIDE);
        assert_eq!(content.total_rows, 6, "the history, and the row above it");
        assert_eq!(content.uncommitted_rows, Some(0), "drawn, and not unfolded");
        let GraphViewRow::Uncommitted(head) = &content.rows[0] else {
            panic!("the first row is the working tree's");
        };
        assert_eq!(head.count, 3);
        assert!(!head.expanded);
    }

    /// V5 — a file that is both staged and changed is one path in the count and
    /// two rows in the list.
    ///
    /// R11's own honesty, in a second surface: the count answers "how much is
    /// there", and the rows answer "what happened", and those are two questions
    /// with two right answers.
    #[test]
    fn a_file_staged_and_changed_is_counted_once_and_listed_twice() {
        let both = b"## main\x00MM both.txt\x00";
        let state = state_with_status(straight(3), both);
        assert_eq!(state.uncommitted(), Some(1));
        let content = frame(&state, Some(GRAPH_UNCOMMITTED_HASH), WIDE);
        let files: Vec<&GraphFileRow> = content
            .rows
            .iter()
            .filter_map(|row| match row {
                GraphViewRow::File(file) => Some(file),
                _ => None,
            })
            .collect();
        assert_eq!(files.len(), 2, "staged, and changed");
        assert_eq!(files[0].working.as_ref().map(|w| w.staged), Some(true));
        assert_eq!(files[1].working.as_ref().map(|w| w.staged), Some(false));
    }

    /// V5 — the row's dot is in `HEAD`'s own lane, and its road runs down into
    /// it.
    ///
    /// The history here is a merge, so `HEAD` is a commit with two parents and
    /// the picture around it has three lanes in play — which is the case a
    /// hand-painted line would get wrong, because it would have had to *choose*
    /// a lane rather than be told one.
    #[test]
    fn the_uncommitted_rows_dot_stands_in_heads_lane_and_its_line_runs_into_it() {
        let history = vec![
            commit("m", &["b", "s"]),
            commit("b", &["a"]),
            commit("s", &["a"]),
            commit("a", &[]),
        ];
        let state = state_with_status(history, DIRTY);
        let head = state.head_lanes().expect("a dirty tree draws the row");
        let first = &state.lanes()[0];
        assert_eq!(head.dot, first.dot, "the same road HEAD is standing on");
        assert!(
            head.lower.contains(&first.dot),
            "and the road leaves this row downward: {head:#?}"
        );
        assert!(
            head.upper.is_empty(),
            "nothing stands above the working tree"
        );
        assert!(
            first.upper.contains(&first.dot),
            "so HEAD's own row draws the line arriving: {first:#?}"
        );
        // The merge's second lane is untouched by any of this.
        assert_eq!(first.open, vec![1]);
    }

    /// V5 — a clean tree that becomes dirty relays the picture rather than
    /// appending to it.
    #[test]
    fn the_picture_is_walked_again_when_the_working_tree_crosses_into_dirt() {
        let clean = state_with_status(straight(4), CLEAN);
        assert!(
            !clean.lanes()[0].upper.contains(&0),
            "with no row above it, HEAD is a tip"
        );
        let dirty = state_with_status(straight(4), DIRTY);
        assert!(
            dirty.lanes()[0].upper.contains(&0),
            "with the working tree above it, HEAD's line arrives from above"
        );
        assert_eq!(dirty.lanes().len(), 4, "and no commit was lost or doubled");
    }

    /// V5 — the working tree's files open the **working tree's** diff, on the
    /// side of the index their group is (R25).
    #[test]
    fn a_working_file_opens_the_diff_of_its_own_side_of_the_index() {
        let state = state_with_status(straight(3), DIRTY);
        let content = frame(&state, Some(GRAPH_UNCOMMITTED_HASH), WIDE);
        let root = std::path::Path::new(r"D:\repo");
        assert_eq!(content.total_rows, 3 + 1 + 3);

        let opened: Vec<crate::git_panel::GitRowOpen> = content
            .rows
            .iter()
            .filter(|row| matches!(row, GraphViewRow::File(_)))
            .filter_map(|row| row_open(row, root))
            .collect();
        assert_eq!(
            opened[0],
            crate::git_panel::GitRowOpen::Document {
                source: crate::preview::PreviewSource::GitDiff {
                    root: root.to_owned(),
                    path: "staged.txt".to_owned(),
                    staged: true,
                },
                name: "staged.txt.diff".to_owned(),
                renamed_from: None,
            },
            "the staged group is a claim about the index"
        );
        assert!(matches!(
            &opened[1],
            crate::git_panel::GitRowOpen::Document {
                source: crate::preview::PreviewSource::GitDiff { staged: false, path, .. },
                ..
            } if path == "mod.txt"
        ));
        assert!(matches!(
            &opened[2],
            crate::git_panel::GitRowOpen::Document {
                source: crate::preview::PreviewSource::GitDiff { staged: false, path, .. },
                ..
            } if path == "new.txt"
        ));
        // And the row itself turns over exactly as a commit does.
        assert_eq!(
            row_open(&content.rows[0], root),
            Some(crate::git_panel::GitRowOpen::Expand {
                hash: GRAPH_UNCOMMITTED_HASH.to_owned()
            })
        );
        // But it is not somewhere you can check out — you are standing in it.
        assert_eq!(row_double_open(&content.rows[0]), None);
    }

    /// V8 — the selected ground is on one row and it is the row the page was
    /// told about.
    #[test]
    fn exactly_the_selected_row_wears_the_selected_ground() {
        let state = state_of(straight(20), false);
        let mut measure = |text: &str, _: f32| text.len() as f32 * 6.0;
        let content = build(
            &state,
            GraphLook {
                expanded: None,
                compare: None,
                selected: Some(4),
            },
            WIDE,
            0.0,
            LaneWidthHold::default(),
            1.0,
            &mut measure,
        );
        assert_eq!(content.selected, Some(4));
        // The expansion is a *separate* fact: a row can be open without the
        // keyboard standing on it, and the ground follows the keyboard.
        let open = build(
            &state,
            GraphLook {
                expanded: Some("c2"),
                compare: None,
                selected: Some(9),
            },
            WIDE,
            0.0,
            LaneWidthHold::default(),
            1.0,
            &mut measure,
        );
        assert_eq!(open.selected, Some(9));
        assert!(matches!(&open.rows[2], GraphViewRow::Commit(row) if row.expanded));
    }

    /// V14 — the six keys, against a list with both accordions in play.
    #[test]
    fn the_arrows_walk_the_list_and_enter_turns_a_row_over() {
        let state = state_with_status(straight(20), DIRTY);
        let mut content = frame(&state, None, WIDE);
        assert_eq!(content.total_rows, 21);

        // With nothing selected the first press lands at the top, whichever
        // direction it was.
        assert_eq!(
            graph_key(&content, GraphKey::Down),
            GraphKeyAction::Select(0)
        );
        assert_eq!(graph_key(&content, GraphKey::Up), GraphKeyAction::Select(0));

        content.selected = Some(5);
        assert_eq!(
            graph_key(&content, GraphKey::Down),
            GraphKeyAction::Select(6)
        );
        assert_eq!(graph_key(&content, GraphKey::Up), GraphKeyAction::Select(4));
        assert_eq!(
            graph_key(&content, GraphKey::Home),
            GraphKeyAction::Select(0)
        );
        assert_eq!(
            graph_key(&content, GraphKey::End),
            GraphKeyAction::Select(20)
        );
        assert_eq!(
            graph_key(&content, GraphKey::Enter),
            GraphKeyAction::Toggle(5)
        );

        // The ends hold: neither arrow walks off the list.
        content.selected = Some(0);
        assert_eq!(graph_key(&content, GraphKey::Up), GraphKeyAction::Select(0));
        content.selected = Some(20);
        assert_eq!(
            graph_key(&content, GraphKey::Down),
            GraphKeyAction::Select(20)
        );
    }

    /// V14 — `Esc` folds what is open and takes nothing when nothing is.
    ///
    /// The second half is the half that matters: this surface sits above the
    /// float dismissal and above a running program, and an `Esc` eaten here for
    /// nothing is an `Esc` that never reached `vim`.
    #[test]
    fn escape_collapses_what_is_open_and_is_passed_on_when_nothing_is() {
        let state = state_with_status(straight(20), DIRTY);
        let shut = frame(&state, None, WIDE);
        assert_eq!(
            shut.open_rows, None,
            "a row that is merely drawn is not open"
        );
        assert_eq!(graph_key(&shut, GraphKey::Escape), GraphKeyAction::Pass);

        let open = frame(&state, Some(GRAPH_UNCOMMITTED_HASH), WIDE);
        assert_eq!(open.open_rows, Some((0, 3)));
        assert_eq!(
            graph_key(&open, GraphKey::Escape),
            GraphKeyAction::Collapse(0)
        );

        // A commit's accordion, which is three rows further down the list than
        // its own log index — the working tree and its files stand above it. It
        // unfolds one row even with no files answered for, because a commit
        // always has a story: since v2 ② the detail block is there before the
        // file list is (D1/D2).
        let commit_open = frame(&state, Some("c1"), WIDE);
        assert_eq!(commit_open.open_rows, Some((2, 1)));
        assert_eq!(
            graph_key(&commit_open, GraphKey::Escape),
            GraphKeyAction::Collapse(2)
        );

        // An empty page takes nothing at all.
        let empty = GraphContent::default();
        assert_eq!(graph_key(&empty, GraphKey::Escape), GraphKeyAction::Pass);
        assert_eq!(graph_key(&empty, GraphKey::Down), GraphKeyAction::None);
    }

    /// V14 + R23 — walking to the last loaded row asks for the next page.
    ///
    /// **Through the wheel's own rule and not a second one**: the selection is
    /// revealed, the scroll it lands on is what the next build sees, and
    /// `wants_more` is then decided by exactly the arithmetic a mouse would have
    /// tripped. What this pins is that the two roads meet.
    #[test]
    fn walking_the_selection_to_the_end_asks_for_the_next_page() {
        let state = state_of(straight(200), true);
        let mut measure = |_: &str, _: f32| 30.0;
        let top = frame(&state, None, WIDE);
        assert!(!top.wants_more);
        // End, then the scroll that reveals it.
        let last = match graph_key(&top, GraphKey::End) {
            GraphKeyAction::Select(row) => row,
            other => panic!("End selects the last row, not {other:?}"),
        };
        assert_eq!(last, 199);
        let scroll = graph_geometry(WIDE, &top, 1.0).reveal(last);
        let after = build(
            &state,
            GraphLook {
                expanded: None,
                compare: None,
                selected: Some(last),
            },
            WIDE,
            scroll,
            LaneWidthHold::default(),
            1.0,
            &mut measure,
        );
        assert!(
            after.wants_more,
            "the window the selection dragged down is a window near the end"
        );
        // A row already on screen moves nothing.
        let still = graph_geometry(WIDE, &top, 1.0);
        assert_eq!(still.reveal(1), still.scroll_px);
    }

    /// V4 — one sentence about the author, spelled git's way.
    #[test]
    fn the_author_sentence_is_a_name_and_an_address_in_gits_own_spelling() {
        let mut one = commit("c", &[]);
        one.author_name = "Weiyi Shi".to_owned();
        one.author_email = "weiyi@example.com".to_owned();
        assert_eq!(author_sentence(&one), "Weiyi Shi <weiyi@example.com>");
        // A commit with no address — git allows it, and importers write them —
        // gets its name and not an empty pair of brackets.
        one.author_email = String::new();
        assert_eq!(author_sentence(&one), "Weiyi Shi");
        // And it is what the row's tooltip says.
        one.author_email = "weiyi@example.com".to_owned();
        assert!(commit_tooltip(&one).contains("Weiyi Shi <weiyi@example.com>"));
    }

    // ── v2 ②: the open row's story, and two rows compared ──────────────────

    /// A history whose newest commit has the body, the committer and the two
    /// parents these tests are about.
    fn told(body: &str) -> Vec<GitCommit> {
        let mut commits = straight(20);
        let told = &mut commits[0];
        told.body = body.to_owned();
        told.author_name = "Weiyi Shi".to_owned();
        told.author_email = "weiyi@example.com".to_owned();
        told.committer_name = "Weiyi Shi".to_owned();
        told.committer_email = "weiyi@example.com".to_owned();
        // In a zone four hours behind UTC, which is what makes the printed date
        // the date it was made *where* it was made.
        told.committer_offset = -4 * 3600;
        told.committer_unix = 1_786_803_504; // 2026-08-15T10:18:24-04:00
        told.parents = vec!["c1".to_owned(), "c4".to_owned()];
        commits
    }

    /// D1 — the body arrives wrapped to the description column, with the
    /// paragraphs it was written with.
    ///
    /// MUTATION: wrap with a helper that treats `\n` as whitespace and the two
    /// paragraphs run together into one — which is the whole of what "preserve
    /// the body's own breaks" is about, and it is invisible at any width where
    /// the text happened to wrap there anyway.
    #[test]
    fn a_commit_body_is_wrapped_to_the_description_column_and_keeps_its_paragraphs() {
        let state = state_of(told("First paragraph.\n\nSecond paragraph."), false);
        let content = frame(&state, Some("c0"), WIDE);
        let story = story_of(&content).expect("an open commit tells its story");
        assert_eq!(
            story.body,
            vec![
                "First paragraph.".to_owned(),
                String::new(),
                "Second paragraph.".to_owned(),
            ],
            "the blank line between the two is a line of its own"
        );

        // The width the wrap is against is the **description column's** and not
        // the seat's, which is what makes the prose start and end where every
        // message on the page does. Pinned by narrowing the seat and watching the
        // same paragraph need more lines.
        let long = "one two three four five six seven eight nine ten eleven twelve \
                    thirteen fourteen fifteen sixteen seventeen eighteen";
        let state = state_of(told(long), false);
        let wide = frame(&state, Some("c0"), WIDE);
        let narrow = looked(
            &state,
            GraphLook {
                expanded: Some("c0"),
                compare: None,
                selected: None,
            },
            [0.0, 0.0, 300.0, 600.0],
        );
        let wide = story_of(&wide).expect("open").body.len();
        let narrow = story_of(&narrow).expect("open").body.len();
        assert!(
            narrow > wide && wide > 1,
            "one paragraph, two widths: {wide} lines wide and {narrow} narrow"
        );

        // A commit with nothing after its subject has no prose block at all —
        // which is not an empty one.
        let plain = state_of(straight(20), false);
        let plain = frame(&plain, Some("c0"), WIDE);
        assert!(story_of(&plain).expect("open").body.is_empty());
    }

    /// D1 — twelve lines, and the twelfth says it stopped.
    #[test]
    fn a_body_longer_than_twelve_lines_stops_at_twelve_and_says_so() {
        let long = (0..40)
            .map(|n| format!("Line number {n}."))
            .collect::<Vec<_>>()
            .join("\n");
        let state = state_of(told(&long), false);
        let story = frame(&state, Some("c0"), WIDE);
        let story = story_of(&story).expect("open");
        assert_eq!(story.body.len(), GRAPH_BODY_MAX_LINES);
        assert!(
            story.body[GRAPH_BODY_MAX_LINES - 1].ends_with(GRAPH_BODY_ELLIPSIS),
            "the last line it draws says there is more: {:?}",
            story.body[GRAPH_BODY_MAX_LINES - 1]
        );
        assert!(
            !story.body[0].ends_with(GRAPH_BODY_ELLIPSIS),
            "and no other line does"
        );
        // A body that exactly fits is not marked — the ellipsis is a claim about
        // text that was left out, not decoration on the last line.
        let just = (0..GRAPH_BODY_MAX_LINES)
            .map(|n| format!("Line {n}."))
            .collect::<Vec<_>>()
            .join("\n");
        let state = state_of(told(&just), false);
        let story = frame(&state, Some("c0"), WIDE);
        let story = story_of(&story).expect("open");
        assert_eq!(story.body.len(), GRAPH_BODY_MAX_LINES);
        assert!(!story.body[GRAPH_BODY_MAX_LINES - 1].ends_with(GRAPH_BODY_ELLIPSIS));
    }

    /// D2 — the meta line: who, when, and which commits this one came from.
    #[test]
    fn the_meta_line_names_the_author_the_date_and_the_parents() {
        let state = state_of(told(""), false);
        let content = frame(&state, Some("c0"), WIDE);
        let story = story_of(&content).expect("open");
        assert_eq!(
            story.meta, "Weiyi Shi <weiyi@example.com> \u{b7} 2026-08-15 10:18 \u{b7} parents: ",
            "the absolute date and not the relative one: an opened row is being \
             read rather than scanned"
        );
        assert_eq!(
            story
                .parents
                .iter()
                .map(|parent| parent.short.as_str())
                .collect::<Vec<_>>(),
            vec!["c1", "c4"],
            "both parents, in git's order"
        );
        assert_eq!(
            story.parents[0].hash, "c1",
            "and the whole hash behind each"
        );

        // A root commit names no parents, and does not end its line on a colon
        // with nothing after it.
        let root = state_of(told(""), false);
        let root = frame(&root, Some("c19"), WIDE);
        let root = story_of(&root).expect("open");
        assert!(root.parents.is_empty());
        assert!(!root.meta.contains(GRAPH_META_PARENTS));
    }

    /// D2 — "committed by" appears exactly when the committer is somebody else.
    ///
    /// MUTATION: say it always, and every row on the page gains a clause that
    /// says nothing; say it never, and the one case the field exists for — a
    /// rebase, a cherry-pick, a patch applied from a list — is the one case the
    /// page is silent about.
    #[test]
    fn the_committer_is_named_only_when_they_are_not_the_author() {
        let state = state_of(told(""), false);
        let same = frame(&state, Some("c0"), WIDE);
        assert!(
            !story_of(&same)
                .expect("open")
                .meta
                .contains(GRAPH_META_COMMITTED_BY)
        );

        let mut commits = told("");
        commits[0].committer_name = "Rebase Bot".to_owned();
        commits[0].committer_email = "bot@example.com".to_owned();
        let state = state_of(commits, false);
        let other = frame(&state, Some("c0"), WIDE);
        assert!(
            story_of(&other)
                .expect("open")
                .meta
                .contains("committed by Rebase Bot <bot@example.com>")
        );

        // The address alone is enough: a rebase keeps the name and takes the
        // address, and two people called the same thing are two people.
        let mut commits = told("");
        commits[0].committer_email = "someone.else@example.com".to_owned();
        let state = state_of(commits, false);
        let addressed = frame(&state, Some("c0"), WIDE);
        assert!(
            story_of(&addressed)
                .expect("open")
                .meta
                .contains(GRAPH_META_COMMITTED_BY)
        );
    }

    /// D7 — the two copy verbs stand at the line's right and answer a press
    /// there, and the parents answer one at their own place.
    ///
    /// The hit test is asked through the **same** layout the paint uses, which is
    /// what makes "the button you can press is the button you can see" a property
    /// rather than a screenshot.
    #[test]
    fn the_detail_line_answers_a_press_on_a_parent_and_on_each_copy_verb() {
        let state = state_of(told("Why."), false);
        let content = frame(&state, Some("c0"), WIDE);
        let detail = detail_of(&content).expect("open");
        let geometry = graph_geometry(WIDE, &content, 1.0);
        let first = geometry.row_rect(detail.index);
        let last = geometry.row_rect(detail.index + detail.rows - 1);
        let rect = [first[0], first[1], first[2], last[3]];
        let layout = detail_layout(rect, detail, content.columns, 1.0);
        assert_eq!(layout.tools.len(), 2, "copy the hash, copy the subject");
        assert_eq!(layout.parents.len(), 2);

        let at = |box_: [f32; 4]| {
            detail_part_at(
                rect,
                detail,
                content.columns,
                1.0,
                (box_[0] + box_[2]) / 2.0,
                (box_[1] + box_[3]) / 2.0,
            )
        };
        assert_eq!(at(layout.parents[0]), Some(GraphDetailPart::Parent(0)));
        assert_eq!(at(layout.parents[1]), Some(GraphDetailPart::Parent(1)));
        for (box_, part) in &layout.tools {
            assert_eq!(at(*box_), Some(*part));
        }
        assert!(
            layout
                .tools
                .iter()
                .any(|(_, part)| *part == GraphDetailPart::CopyHash)
                && layout
                    .tools
                    .iter()
                    .any(|(_, part)| *part == GraphDetailPart::CopySubject)
        );
        // Two verbs, two marks: a glyph drawn twice would leave the reader
        // guessing which copy is which.
        assert_ne!(
            detail_part_mark(GraphDetailPart::CopyHash),
            detail_part_mark(GraphDetailPart::CopySubject)
        );
        // The block's own padding is not a button.
        assert_eq!(
            detail_part_at(
                rect,
                detail,
                content.columns,
                1.0,
                rect[0] + 1.0,
                rect[1] + 1.0
            ),
            None
        );

        // And what the `Ok` notice each of them raises says — a hash whole, a
        // subject only as far as a card can carry it.
        assert_eq!(graph_copied("36d3949"), "Copied 36d3949");
        let essay = "z".repeat(GRAPH_COPIED_MAX_CHARS + 10);
        let said = graph_copied(&essay);
        assert!(said.ends_with(GRAPH_BODY_ELLIPSIS));
        assert_eq!(
            said.chars().count(),
            "Copied ".chars().count() + GRAPH_COPIED_MAX_CHARS + 1
        );
        assert_eq!(
            graph_copied(&"z".repeat(GRAPH_COPIED_MAX_CHARS)),
            format!("Copied {}", "z".repeat(GRAPH_COPIED_MAX_CHARS)),
            "a subject that exactly fits is not marked"
        );
    }

    /// D2 — a parent already loaded is arrived at, and the row it stands on is
    /// the row the reveal is about.
    #[test]
    fn a_parent_already_loaded_is_arrived_at_on_its_own_row() {
        let state = state_with_status(straight(20), DIRTY);
        let content = frame(&state, Some("c0"), WIDE);
        let seek = GraphSeek {
            hash: "c4".to_owned(),
            pages: 0,
        };
        assert_eq!(graph_seek_step(&state, &seek), GraphSeekStep::Arrived(4));
        // The Uncommitted row stands above the commits — collapsed, because a
        // commit is what is open — and the open commit's own block stands
        // between row zero and this one.
        let block = content.open_commit.expect("c0 is open").1;
        assert_eq!(content.commit_row(4), 1 + 4 + block);
        assert_eq!(
            content.commit_row(0),
            1,
            "a commit above the expansion is not shifted by it"
        );
    }

    /// D2 — a parent further back than the loaded pages is paged towards, up to
    /// the cap, and then said out loud.
    ///
    /// MUTATION: drop the cap and a parent git rewrote out from under us pages
    /// to the end of the repository one subprocess at a time, with nothing on
    /// screen to say why.
    #[test]
    fn a_seek_pages_towards_its_commit_and_gives_up_at_the_cap() {
        let state = state_of(straight(20), true);
        let mut seek = GraphSeek {
            hash: "nowhere".to_owned(),
            pages: 0,
        };
        // Every page up to the cap is another ask.
        while seek.pages < GRAPH_SEEK_MAX_PAGES {
            assert_eq!(
                graph_seek_step(&state, &seek),
                GraphSeekStep::NeedPage,
                "page {} is still worth asking for",
                seek.pages
            );
            seek.pages += 1;
        }
        assert_eq!(graph_seek_step(&state, &seek), GraphSeekStep::GaveUp);
        assert_eq!(
            graph_seek_gave_up(&"f".repeat(40)),
            "Commit fffffff is further back than the loaded history"
        );

        // A history that has *ended* gives up whatever the count says: there is
        // no page left to ask for.
        let ended = state_of(straight(20), false);
        assert_eq!(
            graph_seek_step(
                &ended,
                &GraphSeek {
                    hash: "nowhere".to_owned(),
                    pages: 0,
                }
            ),
            GraphSeekStep::GaveUp
        );
        // And the commit is arrived at even on the last allowed page: the cap
        // bounds how long we look, not when we believe what we found.
        assert_eq!(
            graph_seek_step(
                &ended,
                &GraphSeek {
                    hash: "c7".to_owned(),
                    pages: GRAPH_SEEK_MAX_PAGES,
                }
            ),
            GraphSeekStep::Arrived(7)
        );
        // Nothing read yet is not a page spent.
        let cold = GraphState::new(std::path::PathBuf::from(r"D:\repo"));
        assert_eq!(
            graph_seek_step(
                &cold,
                &GraphSeek {
                    hash: "c1".to_owned(),
                    pages: 0,
                }
            ),
            GraphSeekStep::Waiting
        );
    }

    /// D4 — a commit's file row wears its letter and its counts, and a binary
    /// file says git had no lines to count.
    #[test]
    fn a_commits_file_row_wears_its_letter_and_its_counts() {
        let mut state = state_of(straight(20), false);
        let hash = "c0".to_owned();
        assert!(state.cache.begin_commit_files(&hash).is_some());
        assert!(state.cache.accept(crate::git::GitAnswer::CommitFiles {
            root: std::path::PathBuf::from(r"D:\repo"),
            hash,
            outcome: Ok(vec![
                crate::git::GitCommitFile {
                    path: "src/main.rs".to_owned(),
                    code: crate::git::StatusCode::Modified,
                    renamed_from: None,
                    stat: Some(crate::git::GitFileStat {
                        added: 12,
                        removed: 3,
                    }),
                },
                crate::git::GitCommitFile {
                    path: "logo.png".to_owned(),
                    code: crate::git::StatusCode::Added,
                    renamed_from: None,
                    stat: None,
                },
            ]),
        }));
        let content = frame(&state, Some("c0"), WIDE);
        let files: Vec<&GraphFileRow> = content
            .rows
            .iter()
            .filter_map(|row| match row {
                GraphViewRow::File(file) => Some(file),
                _ => None,
            })
            .collect();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].badge.expect("a letter").letter, 'M');
        assert_eq!(
            files[0].stat,
            Some(crate::git::GitFileStat {
                added: 12,
                removed: 3
            })
        );
        assert_eq!(files[1].badge.expect("a letter").letter, 'A');
        assert_eq!(
            files[1].stat, None,
            "binary: git has no lines here, which is not zero lines"
        );
        assert!(files[1].tooltip.contains("Binary"));
        assert!(files[0].tooltip.contains("12 lines added, 3 lines removed"));
    }

    /// D6 — comparing two commits hangs one block off the **older** of them, and
    /// lights both.
    ///
    /// MUTATION: order the pair by the gesture instead of by the graph, and
    /// `Comparing` reads backwards half the time — a diff whose arrow points the
    /// way history does not run.
    #[test]
    fn a_comparison_hangs_its_block_off_the_older_row_and_lights_both() {
        let state = state_with_status(straight(20), DIRTY);
        // The *newer* row is the one turned over, so this is also the case where
        // the block does not stand under the row that was opened.
        let content = looked(
            &state,
            GraphLook {
                expanded: Some("c2"),
                compare: Some("c6"),
                selected: None,
            },
            WIDE,
        );
        let detail = detail_of(&content).expect("a comparison has a head");
        let GraphDetail::Compare(compare) = &detail.detail else {
            panic!("compare mode replaces the story rather than standing beside it");
        };
        assert_eq!(compare.a, "c6", "older first");
        assert_eq!(compare.b.as_deref(), Some("c2"));
        assert_eq!(compare.head, "Comparing c6 \u{2192} c2");
        assert_eq!(
            content.compare_pair,
            Some(("c6".to_owned(), Some("c2".to_owned())))
        );
        // The Uncommitted row stands above the commits, so `c6` is row seven and
        // `c2` is row three; the block stands under the older of the two.
        assert_eq!(content.compare_rows, Some((7, 3)));
        assert_eq!(detail.index, 8);
        // Turning it round changes nothing: the order is the graph's, not the
        // gesture's.
        let swapped = looked(
            &state,
            GraphLook {
                expanded: Some("c6"),
                compare: Some("c2"),
                selected: None,
            },
            WIDE,
        );
        assert_eq!(swapped.compare_pair, content.compare_pair);
        assert_eq!(swapped.compare_rows, content.compare_rows);

        // **Against the working tree** (D6): the far end is absent, because that
        // is git's own grammar for "what is on disk".
        let against_tree = looked(
            &state,
            GraphLook {
                expanded: Some("c2"),
                compare: Some(GRAPH_UNCOMMITTED_HASH),
                selected: None,
            },
            WIDE,
        );
        assert_eq!(
            against_tree.compare_pair,
            Some(("c2".to_owned(), None)),
            "the working tree can never be the older end"
        );
        assert_eq!(
            against_tree.compare_rows,
            Some((3, 0)),
            "and its row is the top of the list"
        );
        let GraphDetail::Compare(compare) = &detail_of(&against_tree).expect("a head").detail
        else {
            panic!("a comparison")
        };
        assert_eq!(compare.head, "Comparing c2 \u{2192} working tree");

        // A row compared with itself is not a comparison.
        let same = looked(
            &state,
            GraphLook {
                expanded: Some("c2"),
                compare: Some("c2"),
                selected: None,
            },
            WIDE,
        );
        assert_eq!(same.compare_pair, None);
        assert!(matches!(
            detail_of(&same).expect("still open").detail,
            GraphDetail::Commit(_)
        ));
    }

    /// D6 — a comparison's file rows open the diff of the pair, and never of one
    /// end of it.
    #[test]
    fn a_comparisons_file_rows_open_the_diff_of_the_pair() {
        let mut state = state_of(straight(20), false);
        assert!(state.cache.begin_compare_files("c6", Some("c2")).is_some());
        assert!(state.cache.accept(crate::git::GitAnswer::CompareFiles {
            root: std::path::PathBuf::from(r"D:\repo"),
            a: "c6".to_owned(),
            b: Some("c2".to_owned()),
            outcome: Ok(vec![crate::git::GitCommitFile {
                path: "src/main.rs".to_owned(),
                code: crate::git::StatusCode::Modified,
                renamed_from: None,
                stat: Some(crate::git::GitFileStat {
                    added: 1,
                    removed: 1,
                }),
            }]),
        }));
        let content = looked(
            &state,
            GraphLook {
                expanded: Some("c2"),
                compare: Some("c6"),
                selected: None,
            },
            WIDE,
        );
        let file = content
            .rows
            .iter()
            .find_map(|row| match row {
                GraphViewRow::File(file) => Some(file),
                _ => None,
            })
            .expect("the comparison lists its files");
        let root = std::path::PathBuf::from(r"D:\repo");
        assert_eq!(
            row_open(&GraphViewRow::File(file.clone()), &root),
            Some(crate::git_panel::GitRowOpen::Document {
                source: crate::preview::PreviewSource::GitDiffRange {
                    root,
                    a: "c6".to_owned(),
                    b: Some("c2".to_owned()),
                    path: "src/main.rs".to_owned(),
                },
                name: crate::git_panel::git_document_name("src/main.rs"),
                renamed_from: None,
            })
        );
    }

    /// D6 — `Esc` gives up the comparison before it gives up the accordion, and
    /// `Ctrl+Enter` needs a row already open to be the other end of.
    #[test]
    fn escape_leaves_a_comparison_before_it_collapses_the_row() {
        let state = state_of(straight(20), false);
        let comparing = looked(
            &state,
            GraphLook {
                expanded: Some("c2"),
                compare: Some("c6"),
                selected: Some(6),
            },
            WIDE,
        );
        assert_eq!(
            graph_key(&comparing, GraphKey::Escape),
            GraphKeyAction::LeaveCompare,
            "the innermost thing goes first"
        );
        // With the comparison gone, the same key folds the row.
        let open = looked(
            &state,
            GraphLook {
                expanded: Some("c2"),
                compare: None,
                selected: Some(2),
            },
            WIDE,
        );
        assert_eq!(
            graph_key(&open, GraphKey::Escape),
            GraphKeyAction::Collapse(2)
        );
        assert_eq!(
            graph_key(&open, GraphKey::Compare),
            GraphKeyAction::Compare(2)
        );
        // And with nothing open, `Ctrl+Enter` has no first end to pair with.
        let shut = looked(
            &state,
            GraphLook {
                expanded: None,
                compare: None,
                selected: Some(2),
            },
            WIDE,
        );
        assert_eq!(graph_key(&shut, GraphKey::Compare), GraphKeyAction::None);
    }

    /// v2 ② — the arrows step **over** the detail block rather than through it.
    ///
    /// The block claims list rows so the geometry stays a multiplication; those
    /// rows answer no press, so the selection may not stand on one. MUTATION:
    /// drop [`step_over_detail`] and `↓` off an open commit spends a keypress on
    /// each line of its own message.
    #[test]
    fn the_arrows_step_over_the_detail_block() {
        let state = state_of(
            told(
                "A body long enough that the block is several rows tall, which is what makes this test about more than one row of prose at all.",
            ),
            false,
        );
        let open = looked(
            &state,
            GraphLook {
                expanded: Some("c0"),
                compare: None,
                selected: Some(0),
            },
            WIDE,
        );
        let (start, rows) = open.detail_rows.expect("an open commit has a block");
        assert_eq!(start, 1, "the block is the row under the one it belongs to");
        assert!(rows > 1, "and it is taller than one row: {rows}");
        assert_eq!(
            graph_key(&open, GraphKey::Down),
            GraphKeyAction::Select(start + rows),
            "one press leaves the whole block, not one line of it"
        );
        // And back up over it from the row underneath.
        let below = looked(
            &state,
            GraphLook {
                expanded: Some("c0"),
                compare: None,
                selected: Some(start + rows),
            },
            WIDE,
        );
        assert_eq!(graph_key(&below, GraphKey::Up), GraphKeyAction::Select(0));
    }

    /// v2 ② — the block is one row of the list that spans several, and every
    /// index inside it finds it.
    #[test]
    fn every_row_the_detail_block_claims_maps_back_to_it() {
        let open = GraphOpen {
            at: 2,
            detail: 3,
            files: 2,
        };
        assert_eq!(item_at(2, None, Some(open)), Some(GraphItem::Commit(2)));
        for index in 3..=5 {
            assert_eq!(
                item_at(index, None, Some(open)),
                Some(GraphItem::Detail),
                "row {index} is the block's"
            );
        }
        assert_eq!(
            item_at(6, None, Some(open)),
            Some(GraphItem::File { commit: 2, file: 0 })
        );
        assert_eq!(
            item_at(7, None, Some(open)),
            Some(GraphItem::File { commit: 2, file: 1 })
        );
        assert_eq!(item_at(8, None, Some(open)), Some(GraphItem::Commit(3)));
    }
}
