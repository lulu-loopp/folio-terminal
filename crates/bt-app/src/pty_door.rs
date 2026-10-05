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

use crate::shell_integration;
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
    powershell_integration: bool,
    environment_derivation: shell_integration::EnvironmentDerivation,
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
            // The PowerShell load is composed here, off the window thread, because naming the
            // script prepares it; the retry's argv is composed only if the retry happens.
            let args = shell_integration::compose_powershell_birth(
                std::path::Path::new(&program),
                &args,
                powershell_integration,
            );
            let fallback_args = || shell_integration::last_resort_arguments(powershell_integration);
            let inherited: Vec<(OsString, OsString)> = std::env::vars_os().collect();
            let refresh = environment_refresh(
                || launch_environment_snapshot(ctx),
                || {
                    bt_platform::environment::fresh_logon_environment(ctx)
                        .map_err(|error| error.to_string())
                },
                inherited.clone(),
                |line| eprintln!("{line}"),
            );
            let before_folio = refresh
                .as_ref()
                .map_or_else(|| inherited.clone(), EnvironmentRefresh::before_folio);
            let mut folio_environment = folio_environment;
            shell_integration::derive_environment_for_birth(
                environment_derivation,
                &before_folio,
                &mut folio_environment,
                &profile_environment,
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

    fn value(environment: &Environment, wanted: &str) -> Option<String> {
        environment
            .iter()
            .find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.to_string_lossy().into_owned())
    }

    fn environment_after_birth(
        refresh: &EnvironmentRefresh,
        derivation: shell_integration::EnvironmentDerivation,
        mut folio: Environment,
        profile: Environment,
    ) -> Environment {
        let before_folio = refresh.before_folio();
        shell_integration::derive_environment_for_birth(
            derivation,
            &before_folio,
            &mut folio,
            &profile,
        );
        bt_pty::spawn_environment(&before_folio, &[], &[], &folio, &profile)
    }

    /// RED (T-ENV-REFRESH-3, mutation `derive_prompt_from_launch_environment`) — a prompt added
    /// to the current account block after Folio launched is wrapped, not replaced by the default.
    #[test]
    fn cmd_prompt_wraps_the_fresh_value_that_was_absent_at_launch() {
        let refresh = EnvironmentRefresh::new(
            environment(&[("PROMPT", "$T$G")]),
            environment(&[]),
            environment(&[]),
        );
        let result = environment_after_birth(
            &refresh,
            shell_integration::EnvironmentDerivation {
                integration: crate::profiles::Integration::CmdPrompt,
                crosses_wsl: false,
                forwards_terminal_into_wsl: false,
            },
            environment(&[("PROMPT", "launch-derived")]),
            environment(&[]),
        );
        let prompt = value(&result, "PROMPT").expect("cmd declaration");
        assert!(prompt.ends_with("$T$G"), "{prompt:?}");
        assert!(!prompt.contains("launch-derived"), "{prompt:?}");
    }

    /// RED (T-ENV-REFRESH-3, mutation `derive_wslenv_from_launch_environment`) — names added to
    /// WSLENV after launch remain ahead of Folio's declarations.
    #[test]
    fn wslenv_keeps_fresh_user_entries_and_adds_folios() {
        let refresh = EnvironmentRefresh::new(
            environment(&[("WSLENV", "USER_VALUE/u")]),
            environment(&[("WSLENV", "OLD_VALUE/u")]),
            environment(&[("WSLENV", "OLD_VALUE/u")]),
        );
        let result = environment_after_birth(
            &refresh,
            shell_integration::EnvironmentDerivation {
                integration: crate::profiles::Integration::BashInitFile,
                crosses_wsl: true,
                forwards_terminal_into_wsl: true,
            },
            environment(&[("WSLENV", "OLD_VALUE/u:TERM_PROGRAM/u")]),
            environment(&[]),
        );
        let listed = value(&result, "WSLENV").expect("WSL declaration");
        assert!(listed.starts_with("USER_VALUE/u:"), "{listed:?}");
        assert!(listed.contains("TERM_PROGRAM/u"), "{listed:?}");
        assert!(!listed.contains("OLD_VALUE/u"), "{listed:?}");
    }

    /// RED (T-ENV-REFRESH-3, mutation `derive_user_zdotdir_from_launch_environment`) — the zsh
    /// bridge carries the current account's startup directory.
    #[test]
    fn zsh_carries_the_fresh_zdotdir_in_bt_user_zdotdir() {
        let refresh = EnvironmentRefresh::new(
            environment(&[("ZDOTDIR", "/fresh/zsh")]),
            environment(&[("ZDOTDIR", "/launch/zsh")]),
            environment(&[("ZDOTDIR", "/launch/zsh")]),
        );
        let result = environment_after_birth(
            &refresh,
            shell_integration::EnvironmentDerivation {
                integration: crate::profiles::Integration::ZshDotDir,
                crosses_wsl: false,
                forwards_terminal_into_wsl: false,
            },
            environment(&[
                ("ZDOTDIR", "/folio/zsh"),
                ("BT_USER_ZDOTDIR", "/launch/zsh"),
            ]),
            environment(&[]),
        );
        assert_eq!(value(&result, "ZDOTDIR").as_deref(), Some("/folio/zsh"));
        assert_eq!(
            value(&result, "BT_USER_ZDOTDIR").as_deref(),
            Some("/fresh/zsh")
        );
    }

    /// RED (T-ENV-REFRESH-3, mutation `fresh_wins_over_explicit_launch_override`) — an inherited
    /// value that differs from the launch snapshot is an explicit launch override and still wins.
    #[test]
    fn an_explicit_launch_override_still_wins_before_derivation() {
        let refresh = EnvironmentRefresh::new(
            environment(&[("PROMPT", "fresh")]),
            environment(&[("PROMPT", "account-at-launch")]),
            environment(&[("PROMPT", "explicit-override")]),
        );
        let result = environment_after_birth(
            &refresh,
            shell_integration::EnvironmentDerivation {
                integration: crate::profiles::Integration::CmdPrompt,
                crosses_wsl: false,
                forwards_terminal_into_wsl: false,
            },
            environment(&[("PROMPT", "launch-derived")]),
            environment(&[]),
        );
        let prompt = value(&result, "PROMPT").expect("cmd declaration");
        assert!(prompt.ends_with("explicit-override"), "{prompt:?}");
        assert!(!prompt.ends_with("fresh"), "{prompt:?}");
    }

    /// RED (T-ENV-REFRESH-3, mutation `force_hyperlink_ignores_fresh_answer`) — Folio does not
    /// overlay its default when the current account block already answers the convention.
    #[test]
    fn force_hyperlink_respects_the_fresh_answer() {
        let refresh = EnvironmentRefresh::new(
            environment(&[("FORCE_HYPERLINK", "0")]),
            environment(&[]),
            environment(&[]),
        );
        let result = environment_after_birth(
            &refresh,
            shell_integration::EnvironmentDerivation {
                integration: crate::profiles::Integration::BashInitFile,
                crosses_wsl: false,
                forwards_terminal_into_wsl: false,
            },
            environment(&[("FORCE_HYPERLINK", "1")]),
            environment(&[]),
        );
        assert_eq!(value(&result, "FORCE_HYPERLINK").as_deref(), Some("0"));
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
