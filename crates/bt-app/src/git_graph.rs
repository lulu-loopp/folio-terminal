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
}

impl LaneWalker {
    #[must_use]
    pub fn rows(&self) -> &[GraphRow] {
        &self.rows
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
            let row = self.step(commit);
            self.rows.push(row);
        }
    }

    /// Forget everything — a history that was rewritten under us is not a
    /// history this walker can resume.
    pub fn reset(&mut self) {
        self.lanes.clear();
        self.rows.clear();
    }

    fn step(&mut self, commit: &GitCommit) -> GraphRow {
        let before: Vec<bool> = self.lanes.iter().map(Option::is_some).collect();

        // Every lane already waiting for this commit. The first is where the dot
        // stands; the rest are other branches whose next step is this same
        // commit, and they curve in.
        let claims: Vec<usize> = self
            .lanes
            .iter()
            .enumerate()
            .filter(|(_, lane)| lane.as_deref() == Some(commit.hash.as_str()))
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
        match commit.parents.split_first() {
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
    Commit(GraphCommitRow),
    /// A file the open commit touched (R15's accordion, in the graph's seat).
    File(GraphFileRow),
}

/// One commit, laid out and measured.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphCommitRow {
    /// Where this row sits in the whole list — what a press is answered with,
    /// and what the painter offsets by.
    pub index: usize,
    pub hash: String,
    pub short: String,
    pub short_width: f32,
    pub subject: String,
    pub time: String,
    pub time_width: f32,
    pub tooltip: String,
    pub refs: Vec<GraphRefPill>,
    /// The lanes this row draws — [`GraphRow`], carried on the row so the
    /// painter never indexes a second collection.
    pub lanes: GraphRow,
    pub expanded: bool,
}

/// One file of the open commit.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphFileRow {
    pub index: usize,
    pub hash: String,
    pub path: String,
    pub renamed_from: Option<String>,
    pub tooltip: String,
}

impl GraphViewRow {
    #[must_use]
    pub fn index(&self) -> usize {
        match self {
            Self::Commit(row) => row.index,
            Self::File(row) => row.index,
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
    pub empty: Option<&'static str>,
    pub banner: Option<String>,
    /// The last row index the held width was needed for — see [`LaneWidthHold`].
    pub lane_width_until: usize,
    /// The reader is near the end and there is more history (R23's auto-paging).
    pub wants_more: bool,
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
}

impl GraphState {
    #[must_use]
    pub fn new(root: std::path::PathBuf) -> Self {
        Self {
            cache: crate::git::GitCache::at_root(root, crate::git::GitRole::Graph),
            lanes: LaneWalker::default(),
            walked: 0,
        }
    }

    /// Bring the lane layout up to date with whatever the cache now holds.
    ///
    /// **A shorter list is a different history.** Pages only ever extend a log
    /// (`GitCache::accept` refuses one that does not start where the list ends),
    /// so the single way the count can fall is a refresh — a checkout, a commit
    /// — and what that gives is a history the running lane state is not about.
    /// Resuming into it would draw roads that belong to the branch you left.
    pub fn sync(&mut self) {
        let Some(log) = self.cache.log().ready() else {
            return;
        };
        if log.commits.len() < self.walked {
            self.lanes.reset();
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
    /// The rows' own box — the body less the head and less any banner.
    pub viewport: [f32; 4],
    pub head: Option<[f32; 4]>,
    pub banner: Option<[f32; 4]>,
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
}

/// Lay a graph out inside a preview body.
#[must_use]
pub fn graph_geometry(body: [f32; 4], content: &GraphContent, scale: f32) -> GraphGeometry {
    let banner_height = (crate::git_panel::GIT_BANNER_HEIGHT_LOGICAL_PX * scale).round();
    let (banner, rest) = match content.banner {
        Some(_) => {
            let bottom = (body[1] + banner_height).min(body[3]);
            (
                Some([body[0], body[1], body[2], bottom]),
                [body[0], bottom, body[2], body[3]],
            )
        }
        None => (None, body),
    };
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
    let row_height = (GRAPH_ROW_HEIGHT_LOGICAL_PX * scale).round().max(1.0);
    let pad_top = (GRAPH_PADDING_TOP_LOGICAL_PX * scale).round();
    let pad_bottom = (GRAPH_PADDING_BOTTOM_LOGICAL_PX * scale).round();
    #[allow(clippy::cast_precision_loss)]
    let content_height = pad_top + content.total_rows as f32 * row_height + pad_bottom;
    let max_scroll = (content_height - (viewport[3] - viewport[1])).max(0.0);
    GraphGeometry {
        viewport,
        head,
        banner,
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

/// Turn what a graph document knows into what it draws.
///
/// `body` and `scroll_px` are here rather than in the painter because **the
/// window is part of the derivation** (R23): which rows exist on this frame is
/// decided by where the body is and how far down the list is, and a build that
/// did not know those two things could only build all of them.
#[must_use]
pub fn build(
    state: &GraphState,
    expanded: Option<&str>,
    body: [f32; 4],
    scroll_px: f32,
    hold: LaneWidthHold,
    scale: f32,
    measure: &mut Measure<'_>,
) -> GraphContent {
    let mut content = GraphContent {
        scroll_px,
        ..GraphContent::default()
    };
    let cache = &state.cache;

    let log = match cache.log() {
        crate::git::GitSlot::Ready(log) => log,
        crate::git::GitSlot::Failed(fault) => {
            content.empty = Some(crate::git_panel::GIT_READING);
            content.banner = Some(crate::git_panel::fault_sentence(fault));
            return content;
        }
        crate::git::GitSlot::Idle | crate::git::GitSlot::Pending => {
            content.empty = Some(crate::git_panel::GIT_READING);
            return content;
        }
    };

    // The head is the panel's own sentence about the same repository (R20).
    content.head = Some(crate::git_panel::head_of(cache, scale, measure));
    if let Some(words) = cache.write_error() {
        content.banner = Some(words.to_owned());
    }

    if log.commits.is_empty() {
        content.empty = Some(crate::git_panel::GIT_NO_COMMITS);
        return content;
    }

    // **Where the accordion is, as a row index.** Held as an index rather than
    // as a hash so that mapping a row number back to what stands on it is
    // arithmetic — see [`item_at`] — which is the other half of what makes a
    // ten-thousand-row list cost a screenful.
    let open = expanded.and_then(|hash| {
        let at = log.commits.iter().position(|commit| commit.hash == hash)?;
        let files = match cache.commit_files(hash) {
            Some(crate::git::GitSlot::Ready(files)) => files.clone(),
            _ => Vec::new(),
        };
        Some((at, files))
    });
    let expansion = open.as_ref().map_or(0, |(_, files)| files.len());
    content.total_rows = log.commits.len() + expansion;

    let geometry = graph_geometry(body, &content, scale);
    let window = geometry.window(content.total_rows);

    // R18's hysteresis, decided over the window that is about to be drawn.
    let lanes = state.lanes();
    let mut needed = 1;
    let mut widest_at = window.start;
    for index in window.clone() {
        let Some(commit) = item_at(index, open.as_ref().map(|(at, files)| (*at, files.len())))
        else {
            continue;
        };
        let GraphItem::Commit(at) = commit else {
            continue;
        };
        let Some(row) = lanes.get(at) else { continue };
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

    let hash_font = crate::git_panel::GIT_HASH_FONT_LOGICAL_PX * scale;
    let time_font = crate::git_panel::GIT_TIME_FONT_LOGICAL_PX * scale;
    let ref_font = GRAPH_REF_FONT_LOGICAL_PX * scale;
    for index in window {
        match item_at(index, open.as_ref().map(|(at, files)| (*at, files.len()))) {
            Some(GraphItem::Commit(at)) => {
                let Some(commit) = log.commits.get(at) else {
                    continue;
                };
                let lane_row = lanes.get(at).cloned().unwrap_or_default();
                let dot = lane_row.dot;
                content.rows.push(GraphViewRow::Commit(GraphCommitRow {
                    index,
                    short_width: measure(&commit.short, hash_font),
                    time_width: measure(&commit.time_relative, time_font),
                    tooltip: commit_tooltip(commit),
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
                let Some((_, files)) = open.as_ref() else {
                    continue;
                };
                let Some(entry) = files.get(file) else {
                    continue;
                };
                let Some(hash) = log.commits.get(commit).map(|c| c.hash.clone()) else {
                    continue;
                };
                content.rows.push(GraphViewRow::File(GraphFileRow {
                    index,
                    hash,
                    tooltip: match &entry.renamed_from {
                        Some(from) => format!("{} - renamed from {from}", entry.path),
                        None => entry.path.clone(),
                    },
                    path: entry.path.clone(),
                    renamed_from: entry.renamed_from.clone(),
                }));
            }
            None => {}
        }
    }
    content
}

/// The tooltip a commit row carries — R16's author, and the merge's own line.
fn commit_tooltip(commit: &GitCommit) -> String {
    if commit.parents.len() > 1 {
        format!(
            "Merge commit - another branch's history joins here\n{}\n{}\nDouble-click to check this commit out",
            commit.subject, commit.author
        )
    } else {
        format!(
            "{}\n{}\nDouble-click to check this commit out",
            commit.subject, commit.author
        )
    }
}

/// What stands on one row of the list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphItem {
    /// The commit at this index of the log.
    Commit(usize),
    /// A file of the one open commit.
    File { commit: usize, file: usize },
}

/// Which item row `index` is, given where the accordion is open.
///
/// **Arithmetic** (R23): one expansion, at one place, so the whole mapping is
/// three comparisons. A `Vec` of rows would be a list the length of the
/// repository, built to answer a question about twenty-five of them.
#[must_use]
pub fn item_at(index: usize, open: Option<(usize, usize)>) -> Option<GraphItem> {
    let Some((at, files)) = open else {
        return Some(GraphItem::Commit(index));
    };
    if index <= at {
        return Some(GraphItem::Commit(index));
    }
    if index <= at + files {
        return Some(GraphItem::File {
            commit: at,
            file: index - at - 1,
        });
    }
    Some(GraphItem::Commit(index - files))
}

/// What a press on a graph row does.
#[must_use]
pub fn row_open(
    row: &GraphViewRow,
    root: &std::path::Path,
) -> Option<crate::git_panel::GitRowOpen> {
    match row {
        GraphViewRow::Commit(commit) => Some(crate::git_panel::GitRowOpen::Expand {
            hash: commit.hash.clone(),
        }),
        GraphViewRow::File(file) => Some(crate::git_panel::GitRowOpen::Document {
            source: crate::preview::PreviewSource::GitShow {
                root: root.to_owned(),
                hash: file.hash.clone(),
                path: file.path.clone(),
            },
            name: crate::git_panel::git_document_name(&file.path),
            renamed_from: file.renamed_from.clone(),
        }),
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
    }
}

// ── the paint ──────────────────────────────────────────────────────────────

/// What the pointer is on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GraphHover {
    pub row: Option<usize>,
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

    if let (Some(strip), Some(words)) = (geometry.banner, content.banner.as_ref()) {
        crate::git_panel::push_git_banner(strip, words, scale, palette, labels);
    }
    if let (Some(rect), Some(head)) = (geometry.head, content.head.as_ref()) {
        crate::git_panel::push_git_masthead(head, rect, scale, palette, (labels, sprites), &|r| r);
    }

    if let Some(sentence) = content.empty {
        labels.push(ChromeLabel {
            text: sentence.to_owned(),
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
        let rect = geometry.row_rect(row.index());
        if !visible(rect) {
            continue;
        }
        let hovered = hover.row == Some(row.index());
        match row {
            GraphViewRow::Commit(commit) => {
                let lit = hovered || commit.expanded;
                push_row_ground(rect, lit, scale, palette, sprites, &crop);
                push_commit_row(
                    commit,
                    rect,
                    content.lane_width,
                    lit,
                    scale,
                    palette,
                    (labels, sprites),
                    &crop,
                );
            }
            GraphViewRow::File(file) => {
                push_row_ground(rect, hovered, scale, palette, sprites, &crop);
                let pad = (GRAPH_FILE_INDENT_LOGICAL_PX * scale).round();
                let box_ = [
                    rect[0] + pad,
                    rect[1],
                    (rect[2] - (GRAPH_ROW_PADDING_X_LOGICAL_PX * scale).round()).max(rect[0] + pad),
                    rect[3],
                ];
                labels.push(ChromeLabel {
                    text: file.path.clone(),
                    rect: box_,
                    font_size_px: crate::git_panel::GIT_TIME_FONT_LOGICAL_PX * scale,
                    color: if hovered {
                        palette.files_row_muted_hover
                    } else {
                        palette.files_row_muted
                    },
                    align_right: false,
                    align_center: false,
                    letter_spacing_em: 0.0,
                    weight: ChromeLabelWeight::Regular,
                    tabular_numerals: false,
                    clip: Some(crop(box_)),
                });
            }
        }
    }
}

/// `.ggrow:hover, .ggrow.open { background: var(--hover) }` (G82).
///
/// The tree's `--hover` and not the panel's, because the graph stands on the
/// pane's own body exactly as the tree does — the panel's rows sit on a `--panel`
/// card and their hover is mixed over that.
fn push_row_ground(
    rect: [f32; 4],
    lit: bool,
    scale: f32,
    palette: &ChromePalette,
    sprites: &mut Vec<ChromeSprite>,
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    if !lit {
        return;
    }
    sprites.push(ChromeSprite::new(
        ChromeMark::ControlPill {
            radius_px: (GRAPH_ROW_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32,
        },
        crop(rect),
        palette.files_row_hover,
    ));
}

/// One commit row: the lanes, the names it wears, and the four columns of R21.
#[allow(clippy::too_many_arguments)]
fn push_commit_row(
    commit: &GraphCommitRow,
    rect: [f32; 4],
    lane_width: usize,
    lit: bool,
    scale: f32,
    palette: &ChromePalette,
    out: (&mut Vec<ChromeLabel>, &mut Vec<ChromeSprite>),
    crop: &dyn Fn([f32; 4]) -> [f32; 4],
) {
    let (labels, sprites) = out;
    let pad = (GRAPH_ROW_PADDING_X_LOGICAL_PX * scale).round();
    let gap = (GRAPH_ROW_GAP_LOGICAL_PX * scale).round();
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
    for lane in &commit.lanes.upper {
        segment(*lane, rect[1], mid);
    }
    for lane in &commit.lanes.lower {
        segment(*lane, mid, rect[3]);
    }

    // The curves. One shape, four mirrorings — see [`ChromeMark::GraphCurve`].
    let mut curve = |lane: usize, opening: bool| {
        let dot_x = lane_x(commit.lanes.dot);
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
                    flip_x: lane < commit.lanes.dot,
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
    for lane in &commit.lanes.close {
        curve(*lane, false);
    }
    for lane in &commit.lanes.open {
        curve(*lane, true);
    }

    // The dot, at full strength: `.ggcell circle { opacity:1 }` (G78) — the node
    // is brighter than the line it stands on, which is the whole of the layering.
    let dot_x = lane_x(commit.lanes.dot);
    sprites.push(ChromeSprite::new(
        ChromeMark::ControlPill {
            radius_px: (dot / 2.0).round().max(1.0) as u32,
        },
        crop([
            dot_x - dot / 2.0,
            mid - dot / 2.0,
            dot_x + dot / 2.0,
            mid + dot / 2.0,
        ]),
        ink(commit.lanes.dot),
    ));

    // ── the columns (R21): graph, refs, message, time, hash ──
    let muted = if lit {
        palette.files_row_muted_hover
    } else {
        palette.files_row_muted
    };
    let hash_right = rect[2] - pad;
    let hash_rect = [
        (hash_right - commit.short_width).max(rect[0]),
        rect[1],
        hash_right,
        rect[3],
    ];
    labels.push(ChromeLabel {
        text: commit.short.clone(),
        rect: hash_rect,
        font_size_px: crate::git_panel::GIT_HASH_FONT_LOGICAL_PX * scale,
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
        font_size_px: crate::git_panel::GIT_TIME_FONT_LOGICAL_PX * scale,
        color: muted,
        align_right: true,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: true,
        clip: Some(crop(time_rect)),
    });

    // The pills, left to right after the graph column.
    let pill_height = (GRAPH_REF_HEIGHT_LOGICAL_PX * scale).round().max(1.0);
    let pill_pad = (GRAPH_REF_PADDING_X_LOGICAL_PX * scale).round();
    let pill_radius = (GRAPH_REF_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    let pill_edge = (GRAPH_REF_EDGE_LOGICAL_PX * scale).round().max(1.0) as u32;
    let pill_top = ((rect[1] + rect[3] - pill_height) / 2.0).round();
    let mut cursor = left + column + gap;
    for pill in &commit.refs {
        let width = pill.text_width + pill_pad * 2.0;
        if cursor + width > time_rect[0] - gap {
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

    let subject_rect = [cursor, rect[1], (time_rect[0] - gap).max(cursor), rect[3]];
    labels.push(ChromeLabel {
        text: commit.subject.clone(),
        rect: subject_rect,
        font_size_px: crate::git_panel::GIT_ROW_FONT_LOGICAL_PX * scale,
        color: if lit {
            palette.files_row_text_hover
        } else {
            palette.files_row_text
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(crop(subject_rect)),
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
            author: "t".to_owned(),
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
            None,
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
            None,
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
            None,
            body,
            0.0,
            LaneWidthHold::default(),
            1.0,
            &mut measure,
        );
        assert!(!top.wants_more, "the top of two hundred wants nothing yet");
        let bottom = build(
            &state,
            None,
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
            None,
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
            None,
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
            None,
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
        assert_eq!(item_at(7, None), Some(GraphItem::Commit(7)));
        let open = Some((2, 3));
        assert_eq!(item_at(2, open), Some(GraphItem::Commit(2)));
        assert_eq!(
            item_at(3, open),
            Some(GraphItem::File { commit: 2, file: 0 })
        );
        assert_eq!(
            item_at(5, open),
            Some(GraphItem::File { commit: 2, file: 2 })
        );
        assert_eq!(item_at(6, open), Some(GraphItem::Commit(3)));
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
}
