//! **Windows PowerShell's language mode, asked once and early** (T-INTEGRATION-INJECT-2-BUILD
//! round 3b; the CLM design audit's ruling B′, 2026-10-18).
//!
//! A Windows PowerShell 5.1 pane is given Folio's PSReadLine only on a known answer: the edition
//! said its `-Command` text runs in `FullLanguage`. On a machine whose 5.1 runs constrained (an
//! application-control lockdown) the pane is exactly the pane Folio starts with Shell integration
//! off — no loader, no module path prefix — so the account's in-box editor works as the
//! administrator's policy vetted it. The question is the edition's own
//! (`$ExecutionContext.SessionState.LanguageMode`, under `-NoProfile -NonInteractive`): PowerShell's
//! lockdown test is an internal that Windows servicing changes, and asking the edition is faithful
//! by construction.
//!
//! **Timing changes only how long a pane takes, never which editor it gets.** The question starts
//! at launch, before the window (`startup::resume`); a 5.1 birth that arrives before the answer
//! waits for it on its own birth worker — never the window thread — bounded by the question's own
//! patience ([`super::POWERSHELL_PROBE_PATIENCE`]). A question that failed or ran out of patience
//! gives that birth no prefix (the loader still integrates the account's editor, as before this
//! ticket), says so once, and the next 5.1 birth asks again. A known answer is kept for the
//! process: no cache across launches, so the launch after a lockdown changes asks afresh.

use super::{
    POWERSHELL_PROBE_PATIENCE, ProbeEnvironment, check_powershell, is_powershell, is_pwsh,
};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use bt_platform::admission::WorkerCtx;

/// **What Windows PowerShell's `-Command` text runs in**, as its own answer said, or that no
/// answer came.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    /// `FullLanguage`: the pane gets Folio's PSReadLine and its loader.
    Full,
    /// Any other mode (`ConstrainedLanguage` under a lockdown): the pane is the switch-off pane.
    Constrained,
    /// The question was not answered — a refused start, a panic, a child that said nothing or ran
    /// out of patience. The pane gets no prefix; the next birth asks again.
    Failed,
}

/// The question the edition is asked: the language mode its `-Command` text runs in.
pub(crate) const LANGUAGE_COMMAND: &str = "$ExecutionContext.SessionState.LanguageMode";

/// **The answer in a child's standard output**: `FullLanguage` is [`Language::Full`]; any other
/// mode name is [`Language::Constrained`]; nothing is no answer.
#[must_use]
pub(crate) fn language_from(stdout: &str) -> Language {
    match stdout.trim() {
        "" => Language::Failed,
        "FullLanguage" => Language::Full,
        _ => Language::Constrained,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    NotAsked,
    Asking,
    Heard(Language),
}

/// **One edition's answer, and the births waiting for it.** A question is out while the state is
/// `Asking`; every question ends by filing an answer, [`Language::Failed`] included, so a waiting
/// birth always wakes.
pub(crate) struct LanguageCell {
    state: Mutex<State>,
    landed: Condvar,
    /// How many births are waiting now — what a test reads to file an answer only once a birth
    /// is waiting for it.
    waiting: std::sync::atomic::AtomicUsize,
}

impl LanguageCell {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(State::NotAsked),
            landed: Condvar::new(),
            waiting: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// **Put the question, unless one is out or a known answer is held**: a cell that never
    /// asked, or whose last question failed, asks. `start` starts the worker that files the
    /// answer and says whether it started; one that did not is a failed question.
    pub(crate) fn ask(&self, start: impl FnOnce() -> bool) {
        {
            let mut state = self.state();
            if matches!(
                *state,
                State::Asking | State::Heard(Language::Full | Language::Constrained)
            ) {
                return;
            }
            *state = State::Asking;
        }
        if !start() {
            self.file(Language::Failed);
        }
    }

    /// File the answer of the question out and wake every birth waiting for it.
    pub(crate) fn file(&self, answer: Language) {
        *self.state() = State::Heard(answer);
        self.landed.notify_all();
    }

    /// **The answer a birth composes from**: a known answer at once; otherwise the question is put
    /// (again, after a failure) and the birth waits for its answer — on `worker`, the birth's own,
    /// never the window thread. Also how long it waited, when it did.
    pub(crate) fn for_birth(
        &self,
        worker: &WorkerCtx,
        start: impl FnOnce() -> bool,
    ) -> (Language, Option<Duration>) {
        let _ = worker;
        if let State::Heard(answer @ (Language::Full | Language::Constrained)) = *self.state() {
            return (answer, None);
        }
        self.ask(start);
        let began = Instant::now();
        let mut waited = false;
        let mut state = self.state();
        loop {
            match *state {
                State::Heard(answer) => return (answer, waited.then(|| began.elapsed())),
                State::Asking | State::NotAsked => {
                    waited = true;
                    self.waiting
                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    state = self
                        .landed
                        .wait(state)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    self.waiting
                        .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                }
            }
        }
    }

    /// How many births are waiting for the answer now.
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> usize {
        self.waiting.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// Windows PowerShell's cell, for the process.
static WINDOWS_POWERSHELL: LanguageCell = LanguageCell::new();

/// Whether the constrained machine has been said this launch.
static CONSTRAINED_SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// **That a start put the language question** (T-INTEGRATION-INJECT-2-BUILD round 3b): the value
/// `startup::FirstWindow::language` must answer, and only [`begin_language_question`] makes one.
pub struct LanguageAsked(());

#[cfg(test)]
impl LanguageAsked {
    /// The headless start's stand-in for the step.
    pub(crate) fn stand_in() -> Self {
        Self(())
    }
}

/// **Ask Windows PowerShell its language mode, now, on a worker nobody waits for** — at launch,
/// before the window, so the first 5.1 pane finds it answered or waits only for what is left of
/// it. Nothing is asked off Windows.
pub fn begin_language_question() -> LanguageAsked {
    if bt_platform::host_platform() == bt_platform::HostPlatform::Windows {
        WINDOWS_POWERSHELL.ask(|| start_question(None));
    }
    LanguageAsked(())
}

/// **The language a Windows PowerShell birth of `program` composes from**, waiting on the birth
/// worker for an answer not yet in, and saying in `diagnostics.log` how long it waited.
pub(crate) fn language_for_birth(worker: &WorkerCtx, program: &Path) -> Language {
    let program = program.to_path_buf();
    language_for_birth_in(
        &WINDOWS_POWERSHELL,
        worker,
        || start_question(Some(program)),
        &crate::diagnostics::note,
    )
}

/// [`language_for_birth`] over a cell, a question and a voice handed in.
pub(crate) fn language_for_birth_in(
    cell: &LanguageCell,
    worker: &WorkerCtx,
    start: impl FnOnce() -> bool,
    note: &dyn Fn(&str),
) -> Language {
    let (answer, waited) = cell.for_birth(worker, start);
    if let Some(waited) = waited {
        note(&format!(
            "Windows PowerShell: a pane waited {} ms for the edition's language mode ({answer:?})",
            waited.as_millis()
        ));
    }
    answer
}

/// Start the question's worker; `program` is the birth's own, or — at launch — the
/// `powershell.exe` the account's environment names.
fn start_question(program: Option<PathBuf>) -> bool {
    crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal)
        .start_worker(
            "powershell-language",
            crate::lane::Answered::new(
                "Windows PowerShell's language question",
                crate::diagnostics::note,
                |_| WINDOWS_POWERSHELL.file(Language::Failed),
            ),
            move |worker| {
                let environment =
                    ProbeEnvironment::current(worker, "Windows PowerShell's language question");
                let asked = program
                    .or_else(|| environment.program(Path::new("powershell.exe")))
                    .ok_or_else(|| "no powershell.exe on this account's path".to_owned())
                    .and_then(|program| {
                        check_powershell(
                            worker,
                            &program,
                            LANGUAGE_COMMAND,
                            None,
                            POWERSHELL_PROBE_PATIENCE,
                            &environment,
                        )
                        .map_err(|failure| failure.to_string())
                    })
                    .and_then(|output| {
                        String::from_utf8(output.stdout).map_err(|error| error.to_string())
                    });
                let answer = match &asked {
                    Ok(stdout) => language_from(stdout),
                    Err(_) => Language::Failed,
                };
                say(answer, asked.err().as_deref(), &crate::diagnostics::note);
                WINDOWS_POWERSHELL.file(answer);
            },
        )
        .is_ok()
}

/// The `diagnostics.log` line an answer earns: a constrained machine once per launch, a failed
/// question every time (with its cause), a `FullLanguage` answer nothing.
fn say(answer: Language, cause: Option<&str>, note: &dyn Fn(&str)) {
    say_with(answer, cause, &CONSTRAINED_SAID, note);
}

/// [`say`] with the launch's latch handed in.
pub(crate) fn say_with(
    answer: Language,
    cause: Option<&str>,
    said: &std::sync::atomic::AtomicBool,
    note: &dyn Fn(&str),
) {
    match answer {
        Language::Full => {}
        Language::Constrained => {
            if !said.swap(true, std::sync::atomic::Ordering::AcqRel) {
                note(
                    "Windows PowerShell runs constrained on this machine (its language mode is not \
                     FullLanguage); Folio's PSReadLine is not offered to its panes, which start as \
                     with Shell integration off",
                );
            }
        }
        Language::Failed => note(&format!(
            "Windows PowerShell's language mode could not be asked ({}); this pane gets no Folio \
             PSReadLine, and the next pane asks again",
            cause.unwrap_or("no answer")
        )),
    }
}

/// **A Windows PowerShell birth, decided from the edition's answer** — what the birth worker
/// composes (`pty_door::bear`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowsPowerShellBirth {
    /// Whether the integration's loader may be composed: the Shell integration switch, except on
    /// a constrained edition, whose pane is the switch-off pane.
    pub(crate) integration: bool,
    /// Whether Folio's PSReadLine may be put on the pane's module path: only on `FullLanguage`.
    pub(crate) prefix: bool,
}

/// **Decide a birth of `program`**: a program that is not Windows PowerShell 5.1, or a birth with
/// the switch off, asks nothing; a 5.1 birth asks `language` — which waits — and composes from
/// what it answers.
pub(crate) fn windows_powershell_birth(
    program: &Path,
    integration: bool,
    language: impl FnOnce() -> Language,
) -> WindowsPowerShellBirth {
    if !integration || !is_powershell(program) || is_pwsh(program) {
        return WindowsPowerShellBirth {
            integration,
            prefix: false,
        };
    }
    match language() {
        Language::Full => WindowsPowerShellBirth {
            integration: true,
            prefix: true,
        },
        Language::Constrained => WindowsPowerShellBirth {
            integration: false,
            prefix: false,
        },
        Language::Failed => WindowsPowerShellBirth {
            integration: true,
            prefix: false,
        },
    }
}
