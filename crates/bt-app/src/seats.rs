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
    ChromeLabel, ChromeQuad, PANE_HEAD_FILE_MARK_LOGICAL_PX, PANE_HEAD_FOLDER_MARK_LOGICAL_PX,
    PANE_HEAD_PROFILE_MARK_LOGICAL_PX, SEAT_DIVIDER_HIT_LOGICAL_PX, SEAT_TITLE_BAR_LOGICAL_PX,
    SEAT_TITLE_EDGE_LOGICAL_PX, SEAT_TITLE_FONT_LOGICAL_PX, SEAT_TITLE_GAP_LOGICAL_PX,
    SEAT_TITLE_PADDING_LOGICAL_PX, SeatViewport, WINDOW_CAPTION_BUTTON_LOGICAL_PX,
    WINDOW_CAPTION_GEAR_GLYPH_LOGICAL_PX, WINDOW_CAPTION_GLYPH_LOGICAL_PX,
    WINDOW_NEW_TAB_BOX_LOGICAL_PX, WINDOW_NEW_TAB_CHEVRON_HEIGHT_LOGICAL_PX,
    WINDOW_NEW_TAB_CHEVRON_WIDTH_LOGICAL_PX, WINDOW_NEW_TAB_GLYPH_LOGICAL_PX,
    WINDOW_NEW_TAB_MARGIN_BOTTOM_LOGICAL_PX, WINDOW_NEW_TAB_MARGIN_LEFT_LOGICAL_PX,
    WINDOW_NEW_TAB_RADIUS_LOGICAL_PX, WINDOW_TAB_CLOSE_BOX_LOGICAL_PX,
    WINDOW_TAB_CLOSE_GLYPH_LOGICAL_PX, WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX,
    WINDOW_TAB_FONT_LOGICAL_PX, WINDOW_TAB_GAP_BETWEEN_LOGICAL_PX, WINDOW_TAB_GAP_LOGICAL_PX,
    WINDOW_TAB_HEIGHT_LOGICAL_PX, WINDOW_TAB_MARK_LOGICAL_PX, WINDOW_TAB_MAX_WIDTH_LOGICAL_PX,
    WINDOW_TAB_PADDING_LEFT_LOGICAL_PX, WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX,
    WINDOW_TAB_RADIUS_LOGICAL_PX, WINDOW_TAB_SQUEEZED_LOGICAL_PX,
    WINDOW_TAB_SQUEEZED_PADDING_LOGICAL_PX, WINDOW_TAB_TIGHT_LOGICAL_PX,
    WINDOW_TITLE_BAR_LOGICAL_PX, chrome_palette,
};

use crate::marks::{ChromeMark, ChromeSprite};

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
    pub fn has_pane_headers(&self) -> bool {
        self.tree.seats_in_order().len() > 1
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

/// The L4 presentation: what to show when `solve` says the window cannot hold
/// this tree at all (tiny-window §2 last row, §4.3).
///
/// The focus seat takes the viewport and every other seat is simply not
/// presented. Two things about that are worth stating plainly:
///
/// * It is **not** a cached previous frame. §4.3 forbids silently reusing the
///   last geometry, because that dresses a failed solve up as a successful one.
///   This rectangle is derived from the current viewport and nothing else.
/// * For a lone terminal leaf it is *numerically the same answer* `solve`
///   returns on success — one seat, the whole viewport — so a window dragged
///   below the terminal's own minimum behaves exactly as it did before seats
///   existed, rather than acquiring a new failure state.
///
/// What is not implemented here is the refinement §4.3 also names: rendering as
/// many collapsed bars as *do* fit with a trailing "N more do not fit". That
/// wants a second allocator with its own stopping rule, and this slice's gate is
/// the lone-leaf identity; the gap is recorded rather than approximated.
pub fn fit_what_fits(seats: &Seats, viewport: LogicalRect, metrics: &SeatMetrics) -> SeatLayout {
    let device = |rect: LogicalRect| bt_layout::DeviceRect {
        left: snap(rect.left, metrics.scale_ppm()),
        top: snap(rect.top, metrics.scale_ppm()),
        right: snap(rect.right, metrics.scale_ppm()),
        bottom: snap(rect.bottom, metrics.scale_ppm()),
    };
    let rects = seats
        .tree
        .seats_in_order()
        .into_iter()
        .map(|seat| {
            let on_stage = seat.id == seats.focus;
            bt_layout::SeatPlacement {
                id: seat.id,
                kind: seat.kind,
                rect: on_stage.then_some(viewport),
                device_rect: on_stage.then(|| device(viewport)),
                presentation: Presentation::Full,
            }
        })
        .collect();
    SeatLayout { rects }
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
    let title_px =
        logical_to_device(WINDOW_TITLE_BAR_LOGICAL_PX, scale_ppm).min(height_px.saturating_sub(1));
    LogicalRect::new(
        LogicalPx::ZERO,
        device_to_logical(title_px, scale_ppm),
        device_to_logical(width_px, scale_ppm),
        device_to_logical(height_px, scale_ppm),
    )
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
    if !seats.has_pane_headers() {
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
    /// The common pane head. Tool buttons arrive in a later slice; for now the
    /// bar is chrome and therefore never terminal input.
    PaneHeader(SeatId),
    Settings,
    Minimize,
    Maximize,
    CloseWindow,
    Tab(usize),
    TabClose(usize),
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabGeometry {
    pub body: [f32; 4],
    /// The `×`'s box — `None` exactly when the mock-up's width tiers take the
    /// affordance away, which is also exactly when a press there must fall
    /// through to the tab instead of closing it.
    pub close: Option<[f32; 4]>,
    pub tier: TabWidthTier,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabStripGeometry {
    pub tabs: Vec<TabGeometry>,
    pub new_tab: [f32; 4],
    /// The `˅` beside the `+`: the same 28px box, no margin between them.
    pub new_tab_menu: [f32; 4],
}

/// Equal-share horizontal tab geometry. The mock-up's 200px cap is retained; once the strip is
/// full every tab compresses by the same amount. Scrolling is deliberately deferred.
///
/// `active_tab` is a geometry input and not merely a paint one, because the
/// mock-up's width tiers keep the active tab's `×` and take everyone else's:
/// `.tab.tight:not(.active) .close { display: none }`.
pub fn tab_strip_geometry(
    width: f32,
    scale: f32,
    tab_count: usize,
    active_tab: usize,
) -> TabStripGeometry {
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
        ((available - total_gaps).max(0.0) / tab_count as f32)
            .min(WINDOW_TAB_MAX_WIDTH_LOGICAL_PX * scale)
    };
    let tier = tab_width_tier(tab_width, scale);
    let tab_height = (WINDOW_TAB_HEIGHT_LOGICAL_PX * scale).round();
    let tab_top = title - tab_height;
    let close_box = (WINDOW_TAB_CLOSE_BOX_LOGICAL_PX * scale).round();
    let close_pad = WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX * scale;
    let mark = (WINDOW_TAB_MARK_LOGICAL_PX * scale).round();
    let content_gap = WINDOW_TAB_GAP_LOGICAL_PX * scale;
    let tabs = (0..tab_count)
        .map(|index| {
            let left = radius + index as f32 * (tab_width + gap);
            let right = left + tab_width;
            let active = index == active_tab;
            let close_top = tab_top + (tab_height - close_box) / 2.0;
            let close = match tier {
                // `.tab.tight:not(.active) .close` and its `.squeezed` twin.
                TabWidthTier::Tight | TabWidthTier::Squeezed if !active => None,
                // A squeezed tab centres what is left of it: the mark, the tab's
                // own 8px gap, and the `×` the active tab keeps.
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
            };
            TabGeometry {
                body: [left, tab_top, right, title],
                close: close.filter(|rect| rect[2] > rect[0]),
                tier,
            }
        })
        .collect::<Vec<_>>();
    let tabs_right = tabs.last().map_or(radius, |tab| tab.body[2]);
    let new_left = (tabs_right + new_margin).min(run_left);
    let new_bottom = title - WINDOW_NEW_TAB_MARGIN_BOTTOM_LOGICAL_PX * scale;
    let menu_left = (new_left + new_box).min(run_left);
    TabStripGeometry {
        tabs,
        new_tab: [
            new_left,
            new_bottom - new_box,
            (new_left + new_box).min(run_left),
            new_bottom,
        ],
        new_tab_menu: [
            menu_left,
            new_bottom - new_box,
            (menu_left + new_box).min(run_left),
            new_bottom,
        ],
    }
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

/// Physical right edge of the app-owned tab run, including both end buttons.
pub fn tab_strip_right_px(width: f32, scale: f32, tab_count: usize) -> i32 {
    // The tiers move nothing outside a tab's own body, so the run's right edge
    // is the same whichever tab is active.
    tab_strip_geometry(width, scale, tab_count, 0).new_tab_menu[2].ceil() as i32
}

pub fn hit_tab_chrome(
    width: f32,
    scale: f32,
    tab_count: usize,
    active_tab: usize,
    x: f64,
    y: f64,
) -> Option<ChromeTarget> {
    let (x, y) = (x as f32, y as f32);
    let geometry = tab_strip_geometry(width, scale, tab_count, active_tab);
    for (index, tab) in geometry.tabs.iter().enumerate() {
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
        if seats.has_pane_headers() {
            let head_bottom = (rect[1] + SEAT_TITLE_BAR_LOGICAL_PX * scale).min(rect[3]);
            if contains([rect[0], rect[1], rect[2], head_bottom], x, y) {
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
    let title = WINDOW_TITLE_BAR_LOGICAL_PX * scale;
    if y < 0.0 || y >= title || x < 0.0 || x >= width {
        return None;
    }
    let button = WINDOW_CAPTION_BUTTON_LOGICAL_PX * scale;
    let run_left = (width - 4.0 * button).max(0.0);
    if x < run_left {
        return None;
    }
    let index = ((x - run_left) / button).floor() as u32;
    match index {
        0 => Some(ChromeTarget::Settings),
        1 => Some(ChromeTarget::Minimize),
        2 => Some(ChromeTarget::Maximize),
        _ => Some(ChromeTarget::CloseWindow),
    }
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
    let titles = [tab_title.unwrap_or("PowerShell").to_owned()];
    build_chrome_for_tabs(
        seats,
        layout,
        scale,
        pointer,
        ChromeContent {
            tab_titles: &titles,
            active_tab: 0,
            preview_title,
            preview_message,
            profile_menu_open: false,
        },
    )
}

pub struct ChromeContent<'a> {
    pub tab_titles: &'a [String],
    pub active_tab: usize,
    pub preview_title: Option<&'a str>,
    pub preview_message: Option<&'a str>,
    /// Whether the profile picker is up. The chevron states where its list is —
    /// down when it is folded away, up when it is already on screen — so the
    /// button has to be told, and the menu itself is drawn in the overlay layer.
    pub profile_menu_open: bool,
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
        tab_titles,
        active_tab,
        preview_title,
        preview_message,
        profile_menu_open,
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
        tab_titles,
        active_tab,
        profile_menu_open,
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
                    seat_title(placement.kind),
                    &mut quads,
                    &mut labels,
                );
            }
            Presentation::Full => {
                if !seats.has_pane_headers() {
                    continue;
                }
                let bar = SEAT_TITLE_BAR_LOGICAL_PX * scale;
                let title_bottom = (rect[1] + bar).min(rect[3]);
                if placement.id != seats.terminal() {
                    quads.push(ChromeQuad {
                        rect: [rect[0], title_bottom, rect[2], rect[3]],
                        color: palette.seat_body,
                    });
                }
                quads.push(ChromeQuad {
                    rect: [rect[0], rect[1], rect[2], title_bottom],
                    color: palette.pane_head,
                });
                // The hairline that makes the bar a caption rather than a stripe.
                let edge = (SEAT_TITLE_EDGE_LOGICAL_PX * scale).max(1.0);
                if title_bottom + edge <= rect[3] {
                    quads.push(ChromeQuad {
                        rect: [rect[0], title_bottom, rect[2], title_bottom + edge],
                        color: palette.pane_head_edge,
                    });
                }
                // `.panehead { gap: 7px; padding: 0 6px 0 12px }` with the seat's
                // own mark leading: a terminal wears its profile square, a
                // preview the file mark, a files pane the folder — the marks the
                // mock-up puts in exactly these three heads.
                let pad = SEAT_TITLE_PADDING_LOGICAL_PX * scale;
                let (mark, mark_logical_px, mark_color) = pane_mark(placement.kind, palette);
                let mark_size = (mark_logical_px * scale).round().max(1.0);
                let mark_left = (rect[0] + pad).round();
                let mark_top = (rect[1] + ((title_bottom - rect[1]) - mark_size) / 2.0).round();
                sprites.push(ChromeSprite {
                    mark,
                    rect: [
                        mark_left,
                        mark_top,
                        mark_left + mark_size,
                        mark_top + mark_size,
                    ],
                    color: mark_color,
                });
                labels.push(ChromeLabel {
                    text: if placement.kind == SeatKind::Preview {
                        preview_title.unwrap_or_else(|| seat_title(placement.kind))
                    } else {
                        seat_title(placement.kind)
                    }
                    .to_owned(),
                    rect: [
                        mark_left + mark_size + SEAT_TITLE_GAP_LOGICAL_PX * scale,
                        rect[1],
                        rect[2] - pad,
                        title_bottom,
                    ],
                    font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
                    color: if placement.id == seats.focus() {
                        palette.pane_title_focus
                    } else {
                        palette.pane_title
                    },
                    align_right: false,
                    align_center: false,
                    letter_spacing_em: 0.0,
                });
                if placement.kind == SeatKind::Preview
                    && let Some(message) = preview_message
                {
                    // A state notice, not content: quiet ink, centred in the
                    // body, so an empty pane reads as an invitation and a
                    // failure reads as a note rather than a wall of alarm.
                    labels.push(ChromeLabel {
                        text: message.to_owned(),
                        rect: [
                            rect[0] + pad,
                            title_bottom + pad,
                            rect[2] - pad,
                            rect[3] - pad,
                        ],
                        font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
                        color: palette.body_hint_text,
                        align_right: false,
                        align_center: true,
                        letter_spacing_em: 0.0,
                    });
                }
            }
        }
    }
    for slot in seats.split_slots(layout) {
        let color = if pointer.dragging == Some(slot.id) {
            palette.divider_active
        } else if pointer.hover == Some(ChromeTarget::Divider(slot.id)) {
            palette.divider_hover
        } else {
            palette.divider
        };
        quads.push(ChromeQuad {
            rect: slot.band,
            color,
        });
    }
    (quads, labels, sprites)
}

fn window_chrome(
    width: f32,
    scale: f32,
    hover: Option<ChromeTarget>,
    tab_titles: &[String],
    active_tab: usize,
    profile_menu_open: bool,
    output: (
        &mut Vec<ChromeQuad>,
        &mut Vec<ChromeLabel>,
        &mut Vec<ChromeSprite>,
    ),
) {
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
    let geometry = tab_strip_geometry(width, scale, tab_titles.len(), active_tab);
    for (index, (tab, tab_title)) in geometry.tabs.iter().zip(tab_titles).enumerate() {
        let [tab_left, tab_top, tab_right, tab_bottom] = tab.body;
        let active = index == active_tab;
        // `.tab:hover` is one hover: a pointer on the `×` is still a pointer on
        // the tab, so the body lights up and the title steps to `--ink` for both.
        let tab_hovered =
            hover == Some(ChromeTarget::Tab(index)) || hover == Some(ChromeTarget::TabClose(index));
        if active && tab_right - tab_left >= 2.0 * radius {
            sprites.push(ChromeSprite {
                mark: ChromeMark::ActiveTab {
                    radius_px: radius as u32,
                },
                rect: [tab_left - radius, tab_top, tab_right + radius, tab_bottom],
                color: palette.active_tab,
            });
        } else if tab_hovered {
            sprites.push(ChromeSprite {
                mark: ChromeMark::TabBody {
                    radius_px: radius as u32,
                },
                rect: tab.body,
                color: palette.caption_hover,
            });
        }
        let mark = (WINDOW_TAB_MARK_LOGICAL_PX * scale).round();
        let content_gap = WINDOW_TAB_GAP_LOGICAL_PX * scale;
        // `.tab.squeezed { justify-content: center; padding: 0 4px }` — the mark
        // and whatever else survived are centred as one group, not indented.
        let mark_left = if tab.tier == TabWidthTier::Squeezed {
            let trailing = tab
                .close
                .map_or(0.0, |close| content_gap + close[2] - close[0]);
            (tab_left + (tab_right - tab_left - mark - trailing) / 2.0)
                .max(tab_left + WINDOW_TAB_SQUEEZED_PADDING_LOGICAL_PX * scale)
                .round()
        } else {
            (tab_left + WINDOW_TAB_PADDING_LEFT_LOGICAL_PX * scale).round()
        };
        let mark_top = (tab_top + (tab_bottom - tab_top - mark) / 2.0).round();
        // What the title may not run past: the `×` and the tab's own 8px gap
        // before it, or the trailing padding when there is no `×` to clear.
        let content_right = tab.close.map_or(
            tab_right - WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX * scale,
            |close| close[0] - content_gap,
        );
        if mark_left + mark <= tab.close.map_or(tab_right, |close| close[0]) {
            sprites.push(ChromeSprite {
                mark: ChromeMark::ProfilePowerShell,
                rect: [mark_left, mark_top, mark_left + mark, mark_top + mark],
                color: palette.accent,
            });
            let label_left = mark_left + mark + content_gap;
            // `.tab.squeezed .ttitle { display: none }`: below 90px the tab is
            // its mark, and nothing is gained by clipping a word to two letters.
            if tab.tier != TabWidthTier::Squeezed && label_left < content_right {
                labels.push(ChromeLabel {
                    text: tab_title.clone(),
                    rect: [label_left, tab_top, content_right, tab_bottom],
                    font_size_px: WINDOW_TAB_FONT_LOGICAL_PX * scale,
                    color: if active || tab_hovered {
                        palette.pane_title_focus
                    } else {
                        palette.title_text
                    },
                    align_right: false,
                    align_center: false,
                    letter_spacing_em: 0.0,
                });
            }
        }
        let Some(close) = tab.close else {
            continue;
        };
        let close_hovered = hover == Some(ChromeTarget::TabClose(index));
        if close_hovered {
            // `.tab .close:hover { background: var(--active) }` — 4px of round,
            // over whichever of the two surfaces this tab is showing: `--termbg`
            // when it is the active one, its own `--hover` fill when it is not.
            sprites.push(ChromeSprite {
                mark: ChromeMark::ControlPill {
                    radius_px: (WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX * scale)
                        .round()
                        .max(1.0) as u32,
                },
                rect: pixel_snapped(close),
                color: if active {
                    palette.tab_close_pill_on_content
                } else {
                    palette.tab_close_pill_on_hovered_tab
                },
            });
        }
        let glyph = (WINDOW_TAB_CLOSE_GLYPH_LOGICAL_PX * scale).round().max(1.0);
        let glyph_left = ((close[0] + close[2] - glyph) / 2.0).round();
        let glyph_top = ((close[1] + close[3] - glyph) / 2.0).round();
        sprites.push(ChromeSprite {
            mark: ChromeMark::TabClose,
            rect: [glyph_left, glyph_top, glyph_left + glyph, glyph_top + glyph],
            color: if close_hovered {
                palette.title_text_hover
            } else {
                // `.tab .close { color: var(--ink3) }` — a step below the caption
                // run's own ink, because closing a tab is not what the strip is for.
                palette.title_text_muted
            },
        });
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
        if hovered && rect[2] > rect[0] {
            sprites.push(ChromeSprite {
                mark: ChromeMark::ControlPill {
                    radius_px: pill_radius,
                },
                rect: pixel_snapped(rect),
                // `--hover` over `--panel` and nothing else is ever under it, so
                // this one the palette can and does pre-composite.
                color: palette.caption_hover,
            });
        }
    }
    let plus = (WINDOW_NEW_TAB_GLYPH_LOGICAL_PX * scale).round().max(1.0);
    let plus_left = ((geometry.new_tab[0] + geometry.new_tab[2] - plus) / 2.0).round();
    let plus_top = ((geometry.new_tab[1] + geometry.new_tab[3] - plus) / 2.0).round();
    if geometry.new_tab[2] > geometry.new_tab[0] {
        sprites.push(ChromeSprite {
            mark: ChromeMark::Plus,
            rect: [plus_left, plus_top, plus_left + plus, plus_top + plus],
            color: if new_hovered {
                palette.title_text_hover
            } else {
                palette.title_text_muted
            },
        });
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
    if geometry.new_tab_menu[2] > geometry.new_tab_menu[0] {
        sprites.push(ChromeSprite {
            mark: ChromeMark::Chevron {
                open: profile_menu_open,
            },
            rect: [
                chevron_left,
                chevron_top,
                chevron_left + chevron_width,
                chevron_top + chevron_height,
            ],
            color: if menu_hovered || profile_menu_open {
                palette.title_text_hover
            } else {
                palette.title_text_muted
            },
        });
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
        sprites.push(ChromeSprite {
            mark,
            rect: [glyph_left, glyph_top, glyph_left + glyph, glyph_top + glyph],
            color: if hovered && target == ChromeTarget::CloseWindow {
                palette.caption_close_text
            } else if hovered {
                palette.title_text_hover
            } else {
                palette.title_text
            },
        });
    }
}

/// A collapsed seat's bar carries a name and a state icon (§2.6.3) — except in
/// the double-collapsed 24x24 case, where 24 square cannot hold a name and
/// forcing one in would be a mosaic pretending to be text (tiny-window §1.3).
/// The state icon is a placeholder block until the shared `stateIcon` component
/// of DESIGN §7.1.5b exists to be borrowed.
fn collapse_bar_contents(
    rect: [f32; 4],
    scale: f32,
    title: &str,
    quads: &mut Vec<ChromeQuad>,
    labels: &mut Vec<ChromeLabel>,
) {
    let palette = chrome_palette();
    let pad = SEAT_TITLE_PADDING_LOGICAL_PX * scale;
    let icon = 6.0 * scale;
    let width = rect[2] - rect[0];
    let height = rect[3] - rect[1];
    let icon_left = rect[0] + (width - icon).max(0.0) / 2.0;
    let icon_top = rect[1] + (height - icon).max(0.0) / 2.0;
    if width >= icon && height >= icon {
        quads.push(ChromeQuad {
            rect: [icon_left, icon_top, icon_left + icon, icon_top + icon],
            color: palette.title_text,
        });
    }
    // A name only fits along the axis that was *not* squeezed. A bar squeezed on
    // both is the degenerate square, and it gets the icon alone.
    let horizontal_bar = width > height;
    if horizontal_bar && width > icon + 4.0 * pad {
        labels.push(ChromeLabel {
            text: title.to_owned(),
            rect: [rect[0] + pad, rect[1], icon_left - pad, rect[3]],
            font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
            color: palette.title_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
        });
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
fn pane_mark(kind: SeatKind, palette: bt_render::ChromePalette) -> (ChromeMark, f32, [u8; 3]) {
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
    /// The durable form of this tree.
    pub fn to_persisted(&self) -> LayoutNodeV1 {
        to_persisted(&self.tree)
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

fn to_persisted(node: &LayoutNode) -> LayoutNodeV1 {
    match node {
        LayoutNode::Seat(seat) => LayoutNodeV1::Leaf(match seat.kind {
            // `profile_id` is v1's transitional "the shell this pane actually
            // launched" (§3.3); `cwd` is left to the restart-shell contract's
            // consumer, which is not this slice.
            SeatKind::Terminal => LeafNodeV1::Term(TermLeafV1 {
                profile_id: "pwsh.exe".to_owned(),
                cwd: String::new(),
                manual_name: None,
            }),
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
            children: [Box::new(to_persisted(a)), Box::new(to_persisted(b))],
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
        seats
            .solve(viewport, metrics)
            .unwrap_or_else(|_| fit_what_fits(seats, viewport, metrics))
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
                            rect.top as f32 + SEAT_TITLE_BAR_LOGICAL_PX,
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
                root: seats.to_persisted(),
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
        let overlay = crate::settings::layout(width as f32, height as f32, 1.0, false)
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

    #[test]
    fn multi_tab_strip_is_equal_width_and_exposes_plus_close_and_middle_click_targets() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let geometry = tab_strip_geometry(960.0 * scale, scale, 4, 2);
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
                    4,
                    2,
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
                    4,
                    2,
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
                    4,
                    2,
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
                tab_strip_geometry(width, scale, 1, 0).new_tab_menu[2].ceil() as i32,
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
                dragging: None,
            },
            ChromeContent {
                tab_titles: titles,
                active_tab,
                preview_title: None,
                preview_message: None,
                profile_menu_open,
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
            let geometry = tab_strip_geometry(960.0 * scale, scale, 1, 0);
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
            let geometry = tab_strip_geometry(960.0 * scale, scale, 1, 0);
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
            for mark in [ChromeMark::Plus, ChromeMark::Chevron { open: false }] {
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
        assert_eq!(shut_chevron.mark, ChromeMark::Chevron { open: false });
        assert_eq!(open_chevron.mark, ChromeMark::Chevron { open: true });
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
                let geometry = tab_strip_geometry(width, scale, count, 0);
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
                            count,
                            0,
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
                            count,
                            0,
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
            let geometry = tab_strip_geometry(960.0 * scale, scale, 9, 0);
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

            // Ink: the resting × is `--ink3`, a step below the caption run's own.
            let full_titles = strip_titles(2);
            let (_, _, rest) = strip_chrome(scale, &full_titles, 0, None, false);
            assert!(
                rest.iter()
                    .filter(|sprite| sprite.mark == ChromeMark::TabClose)
                    .all(|sprite| sprite.color == palette.title_text_muted),
                "scale {scale}: `.tab .close {{ color: var(--ink3) }}`"
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
            let geometry = tab_strip_geometry(960.0 * scale, scale, 2, 0);
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

    /// PIN — the title stops one 8px gap short of the `×` rather than running
    /// under it. `.tab { gap: 8px }` sits between the title and the controls.
    #[test]
    fn a_tab_title_clears_the_close_affordance_by_the_tab_s_own_gap() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let titles = strip_titles(2);
            let geometry = tab_strip_geometry(960.0 * scale, scale, 2, 0);
            let close = geometry.tabs[0].close.expect("a Full-tier tab has its ×");
            let (_, labels, _) = strip_chrome(scale, &titles, 0, None, false);
            let title = labels
                .iter()
                .find(|label| label.text == "tab 0")
                .expect("the active tab carries its name");
            assert!(
                (title.rect[2] - (close[0] - WINDOW_TAB_GAP_LOGICAL_PX * scale)).abs() < 0.01,
                "scale {scale}: title ends at {} but the × starts at {}",
                title.rect[2],
                close[0]
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
}
