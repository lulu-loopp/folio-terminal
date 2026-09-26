//! **The shell's birth, its resize and the quit's wait for retirements, each an owner-thread door**
//! (§5.3 rows 11, 12 and 15; design note `docs/plans/design/thread-door-2026-09-26.md`, §7
//! departure 1 and revisions (c)3 and (e)2).
//!
//! `bt-pty` does not depend on `bt-platform`, so its own functions cannot take a
//! [`WaitToken`]. The doors are therefore here, one `bt-app` function around each of the three
//! `bt-pty` calls the window thread waits on, and the rest of `bt-app` reaches those calls only
//! through them. Each is minted at the statement that used to make the call — the preparation
//! around it stays outside — and a refusal is handled there, before anything the call would have
//! changed.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use bt_platform::admission::{WaitToken, doors};
use bt_pty::{OutputWake, PtyError, PtySession, PtySize};

/// **`CreatePseudoConsole` and the shell's process** (row 11): the one
/// `PtySession::spawn_shell_in`, minted in `create_leaf_session`.
pub(crate) fn spawn_shell(
    token: WaitToken<'_, doors::PtyBirth>,
    program: OsString,
    args: &[OsString],
    environment: &[(OsString, OsString)],
    size: PtySize,
    wake: OutputWake,
    working_directory: Option<PathBuf>,
) -> Result<PtySession, PtyError> {
    let _ = token;
    PtySession::spawn_shell_in(program, args, environment, size, wake, working_directory)
}

/// **One leaf's `ResizePseudoConsole` round trip** (row 12): the one `PtySession::resize`,
/// minted in `commit_leaf_resize` after the reflow and before the reconcile. One admission per
/// leaf; the flush that walks the leaves is not admitted.
pub(crate) fn resize(
    token: WaitToken<'_, doors::PtyResize>,
    pty: &mut PtySession,
    size: PtySize,
) -> Result<(), PtyError> {
    let _ = token;
    pty.resize(size)
}

/// **The quit's bounded wait for the panes being taken apart** (row 15): the one
/// `bt_pty::wait_for_retirements`, minted in `settle_quit`'s `Retire` arm, in `Exiting`. Answers
/// how many were still going when the budget ran out.
pub(crate) fn wait_for_retirements(
    token: WaitToken<'_, doors::PaneRetirementWait>,
    budget: Duration,
) -> usize {
    let _ = token;
    bt_pty::wait_for_retirements(budget)
}
