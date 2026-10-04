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
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use bt_platform::admission::{WaitToken, WorkerCtx, doors};
use bt_platform::environment::Environment;
use bt_pty::{EnvironmentRefresh, OutputWake, PtyError, PtySession, PtySize};

type SnapshotResult = Result<Option<Environment>, String>;
type SnapshotWorker = JoinHandle<SnapshotResult>;

fn launch_snapshot_worker() -> &'static Mutex<Option<Result<SnapshotWorker, String>>> {
    static WORKER: OnceLock<Mutex<Option<Result<SnapshotWorker, String>>>> = OnceLock::new();
    WORKER.get_or_init(|| Mutex::new(None))
}

fn launch_snapshot_result() -> &'static OnceLock<SnapshotResult> {
    static SNAPSHOT: OnceLock<SnapshotResult> = OnceLock::new();
    &SNAPSHOT
}

/// Start the launch-time account snapshot before the resident run can create a window.
pub(crate) fn begin_launch_environment_snapshot() {
    if launch_snapshot_result().get().is_some() {
        return;
    }
    let Ok(mut worker) = launch_snapshot_worker().lock() else {
        return;
    };
    worker.get_or_insert_with(|| {
        bt_platform::spawn_at_priority(
            "bt-environment-snapshot",
            bt_platform::ThreadPriority::BelowNormal,
            |ctx| {
                bt_platform::environment::fresh_logon_environment(ctx)
                    .map_err(|error| error.to_string())
            },
        )
        .map_err(|error| error.to_string())
    });
}

fn launch_environment_snapshot(worker: &WorkerCtx) -> Result<Option<Environment>, PtyError> {
    launch_snapshot_result()
        .get_or_init(|| {
            let pending = launch_snapshot_worker()
                .lock()
                .map_err(|_| "launch environment snapshot lock was poisoned".to_owned())?
                .take();
            match pending {
                Some(Ok(pending)) => pending
                    .join()
                    .map_err(|_| "launch environment snapshot worker panicked".to_owned())
                    .and_then(|result| result),
                Some(Err(error)) => Err(error),
                None => bt_platform::environment::fresh_logon_environment(worker)
                    .map_err(|error| error.to_string()),
            }
        })
        .clone()
        .map_err(PtyError::Backend)
}

/// **`CreatePseudoConsole` and the shell's process** (row 11): the one
/// `PtySession::spawn_shell_in`, minted in `create_leaf_session`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_shell(
    token: WaitToken<'_, doors::PtyBirth>,
    program: OsString,
    args: &[OsString],
    folio_environment: &[(OsString, OsString)],
    profile_environment: &[(OsString, OsString)],
    size: PtySize,
    wake: OutputWake,
    working_directory: Option<PathBuf>,
) -> Result<PtySession, PtyError> {
    let _ = token;
    let args = args.to_vec();
    let folio_environment = folio_environment.to_vec();
    let profile_environment = profile_environment.to_vec();
    let worker = bt_platform::spawn_at_priority(
        "bt-pty-birth",
        bt_platform::ThreadPriority::BelowNormal,
        move |ctx| {
            let launch_snapshot = launch_environment_snapshot(ctx)?;
            let fresh = bt_platform::environment::fresh_logon_environment(ctx)?;
            match (fresh, launch_snapshot) {
                (Some(fresh), Some(launch_snapshot)) => PtySession::spawn_refreshed(
                    program,
                    &args,
                    &folio_environment,
                    &profile_environment,
                    EnvironmentRefresh::new(fresh, launch_snapshot, std::env::vars_os().collect()),
                    size,
                    wake,
                    working_directory,
                ),
                _ => PtySession::spawn_shell_in(
                    program,
                    &args,
                    &folio_environment
                        .into_iter()
                        .chain(profile_environment)
                        .collect::<Vec<_>>(),
                    size,
                    wake,
                    working_directory,
                ),
            }
        },
    )?;
    worker
        .join()
        .map_err(|_| PtyError::Backend("PTY birth worker panicked".into()))?
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
