//! **A one-shot check of a packaged program and the birth of a pane of the same package take
//! turns** (T-CHECK-NEVER-OVERLAPS-A-PACKAGED-BIRTH).
//!
//! Windows runs the processes of one packaged app that Folio starts — PowerShell 7 from the
//! Microsoft Store — in one desktop app container per package: the first creates it, the rest join
//! it. In every recorded 0.4.7 death a `$PROFILE` check of that PowerShell created the container,
//! a pane's PowerShell born 30–150 ms later joined it, and when the check exited and Folio settled
//! it (closing its kill-on-close job) the container was destroyed 11–17 ms later with the pane in
//! it. A check that joins a container a pane created is harmless, and so is a pane born after the
//! check settled. So the rule: **a check of a package and the birth of a pane of that package
//! never run at once** — a birth waits for every running check of its package to settle, and a
//! check does not start while a birth of its package is under way. Whether the trigger is the job
//! close or the container ending with its creator is not settled; the rule removes the condition
//! both need.
//!
//! **One owner, [`TURNS`], waited on only on workers.** A birth takes its turn on its
//! `bt-pty-birth` worker ([`born_in_turn`], around `bt-pty`'s spawn), a check on the worker that
//! asks it ([`PackagedTurns::check_turn`], from the probe's spawn until its output is collected —
//! the last of its handles let go). **A waiting birth goes first**: a check asked while a birth of
//! its package waits, or is under way, waits for that birth, so a birth waits only for the checks
//! that already held the package when it came. Neither wait has a clock of its own: each of those
//! checks is ended by its own patience (`bt_platform::ProbeChild::wait_within`, which ends a probe
//! that outlives it), so a birth waits at most the longest of their patiences; a check's wait ends
//! when the births before it have spawned. A program in no package takes no turn and never waits.
//!
//! **What the turns do not cover** (named, not built):
//! - **Other processes.** The turns are this process's: a command-line verb
//!   (`--remove-shell-integration`) asking a Store PowerShell, or an update candidate during its
//!   hand-off, is not ordered against this process's births.
//! - **A Store PowerShell started outside a birth** while a check runs — a Windows Terminal tab, or
//!   `pwsh` typed in a Windows PowerShell or Command Prompt pane — is not a birth and takes no turn.
//! - **A program the package lookup does not recognise** takes no turn on either side: a symbolic
//!   link a person made to the alias or to the package's `pwsh.exe` (it is read as the link, not
//!   followed), and a program inside a package registered from a development folder not named for
//!   its full name, reached by path ([`bt_platform::package_family_of`]). A `\\?\` verbatim
//!   spelling of either is recognised.

use std::path::Path;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bt_platform::PackageFamily;
use bt_platform::admission::WorkerCtx;

/// **The process's turns** — the one place a running check or a birth under way is held.
pub(crate) static TURNS: PackagedTurns = PackagedTurns::new();

/// **Whose turn each package is**: its checks' or its births' — never both. Births of one package
/// run beside each other, and so do its checks; a package nothing holds is not in the list.
pub(crate) struct PackagedTurns {
    state: Mutex<Vec<Turning>>,
    changed: Condvar,
}

/// One package something holds now.
struct Turning {
    package: PackageFamily,
    /// The edition of each check of it running now.
    checks: Vec<&'static str>,
    /// How many births of it are under way.
    births: usize,
    /// How many births of it wait for its checks: a check asked meanwhile waits for them too.
    births_waiting: usize,
    /// How many checks of it wait for its births.
    #[cfg(test)]
    checks_waiting: usize,
}

impl Turning {
    fn new(package: PackageFamily) -> Self {
        Self {
            package,
            checks: Vec::new(),
            births: 0,
            births_waiting: 0,
            #[cfg(test)]
            checks_waiting: 0,
        }
    }

    fn holds_nothing(&self) -> bool {
        #[cfg(test)]
        if self.checks_waiting > 0 {
            return false;
        }
        self.checks.is_empty() && self.births == 0 && self.births_waiting == 0
    }
}

/// **How long a birth waited for a check, and of which edition** — said once in
/// `diagnostics.log` ([`waited_line`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Waited {
    pub(crate) length: Duration,
    pub(crate) edition: &'static str,
}

/// The `diagnostics.log` line a birth that waited leaves.
pub(crate) fn waited_line(waited: Waited) -> String {
    format!(
        "Folio: a pane's birth waited {} ms for the {} check to settle",
        waited.length.as_millis(),
        waited.edition
    )
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The entry for `package`, made when nothing held it.
fn turning<'a>(state: &'a mut Vec<Turning>, package: &PackageFamily) -> &'a mut Turning {
    let at = state
        .iter()
        .position(|turning| turning.package == *package)
        .unwrap_or_else(|| {
            state.push(Turning::new(package.clone()));
            state.len() - 1
        });
    &mut state[at]
}

impl PackagedTurns {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(Vec::new()),
            changed: Condvar::new(),
        }
    }

    /// **A check's turn on `package`** (`worker`, the capability the thread door lent it): waits
    /// while a birth of the package is under way or waiting — a waiting birth goes first — then
    /// holds the package for checks — a birth of it waits — until the turn is dropped, which the
    /// caller does once the check has settled and its output is collected.
    /// `edition` names the check in the line a birth that waited for it leaves. `None` (a program
    /// in no package) takes no turn.
    pub(crate) fn check_turn(
        &self,
        worker: &WorkerCtx,
        package: Option<PackageFamily>,
        edition: &'static str,
    ) -> CheckTurn<'_> {
        let _ = worker;
        let Some(package) = package else {
            return CheckTurn {
                turns: self,
                held: None,
            };
        };
        let mut state = locked(&self.state);
        #[cfg(test)]
        {
            turning(&mut state, &package).checks_waiting += 1;
            self.changed.notify_all();
        }
        while {
            let turn = turning(&mut state, &package);
            turn.births > 0 || turn.births_waiting > 0
        } {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let turn = turning(&mut state, &package);
        #[cfg(test)]
        {
            turn.checks_waiting -= 1;
        }
        turn.checks.push(edition);
        CheckTurn {
            turns: self,
            held: Some((package, edition)),
        }
    }

    /// **A birth's turn on `package`** (`worker`, its `bt-pty-birth` worker's capability): waits
    /// while a check of the package runs — and, while it waits, a check asked after it waits
    /// behind it — then holds the package for births — a check of it waits — until the turn is
    /// dropped, which [`born_in_turn`] does once the spawn has returned. The turn says how long
    /// it waited, and for which check, when it did. `None` takes no turn.
    pub(crate) fn birth_turn(
        &self,
        worker: &WorkerCtx,
        package: Option<PackageFamily>,
    ) -> BirthTurn<'_> {
        let _ = worker;
        let Some(package) = package else {
            return BirthTurn {
                turns: self,
                held: None,
                waited: None,
            };
        };
        let mut state = locked(&self.state);
        let mut waiting: Option<(std::time::Instant, &'static str)> = None;
        while let Some(edition) = turning(&mut state, &package).checks.first().copied() {
            if waiting.is_none() {
                waiting = Some((std::time::Instant::now(), edition));
                turning(&mut state, &package).births_waiting += 1;
                self.changed.notify_all();
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let turn = turning(&mut state, &package);
        if waiting.is_some() {
            turn.births_waiting -= 1;
        }
        turn.births += 1;
        BirthTurn {
            turns: self,
            held: Some(package),
            waited: waiting.map(|(since, edition)| Waited {
                length: since.elapsed(),
                edition,
            }),
        }
    }

    /// Let go of one hold on `package` (`release` takes it off the entry), and wake every waiter.
    fn let_go(&self, package: &PackageFamily, release: impl FnOnce(&mut Turning)) {
        {
            let mut state = locked(&self.state);
            let turn = turning(&mut state, package);
            release(turn);
            if turn.holds_nothing() {
                state.retain(|turning| turning.package != *package);
            }
        }
        self.changed.notify_all();
    }

    /// How many births of `package` wait for its checks now, and how many checks of it wait for
    /// its births.
    #[cfg(test)]
    pub(crate) fn waiting(&self, package: &PackageFamily) -> (usize, usize) {
        locked(&self.state)
            .iter()
            .find(|turning| turning.package == *package)
            .map_or((0, 0), |turning| {
                (turning.births_waiting, turning.checks_waiting)
            })
    }

    /// Whether nothing holds `package` and nothing waits on it.
    #[cfg(test)]
    pub(crate) fn idle(&self, package: &PackageFamily) -> bool {
        !locked(&self.state)
            .iter()
            .any(|turning| turning.package == *package)
    }
}

/// **A check holding its package** — dropped once the check has settled.
#[must_use = "a check holds its package until this is dropped"]
pub(crate) struct CheckTurn<'a> {
    turns: &'a PackagedTurns,
    held: Option<(PackageFamily, &'static str)>,
}

impl Drop for CheckTurn<'_> {
    fn drop(&mut self) {
        if let Some((package, edition)) = self.held.take() {
            self.turns.let_go(&package, |turn| {
                if let Some(at) = turn.checks.iter().position(|held| *held == edition) {
                    turn.checks.remove(at);
                }
            });
        }
    }
}

/// **A birth holding its package** — dropped once its spawn has returned.
#[must_use = "a birth holds its package until this is dropped"]
pub(crate) struct BirthTurn<'a> {
    turns: &'a PackagedTurns,
    held: Option<PackageFamily>,
    waited: Option<Waited>,
}

impl BirthTurn<'_> {
    /// How long this birth waited for a check, when it did.
    pub(crate) fn waited(&self) -> Option<Waited> {
        self.waited
    }
}

impl Drop for BirthTurn<'_> {
    fn drop(&mut self) {
        if let Some(package) = self.held.take() {
            self.turns
                .let_go(&package, |turn| turn.births = turn.births.saturating_sub(1));
        }
    }
}

/// **A pane's birth, in its package's turn**: `birth` (`bt-pty`'s spawn) runs once no check of
/// `package` runs, and a check of it waits until `birth` has returned; a birth that waited says so
/// once through `note`, with the wait's length.
pub(crate) fn born_in_turn<T>(
    turns: &PackagedTurns,
    worker: &WorkerCtx,
    package: Option<PackageFamily>,
    note: &dyn Fn(&str),
    birth: impl FnOnce() -> T,
) -> T {
    let turn = turns.birth_turn(worker, package);
    if let Some(waited) = turn.waited() {
        note(&waited_line(waited));
    }
    let born = birth();
    drop(turn);
    born
}

/// **The package a pane's `program` belongs to**: the program as the pane names it, a bare name
/// looked up over the `PATH` of `environment` — the environment the pane is born with before
/// Folio's and its profile's declarations — absolute entries only, with its `PATHEXT`; then asked
/// of Windows ([`bt_platform::package_family_of`]). `None` for a program in no package, and
/// everywhere off Windows.
pub(crate) fn package_of_birth(
    program: &Path,
    environment: &[(std::ffi::OsString, std::ffi::OsString)],
) -> Option<PackageFamily> {
    let variable = |name: &str| {
        environment
            .iter()
            .find(|(held, _)| held.as_os_str() == name)
            .or_else(|| {
                environment
                    .iter()
                    .find(|(held, _)| held.to_string_lossy().eq_ignore_ascii_case(name))
            })
            .map(|(_, value)| value)
    };
    let directories: Vec<std::path::PathBuf> = variable("PATH")
        .map(|path| {
            std::env::split_paths(path)
                .filter(|entry| entry.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    let pathext =
        variable("PATHEXT").map_or_else(String::new, |value| value.to_string_lossy().into_owned());
    let resolved =
        bt_platform::program_in_directories(program, &directories, &pathext, &Path::is_file)?;
    bt_platform::package_family_of(&resolved)
}

#[cfg(test)]
#[path = "packaged_turns_tests.rs"]
mod tests;
