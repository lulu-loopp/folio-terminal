//! **`foreground_program`, as the application drives it.** Tests whose first assertion is about
//! `foreground_program`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::leaf_saying;

/// RED (69a round 2, E8) — a worker answer lands only in the addressed shell incarnation. The
/// platform fake-tree pin proves WSL/ssh produce `Unknown`; this pin proves that answer is stored
/// as unknown, while an unlisted local image remains known for ordinary E3(b).
#[test]
fn an_addressed_foreground_answer_stores_unknown_and_preserves_an_unlisted_local_name() {
    let mut leaf = leaf_saying("foreground answer");
    let incarnation = leaf.incarnation;
    assert_eq!(
        foreground_program::apply_answer(
            &mut leaf,
            incarnation,
            bt_platform::foreground_program::ForegroundProgram::Unknown,
        ),
        Some((false, false))
    );
    assert_eq!(
        leaf.session.foreground_program(),
        &bt_detect::ForegroundProgram::Unknown
    );

    assert_eq!(
        foreground_program::apply_answer(
            &mut leaf,
            incarnation,
            bt_platform::foreground_program::ForegroundProgram::Known("powershell".to_owned()),
        ),
        Some((true, false))
    );
    assert_eq!(
        leaf.session.foreground_program(),
        &bt_detect::ForegroundProgram::known("powershell")
    );

    assert_eq!(
        foreground_program::apply_answer(
            &mut leaf,
            incarnation + 1,
            bt_platform::foreground_program::ForegroundProgram::Unknown,
        ),
        None
    );
    assert_eq!(
        leaf.session.foreground_program(),
        &bt_detect::ForegroundProgram::known("powershell")
    );
}
