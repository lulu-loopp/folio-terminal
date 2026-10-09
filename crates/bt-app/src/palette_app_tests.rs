//! **`palette`, as the application drives it.** Tests whose first assertion is about
//! `palette`, written in the crate root's scope rather than in the module's own.

/// RED (27) — **Palette rows use list corners and the shared icon-to-label gap.**
///
/// UI-SPEC.md R3 and G3 replace the palette's own 7-point corners and
/// 7/9-point gaps. The restore row radius is private, so its rule is pinned
/// numerically without changing its visibility.
///
/// MUTATION: restore palette::DOT_GAP_LOGICAL_PX to 7.0.
/// MUTATION: restore palette::ROW_GAP_LOGICAL_PX to 9.0.
/// MUTATION: restore palette::ROW_RADIUS_LOGICAL_PX to 7.0.
#[test]
fn ui_spec_palette_rest_values_follow_the_rule() {
    assert_eq!(
        crate::palette::DOT_GAP_LOGICAL_PX,
        8.0,
        "UI-SPEC.md G3: the label-to-dot gap is 8"
    );
    assert_eq!(
        crate::palette::ROW_GAP_LOGICAL_PX,
        8.0,
        "UI-SPEC.md G3: the mark-to-label gap is 8"
    );
    assert_eq!(
        crate::palette::ROW_RADIUS_LOGICAL_PX,
        6.0,
        "UI-SPEC.md R3: restore::ROW_RADIUS_LOGICAL_PX is the list-row rule"
    );
}
