//! Units and rectangles.
//!
//! The one authoritative unit is a fixed-point logical pixel
//! (`M2-layout-solver-spec.md` §2.5). Everything below is integer arithmetic:
//! red line D3 forbids `f32`/`f64` anywhere on a solve path, because D1 asks for
//! bit-identical output and a float multiply can differ by a ULP between two
//! code paths that are supposed to be the same picture.

/// Subpixels per logical pixel.
///
/// `M2-layout-solver-spec.md` §2.5: bt-layout **self-holds** this constant and
/// does not depend on `bt-doc` (red line L7), but the two values must agree.
/// The compile-time self-check below pins the value; the cross-crate assertion
/// belongs on the seam that can legally see both crates (§4.1).
pub const SUBPIXELS_PER_PX: i64 = 1024;

const _: () = assert!(
    SUBPIXELS_PER_PX == 1024,
    "bt-doc::SUBPIXELS_PER_PX is 1024; §2.5 requires the two to agree"
);

/// A length along one axis, in subpixels of a logical pixel.
///
/// DPI-independent by construction (§2.5): a scale-factor change is a similarity
/// transform that leaves every logical number alone.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct LogicalPx(i64);

impl LogicalPx {
    /// Zero length.
    pub const ZERO: Self = Self(0);

    /// A whole number of logical pixels.
    #[must_use]
    pub const fn px(whole: i64) -> Self {
        Self(whole * SUBPIXELS_PER_PX)
    }

    /// A raw subpixel count.
    #[must_use]
    pub const fn from_subpixels(sub: i64) -> Self {
        Self(sub)
    }

    /// The raw subpixel count.
    #[must_use]
    pub const fn subpixels(self) -> i64 {
        self.0
    }

    /// The largest whole logical pixel not exceeding this length.
    #[must_use]
    pub const fn floor_px(self) -> i64 {
        self.0.div_euclid(SUBPIXELS_PER_PX)
    }

    #[must_use]
    pub(crate) fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    #[must_use]
    pub(crate) fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[must_use]
    pub(crate) fn clamp_to(self, lo: Self, hi: Self) -> Self {
        Self(self.0.clamp(lo.0, hi.0))
    }

    #[must_use]
    pub(crate) fn is_positive(self) -> bool {
        self.0 > 0
    }
}

impl core::ops::Add for LogicalPx {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl core::ops::Sub for LogicalPx {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl core::ops::AddAssign for LogicalPx {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

/// Which way a split divides its children, and therefore which axis a length
/// is measured along.
///
/// `Row` = side by side, allocated along x. `Col` = stacked, allocated along y.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Axis {
    Row,
    Col,
}

impl Axis {
    /// Both axes, in a fixed order — never a hash-container iteration (red line L8).
    pub const BOTH: [Axis; 2] = [Axis::Row, Axis::Col];
}

/// A non-empty set of axes.
///
/// `M2-tiny-window-priority.md` §1.3: the two concession chains run in parallel
/// and their verdicts are intersected per seat, so `Collapsed` must be able to
/// say *which* axis was squeezed — `{Row}`, `{Col}` or `{Row, Col}`.
///
/// A `Vec`/bitset rather than a `HashSet`: L8 forbids hash iteration order from
/// touching geometry, and this set is compared for equality in pinned tests.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AxisSet(u8);

impl AxisSet {
    const ROW_BIT: u8 = 1;
    const COL_BIT: u8 = 2;

    /// Just the row axis.
    pub const ROW: Self = Self(Self::ROW_BIT);
    /// Just the column axis.
    pub const COL: Self = Self(Self::COL_BIT);
    /// Both axes — the 24x24 degenerate square of tiny-window §1.3.
    pub const BOTH: Self = Self(Self::ROW_BIT | Self::COL_BIT);

    #[must_use]
    pub(crate) const fn of(axis: Axis) -> Self {
        match axis {
            Axis::Row => Self::ROW,
            Axis::Col => Self::COL,
        }
    }

    #[must_use]
    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether `axis` is in the set.
    #[must_use]
    pub const fn contains(self, axis: Axis) -> bool {
        self.0 & Self::of(axis).0 != 0
    }

    /// Whether both axes are in the set.
    #[must_use]
    pub const fn is_both(self) -> bool {
        self.0 == Self::BOTH.0
    }
}

/// A rectangle in logical pixels, stored as its four boundaries rather than as
/// origin + size.
///
/// Red line L6: adjacent seats share the same boundary *number*. Storing widths
/// and re-deriving edges is exactly how a long chain accumulates a one-pixel
/// seam, and one seam is a fake divider the user can see. Here the right edge of
/// a seat is literally the value that became the left edge of its neighbour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogicalRect {
    pub left: LogicalPx,
    pub top: LogicalPx,
    pub right: LogicalPx,
    pub bottom: LogicalPx,
}

impl LogicalRect {
    /// A rectangle from its four boundaries.
    #[must_use]
    pub const fn new(left: LogicalPx, top: LogicalPx, right: LogicalPx, bottom: LogicalPx) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// A rectangle at the origin with the given whole-pixel size.
    #[must_use]
    pub const fn from_px(width: i64, height: i64) -> Self {
        Self::new(
            LogicalPx::ZERO,
            LogicalPx::ZERO,
            LogicalPx::px(width),
            LogicalPx::px(height),
        )
    }

    /// Extent along `axis`.
    #[must_use]
    pub fn extent(&self, axis: Axis) -> LogicalPx {
        match axis {
            Axis::Row => self.right - self.left,
            Axis::Col => self.bottom - self.top,
        }
    }

    /// The near boundary along `axis` (left for `Row`, top for `Col`).
    #[must_use]
    pub fn near(&self, axis: Axis) -> LogicalPx {
        match axis {
            Axis::Row => self.left,
            Axis::Col => self.top,
        }
    }

    /// Whether both extents are strictly positive.
    ///
    /// Red line L4: the solver never emits a zero-area rectangle. A collapsed
    /// seat is a real 24px bar with real area, not an absence.
    #[must_use]
    pub fn is_non_degenerate(&self) -> bool {
        self.extent(Axis::Row).is_positive() && self.extent(Axis::Col).is_positive()
    }
}

/// A size in logical pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogicalSize {
    pub width: LogicalPx,
    pub height: LogicalPx,
}

impl LogicalSize {
    /// A size from two lengths.
    #[must_use]
    pub const fn new(width: LogicalPx, height: LogicalPx) -> Self {
        Self { width, height }
    }

    /// A whole-pixel size.
    #[must_use]
    pub const fn px(width: i64, height: i64) -> Self {
        Self::new(LogicalPx::px(width), LogicalPx::px(height))
    }

    /// Zero.
    pub const ZERO: Self = Self::new(LogicalPx::ZERO, LogicalPx::ZERO);

    /// The length along `axis`.
    #[must_use]
    pub fn along(&self, axis: Axis) -> LogicalPx {
        match axis {
            Axis::Row => self.width,
            Axis::Col => self.height,
        }
    }
}

/// A rectangle snapped to the physical pixel grid, in whole device pixels.
///
/// §2.5 puts rounding in the solver and makes it *boundary* rounding: each
/// seat's near edge is rounded, and its far edge is read off the next seat's
/// near edge. That guarantee is only expressible without loss in device pixels,
/// which is why it is a separate output rather than a mutation of the logical
/// rectangle — a snapped boundary is generally not an integral number of
/// logical subpixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeviceRect {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

impl DeviceRect {
    /// Width in device pixels.
    #[must_use]
    pub const fn width(&self) -> i64 {
        self.right - self.left
    }

    /// Height in device pixels.
    #[must_use]
    pub const fn height(&self) -> i64 {
        self.bottom - self.top
    }
}

/// Round a logical boundary onto the device pixel grid, half away from zero.
///
/// Integer only (D3): `scale_ppm` is device pixels per logical pixel in parts
/// per million, so the device position is `sub * scale_ppm / (SUBPIXELS * 1e6)`.
pub(crate) fn snap_boundary(value: LogicalPx, scale_ppm: u32) -> i64 {
    let numer = i128::from(value.subpixels()) * i128::from(scale_ppm);
    let denom = i128::from(SUBPIXELS_PER_PX) * 1_000_000;
    let half = denom / 2;
    if numer >= 0 {
        ((numer + half) / denom) as i64
    } else {
        ((numer - half) / denom) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_is_integer_and_symmetric_about_zero() {
        // 1.5 logical px at 200% scale = 3 device px, exactly.
        let v = LogicalPx::from_subpixels(SUBPIXELS_PER_PX * 3 / 2);
        assert_eq!(snap_boundary(v, 2_000_000), 3);
        assert_eq!(snap_boundary(LogicalPx::ZERO, 1_000_000), 0);
        assert_eq!(snap_boundary(LogicalPx::px(7), 1_500_000), 11); // 10.5 -> 11
    }

    #[test]
    fn an_axis_set_distinguishes_one_axis_from_both() {
        assert!(!AxisSet::ROW.is_both());
        assert!(AxisSet::ROW.union(AxisSet::COL).is_both());
        assert!(AxisSet::BOTH.contains(Axis::Col));
    }
}
