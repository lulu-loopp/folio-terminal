//! Per-kind minimum sizes and extent classes — the whole extension seam.

use crate::geom::Axis;
use crate::tree::{ExtentClass, SeatKind};
use crate::{FILES_W, FILES_W_MIN, LogicalPx, MIN_PANE_H, MIN_PANE_W, MIN_PREVIEW_W};

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

/// The per-kind table plus the device scale.
///
/// §2.1: the three row minima are three independent lines, not one line reused —
/// 260 is "a screen of readable command output", 170 is "a column of filenames",
/// 360 is "a line of code that does not wrap". They are policy: overturning one
/// edits this table and nothing else.
///
/// Keyed by a sorted `Vec` rather than a hash map: red line L8 keeps hash
/// iteration order out of every geometric decision.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeatMetrics {
    table: Vec<(SeatKind, KindMetrics)>,
    scale_ppm: u32,
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
