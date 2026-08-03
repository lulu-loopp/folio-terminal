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
    ChromeLabel, ChromeQuad, SEAT_BODY_BACKGROUND_RGB, SEAT_COLLAPSE_BAR_HOVER_RGB,
    SEAT_COLLAPSE_BAR_RGB, SEAT_DIVIDER_ACTIVE_RGB, SEAT_DIVIDER_HIT_LOGICAL_PX,
    SEAT_DIVIDER_HOVER_RGB, SEAT_DIVIDER_RGB, SEAT_TITLE_BAR_BACKGROUND_RGB,
    SEAT_TITLE_BAR_LOGICAL_PX, SEAT_TITLE_FONT_LOGICAL_PX, SEAT_TITLE_PADDING_LOGICAL_PX,
    SEAT_TITLE_TEXT_RGB, SeatViewport,
};

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
        // No chrome is subtracted from the window today: the seat tree owns the
        // whole client area. When a sidebar or a card column arrives, its extent
        // enters here and nowhere else.
        window_min_inner_size(
            &self.tree,
            metrics,
            self.focus,
            LogicalSize::ZERO,
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

/// The viewport rectangle, in logical pixels, for a client area of this many
/// device pixels.
///
/// The rounding here is the exact inverse of the solver's boundary snapping: a
/// lone leaf's rectangle *is* this viewport, and snapping it back must land on
/// the original device pixel or the byte-identity gate fails on the first
/// fractional DPI. It does: the inverse errs by at most half a subpixel, which
/// re-snaps to under 0.002 device pixels at any scale this product will meet.
pub fn logical_viewport(width_px: u32, height_px: u32, scale_ppm: u32) -> LogicalRect {
    LogicalRect::new(
        LogicalPx::ZERO,
        LogicalPx::ZERO,
        device_to_logical(width_px, scale_ppm),
        device_to_logical(height_px, scale_ppm),
    )
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

/// Something in the chrome the pointer can be over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChromeTarget {
    Divider(SplitId),
    /// A seat's title-bar close affordance.
    Close(SeatId),
    /// A collapsed seat's bar (§2.6.3): the whole strip is clickable.
    CollapseBar(SeatId),
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
        if placement.id == seats.terminal() {
            continue;
        }
        if contains(close_button_rect(rect, scale), x, y) {
            return Some(ChromeTarget::Close(placement.id));
        }
    }
    for slot in seats.split_slots(layout) {
        if contains(hit_band(slot, scale), x, y) {
            return Some(ChromeTarget::Divider(slot.id));
        }
    }
    None
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

fn close_button_rect(seat: [f32; 4], scale: f32) -> [f32; 4] {
    let bar = SEAT_TITLE_BAR_LOGICAL_PX * scale;
    [
        (seat[2] - bar).max(seat[0]),
        seat[1],
        seat[2],
        seat[1] + bar,
    ]
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
pub fn build_chrome(
    seats: &Seats,
    layout: &SeatLayout,
    scale: f32,
    pointer: ChromePointer,
) -> (Vec<ChromeQuad>, Vec<ChromeLabel>) {
    let mut quads = Vec::new();
    let mut labels = Vec::new();
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
                        SEAT_COLLAPSE_BAR_HOVER_RGB
                    } else {
                        SEAT_COLLAPSE_BAR_RGB
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
                if placement.id == seats.terminal() {
                    // The terminal draws itself, into its own seat viewport.
                    continue;
                }
                let bar = SEAT_TITLE_BAR_LOGICAL_PX * scale;
                let title_bottom = (rect[1] + bar).min(rect[3]);
                quads.push(ChromeQuad {
                    rect: [rect[0], title_bottom, rect[2], rect[3]],
                    color: SEAT_BODY_BACKGROUND_RGB,
                });
                quads.push(ChromeQuad {
                    rect: [rect[0], rect[1], rect[2], title_bottom],
                    color: SEAT_TITLE_BAR_BACKGROUND_RGB,
                });
                let pad = SEAT_TITLE_PADDING_LOGICAL_PX * scale;
                let close = close_button_rect(rect, scale);
                labels.push(ChromeLabel {
                    text: seat_title(placement.kind).to_owned(),
                    rect: [
                        rect[0] + pad,
                        rect[1],
                        (close[0] - pad).max(rect[0]),
                        title_bottom,
                    ],
                    font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
                    color: SEAT_TITLE_TEXT_RGB,
                    align_right: false,
                });
                labels.push(ChromeLabel {
                    text: "\u{00d7}".to_owned(),
                    rect: [close[0], close[1], close[2] - pad, title_bottom],
                    font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
                    color: SEAT_TITLE_TEXT_RGB,
                    align_right: true,
                });
            }
        }
    }
    for slot in seats.split_slots(layout) {
        let color = if pointer.dragging == Some(slot.id) {
            SEAT_DIVIDER_ACTIVE_RGB
        } else if pointer.hover == Some(ChromeTarget::Divider(slot.id)) {
            SEAT_DIVIDER_HOVER_RGB
        } else {
            SEAT_DIVIDER_RGB
        };
        quads.push(ChromeQuad {
            rect: slot.band,
            color,
        });
    }
    (quads, labels)
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
    let pad = SEAT_TITLE_PADDING_LOGICAL_PX * scale;
    let icon = 6.0 * scale;
    let width = rect[2] - rect[0];
    let height = rect[3] - rect[1];
    let icon_left = rect[0] + (width - icon).max(0.0) / 2.0;
    let icon_top = rect[1] + (height - icon).max(0.0) / 2.0;
    if width >= icon && height >= icon {
        quads.push(ChromeQuad {
            rect: [icon_left, icon_top, icon_left + icon, icon_top + icon],
            color: SEAT_TITLE_TEXT_RGB,
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
            color: SEAT_TITLE_TEXT_RGB,
            align_right: false,
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

    fn solved(seats: &Seats, viewport: LogicalRect, metrics: &SeatMetrics) -> SeatLayout {
        seats
            .solve(viewport, metrics)
            .unwrap_or_else(|_| fit_what_fits(seats, viewport, metrics))
    }

    /// The hard gate of this slice, stated as an equality rather than as a
    /// promise: a lone terminal leaf's rectangle *is* the viewport, and its
    /// device rectangle *is* the whole surface — no origin, no inset, nothing
    /// taken off any edge. Every downstream number the terminal computes is a
    /// function of those two, so if this holds at every DPI the pixels cannot
    /// have moved.
    ///
    /// The second half is the red gate: shift the viewport by one physical
    /// pixel and the same assertions fail, so the equality above is testing
    /// something rather than restating a tautology.
    #[test]
    fn a_lone_leaf_solves_to_the_whole_viewport_and_nothing_is_taken_off_it() {
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
                    Some(SeatViewport::whole(width, height)),
                    "{width}x{height} at {dpi_milli} milli-DPI must snap back to the whole surface"
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
                    Some(SeatViewport::whole(width, height)),
                    "the pin would pass even with an injected offset"
                );
            }
        }
    }

    /// The other half of the byte-identity argument: with a lone leaf there is
    /// no chrome at all, so the chrome draw calls are not merely no-ops — they
    /// are not issued.
    #[test]
    fn a_lone_leaf_draws_no_chrome_at_all() {
        let seats = Seats::lone_terminal();
        let metrics = seat_metrics(1_000);
        let layout = solved(&seats, viewport_of(960, 600, 1_000), &metrics);
        let (quads, labels) = build_chrome(&seats, &layout, 1.0, ChromePointer::default());
        assert!(
            quads.is_empty(),
            "a lone leaf has no divider and no title bar"
        );
        assert!(labels.is_empty());
        assert!(hit_chrome(&seats, &layout, 1.0, 480.0, 300.0).is_none());
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
                    viewport_of(opened.width, opened.height, dpi_milli),
                    &metrics,
                ),
                SeatId(1),
            )
            .expect("a lone leaf keeps a rectangle");
            assert_eq!(
                narrowed,
                SeatViewport::whole(opened.width, opened.height),
                "a lone leaf in a {}x{} window is that window at {dpi_milli} milli-DPI",
                opened.width,
                opened.height
            );
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
            assert_eq!(closed, SeatViewport::whole(width, height));
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

        let (quads, _labels) = build_chrome(&seats, &layout, 1.0, ChromePointer::default());
        assert!(
            quads.iter().any(|quad| quad.color == SEAT_COLLAPSE_BAR_RGB),
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
}
