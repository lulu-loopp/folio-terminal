//! **`float`, as the application drives it.** Tests whose first assertion is about
//! `float`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::method_body;

/// RED (review row D1, the premise) — **a float that declines a point still
/// consumes it.**
///
/// This is why the fall-through was a defect rather than a nicety.
/// `float_hit` is total inside the frame: a body its tenant has no answer
/// for comes back `FloatPart::Body`, and anything the named rectangles miss
/// comes back `FloatPart::Head`. There is no `None` for the caller to read
/// as "the pointer went through". So the *router* is where the rule has to
/// live, and since the report of 2026-09-12 it lives there — one place for
/// the press and the hover both.
///
/// Red gate: make `float_hit`'s body arm answer `None` when the tenant
/// declines and the first assertion fails; then `file_row_under`'s rule
/// would be unnecessary — and the window would be transparent to the
/// pointer, which is the bug this window was built not to have.
#[test]
fn a_float_that_declines_a_point_still_consumes_it() {
    let geometry = float::float_geometry(
        [100.0, 100.0, 364.0, 500.0],
        float::FloatMode::Peek,
        1.0,
        30.0,
        float::FloatHeadTools::default(),
    );
    let middle = |rect: [f32; 4]| ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
    let (x, y) = middle(geometry.body);
    assert_eq!(
        float::float_hit(&geometry, x, y, None, |_, _| None),
        Some(float::FloatPart::Body),
        "a tenant with no row under the pointer — a preview's text, the \
             space below a tree's last row — still hands the point to the window"
    );
    assert_eq!(
        float::float_hit(&geometry, x, y, None, |_, _| Some(float::FloatPart::Row(2))),
        Some(float::FloatPart::Row(2)),
        "and a tenant that does have one answers with it"
    );
    assert_eq!(
        float::float_hit(&geometry, geometry.frame[0] - 1.0, y, None, |_, _| None),
        None,
        "outside the frame, and only outside it, the pointer goes past"
    );
    // The one part that is not a tree row and not silence either: whatever
    // the named rectangles leave over is the head, which is what makes a
    // press anywhere inside this window a drag of it.
    let body = method_body("Runtime", "file_row_under");
    assert!(
        body.contains("Some(PointerTarget::Float(id, float::FloatPart::Row(index)))"),
        "so the door that raises a file menu names the one part it can \
             answer for and returns nothing for the rest"
    );
}
