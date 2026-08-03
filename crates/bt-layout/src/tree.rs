//! The layout tree: a binary tree whose only leaf type is a seat.

use crate::geom::Axis;
use crate::metrics::SeatMetrics;
use crate::{LogicalPx, MIN_RATIO_PPM, RATIO_DENOM_PPM};

/// Identity of a seat within one tab's tree.
///
/// Red line L1: this is a *geometry* identity, not a content identity. No
/// session uid, no buffer, no files root ever enters the solver — content moving
/// house produces no layout event, and a layout change produces no content event.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SeatId(pub u64);

/// Identity of a split node.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SplitId(pub u64);

/// The family of seats.
///
/// The extension seam of §1.1 is exactly two things: a row in
/// [`SeatMetrics`](crate::SeatMetrics) (its minimum sizes and its extent class),
/// and a variant here. The tree, the allocation formulas and the determinism
/// rules do not change when a new pane kind arrives — that is the point.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SeatKind {
    Terminal,
    Files,
    Preview,
    /// A leaf that was loaded from a persisted tree whose `kind` this build does
    /// not recognise.
    ///
    /// §1.1 / §5 constraint 2: an unknown kind degrades *per leaf* into a
    /// visible placeholder rather than losing the tree, and it must not be
    /// silently turned into a terminal.
    Placeholder,
}

/// How a seat's extent is decided along an axis (§1.3).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExtentClass {
    /// A share of what is left, by ratio, on both axes.
    Flex,
    /// Pixels along `Row`, full slot along `Col`.
    ///
    /// Fixedness holds only on the row axis: a files column dropped into a
    /// vertical slot fills that slot's width. A column of *only* fixed subtrees
    /// is still fixed, at its widest member — without that, two stacked files
    /// columns balloon to half the window (a bug the prototype actually shipped).
    FixedAlongRow,
    /// Reserved: extent from the content's intrinsic size. No member today.
    ///
    /// Named because §1.3 tabulates three classes; it behaves as [`Flex`] until
    /// a member exists, which is honest — there is no intrinsic size to ask for.
    ///
    /// [`Flex`]: ExtentClass::Flex
    ReservedIntrinsic,
}

impl ExtentClass {
    /// Whether this class is fixed along `axis`.
    #[must_use]
    pub fn is_fixed_along(self, axis: Axis) -> bool {
        matches!(self, ExtentClass::FixedAlongRow) && axis == Axis::Row
    }
}

/// A leaf.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Seat {
    pub id: SeatId,
    pub kind: SeatKind,
    /// The row-axis pixel width of a fixed seat. Ignored for flex kinds.
    ///
    /// Durable (§5). `None` means "this kind's default".
    pub fixed_extent: Option<LogicalPx>,
    /// Whether a preview seat is pinned. Durable (§5); consumed by the
    /// preview-landing edit of §1.3, never by the allocator.
    pub pinned: bool,
}

impl Seat {
    /// A seat of `kind` with default geometry.
    #[must_use]
    pub fn new(id: SeatId, kind: SeatKind) -> Self {
        Self {
            id,
            kind,
            fixed_extent: None,
            pinned: false,
        }
    }

    /// The same seat with an explicit fixed extent.
    #[must_use]
    pub fn with_fixed_extent(mut self, extent: LogicalPx) -> Self {
        self.fixed_extent = Some(extent);
        self
    }

    /// The same seat, pinned.
    #[must_use]
    pub fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }

    /// This seat's fixed width along the row axis, if its kind is fixed there.
    ///
    /// §2.3: `max(fixed_extent, min_size(kind, Row))` — a fixed column is never
    /// narrower than its own honest minimum.
    #[must_use]
    pub fn fixed_width(&self, metrics: &SeatMetrics, axis: Axis) -> Option<LogicalPx> {
        if !metrics.extent_class(self.kind).is_fixed_along(axis) {
            return None;
        }
        let want = self
            .fixed_extent
            .unwrap_or_else(|| metrics.default_fixed_extent(self.kind));
        Some(want.max(metrics.min_size(self.kind, axis)))
    }
}

/// The share one side of a split takes, in parts per million.
///
/// §2.4, user ruling 3 of 2026-08-03. Integers because D3 wants bit-identical
/// arithmetic, and ppm because it is the cheapest integer that is accurate
/// enough: the quantisation error across a 4K width is under 0.004px, well below
/// one physical pixel, while a rational pair would need reducing and
/// cross-multiplying for no gain.
///
/// Both endpoints of `(0, 1)` are excluded *by the type*: a zero-extent seat is
/// something [`Presentation::Collapsed`] or a close says, never something a
/// ratio says (red line L4).
///
/// [`Presentation::Collapsed`]: crate::Presentation::Collapsed
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Ratio(u32);

impl Ratio {
    /// Half and half — the birth ratio of a fresh split, before its run is
    /// rebalanced.
    pub const HALF: Self = Self(RATIO_DENOM_PPM / 2);

    /// A ratio from parts per million, rejecting the excluded endpoints.
    #[must_use]
    pub const fn from_ppm(ppm: u32) -> Option<Self> {
        if ppm >= MIN_RATIO_PPM && ppm <= RATIO_DENOM_PPM - MIN_RATIO_PPM {
            Some(Self(ppm))
        } else {
            None
        }
    }

    /// A ratio from parts per million, clamped into the domain.
    ///
    /// §5 constraint 2: a persisted tree carrying an illegal ratio is clamped,
    /// not rejected — one bad number must not cost the whole tree.
    #[must_use]
    pub const fn clamped_from_ppm(ppm: u32) -> Self {
        if ppm < MIN_RATIO_PPM {
            Self(MIN_RATIO_PPM)
        } else if ppm > RATIO_DENOM_PPM - MIN_RATIO_PPM {
            Self(RATIO_DENOM_PPM - MIN_RATIO_PPM)
        } else {
            Self(ppm)
        }
    }

    /// The canonical representation: parts per million.
    ///
    /// §5 constraint 1: persistence writes *this* `u32` and reads it back
    /// unchanged. Serialising it as a decimal or a JSON float breaks the
    /// round-trip on multi-column layouts.
    #[must_use]
    pub const fn ppm(self) -> u32 {
        self.0
    }

    /// `avail * ratio`, multiply-then-divide so the division happens once.
    pub(crate) fn apply(self, avail: LogicalPx) -> LogicalPx {
        let numer = i128::from(avail.subpixels()) * i128::from(self.0);
        LogicalPx::from_subpixels((numer / i128::from(RATIO_DENOM_PPM)) as i64)
    }

    /// The ratio that gives `a` out of `a + b` of the space, clamped into the
    /// domain. Used only by rebalance, which divides a run by integer demands.
    pub(crate) fn from_parts(a: u64, b: u64) -> Self {
        let total = a + b;
        let ppm = (u128::from(a) * u128::from(RATIO_DENOM_PPM) / u128::from(total)) as u32;
        Self::clamped_from_ppm(ppm)
    }
}

/// A node of the layout tree.
///
/// Binary, not n-ary (§1.2, structural). Every ruled edit is a binary edit — an
/// edge drop halves a leaf, the centre swaps, a close promotes a sibling — and
/// the n-ary feel of "a row of equal panes" is recovered by the [`run`] concept,
/// which is a transitive closure over same-direction splits rather than a node.
/// Promoting runs to n-ary nodes would erase the structural difference between
/// "split this pane" and "add a column to this row", two gestures the UI-UX
/// ruling keeps apart.
///
/// [`run`]: crate::members
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LayoutNode {
    Split {
        id: SplitId,
        dir: Axis,
        ratio: Ratio,
        a: Box<LayoutNode>,
        b: Box<LayoutNode>,
    },
    Seat(Seat),
}

impl LayoutNode {
    /// A leaf.
    #[must_use]
    pub fn seat(seat: Seat) -> Self {
        LayoutNode::Seat(seat)
    }

    /// A split of two subtrees at the birth ratio.
    #[must_use]
    pub fn split(id: SplitId, dir: Axis, a: LayoutNode, b: LayoutNode) -> Self {
        Self::split_at(id, dir, Ratio::HALF, a, b)
    }

    /// A split of two subtrees at an explicit ratio.
    #[must_use]
    pub fn split_at(id: SplitId, dir: Axis, ratio: Ratio, a: LayoutNode, b: LayoutNode) -> Self {
        LayoutNode::Split {
            id,
            dir,
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    /// The seats of this subtree in in-order.
    ///
    /// D2: output order is a function of the tree, never of an iteration order.
    #[must_use]
    pub fn seats_in_order(&self) -> Vec<&Seat> {
        let mut out = Vec::new();
        self.walk_in_order(&mut |seat| out.push(seat));
        out
    }

    pub(crate) fn walk_in_order<'a>(&'a self, f: &mut impl FnMut(&'a Seat)) {
        match self {
            LayoutNode::Seat(seat) => f(seat),
            LayoutNode::Split { a, b, .. } => {
                a.walk_in_order(f);
                b.walk_in_order(f);
            }
        }
    }

    pub(crate) fn walk_splits(&self, f: &mut impl FnMut(SplitId, Ratio)) {
        if let LayoutNode::Split {
            id, ratio, a, b, ..
        } = self
        {
            f(*id, *ratio);
            a.walk_splits(f);
            b.walk_splits(f);
        }
    }

    /// Every split's ratio, keyed by id, in in-order. A `Vec` of pairs rather
    /// than a hash map, by red line L8.
    #[must_use]
    pub fn ratios(&self) -> Vec<(SplitId, Ratio)> {
        let mut out = Vec::new();
        self.walk_splits(&mut |id, ratio| out.push((id, ratio)));
        out.sort_unstable_by_key(|(id, _)| *id);
        out
    }

    /// Whether this subtree contains `seat`.
    #[must_use]
    pub fn contains(&self, seat: SeatId) -> bool {
        match self {
            LayoutNode::Seat(s) => s.id == seat,
            LayoutNode::Split { a, b, .. } => a.contains(seat) || b.contains(seat),
        }
    }

    /// The seat with this id, if the tree holds one.
    #[must_use]
    pub fn find_seat(&self, id: SeatId) -> Option<&Seat> {
        match self {
            LayoutNode::Seat(s) => (s.id == id).then_some(s),
            LayoutNode::Split { a, b, .. } => a.find_seat(id).or_else(|| b.find_seat(id)),
        }
    }

    pub(crate) fn find_seat_mut(&mut self, id: SeatId) -> Option<&mut Seat> {
        match self {
            LayoutNode::Seat(s) => (s.id == id).then_some(s),
            LayoutNode::Split { a, b, .. } => a.find_seat_mut(id).or_else(|| b.find_seat_mut(id)),
        }
    }

    /// The number of seats.
    #[must_use]
    pub fn seat_count(&self) -> usize {
        match self {
            LayoutNode::Seat(_) => 1,
            LayoutNode::Split { a, b, .. } => a.seat_count() + b.seat_count(),
        }
    }
}
