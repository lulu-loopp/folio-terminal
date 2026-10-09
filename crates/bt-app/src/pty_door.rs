//! **The shell's birth, a request the window thread never waits on; its resize and the quit's wait
//! for retirements, each an owner-thread door** (§5.3 rows 12 and 15; design note
//! `docs/plans/design/thread-door-2026-09-26.md`, §7 departure 1 and revisions (c)3 and (e)2).
//!
//! `bt-pty` does not depend on `bt-platform`, so its own functions cannot take a
//! [`WaitToken`]. The doors are therefore here, one `bt-app` function around each of the two
//! `bt-pty` calls the window thread waits on, and the rest of `bt-app` reaches those calls only
//! through them. Each is minted at the statement that used to make the call — the preparation
//! around it stays outside — and a refusal is handled there, before anything the call would have
//! changed.
//!
//! **A shell's birth is not one of them** (T-BIRTH-OFF-WINDOW). It is asked of a `bt-pty-birth`
//! worker of its own ([`request_shell`]), numbered, and answered into one process-wide mailbox
//! under its number; the pane that asked holds the number ([`ShellBirth`]) and takes the answer on
//! a later turn of whichever window it is in by then, woken through its own [`OutputWake`]. An
//! answer whose asker has gone is retired off the window thread.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::shell_integration;
use bt_platform::admission::{WaitToken, WorkerCtx, doors};
use bt_platform::environment::Environment;
use bt_pty::{EnvironmentRefresh, OutputWake, PtyError, PtySession, PtySize};

fn environment_refresh(
    fresh: impl FnOnce() -> Result<Option<Environment>, String>,
    launch_overrides: Environment,
    mut diagnostic: impl FnMut(&str),
) -> Option<EnvironmentRefresh> {
    match fresh() {
        Ok(Some(fresh)) => Some(EnvironmentRefresh::new(fresh, launch_overrides)),
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

/// **Everything one shell's birth is made of**, composed on the window thread by
/// `create_leaf_session` and finished on the `bt-pty-birth` worker.
pub(crate) struct ShellSpec {
    pub(crate) program: OsString,
    pub(crate) arguments: Vec<OsString>,
    pub(crate) powershell_integration: bool,
    pub(crate) environment_derivation: shell_integration::EnvironmentDerivation,
    pub(crate) folio_environment: Vec<(OsString, OsString)>,
    pub(crate) profile_environment: Vec<(OsString, OsString)>,
    pub(crate) size: PtySize,
    pub(crate) working_directory: Option<PathBuf>,
    /// The carried folder this birth asks the disk about first, and the command line and working
    /// directory it starts with instead when that folder is not a directory now (G-SWEEP-048,
    /// #29; `profiles::BirthPlace`).
    pub(crate) unless_gone: Option<GoneSpec>,
}

/// [`ShellSpec::unless_gone`].
pub(crate) struct GoneSpec {
    pub(crate) folder: PathBuf,
    pub(crate) arguments: Vec<OsString>,
    pub(crate) working_directory: Option<PathBuf>,
}

/// **What a shell's birth answers**: the session, or why there is none, and whether the folder
/// the pane carried was found gone — the pane then stands where no folder would have put it.
pub(crate) struct Born {
    pub(crate) session: Result<PtySession, PtyError>,
    pub(crate) folder_gone: bool,
}

impl From<Result<PtySession, PtyError>> for Born {
    fn from(session: Result<PtySession, PtyError>) -> Self {
        Self {
            session,
            folder_gone: false,
        }
    }
}

/// **Whether the carried folder still stands, asked here, on the birth worker** (G-SWEEP-048,
/// #29): `None` when it does (or nothing needs asking), the place to start in instead when
/// `is_dir` says it is not a directory. On an offline network share `is_dir` waits out the
/// redirector's timeout; this worker waits it, and the pane waits in birth.
pub(crate) fn gone_place(
    unless_gone: Option<GoneSpec>,
    is_dir: &dyn Fn(&Path) -> bool,
) -> Option<GoneSpec> {
    unless_gone.filter(|gone| !is_dir(&gone.folder))
}

/// **The shell's pseudoconsole and process, made on the `bt-pty-birth` worker**: the carried
/// folder asked about ([`gone_place`]), the PowerShell load composed (naming the script prepares
/// it), the current account's environment read and the declarations derived from it, then
/// `bt-pty`'s spawn — the folder's existence check, the pseudoconsole, the process, and the
/// one-shot retry to the last-resort shell.
fn bear(ctx: &WorkerCtx, spec: ShellSpec, wake: OutputWake) -> Born {
    let ShellSpec {
        program,
        arguments,
        powershell_integration,
        environment_derivation,
        mut folio_environment,
        profile_environment,
        size,
        working_directory,
        unless_gone,
    } = spec;
    let gone = gone_place(unless_gone, &Path::is_dir);
    let folder_gone = gone.is_some();
    let (arguments, working_directory) = match gone {
        Some(gone) => (gone.arguments, gone.working_directory),
        None => (arguments, working_directory),
    };
    let args = shell_integration::compose_powershell_birth(
        std::path::Path::new(&program),
        &arguments,
        powershell_integration,
    );
    let fallback_args = || shell_integration::last_resort_arguments(powershell_integration);
    let inherited: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let refresh = environment_refresh(
        || {
            bt_platform::environment::fresh_logon_environment(ctx)
                .map_err(|error| error.to_string())
        },
        Vec::new(),
        |line| eprintln!("{line}"),
    );
    let before_folio = refresh
        .as_ref()
        .map_or_else(|| inherited.clone(), EnvironmentRefresh::before_folio);
    shell_integration::derive_environment_for_birth(
        environment_derivation,
        &before_folio,
        &mut folio_environment,
        &profile_environment,
    );
    let session = match refresh {
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
    };
    Born {
        session,
        folder_gone,
    }
}

/// **Ask for one shell** (T-BIRTH-OFF-WINDOW): a `bt-pty-birth` worker of its own, at below-normal
/// priority, makes it ([`bear`]), publishes the answer under the request's number — or retires it,
/// when its asker has gone — and then wakes the pane through `wake`, the wake its reader thread is
/// handed too, so a pane moved to another window while it is being born is woken there. Answers at
/// once; only a thread that cannot be started is an error here.
pub(crate) fn request_shell(spec: ShellSpec, wake: OutputWake) -> Result<ShellBirth, PtyError> {
    let reader_wake = wake.clone();
    request(move |ctx| bear(ctx, spec, reader_wake), wake)
}

/// **The process's shell births**: the last number given, the answers published and not yet
/// taken, and the numbers whose asker has gone. One mutex, held for a map operation and never
/// across a birth.
struct Births {
    last: u64,
    answers: BTreeMap<u64, Born>,
    abandoned: BTreeSet<u64>,
}

static BIRTHS: Mutex<Births> = Mutex::new(Births {
    last: 0,
    answers: BTreeMap::new(),
    abandoned: BTreeSet::new(),
});

fn births() -> MutexGuard<'static, Births> {
    BIRTHS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether the mailbox still holds anything under `generation` — an answer, or the mark of an
/// asker that has gone.
#[cfg(test)]
pub(crate) fn holds(generation: u64) -> bool {
    let births = births();
    births.answers.contains_key(&generation) || births.abandoned.contains(&generation)
}

/// What a panic said, when it said it in text.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message")
}

/// A session nobody will take, taken apart on a thread of its own (`bt_pty::retire_session`).
fn retire(answer: Born) {
    if let Ok(session) = answer.session {
        bt_pty::retire_session(session);
    }
}

/// [`request_shell`] with the birth handed in — the seam a test holds a birth at a gate through.
pub(crate) fn request<A: Into<Born>>(
    birth: impl FnOnce(&WorkerCtx) -> A + Send + 'static,
    wake: OutputWake,
) -> Result<ShellBirth, PtyError> {
    let generation = {
        let mut births = births();
        births.last += 1;
        births.last
    };
    let asked = ShellBirth {
        generation,
        settled: false,
    };
    bt_platform::spawn_at_priority(
        "bt-pty-birth",
        bt_platform::ThreadPriority::BelowNormal,
        move |ctx| {
            // **A birth that panics is a birth that failed** (round 2): an unwinding worker would
            // publish nothing and wake nobody, and its pane would wait in birth for ever.
            let answer =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| birth(ctx).into()))
                    .unwrap_or_else(|panic| {
                        Born::from(Err(PtyError::Backend(format!(
                            "the PTY birth worker panicked: {}",
                            panic_message(panic.as_ref())
                        ))))
                    });
            let stale = {
                let mut births = births();
                if births.abandoned.remove(&generation) {
                    Some(answer)
                } else {
                    births.answers.insert(generation, answer);
                    None
                }
            };
            if let Some(answer) = stale {
                retire(answer);
            }
            wake();
        },
    )?;
    Ok(asked)
}

/// **One pane's shell being born, by its number.** Held by the pane in birth, which takes the
/// answer ([`Self::take`]) once it is [`Self::answered`]. Dropped unanswered — the pane, its tab or
/// its window closed — it leaves its number abandoned, and the answer is retired off the window
/// thread when it comes, or at once if it has come already.
#[derive(Debug)]
pub(crate) struct ShellBirth {
    generation: u64,
    settled: bool,
}

impl ShellBirth {
    /// This birth's number.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether the worker has published this birth's answer.
    pub(crate) fn answered(&self) -> bool {
        births().answers.contains_key(&self.generation)
    }

    /// The answer, once [`Self::answered`]; `None` before, and after it was taken.
    pub(crate) fn take(&mut self) -> Option<Born> {
        if self.settled {
            return None;
        }
        let answer = births().answers.remove(&self.generation);
        self.settled = answer.is_some();
        answer
    }
}

impl Drop for ShellBirth {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let published = {
            let mut births = births();
            let published = births.answers.remove(&self.generation);
            if published.is_none() {
                births.abandoned.insert(self.generation);
            }
            published
        };
        if let Some(answer) = published {
            retire(answer);
        }
    }
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
        bt_pty::spawn_environment(&before_folio, &[], &folio, &profile)
    }

    /// RED (T-ENV-REFRESH-3, mutation `derive_prompt_from_launch_environment`) — a prompt added
    /// to the current account block after Folio launched is wrapped, not replaced by the default.
    #[test]
    fn cmd_prompt_wraps_the_fresh_value_that_was_absent_at_launch() {
        let refresh = EnvironmentRefresh::new(environment(&[("PROMPT", "$T$G")]), environment(&[]));
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
        let refresh =
            EnvironmentRefresh::new(environment(&[("WSLENV", "USER_VALUE/u")]), environment(&[]));
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
        let refresh =
            EnvironmentRefresh::new(environment(&[("ZDOTDIR", "/fresh/zsh")]), environment(&[]));
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

    /// RED (T-ENV-REFRESH-3, mutation `fresh_wins_over_explicit_launch_override`) — a value the
    /// caller identifies as a launch override still wins.
    #[test]
    fn an_explicit_launch_override_still_wins_before_derivation() {
        let refresh = EnvironmentRefresh::new(
            environment(&[("PROMPT", "fresh")]),
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
        let refresh =
            EnvironmentRefresh::new(environment(&[("FORCE_HYPERLINK", "0")]), environment(&[]));
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
            || Err("CreateEnvironmentBlock refused".to_owned()),
            environment(&[]),
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

    /// RED (T-ENV-REFRESH round 2, mutation `cache_current_environment_error`) — a transient
    /// current-account read failure degrades only its spawn; the next spawn asks again and can
    /// compose its fresh account block and explicit override layer normally.
    #[test]
    fn a_current_environment_error_is_retried_and_a_later_spawn_composes_normally() {
        let mut first_diagnostics = Vec::new();
        let first = environment_refresh(
            || Err("OpenProcessToken refused".to_owned()),
            environment(&[]),
            |line| first_diagnostics.push(line.to_owned()),
        );
        assert!(
            first.is_none(),
            "the failed attempt uses inherited spawning"
        );
        assert_eq!(first_diagnostics.len(), 1);

        let expected = EnvironmentRefresh::new(
            environment(&[("PATH", "fresh")]),
            environment(&[("PORTABLE_ROOT", "chosen")]),
        );
        let second = environment_refresh(
            || Ok(Some(environment(&[("PATH", "fresh")]))),
            environment(&[("PORTABLE_ROOT", "chosen")]),
            |_| panic!("the retry succeeds"),
        );
        assert_eq!(second, Some(expected));
    }
}
