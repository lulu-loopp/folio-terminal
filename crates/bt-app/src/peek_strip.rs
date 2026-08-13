//! The layout peek: hover a tab that holds a layout, see the layout it holds.
//!
//! A tab with two panes in it says one name, and one name cannot describe a
//! shape. The peek answers the question the name cannot — *what is in there* —
//! with a schematic of the tab's own seat tree and a list of what each leaf is
//! called (mock-up 6228-6259, `.layout-peek` at 1718-1728).
//!
//! # Not a tooltip, and deliberately not the tooltip's singleton
//!
//! The two are near relatives: both are hover-summoned floating boxes, both are
//! `pointer-events: none`, both go the instant the pointer leaves. They are
//! nonetheless two hosts, because they disagree about the two things a hover
//! popup *is*:
//!
//! * **The clock.** 350ms here against the tip's 380ms (mock-up 6237 against
//!   8716). Not a rounding difference — a deliberate ordering, and §6 of the
//!   ticket pins the consequence: on a tab that qualifies for both, the peek is
//!   due first and the peek wins.
//! * **The fade.** `.tip` carries `opacity: 0; transition: opacity .09s ease`
//!   (mock-up 1217). `.layout-peek` carries no transition at all — it is a
//!   `display: none` / `display: block` switch and nothing more (1719-1728). So
//!   the peek has no fade in *any* motion mode, which is why this module has no
//!   opacity clock, no [`crate::Motion`] parameter, and nothing for a
//!   reduced-motion branch to stand down. §7 asked for "no fade under reduced
//!   motion"; the mock-up's answer is that there was never a fade to remove.
//!
//! Folding these into one host would have meant a singleton with two delays and
//! a conditional fade, and then the mutual-exclusion rule — the whole point of
//! keeping them apart — would have had nothing to exclude.
//!
//! # The mutual exclusion
//!
//! Two popups must never speak at once. The mock-up does not implement this (it
//! arms both timers and lets `z-index` 60 draw the tip over the peek's 35); the
//! ticket does, and the rule is one-directional because the clocks are: the peek
//! is due at 350ms, and on promotion it disarms the tip. See
//! `Runtime::advance_layout_peek_if_due` and `Runtime::layout_peek_suppresses`.
//! A tab the peek refuses — one pane, the active tab, a drag in flight —
//! suppresses nothing, and its tip arrives at 380ms exactly as it always did.
//!
//! (`layout_peek`, never `peek`: this window already has a *hover peek* over an
//! image path in the terminal body, and the two share nothing but the word.)
//!
//! # The pieces
//!
//! * [`PeekHost`] — the settle/show state machine, the tooltip's own shape minus
//!   the fade. It knows nothing about tabs beyond an index.
//! * [`schematic`] — the seat tree as rectangles. The whole of the drawing.
//! * [`layout`] and [`build`] — the box, placed and painted.

use std::time::{Duration, Instant};

use bt_layout::{Axis, LayoutNode, RATIO_DENOM_PPM, SeatKind};
use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad};

use crate::marks::{ChromeMark, ChromeSprite, OverlayLayer};
use crate::settings::push_float_window;

/// How long the pointer must rest on a tab before its layout appears
/// (mock-up 6237).
///
/// Shorter than [`crate::tooltip::TOOLTIP_DELAY`] by 30ms, and the gap is the
/// whole of §6: a multi-pane tab qualifies for both popups, both clocks are
/// armed by the same pointer, and this one comes due first. The peek is also the
/// better answer to that hover — a schematic of what the tab holds says strictly
/// more than the tab's own name repeated back — so the shorter clock is not just
/// a tiebreak, it is the tiebreak going the right way.
pub const PEEK_DELAY: Duration = Duration::from_millis(350);

/// `border-radius: 8px` (mock-up 1723).
pub const PEEK_RADIUS_LOGICAL_PX: f32 = 8.0;
/// `border: 1px solid var(--border)` (1722).
pub const PEEK_BORDER_LOGICAL_PX: f32 = 1.0;
/// `padding: 7px` (1725).
pub const PEEK_PADDING_LOGICAL_PX: f32 = 7.0;

/// How far off the tab the box stands — `r.bottom + 5` (mock-up 6252).
///
/// One pixel less than the tip's 6, and it is the mock-up's own number rather
/// than a shared constant, because the two are not the same measurement: the tip
/// clears a control it is annotating, the peek hangs off the tab it belongs to.
///
/// The same five pixels serve both axes: beside a rail row it is the gap from
/// the row's right edge rather than from its bottom.
pub const PEEK_OFFSET_LOGICAL_PX: f32 = 5.0;

/// Which way the box hangs off the row it explains.
///
/// The mock-up needs no such choice — `showPeek` reads `getBoundingClientRect()`
/// off whichever element was hovered and always writes `top: r.bottom + 5; left:
/// r.left`, because in a document that arithmetic was only ever asked about a
/// tab in a horizontal strip. Asked about a *rail* row it answers badly: "below
/// and sharing the left edge" puts the card over the next row down and over the
/// rail itself, which is precisely what a real window showed — the schematic
/// pinned to the window's top-left corner, lying across the rail.
///
/// A vertical strip has its free space to the side, so that is where the card
/// goes. Stated as a choice the caller makes, because only the caller knows
/// which way its tabs run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeekSide {
    /// Under the tab, sharing its left edge. The horizontal strip.
    Below,
    /// To the right of the row, aligned with its top. The vertical rail.
    Beside,
}

/// How close the box may come to the window's edge — the `6` in
/// `Math.max(6, Math.min(…, win.width - pw - 6))` (mock-up 6254).
pub const PEEK_MARGIN_LOGICAL_PX: f32 = 6.0;

/// `.peek-grid { width: 210px }` (mock-up 1891).
pub const GRID_WIDTH_LOGICAL_PX: f32 = 210.0;
/// `.peek-grid { height: 92px }` (1891).
pub const GRID_HEIGHT_LOGICAL_PX: f32 = 92.0;
/// `.peek-grid { gap: 3px }` and `.mini-split { gap: 3px }` (1891, 1907) — the
/// one gutter every split in the schematic is drawn with.
pub const GRID_GAP_LOGICAL_PX: f32 = 3.0;

/// `.mini-leaf { border-radius: 4px }` (1917).
pub const LEAF_RADIUS_LOGICAL_PX: f32 = 4.0;
/// `.mini-leaf { border: 1px solid var(--border-soft) }` (1917).
pub const LEAF_BORDER_LOGICAL_PX: f32 = 1.0;
/// The `5px` of `.mini-leaf { padding: 4px 5px }` (1916).
///
/// There is no `Y` twin, and its absence is a finding rather than an omission:
/// the `4px` is inert here. `.mini-leaf` is `align-items: center`, and padding
/// that is equal top and bottom leaves the content box's middle exactly where
/// the border box's middle already was — so a mark and a name centred on the
/// cell land in the same place whether the 4px is spent or not. It would start
/// to matter only if this laid its contents out from the top edge, which is the
/// one thing the mock-up's `center` says not to do.
pub const LEAF_PADDING_X_LOGICAL_PX: f32 = 5.0;
/// `.mini-leaf { gap: 4px }` — between the mark and the name (1915).
pub const LEAF_GAP_LOGICAL_PX: f32 = 4.0;
/// `.mini-leaf { font-size: 10.5px }` (1919).
pub const LEAF_FONT_LOGICAL_PX: f32 = 10.5;
/// `.mini-leaf .ticon { font-size: 9px }` (1922) — the schematic's mark, smaller
/// than the list's because it has a cell to fit inside rather than a line.
pub const LEAF_MARK_LOGICAL_PX: f32 = 9.0;

/// `.peek-list { font-size: 11px }` (1897).
pub const LIST_FONT_LOGICAL_PX: f32 = 11.0;
/// `.peek-list .ticon { width: auto }` at the list's own 11px (1897, 1904).
pub const LIST_MARK_LOGICAL_PX: f32 = 11.0;
/// `.peek-list > span { gap: 5px }` — inside one entry (1901).
pub const LIST_ITEM_GAP_LOGICAL_PX: f32 = 5.0;
/// The `3px` of `.peek-list { gap: 3px 10px }` — between wrapped lines (1895).
pub const LIST_ROW_GAP_LOGICAL_PX: f32 = 3.0;
/// The `10px` of `.peek-list { gap: 3px 10px }` — between entries on a line.
pub const LIST_COLUMN_GAP_LOGICAL_PX: f32 = 10.0;
/// The `7px` of `.peek-list { padding: 7px 2px 1px }` (1896).
pub const LIST_PADDING_TOP_LOGICAL_PX: f32 = 7.0;
/// The `2px` of `.peek-list { padding: 7px 2px 1px }`.
pub const LIST_PADDING_X_LOGICAL_PX: f32 = 2.0;
/// The `1px` of `.peek-list { padding: 7px 2px 1px }`.
pub const LIST_PADDING_BOTTOM_LOGICAL_PX: f32 = 1.0;

// ── the status channels, at the peek's own scale ──
//
// The peek's mark slots are 9px in the schematic and 11px in the list, against
// the strip's 15px, so a dot and a ring stated in the strip's absolute pixels
// would swamp them. They are stated here as *shares of the mark slot they
// badge*, taken from the strip's own declarations, so the miniature is the same
// drawing scaled and not a second design: change the tab's numbers and these
// follow. The two offsets are negative in the strip and stay negative here —
// the dot is meant to overhang its slot on both axes, which is what makes it
// read as a badge on the mark rather than a thing standing beside it.

/// `WINDOW_TAB_STATUS_DOT / WINDOW_TAB_MARK` — the dot's side.
const DOT_SHARE_OF_MARK: f32 =
    bt_render::WINDOW_TAB_STATUS_DOT_LOGICAL_PX / bt_render::WINDOW_TAB_MARK_LOGICAL_PX;
/// `WINDOW_TAB_STATUS_DOT_TOP / WINDOW_TAB_MARK` — how far above the slot's top
/// edge the dot sits.
const DOT_TOP_SHARE_OF_MARK: f32 =
    bt_render::WINDOW_TAB_STATUS_DOT_TOP_LOGICAL_PX / bt_render::WINDOW_TAB_MARK_LOGICAL_PX;
/// `WINDOW_TAB_STATUS_DOT_RIGHT / WINDOW_TAB_MARK` — how far past the slot's
/// right edge it hangs.
const DOT_RIGHT_SHARE_OF_MARK: f32 =
    bt_render::WINDOW_TAB_STATUS_DOT_RIGHT_LOGICAL_PX / bt_render::WINDOW_TAB_MARK_LOGICAL_PX;
/// `WINDOW_TAB_RING_STROKE / WINDOW_TAB_MARK` — the progress ring's stroke.
const RING_STROKE_SHARE_OF_MARK: f32 =
    bt_render::WINDOW_TAB_RING_STROKE_LOGICAL_PX / bt_render::WINDOW_TAB_MARK_LOGICAL_PX;

// The two shape facts the drawing depends on, pinned where they cannot be
// edited apart from the constants above. Compile-time rather than a test,
// because both are decidable without running anything — and because a `#[test]`
// asserting a constant is a test that can only ever pass.
const _: () = assert!(
    DOT_TOP_SHARE_OF_MARK < 0.0 && DOT_RIGHT_SHARE_OF_MARK < 0.0,
    "both offsets are negative, which is what 'the dot overhangs its slot' means"
);
const _: () = assert!(
    DOT_SHARE_OF_MARK > 0.0 && DOT_SHARE_OF_MARK < 1.0,
    "the badge is a fraction of the slot it badges, never a replacement for it"
);

/// Where a status dot goes, given the mark slot it badges.
///
/// One function for both the schematic's cell and the list's row, because they
/// are the same badge on two differently sized slots — which is exactly the
/// thing that would drift if each laid it out itself.
#[must_use]
fn status_dot_rect(mark: [f32; 4]) -> [f32; 4] {
    let slot = mark[2] - mark[0];
    let side = (slot * DOT_SHARE_OF_MARK).round().max(1.0);
    let left = (mark[2] - slot * DOT_RIGHT_SHARE_OF_MARK - side).round();
    let top = (mark[1] + slot * DOT_TOP_SHARE_OF_MARK).round();
    [left, top, left + side, top + side]
}

/// The line box every other piece of chrome text is laid out in — the same 1.4
/// [`crate::tooltip`] borrows, and for the same reason: it is what
/// `shape_chrome_labels` sizes a buffer to, so a peek row agrees with every
/// label beside it.
const CHROME_LINE_HEIGHT: f32 = 1.4;

/// The fewest panes a tab must hold before it has a layout worth showing
/// (`paneCount(w) < 2` — mock-up 6232).
///
/// A lone pane's schematic is one rectangle filling the box, which is a picture
/// of nothing: it says only "there is a terminal here", which is what the tab
/// already said.
pub const PEEK_MIN_PANES: usize = 2;

/// Whether a tab has a layout worth showing, and whether this is a moment to
/// show it (L131, mock-up 6230-6236).
///
/// A free function rather than a method on the window, for two reasons. It can
/// be stated and tested without standing up a window — and it is read by both
/// the arming path and the retiring one, which is the whole point: a popup
/// outlives its own subject exactly when those two paths ask different
/// questions.
///
/// `is_active_tab` is the mock-up's rail clause in translation. It refuses a tab
/// "whose cards are already unfolded beneath it — the real thing is on screen,
/// and the popup would sit on top of it". This window has no card rail, so the
/// tab whose layout is already on screen is the active one, and its layout is
/// not a card stack under the strip but the whole window behind it.
#[must_use]
pub fn eligible(
    pane_count: usize,
    is_active_tab: bool,
    dragging: bool,
    renaming_this_tab: bool,
) -> bool {
    !dragging && !renaming_this_tab && !is_active_tab && pane_count >= PEEK_MIN_PANES
}

/// Whether a showing peek has already answered for this tooltip anchor (§6).
///
/// The other half of the mutual exclusion. Promotion silences the tip once; this
/// is what stops the *next* mouse-move inside the same tab from arming it again,
/// and without it the tip would reappear 380ms after any twitch of the hand.
///
/// Every anchor the tab owns, not merely its body: the pin carries a tip of its
/// own, and "two floating boxes never speak at once" is a rule about the boxes
/// rather than about which of them had something to say. A tab the peek is not
/// showing suppresses nothing at all, which is what leaves a one-pane tab's tip
/// working exactly as it always did.
#[must_use]
pub fn suppresses(showing: Option<usize>, anchor: crate::tooltip::TooltipAnchorId) -> bool {
    use crate::tooltip::TooltipAnchorId;
    let Some(shown) = showing else {
        return false;
    };
    matches!(
        anchor,
        TooltipAnchorId::Tab(index) | TooltipAnchorId::TabIcon(index) | TooltipAnchorId::TabPin(index)
        if index == shown
    )
}

/// One leaf, as the peek needs to know it.
///
/// Handed in rather than read out of a tree, because none of these three facts
/// live in `bt-layout`: the title comes from the pane-title channel, the focus
/// from the tab's own `Seats`, and the mark's alpha from a clock this module
/// does not own. Keeping them as data is what lets every test below state a
/// layout without standing up a session.
#[derive(Clone, Debug, PartialEq)]
pub struct PeekLeaf {
    pub kind: SeatKind,
    /// Which shell this leaf is running, as the mark it wears — `None` for a
    /// leaf that has no shell (a files column, a preview, a placeholder), which
    /// picks its mark from its kind instead.
    ///
    /// Carried rather than derived, because the schematic must not have a second
    /// opinion about which shell a pane is: the same argument
    /// [`Self::mark_opacity`] makes about the breath, and the same failure —
    /// a peek that draws a PowerShell square over the pane whose head says
    /// `Git Bash` is a schematic of a different window.
    pub profile_mark: Option<ChromeMark>,
    /// What this pane calls itself — see `seats::seat_caption`, the one channel
    /// both this and the pane head read.
    pub title: String,
    pub focused: bool,
    /// The mark's own alpha this frame: 1.0 at rest, breathing while this
    /// leaf's session is working (`mark_opacity`, which owns the clock).
    ///
    /// The peek does not compute it, because the peek must not have a second
    /// opinion about a breath the strip is already drawing.
    pub mark_opacity: f32,
    /// This leaf's status dot, or `None` when it has nothing to claim.
    ///
    /// Already resolved to a colour by `StatusClaim::dot_color`, so the loudness
    /// order — failed over bell over unread — is settled by the same `Ord` the
    /// tab strip's own aggregate is settled by, one ladder for both. What the
    /// peek adds is only that it asks the question *per leaf*: the tab wears the
    /// loudest of its fleet, and the whole point of a schematic is to say which
    /// room the noise is coming from.
    pub dot: Option<[u8; 3]>,
    /// The progress this leaf's session is reporting. Like the tab's, it
    /// *replaces* the mark rather than surrounding it.
    pub ring: Option<crate::seats::TabRing>,
}

/// One cell of the schematic: where the leaf sits and what goes in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeekCell {
    /// `[left, top, right, bottom]`, physical pixels — the `.mini-leaf` box.
    pub rect: [f32; 4],
    pub mark: [f32; 4],
    /// The status dot's box, hanging off the mark slot's top-right corner.
    /// Always computed — whether anything is drawn in it is `PeekLeaf::dot`.
    pub dot: [f32; 4],
    pub title: [f32; 4],
    pub focused: bool,
}

/// One entry of the name list under the schematic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeekRow {
    pub mark: [f32; 4],
    pub dot: [f32; 4],
    pub title: [f32; 4],
}

/// A placed peek: the box, the schematic's cells, and the list's entries.
#[derive(Clone, Debug, PartialEq)]
pub struct PeekLayout {
    /// `[left, top, right, bottom]`, physical pixels.
    pub frame: [f32; 4],
    /// One per leaf, in the tree's own in-order — the same order as the
    /// [`PeekLeaf`] slice it was laid out from.
    pub cells: Vec<PeekCell>,
    /// One per leaf, same order.
    pub rows: Vec<PeekRow>,
}

/// The singleton: which tab is settling, which is showing.
///
/// [`crate::tooltip::TooltipHost`]'s shape with one field's worth taken out. The
/// tip pairs its `showing` anchor with the instant it appeared because it owes
/// frames for 90ms afterwards; a peek that has appeared is finished, so the
/// instant would be a value nothing could ever read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeekHost {
    /// The tab the pointer is resting on, and when its peek comes due.
    settling: Option<(usize, Instant)>,
    /// The tab whose layout is on screen.
    showing: Option<usize>,
}

impl PeekHost {
    /// Track the tab under the pointer. `None` means "nothing worth peeking",
    /// and every refusal the design asks for is spelled as `None` by the caller
    /// — one pane, the active tab, a drag, a rename. Returns whether anything
    /// visible changed.
    ///
    /// Resting on the tab that is *already* showing must not re-arm anything,
    /// for the tooltip's own reason: a hand that trembles inside one tab would
    /// otherwise take the schematic down and put it back 350ms later, forever.
    pub fn observe(&mut self, tab: Option<usize>, now: Instant) -> bool {
        if tab.is_some() && tab == self.showing {
            self.settling = None;
            return false;
        }
        // The pointer left the tab, so the schematic goes at once — not a
        // fade-out, because there was no fade in.
        let hidden = self.showing.take().is_some();
        match tab {
            Some(tab) => {
                if self.settling.map(|(id, _)| id) != Some(tab) {
                    self.settling = Some((tab, now + PEEK_DELAY));
                }
            }
            None => self.settling = None,
        }
        hidden
    }

    /// Promote a candidate whose delay has elapsed. Returns whether it did.
    pub fn activate_if_due(&mut self, now: Instant) -> bool {
        let Some((tab, due)) = self.settling else {
            return false;
        };
        if now < due {
            return false;
        }
        self.settling = None;
        self.showing = Some(tab);
        true
    }

    /// Forget a subject that no longer qualifies — the tab closed, its last
    /// split closed, it became the active tab, a drag began. Returns whether a
    /// *visible* peek came down.
    ///
    /// Both states, for the tooltip's reason: a candidate left to mature past
    /// the death of its subject becomes a box that cannot be laid out and cannot
    /// stop asking for the frame it will never draw.
    pub fn retain(&mut self, eligible: impl Fn(usize) -> bool) -> bool {
        if self.settling.is_some_and(|(tab, _)| !eligible(tab)) {
            self.settling = None;
        }
        if self.showing.is_some_and(|tab| !eligible(tab)) {
            return self.showing.take().is_some();
        }
        false
    }

    /// Take the peek down and disarm the clock — any press, the window losing
    /// focus, a drag starting (§5). Returns whether anything was visible.
    pub fn hide(&mut self) -> bool {
        self.settling = None;
        self.showing.take().is_some()
    }

    /// The tab whose layout is on screen.
    #[must_use]
    pub fn active(&self) -> Option<usize> {
        self.showing
    }

    /// The next instant this host has something to do.
    ///
    /// The settle deadline, or nothing. There is no second clause because there
    /// is no fade: once shown, a peek is still, and a still popup asks the loop
    /// for no wakeups at all.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.settling.map(|(_, due)| due)
    }
}

/// The seat tree as rectangles, in the tree's own in-order.
///
/// This is the whole of the schematic, and it is the mock-up's flexbox written
/// as arithmetic. `.mini-slot` is `flex-basis: 0` with `flex-grow: ratio` and
/// `1 - ratio` (mock-up 4610-4611) inside a `.mini-split` with `gap: 3px`, and
/// two grow factors summing to one over a zero basis divide the *remaining*
/// space in exactly that proportion. So: take the gutter out first, then split
/// what is left by the ratio.
///
/// Taking the gutter out first is the load-bearing half. Splitting the full
/// extent and then insetting each half by the gutter is the intuitive
/// alternative and it is wrong twice over — it steals 3px from a pane that never
/// touches the gutter, and the error compounds with depth, so a four-way split
/// drifts visibly from the layout it claims to be a picture of.
///
/// The axis mapping needs no translation: `Axis::Row` is side-by-side and
/// `Axis::Col` is stacked (`bt_layout::geom`), which is `.mini-row` /
/// `.mini-col` exactly, and `a` takes the near side in both the mock-up and the
/// real solver. A schematic that transposed its splits would be a confident lie,
/// so the agreement is asserted by test rather than assumed.
#[must_use]
pub fn schematic(tree: &LayoutNode, rect: [f32; 4], scale: f32) -> Vec<[f32; 4]> {
    let mut cells = Vec::new();
    divide(tree, rect, GRID_GAP_LOGICAL_PX * scale, &mut cells);
    cells
}

/// One node's share of one rectangle, in-order into `out`.
fn divide(node: &LayoutNode, rect: [f32; 4], gap: f32, out: &mut Vec<[f32; 4]>) {
    let LayoutNode::Split {
        dir, ratio, a, b, ..
    } = node
    else {
        out.push(rect);
        return;
    };
    let (near, far) = match dir {
        Axis::Row => (rect[0], rect[2]),
        Axis::Col => (rect[1], rect[3]),
    };
    // The gutter first, the ratio second — see this function's caller.
    let available = (far - near - gap).max(0.0);
    let share = f64::from(ratio.ppm()) / f64::from(RATIO_DENOM_PPM);
    // Rounded, so a 1px hairline lands on a pixel rather than across two. The
    // gutter is added to the *rounded* cut, which is what keeps it exactly one
    // gutter wide however the division fell.
    let cut = (near + (f64::from(available) * share) as f32).round();
    let (rect_a, rect_b) = match dir {
        Axis::Row => (
            [rect[0], rect[1], cut, rect[3]],
            [cut + gap, rect[1], rect[2], rect[3]],
        ),
        Axis::Col => (
            [rect[0], rect[1], rect[2], cut],
            [rect[0], cut + gap, rect[2], rect[3]],
        ),
    };
    divide(a, rect_a, gap, out);
    divide(b, rect_b, gap, out);
}

/// Place the box and lay out everything in it.
///
/// `row_title_widths` is each list entry's measured text width — only the font
/// knows how wide a string is, so the caller measures and this wraps.
///
/// Returns `None` when the tree and the leaf slice disagree about how many
/// leaves there are. That is not a defensive check against a caller mistake: the
/// two arrive from different places (`Seats::tree` and the title channel) and a
/// mismatch means the peek would draw one tab's shape with another's names.
#[must_use]
pub fn layout(
    tree: &LayoutNode,
    leaves: &[PeekLeaf],
    row_title_widths: &[f32],
    host: [f32; 4],
    side: PeekSide,
    window: (f32, f32),
    scale: f32,
) -> Option<PeekLayout> {
    let count = tree.seats_in_order().len();
    if count == 0 || leaves.len() != count || row_title_widths.len() != count {
        return None;
    }
    let px = |logical: f32| logical * scale;
    let border = px(PEEK_BORDER_LOGICAL_PX);
    let pad = px(PEEK_PADDING_LOGICAL_PX);
    let grid_width = px(GRID_WIDTH_LOGICAL_PX);
    let grid_height = px(GRID_HEIGHT_LOGICAL_PX);

    // ── the list, wrapped before the box is sized, because it is what decides
    //    how tall the box is ──
    let list_mark = px(LIST_MARK_LOGICAL_PX);
    let list_text = (px(LIST_FONT_LOGICAL_PX) * CHROME_LINE_HEIGHT).round();
    // `align-items: center` on a line whose tallest item sets its height.
    let list_line = list_text.max(list_mark);
    let item_gap = px(LIST_ITEM_GAP_LOGICAL_PX);
    let column_gap = px(LIST_COLUMN_GAP_LOGICAL_PX);
    let row_gap = px(LIST_ROW_GAP_LOGICAL_PX);
    let list_pad_x = px(LIST_PADDING_X_LOGICAL_PX);
    // `max-width: 210px` under `box-sizing: border-box`: the padding comes out
    // of the 210, it is not added to it.
    let list_width = grid_width - 2.0 * list_pad_x;

    let mut wrapped: Vec<(usize, f32)> = Vec::with_capacity(count);
    let mut line = 0_usize;
    let mut pen = 0.0_f32;
    for width in row_title_widths {
        let entry = list_mark + item_gap + width;
        // An entry that cannot fit anywhere still gets a line of its own rather
        // than an infinite loop: the wrap only fires when something is already
        // on the line.
        if pen > 0.0 && pen + entry > list_width {
            line += 1;
            pen = 0.0;
        }
        wrapped.push((line, pen));
        pen += entry + column_gap;
    }
    let line_count = line + 1;
    let list_height = px(LIST_PADDING_TOP_LOGICAL_PX)
        + line_count as f32 * list_line
        + (line_count - 1) as f32 * row_gap
        + px(LIST_PADDING_BOTTOM_LOGICAL_PX);

    // ── the box ──
    let width = (grid_width + 2.0 * (pad + border)).round();
    let height = (grid_height + list_height + 2.0 * (pad + border)).round();
    let margin = px(PEEK_MARGIN_LOGICAL_PX);
    // The clamp is `Math.max(6, Math.min(…, win.size - box - 6))` in that order,
    // on whichever axis the box is free to slide along: on a window too small to
    // hold the box the `max` wins and it hangs off the far edge rather than off
    // the near one, where the tabs are.
    //
    // Neither side flips the way the tip does. There is nowhere to flip to: the
    // strip's tabs live in the title bar and above the title bar is not this
    // window, and a rail row's other side is the rail.
    let clamp = |wanted: f32, extent: f32, span: f32| {
        wanted.min(span - extent - margin).max(margin).round()
    };
    let (left, top) = match side {
        // `top: r.bottom + 5; left: r.left` (mock-up 6252-6254).
        PeekSide::Below => (
            clamp(host[0], width, window.0),
            (host[3] + px(PEEK_OFFSET_LOGICAL_PX)).round(),
        ),
        // The same five pixels turned through a right angle: clear of the row's
        // right edge, and level with its top so the eye can see which row the
        // card belongs to. The vertical clamp is what keeps a card hung off the
        // last row of a long rail from hanging out of the window.
        PeekSide::Beside => (
            clamp(host[2] + px(PEEK_OFFSET_LOGICAL_PX), width, window.0),
            clamp(host[1], height, window.1),
        ),
    };
    let frame = [left, top, left + width, top + height];

    // ── the schematic ──
    let grid_left = left + border + pad;
    let grid_top = top + border + pad;
    let leaf_border = px(LEAF_BORDER_LOGICAL_PX);
    let leaf_pad_x = px(LEAF_PADDING_X_LOGICAL_PX);
    let leaf_mark = px(LEAF_MARK_LOGICAL_PX);
    let leaf_gap = px(LEAF_GAP_LOGICAL_PX);
    let leaf_text = (px(LEAF_FONT_LOGICAL_PX) * CHROME_LINE_HEIGHT).round();

    let cells = schematic(
        tree,
        [
            grid_left,
            grid_top,
            grid_left + grid_width,
            grid_top + grid_height,
        ],
        scale,
    )
    .into_iter()
    .zip(leaves)
    .map(|(rect, leaf)| {
        let inner_left = rect[0] + leaf_border + leaf_pad_x;
        let inner_right = rect[2] - leaf_border - leaf_pad_x;
        let middle = f32::midpoint(rect[1], rect[3]);
        let mark_top = (middle - leaf_mark / 2.0).round();
        let title_top = (middle - leaf_text / 2.0).round();
        let title_left = inner_left + leaf_mark + leaf_gap;
        let mark = [
            inner_left,
            mark_top,
            inner_left + leaf_mark,
            mark_top + leaf_mark,
        ];
        PeekCell {
            rect,
            mark,
            dot: status_dot_rect(mark),
            // Deliberately not clamped to `inner_right`: a cell too narrow for
            // a name yields an inverted box, and the shaper already declines to
            // draw one (`label.rect[2] > label.rect[0]`). Clamping would have
            // produced a zero-width box that draws a single squeezed glyph —
            // `overflow: hidden` says nothing, not something illegible.
            title: [title_left, title_top, inner_right, title_top + leaf_text],
            focused: leaf.focused,
        }
    })
    .collect();

    // ── the names ──
    let list_left = grid_left + list_pad_x;
    let list_top = grid_top + grid_height + px(LIST_PADDING_TOP_LOGICAL_PX);
    let rows = wrapped
        .into_iter()
        .zip(row_title_widths)
        .map(|((line, pen), width)| {
            let line_top = list_top + line as f32 * (list_line + row_gap);
            let mark_left = list_left + pen;
            let mark_top = (line_top + (list_line - list_mark) / 2.0).round();
            let title_top = (line_top + (list_line - list_text) / 2.0).round();
            let title_left = mark_left + list_mark + item_gap;
            let mark = [
                mark_left,
                mark_top,
                mark_left + list_mark,
                mark_top + list_mark,
            ];
            PeekRow {
                mark,
                dot: status_dot_rect(mark),
                // `.peek-list { max-width: 210px }`, and the right edge is a
                // bound and not a suggestion. The wrap above already gives an
                // over-wide entry a line of its own rather than looping forever,
                // but a line of its own is still only 210px wide: without this
                // clamp the label is laid out at its full measured width and
                // paints straight out of the popup and across the terminal.
                //
                // Cropped rather than ellipsised, which is this codebase's
                // standing answer for `overflow: hidden` (see the profile menu's
                // `.recent-item`): a `ChromeLabel` clips per glyph and per pixel,
                // and a real `…` needs a measured *prefix*, which means the font
                // — and only the caller beside the renderer has that. With names
                // now cut to their last segment this is the rare case rather
                // than the common one.
                title: [
                    title_left,
                    title_top,
                    (title_left + width).min(list_left + list_width),
                    title_top + list_text,
                ],
            }
        })
        .collect();

    Some(PeekLayout { frame, cells, rows })
}

/// Paint the peek — one layer, in the tooltip's own family at the top of the
/// stack.
///
/// No `opacity` parameter, unlike the tip's [`crate::tooltip::build`]: there is
/// no fade to carry. The layer is built at full strength or not at all.
#[must_use]
pub fn build(
    layout: &PeekLayout,
    leaves: &[PeekLeaf],
    palette: &ChromePalette,
    scale: f32,
) -> Vec<OverlayLayer> {
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let mut quads: Vec<OverlayQuad> = Vec::new();

    // `0 10px 28px rgba(0,0,0,.18)` with no dark override (mock-up 1724) — the
    // combo menu's declaration exactly, which is the pair this palette already
    // carries. Not the tip's: that one *is* overridden on dark, because a tip is
    // the smallest thing that floats and needs a heavier lift to read.
    push_float_window(
        &mut quads,
        layout.frame,
        px(PEEK_RADIUS_LOGICAL_PX),
        px(PEEK_BORDER_LOGICAL_PX),
        px(bt_render::FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.menu_popup_shadow_inner_alpha),
        alpha(palette.menu_popup_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );

    let mut labels: Vec<ChromeLabel> = Vec::new();
    let mut sprites: Vec<ChromeSprite> = Vec::new();
    let leaf_radius = px(LEAF_RADIUS_LOGICAL_PX);
    let leaf_border = px(LEAF_BORDER_LOGICAL_PX);

    let label = |text: &str, rect: [f32; 4], font_size_px: f32, color: [u8; 3]| ChromeLabel {
        text: text.to_owned(),
        rect,
        font_size_px,
        color,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: None,
    };

    for (cell, leaf) in layout.cells.iter().zip(leaves) {
        // Neutral, never `--accent`: this popup carries per-pane state marks of
        // its own, and "the pane you are in" arriving in the same colour as "the
        // pane that wants you" would put two claims in one box in one colour
        // (mock-up 1920-1921).
        let (edge, face, ink) = if cell.focused {
            (
                palette.peek_leaf_focus_edge,
                palette.peek_leaf_focus_fill,
                palette.peek_leaf_focus_text,
            )
        } else {
            (
                palette.pane_head_edge,
                palette.seat_body,
                palette.pane_title,
            )
        };
        // Hairline then face, concentric — the float window's own construction,
        // one border in on every side and therefore one border less radius.
        quads.extend(bt_render::rounded_overlay_fill(
            cell.rect,
            leaf_radius,
            edge,
            1.0,
        ));
        quads.extend(bt_render::rounded_overlay_fill(
            [
                cell.rect[0] + leaf_border,
                cell.rect[1] + leaf_border,
                cell.rect[2] - leaf_border,
                cell.rect[3] - leaf_border,
            ],
            leaf_radius - leaf_border,
            face,
            1.0,
        ));

        // `.mini-leaf { overflow: hidden }`, for the one thing that cannot be
        // clipped by a rect: a mark that does not fit its cell is not drawn at
        // all, rather than drawn across the neighbour's border. The dot goes
        // with it — a badge on a mark that was not drawn is a badge on nothing.
        if cell.mark[2] <= cell.rect[2] {
            push_mark_slot(&mut sprites, leaf, cell.mark, cell.dot, palette);
        }
        labels.push(label(
            &leaf.title,
            cell.title,
            px(LEAF_FONT_LOGICAL_PX),
            ink,
        ));
    }

    for (row, leaf) in layout.rows.iter().zip(leaves) {
        push_mark_slot(&mut sprites, leaf, row.mark, row.dot, palette);
        // `--ink2` over `--menu` — the ink the tip is written in, because this
        // is the same kind of writing on the same surface.
        labels.push(label(
            &leaf.title,
            row.title,
            px(LIST_FONT_LOGICAL_PX),
            palette.menu_item_text,
        ));
    }

    vec![OverlayLayer {
        quads,
        labels,
        sprites,
        opacity: 1.0,
        body: None,
    }]
}

/// Fill one mark slot: the ring if this leaf is reporting progress, its own
/// mark if not, and the status dot over whichever of the two it was.
///
/// One function for the schematic's cell and the list's row, so a leaf can never
/// be drawn as working in the picture and idle in the names directly beneath it.
///
/// Every rule here is the tab strip's, restated at this size rather than
/// reinvented (`seats::window_chrome`'s mark slot):
///
/// - **The ring replaces the mark**, in the same box — a `match`, not two
///   pushes. While there is progress to report, the slot is for the progress.
/// - **The dot is additional** and never replaced, because it answers a
///   different question from the ring: the ring says what this pane is doing,
///   the dot says what it wants from you.
/// - **The breath lands on the mark alone.** The dot is pushed at full strength
///   and the ring carries no opacity, so a fading mark never drags a claim down
///   with it — and `mark_opacity`'s own contract already flattens the breath to
///   1.0 whenever a ring has taken the slot, which is why nothing here has to
///   ask whether the two are fighting.
fn push_mark_slot(
    sprites: &mut Vec<ChromeSprite>,
    leaf: &PeekLeaf,
    mark_rect: [f32; 4],
    dot_rect: [f32; 4],
    palette: &ChromePalette,
) {
    match leaf.ring {
        Some(ring) => {
            let slot = mark_rect[2] - mark_rect[0];
            let stroke_px = (slot * RING_STROKE_SHARE_OF_MARK).round().max(1.0) as u32;
            // The track first, a full turn under the arc. The *resting* tab's
            // track of the three the palette carries: a peek only ever shows a
            // tab that is not the active one, and its cells are drawn on the
            // pane body — the quietest surface of the three, so the track drawn
            // for the quietest tab is the one that reads correctly on it.
            sprites.push(ChromeSprite::new(
                ChromeMark::ProgressRing {
                    start_milliturns: 0,
                    sweep_milliturns: 1000,
                    stroke_px,
                },
                mark_rect,
                palette.ring_track_on_resting_tab,
            ));
            sprites.push(ChromeSprite::new(
                ChromeMark::ProgressRing {
                    start_milliturns: ring.start_milliturns,
                    sweep_milliturns: ring.sweep_milliturns,
                    stroke_px,
                },
                mark_rect,
                ring.arc,
            ));
        }
        None => {
            let (mark, color) = leaf_mark(leaf.kind, leaf.profile_mark, palette);
            sprites.push(ChromeSprite {
                mark,
                rect: mark_rect,
                color,
                opacity: leaf.mark_opacity,
                grayscale: false,
            });
        }
    }
    if let Some(dot_color) = leaf.dot {
        let side = dot_rect[2] - dot_rect[0];
        sprites.push(ChromeSprite::new(
            // `border-radius: 50%` on a square is a circle, and `ControlPill`
            // clamps its round to half the short side — the same pill the strip's
            // dot is, so the two can never round differently.
            ChromeMark::ControlPill {
                radius_px: (side / 2.0).round().max(1.0) as u32,
            },
            dot_rect,
            dot_color,
        ));
    }
}

/// The mark a peek leaf wears, and the colour it is drawn in.
///
/// The pane head's own pairing (`seats::pane_mark`) with its size dropped: the
/// head sizes a mark for a 28px bar, the peek for a 9px slot and an 11px line.
/// Sharing the *artwork* is the point — a schematic whose folder is not the
/// folder the pane head shows is a schematic of a different window.
fn leaf_mark(
    kind: SeatKind,
    profile_mark: Option<ChromeMark>,
    palette: &ChromePalette,
) -> (ChromeMark, [u8; 3]) {
    let (mark, _size, color) = crate::seats::pane_mark(kind, profile_mark, *palette);
    (mark, color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_layout::{Ratio, Seat, SeatId, SplitId};

    const SCALE: f32 = 1.0;
    const WINDOW: (f32, f32) = (1000.0, 700.0);

    fn seat(id: u64, kind: SeatKind) -> LayoutNode {
        LayoutNode::Seat(Seat::new(SeatId(id), kind))
    }

    fn term(id: u64) -> LayoutNode {
        seat(id, SeatKind::Terminal)
    }

    fn split(id: u64, dir: Axis, ratio_ppm: u32, a: LayoutNode, b: LayoutNode) -> LayoutNode {
        LayoutNode::split_at(
            SplitId(id),
            dir,
            Ratio::from_ppm(ratio_ppm).expect("a legal ratio"),
            a,
            b,
        )
    }

    /// A leaf with nothing to say: no claim, no progress, mark at full strength.
    fn leaf(title: &str) -> PeekLeaf {
        PeekLeaf {
            profile_mark: Some(ChromeMark::ProfilePowerShell),
            kind: SeatKind::Terminal,
            title: title.to_owned(),
            focused: false,
            mark_opacity: 1.0,
            dot: None,
            ring: None,
        }
    }

    fn leaves(n: usize) -> Vec<PeekLeaf> {
        (0..n).map(|i| leaf(&format!("pane {i}"))).collect()
    }

    /// The tab a peek hangs off: somewhere in the middle of a 1000px strip.
    fn host_tab() -> [f32; 4] {
        [300.0, 6.0, 460.0, 40.0]
    }

    const GRID: [f32; 4] = [0.0, 0.0, GRID_WIDTH_LOGICAL_PX, GRID_HEIGHT_LOGICAL_PX];

    // ── L133: the schematic ────────────────────────────────────────────────

    /// A lone leaf fills the box. Never drawn (the peek refuses a one-pane tab),
    /// but it is the base case the recursion is built out of.
    #[test]
    fn a_lone_leaf_takes_the_whole_grid() {
        let cells = schematic(&term(1), GRID, SCALE);
        assert_eq!(cells, vec![GRID]);
    }

    /// The gutter comes out of the total *before* the ratio divides it. Split
    /// first and inset after and each half is 3px short — a schematic that is
    /// not a picture of the layout it claims.
    #[test]
    fn a_split_spends_the_gutter_once_and_divides_what_is_left() {
        let tree = split(1, Axis::Row, 500_000, term(1), term(2));
        let cells = schematic(&tree, GRID, SCALE);
        assert_eq!(cells.len(), 2);

        let available = GRID_WIDTH_LOGICAL_PX - GRID_GAP_LOGICAL_PX;
        assert!((cells[0][2] - cells[0][0] - available / 2.0).abs() < 0.51);
        assert!((cells[1][2] - cells[1][0] - available / 2.0).abs() < 0.51);
        // The gutter is between them, and it is the whole gutter.
        assert!((cells[1][0] - cells[0][2] - GRID_GAP_LOGICAL_PX).abs() < 0.001);
        // Nothing is spent at the ends: the schematic fills its box.
        assert!((cells[0][0] - GRID[0]).abs() < 0.001);
        assert!((cells[1][2] - GRID[2]).abs() < 0.001);
        // The cross axis is untouched by a split along the other one.
        for cell in &cells {
            assert!((cell[1] - GRID[1]).abs() < 0.001);
            assert!((cell[3] - GRID[3]).abs() < 0.001);
        }
    }

    /// `Axis::Row` is side by side and `Axis::Col` is stacked — `bt_layout`'s
    /// own meaning, and `.mini-row` / `.mini-col`'s. A schematic that transposed
    /// these would be a confident picture of a layout the tab does not have, so
    /// the agreement is pinned rather than assumed.
    #[test]
    fn a_row_divides_across_and_a_column_divides_down() {
        let across = schematic(&split(1, Axis::Row, 500_000, term(1), term(2)), GRID, SCALE);
        assert!(across[1][0] > across[0][0], "row: b is to the right");
        assert!((across[1][1] - across[0][1]).abs() < 0.001, "same top");

        let down = schematic(&split(1, Axis::Col, 500_000, term(1), term(2)), GRID, SCALE);
        assert!(down[1][1] > down[0][1], "col: b is below");
        assert!((down[1][0] - down[0][0]).abs() < 0.001, "same left");
    }

    /// The ratio is `a`'s share of what is left, and `b` takes the rest. A
    /// schematic drawn at a fixed half would be a picture of a layout nobody
    /// has once a divider has been dragged.
    #[test]
    fn the_ratio_decides_the_share_and_a_takes_the_named_half() {
        let tree = split(1, Axis::Row, 250_000, term(1), term(2));
        let cells = schematic(&tree, GRID, SCALE);
        let available = GRID_WIDTH_LOGICAL_PX - GRID_GAP_LOGICAL_PX;
        assert!(
            (cells[0][2] - cells[0][0] - available * 0.25).abs() < 0.51,
            "a is the quarter: {cells:?}"
        );
        assert!(
            (cells[1][2] - cells[1][0] - available * 0.75).abs() < 0.51,
            "b is the rest: {cells:?}"
        );
    }

    /// Nesting is where a wrong gutter rule stops being subtle: the error
    /// repeats at every level, so a three-deep tree drifts by three gutters.
    #[test]
    fn a_nested_split_keeps_every_cell_inside_the_grid_and_off_its_neighbours() {
        let tree = split(
            1,
            Axis::Row,
            500_000,
            term(1),
            split(2, Axis::Col, 400_000, term(2), term(3)),
        );
        let cells = schematic(&tree, GRID, SCALE);
        assert_eq!(cells.len(), 3, "in-order: a, then b's own two");

        for cell in &cells {
            assert!(
                cell[0] >= GRID[0] - 0.001 && cell[2] <= GRID[2] + 0.001,
                "{cell:?}"
            );
            assert!(
                cell[1] >= GRID[1] - 0.001 && cell[3] <= GRID[3] + 0.001,
                "{cell:?}"
            );
            assert!(
                cell[2] > cell[0] && cell[3] > cell[1],
                "non-empty: {cell:?}"
            );
        }
        // The inner column shares the outer row's right-hand half.
        assert!((cells[1][0] - cells[2][0]).abs() < 0.001);
        assert!((cells[1][2] - cells[2][2]).abs() < 0.001);
        assert!((cells[2][1] - cells[1][3] - GRID_GAP_LOGICAL_PX).abs() < 0.001);
        // …and that half starts one gutter past the first pane.
        assert!((cells[1][0] - cells[0][2] - GRID_GAP_LOGICAL_PX).abs() < 0.001);
    }

    /// In-order, always — the same walk `LayoutNode::seats_in_order` makes, so
    /// cell *n* and leaf *n* are the same pane. They are matched by position and
    /// nothing else, so a walk that disagreed would put every name in the wrong
    /// box without any test of the names noticing.
    #[test]
    fn cells_come_out_in_the_trees_own_in_order() {
        let tree = split(
            1,
            Axis::Row,
            500_000,
            split(2, Axis::Col, 500_000, term(1), term(2)),
            term(3),
        );
        let cells = schematic(&tree, GRID, SCALE);
        let seats = tree.seats_in_order();
        assert_eq!(cells.len(), seats.len());
        // a's two are both left of b's one.
        assert!(cells[0][2] <= cells[2][0], "{cells:?}");
        assert!(cells[1][2] <= cells[2][0], "{cells:?}");
        // and within a, the first is above the second.
        assert!(cells[0][3] <= cells[1][1], "{cells:?}");
    }

    #[test]
    fn the_schematic_scales_with_the_window() {
        let cells = schematic(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            [
                0.0,
                0.0,
                GRID_WIDTH_LOGICAL_PX * 2.0,
                GRID_HEIGHT_LOGICAL_PX * 2.0,
            ],
            2.0,
        );
        let available = GRID_WIDTH_LOGICAL_PX * 2.0 - GRID_GAP_LOGICAL_PX * 2.0;
        assert!(
            (cells[0][2] - cells[0][0] - available / 2.0).abs() < 0.51,
            "{cells:?}"
        );
        assert!((cells[1][0] - cells[0][2] - GRID_GAP_LOGICAL_PX * 2.0).abs() < 0.001);
    }

    // ── L134: the box, placed ──────────────────────────────────────────────

    /// `top = r.bottom + 5`, `left = r.left` — hung off the tab's leading edge,
    /// *not* centred on it the way a tip is (mock-up 6252-6254).
    #[test]
    fn the_peek_hangs_five_pixels_below_its_tab_and_shares_its_left_edge() {
        let tab = host_tab();
        let laid = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &leaves(2),
            &[40.0, 40.0],
            tab,
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .expect("a two-pane tab peeks");
        assert!((laid.frame[1] - (tab[3] + PEEK_OFFSET_LOGICAL_PX)).abs() < 0.001);
        assert!((laid.frame[0] - tab[0]).abs() < 0.001);
    }

    /// The same five pixels beside a rail row, and the bug that asked for them.
    ///
    /// A real window in rail mode drew the schematic in its own top-left corner,
    /// lying across the rail: the placement was still the horizontal strip's
    /// (`r.bottom + 5`, `r.left`) and the box it was handed was still slot 0's
    /// up in the title bar. Even given the *right* box — the rail row — that
    /// arithmetic answers badly, because below a rail row is the next rail row.
    ///
    /// Beside, then: clear of the row's right edge by the same 5px, and level
    /// with its top so it is obvious which row is being explained.
    #[test]
    fn beside_a_rail_row_the_peek_clears_its_right_edge_and_shares_its_top() {
        let tree = split(1, Axis::Row, 500_000, term(1), term(2));
        // A row of a 220px rail, well down the column.
        let row = [8.0, 300.0, 212.0, 330.0];
        let beside = layout(
            &tree,
            &leaves(2),
            &[40.0, 40.0],
            row,
            PeekSide::Beside,
            WINDOW,
            SCALE,
        )
        .expect("a two-pane tab peeks");
        assert!(
            (beside.frame[0] - (row[2] + PEEK_OFFSET_LOGICAL_PX)).abs() < 0.001,
            "the card stands clear of the row's right edge: {:?}",
            beside.frame
        );
        assert!(
            (beside.frame[1] - row[1]).abs() < 0.001,
            "and level with its top: {:?}",
            beside.frame
        );

        // The two placements are genuinely different answers about one row —
        // the guard against a `side` that is threaded through and then ignored.
        let below = layout(
            &tree,
            &leaves(2),
            &[40.0, 40.0],
            row,
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        assert!(
            below.frame[0] < beside.frame[0] && below.frame[1] > beside.frame[1],
            "below is down-and-left of beside: {:?} vs {:?}",
            below.frame,
            beside.frame
        );
    }

    /// A card hung off the last row of a long rail is pushed back inside rather
    /// than hanging out of the window — the vertical twin of the horizontal
    /// strip's `Math.min(…, win.width - pw - 6)`, which the `Below` placement has
    /// never needed because it only ever hangs off the title bar.
    #[test]
    fn a_peek_beside_a_low_row_is_pushed_up_to_six_pixels_and_no_further() {
        let tree = split(1, Axis::Row, 500_000, term(1), term(2));
        let low = layout(
            &tree,
            &leaves(2),
            &[40.0, 40.0],
            [8.0, WINDOW.1 - 30.0, 212.0, WINDOW.1],
            PeekSide::Beside,
            WINDOW,
            SCALE,
        )
        .unwrap();
        assert!(
            (low.frame[3] - (WINDOW.1 - PEEK_MARGIN_LOGICAL_PX)).abs() < 0.001,
            "the card stops six pixels short of the window's foot: {:?}",
            low.frame
        );
        assert!(low.frame[1] >= PEEK_MARGIN_LOGICAL_PX, "{:?}", low.frame);
    }

    /// The box is a fixed width: a 210px grid in 7px of padding inside a 1px
    /// border. The list can never widen it — `max-width: 210px` under
    /// `box-sizing: border-box` (mock-up 77, 1896) caps it at the grid's own
    /// width however many names there are.
    #[test]
    fn the_box_is_the_grids_width_plus_its_padding_and_never_grows() {
        let narrow = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &leaves(2),
            &[10.0, 10.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        let wide = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &leaves(2),
            &[400.0, 400.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();

        let expected =
            GRID_WIDTH_LOGICAL_PX + 2.0 * (PEEK_PADDING_LOGICAL_PX + PEEK_BORDER_LOGICAL_PX);
        assert!((narrow.frame[2] - narrow.frame[0] - expected).abs() < 0.001);
        assert!(
            (wide.frame[2] - wide.frame[0] - expected).abs() < 0.001,
            "a long name cannot widen the box"
        );
    }

    /// L133, the half the box's own width did not cover: **nothing the peek
    /// draws may leave the peek.**
    ///
    /// The box was already capped at 210px, and that was read as the whole of
    /// `max-width`. It is not: a name is laid out at its measured width inside
    /// that box, and a measured width the box cannot hold is a label painted
    /// straight out of the popup and across the terminal underneath. The frame
    /// stopped growing; the writing did not stop running.
    ///
    /// Red gate: it took whole paths (`seat_caption`) rather than last segments
    /// (`seat_short_caption`), so the common case — every pane named by its
    /// folder — was the overflowing one. Both halves are fixed here: the caller
    /// hands over short names, and this bound holds whatever it is handed.
    #[test]
    fn no_name_in_a_peek_is_drawn_outside_the_peek() {
        let tree = split(1, Axis::Row, 500_000, term(1), term(2));
        let long = [900.0, 900.0];
        let laid = layout(
            &tree,
            &leaves(2),
            &long,
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .expect("a two-pane tab peeks");
        let palette = bt_render::chrome_palette();
        let layers = build(&laid, &leaves(2), &palette, SCALE);
        let frame = laid.frame;
        for layer in &layers {
            for label in &layer.labels {
                assert!(
                    label.rect[0] >= frame[0]
                        && label.rect[2] <= frame[2]
                        && label.rect[1] >= frame[1]
                        && label.rect[3] <= frame[3],
                    "a name ran out of the popup: {:?} against {frame:?}",
                    label.rect
                );
            }
        }
        for row in &laid.rows {
            assert!(
                row.title[2]
                    <= frame[2] - (PEEK_PADDING_LOGICAL_PX + PEEK_BORDER_LOGICAL_PX) * SCALE,
                "and stops inside the padding rather than on the border: {:?}",
                row.title
            );
        }
    }

    /// Two panes standing in the same folder are two panes standing in the same
    /// folder, and the peek says so twice.
    ///
    /// The short name is a *cut*, not an identifier — nothing here disambiguates,
    /// renumbers or falls back to the long form, because the schematic beside the
    /// list is what tells the two apart and inventing "scratchpad (2)" would put
    /// a name on screen that nothing else in the window uses.
    #[test]
    fn two_panes_with_one_name_both_wear_it() {
        let tree = split(1, Axis::Row, 500_000, term(1), term(2));
        let same = vec![leaf("scratchpad"), leaf("scratchpad")];
        let laid = layout(
            &tree,
            &same,
            &[70.0, 70.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .expect("a two-pane tab peeks");
        assert_eq!(
            laid.rows.len(),
            2,
            "one row per leaf, however they are named"
        );
        let palette = bt_render::chrome_palette();
        let layers = build(&laid, &same, &palette, SCALE);
        let spoken = layers
            .iter()
            .flat_map(|layer| layer.labels.iter())
            .filter(|label| label.text == "scratchpad")
            .count();
        assert_eq!(
            spoken, 4,
            "twice in the schematic and twice in the list — no disambiguation invented"
        );
    }

    /// `Math.max(6, Math.min(r.left, win.width - pw - 6))` — clamped at both
    /// ends. A tab near the right edge is the case that actually happens, since
    /// that is where a strip full of tabs puts them.
    #[test]
    fn a_peek_near_an_edge_is_pushed_in_to_six_pixels_and_no_further() {
        let tree = split(1, Axis::Row, 500_000, term(1), term(2));
        let right = layout(
            &tree,
            &leaves(2),
            &[40.0, 40.0],
            [980.0, 6.0, 1000.0, 40.0],
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        assert!(
            (right.frame[2] - (WINDOW.0 - PEEK_MARGIN_LOGICAL_PX)).abs() < 0.001,
            "{:?}",
            right.frame
        );

        let left = layout(
            &tree,
            &leaves(2),
            &[40.0, 40.0],
            [0.0, 6.0, 60.0, 40.0],
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        assert!(
            (left.frame[0] - PEEK_MARGIN_LOGICAL_PX).abs() < 0.001,
            "{:?}",
            left.frame
        );
    }

    /// The box grows downward with the list, and the grid keeps its 92px
    /// whatever the list does.
    #[test]
    fn the_box_is_as_tall_as_its_grid_plus_however_many_lines_the_names_wrap_to() {
        let tree = split(1, Axis::Row, 500_000, term(1), term(2));
        let one_line = layout(
            &tree,
            &leaves(2),
            &[20.0, 20.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        // Two names that cannot share a line: each is wider than half the list.
        let two_lines = layout(
            &tree,
            &leaves(2),
            &[180.0, 180.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();

        assert!(
            two_lines.frame[3] > one_line.frame[3],
            "a wrapped list is taller: {:?} vs {:?}",
            two_lines.frame,
            one_line.frame
        );
        // The grid is untouched by what the list did.
        assert!((one_line.cells[0].rect[1] - two_lines.cells[0].rect[1]).abs() < 0.001);
        assert!((one_line.cells[0].rect[3] - two_lines.cells[0].rect[3]).abs() < 0.001);
    }

    /// The schematic sits inside the padding at its declared size.
    #[test]
    fn the_grid_sits_inside_the_padding_at_two_hundred_and_ten_by_ninety_two() {
        let laid = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &leaves(2),
            &[40.0, 40.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        let inset = PEEK_BORDER_LOGICAL_PX + PEEK_PADDING_LOGICAL_PX;
        assert!((laid.cells[0].rect[0] - (laid.frame[0] + inset)).abs() < 0.001);
        assert!((laid.cells[0].rect[1] - (laid.frame[1] + inset)).abs() < 0.001);
        assert!(
            (laid.cells[1].rect[2] - (laid.frame[0] + inset + GRID_WIDTH_LOGICAL_PX)).abs() < 0.001
        );
        assert!(
            (laid.cells[0].rect[3] - (laid.frame[1] + inset + GRID_HEIGHT_LOGICAL_PX)).abs()
                < 0.001
        );
    }

    /// Every leaf gets a name row, and the rows wrap rather than running off the
    /// box — the list exists precisely because a cell can be too narrow to read.
    #[test]
    fn every_leaf_gets_a_row_and_the_rows_stay_inside_the_box() {
        let tree = split(
            1,
            Axis::Row,
            500_000,
            term(1),
            split(2, Axis::Col, 500_000, term(2), term(3)),
        );
        let laid = layout(
            &tree,
            &leaves(3),
            &[60.0, 60.0, 60.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        assert_eq!(laid.rows.len(), 3);
        assert_eq!(laid.cells.len(), 3);
        for row in &laid.rows {
            assert!(row.mark[0] >= laid.frame[0], "{row:?}");
            assert!(row.title[2] <= laid.frame[2] + 0.001, "{row:?}");
            assert!(row.title[3] <= laid.frame[3] + 0.001, "{row:?}");
            assert!(
                row.title[1] >= laid.cells[0].rect[3],
                "below the grid: {row:?}"
            );
        }
    }

    /// Entries share a line while they fit and start a new one when they do not
    /// — `flex-wrap: wrap` with a `10px` column gap and nothing else.
    #[test]
    fn names_share_a_line_until_the_line_is_full() {
        let tree = split(1, Axis::Row, 500_000, term(1), term(2));
        let together = layout(
            &tree,
            &leaves(2),
            &[20.0, 20.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        assert!(
            (together.rows[0].title[1] - together.rows[1].title[1]).abs() < 0.001,
            "two short names share a line"
        );
        assert!(
            together.rows[1].mark[0] > together.rows[0].title[2],
            "and the second follows the first across"
        );

        let apart = layout(
            &tree,
            &leaves(2),
            &[180.0, 180.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        assert!(
            apart.rows[1].title[1] > apart.rows[0].title[1],
            "two long ones do not"
        );
        assert!(
            (apart.rows[1].mark[0] - apart.rows[0].mark[0]).abs() < 0.001,
            "and the second starts back at the left"
        );
    }

    /// A tree and a name list that disagree would draw one tab's shape with
    /// another's names. There is no sensible half-answer, so there is no answer.
    #[test]
    fn a_tree_and_a_name_list_that_disagree_draw_nothing() {
        let tree = split(1, Axis::Row, 500_000, term(1), term(2));
        assert_eq!(
            layout(
                &tree,
                &leaves(3),
                &[40.0; 3],
                host_tab(),
                PeekSide::Below,
                WINDOW,
                SCALE
            ),
            None
        );
        assert_eq!(
            layout(
                &tree,
                &leaves(2),
                &[40.0],
                host_tab(),
                PeekSide::Below,
                WINDOW,
                SCALE
            ),
            None,
            "…and the widths have to agree too"
        );
    }

    // ── L133: what goes inside a cell ──────────────────────────────────────

    #[test]
    fn a_cell_leads_with_its_mark_and_gives_the_rest_to_the_name() {
        let laid = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &leaves(2),
            &[40.0, 40.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        let cell = laid.cells[0];
        let inner_left = cell.rect[0] + LEAF_BORDER_LOGICAL_PX + LEAF_PADDING_X_LOGICAL_PX;
        assert!((cell.mark[0] - inner_left).abs() < 0.001);
        assert!((cell.mark[2] - cell.mark[0] - LEAF_MARK_LOGICAL_PX).abs() < 0.001);
        assert!((cell.title[0] - (cell.mark[2] + LEAF_GAP_LOGICAL_PX)).abs() < 0.001);
        assert!(
            (cell.title[2] - (cell.rect[2] - LEAF_BORDER_LOGICAL_PX - LEAF_PADDING_X_LOGICAL_PX))
                .abs()
                < 0.001
        );
        // The mark is centred on the cell's own middle, not hung off its top.
        let cell_middle = (cell.rect[1] + cell.rect[3]) / 2.0;
        let mark_middle = (cell.mark[1] + cell.mark[3]) / 2.0;
        assert!((mark_middle - cell_middle).abs() <= 0.51, "{cell:?}");
    }

    /// The focus flag rides through untouched — it is what `build` colours from,
    /// and a schematic that forgot it would show a tab with no focused pane.
    #[test]
    fn the_focused_cell_is_the_focused_leaf() {
        let mut cast = leaves(2);
        cast[1].focused = true;
        let laid = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &cast,
            &[40.0, 40.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        assert!(!laid.cells[0].focused);
        assert!(laid.cells[1].focused);
    }

    // ── L132 / L133: the painted layer ─────────────────────────────────────

    #[test]
    fn the_peek_paints_one_opaque_layer_with_a_face_a_hairline_and_a_lift() {
        let palette = bt_render::chrome_palette();
        let cast = leaves(2);
        let laid = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &cast,
            &[40.0, 40.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        let layers = build(&laid, &cast, &palette, SCALE);
        assert_eq!(layers.len(), 1, "a peek is one layer");
        let layer = &layers[0];
        assert!(
            (layer.opacity - 1.0).abs() < 0.001,
            "there is no fade: it is drawn or it is not"
        );
        assert!(
            layer.quads.iter().any(|q| q.color == palette.menu_surface),
            "the box wears --menu"
        );
        assert!(
            layer.quads.iter().any(|q| q.rect[1] < laid.frame[1]),
            "and it is lifted off the strip"
        );
        assert!(
            layer.quads.iter().any(|q| q.color == palette.seat_body),
            "a resting cell wears --termbg"
        );
    }

    /// Two labels per leaf — one in the cell, one in the list — and the list's
    /// is the larger, because the list is the half you can always read.
    #[test]
    fn every_leaf_is_named_twice_at_the_two_declared_sizes() {
        let palette = bt_render::chrome_palette();
        let cast = leaves(3);
        let tree = split(
            1,
            Axis::Row,
            500_000,
            term(1),
            split(2, Axis::Col, 500_000, term(2), term(3)),
        );
        let laid = layout(
            &tree,
            &cast,
            &[50.0; 3],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        let layer = &build(&laid, &cast, &palette, SCALE)[0];

        assert_eq!(layer.labels.len(), 6);
        for name in ["pane 0", "pane 1", "pane 2"] {
            assert_eq!(
                layer.labels.iter().filter(|l| l.text == name).count(),
                2,
                "{name} is in the schematic and in the list"
            );
        }
        assert_eq!(
            layer
                .labels
                .iter()
                .filter(|l| (l.font_size_px - LEAF_FONT_LOGICAL_PX).abs() < 0.001)
                .count(),
            3
        );
        assert_eq!(
            layer
                .labels
                .iter()
                .filter(|l| (l.font_size_px - LIST_FONT_LOGICAL_PX).abs() < 0.001)
                .count(),
            3
        );
        // One mark per leaf per half, same count.
        assert_eq!(layer.sprites.len(), 6);
    }

    /// The focused leaf is marked in neutral ink, never in `--accent`. The peek
    /// carries state marks of its own, and "the focused one" arriving in the
    /// same colour as "the one wanting you" would put two different claims in
    /// one popup wearing one colour (mock-up 1920-1921).
    #[test]
    fn the_focused_leaf_is_neutral_and_never_the_accent() {
        let palette = bt_render::chrome_palette();
        let mut cast = leaves(2);
        cast[1].focused = true;
        let laid = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &cast,
            &[40.0, 40.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        let layer = &build(&laid, &cast, &palette, SCALE)[0];

        let focused = laid.cells[1].rect;
        let inside = |q: &OverlayQuad| {
            q.rect[0] >= focused[0] - 0.51
                && q.rect[2] <= focused[2] + 0.51
                && q.rect[1] >= focused[1] - 0.51
                && q.rect[3] <= focused[3] + 0.51
        };
        assert!(
            layer
                .quads
                .iter()
                .filter(|q| inside(q))
                .all(|q| q.color != palette.accent),
            "nothing in the focused cell is drawn in the accent"
        );
        assert!(
            layer
                .quads
                .iter()
                .any(|q| inside(q) && q.color == palette.peek_leaf_focus_fill),
            "it is washed with --active instead"
        );
    }

    #[test]
    fn a_resting_leaf_and_a_focused_one_do_not_look_alike() {
        let palette = bt_render::chrome_palette();
        let mut cast = leaves(2);
        cast[1].focused = true;
        let laid = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &cast,
            &[40.0, 40.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        let layer = &build(&laid, &cast, &palette, SCALE)[0];
        let cell_label = |n: usize| {
            layer
                .labels
                .iter()
                .find(|l| {
                    (l.font_size_px - LEAF_FONT_LOGICAL_PX).abs() < 0.001 && l.text == cast[n].title
                })
                .expect("a cell label")
        };
        assert_ne!(cell_label(0).color, cell_label(1).color);
        assert_eq!(cell_label(0).color, palette.pane_title);
        assert_eq!(cell_label(1).color, palette.peek_leaf_focus_text);
    }

    /// The breath belongs to the strip, and the peek only carries what it is
    /// handed. Computing it here would give one window two opinions about one
    /// clock.
    #[test]
    fn a_working_leafs_mark_is_drawn_at_the_alpha_it_was_handed() {
        let palette = bt_render::chrome_palette();
        let mut cast = leaves(2);
        cast[0].mark_opacity = 0.42;
        let laid = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &cast,
            &[40.0, 40.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        let layer = &build(&laid, &cast, &palette, SCALE)[0];
        assert_eq!(
            layer
                .sprites
                .iter()
                .filter(|s| (s.opacity - 0.42).abs() < 0.001)
                .count(),
            2,
            "the working leaf breathes in both halves"
        );
        assert_eq!(
            layer
                .sprites
                .iter()
                .filter(|s| (s.opacity - 1.0).abs() < 0.001)
                .count(),
            2,
            "and the idle one does not"
        );
    }

    // ── L134 — what each mini leaf says about its own session ──────────────

    /// Lay out `cast` over a two-deep tree and paint it, for the status tests
    /// below — all of which ask "what was drawn for leaf *n*", never "where".
    fn painted(cast: &[PeekLeaf]) -> (PeekLayout, OverlayLayer, ChromePalette) {
        let palette = bt_render::chrome_palette();
        let tree = match cast.len() {
            2 => split(1, Axis::Row, 500_000, term(1), term(2)),
            _ => split(
                1,
                Axis::Row,
                500_000,
                term(1),
                split(2, Axis::Col, 500_000, term(2), term(3)),
            ),
        };
        let widths = vec![40.0; cast.len()];
        let laid = layout(
            &tree,
            cast,
            &widths,
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .expect("a placed peek");
        let layer = build(&laid, cast, &palette, SCALE).swap_remove(0);
        (laid, layer, palette)
    }

    /// Every sprite drawn inside `rect`, which for a mark slot is the mark and
    /// whatever badges it.
    fn sprites_over(layer: &OverlayLayer, rect: [f32; 4]) -> Vec<&crate::marks::ChromeSprite> {
        layer
            .sprites
            .iter()
            .filter(|sprite| {
                // The dot deliberately overhangs its slot, so "over this slot"
                // is an overlap test and not a containment one.
                sprite.rect[0] < rect[2] + 0.01
                    && sprite.rect[2] > rect[0] - 0.01
                    && sprite.rect[1] < rect[3] + 0.01
                    && sprite.rect[3] > rect[1] - 0.01
            })
            .collect()
    }

    fn is_dot(sprite: &crate::marks::ChromeSprite) -> bool {
        matches!(sprite.mark, ChromeMark::ControlPill { .. })
    }

    fn is_ring(sprite: &crate::marks::ChromeSprite) -> bool {
        matches!(sprite.mark, ChromeMark::ProgressRing { .. })
    }

    /// The whole point of putting status on the schematic: a tab wears the
    /// loudest claim of its fleet, and the peek says *which room* is making it.
    /// A noisy pane must not light up its quiet siblings.
    #[test]
    fn each_mini_leaf_wears_its_own_claim_and_the_quiet_ones_wear_none() {
        let palette = bt_render::chrome_palette();
        let mut cast = leaves(3);
        cast[0].dot = crate::StatusClaim::Failed.dot_color(&palette);
        cast[2].dot = crate::StatusClaim::Unread.dot_color(&palette);
        let (laid, layer, _) = painted(&cast);

        for (index, expected) in [
            (0, crate::StatusClaim::Failed.dot_color(&palette)),
            (1, None),
            (2, crate::StatusClaim::Unread.dot_color(&palette)),
        ] {
            for slot in [laid.cells[index].mark, laid.rows[index].mark] {
                let dots: Vec<_> = sprites_over(&layer, slot)
                    .into_iter()
                    .filter(|sprite| is_dot(sprite))
                    .collect();
                match expected {
                    Some(color) => {
                        assert_eq!(dots.len(), 1, "leaf {index} draws exactly one dot");
                        assert_eq!(dots[0].color, color, "leaf {index} wears the wrong claim");
                    }
                    None => assert!(
                        dots.is_empty(),
                        "leaf {index} has nothing to say and drew a dot anyway"
                    ),
                }
            }
        }
    }

    /// The ladder is `StatusClaim`'s own `Ord` — failed over bell over unread —
    /// and the peek does not get a second opinion about it. What is pinned here
    /// is that a mini leaf *draws* what that ladder decided, in three colours
    /// that can actually be told apart.
    #[test]
    fn the_mini_leaf_draws_the_colour_its_claim_decided_and_the_three_differ() {
        let palette = bt_render::chrome_palette();
        assert!(
            crate::StatusClaim::Unread < crate::StatusClaim::Bell
                && crate::StatusClaim::Bell < crate::StatusClaim::Failed,
            "the loudness order this drawing depends on"
        );
        let mut drawn = Vec::new();
        for claim in [
            crate::StatusClaim::Unread,
            crate::StatusClaim::Bell,
            crate::StatusClaim::Failed,
        ] {
            let mut cast = leaves(2);
            cast[0].dot = claim.dot_color(&palette);
            let (laid, layer, _) = painted(&cast);
            let dot = sprites_over(&layer, laid.cells[0].mark)
                .into_iter()
                .find(|sprite| is_dot(sprite))
                .unwrap_or_else(|| panic!("{claim:?} drew no dot"));
            assert_eq!(
                Some(dot.color),
                claim.dot_color(&palette),
                "{claim:?} was drawn in a colour it did not choose"
            );
            drawn.push(dot.color);
        }
        assert_ne!(drawn[0], drawn[1]);
        assert_ne!(drawn[1], drawn[2]);
        assert_ne!(drawn[0], drawn[2]);
    }

    /// The strip's `.unreaddot { top: -2px; right: -4px }`, in miniature: the
    /// badge overhangs its slot on both axes so it reads as a badge *on* the
    /// mark rather than a thing standing beside it.
    #[test]
    fn the_dot_hangs_off_its_marks_top_right_corner_in_both_halves() {
        let palette = bt_render::chrome_palette();
        let mut cast = leaves(2);
        cast[0].dot = crate::StatusClaim::Bell.dot_color(&palette);
        let (laid, layer, _) = painted(&cast);

        for slot in [laid.cells[0].mark, laid.rows[0].mark] {
            let dot = sprites_over(&layer, slot)
                .into_iter()
                .find(|sprite| is_dot(sprite))
                .expect("a dot");
            let side = dot.rect[2] - dot.rect[0];
            assert!(
                (side - (dot.rect[3] - dot.rect[1])).abs() < 0.01,
                "the dot is square"
            );
            let slot_side = slot[2] - slot[0];
            assert!(
                (side - (slot_side * DOT_SHARE_OF_MARK).round()).abs() < 0.01,
                "the dot is the strip's own share of a {slot_side}px slot, got {side}"
            );
            assert!(dot.rect[2] > slot[2], "it overhangs to the right");
            assert!(dot.rect[1] < slot[1], "and above");
            // Small enough to still be a badge rather than a second mark.
            assert!(side < slot_side, "the badge never outgrows what it badges");
        }
    }

    /// The tab's ruling, in miniature: while there is progress to report, the
    /// slot is for the progress. The mark is *replaced* — the same box, not a
    /// second sprite beside it — and the dot, which answers a different
    /// question, survives it.
    #[test]
    fn a_progress_ring_replaces_the_mini_leafs_mark_and_the_dot_survives_it() {
        let palette = bt_render::chrome_palette();
        let mut cast = leaves(2);
        cast[0].ring = Some(crate::seats::TabRing {
            arc: palette.accent,
            start_milliturns: 0,
            sweep_milliturns: 400,
        });
        cast[0].dot = crate::StatusClaim::Bell.dot_color(&palette);
        let (laid, layer, _) = painted(&cast);

        let slot = laid.cells[0].mark;
        let over = sprites_over(&layer, slot);
        let rings: Vec<_> = over.iter().filter(|s| is_ring(s)).collect();
        assert_eq!(rings.len(), 2, "a track and an arc");
        for ring in &rings {
            assert_eq!(ring.rect, slot, "the ring takes the mark's exact box");
        }
        assert!(
            matches!(
                rings[0].mark,
                ChromeMark::ProgressRing {
                    sweep_milliturns: 1000,
                    ..
                }
            ),
            "the track is drawn first and is a full turn"
        );
        assert_eq!(
            rings[1].color, palette.accent,
            "the arc wears its own colour"
        );
        assert!(
            matches!(
                rings[1].mark,
                ChromeMark::ProgressRing {
                    sweep_milliturns: 400,
                    ..
                }
            ),
            "and reports the reading it was handed"
        );
        let (kind_mark, _) = leaf_mark(cast[0].kind, cast[0].profile_mark, &palette);
        assert!(
            !over.iter().any(|s| s.mark == kind_mark),
            "the leaf's own mark was replaced, not drawn under the ring"
        );
        assert_eq!(
            over.iter().filter(|s| is_dot(s)).count(),
            1,
            "the claim outlives the ring that covered the mark"
        );
        // The untouched sibling still shows its mark and no ring.
        let sibling = sprites_over(&layer, laid.cells[1].mark);
        assert!(sibling.iter().any(|s| s.mark == kind_mark));
        assert!(!sibling.iter().any(|s| is_ring(s)));
    }

    /// `seats`' `the_breath_fades_the_mark_and_leaves_the_dot_alone`, asked of
    /// the miniature: a breath is about the mark, and a claim is not a thing
    /// that fades in and out.
    #[test]
    fn the_breath_fades_the_mini_mark_and_leaves_the_claim_alone() {
        let palette = bt_render::chrome_palette();
        let mut cast = leaves(2);
        cast[0].mark_opacity = 0.42;
        cast[0].dot = crate::StatusClaim::Failed.dot_color(&palette);
        let (laid, layer, _) = painted(&cast);

        for slot in [laid.cells[0].mark, laid.rows[0].mark] {
            let over = sprites_over(&layer, slot);
            let dot = over.iter().find(|s| is_dot(s)).expect("a dot");
            assert_eq!(
                dot.opacity.to_bits(),
                1.0_f32.to_bits(),
                "the dot breathed along with the mark"
            );
            let mark = over
                .iter()
                .find(|s| !is_dot(s))
                .expect("the leaf's own mark");
            assert!((mark.opacity - 0.42).abs() < 0.001);
        }
    }

    /// The miniature is the strip's drawing scaled, not a second design: every
    /// share is stated as the strip's own number over the strip's own mark, so
    /// editing a tab constant moves the peek with it and neither can drift.
    #[test]
    fn the_status_channels_keep_the_strips_own_proportions() {
        let mark = bt_render::WINDOW_TAB_MARK_LOGICAL_PX;
        assert!(
            (DOT_SHARE_OF_MARK * mark - bt_render::WINDOW_TAB_STATUS_DOT_LOGICAL_PX).abs() < 0.001
        );
        assert!(
            (DOT_TOP_SHARE_OF_MARK * mark - bt_render::WINDOW_TAB_STATUS_DOT_TOP_LOGICAL_PX).abs()
                < 0.001
        );
        assert!(
            (DOT_RIGHT_SHARE_OF_MARK * mark - bt_render::WINDOW_TAB_STATUS_DOT_RIGHT_LOGICAL_PX)
                .abs()
                < 0.001
        );
        assert!(
            (RING_STROKE_SHARE_OF_MARK * mark - bt_render::WINDOW_TAB_RING_STROKE_LOGICAL_PX).abs()
                < 0.001
        );
    }

    /// The list's ink is `--ink2` over `--menu` — the same ink the tip is
    /// written in, because it is the same kind of writing on the same surface.
    #[test]
    fn the_name_list_is_written_in_the_menus_secondary_ink() {
        let palette = bt_render::chrome_palette();
        let cast = leaves(2);
        let laid = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &cast,
            &[40.0, 40.0],
            host_tab(),
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        let layer = &build(&laid, &cast, &palette, SCALE)[0];
        assert!(
            layer
                .labels
                .iter()
                .filter(|l| (l.font_size_px - LIST_FONT_LOGICAL_PX).abs() < 0.001)
                .all(|l| l.color == palette.menu_item_text)
        );
    }

    // ── the mock-up's numbers, as numbers ──────────────────────────────────

    /// Every other test in this file states a *relationship* — the box is its
    /// padding wider than its grid, the peek stands off its tab by its offset —
    /// and a relationship written in terms of the constants moves when the
    /// constants do. So none of them would notice someone editing the `7` to a
    /// `4`: a mutation run proved exactly that, and this test is the answer.
    ///
    /// The literals here are the mock-up's, transcribed once. They are the only
    /// place in this module where a number is checked against the design rather
    /// than against itself.
    #[test]
    fn the_declared_values_are_the_mockups_own() {
        // `.layout-peek` (1719-1728)
        assert_eq!(PEEK_DELAY, Duration::from_millis(350));
        assert_eq!(PEEK_RADIUS_LOGICAL_PX, 8.0);
        assert_eq!(PEEK_BORDER_LOGICAL_PX, 1.0);
        assert_eq!(PEEK_PADDING_LOGICAL_PX, 7.0);
        // placement (6252-6254)
        assert_eq!(PEEK_OFFSET_LOGICAL_PX, 5.0);
        assert_eq!(PEEK_MARGIN_LOGICAL_PX, 6.0);
        // `.peek-grid` (1891) and `.mini-leaf` (1913-1922)
        assert_eq!(GRID_WIDTH_LOGICAL_PX, 210.0);
        assert_eq!(GRID_HEIGHT_LOGICAL_PX, 92.0);
        assert_eq!(GRID_GAP_LOGICAL_PX, 3.0);
        assert_eq!(LEAF_RADIUS_LOGICAL_PX, 4.0);
        assert_eq!(LEAF_FONT_LOGICAL_PX, 10.5);
        assert_eq!(LEAF_GAP_LOGICAL_PX, 4.0);
        // `.peek-list` (1894-1904)
        assert_eq!(LIST_FONT_LOGICAL_PX, 11.0);
        assert_eq!(LIST_ROW_GAP_LOGICAL_PX, 3.0);
        assert_eq!(LIST_COLUMN_GAP_LOGICAL_PX, 10.0);
        // `paneCount(w) < 2` (6232)
        assert_eq!(PEEK_MIN_PANES, 2);

        // …and the box those numbers actually produce.
        let tab = host_tab();
        let laid = layout(
            &split(1, Axis::Row, 500_000, term(1), term(2)),
            &leaves(2),
            &[40.0, 40.0],
            tab,
            PeekSide::Below,
            WINDOW,
            SCALE,
        )
        .unwrap();
        assert!(
            (laid.frame[2] - laid.frame[0] - 226.0).abs() < 0.001,
            "210 + 2×(7 padding + 1 border): {:?}",
            laid.frame
        );
        assert!(
            (laid.frame[1] - (tab[3] + 5.0)).abs() < 0.001,
            "five below the tab: {:?}",
            laid.frame
        );
    }

    // ── L131: which tabs have a layout worth showing ───────────────────────

    /// A lone pane's schematic is one rectangle filling the box, which says only
    /// "there is a terminal here" — what the tab already said.
    #[test]
    fn a_tab_needs_two_panes_before_it_has_a_layout_to_show() {
        assert!(!eligible(1, false, false, false));
        assert!(eligible(2, false, false, false));
        assert!(eligible(7, false, false, false));
        // A tab with no panes at all is not a special case, it is fewer than two.
        assert!(!eligible(0, false, false, false));
    }

    /// The active tab's layout is not a schematic away — it is the window behind
    /// the strip. The mock-up's rail clause, translated (6233-6236).
    #[test]
    fn the_active_tab_never_peeks_however_many_panes_it_holds() {
        assert!(!eligible(4, true, false, false));
        assert!(eligible(4, false, false, false), "…but its neighbour does");
    }

    /// A drag owns the pointer outright (T5), and a rename owns the tab (T4).
    #[test]
    fn a_drag_or_a_rename_refuses_every_tab() {
        assert!(!eligible(3, false, true, false), "a drag in flight");
        assert!(!eligible(3, false, false, true), "the tab being renamed");
        assert!(eligible(3, false, false, false));
    }

    // ── L131 / L135: the clock ─────────────────────────────────────────────

    #[test]
    fn a_peek_waits_three_hundred_and_fifty_milliseconds_and_not_a_moment_less() {
        let mut host = PeekHost::default();
        let start = Instant::now();
        host.observe(Some(3), start);

        assert!(!host.activate_if_due(start + Duration::from_millis(349)));
        assert_eq!(host.active(), None);
        assert!(host.activate_if_due(start + PEEK_DELAY));
        assert_eq!(host.active(), Some(3));
    }

    /// §6, pinned: the peek's clock is shorter than the tip's, so on a tab that
    /// qualifies for both the peek is due first. This is the ordering the whole
    /// mutual exclusion rests on, and it is a property of two constants.
    #[test]
    fn the_peek_is_due_before_the_tooltip_is() {
        assert!(
            PEEK_DELAY < crate::tooltip::TOOLTIP_DELAY,
            "the peek wins the race by construction"
        );
        let start = Instant::now();
        let mut peek = PeekHost::default();
        let mut tip = crate::tooltip::TooltipHost::default();
        peek.observe(Some(2), start);
        tip.observe(Some(crate::tooltip::TooltipAnchorId::Tab(2)), start);

        // At the peek's due instant the tip is still counting.
        assert!(peek.activate_if_due(start + PEEK_DELAY));
        assert!(!tip.activate_if_due(start + PEEK_DELAY));
    }

    /// §6's second half: while a peek is up, that tab says nothing else. Every
    /// anchor it owns is silenced, and every other tab's is untouched.
    #[test]
    fn a_showing_peek_silences_its_own_tabs_tips_and_no_others() {
        use crate::tooltip::TooltipAnchorId;
        for anchor in [
            TooltipAnchorId::Tab(2),
            TooltipAnchorId::TabIcon(2),
            TooltipAnchorId::TabPin(2),
        ] {
            assert!(suppresses(Some(2), anchor), "{anchor:?}");
            assert!(
                !suppresses(Some(3), anchor),
                "a neighbour's peek: {anchor:?}"
            );
            assert!(
                !suppresses(None, anchor),
                "no peek at all suppresses nothing: {anchor:?}"
            );
        }
        // The window's own controls are nobody's tab.
        assert!(!suppresses(Some(2), TooltipAnchorId::NewTab));
        assert!(!suppresses(Some(2), TooltipAnchorId::Settings));
        assert!(!suppresses(Some(2), TooltipAnchorId::CloseWindow));
    }

    #[test]
    fn resting_on_a_showing_peek_does_not_restart_its_clock() {
        let mut host = PeekHost::default();
        let start = Instant::now();
        host.observe(Some(1), start);
        assert!(host.activate_if_due(start + PEEK_DELAY));

        let changed = host.observe(Some(1), start + PEEK_DELAY);
        assert!(!changed, "nothing changed");
        assert_eq!(host.active(), Some(1), "the schematic stays up");
        assert_eq!(host.deadline(), None, "and asks for no wakeups");
    }

    #[test]
    fn moving_to_another_tab_takes_the_old_peek_down_and_starts_over() {
        let mut host = PeekHost::default();
        let start = Instant::now();
        host.observe(Some(1), start);
        assert!(host.activate_if_due(start + PEEK_DELAY));

        let moved = start + PEEK_DELAY + Duration::from_millis(1);
        assert!(host.observe(Some(4), moved));
        assert_eq!(host.active(), None, "the old one is gone at once");
        assert!(!host.activate_if_due(moved + Duration::from_millis(349)));
        assert!(host.activate_if_due(moved + PEEK_DELAY));
        assert_eq!(host.active(), Some(4));
    }

    /// §5: the pointer leaves and the schematic goes *and* the timer goes.
    /// Hiding without disarming is the bug where the peek lands 350ms later over
    /// a tab the pointer left.
    #[test]
    fn leaving_a_tab_clears_the_timer_as_well_as_the_peek() {
        let mut host = PeekHost::default();
        let start = Instant::now();
        host.observe(Some(1), start);
        host.observe(None, start + Duration::from_millis(10));

        // Asked before anything is polled: a host that quietly self-heals on the
        // way past looks identical to one that never armed.
        assert_eq!(host.deadline(), None, "a disarmed host asks for no wakeups");
        assert!(!host.activate_if_due(start + Duration::from_secs(5)));
        assert_eq!(host.active(), None);
    }

    /// §5: a press anywhere, or the window losing focus, or a drag starting.
    #[test]
    fn a_press_a_lost_window_or_a_drag_takes_the_peek_down_immediately() {
        for settle in [false, true] {
            let mut host = PeekHost::default();
            let start = Instant::now();
            host.observe(Some(2), start);
            if settle {
                assert!(host.activate_if_due(start + PEEK_DELAY));
            }
            assert_eq!(host.hide(), settle, "reports whether anything was visible");
            assert_eq!(host.active(), None);
            assert!(!host.activate_if_due(start + Duration::from_secs(5)));
        }
    }

    /// A tab that stops qualifying mid-wait takes its pending peek with it — its
    /// last split closed, it became the active tab, a drag began.
    #[test]
    fn a_tab_that_stops_qualifying_takes_its_pending_peek_with_it() {
        let mut host = PeekHost::default();
        let start = Instant::now();
        host.observe(Some(3), start);

        assert!(!host.retain(|tab| tab != 3), "nothing was visible yet");
        assert_eq!(host.deadline(), None);
        assert!(!host.activate_if_due(start + Duration::from_secs(5)));
        assert_eq!(host.active(), None);
    }

    #[test]
    fn a_tab_that_stops_qualifying_takes_its_showing_peek_with_it() {
        let mut host = PeekHost::default();
        let start = Instant::now();
        host.observe(Some(3), start);
        assert!(host.activate_if_due(start + PEEK_DELAY));

        assert!(host.retain(|tab| tab != 3), "a visible peek came down");
        assert_eq!(host.active(), None);

        host.observe(Some(1), start);
        assert!(host.activate_if_due(start + PEEK_DELAY));
        assert!(
            !host.retain(|_| true),
            "a subject still there is left alone"
        );
        assert_eq!(host.active(), Some(1));
    }

    #[test]
    fn an_armed_host_asks_to_be_woken_exactly_when_the_delay_is_up() {
        let mut host = PeekHost::default();
        let start = Instant::now();
        host.observe(Some(0), start);
        assert_eq!(host.deadline(), Some(start + PEEK_DELAY));
    }

    /// There is no fade, so a shown peek is finished and owes the loop nothing.
    /// This is the whole of §7 for this popup.
    #[test]
    fn a_shown_peek_is_still_and_owes_no_frames() {
        let mut host = PeekHost::default();
        let start = Instant::now();
        host.observe(Some(0), start);
        host.activate_if_due(start + PEEK_DELAY);
        assert_eq!(host.deadline(), None);
    }
}
