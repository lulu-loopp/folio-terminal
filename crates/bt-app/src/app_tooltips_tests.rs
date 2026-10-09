//! **The crate root: tooltips and hints.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;

/// The caption run's four boxes map to four tooltip anchors and nothing else
/// does — a divider is a click target nobody hovers for an explanation.
#[test]
fn only_the_caption_run_carries_a_window_chrome_tooltip() {
    assert_eq!(
        tooltip_anchor_for(seats::ChromeTarget::Settings),
        Some(tooltip::TooltipAnchorId::Settings)
    );
    assert_eq!(
        tooltip_anchor_for(seats::ChromeTarget::CloseWindow),
        Some(tooltip::TooltipAnchorId::CloseWindow)
    );
    assert_eq!(tooltip_anchor_for(seats::ChromeTarget::Tab(0)), None);
    assert_eq!(tooltip_anchor_for(seats::ChromeTarget::TabClose(0)), None);
}
