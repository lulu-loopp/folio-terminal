//! **One terminal pane's text size** (ticket 37; `docs/plans/design/pane-text-size-2026-09-23.md`).
//!
//! The fact this module names is the pane's **requested rung** on a fixed ladder, and nothing
//! else. Its only home is the terminal view of one leaf (`LeafSession::text_scale`); it is never
//! a map on the window, the tab, the renderer or anything written to disk, and it is never a
//! product of sizes. Every size that depends on it is derived by [`TextScale::effective_logical_px`]
//! from the rung and the Settings size, and measured at the window's display scale by the one
//! measuring service `bt_render::GpuContext::terminal_cell_metrics`. A Settings change moves the
//! base and a display change moves the scale; neither touches the rung, and because nothing
//! stores a product, the order the three arrive in never matters.
//!
//! **Who writes it.** One mutation door — `Runtime::step_pane_text_scale`, which the three
//! keyboard rows, the wheel rung and the pane head's indicator all call — and two documented
//! lifecycle operations that are not writers in disguise: construction (a new leaf is handed its
//! rung by `create_leaf_session`; a split, a duplicate and every rebuilt view are handed
//! [`TextScale::ACTUAL`], a restarted shell is handed the rung of the leaf it replaces) and the
//! move of the leaf itself (tear-out, merge and a transfer to another window carry the
//! `LeafSession`, and the rung with it). `text_size_tests::the_rung_has_one_owner` holds it.
//!
//! **Who reads it.** The derivation, the pane head's indicator (which shows the requested rung,
//! not the effective size) and nothing else.

/// **The ladder**, in percent (confirmed 2026-09-23). Twelve rungs; the index is an integer, so
/// stepping up and back down returns to exactly where it began and no float drifts.
///
/// Not `webhost::ZOOM_LADDER`, which is a page's own ladder and a different constant.
pub(crate) const LADDER: [u16; 12] = [50, 67, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300];

/// The rung that is the Settings size itself — the reset, and where every new view starts.
const ACTUAL_RUNG: u8 = 4;

/// The smallest effective size a pane is drawn at, in logical pixels.
pub(crate) const MIN_EFFECTIVE_LOGICAL_PX: f32 = 8.0;
/// The largest effective size a pane is drawn at, in logical pixels.
pub(crate) const MAX_EFFECTIVE_LOGICAL_PX: f32 = 72.0;

/// One pane's requested text size: a rung of [`LADDER`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TextScale {
    rung: u8,
}

/// What the one mutation door is asked to do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextStep {
    /// One rung up (`text-larger`, a notch away from the hand).
    Larger,
    /// One rung down (`text-smaller`, a notch towards the hand).
    Smaller,
    /// Back to 100 % (`text-actual-size`, a click on the indicator).
    Actual,
}

impl TextScale {
    /// 100 %: the Settings size.
    pub(crate) const ACTUAL: Self = Self { rung: ACTUAL_RUNG };

    /// The requested percentage — what the pane head shows.
    #[must_use]
    pub(crate) const fn percent(self) -> u16 {
        LADDER[self.rung as usize]
    }

    /// Whether this is 100 %, where nothing is shown.
    #[must_use]
    pub(crate) const fn is_actual(self) -> bool {
        self.rung == ACTUAL_RUNG
    }

    /// The rung `step` leads to. Saturating: a step past either end of the ladder is the end.
    #[must_use]
    pub(crate) const fn stepped(self, step: TextStep) -> Self {
        match step {
            TextStep::Larger if (self.rung as usize) + 1 < LADDER.len() => Self {
                rung: self.rung + 1,
            },
            TextStep::Smaller if self.rung > 0 => Self {
                rung: self.rung - 1,
            },
            TextStep::Actual => Self::ACTUAL,
            TextStep::Larger | TextStep::Smaller => self,
        }
    }

    /// **The effective face size, in logical pixels** — the one derivation.
    ///
    /// `base_logical_px` is the Settings size as the renderer was given it
    /// (`settings::drawable_font_size`, validated to 10–24 there and nowhere else). The product is
    /// taken once, in `f32`, and clamped once to 8–72: at a base of 10 the three lowest rungs all
    /// draw at 8, and at a base of 24 the top rung is exactly 72. Nothing rounds it further, and
    /// the window's display scale is applied once, by the measurement, never here.
    #[must_use]
    pub(crate) fn effective_logical_px(self, base_logical_px: f32) -> f32 {
        (base_logical_px * f32::from(self.percent()) / 100.0)
            .clamp(MIN_EFFECTIVE_LOGICAL_PX, MAX_EFFECTIVE_LOGICAL_PX)
    }
}
