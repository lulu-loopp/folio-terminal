//! The seat tree this window hosts, and the chrome drawn around it.
//!
//! `docs/M2-layout-solver-spec.md` §4.1 draws the seam this module sits on:
//!
//! ```text
//! window geometry -> viewport rect -> solve -> seat rects -> (terminal) cols/rows
//!    -> ConPTY 200ms quiet coalescing (existing, inherited unchanged)
//!    -> LayoutKey { width, dpi, font_rev, theme_rev }
//! ```
//!
//! Red line L10 forbids the reverse: nothing downstream of `solve` — not a
//! formula that grew tall, not an inline image, not a long filename — may ask
//! for a different seat rectangle. Everything in this file therefore *consumes*
//! the solver's answer and never feeds it, and the one input the solver takes
//! from the window is the viewport rectangle itself.
//!
//! Red line L1 is the other half: the solver is told a `SeatKind` and nothing
//! about content. Which session lives in the terminal seat, which file the
//! preview shows — those never enter a `LayoutNode`.

use bt_layout::{
    Axis, DIVIDER, Edit, EditError, LayoutMode, LayoutNode, LogicalPx, LogicalRect, LogicalSize,
    Presentation, Ratio, SUBPIXELS_PER_PX, Seat, SeatId, SeatKind, SeatLayout, SeatMetrics,
    SplitId, WorkAreaHint, apply, solve, window_min_inner_size,
};
use bt_persist::{LayoutNodeV1, LeafNodeV1, SplitDirV1, SplitNodeV1, TermLeafV1};
use bt_render::{
    ChromeLabel, ChromeLabelWeight, ChromeQuad, PANE_HEAD_FILE_MARK_LOGICAL_PX,
    PANE_HEAD_FOLDER_MARK_LOGICAL_PX, PANE_HEAD_PROFILE_MARK_LOGICAL_PX,
    SEAT_DIVIDER_GRIP_LENGTH_LOGICAL_PX, SEAT_DIVIDER_GRIP_RADIUS_LOGICAL_PX,
    SEAT_DIVIDER_GRIP_THICKNESS_LOGICAL_PX, SEAT_DIVIDER_HIT_LOGICAL_PX,
    SEAT_PANE_CLOSE_BOX_LOGICAL_PX, SEAT_PANE_CLOSE_GLYPH_LOGICAL_PX,
    SEAT_PANE_CLOSE_RADIUS_LOGICAL_PX, SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX,
    SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX, SEAT_TITLE_BAR_LOGICAL_PX, SEAT_TITLE_EDGE_LOGICAL_PX,
    SEAT_TITLE_FONT_LOGICAL_PX, SEAT_TITLE_GAP_LOGICAL_PX, SEAT_TITLE_PADDING_LOGICAL_PX,
    SEAT_TITLE_TRAILING_PADDING_LOGICAL_PX, SeatViewport, WINDOW_CAPTION_BUTTON_LOGICAL_PX,
    WINDOW_CAPTION_GEAR_GLYPH_LOGICAL_PX, WINDOW_CAPTION_GLYPH_LOGICAL_PX,
    WINDOW_NEW_TAB_BOX_LOGICAL_PX, WINDOW_NEW_TAB_CHEVRON_HEIGHT_LOGICAL_PX,
    WINDOW_NEW_TAB_CHEVRON_WIDTH_LOGICAL_PX, WINDOW_NEW_TAB_GLYPH_LOGICAL_PX,
    WINDOW_NEW_TAB_MARGIN_BOTTOM_LOGICAL_PX, WINDOW_NEW_TAB_MARGIN_LEFT_LOGICAL_PX,
    WINDOW_NEW_TAB_RADIUS_LOGICAL_PX, WINDOW_TAB_BADGE_FONT_LOGICAL_PX,
    WINDOW_TAB_BADGE_HEIGHT_LOGICAL_PX, WINDOW_TAB_BADGE_MIN_WIDTH_LOGICAL_PX,
    WINDOW_TAB_BADGE_PADDING_X_LOGICAL_PX, WINDOW_TAB_BADGE_RADIUS_LOGICAL_PX,
    WINDOW_TAB_CLOSE_BOX_LOGICAL_PX, WINDOW_TAB_CLOSE_GLYPH_LOGICAL_PX,
    WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX, WINDOW_TAB_FONT_LOGICAL_PX,
    WINDOW_TAB_GAP_BETWEEN_LOGICAL_PX, WINDOW_TAB_GAP_LOGICAL_PX, WINDOW_TAB_HEIGHT_LOGICAL_PX,
    WINDOW_TAB_MARK_LOGICAL_PX, WINDOW_TAB_MAX_WIDTH_LOGICAL_PX, WINDOW_TAB_MIN_WIDTH_LOGICAL_PX,
    WINDOW_TAB_PADDING_LEFT_LOGICAL_PX, WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX,
    WINDOW_TAB_RADIUS_LOGICAL_PX, WINDOW_TAB_RING_STROKE_LOGICAL_PX,
    WINDOW_TAB_SQUEEZED_LOGICAL_PX, WINDOW_TAB_SQUEEZED_PADDING_LOGICAL_PX,
    WINDOW_TAB_STATUS_DOT_LOGICAL_PX, WINDOW_TAB_STATUS_DOT_RIGHT_LOGICAL_PX,
    WINDOW_TAB_STATUS_DOT_TOP_LOGICAL_PX, WINDOW_TAB_TIGHT_LOGICAL_PX, WINDOW_TITLE_BAR_LOGICAL_PX,
    chrome_palette,
};

use crate::marks::{ChromeMark, ChromeSprite, Corner};

/// `.pane:not(.focused) .panehead .ticon { opacity: .5 }` (mock-up 1647).
///
/// The mark recedes on panes you are not in, through a channel of its own so it
/// cannot collide with what the accent or the breathing already say.
pub const PANE_MARK_UNFOCUSED_OPACITY: f32 = 0.5;

/// The tab-name editor's caret, in logical pixels.
///
/// `.rename` (mock-up 379-385) declares no caret of its own, so it wears the
/// browser's: a one-pixel hairline at 100%, DPI-rounded and never thinner than
/// one device pixel.
///
/// It is the same measure the terminal's own bar caret currently takes, and
/// deliberately not the *same constant*: that one lives behind the cursor
/// machine's exports, and an insertion point in the chrome is a chrome measure —
/// the day the terminal's caret becomes configurable, the tab strip's must not
/// follow it into a setting about terminal cursors.
pub const TAB_RENAME_CARET_LOGICAL_PX: f32 = 1.0;

/// `@keyframes tab-land`'s `from`, read straight off mock-up 962-965.
///
/// `background: color-mix(in srgb, var(--accent) 9%, transparent)` — the accent
/// at 9%, over whatever surface the tab is wearing.
pub const TAB_LAND_WASH_ALPHA: f32 = 0.09;
/// `box-shadow: inset 0 0 0 1.5px color-mix(in srgb, var(--accent) 45%, transparent)`
/// — the same accent at 45%, as a ring drawn *inside* the tab's own box.
pub const TAB_LAND_RING_ALPHA: f32 = 0.45;
/// The `1.5px` of that inset ring, in logical pixels.
pub const TAB_LAND_RING_LOGICAL_PX: f32 = 1.5;

/// §2.5 asks `bt-layout` to hold its own subpixel denominator and to pin it
/// against `bt-doc`'s "on the seam that can legally see both crates". This is
/// that seam: `bt-app` is the first place both are in scope at once.
const _: () = assert!(
    SUBPIXELS_PER_PX == bt_doc::SUBPIXELS_PER_PX,
    "bt-layout and bt-doc must agree on subpixels per logical pixel (§2.5)"
);

/// The tree, plus the two seat identities this window has a use for.
///
/// `terminal` is a geometry identity, not a content one (L1): it says *which
/// rectangle the terminal draws into*, and it would keep saying that if the
/// session inside it were swapped for another.
#[derive(Clone, Debug)]
pub struct Seats {
    tree: LayoutNode,
    terminal: SeatId,
    focus: SeatId,
    next_seat: u64,
    next_split: u64,
}

impl Seats {
    /// One terminal seat and nothing else: today's window, expressed as a tree.
    pub fn lone_terminal() -> Self {
        let terminal = SeatId(1);
        Self {
            tree: LayoutNode::seat(Seat::new(terminal, SeatKind::Terminal)),
            terminal,
            focus: terminal,
            next_seat: 2,
            next_split: 1,
        }
    }

    pub fn tree(&self) -> &LayoutNode {
        &self.tree
    }

    pub fn terminal(&self) -> SeatId {
        self.terminal
    }

    pub fn focus(&self) -> SeatId {
        self.focus
    }

    /// Whether the tree is a single terminal leaf — the shape that must render
    /// exactly as this product rendered before seats existed.
    pub fn is_lone_terminal(&self) -> bool {
        matches!(&self.tree, LayoutNode::Seat(seat) if seat.kind == SeatKind::Terminal)
    }

    /// Pane heads disambiguate siblings; a one-pane tree needs no pane chrome.
    ///
    /// The tree-wide answer, which is the one the *terminal* obeys. A files pane
    /// does not — see [`Self::seat_wears_head`].
    pub fn has_pane_headers(&self) -> bool {
        self.pane_count() > 1
    }

    /// Whether this one seat draws a head (C25, C26).
    ///
    /// Two rules, not one, and the mock-up states both at 4534-4542. A terminal
    /// pane earns its head only by having a sibling — a lone terminal's head
    /// would name the thing the tab above it already names, which is the zero-
    /// chrome discipline the whole product opens with. A files pane draws one
    /// *always*, because a tree that does not say where it is rooted is not
    /// useful, and because that head is also the pane's drag handle and its
    /// close button — take it away and the pane loses the two verbs every other
    /// pane has.
    ///
    /// Preview follows the terminal's rule rather than the files pane's: it is
    /// only ever opened beside something, so `pane_count > 1` is already true
    /// wherever it exists, and giving it the unconditional rule would be writing
    /// a branch no state can reach.
    /// A placeholder joins the files pane on the unconditional side, for the
    /// same reason and a sharper one: T227 asks the degradation to be visible,
    /// and a lone unrecognised leaf with no head and no body notice is a blank
    /// window — indistinguishable from the silent destruction the rule exists to
    /// forbid. The pane that cannot say what it is, is exactly the pane that has
    /// to say it.
    pub fn seat_wears_head(&self, kind: SeatKind) -> bool {
        matches!(kind, SeatKind::Files | SeatKind::Placeholder) || self.has_pane_headers()
    }

    /// How many panes this tab holds — `paneCount = leavesOf(w.tree).length`
    /// (mock-up line 3222).
    ///
    /// Every leaf counts, not only the terminals: the badge answers "how many
    /// rooms are behind this door", and a files pane is a room.
    pub fn pane_count(&self) -> usize {
        self.tree.seats_in_order().len()
    }

    /// The unpinned preview seat, if the tree has one.
    pub fn preview(&self) -> Option<SeatId> {
        self.tree
            .seats_in_order()
            .into_iter()
            .find(|seat| seat.kind == SeatKind::Preview)
            .map(|seat| seat.id)
    }

    /// Move layout focus. Focus is the solver's only input to W2 ("the focus
    /// seat falls last") and to L3's collapse order; it is deliberately *not*
    /// the same thing as keyboard focus, which v1 keeps on the terminal.
    pub fn set_focus(&mut self, seat: SeatId) -> bool {
        if self.focus == seat || self.tree.find_seat(seat).is_none() {
            return false;
        }
        self.focus = seat;
        true
    }

    /// Restore the positional `leaf-N` token carried beside the persisted tree.
    pub fn restore_focus_token(&mut self, token: &str) {
        let Some(index) = token
            .strip_prefix("leaf-")
            .and_then(|value| value.parse::<usize>().ok())
        else {
            return;
        };
        let Some(id) = self.tree.seats_in_order().get(index).map(|seat| seat.id) else {
            return;
        };
        self.focus = id;
    }

    /// Open the preview seat at its ruled fixed-right address, or close the one
    /// that is open. Returns whether the tree changed.
    pub fn toggle_preview(&mut self, metrics: &SeatMetrics) -> bool {
        match self.preview() {
            Some(existing) => self.close_seat(metrics, existing),
            None => {
                let seat = Seat::new(SeatId(self.next_seat), SeatKind::Preview);
                let split_id = SplitId(self.next_split);
                match apply(&self.tree, metrics, &Edit::LandPreview { seat, split_id }) {
                    Ok(outcome) => {
                        self.tree = outcome.tree;
                        self.next_seat += 1;
                        self.next_split += 1;
                        true
                    }
                    Err(_) => false,
                }
            }
        }
    }

    /// Close one seat, promoting its sibling. Refused for the last seat: an
    /// empty tree is not a state the solver can represent, and the last pane
    /// closing is the *tab* closing (§7.1.4), which this slice does not host.
    ///
    /// Takes the same `SeatMetrics` every other edit takes: one device scale is
    /// in force at a time, and an edit that reads a different one than the solve
    /// that follows it is two tables where §2.1 rules there is one.
    pub fn close_seat(&mut self, metrics: &SeatMetrics, seat: SeatId) -> bool {
        match apply(&self.tree, metrics, &Edit::CloseSeat { target: seat }) {
            Ok(outcome) => {
                self.tree = outcome.tree;
                if self.focus == seat {
                    self.focus = self.terminal;
                }
                true
            }
            Err(_) => false,
        }
    }

    /// Drag one divider. Returns whether a ratio was written.
    ///
    /// The clamp lives in `bt-layout::apply` and is not second-guessed here:
    /// §2.4 judges feasibility *before* clamping and refuses an impossible drag
    /// with zero side effects, and re-deriving that judgement at the call site
    /// is how a second, drifting copy of the rule gets born.
    pub fn drag_divider(
        &mut self,
        metrics: &SeatMetrics,
        split: SplitId,
        requested: Ratio,
        usable: LogicalPx,
    ) -> Result<bool, EditError> {
        let before = self.tree.ratios();
        let outcome = apply(
            &self.tree,
            metrics,
            &Edit::DragDivider {
                split,
                requested,
                usable,
            },
        )?;
        let changed = outcome.tree.ratios() != before;
        self.tree = outcome.tree;
        Ok(changed)
    }

    /// Solve this tree into the given viewport.
    pub fn solve(
        &self,
        viewport: LogicalRect,
        metrics: &SeatMetrics,
    ) -> Result<SeatLayout, bt_layout::LayoutError> {
        solve(
            &self.tree,
            viewport,
            metrics,
            self.focus,
            LayoutMode::Parallel,
        )
    }

    /// The minimum inner size to hand the OS (§2.6.5, tiny-window §4.2).
    pub fn min_inner_size(
        &self,
        metrics: &SeatMetrics,
        work_area: WorkAreaHint,
    ) -> Option<LogicalSize> {
        window_min_inner_size(
            &self.tree,
            metrics,
            self.focus,
            LogicalSize {
                width: LogicalPx::ZERO,
                height: LogicalPx::px(WINDOW_TITLE_BAR_LOGICAL_PX as i64),
            },
            work_area,
        )
    }

    /// Every split, with the axis it divides and the slot it divides — read off
    /// the solved rectangles rather than recomputed, so the divider the user
    /// grabs is the divider the solver drew (D4: one geometry, not two).
    pub fn split_slots(&self, layout: &SeatLayout) -> Vec<SplitSlot> {
        let mut out = Vec::new();
        collect_split_slots(&self.tree, layout, &mut out);
        out
    }
}

/// What L4 had no room to show, and the row that says so.
///
/// Not a seat, so it cannot travel as a [`bt_layout::SeatPlacement`]: it stands
/// for a number of seats rather than for one, and the solver's output type is
/// the tree's shape, not the chrome's. It rides beside the layout instead, laid
/// out by the same pass that laid out the bars so that the two cannot disagree
/// about where the strip ends (D4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FitOverflow {
    /// How many seats the foot had no row for. Never zero.
    pub hidden: usize,
    /// The row that names that number, in device pixels.
    pub row: bt_layout::DeviceRect,
}

/// The L4 presentation: what to show when `solve` says the window cannot hold
/// this tree at all (tiny-window §2 last row, §4.3).
///
/// The focus seat keeps a real pane (DESIGN §7.1.1, "保 focused pane") and every
/// other seat becomes a collapsed bar in a strip along the foot of the viewport,
/// as many bars as go in, with a trailing "N more do not fit" (tiny-window §4.3,
/// ruling 1). No buttons: window size is a gesture the user makes with the OS and
/// the app does not reach over and change it (§4.3, ruling and T214).
///
/// Three things about that are worth stating plainly:
///
/// * It is **not** a cached previous frame. §4.3 forbids silently reusing the
///   last geometry, because that dresses a failed solve up as a successful one.
///   Every rectangle here is derived from the current viewport and nothing else.
/// * For a lone terminal leaf it is *numerically the same answer* `solve`
///   returns on success — one seat, the whole viewport — so a window dragged
///   below the terminal's own minimum behaves exactly as it did before seats
///   existed, rather than acquiring a new failure state. There are no other
///   seats, so there is no strip, so the stage is the viewport.
/// * The bars are clickable exactly as L3's are, because they are the same
///   `Collapsed` placements: [`hit_chrome`] answers `CollapseBar`, the click
///   promotes that seat to the focus, and the next solve makes it the stage
///   (§2.6.3). L4 is therefore an escapable state without owning a single verb.
///
/// **Rulings this function makes, which neither document spells out.**
///
/// *The strip runs along the foot, in rows.* §4.3 says "尾行" — a tail *line* —
/// which only reads as a line if the bars are lines. A column of 24-wide strips
/// could carry neither the names §2.6.3 asks a bar for nor the sentence §4.3
/// asks the tail for. The foot rather than the head, because the tab strip is
/// already at the head and C24's argument is that the active tab must stay
/// welded to the surface below it.
///
/// *The stage is never given less than one bar's worth.* A pane thinner than a
/// collapsed bar is not a pane, it is a fourth bar without a name — so the strip
/// stops one row short of the viewport. When even that leaves no row, there is no
/// strip and no tail: at a height under two bars there is nowhere honest to print
/// a sentence, and the other seats go unpresented and unreported. That is the one
/// place this state cannot say everything it knows, and it is recorded rather
/// than papered over with a rectangle that does not fit.
///
/// *The seats that lose their row are the ones furthest from the focus.* That is
/// not a new order — it is `collapse_order`, the very order L3 uses to decide who
/// gives way first (H99), read one step further. The bars that remain are drawn
/// in tree order, because a bar's claim is that it still holds its place in the
/// tree (§2.6.3) and reading order is what that place looks like.
pub fn fit_what_fits(
    seats: &Seats,
    viewport: LogicalRect,
    metrics: &SeatMetrics,
) -> (SeatLayout, Option<FitOverflow>) {
    let device = |rect: LogicalRect| bt_layout::DeviceRect {
        left: snap(rect.left, metrics.scale_ppm()),
        top: snap(rect.top, metrics.scale_ppm()),
        right: snap(rect.right, metrics.scale_ppm()),
        bottom: snap(rect.bottom, metrics.scale_ppm()),
    };
    let in_order = seats.tree.seats_in_order();
    let others = in_order
        .iter()
        .filter(|seat| seat.id != seats.focus)
        .count();
    // One row of the foot per seat, and one row the strip may not touch so the
    // stage keeps a pane's worth of its own.
    let unit = bt_layout::COLLAPSED_EXTENT.subpixels();
    let capacity = (viewport.extent(Axis::Col).subpixels() / unit).max(0) as usize;
    let capacity = capacity.saturating_sub(1);
    let (shown, tail) = if capacity == 0 || others == 0 {
        (0, false)
    } else if others <= capacity {
        (others, false)
    } else {
        (capacity - 1, true)
    };
    let rows = shown + usize::from(tail);

    let unit = bt_layout::LogicalPx::from_subpixels(unit);
    let strip_top =
        viewport.bottom - bt_layout::LogicalPx::from_subpixels(unit.subpixels() * rows as i64);
    let row_rect = |index: usize| {
        let top = strip_top + bt_layout::LogicalPx::from_subpixels(unit.subpixels() * index as i64);
        LogicalRect::new(viewport.left, top, viewport.right, top + unit)
    };
    let stage = LogicalRect::new(viewport.left, viewport.top, viewport.right, strip_top);

    // The nearest `shown` seats keep a row; `collapse_order` runs farthest first,
    // so the survivors are its tail.
    let keeping = bt_layout::collapse_order(&seats.tree, seats.focus);
    let keeping: Vec<SeatId> = keeping
        .into_iter()
        .filter(|id| *id != seats.focus)
        .rev()
        .take(shown)
        .collect();

    let mut next_row = 0usize;
    let rects = in_order
        .into_iter()
        .map(|seat| {
            if seat.id == seats.focus {
                return bt_layout::SeatPlacement {
                    id: seat.id,
                    kind: seat.kind,
                    rect: Some(stage),
                    device_rect: Some(device(stage)),
                    presentation: Presentation::Full,
                };
            }
            if !keeping.contains(&seat.id) {
                // Not presented, and honestly so: a seat with no rectangle is the
                // shape the solver itself uses for "this one is not on screen",
                // never a zero-area one (red line L4).
                return bt_layout::SeatPlacement {
                    id: seat.id,
                    kind: seat.kind,
                    rect: None,
                    device_rect: None,
                    presentation: Presentation::Full,
                };
            }
            let rect = row_rect(next_row);
            next_row += 1;
            bt_layout::SeatPlacement {
                id: seat.id,
                kind: seat.kind,
                rect: Some(rect),
                device_rect: Some(device(rect)),
                // Squeezed along Col: 24 tall, the slot's full width — the shape
                // that can still carry a name.
                presentation: Presentation::Collapsed(bt_layout::AxisSet::COL),
            }
        })
        .collect();

    let overflow = tail.then(|| FitOverflow {
        hidden: others - shown,
        row: device(row_rect(rows - 1)),
    });
    (SeatLayout { rects }, overflow)
}

/// One seat's geometry stopped being what it was at the last commit (T230).
///
/// The obligation this block owes outward, stated as a type. `M2-tiny-window-
/// priority.md` §3.5 generalises an already-implemented rule — a TRANSIENT
/// overlay dissolves the moment the rectangle it anchored itself to moves —
/// from resize and wheel notches to *every* cause, and names four of them:
/// the concession ladder collapsing or expanding a seat across the L3 boundary,
/// a divider drag, a centre swap or replace, and entering or leaving focus mode.
/// Those four are things this block does; nobody outside it can see them happen;
/// so this block has to say so.
///
/// The four variants below are not those four causes. They are the four *facts*
/// a consumer can act on, and every one of the causes lands in one of them: the
/// ladder shows up as [`Self::Presentation`], a divider drag and a resize as
/// [`Self::Moved`], focus mode's parked seats as [`Self::Vanished`] and
/// [`Self::Appeared`], and so does opening or closing a pane. Naming causes
/// instead would put the burden of the mapping on every consumer, and each of
/// them would get it slightly differently.
///
/// One event per seat per commit, in the precedence the variants are declared
/// in. A collapse is also a move — 24 logical pixels is a different rectangle —
/// and reporting it as two facts would only ask a consumer to decide which of
/// them it already handled. The single case where a presentation could change
/// without the rectangle following, a seat that was already exactly
/// `COLLAPSED_EXTENT` along the axis it is now collapsed on, is subsumed rather
/// than lost: the consumer is told the seat changed, which is the question it
/// asked.
///
/// **Not covered, and deliberately so.** A centre swap exchanges what two seats
/// *hold* without moving either rectangle (`a_center_swap_moves_no_rectangle`).
/// Today a seat holds nothing but its `SeatKind`, which travels on the placement,
/// so a swap that changes anything at all is visible here as a
/// [`Self::Presentation`]-free `kind` difference — and a swap that changes
/// nothing is not an event. When seats grow payloads of their own, the edit that
/// swaps them has to publish for itself; a diff of rectangles will not see it,
/// and this comment is the place that says so before it is a bug.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutEvent {
    /// A seat that had no rectangle at the last commit has one now.
    Appeared(SeatId),
    /// A seat that had a rectangle no longer has one — closed, or parked off
    /// the stage by focus mode.
    Vanished(SeatId),
    /// The same seat, presented differently: the ladder crossed L3 for it, in
    /// either direction, or it changed which axis it is squeezed on.
    Presentation(SeatId),
    /// The same seat, the same presentation, a different rectangle.
    Moved(SeatId),
}

/// Everything that changed between two commits of the layout (T230).
///
/// A diff and not a set of hooks hung off the edits, for the reason §4.3 gives
/// about geometry generally: the edits are many and growing, the commit is one,
/// and an edit added later that forgets to publish is a bug that shows up as a
/// stale overlay days afterwards. Here an edit *cannot* forget, because it is not
/// asked — the commit point compares what it is about to store against what it
/// stored last, and a rebuild that changed nothing produces nothing.
///
/// Events come out in tree order (D2/L8: never a hash iteration), seats that
/// left the tree last.
#[must_use]
pub fn layout_events(before: &SeatLayout, after: &SeatLayout) -> Vec<LayoutEvent> {
    let mut events = Vec::new();
    for placement in &after.rects {
        let was = before.get(placement.id);
        match (was.and_then(|was| was.rect), placement.rect) {
            (None, Some(_)) => events.push(LayoutEvent::Appeared(placement.id)),
            (Some(_), None) => events.push(LayoutEvent::Vanished(placement.id)),
            (None, None) => {}
            (Some(old), Some(new)) => {
                let was = was.expect("a rectangle came from a placement");
                if was.presentation != placement.presentation || was.kind != placement.kind {
                    events.push(LayoutEvent::Presentation(placement.id));
                } else if old != new {
                    events.push(LayoutEvent::Moved(placement.id));
                }
            }
        }
    }
    for placement in &before.rects {
        if placement.rect.is_some() && after.get(placement.id).is_none() {
            events.push(LayoutEvent::Vanished(placement.id));
        }
    }
    events
}

/// Boundary snapping, matching `bt-layout`'s own: round half away from zero, in
/// integers (D3).
fn snap(value: LogicalPx, scale_ppm: u32) -> i64 {
    let numer = i128::from(value.subpixels()) * i128::from(scale_ppm);
    let denom = i128::from(SUBPIXELS_PER_PX) * 1_000_000;
    let half = denom / 2;
    if numer >= 0 {
        ((numer + half) / denom) as i64
    } else {
        ((numer - half) / denom) as i64
    }
}

/// One split's divider, in the coordinates the solver answered in.
#[derive(Clone, Copy, Debug)]
pub struct SplitSlot {
    pub id: SplitId,
    pub dir: Axis,
    /// The whole rectangle the two sides share, including the divider.
    pub slot: LogicalRect,
    /// The divider band itself, in device pixels: from the leading side's far
    /// device edge to the trailing side's near device edge. Taking it from the
    /// two device rectangles rather than rounding it separately is red line L6
    /// applied to chrome — the band is exactly the gap, so it cannot leave a
    /// seam on one side and overlap on the other.
    pub band: [f32; 4],
}

fn collect_split_slots(node: &LayoutNode, layout: &SeatLayout, out: &mut Vec<SplitSlot>) {
    let LayoutNode::Split { id, dir, a, b, .. } = node else {
        return;
    };
    if let (Some(rect_a), Some(rect_b), Some(dev_a), Some(dev_b)) = (
        logical_bounds(a, layout),
        logical_bounds(b, layout),
        device_bounds(a, layout),
        device_bounds(b, layout),
    ) {
        let slot = LogicalRect::new(
            rect_a.left.min(rect_b.left),
            rect_a.top.min(rect_b.top),
            rect_a.right.max(rect_b.right),
            rect_a.bottom.max(rect_b.bottom),
        );
        let band = match dir {
            Axis::Row => [
                dev_a[2] as f32,
                dev_a[1].max(dev_b[1]) as f32,
                dev_b[0] as f32,
                dev_a[3].min(dev_b[3]) as f32,
            ],
            Axis::Col => [
                dev_a[0].max(dev_b[0]) as f32,
                dev_a[3] as f32,
                dev_a[2].min(dev_b[2]) as f32,
                dev_b[1] as f32,
            ],
        };
        out.push(SplitSlot {
            id: *id,
            dir: *dir,
            slot,
            band,
        });
    }
    collect_split_slots(a, layout, out);
    collect_split_slots(b, layout, out);
}

fn logical_bounds(node: &LayoutNode, layout: &SeatLayout) -> Option<LogicalRect> {
    let mut bounds: Option<LogicalRect> = None;
    for seat in node.seats_in_order() {
        let rect = layout.get(seat.id)?.rect?;
        bounds = Some(match bounds {
            None => rect,
            Some(acc) => LogicalRect::new(
                acc.left.min(rect.left),
                acc.top.min(rect.top),
                acc.right.max(rect.right),
                acc.bottom.max(rect.bottom),
            ),
        });
    }
    bounds
}

fn device_bounds(node: &LayoutNode, layout: &SeatLayout) -> Option<[i64; 4]> {
    let mut bounds: Option<[i64; 4]> = None;
    for seat in node.seats_in_order() {
        let rect = layout.get(seat.id)?.device_rect?;
        bounds = Some(match bounds {
            None => [rect.left, rect.top, rect.right, rect.bottom],
            Some(acc) => [
                acc[0].min(rect.left),
                acc[1].min(rect.top),
                acc[2].max(rect.right),
                acc[3].max(rect.bottom),
            ],
        });
    }
    bounds
}

/// Device pixels per logical pixel in parts per million, from the renderer's
/// already-quantised DPI. Derived from `dpi_milli` rather than from the raw
/// `f64` scale factor so two calls at the same DPI cannot disagree by a ULP —
/// D3 wants the solver's inputs as stable as its arithmetic.
pub fn scale_ppm(dpi_milli: u32) -> u32 {
    dpi_milli.saturating_mul(1_000).max(1)
}

/// The ruled per-kind table at this device scale.
pub fn seat_metrics(dpi_milli: u32) -> SeatMetrics {
    SeatMetrics::ruled(scale_ppm(dpi_milli))
}

/// The seats viewport rectangle, in logical pixels, for a client area of this
/// many device pixels. Its top is the lower edge of the 40px window title bar.
///
/// The rounding here is the exact inverse of the solver's boundary snapping: a
/// lone leaf's rectangle *is* this viewport, and snapping it back must land on
/// the original device pixel or the byte-identity gate fails on the first
/// fractional DPI. It does: the inverse errs by at most half a subpixel, which
/// re-snaps to under 0.002 device pixels at any scale this product will meet.
pub fn logical_viewport(width_px: u32, height_px: u32, scale_ppm: u32) -> LogicalRect {
    let title_px = seats_top_device_px(height_px, scale_ppm);
    LogicalRect::new(
        LogicalPx::ZERO,
        device_to_logical(title_px, scale_ppm),
        device_to_logical(width_px, scale_ppm),
        device_to_logical(height_px, scale_ppm),
    )
}

/// The device row the seats begin on: the lower edge of the 40px title bar,
/// never past the bottom of a client area too short to hold it.
fn seats_top_device_px(height_px: u32, scale_ppm: u32) -> u32 {
    logical_to_device(WINDOW_TITLE_BAR_LOGICAL_PX, scale_ppm).min(height_px.saturating_sub(1))
}

/// The layout's own box in device pixels — the mock-up's `#termhost` rectangle
/// (`[left, top, right, bottom]`).
///
/// Every drop-zone distance in K is measured from *this* and not from the union
/// of the seats inside it, and the difference is the whole of K130: the rim
/// belongs to the layout, so it has to exist even where no seat reaches. Under
/// the L4 concession ladder the seats stop short of the bottom, and a rim
/// derived from them would move with the ladder — the window's own edge does
/// not.
///
/// It shares [`seats_top_device_px`] with [`logical_viewport`] rather than
/// re-deriving the title bar, because the two answers are the same fact seen
/// from either side: the row the solver's viewport starts on is the row the
/// pointer enters the layout on, and a drop zone that disagreed with the solver
/// by one pixel would be a zone that aims at a seam.
#[must_use]
pub fn device_viewport(width_px: u32, height_px: u32, scale_ppm: u32) -> [f64; 4] {
    [
        0.0,
        f64::from(seats_top_device_px(height_px, scale_ppm)),
        f64::from(width_px),
        f64::from(height_px),
    ]
}

fn logical_to_device(logical_px: f32, scale_ppm: u32) -> u32 {
    (logical_px * scale_ppm as f32 / 1_000_000.0)
        .round()
        .max(0.0) as u32
}

fn device_to_logical(device_px: u32, scale_ppm: u32) -> LogicalPx {
    let numer = i128::from(device_px) * i128::from(SUBPIXELS_PER_PX) * 1_000_000;
    let denom = i128::from(scale_ppm.max(1));
    LogicalPx::from_subpixels(((numer + denom / 2) / denom) as i64)
}

/// The terminal seat's rectangle as the renderer wants it.
pub fn seat_viewport(layout: &SeatLayout, seat: SeatId) -> Option<SeatViewport> {
    let device = layout.get(seat)?.device_rect?;
    Some(SeatViewport {
        x: device.left.max(0) as u32,
        y: device.top.max(0) as u32,
        width: device.width().max(1) as u32,
        height: device.height().max(1) as u32,
    })
}

/// A pane's content rectangle. A multi-pane tree excludes the common 28px pane
/// head; a lone terminal leaf consumes its whole seat. This is the only
/// rectangle allowed to derive terminal rows.
pub fn pane_body_viewport(
    seats: &Seats,
    layout: &SeatLayout,
    seat: SeatId,
    scale: f32,
) -> Option<SeatViewport> {
    let mut viewport = seat_viewport(layout, seat)?;
    let kind = layout.get(seat)?.kind;
    if !seats.seat_wears_head(kind) {
        return Some(viewport);
    }
    let head_height = (SEAT_TITLE_BAR_LOGICAL_PX * scale).round().max(1.0) as u32;
    let consumed = head_height.min(viewport.height.saturating_sub(1));
    viewport.y = viewport.y.saturating_add(consumed);
    viewport.height = viewport.height.saturating_sub(consumed).max(1);
    Some(viewport)
}

/// The drawable body of a preview seat, excluding its existing title bar.
pub fn preview_body_viewport(
    seats: &Seats,
    layout: &SeatLayout,
    seat: SeatId,
    scale: f32,
) -> Option<SeatViewport> {
    pane_body_viewport(seats, layout, seat, scale)
}

/// Something in the chrome the pointer can be over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChromeTarget {
    Divider(SplitId),
    /// A collapsed seat's bar (§2.6.3): the whole strip is clickable.
    CollapseBar(SeatId),
    /// The common pane head, everywhere its own controls are not.
    PaneHeader(SeatId),
    /// `.panehead .pane-close` — the `×` that closes this pane (mock-up
    /// 1650-1657, `closePane` at 5825).
    ///
    /// Its own target rather than a sub-case of [`Self::PaneHeader`] because the
    /// head is a drag handle and the `×` is a dead zone inside it (C35, mock-up
    /// 5837/5844): "the button is not the bar" has to be true at the hit test or
    /// it is not true anywhere.
    PaneClose(SeatId),
    Settings,
    Minimize,
    Maximize,
    CloseWindow,
    Tab(usize),
    TabClose(usize),
    /// The pin, which stands in the `×`'s own slot: an unpinned tab's while the
    /// pointer is on it, a pinned tab's always. Never both at once — a pinned
    /// tab has no `×` at all, and that it cannot be shut by a stray click is the
    /// feature rather than a side effect (mock-up 4059-4065).
    TabPin(usize),
    NewTab,
    /// `.newtab.chevbtn.nt-chev` — the profile picker that shares the `+`'s box
    /// and stands immediately beside it.
    NewTabMenu,
}

/// How much room a tab has, measured the way the mock-up's own
/// `updateTabSqueeze` measures it: off the tab's rendered width, never off the
/// number of tabs. Counting would be a heuristic; the strip's width, the cap and
/// the window's own size all feed the answer, and only the width knows all three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabWidthTier {
    /// Room for everything the tab carries.
    Full,
    /// Below 140px: the title keeps its room, and a tab that is not the active
    /// one gives up its `×` to buy that room.
    Tight,
    /// Below 90px: no legible room for words at all, so the tab is its centred
    /// mark and nothing else.
    Squeezed,
}

/// The pin's box — `.tab .pin { width: 17px; height: 17px; border-radius: 4px }`
/// (mock-up 319-320). Written as the `×`'s own box rather than as a second 17,
/// because "same box as `.close` because it stands in the same slot" (line 314)
/// is the rule; two copies of the number could drift apart while every test that
/// only checked the number still passed.
const WINDOW_TAB_PIN_BOX_LOGICAL_PX: f32 = WINDOW_TAB_CLOSE_BOX_LOGICAL_PX;
const WINDOW_TAB_PIN_RADIUS_LOGICAL_PX: f32 = WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX;
/// `.pinsvg { width: 13px; height: 13px }` (mock-up 365) — deliberately *not*
/// the `×` mark's 8px, and the mock-up says why at 362-364: "the pin carries a
/// state and a glyph that has to survive a 45° turn, and both cost silhouette.
/// It is not the close button's twin and sizing it like one made it read as
/// lint."
const WINDOW_TAB_PIN_GLYPH_LOGICAL_PX: f32 = 13.0;
/// `.tab .pin + .close { margin-left: -4px }` (mock-up 329-333), and the same
/// -4px again on a lone `.pin.on` (353-357): "the trailing controls cluster
/// tighter than the tab's 8px gap — that gap is right between the title and the
/// controls, too airy between the controls themselves".
const WINDOW_TAB_TRAILER_TIGHTEN_LOGICAL_PX: f32 = 4.0;

/// What one tab hangs off its trailing end, and how far into the reveal it is.
///
/// `reveal` is `.tab:hover .pin`'s expansion, `0.0 ..= 1.0`: the mock-up animates
/// `width: 0 -> 17px` and `margin-left: -8px -> 0` together over .16s (lines
/// 338-349), so a half-open pin is a real frame that has to lay out. It arrives
/// as an *input* because the clock belongs to the caller — this module stays a
/// pure function of the numbers it is handed.
///
/// A pinned tab ignores `reveal` entirely: `.pin.on` stands at full width whether
/// or not the pointer is anywhere near it, because it is not an offer that comes
/// and goes but a fact about the tab.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TabTrailer {
    pub pinned: bool,
    pub reveal: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabGeometry {
    pub body: [f32; 4],
    /// The `×`'s box — `None` exactly when the mock-up's width tiers take the
    /// affordance away, which is also exactly when a press there must fall
    /// through to the tab instead of closing it; and `None` on every pinned tab
    /// at every width, because `tabTrailer` (mock-up 4204-4207) writes no
    /// `.close` element for one at all.
    pub close: Option<[f32; 4]>,
    /// The pin's box — the same 17px box as the `×` because it stands in the
    /// same slot. `None` whenever nothing is drawn there: the two narrow tiers
    /// take the pin away outright (`.tab.tight .pin { display: none }`), and an
    /// unpinned tab at rest has one of literally zero width.
    pub pin: Option<[f32; 4]>,
    /// The trailer this geometry was built from, clamped.
    ///
    /// Kept because the trailing boundary cannot be read off the rectangles
    /// alone: the pin's `margin-left` runs from -8px to 0 as it opens, and a
    /// margin leaves no box behind to measure.
    pub trailer: TabTrailer,
    pub tier: TabWidthTier,
}

impl TabGeometry {
    /// The same tab drawn `dx` physical pixels along the strip.
    ///
    /// A drag moves a tab *visually* and never in layout: the slot this geometry
    /// describes stays where the tab's index puts it, and only the paint is
    /// displaced. Every box inside a tab is measured off its body, so shifting
    /// the whole geometry once — rather than each of the mark, the title, the
    /// badge, the pin and the `×` at its own call site — is what keeps a
    /// translated tab internally consistent by construction.
    #[must_use]
    pub fn shifted(&self, dx: f32) -> Self {
        let slide = |rect: [f32; 4]| [rect[0] + dx, rect[1], rect[2] + dx, rect[3]];
        Self {
            body: slide(self.body),
            close: self.close.map(slide),
            pin: self.pin.map(slide),
            ..*self
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabStripGeometry {
    pub tabs: Vec<TabGeometry>,
    pub new_tab: [f32; 4],
    /// The `˅` beside the `+`: the same 28px box, no margin between them.
    pub new_tab_menu: [f32; 4],
    /// The strip's clip box as `[left, right]` — `.tabs-inline`'s own border box,
    /// which is what `overflow-x: auto` crops its content to.
    ///
    /// The left edge is the window's, not the first tab's inset: content scrolled
    /// off that end leaves the surface entirely and the framebuffer is its clip.
    /// The right edge is where the caption run begins, and it is the edge that
    /// matters — the mock-up added the scroller precisely so that many tabs stop
    /// "spilling into the caption buttons" (line 187).
    pub viewport: [f32; 2],
    /// The furthest this strip may be scrolled, and therefore also the test for
    /// whether it scrolls at all: `0.0` exactly when everything fits.
    pub max_scroll: f32,
}

/// Equal-share horizontal tab geometry, scrolled by `scroll` physical pixels.
///
/// Every tab takes the same share of the run, clamped to the mock-up's own two
/// bounds — `.tab { flex: 1 1 0; min-width: 46px; max-width: 200px }`. The floor
/// is the whole of A7/A8: a flex item that has hit `min-width` stops shrinking,
/// so the row overflows its `overflow-x: auto` parent and becomes a scroller
/// instead of compressing into illegibility.
///
/// `scroll` is clamped here rather than trusted, so no caller can produce a strip
/// scrolled past its own content — including the caller that has not yet noticed
/// the window got wider or a tab got closed.
///
/// The `+`/`˅` pair rides *inside* the scroller, because the mock-up puts it
/// inside: `paintStrip` writes both buttons into `#tabstrip` (line 4315), and
/// `.tabs-inline .newtab` (line 404) is the rule that spaces them there. So they
/// are content, they scroll with the tabs, and they are last in the run.
///
/// `active_tab` is a geometry input and not merely a paint one, because the
/// mock-up's width tiers keep the active tab's `×` and take everyone else's:
/// `.tab.tight:not(.active) .close { display: none }`.
///
/// `trailers` is one entry per tab and is therefore also the tab count: what a
/// tab hangs off its trailing end changes that tab's own furniture and nothing
/// else in the strip, but it changes it per tab, so a bare count could no longer
/// answer the question (pinned in `a_trailer_moves_nothing_outside_its_own_tab`).
pub fn tab_strip_geometry(
    width: f32,
    scale: f32,
    trailers: &[TabTrailer],
    active_tab: usize,
    scroll: f32,
) -> TabStripGeometry {
    let tab_count = trailers.len();
    let title = (WINDOW_TITLE_BAR_LOGICAL_PX * scale).round();
    let radius = (WINDOW_TAB_RADIUS_LOGICAL_PX * scale).round().max(1.0);
    let caption = WINDOW_CAPTION_BUTTON_LOGICAL_PX * scale;
    let run_left = (width - 4.0 * caption).max(0.0);
    let gap = WINDOW_TAB_GAP_BETWEEN_LOGICAL_PX * scale;
    let new_box = WINDOW_NEW_TAB_BOX_LOGICAL_PX * scale;
    let new_margin = WINDOW_NEW_TAB_MARGIN_LEFT_LOGICAL_PX * scale;
    // Two buttons now stand at the end of the run, and both of them have to fit
    // before a tab may claim the rest — the `˅` is `margin-left: 0`, so the pair
    // costs one margin and two boxes.
    let available = (run_left - radius - new_margin - 2.0 * new_box).max(0.0);
    let total_gaps = gap * tab_count.saturating_sub(1) as f32;
    let tab_width = if tab_count == 0 {
        0.0
    } else {
        // `min-width: 46px` is a floor, not a preference: below it the row stops
        // shrinking and the parent starts scrolling. Clamping here is what turns
        // "infinitely compressible" into "scrollable past 46".
        ((available - total_gaps).max(0.0) / tab_count as f32).clamp(
            WINDOW_TAB_MIN_WIDTH_LOGICAL_PX * scale,
            WINDOW_TAB_MAX_WIDTH_LOGICAL_PX * scale,
        )
    };
    // The run at its natural width, buttons included. When the tabs were free to
    // compress this always equalled the space available and nothing scrolled;
    // now that they stop at 46px it is allowed to exceed it, and the excess is
    // exactly how far the strip may be scrolled.
    let content = tab_count as f32 * tab_width + total_gaps + new_margin + 2.0 * new_box;
    let max_scroll = (radius + content - run_left).max(0.0);
    let scroll = scroll.clamp(0.0, max_scroll);
    let origin = radius - scroll;
    let tier = tab_width_tier(tab_width, scale);
    let tab_height = (WINDOW_TAB_HEIGHT_LOGICAL_PX * scale).round();
    let tab_top = title - tab_height;
    let close_box = (WINDOW_TAB_CLOSE_BOX_LOGICAL_PX * scale).round();
    let close_pad = WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX * scale;
    let pin_box = (WINDOW_TAB_PIN_BOX_LOGICAL_PX * scale).round();
    let tighten = WINDOW_TAB_TRAILER_TIGHTEN_LOGICAL_PX * scale;
    let mark = (WINDOW_TAB_MARK_LOGICAL_PX * scale).round();
    let content_gap = WINDOW_TAB_GAP_LOGICAL_PX * scale;
    let tabs = trailers
        .iter()
        .enumerate()
        .map(|(index, trailer)| {
            // Clamped here rather than trusted, for the same reason `scroll` is:
            // no caller can hand this module an animation that has overshot.
            let trailer = TabTrailer {
                pinned: trailer.pinned,
                reveal: trailer.reveal.clamp(0.0, 1.0),
            };
            let left = origin + index as f32 * (tab_width + gap);
            let right = left + tab_width;
            let active = index == active_tab;
            let close_top = tab_top + (tab_height - close_box) / 2.0;
            let close = if trailer.pinned {
                // `tabTrailer` (mock-up 4204-4207) writes *either* a `.pin.on`
                // *or* a `.pin` + `.close` pair: a pinned tab has no `×` in the
                // DOM at all, at any width. This outranks the width tiers, which
                // only ever take affordances away.
                None
            } else {
                match tier {
                    // `.tab.tight:not(.active) .close` and its `.squeezed` twin.
                    TabWidthTier::Tight | TabWidthTier::Squeezed if !active => None,
                    // A squeezed tab centres what is left of it: the mark, the
                    // tab's own 8px gap, and the `×` the active tab keeps.
                    TabWidthTier::Squeezed => {
                        let content = mark + content_gap + close_box;
                        let close_left = (left + (tab_width - content) / 2.0 + mark + content_gap)
                            .max(left)
                            .round();
                        Some([
                            close_left,
                            close_top,
                            (close_left + close_box).min(right),
                            close_top + close_box,
                        ])
                    }
                    _ => {
                        let close_right = (right - close_pad).max(left);
                        Some([
                            (close_right - close_box).max(left),
                            close_top,
                            close_right,
                            close_top + close_box,
                        ])
                    }
                }
            };
            let pin = match tier {
                // `.tab.tight .pin` and `.tab.squeezed .pin { display: none }`
                // (mock-up 197, 201). Unlike the `×`'s rule this one carries no
                // `:not(.active)`: at these widths the pin retreats to give the
                // title its room, and even a pinned active tab's `.pin.on` goes
                // with it — summoning the hover controls here "crushed the title
                // to 0px and left an icon soup".
                TabWidthTier::Tight | TabWidthTier::Squeezed => None,
                // Pinned: the pin *is* the trailer, and it stands exactly where
                // the `×` would have — right edge on the tab's own trailing
                // padding. Same place, so unpinning is where you already are.
                TabWidthTier::Full if trailer.pinned => {
                    let pin_right = (right - close_pad).max(left);
                    Some([
                        (pin_right - pin_box).max(left),
                        close_top,
                        pin_right,
                        close_top + pin_box,
                    ])
                }
                // Unpinned: the pin sits to the LEFT of the `×`, and the revealed
                // cluster is tighter than the tab's own 8px gap by the -4px of
                // `.pin + .close`. Its width is the reveal itself, so at rest it
                // is a zero-width box — dropped below, and costing the title
                // nothing at all.
                TabWidthTier::Full => close.map(|close| {
                    let pin_right = (close[0] - tighten).max(left);
                    [
                        (pin_right - pin_box * trailer.reveal).max(left),
                        close_top,
                        pin_right,
                        close_top + pin_box,
                    ]
                }),
            };
            TabGeometry {
                body: [left, tab_top, right, title],
                close: close.filter(|rect| rect[2] > rect[0]),
                pin: pin.filter(|rect| rect[2] > rect[0]),
                trailer,
                tier,
            }
        })
        .collect::<Vec<_>>();
    let tabs_right = tabs.last().map_or(origin, |tab| tab.body[2]);
    let new_left = tabs_right + new_margin;
    let new_bottom = title - WINDOW_NEW_TAB_MARGIN_BOTTOM_LOGICAL_PX * scale;
    let menu_left = new_left + new_box;
    // Neither button is clamped to the run's end any more. They used to be,
    // because a strip that could not scroll had to stop *somewhere* and the
    // caption run was the wall; a strip that scrolls has the wall in the right
    // place already — `viewport` — and clamping on top of it would pin the pair
    // to the edge while the tabs slid under them.
    TabStripGeometry {
        tabs,
        new_tab: [
            new_left,
            new_bottom - new_box,
            new_left + new_box,
            new_bottom,
        ],
        new_tab_menu: [
            menu_left,
            new_bottom - new_box,
            menu_left + new_box,
            new_bottom,
        ],
        viewport: [0.0, run_left],
        max_scroll,
    }
}

/// Whether a mark at `rect` may be drawn without spilling past the strip's
/// right edge.
///
/// Only that edge is tested, and that asymmetry is the honest one: the strip's
/// left edge *is* the surface's left edge, so a quad running off it is clipped
/// by the framebuffer with its texture coordinates interpolated correctly, for
/// free and exactly. Nothing lies beyond the right edge but the caption buttons,
/// and a tab drawn over those is the very bug the mock-up added this scroller to
/// fix (line 187).
///
/// That a mark crossing the right edge is dropped rather than cropped is a
/// **ruling**. `ChromeLabel` clips per glyph and per pixel, so a title is cropped
/// exactly as CSS would crop it; a chrome icon, by contrast, is rasterised into
/// the precise box it occupies and drawn with whole-texture UVs, so cropping one
/// would mean re-rasterising it on every scrolled pixel — trading a real cache
/// for half a 15px square that says nothing the whole one did not. The case that
/// would actually have been felt cannot arise: the active tab's silhouette is
/// always whole, because activating a tab scrolls it wholly into view first (see
/// [`tab_scroll_to_reveal`]).
fn within_strip(viewport: [f32; 2], rect: [f32; 4]) -> bool {
    rect[2] <= viewport[1]
}

/// The leftmost box of a tab's trailing cluster — the pin when one is drawn, the
/// `×` otherwise, and `None` when the tab carries neither.
///
/// The pin is always left of the `×` when both exist, so `or` is an ordering
/// claim and not a preference.
fn tab_trailer_box(tab: &TabGeometry) -> Option<[f32; 4]> {
    tab.pin.or(tab.close)
}

/// The tab row's trailing boundary: the left edge of the trailing cluster,
/// *including* the flex gap standing before it. The badge docks its right edge
/// here and the title stops one badge further back, and both ask this one
/// function — they are the exact pair the mock-up's own -4px note is about
/// (lines 353-357), so they must not be able to drift.
///
/// The flex arithmetic, spelled out because the rectangles alone do not show it.
/// `.tab` is a flex row with `gap: 8px`; the pin and the `×` are `flex: none` and
/// last in it, so the pair packs against the content box's right edge while the
/// title (`flex: 1; min-width: 0`) eats every remaining pixel. Walking leftwards
/// from the tab's right edge, with `p` for the 6px trailing padding:
///
/// ```text
/// pinned — the trailer is the pin alone (mock-up 4204-4205):
///   pin.right = tab_right - p          pin.left = pin.right - 17
///   boundary  = pin.left - (8 - 4)     `.tab .pin.on { margin-left: -4px }`
///             = tab_right - 27
///
/// unpinned, at rest (reveal 0): `.pin` is still in the flow — width 0, and
/// `margin-left: -8px` "cancel[s] this item's share of the flex gap" (line 339),
/// so it costs the row nothing. But it is still a sibling, so `.pin + .close`'s
/// own -4px survives:
///   close.right = tab_right - p        close.left = tab_right - 23
///   boundary    = close.left - (8 - 4) = tab_right - 27   ← the same column
///
/// unpinned, revealed by `r`: width is 17r and margin-left is -8(1 - r), so the
/// pin's whole contribution to the row is 8 + (8r - 8) + 17r = 25r:
///   pin.right = close.left - 4         pin.left = pin.right - 17r
///   boundary  = pin.left - 8r          = tab_right - 27 - 25r
///
/// tight / squeezed: `display: none` takes `.pin` out of the flow entirely, and
/// takes `.pin + .close`'s -4px with it — there is no such sibling pair left:
///   boundary = close.left - 8, or tab_right - p when there is no `×` either
/// ```
///
/// That the pinned row and the resting unpinned row land on the *same* column is
/// the whole point of the -4px on `.pin.on`, and the mock-up records the bug it
/// was written for: "the two counts sat 4px apart".
fn tab_trailing_edge(tab: &TabGeometry, scale: f32) -> f32 {
    let gap = WINDOW_TAB_GAP_LOGICAL_PX * scale;
    let tightened = gap - WINDOW_TAB_TRAILER_TIGHTEN_LOGICAL_PX * scale;
    match (tab.pin, tab.close) {
        // Pinned: `.pin.on` alone, carrying the -4px that lines it up with the
        // row below it.
        (Some(pin), None) => pin[0] - tightened,
        // Unpinned and open: the gap before the pin has opened by exactly the
        // reveal, because that is how far its -8px margin has run back to 0.
        (Some(pin), Some(_)) => pin[0] - gap * tab.trailer.reveal,
        // Unpinned and shut: a zero-width `.pin` still standing between the
        // badge and the `×`, which is what leaves the -4px behind.
        (None, Some(close)) if tab.tier == TabWidthTier::Full => close[0] - tightened,
        // Narrow tiers: no `.pin` in the flow, so the tab's own 8px gap is all
        // that stands before the `×`.
        (None, Some(close)) => close[0] - gap,
        // Nothing trails at all: the row ends on the tab's own padding.
        (None, None) => tab.body[2] - WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX * scale,
    }
}

/// The `.panecount` pill's box on a tab, or `None` when the tab shows none.
///
/// Two conditions take it away, and they are different facts. `paneCount > 1`
/// is the badge's whole reason — "only shown once it holds more than one"
/// (mock-up line 292) — and a lone pane reserves no space for the badge it is
/// not drawing. `.tab.squeezed .panecount { display: none }` (line 201) is the
/// other: under 90px the tab is its centred mark and carries nothing else.
///
/// The width is the mock-up's own `max(min-width, text + padding)`: 15px until
/// the number needs more, and then 4px of padding either side of it.
#[must_use]
pub fn tab_badge_rect(
    tab: &TabGeometry,
    pane_count: usize,
    badge_text_width: f32,
    scale: f32,
) -> Option<[f32; 4]> {
    if pane_count <= 1 || tab.tier == TabWidthTier::Squeezed {
        return None;
    }
    let trailing = tab_trailing_edge(tab, scale);
    let badge_width = (badge_text_width + 2.0 * WINDOW_TAB_BADGE_PADDING_X_LOGICAL_PX * scale)
        .max(WINDOW_TAB_BADGE_MIN_WIDTH_LOGICAL_PX * scale)
        .round();
    let badge_height = (WINDOW_TAB_BADGE_HEIGHT_LOGICAL_PX * scale).round();
    let left = (trailing - badge_width).round();
    let top = (tab.body[1] + (tab.body[3] - tab.body[1] - badge_height) / 2.0).round();
    Some([left, top, left + badge_width, top + badge_height])
}

/// Where a tab's mark sits, which is also where its title starts.
///
/// Split out of `window_chrome` when the rename editor needed the same answer:
/// the editor *is* the title (mock-up 376-378, "same box, same metrics"), so the
/// box it measures its caret against and the box the strip draws the title in
/// have to be one computation. Two would drift, and the drift would show as a
/// caret standing beside its own letters.
fn tab_mark_left(tab: &TabGeometry, scale: f32) -> f32 {
    let mark = (WINDOW_TAB_MARK_LOGICAL_PX * scale).round();
    let content_gap = WINDOW_TAB_GAP_LOGICAL_PX * scale;
    if tab.tier == TabWidthTier::Squeezed {
        let trailing =
            tab_trailer_box(tab).map_or(0.0, |trailer| content_gap + trailer[2] - trailer[0]);
        (tab.body[0] + (tab.body[2] - tab.body[0] - mark - trailing) / 2.0)
            .max(tab.body[0] + WINDOW_TAB_SQUEEZED_PADDING_LOGICAL_PX * scale)
            .round()
    } else {
        (tab.body[0] + WINDOW_TAB_PADDING_LEFT_LOGICAL_PX * scale).round()
    }
}

/// The box a tab's mark occupies — its icon, or the progress ring that replaces
/// it in the same slot.
///
/// Public for the reason [`tab_title_box`] is: something outside this module has
/// to measure the same box the strip draws in. The tooltip host anchors on it,
/// because D38's `NN%` belongs to the *ring* and not to the tab it sits on, and
/// an anchor measured a second way would be a tip standing beside the thing it
/// claims to describe.
#[must_use]
pub fn tab_mark_box(tab: &TabGeometry, scale: f32) -> [f32; 4] {
    let mark = (WINDOW_TAB_MARK_LOGICAL_PX * scale).round();
    let left = tab_mark_left(tab, scale);
    let top = (tab.body[1] + (tab.body[3] - tab.body[1] - mark) / 2.0).round();
    [left, top, left + mark, top + mark]
}

/// The box a tab's title is laid out in and clipped to, or `None` when the tab
/// is too narrow to hold one.
///
/// `.tab .ttitle { flex: 1; min-width: 0 }` between the mark and whatever the
/// tab hangs off its trailing end — the `×`, the pin, or the pane-count badge in
/// front of them. `.tab.squeezed .ttitle { display: none }` (mock-up line 201)
/// is the `None`: below 90px "the tab is its centred icon" and there is nothing
/// gained by clipping a word to two letters.
///
/// Public because the rename editor is drawn *into* this box and has to measure
/// its own text against the box's width before the strip is built — only the
/// font knows how wide a draft is, and only the strip knows how much room it has.
#[must_use]
pub fn tab_title_box(
    tab: &TabGeometry,
    pane_count: usize,
    badge_text_width: f32,
    scale: f32,
) -> Option<[f32; 4]> {
    if tab.tier == TabWidthTier::Squeezed {
        return None;
    }
    let mark = (WINDOW_TAB_MARK_LOGICAL_PX * scale).round();
    let content_gap = WINDOW_TAB_GAP_LOGICAL_PX * scale;
    let left = tab_mark_left(tab, scale) + mark + content_gap;
    let right = tab_badge_rect(tab, pane_count, badge_text_width, scale)
        .map_or(tab_trailing_edge(tab, scale), |badge| {
            badge[0] - content_gap
        });
    (left < right).then_some([left, tab.body[1], right, tab.body[3]])
}

/// The strip's geometry when the caller has no trailers to hand and no use for
/// them: every tab resting and unpinned.
///
/// A trailer changes its own tab's furniture and nothing else — the run is shared
/// out equally whatever each tab hangs off its end — so the three questions that
/// are about the *run* rather than about one tab's controls are answered without
/// making every caller carry a list it would only fill with defaults. Pinned in
/// `a_trailer_moves_nothing_outside_its_own_tab`.
fn tab_strip_bodies(
    width: f32,
    scale: f32,
    tab_count: usize,
    active_tab: usize,
    scroll: f32,
) -> TabStripGeometry {
    tab_strip_geometry(
        width,
        scale,
        &vec![TabTrailer::default(); tab_count],
        active_tab,
        scroll,
    )
}

/// The scroll offset that brings tab `index` wholly inside the strip, moving as
/// little as it can — the mock-up's `scrollIntoView({ block: "nearest" })` habit,
/// which is also the only behaviour that does not yank the strip about when the
/// tab you asked for was already on screen.
///
/// The tab is measured with its skirt: an active tab paints
/// [`WINDOW_TAB_RADIUS_LOGICAL_PX`] of outward corner on each side, so bringing
/// only its body into view would still clip the very thing that marks it active.
#[must_use]
pub fn tab_scroll_to_reveal(
    width: f32,
    scale: f32,
    tab_count: usize,
    active_tab: usize,
    scroll: f32,
    index: usize,
) -> f32 {
    let geometry = tab_strip_bodies(width, scale, tab_count, active_tab, scroll);
    let Some(tab) = geometry.tabs.get(index) else {
        return scroll.clamp(0.0, geometry.max_scroll);
    };
    let skirt = (WINDOW_TAB_RADIUS_LOGICAL_PX * scale).round().max(1.0);
    let [left, right] = [tab.body[0] - skirt, tab.body[2] + skirt];
    let [view_left, view_right] = geometry.viewport;
    // Already framed: do not move. Overhanging one end: move by just the
    // overhang, and let the clamp below refuse anything the content cannot back.
    let scrolled = if left < view_left {
        scroll - (view_left - left)
    } else if right > view_right {
        scroll + (right - view_right)
    } else {
        scroll
    };
    scrolled.clamp(0.0, geometry.max_scroll)
}

/// How far past "half the neighbour is covered" a tab must travel before the two
/// trade places, as a fraction of half a tab (mock-up 6571-6576).
///
/// "Half is the floor stability allows; everything above it is dead zone bought
/// at the price of travel. 10% of half a tab ≈ 21px of dead zone on a 200px tab —
/// twenty times pointer noise, and invisible to the hand."
pub const TAB_REORDER_MARGIN: f32 = 0.1;

/// Where the strip's slots have their centres, in physical pixels.
///
/// The reorder judgement is made against *layout* and never against paint, which
/// is the whole of mock-up 6659-6662: "rects include transforms, and the
/// neighbours are mid-FLIP for 160ms after every swap, so rects would report
/// where a tab is flying rather than where its slot is". These are slot centres —
/// the `offsetLeft` half of that sentence — and they do not move when two tabs
/// trade places, because `tab_strip_geometry` gives every tab in the run the same
/// width.
#[must_use]
pub fn tab_slot_mids(geometry: &TabStripGeometry) -> Vec<f32> {
    geometry
        .tabs
        .iter()
        .map(|tab| (tab.body[0] + tab.body[2]) / 2.0)
        .collect()
}

/// One step of the reorder judgement: the neighbour the dragged tab has now
/// covered enough of to trade places with, or `None` while it has not.
///
/// **Tab against tab, never the pointer against a tab** (mock-up 6640-6658). The
/// pointer sits wherever you happened to take hold, so judging by it moves the
/// threshold with your grip and can send a tab past a neighbour it has not
/// visually reached. The test is the dragged tab's *leading edge* against the
/// neighbour's *centre*: they swap once it covers more than half of it.
///
/// The margin is the hysteresis, and half a slot is the floor beneath which
/// dithering becomes possible: a forward swap at `d = T` makes `d' = d - pitch`,
/// so a swap straight back needs `d < pitch - T`, which can only happen when
/// `T < pitch/2`.
#[must_use]
pub fn reorder_step(
    slot_mids: &[f32],
    cur: usize,
    visual_mid: f32,
    half_width: f32,
) -> Option<usize> {
    let margin = half_width * TAB_REORDER_MARGIN;
    if let Some(next) = slot_mids.get(cur + 1)
        && visual_mid + half_width > next + margin
    {
        return Some(cur + 1);
    }
    if let Some(prev) = cur.checked_sub(1).and_then(|prev| slot_mids.get(prev))
        && visual_mid - half_width < prev - margin
    {
        return cur.checked_sub(1);
    }
    None
}

/// The slot a tab dragged to `visual_mid` belongs in — the whole judgement, F57
/// included.
///
/// Loops one slot at a time so that flinging a tab across several slots in a
/// single pointer event still lands right; each pass moves it exactly one slot,
/// so the strip's own length bounds the walk.
///
/// **F57.** Pinned is a partition, not a decoration, and a reorder may not cross
/// it: without this the strip has two authorities on order — `normalize_pins`'
/// "pinned lead" invariant and whatever the drag last wrote — and they disagree
/// the moment you drag across the seam. Refusing the step leaves `tabs` the
/// single truth and stops the tab at the boundary, which is also what it looks
/// like it should do.
#[must_use]
pub fn reorder_target(
    slot_mids: &[f32],
    pinned: &[bool],
    cur: usize,
    visual_mid: f32,
    half_width: f32,
) -> usize {
    let mut cur = cur;
    for _ in 0..slot_mids.len() {
        let Some(to) = reorder_step(slot_mids, cur, visual_mid, half_width) else {
            break;
        };
        if pinned.get(to) != pinned.get(cur) {
            break;
        }
        cur = to;
    }
    cur
}

/// The slot a pointer at `pos` would insert into: the first entry whose centre it
/// has not yet passed, else the end of the run (mock-up 6467-6477).
///
/// Unused by the reorder above on purpose, and the two are not rivals: a reorder
/// moves a tab that is *already in* the strip and therefore judges tab against
/// tab, while this answers "where would a thing that is not in the strip yet go",
/// which is a question only the pointer can answer. It is stated here, as a pure
/// function of the same slot centres, because it is the public part of the
/// cross-boundary drop (K123-K125/K130/J107) and stating it once is what stops
/// the two readings of "which slot" from drifting apart. K124 is now its caller:
/// a pane arriving from the layout has no body in the strip to judge against, so
/// the pointer is the only operand there is.
#[must_use]
pub fn insert_index_at(slot_mids: &[f32], pos: f32) -> usize {
    slot_mids
        .iter()
        .position(|mid| pos < *mid)
        .unwrap_or(slot_mids.len())
}

/// The mock-up's two measured thresholds, read off the tab's own logical width.
fn tab_width_tier(tab_width: f32, scale: f32) -> TabWidthTier {
    let logical = tab_width / scale.max(f32::EPSILON);
    if logical < WINDOW_TAB_SQUEEZED_LOGICAL_PX {
        TabWidthTier::Squeezed
    } else if logical < WINDOW_TAB_TIGHT_LOGICAL_PX {
        TabWidthTier::Tight
    } else {
        TabWidthTier::Full
    }
}

/// Physical right edge of the app-owned tab run — everything left of it is the
/// app's, and everything right of it up to the caption run is window drag.
///
/// Under scroll the answer is the strip's own right edge rather than the `˅`'s:
/// a scrolling strip has no slack left in it, every pixel of the run is content,
/// and the `˅` that used to mark the end of the app's territory is now somewhere
/// off to the right of the viewport. Reporting the button's edge there would
/// hand the app's own tabs to the window's drag handler.
pub fn tab_strip_right_px(width: f32, scale: f32, tab_count: usize) -> i32 {
    // Neither the tiers nor the scroll offset move this answer: the tiers change
    // nothing outside a tab's own body, and a strip either scrolls or it does
    // not, whatever it currently shows.
    let geometry = tab_strip_bodies(width, scale, tab_count, 0, 0.0);
    if geometry.max_scroll > 0.0 {
        geometry.viewport[1].ceil() as i32
    } else {
        geometry.new_tab_menu[2].ceil() as i32
    }
}

pub fn hit_tab_chrome(
    width: f32,
    scale: f32,
    trailers: &[TabTrailer],
    active_tab: usize,
    scroll: f32,
    x: f64,
    y: f64,
) -> Option<ChromeTarget> {
    let (x, y) = (x as f32, y as f32);
    let geometry = tab_strip_geometry(width, scale, trailers, active_tab, scroll);
    // What is cropped away is not there to be clicked. Without this the run's
    // scrolled-out tail would still answer the pointer, under the caption
    // buttons drawn on top of it.
    if x < geometry.viewport[0] || x >= geometry.viewport[1] {
        return None;
    }
    for (index, tab) in geometry.tabs.iter().enumerate() {
        // Smallest target first, so the specific affordance wins over the
        // surface it lives on. The pin and the `×` never overlap — the pin's
        // right edge is 4px short of the `×`'s left — so the order *between* the
        // two is a statement of intent rather than a tie-break; what matters is
        // that the tab body, which contains both, is asked last.
        if tab.pin.is_some_and(|pin| contains(pin, x, y)) {
            return Some(ChromeTarget::TabPin(index));
        }
        if tab.close.is_some_and(|close| contains(close, x, y)) {
            return Some(ChromeTarget::TabClose(index));
        }
        if contains(tab.body, x, y) {
            return Some(ChromeTarget::Tab(index));
        }
    }
    if contains(geometry.new_tab, x, y) {
        return Some(ChromeTarget::NewTab);
    }
    contains(geometry.new_tab_menu, x, y).then_some(ChromeTarget::NewTabMenu)
}

/// Whether the pointer is over the tab strip's viewport — the question the wheel
/// asks before it scrolls the strip instead of the terminal.
///
/// It is the *strip's* box and not the title bar's: the caption buttons share
/// that bar and a notch over them is not the strip's to take.
#[must_use]
pub fn tab_strip_contains(width: f32, scale: f32, tab_count: usize, x: f64, y: f64) -> bool {
    let (x, y) = (x as f32, y as f32);
    let geometry = tab_strip_bodies(width, scale, tab_count, 0, 0.0);
    let title = (WINDOW_TITLE_BAR_LOGICAL_PX * scale).round();
    x >= geometry.viewport[0] && x < geometry.viewport[1] && y >= 0.0 && y < title
}

/// What the pointer is over, in device pixels of the window.
///
/// Order is significant and is a ruling, not an accident: the close affordance
/// sits inside a title bar which sits inside a seat, and a divider's hit zone
/// overhangs both of its neighbours. Smallest target first, so the specific
/// affordance wins over the general surface it lives on.
pub fn hit_chrome(
    seats: &Seats,
    layout: &SeatLayout,
    scale: f32,
    x: f64,
    y: f64,
) -> Option<ChromeTarget> {
    let (x, y) = (x as f32, y as f32);
    for placement in &layout.rects {
        let Some(device) = placement.device_rect else {
            continue;
        };
        let rect = [
            device.left as f32,
            device.top as f32,
            device.right as f32,
            device.bottom as f32,
        ];
        if matches!(placement.presentation, Presentation::Collapsed(_)) {
            if contains(rect, x, y) {
                return Some(ChromeTarget::CollapseBar(placement.id));
            }
            continue;
        }
        if seats.seat_wears_head(placement.kind) {
            // The border box, hairline included, rounded the way the drawing and
            // `pane_body_viewport` both round it: the header you can grab is
            // exactly the header you can see.
            let head = pane_head_geometry(rect, placement.kind, scale);
            // Smallest first inside the head too: the `×` is a dead zone in the
            // drag handle (C35), so it has to answer before the handle does.
            if head.close.is_some_and(|close| contains(close, x, y)) {
                return Some(ChromeTarget::PaneClose(placement.id));
            }
            if contains(head.head, x, y) {
                return Some(ChromeTarget::PaneHeader(placement.id));
            }
        }
    }
    for slot in seats.split_slots(layout) {
        if contains(hit_band(slot, scale), x, y) {
            return Some(ChromeTarget::Divider(slot.id));
        }
    }
    None
}

/// Hit-test the four application-owned boxes at the right edge of the title
/// bar. The remaining title area is deliberately absent: Win32 owns it through
/// `HTCAPTION`, not winit client input.
pub fn hit_window_chrome(width: f32, scale: f32, x: f64, y: f64) -> Option<ChromeTarget> {
    let (x, y) = (x as f32, y as f32);
    window_chrome_boxes(width, scale)
        .into_iter()
        .find(|(_, rect)| contains(*rect, x, y))
        .map(|(target, _)| target)
}

/// The four boxes of the caption run, in the order they stand: the gear, then
/// minimize, maximize and close.
///
/// The gear is one of them despite the mock-up giving it its own element outside
/// `.caption` (line 2243): it is the same 46px square in the same row, and the
/// run's arithmetic is simplest when it counts four.
///
/// One function rather than two, because [`hit_window_chrome`] now reads its
/// answer from here. A tooltip has to know where these buttons *are* and not
/// merely what is under the pointer, and a second copy of `width - 4 * button`
/// is exactly the kind of duplicate that stays right until someone adds a fifth
/// button.
#[must_use]
pub fn window_chrome_boxes(width: f32, scale: f32) -> [(ChromeTarget, [f32; 4]); 4] {
    let title = WINDOW_TITLE_BAR_LOGICAL_PX * scale;
    let button = WINDOW_CAPTION_BUTTON_LOGICAL_PX * scale;
    let run_left = (width - 4.0 * button).max(0.0);
    // The last button ends at the window's own edge rather than at
    // `run_left + 4 * button`, so a fractional scale cannot leave a seam of
    // unclaimed pixels in the corner where "close" is supposed to be.
    let edge = |index: f32| {
        if index == 4.0 {
            width
        } else {
            run_left + index * button
        }
    };
    [
        ChromeTarget::Settings,
        ChromeTarget::Minimize,
        ChromeTarget::Maximize,
        ChromeTarget::CloseWindow,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, target)| {
        let index = index as f32;
        (
            target,
            [edge(index).max(0.0), 0.0, edge(index + 1.0).max(0.0), title],
        )
    })
    .collect::<Vec<_>>()
    .try_into()
    .expect("four targets make four boxes")
}

/// Whether this device point lands inside the terminal seat's own rectangle.
pub fn terminal_contains(layout: &SeatLayout, terminal: SeatId, x: f64, y: f64) -> bool {
    let Some(Some(device)) = layout.get(terminal).map(|p| p.device_rect) else {
        return false;
    };
    let rect = [
        device.left as f32,
        device.top as f32,
        device.right as f32,
        device.bottom as f32,
    ];
    contains(rect, x as f32, y as f32)
}

fn contains(rect: [f32; 4], x: f32, y: f32) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// A box put on whole physical pixels, for the fills whose *edges* are the
/// point. A hit rectangle may sit on a subpixel — a pointer does not care — but
/// a rounded fill placed on one is resampled, and a resampled round is a soft
/// round: the corner the design asked for, blurred by half a pixel on two sides.
fn pixel_snapped(rect: [f32; 4]) -> [f32; 4] {
    [
        rect[0].round(),
        rect[1].round(),
        rect[2].round(),
        rect[3].round(),
    ]
}

/// Where everything inside one pane head stands, in physical pixels.
///
/// One function answers this for the hit test and for the drawing both, which is
/// D4 applied to chrome: the `×` you can press is the `×` you can see, and a
/// second copy of `right - 6px - 17px` is how those two come apart at the one
/// fractional scale nobody tested.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneHeadGeometry {
    /// The head's border box — 28 logical pixels including the hairline.
    pub head: [f32; 4],
    /// The content box the flex row centres in: the border box less its border.
    pub content_bottom: f32,
    /// The seat's own mark, at its per-kind size.
    pub mark: [f32; 4],
    /// What is left for the title once the mark and the trailing run have taken
    /// theirs. `.ptitle` is the flex child that gives, so this shrinks rather
    /// than pushing the controls off the end.
    pub title: [f32; 4],
    /// `.pane-close`'s 17px box, or `None` when the head is too narrow to seat
    /// it — at which point a press there must fall through to the head, exactly
    /// as a squeezed tab's does.
    pub close: Option<[f32; 4]>,
}

/// Lay out one pane head.
///
/// The trailing run is placed from the right edge inward, which is what
/// `margin-left: auto` does: the controls keep their boxes and the title is the
/// one that yields. The mock-up hangs three things off that end — `.pane-files`,
/// `.pane-float` and `.pane-close` — and only the `×` is this slice's; the other
/// two belong to the Files and floating-window blocks. They arrive by taking
/// their box off `trailing` before the `×` does, in the mock-up's own DOM order,
/// and nothing else here has to change.
pub fn pane_head_geometry(rect: [f32; 4], kind: SeatKind, scale: f32) -> PaneHeadGeometry {
    let bar = (SEAT_TITLE_BAR_LOGICAL_PX * scale).round();
    let edge = (SEAT_TITLE_EDGE_LOGICAL_PX * scale).max(1.0);
    let head_bottom = (rect[1] + bar).min(rect[3]);
    let content_bottom = (head_bottom - edge).max(rect[1]);
    let head = [rect[0], rect[1], rect[2], head_bottom];

    let pad = SEAT_TITLE_PADDING_LOGICAL_PX * scale;
    let (_, mark_logical_px, _) = pane_mark(kind, chrome_palette());
    let mark_size = (mark_logical_px * scale).round().max(1.0);
    let mark_left = (rect[0] + pad).round();
    let mark_top = (rect[1] + ((content_bottom - rect[1]) - mark_size) / 2.0).round();
    let mark = [
        mark_left,
        mark_top,
        mark_left + mark_size,
        mark_top + mark_size,
    ];

    // The trailing run, right to left from `padding-right: 6px`.
    let trailing_pad = SEAT_TITLE_TRAILING_PADDING_LOGICAL_PX * scale;
    let close_box = (SEAT_PANE_CLOSE_BOX_LOGICAL_PX * scale).round().max(1.0);
    let close_right = rect[2] - trailing_pad;
    let close_left = close_right - close_box;
    let close_top = (rect[1] + ((content_bottom - rect[1]) - close_box) / 2.0).round();
    // A control that would overlap the mark has nowhere to stand, and a control
    // half off its own head is worse than none: the press lands on a box whose
    // other half belongs to the pane next door.
    let close = (close_left > mark[2] && close_top >= rect[1])
        .then(|| pixel_snapped([close_left, close_top, close_right, close_top + close_box]));

    let title_right = match close {
        Some(close) => close[0] - SEAT_TITLE_GAP_LOGICAL_PX * scale,
        None => rect[2] - trailing_pad,
    };
    PaneHeadGeometry {
        head,
        content_bottom,
        mark,
        title: [
            mark[2] + SEAT_TITLE_GAP_LOGICAL_PX * scale,
            rect[1],
            title_right.max(mark[2]),
            content_bottom,
        ],
        close,
    }
}

/// Which pane the pointer is inside — `.pane:hover`, and nothing narrower.
///
/// The whole seat rectangle, head and body together, because that is what the
/// mock-up's `.pane` element is. A collapsed seat is not one: it is a bar with
/// no room for a control to be revealed in, and its own hover already has a
/// meaning.
#[must_use]
pub fn pane_at(layout: &SeatLayout, x: f64, y: f64) -> Option<SeatId> {
    let (x, y) = (x as f32, y as f32);
    layout
        .rects
        .iter()
        .filter(|placement| matches!(placement.presentation, Presentation::Full))
        .find_map(|placement| {
            let device = placement.device_rect?;
            contains(
                [
                    device.left as f32,
                    device.top as f32,
                    device.right as f32,
                    device.bottom as f32,
                ],
                x,
                y,
            )
            .then_some(placement.id)
        })
}

/// `OUTER_RIM = 48` (mock-up 7060), in logical pixels.
///
/// Kept here rather than in `bt-render`'s theme table on purpose: it is not a
/// dimension anything draws. It is the depth of a question the pointer is asked,
/// and the only thing that reads it is the answer below.
///
/// The mock-up argues the number twice over (7042-7059). Being generous costs a
/// pane's own edge zone nothing — a pane claims its outer 35%, around 190px on a
/// half-width pane, so a 48px rim pushes that zone inward rather than displacing
/// it and both gestures stay reachable. And 22px was rejected by name: a ribbon
/// two percent of the window wide asks for precision this gesture has not
/// earned.
const DROP_RIM_LOGICAL_PX: f32 = 48.0;

/// `zone = d4[near] < 0.35 ? near : "center"` (mock-up 7100).
///
/// A fraction of the pane rather than a distance, which is what makes "the outer
/// third splits" true of a 260px pane and a 1600px one alike.
const DROP_EDGE_FRACTION: f64 = 0.35;

/// Which side of a rectangle a drop is aiming at.
///
/// **Declaration order is load-bearing.** The mock-up picks the nearest side with
/// `Object.keys(d).reduce((a, b) => d[a] <= d[b] ? a : b)` over `{left, right,
/// top, bottom}` — `<=` keeps the accumulator, so an exact tie goes to the key
/// named *first*. That is the whole of the corner rule: at the corner of a
/// square pane all four distances are equal and the answer is `left`; along the
/// vertical centre line of a wide pane `top` beats `bottom`. Reordering these
/// four names silently re-decides every corner in the product, so
/// [`DropEdge::NEAREST_FIRST`] is the one place the order is written and every
/// tie-break reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropEdge {
    Left,
    Right,
    Top,
    Bottom,
}

impl DropEdge {
    /// The four sides in the order a tie is settled in — the mock-up's own key
    /// order, which is also the order distances are indexed in below.
    pub const NEAREST_FIRST: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    /// The axis a split on this side runs along: left/right divide a row,
    /// top/bottom divide a column (`zone→dir`, mock-up 3412-3427).
    ///
    /// Stated here with no caller yet, for the same reason
    /// [`insert_index_at`] was: this pair is `splitLeafIn`'s first two lines,
    /// and both of the things that will read it — U6 drawing which half of the
    /// target the arriving pane takes, U7 building `Edit::SplitSeat` and
    /// `Edit::RootRimDrop` — need the *same* answer. Two readings of "which
    /// side is left" is exactly the drift the mock-up's one-engine ruling
    /// (6352) is about, and a rule with a test on it does not drift.
    #[allow(dead_code)]
    #[must_use]
    pub fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Row,
            Self::Top | Self::Bottom => Axis::Col,
        }
    }

    /// Whether the arriving seat takes the *first* slot of the new split —
    /// `first = (zone === "left" || zone === "top")`. See [`Self::axis`] for why
    /// it is stated ahead of its caller.
    #[allow(dead_code)]
    #[must_use]
    pub fn leading(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

/// The nearest side, ties going to the side named first (see [`DropEdge`]).
///
/// The distances are indexed in [`DropEdge::NEAREST_FIRST`]'s order, so the
/// comparison here is exactly the mock-up's `reduce`: keep what you have unless
/// the next one is strictly smaller.
fn nearest_side(distances: [f64; 4]) -> DropEdge {
    let mut near = 0usize;
    for candidate in 1..4 {
        if distances[candidate] < distances[near] {
            near = candidate;
        }
    }
    DropEdge::NEAREST_FIRST[near]
}

/// What the pointer is aiming at inside the layout — K130 through K134, with
/// nothing about *what* is being dropped.
///
/// Split from the landing proper because these three are facts about a pointer
/// and a set of rectangles, and nothing else: they are the same whether a tab, a
/// pane or a file is in the hand. Who may drop what on them is the caller's
/// question, and keeping it there is what lets this be tested against solved
/// rectangles alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutAim {
    /// K130 — the layout's own rim: the root splits on this side.
    Rim(DropEdge),
    /// K134 — the outer 35% of one pane: that pane splits on this side.
    SeatEdge(SeatId, DropEdge),
    /// K134 — a pane's middle: its place is taken.
    SeatCentre(SeatId),
}

/// Where in the layout a pointer is aiming (K127, K128, K130-K134).
///
/// The order of the four questions below is not an implementation detail, and
/// the mock-up spends twenty lines saying so (7042-7059). **The rim is measured
/// before any pane is looked for.** Run the pane hit first and a pointer on the
/// rim at the height of a divider matches no pane, returns early, and never
/// reaches the rim at all — the gesture dies exactly along the seam of the
/// layout it exists to serve, which is how the feature was lost on its first
/// attempt.
///
/// * **K127 — above is not overshooting.** A pointer above the layout gets
///   nothing, because the strip lives up there and has already had its say.
/// * **K128 — the other three sides clamp rather than cancel.** Pushing past an
///   edge is how a hand aims at an edge, so the rim is 48px deep going in and
///   bottomless going out. Unclamped it would be a 48px target with a wall
///   behind it, and every overshoot a miss; widening the band would only move
///   the wall.
/// * **K131 — a lone pane skips the rim entirely**, because splitting the root
///   and splitting the only pane are the same act.
/// * **K132 — no pane under the pointer means nothing to aim at.** That is a
///   pointer on a divider, and a divider is not a target.
///
/// `host` is the layout's box in device pixels ([`device_viewport`]) and
/// `layout` carries the solved rectangles for this same frame. Both are passed
/// in rather than derived, so this function has no way of reading a rectangle
/// from a frame that is over (T228).
#[must_use]
pub fn aim_at_layout(
    layout: &SeatLayout,
    host: [f64; 4],
    pane_count: usize,
    scale: f32,
    x: f64,
    y: f64,
) -> Option<LayoutAim> {
    if y < host[1] {
        return None;
    }
    // The mock-up clamps to a *closed* box, because the DOM's rectangle
    // comparisons are closed on both sides (`cx <= r.right`). Every rectangle in
    // this build is half-open instead — a device column belongs to exactly one
    // seat, and `right` is the first column that is not inside — so "push it back
    // to the edge" has to mean the last column *in*, not the first column out.
    // Clamping to `host[2]` itself lands the pointer on the one column no seat
    // claims. Two panes hide that (the rim answers first at a distance of zero),
    // and a lone pane does not: it skips the rim entirely (K131), so a hand
    // pressed past its own right edge would find nothing at all.
    let cx = x.clamp(host[0], (host[2] - 1.0).max(host[0]));
    let cy = y.min((host[3] - 1.0).max(host[1]));
    let rim = [cx - host[0], host[2] - cx, cy - host[1], host[3] - cy];
    let near_rim = nearest_side(rim);
    let depth = f64::from(DROP_RIM_LOGICAL_PX * scale);
    if pane_count > 1 && rim[side_index(near_rim)] < depth {
        return Some(LayoutAim::Rim(near_rim));
    }
    let seat = pane_at(layout, cx, cy)?;
    let device = layout.get(seat)?.device_rect?;
    let (width, height) = (device.width() as f64, device.height() as f64);
    // A seat the solver has published is non-degenerate (red line L4), so the
    // normalisation below cannot divide by zero — `pane_at` only answered
    // because the pointer is inside a rectangle with room in it.
    let rx = (cx - device.left as f64) / width;
    let ry = (cy - device.top as f64) / height;
    let d4 = [rx, 1.0 - rx, ry, 1.0 - ry];
    let near = nearest_side(d4);
    Some(if d4[side_index(near)] < DROP_EDGE_FRACTION {
        LayoutAim::SeatEdge(seat, near)
    } else {
        LayoutAim::SeatCentre(seat)
    })
}

/// A side's index into the distance arrays [`aim_at_layout`] builds — the
/// inverse of [`DropEdge::NEAREST_FIRST`].
fn side_index(edge: DropEdge) -> usize {
    match edge {
        DropEdge::Left => 0,
        DropEdge::Right => 1,
        DropEdge::Top => 2,
        DropEdge::Bottom => 3,
    }
}

/// The strip's own rectangle, `[left, top, right, bottom]` in device pixels —
/// the mock-up's `stripEl().getBoundingClientRect()` (K123).
///
/// Horizontally it is the scroller's clip box, which stops where the caption run
/// begins; vertically it is the whole 40px title band, because that band is the
/// strip's element and a pointer anywhere in it is over the strip. Its bottom
/// edge is [`device_viewport`]'s top edge, so the two boxes partition the window
/// between them with neither a gap nor an overlap — which is what lets K123's
/// "is it in the strip" and K127's "is it above the layout" be asked in that
/// order without either of them having to know about the other.
#[must_use]
pub fn strip_band(geometry: &TabStripGeometry, scale: f32) -> [f32; 4] {
    [
        geometry.viewport[0],
        0.0,
        geometry.viewport[1],
        (WINDOW_TITLE_BAR_LOGICAL_PX * scale).round(),
    ]
}

/// Whether the pointer is over the strip (K123).
#[must_use]
pub fn in_strip(geometry: &TabStripGeometry, scale: f32, x: f64, y: f64) -> bool {
    contains(strip_band(geometry, scale), x as f32, y as f32)
}

/// A divider's hit zone: the drawn band widened to something a hand can land
/// on. One drawn pixel is not a target.
fn hit_band(slot: SplitSlot, scale: f32) -> [f32; 4] {
    let want = SEAT_DIVIDER_HIT_LOGICAL_PX * scale;
    let band = slot.band;
    match slot.dir {
        Axis::Row => {
            let grow = ((want - (band[2] - band[0])) / 2.0).max(0.0);
            [band[0] - grow, band[1], band[2] + grow, band[3]]
        }
        Axis::Col => {
            let grow = ((want - (band[3] - band[1])) / 2.0).max(0.0);
            [band[0], band[1] - grow, band[2], band[3] + grow]
        }
    }
}

/// The pointer state the chrome's colours depend on.
#[derive(Clone, Copy, Default, Debug)]
pub struct ChromePointer {
    pub hover: Option<ChromeTarget>,
    pub dragging: Option<SplitId>,
    /// `.pane:hover` — which pane the pointer is *inside*, whatever it is on.
    ///
    /// A separate channel from [`Self::hover`] and it has to be: the pane's tool
    /// buttons are revealed by the pointer being anywhere in the pane, and for
    /// most of that area — the terminal's own body — `hover` is `None`, because
    /// the terminal is not chrome. Deriving one from the other would mean the
    /// `×` appeared only while the pointer was already on the head, which is the
    /// "you have to know it is there to make it appear" bug C33 names.
    pub pane_hover: Option<SeatId>,
    /// `body.dragging` — something is being dragged *somewhere* (E53).
    ///
    /// Not the same question as [`Self::dragging`], which names the divider this
    /// gesture is moving. This one is about every other gesture: while one is in
    /// flight the dividers go silent, because a divider lighting up under the
    /// pointer is an offer that is not on the table, made in the very colour
    /// that during a drag means "let go and it lands here".
    pub other_drag_in_flight: bool,
}

/// Build every flat rectangle and label the chrome layer draws.
///
/// Empty for a lone terminal leaf: there is no divider, no other seat, and the
/// terminal's own pixels are not chrome. That emptiness is what makes the
/// byte-identity gate an argument about *values* rather than about a flag.
#[cfg(test)]
pub fn build_chrome(
    seats: &Seats,
    layout: &SeatLayout,
    scale: f32,
    pointer: ChromePointer,
) -> (Vec<ChromeQuad>, Vec<ChromeLabel>, Vec<ChromeSprite>) {
    build_chrome_with_preview(seats, layout, scale, pointer, None, None, None)
}

/// Build chrome while supplying the preview seat's content title and optional body placeholder.
#[cfg(test)]
pub fn build_chrome_with_preview(
    seats: &Seats,
    layout: &SeatLayout,
    scale: f32,
    pointer: ChromePointer,
    tab_title: Option<&str>,
    preview_title: Option<&str>,
    preview_message: Option<&str>,
) -> (Vec<ChromeQuad>, Vec<ChromeLabel>, Vec<ChromeSprite>) {
    let tabs = [TabContent {
        title: tab_title.unwrap_or("PowerShell").to_owned(),
        pane_count: seats.pane_count(),
        badge_text_width: 0.0,
        mark: TabMarkState::default(),
        trailer: TabTrailer::default(),
        offset: 0.0,
        landing: 0.0,
        edit: None,
    }];
    build_chrome_for_tabs(
        seats,
        layout,
        scale,
        pointer,
        ChromeContent {
            tabs: &tabs,
            active_tab: 0,
            grabbed: None,
            tab_scroll: 0.0,
            preview_title,
            terminal_cwd: None,
            preview_message,
            fit_overflow: None,
            profile_menu_open: false,
            chevron_turn: 0.0,
        },
    )
}

/// What one tab in the strip has to say for itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabContent {
    pub title: String,
    /// How many panes this tab holds. The badge appears above one and never at
    /// one — `paneBadge` (mock-up line 4189) prints nothing for a lone pane,
    /// and prints no placeholder either.
    pub pane_count: usize,
    /// The physical width of `pane_count`'s own digits at the badge's font size.
    ///
    /// Measured by the renderer and carried in, because it is the one thing
    /// about this strip that geometry cannot derive: only the font knows how
    /// wide a number is, and the mock-up sizes the pill from exactly that
    /// (`max(min-width, text + padding)`).
    pub badge_text_width: f32,
    /// What this tab's mark slot is saying about its sessions.
    pub mark: TabMarkState,
    /// What hangs off this tab's trailing end: its pin state, and how far the
    /// hover reveal has run. The caller owns both — one is a fact about the tab,
    /// the other is the clock.
    pub trailer: TabTrailer,
    /// How far this tab is *drawn* from the slot its index gives it, in physical
    /// pixels along the strip.
    ///
    /// Paint only: it moves nothing in layout, and every judgement the strip
    /// makes — which slot a tab is in, where the pointer's reorder threshold
    /// lies, what the `×` will close — is made against the unshifted geometry.
    /// One tab rides the pointer while its displaced neighbours run their FLIP
    /// home, and both are this one number sampled from two different clocks.
    pub offset: f32,
    /// How much of the landing wash this tab still wears, `0.0 ..= 1.0`
    /// (`@keyframes tab-land`, mock-up 955-968).
    ///
    /// Only the animation's `from` is a design value — an accent wash and an
    /// accent inset ring — because the keyframe writes only a `from`: "the
    /// animation ends at whatever the tab already is, so it needs no knowledge of
    /// the tab's real styling". `0.0` is that end, and is therefore also "no
    /// landing in flight".
    pub landing: f32,
    /// The open rename editor, when this is the tab being renamed.
    ///
    /// `Some` replaces [`Self::title`] in the title's own box and changes
    /// nothing else about the tab: the mark, the badge and the trailing controls
    /// stay exactly where they were. That is the whole of mock-up 376-378 — "the
    /// editor is the tab: same box, same metrics, so committing a name does not
    /// make the strip jump".
    pub edit: Option<TabEdit>,
}

/// The rename editor's contents, already measured.
///
/// Everything here is in physical pixels off the title box's left edge, because
/// the measuring has to happen where the font is (`main.rs`, exactly as
/// [`TabContent::badge_text_width`] does) and the placing has to happen where the
/// geometry is. This struct is the seam between the two.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabEdit {
    /// The draft from its first visible character on — a box narrower than its
    /// text scrolls by cutting the head off, and the label's own rect clips the
    /// tail. Empty means the placeholder shows through.
    pub text: String,
    /// The auto name an empty draft reveals: `input.placeholder = autoName(s)`
    /// (mock-up 5866). Not a hint and not a label — it is the layer *underneath*
    /// the override, shown so that clearing the box is a visible choice rather
    /// than a leap.
    pub placeholder: String,
    /// The caret's offset from the box's left edge.
    pub caret_px: f32,
    /// How much of the visible draft is selected, from the box's left edge —
    /// the one selection this editor has (`input.select()`, mock-up 5870). Zero
    /// when nothing is selected.
    pub selection_px: f32,
    /// Whether the caret is in its lit phase.
    pub caret_lit: bool,
}

/// One tab's mark slot, resolved to pixels-worth of decisions.
///
/// Deliberately free of any session type: `main.rs` reads the sessions and
/// decides *what is true*, this says *what to draw*, and `seats.rs` says
/// *where*. Keeping the three apart is what lets the strip's geometry be tested
/// without a terminal and the state taxonomy be tested without a window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabMarkState {
    /// The status dot's fill, or `None` when the tab has nothing to claim.
    pub dot: Option<[u8; 3]>,
    /// The progress ring that replaces the mark, or `None` to draw the mark.
    pub ring: Option<TabRing>,
    /// The mark's own opacity — the working breath, or a dead session's fade.
    pub opacity: f32,
    /// Whether the mark is drawn with its hue removed (a dead session).
    pub grayscale: bool,
}

impl Default for TabMarkState {
    /// A tab with nothing to report: its mark, at full strength, and no badge.
    ///
    /// Written out rather than derived, because a derived default would give
    /// the mark an opacity of `0.0` — an invisible mark on every tab that had
    /// not been told otherwise, which is the worst available reading of "this
    /// tab has no state to report".
    fn default() -> Self {
        Self {
            dot: None,
            ring: None,
            opacity: 1.0,
            grayscale: false,
        }
    }
}

/// The live arc of one tab's progress ring.
///
/// Only the arc: the track is a property of the tab it lies on, so the strip
/// picks that from the palette rather than being told it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabRing {
    pub arc: [u8; 3],
    pub start_milliturns: u16,
    pub sweep_milliturns: u16,
}

pub struct ChromeContent<'a> {
    pub tabs: &'a [TabContent],
    pub active_tab: usize,
    /// The tab currently riding the pointer, if any — `.tab.grabbed`.
    ///
    /// It buys exactly one thing: paint order. `.tab.grabbed { z-index: 20 }`
    /// against `.tab.active { z-index: 1 }` (mock-up 971 and 216), and a
    /// painter's-algorithm list has no `z-index`, so the tab in hand is laid down
    /// after the active tab rather than merely after its ordinary neighbours.
    pub grabbed: Option<usize>,
    /// How far the strip is scrolled, in physical pixels. Clamped by the
    /// geometry, so a stale value cannot draw a strip past its own content.
    pub tab_scroll: f32,
    pub preview_title: Option<&'a str>,
    /// The focused terminal session's own `cwd` — C28's `s.cwd`.
    ///
    /// It arrives from above rather than being read here for the reason
    /// `to_persisted` gives: a seat does not know its session. This window runs
    /// one shell to a tab, so one path describes every terminal leaf in it;
    /// when panes get their own children this becomes the per-leaf lookup and
    /// nothing else about the caption changes.
    pub terminal_cwd: Option<&'a str>,
    pub preview_message: Option<&'a str>,
    /// What the L4 fit-what-fits strip could not show, when the window is in
    /// that state at all ([`fit_what_fits`]). `None` on every ordinary solve.
    pub fit_overflow: Option<FitOverflow>,
    /// Whether the profile picker is up. The chevron states where its list is —
    /// down when it is folded away, up when it is already on screen — so the
    /// button has to be told, and the menu itself is drawn in the overlay layer.
    pub profile_menu_open: bool,
    /// How far through its turn the chevron is: 0.0 resting, 1.0 fully over.
    ///
    /// Separate from [`Self::profile_menu_open`] because the two do not move
    /// together and the mock-up does not ask them to. The menu is open or shut
    /// the instant it is clicked, and so is the button's ink — `.newtab` has no
    /// `transition` on `color` at all. Only `transform` has one, and for 140ms
    /// after the click the arrow is still on its way to where the menu already
    /// is. Deriving one from the other would either snap the arrow or stall the
    /// menu.
    ///
    /// A sampled number and not a clock, like every other animated value that
    /// reaches this module — see [`TabContent::offset`] and
    /// [`TabContent::landing`]. Nothing in here knows what time it is.
    pub chevron_turn: f32,
}

/// Build chrome for every runtime tab while the pane layer still follows the active tab's solve.
pub fn build_chrome_for_tabs(
    seats: &Seats,
    layout: &SeatLayout,
    scale: f32,
    pointer: ChromePointer,
    content: ChromeContent<'_>,
) -> (Vec<ChromeQuad>, Vec<ChromeLabel>, Vec<ChromeSprite>) {
    let ChromeContent {
        tabs,
        active_tab,
        grabbed,
        tab_scroll,
        preview_title,
        terminal_cwd,
        preview_message,
        fit_overflow,
        profile_menu_open,
        chevron_turn,
    } = content;
    let palette = chrome_palette();
    let mut quads = Vec::new();
    let mut labels = Vec::new();
    let mut sprites = Vec::new();
    let surface_width = layout
        .rects
        .iter()
        .filter_map(|placement| placement.device_rect)
        .map(|rect| rect.right as f32)
        .fold(1.0, f32::max);
    window_chrome(
        surface_width,
        scale,
        pointer.hover,
        TabStrip {
            tabs,
            active_tab,
            grabbed,
            scroll: tab_scroll,
            profile_menu_open,
            chevron_turn,
        },
        (&mut quads, &mut labels, &mut sprites),
    );
    for placement in &layout.rects {
        let Some(device) = placement.device_rect else {
            continue;
        };
        let rect = [
            device.left as f32,
            device.top as f32,
            device.right as f32,
            device.bottom as f32,
        ];
        match placement.presentation {
            Presentation::Collapsed(_) => {
                let hovered = pointer.hover == Some(ChromeTarget::CollapseBar(placement.id));
                quads.push(ChromeQuad {
                    rect,
                    color: if hovered {
                        palette.collapse_bar_hover
                    } else {
                        palette.collapse_bar
                    },
                });
                collapse_bar_contents(
                    rect,
                    scale,
                    placement.kind,
                    placement.presentation,
                    seat_short_caption(placement.kind, preview_title, terminal_cwd),
                    &mut labels,
                    &mut sprites,
                );
            }
            Presentation::Full => {
                if !seats.seat_wears_head(placement.kind) {
                    continue;
                }
                // `* { box-sizing: border-box }` (mock-up line 77) rules the
                // arithmetic here: `.panehead { height: 28px; border-bottom: 1px }`
                // is twenty-eight rows *including* the hairline, not twenty-eight
                // plus one. Rounded once, in `pane_head_geometry`, because
                // `pane_body_viewport` and the hit test round the same product
                // and the three must not round apart at a fractional scale.
                let head = pane_head_geometry(rect, placement.kind, scale);
                let head_bottom = head.head[3];
                let title_bottom = head.content_bottom;
                if placement.id != seats.terminal() {
                    quads.push(ChromeQuad {
                        rect: [rect[0], head_bottom, rect[2], rect[3]],
                        color: palette.seat_body,
                    });
                }
                quads.push(ChromeQuad {
                    rect: [rect[0], rect[1], rect[2], title_bottom],
                    color: palette.pane_head,
                });
                // The hairline that makes the bar a caption rather than a stripe.
                // It is the head's last row, so it stops where the body begins:
                // drawn below `head_bottom` it would paint over the terminal's
                // own first row, which is the bug this reading fixes.
                if title_bottom < head_bottom {
                    quads.push(ChromeQuad {
                        rect: [rect[0], title_bottom, rect[2], head_bottom],
                        color: palette.pane_head_edge,
                    });
                }
                // `.panehead { gap: 7px; padding: 0 6px 0 12px }` with the seat's
                // own mark leading: a terminal wears its profile square, a
                // preview the file mark, a files pane the folder — the marks the
                // mock-up puts in exactly these three heads.
                let pad = SEAT_TITLE_PADDING_LOGICAL_PX * scale;
                let focused = placement.id == seats.focus();
                let (mark, _, mark_color) = pane_mark(placement.kind, palette);
                sprites.push(ChromeSprite::new(mark, head.mark, mark_color).with_opacity(
                    // `.pane:not(.focused) .panehead .ticon { opacity: .5 }`
                    // (mock-up 1645-1647). Opacity and not a paler ink,
                    // because the mark is often the profile square, which
                    // carries colours of its own that no palette entry can
                    // stand in for — and because D38's whole argument is
                    // that focus must not move a hue. A channel of its own,
                    // so it cannot collide with what the accent or the
                    // breathing already say.
                    if focused {
                        1.0
                    } else {
                        PANE_MARK_UNFOCUSED_OPACITY
                    },
                ));
                labels.push(ChromeLabel {
                    text: seat_caption(placement.kind, preview_title, terminal_cwd).to_owned(),
                    rect: head.title,
                    font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
                    // `.pane.focused .panehead { color: var(--ink); font-weight: 500 }`
                    // (mock-up line 1644) — one declaration with two halves, and
                    // the mock-up's note beside it turns on having both: the
                    // focused pane is marked by *hierarchy* rather than by a fill,
                    // after tinting it with the accent was ruled out for colliding
                    // with the unread dot in the same row.
                    color: if focused {
                        palette.pane_title_focus
                    } else {
                        palette.pane_title
                    },
                    align_right: false,
                    align_center: false,
                    letter_spacing_em: 0.0,
                    weight: if focused {
                        ChromeLabelWeight::Medium
                    } else {
                        ChromeLabelWeight::Regular
                    },
                    tabular_numerals: false,
                });
                // `.panehead .pane-close { visibility: hidden }` with
                // `.pane:hover .pane-close { visibility: visible }` (mock-up
                // 1650-1657): the control is not there at all until the pointer
                // is in this pane, and then it is there at once.
                //
                // `visibility` and not `opacity` is the mock-up's own choice and
                // it is the reason nothing here fades: the `×` has no
                // `transition` of any kind, unlike `.pane-files` two rules above
                // it, which does. So there is no reduced-motion branch to write
                // — the control already behaves the way reduced motion would ask
                // it to.
                if pointer.pane_hover == Some(placement.id)
                    && let Some(close) = head.close
                {
                    let close_hovered =
                        pointer.hover == Some(ChromeTarget::PaneClose(placement.id));
                    if close_hovered {
                        // `.panehead .pane-close:hover { background: var(--active) }`
                        // at `border-radius: 4px`, over the one ground a pane
                        // head has.
                        sprites.push(ChromeSprite::new(
                            ChromeMark::ControlPill {
                                radius_px: (SEAT_PANE_CLOSE_RADIUS_LOGICAL_PX * scale)
                                    .round()
                                    .max(1.0) as u32,
                            },
                            close,
                            palette.pane_close_pill,
                        ));
                    }
                    let glyph = (SEAT_PANE_CLOSE_GLYPH_LOGICAL_PX * scale).round().max(1.0);
                    let glyph_left = ((close[0] + close[2] - glyph) / 2.0).round();
                    let glyph_top = ((close[1] + close[3] - glyph) / 2.0).round();
                    sprites.push(ChromeSprite::new(
                        ChromeMark::PaneClose,
                        [glyph_left, glyph_top, glyph_left + glyph, glyph_top + glyph],
                        // `color: var(--ink3)` at rest, `--ink` under the
                        // pointer — and under the pointer there is always the
                        // pill this pass has just drawn, never the bare head.
                        if close_hovered {
                            palette.pane_close_glyph_on_pill
                        } else {
                            palette.pane_close_glyph
                        },
                    ));
                }
                let body_notice = match placement.kind {
                    SeatKind::Preview => preview_message,
                    // T227: the degradation has to be *visible*. A leaf whose
                    // kind this build does not know keeps its place in the tree
                    // rather than taking the tree down with it (§2.1), but a
                    // silent placeholder and a silently destroyed pane look the
                    // same on screen — and the second is the thing the rule
                    // exists to forbid. So it says what it is, in its own body,
                    // in the same quiet ink an empty preview uses.
                    SeatKind::Placeholder => Some(PLACEHOLDER_SEAT_NOTICE),
                    _ => None,
                };
                if let Some(message) = body_notice {
                    // A state notice, not content: quiet ink, centred in the
                    // body, so an empty pane reads as an invitation and a
                    // failure reads as a note rather than a wall of alarm.
                    labels.push(ChromeLabel {
                        text: message.to_owned(),
                        rect: [
                            rect[0] + pad,
                            // Padded from the *body's* top, which is where the
                            // head's border box ends — not from its fill.
                            head_bottom + pad,
                            rect[2] - pad,
                            rect[3] - pad,
                        ],
                        font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
                        color: palette.body_hint_text,
                        align_right: false,
                        align_center: true,
                        letter_spacing_em: 0.0,
                        weight: ChromeLabelWeight::Regular,
                        tabular_numerals: false,
                    });
                }
            }
        }
    }
    if let Some(overflow) = fit_overflow {
        let row = [
            overflow.row.left as f32,
            overflow.row.top as f32,
            overflow.row.right as f32,
            overflow.row.bottom as f32,
        ];
        // The same ground the bars stand on, because it is the same strip and
        // the sentence is the strip's last line — not a banner laid over it.
        quads.push(ChromeQuad {
            rect: row,
            color: palette.collapse_bar,
        });
        let pad = SEAT_TITLE_PADDING_LOGICAL_PX * scale;
        labels.push(ChromeLabel {
            text: overflow_notice(overflow.hidden),
            rect: [row[0] + pad, row[1], row[2] - pad, row[3]],
            font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
            // Quiet ink and no mark: it is not a seat and must not be mistaken
            // for one, and it is a limit rather than an error (M147's reading of
            // the same distinction). Nothing to press — T214 rules the app does
            // not offer to resize a window the user is holding.
            color: palette.body_hint_text,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
        });
    }
    let slots = seats.split_slots(layout);
    // F63, and it is drawn before the dividers so the accent line and its grip
    // stay on top of the gap they are opening.
    if let Some(split) = pointer.dragging
        && let Some(slot) = slots.iter().find(|slot| slot.id == split)
    {
        resizing_cards(layout, *slot, scale, palette, &mut quads, &mut sprites);
    }
    for slot in &slots {
        let dragging = pointer.dragging == Some(slot.id);
        // E53: while any other gesture owns the pointer, a divider says nothing.
        // The offer is not on the table with the button already down, and it
        // would be made in the very colour that during a drag means "let go and
        // it lands here" — the one line under the pointer that means nothing at
        // all, impersonating the one thing that does.
        let lit =
            !pointer.other_drag_in_flight && pointer.hover == Some(ChromeTarget::Divider(slot.id));
        quads.push(ChromeQuad {
            rect: slot.band,
            color: if dragging {
                palette.divider_active
            } else if lit {
                palette.divider_hover
            } else {
                palette.divider
            },
        });
        // `.divider::after` (mock-up 1485-1492): a grip, so the boundary reads
        // as something you can grab. `opacity: 0` at rest and 1 on hover or
        // while dragging — so it is simply not drawn otherwise.
        if dragging || lit {
            sprites.push(ChromeSprite::new(
                ChromeMark::ControlPill {
                    radius_px: (SEAT_DIVIDER_GRIP_RADIUS_LOGICAL_PX * scale)
                        .round()
                        .max(1.0) as u32,
                },
                divider_grip(*slot, scale),
                palette.divider_active,
            ));
        }
    }
    (quads, labels, sprites)
}

/// `.divider::after` in physical pixels: 3 logical pixels across the band, 28
/// along it, centred on both.
///
/// Centred on the *band* rather than placed at the mock-up's `left: 2px`, which
/// is the same thing said in the coordinate system we have. The mock-up's grip
/// spans 2..5 inside a 7px box whose hairline spans 3..4 — one pixel of grip on
/// each side of the line. Here the band is the hairline itself, wherever the
/// solver's boundary snapping put it, so the grip grows symmetrically out of it
/// and lands on the same pixels at every scale instead of inheriting a 3 that
/// only means "centred" at 1x.
fn divider_grip(slot: SplitSlot, scale: f32) -> [f32; 4] {
    let thickness = (SEAT_DIVIDER_GRIP_THICKNESS_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    let length = (SEAT_DIVIDER_GRIP_LENGTH_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    let band = slot.band;
    match slot.dir {
        Axis::Row => {
            let centre_x = (band[0] + band[2]) / 2.0;
            let centre_y = (band[1] + band[3]) / 2.0;
            // A grip longer than the boundary it sits on would poke out of both
            // ends of it, which is a shape the mock-up's `overflow` never lets
            // happen.
            let length = length.min(band[3] - band[1]);
            pixel_snapped([
                centre_x - thickness / 2.0,
                centre_y - length / 2.0,
                centre_x + thickness / 2.0,
                centre_y + length / 2.0,
            ])
        }
        Axis::Col => {
            let centre_x = (band[0] + band[2]) / 2.0;
            let centre_y = (band[1] + band[3]) / 2.0;
            let length = length.min(band[2] - band[0]);
            pixel_snapped([
                centre_x - length / 2.0,
                centre_y - thickness / 2.0,
                centre_x + length / 2.0,
                centre_y + thickness / 2.0,
            ])
        }
    }
}

/// F63: the two panes a divider drag is resizing pull in from their own edges
/// into slightly smaller rounded cards, and the `--panel` floor shows through.
///
/// The mock-up's own note (1458-1463) is what decides the shape: *the gap is the
/// consequence of the panes getting smaller, not a seam that widened*. So it
/// appears on all four sides of each pane, not only along the boundary being
/// moved — what you read is "these two are being resized", not "the seam moved".
///
/// Drawn as four rectangles and four corners rather than as one frame-shaped
/// mark, for the reason [`ChromeMark::CardCorner`] gives: this shape exists only
/// while the pane's size is changing on every frame, and a size-keyed raster
/// would miss its cache on every one of them.
///
/// What is *not* here is `box-shadow: 0 2px 12px`. A blurred rounded rectangle
/// is size-keyed the same way the frame would be, and it cannot be cut into
/// fixed corners the way a hard edge can — its straight sections have to stretch,
/// and the sprite pipeline blits a raster at its own size rather than scaling
/// one. Recorded rather than approximated: a hand-drawn falloff would be a
/// different shadow wearing this one's name.
fn resizing_cards(
    layout: &SeatLayout,
    slot: SplitSlot,
    scale: f32,
    palette: bt_render::ChromePalette,
    quads: &mut Vec<ChromeQuad>,
    sprites: &mut Vec<ChromeSprite>,
) {
    let margin = (SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    let radius = (SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    for placement in &layout.rects {
        let Some(device) = placement.device_rect else {
            continue;
        };
        if !matches!(placement.presentation, Presentation::Full) {
            continue;
        }
        let rect = [
            device.left as f32,
            device.top as f32,
            device.right as f32,
            device.bottom as f32,
        ];
        // `.slot.resizing` is the two slots of *this* split and no others, at
        // any depth: a pane inside the leading slot is inside the slot being
        // resized. Membership is therefore the slot's own rectangle, which is
        // the same rectangle `split_slots` read the band out of.
        if !slot_contains(slot.slot, layout, placement.id) {
            continue;
        }
        let card = [
            rect[0] + margin,
            rect[1] + margin,
            rect[2] - margin,
            rect[3] - margin,
        ];
        if card[2] - card[0] < 2.0 * radius || card[3] - card[1] < 2.0 * radius {
            // Too small to have both of its rounds: drawing the frame anyway
            // would eat the pane rather than inset it.
            continue;
        }
        let floor = palette.termhost;
        for band in [
            [rect[0], rect[1], rect[2], card[1]],
            [rect[0], card[3], rect[2], rect[3]],
            [rect[0], card[1], card[0], card[3]],
            [card[2], card[1], rect[2], card[3]],
        ] {
            quads.push(ChromeQuad {
                rect: band,
                color: floor,
            });
        }
        for (corner, at) in [
            (Corner::TopLeft, [card[0], card[1]]),
            (Corner::TopRight, [card[2] - radius, card[1]]),
            (Corner::BottomLeft, [card[0], card[3] - radius]),
            (Corner::BottomRight, [card[2] - radius, card[3] - radius]),
        ] {
            sprites.push(ChromeSprite::new(
                ChromeMark::CardCorner {
                    radius_px: radius as u32,
                    corner,
                },
                [at[0], at[1], at[0] + radius, at[1] + radius],
                floor,
            ));
        }
    }
}

/// Whether this seat's rectangle lies inside the slot a divider drag is moving.
///
/// Geometry rather than a tree walk, and deliberately: `SplitSlot::slot` is read
/// off the solved rectangles, so asking "is this seat in there" in the same
/// coordinates keeps the card and the band answering to one measurement (D4).
fn slot_contains(slot: LogicalRect, layout: &SeatLayout, seat: SeatId) -> bool {
    let Some(Some(rect)) = layout.get(seat).map(|placement| placement.rect) else {
        return false;
    };
    rect.left >= slot.left
        && rect.right <= slot.right
        && rect.top >= slot.top
        && rect.bottom <= slot.bottom
}

/// Everything the tab strip needs to know about itself: what it holds, which of
/// them is active, how far it is scrolled, and whether its `˅` is open.
struct TabStrip<'a> {
    tabs: &'a [TabContent],
    active_tab: usize,
    grabbed: Option<usize>,
    scroll: f32,
    profile_menu_open: bool,
    chevron_turn: f32,
}

fn window_chrome(
    width: f32,
    scale: f32,
    hover: Option<ChromeTarget>,
    strip: TabStrip<'_>,
    output: (
        &mut Vec<ChromeQuad>,
        &mut Vec<ChromeLabel>,
        &mut Vec<ChromeSprite>,
    ),
) {
    let TabStrip {
        tabs,
        active_tab,
        grabbed,
        scroll: tab_scroll,
        profile_menu_open,
        chevron_turn,
    } = strip;
    let (quads, labels, sprites) = output;
    let palette = chrome_palette();
    let title = (WINDOW_TITLE_BAR_LOGICAL_PX * scale).round();
    // `.titlebar` in the mock-up carries a background and nothing else — no
    // border, no rule, no hairline. What separates it from the content below is
    // the tonal step from `--panel` to `--termbg`, and in the tab's own span
    // there is deliberately no step at all: the tab *is* `--termbg`, so a line
    // drawn across the bar's foot would be the one thing that severs the tab
    // from the terminal it is shaped to join.
    quads.push(ChromeQuad {
        rect: [0.0, 0.0, width, title],
        color: palette.title_bar,
    });

    let radius = (WINDOW_TAB_RADIUS_LOGICAL_PX * scale).round().max(1.0);
    let button = WINDOW_CAPTION_BUTTON_LOGICAL_PX * scale;
    let run_left = (width - 4.0 * button).max(0.0);
    let trailers = tabs.iter().map(|tab| tab.trailer).collect::<Vec<_>>();
    let geometry = tab_strip_geometry(width, scale, &trailers, active_tab, tab_scroll);
    let viewport = geometry.viewport;
    // `.tabs-inline` crops its content, and a label is the one chrome primitive
    // that can be cropped exactly: `ChromeLabel`'s rect is also its clip box, and
    // the text renderer clips it per glyph and per pixel. Only the right edge
    // needs pulling in — the left edge of the strip is the surface's own, so a
    // glyph that runs off it is clipped by the framebuffer for free.
    let clip_label = |rect: [f32; 4]| [rect[0], rect[1], rect[2].min(viewport[1]), rect[3]];
    // `.tab.active { z-index: 1 }` (mock-up line 216) — the active tab stands
    // above its neighbours, corners and all. A painter's-algorithm list has no
    // z-index, so the strip is laid down in two passes: every other tab first,
    // the active one last.
    //
    // What rides on this is the join. The active tab's two concave corners
    // (`::before`/`::after`, one `--tabr` box outside each edge) overhang its
    // neighbours, and they are what curves the tab into the content plane. A
    // neighbour painted afterwards fills its whole box — and a box has square
    // corners — so its `--hover` fill squared the arc off and the tab met the
    // terminal at a notch. The mock-up records the same bug in its own medium at
    // lines 178-180, where DOM order plays the part paint order plays here.
    //
    // The tab in hand is a third layer above both (`.tab.grabbed { z-index: 20 }`,
    // line 971): while it is being dragged it passes *over* its neighbours, and
    // the active tab is one of them — usually the same tab, since starting a
    // reorder commits the activation (J106), but the ordering is stated rather
    // than assumed.
    let layer_of = |index: usize| -> u8 {
        if grabbed == Some(index) {
            2
        } else if index == active_tab {
            1
        } else {
            0
        }
    };
    for layer in 0..=2u8 {
        for (index, (slot, content)) in geometry.tabs.iter().zip(tabs).enumerate() {
            if layer_of(index) != layer {
                continue;
            }
            let active = index == active_tab;
            // Everything below draws the tab where the drag has *put* it, while
            // every rectangle the strip reasons about stays in the slot the
            // index gives it. One shift, at the top, so no box inside the tab
            // can be left behind by it.
            let shifted;
            let tab = if content.offset == 0.0 {
                slot
            } else {
                shifted = slot.shifted(content.offset);
                &shifted
            };
            let [tab_left, tab_top, tab_right, tab_bottom] = tab.body;
            // `.tab:hover` is one hover: a pointer on the `×` — or on the pin
            // beside it — is still a pointer on the tab, so the body lights up
            // and the title steps to `--ink` for all three.
            let tab_hovered = hover == Some(ChromeTarget::Tab(index))
                || hover == Some(ChromeTarget::TabClose(index))
                || hover == Some(ChromeTarget::TabPin(index));
            let skirted = [tab_left - radius, tab_top, tab_right + radius, tab_bottom];
            if active && tab_right - tab_left >= 2.0 * radius && within_strip(viewport, skirted) {
                sprites.push(ChromeSprite::new(
                    ChromeMark::ActiveTab {
                        radius_px: radius as u32,
                    },
                    skirted,
                    palette.active_tab,
                ));
            } else if tab_hovered && within_strip(viewport, tab.body) {
                sprites.push(ChromeSprite::new(
                    ChromeMark::TabBody {
                        radius_px: radius as u32,
                    },
                    tab.body,
                    palette.caption_hover,
                ));
            }
            // `@keyframes tab-land` — the wash and the ring the landing tab
            // arrives wearing, on their way to nothing. Both are the accent at a
            // stated alpha, so both ride as `opacity` on the accent rather than
            // as pre-composited palette entries: the alpha is a function of the
            // clock here, and a constant cannot be one.
            //
            // They go down after the tab's own silhouette and before everything
            // inside it, which is where a `background` and an inset `box-shadow`
            // sit in CSS — over the surface, under the content.
            if content.landing > 0.0 && within_strip(viewport, tab.body) {
                let mut wash = ChromeSprite::new(
                    ChromeMark::TabBody {
                        radius_px: radius as u32,
                    },
                    tab.body,
                    palette.accent,
                );
                wash.opacity = TAB_LAND_WASH_ALPHA * content.landing;
                sprites.push(wash);
                let mut ring = ChromeSprite::new(
                    ChromeMark::TabBodyRing {
                        radius_px: radius as u32,
                        stroke_px: (TAB_LAND_RING_LOGICAL_PX * scale).round().max(1.0) as u32,
                    },
                    tab.body,
                    palette.accent,
                );
                ring.opacity = TAB_LAND_RING_ALPHA * content.landing;
                sprites.push(ring);
            }
            let mark = (WINDOW_TAB_MARK_LOGICAL_PX * scale).round();
            let content_gap = WINDOW_TAB_GAP_LOGICAL_PX * scale;
            // `.tab.squeezed { justify-content: center; padding: 0 4px }` — the mark
            // and whatever else survived are centred as one group, not indented.
            let mark_rect = tab_mark_box(tab, scale);
            let [mark_left, ..] = mark_rect;
            // The tab row's trailing boundary — the pin, the `×`, or the trailing
            // padding, whichever the cluster leads with. One function with
            // `tab_badge_rect`, because the two measure the same edge.
            let trailing = tab_trailing_edge(tab, scale);
            // `.panecount` is `flex: none` and stands between the title and the `×`,
            // so it takes its width off the trailing end and the title — `flex: 1`,
            // `min-width: 0` — keeps whatever is left.
            let badge = tab_badge_rect(tab, content.pane_count, content.badge_text_width, scale);
            // What the title may not run past.
            let content_right = badge.map_or(trailing, |badge| badge[0] - content_gap);
            if mark_left + mark <= tab_trailer_box(tab).map_or(tab_right, |trailer| trailer[0]) {
                if within_strip(viewport, mark_rect) {
                    // The mark slot is `.ticon-wrap` (mock-up line 238): a
                    // positioning origin whose contents are absolutely placed,
                    // so nothing here can move the tab's layout by a pixel.
                    // That is why the ring may replace the mark and the dot may
                    // overhang it without either one touching the title.
                    match content.mark.ring {
                        // The ring *replaces* the mark in the same box (user
                        // ruling, deviating from `.pring`, which overlays a
                        // larger circle around it). Chrome's own loading
                        // indicator does exactly this to a favicon: while there
                        // is progress to report, the progress is what the slot
                        // is for.
                        Some(ring) => {
                            let stroke =
                                (WINDOW_TAB_RING_STROKE_LOGICAL_PX * scale).round().max(1.0) as u32;
                            // The track first — a full turn under the arc, in
                            // `--border` at .7 over whichever surface this tab
                            // is wearing.
                            sprites.push(ChromeSprite::new(
                                ChromeMark::ProgressRing {
                                    start_milliturns: 0,
                                    sweep_milliturns: 1000,
                                    stroke_px: stroke,
                                },
                                mark_rect,
                                if active {
                                    palette.ring_track_on_active_tab
                                } else if tab_hovered {
                                    palette.ring_track_on_hovered_tab
                                } else {
                                    palette.ring_track_on_resting_tab
                                },
                            ));
                            sprites.push(ChromeSprite::new(
                                ChromeMark::ProgressRing {
                                    start_milliturns: ring.start_milliturns,
                                    sweep_milliturns: ring.sweep_milliturns,
                                    stroke_px: stroke,
                                },
                                mark_rect,
                                ring.arc,
                            ));
                        }
                        None => {
                            let mut profile = ChromeSprite::new(
                                ChromeMark::ProfilePowerShell,
                                mark_rect,
                                palette.accent,
                            );
                            // `.ticon.working`'s breath and `.ticon-wrap.dead`'s
                            // fade both land here, on the mark alone — never on
                            // the dot or the ring, which are other claims.
                            profile.opacity = content.mark.opacity;
                            profile.grayscale = content.mark.grayscale;
                            sprites.push(profile);
                        }
                    }
                }
                // `.unreaddot { position: absolute; top: -2px; right: -4px }` —
                // anchored to the slot's top-right and deliberately overhanging
                // it on both axes, so it reads as a badge *on* the mark rather
                // than as a thing standing beside it. It survives every squeeze
                // tier for the same reason it takes no layout space: the
                // stylesheet hides `.ttitle`, `.panecount`, `.pin` and the `×`
                // as tabs narrow (lines 197-202) and never touches
                // `.ticon-wrap`.
                if let Some(dot_color) = content.mark.dot {
                    let dot = (WINDOW_TAB_STATUS_DOT_LOGICAL_PX * scale).round().max(1.0);
                    let dot_left =
                        (mark_rect[2] - WINDOW_TAB_STATUS_DOT_RIGHT_LOGICAL_PX * scale - dot)
                            .round();
                    let dot_top =
                        (mark_rect[1] + WINDOW_TAB_STATUS_DOT_TOP_LOGICAL_PX * scale).round();
                    let dot_rect = [dot_left, dot_top, dot_left + dot, dot_top + dot];
                    if within_strip(viewport, dot_rect) {
                        sprites.push(ChromeSprite::new(
                            // `border-radius: 50%` on a square is a circle, and
                            // `ControlPill` clamps its round to half the short
                            // side — so the dot is the pill the chrome already
                            // has, not a second circle to keep in step.
                            ChromeMark::ControlPill {
                                radius_px: (dot / 2.0).round().max(1.0) as u32,
                            },
                            dot_rect,
                            dot_color,
                        ));
                    }
                }
                let label_left = mark_left + mark + content_gap;
                // `.tab.squeezed .ttitle { display: none }`: below 90px the tab is
                // its mark, and nothing is gained by clipping a word to two letters.
                if tab.tier != TabWidthTier::Squeezed
                    && label_left < content_right
                    && label_left < viewport[1]
                {
                    let title_box = clip_label([label_left, tab_top, content_right, tab_bottom]);
                    // The editor is the tab. `.rename` (mock-up 379-385) declares
                    // `background: transparent; font: inherit; padding: 0` and
                    // `flex: 1 1 auto` — every one of which says the same thing:
                    // it takes the title's box and its metrics and changes only
                    // the ink and what is written in it. So the label below is
                    // the *same* label with a different `text` and `color`, and
                    // the strip cannot jump when the editor opens or closes.
                    let (text, color) = match &content.edit {
                        // An empty draft shows the layer underneath, in the ink
                        // the mock-up gives it: `.rename::placeholder { color:
                        // var(--ink3) }` (385). `--ink3` over the terminal
                        // surface this tab wears is `pane_title` — the same ink
                        // an unfocused pane head takes over the same canvas.
                        Some(edit) if edit.text.is_empty() => {
                            (edit.placeholder.clone(), palette.pane_title)
                        }
                        // `.rename { color: var(--ink) }` (382). The editing tab
                        // is always the active one — the first click of the
                        // double click put it there — so `--ink` over its
                        // surface is `pane_title_focus`, which is what this tab's
                        // title was already wearing. The editor changes the ink
                        // by *not* changing it.
                        Some(edit) => (edit.text.clone(), palette.pane_title_focus),
                        None => (
                            content.title.clone(),
                            if active || tab_hovered {
                                palette.pane_title_focus
                            } else {
                                palette.title_text
                            },
                        ),
                    };
                    // The selection goes down before the text and after the tab's
                    // own silhouette, which is why it is a sprite and not a
                    // `ChromeQuad`: quads are drawn *under* every mark, and the
                    // active tab's body is a mark. `input.select()` (5870) is the
                    // only selection this editor has, and drawing it is what
                    // makes "type and the old name is gone" a thing you can see
                    // coming rather than a surprise.
                    if let Some(edit) = &content.edit
                        && edit.selection_px > 0.0
                    {
                        let band = [
                            title_box[0],
                            (tab_top + (tab_bottom - tab_top - mark) / 2.0).round(),
                            (title_box[0] + edit.selection_px).min(title_box[2]),
                            (tab_top + (tab_bottom - tab_top + mark) / 2.0).round(),
                        ];
                        if band[2] > band[0] && within_strip(viewport, band) {
                            sprites.push(ChromeSprite::new(
                                ChromeMark::Fill,
                                band,
                                // `--active`, already composited over this tab's
                                // own surface. The mock-up declares no
                                // `::selection` and leans on the browser's; what
                                // it *does* have is one neutral wash meaning
                                // "this is the chosen thing", and it is this one.
                                palette.tab_close_pill_on_content,
                            ));
                        }
                    }
                    labels.push(ChromeLabel {
                        text,
                        rect: title_box,
                        font_size_px: WINDOW_TAB_FONT_LOGICAL_PX * scale,
                        color,
                        align_right: false,
                        align_center: false,
                        letter_spacing_em: 0.0,
                        weight: ChromeLabelWeight::Regular,
                        tabular_numerals: false,
                    });
                    // The caret last: it is the one thing in the box that has to
                    // be visible over the letters as well as over the fill.
                    if let Some(edit) = &content.edit
                        && edit.caret_lit
                    {
                        // The terminal's own hairline (`CURSOR_BAR_WIDTH_LOGICAL_PX`),
                        // because an insertion point is an insertion point: two
                        // carets in one window that disagree about their width
                        // read as two different applications.
                        let width = (TAB_RENAME_CARET_LOGICAL_PX * scale).round().max(1.0);
                        let left = (title_box[0] + edit.caret_px).round();
                        // The tab's own content band — the 15px the mark beside
                        // it occupies — so the caret is exactly as tall as the
                        // row it stands in and sits on the same axis everything
                        // else in the tab is centred on.
                        let caret = [
                            left,
                            (tab_top + (tab_bottom - tab_top - mark) / 2.0).round(),
                            left + width,
                            (tab_top + (tab_bottom - tab_top + mark) / 2.0).round(),
                        ];
                        if caret[0] >= title_box[0]
                            && caret[2] <= title_box[2]
                            && within_strip(viewport, caret)
                        {
                            sprites.push(ChromeSprite::new(
                                ChromeMark::Fill,
                                caret,
                                palette.pane_title_focus,
                            ));
                        }
                    }
                }
            }
            // `.panecount` — the count, its pill, and nothing when there is one pane.
            if let Some(badge) = badge
                && within_strip(viewport, badge)
            {
                sprites.push(ChromeSprite::new(
                    ChromeMark::ControlPill {
                        radius_px: (WINDOW_TAB_BADGE_RADIUS_LOGICAL_PX * scale)
                            .round()
                            .max(1.0) as u32,
                    },
                    pixel_snapped(badge),
                    // `background: var(--active)` on every tab — the same fill the
                    // `×`'s pill wears, over whichever of the three surfaces this tab
                    // is showing.
                    if active {
                        palette.tab_close_pill_on_content
                    } else if tab_hovered {
                        palette.tab_close_pill_on_hovered_tab
                    } else {
                        palette.tab_badge_on_resting_tab
                    },
                ));
                labels.push(ChromeLabel {
                    text: content.pane_count.to_string(),
                    // `justify-content: center` — the number is centred in its pill,
                    // which is what makes `min-width` a floor and not an indent.
                    rect: badge,
                    font_size_px: WINDOW_TAB_BADGE_FONT_LOGICAL_PX * scale,
                    // `--ink2`, rising to `--ink` on the active tab and deliberately
                    // never to the accent (mock-up line 297).
                    color: if active {
                        palette.tab_badge_text_on_active_tab
                    } else if tab_hovered {
                        palette.tab_badge_text_on_hovered_tab
                    } else {
                        palette.tab_badge_text_on_resting_tab
                    },
                    align_right: false,
                    align_center: true,
                    letter_spacing_em: 0.0,
                    // `.panecount { font-weight: 600 }` (mock-up line 296). The
                    // badge is the one label in the chrome that is not prose:
                    // at the regular weight a 10px digit inside a filled pill
                    // read as a smudge rather than as a count.
                    weight: ChromeLabelWeight::SemiBold,
                    // `font-variant-numeric: tabular-nums` (line 302) — the
                    // number is centred in a box that does not move, so its
                    // figures must not either.
                    tabular_numerals: true,
                });
            }
            // ── the pin, in the `×`'s own slot ──
            if let Some(pin) = tab.pin {
                let pinned = tab.trailer.pinned;
                let pin_hovered = hover == Some(ChromeTarget::TabPin(index));
                // `transition: … opacity .12s ease` (mock-up 341) — an unpinned
                // pin fades in as it widens. A pinned one never fades: it is a
                // fact about the tab, not an offer that comes and goes.
                let opacity = if pinned { 1.0 } else { tab.trailer.reveal };
                if pin_hovered && within_strip(viewport, pin) {
                    // `.tab .pin:hover { background: var(--active) }` — the `×`'s
                    // own pill, because it is the `×`'s own slot: same box, same
                    // 4px of round, same two pre-composited surfaces.
                    let mut pill = ChromeSprite::new(
                        ChromeMark::ControlPill {
                            radius_px: (WINDOW_TAB_PIN_RADIUS_LOGICAL_PX * scale).round().max(1.0)
                                as u32,
                        },
                        pixel_snapped(pin),
                        if active {
                            palette.tab_close_pill_on_content
                        } else {
                            palette.tab_close_pill_on_hovered_tab
                        },
                    );
                    pill.opacity = opacity;
                    sprites.push(pill);
                }
                let pin_glyph = (WINDOW_TAB_PIN_GLYPH_LOGICAL_PX * scale).round().max(1.0);
                let pin_glyph_left = ((pin[0] + pin[2] - pin_glyph) / 2.0).round();
                let pin_glyph_top = ((pin[1] + pin[3] - pin_glyph) / 2.0).round();
                let pin_glyph_rect = [
                    pin_glyph_left,
                    pin_glyph_top,
                    pin_glyph_left + pin_glyph,
                    pin_glyph_top + pin_glyph,
                ];
                // `.tab .pin { overflow: hidden }` while the box is opening. A
                // chrome mark is rasterised into the box it fills and drawn with
                // whole-texture UVs, so a half-open box cannot crop one — the
                // same ruling `within_strip` records. What it can do is wait: the
                // glyph arrives once the box can hold it, inside the fade that is
                // already running, rather than spilling 12px over the title.
                if pin[2] - pin[0] >= pin_glyph && within_strip(viewport, pin_glyph_rect) {
                    let mut glyph = ChromeSprite::new(
                        // Fluent 2's fill axis: regular is the action ("you could
                        // pin this"), filled is the state ("it is pinned").
                        ChromeMark::Pin { filled: pinned },
                        pin_glyph_rect,
                        // Same slot as the `×`, same two declarations, same duty
                        // to arrive already mixed over the ground beneath it —
                        // with one ground the `×` never has to answer for. The
                        // `×` reaches `--ink` only under the pointer, where its
                        // pill is always down; a *pinned* pin reaches `--ink` as
                        // a state, standing on the bare tab.
                        if pin_hovered {
                            // `.pin:hover` — `var(--ink)` over the pill this pass
                            // has just drawn, which is the `×`'s own lit ink.
                            if active {
                                palette.tab_close_glyph_on_pill_over_active_tab
                            } else {
                                palette.tab_close_glyph_on_pill_over_hovered_tab
                            }
                        } else if pinned {
                            // `.pin.on` — `var(--ink)` with nothing under it but
                            // the tab. "The state is darker than the action: one
                            // is a fact about this tab, the other is an offer that
                            // only exists while you are hovering it."
                            if active {
                                palette.tab_pin_state_on_active_tab
                            } else if tab_hovered {
                                palette.tab_pin_state_on_hovered_tab
                            } else {
                                palette.title_text_hover
                            }
                        } else if active {
                            // `.tab .pin { color: var(--ink3) }` — the resting
                            // `×`'s own ink, because at rest they are the same
                            // kind of offer, so they are the same three mixes.
                            palette.tab_close_glyph_on_active_tab
                        } else if tab_hovered {
                            palette.tab_close_glyph_on_hovered_tab
                        } else {
                            palette.title_text_muted
                        },
                    );
                    glyph.opacity = opacity;
                    sprites.push(glyph);
                }
            }
            let Some(close) = tab.close else {
                continue;
            };
            let close_hovered = hover == Some(ChromeTarget::TabClose(index));
            if close_hovered && within_strip(viewport, close) {
                // `.tab .close:hover { background: var(--active) }` — 4px of round,
                // over whichever of the two surfaces this tab is showing: `--termbg`
                // when it is the active one, its own `--hover` fill when it is not.
                sprites.push(ChromeSprite::new(
                    ChromeMark::ControlPill {
                        radius_px: (WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX * scale)
                            .round()
                            .max(1.0) as u32,
                    },
                    pixel_snapped(close),
                    if active {
                        palette.tab_close_pill_on_content
                    } else {
                        palette.tab_close_pill_on_hovered_tab
                    },
                ));
            }
            let glyph = (WINDOW_TAB_CLOSE_GLYPH_LOGICAL_PX * scale).round().max(1.0);
            let glyph_left = ((close[0] + close[2] - glyph) / 2.0).round();
            let glyph_top = ((close[1] + close[3] - glyph) / 2.0).round();
            let glyph_rect = [glyph_left, glyph_top, glyph_left + glyph, glyph_top + glyph];
            if !within_strip(viewport, glyph_rect) {
                continue;
            }
            sprites.push(ChromeSprite::new(
                ChromeMark::TabClose,
                glyph_rect,
                // One declaration, five grounds — and the glyph has to answer to
                // the one under it for the same reason its pill does, six lines
                // above: this pipeline composites in linear light, so a
                // translucent ink cannot be handed to the blender and has to
                // arrive already mixed over the surface it lands on.
                if close_hovered {
                    // `.tab .close:hover { color: var(--ink) }`, standing on the
                    // pill this pass has just drawn — never on the bare tab.
                    if active {
                        palette.tab_close_glyph_on_pill_over_active_tab
                    } else {
                        palette.tab_close_glyph_on_pill_over_hovered_tab
                    }
                } else if active {
                    palette.tab_close_glyph_on_active_tab
                } else if tab_hovered {
                    palette.tab_close_glyph_on_hovered_tab
                } else {
                    // `.tab .close { color: var(--ink3) }` — a step below the caption
                    // run's own ink, because closing a tab is not what the strip is for.
                    // Over `--panel` that ink is the strip's own muted one, which the
                    // `+`/`˅` pair beside these tabs already wears.
                    palette.title_text_muted
                },
            ));
        }
    }

    // `.newtab` and the `.chevbtn` beside it: one family, one box, one hover.
    // `background: none` at rest is the whole of the resting state — the button
    // is a glyph on the title bar until the pointer arrives.
    let pill_radius = (WINDOW_NEW_TAB_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    let new_hovered = hover == Some(ChromeTarget::NewTab);
    let menu_hovered = hover == Some(ChromeTarget::NewTabMenu);
    for (rect, hovered) in [
        (geometry.new_tab, new_hovered),
        (geometry.new_tab_menu, menu_hovered),
    ] {
        if hovered && within_strip(viewport, rect) {
            sprites.push(ChromeSprite::new(
                ChromeMark::ControlPill {
                    radius_px: pill_radius,
                },
                pixel_snapped(rect),
                // `--hover` over `--panel` and nothing else is ever under it, so
                // this one the palette can and does pre-composite.
                palette.caption_hover,
            ));
        }
    }
    let plus = (WINDOW_NEW_TAB_GLYPH_LOGICAL_PX * scale).round().max(1.0);
    let plus_left = ((geometry.new_tab[0] + geometry.new_tab[2] - plus) / 2.0).round();
    let plus_top = ((geometry.new_tab[1] + geometry.new_tab[3] - plus) / 2.0).round();
    let plus_rect = [plus_left, plus_top, plus_left + plus, plus_top + plus];
    if within_strip(viewport, plus_rect) {
        sprites.push(ChromeSprite::new(
            ChromeMark::Plus,
            plus_rect,
            if new_hovered {
                palette.title_text_hover
            } else {
                palette.title_text_muted
            },
        ));
    }
    let chevron_width = (WINDOW_NEW_TAB_CHEVRON_WIDTH_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    let chevron_height = (WINDOW_NEW_TAB_CHEVRON_HEIGHT_LOGICAL_PX * scale)
        .round()
        .max(1.0);
    let chevron_left =
        ((geometry.new_tab_menu[0] + geometry.new_tab_menu[2] - chevron_width) / 2.0).round();
    let chevron_top =
        ((geometry.new_tab_menu[1] + geometry.new_tab_menu[3] - chevron_height) / 2.0).round();
    let chevron_rect = [
        chevron_left,
        chevron_top,
        chevron_left + chevron_width,
        chevron_top + chevron_height,
    ];
    // The rect is `.chevbtn svg`'s own 9×6 box and stays that whatever the
    // arrow is doing: the turn is a `transform`, and a CSS transform does not
    // touch layout. Room for the rotated glyph is taken by the rasterizer
    // outside this box (`ChromeMark::raster_bleed`), which is why the button
    // does not grow and the `+` beside it does not move.
    if within_strip(viewport, chevron_rect) {
        sprites.push(ChromeSprite::new(
            ChromeMark::chevron(chevron_turn),
            chevron_rect,
            if menu_hovered || profile_menu_open {
                palette.title_text_hover
            } else {
                palette.title_text_muted
            },
        ));
    }

    // `.capbtn`: a 46x40 box, a 10px glyph, and 14px for the gear alone.
    let buttons = [
        (
            ChromeTarget::Settings,
            ChromeMark::Gear,
            WINDOW_CAPTION_GEAR_GLYPH_LOGICAL_PX,
        ),
        (
            ChromeTarget::Minimize,
            ChromeMark::WindowMinimize,
            WINDOW_CAPTION_GLYPH_LOGICAL_PX,
        ),
        (
            ChromeTarget::Maximize,
            ChromeMark::WindowMaximize,
            WINDOW_CAPTION_GLYPH_LOGICAL_PX,
        ),
        (
            ChromeTarget::CloseWindow,
            ChromeMark::WindowClose,
            WINDOW_CAPTION_GLYPH_LOGICAL_PX,
        ),
    ];
    for (index, (target, mark, glyph_logical_px)) in buttons.into_iter().enumerate() {
        let left = run_left + index as f32 * button;
        let rect = [left, 0.0, (left + button).min(width), title];
        let hovered = hover == Some(target);
        if hovered {
            quads.push(ChromeQuad {
                rect,
                color: if target == ChromeTarget::CloseWindow {
                    palette.caption_close_hover
                } else {
                    palette.caption_hover
                },
            });
        }
        let glyph = (glyph_logical_px * scale).round().max(1.0);
        let glyph_left = ((rect[0] + rect[2]) / 2.0 - glyph / 2.0).round();
        let glyph_top = (title / 2.0 - glyph / 2.0).round();
        sprites.push(ChromeSprite::new(
            mark,
            [glyph_left, glyph_top, glyph_left + glyph, glyph_top + glyph],
            if hovered && target == ChromeTarget::CloseWindow {
                palette.caption_close_text
            } else if hovered {
                palette.title_text_hover
            } else {
                palette.title_text
            },
        ));
    }
}

/// A collapsed seat's bar carries its name and its state icon (§2.6.3) — except
/// in the double-collapsed 24x24 case, where 24 square cannot hold a name and
/// forcing one in would be a mosaic pretending to be text (tiny-window §1.3).
///
/// The icon is [`pane_mark`], the same component the pane head wears, because
/// §2.6.3 asks for the very same `stateIcon` a tab and a card carry: a bar that
/// breathes and lights a dot is the cheapest way to say "something is running in
/// here", and it can only do that by being the same thing. A block of ink shaped
/// like an icon says nothing at all.
///
/// Which of the three shapes to draw is read from the solver's own verdict and
/// not from the rectangle's aspect ratio. `Collapsed(AxisSet)` already names
/// which axis was squeezed (tiny-window §1.3), and a 24-wide bar in a 24-tall
/// slot is a legal single-axis collapse that guessing from `width > height`
/// would silently promote to the degenerate square.
///
/// * Squeezed along Col — a bar 24 tall and as wide as its slot. The pane head's
///   own row: mark at the leading padding, name after the gap.
/// * Squeezed along Row — a bar 24 wide and as tall as its slot. No name: the
///   chrome label pipeline draws horizontally, and 24 logical pixels do not hold
///   a horizontal word. The mark sits at the head of the strip, where a title
///   would have started, rather than adrift in the middle of it.
/// * Squeezed along both — the mark alone, centred (T209).
fn collapse_bar_contents(
    rect: [f32; 4],
    scale: f32,
    kind: SeatKind,
    presentation: Presentation,
    title: &str,
    labels: &mut Vec<ChromeLabel>,
    sprites: &mut Vec<ChromeSprite>,
) {
    let palette = chrome_palette();
    let width = rect[2] - rect[0];
    let height = rect[3] - rect[1];
    let (mark, mark_logical_px, mark_color) = pane_mark(kind, palette);
    let size = (mark_logical_px * scale)
        .round()
        .max(1.0)
        .min(width)
        .min(height);
    if size < 1.0 {
        return;
    }
    let pad = SEAT_TITLE_PADDING_LOGICAL_PX * scale;
    let names_itself =
        presentation.is_collapsed_along(Axis::Col) && !presentation.is_double_collapsed();
    let centre_x = rect[0] + (width - size) / 2.0;
    let centre_y = rect[1] + (height - size) / 2.0;
    let (mark_left, mark_top) = if names_itself {
        ((rect[0] + pad).min(rect[2] - size), centre_y)
    } else if presentation.is_double_collapsed() {
        (centre_x, centre_y)
    } else {
        (centre_x, (rect[1] + pad).min(rect[3] - size))
    };
    let mark_left = mark_left.round();
    let mark_top = mark_top.round();
    let mark_box = [mark_left, mark_top, mark_left + size, mark_top + size];
    // Full ink, where a pane head's unfocused mark is halved (D39). A collapsed
    // seat is never the focus — W2 makes it the last to fall — so the halving
    // rule would apply to every bar there is, and on a bar the mark is not a
    // redundant flourish beside a name and a body, it is the whole message.
    sprites.push(ChromeSprite::new(mark, mark_box, mark_color));
    if !names_itself {
        return;
    }
    let title_left = mark_box[2] + SEAT_TITLE_GAP_LOGICAL_PX * scale;
    let title_right = rect[2] - SEAT_TITLE_TRAILING_PADDING_LOGICAL_PX * scale;
    if title_right <= title_left {
        return;
    }
    labels.push(ChromeLabel {
        text: title.to_owned(),
        rect: [title_left, rect[1], title_right, rect[3]],
        font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
        color: palette.title_text,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
    });
}

/// What a pane calls itself.
///
/// The one channel both the pane head and the layout peek read, so a schematic
/// can never name a pane something other than what the pane's own caption says.
/// Two call sites reading two expressions is how those two drift apart, and a
/// peek that disagrees with the window behind it is worse than no peek.
///
/// Today only a preview carries a name of its own; the rest answer by kind,
/// because a tab holds exactly one terminal and that terminal's name is the
/// tab's. When terminals learn per-seat titles, this is the one place that has
/// to hear about it.
pub(crate) fn seat_caption<'a>(
    kind: SeatKind,
    preview_title: Option<&'a str>,
    terminal_cwd: Option<&'a str>,
) -> &'a str {
    match kind {
        SeatKind::Preview => preview_title.unwrap_or_else(|| seat_title(kind)),
        // C28: `<span class="ptitle">${s.cwd}</span>` (mock-up 4559) — the whole
        // path, and the mock-up is deliberate about that. The *other* place a
        // pane's name is written, the drag ghost and the drop preview, uses
        // `cwdLeaf(s)` and shows only the last segment (3304): a caption has a
        // whole head to fill and answers "where is this", a label riding the
        // pointer has one line and answers "which one is this". Two questions,
        // two lengths, one place each.
        //
        // Falls back to the kind's own name when the shell has never reported an
        // OSC 7, because that is the honest answer to "where is this" when
        // nobody has said — the same reading `term_leaf` gives the vault.
        SeatKind::Terminal => terminal_cwd
            .filter(|cwd| !cwd.is_empty())
            .unwrap_or_else(|| seat_title(kind)),
        _ => seat_title(kind),
    }
}

/// What a leaf this build cannot name says about itself, in its own body (T227).
///
/// It names the cause rather than apologising, because the cause is the only
/// actionable thing in it: the tree came off disk carrying a kind this binary has
/// no code for, which is what a session written by a newer build looks like.
pub(crate) const PLACEHOLDER_SEAT_NOTICE: &str =
    "This pane was saved by a newer version of BetterTerminal";

/// The tail line of the L4 strip: how many seats it had no row for.
///
/// Plain counting, and a verb that agrees with it — the sentence is read at the
/// exact moment the window is at its least trustworthy, and a number wearing the
/// wrong verb is one more thing that looks broken.
fn overflow_notice(hidden: usize) -> String {
    if hidden == 1 {
        "1 more does not fit".to_owned()
    } else {
        format!("{hidden} more do not fit")
    }
}

/// The same name, cut to the one segment that answers "which one is this".
///
/// C28 is a ruling about two lengths, not two names: a pane head has a whole bar
/// to fill and answers "where is this" with the full path, while a label riding
/// the pointer has one line and shows `cwdLeaf(s)` — the last segment only
/// (mock-up 3304). A collapsed bar is 24 logical pixels of the second kind, so
/// it takes the second reading.
///
/// It is *derived* from [`seat_caption`] rather than read from the source a
/// second time, which is the whole of that function's warning: two call sites
/// evaluating two expressions is how two names drift apart. Cutting a path at
/// its last separator is a no-op on every name that is not a path, so preview
/// titles and kind names arrive unchanged.
pub(crate) fn seat_short_caption<'a>(
    kind: SeatKind,
    preview_title: Option<&'a str>,
    terminal_cwd: Option<&'a str>,
) -> &'a str {
    let full = seat_caption(kind, preview_title, terminal_cwd);
    full.rsplit(['/', '\\'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(full)
}

/// The line box every piece of chrome text is laid out in — `shape_chrome_labels`
/// sizes a label's buffer to `font_size * 1.4`, and this is that fact restated
/// where a layout needs to reserve room for it.
///
/// **Duplicated**, deliberately and visibly: `tooltip.rs` and `peek_strip.rs`
/// each carry their own copy of this number for the same reason, and it is one
/// renderer fact behind all three. The day it moves, it moves to `bt-render`
/// beside the function that decides it and all three call sites follow — which
/// is a change to two modules this slice has no business touching.
const CHROME_LINE_HEIGHT: f32 = 1.4;

/// The drag ghost's box and the two things standing in it, in physical pixels
/// (J114).
///
/// `.drag-ghost` is a two-item flex row — `mark + name`, `gap: 7px`,
/// `align-items: center` — inside `padding: 5px 12px` and a 1px border, so the
/// box shrink-wraps whatever is in it and there is no wrapping, no ellipsis and
/// no width bound. That last part is the mock-up's, not an omission here: the
/// label is a short name by construction (`seat_short_caption`), and a ghost
/// that truncated would be answering "which one is this" with half a word.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DragGhostLayout {
    /// The ghost's outer box, border included.
    pub frame: [f32; 4],
    /// The mark's square, vertically centred on the row.
    pub mark: [f32; 4],
    /// The name's line box, vertically centred on the same row.
    pub label: [f32; 4],
}

/// Hang the ghost off the pointer (J115).
///
/// `pointer` is the hotspot in physical pixels and the box goes down-and-right of
/// it by [`bt_render::DRAG_GHOST_POINTER_OFFSET_LOGICAL_PX`] — deliberately
/// unclamped, for the reason recorded on that constant.
///
/// The row's height is the taller of its two items rather than the mark's or the
/// text's alone: `align-items: center` on a flex row makes the line box as tall
/// as the tallest item, and at every scale this build ships the *text* is the
/// taller of the two (17.5 against 15). Writing it as a `max` rather than as the
/// line height keeps that a fact about the numbers instead of a coincidence the
/// code depends on.
pub(crate) fn drag_ghost_layout(
    pointer: [f32; 2],
    mark_logical: f32,
    label_width: f32,
    scale: f32,
) -> DragGhostLayout {
    let px = |logical: f32| logical * scale;
    let border = px(bt_render::DRAG_GHOST_BORDER_LOGICAL_PX);
    let pad_x = px(bt_render::DRAG_GHOST_PADDING_X_LOGICAL_PX);
    let pad_y = px(bt_render::DRAG_GHOST_PADDING_Y_LOGICAL_PX);
    let gap = px(bt_render::DRAG_GHOST_GAP_LOGICAL_PX);
    let mark = px(mark_logical).round();
    let line = (px(bt_render::DRAG_GHOST_FONT_LOGICAL_PX) * CHROME_LINE_HEIGHT).round();
    let row = mark.max(line);

    let left = (pointer[0] + px(bt_render::DRAG_GHOST_POINTER_OFFSET_LOGICAL_PX[0])).round();
    let top = (pointer[1] + px(bt_render::DRAG_GHOST_POINTER_OFFSET_LOGICAL_PX[1])).round();
    let width = (2.0 * (border + pad_x) + mark + gap + label_width).round();
    let height = (2.0 * (border + pad_y) + row).round();
    let frame = [left, top, left + width, top + height];

    let centre = (frame[1] + frame[3]) / 2.0;
    let mark_left = left + border + pad_x;
    let mark_top = (centre - mark / 2.0).round();
    let label_left = mark_left + mark + gap;
    let label_top = (centre - line / 2.0).round();
    DragGhostLayout {
        frame,
        mark: [mark_left, mark_top, mark_left + mark, mark_top + mark],
        label: [
            label_left,
            label_top,
            frame[2] - border - pad_x,
            label_top + line,
        ],
    }
}

/// Paint the ghost — one layer, handed to the renderer above every other
/// floating surface.
///
/// `z-index: 100` against the tip's `60` (mock-up 1717 and 1207). The tip is
/// otherwise the one surface in this window that is never covered, and the ghost
/// is the single exception the design allows itself: a tip explains what is under
/// the pointer, and during a drag what is under the pointer is *this*.
///
/// It goes through the same float-window recipe every other floating surface
/// does — halo, hairline, face — with its own radius and its own pair of shadow
/// alphas. Nothing about it is hand-drawn.
pub(crate) fn build_drag_ghost(
    layout: &DragGhostLayout,
    mark: ChromeMark,
    mark_color: [u8; 3],
    text: &str,
    scale: f32,
    palette: bt_render::ChromePalette,
) -> crate::marks::OverlayLayer {
    let alpha = |value: u8| f32::from(value) / 255.0;
    let mut quads = Vec::new();
    crate::settings::push_float_window(
        &mut quads,
        layout.frame,
        bt_render::DRAG_GHOST_RADIUS_LOGICAL_PX * scale,
        bt_render::DRAG_GHOST_BORDER_LOGICAL_PX * scale,
        bt_render::FLOAT_WINDOW_SHADOW_LOGICAL_PX * scale,
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.drag_ghost_shadow_inner_alpha),
        alpha(palette.drag_ghost_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );
    crate::marks::OverlayLayer {
        quads,
        labels: vec![ChromeLabel {
            text: text.to_owned(),
            rect: layout.label,
            font_size_px: bt_render::DRAG_GHOST_FONT_LOGICAL_PX * scale,
            // `color: var(--ink)` over `--menu`. That composite already has a
            // name — the one the combo's selected row needed first — and a
            // second constant holding the same two bytes is how two inks that
            // are the same ink drift apart.
            color: palette.menu_item_text_selected,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
        }],
        sprites: vec![ChromeSprite::new(mark, layout.mark, mark_color)],
        opacity: 1.0,
    }
}

fn seat_title(kind: SeatKind) -> &'static str {
    match kind {
        SeatKind::Terminal => "Terminal",
        SeatKind::Files => "Files",
        SeatKind::Preview => "Preview",
        SeatKind::Placeholder => "Unavailable",
    }
}

/// The mark a pane head wears, its size in logical pixels, and the colour
/// `currentColor` resolves to.
///
/// Three of the four are the mock-up's own pairings: `stateIcon` puts the
/// session's profile mark on a terminal head at `.pmark`'s 15px,
/// `.preview-head .files-ico` is `#i-file` at 14px in `--accent`, and
/// `.files-head .files-ico` is `#i-folder` at 13px in `--accent`. A profile mark
/// carries its own colours, so the colour handed to it is never used.
///
/// The fourth has no counterpart, because the mock-up has no notion of a leaf
/// this build cannot name. It gets the generic pane outline in the same quiet
/// ink a body notice uses — a placeholder is a statement about this build, not
/// an invitation, and the accent is reserved for things that want you.
pub(crate) fn pane_mark(
    kind: SeatKind,
    palette: bt_render::ChromePalette,
) -> (ChromeMark, f32, [u8; 3]) {
    match kind {
        SeatKind::Terminal => (
            ChromeMark::ProfilePowerShell,
            PANE_HEAD_PROFILE_MARK_LOGICAL_PX,
            palette.accent,
        ),
        SeatKind::Files => (
            ChromeMark::Folder,
            PANE_HEAD_FOLDER_MARK_LOGICAL_PX,
            palette.accent,
        ),
        SeatKind::Preview => (
            ChromeMark::File,
            PANE_HEAD_FILE_MARK_LOGICAL_PX,
            palette.accent,
        ),
        SeatKind::Placeholder => (
            ChromeMark::Panel,
            PANE_HEAD_FOLDER_MARK_LOGICAL_PX,
            palette.body_hint_text,
        ),
    }
}

/// The ratio a pointer at `position` (device pixels along `slot.dir`) is asking
/// this split for. The clamp is `apply`'s job, not this function's.
pub fn requested_ratio(
    slot: SplitSlot,
    scale_ppm: u32,
    position: f64,
) -> Option<(Ratio, LogicalPx)> {
    let usable = slot.slot.extent(slot.dir) - DIVIDER;
    if !usable.subpixels().is_positive() {
        return None;
    }
    let pointer = device_to_logical_signed(position, scale_ppm);
    let leading = pointer - slot.slot.near(slot.dir).subpixels();
    let ppm = (i128::from(leading) * 1_000_000 / i128::from(usable.subpixels())).clamp(0, 1_000_000)
        as u32;
    Some((Ratio::clamped_from_ppm(ppm), usable))
}

fn device_to_logical_signed(device_px: f64, scale_ppm: u32) -> i64 {
    let numer = device_px * SUBPIXELS_PER_PX as f64 * 1_000_000.0;
    (numer / f64::from(scale_ppm.max(1))).round() as i64
}

// ---------------------------------------------------------------------------
// Persistence (docs/M2-persistence-schema-v1.md §3.2, solver spec §5)
//
// Red line L11: what goes to disk is layout *intent* — the tree's shape, each
// split's direction and its `u32` ppm ratio — never a rectangle, never a cols/
// rows count, never a DPI. Restoring onto a machine with a different DPI or
// font size has to be a fresh `solve`, and storing a rectangle would pass one
// solve's *result* off as its *intent*.
// ---------------------------------------------------------------------------

impl Seats {
    /// The durable form of this tree, with `term` written into every terminal
    /// leaf.
    ///
    /// The seed arrives from above rather than being read here because a seat
    /// does not know it: `profile_id`, `cwd` and the manual name belong to the
    /// *session* a tab holds, and this module owns rectangles, not sessions.
    /// This window runs one shell per tab, so one seed describes every terminal
    /// leaf in it; when panes get their own children, this parameter becomes the
    /// per-leaf lookup and nothing else about the shape changes.
    pub fn to_persisted(&self, term: &TermLeafV1) -> LayoutNodeV1 {
        to_persisted(&self.tree, term)
    }

    /// Rebuild a tree from disk. Split ids are re-minted from the shape (§3.2:
    /// the runtime `id` is a handle and is not persisted).
    ///
    /// `None` when the persisted tree holds no terminal leaf at all — this
    /// window has exactly one terminal and no way to be honest about a tree
    /// that does not contain it. That is a per-*document* refusal, distinct
    /// from the per-*leaf* degradation §5 asks for, which `bt-persist` has
    /// already applied by the time this is called.
    pub fn from_persisted(node: &LayoutNodeV1) -> Option<Self> {
        let mut next_seat = 1u64;
        let mut next_split = 1u64;
        let tree = from_persisted(node, &mut next_seat, &mut next_split);
        let terminal = tree
            .seats_in_order()
            .into_iter()
            .find(|seat| seat.kind == SeatKind::Terminal)?
            .id;
        Some(Self {
            tree,
            terminal,
            focus: terminal,
            next_seat,
            next_split,
        })
    }
}

fn to_persisted(node: &LayoutNode, term: &TermLeafV1) -> LayoutNodeV1 {
    match node {
        LayoutNode::Seat(seat) => LayoutNodeV1::Leaf(match seat.kind {
            // The seed proper — profile, place, your name for it — and the whole
            // of what a closed tab can be rebuilt from. It used to be three
            // placeholders written unconditionally, which meant every restored
            // tab came back in the wrong folder under the wrong name.
            SeatKind::Terminal => LeafNodeV1::Term(term.clone()),
            SeatKind::Files => LeafNodeV1::Files(bt_persist::FilesLeafV1 {
                root: String::new(),
                open: Vec::new(),
                sel: None,
                width: seat
                    .fixed_extent
                    .map_or(240, |extent| extent.floor_px().max(0) as u32),
            }),
            SeatKind::Preview => LeafNodeV1::Preview(bt_persist::PreviewLeafV1 {
                pinned: seat.pinned,
            }),
            // A placeholder is what an unknown kind read *as*; writing it back
            // as `unknown` keeps the leaf visible for the build that does know
            // it rather than quietly deleting a seat this build cannot name.
            SeatKind::Placeholder => LeafNodeV1::Unknown,
        }),
        LayoutNode::Split {
            dir, ratio, a, b, ..
        } => LayoutNodeV1::Split(SplitNodeV1 {
            dir: match dir {
                Axis::Row => SplitDirV1::Row,
                Axis::Col => SplitDirV1::Col,
            },
            // The `u32` ppm, written out and read back unchanged (§5 constraint
            // 1). A decimal string or a JSON float would not round-trip.
            ratio: ratio.ppm(),
            children: [
                Box::new(to_persisted(a, term)),
                Box::new(to_persisted(b, term)),
            ],
        }),
    }
}

fn from_persisted(node: &LayoutNodeV1, next_seat: &mut u64, next_split: &mut u64) -> LayoutNode {
    match node {
        LayoutNodeV1::Leaf(leaf) => {
            let id = SeatId(*next_seat);
            *next_seat += 1;
            let kind = match leaf {
                LeafNodeV1::Term(_) => SeatKind::Terminal,
                LeafNodeV1::Files(_) => SeatKind::Files,
                LeafNodeV1::Preview(_) => SeatKind::Preview,
                // §5 constraint 2: a kind this build does not know degrades per
                // leaf into a *visible* placeholder. Turning it silently into a
                // terminal is the one thing the ruling forbids.
                LeafNodeV1::Unknown => SeatKind::Placeholder,
            };
            let mut seat = Seat::new(id, kind);
            match leaf {
                LeafNodeV1::Files(files) => {
                    seat = seat.with_fixed_extent(LogicalPx::px(i64::from(files.width)));
                }
                LeafNodeV1::Preview(preview) if preview.pinned => seat = seat.pinned(),
                _ => {}
            }
            LayoutNode::seat(seat)
        }
        LayoutNodeV1::Split(split) => {
            let id = SplitId(*next_split);
            *next_split += 1;
            let dir = match split.dir {
                SplitDirV1::Row => Axis::Row,
                SplitDirV1::Col => Axis::Col,
            };
            let a = from_persisted(&split.children[0], next_seat, next_split);
            let b = from_persisted(&split.children[1], next_seat, next_split);
            LayoutNode::split_at(id, dir, Ratio::clamped_from_ppm(split.ratio), a, b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_persist::{
        SESSION_SCHEMA_VERSION, SessionV1, TabV1, read_session, write_session_atomic,
    };

    fn viewport_of(width: u32, height: u32, dpi_milli: u32) -> LogicalRect {
        logical_viewport(width, height, scale_ppm(dpi_milli))
    }

    fn seats_surface(width: u32, height: u32, dpi_milli: u32) -> SeatViewport {
        let title = logical_to_device(WINDOW_TITLE_BAR_LOGICAL_PX, scale_ppm(dpi_milli))
            .min(height.saturating_sub(1));
        SeatViewport {
            x: 0,
            y: title,
            width,
            height: height.saturating_sub(title).max(1),
        }
    }

    fn solved(seats: &Seats, viewport: LogicalRect, metrics: &SeatMetrics) -> SeatLayout {
        solved_with_overflow(seats, viewport, metrics).0
    }

    fn solved_with_overflow(
        seats: &Seats,
        viewport: LogicalRect,
        metrics: &SeatMetrics,
    ) -> (SeatLayout, Option<FitOverflow>) {
        match seats.solve(viewport, metrics) {
            Ok(layout) => (layout, None),
            Err(_) => fit_what_fits(seats, viewport, metrics),
        }
    }

    /// The hard gate of this slice, stated as an equality rather than as a
    /// promise: a lone terminal leaf's rectangle *is* the viewport, and its
    /// device rectangle *is* the surface below the 40px titlebar. Every
    /// downstream number the terminal computes is a function of that one
    /// rectangle, so the titlebar cannot be consumed twice or ignored.
    ///
    /// The second half is the red gate: shift the viewport by one physical
    /// pixel and the same assertions fail, so the equality above is testing
    /// something rather than restating a tautology.
    #[test]
    fn title_bar_consumes_40_logical_pixels_and_moves_the_seats_viewport() {
        for dpi_milli in [1_000u32, 1_250, 1_500, 1_750, 2_000, 2_500] {
            for (width, height) in [(960u32, 600u32), (1, 1), (1279, 721), (3840, 2160)] {
                let seats = Seats::lone_terminal();
                let metrics = seat_metrics(dpi_milli);
                let viewport = viewport_of(width, height, dpi_milli);
                let layout = solved(&seats, viewport, &metrics);
                assert_eq!(layout.rects.len(), 1, "a lone leaf is one seat");
                assert_eq!(
                    layout.get(seats.terminal()).unwrap().rect,
                    Some(viewport),
                    "the seat rectangle is the viewport itself at {dpi_milli} milli-DPI"
                );
                assert_eq!(
                    seat_viewport(&layout, seats.terminal()),
                    Some(seats_surface(width, height, dpi_milli)),
                    "{width}x{height} at {dpi_milli} milli-DPI must reserve exactly the title bar"
                );

                // Red gate: one physical pixel of offset, and the seat is no
                // longer the surface.
                let one_px = device_to_logical(1, scale_ppm(dpi_milli));
                let shifted = LogicalRect::new(
                    one_px,
                    one_px,
                    viewport.right + one_px,
                    viewport.bottom + one_px,
                );
                let shifted_layout = solved(&seats, shifted, &metrics);
                assert_ne!(
                    seat_viewport(&shifted_layout, seats.terminal()),
                    Some(seats_surface(width, height, dpi_milli)),
                    "the pin would pass even with an injected offset"
                );
            }
        }
    }

    /// A lone terminal leaf owns only window chrome: pane chrome starts when a
    /// second pane gives the head a disambiguating job.
    #[test]
    fn a_lone_leaf_draws_no_terminal_pane_head() {
        let seats = Seats::lone_terminal();
        let metrics = seat_metrics(1_000);
        let layout = solved(&seats, viewport_of(960, 600, 1_000), &metrics);
        let (quads, labels, sprites) = build_chrome(&seats, &layout, 1.0, ChromePointer::default());
        let palette = chrome_palette();
        assert!(quads.iter().any(|quad| {
            quad.rect == [0.0, 0.0, 960.0, WINDOW_TITLE_BAR_LOGICAL_PX]
                && quad.color == palette.title_bar
        }));
        assert!(labels.iter().any(|label| label.text == "PowerShell"));
        assert!(!labels.iter().any(|label| label.text == "Terminal"));
        for mark in [
            ChromeMark::Gear,
            ChromeMark::WindowMinimize,
            ChromeMark::WindowMaximize,
            ChromeMark::WindowClose,
        ] {
            assert!(
                sprites.iter().any(|sprite| sprite.mark == mark),
                "{mark:?} must be a caption mark"
            );
        }
        assert_eq!(
            sprites
                .iter()
                .filter(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
                .count(),
            1,
            "the lone leaf wears its profile mark only in the window tab"
        );
        assert!(!quads.iter().any(|quad| quad.color == palette.pane_head));
        assert!(hit_chrome(&seats, &layout, 1.0, 480.0, 300.0).is_none());
    }

    /// PIN (visual fidelity pass): the caption glyphs are the mock-up's own
    /// `<symbol>`s at the sizes `.capbtn svg` gives them — 14px for the gear,
    /// 10px for the three window buttons — centred in their 46x40 box, and the
    /// glyph under the pointer wears the hover ink while `.close-w:hover` wears
    /// white.
    ///
    /// Red gate: the box is 46 wide and the glyph 10, so a sprite that had been
    /// handed the whole button rectangle — the shape the previous text glyphs
    /// were given — fails every size assertion here.
    #[test]
    fn caption_marks_are_mockup_symbols_at_mockup_sizes_centred_in_their_box() {
        let seats = Seats::lone_terminal();
        let palette = chrome_palette();
        for dpi_milli in [1_000u32, 1_250, 1_500, 2_000] {
            let scale = dpi_milli as f32 / 1_000.0;
            let metrics = seat_metrics(dpi_milli);
            let layout = solved(&seats, viewport_of(1600, 900, dpi_milli), &metrics);
            let (_, _, sprites) = build_chrome(
                &seats,
                &layout,
                scale,
                ChromePointer {
                    hover: Some(ChromeTarget::CloseWindow),
                    dragging: None,
                    ..ChromePointer::default()
                },
            );
            let expected = [
                (ChromeMark::Gear, WINDOW_CAPTION_GEAR_GLYPH_LOGICAL_PX),
                (ChromeMark::WindowMinimize, WINDOW_CAPTION_GLYPH_LOGICAL_PX),
                (ChromeMark::WindowMaximize, WINDOW_CAPTION_GLYPH_LOGICAL_PX),
                (ChromeMark::WindowClose, WINDOW_CAPTION_GLYPH_LOGICAL_PX),
            ];
            let button = WINDOW_CAPTION_BUTTON_LOGICAL_PX * scale;
            let title = (WINDOW_TITLE_BAR_LOGICAL_PX * scale).round();
            let run_left = 1600.0 - 4.0 * button;
            for (index, (mark, logical_px)) in expected.into_iter().enumerate() {
                let sprite = sprites
                    .iter()
                    .find(|sprite| sprite.mark == mark)
                    .unwrap_or_else(|| panic!("{mark:?} missing at {dpi_milli} milli-DPI"));
                let side = (logical_px * scale).round();
                assert_eq!(
                    sprite.rect[2] - sprite.rect[0],
                    side,
                    "{mark:?} width at {dpi_milli} milli-DPI"
                );
                assert_eq!(
                    sprite.rect[3] - sprite.rect[1],
                    side,
                    "{mark:?} height at {dpi_milli} milli-DPI"
                );
                let box_centre = run_left + (index as f32 + 0.5) * button;
                assert!(
                    ((sprite.rect[0] + sprite.rect[2]) / 2.0 - box_centre).abs() <= 0.5,
                    "{mark:?} must sit in the middle of its 46px box: sprite {:?} vs centre {box_centre} (run_left {run_left}, button {button})",
                    sprite.rect
                );
                assert!(
                    ((sprite.rect[1] + sprite.rect[3]) / 2.0 - title / 2.0).abs() <= 0.5,
                    "{mark:?} must sit in the middle of the 40px bar"
                );
                assert_eq!(
                    sprite.color,
                    if mark == ChromeMark::WindowClose {
                        palette.caption_close_text
                    } else {
                        palette.title_text
                    },
                    "{mark:?} ink at {dpi_milli} milli-DPI"
                );
            }
        }
    }

    /// PIN (visual fidelity pass): the tab is a rounded silhouette, not a stack
    /// of quads, and the title bar's foot carries nothing across it.
    ///
    /// Three claims, each of which the previous drawing broke:
    ///
    /// * the tab's fill is the same value as the pane head below it and as the
    ///   terminal's own background, so tab and content are one surface;
    /// * the silhouette starts at the window's own left edge (`x = 0`) and its
    ///   lower edge is the title bar's lower edge, so nothing of it spills into
    ///   the pane head row;
    /// * no quad spans the surface at the bar's foot — the horizontal rule that
    ///   used to cut the tab off from the terminal is gone.
    #[test]
    fn the_active_tab_joins_the_content_plane_and_the_bar_has_no_rule_across_it() {
        let seats = Seats::lone_terminal();
        let palette = chrome_palette();
        for dpi_milli in [1_000u32, 1_250, 1_500, 1_750, 2_000] {
            let scale = dpi_milli as f32 / 1_000.0;
            let width = 1600.0_f32;
            let metrics = seat_metrics(dpi_milli);
            let layout = solved(&seats, viewport_of(1600, 900, dpi_milli), &metrics);
            let (quads, _, sprites) =
                build_chrome(&seats, &layout, scale, ChromePointer::default());
            let title = (WINDOW_TITLE_BAR_LOGICAL_PX * scale).round();
            let radius = (WINDOW_TAB_RADIUS_LOGICAL_PX * scale).round();

            let tab = sprites
                .iter()
                .find(|sprite| matches!(sprite.mark, ChromeMark::ActiveTab { .. }))
                .unwrap_or_else(|| panic!("no tab silhouette at {dpi_milli} milli-DPI"));
            assert_eq!(
                tab.mark,
                ChromeMark::ActiveTab {
                    radius_px: radius as u32
                },
                "the tab's corners are --tabr at {dpi_milli} milli-DPI"
            );
            assert_eq!(
                tab.color, palette.active_tab,
                "the active tab is filled with --termbg"
            );
            assert_eq!(
                palette.active_tab, palette.pane_head,
                "tab and pane head must be the same surface, or the join is a lie"
            );
            assert_eq!(
                tab.rect[0], 0.0,
                "the first tab's skirt lands on the window edge"
            );
            assert_eq!(
                tab.rect[3], title,
                "the tab stops at the bar's foot and never reaches into the pane head"
            );
            assert_eq!(
                tab.rect[3] - tab.rect[1],
                (WINDOW_TAB_HEIGHT_LOGICAL_PX * scale).round(),
                "the tab is 34px tall"
            );

            // The bar's foot: nothing may run across it.
            for quad in &quads {
                let spans_the_bar_foot = quad.rect[1] < title
                    && quad.rect[3] >= title
                    && quad.rect[0] <= 0.0
                    && quad.rect[2] >= width;
                if spans_the_bar_foot {
                    assert_eq!(
                        quad.color, palette.title_bar,
                        "only the bar's own fill may reach its foot; found {:?}",
                        quad.color
                    );
                }
            }
            assert!(
                !quads.iter().any(|quad| {
                    quad.rect[3] == title && quad.rect[3] - quad.rect[1] < 4.0 * scale
                }),
                "`.titlebar` has no border in the mock-up — no hairline of any \
                 colour may sit at the bar's foot at {dpi_milli} milli-DPI"
            );
        }
    }

    /// Red gate: titlebar primitives, the one active tab, all four caption
    /// labels, and both ordinary/destructive hover colors must be emitted by the
    /// production chrome builder.
    #[test]
    fn window_chrome_contains_tab_caption_buttons_and_mockup_hover_colors() {
        let seats = Seats::lone_terminal();
        let metrics = seat_metrics(1_000);
        let layout = solved(&seats, viewport_of(960, 600, 1_000), &metrics);
        let palette = chrome_palette();

        let (settings_quads, settings_labels, settings_sprites) = build_chrome(
            &seats,
            &layout,
            1.0,
            ChromePointer {
                hover: Some(ChromeTarget::Settings),
                dragging: None,
                ..ChromePointer::default()
            },
        );
        assert!(settings_quads.iter().any(|quad| {
            quad.rect == [776.0, 0.0, 822.0, 40.0] && quad.color == palette.caption_hover
        }));
        assert!(settings_sprites.iter().any(|sprite| {
            matches!(sprite.mark, ChromeMark::ActiveTab { .. })
                && sprite.color == palette.active_tab
                && sprite.rect[2] > WINDOW_TAB_RADIUS_LOGICAL_PX
                && sprite.rect[3] == WINDOW_TITLE_BAR_LOGICAL_PX
        }));
        assert!(
            settings_sprites
                .iter()
                .any(|sprite| sprite.mark == ChromeMark::Gear
                    && sprite.color == palette.title_text_hover),
            "the gear under the pointer takes the hover ink"
        );
        assert!(
            settings_labels
                .iter()
                .any(|label| label.text == "PowerShell")
        );

        let (close_quads, _close_labels, close_sprites) = build_chrome(
            &seats,
            &layout,
            1.0,
            ChromePointer {
                hover: Some(ChromeTarget::CloseWindow),
                dragging: None,
                ..ChromePointer::default()
            },
        );
        assert!(close_quads.iter().any(|quad| {
            quad.rect == [914.0, 0.0, 960.0, 40.0] && quad.color == palette.caption_close_hover
        }));
        assert!(close_sprites.iter().any(|sprite| {
            sprite.mark == ChromeMark::WindowClose && sprite.color == palette.caption_close_text
        }));
    }

    /// A lone leaf has no pane head, so its terminal body is exactly its seat.
    #[test]
    fn lone_terminal_body_viewport_is_the_whole_seat() {
        for dpi_milli in [1_000u32, 1_250, 1_500, 1_750, 2_000, 2_500] {
            let seats = Seats::lone_terminal();
            let metrics = seat_metrics(dpi_milli);
            let layout = solved(&seats, viewport_of(1200, 900, dpi_milli), &metrics);
            let whole = seat_viewport(&layout, seats.terminal()).unwrap();
            let body = pane_body_viewport(
                &seats,
                &layout,
                seats.terminal(),
                dpi_milli as f32 / 1_000.0,
            )
            .unwrap();
            assert_eq!(
                body, whole,
                "lone body must equal its seat at {dpi_milli} milli-DPI"
            );
        }
    }

    /// Once the tree has two panes, every full pane keeps the common head and
    /// terminal grid sizing excludes it.
    #[test]
    fn two_panes_draw_heads_and_deduct_them_from_their_bodies() {
        let dpi_milli = 1_000;
        let metrics = seat_metrics(dpi_milli);
        let mut seats = Seats::lone_terminal();
        assert!(seats.toggle_preview(&metrics));
        let layout = solved(&seats, viewport_of(1600, 900, dpi_milli), &metrics);
        let whole = seat_viewport(&layout, seats.terminal()).unwrap();
        let body = pane_body_viewport(&seats, &layout, seats.terminal(), 1.0).unwrap();
        assert_eq!(body.y, whole.y + SEAT_TITLE_BAR_LOGICAL_PX as u32);
        assert_eq!(body.height, whole.height - SEAT_TITLE_BAR_LOGICAL_PX as u32);

        let (quads, labels, sprites) = build_chrome(&seats, &layout, 1.0, ChromePointer::default());
        let palette = chrome_palette();
        for placement in &layout.rects {
            let rect = placement.device_rect.unwrap();
            assert!(quads.iter().any(|quad| {
                quad.color == palette.pane_head
                    && quad.rect
                        == [
                            rect.left as f32,
                            rect.top as f32,
                            rect.right as f32,
                            // The head's *fill*, which is 27 and not 28: the
                            // mock-up is `box-sizing: border-box`, so the
                            // hairline is the twenty-eighth row rather than a
                            // twenty-ninth. Pinned in full below.
                            rect.top as f32 + SEAT_TITLE_BAR_LOGICAL_PX
                                - SEAT_TITLE_EDGE_LOGICAL_PX,
                        ]
            }));
        }
        assert!(labels.iter().any(|label| label.text == "Terminal"));
        assert!(labels.iter().any(|label| label.text == "Preview"));
        assert!(
            sprites
                .iter()
                .any(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
        );
        assert!(sprites.iter().any(|sprite| sprite.mark == ChromeMark::File));
    }

    /// PIN (D2): the pane head is twenty-eight pixels *including* its hairline,
    /// and the terminal's first row starts on the twenty-ninth.
    ///
    /// The mock-up opens with `* { box-sizing: border-box }` (line 77), so
    /// `.panehead { height: 28px; border-bottom: 1px }` (lines 1515-1523) is 27
    /// rows of fill plus one of `--border-soft` — twenty-eight in total, and
    /// the flex box centres its mark and caption in the 27 that are left.
    ///
    /// Red gate: the head was drawn as 28 rows of fill with the hairline laid
    /// across 28..29, while `pane_body_viewport` advanced the terminal by 28.
    /// The hairline therefore sat *on top of* the terminal's first logical
    /// pixel row — one row of every multi-pane terminal painted over by
    /// chrome, at every scale, in both themes. Border-box is not a detail here:
    /// it is the difference between a caption that ends where the body begins
    /// and one that eats into it.
    ///
    /// The three numbers are asserted as one chain — fill, then hairline, then
    /// the body's own origin — because any two of them can agree while the
    /// third is wrong, and the overlap is exactly what a two-way check misses.
    #[test]
    fn the_pane_heads_hairline_is_the_last_row_of_its_own_twenty_eight() {
        let palette = chrome_palette();
        for dpi_milli in [1_000u32, 1_250, 1_500, 1_750, 2_000, 2_500] {
            let scale = dpi_milli as f32 / 1_000.0;
            let metrics = seat_metrics(dpi_milli);
            let mut seats = Seats::lone_terminal();
            assert!(seats.toggle_preview(&metrics));
            let layout = solved(&seats, viewport_of(1600, 900, dpi_milli), &metrics);
            let (quads, _, _) = build_chrome(&seats, &layout, scale, ChromePointer::default());

            // The border box, rounded to whole device pixels exactly once, so
            // the drawing and the viewport cannot round apart.
            let bar = (SEAT_TITLE_BAR_LOGICAL_PX * scale).round();
            let edge = (SEAT_TITLE_EDGE_LOGICAL_PX * scale).max(1.0);
            for placement in &layout.rects {
                let rect = placement.device_rect.unwrap();
                let top = rect.top as f32;
                let head = quads
                    .iter()
                    .find(|quad| quad.color == palette.pane_head && quad.rect[1] == top)
                    .unwrap_or_else(|| panic!("{dpi_milli}: every full pane wears a head"));
                assert_eq!(
                    head.rect[3],
                    top + bar - edge,
                    "{dpi_milli}: the fill is the border box less its border"
                );
                let hairline = quads
                    .iter()
                    .find(|quad| quad.color == palette.pane_head_edge && quad.rect[1] >= top)
                    .unwrap_or_else(|| panic!("{dpi_milli}: and the hairline under it"));
                assert_eq!(
                    hairline.rect[1], head.rect[3],
                    "{dpi_milli}: no seam between the fill and its border"
                );
                assert_eq!(
                    hairline.rect[3],
                    top + bar,
                    "{dpi_milli}: `border-box` — the hairline is inside the 28"
                );

                // And the body starts where the border box ends, so nothing the
                // head draws can land on a terminal row.
                let body = pane_body_viewport(&seats, &layout, placement.id, scale)
                    .expect("a full pane has a body");
                let whole = seat_viewport(&layout, placement.id).unwrap();
                assert_eq!(
                    body.y,
                    whole.y + bar as u32,
                    "{dpi_milli}: the body is deducted the whole border box"
                );
                assert!(
                    hairline.rect[3] <= body.y as f32,
                    "{dpi_milli}: the hairline must not cover the terminal's \
                     first row — saw it end at {} against a body at {}",
                    hairline.rect[3],
                    body.y
                );

                // The head's hit target is the box the design draws, hairline
                // and all: a pointer on the last row of the caption is still on
                // the caption.
                assert_eq!(
                    hit_chrome(
                        &seats,
                        &layout,
                        scale,
                        f64::from(rect.left as f32 + 1.0),
                        f64::from(top + bar - 1.0),
                    ),
                    Some(ChromeTarget::PaneHeader(placement.id)),
                    "{dpi_milli}: the border box is the header, hairline included"
                );
            }
        }
    }

    /// PIN (D4): the focused pane's caption is the mock-up's `500`, and it is
    /// the only one.
    ///
    /// `.pane.focused .panehead { color: var(--ink); font-weight: 500 }`
    /// (mock-up line 1644) is one declaration with two halves, and the mock-up's
    /// own note beside it says why both are needed: the focused pane is told
    /// apart by *hierarchy* rather than by a fill, after tinting it with the
    /// accent was ruled out for colliding with the unread dot. "A title at
    /// `--ink` beside titles at `--ink3` is already a hierarchy" — and the
    /// weight is the other half of that hierarchy.
    ///
    /// Red gate: the colour half shipped and the weight half did not. Every
    /// pane head was `ChromeLabelWeight::Regular`, because the enum had no 500
    /// to give it, so the focused head was carrying the whole distinction on
    /// one channel.
    #[test]
    fn the_focused_pane_head_is_the_only_caption_at_five_hundred() {
        let metrics = seat_metrics(1_000);
        let mut seats = Seats::lone_terminal();
        assert!(seats.toggle_preview(&metrics));
        let layout = solved(&seats, viewport_of(1600, 900, 1_000), &metrics);
        let (_, labels, _) = build_chrome(&seats, &layout, 1.0, ChromePointer::default());
        let palette = chrome_palette();
        let head = |text: &str| {
            labels
                .iter()
                .find(|label| label.text == text)
                .unwrap_or_else(|| panic!("the strip draws a {text} head"))
        };
        let focused = head("Terminal");
        let resting = head("Preview");
        assert_eq!(
            focused.color, palette.pane_title_focus,
            "the focused pane is the terminal, and it wears `--ink`"
        );
        assert_eq!(
            focused.weight,
            ChromeLabelWeight::Medium,
            "`.pane.focused .panehead {{ font-weight: 500 }}`"
        );
        assert_eq!(resting.color, palette.pane_title);
        assert_eq!(
            resting.weight,
            ChromeLabelWeight::Regular,
            "an unfocused head is the face's own weight — the step is the signal"
        );
    }

    /// PIN (styling pass): a preview state notice is a quiet centred note in the
    /// body — dim ink, horizontally centred, below the title bar — while the
    /// title keeps full-strength ink. The notice is the only centred label, so
    /// the assertions cannot be satisfied by the title or the `x`.
    #[test]
    fn a_preview_state_notice_is_dim_and_centred_in_the_body() {
        let metrics = seat_metrics(1_000);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        let layout = solved(&seats, viewport_of(1600, 900, 1_000), &metrics);
        let (_, labels, sprites) = build_chrome_with_preview(
            &seats,
            &layout,
            1.0,
            ChromePointer::default(),
            None,
            Some("sunset.svg \u{2014} 800\u{d7}600"),
            Some("Loading sunset.svg\u{2026}"),
        );
        // PIN: the preview head wears `#i-file` at `.preview-head .files-ico`'s
        // 14px in `--accent`, and the terminal beside it keeps its profile mark.
        let palette_marks = chrome_palette();
        assert!(
            sprites.iter().any(|sprite| {
                sprite.mark == ChromeMark::File
                    && sprite.color == palette_marks.accent
                    && sprite.rect[2] - sprite.rect[0] == PANE_HEAD_FILE_MARK_LOGICAL_PX
            }),
            "the preview head's mark is the accent file glyph at 14px"
        );
        assert!(
            sprites.iter().any(|sprite| {
                sprite.mark == ChromeMark::ProfilePowerShell
                    && sprite.rect[2] - sprite.rect[0] == PANE_HEAD_PROFILE_MARK_LOGICAL_PX
                    && sprite.rect[1] >= WINDOW_TITLE_BAR_LOGICAL_PX
            }),
            "the terminal head's mark is the profile square at 15px"
        );
        let notice = labels
            .iter()
            .find(|label| label.text == "Loading sunset.svg\u{2026}")
            .expect("the state notice must exist and be the centred label");
        assert_eq!(notice.text, "Loading sunset.svg\u{2026}");
        let palette = chrome_palette();
        assert_eq!(
            notice.color, palette.body_hint_text,
            "quiet ink, not title ink"
        );
        assert!(
            notice.rect[1] >= SEAT_TITLE_BAR_LOGICAL_PX,
            "the notice lives in the body, not the title bar"
        );
        let title = labels
            .iter()
            .find(|label| label.text.starts_with("sunset.svg"))
            .expect("the title carries the file name and dimensions");
        assert_eq!(title.color, palette.pane_title);
        assert!(!title.align_center);
    }

    /// Opening the preview narrows the terminal and closing it hands the pixels
    /// back — bit for bit, because closing promotes the sibling and rebalances
    /// the run it was left in, which for a run of one is the whole slot.
    #[test]
    fn opening_and_closing_the_preview_restores_the_terminals_own_rectangle() {
        let mut seats = Seats::lone_terminal();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1600, 900, 1_000);
        let before = solved(&seats, viewport, &metrics)
            .get(seats.terminal())
            .unwrap()
            .device_rect
            .unwrap();

        assert!(seats.toggle_preview(&metrics), "the preview must open");
        assert!(seats.preview().is_some());
        let narrowed = solved(&seats, viewport, &metrics)
            .get(seats.terminal())
            .unwrap()
            .device_rect
            .unwrap();
        assert!(
            narrowed.width() < before.width(),
            "the fixed right seat narrows the root run — that is its stated cost"
        );
        assert_eq!(
            narrowed.height(),
            before.height(),
            "a row split takes no height"
        );

        assert!(seats.toggle_preview(&metrics), "the preview must close");
        assert!(seats.is_lone_terminal());
        let after = solved(&seats, viewport, &metrics)
            .get(seats.terminal())
            .unwrap()
            .device_rect
            .unwrap();
        assert_eq!(after, before, "closing must return every pixel it borrowed");
    }

    /// A drag hard against either end stops at the two seats' own minimums —
    /// 260 for the terminal and 360 for the preview, which are three
    /// independent lines rather than one line reused (§2.1).
    #[test]
    fn a_divider_drag_stops_at_the_terminal_and_preview_minimums() {
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(800, 600, 1_000);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        let slot = seats.split_slots(&solved(&seats, viewport, &metrics))[0];

        let (requested, usable) = requested_ratio(slot, 1_000_000, -400.0).unwrap();
        seats
            .drag_divider(&metrics, slot.id, requested, usable)
            .expect("dragging to the left edge is feasible, only clamped");
        let terminal = solved(&seats, viewport, &metrics)
            .get(seats.terminal())
            .unwrap()
            .rect
            .unwrap();
        assert_eq!(terminal.extent(Axis::Row).floor_px(), 260);

        let (requested, usable) = requested_ratio(slot, 1_000_000, 1_600.0).unwrap();
        seats
            .drag_divider(&metrics, slot.id, requested, usable)
            .expect("dragging to the right edge is feasible, only clamped");
        let layout = solved(&seats, viewport, &metrics);
        let preview = layout.get(seats.preview().unwrap()).unwrap().rect.unwrap();
        assert_eq!(preview.extent(Axis::Row).floor_px(), 360);
    }

    /// A drag that cannot be satisfied at all is refused outright, with no
    /// ratio written — §2.4's ordering, which this crate consumes rather than
    /// re-derives. 500 logical pixels cannot hold 260 + 1 + 360.
    #[test]
    fn a_divider_drag_in_a_window_too_small_for_both_seats_is_refused() {
        let metrics = seat_metrics(1_000);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        let viewport = viewport_of(1600, 900, 1_000);
        let slot = seats.split_slots(&solved(&seats, viewport, &metrics))[0];
        let before = seats.tree().ratios();
        let cramped = LogicalPx::px(499);
        let result = seats.drag_divider(&metrics, slot.id, Ratio::HALF, cramped);
        assert_eq!(result, Err(EditError::Refused));
        assert_eq!(
            seats.tree().ratios(),
            before,
            "a refusal has zero side effects"
        );
    }

    /// Save, "restart", reload: the tree comes back and solves to the same
    /// rectangles. What crossed the disk was intent — shape, direction, and a
    /// `u32` ppm ratio — and never a rectangle (red line L11).
    #[test]
    fn a_saved_seat_tree_reloads_to_the_same_rects() {
        let dpi_milli = 1_250;
        let metrics = seat_metrics(dpi_milli);
        let viewport = viewport_of(1600, 900, dpi_milli);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        // Drag to a ratio that is nobody's default, so a round trip cannot pass
        // by landing back on a constant.
        let slot = seats.split_slots(&solved(&seats, viewport, &metrics))[0];
        let (requested, usable) = requested_ratio(slot, scale_ppm(dpi_milli), 913.0).unwrap();
        seats
            .drag_divider(&metrics, slot.id, requested, usable)
            .unwrap();
        let ratio_before = seats.tree().ratios();
        assert_ne!(
            ratio_before[0].1,
            Ratio::HALF,
            "the drag must have moved it"
        );
        let before = solved(&seats, viewport, &metrics).logical_rects();

        let session = SessionV1 {
            schema_version: SESSION_SCHEMA_VERSION,
            tabs: vec![TabV1 {
                root: seats.to_persisted(&TermLeafV1 {
                    profile_id: "pwsh".to_owned(),
                    cwd: String::new(),
                    manual_name: None,
                }),
                pinned: false,
                focused_leaf: "leaf-0".to_owned(),
            }],
            ..SessionV1::default()
        };
        let dir = std::env::temp_dir().join(format!("bt-app-seats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        write_session_atomic(&path, &session).unwrap();

        let (loaded, _report, degradation) = read_session(&path);
        assert!(
            degradation.is_clean(),
            "a tree this build wrote must reload without degrading"
        );
        let restored = Seats::from_persisted(&loaded.tabs[0].root)
            .expect("the reloaded tree still holds a terminal");
        assert_eq!(
            restored.tree().ratios().len(),
            ratio_before.len(),
            "the shape survived"
        );
        assert_eq!(
            restored.tree().ratios()[0].1.ppm(),
            ratio_before[0].1.ppm(),
            "the ppm ratio crossed the disk unchanged — a float or a rounded \
             decimal would land somewhere near here instead of here"
        );
        assert_eq!(
            solved(&restored, viewport, &metrics).logical_rects(),
            before,
            "the same intent solves to the same picture"
        );
        assert!(
            restored.preview().is_some(),
            "the preview seat is a seat again"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A window too narrow for both seats collapses the non-focus one into a
    /// clickable bar that keeps its place in the tree (§2.6.3) — and the bar is
    /// real chrome with a real hit target, not an absence.
    ///
    /// Opening the preview must be indistinguishable, downstream, from a window
    /// narrowing that leaves the terminal the same rectangle.
    ///
    /// Everything the resize machinery is handed — the grid, and the pixel size
    /// ConPTY is told — is a function of the terminal seat's rectangle and
    /// nothing else (`grid_for_pixels(seat)`, `terminal_pty_physical(seat)`), so
    /// the pin is that the rectangle a preview leaves the terminal is *exactly*
    /// the rectangle a lone leaf gets from a window of that size. If that holds
    /// at every DPI, the two paths cannot hand the coalescer different numbers,
    /// and there is no seat-shaped resize for anything downstream to notice.
    ///
    /// Red gate: derive the second rectangle from the *window* rather than from
    /// the seat — the discrepancy the bug report described — and the equality
    /// fails at every DPI.
    #[test]
    fn opening_the_preview_hands_the_resize_machinery_a_window_narrowings_rectangle() {
        for dpi_milli in [1_000u32, 1_250, 1_500, 1_750, 2_000, 2_500] {
            let metrics = seat_metrics(dpi_milli);
            let (width, height) = (1920u32, 1200u32);

            let mut seats = Seats::lone_terminal();
            assert!(seats.toggle_preview(&metrics), "the preview must open");
            let opened = seat_viewport(
                &solved(&seats, viewport_of(width, height, dpi_milli), &metrics),
                seats.terminal(),
            )
            .expect("the terminal keeps a rectangle");
            assert!(
                opened.width < width,
                "the preview must actually cost the terminal width at {dpi_milli} milli-DPI"
            );

            // The same rectangle, reached by narrowing the window instead.
            let narrowed = seat_viewport(
                &solved(
                    &Seats::lone_terminal(),
                    viewport_of(opened.width, height, dpi_milli),
                    &metrics,
                ),
                SeatId(1),
            )
            .expect("a lone leaf keeps a rectangle");
            let expected = seats_surface(opened.width, height, dpi_milli);
            assert_eq!(narrowed, expected);
            assert_eq!(
                (narrowed.width, narrowed.height),
                (opened.width, opened.height),
                "the two paths must present the same extent to `grid_for_pixels` \
                 and to the ConPTY pixel size at {dpi_milli} milli-DPI"
            );
            // Red gate: the window's extent is not that extent, so an
            // implementation that read it would be caught here.
            assert_ne!(
                (width, height),
                (opened.width, opened.height),
                "the pin would pass even if the grid were derived from the window"
            );

            // Closing hands every pixel back, so the sequence is symmetric.
            assert!(seats.toggle_preview(&metrics), "the preview must close");
            let closed = seat_viewport(
                &solved(&seats, viewport_of(width, height, dpi_milli), &metrics),
                seats.terminal(),
            )
            .expect("the terminal keeps a rectangle");
            assert_eq!(closed, seats_surface(width, height, dpi_milli));
        }
    }

    /// This state is deliberately hard to reach by dragging the window, because
    /// §2.6.5's minimum inner size stops the OS well above it; that is the
    /// concession chain being a degradation path rather than an everyday one.
    /// It is reachable when the OS ignores the hint (tiny-window §4.3), so it is
    /// pinned here rather than left to a gesture nobody can perform.
    #[test]
    fn a_squeezed_seat_becomes_a_clickable_bar_that_keeps_its_place() {
        let metrics = seat_metrics(1_000);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        // 260 + 1 + 360 needs 621; 500 does not have it, so the non-focus seat
        // collapses and the focus seat is the last to fall.
        let viewport = viewport_of(500, 600, 1_000);
        let layout = seats
            .solve(viewport, &metrics)
            .expect("L3 must satisfy 500px");
        let preview = seats.preview().unwrap();
        assert!(
            matches!(
                layout.get(preview).unwrap().presentation,
                Presentation::Collapsed(_)
            ),
            "the non-focus seat is the one that gives way"
        );
        assert_eq!(
            layout.get(seats.terminal()).unwrap().presentation,
            Presentation::Full,
            "W2: the focus seat falls last"
        );
        assert_eq!(
            layout.get(preview).unwrap().rect.unwrap().extent(Axis::Row),
            bt_layout::COLLAPSED_EXTENT
        );

        let (quads, _labels, _sprites) =
            build_chrome(&seats, &layout, 1.0, ChromePointer::default());
        assert!(
            quads
                .iter()
                .any(|quad| quad.color == chrome_palette().collapse_bar),
            "a collapsed seat is drawn as a bar"
        );
        let bar = layout.get(preview).unwrap().device_rect.unwrap();
        assert_eq!(
            hit_chrome(
                &seats,
                &layout,
                1.0,
                f64::from((bar.left + bar.right) as i32) / 2.0,
                f64::from((bar.top + bar.bottom) as i32) / 2.0,
            ),
            Some(ChromeTarget::CollapseBar(preview)),
            "the whole strip is clickable"
        );
        assert!(
            seats.tree().find_seat(preview).is_some(),
            "W1: collapsing is a presentation, never an edit"
        );
    }

    /// PIN (modal): with the settings dialog up, the three things a press in
    /// this window can otherwise reach — a divider, another seat's bar, the
    /// terminal's own rectangle — are all the scrim's.
    ///
    /// Red gate: each point is first shown to be the thing it claims to be, by
    /// the very functions the router consults (`hit_chrome`, `terminal_contains`).
    /// Without that half the assertions would pass over any three points at all,
    /// including three that are inside the dialog.
    #[test]
    fn a_modal_swallows_the_divider_the_seat_and_the_terminal_under_it() {
        let metrics = seat_metrics(1_000);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        let (width, height) = (1_280u32, 800u32);
        let layout = solved(&seats, viewport_of(width, height, 1_000), &metrics);
        let overlay = crate::settings::layout_for_menu(width as f32, height as f32, 1.0, None)
            .expect("this window hosts the dialog");

        let slot = seats
            .split_slots(&layout)
            .into_iter()
            .next()
            .expect("two seats have a divider between them");
        let divider = (
            f64::from((slot.band[0] + slot.band[2]) / 2.0),
            f64::from((slot.band[1] + slot.band[3]) / 2.0),
        );
        let preview = seats.preview().expect("the preview seat is open");
        let head = layout.get(preview).unwrap().device_rect.unwrap();
        let seat = (
            f64::from((head.left + head.right) as i32) / 2.0,
            f64::from(head.top as i32) + 8.0,
        );
        let terminal_rect = layout
            .get(seats.terminal())
            .unwrap()
            .device_rect
            .expect("the terminal seat has a rectangle");
        let terminal = (
            f64::from((terminal_rect.left + terminal_rect.right) as i32) / 2.0,
            f64::from(terminal_rect.bottom as i32) - 8.0,
        );

        assert_eq!(
            hit_chrome(&seats, &layout, 1.0, divider.0, divider.1),
            Some(ChromeTarget::Divider(slot.id)),
            "the first point really is a divider"
        );
        assert_eq!(
            hit_chrome(&seats, &layout, 1.0, seat.0, seat.1),
            Some(ChromeTarget::PaneHeader(preview)),
            "the second point really is another seat's head"
        );
        assert!(
            terminal_contains(&layout, seats.terminal(), terminal.0, terminal.1),
            "the third point really is inside the terminal"
        );

        for (what, (x, y)) in [
            ("the divider", divider),
            ("the seat head", seat),
            ("the terminal", terminal),
        ] {
            assert_eq!(
                crate::settings::hit(&overlay, x, y),
                crate::settings::SettingsTarget::Scrim,
                "a modal means MODAL: {what} is behind the scrim"
            );
        }
    }

    /// An unknown leaf kind stays visible as a placeholder rather than being
    /// quietly turned into a terminal (§5 constraint 2).
    #[test]
    fn an_unknown_persisted_leaf_reloads_as_a_visible_placeholder() {
        let node = LayoutNodeV1::Split(SplitNodeV1 {
            dir: SplitDirV1::Row,
            ratio: 600_000,
            children: [
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                    profile_id: "pwsh.exe".to_owned(),
                    cwd: String::new(),
                    manual_name: None,
                }))),
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Unknown)),
            ],
        });
        let seats = Seats::from_persisted(&node).unwrap();
        let kinds: Vec<_> = seats
            .tree()
            .seats_in_order()
            .into_iter()
            .map(|seat| seat.kind)
            .collect();
        assert_eq!(kinds, vec![SeatKind::Terminal, SeatKind::Placeholder]);
    }

    /// The chrome at one DPI, with a preview open so the terminal wears a head.
    fn chrome_at(dpi_milli: u32) -> (Vec<ChromeLabel>, Vec<ChromeSprite>) {
        let metrics = seat_metrics(dpi_milli);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        let layout = solved(&seats, viewport_of(1600, 900, dpi_milli), &metrics);
        let (_, labels, sprites) = build_chrome_with_preview(
            &seats,
            &layout,
            dpi_milli as f32 / 1000.0,
            ChromePointer::default(),
            None,
            None,
            None,
        );
        (labels, sprites)
    }

    fn vertical_centre(rect: [f32; 4]) -> f32 {
        (rect[1] + rect[3]) / 2.0
    }

    /// PIN — the active tab's mark and its title hang off one axis.
    ///
    /// `.tab { display: flex; align-items: center }` in the mock-up: the mark box
    /// and the title share the tab's own centre line. Both are placed from that
    /// centre with the *same* rounding — the mark's box is rounded to whole
    /// physical pixels so its texture lands on the grid, and the title's rect is
    /// the tab itself — so no DPI can open a step between them by rounding one
    /// down and the other up.
    #[test]
    fn tab_mark_and_title_share_one_vertical_axis() {
        for dpi_milli in [1_000u32, 1_250, 1_500, 2_000] {
            let (labels, sprites) = chrome_at(dpi_milli);
            let title_bar = WINDOW_TITLE_BAR_LOGICAL_PX * dpi_milli as f32 / 1000.0;
            let mark = sprites
                .iter()
                .find(|sprite| {
                    sprite.mark == ChromeMark::ProfilePowerShell && sprite.rect[3] <= title_bar
                })
                .expect("the active tab wears the session's profile mark");
            let title = labels
                .iter()
                .find(|label| label.text == "PowerShell")
                .expect("the active tab carries its session's name");
            let delta = vertical_centre(mark.rect) - vertical_centre(title.rect);
            assert!(
                delta.abs() <= 0.5,
                "tab mark and title off one axis by {delta} physical px at {dpi_milli} milli-DPI \
                 (mark {:?}, title {:?})",
                mark.rect,
                title.rect
            );
        }
    }

    #[test]
    fn tab_title_uses_session_text_or_the_profile_fallback() {
        let metrics = seat_metrics(1_000);
        let seats = Seats::lone_terminal();
        let layout = solved(&seats, viewport_of(1600, 900, 1_000), &metrics);

        let (_, titled, _) = build_chrome_with_preview(
            &seats,
            &layout,
            1.0,
            ChromePointer::default(),
            Some("Claude ✳ 任务"),
            None,
            None,
        );
        assert!(titled.iter().any(|label| label.text == "Claude ✳ 任务"));

        let (_, fallback, _) = build_chrome_with_preview(
            &seats,
            &layout,
            1.0,
            ChromePointer::default(),
            None,
            None,
            None,
        );
        assert!(fallback.iter().any(|label| label.text == "PowerShell"));
    }

    /// The caption run's boxes and its hit test are one arithmetic, and this is
    /// what says so: every box answers with its own target when asked at its
    /// centre, and the run tiles the corner with no seam between the buttons.
    ///
    /// The tooltip anchors on these boxes; a second `width - 4 * button` would
    /// stay right until the day it did not, and the symptom would be a tip
    /// labelled "Maximize" standing under the close button.
    #[test]
    fn the_caption_boxes_are_the_boxes_the_caption_hit_test_answers_from() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let width = 960.0 * scale;
            let boxes = window_chrome_boxes(width, scale);
            assert_eq!(
                boxes.map(|(target, _)| target),
                [
                    ChromeTarget::Settings,
                    ChromeTarget::Minimize,
                    ChromeTarget::Maximize,
                    ChromeTarget::CloseWindow,
                ],
                "the gear leads the run"
            );
            for (target, rect) in boxes {
                assert_eq!(
                    hit_window_chrome(
                        width,
                        scale,
                        f64::from((rect[0] + rect[2]) / 2.0),
                        f64::from((rect[1] + rect[3]) / 2.0),
                    ),
                    Some(target),
                    "{target:?} at {scale}x"
                );
            }
            // No seam: each box begins where the last one ended, and the run
            // finishes hard against the window's own edge — the corner pixel
            // belongs to `close` at every scale.
            for pair in boxes.windows(2) {
                assert!((pair[0].1[2] - pair[1].1[0]).abs() < 1e-4, "{pair:?}");
            }
            assert!((boxes[3].1[2] - width).abs() < 1e-4);
            assert_eq!(
                hit_window_chrome(width, scale, f64::from(width) - 0.5, 1.0),
                Some(ChromeTarget::CloseWindow)
            );
            // And nothing to the left of the run is the run's.
            assert_eq!(
                hit_window_chrome(width, scale, f64::from(boxes[0].1[0]) - 1.0, 1.0),
                None
            );
        }
    }

    /// The box the tooltip anchors its `NN%` on is the box the strip draws the
    /// mark in — read off the sprite the strip actually emitted, not recomputed.
    #[test]
    fn the_mark_box_the_tooltip_anchors_on_is_the_box_the_strip_draws_in() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dpi_milli = (scale * 1_000.0) as u32;
            let metrics = seat_metrics(dpi_milli);
            let seats = Seats::lone_terminal();
            let layout = solved(
                &seats,
                viewport_of((960.0 * scale) as u32, (600.0 * scale) as u32, dpi_milli),
                &metrics,
            );
            let tabs = plain_tabs(3);
            let (_, _, sprites) = build_chrome_for_tabs(
                &seats,
                &layout,
                scale,
                ChromePointer {
                    hover: None,
                    dragging: None,
                    ..ChromePointer::default()
                },
                ChromeContent {
                    tabs: &tabs,
                    active_tab: 0,
                    grabbed: None,
                    tab_scroll: 0.0,
                    preview_title: None,
                    terminal_cwd: None,
                    preview_message: None,
                    fit_overflow: None,
                    profile_menu_open: false,
                    chevron_turn: 0.0,
                },
            );
            let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(3), 0, 0.0);
            // Compared as sets, because the strip paints the quiet tabs first
            // and the active one last (`.tab.active { z-index: 1 }`), so draw
            // order is deliberately not strip order.
            let mut anchored: Vec<[f32; 4]> = geometry
                .tabs
                .iter()
                .map(|tab| tab_mark_box(tab, scale))
                .collect();
            let mut drawn: Vec<[f32; 4]> = sprites
                .iter()
                .filter(|sprite| matches!(sprite.mark, ChromeMark::ProfilePowerShell))
                .map(|sprite| sprite.rect)
                .collect();
            let by_left =
                |a: &[f32; 4], b: &[f32; 4]| a[0].partial_cmp(&b[0]).expect("finite lefts");
            anchored.sort_by(by_left);
            drawn.sort_by(by_left);
            assert_eq!(anchored, drawn, "at {scale}x");
            // And each is square and inside the tab it belongs to.
            for (tab, mark) in geometry.tabs.iter().zip(&anchored) {
                assert!((mark[2] - mark[0] - (mark[3] - mark[1])).abs() < 1e-4);
                assert!(mark[0] >= tab.body[0]);
                assert!(mark[2] <= tab.body[2]);
            }
        }
    }

    #[test]
    fn multi_tab_strip_is_equal_width_and_exposes_plus_close_and_middle_click_targets() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(4), 2, 0.0);
            assert_eq!(geometry.tabs.len(), 4);
            let widths = geometry
                .tabs
                .iter()
                .map(|tab| tab.body[2] - tab.body[0])
                .collect::<Vec<_>>();
            assert!(
                widths
                    .windows(2)
                    .all(|pair| (pair[0] - pair[1]).abs() < 0.01)
            );
            assert!(widths[0] <= WINDOW_TAB_MAX_WIDTH_LOGICAL_PX * scale);

            let plus = geometry.new_tab;
            assert_eq!(
                hit_tab_chrome(
                    960.0 * scale,
                    scale,
                    &resting(4),
                    2,
                    0.0,
                    f64::from((plus[0] + plus[2]) / 2.0),
                    f64::from((plus[1] + plus[3]) / 2.0),
                ),
                Some(ChromeTarget::NewTab)
            );
            let close = geometry.tabs[2].close.expect("the active tab keeps its ×");
            assert_eq!(
                hit_tab_chrome(
                    960.0 * scale,
                    scale,
                    &resting(4),
                    2,
                    0.0,
                    f64::from((close[0] + close[2]) / 2.0),
                    f64::from((close[1] + close[3]) / 2.0),
                ),
                Some(ChromeTarget::TabClose(2))
            );
            let body = geometry.tabs[1].body;
            assert_eq!(
                hit_tab_chrome(
                    960.0 * scale,
                    scale,
                    &resting(4),
                    2,
                    0.0,
                    f64::from(body[0] + 2.0 * scale),
                    f64::from((body[1] + body[3]) / 2.0),
                ),
                Some(ChromeTarget::Tab(1)),
                "the tab body is the target consumed by middle-click close"
            );
        }
    }

    #[test]
    fn tab_count_layout_changes_publish_a_new_strip_right_edge() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let width = 960.0 * scale;
            let one = tab_strip_right_px(width, scale, 1);
            let two = tab_strip_right_px(width, scale, 2);
            assert!(
                two > one,
                "adding a tab must move the edge at scale {scale}"
            );
            assert_eq!(
                one,
                tab_strip_geometry(width, scale, &resting(1), 0, 0.0).new_tab_menu[2].ceil() as i32,
                "the published edge includes both end buttons at scale {scale}"
            );
        }
    }

    /// The strip at one scale, with `hover` wherever the caller says.
    fn strip_chrome(
        scale: f32,
        titles: &[String],
        active_tab: usize,
        hover: Option<ChromeTarget>,
        profile_menu_open: bool,
    ) -> (Vec<ChromeQuad>, Vec<ChromeLabel>, Vec<ChromeSprite>) {
        let tabs = titles
            .iter()
            .map(|title| TabContent {
                title: title.clone(),
                pane_count: 1,
                badge_text_width: 0.0,
                mark: TabMarkState::default(),
                trailer: TabTrailer::default(),
                offset: 0.0,
                landing: 0.0,
                edit: None,
            })
            .collect::<Vec<_>>();
        strip_chrome_of(scale, &tabs, active_tab, 0.0, hover, profile_menu_open)
    }

    /// The strip at one scale, told exactly what each tab holds and how far it
    /// is scrolled, with the chevron settled wherever its menu is.
    ///
    /// Settled is the right default for every caller but the turn's own tests:
    /// 140ms after the click the arrow has arrived, and a test that is not
    /// about the transition is asking about a strip at rest.
    fn strip_chrome_of(
        scale: f32,
        tabs: &[TabContent],
        active_tab: usize,
        tab_scroll: f32,
        hover: Option<ChromeTarget>,
        profile_menu_open: bool,
    ) -> (Vec<ChromeQuad>, Vec<ChromeLabel>, Vec<ChromeSprite>) {
        strip_chrome_of_turn(
            scale,
            tabs,
            active_tab,
            tab_scroll,
            hover,
            profile_menu_open,
            f32::from(u8::from(profile_menu_open)),
        )
    }

    /// The same, with the arrow caught partway through its turn.
    fn strip_chrome_of_turn(
        scale: f32,
        tabs: &[TabContent],
        active_tab: usize,
        tab_scroll: f32,
        hover: Option<ChromeTarget>,
        profile_menu_open: bool,
        chevron_turn: f32,
    ) -> (Vec<ChromeQuad>, Vec<ChromeLabel>, Vec<ChromeSprite>) {
        // A 960x600 *logical* window, so the strip's own geometry can be
        // restated at the same width the builder will see.
        let dpi_milli = (scale * 1_000.0) as u32;
        let metrics = seat_metrics(dpi_milli);
        let seats = Seats::lone_terminal();
        let layout = solved(
            &seats,
            viewport_of((960.0 * scale) as u32, (600.0 * scale) as u32, dpi_milli),
            &metrics,
        );
        build_chrome_for_tabs(
            &seats,
            &layout,
            scale,
            ChromePointer {
                hover,
                ..ChromePointer::default()
            },
            ChromeContent {
                tabs,
                active_tab,
                grabbed: None,
                tab_scroll,
                preview_title: None,
                terminal_cwd: None,
                preview_message: None,
                fit_overflow: None,
                profile_menu_open,
                chevron_turn,
            },
        )
    }

    fn strip_titles(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("tab {index}")).collect()
    }

    #[test]
    fn inactive_hover_and_new_tab_button_use_the_mockup_shapes() {
        let titles = strip_titles(2);
        let (_, _, hover_sprites) =
            strip_chrome(1.0, &titles, 0, Some(ChromeTarget::Tab(1)), false);
        assert!(
            hover_sprites
                .iter()
                .any(|sprite| matches!(sprite.mark, ChromeMark::TabBody { radius_px: 7 }))
        );
        assert!(
            hover_sprites
                .iter()
                .any(|sprite| sprite.mark == ChromeMark::Plus)
        );
    }

    /// Do two boxes share any area at all — the question that decides whether a
    /// claim about paint order is about anything.
    fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
        a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
    }

    /// PIN (tab-strip layering) — `.tab.active { z-index: 1 }` (mock-up line
    /// 216) together with the concave-corner pair on lines 220-229, and the
    /// mock-up's own note above them: *"The active tab paints above its
    /// neighbours, so its concave corners simply cover their hover fill. The
    /// earlier problem was the opposite: neighbours come later in the DOM, so
    /// their fill was painting over the corner and slicing it."*
    ///
    /// The active tab's two skirt corners overhang its neighbours by `--tabr`,
    /// and what makes them read as a *join* with the content plane rather than a
    /// notch is that they land over whatever the neighbour laid down there. A
    /// neighbour under the pointer fills its whole box with `--hover`; when that
    /// fill arrives after the silhouette, the square corner of a rectangle eats
    /// the arc and the active tab stops meeting the terminal below it.
    ///
    /// Chrome sprites are painted in vector order — `bt-render` issues one draw
    /// per icon, in order, with no sort and no depth buffer — so "over" is
    /// literally "later in `sprites`".
    ///
    /// Red gate: each ordering claim is bracketed by an overlap check, so it is
    /// only ever made about a corner the neighbour genuinely collides with;
    /// against a builder that emits tabs strictly left to right, the *right*
    /// hand neighbour's fill is pushed after the silhouette and this fails.
    #[test]
    fn the_active_tab_s_concave_corners_paint_over_a_neighbour_s_hover_fill() {
        let palette = chrome_palette();
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let titles = strip_titles(3);
            let radius = (WINDOW_TAB_RADIUS_LOGICAL_PX * scale).round().max(1.0);
            let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(3), 1, 0.0);
            let body = geometry.tabs[1].body;
            // The two 7x7 boxes the `::before`/`::after` pair occupies: one
            // `--tabr` outside each edge of the active tab, sitting on its foot.
            for (neighbour, side, corner) in [
                (
                    0_usize,
                    "left",
                    [body[0] - radius, body[3] - radius, body[0], body[3]],
                ),
                (
                    2_usize,
                    "right",
                    [body[2], body[3] - radius, body[2] + radius, body[3]],
                ),
            ] {
                let (_, _, sprites) =
                    strip_chrome(scale, &titles, 1, Some(ChromeTarget::Tab(neighbour)), false);
                let silhouette = sprites
                    .iter()
                    .position(|sprite| matches!(sprite.mark, ChromeMark::ActiveTab { .. }))
                    .unwrap_or_else(|| panic!("scale {scale}: no active silhouette"));
                let fill = sprites
                    .iter()
                    .position(|sprite| matches!(sprite.mark, ChromeMark::TabBody { .. }))
                    .unwrap_or_else(|| {
                        panic!("scale {scale}: the {side} neighbour paints no hover fill")
                    });
                assert_eq!(
                    sprites[fill].rect, geometry.tabs[neighbour].body,
                    "scale {scale}: the fill under test is the {side} neighbour's own box"
                );
                assert_eq!(
                    sprites[fill].color, palette.caption_hover,
                    "scale {scale}: and it is `--hover`"
                );

                // The claim below is only worth making where the two collide.
                assert!(
                    overlaps(sprites[fill].rect, corner),
                    "scale {scale}: the {side} neighbour's hover fill must reach into the \
                     active tab's concave corner, or this test pins nothing"
                );
                assert!(
                    sprites[silhouette].rect[0] <= corner[0]
                        && sprites[silhouette].rect[2] >= corner[2]
                        && sprites[silhouette].rect[3] == corner[3],
                    "scale {scale}: the silhouette's skirt is what carries the {side} corner"
                );
                assert!(
                    silhouette > fill,
                    "scale {scale}: `.tab.active {{ z-index: 1 }}` — the active tab's {side} \
                     concave corner must paint AFTER the neighbour's `--hover` fill, or the \
                     rectangle squares off the arc that joins the tab to the content plane \
                     (silhouette at sprite {silhouette}, fill at sprite {fill})"
                );
            }
        }
    }

    /// PIN — the layering rule is local to the join. A tab that is not beside
    /// the active one shares no pixel with its skirt, so hovering it must leave
    /// the strip exactly as it was: the same `--hover` fill, in its own box, at
    /// `--tabr`, and no collision with the silhouette for paint order to settle.
    ///
    /// The active tab here is the first one, whose left skirt lands on the
    /// window edge (the mock-up's A3 join) — so this also holds that edge whole
    /// while the corner rule is being taught.
    #[test]
    fn hovering_a_tab_away_from_the_active_one_leaves_the_skirts_alone() {
        let palette = chrome_palette();
        for scale in [1.0_f32, 1.5, 2.0] {
            let titles = strip_titles(4);
            let radius = (WINDOW_TAB_RADIUS_LOGICAL_PX * scale).round().max(1.0);
            let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(4), 0, 0.0);
            let (_, _, sprites) =
                strip_chrome(scale, &titles, 0, Some(ChromeTarget::Tab(2)), false);
            let silhouette = sprites
                .iter()
                .find(|sprite| matches!(sprite.mark, ChromeMark::ActiveTab { .. }))
                .unwrap_or_else(|| panic!("scale {scale}: no active silhouette"));
            let fill = sprites
                .iter()
                .find(|sprite| matches!(sprite.mark, ChromeMark::TabBody { .. }))
                .unwrap_or_else(|| panic!("scale {scale}: the hovered tab paints no fill"));

            assert_eq!(
                fill.mark,
                ChromeMark::TabBody {
                    radius_px: radius as u32
                },
                "scale {scale}: an unrelated hover is still `border-radius: 7px 7px 0 0`"
            );
            assert_eq!(
                fill.rect, geometry.tabs[2].body,
                "scale {scale}: and still exactly its own box"
            );
            assert_eq!(
                fill.color, palette.caption_hover,
                "scale {scale}: and still `--hover`"
            );
            assert_eq!(
                silhouette.rect[0], 0.0,
                "scale {scale}: the first tab's skirt still lands on the window edge"
            );
            assert!(
                !overlaps(fill.rect, silhouette.rect),
                "scale {scale}: a tab two places along touches no skirt, so there is \
                 nothing here for paint order to decide"
            );
        }
    }

    /// PIN (`+` fidelity) — the new-tab button has **no fill at rest** and a
    /// **rounded** one under the pointer, on both palettes.
    ///
    /// `.newtab { background: none; border-radius: 6px }` and
    /// `.newtab:hover { background: var(--hover) }`. Both halves are the report:
    /// the resting button was drawing nothing already, and the hover fill was a
    /// `ChromeQuad` — a rectangle, and rectangles have no radius — which is why
    /// it read as a hard grey block pressed against the tab's own round.
    ///
    /// Red gate: `mark: ChromeMark::ControlPill { .. }` is exactly what a quad
    /// cannot be, so the shape claim fails against the old drawing; and the
    /// resting assertion fails against any drawing that fills the box at rest.
    #[test]
    fn the_new_tab_button_is_bare_at_rest_and_rounds_its_hover_fill() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let titles = strip_titles(1);
            let radius = (WINDOW_NEW_TAB_RADIUS_LOGICAL_PX * scale).round() as u32;
            let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(1), 0, 0.0);
            for (rest_hover, hovered_target, box_rect) in [
                (None, ChromeTarget::NewTab, geometry.new_tab),
                (None, ChromeTarget::NewTabMenu, geometry.new_tab_menu),
            ] {
                let (rest_quads, _, rest_sprites) =
                    strip_chrome(scale, &titles, 0, rest_hover, false);
                assert!(
                    !rest_quads
                        .iter()
                        .any(|quad| quad.rect == pixel_snapped(box_rect)),
                    "scale {scale}: `background: none` — a resting {hovered_target:?} paints no fill"
                );
                assert!(
                    !rest_sprites
                        .iter()
                        .any(|sprite| matches!(sprite.mark, ChromeMark::ControlPill { .. })),
                    "scale {scale}: nothing in the strip wears a pill until the pointer arrives"
                );

                let (hover_quads, _, hover_sprites) =
                    strip_chrome(scale, &titles, 0, Some(hovered_target), false);
                let pill = hover_sprites
                    .iter()
                    .find(|sprite| sprite.rect == pixel_snapped(box_rect))
                    .unwrap_or_else(|| {
                        panic!("scale {scale}: {hovered_target:?} must fill its box on hover")
                    });
                assert_eq!(
                    pill.mark,
                    ChromeMark::ControlPill { radius_px: radius },
                    "scale {scale}: `.newtab` rounds at 6px"
                );
                assert_eq!(
                    pill.color,
                    chrome_palette().caption_hover,
                    "scale {scale}: and the fill is `--hover`"
                );
                assert!(
                    !hover_quads
                        .iter()
                        .any(|quad| quad.rect == pixel_snapped(box_rect)),
                    "scale {scale}: a rectangle is what this pass deletes"
                );
            }
        }
    }

    /// PIN — the `+` and the `˅` are one family: `--ink3` at rest, `--ink` under
    /// the pointer, and the same 28px box side by side with no margin between.
    #[test]
    fn the_strip_s_two_end_buttons_share_one_box_and_one_ink() {
        let palette = chrome_palette();
        for scale in [1.0_f32, 1.25, 2.0] {
            let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(1), 0, 0.0);
            let box_side = WINDOW_NEW_TAB_BOX_LOGICAL_PX * scale;
            for rect in [geometry.new_tab, geometry.new_tab_menu] {
                assert!((rect[2] - rect[0] - box_side).abs() < 0.01);
                assert!((rect[3] - rect[1] - box_side).abs() < 0.01);
            }
            assert_eq!(
                geometry.new_tab_menu[0], geometry.new_tab[2],
                "scale {scale}: `.tabs-inline .chevbtn {{ margin-left: 0 }}`"
            );
            assert_eq!(geometry.new_tab_menu[1], geometry.new_tab[1]);

            let titles = strip_titles(1);
            let (_, _, rest) = strip_chrome(scale, &titles, 0, None, false);
            for mark in [ChromeMark::Plus, ChromeMark::chevron(0.0)] {
                let sprite = rest
                    .iter()
                    .find(|sprite| sprite.mark == mark)
                    .unwrap_or_else(|| panic!("scale {scale}: {mark:?} is always drawn"));
                assert_eq!(
                    sprite.color, palette.title_text_muted,
                    "scale {scale}: `.newtab {{ color: var(--ink3) }}`"
                );
            }
            let (_, _, hovered) =
                strip_chrome(scale, &titles, 0, Some(ChromeTarget::NewTab), false);
            assert_eq!(
                hovered
                    .iter()
                    .find(|sprite| sprite.mark == ChromeMark::Plus)
                    .expect("the + is drawn")
                    .color,
                palette.title_text_hover,
                "scale {scale}: `.newtab:hover {{ color: var(--ink) }}`"
            );
        }
    }

    /// PIN — the chevron turns over when its list is on screen, and it is the
    /// same arrow that turned: one glyph, two orientations, no second symbol.
    #[test]
    fn the_profile_chevron_turns_over_while_its_menu_is_up() {
        let titles = strip_titles(1);
        let (_, _, shut) = strip_chrome(1.0, &titles, 0, None, false);
        let (_, _, open) = strip_chrome(1.0, &titles, 0, None, true);
        let shut_chevron = shut
            .iter()
            .find(|sprite| matches!(sprite.mark, ChromeMark::Chevron { .. }))
            .expect("the strip carries a profile chevron");
        let open_chevron = open
            .iter()
            .find(|sprite| matches!(sprite.mark, ChromeMark::Chevron { .. }))
            .expect("and it is still there while the menu is up");
        assert_eq!(shut_chevron.mark, ChromeMark::chevron(0.0));
        assert_eq!(open_chevron.mark, ChromeMark::chevron(1.0));
        assert_eq!(
            shut_chevron.rect, open_chevron.rect,
            "the button does not move when its menu opens"
        );
        assert_eq!(
            open_chevron.color,
            chrome_palette().title_text_hover,
            "a control with its menu open is not resting"
        );
    }

    /// PIN — the 140ms between the two ends is drawn, and drawing it does not
    /// disturb the strip.
    ///
    /// `.chevbtn svg { transition: transform 140ms cubic-bezier(.2,0,0,1) }`
    /// (mock-up 415-418). The turn is a `transform`, and a CSS transform is not
    /// layout: the button keeps its 9x6 box at every angle, the `+` beside it
    /// does not shuffle, and the room the rotated arrow needs is taken by the
    /// rasterizer outside that box rather than by the strip.
    ///
    /// The other half is that the arrow and its menu are on different clocks.
    /// The list is up the instant the button is clicked and the ink follows it
    /// at once — `.newtab` has no transition on `color` — while the arrow is
    /// still on its way. A build that derived one from the other would have to
    /// either snap the arrow or stall the menu.
    #[test]
    fn the_chevron_is_drawn_partway_through_its_turn() {
        let titles = strip_titles(1);
        let tabs = [TabContent {
            title: titles[0].clone(),
            pane_count: 1,
            ..TabContent::default()
        }];
        let chevron_of = |turn: f32, open: bool| {
            let (_, _, sprites) = strip_chrome_of_turn(1.0, &tabs, 0, 0.0, None, open, turn);
            *sprites
                .iter()
                .find(|sprite| matches!(sprite.mark, ChromeMark::Chevron { .. }))
                .expect("the strip carries a profile chevron")
        };
        let resting = chevron_of(0.0, false);
        let mut seen = std::collections::HashSet::new();
        for step in 0_u8..=20 {
            let turn = f32::from(step) / 20.0;
            let midway = chevron_of(turn, true);
            assert_eq!(
                midway.rect, resting.rect,
                "the `.chevbtn` box is the same 9x6 at {turn} of the turn —                  a transform does not touch layout"
            );
            let ChromeMark::Chevron { turned_degrees } = midway.mark else {
                panic!("the strip draws a chevron");
            };
            seen.insert(turned_degrees);
        }
        assert!(
            seen.len() >= 12,
            "twenty samples of the turn produced {} angles — the strip is not drawing              the transition, only its two ends",
            seen.len()
        );

        // The ink is the menu's, and the angle is the arrow's own.
        let palette = chrome_palette();
        assert_eq!(
            chevron_of(0.0, true).color,
            palette.title_text_hover,
            "the list is already up, whatever the arrow is still doing"
        );
        assert_eq!(
            chevron_of(1.0, false).color,
            palette.title_text_muted,
            "and already down"
        );
        assert_eq!(
            chevron_of(1.0, false).mark,
            ChromeMark::chevron(1.0),
            "an arrow that has not started back is still over"
        );
    }

    /// PIN (`×` display rule) — the mock-up's `×` is **always shown**, and its
    /// only conditional is the measured width tier:
    /// `.tab.tight:not(.active) .close { display: none }` at 140px, and the same
    /// again for `.squeezed` at 90px, where the tab becomes its centred mark.
    ///
    /// The rule is a *hit-testing* claim as much as a drawing one: an affordance
    /// that is not drawn must not be pressable, or a narrow strip closes tabs
    /// where it appears to do nothing.
    #[test]
    fn the_close_affordance_follows_the_mockup_s_measured_width_tiers() {
        let palette = chrome_palette();
        for scale in [1.0_f32, 1.5, 2.0] {
            // Three counts chosen to land one on each side of the two
            // thresholds at this window width, then asserted to have done so.
            for (count, tier) in [
                (2, TabWidthTier::Full),
                (6, TabWidthTier::Tight),
                (9, TabWidthTier::Squeezed),
            ] {
                let width = 960.0 * scale;
                let geometry = tab_strip_geometry(width, scale, &resting(count), 0, 0.0);
                assert_eq!(
                    geometry.tabs[0].tier, tier,
                    "scale {scale}: {count} tabs must land in {tier:?}"
                );
                assert!(
                    geometry.tabs[0].close.is_some(),
                    "scale {scale}/{tier:?}: the active tab keeps its × at every width"
                );
                let inactive = geometry.tabs[1];
                assert_eq!(
                    inactive.close.is_some(),
                    tier == TabWidthTier::Full,
                    "scale {scale}: an inactive tab's × survives exactly the Full tier, saw {tier:?}"
                );
                if let Some(close) = inactive.close {
                    // Where it is drawn, it is pressable.
                    assert_eq!(
                        hit_tab_chrome(
                            width,
                            scale,
                            &resting(count),
                            0,
                            0.0,
                            f64::from((close[0] + close[2]) / 2.0),
                            f64::from((close[1] + close[3]) / 2.0),
                        ),
                        Some(ChromeTarget::TabClose(1))
                    );
                } else {
                    // Where it is not, the press is the tab's.
                    let body = inactive.body;
                    let trailing = body[2] - WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX * scale - 1.0;
                    assert_eq!(
                        hit_tab_chrome(
                            width,
                            scale,
                            &resting(count),
                            0,
                            0.0,
                            f64::from(trailing),
                            f64::from((body[1] + body[3]) / 2.0),
                        ),
                        Some(ChromeTarget::Tab(1)),
                        "scale {scale}/{tier:?}: a × that is not drawn is not pressable"
                    );
                }
            }

            // And a squeezed tab is its mark: no words, and the mark is centred
            // rather than sitting at the 12px leading inset.
            let titles = strip_titles(9);
            let (_, labels, sprites) = strip_chrome(scale, &titles, 0, None, false);
            assert!(
                !labels.iter().any(|label| label.text == "tab 1"),
                "scale {scale}: `.tab.squeezed .ttitle {{ display: none }}`"
            );
            let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(9), 0, 0.0);
            let body = geometry.tabs[1].body;
            let mark = sprites
                .iter()
                .filter(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
                .find(|sprite| sprite.rect[0] >= body[0] && sprite.rect[2] <= body[2])
                .expect("a squeezed tab still wears its mark");
            let mark_centre = (mark.rect[0] + mark.rect[2]) / 2.0;
            let body_centre = (body[0] + body[2]) / 2.0;
            assert!(
                (mark_centre - body_centre).abs() <= 1.0,
                "scale {scale}: `justify-content: center` — the mark is centred, saw \
                 {mark_centre} against {body_centre}"
            );

            // Ink: the `×` is `--ink3`, a step below the caption run's own —
            // over the surface its own tab is wearing, which is not one grey.
            //
            // Red gate (D1): this read `.all(|s| s.color == title_text_muted)`,
            // and that single ink is `--ink3` over `--panel`. `strip_chrome`
            // makes tab 0 the active one, so on dark the first assertion below
            // saw `0x78` where its `--termbg` ground asks for `0x72` — the pill
            // six lines above the glyph in `window_chrome` had already been
            // split into two composites, and the glyph standing inside it had
            // not.
            let full_titles = strip_titles(2);
            let close_ink = |hover: Option<ChromeTarget>, index: usize| {
                let close = tab_strip_geometry(960.0 * scale, scale, &resting(2), 0, 0.0).tabs
                    [index]
                    .close
                    .expect("a Full-tier tab has its ×");
                let (_, _, sprites) = strip_chrome(scale, &full_titles, 0, hover, false);
                sprites
                    .iter()
                    .filter(|sprite| sprite.mark == ChromeMark::TabClose)
                    .find(|sprite| sprite.rect[0] >= close[0] && sprite.rect[2] <= close[2])
                    .expect("every Full-tier tab draws its ×")
                    .color
            };
            assert_eq!(
                close_ink(None, 0),
                palette.tab_close_glyph_on_active_tab,
                "scale {scale}: the active tab's `×` is `--ink3` over `--termbg`"
            );
            assert_eq!(
                close_ink(None, 1),
                palette.title_text_muted,
                "scale {scale}: a resting tab's is the same ink over `--panel`, \
                 which is the strip's own muted ink and needs no second name"
            );
            assert_eq!(
                close_ink(Some(ChromeTarget::Tab(1)), 1),
                palette.tab_close_glyph_on_hovered_tab,
                "scale {scale}: and over that tab's own `--hover` fill once the \
                 pointer is inside it"
            );
            // `.tab .close:hover { color: var(--ink) }` — over the pill, which
            // is itself one of two surfaces.
            assert_eq!(
                close_ink(Some(ChromeTarget::TabClose(0)), 0),
                palette.tab_close_glyph_on_pill_over_active_tab,
                "scale {scale}: the lit `×` stands on its pill, not on the bar"
            );
            assert_eq!(
                close_ink(Some(ChromeTarget::TabClose(1)), 1),
                palette.tab_close_glyph_on_pill_over_hovered_tab,
                "scale {scale}: and on a hovered tab that pill is a lighter one"
            );
        }
    }

    /// PIN (`×` hover) — the pill is `--active` at 4px of round, pre-composited
    /// over *the surface it actually lands on*: the active tab's `--termbg`, or
    /// a hovered tab's own `--hover` fill. And a pointer on the `×` is still a
    /// pointer on the tab: the body lights up and the title steps to `--ink`.
    ///
    /// Red gates, two:
    ///
    /// * the previous drawing was a `ChromeQuad` in `collapse_bar_hover` — wrong
    ///   shape, wrong token, and it left the title at `--ink2` while the tab
    ///   under it was already lit;
    /// * one colour for both tabs is the other way to get this wrong, and it is
    ///   the reason there are two fields: measured on the light palette they are
    ///   `#EDEDEC` and `#DCDCD9`, seventeen levels apart.
    #[test]
    fn the_tab_close_pill_rounds_and_answers_to_the_surface_it_lands_on() {
        let palette = chrome_palette();
        for scale in [1.0_f32, 1.5, 2.0] {
            let titles = strip_titles(2);
            let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(2), 0, 0.0);
            let radius = (WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX * scale).round() as u32;
            for (index, expected) in [
                (0, palette.tab_close_pill_on_content),
                (1, palette.tab_close_pill_on_hovered_tab),
            ] {
                let close = geometry.tabs[index]
                    .close
                    .expect("a Full-tier tab has its ×");
                let (quads, _, sprites) = strip_chrome(
                    scale,
                    &titles,
                    0,
                    Some(ChromeTarget::TabClose(index)),
                    false,
                );
                let pill = sprites
                    .iter()
                    .find(|sprite| sprite.rect == pixel_snapped(close))
                    .expect("the × fills its box under the pointer");
                assert_eq!(
                    pill.mark,
                    ChromeMark::ControlPill { radius_px: radius },
                    "scale {scale}: `.tab .close {{ border-radius: 4px }}`"
                );
                assert_eq!(
                    pill.color, expected,
                    "scale {scale}: tab {index}'s pill sits on the wrong surface"
                );
                assert!(
                    !quads.iter().any(|quad| quad.rect == pixel_snapped(close)),
                    "scale {scale}: a rectangle is what this pass deletes"
                );
            }
            assert_ne!(
                palette.tab_close_pill_on_content, palette.tab_close_pill_on_hovered_tab,
                "two surfaces, two composites"
            );

            let (_, labels, sprites) =
                strip_chrome(scale, &titles, 0, Some(ChromeTarget::TabClose(1)), false);
            assert!(
                sprites
                    .iter()
                    .any(|sprite| matches!(sprite.mark, ChromeMark::TabBody { .. })),
                "scale {scale}: `.tab:hover` — the × is inside the tab"
            );
            assert_eq!(
                labels
                    .iter()
                    .find(|label| label.text == "tab 1")
                    .expect("the hovered tab keeps its name")
                    .color,
                palette.pane_title_focus,
                "scale {scale}: `.tab:not(.active):hover {{ color: var(--ink) }}`"
            );
        }
    }

    /// PIN — the title stops short of the trailing cluster rather than running
    /// under it, and it stops by the cluster's *tightened* gap: `.tab { gap: 8px }`
    /// stands between the title and the controls, and `.pin + .close`'s -4px
    /// (mock-up 329-333) takes four of those eight back.
    ///
    /// Red gate: the assertion used to read the whole 8px, which was right only
    /// while there was no `.pin` in the row. There is one now — zero-width and
    /// invisible at rest, but a sibling of the `×`, so its -4px applies and the
    /// title has four more pixels than this test used to allow it.
    #[test]
    fn a_tab_title_clears_the_trailing_cluster_by_the_clusters_own_tighter_gap() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let titles = strip_titles(2);
            let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(2), 0, 0.0);
            let close = geometry.tabs[0].close.expect("a Full-tier tab has its ×");
            let gap = WINDOW_TAB_GAP_LOGICAL_PX * scale;
            let tightened = gap - WINDOW_TAB_TRAILER_TIGHTEN_LOGICAL_PX * scale;
            let (_, labels, _) = strip_chrome(scale, &titles, 0, None, false);
            let title = labels
                .iter()
                .find(|label| label.text == "tab 0")
                .expect("the active tab carries its name");
            assert!(
                (title.rect[2] - (close[0] - tightened)).abs() < 0.01,
                "scale {scale}: title ends at {} but the × starts at {}",
                title.rect[2],
                close[0]
            );
            // And it really is a *tightening*: the title reaches further than the
            // tab's own gap would have let it.
            assert!(
                title.rect[2] > close[0] - gap,
                "scale {scale}: the cluster is tighter than the row's own gap"
            );
        }
    }

    /// PIN — a pane head's mark and its title hang off one axis, the same way
    /// `.panehead { display: flex; align-items: center }` hangs them.
    #[test]
    fn pane_head_mark_and_title_share_one_vertical_axis() {
        for dpi_milli in [1_000u32, 1_250, 1_500, 2_000] {
            let (labels, sprites) = chrome_at(dpi_milli);
            let title_bar = WINDOW_TITLE_BAR_LOGICAL_PX * dpi_milli as f32 / 1000.0;
            let mark = sprites
                .iter()
                .find(|sprite| {
                    sprite.mark == ChromeMark::ProfilePowerShell && sprite.rect[1] >= title_bar
                })
                .expect("the terminal head wears the session's profile mark");
            let title = labels
                .iter()
                .find(|label| label.text == "Terminal")
                .expect("the terminal head carries its name");
            let delta = vertical_centre(mark.rect) - vertical_centre(title.rect);
            assert!(
                delta.abs() <= 0.5,
                "pane head mark and title off one axis by {delta} physical px at {dpi_milli} \
                 milli-DPI (mark {:?}, title {:?})",
                mark.rect,
                title.rect
            );
        }
    }

    /// PIN — A7/A8: `.tab { min-width: 46px }` is a floor, and past it the strip
    /// scrolls rather than compressing further (mock-up lines 187-191, 208).
    ///
    /// Red gate: the run used to be divided evenly under only a 200px cap, so
    /// forty tabs were seventeen pixels wide each and the strip never scrolled.
    #[test]
    fn tabs_stop_shrinking_at_the_46px_floor_and_the_strip_scrolls_instead() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let width = 960.0 * scale;
            let floor = WINDOW_TAB_MIN_WIDTH_LOGICAL_PX * scale;
            let mut ever_scrolled = false;
            for count in 1..=40 {
                let geometry = tab_strip_geometry(width, scale, &resting(count), 0, 0.0);
                let tab_width = geometry.tabs[0].body[2] - geometry.tabs[0].body[0];
                assert!(
                    tab_width >= floor - 0.01,
                    "scale {scale}, {count} tabs: {tab_width} is under the 46px floor"
                );
                assert!(tab_width <= WINDOW_TAB_MAX_WIDTH_LOGICAL_PX * scale + 0.01);
                // The two are one fact: a strip scrolls exactly when its tabs
                // have reached the floor and the run stopped fitting.
                if geometry.max_scroll > 0.0 {
                    ever_scrolled = true;
                    assert!(
                        (tab_width - floor).abs() < 0.01,
                        "scale {scale}, {count} tabs: a scrolling strip sits on its floor"
                    );
                }
            }
            assert!(
                ever_scrolled,
                "scale {scale}: forty tabs cannot fit a 960px window without scrolling"
            );
        }
        // The anchor, worked through by hand at 1x: a 960px window leaves a
        // 776px run, 707px of it for tabs once the 7px inset and the 62px of
        // button furniture are taken. Fifteen tabs share that at 46.2px each and
        // still fit; the sixteenth puts every tab on the floor and starts the
        // scroller.
        assert_eq!(
            tab_strip_geometry(960.0, 1.0, &resting(15), 0, 0.0).max_scroll,
            0.0
        );
        assert!(tab_strip_geometry(960.0, 1.0, &resting(16), 0, 0.0).max_scroll > 0.0);
    }

    /// PIN — A7/A8: the strip is cropped to its viewport, and no caller can park
    /// it past its own content.
    #[test]
    fn a_scrolling_strip_is_cropped_and_never_parks_past_its_content() {
        let (scale, width, count) = (1.0_f32, 960.0_f32, 30);
        let rest = tab_strip_geometry(width, scale, &resting(count), 0, 0.0);
        assert!(rest.max_scroll > 0.0);
        assert!(
            (rest.tabs[0].body[0] - (WINDOW_TAB_RADIUS_LOGICAL_PX * scale).round()).abs() < 0.01,
            "at rest the first tab still sits at its own inset"
        );
        let end = tab_strip_geometry(width, scale, &resting(count), 0, rest.max_scroll);
        assert_eq!(
            tab_strip_geometry(width, scale, &resting(count), 0, rest.max_scroll * 4.0),
            end,
            "a strip cannot be scrolled past its own content"
        );
        assert_eq!(
            tab_strip_geometry(width, scale, &resting(count), 0, -500.0),
            rest,
            "nor before the start of it"
        );
        assert!(
            (end.new_tab_menu[2] - end.viewport[1]).abs() < 0.01,
            "scrolled to the end, the last thing in the run lands on the last pixel of it"
        );
        // What is cropped away is not there to be clicked.
        let y = f64::from(rest.tabs[0].body[1] + 1.0);
        for x in [
            rest.viewport[1],
            rest.viewport[1] + 4.0,
            rest.viewport[1] + 40.0,
        ] {
            assert_eq!(
                hit_tab_chrome(width, scale, &resting(count), 0, 0.0, f64::from(x), y),
                None,
                "x={x} is past the strip's crop and belongs to the caption run"
            );
        }
    }

    /// PIN — A7/A8: a scrolling strip leaves no slack, so it leaves no window
    /// drag room either. `tab_strip_right_px` is the boundary the platform's
    /// hit-test uses, and reporting the `˅`'s edge under scroll would hand the
    /// app's own tabs to the window drag handler.
    #[test]
    fn a_scrolling_strip_leaves_no_window_drag_room_beside_it() {
        let (scale, width) = (1.0_f32, 960.0_f32);
        let roomy = tab_strip_geometry(width, scale, &resting(2), 0, 0.0);
        assert_eq!(roomy.max_scroll, 0.0);
        assert_eq!(
            tab_strip_right_px(width, scale, 2),
            roomy.new_tab_menu[2].ceil() as i32,
            "with room to spare the app owns up to the `˅`, and the rest is drag"
        );
        let full = tab_strip_geometry(width, scale, &resting(30), 0, 0.0);
        assert!(full.max_scroll > 0.0);
        assert_eq!(
            tab_strip_right_px(width, scale, 30),
            full.viewport[1].ceil() as i32,
            "a scrolling strip owns its whole run"
        );
    }

    /// PIN — A7/A8: activating or opening a tab scrolls it wholly into view, and
    /// moves the strip no further than it must.
    #[test]
    fn revealing_a_tab_frames_it_whole_and_moves_no_further_than_needed() {
        let (scale, width, count) = (1.0_f32, 960.0_f32, 30);
        let skirt = (WINDOW_TAB_RADIUS_LOGICAL_PX * scale).round();
        for index in [0, 1, 7, 15, count - 1] {
            let scrolled = tab_scroll_to_reveal(width, scale, count, index, 0.0, index);
            let geometry = tab_strip_geometry(width, scale, &resting(count), index, scrolled);
            let body = geometry.tabs[index].body;
            assert!(
                body[0] - skirt >= geometry.viewport[0] - 0.01
                    && body[2] + skirt <= geometry.viewport[1] + 0.01,
                "tab {index} is still cropped at scroll {scrolled}: {body:?} in {:?}",
                geometry.viewport
            );
        }
        assert_eq!(
            tab_scroll_to_reveal(width, scale, count, 0, 0.0, 0),
            0.0,
            "a tab already framed does not move the strip"
        );
        let once = tab_scroll_to_reveal(width, scale, count, 20, 0.0, 20);
        assert!(once > 0.0);
        assert_eq!(
            tab_scroll_to_reveal(width, scale, count, 20, once, 20),
            once,
            "revealing does not overshoot: asking twice is asking once"
        );
    }

    /// PIN — C27-C29: the pane-count badge appears above one pane and reserves
    /// nothing below it (mock-up lines 292-304, 4189).
    ///
    /// Red gate: there was no badge at all, so a tab holding three panes said so
    /// nowhere in the strip.
    #[test]
    fn the_pane_count_badge_appears_only_above_one_pane() {
        let tab = |pane_count| TabContent {
            title: "tab".to_owned(),
            pane_count,
            badge_text_width: 6.0,
            mark: TabMarkState::default(),
            trailer: TabTrailer::default(),
            offset: 0.0,
            landing: 0.0,
            edit: None,
        };
        let (_, lone_labels, lone_sprites) = strip_chrome_of(1.0, &[tab(1)], 0, 0.0, None, false);
        let (_, pair_labels, pair_sprites) = strip_chrome_of(1.0, &[tab(2)], 0, 0.0, None, false);
        assert!(
            !lone_labels.iter().any(|label| label.text == "1"),
            "one pane prints no number and reserves no hole for one"
        );
        assert!(pair_labels.iter().any(|label| label.text == "2"));
        assert_eq!(
            pair_sprites.len(),
            lone_sprites.len() + 1,
            "and exactly one pill arrives with it"
        );
        let title_right = |labels: &[ChromeLabel]| {
            labels
                .iter()
                .find(|label| label.text == "tab")
                .expect("the tab is titled")
                .rect[2]
        };
        assert!(
            title_right(&lone_labels) > title_right(&pair_labels),
            "the badge takes its width off the title's room, and only when it is there"
        );
    }

    /// PIN — C27-C29: the badge is the mock-up's own pill, in the mock-up's own
    /// place — icon, title, badge, then the `×`.
    #[test]
    fn the_badge_is_the_mockups_pill_and_stands_between_the_title_and_the_close() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(2), 0, 0.0);
            let tab = &geometry.tabs[0];
            let badge = tab_badge_rect(tab, 3, 0.0, scale).expect("three panes wear a badge");
            assert_eq!(
                badge[3] - badge[1],
                (WINDOW_TAB_BADGE_HEIGHT_LOGICAL_PX * scale).round(),
                "height: 15px"
            );
            assert_eq!(
                badge[2] - badge[0],
                (WINDOW_TAB_BADGE_MIN_WIDTH_LOGICAL_PX * scale).round(),
                "min-width: 15px holds a narrow digit"
            );
            let wide = tab_badge_rect(tab, 12, 20.0 * scale, scale).expect("two digits");
            assert_eq!(
                wide[2] - wide[0],
                (20.0 * scale + 2.0 * WINDOW_TAB_BADGE_PADDING_X_LOGICAL_PX * scale).round(),
                "padding: 0 4px is what a wider number grows by"
            );
            let close = tab.close.expect("a roomy active tab keeps its ×");
            let tightened =
                (WINDOW_TAB_GAP_LOGICAL_PX - WINDOW_TAB_TRAILER_TIGHTEN_LOGICAL_PX) * scale;
            assert!(
                (badge[2] - (close[0] - tightened)).abs() < 0.51,
                "the badge docks by the × across the cluster's tightened gap — 8px \
                 less the -4px `.pin + .close` takes back (mock-up 329-333)"
            );
            let axis = (tab.body[1] + tab.body[3]) / 2.0;
            assert!(
                ((badge[1] + badge[3]) / 2.0 - axis).abs() <= 1.0,
                "`align-items: center` puts it on the tab's own axis"
            );
        }
        // `.tab.squeezed .panecount { display: none }` (mock-up line 201).
        let squeezed = tab_strip_geometry(960.0, 1.0, &resting(30), 0, 0.0);
        assert_eq!(squeezed.tabs[1].tier, TabWidthTier::Squeezed);
        assert!(
            tab_badge_rect(&squeezed.tabs[1], 3, 0.0, 1.0).is_none(),
            "under 90px the tab is its mark and carries nothing else"
        );
    }

    /// A tab with the rename editor open on it, already measured.
    fn editing_tab(edit: TabEdit) -> TabContent {
        TabContent {
            title: "committed-title".to_owned(),
            pane_count: 1,
            badge_text_width: 0.0,
            mark: TabMarkState::default(),
            trailer: TabTrailer::default(),
            offset: 0.0,
            landing: 0.0,
            edit: Some(edit),
        }
    }

    /// The title box the strip gives the one tab in `tabs`.
    fn only_title_box(scale: f32) -> [f32; 4] {
        let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(1), 0, 0.0);
        tab_title_box(&geometry.tabs[0], 1, 0.0, scale).expect("a lone tab has room for its title")
    }

    /// J100 (mock-up 376-385) — "the editor is the tab: same box, same metrics,
    /// so committing a name does not make the strip jump".
    ///
    /// Red gate: there was no editor at all, so nothing checked that opening one
    /// leaves the strip's geometry alone. The assertion that matters is the
    /// *identity* of the two rects — an editor drawn in a box of its own would
    /// pass every "it renders" test and still move the letters when you clicked.
    #[test]
    fn the_editor_takes_the_titles_own_box_and_leaves_the_tab_alone() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let resting_tab = TabContent {
                title: "committed-title".to_owned(),
                pane_count: 1,
                badge_text_width: 0.0,
                mark: TabMarkState::default(),
                trailer: TabTrailer::default(),
                offset: 0.0,
                landing: 0.0,
                edit: None,
            };
            let editing = editing_tab(TabEdit {
                text: "draft".to_owned(),
                placeholder: "auto".to_owned(),
                caret_px: 10.0 * scale,
                selection_px: 0.0,
                caret_lit: false,
            });
            let (resting_quads, resting_labels, resting_sprites) =
                strip_chrome_of(scale, &[resting_tab], 0, 0.0, None, false);
            let (edit_quads, edit_labels, edit_sprites) =
                strip_chrome_of(scale, &[editing], 0, 0.0, None, false);

            let title = resting_labels
                .iter()
                .find(|label| label.text == "committed-title")
                .expect("the resting tab is titled");
            let draft = edit_labels
                .iter()
                .find(|label| label.text == "draft")
                .expect("the editing tab shows its draft");
            assert_eq!(
                draft.rect, title.rect,
                "same box at {scale}x — `.rename` is `flex: 1 1 auto; padding: 0`"
            );
            assert_eq!(
                draft.font_size_px, title.font_size_px,
                "`font: inherit` (mock-up 383)"
            );
            assert_eq!(
                draft.weight, title.weight,
                "and it inherits the weight with it"
            );
            assert!(
                !edit_labels
                    .iter()
                    .any(|label| label.text == "committed-title"),
                "the committed name is not drawn beside the draft — the draft replaces it"
            );
            assert_eq!(
                edit_quads, resting_quads,
                "nothing in the tab's own structure moves when the editor opens"
            );
            assert_eq!(
                edit_sprites, resting_sprites,
                "the mark, the × and the tab's silhouette all stay exactly where they were \
                 — with the caret dark, an open editor is invisible in the sprite list"
            );
        }
    }

    /// J101 (mock-up 5866, 385) — an empty draft reveals the layer underneath,
    /// in `--ink3`.
    ///
    /// The two inks are the whole point of the placeholder: it has to read as
    /// *what you would get*, not as a name someone already typed.
    #[test]
    fn an_empty_draft_shows_the_auto_name_in_the_placeholder_ink() {
        let palette = chrome_palette();
        let empty = editing_tab(TabEdit {
            text: String::new(),
            placeholder: "bt-app".to_owned(),
            caret_px: 0.0,
            selection_px: 0.0,
            caret_lit: false,
        });
        let (_, labels, _) = strip_chrome_of(1.0, &[empty], 0, 0.0, None, false);
        let shown = labels
            .iter()
            .find(|label| label.text == "bt-app")
            .expect("the placeholder stands in for the empty draft");
        assert_eq!(
            shown.color, palette.pane_title,
            "`.rename::placeholder {{ color: var(--ink3) }}` over the tab's own surface"
        );

        let typed = editing_tab(TabEdit {
            text: "mine".to_owned(),
            placeholder: "bt-app".to_owned(),
            caret_px: 0.0,
            selection_px: 0.0,
            caret_lit: false,
        });
        let (_, labels, _) = strip_chrome_of(1.0, &[typed], 0, 0.0, None, false);
        assert!(
            !labels.iter().any(|label| label.text == "bt-app"),
            "a draft with something in it hides the layer under it"
        );
        assert_eq!(
            labels
                .iter()
                .find(|label| label.text == "mine")
                .expect("the draft is drawn")
                .color,
            palette.pane_title_focus,
            "`.rename {{ color: var(--ink) }}` (mock-up 382)"
        );
    }

    /// J102/J103 — the caret stands where it was measured to, is as tall as the
    /// row it is in, and goes out with the blink.
    #[test]
    fn the_caret_stands_at_its_measured_offset_and_blinks() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let title_box = only_title_box(scale);
            let caret_px = 12.0 * scale;
            let lit = editing_tab(TabEdit {
                text: "draft".to_owned(),
                placeholder: String::new(),
                caret_px,
                selection_px: 0.0,
                caret_lit: true,
            });
            let dark = editing_tab(TabEdit {
                caret_lit: false,
                ..TabEdit {
                    text: "draft".to_owned(),
                    placeholder: String::new(),
                    caret_px,
                    selection_px: 0.0,
                    caret_lit: true,
                }
            });
            let (_, _, lit_sprites) = strip_chrome_of(scale, &[lit], 0, 0.0, None, false);
            let (_, _, dark_sprites) = strip_chrome_of(scale, &[dark], 0, 0.0, None, false);

            assert_eq!(
                lit_sprites.len(),
                dark_sprites.len() + 1,
                "one sprite is the whole difference between a lit caret and a dark one"
            );
            let caret = lit_sprites
                .iter()
                .find(|sprite| sprite.mark == ChromeMark::Fill)
                .expect("a lit caret is drawn");
            assert_eq!(
                caret.rect[0],
                (title_box[0] + caret_px).round(),
                "at the offset the measurement handed it, off the box's own left edge"
            );
            assert_eq!(
                caret.rect[2] - caret.rect[0],
                (TAB_RENAME_CARET_LOGICAL_PX * scale).round().max(1.0),
                "a hairline, DPI-rounded and never thinner than one device pixel"
            );
            assert_eq!(
                caret.rect[3] - caret.rect[1],
                (WINDOW_TAB_MARK_LOGICAL_PX * scale).round(),
                "as tall as the mark beside it — the tab's own content band"
            );
            let axis = (title_box[1] + title_box[3]) / 2.0;
            assert!(
                ((caret.rect[1] + caret.rect[3]) / 2.0 - axis).abs() <= 1.0,
                "on the axis `align-items: center` puts everything else on"
            );
            assert_eq!(
                caret.color,
                chrome_palette().pane_title_focus,
                "the caret is the ink it stands in — `.rename {{ color: var(--ink) }}`"
            );
        }
    }

    /// J101 (mock-up 5870, `input.select()`) — the opening selection is drawn,
    /// under the letters and inside the box.
    ///
    /// A selection nobody can see is not a selection: the whole reason it is
    /// there is to say "the next thing you type replaces this".
    #[test]
    fn the_opening_selection_is_a_visible_band_clipped_to_the_editors_box() {
        let title_box = only_title_box(1.0);
        let box_width = title_box[2] - title_box[0];
        let selected = editing_tab(TabEdit {
            text: "build".to_owned(),
            placeholder: String::new(),
            caret_px: 30.0,
            selection_px: 30.0,
            caret_lit: false,
        });
        let (_, _, sprites) = strip_chrome_of(1.0, &[selected], 0, 0.0, None, false);
        let band = sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::Fill)
            .expect("a selected draft wears its band");
        assert_eq!(band.rect[0], title_box[0], "it starts where the text does");
        assert_eq!(band.rect[2], title_box[0] + 30.0);
        assert_eq!(
            band.color,
            chrome_palette().tab_close_pill_on_content,
            "`--active`, pre-composited over the surface this tab is wearing"
        );

        // A selection wider than the box is clipped by it rather than bleeding
        // over the × beside it.
        let overflowing = editing_tab(TabEdit {
            text: "a very long name indeed".to_owned(),
            placeholder: String::new(),
            caret_px: 0.0,
            selection_px: box_width * 4.0,
            caret_lit: false,
        });
        let (_, _, sprites) = strip_chrome_of(1.0, &[overflowing], 0, 0.0, None, false);
        let band = sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::Fill)
            .expect("the band is still drawn");
        assert_eq!(
            band.rect[2], title_box[2],
            "clipped to the box, which is what `overflow` does to a real input"
        );

        // Nothing selected draws nothing — the band is not a permanent fixture
        // with a zero width.
        let plain = editing_tab(TabEdit {
            text: "build".to_owned(),
            placeholder: String::new(),
            caret_px: 30.0,
            selection_px: 0.0,
            caret_lit: false,
        });
        let (_, _, sprites) = strip_chrome_of(1.0, &[plain], 0, 0.0, None, false);
        assert!(
            !sprites.iter().any(|sprite| sprite.mark == ChromeMark::Fill),
            "a collapsed caret has no band behind it"
        );
    }

    /// The caret and the band are drawn *after* the tab's own silhouette.
    ///
    /// This is the whole reason they are sprites rather than
    /// [`bt_render::ChromeQuad`]s: quads land under every mark, and the active
    /// tab's body is a mark. A caret painted under the tab it is inside is a
    /// caret nobody sees, and it would have looked exactly like "the caret does
    /// not work".
    #[test]
    fn the_editors_marks_are_painted_over_the_tab_they_are_inside() {
        let editing = editing_tab(TabEdit {
            text: "draft".to_owned(),
            placeholder: String::new(),
            caret_px: 10.0,
            selection_px: 20.0,
            caret_lit: true,
        });
        let (_, _, sprites) = strip_chrome_of(1.0, &[editing], 0, 0.0, None, false);
        let body = sprites
            .iter()
            .position(|sprite| matches!(sprite.mark, ChromeMark::ActiveTab { .. }))
            .expect("the active tab paints its silhouette");
        let fills = sprites
            .iter()
            .enumerate()
            .filter(|(_, sprite)| sprite.mark == ChromeMark::Fill)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        assert_eq!(fills.len(), 2, "the selection band and the caret");
        assert!(
            fills.iter().all(|index| *index > body),
            "both stand after the silhouette in the painter's order"
        );
    }

    /// J100 — below the squeeze the tab is its mark, and there is no box to be
    /// an editor. The draft is not lost, it is merely not on screen.
    #[test]
    fn a_squeezed_tab_has_no_title_box_for_an_editor_to_take() {
        let squeezed = tab_strip_geometry(960.0, 1.0, &resting(30), 0, 0.0);
        assert_eq!(squeezed.tabs[1].tier, TabWidthTier::Squeezed);
        assert!(
            tab_title_box(&squeezed.tabs[1], 1, 0.0, 1.0).is_none(),
            "`.tab.squeezed .ttitle {{ display: none }}` (mock-up 201)"
        );
        let roomy = tab_strip_geometry(960.0, 1.0, &resting(2), 0, 0.0);
        assert!(tab_title_box(&roomy.tabs[0], 1, 0.0, 1.0).is_some());
    }

    /// The title box is the one computation the strip and the editor share.
    ///
    /// `tab_title_box` was split out of `window_chrome`, so the two must still
    /// agree exactly — a second derivation would drift, and the drift would show
    /// as a caret standing beside its own letters.
    #[test]
    fn the_title_box_the_editor_measures_is_the_box_the_strip_draws_in() {
        for scale in [1.0_f32, 1.5, 2.0] {
            for pane_count in [1_usize, 3] {
                let badge_text_width = if pane_count > 1 { 6.0 * scale } else { 0.0 };
                let tabs = [TabContent {
                    title: "measure-me".to_owned(),
                    pane_count,
                    badge_text_width,
                    mark: TabMarkState::default(),
                    trailer: TabTrailer::default(),
                    offset: 0.0,
                    landing: 0.0,
                    edit: None,
                }];
                let (_, labels, _) = strip_chrome_of(scale, &tabs, 0, 0.0, None, false);
                let drawn = labels
                    .iter()
                    .find(|label| label.text == "measure-me")
                    .expect("the tab is titled")
                    .rect;
                let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(1), 0, 0.0);
                let measured =
                    tab_title_box(&geometry.tabs[0], pane_count, badge_text_width, scale)
                        .expect("a lone tab has room");
                assert_eq!(
                    drawn, measured,
                    "one box at {scale}x with {pane_count} panes, not two"
                );
            }
        }
    }

    fn tab_with(mark: TabMarkState) -> TabContent {
        TabContent {
            title: "session".to_owned(),
            pane_count: 1,
            badge_text_width: 0.0,
            mark,
            trailer: TabTrailer::default(),
            offset: 0.0,
            landing: 0.0,
            edit: None,
        }
    }

    /// `count` ordinary tabs: unpinned, with nothing revealed.
    fn resting(count: usize) -> Vec<TabTrailer> {
        vec![TabTrailer::default(); count]
    }

    /// One tab that carries nothing but the trailer under test.
    fn pinnable_tab(trailer: TabTrailer) -> TabContent {
        TabContent {
            title: "tab".to_owned(),
            pane_count: 1,
            badge_text_width: 0.0,
            mark: TabMarkState::default(),
            trailer,
            offset: 0.0,
            landing: 0.0,
            edit: None,
        }
    }

    /// Every mark-slot sprite the strip drew for a single tab.
    fn mark_slot_sprites(mark: TabMarkState, tier_width: f32) -> Vec<ChromeSprite> {
        let tabs = [tab_with(mark)];
        let (_, _, sprites) = strip_chrome_of(1.0, &tabs, 0, 0.0, None, false);
        let _ = tier_width;
        sprites
    }

    /// PIN (T2 C22/D36): a progress ring *replaces* the mark in the same box,
    /// and gives it back the moment the progress ends.
    ///
    /// The user's ruling, deviating from `.pring` — which overlays a 25px
    /// circle *around* a 15px mark, a size the mock-up's own comment (line 270)
    /// justifies purely by the need to clear that mark's corners. Replacing it
    /// dissolves the constraint, and Chrome's own loading indicator does the
    /// same thing to a favicon.
    ///
    /// Both halves are asserted because either alone is a bug that ships: a
    /// ring that never appears, or one that never leaves.
    #[test]
    fn a_progress_ring_replaces_the_mark_and_then_returns_it() {
        let resting = mark_slot_sprites(TabMarkState::default(), 960.0);
        let profile = |sprites: &[ChromeSprite]| {
            sprites
                .iter()
                .find(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
                .map(|sprite| sprite.rect)
        };
        let slot = profile(&resting).expect("a resting tab draws its profile mark");

        let ringed = mark_slot_sprites(
            TabMarkState {
                ring: Some(TabRing {
                    arc: [1, 2, 3],
                    start_milliturns: 0,
                    sweep_milliturns: 400,
                }),
                ..TabMarkState::default()
            },
            960.0,
        );
        assert!(
            profile(&ringed).is_none(),
            "the ring replaces the mark rather than sitting over it"
        );
        let rings: Vec<&ChromeSprite> = ringed
            .iter()
            .filter(|sprite| matches!(sprite.mark, ChromeMark::ProgressRing { .. }))
            .collect();
        assert_eq!(rings.len(), 2, "a ring is its track and its arc");
        for ring in &rings {
            assert_eq!(
                ring.rect, slot,
                "the ring must occupy exactly the mark's own box — zero layout shift"
            );
        }
        // Track first, arc over it, and the arc is the one carrying the state's
        // colour. Drawn the other way round the track would erase the arc.
        assert!(
            matches!(
                rings[0].mark,
                ChromeMark::ProgressRing {
                    sweep_milliturns: 1000,
                    ..
                }
            ),
            "the track is a full turn and is drawn first"
        );
        assert_eq!(
            rings[1].color,
            [1, 2, 3],
            "the arc wears the state's colour"
        );
        assert_ne!(
            rings[0].color, rings[1].color,
            "a track the colour of its arc is not a track"
        );
    }

    /// PIN (T2 D32/D33): the dot hangs off the mark slot's top-right corner and
    /// overhangs it on both axes, exactly as `.unreaddot` is placed.
    ///
    /// `position: absolute; top: -2px; right: -4px` (mock-up line 254). The
    /// negative offsets are the design: a badge that fits neatly inside its
    /// host reads as part of the artwork rather than as something added to it.
    #[test]
    fn the_status_dot_hangs_off_the_marks_top_right_corner() {
        let sprites = mark_slot_sprites(
            TabMarkState {
                dot: Some([9, 9, 9]),
                ..TabMarkState::default()
            },
            960.0,
        );
        let slot = sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
            .expect("the mark is still there — a dot does not replace it")
            .rect;
        let dot = sprites
            .iter()
            .find(|sprite| sprite.color == [9, 9, 9])
            .expect("the dot is drawn");
        let side = dot.rect[2] - dot.rect[0];
        assert!(
            (side - WINDOW_TAB_STATUS_DOT_LOGICAL_PX).abs() <= 1.0,
            "the dot is 6px square, got {side}"
        );
        assert!(
            (dot.rect[3] - dot.rect[1] - side).abs() <= 1.0,
            "and square, so `border-radius: 50%` is a circle"
        );
        // Outward on both axes: past the slot's right edge and above its top.
        assert!(
            dot.rect[2] > slot[2],
            "`right: -4px` puts the dot past the mark's right edge"
        );
        assert!(
            dot.rect[1] < slot[1],
            "`top: -2px` lifts it above the mark's top edge"
        );
        // And it is the circle primitive, not a square.
        assert!(matches!(
            dot.mark,
            ChromeMark::ControlPill { radius_px } if radius_px * 2 >= side as u32
        ));
    }

    /// PIN (T2): a silent tab draws no dot at all.
    ///
    /// The mock-up keeps `.unreaddot` in the DOM and toggles a class, for a
    /// reason it records at line 249 — but what reaches the screen is still
    /// nothing. A dot always drawn in some quiet colour would be a permanent
    /// smudge on every tab.
    #[test]
    fn a_tab_with_nothing_to_say_draws_no_dot() {
        let sprites = mark_slot_sprites(TabMarkState::default(), 960.0);
        let dots = sprites
            .iter()
            .filter(|sprite| matches!(sprite.mark, ChromeMark::ControlPill { .. }))
            .count();
        assert_eq!(dots, 0, "a resting single-pane tab draws no pill at all");
    }

    /// PIN (T2): the mark's opacity reaches the sprite, and lands on the mark
    /// alone.
    ///
    /// The breath is `.ticon.working` — the icon, not the wrapper — so it must
    /// never touch the dot beside it. Fading the two together would make
    /// "running" and "finished, unread" pulse as one thing, which is exactly
    /// the collapse the mock-up's own comment at line 247 forbids.
    #[test]
    fn the_breath_fades_the_mark_and_leaves_the_dot_alone() {
        let sprites = mark_slot_sprites(
            TabMarkState {
                dot: Some([9, 9, 9]),
                opacity: 0.28,
                ..TabMarkState::default()
            },
            960.0,
        );
        let mark = sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
            .expect("the mark is drawn");
        assert!((mark.opacity - 0.28).abs() < 1e-6, "the mark breathes");
        let dot = sprites
            .iter()
            .find(|sprite| sprite.color == [9, 9, 9])
            .expect("the dot is drawn");
        assert_eq!(dot.opacity, 1.0, "the dot does not breathe with the mark");
    }

    /// PIN (T2, real-machine bug): the chrome the strip actually produces
    /// carries a fully opaque mark once work has stopped.
    ///
    /// The sibling of `a_session_that_stops_working_returns_its_mark_to_full_opacity`
    /// in `main.rs`, one layer down: that one pins the *decision*, this one
    /// pins what comes out the other end. The bug on hardware was visible as a
    /// sprite, so it is worth asserting on a sprite — a fade that survived into
    /// the drawing code would show here even if the decision above were right.
    #[test]
    fn a_finished_tab_draws_its_mark_fully_opaque() {
        let sprites = mark_slot_sprites(TabMarkState::default(), 960.0);
        let mark = sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
            .expect("a settled tab draws its mark");
        assert_eq!(
            mark.opacity, 1.0,
            "a tab whose command has returned draws no fade at all"
        );
        // Exactly, not nearly: this rides to the shader as a multiplier on the
        // raster's alpha, and 0.999 is a mark that is very slightly not there.
        assert!(mark.opacity.to_bits() == 1.0_f32.to_bits());
    }

    /// PIN (T2): the dot and the ring survive every squeeze tier.
    ///
    /// The stylesheet narrows a tab by hiding `.ttitle`, `.panecount`, `.pin`
    /// and the `×` (lines 197-202) and never once touches `.ticon-wrap`. That
    /// is the design's priority order made explicit: at 46px a tab is its mark,
    /// and what the mark is *saying* is the last thing that may go — a tab too
    /// narrow to name is exactly the tab whose dot you need.
    #[test]
    fn the_dot_and_ring_survive_every_squeeze_tier() {
        let mark = TabMarkState {
            dot: Some([9, 9, 9]),
            ring: Some(TabRing {
                arc: [1, 2, 3],
                start_milliturns: 0,
                sweep_milliturns: 400,
            }),
            ..TabMarkState::default()
        };
        // Enough tabs to drive the strip through all three tiers.
        for count in [2_usize, 8, 30] {
            let tabs: Vec<TabContent> = (0..count).map(|_| tab_with(mark)).collect();
            let geometry = tab_strip_geometry(960.0, 1.0, &resting(count), 0, 0.0);
            let tier = geometry.tabs[0].tier;
            let (_, _, sprites) = strip_chrome_of(1.0, &tabs, 0, 0.0, None, false);
            assert!(
                sprites.iter().any(|sprite| sprite.color == [9, 9, 9]),
                "{tier:?}: the dot must survive the squeeze"
            );
            assert!(
                sprites.iter().any(|sprite| sprite.color == [1, 2, 3]),
                "{tier:?}: the ring must survive the squeeze"
            );
        }
        // And the narrowest tier really is reached, or the loop proved nothing.
        assert_eq!(
            tab_strip_geometry(960.0, 1.0, &resting(30), 0, 0.0).tabs[0].tier,
            TabWidthTier::Squeezed
        );
    }

    /// PIN — C27-C29: the badge wears `--active` over whichever of the three
    /// surfaces its tab is showing, and its ink is never the accent.
    #[test]
    fn the_badge_wears_active_over_its_own_tab_and_never_the_accent() {
        let palette = chrome_palette();
        let tabs = [
            TabContent {
                title: "a".to_owned(),
                pane_count: 2,
                badge_text_width: 6.0,
                mark: TabMarkState::default(),
                trailer: TabTrailer::default(),
                offset: 0.0,
                landing: 0.0,
                edit: None,
            },
            TabContent {
                title: "b".to_owned(),
                pane_count: 3,
                badge_text_width: 6.0,
                mark: TabMarkState::default(),
                trailer: TabTrailer::default(),
                offset: 0.0,
                landing: 0.0,
                edit: None,
            },
        ];
        let (_, labels, sprites) = strip_chrome_of(1.0, &tabs, 0, 0.0, None, false);
        let pill = |color: [u8; 3]| {
            sprites.iter().any(|sprite| {
                sprite.color == color
                    && matches!(
                        sprite.mark,
                        ChromeMark::ControlPill { radius_px }
                            if radius_px == WINDOW_TAB_BADGE_RADIUS_LOGICAL_PX as u32
                    )
            })
        };
        assert!(
            pill(palette.tab_close_pill_on_content),
            "the active tab's badge is `--active` over `--termbg`"
        );
        assert!(
            pill(palette.tab_badge_on_resting_tab),
            "a resting tab's is `--active` over `--panel`"
        );
        let ink = |text: &str| {
            labels
                .iter()
                .find(|label| label.text == text)
                .map(|label| (label.color, label.font_size_px, label.align_center))
        };
        assert_eq!(
            ink("2"),
            Some((palette.tab_badge_text_on_active_tab, 10.0, true)),
            "`--ink` on the active tab, 10px, centred in its pill"
        );
        assert_eq!(
            ink("3"),
            Some((palette.tab_badge_text_on_resting_tab, 10.0, true)),
            "`--ink2` elsewhere"
        );
        assert!(
            !labels.iter().any(|label| {
                matches!(label.text.as_str(), "2" | "3") && label.color == palette.accent
            }),
            "mock-up line 297 rules the accent out here"
        );
    }

    /// The centre of a box, as a pointer would land on it.
    fn centre(rect: [f32; 4]) -> (f64, f64) {
        (
            f64::from((rect[0] + rect[2]) / 2.0),
            f64::from((rect[1] + rect[3]) / 2.0),
        )
    }

    /// PIN — F48/F51: the pin stands in the slot the `×` gave up. The *same* box
    /// — 17px square at 4px of round (mock-up 319-320) — with its right edge on
    /// the tab's own 6px trailing padding, and no `×` anywhere on the tab.
    ///
    /// Both halves are the design (mock-up 4059-4065): the shared slot is what
    /// puts unpinning where you already are, and the missing `×` is what stops a
    /// stray click shutting a tab you promised to keep. "That protection IS the
    /// feature, not a side effect."
    ///
    /// Red gate: drawing the pin *beside* a kept `×` — the obvious layout, and
    /// the one that hands a pinned tab a one-click close.
    #[test]
    fn a_pinned_tab_wears_its_pin_in_the_slot_the_close_gave_up() {
        // The alias, not a second 17: "same box as `.close` because it stands in
        // the same slot" is the rule, and two literals could drift.
        assert_eq!(
            WINDOW_TAB_PIN_BOX_LOGICAL_PX,
            WINDOW_TAB_CLOSE_BOX_LOGICAL_PX
        );
        assert_eq!(
            WINDOW_TAB_PIN_RADIUS_LOGICAL_PX,
            WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX
        );
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let width = 960.0 * scale;
            let box_px = (WINDOW_TAB_PIN_BOX_LOGICAL_PX * scale).round();
            let pinned = [
                TabTrailer {
                    pinned: true,
                    reveal: 0.0,
                },
                TabTrailer::default(),
            ];
            let geometry = tab_strip_geometry(width, scale, &pinned, 0, 0.0);
            let tab = geometry.tabs[0];
            assert_eq!(
                tab.close, None,
                "scale {scale}: `tabTrailer` writes no `.close` for a pinned tab"
            );
            let pin = tab.pin.expect("a pinned tab wears its pin");
            let close = tab_strip_geometry(width, scale, &resting(2), 0, 0.0).tabs[0]
                .close
                .expect("an unpinned Full-tier tab has its ×");
            assert_eq!(
                pin, close,
                "scale {scale}: the pin stands in the ×'s slot, to the pixel"
            );
            assert_eq!(
                [pin[2] - pin[0], pin[3] - pin[1]],
                [box_px, box_px],
                "scale {scale}: `.tab .pin {{ width: 17px; height: 17px }}`"
            );
            assert!(
                (pin[2] - (tab.body[2] - WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX * scale)).abs() < 0.01,
                "scale {scale}: its right edge is the tab's own trailing padding"
            );
            // `.pin.on` does not wait for a pointer: a fact about the tab is not
            // an offer that appears on hover.
            let hovered = tab_strip_geometry(
                width,
                scale,
                &[
                    TabTrailer {
                        pinned: true,
                        reveal: 1.0,
                    },
                    TabTrailer::default(),
                ],
                0,
                0.0,
            );
            assert_eq!(
                hovered.tabs[0].pin,
                Some(pin),
                "scale {scale}: a pinned tab ignores the reveal"
            );
        }
    }

    /// PIN — E42: the two narrow tiers take the pin from every tab, including a
    /// pinned one and including the active one.
    ///
    /// `.tab.tight .pin` and `.tab.squeezed .pin { display: none }` (mock-up 197,
    /// 201) carry no `:not(.active)` the way the `×`'s rules do, and that
    /// asymmetry is deliberate: "summoning files+pin+close at this width crushed
    /// the title to 0px and left an icon soup". At these widths the pin retreats
    /// to give the title its room.
    ///
    /// Red gate: keeping `.pin.on` at tight because "a pinned tab must always say
    /// so" — which is exactly the icon soup the mock-up measured, and it is not
    /// even needed: pinned tabs also lead the strip and carry no `×`.
    #[test]
    fn the_narrow_tiers_take_the_pin_from_every_tab_including_a_pinned_active_one() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let width = 960.0 * scale;
            // Three counts chosen to land one on each side of the two thresholds
            // at this window width, then asserted to have done so.
            for (count, tier) in [
                (2, TabWidthTier::Full),
                (6, TabWidthTier::Tight),
                (9, TabWidthTier::Squeezed),
            ] {
                let trailers = vec![
                    TabTrailer {
                        pinned: true,
                        reveal: 1.0,
                    };
                    count
                ];
                let geometry = tab_strip_geometry(width, scale, &trailers, 0, 0.0);
                assert_eq!(
                    geometry.tabs[0].tier, tier,
                    "scale {scale}: {count} tabs must land in {tier:?}"
                );
                for (index, tab) in geometry.tabs.iter().enumerate() {
                    assert_eq!(
                        tab.pin.is_some(),
                        tier == TabWidthTier::Full,
                        "scale {scale}/{tier:?}: tab {index}'s pin survives exactly the Full tier"
                    );
                    assert_eq!(
                        tab.close, None,
                        "scale {scale}/{tier:?}: and a pinned tab never has a × to fall back on"
                    );
                }
                // A tight tab that is *not* pinned still keeps the active tab's
                // `×`: it is the pin's rule that is unqualified, not the `×`'s.
                let ordinary = tab_strip_geometry(width, scale, &resting(count), 0, 0.0);
                assert!(
                    ordinary.tabs[0].close.is_some(),
                    "scale {scale}/{tier:?}: `.tab.tight:not(.active) .close` spares the active tab"
                );
                assert!(
                    ordinary.tabs[0].pin.is_none(),
                    "scale {scale}/{tier:?}: an unrevealed pin is no box either"
                );
            }
        }
    }

    /// PIN — F51: a press lands on the pin exactly where one is drawn, and
    /// nowhere else. A pin that is not drawn is not pressable.
    ///
    /// The same claim the `×` makes in
    /// `the_close_affordance_follows_the_mockup_s_measured_width_tiers`, and it
    /// matters more here: the pin's resting box is *zero pixels wide*, so a hit
    /// test written off the slot rather than off the box would toggle the pin of
    /// every tab whose `×` you meant to press.
    #[test]
    fn a_press_lands_on_the_pin_only_where_one_is_drawn() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let width = 960.0 * scale;
            let pinned = [
                TabTrailer {
                    pinned: true,
                    reveal: 0.0,
                },
                TabTrailer::default(),
            ];
            let slot = tab_strip_geometry(width, scale, &pinned, 0, 0.0).tabs[0]
                .pin
                .expect("a pinned tab wears its pin");
            let (x, y) = centre(slot);
            assert_eq!(
                hit_tab_chrome(width, scale, &pinned, 0, 0.0, x, y),
                Some(ChromeTarget::TabPin(0)),
                "scale {scale}: the pinned tab's slot is the pin's"
            );
            assert_eq!(
                hit_tab_chrome(width, scale, &resting(2), 0, 0.0, x, y),
                Some(ChromeTarget::TabClose(0)),
                "scale {scale}: and on an unpinned tab the same slot is still the ×'s"
            );

            // Where a revealed pin *would* stand on an unpinned tab, a resting
            // strip has nothing at all, so the press is the tab's.
            let revealed = [
                TabTrailer {
                    pinned: false,
                    reveal: 1.0,
                },
                TabTrailer::default(),
            ];
            let open = tab_strip_geometry(width, scale, &revealed, 0, 0.0).tabs[0]
                .pin
                .expect("a full reveal opens the box");
            let (open_x, open_y) = centre(open);
            assert_eq!(
                hit_tab_chrome(width, scale, &revealed, 0, 0.0, open_x, open_y),
                Some(ChromeTarget::TabPin(0)),
                "scale {scale}: the revealed pin answers the pointer"
            );
            assert_eq!(
                hit_tab_chrome(width, scale, &resting(2), 0, 0.0, open_x, open_y),
                Some(ChromeTarget::Tab(0)),
                "scale {scale}: a pin that is not drawn is not pressable"
            );
            // And the two never trade places: the pin is left of the ×, always.
            let close = tab_strip_geometry(width, scale, &revealed, 0, 0.0).tabs[0]
                .close
                .expect("an unpinned Full-tier tab keeps its ×");
            assert!(open[2] <= close[0], "scale {scale}: pin, then ×");
        }
    }

    /// PIN — F49: the resting pin costs the row nothing, and the -4px it leaves
    /// behind lines the pinned row up with the unpinned one.
    ///
    /// The flex arithmetic is written out at [`tab_trailing_edge`]; this is its
    /// measurement. `width: 0` with `margin-left: -8px` "cancel[s] this item's
    /// share of the flex gap" (mock-up 339), so an invisible pin takes no room —
    /// but it is still a sibling, so `.pin + .close`'s -4px applies, and the
    /// badge docks 4px closer to the `×` than the row's own gap would put it. A
    /// pinned tab's lone `.pin.on` carries the same -4px (353-357) so that the
    /// two rows agree.
    ///
    /// Red gate: the mock-up's own — "the two counts sat 4px apart".
    #[test]
    fn a_resting_pin_costs_the_row_nothing_and_lines_the_two_rows_up() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let width = 960.0 * scale;
            let gap = WINDOW_TAB_GAP_LOGICAL_PX * scale;
            let tightened = gap - WINDOW_TAB_TRAILER_TIGHTEN_LOGICAL_PX * scale;
            let rest = tab_strip_geometry(width, scale, &resting(2), 0, 0.0).tabs[0];
            assert!(
                rest.pin.is_none(),
                "scale {scale}: a zero-width box is no box at all"
            );
            let close = rest.close.expect("an unpinned Full-tier tab has its ×");
            let badge = tab_badge_rect(&rest, 3, 0.0, scale).expect("three panes wear a badge");
            assert!(
                (badge[2] - (close[0] - tightened)).abs() < 0.51,
                "scale {scale}: the badge docks across the tightened gap, saw {} against {}",
                badge[2],
                close[0] - tightened
            );
            assert!(
                badge[2] > close[0] - gap,
                "scale {scale}: the resting pin's -8px cancels its own gap, so nothing \
                 stands between the badge and the × but the cluster's own 4px"
            );
            let pinned = tab_strip_geometry(
                width,
                scale,
                &[
                    TabTrailer {
                        pinned: true,
                        reveal: 0.0,
                    },
                    TabTrailer::default(),
                ],
                0,
                0.0,
            )
            .tabs[0];
            assert_eq!(
                tab_badge_rect(&pinned, 3, 0.0, scale),
                Some(badge),
                "scale {scale}: mock-up 353-357 — the two counts must not sit 4px apart"
            );
        }
    }

    /// PIN — F49: a full reveal opens exactly one 17px box, four pixels clear of
    /// the `×`, and the row pays it 25px — the box itself plus the 8px gap its
    /// `margin-left: -8px` had been cancelling.
    ///
    /// Red gate: reading the animation as `width: 0 -> 17` alone, which moves the
    /// badge by 17 and leaves the pin and the badge touching. The mock-up animates
    /// two properties (lines 347-348) and both are layout.
    #[test]
    fn a_revealed_pin_opens_one_box_and_the_row_pays_it_that_box_and_a_gap() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let width = 960.0 * scale;
            let box_px = (WINDOW_TAB_PIN_BOX_LOGICAL_PX * scale).round();
            let gap = WINDOW_TAB_GAP_LOGICAL_PX * scale;
            let tighten = WINDOW_TAB_TRAILER_TIGHTEN_LOGICAL_PX * scale;
            let open_at = |reveal: f32| {
                tab_strip_geometry(
                    width,
                    scale,
                    &[
                        TabTrailer {
                            pinned: false,
                            reveal,
                        },
                        TabTrailer::default(),
                    ],
                    0,
                    0.0,
                )
                .tabs[0]
            };
            let open = open_at(1.0);
            let pin = open.pin.expect("a full reveal opens the box");
            let close = open.close.expect("an unpinned tab keeps its ×");
            assert!(
                (pin[2] - pin[0] - box_px).abs() < 0.01,
                "scale {scale}: `.tab:hover .pin {{ width: 17px }}`, saw {}",
                pin[2] - pin[0]
            );
            assert_eq!(
                [pin[1], pin[3]],
                [close[1], close[3]],
                "scale {scale}: one box on one axis with the × beside it"
            );
            assert!(
                (close[0] - pin[2] - tighten).abs() < 0.01,
                "scale {scale}: `.pin + .close {{ margin-left: -4px }}` — 8 - 4 = 4px \
                 between them, saw {}",
                close[0] - pin[2]
            );
            // Half open is half a box, and the badge has moved half of the 25px.
            let half = open_at(0.5).pin.expect("a half reveal is still a box");
            assert!(
                (half[2] - half[0] - box_px / 2.0).abs() < 0.01,
                "scale {scale}: the width animates, it does not switch"
            );
            let badge_right = |tab: &TabGeometry| {
                tab_badge_rect(tab, 3, 0.0, scale).expect("three panes wear a badge")[2]
            };
            let rest = tab_strip_geometry(width, scale, &resting(2), 0, 0.0).tabs[0];
            let paid = badge_right(&rest) - badge_right(&open);
            assert!(
                (paid - (box_px + gap)).abs() < 1.01,
                "scale {scale}: the reveal costs the row its box and the gap it had been \
                 cancelling, saw {paid} against {}",
                box_px + gap
            );
            let half_paid = badge_right(&rest) - badge_right(&open_at(0.5));
            assert!(
                (half_paid - (box_px + gap) / 2.0).abs() < 1.01,
                "scale {scale}: and it is paid continuously, saw {half_paid}"
            );
        }
    }

    /// PIN — F52/F53: the drawn pin. A 13px glyph in the 17px box (mock-up 365),
    /// filled for the state and outlined for the offer, and the state is the
    /// darker of the two.
    ///
    /// The sizes are the mock-up's own argument, quoted at 362-364: "the pin
    /// carries a state and a glyph that has to survive a 45° turn, and both cost
    /// silhouette. It is not the close button's twin and sizing it like one made
    /// it read as lint." So the assertion is not merely `== 13`; it is also
    /// `!= the ×'s 8`.
    #[test]
    fn the_drawn_pin_is_thirteen_pixels_filled_for_a_state_and_outlined_for_an_offer() {
        let palette = chrome_palette();
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let glyph = (WINDOW_TAB_PIN_GLYPH_LOGICAL_PX * scale).round();
            assert_ne!(
                glyph,
                (WINDOW_TAB_CLOSE_GLYPH_LOGICAL_PX * scale).round(),
                "scale {scale}: the pin is not the close button's twin"
            );
            let pin_sprite = |sprites: &[ChromeSprite], filled: bool| {
                sprites
                    .iter()
                    .find(|sprite| sprite.mark == ChromeMark::Pin { filled })
                    .copied()
            };

            // Pinned, untouched: filled, `--ink`, and never faded.
            let tabs = [
                pinnable_tab(TabTrailer {
                    pinned: true,
                    reveal: 0.0,
                }),
                pinnable_tab(TabTrailer::default()),
            ];
            let (_, _, sprites) = strip_chrome_of(scale, &tabs, 0, 0.0, None, false);
            let state = pin_sprite(&sprites, true).expect("a pinned tab draws its pin");
            assert_eq!(
                [state.rect[2] - state.rect[0], state.rect[3] - state.rect[1]],
                [glyph, glyph],
                "scale {scale}: `.pinsvg {{ width: 13px; height: 13px }}`"
            );
            let box_rect = tab_strip_geometry(
                960.0 * scale,
                scale,
                &[tabs[0].trailer, tabs[1].trailer],
                0,
                0.0,
            )
            .tabs[0]
                .pin
                .expect("the pinned tab has a box to centre it in");
            assert!(
                ((state.rect[0] + state.rect[2]) / 2.0 - (box_rect[0] + box_rect[2]) / 2.0).abs()
                    <= 0.5,
                "scale {scale}: `justify-content: center` inside the 17px box"
            );
            // `.tab .pin.on { color: var(--ink) }` — over the surface the tab is
            // wearing, and `strip_chrome_of` makes this pinned tab the active
            // one, so the ground is `--termbg` with no pill on it.
            assert_eq!(
                state.color, palette.tab_pin_state_on_active_tab,
                "scale {scale}: `.tab .pin.on {{ color: var(--ink) }}` over `--termbg`"
            );
            assert_eq!(
                state.opacity, 1.0,
                "scale {scale}: a fact about the tab does not fade in"
            );
            assert!(
                pin_sprite(&sprites, false).is_none(),
                "scale {scale}: an unrevealed neighbour draws no pin"
            );

            // Unpinned and untouched: no furniture at all.
            let quiet = [pinnable_tab(TabTrailer::default())];
            let (_, _, quiet_sprites) = strip_chrome_of(scale, &quiet, 0, 0.0, None, false);
            assert!(
                !quiet_sprites
                    .iter()
                    .any(|sprite| matches!(sprite.mark, ChromeMark::Pin { .. })),
                "scale {scale}: a strip of ordinary tabs carries no extra furniture"
            );

            // Unpinned and revealed: outlined, `--ink3` — the `×`'s own ink over
            // the `×`'s own ground, which is the whole reason the pin borrows
            // that field rather than owning a second copy of it.
            let offered = [pinnable_tab(TabTrailer {
                pinned: false,
                reveal: 1.0,
            })];
            let (_, _, offer_sprites) = strip_chrome_of(scale, &offered, 0, 0.0, None, false);
            let offer = pin_sprite(&offer_sprites, false).expect("a revealed pin is drawn");
            assert_eq!(
                offer.color, palette.tab_close_glyph_on_active_tab,
                "scale {scale}: `.tab .pin {{ color: var(--ink3) }}`, mixed like the `×`'s"
            );
            assert_ne!(
                palette.tab_close_glyph_on_active_tab, palette.tab_pin_state_on_active_tab,
                "the state is darker than the action — on one and the same ground"
            );

            // The other two grounds a pinned pin can stand on. A pinned tab that
            // is neither active nor hovered is the only place the strip's own
            // `--panel` mixes are still the right answer, and the hovered one is
            // where a reading that reused the `×`'s pill entry would pass by
            // coincidence — so both are stated.
            let neighbour = [
                pinnable_tab(TabTrailer::default()),
                pinnable_tab(TabTrailer {
                    pinned: true,
                    reveal: 0.0,
                }),
            ];
            let pinned_ink = |hover: Option<ChromeTarget>| {
                let (_, _, sprites) = strip_chrome_of(scale, &neighbour, 0, 0.0, hover, false);
                pin_sprite(&sprites, true)
                    .expect("the pinned neighbour draws its pin")
                    .color
            };
            assert_eq!(
                pinned_ink(None),
                palette.title_text_hover,
                "scale {scale}: on a resting tab that ink is `--ink` over `--panel`"
            );
            assert_eq!(
                pinned_ink(Some(ChromeTarget::Tab(1))),
                palette.tab_pin_state_on_hovered_tab,
                "scale {scale}: and over that tab's own `--hover` fill once the \
                 pointer is inside it"
            );
        }
    }

    /// PIN — F52: the pin fades with its own reveal, and waits for a box that can
    /// hold it.
    ///
    /// `transition: … opacity .12s ease` (mock-up 341) is the fade. The waiting is
    /// this pipeline's answer to `overflow: hidden`: a chrome mark is rasterised
    /// into the box it fills and drawn with whole-texture UVs, so a half-open box
    /// cannot crop one — the same ruling [`within_strip`] records. A 13px glyph
    /// centred in a 3px box would spill five pixels over the title on each side,
    /// so it arrives once the box can hold it, inside the fade already running.
    #[test]
    fn a_pin_fades_with_its_reveal_and_waits_for_a_box_that_can_hold_it() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let drawn = |reveal: f32| {
                let tabs = [pinnable_tab(TabTrailer {
                    pinned: false,
                    reveal,
                })];
                let (_, _, sprites) = strip_chrome_of(scale, &tabs, 0, 0.0, None, false);
                sprites
                    .iter()
                    .find(|sprite| matches!(sprite.mark, ChromeMark::Pin { .. }))
                    .copied()
            };
            assert!(
                drawn(0.2).is_none(),
                "scale {scale}: a sliver of a box holds no 13px glyph"
            );
            let nearly = drawn(0.9).expect("a nearly-open box holds it");
            assert!(
                (nearly.opacity - 0.9).abs() < 1e-6,
                "scale {scale}: the fade is the reveal, saw {}",
                nearly.opacity
            );
            assert_eq!(
                drawn(1.0).expect("a full reveal is drawn").opacity,
                1.0,
                "scale {scale}: and it arrives whole"
            );
        }
    }

    /// PIN — F53: the pin answers the pointer exactly the way the `×` beside it
    /// does — the same `--active` pill at the same 4px of round, over whichever
    /// surface its tab is showing — and a pointer on it is still a pointer on the
    /// tab, so the body lights up under it.
    #[test]
    fn the_pin_wears_the_close_buttons_own_hover_pill_and_lights_its_tab_with_it() {
        let palette = chrome_palette();
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let radius = (WINDOW_TAB_PIN_RADIUS_LOGICAL_PX * scale).round() as u32;
            let trailers = [
                TabTrailer {
                    pinned: true,
                    reveal: 0.0,
                },
                TabTrailer {
                    pinned: true,
                    reveal: 0.0,
                },
            ];
            let tabs = [pinnable_tab(trailers[0]), pinnable_tab(trailers[1])];
            let geometry = tab_strip_geometry(960.0 * scale, scale, &trailers, 0, 0.0);
            for (index, expected, lit) in [
                (
                    0,
                    palette.tab_close_pill_on_content,
                    palette.tab_close_glyph_on_pill_over_active_tab,
                ),
                (
                    1,
                    palette.tab_close_pill_on_hovered_tab,
                    palette.tab_close_glyph_on_pill_over_hovered_tab,
                ),
            ] {
                let pin = geometry.tabs[index]
                    .pin
                    .expect("a pinned tab wears its pin");
                let (_, _, sprites) = strip_chrome_of(
                    scale,
                    &tabs,
                    0,
                    0.0,
                    Some(ChromeTarget::TabPin(index)),
                    false,
                );
                let pill = sprites
                    .iter()
                    .find(|sprite| sprite.rect == pixel_snapped(pin))
                    .expect("the pin fills its box under the pointer");
                assert_eq!(
                    pill.mark,
                    ChromeMark::ControlPill { radius_px: radius },
                    "scale {scale}: `.tab .pin {{ border-radius: 4px }}`"
                );
                assert_eq!(
                    pill.color, expected,
                    "scale {scale}: tab {index}'s pill sits on the wrong surface"
                );
                let glyph = sprites
                    .iter()
                    .filter(|sprite| matches!(sprite.mark, ChromeMark::Pin { .. }))
                    .find(|sprite| sprite.rect[0] >= pin[0] && sprite.rect[2] <= pin[2])
                    .expect("and its glyph is inside it");
                // `.tab .pin:hover { color: var(--ink) }` — and under the pointer
                // there is always a pill, so this is the `×`'s own lit ink, the
                // one tier the two controls can share outright.
                assert_eq!(
                    glyph.color, lit,
                    "scale {scale}: tab {index}'s pin glyph stands on the wrong pill"
                );
            }
            let (_, _, sprites) =
                strip_chrome_of(scale, &tabs, 0, 0.0, Some(ChromeTarget::TabPin(1)), false);
            assert!(
                sprites
                    .iter()
                    .any(|sprite| matches!(sprite.mark, ChromeMark::TabBody { .. })),
                "scale {scale}: `.tab:hover` — the pin is inside the tab"
            );
        }
    }

    /// PIN — F48: a trailer moves nothing outside its own tab.
    ///
    /// Every tab takes the same share of the run whatever it hangs off its
    /// trailing end, so the strip's bodies, its buttons, its viewport and its
    /// scroll are all deaf to the pins. That is the licence [`tab_strip_bodies`]
    /// runs on: the three questions that are about the *run* need no trailer list
    /// at all, and would otherwise have made every caller of
    /// `tab_strip_contains` invent one.
    #[test]
    fn a_trailer_moves_nothing_outside_its_own_tab() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            for count in [1_usize, 2, 6, 9, 30] {
                let width = 960.0 * scale;
                let mixed = (0..count)
                    .map(|index| TabTrailer {
                        pinned: index % 2 == 0,
                        reveal: (index % 3) as f32 / 2.0,
                    })
                    .collect::<Vec<_>>();
                let plain = tab_strip_geometry(width, scale, &resting(count), 0, 0.0);
                let trailed = tab_strip_geometry(width, scale, &mixed, 0, 0.0);
                assert_eq!(
                    plain.new_tab, trailed.new_tab,
                    "scale {scale}, {count} tabs"
                );
                assert_eq!(plain.new_tab_menu, trailed.new_tab_menu);
                assert_eq!(plain.viewport, trailed.viewport);
                assert_eq!(plain.max_scroll, trailed.max_scroll);
                for (index, (plain, trailed)) in plain.tabs.iter().zip(&trailed.tabs).enumerate() {
                    assert_eq!(
                        plain.body, trailed.body,
                        "scale {scale}, {count} tabs: tab {index}'s body moved"
                    );
                    assert_eq!(plain.tier, trailed.tier);
                }
            }
        }
    }
    // ── T5: reordering inside the strip ──

    /// Three 200px slots with the strip's own 4px gap between them, stated as
    /// plainly as the mock-up states the rule they exist to test.
    fn slot_mids_of(count: usize, pitch: f32) -> Vec<f32> {
        (0..count).map(|i| 100.0 + i as f32 * pitch).collect()
    }

    #[test]
    fn a_reorder_fires_when_the_leading_edge_has_covered_half_the_neighbour_plus_the_margin() {
        // Tab against tab, never the pointer against a tab (mock-up 6640-6658):
        // the test is the dragged tab's LEADING edge against the neighbour's
        // CENTRE, plus a tenth of half a tab of hysteresis.
        assert_eq!(
            TAB_REORDER_MARGIN, 0.1,
            "a tenth of half a tab (mock-up 6571-6576)"
        );
        let mids = slot_mids_of(3, 204.0);
        let half = 100.0;
        // Written out rather than derived from the constant: a threshold test
        // that computes its own expectation from the number it is testing agrees
        // with every value that number could take.
        let margin = 10.0;
        // The neighbour's centre is at 304; covering half of it means the leading
        // edge (visual_mid + half) has reached 304, and the margin buys 10 more.
        let fires_at = 304.0 + margin - half;
        assert_eq!(
            reorder_step(&mids, 0, fires_at, half),
            None,
            "exactly on the threshold is not yet past it"
        );
        assert_eq!(
            reorder_step(&mids, 0, fires_at + 0.5, half),
            Some(1),
            "a hair past it, and they trade places"
        );
        // Half a slot is the *travel*, not the threshold: 100 of half-tab plus the
        // 4px gap plus the 10px margin, and no more.
        assert!(
            (fires_at - mids[0] - (half + 4.0 + margin)).abs() < 1e-3,
            "the reorder costs half a tab of travel, not a whole one"
        );
    }

    #[test]
    fn a_reorder_reads_the_same_threshold_backwards() {
        let mids = slot_mids_of(3, 204.0);
        let half = 100.0;
        let margin = 10.0;
        let fires_at = mids[0] - margin + half;
        assert_eq!(reorder_step(&mids, 1, fires_at, half), None);
        assert_eq!(reorder_step(&mids, 1, fires_at - 0.5, half), Some(0));
        assert_eq!(
            reorder_step(&mids, 0, -10_000.0, half),
            None,
            "and the first slot has nothing to its left to trade with"
        );
    }

    #[test]
    fn the_margin_is_hysteresis_so_a_swapped_tab_cannot_dither_back() {
        // The algebra the mock-up spells out at 6650-6655: a forward swap makes
        // d' = d - pitch, so a swap straight back needs d < pitch - T. With T at
        // half a slot plus a margin, that window is empty — which is what makes
        // this a threshold and not a coin toss.
        let mids = slot_mids_of(3, 204.0);
        let half = 100.0;
        let mut worst = f32::NEG_INFINITY;
        for step in 0..2_000 {
            let visual_mid = mids[0] + step as f32 * 0.2;
            let Some(to) = reorder_step(&mids, 0, visual_mid, half) else {
                continue;
            };
            // Immediately re-ask from the slot it just landed in, with the tab
            // still exactly where it is: it must not want to come straight back.
            assert_ne!(
                reorder_step(&mids, to, visual_mid, half),
                Some(0),
                "swapped forward at {visual_mid} and wanted to swap straight back"
            );
            worst = worst.max(visual_mid);
        }
        assert!(
            worst > f32::NEG_INFINITY,
            "the sweep must cross a threshold"
        );
    }

    #[test]
    fn one_event_that_flings_a_tab_across_several_slots_lands_it_right() {
        // "bounded by the strip's length because each pass moves it exactly one
        // slot" (mock-up 6672-6674).
        let mids = slot_mids_of(5, 204.0);
        let unpinned = vec![false; 5];
        assert_eq!(
            reorder_target(&mids, &unpinned, 0, mids[4], 100.0),
            4,
            "carried the whole way in a single pointer event"
        );
        assert_eq!(reorder_target(&mids, &unpinned, 4, mids[0], 100.0), 0);
        assert_eq!(
            reorder_target(&mids, &unpinned, 2, mids[2], 100.0),
            2,
            "and a tab sitting in its own slot does not move at all"
        );
    }

    #[test]
    fn a_reorder_stops_dead_at_the_pinned_seam() {
        // F57. Pinned is a partition, not a decoration.
        let mids = slot_mids_of(4, 204.0);
        let pinned = [true, true, false, false];
        assert_eq!(
            reorder_target(&mids, &pinned, 3, mids[0], 100.0),
            2,
            "an unpinned tab dragged to the head of the strip stops at the seam"
        );
        assert_eq!(
            reorder_target(&mids, &pinned, 0, mids[3], 100.0),
            1,
            "and a pinned one dragged to the tail stops at the same seam"
        );
        assert_eq!(
            reorder_target(&mids, &pinned, 0, mids[1], 100.0),
            1,
            "while inside its own partition it moves freely"
        );
    }

    #[test]
    fn no_reorder_can_ever_break_the_pinned_partition() {
        // The invariant T3 stated and left for this slice to be held to.
        for pinned_count in 0..=4usize {
            let mids = slot_mids_of(4, 204.0);
            let pinned = (0..4).map(|i| i < pinned_count).collect::<Vec<_>>();
            for cur in 0..4 {
                for step in -40..=40 {
                    let visual_mid = mids[cur] + step as f32 * 20.0;
                    let to = reorder_target(&mids, &pinned, cur, visual_mid, 100.0);
                    let mut order = pinned.clone();
                    let moved = order.remove(cur);
                    order.insert(to, moved);
                    assert!(
                        crate::seed::pins_are_normalized(&order, |pinned| *pinned),
                        "{pinned:?}: dragging slot {cur} to {visual_mid} left {order:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn slot_centres_do_not_move_when_two_tabs_trade_places() {
        // The whole reason the judgement is made against layout and not paint
        // (mock-up 6659-6662): a slot is a property of the strip, so swapping
        // who stands in it changes nothing about where it is — and the FLIP that
        // is lying about positions for the next 160ms cannot reach this.
        let pinned = TabTrailer {
            pinned: true,
            reveal: 1.0,
        };
        let open = TabTrailer {
            pinned: false,
            reveal: 0.5,
        };
        let before = tab_strip_geometry(960.0, 1.0, &[pinned, open, open], 0, 0.0);
        let after = tab_strip_geometry(960.0, 1.0, &[open, pinned, open], 1, 0.0);
        let mids = tab_slot_mids(&before);
        assert_eq!(mids, tab_slot_mids(&after));
        // And they are the slots' own centres: each one inside the tab it
        // belongs to, one even pitch apart, which is the property the reorder's
        // whole threshold arithmetic rests on.
        for (mid, slot) in mids.iter().zip(&before.tabs) {
            assert!(
                *mid > slot.body[0] && *mid < slot.body[2],
                "{mid} is not inside {:?}",
                slot.body
            );
        }
        let pitch = mids[1] - mids[0];
        assert_eq!(pitch, mids[2] - mids[1], "one even pitch down the run");
        assert!(
            pitch > before.tabs[0].body[2] - before.tabs[0].body[0],
            "a pitch is a tab plus the gap beside it"
        );
    }

    #[test]
    fn an_insertion_index_is_the_first_centre_the_pointer_has_not_passed() {
        // K126, the public part of the future cross-boundary drop.
        let mids = slot_mids_of(3, 204.0);
        assert_eq!(insert_index_at(&mids, mids[0] - 1.0), 0);
        assert_eq!(
            insert_index_at(&mids, mids[0]),
            1,
            "on the centre is past it: `pos < mid` and not `<=`"
        );
        assert_eq!(insert_index_at(&mids, mids[0] + 1.0), 1);
        assert_eq!(insert_index_at(&mids, mids[1] + 1.0), 2);
        assert_eq!(
            insert_index_at(&mids, mids[2] + 1.0),
            3,
            "past the last one"
        );
        assert_eq!(insert_index_at(&[], 0.0), 0, "an empty strip has one slot");
    }

    #[test]
    fn a_shifted_tab_moves_every_box_it_holds_and_changes_nothing_else() {
        // K114: a drag is paint, so it moves the whole tab and none of its facts.
        let trailer = TabTrailer {
            pinned: false,
            reveal: 1.0,
        };
        let geometry = tab_strip_geometry(960.0, 1.0, &[trailer, trailer], 0, 0.0);
        let slot = geometry.tabs[0];
        let moved = slot.shifted(31.0);
        assert_eq!(moved.body[0], slot.body[0] + 31.0);
        assert_eq!(moved.body[2], slot.body[2] + 31.0);
        assert_eq!([moved.body[1], moved.body[3]], [slot.body[1], slot.body[3]]);
        for (moved, slot) in [(moved.close, slot.close), (moved.pin, slot.pin)] {
            let (moved, slot) = (moved.expect("a full tab"), slot.expect("a full tab"));
            assert_eq!(moved[0], slot[0] + 31.0);
            assert_eq!(moved[2], slot[2] + 31.0);
            assert_eq!([moved[1], moved[3]], [slot[1], slot[3]]);
        }
        assert_eq!(moved.tier, slot.tier);
        assert_eq!(moved.trailer, slot.trailer);
    }

    /// The strip with one tab picked up and carried `offset` pixels along it.
    fn strip_chrome_grabbed(
        tabs: &[TabContent],
        active_tab: usize,
        grabbed: Option<usize>,
    ) -> (Vec<ChromeQuad>, Vec<ChromeLabel>, Vec<ChromeSprite>) {
        let scale = 1.0;
        let dpi_milli = 1_000;
        let metrics = seat_metrics(dpi_milli);
        let seats = Seats::lone_terminal();
        let layout = solved(&seats, viewport_of(960, 600, dpi_milli), &metrics);
        build_chrome_for_tabs(
            &seats,
            &layout,
            scale,
            ChromePointer {
                hover: None,
                dragging: None,
                ..ChromePointer::default()
            },
            ChromeContent {
                tabs,
                active_tab,
                grabbed,
                tab_scroll: 0.0,
                preview_title: None,
                terminal_cwd: None,
                preview_message: None,
                fit_overflow: None,
                profile_menu_open: false,
                chevron_turn: 0.0,
            },
        )
    }

    fn plain_tabs(count: usize) -> Vec<TabContent> {
        (0..count)
            .map(|index| TabContent {
                title: format!("tab {index}"),
                pane_count: 1,
                badge_text_width: 0.0,
                mark: TabMarkState::default(),
                trailer: TabTrailer::default(),
                offset: 0.0,
                landing: 0.0,
                edit: None,
            })
            .collect()
    }

    /// Where in the paint order one named tab's own mark went down.
    ///
    /// By its slot's own address rather than by "the first mark in the list",
    /// because the list's order is exactly what these tests are about.
    fn mark_paint_index(sprites: &[ChromeSprite], count: usize, scale: f32, tab: usize) -> usize {
        let geometry = tab_strip_geometry(960.0 * scale, scale, &resting(count), 0, 0.0);
        let left = tab_mark_left(&geometry.tabs[tab], scale);
        sprites
            .iter()
            .position(|sprite| {
                sprite.mark == ChromeMark::ProfilePowerShell && sprite.rect[0] == left
            })
            .expect("every tab draws its mark")
    }

    #[test]
    fn the_tab_in_hand_is_painted_above_the_active_one() {
        // `.tab.grabbed { z-index: 20 }` against `.tab.active { z-index: 1 }`
        // (mock-up 971, 216), in a painter's-algorithm list.
        let tabs = plain_tabs(3);
        let (_, _, resting) = strip_chrome_grabbed(&tabs, 1, None);
        let active = resting
            .iter()
            .position(|sprite| matches!(sprite.mark, ChromeMark::ActiveTab { .. }))
            .expect("the active tab wears its silhouette");
        assert!(
            mark_paint_index(&resting, 3, 1.0, 0) < active,
            "at rest the active tab is the last one down"
        );
        let (_, _, grabbed) = strip_chrome_grabbed(&tabs, 1, Some(0));
        let active = grabbed
            .iter()
            .position(|sprite| matches!(sprite.mark, ChromeMark::ActiveTab { .. }))
            .expect("the active tab wears its silhouette");
        assert!(
            mark_paint_index(&grabbed, 3, 1.0, 0) > active,
            "the tab in hand passes over the active tab, not under it"
        );
    }

    #[test]
    fn picking_a_tab_up_moves_that_tab_and_nothing_else() {
        // K122's other half: the strip does not re-lay out around a tab that is
        // only being *drawn* somewhere else. Every box in the dragged tab moves
        // by exactly the offset, and every box outside it does not move at all.
        let mut carried = plain_tabs(3);
        carried[0].offset = 37.0;
        let (_, resting_labels, resting_sprites) = strip_chrome_grabbed(&plain_tabs(3), 0, Some(0));
        let (_, moved_labels, moved_sprites) = strip_chrome_grabbed(&carried, 0, Some(0));
        assert_eq!(resting_sprites.len(), moved_sprites.len());
        assert_eq!(resting_labels.len(), moved_labels.len());
        let first_slot_right = tab_strip_geometry(960.0, 1.0, &resting(3), 0, 0.0).tabs[0].body[2];
        for (resting, moved) in resting_sprites.iter().zip(&moved_sprites) {
            let dx = if resting.rect[0] < first_slot_right {
                37.0
            } else {
                0.0
            };
            assert_eq!(moved.rect[0], resting.rect[0] + dx, "{:?}", resting.mark);
            assert_eq!(moved.rect[2], resting.rect[2] + dx, "{:?}", resting.mark);
            assert_eq!(
                [moved.rect[1], moved.rect[3]],
                [resting.rect[1], resting.rect[3]]
            );
        }
        for (resting, moved) in resting_labels.iter().zip(&moved_labels) {
            let dx = if resting.rect[0] < first_slot_right {
                37.0
            } else {
                0.0
            };
            assert_eq!(moved.rect[0], resting.rect[0] + dx, "{}", resting.text);
        }
    }

    #[test]
    fn a_landing_tab_wears_the_accent_wash_and_its_inset_ring() {
        // K121, straight off `@keyframes tab-land` (mock-up 961-967).
        let palette = chrome_palette();
        // The ring's DPI-rounded width, written out at each scale rather than
        // recomputed from the constant it is meant to pin.
        for (scale, ring_px) in [(1.0_f32, 2_u32), (1.5, 2), (2.0, 3)] {
            let mut tabs = plain_tabs(2);
            tabs[0].landing = 1.0;
            let dpi_milli = (scale * 1_000.0) as u32;
            let metrics = seat_metrics(dpi_milli);
            let seats = Seats::lone_terminal();
            let layout = solved(
                &seats,
                viewport_of((960.0 * scale) as u32, (600.0 * scale) as u32, dpi_milli),
                &metrics,
            );
            let (_, _, sprites) = build_chrome_for_tabs(
                &seats,
                &layout,
                scale,
                ChromePointer {
                    hover: None,
                    dragging: None,
                    ..ChromePointer::default()
                },
                ChromeContent {
                    tabs: &tabs,
                    active_tab: 0,
                    grabbed: None,
                    tab_scroll: 0.0,
                    preview_title: None,
                    terminal_cwd: None,
                    preview_message: None,
                    fit_overflow: None,
                    profile_menu_open: false,
                    chevron_turn: 0.0,
                },
            );
            let wash = sprites
                .iter()
                .position(|sprite| {
                    matches!(sprite.mark, ChromeMark::TabBody { .. })
                        && sprite.color == palette.accent
                })
                .expect("the wash");
            let ring = sprites
                .iter()
                .position(|sprite| matches!(sprite.mark, ChromeMark::TabBodyRing { .. }))
                .expect("the ring");
            assert!(
                (sprites[wash].opacity - 0.09).abs() < 1e-6,
                "the accent at 9%, and the constant is not allowed to define itself"
            );
            assert!(
                (sprites[ring].opacity - 0.45).abs() < 1e-6,
                "the accent at 45%"
            );
            assert_eq!(sprites[ring].color, palette.accent);
            let ChromeMark::TabBodyRing { stroke_px, .. } = sprites[ring].mark else {
                unreachable!("matched above");
            };
            assert_eq!(
                stroke_px, ring_px,
                "the inset ring is 1.5 logical pixels wide at scale {scale}"
            );
            let silhouette = sprites
                .iter()
                .position(|sprite| matches!(sprite.mark, ChromeMark::ActiveTab { .. }))
                .expect("the landing tab is the active one");
            let mark = mark_paint_index(&sprites, 2, scale, 0);
            assert!(
                silhouette < wash && wash < ring && ring < mark,
                "a background and an inset shadow go over the surface and under the content"
            );
        }
    }

    #[test]
    fn a_tab_that_is_not_landing_draws_no_wash_at_all() {
        let (_, _, sprites) = strip_chrome_grabbed(&plain_tabs(2), 0, None);
        assert!(
            !sprites
                .iter()
                .any(|sprite| matches!(sprite.mark, ChromeMark::TabBodyRing { .. })),
            "the landing ring exists only while a landing is running"
        );
        let palette = chrome_palette();
        assert!(
            !sprites
                .iter()
                .any(|sprite| matches!(sprite.mark, ChromeMark::TabBody { .. })
                    && sprite.color == palette.accent),
            "and so does the wash"
        );
    }

    // ---------------------------------------------------------------------
    // U1 — the pane head's `×`, and who wears a head at all
    // ---------------------------------------------------------------------

    /// A tree with a terminal beside a files column, so a pane head exists to
    /// measure and a fixed leaf exists to prove `seat_wears_head` is per-seat.
    fn term_beside_files() -> Seats {
        Seats::from_persisted(&LayoutNodeV1::Split(SplitNodeV1 {
            dir: SplitDirV1::Row,
            ratio: 700_000,
            children: [
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                    profile_id: "pwsh".to_owned(),
                    cwd: String::new(),
                    manual_name: None,
                }))),
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Files(
                    bt_persist::FilesLeafV1 {
                        root: "D:\\".to_owned(),
                        open: Vec::new(),
                        sel: None,
                        width: 240,
                    },
                ))),
            ],
        }))
        .expect("a two-leaf tree restores")
    }

    fn term_leaf() -> Box<LayoutNodeV1> {
        Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
            profile_id: "pwsh".to_owned(),
            cwd: String::new(),
            manual_name: None,
        })))
    }

    /// Three terminals as `a | (b | c)`, so one split's slot excludes a pane.
    fn three_in_a_row() -> Seats {
        Seats::from_persisted(&LayoutNodeV1::Split(SplitNodeV1 {
            dir: SplitDirV1::Row,
            ratio: 500_000,
            children: [
                term_leaf(),
                Box::new(LayoutNodeV1::Split(SplitNodeV1 {
                    dir: SplitDirV1::Row,
                    ratio: 500_000,
                    children: [term_leaf(), term_leaf()],
                })),
            ],
        }))
        .expect("a three-leaf tree restores")
    }

    fn head_chrome(
        seats: &Seats,
        layout: &SeatLayout,
        scale: f32,
        pointer: ChromePointer,
    ) -> (Vec<ChromeQuad>, Vec<ChromeLabel>, Vec<ChromeSprite>) {
        build_chrome_for_tabs(
            seats,
            layout,
            scale,
            pointer,
            ChromeContent {
                tabs: &[TabContent {
                    title: "PowerShell".to_owned(),
                    pane_count: seats.pane_count(),
                    badge_text_width: 0.0,
                    mark: TabMarkState::default(),
                    trailer: TabTrailer::default(),
                    offset: 0.0,
                    landing: 0.0,
                    edit: None,
                }],
                active_tab: 0,
                grabbed: None,
                tab_scroll: 0.0,
                preview_title: None,
                terminal_cwd: None,
                preview_message: None,
                fit_overflow: None,
                profile_menu_open: false,
                chevron_turn: 0.0,
            },
        )
    }

    /// Everything below the 40px window title bar — that is, the pane layer.
    ///
    /// The tests below need it because the strip above draws from the *same*
    /// palette: a pane head and the active tab stand on one surface, so the
    /// tab's own close pill and the head's are one colour by construction, and
    /// `--panel` is both the title bar and the termhost floor. Filtering by
    /// colour alone would keep finding the strip and calling it a pane.
    fn in_the_pane_layer(rect: [f32; 4], scale: f32) -> bool {
        rect[1] >= (WINDOW_TITLE_BAR_LOGICAL_PX * scale).round()
    }

    fn device_rect_of(layout: &SeatLayout, seat: SeatId) -> [f32; 4] {
        let rect = layout
            .get(seat)
            .and_then(|placement| placement.device_rect)
            .expect("a presented seat has a device rectangle");
        [
            rect.left as f32,
            rect.top as f32,
            rect.right as f32,
            rect.bottom as f32,
        ]
    }

    /// C25/C26: two rules for one head, and the difference is the whole point.
    ///
    /// A terminal earns its head by having a sibling; a files pane draws one
    /// unconditionally, because a tree that does not say where it is rooted is
    /// useless and because that head carries the pane's other two verbs.
    ///
    /// Red gate: collapse the two rules back into `has_pane_headers()` and the
    /// second assertion fails — which is the state this build shipped in.
    #[test]
    fn a_files_pane_always_wears_a_head_and_a_lone_terminal_never_does() {
        let lone = Seats::lone_terminal();
        assert!(!lone.seat_wears_head(SeatKind::Terminal));
        assert!(
            lone.seat_wears_head(SeatKind::Files),
            "a files pane's head does not depend on the company it keeps"
        );

        let split = term_beside_files();
        assert!(split.seat_wears_head(SeatKind::Terminal));
        assert!(split.seat_wears_head(SeatKind::Files));
    }

    /// C30/C27: the `×`'s box, read off the mock-up's own declaration.
    ///
    /// `margin-left: auto` puts it against the head's trailing padding, and
    /// `padding: 0 6px 0 12px` says that padding is six, not the twelve the
    /// leading side gets. Checked at four scales because the whole reason this
    /// geometry is one function is that a second copy rounds differently.
    #[test]
    fn the_pane_close_button_stands_in_the_mock_ups_own_box() {
        // Spelled out rather than read back off the constants the geometry
        // itself reads: an expectation derived from the value under test is a
        // tautology that passes on every value, which is exactly what this
        // assertion did in its first draft — the trailing padding could be
        // silently re-read as the leading 12 and nothing noticed.
        assert_eq!(SEAT_PANE_CLOSE_BOX_LOGICAL_PX, 17.0);
        assert_eq!(SEAT_PANE_CLOSE_RADIUS_LOGICAL_PX, 4.0);
        assert_eq!(SEAT_PANE_CLOSE_GLYPH_LOGICAL_PX, 8.0);
        assert_eq!(SEAT_TITLE_TRAILING_PADDING_LOGICAL_PX, 6.0);
        assert_ne!(
            SEAT_TITLE_TRAILING_PADDING_LOGICAL_PX, SEAT_TITLE_PADDING_LOGICAL_PX,
            "`padding: 0 6px 0 12px` is two numbers: words get twelve, buttons six"
        );

        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let rect = [100.0_f32, 40.0, 700.0, 500.0];
            let head = pane_head_geometry(rect, SeatKind::Terminal, scale);
            let close = head.close.expect("a 600px head seats a 17px button");
            let box_px = (17.0 * scale).round();

            assert_eq!(close[2] - close[0], box_px, "17 logical pixels wide");
            assert_eq!(close[3] - close[1], box_px, "and square");
            assert_eq!(
                close[2],
                (rect[2] - 6.0 * scale).round(),
                "flush against `padding-right: 6px`, not the leading 12"
            );
            // Centred in the *content* box — the border box less its hairline —
            // which is what `align-items: center` centres in.
            let slack = (head.content_bottom - rect[1]) - box_px;
            assert!(
                (close[1] - (rect[1] + slack / 2.0)).abs() <= 1.0,
                "vertically centred at {scale}x"
            );
            // C29: the title yields, the control does not. The label's box stops
            // before the button rather than running under it.
            assert!(
                head.title[2] <= close[0],
                "the title's box ends before the `×` begins at {scale}x"
            );
            assert!(head.title[0] >= head.mark[2], "and starts after the mark");
        }
    }

    /// C35/I110: the `×` is a dead zone inside the drag handle, so it has to win
    /// the hit test against the head it sits on.
    ///
    /// Red gate: drop the `PaneClose` arm from `hit_chrome` and every press on
    /// the button answers `PaneHeader` — a press that will one day start a drag
    /// instead of closing the pane.
    #[test]
    fn pressing_the_close_button_is_not_pressing_the_head_it_sits_on() {
        let seats = term_beside_files();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1200, 800, 1_000);
        let layout = solved(&seats, viewport, &metrics);
        let terminal = layout.rects[0].id;
        let head = pane_head_geometry(device_rect_of(&layout, terminal), SeatKind::Terminal, 1.0);
        let close = head.close.expect("the head is wide enough");

        let middle_y = f64::from((close[1] + close[3]) / 2.0);
        assert_eq!(
            hit_chrome(
                &seats,
                &layout,
                1.0,
                f64::from((close[0] + close[2]) / 2.0),
                middle_y
            ),
            Some(ChromeTarget::PaneClose(terminal)),
        );
        // One pixel to the left of the box is the head again — the boundary is
        // where the drawing says it is, not a pixel either side of it.
        assert_eq!(
            hit_chrome(&seats, &layout, 1.0, f64::from(close[0]) - 1.0, middle_y),
            Some(ChromeTarget::PaneHeader(terminal)),
        );
        assert_eq!(
            hit_chrome(&seats, &layout, 1.0, f64::from(close[0]), middle_y),
            Some(ChromeTarget::PaneClose(terminal)),
            "the box is half-open on its leading edge, like every other target"
        );
    }

    /// C30/C33: `visibility: hidden` until `.pane:hover`, and the reveal is the
    /// pane's, not the head's.
    ///
    /// The second half is the one that could quietly go wrong: keyed on the
    /// *head* being hovered, the `×` would appear only once the pointer was
    /// already on it — the "you have to know it is there" bug. So the pointer is
    /// put in the pane's **body**, where `hover` is `None` because a terminal is
    /// not chrome, and the button must still be there.
    #[test]
    fn the_close_button_is_not_there_until_the_pointer_is_in_the_pane() {
        let seats = term_beside_files();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1200, 800, 1_000);
        let layout = solved(&seats, viewport, &metrics);
        let terminal = layout.rects[0].id;

        let has_close = |pointer: ChromePointer| {
            head_chrome(&seats, &layout, 1.0, pointer)
                .2
                .iter()
                .any(|sprite| sprite.mark == ChromeMark::PaneClose)
        };

        assert!(
            !has_close(ChromePointer::default()),
            "an idle split header stays quiet"
        );
        assert!(
            has_close(ChromePointer {
                pane_hover: Some(terminal),
                hover: None,
                ..ChromePointer::default()
            }),
            "the pointer anywhere in the pane reveals it, terminal body included"
        );
    }

    /// C30 again, on the ink: `--ink3` at rest, `--ink` on `--active` under the
    /// pointer — and the pill is drawn only under the pointer, never at rest.
    #[test]
    fn the_close_buttons_ink_answers_to_whether_it_is_under_the_pointer() {
        let seats = term_beside_files();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1200, 800, 1_000);
        let layout = solved(&seats, viewport, &metrics);
        let terminal = layout.rects[0].id;
        let palette = chrome_palette();

        let glyph_and_pill = |hover: Option<ChromeTarget>| {
            let (_, _, sprites) = head_chrome(
                &seats,
                &layout,
                1.0,
                ChromePointer {
                    pane_hover: Some(terminal),
                    hover,
                    ..ChromePointer::default()
                },
            );
            let glyph = sprites
                .iter()
                .find(|sprite| sprite.mark == ChromeMark::PaneClose)
                .map(|sprite| sprite.color);
            let pill = sprites
                .iter()
                .find(|sprite| {
                    matches!(sprite.mark, ChromeMark::ControlPill { .. })
                        && sprite.color == palette.pane_close_pill
                        && in_the_pane_layer(sprite.rect, 1.0)
                })
                .map(|sprite| sprite.rect);
            (glyph, pill)
        };

        let (resting, no_pill) = glyph_and_pill(Some(ChromeTarget::PaneHeader(terminal)));
        assert_eq!(resting, Some(palette.pane_close_glyph));
        assert_eq!(no_pill, None, "no fill until the pointer is on the button");

        let (lit, pill) = glyph_and_pill(Some(ChromeTarget::PaneClose(terminal)));
        assert_eq!(lit, Some(palette.pane_close_glyph_on_pill));
        // The pill is the button's own box, so the lit area is exactly the area
        // that answers the press.
        let head = pane_head_geometry(device_rect_of(&layout, terminal), SeatKind::Terminal, 1.0);
        assert_eq!(pill, head.close);
    }

    /// D39: the mark recedes on panes you are not in, through `opacity` — a
    /// channel of its own, so it cannot collide with the accent or the breathing.
    ///
    /// Opacity rather than a paler ink is load-bearing, and the test says why by
    /// leaving the colour alone: a terminal's mark is a profile square carrying
    /// colours no palette entry can restate, and D38 rules that focus must not
    /// move a hue.
    #[test]
    fn the_pane_mark_recedes_on_a_pane_you_are_not_in() {
        let seats = term_beside_files();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1200, 800, 1_000);
        let layout = solved(&seats, viewport, &metrics);
        let focused = seats.focus();

        let (_, _, sprites) = head_chrome(&seats, &layout, 1.0, ChromePointer::default());
        let marks: Vec<_> = sprites
            .iter()
            .filter(|sprite| {
                matches!(
                    sprite.mark,
                    ChromeMark::ProfilePowerShell | ChromeMark::Folder | ChromeMark::File
                ) && in_the_pane_layer(sprite.rect, 1.0)
            })
            .collect();
        assert_eq!(marks.len(), 2, "one mark per head");

        let mut saw_focused = false;
        for placement in &layout.rects {
            let rect = device_rect_of(&layout, placement.id);
            let mark = marks
                .iter()
                .find(|sprite| sprite.rect[0] >= rect[0] && sprite.rect[2] <= rect[2])
                .expect("each head's mark stands inside its own pane");
            if placement.id == focused {
                saw_focused = true;
                assert_eq!(mark.opacity, 1.0, "the pane you are in is at full strength");
            } else {
                assert_eq!(mark.opacity, PANE_MARK_UNFOCUSED_OPACITY);
            }
        }
        assert!(saw_focused, "one of the two panes holds the focus");
        assert_eq!(PANE_MARK_UNFOCUSED_OPACITY, 0.5, "`opacity: .5`");
    }

    // ---------------------------------------------------------------------
    // U2 — the divider's complete state
    // ---------------------------------------------------------------------

    /// E47, and the one carried number this slice was asked to roll back: the
    /// hit zone is seven logical pixels, which the mock-up (`width: 7px;
    /// margin: 0 -3px`) and `DESIGN.md` §7.1.1 both say and which this build
    /// spent a slice answering as six.
    ///
    /// The constant *and* the band it feeds, because pinning only the constant
    /// would leave the arithmetic that consumes it free to drift.
    #[test]
    fn the_divider_hit_zone_is_seven_logical_pixels_wide() {
        assert_eq!(SEAT_DIVIDER_HIT_LOGICAL_PX, 7.0);
        let seats = term_beside_files();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1200, 800, 1_000);
        let layout = solved(&seats, viewport, &metrics);
        let slot = seats.split_slots(&layout)[0];
        let band = hit_band(slot, 1.0);
        assert_eq!(band[2] - band[0], 7.0, "the drawn 1px, widened to seven");
        // The negative margin, stated as the property it buys: the zone reaches
        // three pixels into each neighbour while the layout still spends one.
        assert_eq!(slot.band[2] - slot.band[0], 1.0);
        assert_eq!(slot.band[0] - band[0], 3.0);
        assert_eq!(band[2] - slot.band[2], 3.0);
    }

    /// E50/E51: the grip is `opacity: 0` at rest and 1 on hover or while
    /// dragging, so it is drawn in exactly those two states and no other.
    #[test]
    fn the_divider_grip_appears_on_hover_and_while_dragging_and_never_otherwise() {
        let seats = term_beside_files();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1200, 800, 1_000);
        let layout = solved(&seats, viewport, &metrics);
        let split = seats.split_slots(&layout)[0].id;
        let palette = chrome_palette();

        let grip = |pointer: ChromePointer| {
            head_chrome(&seats, &layout, 1.0, pointer)
                .2
                .into_iter()
                .find(|sprite| {
                    matches!(sprite.mark, ChromeMark::ControlPill { .. })
                        && sprite.color == palette.divider_active
                        && in_the_pane_layer(sprite.rect, 1.0)
                })
                .map(|sprite| sprite.rect)
        };

        assert_eq!(grip(ChromePointer::default()), None, "no grip at rest");
        assert!(
            grip(ChromePointer {
                hover: Some(ChromeTarget::Divider(split)),
                ..ChromePointer::default()
            })
            .is_some(),
            "`:hover::after` turns the grip on"
        );
        assert!(
            grip(ChromePointer {
                dragging: Some(split),
                ..ChromePointer::default()
            })
            .is_some(),
            "and so does `.dragging::after`"
        );
    }

    /// E50: three logical pixels across the boundary and twenty-eight along it,
    /// centred on both — a handle on the line, not a second line.
    #[test]
    fn the_divider_grip_is_three_by_twenty_eight_centred_on_the_band() {
        let seats = term_beside_files();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1200, 800, 1_000);
        let layout = solved(&seats, viewport, &metrics);
        let slot = seats.split_slots(&layout)[0];
        let grip = divider_grip(slot, 1.0);

        assert_eq!(grip[2] - grip[0], 3.0, "across the band");
        assert_eq!(grip[3] - grip[1], 28.0, "along it");
        assert_eq!(
            (grip[0] + grip[2]) / 2.0,
            (slot.band[0] + slot.band[2]) / 2.0,
            "one pixel of grip either side of the hairline"
        );
        assert_eq!(
            (grip[1] + grip[3]) / 2.0,
            (slot.band[1] + slot.band[3]) / 2.0,
            "and halfway down it"
        );
    }

    /// E53: every divider goes quiet while some other gesture owns the pointer.
    ///
    /// A divider lighting up mid-drag is an offer that is not on the table, and
    /// it makes the offer in the very colour that during a drag means "let go
    /// and it lands here" — the one line under the pointer that means nothing at
    /// all, impersonating the one thing that does.
    ///
    /// Red gate: drop the `other_drag_in_flight` guard and the hovered divider
    /// lights and grows a grip in the middle of a tab drag.
    #[test]
    fn a_hovered_divider_says_nothing_while_something_else_is_being_dragged() {
        let seats = term_beside_files();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1200, 800, 1_000);
        let layout = solved(&seats, viewport, &metrics);
        let slot = seats.split_slots(&layout)[0];
        let palette = chrome_palette();

        let hovering = ChromePointer {
            hover: Some(ChromeTarget::Divider(slot.id)),
            ..ChromePointer::default()
        };
        let line_of = |pointer: ChromePointer| {
            head_chrome(&seats, &layout, 1.0, pointer)
                .0
                .into_iter()
                .find(|quad| quad.rect == slot.band)
                .expect("the divider is always drawn")
                .color
        };
        assert_eq!(line_of(hovering), palette.divider_hover);

        let during_a_drag = ChromePointer {
            other_drag_in_flight: true,
            ..hovering
        };
        assert_eq!(
            line_of(during_a_drag),
            palette.divider,
            "the hairline stays at rest"
        );
        assert!(
            !head_chrome(&seats, &layout, 1.0, during_a_drag)
                .2
                .iter()
                .any(
                    |sprite| matches!(sprite.mark, ChromeMark::ControlPill { .. })
                        && sprite.color == palette.divider_active
                        && in_the_pane_layer(sprite.rect, 1.0)
                ),
            "and the grip does not appear either"
        );
        // The divider being *dragged* is a different sentence and keeps saying
        // it: `other_drag_in_flight` is about every gesture but this one.
        assert_eq!(
            line_of(ChromePointer {
                dragging: Some(slot.id),
                ..hovering
            }),
            palette.divider_active,
        );
    }

    /// F63/E57: grabbing a divider pulls the two panes it resizes into slightly
    /// smaller rounded cards, and the `--panel` floor shows through the gap.
    ///
    /// The assertions are the mock-up's own reading of the shape: the gap is on
    /// **all four sides** of each pane, because it is the consequence of the
    /// panes getting smaller rather than of the seam widening. A gap only along
    /// the boundary would read as "this seam moved", which is precisely what the
    /// treatment exists in order not to say.
    #[test]
    fn grabbing_a_divider_pulls_its_two_panes_into_cards_and_shows_the_floor() {
        let seats = term_beside_files();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1200, 800, 1_000);
        let layout = solved(&seats, viewport, &metrics);
        let split = seats.split_slots(&layout)[0].id;
        let palette = chrome_palette();
        let dragging = ChromePointer {
            dragging: Some(split),
            ..ChromePointer::default()
        };

        let floor_of = |pointer: ChromePointer| {
            head_chrome(&seats, &layout, 1.0, pointer)
                .0
                .into_iter()
                .filter(|quad| quad.color == palette.termhost && in_the_pane_layer(quad.rect, 1.0))
                .map(|quad| quad.rect)
                .collect::<Vec<_>>()
        };
        assert!(
            floor_of(ChromePointer::default()).is_empty(),
            "panes sit flush until you grab something"
        );

        let bands = floor_of(dragging);
        assert_eq!(bands.len(), 8, "four sides each, for both panes");
        let margin = SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX;
        for placement in &layout.rects {
            let rect = device_rect_of(&layout, placement.id);
            let mine: Vec<_> = bands
                .iter()
                .filter(|band| band[0] >= rect[0] && band[2] <= rect[2])
                .collect();
            assert_eq!(mine.len(), 4, "this pane gives on every side");
            assert!(
                mine.iter()
                    .any(|b| b[1] == rect[1] && b[3] == rect[1] + margin),
                "and the top is one of them"
            );
            assert!(
                mine.iter()
                    .any(|b| b[3] == rect[3] && b[1] == rect[3] - margin),
                "and so is the bottom, which no seam ever moved"
            );
        }

        // The four rounds, one mark each and all at the card's 8px.
        let corners: Vec<_> = head_chrome(&seats, &layout, 1.0, dragging)
            .2
            .into_iter()
            .filter(|sprite| matches!(sprite.mark, ChromeMark::CardCorner { .. }))
            .collect();
        assert_eq!(corners.len(), 8, "four corners on each of the two cards");
        for corner in &corners {
            assert_eq!(corner.color, palette.termhost);
            assert_eq!(
                corner.rect[2] - corner.rect[0],
                SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX,
            );
        }
    }

    /// F63, the other half: only the slot being resized is carded.
    ///
    /// Three panes, the inner split dragged — the pane outside that split is not
    /// being resized and must stay flush. A card on it would say "this one too",
    /// which is the reverse of the fact.
    #[test]
    fn a_pane_outside_the_slot_being_resized_stays_flush() {
        let seats = three_in_a_row();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1600, 800, 1_000);
        let layout = solved(&seats, viewport, &metrics);
        let palette = chrome_palette();

        let slots = seats.split_slots(&layout);
        let inner = slots
            .iter()
            .find(|slot| slot.slot.left > LogicalPx::ZERO)
            .expect("the inner split does not start at the viewport's edge");
        let (quads, _, _) = head_chrome(
            &seats,
            &layout,
            1.0,
            ChromePointer {
                dragging: Some(inner.id),
                ..ChromePointer::default()
            },
        );
        let carded: Vec<_> = quads
            .iter()
            .filter(|quad| quad.color == palette.termhost && in_the_pane_layer(quad.rect, 1.0))
            .collect();
        assert_eq!(carded.len(), 8, "two panes carded, not three");
        let outsider = device_rect_of(&layout, layout.rects[0].id);
        assert!(
            !carded
                .iter()
                .any(|quad| quad.rect[0] >= outsider[0] && quad.rect[2] <= outsider[2]),
            "the pane nobody is resizing keeps its edges"
        );
    }

    /// F61/F71/T225: cancelling a divider drag restores that one ratio, and
    /// **only** that one.
    ///
    /// This is the assertion the spec asks for in so many words — "零副作用回滚,
    /// 而且只回滚那一个值 —— 不是整棵树快照回滚,后者会把并发的无关编辑一起撤
    /// 掉". So the test does the thing a snapshot would get wrong: while the
    /// gesture is notionally in flight, a *different* split is edited, and the
    /// cancel has to leave that edit standing.
    ///
    /// The restore runs through `Edit::DragDivider`, whose focus set is the one
    /// split, so §3.3's necessity theorem is what makes "only that one" true
    /// rather than merely observed — and it is asserted directly.
    #[test]
    fn cancelling_a_divider_drag_puts_back_one_ratio_and_no_others() {
        let mut seats = three_in_a_row();
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1600, 800, 1_000);
        let usable = |slot: SplitSlot| slot.slot.extent(slot.dir) - DIVIDER;
        let ratio_of = |seats: &Seats, split: SplitId| {
            seats
                .tree()
                .ratios()
                .into_iter()
                .find_map(|(id, ratio)| (id == split).then_some(ratio))
                .expect("the split has a ratio")
        };
        let slot_of = |seats: &Seats, split: SplitId| {
            let layout = solved(seats, viewport, &metrics);
            *seats
                .split_slots(&layout)
                .iter()
                .find(|slot| slot.id == split)
                .expect("the split is in the solve")
        };

        let slots = seats.split_slots(&solved(&seats, viewport, &metrics));
        let outer = slots[0].id;
        let inner = slots
            .iter()
            .find(|slot| slot.id != outer)
            .expect("two splits")
            .id;

        // The value the gesture will have to put back.
        let origin = ratio_of(&seats, outer);

        // Drag it somewhere else …
        let slot = slot_of(&seats, outer);
        assert_eq!(
            seats.drag_divider(
                &metrics,
                outer,
                Ratio::clamped_from_ppm(350_000),
                usable(slot)
            ),
            Ok(true),
        );
        // … and, mid-gesture, let an unrelated edit land on the other split. A
        // window resize, a command finishing, a second pointer: the tree is not
        // frozen while a button is down.
        let slot = slot_of(&seats, inner);
        assert_eq!(
            seats.drag_divider(
                &metrics,
                inner,
                Ratio::clamped_from_ppm(700_000),
                usable(slot)
            ),
            Ok(true),
        );
        let concurrent = ratio_of(&seats, inner);
        assert_ne!(
            concurrent, origin,
            "the two splits are telling values apart"
        );

        // Esc.
        let before = seats.tree().clone();
        let slot = slot_of(&seats, outer);
        assert_eq!(
            seats.drag_divider(&metrics, outer, origin, usable(slot)),
            Ok(true),
        );

        assert_eq!(
            ratio_of(&seats, outer),
            origin,
            "the dragged split comes back byte for byte"
        );
        assert_eq!(
            ratio_of(&seats, inner),
            concurrent,
            "and the edit that happened alongside it is not undone with it"
        );
        assert!(
            bt_layout::necessity_holds(
                &before,
                seats.tree(),
                &bt_layout::FocusSet::of(vec![outer]),
            ),
            "§3.3: the restore writes inside its focus set and nowhere else"
        );
    }

    /// C28: a terminal pane head names the place it is in, at **full length**.
    ///
    /// The mock-up writes `${s.cwd}` into `.ptitle` (4559) and `cwdLeaf(s)` —
    /// the last segment alone — into the drag ghost (3304). Two lengths, and
    /// deliberately: a caption has a head to fill and answers "where is this",
    /// a label riding the pointer has one line and answers "which one". This
    /// pins the caption's half; the ghost's arrives with the ghost.
    ///
    /// Red gate: hand the head `cwd_leaf`'s answer and the first assertion
    /// fails on the very case the distinction exists for — two panes rooted in
    /// two different `src` directories.
    #[test]
    fn a_terminal_pane_head_names_the_whole_path_and_falls_back_honestly() {
        let cwd = r"D:\Developer\BetterTerminal\crates\bt-app";
        assert_eq!(seat_caption(SeatKind::Terminal, None, Some(cwd)), cwd);
        // A shell that never reported an OSC 7 has not said where it is, and
        // the honest answer to "where is this" is then the kind's own name —
        // never an empty caption, and never a guess at the filesystem.
        assert_eq!(
            seat_caption(SeatKind::Terminal, None, None),
            "Terminal",
            "no report, no place"
        );
        assert_eq!(
            seat_caption(SeatKind::Terminal, None, Some("")),
            "Terminal",
            "an empty report is not a path to the root"
        );
        // The other kinds are untouched: a preview names its file, and the two
        // that have no session of their own answer by kind.
        assert_eq!(
            seat_caption(SeatKind::Preview, Some("notes.md"), Some(cwd)),
            "notes.md"
        );
        assert_eq!(seat_caption(SeatKind::Files, None, Some(cwd)), "Files");
        assert_eq!(
            seat_caption(SeatKind::Placeholder, None, Some(cwd)),
            "Unavailable",
            "T227: a leaf this build cannot name says so rather than borrowing one"
        );
    }

    // ---------------------------------------------------------------------
    // U4: the drag ghost (J114, J115, J116)
    // ---------------------------------------------------------------------

    /// `.drag-ghost` is a shrink-wrapped flex row, so every number in its box is
    /// the sum of a declaration and its contents — and the contents are a 15px
    /// mark and one line of 12.5px text.
    ///
    /// Red gate: the three easy ways to get a shrink-wrapped box wrong are to
    /// count the border once instead of on both sides, to forget the gap, and to
    /// take the row's height off the mark instead of off the taller of the two.
    /// Each of the four assertions below fails on exactly one of them.
    #[test]
    fn the_ghost_shrink_wraps_its_mark_and_its_name_inside_the_mockups_padding() {
        let ghost = drag_ghost_layout([100.0, 200.0], 15.0, 60.0, 1.0);
        // 12 + 1 either side, 15 of mark, 7 of gap, 60 of text.
        assert_eq!(
            ghost.frame[2] - ghost.frame[0],
            2.0 * (12.0 + 1.0) + 15.0 + 7.0 + 60.0,
            "border + padding on both sides, then mark, gap and name"
        );
        // The row is `max(15, round(12.5 × 1.4)) = 18`, not the mark's 15.
        assert_eq!(
            ghost.frame[3] - ghost.frame[1],
            2.0 * (5.0 + 1.0) + 18.0,
            "align-items:center makes the row as tall as its tallest item, and \
             here that is the line box"
        );
        assert_eq!(
            ghost.mark[0],
            ghost.frame[0] + 1.0 + 12.0,
            "the mark stands at the leading padding, inside the border"
        );
        assert_eq!(
            ghost.label[0],
            ghost.mark[2] + 7.0,
            "and the name one gap after it"
        );
        // Both are centred on the row rather than on boxes of their own — to
        // within the half pixel an odd item in an even row cannot avoid. The
        // rounding is not a tolerance the assertion is granting: it is the rule
        // every other glyph in this chrome is placed by, because a mark whose
        // top lands on a half pixel is a mark rasterised across two rows.
        let centre = (ghost.frame[1] + ghost.frame[3]) / 2.0;
        for (box_, name) in [(ghost.mark, "mark"), (ghost.label, "label")] {
            assert!(
                ((box_[1] + box_[3]) / 2.0 - centre).abs() <= 0.5,
                "the {name} is centred on the row"
            );
            assert_eq!(box_[1].fract(), 0.0, "the {name} starts on a whole pixel");
        }
    }

    /// J115: `left = clientX + 10`, `top = clientY + 8` — in *logical* pixels, so
    /// the label sits the same distance from the hand on every display.
    ///
    /// Red gate: treat the offset as physical and the ghost drifts onto the
    /// pointer at 200%, which is the one place it must never be — it would cover
    /// the thing being aimed at.
    #[test]
    fn the_ghost_hangs_below_and_right_of_the_hand_by_the_same_logical_amount() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let pointer = [400.0_f32, 300.0];
            let ghost = drag_ghost_layout(pointer, 15.0, 60.0, scale);
            assert_eq!(
                ghost.frame[0] - pointer[0],
                (10.0 * scale).round(),
                "ten logical pixels right of the hand at {scale}×"
            );
            assert_eq!(
                ghost.frame[1] - pointer[1],
                (8.0 * scale).round(),
                "eight logical pixels below it at {scale}×"
            );
        }
    }

    /// The ghost is not clamped to the window, and that is the mock-up's own
    /// `position: fixed` with no bound (1717).
    ///
    /// Red gate: add a clamp and the ghost stops reporting where the pointer is
    /// the moment the pointer nears an edge — which is exactly when a drag is
    /// most likely to be aiming at something.
    #[test]
    fn the_ghost_follows_the_hand_past_the_edge_rather_than_stopping_at_it() {
        let ghost = drag_ghost_layout([1919.0, 1079.0], 15.0, 60.0, 1.0);
        assert_eq!(ghost.frame[0], 1929.0);
        assert_eq!(ghost.frame[1], 1087.0);
    }

    /// One layer, and everything the mock-up puts in it: a floating box, the
    /// source's own mark, and the short name beside it.
    ///
    /// Red gate: drop the `push_float_window` call and the layer still carries
    /// its mark and its label — the first assertion is what notices that the box
    /// they stand in has gone.
    #[test]
    fn the_ghost_paints_one_floating_box_carrying_a_mark_and_one_name() {
        let palette = bt_render::chrome_palette();
        let layout = drag_ghost_layout([100.0, 100.0], 15.0, 60.0, 1.0);
        let layer = build_drag_ghost(
            &layout,
            ChromeMark::ProfilePowerShell,
            palette.accent,
            "bt-app",
            1.0,
            palette,
        );
        assert!(
            !layer.quads.is_empty(),
            "the ghost stands in a box, not on the page"
        );
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.menu_surface),
            "`background: var(--menu)` — a floating plane, not the chrome's"
        );
        // `box-shadow: 0 8px 24px rgba(0,0,0,.25)` — the ghost's own single
        // declaration, and *not* the tip's, which is `.1` on light and `.45` on
        // dark. Borrowing either would make the label's lift a function of the
        // theme, which the mock-up never asked for.
        //
        // Red gate: hand `build_drag_ghost` `tip_shadow_*` and every other
        // assertion in this test still passes.
        let strongest = layer
            .quads
            .iter()
            .filter(|quad| quad.color == palette.menu_shadow)
            .map(|quad| quad.alpha)
            .fold(0.0_f32, f32::max);
        assert!(
            (strongest - f32::from(palette.drag_ghost_shadow_inner_alpha) / 255.0).abs() < 0.002,
            "the ghost casts its own lift, got {strongest}"
        );
        // And the pair itself is `.25` on both themes — the assertion above pins
        // only that `build_drag_ghost` reaches for the right field, which stays
        // true however wrong the field is.
        for (theme, chrome) in [
            ("dark", bt_render::DARK_CHROME),
            ("light", bt_render::LIGHT_CHROME),
        ] {
            assert_eq!(
                chrome.drag_ghost_shadow_inner_alpha, 64,
                "`rgba(0,0,0,.25)` is written once and never overridden, so {theme} \
                 gets the same 255 × .25 the other does"
            );
            assert_eq!(
                chrome.drag_ghost_shadow_outer_alpha,
                chrome.drag_ghost_shadow_inner_alpha / 2,
                "the outer ring is half the inner one, as on every floating surface"
            );
        }
        let [label]: [ChromeLabel; 1] = layer
            .labels
            .try_into()
            .expect("a ghost says one name on one line");
        assert_eq!(label.text, "bt-app");
        assert_eq!(label.font_size_px, 12.5);
        assert_eq!(
            label.color, palette.menu_item_text_selected,
            "`color: var(--ink)` over `--menu` — the full ink, not a menu row's"
        );
        assert!(
            !label.align_center && !label.align_right,
            "a shrink-wrapped row has nothing to align against"
        );
        // `border-radius: 7px`, read off the shape rather than off the constant:
        // a rounded rectangle's straight sides begin one radius below its top
        // edge, so the full-width part of the face is inset by exactly that.
        //
        // Red gate: hand the recipe `FLOAT_WINDOW_RADIUS_LOGICAL_PX` — the 10 every
        // *other* floating surface in this window wears — and the band moves to
        // 10. The ghost is rounded less on purpose: it is the smallest floating
        // thing here, and 10 on a 30px box is most of its height.
        let inner_width = (layout.frame[2] - layout.frame[0]) - 2.0;
        let band_top = layer
            .quads
            .iter()
            .filter(|quad| {
                quad.color == palette.menu_surface
                    && (quad.rect[2] - quad.rect[0] - inner_width).abs() < 0.51
            })
            .map(|quad| quad.rect[1])
            .fold(f32::MAX, f32::min);
        assert_eq!(
            band_top - layout.frame[1],
            bt_render::DRAG_GHOST_RADIUS_LOGICAL_PX,
            "the face goes full width one radius down from the corner"
        );
        assert_eq!(
            bt_render::DRAG_GHOST_RADIUS_LOGICAL_PX,
            7.0,
            "and that radius is the ghost's own, not the float window's 10"
        );
        let [mark] = layer.sprites.as_slice() else {
            panic!("a ghost wears exactly one mark");
        };
        assert_eq!(mark.mark, ChromeMark::ProfilePowerShell);
        assert_eq!(mark.rect, layout.mark);
        assert_eq!(layer.opacity, 1.0);
    }

    /// J116/C28: the ghost answers "which one is this", so it takes the short
    /// name — while the head beside it is still answering "where is this".
    ///
    /// Red gate: hand the ghost `seat_caption` and this passes for `Files` and
    /// `Preview` and fails only for a terminal, which is the one kind whose two
    /// answers actually differ.
    #[test]
    fn the_ghost_names_the_last_segment_where_the_head_names_the_path() {
        let cwd = r"D:\Developer\BetterTerminal\crates\bt-app";
        assert_eq!(
            seat_short_caption(SeatKind::Terminal, None, Some(cwd)),
            "bt-app"
        );
        assert_eq!(seat_caption(SeatKind::Terminal, None, Some(cwd)), cwd);
    }

    // ---------------------------------------------------------------------
    // U9: the collapsed presentation (H101, T209, T210, T211, T227)
    // ---------------------------------------------------------------------

    /// A tree of `count` terminals in one row, focus on the first.
    ///
    /// Built by hand because `bt-app` still has no split verb — `Edit::SplitSeat`
    /// exists and is unconstructed — and the concession ladder's interesting
    /// shapes start at three seats.
    fn row_of_terminals(count: u64) -> Seats {
        let mut tree = LayoutNode::seat(Seat::new(SeatId(1), SeatKind::Terminal));
        for index in 1..count {
            tree = LayoutNode::split(
                SplitId(index),
                Axis::Row,
                tree,
                LayoutNode::seat(Seat::new(SeatId(index + 1), SeatKind::Terminal)),
            );
        }
        Seats {
            tree,
            terminal: SeatId(1),
            focus: SeatId(1),
            next_seat: count + 1,
            next_split: count,
        }
    }

    /// A layout of one hand-placed seat, for the two collapse shapes the app's
    /// own verbs cannot yet reach.
    ///
    /// `Collapsed(Col)` and `Collapsed(Row, Col)` are both legal solver output —
    /// `bt-layout`'s own pins produce them — but reaching them from here needs a
    /// column split, and nothing in `bt-app` constructs one yet. The chrome has
    /// to draw what the solver can say, not only what today's verbs can ask for.
    fn one_collapsed_seat(
        kind: SeatKind,
        presentation: Presentation,
        rect: LogicalRect,
    ) -> SeatLayout {
        SeatLayout {
            rects: vec![bt_layout::SeatPlacement {
                id: SeatId(1),
                kind,
                rect: Some(rect),
                device_rect: Some(bt_layout::DeviceRect {
                    left: rect.left.floor_px(),
                    top: rect.top.floor_px(),
                    right: rect.right.floor_px(),
                    bottom: rect.bottom.floor_px(),
                }),
                presentation,
            }],
        }
    }

    struct ChromeParts {
        quads: Vec<ChromeQuad>,
        labels: Vec<ChromeLabel>,
        sprites: Vec<ChromeSprite>,
    }

    fn chrome_of(seats: &Seats, layout: &SeatLayout, cwd: Option<&str>) -> ChromeParts {
        chrome_with_overflow(seats, layout, cwd, None)
    }

    fn chrome_with_overflow(
        seats: &Seats,
        layout: &SeatLayout,
        cwd: Option<&str>,
        fit_overflow: Option<FitOverflow>,
    ) -> ChromeParts {
        let tabs = [TabContent {
            title: "PowerShell".to_owned(),
            pane_count: seats.pane_count(),
            ..TabContent::default()
        }];
        let (quads, labels, sprites) = build_chrome_for_tabs(
            seats,
            layout,
            1.0,
            ChromePointer::default(),
            ChromeContent {
                tabs: &tabs,
                active_tab: 0,
                grabbed: None,
                tab_scroll: 0.0,
                preview_title: None,
                terminal_cwd: cwd,
                preview_message: None,
                fit_overflow,
                profile_menu_open: false,
                chevron_turn: 0.0,
            },
        );
        ChromeParts {
            quads,
            labels,
            sprites,
        }
    }

    fn inside(outer: [f32; 4], inner: [f32; 4]) -> bool {
        inner[0] >= outer[0] && inner[1] >= outer[1] && inner[2] <= outer[2] && inner[3] <= outer[3]
    }

    /// T210: a collapsed bar wears the *same* mark component a pane head wears.
    ///
    /// §2.6.3 does not ask for "an icon", it asks for the `stateIcon` a tab and a
    /// card carry — the one that breathes while a command runs and lights a dot
    /// when something wants you. A bar can only make that promise by being drawn
    /// from the same component.
    ///
    /// Red gate: the drawing this replaced pushed a plain 6x6 `title_text` quad,
    /// which is a rectangle of ink no state can ever reach. Both halves below
    /// fail against it — there is no sprite in the bar, and there is a quad in
    /// the bar that is neither of the bar's two ground colours.
    #[test]
    fn a_collapsed_bar_wears_the_pane_heads_own_mark() {
        let metrics = seat_metrics(1_000);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        let viewport = viewport_of(500, 600, 1_000);
        let layout = seats.solve(viewport, &metrics).expect("L3 satisfies 500px");
        let preview = seats.preview().unwrap();
        let placement = layout.get(preview).unwrap();
        assert!(
            matches!(placement.presentation, Presentation::Collapsed(_)),
            "the non-focus seat is the one that gives way"
        );
        let device = placement.device_rect.unwrap();
        let bar_rect = [
            device.left as f32,
            device.top as f32,
            device.right as f32,
            device.bottom as f32,
        ];
        let parts = chrome_of(&seats, &layout, None);
        let palette = chrome_palette();
        let (expected_mark, _, _) = pane_mark(SeatKind::Preview, palette);
        let marks: Vec<_> = parts
            .sprites
            .iter()
            .filter(|sprite| inside(bar_rect, sprite.rect))
            .collect();
        assert_eq!(
            marks.len(),
            1,
            "one mark, and nothing else, stands in the bar"
        );
        assert_eq!(
            marks[0].mark, expected_mark,
            "the bar borrows `pane_mark`, so it cannot name the seat differently \
             than the seat's own head would"
        );
        for quad in &parts.quads {
            if inside(bar_rect, quad.rect) {
                assert!(
                    quad.color == palette.collapse_bar || quad.color == palette.collapse_bar_hover,
                    "a hand-drawn block standing in for an icon is what T209 forbids"
                );
            }
        }
    }

    /// T209: squeezed on both axes, the seat is a colour block carrying only its
    /// state icon — no border and, above all, no name (tiny-window §1.3, ruling
    /// 2). Twenty-four square cannot hold a word, and forcing one in is a mosaic
    /// pretending to be text.
    ///
    /// The verdict is read from `Presentation`, not guessed from the rectangle:
    /// this square is 24 by 24, so an implementation that decided by
    /// `width > height` would call it a bar and try to write in it.
    ///
    /// Red gate: pass `AxisSet::COL` instead and the label assertion fails,
    /// because that shape is allowed a name.
    #[test]
    fn a_double_collapsed_seat_draws_a_colour_block_with_only_its_mark() {
        let square = LogicalRect::new(
            LogicalPx::ZERO,
            LogicalPx::ZERO,
            bt_layout::COLLAPSED_EXTENT,
            bt_layout::COLLAPSED_EXTENT,
        );
        let seats = Seats::lone_terminal();
        let layout = one_collapsed_seat(
            SeatKind::Terminal,
            Presentation::Collapsed(bt_layout::AxisSet::BOTH),
            square,
        );
        let parts = chrome_of(&seats, &layout, Some("C:\\Users\\Weiyi\\bt"));
        let block = [0.0, 0.0, 24.0, 24.0];
        assert!(
            parts
                .quads
                .iter()
                .any(|quad| quad.rect == block && quad.color == chrome_palette().collapse_bar),
            "the square is a plain colour block"
        );
        let marks: Vec<_> = parts
            .sprites
            .iter()
            .filter(|sprite| inside(block, sprite.rect))
            .collect();
        assert_eq!(marks.len(), 1, "the state icon, alone");
        assert_eq!(
            marks[0].rect,
            [5.0, 5.0, 20.0, 20.0],
            "centred on both axes: the degenerate square is its own style, not a \
             bar with the writing cut off, and a bar's mark stands at the head"
        );
        assert!(
            !parts.labels.iter().any(|label| inside(block, label.rect)),
            "24 square holds no name (tiny-window 1.3)"
        );
    }

    /// The third shape, and the one with nowhere to put a word: squeezed along
    /// Row, a seat is 24 wide and as tall as its slot.
    ///
    /// **Ruling.** No name. The chrome label pipeline draws horizontally and 24
    /// logical pixels do not hold a horizontal word; rotating one is not a thing
    /// this renderer can do, and cutting it to an initial would be the mosaic
    /// tiny-window §1.3 rejects for the square. The mark instead stands at the
    /// *head* of the strip, where a title would have begun — which is also what
    /// keeps this shape distinguishable from the double-collapsed square, whose
    /// mark is centred. Recorded rather than approximated: a vertical bar cannot
    /// say its name until something can set type on its side.
    ///
    /// Red gate: place it by the square's rule and the mark lands at 5 rather
    /// than at the head's own padding.
    #[test]
    fn a_bar_squeezed_along_row_carries_its_mark_at_the_head_and_no_name() {
        let seats = Seats::lone_terminal();
        // Below the title bar, so the window chrome's own marks cannot wander
        // into the rectangle this pin measures.
        let bar = LogicalRect::new(
            LogicalPx::ZERO,
            LogicalPx::px(100),
            bt_layout::COLLAPSED_EXTENT,
            LogicalPx::px(300),
        );
        let layout = one_collapsed_seat(
            SeatKind::Terminal,
            Presentation::Collapsed(bt_layout::AxisSet::ROW),
            bar,
        );
        let parts = chrome_of(&seats, &layout, Some("C:\\Users\\Weiyi\\bt"));
        let strip = [0.0, 100.0, 24.0, 300.0];
        let marks: Vec<_> = parts
            .sprites
            .iter()
            .filter(|sprite| inside(strip, sprite.rect))
            .collect();
        assert_eq!(marks.len(), 1);
        assert_eq!(
            marks[0].rect,
            [5.0, 112.0, 20.0, 127.0],
            "centred across the 24, and one head's padding down from the top"
        );
        assert!(
            !parts.labels.iter().any(|label| inside(strip, label.rect)),
            "24 logical pixels do not hold a horizontal word"
        );
    }

    /// T210 and C28 together: a bar squeezed along Col has a whole line to give,
    /// so it names its seat — by the *short* name, the one a label riding the
    /// pointer uses, while the pane head of the very same seat spells the path
    /// out in full.
    ///
    /// Red gate: point the bar at `seat_caption` instead of `seat_short_caption`
    /// and the first assertion reads the whole path.
    #[test]
    fn a_collapsed_bar_names_its_seat_short_where_the_head_names_it_long() {
        let cwd = "C:\\Users\\Weiyi\\Developer\\BetterTerminal";
        let seats = Seats::lone_terminal();
        let bar = LogicalRect::new(
            LogicalPx::ZERO,
            LogicalPx::ZERO,
            LogicalPx::px(400),
            bt_layout::COLLAPSED_EXTENT,
        );
        let layout = one_collapsed_seat(
            SeatKind::Terminal,
            Presentation::Collapsed(bt_layout::AxisSet::COL),
            bar,
        );
        let parts = chrome_of(&seats, &layout, Some(cwd));
        let strip = [0.0, 0.0, 400.0, 24.0];
        let named: Vec<_> = parts
            .labels
            .iter()
            .filter(|label| inside(strip, label.rect))
            .collect();
        assert_eq!(named.len(), 1, "one name on the bar");
        assert_eq!(
            named[0].text, "BetterTerminal",
            "the last segment, not the path"
        );
        let mark = parts
            .sprites
            .iter()
            .find(|sprite| inside(strip, sprite.rect))
            .expect("the bar carries its mark");
        assert_eq!(
            mark.rect,
            [12.0, 5.0, 27.0, 20.0],
            "the pane head's own row: the mark at the leading padding, centred \
             on the bar's middle — not the square's two-axis centring"
        );
        assert!(
            named[0].rect[0] >= mark.rect[2],
            "the name follows the mark, as it does in a pane head"
        );

        // The same seat, drawn as a head, answers the other question at the
        // other length (C28).
        let metrics = seat_metrics(1_000);
        let mut with_head = Seats::lone_terminal();
        with_head.toggle_preview(&metrics);
        let full = solved(&with_head, viewport_of(1600, 900, 1_000), &metrics);
        let head = chrome_of(&with_head, &full, Some(cwd));
        assert!(
            head.labels.iter().any(|label| label.text == cwd),
            "a pane head has a bar to fill and answers `where is this` in full"
        );
    }

    /// T227: a leaf this build cannot name says so where it stands.
    ///
    /// The tree survives an unknown kind (§2.1) — but a placeholder that is only
    /// a blank rectangle is indistinguishable from the silent destruction that
    /// rule exists to forbid, and the difference matters most to the user who has
    /// just downgraded and is looking for a pane that is not there.
    ///
    /// Red gate: the notice is the *body*'s, not the head's, so a build that
    /// merely titles the pane "Unavailable" fails the centred-body assertion —
    /// and a lone placeholder, which would wear no head under the sibling rule,
    /// would then say nothing at all.
    #[test]
    fn a_placeholder_leaf_says_why_it_cannot_be_shown() {
        let node = LayoutNodeV1::Split(SplitNodeV1 {
            dir: SplitDirV1::Row,
            ratio: 600_000,
            children: [
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                    profile_id: "pwsh.exe".to_owned(),
                    cwd: String::new(),
                    manual_name: None,
                }))),
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Unknown)),
            ],
        });
        let seats = Seats::from_persisted(&node).unwrap();
        let metrics = seat_metrics(1_000);
        let layout = solved(&seats, viewport_of(1600, 900, 1_000), &metrics);
        let parts = chrome_of(&seats, &layout, None);
        let notice = parts
            .labels
            .iter()
            .find(|label| label.text == PLACEHOLDER_SEAT_NOTICE)
            .expect("T227: the degradation has to be visible");
        assert!(
            notice.align_center,
            "a state notice sits centred in the body, like an empty preview's"
        );
        assert_eq!(
            notice.color,
            chrome_palette().body_hint_text,
            "a note about this build, not an alarm"
        );

        // And it says it even with no sibling to earn it a head.
        let lone = Seats {
            tree: LayoutNode::seat(Seat::new(SeatId(1), SeatKind::Placeholder)),
            terminal: SeatId(1),
            focus: SeatId(1),
            next_seat: 2,
            next_split: 1,
        };
        let lone_layout = solved(&lone, viewport_of(1600, 900, 1_000), &metrics);
        let lone_parts = chrome_of(&lone, &lone_layout, None);
        assert!(
            lone_parts
                .labels
                .iter()
                .any(|label| label.text == PLACEHOLDER_SEAT_NOTICE),
            "the pane that cannot say what it is, is the one that has to say it"
        );
    }

    /// H101: L4 keeps the focus seat a pane and gives every other seat a row of
    /// the strip along the foot (DESIGN §7.1.1 + tiny-window §4.3).
    ///
    /// Red gate: the previous implementation gave the focus seat the whole
    /// viewport and left every other seat unpresented, so both the stage's bottom
    /// edge and every bar rectangle below fail against it.
    #[test]
    fn fit_what_fits_gives_the_stage_the_viewport_less_the_bars_it_shows() {
        let metrics = seat_metrics(1_000);
        let seats = row_of_terminals(4);
        // 4 x 260 plus 3 dividers needs 1043; folded to the floor it still needs
        // 260 + 3 x 24 + 3 = 335. 300 has neither, so the ladder runs out.
        let viewport = viewport_of(300, 240, 1_000);
        assert!(
            seats.solve(viewport, &metrics).is_err(),
            "the scenario has to actually be L4"
        );
        let (layout, overflow) = fit_what_fits(&seats, viewport, &metrics);
        assert_eq!(overflow, None, "200px of height holds every bar");

        let unit = bt_layout::COLLAPSED_EXTENT;
        let strip_top = viewport.bottom - LogicalPx::from_subpixels(unit.subpixels() * 3);
        assert_eq!(
            layout.get(SeatId(1)).unwrap().rect,
            Some(LogicalRect::new(
                viewport.left,
                viewport.top,
                viewport.right,
                strip_top
            )),
            "the focus seat keeps a pane, less the strip"
        );
        for (index, id) in [SeatId(2), SeatId(3), SeatId(4)].into_iter().enumerate() {
            let placement = layout.get(id).unwrap();
            let top = strip_top + LogicalPx::from_subpixels(unit.subpixels() * index as i64);
            assert_eq!(
                placement.rect,
                Some(LogicalRect::new(
                    viewport.left,
                    top,
                    viewport.right,
                    top + unit
                )),
                "bars stack in tree order, each one row tall"
            );
            assert_eq!(
                placement.presentation,
                Presentation::Collapsed(bt_layout::AxisSet::COL),
                "squeezed along Col: the shape that can still carry a name"
            );
        }
        // Red line L6 reaches this allocator too: the stage's foot *is* the first
        // bar's head, so no seam and no overlap can open between them.
        assert_eq!(
            layout.get(SeatId(1)).unwrap().device_rect.unwrap().bottom,
            layout.get(SeatId(2)).unwrap().device_rect.unwrap().top
        );
    }

    /// H101, the other half: when the foot cannot hold every bar it says how many
    /// it dropped, and drops the ones furthest from the focus — `collapse_order`,
    /// the ladder's own order, read one step further.
    ///
    /// Red gate: drop from the tail of tree order instead and seat 2 loses its
    /// row while seat 4 keeps one, which is the assertion below reversed.
    #[test]
    fn fit_what_fits_says_how_many_seats_had_no_row() {
        let metrics = seat_metrics(1_000);
        let seats = row_of_terminals(4);
        // 72 logical pixels of viewport: three rows, one of which the stage
        // keeps, leaving two — one bar and the sentence.
        let viewport = viewport_of(300, 112, 1_000);
        assert_eq!(viewport.extent(Axis::Col), LogicalPx::px(72));
        let (layout, overflow) = fit_what_fits(&seats, viewport, &metrics);
        let overflow = overflow.expect("three seats do not fit in one row");
        assert_eq!(overflow.hidden, 2, "one bar shown, two seats spoken for");
        assert_eq!(
            overflow.row.top, 88,
            "the sentence is the strip's last line"
        );
        assert_eq!(overflow.row.bottom, 112);
        assert!(
            layout.get(SeatId(2)).unwrap().rect.is_some(),
            "the seat nearest the focus keeps its row"
        );
        for id in [SeatId(3), SeatId(4)] {
            assert_eq!(
                layout.get(id).unwrap().rect,
                None,
                "a seat with no row has no rectangle — never a zero-area one (L4)"
            );
        }
        assert_eq!(overflow_notice(2), "2 more do not fit");
        assert_eq!(overflow_notice(1), "1 more does not fit");

        let told = chrome_with_overflow(&seats, &layout, None, Some(overflow));
        assert!(
            told.labels
                .iter()
                .any(|label| label.text == "2 more do not fit"),
            "the tail line is drawn when it is handed over"
        );
        let untold = chrome_of(&seats, &layout, None);
        assert!(
            !untold
                .labels
                .iter()
                .any(|label| label.text == "2 more do not fit"),
            "and only then — an ordinary solve never carries one"
        );
    }

    /// The identity the previous implementation was built to protect, kept: a
    /// lone terminal dragged below its own minimum is one seat filling the
    /// viewport, exactly as it was before seats existed. There is no other seat,
    /// so there is no strip to take a row from it.
    #[test]
    fn fit_what_fits_is_still_the_whole_viewport_for_a_lone_leaf() {
        let metrics = seat_metrics(1_000);
        let seats = Seats::lone_terminal();
        for (width, height) in [(100u32, 100u32), (1, 1), (259, 400)] {
            let viewport = viewport_of(width, height, 1_000);
            let (layout, overflow) = fit_what_fits(&seats, viewport, &metrics);
            assert_eq!(layout.get(seats.terminal()).unwrap().rect, Some(viewport));
            assert_eq!(overflow, None);
        }
    }

    /// T211 reaching L4: the bars down there are the same `Collapsed` placements
    /// L3 makes, so the hit test already answers for them and the click already
    /// means "expand" — which is what makes L4 an escapable state without the
    /// action button §4.3 forbids.
    ///
    /// Red gate: give the strip its own quads instead of real placements and
    /// `hit_chrome` finds nothing to press.
    #[test]
    fn a_bar_in_the_fit_what_fits_strip_is_still_a_hit_target() {
        let metrics = seat_metrics(1_000);
        let mut seats = row_of_terminals(4);
        let viewport = viewport_of(300, 240, 1_000);
        let (layout, _) = fit_what_fits(&seats, viewport, &metrics);
        let bar = layout.get(SeatId(3)).unwrap().device_rect.unwrap();
        assert_eq!(
            hit_chrome(
                &seats,
                &layout,
                1.0,
                f64::from((bar.left + bar.right) as i32) / 2.0,
                f64::from((bar.top + bar.bottom) as i32) / 2.0,
            ),
            Some(ChromeTarget::CollapseBar(SeatId(3))),
        );
        // And pressing it is the whole escape: the seat becomes the focus, so the
        // next solve puts it on the stage.
        assert!(seats.set_focus(SeatId(3)));
        let (after, _) = fit_what_fits(&seats, viewport, &metrics);
        assert_eq!(
            after.get(SeatId(3)).unwrap().presentation,
            Presentation::Full,
            "the bar you pressed is the pane you get"
        );
    }

    // ---------------------------------------------------------------------
    // U10: the layout event broadcast (T230)
    // ---------------------------------------------------------------------

    /// The contract's floor: solving the same tree against the same viewport
    /// twice is not an event. A consumer that dissolved an overlay on every
    /// commit would dissolve it on every frame of a resize that had settled.
    #[test]
    fn an_idempotent_rebuild_publishes_no_layout_events() {
        let metrics = seat_metrics(1_000);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        for (width, height) in [(1600u32, 900u32), (500, 600), (300, 240)] {
            let viewport = viewport_of(width, height, 1_000);
            let first = solved(&seats, viewport, &metrics);
            let second = solved(&seats, viewport, &metrics);
            assert_eq!(
                layout_events(&first, &second),
                vec![],
                "{width}x{height}: a rebuild that landed on the same answer says nothing"
            );
        }
    }

    /// Opening a pane is one arrival and one displacement, and closing it is the
    /// mirror. The seat that made room moved, and it is told so — an overlay
    /// anchored in the terminal is no longer over the rectangle it measured.
    ///
    /// Red gate: report only the new seat and the `Moved` assertion fails; walk
    /// only the new layout and the closed seat's `Vanished` never arrives,
    /// because it is not there to be compared against.
    #[test]
    fn opening_and_closing_a_pane_publishes_the_arrival_and_the_departure() {
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1600, 900, 1_000);
        let mut seats = Seats::lone_terminal();
        let before = solved(&seats, viewport, &metrics);
        assert!(seats.toggle_preview(&metrics));
        let opened = solved(&seats, viewport, &metrics);
        let preview = seats.preview().unwrap();
        assert_eq!(
            layout_events(&before, &opened),
            vec![
                LayoutEvent::Moved(seats.terminal()),
                LayoutEvent::Appeared(preview),
            ],
            "in tree order, and the terminal really did give up width"
        );
        assert!(seats.toggle_preview(&metrics));
        let closed = solved(&seats, viewport, &metrics);
        assert_eq!(
            layout_events(&opened, &closed),
            vec![
                LayoutEvent::Moved(seats.terminal()),
                LayoutEvent::Vanished(preview),
            ],
            "a seat that left the tree is still reported, last"
        );
    }

    /// A divider drag moves exactly the two seats it is between, and says so once
    /// each — §3.5 names it as one of the four causes an anchored overlay has to
    /// hear about.
    ///
    /// Red gate: the second half. Re-solving after the drag writes nothing, so a
    /// diff that reported on being called rather than on having changed fails
    /// there.
    #[test]
    fn a_divider_drag_publishes_one_move_for_each_seat_it_moved() {
        let metrics = seat_metrics(1_000);
        let viewport = viewport_of(1600, 900, 1_000);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        let before = solved(&seats, viewport, &metrics);
        let slot = seats.split_slots(&before)[0];
        let usable = slot.slot.extent(slot.dir) - bt_layout::DIVIDER;
        assert!(
            seats
                .drag_divider(&metrics, slot.id, Ratio::clamped_from_ppm(400_000), usable)
                .unwrap(),
            "a legal drag writes a ratio"
        );
        let after = solved(&seats, viewport, &metrics);
        let events = layout_events(&before, &after);
        assert_eq!(events.len(), 2, "both sides moved, and nothing else did");
        assert!(
            events
                .iter()
                .all(|event| matches!(event, LayoutEvent::Moved(_)))
        );

        let again = solved(&seats, viewport, &metrics);
        assert_eq!(layout_events(&after, &again), vec![]);
    }

    /// Crossing the L3 boundary is a presentation change, not merely a move —
    /// the seat did not just get smaller, it stopped being a pane. §3.5 lists it
    /// first among the four, because it is the one an overlay cannot survive.
    ///
    /// Red gate: compare rectangles only and this reports `Moved`, which tells a
    /// consumer the anchor shifted when what actually happened is that the thing
    /// it was anchored to is no longer there.
    #[test]
    fn crossing_the_collapse_boundary_publishes_a_presentation_change() {
        let metrics = seat_metrics(1_000);
        let mut seats = Seats::lone_terminal();
        seats.toggle_preview(&metrics);
        let preview = seats.preview().unwrap();
        let roomy = solved(&seats, viewport_of(1600, 900, 1_000), &metrics);
        let cramped = solved(&seats, viewport_of(500, 900, 1_000), &metrics);
        assert!(matches!(
            cramped.get(preview).unwrap().presentation,
            Presentation::Collapsed(_)
        ));
        let events = layout_events(&roomy, &cramped);
        assert!(
            events.contains(&LayoutEvent::Presentation(preview)),
            "the seat that collapsed is reported as a presentation change, got {events:?}"
        );
        assert!(
            events.contains(&LayoutEvent::Moved(seats.terminal())),
            "and the seat that took the room is reported as having moved"
        );
        assert_eq!(events.len(), 2, "one event per seat per commit");
        assert!(
            layout_events(&cramped, &roomy).contains(&LayoutEvent::Presentation(preview)),
            "the ladder crosses L3 in both directions"
        );
    }

    /// A window resize is the plainest of the four: every seat whose rectangle
    /// changed says so, and one whose rectangle did not stays quiet.
    #[test]
    fn a_resize_publishes_a_move_for_every_seat_whose_rectangle_changed() {
        let metrics = seat_metrics(1_000);
        let seats = Seats::lone_terminal();
        let wide = solved(&seats, viewport_of(1600, 900, 1_000), &metrics);
        let narrow = solved(&seats, viewport_of(1200, 900, 1_000), &metrics);
        assert_eq!(
            layout_events(&wide, &narrow),
            vec![LayoutEvent::Moved(seats.terminal())]
        );
    }

    /// Falling to L4 takes rectangles away from the seats that lose their row,
    /// and that is a departure like any other — the four facts cover a fifth
    /// cause without anyone having named it.
    #[test]
    fn falling_to_fit_what_fits_publishes_what_left_the_screen() {
        let metrics = seat_metrics(1_000);
        let seats = row_of_terminals(4);
        let roomy = solved(&seats, viewport_of(1600, 900, 1_000), &metrics);
        let (starved, _) = solved_with_overflow(&seats, viewport_of(300, 112, 1_000), &metrics);
        let events = layout_events(&roomy, &starved);
        assert!(events.contains(&LayoutEvent::Vanished(SeatId(3))));
        assert!(events.contains(&LayoutEvent::Vanished(SeatId(4))));
        assert!(events.contains(&LayoutEvent::Presentation(SeatId(2))));
        assert!(events.contains(&LayoutEvent::Moved(SeatId(1))));
    }
}

/// U5 — drop-landing geometry (K127-K134), tested against solved rectangles.
///
/// Every case here builds a real tree, runs the real solver and asks the real
/// question. Nothing hand-places a rectangle, because the ruling these tests
/// exist to protect is that the aim is taken against the layout that *is* rather
/// than against one reconstructed beside it (T228, A12).
#[cfg(test)]
mod drop_geometry_tests {
    use super::*;

    const DPI: u32 = 1_000;
    const W: u32 = 1_600;
    const H: u32 = 900;

    fn term(id: u64) -> LayoutNode {
        LayoutNode::seat(Seat::new(SeatId(id), SeatKind::Terminal))
    }

    /// A solved layout, its host box, its pane count and its scale — the four
    /// things `aim_at_layout` takes, all derived from one tree so that no two of
    /// them can disagree about the window they describe.
    fn stage(tree: LayoutNode, dpi_milli: u32) -> (SeatLayout, [f64; 4], usize, f32) {
        let metrics = seat_metrics(dpi_milli);
        let ppm = scale_ppm(dpi_milli);
        let layout = solve(
            &tree,
            logical_viewport(W, H, ppm),
            &metrics,
            SeatId(1),
            LayoutMode::Parallel,
        )
        .expect("the stage layouts all fit");
        let count = tree.seats_in_order().len();
        (
            layout,
            device_viewport(W, H, ppm),
            count,
            dpi_milli as f32 / 1_000.0,
        )
    }

    /// Two panes stacked, so the seam between them runs horizontally and meets
    /// both side rims.
    fn stacked() -> LayoutNode {
        LayoutNode::split(SplitId(1), Axis::Col, term(1), term(2))
    }

    fn side_by_side() -> LayoutNode {
        LayoutNode::split(SplitId(1), Axis::Row, term(1), term(2))
    }

    fn aim(stage: &(SeatLayout, [f64; 4], usize, f32), x: f64, y: f64) -> Option<LayoutAim> {
        aim_at_layout(&stage.0, stage.1, stage.2, stage.3, x, y)
    }

    fn rect_of(layout: &SeatLayout, seat: u64) -> bt_layout::DeviceRect {
        layout.get(SeatId(seat)).unwrap().device_rect.unwrap()
    }

    /// **K131, the ruling the whole ordering exists for.** The rim is measured
    /// before any pane is looked for, and the proof is the one pointer that can
    /// tell the two orders apart: on the rim, at the height of a divider.
    ///
    /// Run the pane hit first and this pointer matches no pane, returns early,
    /// and never reaches the rim — "the gesture died exactly along the seam of
    /// the very layout it exists to serve" (mock-up 7048-7051). The assertion
    /// below is that it does not.
    ///
    /// Red gate: the mutation is the reordering itself. Move the `pane_at` call
    /// above the rim test in `aim_at_layout` and the first two assertions answer
    /// `None`, while every other test in this module still passes — which is
    /// exactly how the feature was lost the first time.
    #[test]
    fn the_rim_is_measured_before_any_pane_is_found() {
        let stage = stage(stacked(), DPI);
        let seam = rect_of(&stage.0, 1).bottom as f64;
        assert_eq!(
            aim(&stage, 4.0, seam),
            Some(LayoutAim::Rim(DropEdge::Left)),
            "a pointer on the left rim at the seam's height must reach the rim"
        );
        assert_eq!(
            aim(&stage, f64::from(W) - 4.0, seam),
            Some(LayoutAim::Rim(DropEdge::Right)),
            "and so must the right rim"
        );
        // The same height away from the rim is the seam and nothing else (K132),
        // which is what makes the pair above a statement about ordering rather
        // than about the seam being generous.
        assert_eq!(aim(&stage, f64::from(W) / 2.0, seam), None);
    }

    /// **K130 — `OUTER_RIM = 48`, and it is 48 *logical* pixels.**
    ///
    /// Both halves are load-bearing: the depth is exact and strict (`< 48`), and
    /// it scales with the display, because a rim measured in device pixels would
    /// be half as deep on a 200% screen and the gesture would get harder as the
    /// window got bigger.
    ///
    /// Red gate: change the constant to 47 or 49 and the boundary pair below
    /// disagrees; drop the `* scale` and the 1.5x and 2x cases fall through to a
    /// pane zone.
    #[test]
    fn the_rim_is_forty_eight_logical_pixels_deep_at_every_scale() {
        for dpi_milli in [1_000u32, 1_500, 2_000] {
            let stage = stage(stacked(), dpi_milli);
            let scale = f64::from(stage.3);
            let mid = f64::from(H) / 2.0;
            assert_eq!(
                aim(&stage, 48.0 * scale - 0.5, mid),
                Some(LayoutAim::Rim(DropEdge::Left)),
                "just inside the rim at {dpi_milli} milli-DPI"
            );
            let outside = aim(&stage, 48.0 * scale, mid);
            assert!(
                matches!(outside, Some(LayoutAim::SeatEdge(_, DropEdge::Left))),
                "exactly 48 logical px in is past the rim and inside a pane's own \
                 edge zone at {dpi_milli} milli-DPI, got {outside:?}"
            );
        }
    }

    /// **K131 — a lone pane skips the rim entirely**, because splitting the root
    /// and splitting the only pane are the same act.
    ///
    /// Red gate: delete the `pane_count > 1` guard and this answers `Rim(Left)`.
    #[test]
    fn a_lone_pane_has_no_rim() {
        let stage = stage(term(1), DPI);
        assert_eq!(stage.2, 1);
        assert_eq!(
            aim(&stage, 4.0, f64::from(H) / 2.0),
            Some(LayoutAim::SeatEdge(SeatId(1), DropEdge::Left)),
            "the only pane's own left edge, not the root's"
        );
    }

    /// **K127 and K128 together: up is somewhere else, the other three clamp.**
    ///
    /// Pushing past an edge is how a hand aims at an edge, so the rim is 48px
    /// deep going in and bottomless going out — a pointer half a window past the
    /// left edge is still asking for the left rim. Up is the exception, because
    /// the strip lives up there and has already had its say.
    ///
    /// Red gate: drop either clamp and the far-out pointers fall through to
    /// `pane_at`, which answers `None`; clamp the top as well and the pointer
    /// above the window starts claiming the top rim, which is the strip's.
    #[test]
    fn three_sides_clamp_and_above_the_layout_is_somewhere_else() {
        let stacked_stage = stage(stacked(), DPI);
        let mid_x = f64::from(W) / 2.0;
        let mid_y = f64::from(H) / 2.0;
        assert_eq!(
            aim(&stacked_stage, -500.0, mid_y),
            Some(LayoutAim::Rim(DropEdge::Left))
        );
        assert_eq!(
            aim(&stacked_stage, f64::from(W) + 500.0, mid_y),
            Some(LayoutAim::Rim(DropEdge::Right))
        );
        assert_eq!(
            aim(&stacked_stage, mid_x, f64::from(H) + 500.0),
            Some(LayoutAim::Rim(DropEdge::Bottom))
        );
        assert_eq!(
            aim(&stacked_stage, mid_x, stacked_stage.1[1] - 0.5),
            None,
            "one pixel above the layout is the strip's business, not a rim"
        );
        assert_eq!(
            aim(&stacked_stage, mid_x, stacked_stage.1[1]),
            Some(LayoutAim::Rim(DropEdge::Top)),
            "the layout's own first row is the top rim"
        );
        // With two panes the rim answers a pushed-past pointer at a distance of
        // zero, so it would answer even unclamped. A *lone* pane skips the rim
        // (K131) and therefore has to be hit by the clamp itself — which is the
        // only place the clamp is observable, and the reason it must land on the
        // last column inside the layout rather than on the first one outside it.
        let lone = stage(term(1), DPI);
        assert_eq!(
            aim(&lone, -500.0, mid_y),
            Some(LayoutAim::SeatEdge(SeatId(1), DropEdge::Left))
        );
        assert_eq!(
            aim(&lone, f64::from(W) + 500.0, mid_y),
            Some(LayoutAim::SeatEdge(SeatId(1), DropEdge::Right))
        );
        assert_eq!(
            aim(&lone, mid_x, f64::from(H) + 500.0),
            Some(LayoutAim::SeatEdge(SeatId(1), DropEdge::Bottom))
        );
    }

    /// **The corner rule, in both places it applies.** `reduce((a, b) => d[a] <=
    /// d[b] ? a : b)` keeps the accumulator on a tie, so an exact tie goes to the
    /// side named first in `{left, right, top, bottom}`.
    ///
    /// Red gate: reorder `DropEdge::NEAREST_FIRST`, or turn `<` into `<=` in
    /// `nearest_side`, and every assertion here picks the other side of its
    /// corner.
    #[test]
    fn a_corner_belongs_to_the_side_named_first() {
        let stacked_stage = stage(stacked(), DPI);
        // The layout's own top-left corner: left and top are both zero.
        assert_eq!(
            aim(&stacked_stage, 0.0, stacked_stage.1[1]),
            Some(LayoutAim::Rim(DropEdge::Left)),
            "left is named before top"
        );
        // The bottom-right: right and bottom are both zero, right is named first.
        assert_eq!(
            aim(&stacked_stage, f64::from(W), f64::from(H)),
            Some(LayoutAim::Rim(DropEdge::Right))
        );
        // And inside a pane, away from the rim: a pointer equidistant from the
        // left and the top of its own pane takes the left.
        let stage = stage(side_by_side(), DPI);
        let pane = rect_of(&stage.0, 1);
        let (w, h) = (pane.width() as f64, pane.height() as f64);
        assert_eq!(
            aim(
                &stage,
                pane.left as f64 + w * 0.2,
                pane.top as f64 + h * 0.2
            ),
            Some(LayoutAim::SeatEdge(SeatId(1), DropEdge::Left))
        );
        assert_eq!(
            aim(
                &stage,
                pane.left as f64 + w * 0.8,
                pane.top as f64 + h * 0.8
            ),
            Some(LayoutAim::SeatEdge(SeatId(1), DropEdge::Right)),
            "right is named before bottom"
        );
    }

    /// **K134 — the outer 35% splits, the middle takes its place**, and the
    /// threshold is strict.
    ///
    /// The fraction is what makes "anywhere in the outer third splits" true
    /// without the hand having to reach the very edge, and what makes the middle
    /// a target that needs no aim at all.
    ///
    /// Red gate: move the fraction to 0.3 or 0.4 and the boundary pair below
    /// disagrees; turn `<` into `<=` and the exact-35% case flips.
    #[test]
    fn the_outer_thirty_five_percent_splits_and_the_middle_takes_its_place() {
        let stage = stage(side_by_side(), DPI);
        let pane = rect_of(&stage.0, 1);
        let (w, h) = (pane.width() as f64, pane.height() as f64);
        let y = pane.top as f64 + h / 2.0;
        let at = |fraction: f64| aim(&stage, pane.left as f64 + w * fraction, y);
        assert_eq!(
            at(0.34),
            Some(LayoutAim::SeatEdge(SeatId(1), DropEdge::Left))
        );
        assert_eq!(
            at(0.35),
            Some(LayoutAim::SeatCentre(SeatId(1))),
            "exactly 35% in is already the middle"
        );
        assert_eq!(at(0.5), Some(LayoutAim::SeatCentre(SeatId(1))));
        assert_eq!(
            at(0.66),
            Some(LayoutAim::SeatEdge(SeatId(1), DropEdge::Right))
        );
        // The vertical zones of the same pane, which the horizontal ones must not
        // have eaten: at the pane's horizontal centre the nearest side is a top
        // or a bottom.
        let x = pane.left as f64 + w / 2.0;
        assert_eq!(
            aim(&stage, x, pane.top as f64 + h * 0.1),
            Some(LayoutAim::SeatEdge(SeatId(1), DropEdge::Top))
        );
        assert_eq!(
            aim(&stage, x, pane.top as f64 + h * 0.9),
            Some(LayoutAim::SeatEdge(SeatId(1), DropEdge::Bottom))
        );
    }

    /// **K132 — no pane under the pointer means nothing to aim at.** That is a
    /// pointer on a divider, and a divider is not a target.
    ///
    /// Red gate: widen the pane hit to the nearest rectangle instead of the
    /// containing one and this starts answering a seat.
    #[test]
    fn a_pointer_on_a_divider_aims_at_nothing() {
        let stage = stage(side_by_side(), DPI);
        assert_eq!(
            aim(
                &stage,
                rect_of(&stage.0, 1).right as f64,
                f64::from(H) / 2.0
            ),
            None,
            "the device column the divider occupies belongs to neither pane"
        );
    }

    /// **G81's `zone→dir`, stated as a table.** left/right divide a row,
    /// top/bottom divide a column, and the leading slot is the one named by left
    /// or top — which is what makes "drop on the left" put the arriving seat on
    /// the left.
    ///
    /// Red gate: swap either mapping and half the drops land on the wrong side of
    /// their own divider.
    #[test]
    fn an_edge_names_an_axis_and_a_side() {
        assert_eq!(DropEdge::Left.axis(), Axis::Row);
        assert_eq!(DropEdge::Right.axis(), Axis::Row);
        assert_eq!(DropEdge::Top.axis(), Axis::Col);
        assert_eq!(DropEdge::Bottom.axis(), Axis::Col);
        assert!(DropEdge::Left.leading());
        assert!(DropEdge::Top.leading());
        assert!(!DropEdge::Right.leading());
        assert!(!DropEdge::Bottom.leading());
    }

    /// **K123 against K127: the strip and the layout partition the window.**
    ///
    /// The strip's bottom edge is the layout's top edge exactly, and the
    /// half-open test gives the boundary row to the layout. That is what lets the
    /// two questions be asked in order without either knowing about the other:
    /// no row is in both, and no row is in neither.
    ///
    /// Red gate: give the strip its own idea of the title bar's height — round
    /// where the solver floors, or read a different constant — and some scale
    /// produces a row that answers "not the strip" and "above the layout" at
    /// once, which is a row where a drag can point at nothing.
    #[test]
    fn the_strip_ends_exactly_where_the_layout_begins() {
        for dpi_milli in [1_000u32, 1_250, 1_500, 1_750, 2_000, 2_500] {
            let scale = dpi_milli as f32 / 1_000.0;
            let geometry = tab_strip_geometry(W as f32, scale, &[TabTrailer::default()], 0, 0.0);
            let band = strip_band(&geometry, scale);
            let host = device_viewport(W, H, scale_ppm(dpi_milli));
            assert_eq!(
                f64::from(band[3]),
                host[1],
                "the strip's floor is the layout's ceiling at {dpi_milli} milli-DPI"
            );
            assert!(in_strip(&geometry, scale, 10.0, host[1] - 1.0));
            assert!(!in_strip(&geometry, scale, 10.0, host[1]));
        }
    }

    /// **K125 — `insertIndexAt`: the first slot whose middle the pointer has not
    /// passed, else the end of the run.**
    ///
    /// Red gate: turn `<` into `<=`, or drop the `unwrap_or(len)`, and a pointer
    /// past the last tab stops asking for the end of the strip.
    #[test]
    fn a_pane_arriving_at_the_strip_takes_the_slot_it_points_at() {
        let mids = [50.0f32, 150.0, 250.0];
        assert_eq!(insert_index_at(&mids, 0.0), 0);
        assert_eq!(insert_index_at(&mids, 49.0), 0);
        assert_eq!(
            insert_index_at(&mids, 50.0),
            1,
            "a middle belongs to the slot after it"
        );
        assert_eq!(insert_index_at(&mids, 149.0), 1);
        assert_eq!(
            insert_index_at(&mids, 251.0),
            3,
            "past every middle is the end of the run"
        );
        assert_eq!(insert_index_at(&[], 10.0), 0);
    }
}
