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

struct LaunchEnvironmentSnapshot {
    worker: Mutex<Option<Result<SnapshotWorker, String>>>,
    attempt: Mutex<()>,
    result: OnceLock<Option<Environment>>,
}

impl LaunchEnvironmentSnapshot {
    const fn new() -> Self {
        Self {
            worker: Mutex::new(None),
            attempt: Mutex::new(()),
            result: OnceLock::new(),
        }
    }

    fn begin(&self) {
        if self.result.get().is_some() {
            return;
        }
        let mut worker = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

    fn snapshot(&self, fresh: impl FnOnce() -> SnapshotResult) -> SnapshotResult {
        if let Some(result) = self.result.get() {
            return Ok(result.clone());
        }
        // Only a successful snapshot is permanent. A failed worker or platform read leaves this
        // owner empty, so the next spawn enters this same attempt and asks the door again.
        let _attempt = self
            .attempt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(result) = self.result.get() {
            return Ok(result.clone());
        }
        let pending = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let result = match pending {
            Some(Ok(pending)) => pending
                .join()
                .map_err(|_| "launch environment snapshot worker panicked".to_owned())
                .and_then(|result| result),
            Some(Err(error)) => Err(error),
            None => fresh(),
        };
        if let Ok(snapshot) = &result {
            let _ = self.result.set(snapshot.clone());
        }
        result
    }
}

static LAUNCH_ENVIRONMENT_SNAPSHOT: LaunchEnvironmentSnapshot = LaunchEnvironmentSnapshot::new();

/// Start the launch-time account snapshot before the resident run can create a window.
pub(crate) fn begin_launch_environment_snapshot() {
    LAUNCH_ENVIRONMENT_SNAPSHOT.begin();
}

fn launch_environment_snapshot(worker: &WorkerCtx) -> SnapshotResult {
    LAUNCH_ENVIRONMENT_SNAPSHOT.snapshot(|| {
        bt_platform::environment::fresh_logon_environment(worker).map_err(|error| error.to_string())
    })
}

fn environment_refresh(
    launch_snapshot: impl FnOnce() -> SnapshotResult,
    fresh: impl FnOnce() -> SnapshotResult,
    inherited: Environment,
    mut diagnostic: impl FnMut(&str),
) -> Option<EnvironmentRefresh> {
    let launch_snapshot = match launch_snapshot() {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return None,
        Err(error) => {
            diagnostic(&format!(
                "recoverable launch environment snapshot failure: {error}; using inherited \
                 environment"
            ));
            return None;
        }
    };
    match fresh() {
        Ok(Some(fresh)) => Some(EnvironmentRefresh::new(fresh, launch_snapshot, inherited)),
        Ok(None) => None,
        Err(error) => {
            diagnostic(&format!(
                "recoverable current environment read failure: {error}; using inherited \
                 environment"
            ));
            None
        }
    }
}

/// **The `bt-pty-birth` worker that creates the pseudoconsole and shell process** (row 11),
/// joined by the one admission minted in `create_leaf_session`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_shell(
    token: WaitToken<'_, doors::PtyBirth>,
    program: OsString,
    args: &[OsString],
    fallback_args: &[OsString],
    folio_environment: &[(OsString, OsString)],
    profile_environment: &[(OsString, OsString)],
    size: PtySize,
    wake: OutputWake,
    working_directory: Option<PathBuf>,
) -> Result<PtySession, PtyError> {
    let _ = token;
    let args = args.to_vec();
    let fallback_args = fallback_args.to_vec();
    let folio_environment = folio_environment.to_vec();
    let profile_environment = profile_environment.to_vec();
    let worker = bt_platform::spawn_at_priority(
        "bt-pty-birth",
        bt_platform::ThreadPriority::BelowNormal,
        move |ctx| {
            let inherited = std::env::vars_os().collect();
            let refresh = environment_refresh(
                || launch_environment_snapshot(ctx),
                || {
                    bt_platform::environment::fresh_logon_environment(ctx)
                        .map_err(|error| error.to_string())
                },
                inherited,
                |line| eprintln!("{line}"),
            );
            match refresh {
                Some(refresh) => PtySession::spawn_refreshed(
                    program,
                    &args,
                    &fallback_args,
                    &folio_environment,
                    &profile_environment,
                    refresh,
                    size,
                    wake,
                    working_directory,
                ),
                None => PtySession::spawn_shell_in(
                    program,
                    &args,
                    &fallback_args,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(rows: &[(&str, &str)]) -> Environment {
        rows.iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    /// RED (T-ENV-REFRESH round 2, mutation `refresh_after_current_environment_error`) — a current
    /// block that cannot be read chooses the ordinary inherited spawn and records that one failed
    /// read once.
    #[test]
    fn a_current_environment_door_error_uses_the_inherited_spawn() {
        let mut diagnostics = Vec::new();
        let refresh = environment_refresh(
            || Ok(Some(environment(&[("PATH", "launch")]))),
            || Err("CreateEnvironmentBlock refused".to_owned()),
            environment(&[("PATH", "inherited"), ("天下", "为公")]),
            |line| diagnostics.push(line.to_owned()),
        );
        let spawn = match refresh {
            Some(_) => "refreshed",
            None => "inherited",
        };
        assert_eq!(spawn, "inherited");
        assert_eq!(diagnostics.len(), 1, "one failed read writes one line");
        assert!(diagnostics[0].contains("CreateEnvironmentBlock refused"));
    }

    /// RED (T-ENV-REFRESH round 2, mutation `cache_snapshot_error`) — a transient snapshot
    /// failure degrades only its spawn; the next spawn retries, caches the success, and can compose
    /// its fresh, inherited and declaration layers normally.
    #[test]
    fn a_snapshot_error_is_retried_and_a_later_spawn_composes_normally() {
        let snapshot = LaunchEnvironmentSnapshot::new();
        let mut first_diagnostics = Vec::new();
        let first = environment_refresh(
            || snapshot.snapshot(|| Err("OpenProcessToken refused".to_owned())),
            || panic!("a spawn without a snapshot does not ask for a current block"),
            environment(&[("PATH", "inherited")]),
            |line| first_diagnostics.push(line.to_owned()),
        );
        assert!(
            first.is_none(),
            "the failed attempt uses inherited spawning"
        );
        assert_eq!(first_diagnostics.len(), 1);

        let expected = EnvironmentRefresh::new(
            environment(&[("PATH", "fresh")]),
            environment(&[("PATH", "launch")]),
            environment(&[("PATH", "inherited")]),
        );
        let second = environment_refresh(
            || snapshot.snapshot(|| Ok(Some(environment(&[("PATH", "launch")])))),
            || Ok(Some(environment(&[("PATH", "fresh")]))),
            environment(&[("PATH", "inherited")]),
            |_| panic!("the retry succeeds"),
        );
        assert_eq!(second, Some(expected));

        let cached = snapshot
            .snapshot(|| panic!("a successful snapshot is the permanent launch baseline"))
            .expect("cached snapshot");
        assert_eq!(cached, Some(environment(&[("PATH", "launch")])));
    }
}
