//! Per-kind minimum sizes and extent classes — the whole extension seam.

use crate::geom::Axis;
use crate::tree::{ExtentClass, SeatKind};
use crate::{
    DIVIDER, FILES_W, FILES_W_MIN, LogicalPx, MIN_PANE_H, MIN_PANE_W, MIN_PREVIEW_W,
    RATIO_DENOM_PPM,
};

/// Everything the solver knows about one kind of seat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KindMetrics {
    /// Minimum along the row axis.
    pub min_row: LogicalPx,
    /// Minimum along the column axis.
    pub min_col: LogicalPx,
    /// How this kind's extent is decided.
    pub extent: ExtentClass,
    /// Opening width for a fixed kind; ignored otherwise.
    pub default_fixed_extent: LogicalPx,
}

/// The per-kind table plus the three lengths that are not per kind: the device
/// scale, the divider, and the reduction a band is drawn at.
///
/// §2.1: the three row minima are three independent lines, not one line reused —
/// 260 is "a screen of readable command output", 170 is "a column of filenames",
/// 360 is "a line of code that does not wrap". They are policy: overturning one
/// edits this table and nothing else.
///
/// **Every length the solver measures with is in here** — that is what lets one
/// solver answer at two sizes (§7.1.6b′). A card's mini tree is the same tree,
/// the same ratios and the same §2.3 arithmetic at a reduction: its seam is
/// three pixels rather than one because that is what reads as a division at
/// 253px, and its files column spends a sixth of the band it spends on the
/// stage. Nothing about the *shape* of the answer changes, which is the whole
/// reason the card may not have a second walk of its own.
///
/// Keyed by a sorted `Vec` rather than a hash map: red line L8 keeps hash
/// iteration order out of every geometric decision.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeatMetrics {
    table: Vec<(SeatKind, KindMetrics)>,
    scale_ppm: u32,
    divider: LogicalPx,
    band_reduction_ppm: u32,
}

impl SeatMetrics {
    /// The ruled table of §2.1 at the given device scale.
    ///
    /// `scale_ppm` is device pixels per logical pixel in parts per million; it
    /// affects boundary rounding only (§2.5) and is *not* allowed to change any
    /// ratio or fixed extent (red line L5).
    #[must_use]
    pub fn ruled(scale_ppm: u32) -> Self {
        let flex = |min_row: LogicalPx| KindMetrics {
            min_row,
            min_col: MIN_PANE_H,
            extent: ExtentClass::Flex,
            default_fixed_extent: LogicalPx::ZERO,
        };
        Self {
            table: vec![
                (SeatKind::Terminal, flex(MIN_PANE_W)),
                (
                    SeatKind::Files,
                    KindMetrics {
                        min_row: FILES_W_MIN,
                        min_col: MIN_PANE_H,
                        extent: ExtentClass::FixedAlongRow,
                        default_fixed_extent: FILES_W,
                    },
                ),
                // Preview is flex, not fixed (§1.3, policy): "fixed right seat"
                // names an *address*, not a size. Conflating the two yields a
                // 240px code preview, which is a thumbnail rather than a preview.
                (SeatKind::Preview, flex(MIN_PREVIEW_W)),
                // A placeholder for an unrecognised persisted kind holds the
                // terminal's line: it must be visible and usable enough to say
                // "something I do not know was here" (§5 constraint 2).
                (SeatKind::Placeholder, flex(MIN_PANE_W)),
            ],
            scale_ppm,
            divider: DIVIDER,
            band_reduction_ppm: RATIO_DENOM_PPM,
        }
    }

    /// The ruled table at 100% scale.
    #[must_use]
    pub fn ruled_at_unit_scale() -> Self {
        Self::ruled(1_000_000)
    }

    /// Replace one kind's metrics. The extension seam of §1.1, and the way a
    /// policy overturn lands.
    #[must_use]
    pub fn with_kind(mut self, kind: SeatKind, metrics: KindMetrics) -> Self {
        match self.table.iter_mut().find(|(k, _)| *k == kind) {
            Some(slot) => slot.1 = metrics,
            None => {
                self.table.push((kind, metrics));
                self.table.sort_unstable_by_key(|(k, _)| *k);
            }
        }
        self
    }

    /// The same table at a different device scale. Touches no ratio (L5).
    #[must_use]
    pub fn with_scale_ppm(mut self, scale_ppm: u32) -> Self {
        self.scale_ppm = scale_ppm;
        self
    }

    /// Device pixels per logical pixel, in parts per million.
    #[must_use]
    pub fn scale_ppm(&self) -> u32 {
        self.scale_ppm
    }

    /// The same table with a divider of a stated width.
    ///
    /// [`DIVIDER`](crate::DIVIDER) everywhere a window is being solved. It is a
    /// parameter rather than a constant read at the point of use for one
    /// reason: a card's mini tree is solved by *this* solver, and three physical
    /// pixels of the card's own ground is what reads as a division between two
    /// seats 60px wide. Handing that seam in keeps the card's picture and the
    /// window's picture one piece of arithmetic (D4).
    #[must_use]
    pub fn with_divider(mut self, divider: LogicalPx) -> Self {
        self.divider = divider;
        self
    }

    /// The space a divider occupies in the allocation, in this table.
    #[must_use]
    pub fn divider(&self) -> LogicalPx {
        self.divider
    }

    /// The same table with every fixed column's *band* reduced by `ppm`.
    ///
    /// [`RATIO_DENOM_PPM`](crate::RATIO_DENOM_PPM) — no reduction at all —
    /// whenever a window is being solved, and red line L5 keeps it that way:
    /// the device scale may not touch a band, and neither may anything else a
    /// window does.
    ///
    /// It exists because a *reduction* is a different transform from a scale. A
    /// card body is 253 logical px standing in for a pane area around six times
    /// that, and the one length in this crate that is a pixel count rather than
    /// a share — a fixed column's width — has to come down with the picture or
    /// it eats a share of the card that it never had of the window. The ratios
    /// are untouched: a ratio is a ratio at every size, which is exactly why
    /// only the bands are named here.
    #[must_use]
    pub fn with_band_reduction_ppm(mut self, ppm: u32) -> Self {
        self.band_reduction_ppm = ppm;
        self
    }

    /// `band` brought down by [`Self::with_band_reduction_ppm`].
    ///
    /// Integer throughout (D3), and the one place the reduction is applied, so a
    /// declared width and a hand-dragged one cannot be reduced by two different
    /// roundings.
    #[must_use]
    pub(crate) fn reduce_band(&self, band: LogicalPx) -> LogicalPx {
        if self.band_reduction_ppm == RATIO_DENOM_PPM {
            return band;
        }
        let scaled = i128::from(band.subpixels()) * i128::from(self.band_reduction_ppm)
            / i128::from(RATIO_DENOM_PPM);
        LogicalPx::from_subpixels(scaled as i64)
    }

    fn get(&self, kind: SeatKind) -> KindMetrics {
        // A kind with no row is a kind this build does not know; it is held to
        // the placeholder's line rather than to nothing at all. The tree never
        // constructs one — `SeatKind` is closed — so this is the table's own
        // total-function guarantee, not a runtime fallback for user input.
        self.table
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, m)| *m)
            .unwrap_or(KindMetrics {
                min_row: MIN_PANE_W,
                min_col: MIN_PANE_H,
                extent: ExtentClass::Flex,
                default_fixed_extent: LogicalPx::ZERO,
            })
    }

    /// The minimum extent of `kind` along `axis`.
    ///
    /// Red line L2: this is answered from the kind carried *on the rectangle*,
    /// never by looking the leaf up in the live tree. A centre swap puts a
    /// terminal where the files pane was, and from that instant the live tree
    /// answers wrong.
    #[must_use]
    pub fn min_size(&self, kind: SeatKind, axis: Axis) -> LogicalPx {
        let m = self.get(kind);
        match axis {
            Axis::Row => m.min_row,
            Axis::Col => m.min_col,
        }
    }

    /// The extent class of `kind`.
    #[must_use]
    pub fn extent_class(&self, kind: SeatKind) -> ExtentClass {
        self.get(kind).extent
    }

    /// The opening width of a fixed `kind`.
    #[must_use]
    pub fn default_fixed_extent(&self, kind: SeatKind) -> LogicalPx {
        self.get(kind).default_fixed_extent
    }
}

impl Default for SeatMetrics {
    fn default() -> Self {
        Self::ruled_at_unit_scale()
    }
}
