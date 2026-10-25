//! **The order a start runs in** (T-STARTUP-ORDER-SHARED): the steps a launch takes before its
//! first window has a tab, written once, as two functions every start goes through.
//!
//! A start is split in two by the event loop, and so is this order. [`begin`] is the process's
//! half, which `fn main` calls before there is a loop: the removal verbs, the command line, the
//! hand-over. [`resume`] is the first window's half, which `Runtime::create` calls when the loop
//! first resumes: Windows PowerShell's language question, the startup migration and the
//! script's preparation before the window, the
//! window, the PowerShell questions before the first tab, the first tab. The window's
//! half needs the running loop the process's half builds, so nothing can call it first.
//!
//! What each step does is the caller's, handed in as the step: the product's are the bodies
//! `main` and `Runtime::create` write inline, and a test's are stand-ins that record which step
//! ran (`headless::StartupStep`). What the order is, is this module's — the one thing a test of
//! the steps' order reads, and the one thing the product cannot do differently.

use crate::shell_integration::{LanguageAsked, Migrated, QuestionsAsked, ScriptPrepared};

/// **The process's steps, in the fields' order**: what [`begin`] is handed.
pub(crate) struct FrontDoor<Removal, Parse, Claim> {
    /// A removal verb answered (`--remove-shell-integration`, `--remove-explorer-menu`): its
    /// process leaves here, having opened nothing, written no settings and handed nothing over.
    pub(crate) removal: Removal,
    /// The command line read: a rescue door's line answered, every other line parsed, and a
    /// refusal answered on the console it was typed at.
    pub(crate) parse: Parse,
    /// The hand-over: the data directory's claim taken, or the parsed request handed to the Folio
    /// that holds it and this process gone. What comes back is what it carries into its own run.
    pub(crate) claim: Claim,
}

/// **The process's half of a start**: the removal verbs, then the command line, then the
/// hand-over — and what the hand-over kept, for the run.
///
/// The removal comes before the parse because a verb that takes Folio back out of the machine is
/// owed nothing the parse leads to; the parse before the hand-over because what is handed over is
/// the parsed request.
pub(crate) fn begin<Request, Resident>(
    door: FrontDoor<impl FnOnce(), impl FnOnce() -> Request, impl FnOnce(Request) -> Resident>,
) -> Resident {
    (door.removal)();
    let request = (door.parse)();
    (door.claim)(request)
}

/// **The first window's steps, in the fields' order**: what [`resume`] is handed.
///
/// Four of them answer a witness only their product function makes (`LanguageAsked`,
/// `Migrated`, `ScriptPrepared`, `QuestionsAsked`), so the workers' steps cannot be handed each
/// other's call, and none of the four can be handed no call. The window's and the first tab's are
/// told apart by what they take and answer.
pub(crate) struct FirstWindow<Language, Migration, Script, Window, Questions, FirstTab> {
    /// Windows PowerShell's language-mode question (`shell_integration::begin_language_question`,
    /// T-INTEGRATION-INJECT-2-BUILD round 3b): first, so the first 5.1 birth waits for as little
    /// of it as there is left.
    pub(crate) language: Language,
    /// The startup's observation of an older Folio line in a `$PROFILE`
    /// (`shell_integration::begin_startup_migration`), started on a worker nobody waits for.
    pub(crate) migration: Migration,
    /// Folio's own `folio.ps1`, compared and repaired on a worker nobody waits for
    /// (`shell_integration::begin_powershell_script_preparation`).
    pub(crate) script: Script,
    /// The window, opened through the backend's door, with what it is painted with.
    pub(crate) window: Window,
    /// The parser questions about the PowerShell rows, put to the probe on a worker nobody waits
    /// for, about the program answers the first tab is made from.
    pub(crate) questions: Questions,
    /// The first window's tabs, their panes made from those answers in the opened window.
    pub(crate) first_tab: FirstTab,
}

/// **The first window's half of a start**: Windows PowerShell's language question, the startup
/// migration and the script's preparation, then the window, then the PowerShell questions, then
/// the first tab.
///
/// The three workers start before the window because none is the window's to wait for: what they
/// do is done by the time a PowerShell is born, or that birth waits for the language answer (on
/// its birth worker) or prepares the script itself.
/// The questions are asked before the first tab because a PowerShell row's pane composes its
/// command line from the answer, and a question asked after the birth is one the birth could not
/// have found answered in time. `answers` is what the steps share: the program walk's answers, which the
/// window and the questions adopt and the first tab is born from.
pub(crate) fn resume<Answers, Window, Tabs, Error>(
    answers: &mut Answers,
    steps: FirstWindow<
        impl FnOnce() -> LanguageAsked,
        impl FnOnce() -> Migrated,
        impl FnOnce() -> ScriptPrepared,
        impl FnOnce(&mut Answers) -> Result<Window, Error>,
        impl FnOnce(&mut Answers) -> QuestionsAsked,
        impl FnOnce(&mut Answers, &mut Window) -> Result<Tabs, Error>,
    >,
) -> Result<(Window, Tabs), Error> {
    let LanguageAsked { .. } = (steps.language)();
    let Migrated { .. } = (steps.migration)();
    let ScriptPrepared { .. } = (steps.script)();
    let mut window = (steps.window)(answers)?;
    let QuestionsAsked { .. } = (steps.questions)(answers);
    let tabs = (steps.first_tab)(answers, &mut window)?;
    Ok((window, tabs))
}

/// **A process-wide value `fn main` settles before anything reads it**
/// (T-LAZY-INITIALIZERS-BEFORE-THE-LOOP; the side-effects contract C1).
///
/// A lazily initialised cell pays its initializer on whichever read comes first, and when that
/// initializer touches the disk the first read may be a request door on the window thread. This
/// cell has no initializer a read can run: [`settle`](Self::settle) is the one call that makes the
/// value, and `main` makes it before its first branch or before the loop (each cell's own doc
/// says which, and `window_waits_tests`' pinned writers hold the place). A read before it is a
/// start in the wrong order and panics with the cell's name: there is no answer it could give
/// that is not the effect it exists to keep off the read.
///
/// **A test process has no `main`**, and about six hundred of this crate's tests read the profile
/// table through a window they build. So a cell may name, for test builds only, what a test
/// process settles it with on its first read ([`settled_in_a_test_by`](Self::settled_in_a_test_by)):
/// the same seed `main` settles the profile table with, and a scratch folder of the test
/// process's own for the data directory — never the person's `%APPDATA%`. The product build has
/// no such arm; the order there is proven by the pins and the witnesses.
pub(crate) struct SettledOnce<T: 'static> {
    cell: std::sync::OnceLock<T>,
    /// What the value is, for the panic of a read that came first.
    what: &'static str,
    /// What a test process settles the cell with on its first read, when the cell names one.
    #[cfg(test)]
    in_a_test: Option<fn() -> T>,
}

impl<T: 'static> SettledOnce<T> {
    pub(crate) const fn new(what: &'static str) -> Self {
        Self {
            cell: std::sync::OnceLock::new(),
            what,
            #[cfg(test)]
            in_a_test: None,
        }
    }

    /// [`new`](Self::new), whose first read in a test process settles it by `make`.
    #[cfg(test)]
    pub(crate) const fn settled_in_a_test_by(what: &'static str, make: fn() -> T) -> Self {
        Self {
            cell: std::sync::OnceLock::new(),
            what,
            in_a_test: Some(make),
        }
    }

    /// Makes the value by `make` on the first call and answers its witness; a later call runs
    /// nothing and answers the first value's witness.
    pub(crate) fn settle(&'static self, make: impl FnOnce() -> T) -> Settled<T> {
        Settled {
            value: self.cell.get_or_init(make),
        }
    }

    /// The settled value.
    ///
    /// # Panics
    ///
    /// Before [`settle`](Self::settle): the start's order is `main`'s, so a read that comes first
    /// is a defect of the code, not of the machine.
    pub(crate) fn get(&self) -> &T {
        match self.cell.get() {
            Some(value) => value,
            None => self.unsettled(),
        }
    }

    /// A read of an unsettled cell. In a test build, a cell that names a test value is settled
    /// by it here (a test process has no `main`); everywhere else, the panic.
    fn unsettled(&self) -> &T {
        #[cfg(test)]
        if let Some(make) = self.in_a_test {
            return self.cell.get_or_init(make);
        }
        panic!(
            "{} is read before `main` settled it (startup::SettledOnce)",
            self.what
        )
    }
}

/// **The witness that a [`SettledOnce`] was settled**: only [`SettledOnce::settle`] makes one,
/// so a function that takes it cannot be called before the settle ran. `FolioApp::new` takes the
/// two the loop's process settles, which makes "settled before `run_app`" a fact of the types.
pub(crate) struct Settled<T: 'static> {
    value: &'static T,
}

impl<T: 'static> Settled<T> {
    /// The value the settle made.
    pub(crate) fn value(&self) -> &'static T {
        self.value
    }

    /// A witness over a value of a test's own, for a test that builds the loop's application
    /// without settling the process's cells (a headless `FolioApp` must not resolve `%APPDATA%`).
    #[cfg(test)]
    pub(crate) fn planted(value: &'static T) -> Self {
        Self { value }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::SettledOnce;
    use crate::headless::{self, StartupStep};

    /// RED (T-LAZY-INITIALIZERS-BEFORE-THE-LOOP) — **a settled cell has no initializer a read can
    /// run**: a read before the settle panics with the cell's name instead of making the value.
    ///
    /// MUTATION: drop the cell's name from `unsettled`'s panic (`panic!("read before settled")`)
    /// — the panic no longer says which cell was read first.
    #[test]
    fn a_settled_cell_read_before_its_settle_panics() {
        static UNSETTLED: SettledOnce<u32> = SettledOnce::new("the planted value");
        let read = std::panic::catch_unwind(|| *UNSETTLED.get());
        let said = read.expect_err("a read before the settle panics");
        let said = said
            .downcast_ref::<String>()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(said.contains("the planted value"), "{said}");
    }

    /// RED (T-LAZY-INITIALIZERS-BEFORE-THE-LOOP) — **a cell is settled once**: the second settle
    /// runs nothing and answers the first value, and a read after it answers that value too.
    ///
    /// MUTATION: make `SettledOnce::settle` make the value on every call
    /// (`Box::leak(Box::new(make()))`) — the counter reads 2 and the second witness holds 2.
    #[test]
    fn a_settled_cell_makes_its_value_once() {
        static CELL: SettledOnce<usize> = SettledOnce::new("the counted value");
        static MADE: AtomicUsize = AtomicUsize::new(0);
        let make = || MADE.fetch_add(1, Ordering::Relaxed) + 1;
        let first = CELL.settle(make);
        let second = CELL.settle(make);
        assert_eq!(MADE.load(Ordering::Relaxed), 1);
        assert_eq!((*first.value(), *second.value(), *CELL.get()), (1, 1, 1));
    }

    /// The walk's answers as two looks land them: a PowerShell 7 row first, a Windows PowerShell
    /// row (its name in Chinese) once the window is up.
    fn walk() -> Vec<Vec<String>> {
        vec![
            vec!["pwsh 7".to_owned()],
            vec!["Windows PowerShell 中文".to_owned()],
        ]
    }

    /// RED (T-STARTUP-ORDER-SHARED; T-INTEGRATION-INJECT-2-BUILD round 3b) — **a start runs its
    /// nine steps in one order**: a removal verb, the command line, the hand-over, Windows
    /// PowerShell's language question, the startup migration, the script's preparation, the
    /// window, the PowerShell questions, the first tab — through `begin` and `resume`,
    /// the functions `main` and `Runtime::create` call, with the headless start's stand-ins. The
    /// hand-over is handed what the parse read.
    ///
    /// MUTATIONS: swap the removal and the parse in `begin` — the command line is read before a
    /// removal verb is answered; swap the script's preparation and the window in
    /// `resume` — the window opens before the script is being prepared; ask the language question
    /// after the window — the first 5.1 pane would wait for all of it.
    #[test]
    fn the_startup_steps_run_in_their_order() {
        let argv = r"folio --cwd D:\项目 --profile pwsh";
        let ran = headless::start(argv, walk());
        assert_eq!(
            ran.steps,
            vec![
                StartupStep::Removal,
                StartupStep::CommandLine,
                StartupStep::HandOver,
                StartupStep::LanguageQuestion,
                StartupStep::StartupMigration,
                StartupStep::ScriptPreparation,
                StartupStep::WindowOpened,
                StartupStep::PowerShellQuestions,
                StartupStep::FirstTab,
            ]
        );
        assert_eq!(ran.handed_over.as_deref(), Some(argv));
    }

    /// RED (T-STARTUP-ORDER-SHARED) — **the PowerShell questions are asked before the first tab,
    /// about every program answer the first tab is made from.** The walk answers one row when the
    /// window is titled and another before the questions; the questions are asked about both, and
    /// the first tab, made after them, is made from exactly those.
    ///
    /// MUTATION: make the first tab before the questions in `resume` — the first tab
    /// comes first, and it is made from the one answer the window adopted, never asked about the
    /// Windows PowerShell row it would be born without an answer for.
    #[test]
    fn the_powershell_questions_are_asked_before_the_first_tab() {
        let ran = headless::start("folio", walk());
        let at = |wanted: StartupStep| {
            ran.steps
                .iter()
                .position(|step| *step == wanted)
                .unwrap_or_else(|| panic!("{wanted:?} never ran: {:?}", ran.steps))
        };
        assert!(
            at(StartupStep::PowerShellQuestions) < at(StartupStep::FirstTab),
            "{:?}",
            ran.steps
        );
        assert_eq!(ran.asked, ["pwsh 7", "Windows PowerShell 中文"]);
        assert_eq!(ran.born_from, ran.asked);
    }
}
