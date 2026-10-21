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

/// **The second layer of a pane's environment** (`bt_pty::spawn_environment`'s `launch_overrides`):
/// what `--with-environment` carried, whole, or nothing (owner ruling 2026-10-05, choice A — a pane
/// takes the account's current environment and never the launcher's unless asked).
///
/// The seam is an overlay: every carried variable is laid over the account's environment at the
/// launcher's value, and an account variable the launcher does not have stays.
fn launch_overrides(carried: Option<&crate::cli::CarriedEnvironment>) -> Vec<(OsString, OsString)> {
    carried.map_or_else(Vec::new, |carried| carried.pairs().to_vec())
}

/// **The most UTF-16 units one Windows environment variable may hold**, its `name=value` entry:
/// the limit Windows documents for one variable. A whole environment block has none, so this is
/// the one an environment carried over the 256 KiB launch wire can break.
const WINDOWS_VARIABLE_UNITS: usize = 32_767;

/// **Why a carried environment cannot be given to a shell on `platform`**, or `None` when it can
/// (F-SWEEP-2-048 round 3): on Windows, a carried variable longer than [`WINDOWS_VARIABLE_UNITS`].
/// The birth is refused with the line rather than the variable cut — a shell given half a `PATH`
/// is quietly wrong. Names the variable and its size, never its value.
fn carried_environment_refusal(
    carried: Option<&crate::cli::CarriedEnvironment>,
    platform: bt_platform::HostPlatform,
) -> Option<String> {
    let carried = carried?;
    if platform != bt_platform::HostPlatform::Windows {
        return None;
    }
    let (name, units) = carried
        .pairs()
        .iter()
        .map(|(name, value)| {
            let units = name.encode_wide_len() + 1 + value.encode_wide_len();
            (name, units)
        })
        .find(|(_, units)| *units > WINDOWS_VARIABLE_UNITS)?;
    Some(carried.refusal_line(
        "given to the shell",
        &format!(
            "{} is {units} characters, more than the {WINDOWS_VARIABLE_UNITS} Windows takes for one \
             variable",
            name.to_string_lossy()
        ),
        "the shell was not started",
    ))
}

/// The length an `OsStr` has as UTF-16, which is how Windows counts an environment variable.
trait EncodeWideLen {
    fn encode_wide_len(&self) -> usize;
}

impl EncodeWideLen for std::ffi::OsStr {
    fn encode_wide_len(&self) -> usize {
        self.to_string_lossy().encode_utf16().count()
    }
}

/// **The environment a pane's derived declarations read** — the account's with the launch layer
/// over it, or, where there is no account environment to read, this process's with the same layer
/// over it.
fn before_folio(
    refresh: Option<&EnvironmentRefresh>,
    inherited: &[(OsString, OsString)],
    launch_overrides: &[(OsString, OsString)],
) -> Vec<(OsString, OsString)> {
    refresh.map_or_else(
        || bt_pty::spawn_environment(inherited, launch_overrides, &[], &[]),
        EnvironmentRefresh::before_folio,
    )
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
    /// What `--with-environment` carried into this pane, laid over the account's environment as
    /// the `launch_overrides` layer; `None` for every pane no launch asked it for (owner ruling
    /// 2026-10-05).
    pub(crate) carried_environment: Option<crate::cli::CarriedEnvironment>,
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

/// **What a birth is composed into**: its argv and whether it loads, its last-resort shell's argv
/// (composed, when it is asked for, with the same integration decision), and whether Folio's
/// PSReadLine may go on its path.
struct PlannedBirth<'r> {
    composed: shell_integration::ComposedBirth,
    last_resort: Box<dyn Fn() -> Vec<OsString> + 'r>,
    prefix: bool,
}

/// **A birth's composition, decided from the edition's answer** — [`bear`]'s decision seam
/// (T-INTEGRATION-INJECT-2-BUILD round 4): `language` is asked only of a Windows PowerShell 5.1
/// birth with the switch on ([`shell_integration::windows_powershell_birth`]), and `compose` is
/// handed the integration that answer leaves, and so is `last_resort` when the birth asks for it —
/// so a constrained edition's birth is composed, and its last-resort shell after it, exactly as
/// with the switch off.
fn plan_birth<'r>(
    program: &Path,
    integration: bool,
    language: impl FnOnce() -> shell_integration::Language,
    compose: impl FnOnce(bool) -> shell_integration::ComposedBirth,
    last_resort: impl Fn(bool) -> Vec<OsString> + 'r,
) -> PlannedBirth<'r> {
    let birth = shell_integration::windows_powershell_birth(program, integration, language);
    let integration = birth.integration;
    PlannedBirth {
        composed: compose(integration),
        last_resort: Box::new(move || last_resort(integration)),
        prefix: birth.prefix,
    }
}

/// **The shell's pseudoconsole and process, made on the `bt-pty-birth` worker**: the carried
/// folder asked about ([`gone_place`]), the PowerShell load composed (naming the script prepares
/// it), the current account's environment read and the declarations derived from it, then
/// `bt-pty`'s spawn — the folder's existence check, the pseudoconsole, the process, and the
/// one-shot retry to the last-resort shell.
fn bear(
    ctx: &WorkerCtx,
    turns: &crate::packaged_turns::PackagedTurns,
    spec: ShellSpec,
    wake: OutputWake,
) -> Born {
    let ShellSpec {
        program,
        arguments,
        powershell_integration,
        environment_derivation,
        mut folio_environment,
        mut profile_environment,
        carried_environment,
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
    // **A Windows PowerShell 5.1 birth composes from the edition's known language mode**
    // (T-INTEGRATION-INJECT-2-BUILD round 3b, ruling B′): `FullLanguage` gives Folio's loader and
    // PSReadLine; a constrained edition's pane is the Shell-integration-off pane, byte for byte; a
    // birth before the answer waits for it here, on the birth worker; a failed question gives the
    // loader without the prefix.
    let planned = plan_birth(
        std::path::Path::new(&program),
        powershell_integration,
        || shell_integration::language_for_birth(ctx, std::path::Path::new(&program)),
        |integration| {
            shell_integration::compose_powershell_birth_loading(
                std::path::Path::new(&program),
                &arguments,
                integration,
            )
        },
        shell_integration::last_resort_arguments,
    );
    let composed = planned.composed;
    let args = composed.arguments;
    let fallback_args = planned.last_resort;
    let inherited: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    // **A carried environment this system cannot give a shell refuses the birth** (F-SWEEP-2-048
    // round 3): said in the diagnostics and on the window's "Shell not started" card, never cut.
    if let Some(line) =
        carried_environment_refusal(carried_environment.as_ref(), bt_platform::host_platform())
    {
        eprintln!("{line}");
        return Born {
            session: Err(PtyError::Backend(line)),
            folder_gone,
        };
    }
    let launch_overrides = launch_overrides(carried_environment.as_ref());
    let refresh = environment_refresh(
        || {
            bt_platform::environment::fresh_logon_environment(ctx)
                .map_err(|error| error.to_string())
        },
        launch_overrides.clone(),
        |line| eprintln!("{line}"),
    );
    let before_folio = before_folio(refresh.as_ref(), &inherited, &launch_overrides);
    shell_integration::derive_environment_for_birth(
        environment_derivation,
        &before_folio,
        &mut folio_environment,
        &profile_environment,
    );
    // **Folio's PSReadLine, for a Windows PowerShell pane that got the loader**
    // (T-INTEGRATION-INJECT-2-BUILD): the module written if need be — disk work, here on the birth
    // worker — and its directory put first on the pane's own module path, after the row's layer.
    shell_integration::declare_psreadline_module(
        Path::new(&program),
        composed.loads && planned.prefix,
        &before_folio,
        &folio_environment,
        &mut profile_environment,
        shell_integration::ModulePathFacts::of_this_machine,
    );
    // **In its package's turn** (T-CHECK-NEVER-OVERLAPS-A-PACKAGED-BIRTH): a shell whose program
    // is a packaged app's (PowerShell 7 from the Store) is not started while a check of the same
    // package runs, and no such check starts until this spawn has returned; a birth that waited
    // says so once in `diagnostics.log`.
    let package = crate::packaged_turns::package_of_birth(Path::new(&program), &before_folio);
    let session =
        crate::packaged_turns::born_in_turn(turns, ctx, package, &crate::diagnostics::note, || {
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
                // No account environment to read: the shell inherits this process's, and what a launch
                // carried is laid over it in the same place it is laid over the account's.
                None => PtySession::spawn_shell_in(
                    program,
                    &args,
                    &fallback_args,
                    &launch_overrides
                        .into_iter()
                        .chain(folio_environment)
                        .chain(profile_environment)
                        .collect::<Vec<_>>(),
                    size,
                    wake,
                    working_directory,
                ),
            }
        });
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
    request_shell_in(&crate::packaged_turns::TURNS, spec, wake)
}

/// [`request_shell`] in `turns` — the process's, or a test's own.
pub(crate) fn request_shell_in(
    turns: &'static crate::packaged_turns::PackagedTurns,
    spec: ShellSpec,
    wake: OutputWake,
) -> Result<ShellBirth, PtyError> {
    let reader_wake = wake.clone();
    request(move |ctx| bear(ctx, turns, spec, reader_wake), wake)
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
pub(crate) fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
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

/// What a birth that panicked answers (`PtyError::Backend`): a fact for the pane's refusal, with
/// nothing of the panic's own text — that is in the panic log.
pub(crate) const BIRTH_PANICKED: &str = "the PTY birth worker panicked";

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
            // publish nothing and wake nobody, and its pane would wait in birth for ever. Inside
            // `contained_panic::contain`, which the resident panic hook lets unwind, so the panic
            // reaches here in the application as in a test (T-PANIC-ROWS-HONEST). The payload is
            // in the panic log; the error the pane says carries none of it.
            let answer = crate::contained_panic::contain(|| birth(ctx).into())
                .unwrap_or_else(|_| Born::from(Err(PtyError::Backend(BIRTH_PANICKED.to_owned()))));
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
    /// RED (T-INTEGRATION-INJECT-2-BUILD round 4) — **a birth composes with the integration the
    /// edition's answer leaves**: a constrained edition's 5.1 birth is composed — and its
    /// last-resort shell is composed — with the switch off, a `FullLanguage` edition's with it
    /// on and Folio's PSReadLine allowed, a failed question's with it on and no prefix.
    ///
    /// MUTATIONS: compose with the switch the row asked for (drop
    /// `let integration = birth.integration;` in `plan_birth`) — the constrained birth is composed
    /// with the integration on; compose the last resort with the row's switch
    /// (`last_resort(true)`) — the constrained birth's last resort carries the loader (review of
    /// round 4, nit (c′)).
    #[test]
    fn a_birth_composes_with_the_integration_its_edition_leaves() {
        use crate::shell_integration::{ComposedBirth, Language};
        use std::cell::Cell;
        for (answer, integration, prefix) in [
            (Language::Constrained, false, false),
            (Language::Full, true, true),
            (Language::Failed, true, false),
        ] {
            let composed_with = Cell::new(None);
            let last_resort_with = Cell::new(None);
            let planned = super::plan_birth(
                std::path::Path::new("powershell.exe"),
                true,
                || answer,
                |integration| {
                    composed_with.set(Some(integration));
                    ComposedBirth {
                        arguments: Vec::new(),
                        loads: integration,
                    }
                },
                |integration| {
                    last_resort_with.set(Some(integration));
                    Vec::new()
                },
            );
            assert_eq!(
                composed_with.get(),
                Some(integration),
                "{answer:?}: composed with"
            );
            assert_eq!(
                last_resort_with.get(),
                None,
                "{answer:?}: asked only when wanted"
            );
            let _ = (planned.last_resort)();
            assert_eq!(
                last_resort_with.get(),
                Some(integration),
                "{answer:?}: the last resort's"
            );
            assert_eq!(planned.prefix, prefix, "{answer:?}: the prefix");
        }
    }

    use super::*;

    /// RED (T-PANIC-ROWS-HONEST) — **a birth that panics under the resident panic hook is born as
    /// a `PtyError::Backend`**, published once and followed by the wake. The product's hook ends
    /// the process for a panic it is not told is contained, before any catch runs; the birth runs
    /// inside `contained_panic::contain`, so the hook's decision at the panic is yes and the pane's
    /// answer is the `# completions` row's failure, carrying none of the panic's text.
    ///
    /// MUTATION: run the birth under a bare `std::panic::catch_unwind` again — the decision at the
    /// panic is no (the process would end there).
    #[test]
    fn a_birth_panic_under_the_resident_hook_is_born_as_a_backend_error() {
        let (told, decision) = std::sync::mpsc::channel();
        let (woke, wakes) = std::sync::mpsc::channel();
        let woke = Mutex::new(woke);
        let wake: OutputWake = std::sync::Arc::new(move || {
            let _ = woke.lock().unwrap_or_else(PoisonError::into_inner).send(());
        });
        let mut shell = request(
            move |_ctx| -> Born {
                let _ = told.send(crate::the_hook_lets_this_panic_unwind());
                panic!("scripted: 外壳出生失败 the shell's birth fails")
            },
            wake,
        )
        .expect("a birth worker");
        assert!(
            crate::lane::receive(&decision, "the hook's decision at the birth's panic"),
            "the birth runs where the resident hook lets its panic unwind"
        );
        crate::lane::wait_for_a_wake(&wakes, "the birth's wake");
        let born = shell
            .take()
            .expect("the answer is published before the wake");
        let Err(PtyError::Backend(said)) = born.session else {
            panic!("a panicked birth is a backend error");
        };
        assert_eq!(said, BIRTH_PANICKED, "and says none of the panic's text");
        assert!(shell.take().is_none(), "one terminal");
        assert!(wakes.try_recv().is_err(), "one wake");
    }

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

    /// RED (F-SWEEP-2-048 round 3) — **a carried variable Windows cannot hold refuses the birth
    /// with the line, and is never cut.**
    ///
    /// The launch wire carries up to 256 KiB; one Windows variable holds 32,767 characters. A
    /// planted 40 KB variable is refused on Windows by name and size; one that fits, and the same
    /// one on a Mac, are not. Names only: the value is never in the line.
    ///
    /// MUTATION: no guard (`carried_environment_refusal` answering `None`) and the 40 KB variable
    /// reaches the spawn.
    #[test]
    fn birth_refuses_a_carried_variable_windows_cannot_hold() {
        let oversized = crate::cli::CarriedEnvironment::from_pairs(vec![
            ("PATH".into(), "C:\\Windows".into()),
            ("FSWEEP2_BIG_环境".into(), "路".repeat(40_000).into()),
        ]);
        let line =
            carried_environment_refusal(Some(&oversized), bt_platform::HostPlatform::Windows)
                .expect("a 40 KB variable is refused on Windows");
        assert!(
            line.contains(crate::cli::WITH_ENVIRONMENT_FLAG)
                && line.contains("FSWEEP2_BIG_环境")
                && line.contains("40015")
                && line.contains("32767")
                && line.contains("2 variables")
                && line.contains("the shell was not started"),
            "{line}"
        );
        assert!(!line.contains('路'), "the line names no value");
        let fits = crate::cli::CarriedEnvironment::from_pairs(vec![(
            "FSWEEP2_FITS".into(),
            "路".repeat(32_767 - "FSWEEP2_FITS=".len()).into(),
        )]);
        assert_eq!(
            carried_environment_refusal(Some(&fits), bt_platform::HostPlatform::Windows),
            None
        );
        assert_eq!(
            carried_environment_refusal(Some(&oversized), bt_platform::HostPlatform::MacOs),
            None
        );
        assert_eq!(
            carried_environment_refusal(None, bt_platform::HostPlatform::Windows),
            None
        );
    }

    /// RED (F-SWEEP-2-048, owner ruling 2026-10-05) — **`launch_overrides` carry the launcher's
    /// environment into the pane only when the launch asked**, over the account's current
    /// environment, and under Folio's own pane variables.
    ///
    /// The account here has never heard of `FSWEEP2_CARRIED_环境`, and the launcher's shell has it;
    /// so does the launcher's `PATH`, which differs from the account's. Names are asserted and
    /// printed, never values.
    ///
    /// MUTATIONS: ① `launch_overrides` answering nothing for a carried environment and the first
    /// half goes red; ② answering the carried environment for `None` (the launcher's environment
    /// in every pane, the rule 2026-10-05 retired) and the second half goes red.
    #[test]
    fn launch_overrides_carry_the_launchers_environment_only_when_asked() {
        let account = environment(&[("PATH", "C:\\Windows"), ("USERNAME", "账户")]);
        let carried = crate::cli::CarriedEnvironment::from_pairs(environment(&[
            ("PATH", "D:\\venv\\Scripts;C:\\Windows"),
            ("FSWEEP2_CARRIED_环境", "1"),
            ("TERM_PROGRAM", "launcher"),
        ]));
        let has =
            |pairs: &[(OsString, OsString)], name: &str| pairs.iter().any(|(key, _)| key == name);
        let value_of = |pairs: &[(OsString, OsString)], name: &str| {
            pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };

        // Asked: the account's environment with the launcher's laid over it.
        let overrides = launch_overrides(Some(&carried));
        let refresh = environment_refresh(|| Ok(Some(account.clone())), overrides.clone(), |_| {})
            .expect("the account's environment was read");
        let pane = refresh.before_folio();
        assert!(
            has(&pane, "FSWEEP2_CARRIED_环境"),
            "the launcher's variable reached the pane"
        );
        assert!(has(&pane, "USERNAME"), "and the account's own stayed");
        assert!(
            value_of(&pane, "PATH") == value_of(carried.pairs(), "PATH"),
            "the launcher's PATH is the pane's (values not printed)"
        );
        // Folio's own pane variables are laid over it, as over the account's.
        let folio = environment(&[("TERM_PROGRAM", "Folio")]);
        let composed = bt_pty::spawn_environment(&account, &overrides, &folio, &[]);
        assert!(
            value_of(&composed, "TERM_PROGRAM") == value_of(&folio, "TERM_PROGRAM"),
            "Folio's own variable wins over the launcher's"
        );
        // Where no account environment can be read, the same layer goes over this process's.
        let inherited = environment(&[("PATH", "/usr/bin")]);
        assert!(has(
            &before_folio(None, &inherited, &overrides),
            "FSWEEP2_CARRIED_环境"
        ));

        // Not asked: the account's environment and nothing of the launcher's.
        let refresh =
            environment_refresh(|| Ok(Some(account.clone())), launch_overrides(None), |_| {})
                .expect("the account's environment was read");
        let pane = refresh.before_folio();
        assert!(
            !has(&pane, "FSWEEP2_CARRIED_环境"),
            "no launch asked for it"
        );
        let names = |pairs: &[(OsString, OsString)]| {
            pairs.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>()
        };
        assert_eq!(
            names(&pane),
            names(&account),
            "exactly the account's variables"
        );
        assert!(pane == account, "at the account's values (not printed)");
        assert_eq!(
            names(&before_folio(None, &inherited, &launch_overrides(None))),
            names(&inherited)
        );
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
